mod common;

use appnest::application::commands::CloseAppImageCommand;
use appnest::domain::appimage::{AppImage, AppImageError, RuntimeMethod};
use appnest::infrastructure::filesystem_repository::FilesystemAppImageRepository;
use appnest::infrastructure::process::{ProcId, ProcessLauncher, ScanTarget};
use appnest::services::launcher::AppImageLauncher;
use common::*;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn launcher(env: &TestEnv) -> AppImageLauncher {
    AppImageLauncher::new(env.paths.clone()).with_early_exit_window(Duration::from_millis(600))
}

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

fn wait_for(cond: impl Fn() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {}", what);
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn read_when_present(path: &Path) -> String {
    wait_for(|| path.exists(), &path.display().to_string());
    std::thread::sleep(Duration::from_millis(100));
    fs::read_to_string(path).unwrap()
}

fn plain_app(env: &TestEnv, id: &str, method: RuntimeMethod) -> AppImage {
    let mut app = AppImage::new(
        id,
        id,
        simple_appimage(&env.root.path().join(format!("{}.AppImage", id))),
    );
    app.runtime_method = method;
    app
}

#[test]
fn launching_a_missing_file_fails_cleanly() {
    let env = TestEnv::new();
    let mut app = AppImage::new("gone", "Gone", env.root.path().join("gone.AppImage"));
    assert!(matches!(
        launcher(&env).launch(&mut app, &[]),
        Err(AppImageError::NotFound(_))
    ));
}

#[test]
fn an_ordinary_binary_is_never_executed_as_an_appimage() {
    let env = TestEnv::new();
    let p = env.root.path().join("fake.AppImage");
    fs::copy("/usr/bin/touch", &p).unwrap();
    let marker = env.root.path().join("was-run");
    let mut app = AppImage::new("fake", "Fake", p);
    app.runtime_method = RuntimeMethod::Native;
    let res = launcher(&env).launch(&mut app, &args(&[marker.to_str().unwrap()]));
    assert!(matches!(res, Err(AppImageError::InvalidAppImage(_))));
    assert!(!marker.exists());
}

#[test]
fn started_process_is_tracked_terminated_and_reaped() {
    let env = TestEnv::new();
    let l = launcher(&env);
    let mut app = plain_app(&env, "tracked", RuntimeMethod::Native);

    assert!(!l.is_running(&app));
    let pid = l
        .launch(&mut app, &args(&["sh", "-c", "sleep 30; :"]))
        .unwrap();
    assert!(ProcessLauncher::is_pid_alive(pid));
    assert!(l.is_running(&app));
    assert!(l
        .refresh_running(std::slice::from_ref(&app))
        .contains("tracked"));

    l.terminate(&app).unwrap();
    assert!(!l.is_running(&app));
    // The child must be reaped, not left as a zombie
    wait_for(
        || !Path::new(&format!("/proc/{}", pid)).exists(),
        "the process to be reaped",
    );
}

#[test]
fn immediate_failure_is_reported_with_the_program_output() {
    let env = TestEnv::new();
    let l = launcher(&env);
    let mut app = plain_app(&env, "failing", RuntimeMethod::Native);
    let err = l
        .launch(&mut app, &args(&["sh", "-c", "echo boom >&2; exit 3"]))
        .unwrap_err()
        .to_string();
    assert!(err.contains("boom"), "{}", err);
    assert!(err.contains("exit code 3"), "{}", err);
    assert!(!l.is_running(&app));
}

#[test]
fn normal_early_exit_with_success_counts_as_started() {
    let env = TestEnv::new();
    let l = launcher(&env);
    let mut app = plain_app(&env, "quick", RuntimeMethod::Native);
    assert!(l.launch(&mut app, &args(&["sh", "-c", "exit 0"])).is_ok());
}

#[test]
fn application_failures_do_not_trigger_fallbacks_or_a_second_instance() {
    let env = TestEnv::new();
    let l = launcher(&env);
    let mut app = plain_app(&env, "crashy", RuntimeMethod::Auto);
    let marker = env.root.path().join("runs");

    let res = l.launch(
        &mut app,
        &args(&[
            "sh",
            "-c",
            "echo x >> \"$0\"; echo crashed >&2; exit 2",
            marker.to_str().unwrap(),
        ]),
    );
    let err = res.unwrap_err().to_string();
    assert!(err.contains("crashed"), "{}", err);
    assert_eq!(
        fs::read_to_string(&marker).unwrap().lines().count(),
        1,
        "started exactly once"
    );
    assert!(
        !env.paths.app_extracted_dir("crashy").exists(),
        "no extraction without need"
    );
}

#[test]
fn a_manually_chosen_method_is_never_replaced_by_a_fallback() {
    let env = TestEnv::new();
    let l = launcher(&env);
    let mut app = plain_app(&env, "manual", RuntimeMethod::Native);
    let res = l.launch(
        &mut app,
        &args(&["sh", "-c", "echo 'fuse: cannot mount' >&2; exit 1"]),
    );
    assert!(res.is_err());
    assert_eq!(app.runtime_method, RuntimeMethod::Native);
    assert!(!env.paths.app_extracted_dir("manual").exists());
}

const FUSE_FAILURE: [&str; 3] = ["sh", "-c", "echo 'fuse: failed to mount' >&2; exit 1"];

#[test]
fn auto_falls_back_to_the_extracted_apprun_with_a_proper_environment() {
    if !can_extract() {
        eprintln!("skipping: mksquashfs and 7z are required");
        return;
    }
    let env = TestEnv::new();
    let l = launcher(&env);
    let path = appdir_appimage(&env.root.path().join("fallback.AppImage"), |root| {
        script(
            &root.join("AppRun"),
            "echo \"$APPDIR|$APPIMAGE\" > \"$APPDIR/env.txt\"\nprintf '%s\\n' \"$@\" > \"$APPDIR/args.txt\"\nexec sleep 30",
        );
    });
    let mut app = AppImage::new("fallback", "Fallback", path.clone());

    // The failing first two methods receive the same arguments; the AppRun gets them too
    let mut a: Vec<&str> = FUSE_FAILURE.to_vec();
    a.push("user arg");
    l.launch(&mut app, &args(&a)).unwrap();

    let target = env.paths.app_extracted_dir("fallback");
    assert_eq!(app.extracted_dir.as_deref(), Some(target.as_path()));
    assert_eq!(
        app.runtime_method,
        RuntimeMethod::Auto,
        "the preference is preserved"
    );
    assert_eq!(
        read_when_present(&target.join("env.txt")).trim(),
        format!("{}|{}", target.display(), path.display())
    );
    assert!(read_when_present(&target.join("args.txt")).contains("user arg"));
    l.terminate(&app).unwrap();
}

#[test]
fn when_every_method_fails_the_error_explains_each_attempt() {
    if !can_extract() {
        eprintln!("skipping: mksquashfs and 7z are required");
        return;
    }
    let env = TestEnv::new();
    let l = launcher(&env);
    let path = appdir_appimage(&env.root.path().join("allfail.AppImage"), |root| {
        script(&root.join("AppRun"), "echo 'apprun broke' >&2\nexit 1");
    });
    let mut app = AppImage::new("allfail", "All fail", path);
    let err = l
        .launch(&mut app, &args(&FUSE_FAILURE))
        .unwrap_err()
        .to_string();
    assert!(err.contains("Every launch method failed"), "{}", err);
    assert!(err.contains("apprun broke"), "{}", err);
    assert!(
        err.contains("Direct execution") && err.contains("Extract and run"),
        "{}",
        err
    );
}

#[test]
fn a_replaced_appimage_invalidates_the_old_extraction() {
    if !can_extract() {
        eprintln!("skipping: mksquashfs and 7z are required");
        return;
    }
    let env = TestEnv::new();
    let l = launcher(&env);
    let path = env.root.path().join("replace.AppImage");
    let build = |text: &str| {
        let text = text.to_string();
        appdir_appimage(&path, move |root| {
            script(
                &root.join("AppRun"),
                &format!("echo {} > \"$APPDIR/which.txt\"\nexec sleep 30", text),
            );
        });
    };

    build("one");
    let mut app = AppImage::new("replace", "Replace", path.clone());
    app.runtime_method = RuntimeMethod::Extracted;
    l.launch(&mut app, &[]).unwrap();
    let target = env.paths.app_extracted_dir("replace");
    assert_eq!(read_when_present(&target.join("which.txt")).trim(), "one");
    l.terminate(&app).unwrap();

    build("two");
    let later = std::time::SystemTime::now() + Duration::from_secs(30);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(later)
        .unwrap();
    fs::remove_file(target.join("which.txt")).unwrap();
    l.launch(&mut app, &[]).unwrap();
    assert_eq!(read_when_present(&target.join("which.txt")).trim(), "two");
    l.terminate(&app).unwrap();
}

#[test]
fn a_recycled_pid_is_not_signalled() {
    let mut child = Command::new("sleep").arg("30").spawn().unwrap();
    let real = ProcId {
        pid: child.id(),
        start_time: ProcessLauncher::proc_start_time(child.id()).unwrap(),
    };
    let impostor = ProcId {
        start_time: real.start_time + 1,
        ..real
    };

    assert!(!ProcessLauncher::is_alive(impostor));
    ProcessLauncher::terminate(impostor, true).unwrap();
    assert!(
        child.try_wait().unwrap().is_none(),
        "wrong identity must not be killed"
    );

    assert!(ProcessLauncher::is_alive(real));
    ProcessLauncher::terminate(real, false).unwrap();
    child.wait().unwrap();
    assert!(!ProcessLauncher::is_alive(real));
}

#[test]
fn discovery_matches_by_executable_and_environment_but_not_by_arguments() {
    let env = TestEnv::new();
    let app_path = simple_appimage(&env.root.path().join("discover.AppImage"));
    let target = || ScanTarget {
        id: "discover".into(),
        app_path: app_path.clone(),
        apprun_path: None,
    };

    // Merely mentioning the path in the arguments is not a match
    let mut by_arg = Command::new("sleep")
        .arg("30")
        .env("X", "1")
        .spawn()
        .unwrap();
    let mut mention = Command::new("sh")
        .args(["-c", "sleep 30", app_path.to_str().unwrap()])
        .spawn()
        .unwrap();
    assert!(ProcessLauncher::find_running(&[target()]).is_empty());

    // The environment variable the AppImage runtime sets is a match
    let mut by_env = Command::new("sleep")
        .arg("30")
        .env("APPIMAGE", &app_path)
        .spawn()
        .unwrap();
    let found = ProcessLauncher::find_running(&[target()]);
    let pids: Vec<u32> = found["discover"].iter().map(|p| p.pid).collect();
    assert_eq!(pids, vec![by_env.id()]);

    for c in [&mut by_arg, &mut mention, &mut by_env] {
        let _ = c.kill();
        let _ = c.wait();
    }
}

#[test]
fn close_command_handles_unknown_applications() {
    let env = TestEnv::new();
    let repo = Arc::new(FilesystemAppImageRepository::new(env.paths.clone()));
    let close = CloseAppImageCommand::new(repo, Arc::new(launcher(&env)));
    assert!(!close.is_running("random-unknown-id"));
    assert!(matches!(
        close.execute("random-unknown-id"),
        Err(AppImageError::NotFound(_))
    ));
}
