mod common;

use appnest::domain::appimage::{AppImage, RuntimeMethod};
use appnest::domain::repository::AppImageRepository;
use appnest::infrastructure::filesystem_repository::FilesystemAppImageRepository;
use common::*;
use std::fs;
use std::sync::Arc;

fn app(env: &TestEnv, id: &str) -> AppImage {
    AppImage::new(
        id,
        id,
        simple_appimage(&env.root.path().join(format!("{}.AppImage", id))),
    )
}

#[test]
fn crud() {
    let env = TestEnv::new();
    let repo = FilesystemAppImageRepository::new(env.paths.clone());
    assert!(repo.list().unwrap().is_empty());

    let mut a = app(&env, "one");
    repo.save(&a).unwrap();
    assert_eq!(repo.get_by_id("one").unwrap().unwrap().name, "one");

    a.name = "Renamed".into();
    repo.save(&a).unwrap();
    let listed = repo.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Renamed");

    repo.remove("one").unwrap();
    assert!(repo.list().unwrap().is_empty());
    repo.remove("one").unwrap();
}

#[test]
fn concurrent_saves_are_not_lost() {
    let env = TestEnv::new();
    let repo = Arc::new(FilesystemAppImageRepository::new(env.paths.clone()));
    let apps: Vec<AppImage> = (0..24).map(|i| app(&env, &format!("app-{}", i))).collect();

    let handles: Vec<_> = apps
        .iter()
        .cloned()
        .map(|a| {
            let repo = Arc::clone(&repo);
            std::thread::spawn(move || repo.save(&a).unwrap())
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(repo.list().unwrap().len(), 24);
    // A fresh instance reads what is actually on disk
    let fresh = FilesystemAppImageRepository::new(env.paths.clone());
    assert_eq!(fresh.list().unwrap().len(), 24);
}

#[test]
fn two_repository_instances_do_not_overwrite_each_other() {
    let env = TestEnv::new();
    let r1 = FilesystemAppImageRepository::new(env.paths.clone());
    let r2 = FilesystemAppImageRepository::new(env.paths.clone());
    r1.save(&app(&env, "from-one")).unwrap();
    r2.save(&app(&env, "from-two")).unwrap();

    let fresh = FilesystemAppImageRepository::new(env.paths.clone());
    let ids: Vec<_> = fresh.list().unwrap().into_iter().map(|a| a.id).collect();
    assert!(ids.contains(&"from-one".to_string()) && ids.contains(&"from-two".to_string()));
}

#[test]
fn records_with_missing_files_are_kept() {
    let env = TestEnv::new();
    let repo = FilesystemAppImageRepository::new(env.paths.clone());
    let gone = app(&env, "gone");
    repo.save(&gone).unwrap();
    fs::remove_file(&gone.path).unwrap();

    // Saving another app must not drop the record whose file is missing
    repo.save(&app(&env, "other")).unwrap();
    let fresh = FilesystemAppImageRepository::new(env.paths.clone());
    let listed = fresh.list().unwrap();
    assert_eq!(listed.len(), 2);
    assert!(!listed.iter().find(|a| a.id == "gone").unwrap().exists());
}

#[test]
fn corrupt_database_is_kept_aside_and_the_backup_is_used() {
    let env = TestEnv::new();
    let repo = FilesystemAppImageRepository::new(env.paths.clone());
    repo.save(&app(&env, "first")).unwrap();
    repo.save(&app(&env, "second")).unwrap(); // second write creates the .bak of the first

    fs::write(&env.paths.database_file, "this = is [not valid toml").unwrap();

    let fresh = FilesystemAppImageRepository::new(env.paths.clone());
    let listed = fresh.list().unwrap();
    assert_eq!(listed.len(), 1, "recovered from the backup");

    let kept: Vec<_> = fs::read_dir(env.paths.database_file.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
        .collect();
    assert_eq!(kept.len(), 1, "the corrupt file must be preserved");

    fresh.save(&app(&env, "after")).unwrap();
    assert_eq!(fresh.list().unwrap().len(), 2);
}

#[test]
fn corrupt_database_without_backup_starts_empty_but_keeps_the_file() {
    let env = TestEnv::new();
    fs::write(&env.paths.database_file, "garbage ===").unwrap();
    let repo = FilesystemAppImageRepository::new(env.paths.clone());
    assert!(repo.list().unwrap().is_empty());
    let has_corrupt_copy = fs::read_dir(env.paths.database_file.parent().unwrap())
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
    assert!(has_corrupt_copy);
}

#[test]
fn database_is_private_and_written_atomically() {
    use std::os::unix::fs::PermissionsExt;
    let env = TestEnv::new();
    let repo = FilesystemAppImageRepository::new(env.paths.clone());
    repo.save(&app(&env, "p")).unwrap();
    let mode = fs::metadata(&env.paths.database_file)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    let leftovers = fs::read_dir(env.paths.database_file.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".appnest-tmp-"))
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn database_written_by_older_versions_still_loads() {
    let env = TestEnv::new();
    let p = simple_appimage(&env.root.path().join("old.AppImage"));
    let legacy = format!(
        "[[apps]]\nid = \"old\"\nname = \"Old\"\npath = {:?}\nruntime_method = \"fuse\"\nsha256 = \"abc\"\n",
        p.to_string_lossy()
    );
    fs::write(&env.paths.database_file, legacy).unwrap();
    let repo = FilesystemAppImageRepository::new(env.paths.clone());
    let a = repo.get_by_id("old").unwrap().unwrap();
    assert_eq!(a.runtime_method, RuntimeMethod::ExtractAndRun);
    assert!(
        a.cached_sha256().is_none(),
        "unstamped hashes are treated as stale"
    );
}
