use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppImageError {
    #[error("AppImage file not found: {0}")]
    NotFound(PathBuf),

    #[error("File is not a valid AppImage or executable: {0}")]
    InvalidAppImage(String),

    #[error("Failed to extract AppImage contents: {0}")]
    ExtractionError(String),

    #[error("Desktop entry integration error: {0}")]
    DesktopEntryError(String),

    #[error("Failed to launch AppImage: {0}")]
    LaunchError(String),

    #[error("Repository or storage error: {0}")]
    StorageError(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),
}
