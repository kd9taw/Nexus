//! The fonts and icons the street map draws with: one archive, fetched once with the first
//! pack, checked against the SHA-256 the host's index lists, and unpacked into `assets/`.
//!
//! **The archive format** (defined here; the hosting job builds it): a gzip-compressed POSIX
//! ustar archive, as `tar --format=ustar -czf` writes it, holding regular files and folders
//! only, laid out exactly as the webview asks for them through `street_map_asset`:
//!
//! ```text
//! OFL.txt                          the fonts' licence (required)
//! ICONS-LICENSE.txt                the icons' MIT licence (required)
//! fonts/<font stack>/<range>.pbf   glyph ranges: fonts/Noto Sans Regular/0-255.pbf
//! sprites/<name>.json | .png       sprite sheets: sprites/light.json, sprites/light@2x.png
//! ```
//!
//! Every name must pass the same rule as a webview asset path
//! ([`crate::store::validate_asset_path`]), so nothing in the archive can land outside
//! `assets/`. Links, devices and every other entry type are refused, as are duplicate names,
//! more than [`MAX_FILES`] files and more than [`MAX_UNPACKED`] bytes. The two licence files
//! are required so the notices always travel with the fonts and icons.
//!
//! Unpacking goes to a fresh folder beside `assets/`, which then replaces it by rename, so a
//! failed or interrupted install leaves the previous assets untouched.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use ring::digest::{digest, SHA256};

use crate::error::StreetError;
use crate::plan::hex;
use crate::pmtiles::gunzip;
use crate::store::{validate_asset_path, MapsDir};

/// The largest archive accepted (the real one is about 12 MB).
pub const MAX_ARCHIVE: u64 = 64 << 20;
pub const MAX_UNPACKED: u64 = 128 << 20;
pub const MAX_FILES: usize = 20_000;
pub const REQUIRED: [&str; 2] = ["OFL.txt", "ICONS-LICENSE.txt"];

const BLOCK: usize = 512;

pub fn sha256_hex(b: &[u8]) -> String {
    hex(digest(&SHA256, b).as_ref())
}

fn bad(why: impl Into<String>) -> StreetError {
    StreetError::archive(format!("font and icon archive: {}", why.into()))
}

/// A NUL-terminated (or full-width) header field.
fn field(b: &[u8]) -> &[u8] {
    b.iter().position(|&c| c == 0).map_or(b, |n| &b[..n])
}

/// An octal header number (spaces and NULs around it allowed, as tar writes them).
fn octal(b: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(field(b)).ok()?.trim();
    if s.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(s, 8).ok()
}

/// The regular files of a ustar archive, as (path, bytes).
pub(crate) fn untar(t: &[u8]) -> Result<Vec<(String, Vec<u8>)>, StreetError> {
    let mut files = Vec::new();
    let mut names = HashSet::new();
    let mut total = 0u64;
    let mut pos = 0usize;
    loop {
        let Some(h) = t.get(pos..pos + BLOCK) else {
            return Err(bad("it ends without the end-of-archive marker"));
        };
        if h.iter().all(|&b| b == 0) {
            break;
        }
        // The checksum is the byte sum of the header with its own field read as spaces.
        let stored = octal(&h[148..156]).ok_or_else(|| bad("a header checksum is not a number"))?;
        let sum: u64 = h
            .iter()
            .enumerate()
            .map(|(i, &b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    u64::from(b)
                }
            })
            .sum();
        if stored != sum {
            return Err(bad("a header fails its checksum"));
        }
        if &h[257..262] != b"ustar" {
            return Err(bad("not a ustar archive"));
        }
        let name =
            String::from_utf8(field(&h[..100]).to_vec()).map_err(|_| bad("a name is not UTF-8"))?;
        let prefix = String::from_utf8(field(&h[345..500]).to_vec())
            .map_err(|_| bad("a name is not UTF-8"))?;
        let full = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let size = octal(&h[124..136]).ok_or_else(|| bad("a size is not a number"))?;
        pos += BLOCK;
        let end = usize::try_from(size)
            .ok()
            .and_then(|n| pos.checked_add(n))
            .filter(|&end| end <= t.len())
            .ok_or_else(|| bad(format!("{full} runs past the end of the archive")))?;
        let data = &t[pos..end];
        pos = end.div_ceil(BLOCK) * BLOCK;

        let path = full
            .strip_prefix("./")
            .unwrap_or(&full)
            .trim_end_matches('/');
        match h[156] {
            b'0' | 0 => {
                validate_asset_path(path).map_err(|e| bad(format!("{path:?}: {}", e.message)))?;
                if !names.insert(path.to_string()) {
                    return Err(bad(format!("{path} appears twice")));
                }
                total += size;
                if total > MAX_UNPACKED || names.len() > MAX_FILES {
                    return Err(bad("it unpacks too large"));
                }
                files.push((path.to_string(), data.to_vec()));
            }
            b'5' if path.is_empty() || path == "." => {}
            b'5' => {
                validate_asset_path(path).map_err(|e| bad(format!("{path:?}: {}", e.message)))?;
            }
            t => return Err(bad(format!("{path} is entry type {:?}", t as char))),
        }
    }
    Ok(files)
}

