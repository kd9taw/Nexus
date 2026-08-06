//! ⭐ THE BURNED-IN CALLSIGN IS READ BACK OFF THE AIR, NOT ASSUMED.
//!
//! An SSTV over is one continuous PTT hold of up to ≈290 s carrying nothing but
//! picture. §97.119(b)(4) lets the station ident ride in that picture — but only
//! if the receiving station can actually read it. A callsign that is present in
//! the source bitmap and smeared into illegibility by the mode's demodulator is
//! not an identification, and no amount of asserting that `draw_id` was called
//! would show the difference.
//!
//! So this test does the whole loop, for all 15 shipped modes:
//!
//! 1. Rasterize a **hostile** picture — half near-white (240), half near-black
//!    (16), a saturated colour wash, and a mid-grey band behind the plate — then
//!    burn the ID in with the production [`tempo_sstv::draw_id`].
//! 2. Encode with the production [`tempo_sstv::encode_image`] at 12 kHz, the rate
//!    `sstv_send` uses.
//! 3. Optionally add seeded AWGN at a stated SNR (a fixed xorshift, so the noisy
//!    cases are asserted rather than "reported").
//! 4. Decode with the real [`tempo_sstv::SstvDecoder`] — VIS auto-detect, the
//!    SNR-adaptive Hann window bank, per-line demod, `ImageComplete`.
//! 5. **Read the call back out of the decoded pixels**: register on the plate,
//!    threshold, correlate each character cell against all 39 glyph bitmaps,
//!    argmax. Assert the joined string is the callsign.
//! 6. Assert a **contrast margin** as well: mean decoded luma over glyph pixels
//!    minus mean over plate-background pixels. The argmax says a matched filter
//!    can read it; the margin is the proxy for a human reading it. They fail for
//!    different reasons, which is why both are here.
//!
//! ## Why step 5 registers instead of indexing
//!
//! Because the decoder's own line alignment moves under noise, and that is not
//! what this test is about. Measured (PD-50, 20 dB, seed `0x5EED…0001`): the whole
//! decoded picture came back offset **+8 px horizontally** at the shipped
//! geometry, with content rolling in from the previous line — mean |ΔLuma| 19.3
//! against 12.6 at 10 dB and 3.9 clean. Note the direction of that: MORE noise
//! decoded straighter, so this is a sync-lock artifact rather than a smooth
//! degradation. Across the whole `sx_sweep` the offsets run −3 to +20 px and are
//! **horizontal only** — every measured `dy` was 0, which is what the physics
//! predicts, since lines demodulate independently.
//!
//! The *picture* moves, plate and all. A read-back that indexed the nominal plate
//! rectangle would call that an illegible ID when what actually happened is a
//! sync offset in the RX path that a human looking at the received picture would
//! not even notice. So the read-back finds the plate the way a receiving operator
//! does — by looking for it — and then judges only the glyphs.
//!
//! Registration uses the plate's **rectangle** (a black bar of known size against
//! the band behind it), never its letters, so it cannot manufacture a correct
//! read. Two things hold that claim up: with no plate drawn at all this file
//! printed `"      "` for every mode (the state of the world before `draw_id` had
//! a body), and `a_picture_with_no_plate_does_not_read_back_as_a_callsign` keeps
//! it that way.
//!
//! What this does NOT prove: that a *third-party* decoder (MMSSTV, QSSTV, Black
//! Cat, RX-SSTV) reads it. Our demod window bank is slowrx's, which is
//! representative, and the geometry is a fraction of picture width so it travels
//! — but the only proof of the far end is an on-air test, which is the operator's
//! to run.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::manual_midpoint,
    clippy::many_single_char_names,
    clippy::needless_range_loop,
    clippy::panic,
    clippy::unwrap_used
)]

use tempo_sstv::idcard::{
    glyph, normalize_call, plate_for, GLYPHS, GLYPH_H, GLYPH_W, ID_GAP_CELLS, ID_PAD_CELLS,
};
use tempo_sstv::{draw_id, encode_image, for_mode, SourceImage, SstvDecoder, SstvEvent, SstvMode};

