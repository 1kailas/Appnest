use crate::domain::appimage::{AppImageError, AppImageType, Architecture};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    pub appimage_type: AppImageType,
    pub architecture: Architecture,
    pub file_size: u64,
    /// Offset of the embedded filesystem (SquashFS for Type 2, ISO 9660 for Type 1).
    pub payload_offset: u64,
}

pub struct AppImageValidator;

struct Reader {
    big_endian: bool,
}

impl Reader {
    fn u16(&self, b: &[u8]) -> u64 {
        let a = [b[0], b[1]];
        u64::from(if self.big_endian {
            u16::from_be_bytes(a)
        } else {
            u16::from_le_bytes(a)
        })
    }

    fn u32(&self, b: &[u8]) -> u64 {
        let a = [b[0], b[1], b[2], b[3]];
        u64::from(if self.big_endian {
            u32::from_be_bytes(a)
        } else {
            u32::from_le_bytes(a)
        })
    }

    fn u64(&self, b: &[u8]) -> u64 {
        let a = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
        if self.big_endian {
            u64::from_be_bytes(a)
        } else {
            u64::from_le_bytes(a)
        }
    }
}

fn invalid(msg: impl Into<String>) -> AppImageError {
    AppImageError::InvalidAppImage(msg.into())
}

impl AppImageValidator {
    /// Validates that `path` is a genuine AppImage: an ELF executable for a supported
    /// architecture, carrying the AppImage signature, whose embedded filesystem is present.
    /// The file name is never used as evidence.
    pub fn validate<P: AsRef<Path>>(path: P) -> Result<ValidationResult, AppImageError> {
        let p = path.as_ref();
        let metadata = std::fs::metadata(p).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => AppImageError::NotFound(p.to_path_buf()),
            _ => AppImageError::Io(e),
        })?;
        if !metadata.is_file() {
            return Err(invalid("Not a regular file"));
        }
        let file_size = metadata.len();
        if file_size < 64 {
            return Err(invalid(format!("File too small ({} bytes)", file_size)));
        }

        let mut file = File::open(p)?;
        let mut h = [0u8; 64];
        file.read_exact(&mut h)?;

        if h[0..4] != [0x7F, b'E', b'L', b'F'] {
            return Err(invalid("Not an ELF executable"));
        }
        let is_64 = match h[4] {
            1 => false,
            2 => true,
            _ => return Err(invalid("Invalid ELF class")),
        };
        let r = Reader {
            big_endian: match h[5] {
                1 => false,
                2 => true,
                _ => return Err(invalid("Invalid ELF byte order")),
            },
        };

        let architecture = match r.u16(&h[18..20]) {
            62 => Architecture::X86_64,
            183 => Architecture::Aarch64,
            40 => Architecture::Armhf,
            3 => Architecture::I686,
            other => {
                return Err(invalid(format!(
                    "Unsupported CPU architecture (ELF machine {})",
                    other
                )))
            }
        };

        if h[8] != b'A' || h[9] != b'I' {
            return Err(invalid("Not an AppImage (missing AppImage signature)"));
        }
        let appimage_type = match h[10] {
            1 => AppImageType::Type1,
            2 => AppImageType::Type2,
            _ => return Err(invalid("Unsupported AppImage type")),
        };

        let (shoff, shentsize, shnum) = if is_64 {
            (
                r.u64(&h[0x28..0x30]),
                r.u16(&h[0x3A..0x3C]),
                r.u16(&h[0x3C..0x3E]),
            )
        } else {
            (
                r.u32(&h[0x20..0x24]),
                r.u16(&h[0x2E..0x30]),
                r.u16(&h[0x30..0x32]),
            )
        };
        let payload_offset = shentsize
            .checked_mul(shnum)
            .and_then(|t| t.checked_add(shoff))
            .ok_or_else(|| invalid("Corrupt ELF section table"))?;

        let mut magic = [0u8; 5];
        let magic_at = match appimage_type {
            AppImageType::Type1 => 0x8001,
            _ => payload_offset,
        };
        let truncated = || invalid("File is truncated or corrupted");
        if magic_at.checked_add(5).is_none_or(|end| end > file_size) {
            return Err(truncated());
        }
        file.seek(SeekFrom::Start(magic_at))?;
        file.read_exact(&mut magic)?;
        let fs_ok = match appimage_type {
            AppImageType::Type1 => &magic == b"CD001",
            _ => magic[..4] == *b"hsqs" || magic[..4] == *b"sqsh",
        };
        if !fs_ok {
            return Err(invalid(
                "Embedded filesystem not found (corrupted AppImage)",
            ));
        }

        Ok(ValidationResult {
            appimage_type,
            architecture,
            file_size,
            payload_offset,
        })
    }
}
