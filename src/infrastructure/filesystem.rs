use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

pub struct FileSystem;

impl FileSystem {
    pub fn make_executable<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
        let metadata = fs::metadata(&path)?;
        let mut perms = metadata.permissions();
        perms.set_mode(perms.mode() | 0o755);
        fs::set_permissions(&path, perms)
    }

    /// Copies `source` into `dest_dir` under a name that does not exist yet.
    /// The destination is claimed with `O_EXCL`, so an existing file or symlink is never
    /// written through or overwritten. Returns the final path.
    pub fn copy_to_unique_file(
        source: &Path,
        dest_dir: &Path,
        file_name: &str,
    ) -> std::io::Result<PathBuf> {
        fs::create_dir_all(dest_dir)?;
        let (stem, ext) = match file_name.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{}", e)),
            _ => (file_name.to_string(), String::new()),
        };
        for n in 1..1000 {
            let candidate = if n == 1 {
                file_name.to_string()
            } else {
                format!("{}-{}{}", stem, n, ext)
            };
            let dest = dest_dir.join(&candidate);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o755)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&dest)
            {
                Ok(mut out) => {
                    let result = File::open(source).and_then(|mut src| {
                        std::io::copy(&mut src, &mut out)?;
                        out.sync_all()
                    });
                    if let Err(e) = result {
                        let _ = fs::remove_file(&dest);
                        return Err(e);
                    }
                    return Ok(dest);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not find a free file name",
        ))
    }

    /// True when both paths are regular files with identical contents.
    pub fn files_identical(a: &Path, b: &Path) -> bool {
        let (Ok(ma), Ok(mb)) = (fs::metadata(a), fs::metadata(b)) else {
            return false;
        };
        if !ma.is_file() || !mb.is_file() || ma.len() != mb.len() {
            return false;
        }
        if ma.dev() == mb.dev() && ma.ino() == mb.ino() {
            return true;
        }
        let (Ok(mut fa), Ok(mut fb)) = (File::open(a), File::open(b)) else {
            return false;
        };
        let (mut ba, mut bb) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
        loop {
            let Ok(na) = std::io::Read::read(&mut fa, &mut ba) else {
                return false;
            };
            if na == 0 {
                return true;
            }
            if std::io::Read::read_exact(&mut fb, &mut bb[..na]).is_err() || ba[..na] != bb[..na] {
                return false;
            }
        }
    }

    /// True when `path` lives inside `root` (symlinks in the parent directory are resolved).
    /// The final component is not followed, so a symlink inside `root` counts as inside.
    pub fn is_inside(root: &Path, path: &Path) -> bool {
        let (Ok(root), Some(parent), Some(name)) =
            (fs::canonicalize(root), path.parent(), path.file_name())
        else {
            return false;
        };
        match fs::canonicalize(parent) {
            Ok(parent) => name != ".." && parent.starts_with(&root),
            Err(_) => false,
        }
    }

    /// Resolves `.` and `..` lexically. Returns `None` if the path would climb above its start.
    pub fn normalize_relative(path: &Path) -> Option<PathBuf> {
        let mut out = PathBuf::new();
        for comp in path.components() {
            match comp {
                Component::Normal(c) => out.push(c),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() {
                        return None;
                    }
                }
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        Some(out)
    }

    pub fn remove_file_if_exists<P: AsRef<Path>>(path: P) -> std::io::Result<bool> {
        match fs::remove_file(path.as_ref()) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub fn remove_dir_all_if_exists<P: AsRef<Path>>(path: P) -> std::io::Result<bool> {
        let p = path.as_ref();
        match fs::symlink_metadata(p) {
            Ok(m) if m.file_type().is_symlink() => fs::remove_file(p).map(|_| true),
            Ok(_) => fs::remove_dir_all(p).map(|_| true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Writes `content` to a uniquely named temporary file next to `path` and atomically
    /// renames it into place. An existing symlink at `path` is replaced, never written through.
    pub fn atomic_write_with_mode<P: AsRef<Path>>(
        path: P,
        content: &str,
        mode: u32,
    ) -> std::io::Result<()> {
        Self::atomic_write_bytes(path, content.as_bytes(), mode)
    }

    pub fn atomic_write_bytes<P: AsRef<Path>>(
        path: P,
        content: &[u8],
        mode: u32,
    ) -> std::io::Result<()> {
        let p = path.as_ref();
        let parent = p.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let mut tmp = tempfile::Builder::new()
            .prefix(".appnest-tmp-")
            .tempfile_in(parent)?;
        tmp.write_all(content)?;
        tmp.as_file().sync_all()?;
        fs::set_permissions(tmp.path(), fs::Permissions::from_mode(mode))?;
        tmp.persist(p).map_err(|e| e.error)?;
        Ok(())
    }

    pub fn atomic_write<P: AsRef<Path>>(path: P, content: &str) -> std::io::Result<()> {
        Self::atomic_write_with_mode(path, content, 0o644)
    }

    pub fn atomic_write_private<P: AsRef<Path>>(path: P, content: &str) -> std::io::Result<()> {
        Self::atomic_write_with_mode(path, content, 0o600)
    }
}
