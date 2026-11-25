mod common;

use appnest::config::paths::AppPaths;
use appnest::domain::appimage::{AppImage, RuntimeMethod};
use appnest::infrastructure::hash::sha256_file;
use common::*;
use std::fs;

#[test]
fn sanitize_id_is_filesystem_safe() {
    assert_eq!(AppImage::sanitize_id("My Cool_App.v2"), "my-cool-app-v2");
    assert_eq!(AppImage::sanitize_id("../../etc/passwd"), "etcpasswd");
    assert_eq!(AppImage::sanitize_id("名前"), "appimage-item");
    assert_eq!(AppImage::sanitize_id("---"), "appimage-item");
    let long = AppImage::sanitize_id(&"a".repeat(500));
    assert!(long.len() <= 64);
    assert!(long
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'));
}

#[test]
fn formatted_size() {
    let mut app = AppImage::new("a", "A", "/nonexistent".into());
    app.file_size = 512;
    assert_eq!(app.formatted_size(), "512 bytes");
    app.file_size = 2 * 1024 * 1024 * 1024;
    assert_eq!(app.formatted_size(), "2.00 GB");
}

#[test]
fn paths_are_built_from_the_given_roots() {
    let env = TestEnv::new();
    let p: &AppPaths = &env.paths;
    assert!(p.data_dir.starts_with(env.root.path()));
    assert!(p.desktop_applications_dir.starts_with(env.root.path()));
    assert!(p.app_icon_path("x", "png").ends_with("icons/x.png"));
    assert!(p.app_desktop_file("x").ends_with("appnest-x.desktop"));
    assert!(p.app_extracted_dir("x").ends_with("extracted/x"));
}

#[test]
fn sha256_is_computed_by_streaming() {
    let env = TestEnv::new();
    let p = env.root.path().join("hello");
    fs::write(&p, b"hello world\n").unwrap();
    assert_eq!(
        sha256_file(&p).unwrap(),
        "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447"
    );

    let big = env.root.path().join("big");
    fs::write(&big, vec![0u8; 3 * 1024 * 1024 + 5]).unwrap();
    assert_eq!(sha256_file(&big).unwrap().len(), 64);
}

#[test]
fn cached_hash_is_invalidated_when_file_changes() {
    let env = TestEnv::new();
    let p = env.root.path().join("app.bin");
    fs::write(&p, b"version one").unwrap();

    let mut app = AppImage::new("app", "App", p.clone());
    assert!(app.cached_sha256().is_none());

    app.set_sha256(sha256_file(&p).unwrap());
    assert!(app.cached_sha256().is_some());

    fs::write(&p, b"version two is longer").unwrap();
    assert!(
        app.cached_sha256().is_none(),
        "stale hash must not be reported"
    );
}

#[test]
fn legacy_fuse_runtime_value_still_loads() {
    #[derive(serde::Deserialize)]
    struct W {
        runtime_method: RuntimeMethod,
    }
    let old: W = toml::from_str("runtime_method = \"fuse\"").unwrap();
    assert_eq!(old.runtime_method, RuntimeMethod::ExtractAndRun);
    let new: W = toml::from_str("runtime_method = \"extractandrun\"").unwrap();
    assert_eq!(new.runtime_method, RuntimeMethod::ExtractAndRun);
}

#[test]
fn extract_and_run_is_described_as_fuse_free() {
    assert!(RuntimeMethod::ExtractAndRun.to_string().contains("no FUSE"));
}
