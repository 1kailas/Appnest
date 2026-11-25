use crate::config::settings::AppSettings;
use crate::domain::appimage::{AppImage, AppImageError};
use crate::domain::repository::AppImageRepository;
use crate::services::importer::AppImageImporter;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct ImportAppImageCommand {
    repository: Arc<dyn AppImageRepository>,
    importer: Arc<AppImageImporter>,
    /// Imports run one at a time so that identifiers and file names are chosen against an
    /// up-to-date library.
    import_lock: Mutex<()>,
}

impl ImportAppImageCommand {
    pub fn new(repository: Arc<dyn AppImageRepository>, importer: Arc<AppImageImporter>) -> Self {
        Self {
            repository,
            importer,
            import_lock: Mutex::new(()),
        }
    }

    pub fn execute(
        &self,
        source_path: &Path,
        settings: &AppSettings,
    ) -> Result<AppImage, AppImageError> {
        let _guard = self.import_lock.lock().unwrap_or_else(|e| e.into_inner());
        let existing = self.repository.list()?;
        let app = self.importer.import(source_path, settings, &existing)?;
        if let Err(e) = self.repository.save(&app) {
            self.importer.rollback(&app);
            return Err(e);
        }
        Ok(app)
    }
}
