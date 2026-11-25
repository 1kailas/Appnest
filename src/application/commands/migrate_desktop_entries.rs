use crate::config::paths::AppPaths;
use crate::domain::repository::AppImageRepository;
use crate::services::desktop_entry::DesktopEntryService;
use std::sync::Arc;

/// Moves menu entries written by earlier versions (`<id>.desktop`) to the namespaced file
/// name (`appnest-<id>.desktop`). Only files that AppNest itself wrote are touched.
pub struct MigrateDesktopEntriesCommand {
    repository: Arc<dyn AppImageRepository>,
    paths: AppPaths,
}

impl MigrateDesktopEntriesCommand {
    pub fn new(repository: Arc<dyn AppImageRepository>, paths: AppPaths) -> Self {
        Self { repository, paths }
    }

    pub fn execute(&self) {
        let Ok(apps) = self.repository.list() else {
            return;
        };
        for mut app in apps.into_iter().filter(|a| a.desktop_integrated) {
            if let Some(entry) = DesktopEntryService::migrate_legacy_entry(
                &app,
                &self.paths.desktop_applications_dir,
            ) {
                app.desktop_entry_path = Some(entry);
                let _ = self.repository.save(&app);
            }
        }
    }
}
