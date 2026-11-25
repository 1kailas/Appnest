mod common;

use appnest::domain::appimage::{AppImage, RuntimeMethod};
use appnest::services::desktop_entry::DesktopEntryService;
use common::*;
use std::fs;
use std::os::unix::fs::symlink;

fn app(env: &TestEnv, id: &str, name: &str) -> AppImage {
    let p = simple_appimage(&env.root.path().join(format!("{}.AppImage", id)));
    AppImage::new(id, name, p)
}

#[test]
fn entry_is_namespaced_and_roundtrips_through_the_parser() {
    let env = TestEnv::new();
    let mut a = app(&env, "sample", "Sample Application");
    a.version = Some("1.2.3".into());
    a.metadata.comment = Some("A cool sample".into());
    a.metadata.categories = vec!["Utility".into(), "Development".into()];

    let path = DesktopEntryService::create_entry(&a, &env.paths.desktop_applications_dir).unwrap();
    assert_eq!(path.file_name().unwrap(), "appnest-sample.desktop");

    let parsed = appnest::services::extractor::AppImageExtractor::parse_desktop_file(&path);
    assert_eq!(parsed.display_name.as_deref(), Some("Sample Application"));
    assert_eq!(parsed.comment.as_deref(), Some("A cool sample"));
    assert_eq!(parsed.categories, vec!["Utility", "Development"]);

    DesktopEntryService::remove_entry(&a, &env.paths.desktop_applications_dir).unwrap();
    assert!(!path.exists());
}

#[test]
fn newlines_in_metadata_cannot_inject_keys() {
    let env = TestEnv::new();
    let mut a = app(&env, "evil", "Evil\nExec=/bin/sh -c id\nHidden=true");
    a.metadata.comment = Some("hello\r\nExec=/bin/false".into());
    a.metadata.categories = vec!["Util;ity".into(), "X\nExec=evil".into()];
    a.metadata.keywords = vec!["a;b\nExec=evil".into()];
    a.metadata.mime_types = vec!["text/plain\nExec=evil".into()];
    a.version = Some("1\nExec=evil".into());

    let content = DesktopEntryService::generate_entry_content(&a);
    let execs: Vec<_> = content.lines().filter(|l| l.starts_with("Exec=")).collect();
    assert_eq!(
        execs.len(),
        1,
        "exactly one Exec line expected:\n{}",
        content
    );
    assert!(!content.lines().any(|l| l.starts_with("Hidden=")));
    assert!(content.lines().all(|l| l.is_empty()
        || l.starts_with('[')
        || l.contains('=') && !l.starts_with("Exec=evil") && !l.starts_with("Exec=/bin/false")));
    assert!(execs[0].starts_with("Exec=appnest launch "), "{}", execs[0]);
}

