use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Update information embedded in an AppImage (AppImageSpec: the `.upd_info` ELF section).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateInfo {
    /// `gh-releases-zsync|<owner>|<repo>|<tag or "latest">|<zsync file name pattern>`
    GhReleasesZsync {
        owner: String,
        repo: String,
        tag: String,
        pattern: String,
    },
    /// `zsync|<url of the .zsync file>`
    Zsync { url: String },
    /// A transport AppNest does not support (for example `bintray-zsync`).
    Unsupported(String),
}

fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

fn is_pattern(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-*?".contains(&b))
}

impl UpdateInfo {
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        let parts: Vec<&str> = raw.split('|').collect();
        Some(match parts.as_slice() {
            ["gh-releases-zsync", owner, repo, tag, pattern]
                if is_name(owner) && is_name(repo) && is_name(tag) && is_pattern(pattern) =>
            {
                Self::GhReleasesZsync {
                    owner: owner.to_string(),
                    repo: repo.to_string(),
                    tag: tag.to_string(),
                    pattern: pattern.to_string(),
                }
            }
            ["zsync", url] if url.starts_with("https://") => Self::Zsync {
                url: url.to_string(),
            },
            [kind, ..] => Self::Unsupported(kind.to_string()),
            [] => return None,
        })
    }

    /// Reads the `.upd_info` section of an AppImage. Returns `None` if the file has no
    /// update information or is not a readable ELF file.
    pub fn read(path: &Path) -> Option<Self> {
        Self::parse(&read_upd_info_section(path)?)
    }
}

fn read_upd_info_section(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut h = [0u8; 64];
    file.read_exact(&mut h).ok()?;
    if &h[0..4] != b"\x7fELF" {
        return None;
    }
    let is_64 = h[4] == 2;
    let be = h[5] == 2;
    let u16_at = |b: &[u8]| -> u64 {
        let a = [b[0], b[1]];
        u64::from(if be {
            u16::from_be_bytes(a)
        } else {
            u16::from_le_bytes(a)
        })
    };
    let u32_at = |b: &[u8]| -> u64 {
        let a = [b[0], b[1], b[2], b[3]];
        u64::from(if be {
            u32::from_be_bytes(a)
        } else {
            u32::from_le_bytes(a)
        })
    };
    let u64_at = |b: &[u8]| -> u64 {
        let a = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
        if be {
            u64::from_be_bytes(a)
        } else {
            u64::from_le_bytes(a)
        }
    };

    let (shoff, shentsize, shnum, shstrndx) = if is_64 {
        (
            u64_at(&h[0x28..]),
            u16_at(&h[0x3A..]),
            u16_at(&h[0x3C..]),
            u16_at(&h[0x3E..]),
        )
    } else {
        (
            u32_at(&h[0x20..]),
            u16_at(&h[0x2E..]),
            u16_at(&h[0x30..]),
            u16_at(&h[0x32..]),
        )
    };
    let min_entry = if is_64 { 0x28 } else { 0x18 };
    if shnum == 0 || shnum > 4096 || shentsize < min_entry || shstrndx >= shnum {
        return None;
    }
    let table_len = shentsize * shnum;
    if shoff.checked_add(table_len)? > len {
        return None;
    }
    file.seek(SeekFrom::Start(shoff)).ok()?;
    let mut table = vec![0u8; table_len as usize];
    file.read_exact(&mut table).ok()?;

    // (name offset, file offset, size) of a section header
    let entry = |i: u64| -> (u64, u64, u64) {
        let e = &table[(i * shentsize) as usize..];
        if is_64 {
            (u32_at(&e[0..]), u64_at(&e[0x18..]), u64_at(&e[0x20..]))
        } else {
            (u32_at(&e[0..]), u32_at(&e[0x10..]), u32_at(&e[0x14..]))
        }
    };

    let (_, str_off, str_size) = entry(shstrndx);
    if str_size > 1 << 20 || str_off.checked_add(str_size)? > len {
        return None;
    }
    file.seek(SeekFrom::Start(str_off)).ok()?;
    let mut names = vec![0u8; str_size as usize];
    file.read_exact(&mut names).ok()?;

    for i in 0..shnum {
        let (name_off, off, size) = entry(i);
        let name = names.get(name_off as usize..)?;
        let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
        if name != b".upd_info" {
            continue;
        }
        let size = size.min(1024);
        if off.checked_add(size)? > len {
            return None;
        }
        file.seek(SeekFrom::Start(off)).ok()?;
        let mut data = vec![0u8; size as usize];
        file.read_exact(&mut data).ok()?;
        let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
        return std::str::from_utf8(&data[..end]).ok().map(str::to_string);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_releases() {
        assert_eq!(
            UpdateInfo::parse("gh-releases-zsync|owner|my-repo|latest|*x86_64.AppImage.zsync"),
            Some(UpdateInfo::GhReleasesZsync {
                owner: "owner".into(),
                repo: "my-repo".into(),
                tag: "latest".into(),
                pattern: "*x86_64.AppImage.zsync".into(),
            })
        );
    }

    #[test]
    fn parses_plain_zsync_and_rejects_insecure_or_odd_values() {
        assert_eq!(
            UpdateInfo::parse("zsync|https://example.org/app.AppImage.zsync"),
            Some(UpdateInfo::Zsync {
                url: "https://example.org/app.AppImage.zsync".into()
            })
        );
        assert!(matches!(
            UpdateInfo::parse("zsync|http://example.org/app.zsync"),
            Some(UpdateInfo::Unsupported(_))
        ));
        assert!(matches!(
            UpdateInfo::parse("bintray-zsync|a|b|c|d"),
            Some(UpdateInfo::Unsupported(_))
        ));
        assert!(matches!(
            UpdateInfo::parse("gh-releases-zsync|ow ner|repo|latest|*.zsync"),
            Some(UpdateInfo::Unsupported(_))
        ));
        assert_eq!(UpdateInfo::parse("   "), None);
    }
}
