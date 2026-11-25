use crate::config::paths::AppPaths;
use crate::domain::appimage::{AppImage, AppImageError, RuntimeMethod};
use crate::infrastructure::filesystem::FileSystem;
use crate::infrastructure::process::{ExitInfo, ProcId, ProcessLauncher, ScanTarget, SpawnOptions};
use crate::services::extractor::AppImageExtractor;
use crate::services::validator::AppImageValidator;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a freshly started process is watched for an immediate failure.
const EARLY_EXIT_WINDOW: Duration = Duration::from_millis(2500);
/// How long a terminated application is given to exit.
const TERMINATE_GRACE: Duration = Duration::from_secs(3);

pub struct AppImageLauncher {
    paths: AppPaths,
    running_apps: Mutex<HashMap<String, Vec<ProcId>>>,
    early_exit_window: Duration,
}

enum Attempt {
    Started(ProcId),
    Failed {
        exit: Option<ExitInfo>,
        message: String,
        /// The process could not be started at all (for example, "Exec format error").
        spawn_error: bool,
    },
}

/// True when output looks like the AppImage runtime failing to set itself up (FUSE missing,
/// mount or extraction problems) rather than the application itself failing.
fn is_runtime_failure(output: &str) -> bool {
    let out = output.to_lowercase();
    ["fuse", "squashfs", "extract", "mount"]
        .iter()
        .any(|needle| out.contains(needle))
}

