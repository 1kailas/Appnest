use crate::domain::appimage::AppImageError;
use crate::domain::repository::AppImageRepository;
use crate::services::launcher::AppImageLauncher;
use std::ffi::OsString;
use std::sync::Arc;

pub struct LaunchAppImageCommand {
    repository: Arc<dyn AppImageRepository>,
    launcher: Arc<AppImageLauncher>,
}

impl LaunchAppImageCommand {
    pub fn new(repository: Arc<dyn AppImageRepository>, launcher: Arc<AppImageLauncher>) -> Self {
        Self {
            repository,
            launcher,
        }
    }

    pub fn execute(&self, id: &str, args: &[OsString]) -> Result<u32, AppImageError> {
        let mut app = self
            .repository
            .get_by_id(id)?
            .ok_or_else(|| AppImageError::NotFound(std::path::PathBuf::from(id)))?;

        let old_extracted = app.extracted_dir.clone();
        let pid = self.launcher.launch(&mut app, args)?;

        // If extraction occurred during launch, save updated status to repository
        if app.extracted_dir != old_extracted {
            let _ = self.repository.save(&app);
        }

        Ok(pid)
    }
}
