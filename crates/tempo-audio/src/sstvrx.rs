//! The SSTV RX decode thread — same armed-decoder-on-the-RX-path shape as
//! `rttyrx.rs`/`aicw.rs`. While armed (`sstv_arm`), the engine accumulates
//! 12 kHz RX audio; this thread drains it every ~100 ms and feeds the
//! `tempo-sstv` decoder OFF-lock:
//!
//! - `VisDetected` starts an in-flight image (mode label + lines total pushed
//!   to the engine as [`SstvProgress`]);
//! - `LineDecoded` fills a local partial-image buffer and refreshes a cheap
//!   ~160 px-wide RGB preview on the engine;
//! - `ImageComplete` writes `<UTC stamp>_<mode>.bmp` into the operator-browsable
//!   gallery dir, appends the metadata record to the engine's session gallery
//!   (stamping the current dial frequency), and re-persists `gallery.json`.
//!
//! RX ONLY: nothing here keys PTT or emits TX audio.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempo_app::dto::SstvGalleryEntry;
use tempo_app::engine::{Engine, SstvProgress};
use tempo_sstv::{SstvDecoder, SstvEvent};

use crate::service::SHUTDOWN;
use crate::sstv_store;

/// Drain cadence (the decoder buffers internally; 100 ms keeps VIS latency low).
const POLL: Duration = Duration::from_millis(100);
/// Retry backoff if the decoder fails to construct (should never happen at a
/// fixed valid rate, but never busy-loop on an error).
const CONSTRUCT_RETRY: Duration = Duration::from_secs(30);
/// Preview width cap for the in-progress thumbnail pushed to the UI.
const PREVIEW_MAX_W: u32 = 160;
/// The engine's RX audio rate (`ft1::SAMPLE_RATE`).
const INPUT_RATE_HZ: u32 = 12_000;

/// A partial image being filled line-by-line from `LineDecoded` events.
struct InFlight {
    mode_name: &'static str,
    width: u32,
    height: u32,
    pixels: Vec<[u8; 3]>,
    lines_done: u32,
}

/// Spawn the SSTV RX decode thread. `gallery_dir` is the operator-browsable
/// image folder (`<local-appdata>/Nexus/sstv-gallery`).
pub fn spawn_sstv_rx(engine: Arc<Mutex<Engine>>, gallery_dir: PathBuf) {
    std::thread::Builder::new()
        .name("sstv-rx".into())
        .spawn(move || run(engine, gallery_dir))
        .expect("spawn sstv-rx");
}

fn run(engine: Arc<Mutex<Engine>>, gallery_dir: PathBuf) {
    let mut decoder: Option<SstvDecoder> = None;
    let mut inflight: Option<InFlight> = None;
    loop {
        if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(POLL);
        let armed = match engine.lock() {
            Ok(e) => e.sstv_armed(),
            Err(_) => false,
        };
        if !armed {
            // Disarm drops the decoder + any partial image (the engine already
            // cleared its progress in `set_sstv_armed`); re-arm starts clean.
            decoder = None;
            inflight = None;
            continue;
        }
        if decoder.is_none() {
            match SstvDecoder::new(INPUT_RATE_HZ) {
                Ok(d) => decoder = Some(d),
                Err(e) => {
                    eprintln!("sstv-rx: decoder unavailable: {e}");
                    std::thread::sleep(CONSTRUCT_RETRY);
                    continue;
                }
            }
        }
        let audio = match engine.lock() {
            Ok(mut e) => e.take_sstv_audio(),
            Err(_) => continue,
        };
        if audio.is_empty() {
            continue;
        }
        // The heavy part — resample, VIS scan, per-line demod — off-lock.
        let events = decoder.as_mut().unwrap().process(&audio);
        let mut progress_dirty = false;
        for ev in events {
            match ev {
                SstvEvent::VisDetected { mode, .. } => {
                    let spec = tempo_sstv::for_mode(mode);
                    inflight = Some(InFlight {
                        mode_name: spec.name,
                        width: spec.line_pixels,
                        height: spec.image_lines,
                        pixels: vec![[0u8; 3]; (spec.line_pixels * spec.image_lines) as usize],
                        lines_done: 0,
                    });
                    progress_dirty = true;
                }
                SstvEvent::UnknownVis { code, .. } => {
                    eprintln!("sstv-rx: unknown VIS code {code} — burst ignored");
                }
                SstvEvent::LineDecoded {
                    line_index, pixels, ..
                } => {
                    if let Some(img) = inflight.as_mut() {
                        let w = img.width as usize;
                        let row = line_index as usize;
                        if row < img.height as usize && pixels.len() == w {
                            img.pixels[row * w..(row + 1) * w].copy_from_slice(&pixels);
                        }
                        img.lines_done = img.lines_done.max(line_index + 1);
                        progress_dirty = true;
                    }
                }
                SstvEvent::ImageComplete { image, .. } => {
                    if let Some(img) = inflight.take() {
                        finish_image(&engine, &gallery_dir, &img, &image);
                        progress_dirty = false; // finish_image cleared progress
                    }
                }
                // `SstvEvent` is #[non_exhaustive]: future event kinds are
                // simply not surfaced until this thread learns about them.
                _ => {}
            }
        }
        if progress_dirty {
            if let Some(img) = inflight.as_ref() {
                let (pw, ph, rgb) =
                    sstv_store::downscale_rgb(img.width, img.height, &img.pixels, PREVIEW_MAX_W);
                if let Ok(mut e) = engine.lock() {
                    // Disarm race guard: if the operator disarmed while this
                    // batch decoded, don't resurrect stale progress.
                    if e.sstv_armed() {
                        e.set_sstv_progress(Some(SstvProgress {
                            mode: img.mode_name.to_string(),
                            lines_total: img.height,
                            lines_done: img.lines_done,
                            preview_w: pw,
                            preview_h: ph,
                            preview_rgb: rgb,
                        }));
                    }
                }
            }
        }
    }
}

/// Persist a completed image (BMP + gallery.json) and record it on the engine's
/// session gallery, stamped with the dial frequency at completion time.
fn finish_image(
    engine: &Arc<Mutex<Engine>>,
    gallery_dir: &std::path::Path,
    img: &InFlight,
    image: &tempo_sstv::SstvImage,
) {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let spec = tempo_sstv::for_mode(image.mode);
    let filename = format!("{}_{}.bmp", sstv_store::utc_stamp(unix), spec.short_name);
    let path = gallery_dir.join(&filename);
    if let Err(e) = sstv_store::write_bmp(&path, image.width, image.height, &image.pixels) {
        eprintln!("sstv-rx: failed to save {}: {e}", path.display());
        if let Ok(mut e) = engine.lock() {
            e.set_sstv_progress(None);
        }
        return;
    }
    // Record on the session gallery (freq stamped under the same lock), then
    // persist the whole capped list beside the images.
    let snapshot: Vec<SstvGalleryEntry> = match engine.lock() {
        Ok(mut e) => {
            let entry = SstvGalleryEntry {
                path: path.to_string_lossy().into_owned(),
                mode: img.mode_name.to_string(),
                finished_utc: sstv_store::utc_iso(unix),
                freq_mhz: e.settings().dial_mhz,
                lines: image.height,
            };
            e.push_sstv_gallery(entry);
            e.set_sstv_progress(None);
            e.sstv_gallery().to_vec()
        }
        Err(_) => return,
    };
    sstv_store::save_gallery(gallery_dir, &snapshot);
}
