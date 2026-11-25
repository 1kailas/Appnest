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

    /// Removes an application. The record is deleted first, so a failure leaves the files
    /// untouched rather than leaving a record that points at deleted files. Only files inside
    /// AppNest's own directories are ever deleted; a referenced (non-managed) AppImage is
    /// never deleted.
    pub fn execute(&self, id: &str, delete_file: bool) -> Result<(), AppImageError> {
        let Some(app) = self.repository.get_by_id(id)? else {
            return Ok(());
        };

        let _ = self.launcher.terminate(&app);
        self.repository.remove(id)?;

        let _ = DesktopEntryService::remove_entry(&app, &self.paths.desktop_applications_dir);
        DesktopEntryService::remove_user_desktop_shortcut(
            &app,
            self.paths.user_desktop_dir.as_deref(),
        );
        let extracted = self.paths.app_extracted_dir(&app.id);
        if FileSystem::is_inside(&self.paths.extracted_dir, &extracted) {
            let _ = FileSystem::remove_dir_all_if_exists(&extracted);
        }

        if let Some(icon) = &app.icon_path {
            if FileSystem::is_inside(&self.paths.icons_dir, icon) {
                let _ = FileSystem::remove_file_if_exists(icon);
            }
        }
        if delete_file
            && app.managed
            && FileSystem::is_inside(&self.paths.applications_dir, &app.path)
        {
            let _ = FileSystem::remove_file_if_exists(&app.path);
        }
        Ok(())
    }
}
