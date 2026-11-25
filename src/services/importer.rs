use crate::config::paths::AppPaths;
use crate::config::settings::{AppSettings, RuntimeMethodPreference};
use crate::domain::appimage::{AppImage, AppImageError, RuntimeMethod};
use crate::infrastructure::filesystem::FileSystem;
use crate::services::desktop_entry::DesktopEntryService;
use crate::services::extractor::AppImageExtractor;
use crate::services::validator::AppImageValidator;
use std::path::{Path, PathBuf};

pub struct AppImageImporter {
    paths: AppPaths,
}

/// Removes control characters and limits the length of a display name taken from untrusted
/// AppImage metadata.
fn clean_display_name(raw: &str) -> Option<String> {
    let cleaned: String = raw.chars().filter(|c| !c.is_control()).take(100).collect();
    let cleaned = cleaned.trim().to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

impl AppImageImporter {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    fn id_is_free(&self, id: &str, existing: &[AppImage]) -> bool {
        !existing.iter().any(|a| a.id == id)
            && std::fs::symlink_metadata(self.paths.app_extracted_dir(id)).is_err()
            && ["png", "svg"]
                .iter()
                .all(|ext| std::fs::symlink_metadata(self.paths.app_icon_path(id, ext)).is_err())
    }

    fn unique_id(&self, base: &str, existing: &[AppImage]) -> String {
        if self.id_is_free(base, existing) {
            return base.to_string();
        }
        (2u32..)
            .map(|n| format!("{}-{}", base, n))
            .find(|candidate| self.id_is_free(candidate, existing))
            .expect("unbounded range always yields a free id")
    }

    fn find_duplicate<'a>(
        &self,
        source: &Path,
        copy_on_import: bool,
        existing: &'a [AppImage],
    ) -> Option<&'a AppImage> {
        let canonical = source.canonicalize().ok()?;
        existing.iter().find(|a| {
            a.path.canonicalize().is_ok_and(|p| p == canonical)
                || (copy_on_import
                    && a.managed
                    && a.path.starts_with(&self.paths.applications_dir)
                    && FileSystem::files_identical(&a.path, source))
        })
    }

    /// Imports an AppImage. A file that is already in the library is reported as
    /// `AlreadyManaged`; a different file that would collide in name or identifier is stored
    /// side by side under a new name. Nothing existing is overwritten.
    pub fn import(
        &self,
        source_path: &Path,
        settings: &AppSettings,
        existing: &[AppImage],
    ) -> Result<AppImage, AppImageError> {
        let validation = AppImageValidator::validate(source_path)?;

        if let Some(dup) = self.find_duplicate(source_path, settings.copy_on_import, existing) {
            return Err(AppImageError::AlreadyManaged(dup.name.clone()));
        }

        let filename = source_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("application.AppImage");
        let stem = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("app");

        // Metadata and icon are read into a private temporary directory first, because the
        // final identifier is not known until the display name is.
        let icon_staging = tempfile::tempdir()?;
        let (metadata, staged_icon) =
            AppImageExtractor::extract_metadata_and_icon(source_path, icon_staging.path(), "icon")
                .unwrap_or_default();

        let display_name = metadata
            .display_name
            .as_deref()
            .and_then(clean_display_name)
            .or_else(|| clean_display_name(stem))
            .unwrap_or_else(|| "AppImage".to_string());
        let app_id = self.unique_id(&AppImage::sanitize_id(&display_name), existing);

        let final_path: PathBuf = if settings.copy_on_import {
            FileSystem::copy_to_unique_file(source_path, &self.paths.applications_dir, filename)?
        } else {
            // No +x is added here: the launcher does that when the user launches the app, so
            // file managers keep opening the file with AppNest instead of executing it.
            std::path::absolute(source_path)?
        };

        let icon_path = staged_icon.and_then(|staged| {
            let ext = staged.extension()?.to_str()?;
            let dest = self.paths.app_icon_path(&app_id, ext);
            let bytes = std::fs::read(&staged).ok()?;
            FileSystem::atomic_write_bytes(&dest, &bytes, 0o644).ok()?;
            Some(dest)
        });

        let mut app = AppImage::new(&app_id, &display_name, final_path);
        app.appimage_type = validation.appimage_type;
        app.architecture = validation.architecture;
        app.file_size = validation.file_size;
        app.version = metadata.version.clone();
        app.icon_path = icon_path;
        app.runtime_method = match settings.default_runtime_method {
            RuntimeMethodPreference::Auto => RuntimeMethod::Auto,
            RuntimeMethodPreference::Extracted => RuntimeMethod::Extracted,
            RuntimeMethodPreference::Native => RuntimeMethod::Native,
            RuntimeMethodPreference::ExtractAndRun => RuntimeMethod::ExtractAndRun,
        };
        app.metadata = metadata;
        app.managed = settings.copy_on_import;

        if settings.auto_integrate_desktop {
            if let Ok(entry) =
                DesktopEntryService::create_entry(&app, &self.paths.desktop_applications_dir)
            {
                app.desktop_entry_path = Some(entry);
                app.desktop_integrated = true;
            }
        }
        if settings.auto_desktop_shortcut {
            app.desktop_shortcut = DesktopEntryService::create_user_desktop_shortcut(
                &app,
                self.paths.user_desktop_dir.as_deref(),
            )
            .is_some();
        }

        Ok(app)
    }

    /// Undoes the files created by `import` when the application could not be recorded.
    pub fn rollback(&self, app: &AppImage) {
        let _ = DesktopEntryService::remove_entry(app, &self.paths.desktop_applications_dir);
        DesktopEntryService::remove_user_desktop_shortcut(
            app,
            self.paths.user_desktop_dir.as_deref(),
        );
        if let Some(icon) = &app.icon_path {
            if FileSystem::is_inside(&self.paths.icons_dir, icon) {
                let _ = FileSystem::remove_file_if_exists(icon);
            }
        }
        if app.managed && FileSystem::is_inside(&self.paths.applications_dir, &app.path) {
            let _ = FileSystem::remove_file_if_exists(&app.path);
        }
    }
}
