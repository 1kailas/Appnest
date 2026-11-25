use crate::config::paths::DESKTOP_FILE_PREFIX;
use crate::domain::appimage::{AppImage, AppImageError, RuntimeMethod};
use crate::infrastructure::filesystem::FileSystem;
use gio::prelude::*;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

const ID_KEY: &str = "X-AppImage-Manager-ID";

pub struct DesktopEntryService;

/// Escapes a value of type `string` (Desktop Entry Specification, "Value types").
/// Control characters other than newline, carriage return and tab are dropped.
fn escape_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Prepares an item for a `;`-separated list: separators and control characters are removed.
fn list_item(s: &str) -> String {
    let cleaned: String = s.chars().filter(|c| *c != ';' && !c.is_control()).collect();
    escape_value(cleaned.trim())
}

fn join_list(items: &[String]) -> Option<String> {
    let items: Vec<String> = items
        .iter()
        .map(|i| list_item(i))
        .filter(|i| !i.is_empty())
        .collect();
    (!items.is_empty()).then(|| format!("{};", items.join(";")))
}

/// Quotes one `Exec` argument: the argument is wrapped in double quotes, `"`, `` ` ``, `$`
/// and `\` are backslash-escaped, and `%` is doubled so it is never read as a field code.
/// The caller still applies `escape_value` to the finished line.
fn quote_exec_arg(arg: &str) -> String {
    let mut out = String::from("\"");
    for c in arg.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// True if `path` is a regular (non-symlink) file written by AppNest for application `id`.
fn is_owned_entry(path: &Path, id: &str) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.file_type().is_file() {
        return false;
    }
    let mut content = String::new();
    if std::fs::File::open(path)
        .and_then(|f| f.take(64 * 1024).read_to_string(&mut content))
        .is_err()
    {
        return false;
    }
    let marker = format!("{}={}", ID_KEY, id);
    content.lines().any(|l| l.trim() == marker)
}

impl DesktopEntryService {
    fn file_name(app: &AppImage) -> Result<String, AppImageError> {
        if !is_safe_id(&app.id) {
            return Err(AppImageError::UnsafePath(format!(
                "invalid application id {:?}",
                app.id
            )));
        }
        Ok(format!("{}{}.desktop", DESKTOP_FILE_PREFIX, app.id))
    }

    fn legacy_file_name(app: &AppImage) -> Option<String> {
        is_safe_id(&app.id).then(|| format!("{}.desktop", app.id))
    }

    pub fn entry_path(app: &AppImage, desktop_dir: &Path) -> Result<PathBuf, AppImageError> {
        Ok(desktop_dir.join(Self::file_name(app)?))
    }

    pub fn generate_entry_content(app: &AppImage) -> String {
        let target = match app.runtime_method {
            RuntimeMethod::Extracted => app.apprun_path().filter(|a| a.exists()),
            RuntimeMethod::Auto if app.is_extracted() => app.apprun_path(),
            _ => None,
        }
        .unwrap_or_else(|| app.path.clone());

        let mut exec = quote_exec_arg(&target.to_string_lossy());
        if app.runtime_method == RuntimeMethod::ExtractAndRun {
            exec.push_str(" --appimage-extract-and-run");
        }
        exec.push_str(" %U");

        let mut lines = vec![
            "[Desktop Entry]".to_string(),
            "Type=Application".to_string(),
            format!("Name={}", escape_value(&app.name)),
        ];
        if let Some(comment) = app
            .metadata
            .comment
            .as_deref()
            .filter(|c| !c.trim().is_empty())
        {
            lines.push(format!("Comment={}", escape_value(comment)));
        }
        lines.push(format!("Exec={}", escape_value(&exec)));

        if let Some(icon) = &app.icon_path {
            lines.push(format!("Icon={}", escape_value(&icon.to_string_lossy())));
        } else if let Some(name) = &app.metadata.icon_name {
            lines.push(format!("Icon={}", escape_value(name)));
        }

        lines.push(format!("Terminal={}", app.metadata.terminal));
        lines.push(format!(
            "Categories={}",
            join_list(&app.metadata.categories).unwrap_or_else(|| "Utility;".to_string())
        ));
        if let Some(version) = &app.version {
            lines.push(format!("X-AppImage-Version={}", escape_value(version)));
        }
        if let Some(mimes) = join_list(&app.metadata.mime_types) {
            lines.push(format!("MimeType={}", mimes));
        }
        if let Some(wm) = &app.metadata.startup_wm_class {
            lines.push(format!("StartupWMClass={}", escape_value(wm)));
        }
        if let Some(keywords) = join_list(&app.metadata.keywords) {
            lines.push(format!("Keywords={}", keywords));
        }
        lines.push("StartupNotify=true".to_string());
        lines.push(format!("{}={}", ID_KEY, app.id));

        lines.join("\n") + "\n"
    }

