use crate::config::paths::AppPaths;
use crate::domain::appimage::{AppImage, AppImageError};
use crate::domain::repository::AppImageRepository;
use crate::infrastructure::filesystem::FileSystem;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

#[derive(Debug, Serialize, Deserialize, Default)]
struct DatabaseFile {
    #[serde(default)]
    apps: Vec<AppImage>,
}

/// Exclusive advisory lock on a sidecar file, held for the duration of a transaction.
struct FileLock(File);

impl FileLock {
    fn acquire(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        // SAFETY: flock only operates on the valid file descriptor owned by `file`.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self(file))
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // SAFETY: see `acquire`.
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

pub struct FilesystemAppImageRepository {
    paths: AppPaths,
    cache: RwLock<Option<Vec<AppImage>>>,
    write_lock: Mutex<()>,
}

impl FilesystemAppImageRepository {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            cache: RwLock::new(None),
            write_lock: Mutex::new(()),
        }
    }

    fn sidecar(&self, suffix: &str) -> PathBuf {
        let mut name = self
            .paths
            .database_file
            .file_name()
            .unwrap_or_default()
            .to_os_string();
        name.push(suffix);
        self.paths.database_file.with_file_name(name)
    }

    fn parse(content: &str) -> Option<Vec<AppImage>> {
        toml::from_str::<DatabaseFile>(content).ok().map(|d| d.apps)
    }

    /// Reads the database. A corrupt file is moved aside (never deleted) and the last good
    /// backup is used instead. Records whose AppImage file is missing are kept.
    fn read_from_disk(&self) -> Result<Vec<AppImage>, AppImageError> {
        let db_path = &self.paths.database_file;
        let content = match fs::read_to_string(db_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(AppImageError::StorageError(format!(
                    "Failed to read database: {}",
                    e
                )))
            }
        };

        let mut apps = match Self::parse(&content) {
            Some(apps) => apps,
            None => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let aside = self.sidecar(&format!(".corrupt-{}", ts));
                fs::rename(db_path, &aside).map_err(|e| {
                    AppImageError::Serialization(format!(
                        "Database is corrupt and could not be moved aside: {}",
                        e
                    ))
                })?;
                eprintln!(
                    "appnest: database was corrupt; kept a copy at {}",
                    aside.display()
                );
                let recovered = fs::read_to_string(self.sidecar(".bak"))
                    .ok()
                    .and_then(|c| Self::parse(&c))
                    .unwrap_or_default();
                if !recovered.is_empty() {
                    self.write_to_disk(&recovered)?;
                }
                recovered
            }
        };

        for app in &mut apps {
            if app.icon_path.as_ref().is_none_or(|p| !p.exists()) {
                for ext in ["png", "svg"] {
                    let candidate = self.paths.icons_dir.join(format!("{}.{}", app.id, ext));
                    if candidate.exists() {
                        app.icon_path = Some(candidate);
                        break;
                    }
                }
            }
        }
        Ok(apps)
    }

    fn write_to_disk(&self, apps: &[AppImage]) -> Result<(), AppImageError> {
        let content = toml::to_string_pretty(&DatabaseFile {
            apps: apps.to_vec(),
        })
        .map_err(|e| {
            AppImageError::Serialization(format!("Failed to serialize database: {}", e))
        })?;

        if let Ok(previous) = fs::read_to_string(&self.paths.database_file) {
            if Self::parse(&previous).is_some() {
                let _ = FileSystem::atomic_write_private(self.sidecar(".bak"), &previous);
            }
        }
        FileSystem::atomic_write_private(&self.paths.database_file, &content)
            .map_err(|e| AppImageError::StorageError(format!("Failed to write database: {}", e)))
    }

    /// Runs a read-modify-write cycle as one transaction: serialized in-process, locked
    /// against other processes, and always based on the current on-disk contents.
    fn transact<R>(
        &self,
        f: impl FnOnce(&mut Vec<AppImage>) -> (R, bool),
    ) -> Result<R, AppImageError> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let _flock = FileLock::acquire(&self.sidecar(".lock"))
            .map_err(|e| AppImageError::StorageError(format!("Failed to lock database: {}", e)))?;

        let mut apps = self.read_from_disk()?;
        let (result, changed) = f(&mut apps);
        if changed {
            self.write_to_disk(&apps)?;
            *self.cache.write().unwrap_or_else(|e| e.into_inner()) = Some(apps);
        }
        Ok(result)
    }
}

impl AppImageRepository for FilesystemAppImageRepository {
    fn list(&self) -> Result<Vec<AppImage>, AppImageError> {
        if let Some(cached) = self
            .cache
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            return Ok(cached.clone());
        }

        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let apps = self.read_from_disk()?;
        *self.cache.write().unwrap_or_else(|e| e.into_inner()) = Some(apps.clone());
        Ok(apps)
    }

    fn get_by_id(&self, id: &str) -> Result<Option<AppImage>, AppImageError> {
        Ok(self.list()?.into_iter().find(|a| a.id == id))
    }

    fn save(&self, app: &AppImage) -> Result<(), AppImageError> {
        self.transact(|apps| {
            match apps.iter().position(|a| a.id == app.id) {
                Some(pos) if apps[pos] == *app => return ((), false),
                Some(pos) => apps[pos] = app.clone(),
                None => apps.push(app.clone()),
            }
            ((), true)
        })
    }

    fn remove(&self, id: &str) -> Result<(), AppImageError> {
        self.transact(|apps| {
            let before = apps.len();
            apps.retain(|a| a.id != id);
            ((), apps.len() != before)
        })
    }
}
