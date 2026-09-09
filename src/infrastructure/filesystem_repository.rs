use crate::config::paths::AppPaths;
use crate::domain::appimage::{AppImage, AppImageError};
use crate::domain::repository::AppImageRepository;
use crate::infrastructure::filesystem::FileSystem;
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::RwLock;

#[derive(Debug, Serialize, Deserialize, Default)]
struct DatabaseFile {
    #[serde(default)]
    apps: Vec<AppImage>,
}

pub struct FilesystemAppImageRepository {
    paths: AppPaths,
    cache: RwLock<Option<Vec<AppImage>>>,
}

impl FilesystemAppImageRepository {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            cache: RwLock::new(None),
        }
    }

    fn read_from_disk(&self) -> Result<Vec<AppImage>, AppImageError> {
        if !self.paths.database_file.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&self.paths.database_file)
            .map_err(|e| AppImageError::StorageError(format!("Failed to read database: {}", e)))?;

        let db: DatabaseFile = toml::from_str(&content).map_err(|e| {
            AppImageError::Serialization(format!("Failed to parse database: {}", e))
        })?;

        // Filter out applications whose AppImage file has been deleted externally
        // and link icon path if present in icons directory
        let mut valid_apps = Vec::new();
        for mut app in db.apps {
            if app.path.exists() {
                if app.icon_path.as_ref().map_or(true, |p| !p.exists()) {
                    let potential_png = self.paths.icons_dir.join(format!("{}.png", app.id));
                    let potential_svg = self.paths.icons_dir.join(format!("{}.svg", app.id));
                    if potential_png.exists() {
                        app.icon_path = Some(potential_png);
                    } else if potential_svg.exists() {
                        app.icon_path = Some(potential_svg);
                    }
                }
                valid_apps.push(app);
            }
        }

        Ok(valid_apps)
    }

    fn write_to_disk(&self, apps: &[AppImage]) -> Result<(), AppImageError> {
        let db = DatabaseFile {
            apps: apps.to_vec(),
        };
        let content = toml::to_string_pretty(&db).map_err(|e| {
            AppImageError::Serialization(format!("Failed to serialize database: {}", e))
        })?;

        FileSystem::atomic_write(&self.paths.database_file, &content)
            .map_err(|e| AppImageError::StorageError(format!("Failed to write database: {}", e)))?;

        Ok(())
    }
}

impl AppImageRepository for FilesystemAppImageRepository {
    fn list(&self) -> Result<Vec<AppImage>, AppImageError> {
        let mut cache_guard = self.cache.write().unwrap();
        if let Some(ref cached) = *cache_guard {
            return Ok(cached.clone());
        }

        let apps = self.read_from_disk()?;
        *cache_guard = Some(apps.clone());
        Ok(apps)
    }

    fn get_by_id(&self, id: &str) -> Result<Option<AppImage>, AppImageError> {
        let apps = self.list()?;
        Ok(apps.into_iter().find(|a| a.id == id))
    }

    fn save(&self, app: &AppImage) -> Result<(), AppImageError> {
        let mut apps = self.list()?;
        if let Some(pos) = apps.iter().position(|a| a.id == app.id) {
            apps[pos] = app.clone();
        } else {
            apps.push(app.clone());
        }

        self.write_to_disk(&apps)?;
        *self.cache.write().unwrap() = Some(apps);
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<(), AppImageError> {
        let mut apps = self.list()?;
        let initial_len = apps.len();
        apps.retain(|a| a.id != id);

        if apps.len() != initial_len {
            self.write_to_disk(&apps)?;
            *self.cache.write().unwrap() = Some(apps);
        }
        Ok(())
    }
}