/// Nexus TX/RX audio rate (Hz) — what `sstv_send` passes `encode_image`.
const RATE: u32 = 12_000;
/// The operator this repo ships as.
const CALL: &str = "KD9TAW";
/// Minimum decoded contrast between glyph and plate background, 0–255. Well over
/// a third of the mode's full black-to-white deviation: below this a human is
/// squinting even where the matched filter is still right.
const MIN_MARGIN: f64 = 96.0;
/// How far the registration search looks for the plate, in picture pixels. Three
/// times the largest offset measured above (+20 px), and bounded so the search
/// cannot wander into an unrelated dark region.
const SEARCH_X: i32 = 72;
/// Vertical search range — lines demodulate independently, so this only has to
/// absorb whole-image line-offset slip.
const SEARCH_Y: i32 = 6;

/// Every mode `SSTV_TX_MODES` offers the operator.
const ALL_MODES: [SstvMode; 15] = [
    SstvMode::Scottie1,
    SstvMode::Scottie2,
    SstvMode::ScottieDx,
    SstvMode::Martin1,
    SstvMode::Martin2,
    SstvMode::Robot24,
    SstvMode::Robot36,
    SstvMode::Robot72,
    SstvMode::Pd50,
    SstvMode::Pd90,
    SstvMode::Pd120,
    SstvMode::Pd160,
    SstvMode::Pd180,
    SstvMode::Pd240,
    SstvMode::Pd290,
];

/// Plate placement in picture pixels — [`plate_for`]'s answer for the guards, or
/// a forced one for the `sx_sweep` measurement.
#[derive(Clone, Copy)]
struct Geom {
    sx: u32,
    sy: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    n: u32,
}

impl Geom {
    /// What the production geometry gives this raster.
    fn shipped(w: u32, h: u32) -> Self {
        let p = plate_for(w, h, CALL).expect("plate geometry");
        Self {
            sx: p.sx,
            sy: p.sy,
            x: p.x,
            y: p.y,
            w: p.w,
            h: p.h,
            n: p.call.chars().count() as u32,
        }
    }

    /// A plate at a forced horizontal scale (the sweep), same layout rules.
    fn forced(sx: u32, n: u32) -> Self {
        let sy = sx.div_ceil(2);
        Self {
            sx,
            sy,
            x: ID_PAD_CELLS * sx,
            y: ID_PAD_CELLS * sx,
            w: sx * (GLYPH_W * n + ID_GAP_CELLS * (n - 1) + 2 * ID_PAD_CELLS),
            h: GLYPH_H * sy + 2 * ID_PAD_CELLS * sx,
            n,
        }
    }
}

/// A picture chosen to make the plate's job as hard as it can be: the left half
/// near-white and the right half near-black (so the plate straddles a hard
/// vertical edge — the worst case for a horizontally-smearing demod), a saturated
/// colour wash below to keep the chroma path busy, and a mid-grey band behind the
/// plate. The grey is deliberate twice over: it is neither of the two levels the
/// plate itself uses, so a plate that failed to draw cannot pass by accidentally
/// matching its background; and it gives the registration search a consistent
/// surround to find the plate's edges against.
fn hostile_image(w: u32, h: u32, g: &Geom) -> SourceImage {
    let band = (g.y.saturating_sub(2 * g.sx), (g.y + g.h + 2 * g.sx).min(h));
    let mut rgb = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            rgb.push(if y >= band.0 && y < band.1 {
                [128, 128, 128]
            } else if x < w / 2 {
                [240, 236, 244]
            } else if y < h / 2 {
                [16, 20, 12]
            } else {
                [200, 30, 90]
            });
        }
    }
    SourceImage {
        width: w,
        height: h,
        rgb,
    }
}

/// Deterministic xorshift64* — a seeded PRNG so the noisy cases are assertions,
/// not observations. (`rand` is not a dependency of this crate, and one test does
/// not justify making it one.)
struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Approximately-Gaussian sample, unit variance (Irwin–Hall, 12 uniforms).
    fn normal(&mut self) -> f32 {
        let mut s = 0.0f64;
        for _ in 0..12 {
            s += (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        }
        (s - 6.0) as f32
    }
}

/// Add AWGN at `snr_db` relative to the signal's mean power.
fn add_noise(audio: &mut [f32], snr_db: f64) {
    let p: f64 = audio
        .iter()
        .map(|s| f64::from(*s) * f64::from(*s))
        .sum::<f64>()
        / audio.len().max(1) as f64;
    let sigma = (p / 10f64.powf(snr_db / 10.0)).sqrt() as f32;
    let mut rng = Rng(0x5EED_1234_ABCD_0001);
    for s in audio.iter_mut() {
        *s += sigma * rng.normal();
    }
}

