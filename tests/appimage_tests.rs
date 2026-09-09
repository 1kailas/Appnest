use appnest::config::paths::AppPaths;
use appnest::domain::appimage::{AppImage, AppImageType, Architecture, RuntimeMethod};
use std::path::PathBuf;

#[test]
fn test_appimage_creation_and_sanitize_id() {
    assert_eq!(AppImage::sanitize_id("Obsidian-1.13.7"), "obsidian-1-13-7");
    assert_eq!(
        AppImage::sanitize_id("My Great App! 2.0"),
        "my-great-app-2-0"
    );
    assert_eq!(AppImage::sanitize_id(""), "appimage-item");

    let app = AppImage::new("test-app", "Test App", PathBuf::from("/tmp/test.AppImage"));
    assert_eq!(app.id, "test-app");
    assert_eq!(app.name, "Test App");
    assert_eq!(app.runtime_method, RuntimeMethod::Auto);
    assert_eq!(app.architecture, Architecture::Unknown);
    assert_eq!(app.appimage_type, AppImageType::Unknown);
    assert!(!app.is_extracted());
}

#[test]
fn test_formatted_size() {
    let mut app = AppImage::new("test", "Test", PathBuf::from("/tmp/test"));
    app.file_size = 500;
    assert_eq!(app.formatted_size(), "500 bytes");

    app.file_size = 2048;
    assert_eq!(app.formatted_size(), "2 KB");

    app.file_size = 15 * 1024 * 1024;
    assert_eq!(app.formatted_size(), "15.0 MB");

    app.file_size = 2 * 1024 * 1024 * 1024;
    assert_eq!(app.formatted_size(), "2.00 GB");
}

#[test]
fn test_paths_generation() {
    let paths = AppPaths::new();
    let app_id = "sample-app";
    let icon = paths.app_icon_path(app_id, "png");
    assert!(icon.to_string_lossy().ends_with("sample-app.png"));

    let desktop = paths.app_desktop_file(app_id);
    assert!(desktop.to_string_lossy().ends_with("sample-app.desktop"));

    let extracted = paths.app_extracted_dir(app_id);
    assert!(extracted.to_string_lossy().ends_with("sample-app"));
}

#[test]
fn test_sha256_calculation() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(b"hello world\n").unwrap();
    tmp.flush().unwrap();

    let app = AppImage::new("hello-test", "Hello Test", tmp.path().to_path_buf());
    let hash = app
        .calculate_sha256()
        .expect("SHA-256 should compute successfully");
    assert_eq!(
        hash,
        "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447"
    );
}
