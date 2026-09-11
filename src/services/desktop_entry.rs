use crate::domain::appimage::{AppImage, AppImageError, RuntimeMethod};
use crate::infrastructure::filesystem::FileSystem;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct DesktopEntryService;

impl DesktopEntryService {
    pub fn create_entry(app: &AppImage, desktop_dir: &Path) -> Result<PathBuf, AppImageError> {
        let dest = desktop_dir.join(format!("{}.desktop", app.id));

        // Determine executable path and arguments
        let mut exec_line = match app.runtime_method {
            RuntimeMethod::Extracted => {
                let target = if let Some(apprun) = app.apprun_path() {
                    if apprun.exists() {
                        apprun
                    } else {
                        app.path.clone()
                    }
                } else {
                    app.path.clone()
                };
                format!("\"{}\"", target.display())
            }
            RuntimeMethod::Fuse => {
                format!("\"{}\" --appimage-extract-and-run", app.path.display())
            }
            _ => {
                if app.is_extracted() {
                    let target = app.apprun_path().unwrap_or_else(|| app.path.clone());
                    format!("\"{}\"", target.display())
                } else {
                    format!("\"{}\"", app.path.display())
                }
            }
        };

        let mut lines = Vec::new();
        lines.push("[Desktop Entry]".to_string());
        lines.push("Type=Application".to_string());
        lines.push(format!("Name={}", app.name));

        if let Some(ref comment) = app.metadata.comment {
            lines.push(format!("Comment={}", comment));
        }

        // Check if --no-sandbox is needed
        let needs_no_sandbox = app
            .metadata
            .exec_args
            .as_ref()
            .map_or(false, |a| a.contains("--no-sandbox"));
        if needs_no_sandbox && !exec_line.contains("--no-sandbox") {
            exec_line.push_str(" --no-sandbox");
        }
        exec_line.push_str(" %U");
        lines.push(format!("Exec={}", exec_line));


        if let Some(ref icon) = app.icon_path {
            lines.push(format!("Icon={}", icon.display()));
        } else if let Some(ref icon_name) = app.metadata.icon_name {
            lines.push(format!("Icon={}", icon_name));
        }

        lines.push(format!("Terminal={}", app.metadata.terminal));

        let categories = if !app.metadata.categories.is_empty() {
            format!("Categories={};", app.metadata.categories.join(";"))
        } else {
            "Categories=Utility;Application;".to_string()
        };
        lines.push(categories);

        if let Some(ref version) = app.version {
            lines.push(format!("X-AppImage-Version={}", version));
        }

        if !app.metadata.mime_types.is_empty() {
            lines.push(format!("MimeType={};", app.metadata.mime_types.join(";")));
        }

        if let Some(ref wm_class) = app.metadata.startup_wm_class {
            lines.push(format!("StartupWMClass={}", wm_class));
        }

        if !app.metadata.keywords.is_empty() {
            lines.push(format!("Keywords={};", app.metadata.keywords.join(";")));
        }

        lines.push("StartupNotify=true".to_string());
        lines.push(format!("X-AppImage-Manager-ID={}", app.id));

        let content = lines.join("\n") + "\n";
        FileSystem::atomic_write(&dest, &content).map_err(|e| {
            AppImageError::DesktopEntryError(format!("Failed to write desktop file: {}", e))
        })?;

        let _ = FileSystem::make_executable(&dest);

        // Update desktop database and icon caches
        Self::update_desktop_database(desktop_dir);

        Ok(dest)
    }

    pub fn remove_entry(app: &AppImage, desktop_dir: &Path) -> Result<(), AppImageError> {
        let dest = desktop_dir.join(format!("{}.desktop", app.id));
        let _ = FileSystem::remove_file_if_exists(&dest);
        Self::update_desktop_database(desktop_dir);
        Ok(())
    }

    pub fn update_desktop_database(dir: &Path) {
        let _ = Command::new("update-desktop-database").arg(dir).output();
    }
}