/// Rec.601 luma plane of a decoded picture.
fn luma_plane(px: &[[u8; 3]]) -> Vec<f64> {
    px.iter()
        .map(|p| 0.299 * f64::from(p[0]) + 0.587 * f64::from(p[1]) + 0.114 * f64::from(p[2]))
        .collect()
}

/// Mean luma over a rectangle, clamped to the picture. `NaN` when the rectangle
/// lies entirely outside it.
fn rect_mean(lp: &[f64], w: u32, h: u32, x0: i32, y0: i32, rw: u32, rh: u32) -> f64 {
    let (mut sum, mut n) = (0.0f64, 0.0f64);
    for y in y0..y0 + rh as i32 {
        for x in x0..x0 + rw as i32 {
            if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                sum += lp[(y as u32 * w + x as u32) as usize];
                n += 1.0;
            }
        }
    }
    if n == 0.0 {
        f64::NAN
    } else {
        sum / n
    }
}

/// Mean luma of one font-pixel block, inset horizontally by a picture pixel when
/// the block is wide enough — that inset is where the demod's horizontal smear
/// lands. No vertical inset: lines demodulate independently, so there is no
/// vertical smear to avoid.
fn block_luma(lp: &[f64], w: u32, h: u32, x0: i32, y0: i32, sx: u32, sy: u32) -> f64 {
    let inset = i32::from(sx >= 3);
    rect_mean(
        lp,
        w,
        h,
        x0 + inset,
        y0,
        (sx as i32 - 2 * inset).max(1) as u32,
        sy,
    )
}

/// What came back: the call read out of the decoded plate, the contrast margin,
/// and where the plate actually was.
struct ReadBack {
    call: String,
    margin: f64,
    offset: (i32, i32),
}

/// Find the plate by its RECTANGLE, not its letters: score each candidate offset
/// by how much brighter the plate's interior (the glyph run, whatever it spells)
/// is than the plate's own padding frame, which is solid black for every callsign.
/// Take the best. Call-independent by construction — see the module header for
/// why this is registration and not cheating.
///
/// ⚠️ **Every sampled rectangle must be wholly inside the picture.** The first
/// cut let a partly-off-frame candidate score on the pixels that remained, and a
/// flank landing on a white glyph stroke then beat the true position by 149 to
/// 128 — the read came back `" KD9TA"`, one cell early. A decoder roll big enough
/// to push the plate off the edge has genuinely damaged the ident, and this
/// failing is the correct answer rather than something to search around.
fn register(lp: &[f64], w: u32, h: u32, g: &Geom) -> (i32, i32) {
    let (mut best, mut at) = (f64::MIN, (0, 0));
    let pad = ID_PAD_CELLS * g.sx;
    for dy in -SEARCH_Y..=SEARCH_Y {
        for dx in -SEARCH_X..=SEARCH_X {
            let (px, py) = (g.x as i32 + dx, g.y as i32 + dy);
            if px < 0 || py < 0 || px + g.w as i32 > w as i32 || py + g.h as i32 > h as i32 {
                continue;
            }
            // The plate's top and bottom padding bands — black for any call.
            let frame = (rect_mean(lp, w, h, px, py, g.w, pad)
                + rect_mean(lp, w, h, px, py + (g.h - pad) as i32, g.w, pad))
                / 2.0;
            // The glyph run box: black plate plus white strokes, so its mean sits
            // well above the frame wherever the plate actually is.
            let interior = rect_mean(
                lp,
                w,
                h,
                px + pad as i32,
                py + pad as i32,
                g.w - 2 * pad,
                g.h - 2 * pad,
            );
            let score = interior - frame;
            if score.is_finite() && score > best {
                best = score;
                at = (dx, dy);
            }
        }
    }
    at
}

