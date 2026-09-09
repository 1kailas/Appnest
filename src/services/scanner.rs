use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct AppImageScanner;

impl AppImageScanner {
    pub fn scan_directory(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
        if !dir.exists() || !dir.is_dir() {
            return Vec::new();
        }

        let mut results = Vec::new();
        for entry in WalkDir::new(dir).max_depth(max_depth).into_iter().flatten() {
            if entry.file_type().is_file() {
                let path = entry.path();
                if let Some(ext) = path.extension() {
                    if ext.to_string_lossy().eq_ignore_ascii_case("appimage") {
                        results.push(path.to_path_buf());
                    }
                }
            }
        }
        results
    }

    pub fn scan_multiple(directories: &[PathBuf]) -> Vec<PathBuf> {
        let mut results = Vec::new();
        for dir in directories {
            let found = Self::scan_directory(dir, 2);
            for p in found {
                if !results.contains(&p) {
                    results.push(p);
                }
            }
        }
        results
    }
}
