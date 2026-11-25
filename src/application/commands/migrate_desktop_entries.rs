use crate::config::paths::AppPaths;
use crate::domain::repository::AppImageRepository;
use crate::services::desktop_entry::DesktopEntryService;
use std::sync::Arc;

/// Brings desktop files written by earlier versions up to date, touching only files that
/// AppNest itself wrote:
/// - menu entries named `<id>.desktop` move to `appnest-<id>.desktop`;
/// - entries and shortcuts are rewritten when their content is outdated (for example, to
///   launch through `appnest launch`);
/// - a desktop shortcut that exists is recorded as such, since menu entry and shortcut are
///   now tracked separately.
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
        let menu_dir = &self.paths.desktop_applications_dir;

        for mut app in apps {
            let mut changed = false;

            if app.desktop_integrated {
                if let Some(entry) = DesktopEntryService::migrate_legacy_entry(&app, menu_dir) {
                    app.desktop_entry_path = Some(entry);
                    changed = true;
                } else if let Ok(path) = DesktopEntryService::entry_path(&app, menu_dir) {
                    if path.exists() && !DesktopEntryService::is_current(&app, &path) {
                        let _ = DesktopEntryService::create_entry(&app, menu_dir);
                    }
                }
            }

            if let Some(desktop) = self.paths.user_desktop_dir.as_deref() {
                let has_new = DesktopEntryService::has_shortcut(&app, desktop);
                if has_new || DesktopEntryService::has_legacy_shortcut(&app, desktop) {
                    let current = DesktopEntryService::entry_path(&app, desktop)
                        .is_ok_and(|p| DesktopEntryService::is_current(&app, &p));
                    if !has_new || !current {
                        let _ =
                            DesktopEntryService::create_user_desktop_shortcut(&app, Some(desktop));
                    }
                    if !app.desktop_shortcut {
                        app.desktop_shortcut = true;
                        changed = true;
                    }
                }
            }

            if changed {
                let _ = self.repository.save(&app);
            }
        }
    }
}
