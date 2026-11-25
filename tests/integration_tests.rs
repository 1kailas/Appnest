mod common;

use appnest::application::commands::{
    ImportAppImageCommand, MigrateDesktopEntriesCommand, UpdateAppImageCommand,
};
use appnest::config::settings::AppSettings;
use appnest::domain::repository::AppImageRepository;
use appnest::infrastructure::filesystem_repository::FilesystemAppImageRepository;
use appnest::services::desktop_entry::DesktopEntryService;
use appnest::services::importer::AppImageImporter;
use common::*;
use std::fs;
use std::sync::Arc;

fn setup() -> (
    TestEnv,
    Arc<FilesystemAppImageRepository>,
    ImportAppImageCommand,
    UpdateAppImageCommand,
) {
    let env = TestEnv::new();
    let repo = Arc::new(FilesystemAppImageRepository::new(env.paths.clone()));
    let import = ImportAppImageCommand::new(
        repo.clone(),
        Arc::new(AppImageImporter::new(env.paths.clone())),
    );
    let update = UpdateAppImageCommand::new(repo.clone(), env.paths.clone());
    (env, repo, import, update)
}

fn settings(menu: bool, shortcut: bool) -> AppSettings {
    AppSettings {
        copy_on_import: false,
        auto_integrate_desktop: menu,
        auto_desktop_shortcut: shortcut,
        ..Default::default()
    }
}

#[test]
fn menu_entry_and_shortcut_are_independent() {
    let (env, _repo, import, update) = setup();
    let src = simple_appimage(&env.root.path().join("indep.AppImage"));
    let desktop = env.paths.user_desktop_dir.clone().unwrap();
    let menu_file = env
        .paths
        .desktop_applications_dir
        .join("appnest-indep.desktop");
    let shortcut_file = desktop.join("appnest-indep.desktop");

    // Shortcut only
    let app = import.execute(&src, &settings(false, true)).unwrap();
    assert!(!app.desktop_integrated && app.desktop_shortcut);
    assert!(!menu_file.exists() && shortcut_file.exists());

    // Add the menu entry; the shortcut stays
    let app = update.set_desktop_integration("indep", true).unwrap();
    assert!(app.desktop_integrated && app.desktop_shortcut);
    assert!(menu_file.exists() && shortcut_file.exists());

    // Remove the shortcut; the menu entry stays
    let app = update.set_desktop_shortcut("indep", false).unwrap();
    assert!(app.desktop_integrated && !app.desktop_shortcut);
    assert!(menu_file.exists() && !shortcut_file.exists());

    // Remove the menu entry too, then bring back only the shortcut
    let app = update.set_desktop_integration("indep", false).unwrap();
    assert!(!app.desktop_integrated && !menu_file.exists());
    let app = update.set_desktop_shortcut("indep", true).unwrap();
    assert!(app.desktop_shortcut && shortcut_file.exists() && !menu_file.exists());
}

#[test]
fn shortcut_needs_a_desktop_folder() {
    let (env, _repo, import, _) = setup();
    let src = simple_appimage(&env.root.path().join("nodesk.AppImage"));
    import.execute(&src, &settings(false, false)).unwrap();

    let mut paths = env.paths.clone();
    paths.user_desktop_dir = None;
    let repo = Arc::new(FilesystemAppImageRepository::new(paths.clone()));
    let update = UpdateAppImageCommand::new(repo, paths);
    assert!(update.set_desktop_shortcut("nodesk", true).is_err());
}

#[test]
fn startup_migration_refreshes_outdated_entries_and_records_existing_shortcuts() {
    let (env, repo, import, _) = setup();
    let src = simple_appimage(&env.root.path().join("old.AppImage"));
    let app = import.execute(&src, &settings(true, true)).unwrap();
    let menu_file = app.desktop_entry_path.clone().unwrap();
    let shortcut_file = env
        .paths
        .user_desktop_dir
        .clone()
        .unwrap()
        .join("appnest-old.desktop");

    // Entries as an older version wrote them: direct Exec of the AppImage
    let outdated = format!(
        "[Desktop Entry]\nType=Application\nName=Old\nExec=\"{}\" %U\nX-AppImage-Manager-ID=old\n",
        app.path.display()
    );
    fs::write(&menu_file, &outdated).unwrap();
    fs::write(&shortcut_file, &outdated).unwrap();
    let mut stale = app.clone();
    stale.desktop_shortcut = false; // version 0.3 did not track it
    repo.save(&stale).unwrap();

    MigrateDesktopEntriesCommand::new(repo.clone(), env.paths.clone()).execute();

    assert!(DesktopEntryService::is_current(&app, &menu_file));
    assert!(DesktopEntryService::is_current(&app, &shortcut_file));
    assert!(fs::read_to_string(&menu_file)
        .unwrap()
        .contains("Exec=appnest launch"));
    assert!(repo.get_by_id("old").unwrap().unwrap().desktop_shortcut);
}

#[test]
fn migration_leaves_foreign_files_and_unintegrated_apps_alone() {
    let (env, repo, import, _) = setup();
    let src = simple_appimage(&env.root.path().join("plain.AppImage"));
    import.execute(&src, &settings(false, false)).unwrap();

    let desktop = env.paths.user_desktop_dir.clone().unwrap();
    let foreign = desktop.join("appnest-plain.desktop");
    fs::write(&foreign, "[Desktop Entry]\nName=Mine\n").unwrap();

    MigrateDesktopEntriesCommand::new(repo.clone(), env.paths.clone()).execute();
    assert_eq!(
        fs::read_to_string(&foreign).unwrap(),
        "[Desktop Entry]\nName=Mine\n"
    );
    assert!(!repo.get_by_id("plain").unwrap().unwrap().desktop_shortcut);
}

#[test]
fn a_second_process_sees_changes_made_by_the_first() {
    // The CLI and the window are separate processes sharing one database file.
    let (env, repo, import, _) = setup();
    assert!(repo.list().unwrap().is_empty());

    let other = FilesystemAppImageRepository::new(env.paths.clone());
    let other_import = ImportAppImageCommand::new(
        Arc::new(other),
        Arc::new(AppImageImporter::new(env.paths.clone())),
    );
    let src = simple_appimage(&env.root.path().join("elsewhere.AppImage"));
    other_import.execute(&src, &settings(false, false)).unwrap();

    assert_eq!(
        repo.list().unwrap().len(),
        1,
        "the cached listing must notice the new record"
    );
    let src2 = appimage_with_marker(&env.root.path().join("again.AppImage"), "x");
    import.execute(&src2, &settings(false, false)).unwrap();
    assert_eq!(repo.list().unwrap().len(), 2);
}
