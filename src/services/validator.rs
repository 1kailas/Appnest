use crate::domain::appimage::{AppImageError, AppImageType, Architecture};
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    pub appimage_type: AppImageType,
    pub architecture: Architecture,
    pub file_size: u64,
}

pub struct AppImageValidator;

impl AppImageValidator {
    pub fn validate<P: AsRef<Path>>(path: P) -> Result<ValidationResult, AppImageError> {
        let p = path.as_ref();
        if !p.exists() {
            return Err(AppImageError::NotFound(p.to_path_buf()));
        }

        let metadata = std::fs::metadata(p)?;
        let file_size = metadata.len();
        if file_size < 64 {
            return Err(AppImageError::InvalidAppImage(format!(
                "File too small ({} bytes)",
                file_size
            )));
        }

        let mut file = File::open(p)?;
        let mut header = [0u8; 64];
        file.read_exact(&mut header)?;

        // Check ELF magic: 0x7F 'E' 'L' 'F'
        if header[0..4] != [0x7F, b'E', b'L', b'F'] {
            return Err(AppImageError::InvalidAppImage(
                "File is not a valid ELF binary".to_string(),
            ));
        }

        // Architecture check from e_machine (little endian bytes 18..20)
        let e_machine = u16::from_le_bytes([header[18], header[19]]);
        let architecture = match e_machine {
            62 => Architecture::X86_64,
            183 => Architecture::Aarch64,
            40 => Architecture::Armhf,
            3 => Architecture::I686,
            _ => Architecture::Unknown,
        };

        // Check AppImage magic at offset 8..11: 'A' 'I' \x01 or 'A' 'I' \x02
        let appimage_type = if header[8] == b'A' && header[9] == b'I' {
            match header[10] {
                0x01 => AppImageType::Type1,
                0x02 => AppImageType::Type2,
                _ => AppImageType::Unknown,
            }
        } else {
            // Check if squashfs signature appears or if it has AppImage extension
            if p.extension()
                .map_or(false, |ext| ext.eq_ignore_ascii_case("appimage"))
            {
                AppImageType::Type2
            } else {
                AppImageType::Unknown
            }
        };

        Ok(ValidationResult {
            appimage_type,
            architecture,
            file_size,
        })
    }
}
