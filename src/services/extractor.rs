use crate::domain::appimage::{AppImageError, AppImageMetadata};
use crate::infrastructure::filesystem::FileSystem;
use crate::services::validator::AppImageValidator;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use walkdir::WalkDir;

const MAX_EXTRACT_BYTES: u64 = 8 << 30;
const MAX_EXTRACT_ENTRIES: usize = 300_000;
const MAX_PARTIAL_BYTES: u64 = 512 << 20;
const MAX_PARTIAL_ENTRIES: usize = 50_000;
const MAX_ICON_BYTES: u64 = 10 << 20;
const EXEC_EXTRACT_TIMEOUT: Duration = Duration::from_secs(300);

pub struct AppImageExtractor;

fn extraction_error(msg: impl Into<String>) -> AppImageError {
    AppImageError::ExtractionError(msg.into())
}

/// Finds a 7-Zip executable (`7z`, `7zz` or `7za`).
fn seven_zip() -> Option<&'static str> {
    static BIN: OnceLock<Option<&'static str>> = OnceLock::new();
    *BIN.get_or_init(|| {
        ["7z", "7zz", "7za"].into_iter().find(|bin| {
            Command::new(bin)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok()
        })
    })
}

#[derive(Default)]
struct ArchiveEntry {
    path: String,
    is_dir: bool,
    size: u64,
}