impl AppImageLauncher {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            running_apps: Mutex::new(HashMap::new()),
            early_exit_window: EARLY_EXIT_WINDOW,
        }
    }

    pub fn with_early_exit_window(mut self, window: Duration) -> Self {
        self.early_exit_window = window;
        self
    }

    fn tracked(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<ProcId>>> {
        self.running_apps.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn working_dir() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
    }

    fn log_path(&self, app: &AppImage) -> PathBuf {
        self.paths.cache_dir.join(format!("launch-{}.log", app.id))
    }

    /// Starts the application using the selected runtime method. This waits a short time to
    /// detect an immediate failure, so it must not be called from the GUI thread.
    pub fn launch(
        &self,
        app: &mut AppImage,
        extra_args: &[OsString],
    ) -> Result<u32, AppImageError> {
        if !app.path.is_file() {
            return Err(AppImageError::NotFound(app.path.clone()));
        }
        AppImageValidator::validate(&app.path)?;
        let _ = std::fs::create_dir_all(&self.paths.cache_dir);

        let id = match app.runtime_method {
            RuntimeMethod::Extracted => {
                Self::expect_started("Extracted AppRun", self.run_extracted(app, extra_args)?)?
            }
            RuntimeMethod::Native => {
                Self::expect_started("Direct execution", self.run_native(app, extra_args)?)?
            }
            RuntimeMethod::ExtractAndRun => Self::expect_started(
                "Extract and run",
                self.run_extract_and_run(app, extra_args)?,
            )?,
            RuntimeMethod::Auto => self.launch_auto(app, extra_args)?,
        };
        Ok(id.pid)
    }

    fn expect_started(label: &str, attempt: Attempt) -> Result<ProcId, AppImageError> {
        match attempt {
            Attempt::Started(id) => Ok(id),
            Attempt::Failed { exit, message, .. } => Err(Self::failure(label, exit, &message)),
        }
    }

    fn failure(label: &str, exit: Option<ExitInfo>, message: &str) -> AppImageError {
        let status = exit.map_or_else(|| "could not start".to_string(), |e| e.to_string());
        let message = message.trim();
        AppImageError::LaunchError(if message.is_empty() {
            format!("{} failed ({})", label, status)
        } else {
            format!("{} failed ({}): {}", label, status, message)
        })
    }

    fn launch_auto(&self, app: &mut AppImage, args: &[OsString]) -> Result<ProcId, AppImageError> {
        if app.is_extracted()
            && !Self::extraction_is_stale(app, &self.paths.app_extracted_dir(&app.id))
        {
            return Self::expect_started("Extracted AppRun", self.run_extracted(app, args)?);
        }

        let mut history = Vec::new();

        match self.run_native(app, args)? {
            Attempt::Started(id) => return Ok(id),
            Attempt::Failed {
                exit,
                message,
                spawn_error,
            } => {
                if !spawn_error && !is_runtime_failure(&message) {
                    return Err(Self::failure("Direct execution", exit, &message));
                }
                history.push(Self::failure("Direct execution", exit, &message).to_string());
            }
        }

        match self.run_extract_and_run(app, args)? {
            Attempt::Started(id) => return Ok(id),
            Attempt::Failed {
                exit,
                message,
                spawn_error,
            } => {
                if !spawn_error && !is_runtime_failure(&message) {
                    return Err(Self::failure("Extract and run", exit, &message));
                }
                history.push(Self::failure("Extract and run", exit, &message).to_string());
            }
        }

        match self.run_extracted(app, args) {
            Ok(Attempt::Started(id)) => Ok(id),
            Ok(Attempt::Failed { exit, message, .. }) => {
                history.push(Self::failure("Extracted AppRun", exit, &message).to_string());
                Err(AppImageError::LaunchError(format!(
                    "Every launch method failed. {}",
                    history.join("; ")
                )))
            }
            Err(e) => {
                history.push(e.to_string());
                Err(AppImageError::LaunchError(format!(
                    "Every launch method failed. {}",
                    history.join("; ")
                )))
            }
        }
    }

    fn run_native(&self, app: &AppImage, args: &[OsString]) -> Result<Attempt, AppImageError> {
        let _ = FileSystem::make_executable(&app.path);
        Ok(self.attempt(app, &app.path, args, Self::identifying_env(app)))
    }

    fn run_extract_and_run(
        &self,
        app: &AppImage,
        args: &[OsString],
    ) -> Result<Attempt, AppImageError> {
        let _ = FileSystem::make_executable(&app.path);
        let mut full = vec![OsString::from("--appimage-extract-and-run")];
        full.extend(args.iter().cloned());
        Ok(self.attempt(app, &app.path, &full, Self::identifying_env(app)))
    }

    fn run_extracted(
        &self,
        app: &mut AppImage,
        args: &[OsString],
    ) -> Result<Attempt, AppImageError> {
        let dir = self.ensure_extracted(app, true)?;
        let apprun = dir.join("AppRun");
        let _ = FileSystem::make_executable(&apprun);
        let mut env = vec![(OsString::from("APPDIR"), dir.into_os_string())];
        env.extend(Self::identifying_env(app));
        Ok(self.attempt(app, &apprun, args, env))
    }

    /// The `APPIMAGE` variable identifies which AppImage a process belongs to, so instances
    /// can be found again in `/proc` even after the program re-executes into something else.
    /// A real AppImage runtime also sets this variable itself for the same purpose.
    fn identifying_env(app: &AppImage) -> Vec<(OsString, OsString)> {
        vec![(
            OsString::from("APPIMAGE"),
            std::fs::canonicalize(&app.path)
                .unwrap_or_else(|_| app.path.clone())
                .into_os_string(),
        )]
    }

    /// Starts one process and watches it for the early-exit window. A process that is still
    /// running afterwards, or that exits successfully, counts as started.
    fn attempt(
        &self,
        app: &AppImage,
        program: &Path,
        args: &[OsString],
        mut env: Vec<(OsString, OsString)>,
    ) -> Attempt {
        let cwd = Self::working_dir();
        if let Some(cwd) = &cwd {
            env.push((OsString::from("OWD"), cwd.clone().into_os_string()));
        }
        let log = self.log_path(app);
        let opts = SpawnOptions {
            env,
            cwd,
            stderr_log: Some(log.clone()),
        };

        let proc = match ProcessLauncher::spawn(program, args, &opts) {
            Ok(p) => p,
            Err(e) => {
                return Attempt::Failed {
                    exit: None,
                    message: e.to_string(),
                    spawn_error: true,
                }
            }
        };

        match proc.wait_for_exit(self.early_exit_window) {
            None => {
                self.tracked()
                    .entry(app.id.clone())
                    .or_default()
                    .push(proc.id);
                Attempt::Started(proc.id)
            }
            Some(exit) if exit.success() => Attempt::Started(proc.id),
            Some(exit) => Attempt::Failed {
                exit: Some(exit),
                message: Self::read_log_tail(&log),
                spawn_error: false,
            },
        }
    }

    fn read_log_tail(path: &Path) -> String {
        const TAIL: u64 = 2048;
        let Ok(mut file) = std::fs::File::open(path) else {
            return String::new();
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let _ = file.seek(SeekFrom::Start(len.saturating_sub(TAIL)));
        let mut buf = Vec::new();
        let _ = file.take(TAIL).read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).trim().to_string()
    }

    /// True if the AppImage file changed after the extraction was made.
    fn extraction_is_stale(app: &AppImage, extracted: &Path) -> bool {
        let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        match (modified(&app.path), modified(extracted)) {
            (Some(appimage), Some(extraction)) => appimage > extraction,
            _ => false,
        }
    }

    pub fn ensure_extracted(
        &self,
        app: &mut AppImage,
        allow_execute_fallback: bool,
    ) -> Result<PathBuf, AppImageError> {
        let target = self.paths.app_extracted_dir(&app.id);
        if !target.join("AppRun").exists() || Self::extraction_is_stale(app, &target) {
            AppImageExtractor::extract_appdir(&app.path, &target, allow_execute_fallback)?;
        }
        app.extracted_dir = Some(target.clone());
        Ok(target)
    }

    /// Cheap check based on processes started or discovered earlier. It never scans /proc.
    pub fn is_running(&self, app: &AppImage) -> bool {
        let mut running = self.tracked();
        match running.get_mut(&app.id) {
            Some(ids) => {
                ids.retain(|id| ProcessLauncher::is_alive(*id));
                if ids.is_empty() {
                    running.remove(&app.id);
                    false
                } else {
                    true
                }
            }
            None => false,
        }
    }

    /// Scans the system once for running instances of all the given applications, including
    /// ones started outside AppNest, and returns the ids that are running. This reads /proc,
    /// so call it from a background thread.
    pub fn refresh_running(&self, apps: &[AppImage]) -> HashSet<String> {
        let targets: Vec<ScanTarget> = apps
            .iter()
            .filter(|a| a.path.exists())
            .map(|a| ScanTarget {
                id: a.id.clone(),
                app_path: a.path.clone(),
                apprun_path: a.apprun_path(),
            })
            .collect();
        let mut found = ProcessLauncher::find_running(&targets);

        let mut running = self.tracked();
        for app in apps {
            let mut ids = found.remove(&app.id).unwrap_or_default();
            for id in running.get(&app.id).into_iter().flatten() {
                if ProcessLauncher::is_alive(*id) && !ids.contains(id) {
                    ids.push(*id);
                }
            }
            if ids.is_empty() {
                running.remove(&app.id);
            } else {
                running.insert(app.id.clone(), ids);
            }
        }
        apps.iter()
            .filter(|a| running.contains_key(&a.id))
            .map(|a| a.id.clone())
            .collect()
    }

    /// Asks the application to quit (SIGTERM) and waits for it to exit. Only processes that
    /// were started by AppNest, or found running from this AppImage, are signalled, and each
    /// is re-checked against its start time so a recycled PID is never signalled.
    pub fn terminate(&self, app: &AppImage) -> Result<(), AppImageError> {
        let owned: Vec<ProcId> = self.tracked().remove(&app.id).unwrap_or_default();
        let discovered = ProcessLauncher::find_running(&[ScanTarget {
            id: app.id.clone(),
            app_path: app.path.clone(),
            apprun_path: app.apprun_path(),
        }])
        .remove(&app.id)
        .unwrap_or_default();

        let mut targets: Vec<(ProcId, bool)> = owned
            .into_iter()
            .filter(|id| ProcessLauncher::is_alive(*id))
            .map(|id| (id, true))
            .collect();
        for id in discovered {
            if !targets.iter().any(|(t, _)| *t == id) {
                targets.push((id, false));
            }
        }
        if targets.is_empty() {
            return Ok(());
        }

        let mut errors = Vec::new();
        for (id, ours) in &targets {
            if let Err(e) = ProcessLauncher::terminate(*id, *ours) {
                errors.push(format!("PID {}: {}", id.pid, e));
            }
        }

        let deadline = Instant::now() + TERMINATE_GRACE;
        while targets.iter().any(|(id, _)| ProcessLauncher::is_alive(*id)) {
            if Instant::now() >= deadline {
                let alive: Vec<_> = targets
                    .iter()
                    .filter(|(id, _)| ProcessLauncher::is_alive(*id))
                    .map(|(id, _)| id.pid.to_string())
                    .collect();
                // Keep tracking what is still running
                self.tracked().entry(app.id.clone()).or_default().extend(
                    targets
                        .iter()
                        .map(|(id, _)| *id)
                        .filter(|id| ProcessLauncher::is_alive(*id)),
                );
                return Err(AppImageError::LaunchError(format!(
                    "The application did not exit after being asked to close (PID {})",
                    alive.join(", ")
                )));
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(AppImageError::LaunchError(format!(
                "Failed to terminate process: {}",
                errors.join(", ")
            )))
        }
    }
}
