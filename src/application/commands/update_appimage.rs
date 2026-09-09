use crate::config::paths::AppPaths;
use crate::domain::appimage::{AppImage, AppImageError, RuntimeMethod};
use crate::domain::repository::AppImageRepository;
use crate::services::desktop_entry::DesktopEntryService;
use crate::services::extractor::AppImageExtractor;
use std::sync::Arc;

pub struct UpdateAppImageCommand {
    repository: Arc<dyn AppImageRepository>,
    paths: AppPaths,
}

impl UpdateAppImageCommand {
    pub fn new(repository: Arc<dyn AppImageRepository>, paths: AppPaths) -> Self {
        Self { repository, paths }
    }

    pub fn set_runtime_method(
        &self,
        id: &str,
        method: RuntimeMethod,
    ) -> Result<AppImage, AppImageError> {
        let mut app = self
            .repository
            .get_by_id(id)?
            .ok_or_else(|| AppImageError::NotFound(std::path::PathBuf::from(id)))?;

        app.runtime_method = method;

        // If switched to Extracted, ensure extracted folder exists
        if method == RuntimeMethod::Extracted && !app.is_extracted() {
            let target = self.paths.app_extracted_dir(&app.id);
            AppImageExtractor::extract_appdir(&app.path, &target)?;
            app.extracted_dir = Some(target);
        }

        // Re-generate desktop file with updated execution target
        if app.desktop_integrated {
            let desktop_path =
                DesktopEntryService::create_entry(&app, &self.paths.desktop_applications_dir)?;
            app.desktop_entry_path = Some(desktop_path);
        }

        self.repository.save(&app)?;
        Ok(app)
    }

    pub fn set_desktop_integration(
        &self,
        id: &str,
        enable: bool,
    ) -> Result<AppImage, AppImageError> {
        let mut app = self
            .repository
            .get_by_id(id)?
            .ok_or_else(|| AppImageError::NotFound(std::path::PathBuf::from(id)))?;

        if enable {
            let desktop_path =
                DesktopEntryService::create_entry(&app, &self.paths.desktop_applications_dir)?;
            app.desktop_entry_path = Some(desktop_path);
            app.desktop_integrated = true;
        } else {
            DesktopEntryService::remove_entry(&app, &self.paths.desktop_applications_dir)?;
            app.desktop_entry_path = None;
            app.desktop_integrated = false;
        }

        self.repository.save(&app)?;
        Ok(app)
    }

    pub fn extract_appdir(&self, id: &str) -> Result<AppImage, AppImageError> {
        let mut app = self
            .repository
            .get_by_id(id)?
            .ok_or_else(|| AppImageError::NotFound(std::path::PathBuf::from(id)))?;

        let target = self.paths.app_extracted_dir(&app.id);
        AppImageExtractor::extract_appdir(&app.path, &target)?;
        app.extracted_dir = Some(target);

        if app.desktop_integrated {
            let desktop_path =
                DesktopEntryService::create_entry(&app, &self.paths.desktop_applications_dir)?;
            app.desktop_entry_path = Some(desktop_path);
        }

        self.repository.save(&app)?;
        Ok(app)
    }
}