impl AppImageExtractor {
    /// Extracts the full AppDir of an AppImage into `target_dir`.
    ///
    /// Extraction happens in a private staging directory next to the target. The tree is
    /// checked (paths, symlinks, special files, size and entry limits, `AppRun`) and only then
    /// moved into place, so an incomplete or unsafe extraction is never left behind.
    ///
    /// The AppImage is never executed unless `allow_execute_fallback` is set; that is only
    /// appropriate when the user is launching the application anyway.
    pub fn extract_appdir(
        appimage_path: &Path,
        target_dir: &Path,
        allow_execute_fallback: bool,
    ) -> Result<(), AppImageError> {
        AppImageValidator::validate(appimage_path)?;
        let parent = target_dir
            .parent()
            .ok_or_else(|| AppImageError::UnsafePath(target_dir.display().to_string()))?;
        fs::create_dir_all(parent)?;
        let staging = tempfile::Builder::new()
            .prefix(".extract-")
            .tempdir_in(parent)
            .map_err(|e| extraction_error(format!("Failed to create staging directory: {}", e)))?;

        let mut tree = staging.path().join("tree");
        let mut extracted = false;

        if let Some(bin) = seven_zip() {
            Self::check_archive_listing(bin, appimage_path)?;
            fs::create_dir(&tree)?;
            let _ = Command::new(bin)
                .arg("x")
                .arg("-y")
                .arg(format!("-o{}", tree.display()))
                .arg("--")
                .arg(appimage_path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            // 7-Zip reports skipped (unsafe) links through its exit code, so success is judged
            // by the presence of AppRun and by the checks below.
            extracted = fs::symlink_metadata(tree.join("AppRun")).is_ok();
        }

        if !extracted && allow_execute_fallback {
            let work = staging.path().join("exec");
            fs::create_dir(&work)?;
            if Self::run_with_timeout(appimage_path, &work, EXEC_EXTRACT_TIMEOUT) {
                tree = work.join("squashfs-root");
                extracted = fs::symlink_metadata(tree.join("AppRun")).is_ok();
            }
        }

        if !extracted {
            return Err(extraction_error(if seven_zip().is_none() {
                "7-Zip is not installed (install the 7zip package) and the AppImage was not run"
            } else {
                "Failed to extract the AppImage"
            }));
        }

        Self::sanitize_tree(&tree, MAX_EXTRACT_BYTES, MAX_EXTRACT_ENTRIES)?;
        Self::verify_apprun(&tree)?;
        let _ = FileSystem::make_executable(tree.join("AppRun"));
        Self::commit_dir(&tree, target_dir, parent)
    }

    fn run_with_timeout(appimage: &Path, cwd: &Path, timeout: Duration) -> bool {
        let Ok(mut child) = Command::new(appimage)
            .arg("--appimage-extract")
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status.success(),
                Ok(None) if start.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(100))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
            }
        }
    }

    /// Lists the archive and rejects it before extraction if it contains entries with unsafe
    /// paths or exceeds the size or entry limits.
    fn check_archive_listing(bin: &str, appimage: &Path) -> Result<(), AppImageError> {
        let output = Command::new(bin)
            .args(["l", "-slt", "--"])
            .arg(appimage)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| extraction_error(format!("Failed to run {}: {}", bin, e)))?;
        let entries = Self::parse_listing(&String::from_utf8_lossy(&output.stdout));
        if entries.is_empty() {
            return Err(extraction_error("Could not read the AppImage's filesystem"));
        }
        Self::check_entries(&entries, MAX_EXTRACT_BYTES, MAX_EXTRACT_ENTRIES)
    }

    fn parse_listing(listing: &str) -> Vec<ArchiveEntry> {
        let mut entries = Vec::new();
        let mut current: Option<ArchiveEntry> = None;
        let mut in_entries = false;
        for line in listing.lines() {
            if line.starts_with("----------") {
                in_entries = true;
            } else if !in_entries {
                continue;
            } else if let Some(path) = line.strip_prefix("Path = ") {
                entries.extend(current.take());
                current = Some(ArchiveEntry {
                    path: path.to_string(),
                    ..Default::default()
                });
            } else if let (Some(entry), Some(value)) =
                (current.as_mut(), line.strip_prefix("Folder = "))
            {
                entry.is_dir = value.trim() == "+";
            } else if let (Some(entry), Some(value)) =
                (current.as_mut(), line.strip_prefix("Size = "))
            {
                entry.size = value.trim().parse().unwrap_or(0);
            }
        }
        entries.extend(current);
        entries
    }

    fn check_entries(
        entries: &[ArchiveEntry],
        max_bytes: u64,
        max_entries: usize,
    ) -> Result<(), AppImageError> {
        if entries.len() > max_entries {
            return Err(extraction_error(format!(
                "AppImage contains too many files ({})",
                entries.len()
            )));
        }
        let mut total = 0u64;
        for e in entries {
            let p = Path::new(&e.path);
            if p.is_absolute() || p.components().any(|c| c == Component::ParentDir) {
                return Err(AppImageError::UnsafePath(format!(
                    "archive entry escapes the extraction directory: {}",
                    e.path
                )));
            }
            if !e.is_dir {
                total = total.saturating_add(e.size);
            }
        }
        if total > max_bytes {
            return Err(extraction_error(
                "AppImage contents are too large to extract",
            ));
        }
        Ok(())
    }

    /// Makes an extracted tree safe: removes symlinks that point outside the tree, special
    /// files and set-id bits, and enforces size and entry limits. Nothing is followed while
    /// walking.
    pub fn sanitize_tree(
        root: &Path,
        max_bytes: u64,
        max_entries: usize,
    ) -> Result<(), AppImageError> {
        let canon_root = fs::canonicalize(root)?;
        let (mut total, mut count) = (0u64, 0usize);
        let mut remove = Vec::new();

        for entry in WalkDir::new(root).follow_links(false).min_depth(1) {
            let entry = entry.map_err(|e| extraction_error(e.to_string()))?;
            count += 1;
            if count > max_entries {
                return Err(extraction_error("AppImage contains too many files"));
            }
            let ft = entry.file_type();
            let path = entry.path();
            if ft.is_dir() {
                let meta = entry
                    .metadata()
                    .map_err(|e| extraction_error(e.to_string()))?;
                let mode = meta.permissions().mode();
                if mode & 0o700 != 0o700 || mode & 0o7000 != 0 {
                    fs::set_permissions(path, fs::Permissions::from_mode((mode & 0o777) | 0o700))?;
                }
            } else if ft.is_file() {
                let meta = entry
                    .metadata()
                    .map_err(|e| extraction_error(e.to_string()))?;
                total = total.saturating_add(meta.len());
                if total > max_bytes {
                    return Err(extraction_error("AppImage contents are too large"));
                }
                let mode = meta.permissions().mode();
                if mode & 0o7000 != 0 {
                    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777))?;
                }
            } else if ft.is_symlink() {
                let target = fs::read_link(path)?;
                let rel_parent = path
                    .parent()
                    .and_then(|p| p.strip_prefix(root).ok())
                    .unwrap_or(Path::new(""));
                if target.is_absolute()
                    || FileSystem::normalize_relative(&rel_parent.join(&target)).is_none()
                {
                    remove.push(path.to_path_buf());
                }
            } else {
                remove.push(path.to_path_buf());
            }
        }
        for path in remove {
            let _ = fs::remove_file(path);
        }

        // Chains of symlinks can still leave the tree; check the real resolution.
        for _ in 0..8 {
            let mut removed = false;
            for entry in WalkDir::new(root)
                .follow_links(false)
                .min_depth(1)
                .into_iter()
                .flatten()
            {
                if entry.file_type().is_symlink() {
                    if let Ok(real) = fs::canonicalize(entry.path()) {
                        if !real.starts_with(&canon_root) {
                            removed |= fs::remove_file(entry.path()).is_ok();
                        }
                    }
                }
            }
            if !removed {
                break;
            }
        }
        Ok(())
    }

    fn verify_apprun(root: &Path) -> Result<(), AppImageError> {
        let canon_root = fs::canonicalize(root)?;
        match fs::canonicalize(root.join("AppRun")) {
            Ok(real) if real.starts_with(&canon_root) && real.is_file() => Ok(()),
            _ => Err(extraction_error(
                "AppRun is missing or does not resolve inside the extracted AppDir",
            )),
        }
    }

    /// Moves the verified tree into place, keeping any previous extraction until the new one
    /// is in position.
    fn commit_dir(tree: &Path, target: &Path, parent: &Path) -> Result<(), AppImageError> {
        if fs::symlink_metadata(target).is_ok() {
            let old = tempfile::Builder::new()
                .prefix(".old-")
                .tempdir_in(parent)?
                .keep();
            let old_slot = old.join("previous");
            fs::rename(target, &old_slot)?;
            if let Err(e) = fs::rename(tree, target) {
                let _ = fs::rename(&old_slot, target);
                let _ = fs::remove_dir_all(&old);
                return Err(extraction_error(format!(
                    "Failed to install extraction: {}",
                    e
                )));
            }
            let _ = fs::remove_dir_all(&old);
        } else {
            fs::rename(tree, target)
                .map_err(|e| extraction_error(format!("Failed to install extraction: {}", e)))?;
        }
        Ok(())
    }

    /// Extracts only the files needed to describe an AppImage into a private temporary
    /// directory. Never executes the AppImage.
    fn extract_partial(appimage: &Path, patterns: &[&str]) -> Option<tempfile::TempDir> {
        let bin = seven_zip()?;
        let temp = tempfile::tempdir().ok()?;
        let _ = Command::new(bin)
            .arg("x")
            .arg("-y")
            .arg(format!("-o{}", temp.path().display()))
            .arg("--")
            .arg(appimage)
            .args(patterns)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        Self::sanitize_tree(temp.path(), MAX_PARTIAL_BYTES, MAX_PARTIAL_ENTRIES).ok()?;
        Some(temp)
    }

    const DESCRIBE_PATTERNS: [&'static str; 6] = [
        "*.desktop",
        ".DirIcon",
        "*.png",
        "*.svg",
        "usr/share/icons/*",
        "usr/share/pixmaps/*",
    ];

    pub fn extract_metadata_and_icon(
        appimage_path: &Path,
        icons_dir: &Path,
        app_id: &str,
    ) -> Result<(AppImageMetadata, Option<PathBuf>), AppImageError> {
        AppImageValidator::validate(appimage_path)?;
        fs::create_dir_all(icons_dir)?;
        let Some(temp) = Self::extract_partial(appimage_path, &Self::DESCRIBE_PATTERNS) else {
            return Ok((AppImageMetadata::default(), None));
        };

        let desktop_file = Self::find_root_desktop_file(temp.path())
            .or_else(|| Self::find_file_with_ext(temp.path(), "desktop"));
        let metadata = desktop_file
            .map(|f| Self::parse_desktop_file(&f))
            .unwrap_or_default();
        let icon = Self::resolve_and_copy_icon(
            temp.path(),
            icons_dir,
            app_id,
            metadata.icon_name.as_deref(),
        );
        Ok((metadata, icon))
    }

    pub fn extract_icon_only(
        appimage_path: &Path,
        icons_dir: &Path,
        app_id: &str,
        icon_name: Option<&str>,
    ) -> Option<PathBuf> {
        AppImageValidator::validate(appimage_path).ok()?;
        fs::create_dir_all(icons_dir).ok()?;
        let temp = Self::extract_partial(
            appimage_path,
            &[
                ".DirIcon",
                "*.png",
                "*.svg",
                "usr/share/icons/*",
                "usr/share/pixmaps/*",
            ],
        )?;
        Self::resolve_and_copy_icon(temp.path(), icons_dir, app_id, icon_name)
    }

    /// Reads the image type and, for PNG, the pixel size. Returns `None` for anything that is
    /// not a PNG or SVG.
    fn image_info(path: &Path) -> Option<(bool, u32)> {
        let meta = fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_ICON_BYTES {
            return None;
        }
        let mut head = [0u8; 4096];
        let n = File::open(path).ok()?.read(&mut head).ok()?;
        let head = &head[..n];
        if head.len() >= 24 && head.starts_with(b"\x89PNG\r\n\x1a\n") && &head[12..16] == b"IHDR" {
            let w = u32::from_be_bytes(head[16..20].try_into().ok()?);
            let h = u32::from_be_bytes(head[20..24].try_into().ok()?);
            return (w > 0 && h > 0).then_some((false, w.min(h)));
        }
        let text = String::from_utf8_lossy(head);
        let text = text.trim_start_matches('\u{feff}').trim_start();
        let is_svg =
            (text.starts_with("<?xml") || text.starts_with("<svg") || text.starts_with("<!"))
                && text.contains("<svg");
        is_svg.then_some((true, 0))
    }

    /// Scores an icon: SVG is ideal; PNGs are best near 256-512 px.
    fn icon_score(is_svg: bool, size: u32) -> u32 {
        if is_svg {
            512
        } else if size <= 512 {
            size
        } else {
            512u32.saturating_sub((size - 512) / 64)
        }
    }

    /// Resolves a candidate inside `root`, following symlinks only if the final target stays
    /// inside `root`.
    fn resolve_inside(root: &Path, candidate: &Path) -> Option<PathBuf> {
        let real = fs::canonicalize(candidate).ok()?;
        (real.starts_with(fs::canonicalize(root).ok()?) && real.is_file()).then_some(real)
    }

    fn pick_best(
        root: &Path,
        candidates: impl Iterator<Item = PathBuf>,
    ) -> Option<(PathBuf, bool)> {
        candidates
            .filter_map(|c| {
                let real = Self::resolve_inside(root, &c)?;
                let (is_svg, size) = Self::image_info(&real)?;
                let len = fs::metadata(&real).ok()?.len();
                Some((
                    Self::icon_score(is_svg, size),
                    std::cmp::Reverse(len),
                    real,
                    is_svg,
                ))
            })
            .max_by_key(|(score, len, _, _)| (*score, *len))
            .map(|(_, _, path, is_svg)| (path, is_svg))
    }

    fn resolve_and_copy_icon(
        extracted_root: &Path,
        icons_dir: &Path,
        app_id: &str,
        preferred_name: Option<&str>,
    ) -> Option<PathBuf> {
        if app_id.is_empty() || app_id.contains(['/', '\0']) || app_id.starts_with('.') {
            return None;
        }
        let all_files = || {
            WalkDir::new(extracted_root)
                .follow_links(false)
                .into_iter()
                .flatten()
                .filter(|e| !e.file_type().is_dir())
                .map(|e| e.into_path())
        };

        // 1. The icon named by the embedded desktop entry
        let named = preferred_name
            .filter(|n| !n.is_empty() && !n.contains('/'))
            .and_then(|name| {
                Self::pick_best(
                    extracted_root,
                    all_files().filter(|p| {
                        p.file_stem().is_some_and(|s| s.eq_ignore_ascii_case(name))
                            && p.extension().is_some_and(|e| {
                                e.eq_ignore_ascii_case("png") || e.eq_ignore_ascii_case("svg")
                            })
                    }),
                )
            });

        // 2. .DirIcon, 3. any other PNG or SVG
        let chosen = named
            .or_else(|| {
                Self::pick_best(
                    extracted_root,
                    std::iter::once(extracted_root.join(".DirIcon")),
                )
            })
            .or_else(|| {
                Self::pick_best(
                    extracted_root,
                    all_files().filter(|p| {
                        p.extension().is_some_and(|e| {
                            e.eq_ignore_ascii_case("png") || e.eq_ignore_ascii_case("svg")
                        })
                    }),
                )
            })?;

        let (source, is_svg) = chosen;
        let dest = icons_dir.join(format!("{}.{}", app_id, if is_svg { "svg" } else { "png" }));
        let bytes = fs::read(&source).ok()?;
        FileSystem::atomic_write_bytes(&dest, &bytes, 0o644).ok()?;
        Some(dest)
    }

    pub fn parse_desktop_file(path: &Path) -> AppImageMetadata {
        let mut meta = AppImageMetadata::default();
        let Ok(file) = File::open(path) else {
            return meta;
        };

        let mut in_entry_section = false;
        for line in BufReader::new(file.take(1 << 20))
            .lines()
            .map_while(Result::ok)
        {
            let line = line.trim();
            if line.starts_with('[') && line.ends_with(']') {
                in_entry_section = line == "[Desktop Entry]";
                continue;
            }
            if !in_entry_section || line.starts_with('#') {
                continue;
            }
            let Some((key, val)) = line.split_once('=') else {
                continue;
            };
            let (key, val) = (key.trim(), val.trim());
            let list = |v: &str| -> Vec<String> {
                v.split(';')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            };

            match key {
                "Name" if meta.display_name.is_none() => meta.display_name = Some(val.to_string()),
                "Comment" if meta.comment.is_none() => meta.comment = Some(val.to_string()),
                "Icon" => meta.icon_name = Some(val.to_string()),
                "Exec" => meta.exec_args = Some(val.to_string()),
                "X-AppImage-Version" | "Version" if meta.version.is_none() => {
                    meta.version = Some(val.to_string())
                }
                "Terminal" => meta.terminal = val.eq_ignore_ascii_case("true"),
                "Categories" => meta.categories = list(val),
                "MimeType" => meta.mime_types = list(val),
                _ => {}
            }
        }
        meta
    }

    fn find_root_desktop_file(dir: &Path) -> Option<PathBuf> {
        fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.path())
            .find(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("desktop"))
            })
    }

    fn find_file_with_ext(dir: &Path, target_ext: &str) -> Option<PathBuf> {
        WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .find(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case(target_ext))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "\
Path = a.AppImage
Type = SquashFS

----------
Path = AppRun
Folder = -
Size = 10

Path = usr
Folder = +
Size =

Path = usr/bin/tool
Folder = -
Size = 90
";

    #[test]
    fn parses_listing_entries() {
        let entries = AppImageExtractor::parse_listing(LISTING);
        assert_eq!(entries.len(), 3);
        assert!(entries[1].is_dir);
        assert_eq!(entries[2].size, 90);
        assert!(AppImageExtractor::check_entries(&entries, 1000, 10).is_ok());
    }

    #[test]
    fn rejects_unsafe_paths_and_limits() {
        let mut entries = AppImageExtractor::parse_listing(LISTING);
        assert!(
            AppImageExtractor::check_entries(&entries, 50, 10).is_err(),
            "size"
        );
        assert!(
            AppImageExtractor::check_entries(&entries, 1000, 2).is_err(),
            "count"
        );

        entries[0].path = "../escape".into();
        assert!(AppImageExtractor::check_entries(&entries, 1000, 10).is_err());
        entries[0].path = "/etc/passwd".into();
        assert!(AppImageExtractor::check_entries(&entries, 1000, 10).is_err());
        entries[0].path = "usr/../../x".into();
        assert!(AppImageExtractor::check_entries(&entries, 1000, 10).is_err());
    }

    #[test]
    fn icon_scoring_prefers_scalable_then_mid_sized_png() {
        let svg = AppImageExtractor::icon_score(true, 0);
        assert!(svg >= AppImageExtractor::icon_score(false, 512));
        assert!(
            AppImageExtractor::icon_score(false, 256) > AppImageExtractor::icon_score(false, 48)
        );
        assert!(
            AppImageExtractor::icon_score(false, 512) > AppImageExtractor::icon_score(false, 4096)
        );
    }
}