/// Read the plate: register, sample every font pixel of every character cell,
/// threshold at the midpoint of the measured black/white levels, and pick the
/// glyph with the fewest mismatched cells.
fn read_plate(px: &[[u8; 3]], w: u32, h: u32, g: &Geom) -> ReadBack {
    let lp = luma_plane(px);
    let (dx, dy) = register(&lp, w, h, g);
    let gx0 = g.x as i32 + dx + (ID_PAD_CELLS * g.sx) as i32;
    let gy0 = g.y as i32 + dy + (ID_PAD_CELLS * g.sx) as i32;

    let mut cells = vec![vec![[0.0f64; GLYPH_W as usize]; GLYPH_H as usize]; g.n as usize];
    for (i, cell) in cells.iter_mut().enumerate() {
        let cx = gx0 + (i as u32 * (GLYPH_W + ID_GAP_CELLS) * g.sx) as i32;
        for (r, row) in cell.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                let x = cx + (c as u32 * g.sx) as i32;
                let y = gy0 + (r as u32 * g.sy) as i32;
                *v = block_luma(&lp, w, h, x, y, g.sx, g.sy);
            }
        }
    }

    // Threshold from the plate itself — its padding border is black whatever the
    // call is, and the brightest sampled cell is a lit stroke. No fixed constant,
    // so the read-back does not quietly depend on the mode's absolute level.
    let dark = rect_mean(
        &lp,
        w,
        h,
        g.x as i32 + dx + g.sx as i32,
        g.y as i32 + dy,
        g.w - 2 * g.sx,
        g.sx,
    );
    let bright = cells
        .iter()
        .flatten()
        .flatten()
        .copied()
        .fold(f64::MIN, f64::max);
    let thresh = (dark + bright) / 2.0;

    let mut call = String::new();
    let (mut lit, mut lit_n, mut bg, mut bg_n) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for cell in &cells {
        let mut best = (usize::MAX, ' ');
        for (ch, bits) in GLYPHS {
            let mut miss = 0usize;
            for r in 0..GLYPH_H as usize {
                for c in 0..GLYPH_W as usize {
                    let want = bits[r] & (1 << (GLYPH_W - 1 - c as u32)) != 0;
                    if (cell[r][c] > thresh) != want {
                        miss += 1;
                    }
                }
            }
            if miss < best.0 {
                best = (miss, ch);
            }
        }
        call.push(best.1);
        // Margin against the glyph that was DECODED, so a misread cannot be
        // flattered by comparing to a shape that is not what came through.
        if let Some(bits) = glyph(best.1) {
            for r in 0..GLYPH_H as usize {
                for c in 0..GLYPH_W as usize {
                    if bits[r] & (1 << (GLYPH_W - 1 - c as u32)) != 0 {
                        lit += cell[r][c];
                        lit_n += 1.0;
                    } else {
                        bg += cell[r][c];
                        bg_n += 1.0;
                    }
                }
            }
        }
    }
    ReadBack {
        call,
        margin: lit / lit_n.max(1.0) - bg / bg_n.max(1.0),
        offset: (dx, dy),
    }
}

