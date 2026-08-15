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
//! Every drain — including the empty ones — is reported to the engine's
//! [`tempo_app::engine::SstvHealth`], so the view can state what the receiver is
//! hearing instead of showing one hint that meant four different things.
//!
//! RX ONLY: nothing here keys PTT or emits TX audio.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempo_app::dto::SstvGalleryEntry;
use tempo_app::engine::{engine_lock, Engine, SstvProgress};
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
/// The engine's RX audio rate (`tempo_fast::SAMPLE_RATE`).
const INPUT_RATE_HZ: u32 = 12_000;

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// One drain-and-decode step: take whatever the engine's armed SSTV tap holds,
/// run the decoder off-lock, and report the drain to the engine's health.
///
/// Split out of [`run`] because this is THE seam the field bug lived in — the tap
/// is filled by the radio loop while the waterfall is fed by a separate wait-free
/// tee, so a live waterfall proves nothing about whether the decoder is being fed.
/// Nothing in this function needs a sound card or a thread, so the whole chain
/// (`feed_rx_audio` → arm gate → `take_sstv_audio` → decode) is testable; see the
/// tests at the bottom of this file.
///
/// An EMPTY drain is reported too, deliberately: an armed decoder that is being
/// handed nothing is exactly the "the app is deaf" case the operator needs told
/// about, and it is invisible if only productive drains are counted. The engine
/// records it without clobbering the last real level — this poll runs faster than
/// the radio loop feeds it, so empty drains are routine, not evidence of a fault.
fn rx_step(engine: &Arc<Mutex<Engine>>, decoder: &mut SstvDecoder, at_unix: i64) -> Vec<SstvEvent> {
    let audio = engine_lock(engine).take_sstv_audio();
    if audio.is_empty() {
        engine_lock(engine).note_sstv_rx(0, 0.0, 0, None, 0, at_unix);
        return Vec::new();
    }
    let peak = audio.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    // The heavy part — resample, VIS scan, per-line demod — off-lock.
    let events = decoder.process(&audio);
    let vis_seen = events
        .iter()
        .filter(|e| matches!(e, SstvEvent::VisDetected { .. }))
        .count();
    // A parity-passing header for a mode this build cannot decode used to be an
    // `eprintln!` and nothing more, which in a packaged build is silence. Carry the
    // code so the view can name it.
    let unknown_vis = events.iter().find_map(|e| match e {
        SstvEvent::UnknownVis { code, .. } => Some(*code),
        _ => None,
    });
    let images = events
        .iter()
        .filter(|e| matches!(e, SstvEvent::ImageComplete { .. }))
        .count();
    engine_lock(engine).note_sstv_rx(audio.len(), peak, vis_seen, unknown_vis, images, at_unix);
    events
}

/// A partial image being filled line-by-line from `LineDecoded` events.
struct InFlight {
    mode_name: &'static str,
    width: u32,
    height: u32,
    pixels: Vec<[u8; 3]>,
    lines_done: u32,
    /// `observed_leader_hz - 1900` from this image's VIS header — how far off
    /// frequency the radio is. Carried so the band view can state it while the
    /// decoded picture is standing in for the spectrum.
    hedr_shift_hz: f64,
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
    // Path of the most recently saved image — the target for a trailing
    // `FskId` event, which arrives just after that image's `ImageComplete`.
    let mut last_finished_path: Option<String> = None;
    loop {
        if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(POLL);
        let armed = engine_lock(&engine).sstv_armed();
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
        let events = rx_step(&engine, decoder.as_mut().unwrap(), now_unix());
        if events.is_empty() {
            continue;
        }
        let mut progress_dirty = false;
        for ev in events {
            match ev {
                SstvEvent::VisDetected {
                    mode,
                    hedr_shift_hz,
                    ..
                } => {
                    let spec = tempo_sstv::for_mode(mode);
                    inflight = Some(InFlight {
                        mode_name: spec.name,
                        width: spec.line_pixels,
                        height: spec.image_lines,
                        pixels: vec![[0u8; 3]; (spec.line_pixels * spec.image_lines) as usize],
                        lines_done: 0,
                        hedr_shift_hz,
                    });
                    progress_dirty = true;
                }
                SstvEvent::UnknownVis { code, .. } => {
                    // Counted onto `sstv_health` by `rx_step` so the view can say so;
                    // the log line stays for a developer reading a console.
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
                        last_finished_path = finish_image(&engine, &gallery_dir, &img, &image);
                        progress_dirty = false; // finish_image cleared progress
                    }
                }
                SstvEvent::FskId { text } => {
                    // The callsign burst that trailed the just-saved image:
                    // stamp it onto that gallery entry and re-persist the JSON.
                    if let Some(path) = last_finished_path.clone() {
                        let mut e = engine_lock(&engine);
                        e.set_sstv_gallery_fsk_id(&path, text);
                        let snapshot = e.sstv_gallery().to_vec();
                        drop(e);
                        sstv_store::save_gallery(&gallery_dir, &snapshot);
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
                {
                    let mut e = engine_lock(&engine);
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
                            hedr_shift_hz: img.hedr_shift_hz,
                        }));
                    }
                }
            }
        }
    }
}

