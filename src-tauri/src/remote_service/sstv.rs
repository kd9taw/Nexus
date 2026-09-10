//! Received SSTV images are addressed by connection-local opaque handles.
//! Only files already in the native gallery, inside its configured directories,
//! can be read. Image I/O never holds the engine or runs on the live sample loop.
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempo_app::dto::SstvGalleryEntry;

const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;
const CHUNK_BYTES: usize = 48 * 1024;

#[derive(Default)]
pub(super) struct Images {
    // Native paths never appear in a response, error, log, or browser argument.
    paths: HashMap<String, String>,
}
pub(super) type SharedImages = Arc<Mutex<Images>>;

#[derive(Clone, Default)]
pub(crate) struct Source {
    roots: Vec<PathBuf>,
    // Shared across station reconnects. A slow filesystem cannot accumulate
    // another blocking reader for every new connection.
    read_gate: Arc<Mutex<()>>,
}
impl Source {
    pub(crate) fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            ..Self::default()
        }
    }
}

pub(super) fn identifier(id: &str) -> bool {
    let Some((id, ext)) = id.rsplit_once('.') else {
        return false;
    };
    super::transport::identifier(id) && matches!(ext, "png" | "bmp")
}

pub(super) fn preflight(e: &tempo_app::engine::Engine) -> Result<(), &'static str> {
    let gallery = e.sstv_gallery();
    if gallery.len() > 200 {
        return Err("applicationTooLarge");
    }
    let mut text_bytes = 0;
    for g in gallery {
        if g.path.len() > 4096
            || g.mode.len() > 80
            || g.finished_utc.len() > 64
            || g.fsk_id.as_ref().is_some_and(|s| s.len() > 64)
        {
            return Err("applicationTooLarge");
        }
        text_bytes += g.path.len()
            + g.mode.len()
            + g.finished_utc.len()
            + g.fsk_id.as_ref().map_or(0, String::len);
    }
    if text_bytes > 512 * 1024 || e.sstv_tx_mode().is_some_and(|s| s.len() > 80) {
        return Err("applicationTooLarge");
    }
    if let Some(p) = e.sstv_progress() {
        if p.mode.len() > 80
            || p.preview_w > 160
            || p.preview_h > 160
            || p.preview_rgb.len() > 160 * 160 * 3
            || (!p.preview_rgb.is_empty()
                && p.preview_rgb.len() != (p.preview_w * p.preview_h * 3) as usize)
        {
            return Err("applicationTooLarge");
        }
    }
    Ok(())
}

impl Images {
    pub(super) fn project(&mut self, gallery: &mut [SstvGalleryEntry]) -> Result<(), &'static str> {
        let present: HashSet<_> = gallery.iter().map(|g| g.path.as_str()).collect();
        self.paths.retain(|_, path| present.contains(path.as_str()));
        let mut by_path: HashMap<_, _> = self
            .paths
            .iter()
            .map(|(id, path)| (path.clone(), id.clone()))
            .collect();
        for g in gallery {
            let id = if let Some(id) = by_path.get(&g.path) {
                id.clone()
            } else {
                let ext = Path::new(&g.path)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                // An unrecognized local file remains an explicitly unavailable
                // gallery entry. It never becomes an arbitrary browser asset.
                let ext = if ext == "bmp" { "bmp" } else { "png" };
                let id = format!("{}.{}", super::query::snapshot_id()?, ext);
                self.paths.insert(id.clone(), g.path.clone());
                by_path.insert(g.path.clone(), id.clone());
                id
            };
            g.path = id;
        }
        Ok(())
    }
}

fn current_path(
    images: &SharedImages,
    id: &str,
    engine: &crate::SharedEngine,
) -> Result<PathBuf, &'static str> {
    if !identifier(id) {
        return Err("applicationUnsupported");
    }
    let path = images
        .try_lock()
        .map_err(|_| "applicationBusy")?
        .paths
        .get(id)
        .cloned()
        .ok_or("queryExpired")?;
    let e = engine.try_lock().map_err(|_| "applicationBusy")?;
    if !e.sstv_gallery().iter().any(|g| g.path == path) {
        return Err("queryExpired");
    }
    Ok(PathBuf::from(path))
}

pub(super) fn check_current(
    images: &SharedImages,
    id: &str,
    engine: &crate::SharedEngine,
) -> Result<(), &'static str> {
    current_path(images, id, engine).map(|_| ())
}

