use crate::domain::appimage::AppImageError;
use crate::domain::repository::AppImageRepository;
use crate::services::launcher::AppImageLauncher;
use std::sync::Arc;

pub struct CloseAppImageCommand {
    repository: Arc<dyn AppImageRepository>,
    launcher: Arc<AppImageLauncher>,
}

impl CloseAppImageCommand {
    pub fn new(repository: Arc<dyn AppImageRepository>, launcher: Arc<AppImageLauncher>) -> Self {
        Self {
            repository,
            launcher,
        }
    }

    pub fn execute(&self, id: &str) -> Result<(), AppImageError> {
        let app = self
            .repository
            .get_by_id(id)?
            .ok_or_else(|| AppImageError::NotFound(std::path::PathBuf::from(id)))?;

        self.launcher.terminate(&app)
    }

    pub fn is_running(&self, id: &str) -> bool {
        if let Ok(Some(app)) = self.repository.get_by_id(id) {
            self.launcher.is_running(&app)
        } else {
            false
        }
    }
}
