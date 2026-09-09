use crate::domain::appimage::{AppImageError, AppImageMetadata};
use crate::infrastructure::filesystem::FileSystem;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct AppImageExtractor;

impl AppImageExtractor {
    pub fn extract_appdir(appimage_path: &Path, target_dir: &Path) -> Result<(), AppImageError> {
        fs::create_dir_all(target_dir)?;

        // Try using 7z first (most reliable on Linux systems, bypasses binfmt issues)
        let seven_z_status = Command::new("7z")
            .arg("x")
            .arg("-y")
            .arg(format!("-o{}", target_dir.display()))
            .arg(appimage_path)
            .status();

        let mut extracted_successfully = false;

        if let Ok(status) = seven_z_status {
            if status.success() && target_dir.join("AppRun").exists() {
                extracted_successfully = true;
            }
        }

        // Fallback to internal --appimage-extract
        if !extracted_successfully {
            let temp_extract_parent = tempfile::tempdir().map_err(|e| {
                AppImageError::ExtractionError(format!("Failed to create temp dir: {}", e))
            })?;

            let fallback_status = Command::new(appimage_path)
                .arg("--appimage-extract")
                .current_dir(temp_extract_parent.path())
                .status();

            if let Ok(status) = fallback_status {
                let squashfs_root = temp_extract_parent.path().join("squashfs-root");
                if status.success() && squashfs_root.exists() {
                    for entry in fs::read_dir(&squashfs_root)? {
                        let entry = entry?;
                        let file_type = entry.file_type()?;
                        let dest = target_dir.join(entry.file_name());
                        if file_type.is_dir() {
                            let _ = Command::new("cp")
                                .arg("-r")
                                .arg(entry.path())
                                .arg(&dest)
                                .status();
                        } else {
                            let _ = fs::copy(entry.path(), &dest);
                        }
                    }
                    extracted_successfully = true;
                }
            }
        }

        if !extracted_successfully && !target_dir.join("AppRun").exists() {
            return Err(AppImageError::ExtractionError(
                "Failed to extract AppImage using both 7z and --appimage-extract".to_string(),
            ));
        }

        // Ensure AppRun is executable
        let apprun = target_dir.join("AppRun");
        if apprun.exists() {
            let _ = FileSystem::make_executable(&apprun);
        }

        Ok(())
    }

    pub fn extract_metadata_and_icon(
        appimage_path: &Path,
        icons_dir: &Path,
        app_id: &str,
    ) -> Result<(AppImageMetadata, Option<PathBuf>), AppImageError> {
        fs::create_dir_all(icons_dir)?;

        let temp_dir = tempfile::tempdir().map_err(|e| {
            AppImageError::ExtractionError(format!("Failed to create temp dir: {}", e))
        })?;

        // Extract metadata items and icon trees using 7z
        let _ = Command::new("7z")
            .arg("x")
            .arg("-y")
            .arg(format!("-o{}", temp_dir.path().display()))
            .arg(appimage_path)
            .arg("*.desktop")
            .arg(".DirIcon")
            .arg("*.png")
            .arg("*.svg")
            .arg("usr/share/icons/*")
            .arg("usr/share/pixmaps/*")
            .output();

        let mut metadata = AppImageMetadata::default();

        // Search for desktop file
        if let Some(desktop_file) = Self::find_file_with_ext(temp_dir.path(), "desktop") {
            metadata = Self::parse_desktop_file(&desktop_file);
        }

        let icon_dest = Self::resolve_and_copy_icon(
            temp_dir.path(),
            icons_dir,
            app_id,
            metadata.icon_name.as_deref(),
        );

        Ok((metadata, icon_dest))
    }

    pub fn extract_icon_only(
        appimage_path: &Path,
        icons_dir: &Path,
        app_id: &str,
        icon_name: Option<&str>,
    ) -> Option<PathBuf> {
        let _ = fs::create_dir_all(icons_dir);

        let temp_dir = tempfile::tempdir().ok()?;

        let _ = Command::new("7z")
            .arg("x")
            .arg("-y")
            .arg(format!("-o{}", temp_dir.path().display()))
            .arg(appimage_path)
            .arg(".DirIcon")
            .arg("*.png")
            .arg("*.svg")
            .arg("usr/share/icons/*")
            .arg("usr/share/pixmaps/*")
            .output();

        Self::resolve_and_copy_icon(temp_dir.path(), icons_dir, app_id, icon_name)
    }

