use crate::domain::appimage::{AppImage, AppImageError};
use crate::domain::repository::AppImageRepository;
use crate::services::scanner::AppImageScanner;
use std::path::PathBuf;
use std::sync::Arc;

pub struct ListAppImagesQuery {
    repository: Arc<dyn AppImageRepository>,
}

#[derive(Debug, Clone)]
pub struct AppImagesListing {
    pub managed: Vec<AppImage>,
    pub unmanaged_discovered: Vec<PathBuf>,
}

impl ListAppImagesQuery {
    pub fn new(repository: Arc<dyn AppImageRepository>) -> Self {
        Self { repository }
    }

    pub fn execute(&self, scan_directories: &[PathBuf]) -> Result<AppImagesListing, AppImageError> {
        let managed = self.repository.list()?;
        let managed_paths: Vec<PathBuf> = managed.iter().map(|a| a.path.clone()).collect();

        // Scan directories for unmanaged AppImages
        let all_found = AppImageScanner::scan_multiple(scan_directories);
        let unmanaged_discovered = all_found
            .into_iter()
            .filter(|p| !managed_paths.contains(p))
            .collect();

        Ok(AppImagesListing {
            managed,
            unmanaged_discovered,
        })
    }
}
