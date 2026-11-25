mod common;

use appnest::services::extractor::AppImageExtractor;
use common::*;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
use walkdir::WalkDir;

const BIG: u64 = 1 << 30;

#[test]
fn sanitize_removes_symlinks_that_leave_the_tree() {
    let env = TestEnv::new();
    let outside = env.dir("outside");
    fs::write(outside.join("secret"), "s3cret").unwrap();
    let root = env.dir("tree");
    fs::create_dir_all(root.join("usr/bin")).unwrap();
    fs::write(root.join("usr/bin/app"), "#!/bin/sh").unwrap();

    symlink("/etc/passwd", root.join("abs")).unwrap(); // absolute
    symlink("../outside/secret", root.join("up")).unwrap(); // climbs out
    symlink("../../../..", root.join("usr/bin/deep")).unwrap(); // climbs out from a subdir
    symlink(".", root.join("self")).unwrap(); // harmless on its own...
    symlink("self/..", root.join("chain")).unwrap(); // ...but this resolves above the root
    symlink("usr/bin/app", root.join("AppRun")).unwrap(); // legitimate relative link
    symlink("../app", root.join("usr/bin/ok")).unwrap(); // legitimate, inside

    AppImageExtractor::sanitize_tree(&root, BIG, 1000).unwrap();

    for gone in ["abs", "up", "chain"] {
        assert!(
            fs::symlink_metadata(root.join(gone)).is_err(),
            "{} should have been removed",
            gone
        );
    }
    assert!(fs::symlink_metadata(root.join("usr/bin/deep")).is_err());
    assert!(fs::symlink_metadata(root.join("AppRun")).is_ok());
    assert!(fs::symlink_metadata(root.join("usr/bin/ok")).is_ok());
    assert!(outside.join("secret").exists());
}

