use super::metadata::AppImageMetadata;
use chrono::{DateTime, Local, TimeZone};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

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
    Fuse,
}

impl std::fmt::Display for RuntimeMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeMethod::Auto => write!(f, "Auto (Adaptive)"),
            RuntimeMethod::Extracted => write!(f, "Extracted AppRun"),
            RuntimeMethod::Native => write!(f, "Direct Native"),
            RuntimeMethod::Fuse => write!(f, "FUSE / Extract-and-run"),
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
    pub metadata: AppImageMetadata,
    #[serde(default)]
    pub sha256: Option<String>,
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
            metadata: AppImageMetadata::default(),
            sha256: None,
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
            RuntimeMethod::Fuse => {
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

        if let Some(ref args) = self.metadata.exec_args {
            if args.contains("--no-sandbox") && !base.contains("--no-sandbox") {
                return format!("{} --no-sandbox", base);
            }
        }
        base
    }

    pub fn calculate_sha256(&self) -> Option<String> {
        if !self.path.exists() {
            return None;
        }

        let output = Command::new("sha256sum").arg(&self.path).output().ok()?;

        if output.status.success() {
            let out_str = String::from_utf8_lossy(&output.stdout);
            let hash = out_str.split_whitespace().next()?;
            Some(hash.to_string())
        } else {
            None
        }
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
            } else if c == '-' || c == '_' || c == ' ' || c == '.' {
                if !last_dash && !clean.is_empty() {
                    clean.push('-');
                    last_dash = true;
                }
            }
        }
        let trimmed = clean.trim_matches('-').to_string();
        if trimmed.is_empty() {
            "appimage-item".to_string()
        } else {
            trimmed
        }
    }
}
