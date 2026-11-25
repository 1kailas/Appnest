mod common;

use appnest::domain::appimage::{AppImageError, AppImageType, Architecture};
use appnest::services::validator::AppImageValidator;
use common::*;
use std::fs;

fn invalid(res: Result<impl std::fmt::Debug, AppImageError>) -> bool {
    matches!(res, Err(AppImageError::InvalidAppImage(_)))
}

#[test]
fn nonexistent_file_is_not_found() {
    let res = AppImageValidator::validate("/nonexistent/file.AppImage");
    assert!(matches!(res, Err(AppImageError::NotFound(_))));
}

#[test]
fn text_file_is_rejected_even_with_appimage_extension() {
    let env = TestEnv::new();
    let p = env.root.path().join("fake.AppImage");
    fs::write(
        &p,
        "this is definitely not an appimage, just text padded out to more than 64 bytes",
    )
    .unwrap();
    assert!(invalid(AppImageValidator::validate(&p)));
}

#[test]
fn ordinary_elf_renamed_to_appimage_is_rejected() {
    let env = TestEnv::new();
    let p = env.root.path().join("renamed.AppImage");
    fs::copy("/bin/sh", &p).unwrap();
    assert!(invalid(AppImageValidator::validate(&p)));
}

#[test]
fn directory_is_rejected() {
    let env = TestEnv::new();
    let d = env.dir("dir.AppImage");
    assert!(invalid(AppImageValidator::validate(&d)));
}

#[test]
fn valid_type2_appimage_is_accepted_regardless_of_name() {
    let env = TestEnv::new();
    let p = simple_appimage(&env.root.path().join("no-extension"));
    let res = AppImageValidator::validate(&p).unwrap();
    assert_eq!(res.appimage_type, AppImageType::Type2);
    assert_ne!(res.architecture, Architecture::Unknown);
    assert!(res.payload_offset > 64);
}

#[test]
fn appimage_signature_without_filesystem_is_rejected() {
    let env = TestEnv::new();
    let p = env.root.path().join("nofs.AppImage");
    write_executable(&p, &appimage_bytes(b"not a filesystem at all.."));
    assert!(invalid(AppImageValidator::validate(&p)));
}

#[test]
fn truncated_appimage_is_rejected() {
    let env = TestEnv::new();
    let p = simple_appimage(&env.root.path().join("full.AppImage"));
    let full = fs::read(&p).unwrap();
    let offset = AppImageValidator::validate(&p).unwrap().payload_offset as usize;

    let cut = env.root.path().join("cut.AppImage");
    fs::write(&cut, &full[..offset + 2]).unwrap();
    assert!(invalid(AppImageValidator::validate(&cut)));

    let tiny = env.root.path().join("tiny.AppImage");
    fs::write(&tiny, &full[..20]).unwrap();
    assert!(invalid(AppImageValidator::validate(&tiny)));
}

#[test]
fn bad_elf_class_and_byte_order_are_rejected() {
    let env = TestEnv::new();
    let good = fs::read(simple_appimage(&env.root.path().join("good.AppImage"))).unwrap();

    for (index, label) in [(4usize, "class"), (5, "byte order")] {
        let mut bytes = good.clone();
        bytes[index] = 9;
        let p = env
            .root
            .path()
            .join(format!("bad-{}.AppImage", label.replace(' ', "-")));
        fs::write(&p, bytes).unwrap();
        assert!(invalid(AppImageValidator::validate(&p)), "{}", label);
    }
}

#[test]
fn unsupported_architecture_is_rejected() {
    let env = TestEnv::new();
    let mut bytes = fs::read(simple_appimage(&env.root.path().join("good.AppImage"))).unwrap();
    bytes[18..20].copy_from_slice(&243u16.to_le_bytes()); // RISC-V
    let p = env.root.path().join("riscv.AppImage");
    fs::write(&p, bytes).unwrap();
    let err = AppImageValidator::validate(&p).unwrap_err();
    assert!(
        err.to_string().contains("Unsupported CPU architecture"),
        "{}",
        err
    );
}

#[test]
fn unknown_appimage_type_is_rejected() {
    let env = TestEnv::new();
    let mut bytes = fs::read(simple_appimage(&env.root.path().join("good.AppImage"))).unwrap();
    bytes[10] = 3;
    let p = env.root.path().join("type3.AppImage");
    fs::write(&p, bytes).unwrap();
    assert!(invalid(AppImageValidator::validate(&p)));
}

#[test]
fn valid_type1_appimage_is_accepted() {
    let env = TestEnv::new();
    let mut bytes = fs::read(simple_appimage(&env.root.path().join("good.AppImage"))).unwrap();
    bytes[10] = 1;
    bytes.resize(0x8100, 0);
    bytes[0x8001..0x8006].copy_from_slice(b"CD001");
    let p = env.root.path().join("type1.AppImage");
    fs::write(&p, bytes).unwrap();
    assert_eq!(
        AppImageValidator::validate(&p).unwrap().appimage_type,
        AppImageType::Type1
    );
}
