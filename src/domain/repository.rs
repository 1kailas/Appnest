use crate::domain::appimage::{AppImage, AppImageError};

pub trait AppImageRepository: Send + Sync {
    fn list(&self) -> Result<Vec<AppImage>, AppImageError>;
    fn get_by_id(&self, id: &str) -> Result<Option<AppImage>, AppImageError>;
    fn save(&self, app: &AppImage) -> Result<(), AppImageError>;
    fn remove(&self, id: &str) -> Result<(), AppImageError>;
}
