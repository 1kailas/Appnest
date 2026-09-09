use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl std::fmt::Display for ThemePreference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThemePreference::System => write!(f, "System Default"),
            ThemePreference::Light => write!(f, "Light"),
            ThemePreference::Dark => write!(f, "Dark"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeMethodPreference {
    #[default]
    Auto,
    Extracted,
    Native,
    Fuse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub copy_on_import: bool,
    pub auto_integrate_desktop: bool,
    pub scan_directories: Vec<PathBuf>,
    pub default_runtime_method: RuntimeMethodPreference,
    pub theme: ThemePreference,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            copy_on_import: true,
            auto_integrate_desktop: true,
            scan_directories: Vec::new(),
            default_runtime_method: RuntimeMethodPreference::Auto,
            theme: ThemePreference::System,
        }
    }
}