    /// Writes the application menu entry (`appnest-<id>.desktop`) and removes a legacy
    /// `<id>.desktop` entry written by earlier versions. Returns the new entry's path.
    pub fn create_entry(app: &AppImage, desktop_dir: &Path) -> Result<PathBuf, AppImageError> {
        let dest = Self::entry_path(app, desktop_dir)?;
        FileSystem::atomic_write(&dest, &Self::generate_entry_content(app)).map_err(|e| {
            AppImageError::DesktopEntryError(format!("Failed to write desktop file: {}", e))
        })?;
        Self::remove_legacy(app, desktop_dir);
        Self::update_desktop_database(desktop_dir);
        Ok(dest)
    }

    fn remove_legacy(app: &AppImage, dir: &Path) {
        for name in Self::legacy_names(app) {
            let legacy = dir.join(name);
            if is_owned_entry(&legacy, &app.id) {
                let _ = FileSystem::remove_file_if_exists(&legacy);
            }
        }
    }

    fn legacy_names(app: &AppImage) -> Vec<String> {
        let mut names: Vec<String> = Self::legacy_file_name(app).into_iter().collect();
        let name_file = format!("{}.desktop", app.name);
        if !app.name.contains(['/', '\0']) && app.name != ".." && app.name != "." {
            names.push(name_file);
        }
        names
    }

    /// Rewrites an entry that still uses the legacy file name under the new name.
    pub fn migrate_legacy_entry(app: &AppImage, desktop_dir: &Path) -> Option<PathBuf> {
        let legacy = desktop_dir.join(Self::legacy_file_name(app)?);
        is_owned_entry(&legacy, &app.id)
            .then(|| Self::create_entry(app, desktop_dir).ok())
            .flatten()
    }

    pub fn remove_entry(app: &AppImage, desktop_dir: &Path) -> Result<(), AppImageError> {
        let dest = Self::entry_path(app, desktop_dir)?;
        let _ = FileSystem::remove_file_if_exists(&dest);
        Self::remove_legacy(app, desktop_dir);
        Self::update_desktop_database(desktop_dir);
        Ok(())
    }

    /// Creates a shortcut on the user's desktop. An existing file that AppNest did not
    /// create is never overwritten.
    pub fn create_user_desktop_shortcut(
        app: &AppImage,
        user_desktop_dir: Option<&Path>,
    ) -> Option<PathBuf> {
        let desktop_dir = user_desktop_dir.filter(|d| d.is_dir())?;
        let dest = Self::entry_path(app, desktop_dir).ok()?;
        if std::fs::symlink_metadata(&dest).is_ok() && !is_owned_entry(&dest, &app.id) {
            return None;
        }

        FileSystem::atomic_write_with_mode(&dest, &Self::generate_entry_content(app), 0o755)
            .ok()?;
        // Mark trusted so GNOME does not show "Untrusted application launcher"
        let _ = gio::File::for_path(&dest).set_attribute_string(
            "metadata::trusted",
            "true",
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        );
        Some(dest)
    }

    /// Removes only shortcuts that AppNest created for this application.
    pub fn remove_user_desktop_shortcut(app: &AppImage, user_desktop_dir: Option<&Path>) {
        let Some(dir) = user_desktop_dir else { return };
        if let Ok(path) = Self::entry_path(app, dir) {
            if is_owned_entry(&path, &app.id) {
                let _ = FileSystem::remove_file_if_exists(&path);
            }
        }
        Self::remove_legacy(app, dir);
    }

    pub fn update_desktop_database(dir: &Path) {
        let _ = Command::new("update-desktop-database").arg(dir).output();
    }
}
