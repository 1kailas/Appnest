use crate::config::paths::AppPaths;
use crate::config::settings::AppSettings;
use crate::domain::appimage::{AppImage, AppImageError, RuntimeMethod};
use crate::infrastructure::filesystem::FileSystem;
use crate::services::desktop_entry::DesktopEntryService;
use crate::services::extractor::AppImageExtractor;
use crate::services::validator::AppImageValidator;
use std::path::Path;

pub struct AppImageImporter {
    paths: AppPaths,
}

impl AppImageImporter {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn import(
        &self,
        source_path: &Path,
        settings: &AppSettings,
    ) -> Result<AppImage, AppImageError> {
        let validation = AppImageValidator::validate(source_path)?;

        let filename = source_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("application.AppImage");

        let stem = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("app");

        let mut app_id = AppImage::sanitize_id(stem);

        // 1. Determine destination path
        let final_path = if settings.copy_on_import {
            let dest = self.paths.applications_dir.join(filename);
            if source_path != dest {
                FileSystem::copy_file(source_path, &dest)?;
            }
            dest
        } else {
            FileSystem::make_executable(source_path)?;
            source_path.to_path_buf()
        };

        // 2. Extract metadata and icon
        let (metadata, mut icon_path) = AppImageExtractor::extract_metadata_and_icon(
            &final_path,
            &self.paths.icons_dir,
            &app_id,
        )
        .unwrap_or_default();

        let display_name = metadata
            .display_name
            .clone()
            .unwrap_or_else(|| stem.to_string());

        if let Some(ref display) = metadata.display_name {
            let clean = AppImage::sanitize_id(display);
            if !clean.is_empty() && clean != app_id {
                if let Some(ref old_icon) = icon_path {
                    if old_icon.exists() {
                        let ext = old_icon
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or("png");
                        let new_icon = self.paths.icons_dir.join(format!("{}.{}", clean, ext));
                        if std::fs::rename(old_icon, &new_icon).is_ok() {
                            icon_path = Some(new_icon);
                        }
                    }
                }
                app_id = clean;
            }
        }

        let runtime_method = match settings.default_runtime_method {
            crate::config::settings::RuntimeMethodPreference::Auto => RuntimeMethod::Auto,
            crate::config::settings::RuntimeMethodPreference::Extracted => RuntimeMethod::Extracted,
            crate::config::settings::RuntimeMethodPreference::Native => RuntimeMethod::Native,
            crate::config::settings::RuntimeMethodPreference::Fuse => RuntimeMethod::Fuse,
        };

        let mut app = AppImage::new(&app_id, &display_name, final_path);
        app.appimage_type = validation.appimage_type;
        app.architecture = validation.architecture;
        app.file_size = validation.file_size;
        app.version = metadata.version.clone();
        app.icon_path = icon_path;
        app.runtime_method = runtime_method;
        app.metadata = metadata;
        app.managed = settings.copy_on_import;

        // 3. Desktop integration
        if settings.auto_integrate_desktop {
            if let Ok(desktop_path) =
                DesktopEntryService::create_entry(&app, &self.paths.desktop_applications_dir)
            {
                app.desktop_entry_path = Some(desktop_path);
                app.desktop_integrated = true;
            }
        }

        Ok(app)
    }
}