/// Encode → (noise) → decode, returning the decoded pixels.
fn on_air(mode: SstvMode, img: &SourceImage, snr_db: Option<f64>) -> Vec<[u8; 3]> {
    let mut audio = encode_image(mode, img, RATE).expect("encode_image");
    if let Some(snr) = snr_db {
        add_noise(&mut audio, snr);
    }
    // Trailing runway so find-sync fills and the last line's FFT look-ahead plus
    // resampler group delay are covered (same discipline as tx_loopback.rs).
    audio.extend(std::iter::repeat_n(0.0_f32, RATE as usize));
    let mut dec = SstvDecoder::new(RATE).expect("decoder");
    let events = dec.process(&audio);
    let out = events
        .iter()
        .find_map(|e| match e {
            SstvEvent::ImageComplete {
                image,
                partial: false,
            } => Some(image.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{mode:?} @ {snr_db:?}: no ImageComplete"));
    assert_eq!(out.mode, mode, "{mode:?}: wrong decoded mode");
    assert_eq!((out.width, out.height), (img.width, img.height));
    out.pixels
}

/// The whole loop for one mode at one SNR (`None` = clean), through the
/// PRODUCTION [`draw_id`].
fn round_trip(mode: SstvMode, snr_db: Option<f64>) -> ReadBack {
    let spec = for_mode(mode);
    let (w, h) = (spec.line_pixels, spec.image_lines);
    let g = Geom::shipped(w, h);
    let mut img = hostile_image(w, h, &g);
    draw_id(&mut img.rgb, w, h, CALL);
    let px = on_air(mode, &img, snr_db);
    read_plate(&px, w, h, &g)
}

/// Assert one condition across every mode, naming the mode and the offset the
/// registration had to absorb when it fails.
fn assert_all(snr_db: Option<f64>) {
    for mode in ALL_MODES {
        let r = round_trip(mode, snr_db);
        assert_eq!(
            r.call, CALL,
            "{mode:?} @ {snr_db:?}: read back {:?} (plate found at {:?})",
            r.call, r.offset
        );
        assert!(
            r.margin >= MIN_MARGIN,
            "{mode:?} @ {snr_db:?}: contrast margin {:.1} < {MIN_MARGIN}",
            r.margin
        );
    }
}

#[test]
fn the_callsign_reads_back_off_a_clean_signal_in_every_mode() {
    assert_all(None);
}

#[test]
fn the_callsign_reads_back_at_20_db_in_every_mode() {
    assert_all(Some(20.0));
}

#[test]
fn the_callsign_reads_back_at_10_db_in_every_mode() {
    // 10 dB is where `window_idx_for_snr` steps to the 16-sample window — the
    // widest smear the stroke was sized against (Martin 2: 6.3 px of 320).
    assert_all(Some(10.0));
}

/// The guard that keeps the guards honest: with the plate absent, the read-back
/// must NOT return the callsign. Without it, a registration search that had
/// somehow learned to hallucinate would pass everything above silently.
#[test]
fn a_picture_with_no_plate_does_not_read_back_as_a_callsign() {
    let spec = for_mode(SstvMode::Scottie2);
    let (w, h) = (spec.line_pixels, spec.image_lines);
    let g = Geom::shipped(w, h);
    let img = hostile_image(w, h, &g); // deliberately no draw_id
    let px = on_air(SstvMode::Scottie2, &img, None);
    let r = read_plate(&px, w, h, &g);
    assert_ne!(r.call, CALL, "read a callsign out of a picture with no ID");
}

/// ⭐ WHERE `ID_STROKE_FRACTION` COMES FROM — run it, don't argue about it.
///
/// `cargo test -p tempo-sstv --test id_legibility -- --ignored --nocapture sx_sweep`
///
/// Draws the plate at a forced horizontal scale and reads it back across every
/// mode and three SNRs. The shipped constant is the smallest fraction that reads
/// back 100 % everywhere with margin to spare. Ignored by default because it is a
/// measurement, not a regression guard, and it costs minutes.
#[test]
#[ignore = "measurement sweep, not a guard — see the doc comment"]
fn sx_sweep() {
    let call = normalize_call(CALL, 32);
    let n = call.chars().count() as u32;
    for frac in [0.006_f64, 0.009, 0.012, 0.015, 0.019] {
        let (mut fails, mut worst) = (0usize, f64::MAX);
        for mode in ALL_MODES {
            let spec = for_mode(mode);
            let (w, h) = (spec.line_pixels, spec.image_lines);
            let sx = ((frac * f64::from(w)).ceil() as u32).max(1);
            let g = Geom::forced(sx, n);
            for snr in [None, Some(20.0), Some(10.0)] {
                let mut img = hostile_image(w, h, &g);
                draw_plate(&mut img.rgb, w, &g, &call);
                let px = on_air(mode, &img, snr);
                let r = read_plate(&px, w, h, &g);
                if r.call != call {
                    fails += 1;
                }
                worst = worst.min(r.margin);
                println!(
                    "frac={frac:.3} sx={sx:2} {mode:?} snr={snr:?} → {:?} margin={:.1} off={:?}",
                    r.call, r.margin, r.offset
                );
            }
        }
        println!("frac={frac:.3}: {fails} misreads, worst margin {worst:.1}\n");
    }
}

/// The sweep's own rasterizer — the same layout as [`draw_id`] but at a forced
/// scale, which is the whole point of the sweep. Deliberately NOT reused by the
/// guards: those must go through the production path.
fn draw_plate(rgb: &mut [[u8; 3]], w: u32, g: &Geom, call: &str) {
    for y in g.y..g.y + g.h {
        for x in g.x..g.x + g.w {
            rgb[(y * w + x) as usize] = [0, 0, 0];
        }
    }
    for (i, ch) in call.chars().enumerate() {
        let bits = glyph(ch).expect("glyph");
        let cx = g.x + ID_PAD_CELLS * g.sx + (i as u32) * (GLYPH_W + ID_GAP_CELLS) * g.sx;
        for (r, row) in bits.iter().enumerate() {
            for c in 0..GLYPH_W {
                if row & (1 << (GLYPH_W - 1 - c)) == 0 {
                    continue;
                }
                let y0 = g.y + ID_PAD_CELLS * g.sx + (r as u32) * g.sy;
                for y in y0..y0 + g.sy {
                    for x in cx + c * g.sx..cx + c * g.sx + g.sx {
                        rgb[(y * w + x) as usize] = [255, 255, 255];
                    }
                }
            }
        }
    }
}
