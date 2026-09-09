use crate::config::paths::AppPaths;
use crate::domain::appimage::{AppImage, AppImageError, RuntimeMethod};
use crate::infrastructure::process::ProcessLauncher;
use crate::services::extractor::AppImageExtractor;
use std::ffi::OsString;
use std::process::Command;

pub struct AppImageLauncher {
    paths: AppPaths,
}

impl AppImageLauncher {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn launch(
        &self,
        app: &mut AppImage,
        extra_args: &[OsString],
    ) -> Result<u32, AppImageError> {
        let mut combined_args: Vec<OsString> = Vec::new();

        // Check if --no-sandbox is needed from metadata
        if let Some(ref exec) = app.metadata.exec_args {
            if exec.contains("--no-sandbox") {
                combined_args.push(OsString::from("--no-sandbox"));
            }
        }
        combined_args.extend(extra_args.iter().cloned());

        match app.runtime_method {
            RuntimeMethod::Extracted => self.launch_extracted(app, &combined_args),
            RuntimeMethod::Native => self.launch_native(app, &combined_args),
            RuntimeMethod::Fuse => self.launch_fuse(app, &combined_args),
            RuntimeMethod::Auto => self.launch_auto(app, &combined_args),
        }
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

        ProcessLauncher::spawn_detached(&apprun, args, Some(&extracted_dir))
            .map_err(|e| AppImageError::LaunchError(format!("Failed to spawn AppRun: {}", e)))
    }

    fn launch_native(&self, app: &AppImage, args: &[OsString]) -> Result<u32, AppImageError> {
        let parent = app.path.parent();
        ProcessLauncher::spawn_detached(&app.path, args, parent).map_err(|e| {
            AppImageError::LaunchError(format!("Failed to spawn AppImage natively: {}", e))
        })
    }

    fn launch_fuse(&self, app: &AppImage, args: &[OsString]) -> Result<u32, AppImageError> {
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

        // Test running natively or check if execution fails quickly
        let test_status = Command::new(&app.path).arg("--help").output();

        let should_extract = match test_status {
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Detect common binfmt / AppImageLauncher bypass errors or missing FUSE
                stderr.contains("AppImageLauncher")
                    || stderr.contains("realpath")
                    || stderr.contains("fuse: device not found")
                    || stderr.contains("cannot mount AppImage")
                    || stderr.contains("ELF file ABI version invalid")
            }
            Err(_) => true,
        };

        if !should_extract {
            if let Ok(pid) = self.launch_native(app, args) {
                return Ok(pid);
            }
        }

        // Fallback: extract and run AppRun
        let pid = self.launch_extracted(app, args)?;
        app.runtime_method = RuntimeMethod::Extracted;
        Ok(pid)
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
