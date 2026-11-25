use crate::domain::appimage::AppImageError;
use crate::infrastructure::hash::sha1_file;
use crate::services::update_info::UpdateInfo;
use crate::services::validator::AppImageValidator;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const MAX_METADATA_BYTES: usize = 2 << 20;
const MAX_DOWNLOAD_BYTES: u64 = 8 << 30;

fn update_error(msg: impl Into<String>) -> AppImageError {
    AppImageError::UpdateError(msg.into())
}

#[derive(Debug, Clone)]
pub struct UpdaterConfig {
    pub api_base: String,
    /// Allow plain http to 127.0.0.1/localhost. Only meant for tests; never enabled by the
    /// application.
    pub allow_loopback_http: bool,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        Self {
            api_base: "https://api.github.com".to_string(),
            allow_loopback_http: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteUpdate {
    pub download_url: String,
    /// SHA-1 of the new AppImage, taken from the zsync control file
    pub sha1: String,
    pub length: u64,
    pub filename: String,
    pub release: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheck {
    /// The AppImage carries no update information
    NoUpdateInformation,
    /// It uses an update transport AppNest does not support
    Unsupported(String),
    UpToDate,
    Available(RemoteUpdate),
}

#[derive(Default)]
pub struct AppImageUpdater {
    config: UpdaterConfig,
}

/// Matches `name` against a pattern where `*` matches any run of characters and `?` one.
fn glob_match(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ni) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

impl AppImageUpdater {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(config: UpdaterConfig) -> Self {
        Self { config }
    }

    fn check_url(&self, url: &str) -> Result<(), AppImageError> {
        let loopback = url.starts_with("http://127.0.0.1")
            || url.starts_with("http://localhost")
            || url.starts_with("http://[::1]");
        if url.starts_with("https://") || (self.config.allow_loopback_http && loopback) {
            Ok(())
        } else {
            Err(update_error(format!(
                "Refusing to use a non-https URL: {}",
                url
            )))
        }
    }

    fn curl(&self, url: &str) -> Result<Command, AppImageError> {
        self.check_url(url)?;
        let mut cmd = Command::new("curl");
        cmd.args(["--fail", "--silent", "--show-error", "--location"])
            .args([
                "--proto",
                if self.config.allow_loopback_http {
                    "=http,https"
                } else {
                    "=https"
                },
            ])
            .args(["--proto-redir", "=https"])
            .args(["--connect-timeout", "20"])
            .args(["-A", concat!("AppNest/", env!("CARGO_PKG_VERSION"))])
            .stdin(Stdio::null());
        Ok(cmd)
    }

    fn missing_curl(e: std::io::Error) -> AppImageError {
        if e.kind() == std::io::ErrorKind::NotFound {
            update_error(
                "curl is required to check for and download updates (install the curl package)",
            )
        } else {
            update_error(format!("Failed to run curl: {}", e))
        }
    }

    /// Downloads a small document (API response or zsync control file) into memory.
    fn fetch(&self, url: &str, accept: Option<&str>) -> Result<Vec<u8>, AppImageError> {
        let mut cmd = self.curl(url)?;
        cmd.args(["--max-time", "30"])
            .args(["--max-filesize", &MAX_METADATA_BYTES.to_string()]);
        if let Some(a) = accept {
            cmd.args(["-H", &format!("Accept: {}", a)]);
        }
        let out = cmd
            .arg("--")
            .arg(url)
            .output()
            .map_err(Self::missing_curl)?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr);
            return Err(update_error(format!(
                "Could not fetch {}: {}",
                url,
                msg.trim()
            )));
        }
        if out.stdout.len() > MAX_METADATA_BYTES {
            return Err(update_error("The update metadata is unexpectedly large"));
        }
        Ok(out.stdout)
    }

    fn resolve_zsync(&self, info: &UpdateInfo) -> Result<(String, Option<String>), AppImageError> {
        match info {
            UpdateInfo::Zsync { url } => Ok((url.clone(), None)),
            UpdateInfo::GhReleasesZsync {
                owner,
                repo,
                tag,
                pattern,
            } => {
                let url = if tag == "latest" {
                    format!(
                        "{}/repos/{}/{}/releases/latest",
                        self.config.api_base, owner, repo
                    )
                } else {
                    format!(
                        "{}/repos/{}/{}/releases/tags/{}",
                        self.config.api_base, owner, repo, tag
                    )
                };
                let body = self.fetch(&url, Some("application/vnd.github+json"))?;
                let json: serde_json::Value = serde_json::from_slice(&body).map_err(|e| {
                    update_error(format!("Unexpected response from {}: {}", url, e))
                })?;
                let release = json["tag_name"].as_str().map(str::to_string);
                let asset = json["assets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|a| a["name"].as_str().is_some_and(|n| glob_match(pattern, n)))
                    .ok_or_else(|| {
                        update_error(format!("No release asset matches \"{}\"", pattern))
                    })?;
                let download = asset["browser_download_url"]
                    .as_str()
                    .ok_or_else(|| update_error("The release asset has no download URL"))?;
                Ok((download.to_string(), release))
            }
            UpdateInfo::Unsupported(kind) => {
                Err(update_error(format!("Unsupported update method: {}", kind)))
            }
        }
    }

    /// Resolves a `URL:` value from a zsync file, which may be relative to the zsync URL.
    fn resolve_download_url(&self, zsync_url: &str, value: &str) -> Result<String, AppImageError> {
        let url = if value.starts_with("https://") || value.starts_with("http://") {
            value.to_string()
        } else {
            let safe = !value.is_empty()
                && !value.starts_with('/')
                && !value.split('/').any(|c| c == "..")
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-~+%/".contains(&b));
            if !safe {
                return Err(update_error(
                    "The update metadata contains an unusable download URL",
                ));
            }
            let base = zsync_url.split(['?', '#']).next().unwrap_or(zsync_url);
            let dir = &base[..base.rfind('/').map_or(0, |i| i + 1)];
            format!("{}{}", dir, value)
        };
        self.check_url(&url)?;
        Ok(url)
    }

    fn parse_zsync_header(
        &self,
        zsync_url: &str,
        data: &[u8],
        release: Option<String>,
    ) -> Result<RemoteUpdate, AppImageError> {
        let end = data
            .windows(2)
            .position(|w| w == b"\n\n")
            .ok_or_else(|| update_error("Not a valid zsync control file"))?;
        let text = String::from_utf8_lossy(&data[..end]);
        let field = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(key).and_then(|v| v.strip_prefix(':')))
                .map(|v| v.trim().to_string())
        };
        let sha1 = field("SHA-1")
            .filter(|h| h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| {
                update_error("The update metadata has no SHA-1 checksum, so it cannot be verified")
            })?
            .to_lowercase();
        let length = field("Length").and_then(|l| l.parse().ok()).unwrap_or(0);
        let filename = field("Filename").unwrap_or_default();
        let url_value =
            field("URL").ok_or_else(|| update_error("The update metadata has no download URL"))?;
        Ok(RemoteUpdate {
            download_url: self.resolve_download_url(zsync_url, &url_value)?,
            sha1,
            length,
            filename,
            release,
        })
    }

