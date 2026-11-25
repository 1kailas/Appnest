mod common;

use appnest::application::commands::{
    ImportAppImageCommand, MigrateDesktopEntriesCommand, RemoveAppImageCommand,
};
use appnest::config::settings::AppSettings;
use appnest::domain::appimage::{AppImage, AppImageError};
use appnest::domain::repository::AppImageRepository;
use appnest::infrastructure::filesystem_repository::FilesystemAppImageRepository;
use appnest::services::importer::AppImageImporter;
use appnest::services::launcher::AppImageLauncher;
use appnest::services::scanner::AppImageScanner;
use common::*;
use std::fs;
use std::sync::Arc;

struct Fixture {
    env: TestEnv,
    repo: Arc<FilesystemAppImageRepository>,
    cmd: Arc<ImportAppImageCommand>,
}

fn fixture() -> Fixture {
    let env = TestEnv::new();
    let repo = Arc::new(FilesystemAppImageRepository::new(env.paths.clone()));
    let importer = Arc::new(AppImageImporter::new(env.paths.clone()));
    let cmd = Arc::new(ImportAppImageCommand::new(repo.clone(), importer));
    Fixture { env, repo, cmd }
}

fn settings(copy: bool, integrate: bool) -> AppSettings {
    AppSettings {
        copy_on_import: copy,
        auto_integrate_desktop: integrate,
        ..Default::default()
    }
}

#[test]
fn import_by_reference_with_desktop_integration() {
    let f = fixture();
    let src = simple_appimage(&f.env.root.path().join("my-test-app.AppImage"));

    let app = f.cmd.execute(&src, &settings(false, true)).unwrap();
    assert_eq!(app.id, "my-test-app");
    assert!(!app.managed);
    assert_eq!(app.path, src);
    assert!(app.desktop_integrated);
    assert!(app.desktop_entry_path.as_ref().unwrap().exists());
    assert!(f
        .env
        .paths
        .user_desktop_dir
        .as_ref()
        .unwrap()
        .join("appnest-my-test-app.desktop")
        .exists());
    assert_eq!(f.repo.list().unwrap().len(), 1);
}

#[test]
fn import_with_copy_keeps_the_original() {
    let f = fixture();
    let src = simple_appimage(&f.env.root.path().join("copied.AppImage"));
    let app = f.cmd.execute(&src, &settings(true, false)).unwrap();
    assert!(app.managed);
    assert!(app.path.starts_with(&f.env.paths.applications_dir));
    assert!(src.exists(), "the user's original must be kept");
    assert!(!app.desktop_integrated);
}

#[test]
fn renamed_elf_is_not_imported() {
    let f = fixture();
    let src = f.env.root.path().join("not-really.AppImage");
    fs::copy("/bin/sh", &src).unwrap();
    let err = f.cmd.execute(&src, &settings(true, true)).unwrap_err();
    assert!(matches!(err, AppImageError::InvalidAppImage(_)), "{}", err);
    assert!(f.repo.list().unwrap().is_empty());
    assert_eq!(
        fs::read_dir(&f.env.paths.applications_dir).unwrap().count(),
        0
    );
}

#[test]
fn importing_the_same_file_twice_is_reported() {
    let f = fixture();
    let src = simple_appimage(&f.env.root.path().join("once.AppImage"));
    f.cmd.execute(&src, &settings(false, false)).unwrap();
    let err = f.cmd.execute(&src, &settings(false, false)).unwrap_err();
    assert!(matches!(err, AppImageError::AlreadyManaged(_)), "{}", err);
    assert_eq!(f.repo.list().unwrap().len(), 1);
}

#[test]
fn importing_an_identical_copy_twice_is_reported() {
    let f = fixture();
    let a = simple_appimage(&f.env.dir("one").join("same.AppImage"));
    let b = simple_appimage(&f.env.dir("two").join("same.AppImage"));
    f.cmd.execute(&a, &settings(true, false)).unwrap();
    let err = f.cmd.execute(&b, &settings(true, false)).unwrap_err();
    assert!(matches!(err, AppImageError::AlreadyManaged(_)), "{}", err);
    assert_eq!(
        fs::read_dir(&f.env.paths.applications_dir).unwrap().count(),
        1
    );
}

#[test]
fn same_filename_different_content_is_stored_side_by_side() {
    let f = fixture();
    let a = appimage_with_marker(&f.env.dir("one").join("tool.AppImage"), "version-1");
    let b = appimage_with_marker(&f.env.dir("two").join("tool.AppImage"), "version-2");

    let first = f.cmd.execute(&a, &settings(true, false)).unwrap();
    let second = f.cmd.execute(&b, &settings(true, false)).unwrap();

    assert_ne!(first.id, second.id);
    assert_ne!(first.path, second.path);
    assert_eq!(fs::read(&first.path).unwrap(), fs::read(&a).unwrap());
    assert_eq!(fs::read(&second.path).unwrap(), fs::read(&b).unwrap());
    assert_eq!(f.repo.list().unwrap().len(), 2);
}