fn write_tree(root: &Path, files: &[(String, Vec<u8>)]) -> Result<(), StreetError> {
    for (path, data) in files {
        let mut p = root.to_path_buf();
        p.extend(path.split('/'));
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(|e| StreetError::io("unpack the map fonts", &e))?;
        }
        fs::write(&p, data).map_err(|e| StreetError::io("unpack the map fonts", &e))?;
    }
    Ok(())
}

/// Check `archive` against `expected_sha256`, unpack it and put it in place as `assets/`.
pub fn install(dir: &MapsDir, archive: &[u8], expected_sha256: &str) -> Result<(), StreetError> {
    let got = sha256_hex(archive);
    if got != expected_sha256 {
        return Err(bad(format!(
            "its SHA-256 is {got} and the host's index lists {expected_sha256}"
        )));
    }
    let tar = gunzip(archive, MAX_UNPACKED + (MAX_FILES * BLOCK * 2) as u64).map_err(bad)?;
    let files = untar(&tar)?;
    for required in REQUIRED {
        if !files.iter().any(|(p, _)| p == required) {
            return Err(bad(format!("it has no {required}")));
        }
    }

    let pid = std::process::id();
    let staging: PathBuf = dir.root().join(format!("assets.new-{pid}"));
    let old: PathBuf = dir.root().join(format!("assets.old-{pid}"));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| StreetError::io("unpack the map fonts", &e))?;
    if let Err(e) = write_tree(&staging, &files) {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }
    let live = dir.assets();
    let had_old = live.exists();
    if had_old {
        let _ = fs::remove_dir_all(&old);
        fs::rename(&live, &old).map_err(|e| StreetError::io("replace the map fonts", &e))?;
    }
    if let Err(e) = fs::rename(&staging, &live) {
        // Put the previous assets back rather than leave none.
        if had_old {
            let _ = fs::rename(&old, &live);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(StreetError::io("replace the map fonts", &e));
    }
    if had_old {
        let _ = fs::remove_dir_all(&old);
    }
    Ok(())
}

