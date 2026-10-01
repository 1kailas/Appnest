use appnest::application::commands::close_appimage::CloseAppImageCommand;
use appnest::config::paths::AppPaths;
use appnest::domain::appimage::{AppImage, AppImageError};
use appnest::domain::repository::AppImageRepository;
use appnest::infrastructure::filesystem_repository::FilesystemAppImageRepository;
use appnest::infrastructure::process::ProcessLauncher;
use appnest::services::launcher::AppImageLauncher;
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn test_process_launcher_is_pid_alive() {
    let current_pid = std::process::id();
    assert!(ProcessLauncher::is_pid_alive(current_pid));

    // PID 0 must never be considered alive
    assert!(!ProcessLauncher::is_pid_alive(0));

    // A very large PID unlikely to exist
    assert!(!ProcessLauncher::is_pid_alive(9_999_999));
}

#[test]
fn test_find_pids_does_not_match_subcommand_arguments() {
    use std::process::Command;
    let fake_app = PathBuf::from("/tmp/fake-test-target.AppImage");

    // Spawn a process that passes fake_app as an argument, NOT as the executable
    let mut child = Command::new("sleep")
        .arg("0.5")
        .spawn()
        .expect("Failed to spawn sleep");

    let pids = ProcessLauncher::find_pids_for_appimage(&fake_app, None);
    assert!(!pids.contains(&child.id()));

    let _ = child.kill();
    let _ = child.wait();
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

#[test]
fn test_launcher_track_and_terminate_process() {
    use std::fs::File;
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let mut paths = AppPaths::new();
    paths.database_file = tmp.path().join("apps.toml");
    paths.applications_dir = tmp.path().join("applications");
    std::fs::create_dir_all(&paths.applications_dir).unwrap();

    let script_path = tmp.path().join("mock-app.AppImage");
    {
        let mut f = File::create(&script_path).unwrap();
        writeln!(f, "#!/bin/sh\nsleep 30\n").unwrap();
        #[cfg(unix)]
        {
            let mut perms = f.metadata().unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms).unwrap();
        }
    }

    let repo = Arc::new(FilesystemAppImageRepository::new(paths.clone()));
    let launcher = Arc::new(AppImageLauncher::new(paths));

    let mut app = AppImage::new("mock-app", "Mock App", script_path);
    app.runtime_method = appnest::domain::appimage::RuntimeMethod::Native;
    repo.save(&app).unwrap();

    let close_cmd = CloseAppImageCommand::new(repo.clone(), launcher.clone());

    // Initially not running
    assert!(!close_cmd.is_running(&app.id));

    // Launch it natively
    let pid = launcher.launch(&mut app, &[]).unwrap();
    assert!(ProcessLauncher::is_pid_alive(pid));

    // Now is_running should be true
    assert!(close_cmd.is_running(&app.id));

    // Terminate it
    close_cmd.execute(&app.id).unwrap();

    // Now is_running should be false
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!close_cmd.is_running(&app.id));
}
