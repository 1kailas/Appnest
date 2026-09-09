use crate::domain::appimage::{AppImage, AppImageError};
use crate::domain::repository::AppImageRepository;
use std::sync::Arc;

pub struct GetAppImageQuery {
    repository: Arc<dyn AppImageRepository>,
}

impl GetAppImageQuery {
    pub fn new(repository: Arc<dyn AppImageRepository>) -> Self {
        Self { repository }
    }

    pub fn execute(&self, id: &str) -> Result<Option<AppImage>, AppImageError> {
        self.repository.get_by_id(id)
    }
}
