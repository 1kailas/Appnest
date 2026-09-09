use appnest::domain::appimage::{AppImageError, AppImageType, Architecture};
use appnest::services::validator::AppImageValidator;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_validator_nonexistent() {
    let result = AppImageValidator::validate("/path/to/definitely/nonexistent.appimage");
    assert!(matches!(result, Err(AppImageError::NotFound(_))));
}

#[test]
fn test_validator_invalid_file() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"Hello this is not an elf binary at all")
        .unwrap();

    let result = AppImageValidator::validate(file.path());
    assert!(matches!(result, Err(AppImageError::InvalidAppImage(_))));
}

#[test]
fn test_validator_valid_elf_appimage_type2() {
    let mut file = NamedTempFile::new().unwrap();
    let mut header = vec![0u8; 128];
    // ELF magic
    header[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    // AppImage magic at offset 8: 'A' 'I' \x02
    header[8] = b'A';
    header[9] = b'I';
    header[10] = 0x02;
    // Architecture x86_64: 62 (0x3E) little endian at 18..20
    header[18] = 0x3E;
    header[19] = 0x00;

    file.write_all(&header).unwrap();

    let result = AppImageValidator::validate(file.path()).unwrap();
    assert_eq!(result.appimage_type, AppImageType::Type2);
    assert_eq!(result.architecture, Architecture::X86_64);
    assert_eq!(result.file_size, 128);
}

#[test]
fn test_validator_valid_elf_appimage_type1() {
    let mut file = NamedTempFile::new().unwrap();
    let mut header = vec![0u8; 128];
    // ELF magic
    header[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    // AppImage magic at offset 8: 'A' 'I' \x01
    header[8] = b'A';
    header[9] = b'I';
    header[10] = 0x01;
    // Architecture AArch64: 183 (0xB7)
    header[18] = 0xB7;
    header[19] = 0x00;

    file.write_all(&header).unwrap();

    let result = AppImageValidator::validate(file.path()).unwrap();
    assert_eq!(result.appimage_type, AppImageType::Type1);
    assert_eq!(result.architecture, Architecture::Aarch64);
}
