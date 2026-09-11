use crate::config::paths::AppPaths;
use crate::domain::appimage::{AppImage, AppImageError, RuntimeMethod};
use crate::infrastructure::filesystem::FileSystem;
use crate::infrastructure::process::ProcessLauncher;
use crate::services::extractor::AppImageExtractor;
use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Mutex;

pub struct AppImageLauncher {
    paths: AppPaths,
    running_apps: Mutex<HashMap<String, Vec<u32>>>,
}

impl AppImageLauncher {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            running_apps: Mutex::new(HashMap::new()),
        }
    }

    pub fn launch(
        &self,
        app: &mut AppImage,
        extra_args: &[OsString],
    ) -> Result<u32, AppImageError> {
        if !app.path.exists() {
            return Err(AppImageError::NotFound(app.path.clone()));
        }

        let mut combined_args: Vec<OsString> = Vec::new();

        // Check if --no-sandbox is needed from metadata
        if let Some(ref exec) = app.metadata.exec_args {
            if exec.contains("--no-sandbox") {
                combined_args.push(OsString::from("--no-sandbox"));
            }
        }
        combined_args.extend(extra_args.iter().cloned());

        let pid = match app.runtime_method {
            RuntimeMethod::Extracted => self.launch_extracted(app, &combined_args)?,
            RuntimeMethod::Native => self.launch_native(app, &combined_args)?,
            RuntimeMethod::Fuse => self.launch_fuse(app, &combined_args)?,
            RuntimeMethod::Auto => self.launch_auto(app, &combined_args)?,
        };

        if let Ok(mut running) = self.running_apps.lock() {
            running.entry(app.id.clone()).or_default().push(pid);
        }

        Ok(pid)
    }

    pub fn is_running(&self, app: &AppImage) -> bool {
        // 1. Check tracked PIDs
        if let Ok(mut running) = self.running_apps.lock() {
            if let Some(pids) = running.get_mut(&app.id) {
                pids.retain(|&pid| ProcessLauncher::is_pid_alive(pid));
                if !pids.is_empty() {
                    return true;
                }
            }
        }

        // 2. Discover running instances on the system
        let system_pids =
            ProcessLauncher::find_pids_for_appimage(&app.path, app.apprun_path().as_deref());
        if !system_pids.is_empty() {
            if let Ok(mut running) = self.running_apps.lock() {
                running.insert(app.id.clone(), system_pids);
            }
            return true;
        }

        false
    }

    pub fn terminate(&self, app: &AppImage) -> Result<(), AppImageError> {
        let mut pids_to_kill = Vec::new();

        if let Ok(mut running) = self.running_apps.lock() {
            if let Some(pids) = running.remove(&app.id) {
                for pid in pids {
                    if ProcessLauncher::is_pid_alive(pid) {
                        pids_to_kill.push(pid);
                    }
                }
            }
        }

        let system_pids =
            ProcessLauncher::find_pids_for_appimage(&app.path, app.apprun_path().as_deref());
        for sp in system_pids {
            if !pids_to_kill.contains(&sp) {
                pids_to_kill.push(sp);
            }
        }

        if pids_to_kill.is_empty() {
            return Ok(());
        }

        let mut errors = Vec::new();
        for pid in pids_to_kill {
            if let Err(e) = ProcessLauncher::terminate_pid(pid) {
                errors.push(format!("PID {}: {}", pid, e));
            }
        }

        if !errors.is_empty() {
            return Err(AppImageError::LaunchError(format!(
                "Failed to terminate process: {}",
                errors.join(", ")
            )));
        }

        Ok(())
    }

    fn launch_extracted(
        &self,
        app: &mut AppImage,
        args: &[OsString],
    ) -> Result<u32, AppImageError> {
        let extracted_dir = self.ensure_extracted(app)?;
        let apprun = extracted_dir.join("AppRun");

        if !apprun.exists() {
            return Err(AppImageError::LaunchError(format!(
                "AppRun not found in extracted directory {}",
                extracted_dir.display()
            )));
        }

        let _ = FileSystem::make_executable(&apprun);

        ProcessLauncher::spawn_detached(&apprun, args, Some(&extracted_dir))
            .map_err(|e| AppImageError::LaunchError(format!("Failed to spawn AppRun: {}", e)))
    }

    fn launch_native(&self, app: &AppImage, args: &[OsString]) -> Result<u32, AppImageError> {
        let _ = FileSystem::make_executable(&app.path);
        let parent = app.path.parent();
        ProcessLauncher::spawn_detached(&app.path, args, parent).map_err(|e| {
            AppImageError::LaunchError(format!("Failed to spawn AppImage natively: {}", e))
        })
    }

    fn launch_fuse(&self, app: &AppImage, args: &[OsString]) -> Result<u32, AppImageError> {
        let _ = FileSystem::make_executable(&app.path);
        let mut fuse_args = vec![std::ffi::OsString::from("--appimage-extract-and-run")];
        fuse_args.extend(args.iter().cloned());

        let parent = app.path.parent();
        ProcessLauncher::spawn_detached(&app.path, &fuse_args, parent).map_err(|e| {
            AppImageError::LaunchError(format!("Failed to spawn with extract-and-run: {}", e))
        })
    }

    fn launch_auto(&self, app: &mut AppImage, args: &[OsString]) -> Result<u32, AppImageError> {
        // If already extracted, launching AppRun is fastest and avoids any binfmt / kernel interception
        if app.is_extracted() {
            return self.launch_extracted(app, args);
        }

        let _ = FileSystem::make_executable(&app.path);

        // Attempt direct native execution first (single spawn)
        match self.launch_native(app, args) {
            Ok(pid) => Ok(pid),
            Err(_) => {
                // If native spawn fails, fallback to extract-and-run (bypasses FUSE issues)
                match self.launch_fuse(app, args) {
                    Ok(pid) => Ok(pid),
                    Err(_) => {
                        // Fallback: extract to managed directory and run AppRun
                        let pid = self.launch_extracted(app, args)?;
                        app.runtime_method = RuntimeMethod::Extracted;
                        Ok(pid)
                    }
                }
            }
        }
    }

    pub fn ensure_extracted(
        &self,
        app: &mut AppImage,
    ) -> Result<std::path::PathBuf, AppImageError> {
        let target = self.paths.app_extracted_dir(&app.id);
        if !target.join("AppRun").exists() {
            AppImageExtractor::extract_appdir(&app.path, &target)?;
        }
        app.extracted_dir = Some(target.clone());
        Ok(target)
    }
}

