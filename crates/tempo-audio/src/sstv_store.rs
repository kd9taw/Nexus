//! SSTV image + gallery persistence — the pure half of the SSTV RX path.
//!
//! The `sstvrx` decode thread calls these to persist each completed image to
//! the operator-browsable local data dir (`<local-appdata>/Nexus/sstv-gallery/`,
//! the ALL.TXT location pattern) and to keep `gallery.json` — the small metadata
//! sidecar living BESIDE the images (atomic tmp+rename, the `openings_log.json`
//! pattern) — in step with the engine's session gallery list.
//!
//! Images are written as uncompressed 24-bit BMP: universally openable, zero new
//! dependencies. (Swap in a PNG encoder later if one ever enters the tree.)

use std::io;
use std::path::Path;

use tempo_app::dto::SstvGalleryEntry;

/// The gallery metadata sidecar's filename, inside the gallery directory.
pub const GALLERY_JSON: &str = "gallery.json";

/// Encode `width × height` RGB pixels (row-major, top-down) as a 24-bit
/// uncompressed BMP (BITMAPINFOHEADER, bottom-up rows, BGR byte order, rows
/// padded to 4 bytes).
pub fn encode_bmp(width: u32, height: u32, pixels: &[[u8; 3]]) -> Vec<u8> {
    let row_bytes = (width as usize) * 3;
    let pad = (4 - row_bytes % 4) % 4;
    let image_size = (row_bytes + pad) * height as usize;
    let file_size = 54 + image_size;

    let mut out = Vec::with_capacity(file_size);
    // BITMAPFILEHEADER (14 bytes).
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 4]); // reserved
    out.extend_from_slice(&54u32.to_le_bytes()); // pixel-data offset
                                                 // BITMAPINFOHEADER (40 bytes).
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes()); // positive = bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB (uncompressed)
    out.extend_from_slice(&(image_size as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes()); // ~72 dpi
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // palette colors
    out.extend_from_slice(&0u32.to_le_bytes()); // important colors
                                                // Pixel rows, bottom-up, BGR, padded.
    for y in (0..height as usize).rev() {
        let row = &pixels[y * width as usize..(y + 1) * width as usize];
        for [r, g, b] in row {
            out.extend_from_slice(&[*b, *g, *r]);
        }
        out.extend(std::iter::repeat_n(0u8, pad));
    }
    out
}

/// Write `pixels` to `path` as a 24-bit BMP (see [`encode_bmp`]).
pub fn write_bmp(path: &Path, width: u32, height: u32, pixels: &[[u8; 3]]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, encode_bmp(width, height, pixels))
}

/// Nearest-neighbor downscale of an RGB image to at most `max_w` pixels wide
/// (aspect preserved, height ≥ 1). Returns `(width, height, flat RGB bytes)`.
/// An image already ≤ `max_w` wide passes through at full size.
pub fn downscale_rgb(
    width: u32,
    height: u32,
    pixels: &[[u8; 3]],
    max_w: u32,
) -> (u32, u32, Vec<u8>) {
    let (pw, ph) = if width <= max_w {
        (width.max(1), height.max(1))
    } else {
        (max_w, ((height * max_w) / width).max(1))
    };
    let mut out = Vec::with_capacity((pw * ph * 3) as usize);
    for y in 0..ph {
        let sy = (y as usize * height as usize) / ph as usize;
        for x in 0..pw {
            let sx = (x as usize * width as usize) / pw as usize;
            let [r, g, b] = pixels[sy * width as usize + sx];
            out.extend_from_slice(&[r, g, b]);
        }
    }
    (pw, ph, out)
}

