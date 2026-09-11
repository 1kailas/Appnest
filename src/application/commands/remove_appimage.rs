use crate::config::paths::AppPaths;
use crate::domain::appimage::AppImageError;
use crate::domain::repository::AppImageRepository;
use crate::infrastructure::filesystem::FileSystem;
use crate::services::desktop_entry::DesktopEntryService;
use crate::services::launcher::AppImageLauncher;
use std::sync::Arc;

pub struct RemoveAppImageCommand {
    repository: Arc<dyn AppImageRepository>,
    paths: AppPaths,
    launcher: Arc<AppImageLauncher>,
}

impl RemoveAppImageCommand {
    pub fn new(
        repository: Arc<dyn AppImageRepository>,
        paths: AppPaths,
        launcher: Arc<AppImageLauncher>,
    ) -> Self {
        Self {
            repository,
            paths,
            launcher,
        }
    }

    pub fn execute(&self, id: &str, delete_file: bool) -> Result<(), AppImageError> {
        if let Some(app) = self.repository.get_by_id(id)? {
            // Terminate running instance if any
            let _ = self.launcher.terminate(&app);

            // Remove desktop integration
            let _ = DesktopEntryService::remove_entry(&app, &self.paths.desktop_applications_dir);


            // Remove extracted folder if exists
            let extracted = self.paths.app_extracted_dir(&app.id);
            let _ = FileSystem::remove_dir_all_if_exists(&extracted);

            // Remove icon if exists
            if let Some(ref icon) = app.icon_path {
                let _ = FileSystem::remove_file_if_exists(icon);
            }

            // Remove binary if managed and requested
            if delete_file && app.managed {
                let _ = FileSystem::remove_file_if_exists(&app.path);
            }

            // Remove from database
            self.repository.remove(id)?;
        }

        Ok(())
    }
}
