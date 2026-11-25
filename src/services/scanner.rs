use crate::services::validator::AppImageValidator;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct AppImageScanner;

impl AppImageScanner {
    /// Finds valid AppImages below `dir`. The extension only selects candidates; every
    /// candidate must also pass full validation.
    pub fn scan_directory(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
        if !dir.is_dir() {
            return Vec::new();
        }

        WalkDir::new(dir)
            .max_depth(max_depth)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .filter(|p| {
                p.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("appimage"))
                    && AppImageValidator::validate(p).is_ok()
            })
            .collect()
    }

    pub fn scan_multiple(directories: &[PathBuf]) -> Vec<PathBuf> {
        let mut seen = std::collections::HashSet::new();
        let mut results = Vec::new();
        for dir in directories {
            for p in Self::scan_directory(dir, 2) {
                if seen.insert(p.clone()) {
                    results.push(p);
                }
            }
        }
        results
    }
}