    /// Checks the publisher's update information. This contacts the network.
    pub fn check(&self, appimage: &Path) -> Result<UpdateCheck, AppImageError> {
        let Some(info) = UpdateInfo::read(appimage) else {
            return Ok(UpdateCheck::NoUpdateInformation);
        };
        if let UpdateInfo::Unsupported(kind) = &info {
            return Ok(UpdateCheck::Unsupported(kind.clone()));
        }

        let (zsync_url, release) = self.resolve_zsync(&info)?;
        let control = self.fetch(&zsync_url, None)?;
        let update = self.parse_zsync_header(&zsync_url, &control, release)?;

        let local = sha1_file(appimage)?;
        Ok(if local == update.sha1 {
            UpdateCheck::UpToDate
        } else {
            UpdateCheck::Available(update)
        })
    }

    /// Creates an empty private file to download into.
    pub fn new_download_path(dir: &Path) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let file = tempfile::Builder::new()
            .prefix("update-")
            .suffix(".AppImage")
            .tempfile_in(dir)?;
        let (_, path) = file.keep().map_err(|e| e.error)?;
        Ok(path)
    }

    /// Downloads the update into `dest`, reporting `(bytes so far, expected total)`. The file
    /// is verified (size, SHA-1 from the update metadata, AppImage validation) and removed
    /// again if anything is wrong. `cancel` aborts the transfer.
    pub fn download(
        &self,
        update: &RemoteUpdate,
        dest: &Path,
        progress: &mut dyn FnMut(u64, u64),
        cancel: &AtomicBool,
    ) -> Result<(), AppImageError> {
        let result = self.download_inner(update, dest, progress, cancel);
        if result.is_err() {
            let _ = std::fs::remove_file(dest);
        }
        result
    }

    fn download_inner(
        &self,
        update: &RemoteUpdate,
        dest: &Path,
        progress: &mut dyn FnMut(u64, u64),
        cancel: &AtomicBool,
    ) -> Result<(), AppImageError> {
        let limit = if update.length > 0 {
            update.length
        } else {
            MAX_DOWNLOAD_BYTES
        };
        let mut child = self
            .curl(&update.download_url)?
            .args(["--speed-limit", "1024", "--speed-time", "60"])
            .args(["--max-filesize", &limit.min(MAX_DOWNLOAD_BYTES).to_string()])
            .arg("--output")
            .arg(dest)
            .arg("--")
            .arg(&update.download_url)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(Self::missing_curl)?;

        let status = loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(update_error("Download cancelled"));
            }
            if let Some(status) = child.try_wait()? {
                break status;
            }
            let done = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
            progress(done, update.length);
            std::thread::sleep(Duration::from_millis(250));
        };
        if !status.success() {
            let mut msg = String::new();
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_string(&mut msg);
            }
            return Err(update_error(format!("Download failed: {}", msg.trim())));
        }

        let size = std::fs::metadata(dest)?.len();
        progress(size, update.length.max(size));
        if update.length > 0 && size != update.length {
            return Err(update_error(format!(
                "The downloaded file has the wrong size ({} instead of {} bytes)",
                size, update.length
            )));
        }
        if sha1_file(dest)? != update.sha1 {
            return Err(update_error(
                "The downloaded file does not match the published SHA-1 checksum",
            ));
        }
        AppImageValidator::validate(dest).map_err(|e| {
            update_error(format!(
                "The downloaded file is not a valid AppImage: {}",
                e
            ))
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_matching() {
        assert!(glob_match(
            "*x86_64.AppImage.zsync",
            "Tool-1.2-x86_64.AppImage.zsync"
        ));
        assert!(glob_match("Tool-?.AppImage.zsync", "Tool-1.AppImage.zsync"));
        assert!(!glob_match(
            "*x86_64.AppImage.zsync",
            "Tool-1.2-aarch64.AppImage.zsync"
        ));
        assert!(glob_match("*", "anything"));
        assert!(!glob_match("a*b", "ac"));
    }

    #[test]
    fn relative_download_urls_resolve_against_the_zsync_url() {
        let u = AppImageUpdater::new();
        assert_eq!(
            u.resolve_download_url(
                "https://example.org/dl/app.AppImage.zsync?x=1",
                "app-2.AppImage"
            )
            .unwrap(),
            "https://example.org/dl/app-2.AppImage"
        );
        assert!(u
            .resolve_download_url("https://example.org/a.zsync", "../evil")
            .is_err());
        assert!(u
            .resolve_download_url("https://example.org/a.zsync", "/abs")
            .is_err());
        assert!(u
            .resolve_download_url("https://example.org/a.zsync", "http://example.org/x")
            .is_err());
    }
}