#[test]
fn exec_launches_through_appnest_with_a_quoted_id() {
    let env = TestEnv::new();
    let a = app(&env, "plain-id", "Plain");
    let content = DesktopEntryService::generate_entry_content(&a);
    assert!(
        content.contains("Exec=appnest launch \"plain-id\" -- %U\n"),
        "{}",
        content
    );

    // Even an id that should never occur is quoted, not interpreted
    let mut odd = app(&env, "x", "Odd");
    odd.id = "a\"b$c%d`e\\f".to_string();
    let content = DesktopEntryService::generate_entry_content(&odd);
    // Quoting (spec: Exec) first, then the string-value escaping that doubles backslashes
    assert!(
        content.contains(r#"Exec=appnest launch "a\\"b\\$c%%d\\`e\\\\f" -- %U"#),
        "{}",
        content
    );
}

#[test]
fn the_runtime_method_is_not_baked_into_the_entry() {
    let env = TestEnv::new();
    let mut a = app(&env, "ear", "Ear");
    a.runtime_method = RuntimeMethod::ExtractAndRun;
    let content = DesktopEntryService::generate_entry_content(&a);
    assert!(!content.contains("--appimage-extract-and-run"));
    assert!(content.contains("appnest launch"));
}

#[test]
fn no_sandbox_is_never_added_automatically() {
    let env = TestEnv::new();
    let mut a = app(&env, "electron", "Electron App");
    a.metadata.exec_args = Some("electron-app --no-sandbox %U".into());
    let content = DesktopEntryService::generate_entry_content(&a);
    assert!(!content.contains("no-sandbox"), "{}", content);
    assert!(!a.command_preview().contains("no-sandbox"));
}

#[test]
fn generated_entry_passes_desktop_file_validate() {
    if !have_tool("desktop-file-validate") {
        eprintln!("skipping: desktop-file-validate not installed");
        return;
    }
    let env = TestEnv::new();
    let mut a = app(&env, "valid", "Valid App");
    a.metadata.comment = Some("Comment with \\ backslash and ünïcode".into());
    a.metadata.categories = vec!["Utility".into()];
    a.metadata.mime_types = vec!["text/plain".into()];
    let path = DesktopEntryService::create_entry(&a, &env.paths.desktop_applications_dir).unwrap();
    let out = std::process::Command::new("desktop-file-validate")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn unsafe_ids_are_refused() {
    let env = TestEnv::new();
    for bad in ["../escape", "a/b", "", "UPPER", "x y"] {
        let a = app(&env, "ok", "Name");
        let mut a = a;
        a.id = bad.to_string();
        assert!(
            DesktopEntryService::create_entry(&a, &env.paths.desktop_applications_dir).is_err(),
            "{:?}",
            bad
        );
        assert!(DesktopEntryService::create_user_desktop_shortcut(
            &a,
            env.paths.user_desktop_dir.as_deref()
        )
        .is_none());
    }
}

#[test]
fn entry_never_overwrites_or_removes_an_unrelated_desktop_file() {
    let env = TestEnv::new();
    let dir = &env.paths.desktop_applications_dir;
    fs::create_dir_all(dir).unwrap();
    let foreign = dir.join("firefox.desktop");
    fs::write(&foreign, "[Desktop Entry]\nName=Real Firefox\n").unwrap();

    let a = app(&env, "firefox", "Firefox");
    DesktopEntryService::create_entry(&a, dir).unwrap();
    assert_eq!(
        fs::read_to_string(&foreign).unwrap(),
        "[Desktop Entry]\nName=Real Firefox\n"
    );

    DesktopEntryService::remove_entry(&a, dir).unwrap();
    assert!(foreign.exists(), "an unrelated file must survive removal");
}

#[test]
fn legacy_entries_are_migrated_only_when_they_are_ours() {
    let env = TestEnv::new();
    let dir = &env.paths.desktop_applications_dir;
    let a = app(&env, "legacy", "Legacy");

    let legacy = dir.join("legacy.desktop");
    fs::write(
        &legacy,
        "[Desktop Entry]\nName=Legacy\nX-AppImage-Manager-ID=legacy\n",
    )
    .unwrap();
    let new = DesktopEntryService::migrate_legacy_entry(&a, dir).unwrap();
    assert!(new.exists());
    assert!(!legacy.exists());

    let b = app(&env, "foreign", "Foreign");
    let theirs = dir.join("foreign.desktop");
    fs::write(&theirs, "[Desktop Entry]\nName=Someone else's\n").unwrap();
    assert!(DesktopEntryService::migrate_legacy_entry(&b, dir).is_none());
    assert!(theirs.exists());
}

#[test]
fn shortcut_name_cannot_escape_the_desktop_directory() {
    let env = TestEnv::new();
    let victim_dir = env.dir("victim");
    let a = app(&env, "traversal", "../victim/pwn");

    let shortcut = DesktopEntryService::create_user_desktop_shortcut(
        &a,
        env.paths.user_desktop_dir.as_deref(),
    )
    .unwrap();
    assert_eq!(
        shortcut.parent().unwrap(),
        env.paths.user_desktop_dir.as_deref().unwrap()
    );
    assert_eq!(shortcut.file_name().unwrap(), "appnest-traversal.desktop");
    assert_eq!(fs::read_dir(&victim_dir).unwrap().count(), 0);
}

#[test]
fn shortcut_does_not_follow_or_overwrite_foreign_files() {
    let env = TestEnv::new();
    let desktop = env.paths.user_desktop_dir.clone().unwrap();
    let a = app(&env, "target", "Target");

    // A regular file we did not create
    let dest = desktop.join("appnest-target.desktop");
    fs::write(&dest, "mine").unwrap();
    assert!(DesktopEntryService::create_user_desktop_shortcut(&a, Some(&desktop)).is_none());
    assert_eq!(fs::read_to_string(&dest).unwrap(), "mine");

    // A symlink pointing at a file somewhere else
    fs::remove_file(&dest).unwrap();
    let victim = env.root.path().join("victim.txt");
    fs::write(&victim, "precious").unwrap();
    symlink(&victim, &dest).unwrap();
    assert!(DesktopEntryService::create_user_desktop_shortcut(&a, Some(&desktop)).is_none());
    assert_eq!(fs::read_to_string(&victim).unwrap(), "precious");
}

#[test]
fn removing_shortcuts_only_removes_ours() {
    let env = TestEnv::new();
    let desktop = env.paths.user_desktop_dir.clone().unwrap();
    let a = app(&env, "cleanup", "Cleanup");

    let ours = DesktopEntryService::create_user_desktop_shortcut(&a, Some(&desktop)).unwrap();
    let unrelated = desktop.join("Cleanup.desktop");
    fs::write(&unrelated, "[Desktop Entry]\nName=Hand made\n").unwrap();

    DesktopEntryService::remove_user_desktop_shortcut(&a, Some(&desktop));
    assert!(!ours.exists());
    assert!(
        unrelated.exists(),
        "a hand-made file with the app's name must survive"
    );
}
