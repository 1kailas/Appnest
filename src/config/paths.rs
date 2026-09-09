use directories::{BaseDirs, ProjectDirs};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub applications_dir: PathBuf,
    pub extracted_dir: PathBuf,
    pub icons_dir: PathBuf,
    pub database_file: PathBuf,
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub cache_dir: PathBuf,
    pub desktop_applications_dir: PathBuf,
}

impl Default for AppPaths {
    fn default() -> Self {
        Self::new()
    }
}

impl AppPaths {
    pub fn new() -> Self {
        let proj_dirs = ProjectDirs::from("io", "github", "appnest");
        let base_dirs = BaseDirs::new();

        let data_dir = proj_dirs
            .as_ref()
            .map(|p| p.data_local_dir().to_path_buf())
            .unwrap_or_else(|| {
                let mut home = base_dirs
                    .as_ref()
                    .map(|b| b.home_dir().to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("/tmp"));
                home.push(".local/share/appnest");
                home
            });

        // Migrate old data directory if needed
        if let Some(b) = base_dirs.as_ref() {
            let old_data = b.home_dir().join(".local/share/appimage-manager");
            if old_data.exists() && !data_dir.exists() {
                let _ = std::fs::rename(&old_data, &data_dir);
            }
        }

        let config_dir = proj_dirs
            .as_ref()
            .map(|p| p.config_dir().to_path_buf())
            .unwrap_or_else(|| {
                let mut home = base_dirs
                    .as_ref()
                    .map(|b| b.home_dir().to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("/tmp"));
                home.push(".config/appnest");
                home
            });

        // Migrate old config directory if needed
        if let Some(b) = base_dirs.as_ref() {
            let old_config = b.home_dir().join(".config/appimage-manager");
            if old_config.exists() && !config_dir.exists() {
                let _ = std::fs::rename(&old_config, &config_dir);
            }
        }

        let cache_dir = proj_dirs
            .as_ref()
            .map(|p| p.cache_dir().to_path_buf())
            .unwrap_or_else(|| {
                let mut home = base_dirs
                    .as_ref()
                    .map(|b| b.home_dir().to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("/tmp"));
                home.push(".cache/appnest");
                home
            });

        let desktop_applications_dir = base_dirs
            .as_ref()
            .map(|b| b.data_local_dir().join("applications"))
            .unwrap_or_else(|| PathBuf::from("/usr/share/applications"));

        let applications_dir = data_dir.join("applications");
        let extracted_dir = data_dir.join("extracted");
        let icons_dir = data_dir.join("icons");
        let database_file = data_dir.join("apps.toml");
        let config_file = config_dir.join("config.toml");

        Self {
            data_dir,
            applications_dir,
            extracted_dir,
            icons_dir,
            database_file,
            config_dir,
            config_file,
            cache_dir,
            desktop_applications_dir,
        }
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.applications_dir)?;
        std::fs::create_dir_all(&self.extracted_dir)?;
        std::fs::create_dir_all(&self.icons_dir)?;
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.cache_dir)?;
        std::fs::create_dir_all(&self.desktop_applications_dir)?;
        Ok(())
    }

    pub fn app_extracted_dir(&self, app_id: &str) -> PathBuf {
        self.extracted_dir.join(app_id)
    }

    pub fn app_icon_path(&self, app_id: &str, ext: &str) -> PathBuf {
        self.icons_dir
            .join(format!("{}.{}", app_id, ext.trim_start_matches('.')))
    }

    pub fn app_desktop_file(&self, app_id: &str) -> PathBuf {
        self.desktop_applications_dir
            .join(format!("{}.desktop", app_id))
    }

    pub fn default_scan_directories(&self) -> Vec<PathBuf> {
        let mut list = Vec::new();
        list.push(self.applications_dir.clone());

        if let Some(base) = BaseDirs::new() {
            let home_apps = base.home_dir().join("Applications");
            if home_apps.exists() && !list.contains(&home_apps) {
                list.push(home_apps);
            }
            let downloads = base.home_dir().join("Downloads");
            if downloads.exists() && !list.contains(&downloads) {
                list.push(downloads);
            }
        }
        list
    }
}
