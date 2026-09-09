use crate::domain::appimage::{AppImage, AppImageError};
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub struct AppImageUpdater;

impl AppImageUpdater {
    pub fn read_update_information<P: AsRef<Path>>(path: P) -> Option<String> {
        let mut file = File::open(path).ok()?;
        let mut buffer = Vec::new();
        let _ = file.read_to_end(&mut buffer);

        // Search for AppImage update string signature (e.g. zsync|gh-releases-zsync)
        let needles = ["zsync|", "gh-releases-zsync|", "bintray-zsync|"];
        for needle in &needles {
            if let Some(pos) = buffer
                .windows(needle.len())
                .position(|w| w == needle.as_bytes())
            {
                let end = buffer[pos..]
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(buffer.len() - pos);
                if let Ok(info) = std::str::from_utf8(&buffer[pos..pos + end]) {
                    return Some(info.to_string());
                }
            }
        }
        None
    }

    pub fn check_update_available(_app: &AppImage) -> Result<bool, AppImageError> {
        // Extensible hook for remote version checking / zsync
        Ok(false)
    }
}
