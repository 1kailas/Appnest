use crate::domain::appimage::{AppImage, AppImageError, RuntimeMethod};
use crate::infrastructure::filesystem::FileSystem;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct DesktopEntryService;

impl DesktopEntryService {
    pub fn generate_entry_content(app: &AppImage) -> String {
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

        lines.join("\n") + "\n"
    }

    pub fn create_entry(app: &AppImage, desktop_dir: &Path) -> Result<PathBuf, AppImageError> {
        let dest = desktop_dir.join(format!("{}.desktop", app.id));
        let content = Self::generate_entry_content(app);

        FileSystem::atomic_write(&dest, &content).map_err(|e| {
            AppImageError::DesktopEntryError(format!("Failed to write desktop file: {}", e))
        })?;

        let _ = FileSystem::make_executable(&dest);

        // Update desktop database and icon caches
        Self::update_desktop_database(desktop_dir);

        Ok(dest)
    }

    pub fn create_user_desktop_shortcut(
        app: &AppImage,
        user_desktop_dir: Option<&Path>,
    ) -> Option<PathBuf> {
        let desktop_dir = user_desktop_dir?;
        if !desktop_dir.is_dir() {
            return None;
        }

        let dest = desktop_dir.join(format!("{}.desktop", app.name));
        let content = Self::generate_entry_content(app);

        if FileSystem::atomic_write(&dest, &content).is_ok() {
            let _ = FileSystem::make_executable(&dest);
            // Mark trusted in GNOME so it doesn't show "Untrusted application launcher"
            let _ = Command::new("gio")
                .args([
                    "set",
                    "-t",
                    "string",
                    &dest.to_string_lossy(),
                    "metadata::trusted",
                    "true",
                ])
                .output();
            Some(dest)
        } else {
            None
        }
    }

    pub fn remove_user_desktop_shortcut(app: &AppImage, user_desktop_dir: Option<&Path>) {
        if let Some(dir) = user_desktop_dir {
            let _ = FileSystem::remove_file_if_exists(&dir.join(format!("{}.desktop", app.name)));
            let _ = FileSystem::remove_file_if_exists(&dir.join(format!("{}.desktop", app.id)));
        }
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

    pub fn ensure_mime_integration() {
        if let Some(base) = directories::BaseDirs::new() {
            // 1. Ensure MIME type definition XML exists
            let mime_pkg_dir = base.data_local_dir().join("mime/packages");
            let _ = std::fs::create_dir_all(&mime_pkg_dir);
            let xml_file = mime_pkg_dir.join("appimage.xml");
            let xml_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/vnd.appimage">
    <comment>AppImage application</comment>
    <glob pattern="*.AppImage" weight="80"/>
    <glob pattern="*.appimage" weight="80"/>
    <glob pattern="*.APPIMAGE" weight="80"/>
    <magic priority="80">
      <match type="string" offset="1" value="ELF">
        <match type="string" offset="8" value="AI\x02"/>
        <match type="string" offset="8" value="AI\x01"/>
      </match>
    </magic>
  </mime-type>
</mime-info>
"#;
            let needs_mime_update = !xml_file.exists()
                || std::fs::read_to_string(&xml_file).map_or(true, |c| c != xml_content);

            if needs_mime_update {
                let _ = std::fs::write(&xml_file, xml_content);
                let mime_dir = base.data_local_dir().join("mime");
                let _ = Command::new("update-mime-database").arg(&mime_dir).output();
            }

            // 2. Check if MIME associations already exist in mimeapps.list
            let mimeapps_path = base.config_dir().join("mimeapps.list");
            let app_desktop = "io.github._1kailas.AppNest.desktop";
            let mime_types = [
                "application/vnd.appimage",
                "application/x-appimage",
                "application/x-iso9660-appimage",
            ];

            let mimeapps_content = std::fs::read_to_string(&mimeapps_path).unwrap_or_default();
            let all_registered = mime_types
                .iter()
                .all(|mt| mimeapps_content.contains(&format!("{}={}", mt, app_desktop)));

            if !all_registered {
                // Register with xdg-mime and gio
                for mt in &mime_types {
                    let _ = Command::new("xdg-mime")
                        .args(["default", app_desktop, mt])
                        .output();
                    let _ = Command::new("gio").args(["mime", mt, app_desktop]).output();
                }

                // Ensure mimeapps.list has all associations
                let content = std::fs::read_to_string(&mimeapps_path).unwrap_or_default();
                let associations: Vec<String> = mime_types
                    .iter()
                    .filter(|mt| !content.contains(&format!("{}={}", mt, app_desktop)))
                    .map(|mt| format!("{}={}", mt, app_desktop))
                    .collect();

                if !associations.is_empty() {
                    let mut new_lines = Vec::new();
                    let mut added = false;
                    for line in content.lines() {
                        new_lines.push(line.to_string());
                        if line.trim() == "[Default Applications]" && !added {
                            new_lines.extend(associations.iter().cloned());
                            added = true;
                        }
                    }
                    if !added {
                        new_lines.push("[Default Applications]".to_string());
                        new_lines.extend(associations);
                    }
                    let _ = std::fs::write(&mimeapps_path, new_lines.join("\n") + "\n");
                }
            }
        }
    }
}
