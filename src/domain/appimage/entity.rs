use super::metadata::AppImageMetadata;
use chrono::{DateTime, Local, TimeZone};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AppImageType {
    Type1,
    Type2,
    #[default]
    Unknown,
}

impl std::fmt::Display for AppImageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppImageType::Type1 => write!(f, "Type 1"),
            AppImageType::Type2 => write!(f, "Type 2"),
            AppImageType::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Architecture {
    X86_64,
    Aarch64,
    Armhf,
    I686,
    #[default]
    Unknown,
}

impl std::fmt::Display for Architecture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Architecture::X86_64 => write!(f, "x86_64"),
            Architecture::Aarch64 => write!(f, "aarch64"),
            Architecture::Armhf => write!(f, "armhf"),
            Architecture::I686 => write!(f, "i686"),
            Architecture::Unknown => write!(f, "Unknown Arch"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeMethod {
    #[default]
    Auto,
    Extracted,
    Native,
    #[serde(alias = "fuse")]
    ExtractAndRun,
}

impl std::fmt::Display for RuntimeMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeMethod::Auto => write!(f, "Auto (Adaptive)"),
            RuntimeMethod::Extracted => write!(f, "Extracted AppRun"),
            RuntimeMethod::Native => write!(f, "Direct Native"),
            RuntimeMethod::ExtractAndRun => write!(f, "Extract and run (no FUSE)"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppImage {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub file_size: u64,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub icon_path: Option<PathBuf>,
    #[serde(default)]
    pub desktop_entry_path: Option<PathBuf>,
    #[serde(default)]
    pub extracted_dir: Option<PathBuf>,
    #[serde(default)]
    pub appimage_type: AppImageType,
    #[serde(default)]
    pub architecture: Architecture,
    #[serde(default)]
    pub runtime_method: RuntimeMethod,
    #[serde(default)]
    pub installed_at: u64,
    #[serde(default)]
    pub managed: bool,
    #[serde(default)]
    pub desktop_integrated: bool,
    #[serde(default)]
    pub desktop_shortcut: bool,
    #[serde(default)]
    pub metadata: AppImageMetadata,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub sha256_size: Option<u64>,
    #[serde(default)]
    pub sha256_mtime: Option<u64>,
}

impl AppImage {
    pub fn new(id: impl Into<String>, name: impl Into<String>, path: PathBuf) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

        Self {
            id: id.into(),
            name: name.into(),
            path,
            file_size,
            version: None,
            icon_path: None,
            desktop_entry_path: None,
            extracted_dir: None,
            appimage_type: AppImageType::Unknown,
            architecture: Architecture::Unknown,
            runtime_method: RuntimeMethod::Auto,
            installed_at: now,
            managed: true,
            desktop_integrated: false,
            desktop_shortcut: false,
            metadata: AppImageMetadata::default(),
            sha256: None,
            sha256_size: None,
            sha256_mtime: None,
        }
    }

    pub fn formatted_size(&self) -> String {
        const KB: f64 = 1024.0;
        const MB: f64 = KB * 1024.0;
        const GB: f64 = MB * 1024.0;

        let bytes = self.file_size as f64;
        if bytes >= GB {
            format!("{:.2} GB", bytes / GB)
        } else if bytes >= MB {
            format!("{:.1} MB", bytes / MB)
        } else if bytes >= KB {
            format!("{:.0} KB", bytes / KB)
        } else {
            format!("{} bytes", self.file_size)
        }
    }

    pub fn formatted_date(&self) -> String {
        if self.installed_at == 0 {
            return "Unknown".to_string();
        }
        let dt: Option<DateTime<Local>> = Local.timestamp_opt(self.installed_at as i64, 0).single();
        dt.map(|d| d.format("%b %-d, %Y • %-I:%M %p").to_string())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    pub fn command_preview(&self) -> String {
        let base = match self.runtime_method {
            RuntimeMethod::Extracted => {
                if let Some(ref apprun) = self.apprun_path() {
                    format!("\"{}\"", apprun.display())
                } else {
                    format!("\"{}\"", self.path.display())
                }
            }
            RuntimeMethod::ExtractAndRun => {
                format!("\"{}\" --appimage-extract-and-run", self.path.display())
            }
            RuntimeMethod::Native => {
                format!("\"{}\"", self.path.display())
            }
            RuntimeMethod::Auto => {
                if self.is_extracted() {
                    if let Some(ref apprun) = self.apprun_path() {
                        format!("\"{}\" (Auto -> Extracted)", apprun.display())
                    } else {
                        format!("\"{}\" (Auto)", self.path.display())
                    }
                } else {
                    format!("\"{}\" (Auto)", self.path.display())
                }
            }
        };

        base
    }

    pub fn exists(&self) -> bool {
        self.path.is_file()
    }

    fn file_stamp(&self) -> Option<(u64, u64)> {
        let meta = std::fs::metadata(&self.path).ok()?;
        let mtime = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        Some((meta.len(), mtime))
    }

    /// The stored SHA-256 hash, only if the file has not changed since it was computed.
    pub fn cached_sha256(&self) -> Option<&str> {
        let hash = self.sha256.as_deref()?;
        let stamp = self.file_stamp()?;
        (self.sha256_size == Some(stamp.0) && self.sha256_mtime == Some(stamp.1)).then_some(hash)
    }

    pub fn set_sha256(&mut self, hash: String) {
        let stamp = self.file_stamp();
        self.sha256 = Some(hash);
        self.sha256_size = stamp.map(|s| s.0);
        self.sha256_mtime = stamp.map(|s| s.1);
    }

    pub fn is_extracted(&self) -> bool {
        if let Some(ref dir) = self.extracted_dir {
            dir.join("AppRun").exists()
        } else {
            false
        }
    }

    pub fn apprun_path(&self) -> Option<PathBuf> {
        self.extracted_dir.as_ref().map(|d| d.join("AppRun"))
    }

    pub fn sanitize_id(raw_name: &str) -> String {
        let mut clean = String::new();
        let mut last_dash = false;

        for c in raw_name.chars() {
            if c.is_ascii_alphanumeric() {
                clean.push(c.to_ascii_lowercase());
                last_dash = false;
            } else if matches!(c, '-' | '_' | ' ' | '.') && !last_dash && !clean.is_empty() {
                clean.push('-');
                last_dash = true;
            }
        }
        let trimmed: String = clean.chars().take(64).collect();
        let trimmed = trimmed.trim_matches('-').to_string();
        if trimmed.is_empty() {
            "appimage-item".to_string()
        } else {
            trimmed
        }
    }
}
