use crate::config::paths::AppPaths;
use crate::config::settings::AppSettings;
use crate::infrastructure::filesystem::FileSystem;
use std::fs;

pub struct ConfigStore {
    paths: AppPaths,
}

impl ConfigStore {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn load(&self) -> AppSettings {
        if !self.paths.config_file.exists() {
            let default_settings = AppSettings {
                scan_directories: self.paths.default_scan_directories(),
                ..Default::default()
            };
            let _ = self.save(&default_settings);
            return default_settings;
        }

        match fs::read_to_string(&self.paths.config_file) {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|_| AppSettings {
                scan_directories: self.paths.default_scan_directories(),
                ..Default::default()
            }),
            Err(_) => AppSettings {
                scan_directories: self.paths.default_scan_directories(),
                ..Default::default()
            },
        }
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), std::io::Error> {
        let serialized = toml::to_string_pretty(settings)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        FileSystem::atomic_write(&self.paths.config_file, &serialized)
    }
}