fn dimensions(bytes: &[u8], ext: &str) -> Result<(u32, u32), &'static str> {
    let pair = if ext == "png"
        && bytes.len() >= 33
        && &bytes[..8] == b"\x89PNG\r\n\x1a\n"
        && u32::from_be_bytes(bytes[8..12].try_into().unwrap()) == 13
        && &bytes[12..16] == b"IHDR"
    {
        (
            u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
            u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
        )
    } else if ext == "bmp"
        && bytes.len() >= 54
        && &bytes[..2] == b"BM"
        && u32::from_le_bytes(bytes[10..14].try_into().unwrap()) == 54
        && u32::from_le_bytes(bytes[14..18].try_into().unwrap()) == 40
        && u16::from_le_bytes(bytes[26..28].try_into().unwrap()) == 1
        && u16::from_le_bytes(bytes[28..30].try_into().unwrap()) == 24
        && u32::from_le_bytes(bytes[30..34].try_into().unwrap()) == 0
    {
        let w = i32::from_le_bytes(bytes[18..22].try_into().unwrap());
        let h = i32::from_le_bytes(bytes[22..26].try_into().unwrap());
        if w <= 0 || h <= 0 {
            return Err("applicationUnavailable");
        }
        let row = (u64::from(w as u32) * 3).div_ceil(4) * 4;
        if 54 + row * u64::from(h as u32) != bytes.len() as u64 {
            return Err("applicationUnavailable");
        }
        (w as u32, h as u32)
    } else {
        return Err("applicationUnavailable");
    };
    // Larger than every native SSTV mode, still a strict decoded-pixel bound.
    if pair.0 == 0 || pair.1 == 0 || pair.0 > 1024 || pair.1 > 1024 {
        return Err("applicationTooLarge");
    }
    Ok(pair)
}

pub(super) fn capture(
    images: &SharedImages,
    id: &str,
    engine: &crate::SharedEngine,
    source: &Source,
) -> Result<(Vec<Value>, usize, Value), &'static str> {
    let _reader = source.read_gate.try_lock().map_err(|_| "applicationBusy")?;
    let path = current_path(images, id, engine)?;
    let ext = id.rsplit_once('.').ok_or("applicationUnsupported")?.1;
    if !path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case(ext))
    {
        return Err("applicationUnavailable");
    }
    let canonical = path.canonicalize().map_err(|_| "applicationUnavailable")?;
    if !source
        .roots
        .iter()
        .filter_map(|p| p.canonicalize().ok())
        .any(|root| canonical.parent() == Some(root.as_path()))
    {
        return Err("applicationUnavailable");
    }
    // Refuse non-files before opening, as well as checking the opened handle.
    // A gallery entry replaced with a directory or pipe is not image content.
    let entry = std::fs::symlink_metadata(&canonical).map_err(|_| "applicationUnavailable")?;
    if !entry.is_file() || entry.len() > MAX_IMAGE_BYTES as u64 {
        return Err("applicationTooLarge");
    }
    let mut file = std::fs::File::open(&canonical).map_err(|_| "applicationUnavailable")?;
    let before = file.metadata().map_err(|_| "applicationUnavailable")?;
    if !before.is_file() || before.len() > MAX_IMAGE_BYTES as u64 {
        return Err("applicationTooLarge");
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut file)
        .take((MAX_IMAGE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "applicationUnavailable")?;
    let after = file.metadata().map_err(|_| "applicationUnavailable")?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("applicationTooLarge");
    }
    if before.len() != bytes.len() as u64
        || after.len() != before.len()
        || before.modified().ok() != after.modified().ok()
        || path.canonicalize().ok().as_ref() != Some(&canonical)
    {
        return Err("queryExpired");
    }
    check_current(images, id, engine)?;
    let (width, height) = dimensions(&bytes, ext)?;
    let digest: String = ring::digest::digest(&ring::digest::SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let rows: Vec<_> = bytes
        .chunks(CHUNK_BYTES)
        .enumerate()
        .map(|(index, chunk)| json!({ "index": index, "base64": crate::b64_encode(chunk) }))
        .collect();
    let total = rows.len();
    Ok((
        rows,
        total,
        json!({ "imageId": id, "mime": if ext == "bmp" { "image/bmp" } else { "image/png" },
        "byteLength": bytes.len(), "sha256": digest, "width": width, "height": height }),
    ))
}