    fn resolve_and_copy_icon(
        extracted_root: &Path,
        icons_dir: &Path,
        app_id: &str,
        preferred_name: Option<&str>,
    ) -> Option<PathBuf> {
        // Strategy 1: Check .DirIcon (handling symlinks)
        let dir_icon = extracted_root.join(".DirIcon");
        if dir_icon.is_symlink() {
            if let Ok(target) = fs::read_link(&dir_icon) {
                let resolved = if target.is_relative() {
                    extracted_root.join(&target)
                } else {
                    target
                };
                if resolved.exists() && resolved.is_file() {
                    let ext = resolved
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("png");
                    let dest = icons_dir.join(format!("{}.{}", app_id, ext));
                    if fs::copy(&resolved, &dest).is_ok() {
                        return Some(dest);
                    }
                }
            }
        } else if dir_icon.exists() && dir_icon.is_file() {
            let dest = icons_dir.join(format!("{}.png", app_id));
            if fs::copy(&dir_icon, &dest).is_ok() {
                return Some(dest);
            }
        }

        // Strategy 2: If preferred_name (icon_name from desktop file) is known
        if let Some(name) = preferred_name {
            let candidates = [format!("{}.png", name), format!("{}.svg", name)];
            for cand in candidates {
                if let Some(found) = Self::find_file_named(extracted_root, &cand) {
                    let real_found = if found.is_symlink() {
                        if let Ok(target) = fs::read_link(&found) {
                            if target.is_relative() {
                                found.parent().map(|p| p.join(&target)).unwrap_or(target)
                            } else {
                                target
                            }
                        } else {
                            found.clone()
                        }
                    } else {
                        found.clone()
                    };

                    if real_found.exists() && real_found.is_file() {
                        let ext = real_found
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or("png");
                        let dest = icons_dir.join(format!("{}.{}", app_id, ext));
                        if fs::copy(&real_found, &dest).is_ok() {
                            return Some(dest);
                        }
                    }
                }
            }
        }

        // Strategy 3: Find any high-res PNG or SVG in usr/share/icons
        let mut largest_png: Option<(PathBuf, u64)> = None;
        let mut first_svg: Option<PathBuf> = None;

        for entry in walkdir::WalkDir::new(extracted_root).into_iter().flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                if ext_str == "svg" && first_svg.is_none() {
                    first_svg = Some(path.to_path_buf());
                } else if ext_str == "png" {
                    if let Ok(meta) = fs::metadata(path) {
                        let size = meta.len();
                        if largest_png.as_ref().map_or(true, |(_, s)| size > *s) {
                            largest_png = Some((path.to_path_buf(), size));
                        }
                    }
                }
            }
        }

        if let Some((png_path, _)) = largest_png {
            let dest = icons_dir.join(format!("{}.png", app_id));
            if fs::copy(&png_path, &dest).is_ok() {
                return Some(dest);
            }
        }

        if let Some(svg_path) = first_svg {
            let dest = icons_dir.join(format!("{}.svg", app_id));
            if fs::copy(&svg_path, &dest).is_ok() {
                return Some(dest);
            }
        }

        None
    }

    pub fn parse_desktop_file(path: &Path) -> AppImageMetadata {
        let mut meta = AppImageMetadata::default();
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return meta,
        };

        let reader = BufReader::new(file);
        let mut in_entry_section = false;

        for line in reader.lines().flatten() {
            let line = line.trim();
            if line.starts_with('[') && line.ends_with(']') {
                in_entry_section = line == "[Desktop Entry]";
                continue;
            }

            if !in_entry_section || line.starts_with('#') || !line.contains('=') {
                continue;
            }

            let mut parts = line.splitn(2, '=');
            let key = parts.next().unwrap_or("").trim();
            let val = parts.next().unwrap_or("").trim();

            match key {
                "Name" if meta.display_name.is_none() => {
                    meta.display_name = Some(val.to_string());
                }
                "Comment" if meta.comment.is_none() => {
                    meta.comment = Some(val.to_string());
                }
                "Icon" => {
                    meta.icon_name = Some(val.to_string());
                }
                "Exec" => {
                    meta.exec_args = Some(val.to_string());
                }
                "X-AppImage-Version" | "Version" if meta.version.is_none() => {
                    meta.version = Some(val.to_string());
                }
                "Terminal" => {
                    meta.terminal = val.eq_ignore_ascii_case("true");
                }
                "Categories" => {
                    meta.categories = val
                        .split(';')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                "MimeType" => {
                    meta.mime_types = val
                        .split(';')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                _ => {}
            }
        }

        meta
    }

    fn find_file_with_ext(dir: &Path, target_ext: &str) -> Option<PathBuf> {
        for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
            if entry.file_type().is_file() {
                if let Some(ext) = entry.path().extension() {
                    if ext.to_string_lossy().eq_ignore_ascii_case(target_ext) {
                        return Some(entry.path().to_path_buf());
                    }
                }
            }
        }
        None
    }

    fn find_file_named(dir: &Path, target_name: &str) -> Option<PathBuf> {
        for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(target_name)
            {
                return Some(entry.path().to_path_buf());
            }
        }
        None
    }
}