/// Whether `assets/` holds an install (its licence file is the last thing a complete one has).
pub fn installed(dir: &MapsDir) -> bool {
    REQUIRED.iter().all(|f| dir.assets().join(f).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::pmtiles::gzip;
    use crate::testutil::{scratch_dir, ustar};

    fn good_files() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("OFL.txt", b"SIL Open Font License".to_vec()),
            ("ICONS-LICENSE.txt", b"MIT".to_vec()),
            ("fonts/Noto Sans Regular/0-255.pbf", vec![1, 2, 3]),
            ("sprites/light@2x.png", vec![0x89, b'P', b'N', b'G']),
        ]
    }

    #[test]
    fn a_good_archive_installs_and_replaces_the_previous_one() {
        let tmp = scratch_dir("assets-install");
        let dir = MapsDir::new(tmp.to_path_buf());
        let archive = gzip(&ustar(&good_files(), &[]));
        install(&dir, &archive, &sha256_hex(&archive)).unwrap();
        assert!(installed(&dir));
        assert_eq!(
            fs::read(dir.assets().join("fonts/Noto Sans Regular/0-255.pbf")).unwrap(),
            [1, 2, 3]
        );

        // A newer archive replaces the folder whole: a file it no longer has is gone.
        let mut newer = good_files();
        newer.pop();
        let archive = gzip(&ustar(&newer, &["fonts"]));
        install(&dir, &archive, &sha256_hex(&archive)).unwrap();
        assert!(!dir.assets().join("sprites/light@2x.png").exists());
        let leftovers: Vec<_> = fs::read_dir(&*tmp)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .collect();
        assert_eq!(
            leftovers,
            ["assets"],
            "no staging or old folder is left behind"
        );
    }

    #[test]
    fn the_sha256_is_checked_before_anything_is_unpacked() {
        let tmp = scratch_dir("assets-sha");
        let dir = MapsDir::new(tmp.to_path_buf());
        let archive = gzip(&ustar(&good_files(), &[]));
        let mut wrong = sha256_hex(&archive);
        wrong.replace_range(..1, if wrong.starts_with('0') { "1" } else { "0" });
        let e = install(&dir, &archive, &wrong).unwrap_err();
        assert_eq!(e.kind, ErrorKind::InvalidArchive);
        assert!(!dir.assets().exists());
        // One flipped byte in the archive changes its hash just the same.
        let mut flipped = archive.clone();
        let last = flipped.len() - 9;
        flipped[last] ^= 1;
        assert!(install(&dir, &flipped, &sha256_hex(&archive)).is_err());
    }

    #[test]
    fn archives_that_could_escape_or_lack_their_licences_are_refused() {
        let tmp = scratch_dir("assets-bad");
        let dir = MapsDir::new(tmp.to_path_buf());
        let try_files = |files: &[(&str, Vec<u8>)]| {
            let archive = gzip(&ustar(files, &[]));
            install(&dir, &archive, &sha256_hex(&archive))
        };
        let mut escape = good_files();
        escape.push(("../packs.json", b"x".to_vec()));
        assert!(try_files(&escape).is_err());
        let mut absolute = good_files();
        absolute.push(("/etc/x", b"x".to_vec()));
        assert!(try_files(&absolute).is_err());
        let mut twice = good_files();
        twice.push(("OFL.txt", b"again".to_vec()));
        assert!(try_files(&twice).is_err());
        let no_ofl: Vec<_> = good_files()
            .into_iter()
            .filter(|(p, _)| *p != "OFL.txt")
            .collect();
        assert!(try_files(&no_ofl).is_err());
        let no_icons: Vec<_> = good_files()
            .into_iter()
            .filter(|(p, _)| *p != "ICONS-LICENSE.txt")
            .collect();
        assert!(try_files(&no_icons).is_err());
        assert!(
            !dir.assets().exists(),
            "nothing was installed by any of them"
        );
        assert!(try_files(&good_files()).is_ok(), "control");

        // A symlink entry (type '2') is refused outright.
        let mut tar = ustar(&good_files(), &[]);
        let at = tar.len() - 2 * BLOCK;
        tar.splice(
            at..at,
            ustar(&[("link", Vec::new())], &[])[..BLOCK].iter().copied(),
        );
        tar[at + 156] = b'2';
        let sum: u64 = tar[at..at + BLOCK]
            .iter()
            .enumerate()
            .map(|(i, &b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    u64::from(b)
                }
            })
            .sum();
        tar[at + 148..at + 156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        assert!(untar(&tar).unwrap_err().message.contains("entry type"));

        // A damaged header checksum, and an archive cut short.
        let mut damaged = ustar(&good_files(), &[]);
        damaged[0] ^= 1;
        assert!(untar(&damaged).is_err());
        let whole = ustar(&good_files(), &[]);
        assert!(untar(&whole[..whole.len() - 2 * BLOCK]).is_err());
        assert_eq!(untar(&whole).unwrap().len(), 4, "control");
    }
}