#[cfg(test)]
pub(super) mod fixture {
    use super::*;
    pub struct Gallery(pub PathBuf);
    impl Gallery {
        pub fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "nexus-sstv-{}",
                super::super::query::snapshot_id().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
        pub fn source(&self) -> Source {
            Source::new(vec![self.0.clone()])
        }
        pub fn seed(&self, engine: &mut tempo_app::engine::Engine) {
            let mut n = 17u32;
            let pixels: Vec<[u8; 3]> = (0..320 * 256)
                .map(|_| {
                    let mut p = [0; 3];
                    for c in &mut p {
                        n = n.wrapping_mul(1664525).wrapping_add(1013904223);
                        *c = (n >> 24) as u8;
                    }
                    p
                })
                .collect();
            for ext in ["png", "bmp"] {
                let path = self.0.join(format!("received.{ext}"));
                if ext == "png" {
                    tempo_audio::sstv_store::write_png(&path, 320, 256, &pixels, &[]).unwrap();
                } else {
                    std::fs::write(
                        &path,
                        tempo_audio::sstv_store::encode_bmp(320, 256, &pixels),
                    )
                    .unwrap();
                }
                engine.push_sstv_gallery(SstvGalleryEntry {
                    path: path.to_string_lossy().into_owned(),
                    mode: "Scottie 1".into(),
                    finished_utc: "2026-09-10T01:00:00Z".into(),
                    freq_mhz: 14.230,
                    lines: 256,
                    fsk_id: Some("W1AW".into()),
                });
            }
            engine.set_sstv_armed(true);
            engine.set_sstv_progress(Some(tempo_app::engine::SstvProgress {
                mode: "Scottie 1".into(),
                lines_total: 256,
                lines_done: 128,
                preview_w: 160,
                preview_h: 128,
                preview_rgb: pixels[..160 * 128].as_flattened().to_vec(),
                hedr_shift_hz: 15.0,
            }));
        }
    }
    impl Drop for Gallery {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_app::engine::Engine;
    #[test]
    fn gallery_files_round_trip_through_opaque_handles_without_changing_the_station() {
        let files = fixture::Gallery::new();
        let mut e = Engine::with_settings(Default::default());
        files.seed(&mut e);
        let native = e.sstv_gallery().to_vec();
        let shared = Arc::new(Mutex::new(e));
        let images = SharedImages::default();
        let mut published = native.clone();
        images.lock().unwrap().project(&mut published).unwrap();
        let first = published.clone();
        let mut again = native.clone();
        images.lock().unwrap().project(&mut again).unwrap();
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(&again).unwrap()
        );
        for (p, n) in published.iter().zip(&native) {
            assert!(identifier(&p.path));
            assert!(!p.path.contains('/'));
            let (rows, total, meta) = capture(&images, &p.path, &shared, &files.source()).unwrap();
            assert!(total > 3);
            assert_eq!(rows.len(), total);
            let bytes: Vec<u8> = rows
                .into_iter()
                .flat_map(|row| crate::b64_decode(row["base64"].as_str().unwrap()).unwrap())
                .collect();
            assert_eq!(bytes, std::fs::read(&n.path).unwrap());
            assert_eq!(meta["byteLength"], bytes.len());
            assert_eq!(meta["width"], 320);
            assert_eq!(meta["height"], 256);
        }
        let e = shared.lock().unwrap();
        assert_eq!(
            serde_json::to_value(e.sstv_gallery()).unwrap(),
            serde_json::to_value(native).unwrap()
        );
        assert!(!e.snapshot().radio.tx_enabled);
        assert!(!e.sstv_sending());
    }
    #[test]
    fn gallery_capabilities_expire_and_refuse_unlisted_outside_missing_or_oversized_files() {
        let files = fixture::Gallery::new();
        let outside = fixture::Gallery::new();
        let mut e = Engine::with_settings(Default::default());
        files.seed(&mut e);
        let native = e.sstv_gallery().to_vec();
        let shared = Arc::new(Mutex::new(e));
        let images = SharedImages::default();
        let mut published = native.clone();
        images.lock().unwrap().project(&mut published).unwrap();
        let id = &published[0].path;
        assert_eq!(
            capture(&images, &native[0].path, &shared, &files.source()).unwrap_err(),
            "applicationUnsupported"
        );
        assert_eq!(
            capture(&images, id, &shared, &outside.source()).unwrap_err(),
            "applicationUnavailable"
        );
        let source = files.source();
        let clone = source.clone();
        let hold = source.read_gate.lock().unwrap();
        assert_eq!(
            capture(&images, id, &shared, &clone).unwrap_err(),
            "applicationBusy"
        );
        drop(hold);
        let e = shared.lock().unwrap();
        assert_eq!(
            capture(&images, id, &shared, &source).unwrap_err(),
            "applicationBusy"
        );
        drop(e);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&native[0].path)
            .unwrap();
        file.set_len((MAX_IMAGE_BYTES + 1) as u64).unwrap();
        assert_eq!(
            capture(&images, id, &shared, &source).unwrap_err(),
            "applicationTooLarge"
        );
        use std::io::Write;
        file.set_len(0).unwrap();
        file.write_all(b"unrecognized file").unwrap();
        drop(file);
        assert_eq!(
            capture(&images, id, &shared, &source).unwrap_err(),
            "applicationUnavailable"
        );
        std::fs::remove_file(&native[0].path).unwrap();
        assert_eq!(
            capture(&images, id, &shared, &source).unwrap_err(),
            "applicationUnavailable"
        );
        shared.lock().unwrap().remove_sstv_gallery(&native[0].path);
        assert_eq!(check_current(&images, id, &shared), Err("queryExpired"));
    }
    #[test]
    #[cfg(unix)]
    fn a_gallery_symlink_cannot_read_a_file_outside_the_gallery() {
        let files = fixture::Gallery::new();
        let outside = fixture::Gallery::new();
        let mut e = Engine::with_settings(Default::default());
        files.seed(&mut e);
        let path = e.sstv_gallery()[0].path.clone();
        let target = outside.0.join("private.png");
        std::fs::rename(&path, &target).unwrap();
        std::os::unix::fs::symlink(target, &path).unwrap();
        let images = SharedImages::default();
        let mut gallery = e.sstv_gallery().to_vec();
        images.lock().unwrap().project(&mut gallery).unwrap();
        assert_eq!(
            capture(
                &images,
                &gallery[0].path,
                &Arc::new(Mutex::new(e)),
                &files.source()
            )
            .unwrap_err(),
            "applicationUnavailable"
        );
    }
}
