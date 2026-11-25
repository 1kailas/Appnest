use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

fn stream_digest<D: Digest>(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = D::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect())
}

/// Computes the SHA-256 of a file by streaming it, without loading it into memory.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    stream_digest::<Sha256>(path)
}

/// SHA-1, only used to compare against the checksum in AppImage update metadata (zsync).
pub fn sha1_file(path: &Path) -> std::io::Result<String> {
    stream_digest::<Sha1>(path)
}
