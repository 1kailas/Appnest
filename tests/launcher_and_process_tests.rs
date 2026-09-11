use appnest::application::commands::close_appimage::CloseAppImageCommand;
use appnest::config::paths::AppPaths;
use appnest::domain::appimage::{AppImage, AppImageError};
use appnest::infrastructure::filesystem_repository::FilesystemAppImageRepository;
use appnest::infrastructure::process::ProcessLauncher;
use appnest::services::launcher::AppImageLauncher;
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn test_process_launcher_is_pid_alive() {
    let current_pid = std::process::id();
    assert!(ProcessLauncher::is_pid_alive(current_pid));

    // A very large PID unlikely to exist
    assert!(!ProcessLauncher::is_pid_alive(9_999_999));
}

#[test]
fn test_launcher_launch_nonexistent_returns_error() {
    let paths = AppPaths::new();
    let launcher = AppImageLauncher::new(paths);
    let mut app = AppImage::new(
        "nonexistent",
        "Nonexistent App",
        PathBuf::from("/tmp/nonexistent-file.AppImage"),
    );

    let res = launcher.launch(&mut app, &[]);
    assert!(matches!(res, Err(AppImageError::NotFound(_))));
}

#[test]
fn test_launcher_is_running_false_for_non_running_app() {
    let paths = AppPaths::new();
    let launcher = AppImageLauncher::new(paths);
    let app = AppImage::new(
        "not-running-app",
        "Not Running App",
        PathBuf::from("/tmp/definitely-not-running.AppImage"),
    );

    assert!(!launcher.is_running(&app));
}

#[test]
fn test_close_command_is_running_and_terminate() {
    let tmp = tempfile::tempdir().unwrap();
    let mut paths = AppPaths::new();
    paths.database_file = tmp.path().join("apps.toml");
    paths.applications_dir = tmp.path().join("applications");
    std::fs::create_dir_all(&paths.applications_dir).unwrap();

    let repo = Arc::new(FilesystemAppImageRepository::new(paths.clone()));
    let launcher = Arc::new(AppImageLauncher::new(paths));

    let close_cmd = CloseAppImageCommand::new(repo, launcher);

    // Should return false for unknown app id
    assert!(!close_cmd.is_running("random-unknown-id"));

    // Execute terminate on unknown app id returns NotFound
    let res = close_cmd.execute("random-unknown-id");
    assert!(matches!(res, Err(AppImageError::NotFound(_))));
}
