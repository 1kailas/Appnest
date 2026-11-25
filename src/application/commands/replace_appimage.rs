use crate::config::paths::AppPaths;
use crate::domain::appimage::{AppImage, AppImageError};
use crate::domain::repository::AppImageRepository;
use crate::infrastructure::filesystem::FileSystem;
use crate::services::desktop_entry::DesktopEntryService;
use crate::services::extractor::AppImageExtractor;
use crate::services::launcher::AppImageLauncher;
use crate::services::validator::AppImageValidator;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Replaces the AppImage file behind an existing application with a newer one (chosen by the
/// user or downloaded by the updater), keeping the application's identity, runtime choice and
/// integration settings.
///
/// The previous file is kept until the new record has been saved, so a failure at any point
/// leaves the application exactly as it was.
pub struct ReplaceAppImageCommand {
    repository: Arc<dyn AppImageRepository>,
    launcher: Arc<AppImageLauncher>,
    paths: AppPaths,
}

impl ReplaceAppImageCommand {
    pub fn new(
        repository: Arc<dyn AppImageRepository>,
        launcher: Arc<AppImageLauncher>,
        paths: AppPaths,
    ) -> Self {
        Self {
            repository,
            launcher,
            paths,
        }
    }

    pub fn execute(&self, id: &str, new_file: &Path) -> Result<AppImage, AppImageError> {
        let old = self
            .repository
            .get_by_id(id)?
            .ok_or_else(|| AppImageError::NotFound(PathBuf::from(id)))?;

        let validation = AppImageValidator::validate(new_file)?;

        if old.path.canonicalize().ok() == new_file.canonicalize().ok() {
            return Err(AppImageError::UpdateError(
                "That is the file the application already uses".into(),
            ));
        }
        if self
            .launcher
            .refresh_running(std::slice::from_ref(&old))
            .contains(&old.id)
        {
            return Err(AppImageError::UpdateError(
                "Close the application before replacing it".into(),
            ));
        }

        // Read the new file's metadata and icon into a private directory first
        let staging = tempfile::tempdir()?;
        let (metadata, staged_icon) =
            AppImageExtractor::extract_metadata_and_icon(new_file, staging.path(), "icon")
                .unwrap_or_default();

        let filename = new_file
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("application.AppImage");
        let (new_path, copied) = if old.managed {
            (
                FileSystem::copy_to_unique_file(new_file, &self.paths.applications_dir, filename)?,
                true,
            )
        } else {
            (std::path::absolute(new_file)?, false)
        };

        let mut updated = old.clone();
        updated.path = new_path.clone();
        updated.file_size = validation.file_size;
        updated.appimage_type = validation.appimage_type;
        updated.architecture = validation.architecture;
        updated.version = metadata.version.clone().or_else(|| old.version.clone());
        updated.metadata = metadata;
        updated.extracted_dir = None;
        updated.sha256 = None;
        updated.sha256_size = None;
        updated.sha256_mtime = None;

        let staged_icon_bytes = staged_icon.and_then(|p| {
            let ext = p.extension()?.to_str()?.to_string();
            Some((ext, std::fs::read(p).ok()?))
        });
        let new_icon = staged_icon_bytes
            .as_ref()
            .map(|(ext, _)| self.paths.app_icon_path(&old.id, ext));
        if let Some(icon) = &new_icon {
            updated.icon_path = Some(icon.clone());
        }

        if let Err(e) = self.repository.save(&updated) {
            if copied {
                let _ = FileSystem::remove_file_if_exists(&new_path);
            }
            return Err(e);
        }

        // From here on the replacement has happened; the rest is cleanup and refresh.
        if let (Some(icon), Some((_, bytes))) = (&new_icon, &staged_icon_bytes) {
            let _ = FileSystem::atomic_write_bytes(icon, bytes, 0o644);
            if let Some(previous) = &old.icon_path {
                if previous != icon && FileSystem::is_inside(&self.paths.icons_dir, previous) {
                    let _ = FileSystem::remove_file_if_exists(previous);
                }
            }
        }

        let extracted = self.paths.app_extracted_dir(&old.id);
        if FileSystem::is_inside(&self.paths.extracted_dir, &extracted) {
            let _ = FileSystem::remove_dir_all_if_exists(&extracted);
        }

        if updated.desktop_integrated {
            let _ =
                DesktopEntryService::create_entry(&updated, &self.paths.desktop_applications_dir);
        }
        if updated.desktop_shortcut {
            let _ = DesktopEntryService::create_user_desktop_shortcut(
                &updated,
                self.paths.user_desktop_dir.as_deref(),
            );
        }

        if old.managed
            && old.path != new_path
            && FileSystem::is_inside(&self.paths.applications_dir, &old.path)
        {
            let _ = FileSystem::remove_file_if_exists(&old.path);
        }
        Ok(updated)
    }
}
