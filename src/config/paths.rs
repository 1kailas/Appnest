use directories::{BaseDirs, ProjectDirs, UserDirs};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

pub const DESKTOP_FILE_PREFIX: &str = "appnest-";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub applications_dir: PathBuf,
    pub extracted_dir: PathBuf,
    pub icons_dir: PathBuf,
    pub database_file: PathBuf,
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub cache_dir: PathBuf,
    pub desktop_applications_dir: PathBuf,
    pub user_desktop_dir: Option<PathBuf>,
    pub download_dir: Option<PathBuf>,
    pub home_applications_dir: Option<PathBuf>,
}

impl Default for AppPaths {
    fn default() -> Self {
        Self::new()
    }
}

impl AppPaths {
    /// Resolves all locations from the XDG Base Directory and XDG user directory settings.
    /// Returns `None` when no home directory can be determined.
    pub fn try_new() -> Option<Self> {
        let base = BaseDirs::new()?;
        let proj = ProjectDirs::from("io", "github", "appnest")?;
        let data_home = base.data_local_dir().to_path_buf();

        // Migrate data and config from the pre-rename "appimage-manager" locations
        let migrate = |old: PathBuf, new: &Path| {
            if old.exists() && !new.exists() {
                let _ = std::fs::rename(&old, new);
            }
        };
        migrate(data_home.join("appimage-manager"), proj.data_local_dir());
        migrate(
            base.config_dir().join("appimage-manager"),
            proj.config_dir(),
        );

        let user_dirs = UserDirs::new();
        let mut paths = Self::from_dirs(
            &data_home,
            base.config_dir(),
            base.cache_dir(),
            user_dirs
                .as_ref()
                .and_then(|u| u.desktop_dir())
                .map(Path::to_path_buf)
                .filter(|p| p.is_dir()),
        );
        paths.download_dir = user_dirs
            .as_ref()
            .and_then(|u| u.download_dir())
            .map(Path::to_path_buf);
        paths.home_applications_dir = Some(base.home_dir().join("Applications"));
        Some(paths)
    }

    pub fn new() -> Self {
        Self::try_new().expect("could not determine the user's home directory")
    }

    /// Builds all paths from explicit roots. Used by `try_new` and by tests that must not
    /// touch the real home directory.
    pub fn from_dirs(
        data_home: &Path,
        config_home: &Path,
        cache_home: &Path,
        user_desktop_dir: Option<PathBuf>,
    ) -> Self {
        let data_dir = data_home.join("appnest");
        let config_dir = config_home.join("appnest");
        Self {
            applications_dir: data_dir.join("applications"),
            extracted_dir: data_dir.join("extracted"),
            icons_dir: data_dir.join("icons"),
            database_file: data_dir.join("apps.toml"),
            config_file: config_dir.join("config.toml"),
            cache_dir: cache_home.join("appnest"),
            desktop_applications_dir: data_home.join("applications"),
            data_dir,
            config_dir,
            user_desktop_dir,
            download_dir: None,
            home_applications_dir: None,
        }
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        let private = |p: &Path| {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(p)
        };
        for dir in [
            &self.data_dir,
            &self.applications_dir,
            &self.extracted_dir,
            &self.icons_dir,
            &self.config_dir,
            &self.cache_dir,
        ] {
            private(dir)?;
        }
        std::fs::create_dir_all(&self.desktop_applications_dir)
    }

    pub fn default_scan_directories(&self) -> Vec<PathBuf> {
        let mut list = vec![self.applications_dir.clone()];
        for dir in [&self.home_applications_dir, &self.download_dir]
            .into_iter()
            .flatten()
        {
            if dir.is_dir() && !list.contains(dir) {
                list.push(dir.clone());
            }
        }
        list
    }

    pub fn app_extracted_dir(&self, app_id: &str) -> PathBuf {
        self.extracted_dir.join(app_id)
    }

    pub fn app_icon_path(&self, app_id: &str, ext: &str) -> PathBuf {
        self.icons_dir
            .join(format!("{}.{}", app_id, ext.trim_start_matches('.')))
    }

    pub fn app_desktop_file(&self, app_id: &str) -> PathBuf {
        self.desktop_applications_dir
            .join(format!("{}{}.desktop", DESKTOP_FILE_PREFIX, app_id))
    }
}
