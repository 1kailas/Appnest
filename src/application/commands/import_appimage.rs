use crate::config::settings::AppSettings;
use crate::domain::appimage::{AppImage, AppImageError};
use crate::domain::repository::AppImageRepository;
use crate::services::importer::AppImageImporter;
use std::path::Path;
use std::sync::Arc;

pub struct ImportAppImageCommand {
    repository: Arc<dyn AppImageRepository>,
    importer: Arc<AppImageImporter>,
}

impl ImportAppImageCommand {
    pub fn new(repository: Arc<dyn AppImageRepository>, importer: Arc<AppImageImporter>) -> Self {
        Self {
            repository,
            importer,
        }
    }

    pub fn execute(
        &self,
        source_path: &Path,
        settings: &AppSettings,
    ) -> Result<AppImage, AppImageError> {
        let app = self.importer.import(source_path, settings)?;
        self.repository.save(&app)?;
        Ok(app)
    }
}
