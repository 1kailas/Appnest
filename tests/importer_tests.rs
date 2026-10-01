use appnest::config::paths::AppPaths;
use appnest::domain::appimage::{AppImage, AppImageType, Architecture, RuntimeMethod};
use appnest::domain::repository::AppImageRepository;
use appnest::infrastructure::filesystem_repository::FilesystemAppImageRepository;
use appnest::services::desktop_entry::DesktopEntryService;
use appnest::services::extractor::AppImageExtractor;
use appnest::services::scanner::AppImageScanner;
use std::fs::File;
use tempfile::tempdir;

#[test]
fn test_desktop_entry_creation_and_parsing() {
    let tmp = tempdir().unwrap();
    let app_path = tmp.path().join("sample.AppImage");
    File::create(&app_path).unwrap();

    let mut app = AppImage::new("sample-app", "Sample Application", app_path);
    app.version = Some("1.2.3".to_string());
    app.metadata.comment = Some("A cool sample app".to_string());
    app.metadata.categories = vec!["Utility".to_string(), "Development".to_string()];

    let desktop_dir = tmp.path().join("applications");
    std::fs::create_dir_all(&desktop_dir).unwrap();

    let desktop_file = DesktopEntryService::create_entry(&app, &desktop_dir).unwrap();
    assert!(desktop_file.exists());

    let parsed = AppImageExtractor::parse_desktop_file(&desktop_file);
    assert_eq!(parsed.display_name.as_deref(), Some("Sample Application"));
    assert_eq!(parsed.comment.as_deref(), Some("A cool sample app"));
    assert_eq!(parsed.version.as_deref(), Some("1.2.3"));
    assert_eq!(parsed.categories, vec!["Utility", "Development"]);

    DesktopEntryService::remove_entry(&app, &desktop_dir).unwrap();
    assert!(!desktop_file.exists());
}

#[test]
fn test_filesystem_repository_crud() {
    let tmp = tempdir().unwrap();
    let mut paths = AppPaths::new();
    paths.database_file = tmp.path().join("apps.toml");
    paths.applications_dir = tmp.path().join("applications");
    std::fs::create_dir_all(&paths.applications_dir).unwrap();

    let repo = FilesystemAppImageRepository::new(paths);

    // Initial list is empty
    let initial = repo.list().unwrap();
    assert_eq!(initial.len(), 0);

    // Create an app
    let app_file = tmp.path().join("foo.AppImage");
    File::create(&app_file).unwrap();

    let mut app = AppImage::new("foo", "Foo App", app_file);
    app.version = Some("0.1.0".to_string());
    app.architecture = Architecture::X86_64;
    app.appimage_type = AppImageType::Type2;
    app.runtime_method = RuntimeMethod::Auto;

    repo.save(&app).unwrap();

    // Query back
    let fetched = repo.get_by_id("foo").unwrap().expect("App should exist");
    assert_eq!(fetched.name, "Foo App");
    assert_eq!(fetched.version.as_deref(), Some("0.1.0"));

    // Remove
    repo.remove("foo").unwrap();
    let after_remove = repo.get_by_id("foo").unwrap();
    assert!(after_remove.is_none());
}

#[test]
fn test_scanner_discovers_appimages() {
    let tmp = tempdir().unwrap();
    let app1 = tmp.path().join("One.AppImage");
    let app2 = tmp.path().join("two.appimage");
    let non_app = tmp.path().join("other.txt");

    File::create(&app1).unwrap();
    File::create(&app2).unwrap();
    File::create(&non_app).unwrap();

    let found = AppImageScanner::scan_directory(tmp.path(), 1);
    assert_eq!(found.len(), 2);
    assert!(found.contains(&app1));
    assert!(found.contains(&app2));
}

#[test]
fn test_obsidian_icon_extraction() {
    let app_paths = AppPaths::new();
    let obsidian = app_paths.applications_dir.join("Obsidian-1.13.7.AppImage");
    if obsidian.exists() {
        let tmp = tempfile::tempdir().unwrap();
        let icon = AppImageExtractor::extract_icon_only(
            &obsidian,
            tmp.path(),
            "obsidian",
            Some("obsidian"),
        );
        assert!(icon.is_some());
        assert!(icon.unwrap().exists());
    }
}

#[test]
fn test_appimage_import_and_desktop_integration() {
    let tmp = tempdir().unwrap();
    let mut paths = AppPaths::new();
    paths.database_file = tmp.path().join("apps.toml");
    paths.applications_dir = tmp.path().join("applications");
    paths.desktop_applications_dir = tmp.path().join("desktop_entries");
    paths.icons_dir = tmp.path().join("icons");
    let _ = paths.ensure_dirs();

    let repo = FilesystemAppImageRepository::new(paths.clone());

    // Create a mock executable AppImage
    let app_src = tmp.path().join("my-test-app.AppImage");
    {
        use std::io::Write;
        let mut f = File::create(&app_src).unwrap();
        let mut content = vec![0x7f, b'E', b'L', b'F', 2, 1, 1, 0];
        content.extend_from_slice(&[b'A', b'I', 0x02]);
        content.resize(128, 0);
        f.write_all(&content).unwrap();
    }

    let importer = appnest::services::importer::AppImageImporter::new(paths.clone());
    let mut settings = appnest::config::settings::AppSettings::default();
    settings.auto_integrate_desktop = true;
    settings.copy_on_import = false;

    let imported = importer.import(&app_src, &settings).unwrap();
    assert_eq!(imported.name, "my-test-app");
    assert!(imported.desktop_integrated);
    assert!(imported.desktop_entry_path.is_some());
    assert!(imported.desktop_entry_path.as_ref().unwrap().exists());

    // Save in repo
    repo.save(&imported).unwrap();
    let listed = repo.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, imported.id);
    assert!(listed[0].desktop_integrated);
}
