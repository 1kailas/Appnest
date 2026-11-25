use appnest::config::paths::AppPaths;
use std::fs;

// Lives in its own test binary because it changes process-wide environment variables.
#[test]
fn locations_follow_xdg_settings() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let (data, config, cache) = (
        root.path().join("xdg-data"),
        root.path().join("xdg-config"),
        root.path().join("xdg-cache"),
    );
    fs::create_dir_all(home.join("Escritorio")).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("user-dirs.dirs"),
        "XDG_DESKTOP_DIR=\"$HOME/Escritorio\"\nXDG_DOWNLOAD_DIR=\"$HOME/Descargas\"\n",
    )
    .unwrap();
    fs::create_dir_all(home.join("Descargas")).unwrap();

    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_DATA_HOME", &data);
    std::env::set_var("XDG_CONFIG_HOME", &config);
    std::env::set_var("XDG_CACHE_HOME", &cache);

    let paths = AppPaths::try_new().expect("paths");
    assert_eq!(paths.data_dir, data.join("appnest"));
    assert_eq!(paths.config_dir, config.join("appnest"));
    assert_eq!(paths.cache_dir, cache.join("appnest"));
    assert_eq!(paths.desktop_applications_dir, data.join("applications"));
    assert_eq!(paths.user_desktop_dir, Some(home.join("Escritorio")));
    assert_eq!(paths.download_dir, Some(home.join("Descargas")));
    assert!(paths
        .default_scan_directories()
        .contains(&home.join("Descargas")));
    assert!(!paths
        .default_scan_directories()
        .iter()
        .any(|p| p.ends_with("Downloads")));
}