/// Persist a completed image (BMP + gallery.json) and record it on the engine's
/// session gallery, stamped with the dial frequency at completion time. Returns
/// the saved image path on success (the anchor a trailing `FskId` event stamps),
/// or `None` if the image couldn't be written / the engine lock was poisoned.
fn finish_image(
    engine: &Arc<Mutex<Engine>>,
    gallery_dir: &std::path::Path,
    img: &InFlight,
    image: &tempo_sstv::SstvImage,
) -> Option<String> {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let spec = tempo_sstv::for_mode(image.mode);
    let filename = format!("{}_{}.png", sstv_store::utc_stamp(unix), spec.short_name);
    let path = gallery_dir.join(&filename);
    // Stamp what this picture IS into the file itself, not only into gallery.json beside it —
    // a picture mailed back to the sender or posted to a club page keeps its provenance.
    // Read the dial under the same lock the gallery entry below uses, so the two agree.
    let dial_mhz = engine_lock(engine).settings().dial_mhz;
    let meta = [
        ("Software", format!("Nexus {}", env!("CARGO_PKG_VERSION"))),
        (
            "Title",
            format!(
                "SSTV {} received {}",
                spec.short_name,
                sstv_store::utc_iso(unix)
            ),
        ),
        ("Source", format!("{} SSTV", spec.short_name)),
        ("Creation Time", sstv_store::utc_iso(unix)),
        ("Comment", format!("{:.4} MHz dial", dial_mhz)),
    ];
    if let Err(e) = sstv_store::write_png(&path, image.width, image.height, &image.pixels, &meta) {
        eprintln!("sstv-rx: failed to save {}: {e}", path.display());
        engine_lock(engine).set_sstv_progress(None);
        return None;
    }
    let path_str = path.to_string_lossy().into_owned();
    // Record on the session gallery (freq stamped under the same lock), then
    // persist the whole capped list beside the images.
    let snapshot: Vec<SstvGalleryEntry> = {
        let mut e = engine_lock(engine);
        let entry = SstvGalleryEntry {
            path: path_str.clone(),
            mode: img.mode_name.to_string(),
            finished_utc: sstv_store::utc_iso(unix),
            freq_mhz: e.settings().dial_mhz,
            lines: image.height,
            // Filled in later if the trailing FSK-ID burst decodes.
            fsk_id: None,
        };
        e.push_sstv_gallery(entry);
        e.set_sstv_progress(None);
        e.sstv_gallery().to_vec()
    };
    sstv_store::save_gallery(gallery_dir, &snapshot);
    Some(path_str)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
mod tests {
    use super::*;
    use tempo_sstv::{encode_image, for_mode, SourceImage, SstvMode};

    /// ⭐ THE FIELD BUG, 2026-08-01 (KD9TAW, FTdx10, 14.236 then 14.230):
    /// "I hear a signal but the SSTV is not decoding … not working or decoding as
    /// the image comes in." The waterfall on that same screen showed the signal, so
    /// audio was reaching the app — but the waterfall rides a wait-free tee of the
    /// capture callback (`rxtap`/`rxdsp`) while the DECODER is fed by the radio loop
    /// through `Engine::feed_rx_audio`, behind an arm flag. Nothing in the tree
    /// tested that seam: `tempo-sstv` proves the decoder decodes, and the engine's
    /// own SSTV test feeds it 128 zeros. So the one chain that actually failed on
    /// the air — capture → engine tap → arm gate → decode thread → image — had no
    /// coverage at all.
    ///
    /// These tests run that whole chain with a real transmission and no sound card.
    /// Scottie 1 on purpose: it is what 14.230 carries in North America.
    const RATE: u32 = 12_000;

    /// The radio loop hands the engine roughly a slot-tick of audio at a time and
    /// the decode thread drains every 100 ms. Feed at that shape rather than in one
    /// giant push — `RX_TAP_CAP` is 10 s, so a single push of a 110 s transmission
    /// would be silently truncated, which is a property of the real plumbing worth
    /// keeping in the harness.
    const FEED_CHUNK: usize = (RATE as usize) / 10;

    fn engine() -> Arc<Mutex<Engine>> {
        Arc::new(Mutex::new(Engine::new("W9XYZ", "EN61", 0)))
    }

    /// A known picture at the mode's exact geometry: horizontal red ramp, vertical
    /// green ramp, constant blue (the `tx_loopback.rs` fixture).
    fn source_image(w: u32, h: u32) -> SourceImage {
        let mut rgb = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                rgb.push([(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 128u8]);
            }
        }
        SourceImage {
            width: w,
            height: h,
            rgb,
        }
    }

    /// Everything the app does to the receiver when the operator OPENS the SSTV
    /// view. Before the fix this was the whole bug: nothing reached the receiver,
    /// so the ordinary way to use SSTV decoded nothing.
    fn open_sstv_view(engine: &Arc<Mutex<Engine>>) {
        engine_lock(engine).sstv_auto_arm();
    }

    /// Play a real transmission at the radio into `engine` exactly the way the radio
    /// loop does, running the decode thread's step between feeds. Returns every
    /// event the decoder produced, plus the health snapshot AS OF the drain that
    /// finished the picture — which is what the operator's screen was showing at
    /// that moment. Sampling it there rather than at the end matters: the pass ends
    /// in trailing silence, and `audio_peak` is the level of the most recent drain
    /// that carried samples, silence included.
    fn play_transmission(
        engine: &Arc<Mutex<Engine>>,
        mode: SstvMode,
    ) -> (Vec<SstvEvent>, tempo_app::engine::SstvHealth) {
        let spec = for_mode(mode);
        let img = source_image(spec.line_pixels, spec.image_lines);
        let mut audio = encode_image(mode, &img, RATE).expect("encode_image");
        // Trailing runway: the decoder buffers a whole image before find-sync runs,
        // and the last line's FFT look-ahead needs samples past the final scanline.
        audio.extend(std::iter::repeat_n(0.0_f32, RATE as usize * 2));

        let mut decoder = SstvDecoder::new(INPUT_RATE_HZ).expect("decoder");
        let mut out = Vec::new();
        let mut at_image = None;
        let mut at = 1_754_000_000_i64;
        for chunk in audio.chunks(FEED_CHUNK) {
            engine_lock(engine).feed_rx_audio(chunk);
            at += 1;
            let events = rx_step(engine, &mut decoder, at);
            if at_image.is_none()
                && events
                    .iter()
                    .any(|e| matches!(e, SstvEvent::ImageComplete { .. }))
            {
                at_image = Some(engine_lock(engine).sstv_health());
            }
            out.extend(events);
        }
        let health = at_image.unwrap_or_else(|| engine_lock(engine).sstv_health());
        (out, health)
    }

    fn completed_image(events: &[SstvEvent]) -> Option<tempo_sstv::SstvImage> {
        events.iter().find_map(|e| match e {
            SstvEvent::ImageComplete { image, .. } => Some(image.clone()),
            _ => None,
        })
    }

    /// Mean per-channel |difference| between the sent and the received picture.
    /// The `tx_loopback.rs` bar is < 5.0 for a noiseless path.
    fn mean_diff(src: &SourceImage, got: &tempo_sstv::SstvImage) -> f64 {
        let (mut sum, mut n) = (0_u64, 0_u64);
        for (a, b) in src.rgb.iter().zip(got.pixels.iter()) {
            for ch in 0..3 {
                sum += u64::from((i32::from(a[ch]) - i32::from(b[ch])).unsigned_abs() as u8);
                n += 1;
            }
        }
        sum as f64 / n as f64
    }

    /// ⭐ THE REPRO. A receiver nobody armed swallows the entire transmission: the
    /// engine tap never fills, so the decoder is handed nothing for the whole
    /// 110 seconds and produces not one event. This is the field report exactly —
    /// audio present at the radio, waterfall alive, no picture — and it is pinned
    /// here so the "it decodes but the screen is stale" theories stay dead.
    #[test]
    fn an_unarmed_receiver_swallows_the_whole_transmission() {
        let eng = engine();
        let (events, health) = play_transmission(&eng, SstvMode::Scottie1);
        assert!(
            completed_image(&events).is_none(),
            "an unarmed receiver must not decode — it is not being fed"
        );
        assert!(
            events.is_empty(),
            "and it sees no events at all: {events:?}"
        );
        assert!(!health.armed);
        assert_eq!(
            health.last_audio_unix, None,
            "not one sample reached the decoder, and the health says so"
        );
    }

    /// ⭐ THE FIX, end to end: the operator opens the SSTV view, a station transmits,
    /// and a picture comes out — through the real engine tap, the real arm gate and
    /// the real decoder, with no sound card and no thread.
    #[test]
    fn opening_the_view_decodes_a_transmission_off_the_engine_tap() {
        let eng = engine();
        open_sstv_view(&eng);

        let (events, h) = play_transmission(&eng, SstvMode::Scottie1);
        let spec = for_mode(SstvMode::Scottie1);
        let got = completed_image(&events).unwrap_or_else(|| {
            panic!(
                "no image off the live path; events={:?}",
                events
                    .iter()
                    .map(|e| match e {
                        SstvEvent::VisDetected { .. } => "VisDetected",
                        SstvEvent::UnknownVis { .. } => "UnknownVis",
                        SstvEvent::LineDecoded { .. } => "LineDecoded",
                        SstvEvent::ImageComplete { .. } => "ImageComplete",
                        _ => "?",
                    })
                    .collect::<Vec<_>>()
            )
        });

        assert_eq!(got.mode, SstvMode::Scottie1, "decoded the transmitted mode");
        assert_eq!(
            (got.width, got.height),
            (spec.line_pixels, spec.image_lines),
            "full-size picture"
        );
        // Recognizable content, not just the right shape.
        let mean = mean_diff(&source_image(spec.line_pixels, spec.image_lines), &got);
        assert!(
            mean < 5.0,
            "picture does not match what was sent: mean per-channel diff {mean:.2}"
        );

        // And the health the view reads tells the true story of the pass.
        assert!(h.armed, "the view opening is what armed it");
        assert!(h.last_audio_unix.is_some(), "audio reached the decoder");
        assert!(h.audio_peak > 0.1, "at a real level, got {}", h.audio_peak);
        assert_eq!(h.vis_seen, 1, "one VIS header");
        assert_eq!(h.images, 1, "one image");
        assert_eq!(h.unknown_vis, 0);
    }

    /// An operator who STOPS the receiver has decided. Re-entering the view must not
    /// restart it behind them — the same rule `aprs_auto_arm` follows.
    #[test]
    fn opening_the_view_does_not_undo_an_explicit_stop() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        assert!(e.sstv_auto_arm(), "first entry arms");
        e.set_sstv_armed(false);
        assert!(
            !e.sstv_auto_arm(),
            "re-entry must not override the operator"
        );
        assert!(!e.sstv_armed());
        // Explicitly arming again still works — only the automatic path is refused.
        e.set_sstv_armed(true);
        assert!(e.sstv_armed());
    }

    /// A drain that carries nothing is still reported, and it must not clobber the
    /// last real level: the decode thread polls every 100 ms while the radio loop
    /// that feeds it can take far longer per iteration.
    #[test]
    fn empty_drains_are_reported_without_erasing_the_level() {
        let eng = engine();
        open_sstv_view(&eng);
        let mut decoder = SstvDecoder::new(INPUT_RATE_HZ).expect("decoder");

        engine_lock(&eng).feed_rx_audio(&[0.5_f32; 240]);
        let _ = rx_step(&eng, &mut decoder, 100);
        let _ = rx_step(&eng, &mut decoder, 101); // nothing new arrived

        let h = engine_lock(&eng).sstv_health();
        assert_eq!(h.drains, 2, "both drains counted");
        assert_eq!(
            h.last_audio_unix,
            Some(100),
            "the empty one is not new audio"
        );
        assert!(
            (h.audio_peak - 0.5).abs() < 1e-6,
            "level survives, got {}",
            h.audio_peak
        );
    }
}