#[test]
fn sanitize_removes_special_files_and_setid_bits_and_enforces_limits() {
    let env = TestEnv::new();
    let root = env.dir("tree2");
    let fifo = root.join("pipe");
    assert!(std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let suid = root.join("suid");
    fs::write(&suid, "x").unwrap();
    fs::set_permissions(&suid, fs::Permissions::from_mode(0o6755)).unwrap();

    AppImageExtractor::sanitize_tree(&root, BIG, 1000).unwrap();
    assert!(fs::symlink_metadata(&fifo).is_err());
    assert_eq!(
        fs::metadata(&suid).unwrap().permissions().mode() & 0o7000,
        0
    );

    fs::write(root.join("a"), vec![0u8; 100]).unwrap();
    fs::write(root.join("b"), vec![0u8; 100]).unwrap();
    assert!(
        AppImageExtractor::sanitize_tree(&root, 150, 1000).is_err(),
        "size limit"
    );
    assert!(
        AppImageExtractor::sanitize_tree(&root, BIG, 2).is_err(),
        "entry limit"
    );
}

fn desktop_appdir(root: &Path) {
    fs::write(root.join("AppRun"), "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(root.join("AppRun"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        root.join("test.desktop"),
        "[Desktop Entry]\nName=Test App\nExec=test\nIcon=testicon\nCategories=Utility;\nX-AppImage-Version=9.9\n",
    )
    .unwrap();
}

#[test]
fn metadata_and_icon_are_read_without_running_the_appimage() {
    if !can_extract() {
        eprintln!("skipping: mksquashfs and 7z are required");
        return;
    }
    let env = TestEnv::new();
    let ai = appdir_appimage(&env.root.path().join("meta.AppImage"), |root| {
        desktop_appdir(root);
        fs::write(root.join("testicon.png"), PNG_1X1).unwrap();
    });
    let icons = env.dir("icons-out");
    let (meta, icon) = AppImageExtractor::extract_metadata_and_icon(&ai, &icons, "test").unwrap();

    assert_eq!(meta.display_name.as_deref(), Some("Test App"));
    assert_eq!(meta.version.as_deref(), Some("9.9"));
    let icon = icon.expect("icon");
    assert_eq!(icon, icons.join("test.png"));
    assert_eq!(fs::read(icon).unwrap(), PNG_1X1);
}

#[test]
fn dir_icon_symlink_cannot_be_used_to_copy_arbitrary_files() {
    if !can_extract() {
        eprintln!("skipping: mksquashfs and 7z are required");
        return;
    }
    let env = TestEnv::new();
    let secret = env.root.path().join("secret.txt");
    fs::write(&secret, "do not copy me").unwrap();
    let secret_path = secret.clone();

    let ai = appdir_appimage(&env.root.path().join("evil.AppImage"), move |root| {
        desktop_appdir(root);
        symlink(&secret_path, root.join(".DirIcon")).unwrap();
        symlink("../../../etc/passwd", root.join("testicon.png")).unwrap();
    });
    let icons = env.dir("icons-out");
    let (_, icon) = AppImageExtractor::extract_metadata_and_icon(&ai, &icons, "evil").unwrap();

    assert!(
        icon.is_none(),
        "no icon may come from a link that leaves the AppImage"
    );
    assert_eq!(fs::read_dir(&icons).unwrap().count(), 0);
}

#[test]
fn files_that_are_not_images_are_never_used_as_icons() {
    if !can_extract() {
        eprintln!("skipping: mksquashfs and 7z are required");
        return;
    }
    let env = TestEnv::new();
    let ai = appdir_appimage(&env.root.path().join("bad-icon.AppImage"), |root| {
        desktop_appdir(root);
        fs::write(root.join("testicon.png"), "#!/bin/sh\necho not a png").unwrap();
        fs::write(root.join(".DirIcon"), "also not an image").unwrap();
    });
    let icons = env.dir("icons-out");
    let (_, icon) = AppImageExtractor::extract_metadata_and_icon(&ai, &icons, "x").unwrap();
    assert!(icon.is_none());
}

#[test]
fn full_extraction_is_checked_and_installed_atomically() {
    if !can_extract() {
        eprintln!("skipping: mksquashfs and 7z are required");
        return;
    }
    let env = TestEnv::new();
    let ai = appdir_appimage(&env.root.path().join("full.AppImage"), |root| {
        desktop_appdir(root);
        fs::create_dir_all(root.join("usr/bin")).unwrap();
        symlink("/etc", root.join("etc-link")).unwrap();
        symlink("../../..", root.join("usr/bin/up")).unwrap();
        symlink("usr", root.join("good-link")).unwrap();
    });

    let target = env.paths.app_extracted_dir("full");
    AppImageExtractor::extract_appdir(&ai, &target, false).unwrap();

    assert!(target.join("AppRun").is_file());
    assert!(
        fs::metadata(target.join("AppRun"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111
            != 0
    );
    let canon = fs::canonicalize(&target).unwrap();
    for e in WalkDir::new(&target)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if e.file_type().is_symlink() {
            if let Ok(real) = fs::canonicalize(e.path()) {
                assert!(real.starts_with(&canon), "{} escapes", e.path().display());
            }
        }
    }
    assert!(fs::symlink_metadata(target.join("good-link")).is_ok());

    let leftovers: Vec<_> = fs::read_dir(&env.paths.extracted_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        leftovers,
        vec!["full"],
        "staging directories must be cleaned up"
    );

    // Re-extraction replaces the previous result without leaving anything behind
    fs::write(target.join("marker"), "old").unwrap();
    AppImageExtractor::extract_appdir(&ai, &target, false).unwrap();
    assert!(!target.join("marker").exists());
    assert_eq!(fs::read_dir(&env.paths.extracted_dir).unwrap().count(), 1);
}

#[test]
fn incomplete_extraction_is_never_installed() {
    if !can_extract() {
        eprintln!("skipping: mksquashfs and 7z are required");
        return;
    }
    let env = TestEnv::new();
    let ai = appdir_appimage(&env.root.path().join("no-apprun.AppImage"), |root| {
        fs::write(root.join("something"), "x").unwrap();
    });
    let target = env.paths.app_extracted_dir("broken");
    assert!(AppImageExtractor::extract_appdir(&ai, &target, false).is_err());
    assert!(!target.exists());
    assert_eq!(fs::read_dir(&env.paths.extracted_dir).unwrap().count(), 0);
}

#[test]
fn apprun_symlink_must_resolve_inside_the_appdir() {
    if !can_extract() {
        eprintln!("skipping: mksquashfs and 7z are required");
        return;
    }
    let env = TestEnv::new();
    let ai = appdir_appimage(&env.root.path().join("outside-apprun.AppImage"), |root| {
        symlink("/bin/sh", root.join("AppRun")).unwrap();
    });
    let target = env.paths.app_extracted_dir("outside");
    assert!(AppImageExtractor::extract_appdir(&ai, &target, false).is_err());
    assert!(!target.exists());
}

#[test]
fn an_invalid_appimage_is_refused_before_anything_is_extracted() {
    let env = TestEnv::new();
    let fake = env.root.path().join("fake.AppImage");
    fs::copy("/bin/sh", &fake).unwrap();
    let target = env.paths.app_extracted_dir("fake");
    assert!(AppImageExtractor::extract_appdir(&fake, &target, true).is_err());
    assert!(!target.exists());
}