/// Load the persisted gallery metadata from `dir`/`gallery.json`, oldest first.
/// Missing or unparseable file → empty (the gallery re-accumulates; images on
/// disk are untouched either way).
pub fn load_gallery(dir: &Path) -> Vec<SstvGalleryEntry> {
    std::fs::read_to_string(dir.join(GALLERY_JSON))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Persist the gallery metadata to `dir`/`gallery.json` (atomic tmp+rename,
/// best-effort — a failed write never disturbs the decode thread).
pub fn save_gallery(dir: &Path, entries: &[SstvGalleryEntry]) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(text) = serde_json::to_string(entries) {
        let path = dir.join(GALLERY_JSON);
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// Filename-safe UTC stamp, e.g. `20260717T153000Z`.
pub fn utc_stamp(unix: u64) -> String {
    let (y, mo, d, h, mi, s) = tempo_core::logbook::datetime_utc(unix);
    format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z")
}

/// ISO-8601 UTC time, e.g. `2026-07-17T15:30:00Z` (the gallery record form).
pub fn utc_iso(unix: u64) -> String {
    let (y, mo, d, h, mi, s) = tempo_core::logbook::datetime_utc(unix);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A unique per-test scratch dir under the OS temp dir.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nexus-sstv-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn bmp_encoding_layout_is_exact() {
        // 3×2 top-down: row 0 = red green blue, row 1 = white black gray.
        let px = [
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [255, 255, 255],
            [0, 0, 0],
            [128, 128, 128],
        ];
        let bmp = encode_bmp(3, 2, &px);
        // 3 px × 3 B = 9 B/row, padded to 12; 54-byte header + 2 rows.
        assert_eq!(bmp.len(), 54 + 24);
        assert_eq!(&bmp[0..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[2..6].try_into().unwrap()), 78);
        assert_eq!(u32::from_le_bytes(bmp[10..14].try_into().unwrap()), 54);
        assert_eq!(i32::from_le_bytes(bmp[18..22].try_into().unwrap()), 3);
        assert_eq!(i32::from_le_bytes(bmp[22..26].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(bmp[28..30].try_into().unwrap()), 24);
        // Bottom-up: the first stored row is image row 1; BGR order, so white
        // = FF FF FF then black then gray.
        assert_eq!(&bmp[54..63], &[255, 255, 255, 0, 0, 0, 128, 128, 128]);
        assert_eq!(&bmp[63..66], &[0, 0, 0], "row padding");
        // Second stored row is image row 0: red in BGR = 00 00 FF.
        assert_eq!(&bmp[66..69], &[0, 0, 255]);
    }

    #[test]
    fn gallery_round_trips_through_the_real_encoder_path() {
        let dir = scratch("roundtrip");
        // A tiny synthetic image through the REAL encoder path (write_bmp).
        let px = vec![[10, 20, 30]; 4];
        let img_path = dir.join(format!("{}_{}.bmp", utc_stamp(1_700_000_000), "robot36"));
        write_bmp(&img_path, 2, 2, &px).unwrap();
        let on_disk = std::fs::read(&img_path).unwrap();
        assert_eq!(&on_disk[0..2], b"BM");
        assert_eq!(on_disk.len(), 54 + 2 * 8, "2 rows of 6 B padded to 8");

        // The metadata record round-trips through gallery.json.
        let entry = SstvGalleryEntry {
            path: img_path.to_string_lossy().into_owned(),
            mode: "Robot 36".into(),
            finished_utc: utc_iso(1_700_000_000),
            freq_mhz: 14.230,
            lines: 2,
            fsk_id: None,
        };
        save_gallery(&dir, std::slice::from_ref(&entry));
        let loaded = load_gallery(&dir);
        assert_eq!(loaded, vec![entry]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_or_corrupt_gallery_json_loads_empty() {
        let dir = scratch("corrupt");
        assert!(load_gallery(&dir).is_empty(), "missing file");
        std::fs::write(dir.join(GALLERY_JSON), "not json {{").unwrap();
        assert!(load_gallery(&dir).is_empty(), "corrupt file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preview_downscales_to_max_width_and_passes_small_images_through() {
        // 320×2 gradient → 160×1; nearest-neighbor keeps exact source bytes.
        let mut px = Vec::new();
        for y in 0..2u32 {
            for x in 0..320u32 {
                px.push([(x % 256) as u8, y as u8, 0]);
            }
        }
        let (w, h, rgb) = downscale_rgb(320, 2, &px, 160);
        assert_eq!((w, h), (160, 1));
        assert_eq!(rgb.len(), 160 * 3);
        assert_eq!(&rgb[0..3], &[0, 0, 0], "first sample from source col 0");
        assert_eq!(&rgb[3..6], &[2, 0, 0], "second sample from source col 2");

        // Already narrow → untouched dimensions.
        let (w, h, rgb) = downscale_rgb(4, 3, &[[9, 9, 9]; 12], 160);
        assert_eq!((w, h), (4, 3));
        assert_eq!(rgb.len(), 36);
    }

    #[test]
    fn utc_stamps_match_the_known_epoch() {
        // unix 1_700_000_000 == 2023-11-14 22:13:20 UTC (the alltxt.rs anchor).
        assert_eq!(utc_stamp(1_700_000_000), "20231114T221320Z");
        assert_eq!(utc_iso(1_700_000_000), "2023-11-14T22:13:20Z");
    }
}