#[test]
fn concurrent_imports_get_unique_ids_and_none_are_lost() {
    let f = fixture();
    let sources: Vec<_> = (0..6)
        .map(|i| {
            appimage_with_marker(
                &f.env.dir(&format!("src{}", i)).join("same-name.AppImage"),
                &format!("content-{}", i),
            )
        })
        .collect();

    let handles: Vec<_> = sources
        .into_iter()
        .map(|src| {
            let cmd = Arc::clone(&f.cmd);
            std::thread::spawn(move || cmd.execute(&src, &settings(true, true)).unwrap())
        })
        .collect();
    let apps: Vec<AppImage> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    let mut ids: Vec<_> = apps.iter().map(|a| a.id.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 6);
    let fresh = FilesystemAppImageRepository::new(f.env.paths.clone());
    assert_eq!(fresh.list().unwrap().len(), 6);
    assert_eq!(
        fs::read_dir(&f.env.paths.applications_dir).unwrap().count(),
        6
    );
}

struct FailingRepo;
impl AppImageRepository for FailingRepo {
    fn list(&self) -> Result<Vec<AppImage>, AppImageError> {
        Ok(Vec::new())
    }
    fn get_by_id(&self, _: &str) -> Result<Option<AppImage>, AppImageError> {
        Ok(None)
    }
    fn save(&self, _: &AppImage) -> Result<(), AppImageError> {
        Err(AppImageError::StorageError("disk full".into()))
    }
    fn remove(&self, _: &str) -> Result<(), AppImageError> {
        Ok(())
    }
}

#[test]
fn failed_import_leaves_nothing_behind() {
    let env = TestEnv::new();
    let importer = Arc::new(AppImageImporter::new(env.paths.clone()));
    let cmd = ImportAppImageCommand::new(Arc::new(FailingRepo), importer);
    let src = simple_appimage(&env.root.path().join("doomed.AppImage"));

    assert!(cmd.execute(&src, &settings(true, true)).is_err());

    assert_eq!(
        fs::read_dir(&env.paths.applications_dir).unwrap().count(),
        0
    );
    assert_eq!(fs::read_dir(&env.paths.icons_dir).unwrap().count(), 0);
    let desktop_files = |dir: &std::path::Path| {
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "desktop"))
            .count()
    };
    assert_eq!(desktop_files(&env.paths.desktop_applications_dir), 0);
    assert_eq!(
        desktop_files(env.paths.user_desktop_dir.as_ref().unwrap()),
        0
    );
    assert!(src.exists());
}

#[test]
fn scanner_only_reports_real_appimages() {
    let env = TestEnv::new();
    let dir = env.dir("downloads");
    simple_appimage(&dir.join("real.AppImage"));
    fs::copy("/bin/sh", dir.join("fake.AppImage")).unwrap();
    fs::write(dir.join("notes.txt"), "hi").unwrap();

    let found = AppImageScanner::scan_directory(&dir, 2);
    assert_eq!(found, vec![dir.join("real.AppImage")]);
}

#[test]
fn removal_never_deletes_files_outside_appnests_directories() {
    let f = fixture();
    let launcher = Arc::new(AppImageLauncher::new(f.env.paths.clone()));
    let remove = RemoveAppImageCommand::new(f.repo.clone(), f.env.paths.clone(), launcher);

    let user_file = f.env.root.path().join("important.png");
    let user_appimage = simple_appimage(&f.env.root.path().join("keep-me.AppImage"));
    fs::write(&user_file, "precious").unwrap();

    let mut app = AppImage::new("tampered", "Tampered", user_appimage.clone());
    app.managed = true; // claims to be managed, but the file is outside the managed directory
    app.icon_path = Some(user_file.clone());
    f.repo.save(&app).unwrap();

    remove.execute("tampered", true).unwrap();
    assert!(f.repo.list().unwrap().is_empty());
    assert!(user_file.exists());
    assert!(user_appimage.exists());
}

#[test]
fn removal_deletes_managed_copy_but_not_a_referenced_original() {
    let f = fixture();
    let launcher = Arc::new(AppImageLauncher::new(f.env.paths.clone()));
    let remove = RemoveAppImageCommand::new(f.repo.clone(), f.env.paths.clone(), launcher);

    let referenced = simple_appimage(&f.env.root.path().join("ref.AppImage"));
    let r = f.cmd.execute(&referenced, &settings(false, true)).unwrap();
    let copied_src = appimage_with_marker(&f.env.root.path().join("cp.AppImage"), "x");
    let c = f.cmd.execute(&copied_src, &settings(true, true)).unwrap();

    remove.execute(&r.id, true).unwrap();
    remove.execute(&c.id, true).unwrap();

    assert!(
        referenced.exists(),
        "a referenced AppImage must never be deleted"
    );
    assert!(copied_src.exists());
    assert!(!c.path.exists(), "the managed copy is deleted");
    assert!(!r.desktop_entry_path.unwrap().exists());
    assert!(!c.desktop_entry_path.unwrap().exists());
}

#[test]
fn startup_migration_renames_legacy_menu_entries() {
    let f = fixture();
    let src = simple_appimage(&f.env.root.path().join("oldapp.AppImage"));
    let mut app = f.cmd.execute(&src, &settings(false, true)).unwrap();

    // Put the entry back where version 0.2 would have written it
    let new_entry = app.desktop_entry_path.clone().unwrap();
    let legacy = f.env.paths.desktop_applications_dir.join("oldapp.desktop");
    fs::rename(&new_entry, &legacy).unwrap();
    app.desktop_entry_path = Some(legacy.clone());
    f.repo.save(&app).unwrap();

    MigrateDesktopEntriesCommand::new(f.repo.clone(), f.env.paths.clone()).execute();

    assert!(!legacy.exists());
    assert!(new_entry.exists());
    assert_eq!(
        f.repo
            .get_by_id("oldapp")
            .unwrap()
            .unwrap()
            .desktop_entry_path,
        Some(new_entry)
    );
}
