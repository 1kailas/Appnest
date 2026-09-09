use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub struct FileSystem;

impl FileSystem {
    pub fn make_executable<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
        let metadata = fs::metadata(&path)?;
        let mut perms = metadata.permissions();
        let mode = perms.mode() | 0o755;
        perms.set_mode(mode);
        fs::set_permissions(&path, perms)?;
        Ok(())
    }

    pub fn copy_file<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> std::io::Result<u64> {
        if let Some(parent) = to.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = fs::copy(&from, &to)?;
        let _ = Self::make_executable(&to);
        Ok(bytes)
    }

    pub fn move_file<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> std::io::Result<()> {
        if let Some(parent) = to.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::rename(&from, &to).is_err() {
            fs::copy(&from, &to)?;
            fs::remove_file(&from)?;
        }
        let _ = Self::make_executable(&to);
        Ok(())
    }

    pub fn remove_file_if_exists<P: AsRef<Path>>(path: P) -> std::io::Result<bool> {
        let p = path.as_ref();
        if p.exists() {
            fs::remove_file(p)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn remove_dir_all_if_exists<P: AsRef<Path>>(path: P) -> std::io::Result<bool> {
        let p = path.as_ref();
        if p.exists() {
            fs::remove_dir_all(p)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn atomic_write<P: AsRef<Path>>(path: P, content: &str) -> std::io::Result<()> {
        let p = path.as_ref();
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp_path = p.with_extension("tmp_write");
        {
            let mut file = File::create(&tmp_path)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(tmp_path, p)?;
        Ok(())
    }
}
