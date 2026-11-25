#![allow(dead_code)]
//! Shared fixtures. Nothing here touches the real home directory: every test works inside
//! its own temporary directory tree.

use appnest::config::paths::AppPaths;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

pub const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0,
    0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

pub struct TestEnv {
    pub root: TempDir,
    pub paths: AppPaths,
}

impl TestEnv {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let desktop = root.path().join("Desktop");
        fs::create_dir_all(&desktop).unwrap();
        let paths = AppPaths::from_dirs(
            &root.path().join("data"),
            &root.path().join("config"),
            &root.path().join("cache"),
            Some(desktop),
        );
        paths.ensure_dirs().unwrap();
        Self { root, paths }
    }

    pub fn dir(&self, name: &str) -> PathBuf {
        let d = self.root.path().join(name);
        fs::create_dir_all(&d).unwrap();
        d
    }
}

pub fn have_tool(cmd: &str) -> bool {
    Command::new(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

pub fn can_extract() -> bool {
    have_tool("mksquashfs") && (have_tool("7z") || have_tool("7zz") || have_tool("7za"))
}

/// Offset just past the ELF section header table, where an AppImage's filesystem starts.
fn payload_offset(elf: &[u8]) -> usize {
    let shoff = u64::from_le_bytes(elf[0x28..0x30].try_into().unwrap()) as usize;
    let shentsize = u16::from_le_bytes(elf[0x3A..0x3C].try_into().unwrap()) as usize;
    let shnum = u16::from_le_bytes(elf[0x3C..0x3E].try_into().unwrap()) as usize;
    shoff + shentsize * shnum
}

/// A real, runnable ELF (a copy of /usr/bin/env) carrying the AppImage type-2 signature,
/// followed by `payload`. Running it behaves like `env`, so tests can drive its behaviour with
/// arguments such as `sh -c "..."`.
pub fn appimage_bytes(payload: &[u8]) -> Vec<u8> {
    let mut elf = fs::read("/usr/bin/env").expect("/usr/bin/env must exist");
    assert_eq!(&elf[0..4], b"\x7fELF");
    assert_eq!(elf[4], 2, "tests need a 64-bit little-endian /usr/bin/env");
    let offset = payload_offset(&elf);
    elf.truncate(offset);
    // Distribution binaries can embed compressed data (for example .gnu_debugdata). Break
    // those signatures so 7-Zip does not mistake the stub for an archive; real AppImage
    // runtimes do not contain any.
    for magic in [
        &b"\xfd7zXZ\x00"[..],
        b"PK\x03\x04",
        b"7z\xbc\xaf\x27\x1c",
        b"hsqs",
    ] {
        let mut from = 0;
        while let Some(pos) = elf[from..].windows(magic.len()).position(|w| w == magic) {
            elf[from + pos] ^= 0xff;
            from += pos + magic.len();
        }
    }
    elf[8..11].copy_from_slice(b"AI\x02");
    elf.extend_from_slice(payload);
    elf
}

pub fn write_executable(path: &Path, bytes: &[u8]) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// An AppImage whose "filesystem" is only the SquashFS magic. Enough for validation and
/// for running the stub, not for extraction.
pub fn simple_appimage(path: &Path) -> PathBuf {
    let mut payload = b"hsqs".to_vec();
    payload.resize(256, 0);
    write_executable(path, &appimage_bytes(&payload));
    path.to_path_buf()
}

/// Like `simple_appimage` but with distinct contents, so two files differ.
pub fn appimage_with_marker(path: &Path, marker: &str) -> PathBuf {
    let mut payload = b"hsqs".to_vec();
    payload.extend_from_slice(marker.as_bytes());
    payload.resize(256, 0);
    write_executable(path, &appimage_bytes(&payload));
    path.to_path_buf()
}

pub fn squashfs_of(dir: &Path) -> Vec<u8> {
    let out = tempfile::NamedTempFile::new().unwrap();
    let status = Command::new("mksquashfs")
        .arg(dir)
        .arg(out.path())
        .args(["-noappend", "-quiet", "-all-root"])
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "mksquashfs failed");
    fs::read(out.path()).unwrap()
}

/// Builds an AppImage with a real SquashFS AppDir. `setup` fills the AppDir.
pub fn appdir_appimage(path: &Path, setup: impl FnOnce(&Path)) -> PathBuf {
    let appdir = tempfile::tempdir().unwrap();
    setup(appdir.path());
    write_executable(path, &appimage_bytes(&squashfs_of(appdir.path())));
    path.to_path_buf()
}

pub fn script(path: &Path, body: &str) {
    write_executable(path, format!("#!/bin/sh\n{}\n", body).as_bytes());
}

/// An AppImage with a hand-built ELF that has a genuine `.upd_info` section (as written by
/// appimagetool), followed by a minimal "filesystem". It is a valid AppImage as far as
/// AppNest's validator is concerned, but it is not executable.
pub fn upd_info_appimage(path: &Path, info: &str) -> PathBuf {
    let shstrtab = b"\0.shstrtab\0.upd_info\0";
    let str_off = 64u64;
    let upd_off = str_off + shstrtab.len() as u64;
    let table_off = upd_off + 1024;

    let mut elf = vec![0u8; 64];
    elf[0..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2; // 64-bit
    elf[5] = 1; // little endian
    elf[6] = 1;
    elf[8..11].copy_from_slice(b"AI\x02");
    elf[16..18].copy_from_slice(&2u16.to_le_bytes());
    elf[18..20].copy_from_slice(&62u16.to_le_bytes());
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf[40..48].copy_from_slice(&table_off.to_le_bytes());
    elf[52..54].copy_from_slice(&64u16.to_le_bytes());
    elf[58..60].copy_from_slice(&64u16.to_le_bytes());
    elf[60..62].copy_from_slice(&3u16.to_le_bytes());
    elf[62..64].copy_from_slice(&1u16.to_le_bytes());

    elf.extend_from_slice(shstrtab);
    let mut section = info.as_bytes().to_vec();
    section.resize(1024, 0);
    elf.extend_from_slice(&section);

    let header = |name: u32, ty: u32, off: u64, size: u64| {
        let mut h = vec![0u8; 64];
        h[0..4].copy_from_slice(&name.to_le_bytes());
        h[4..8].copy_from_slice(&ty.to_le_bytes());
        h[24..32].copy_from_slice(&off.to_le_bytes());
        h[32..40].copy_from_slice(&size.to_le_bytes());
        h
    };
    elf.extend_from_slice(&[0u8; 64]);
    elf.extend_from_slice(&header(1, 3, str_off, shstrtab.len() as u64));
    elf.extend_from_slice(&header(11, 1, upd_off, 1024));
    elf.extend_from_slice(b"hsqs");
    elf.resize(elf.len() + 256, 0);
    write_executable(path, &elf);
    path.to_path_buf()
}

/// A minimal HTTP server on the loopback interface for update tests.
pub struct TestServer {
    pub base: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl TestServer {
    /// `routes` maps a request path to a response body; anything else is a 404.
    pub fn start(
        routes: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>>,
    ) -> Self {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop2 = stop.clone();
        let handle = std::thread::spawn(move || {
            while !stop2.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut conn, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                };
                conn.set_nonblocking(false).unwrap();
                let mut buf = [0u8; 4096];
                let n = conn.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                let body = routes.lock().unwrap().get(&path).cloned();
                let (status, body) = match body {
                    Some(b) => ("200 OK", b),
                    None => ("404 Not Found", b"not found".to_vec()),
                };
                let _ = write!(
                    conn,
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    status,
                    body.len()
                );
                let _ = conn.write_all(&body);
            }
        });
        Self {
            base,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
