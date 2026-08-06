//! Burned-in station identification for SSTV transmit — the ID plate.
//!
//! ⚠️ THIS IS A COMPLIANCE FEATURE, NOT DECORATION. Before it, an SSTV over
//! carried **no station identification of any kind**: no burned-in overlay
//! (`SstvView.tsx` was `drawImage` only), no CW ident (`cw_id_after_73` sets
//! `pending_cw_id` in the FT/digital QSO state machine alone, and
//! `Engine::sstv_send` never touched it), and the FSK ID is decode-only — no
//! encoder exists. One SSTV over is a single continuous PTT hold of up to
//! ≈290 s (PD-290) of picture-only audio.
//!
//! §97.119(b)(4) lets the call go out "by an image emission conforming to the
//! applicable transmission standards … when all or part of the communications
//! are transmitted in the same image emission" — the picture *is* the
//! communication, so a call burned into the raster is the ident. But it only
//! counts if it **survives the raster it is drawn into**. A call that is present
//! in the source bitmap and smeared into illegibility by the mode is not an
//! identification. That is what the geometry below is for, and
//! `tests/id_legibility.rs` is the evidence.
//!
//! ## Why a bitmap font and not the webview's `fillText`
//!
//! Our own demod (`crate::demod`) takes an FFT **every sample**
//! (`PIXEL_FFT_STRIDE = 1`) through an SNR-adaptive Hann window drawn from
//! `HANN_LENS = [12, 16, 24, 32, 64, 128, 256]` at the 11 025 Hz working rate —
//! ≥20 dB picks 12 samples, ≥10 dB picks 16. One decoded pixel is one sample's
//! estimate, so the effective horizontal support of a pixel is the **window
//! length**, not the pixel period. In picture-width terms that is a smear of
//! ≈0.4 % (Scottie DX) to ≈2.0 % (Martin 2 at 10 dB). Vertically there is no
//! such integration — lines are demodulated independently, and every one of the
//! 15 modes carries one luma (or one full RGB triple) per image row. **The
//! raster punishes horizontal detail and colour detail; it does not punish
//! vertical detail.**
//!
//! Three rulings fall out of that:
//!
//! 1. **Stroke width is the binding constraint, not glyph height.** A vertical
//!    stroke narrower than the demod window smears toward the background. So the
//!    stroke is sized as a fraction of picture width ([`ID_STROKE_FRACTION`]),
//!    which travels across all five rasters instead of being tuned for one.
//! 2. **The ID must be a luminance feature.** `tone::lum_to_freq` maps 0 → 1500 Hz
//!    and 255 → 2300 Hz: black-to-white is the *entire* 800 Hz deviation. Colour
//!    cannot carry it — PD averages chroma across row pairs (`encode_pd`),
//!    Robot 24/36 send one chroma component per radio line and duplicate it
//!    (`encode_robot`), and Scottie/Martin sample R, G and B at different times
//!    within the line, so vertical edges fringe. A **monochrome** plate is exactly
//!    neutral chroma (Cr = Cb = 128 for both black and white), so every one of
//!    those subsampling schemes is lossless over it — the one region of the frame
//!    where chroma handling costs nothing.
//! 3. **An outline does not work at this size.** A 1 px halo is a fraction of the
//!    smear and vanishes; an outline thick enough to survive *is* a plate. So:
//!    white glyphs on a solid black plate, which is also the maximum deviation the
//!    mode offers and makes the ID independent of the picture behind it.
//!
//! And because vertical resolution is exact while horizontal is not, the glyphs
//! are scaled **anisotropically** — [`ID_STROKE_FRACTION`] sets the horizontal
//! scale, and the vertical scale is half of it. That spends pixels on the axis
//! that needs them and gives back roughly half the frame height the plate would
//! otherwise eat.
//!
//! `fillText` could satisfy none of this: at 320 px wide a readable point size
//! lays down ~1.3 px antialiased strokes, and its glyph shapes vary by platform —
//! unacceptable in the one feature whose entire purpose is being readable. A
//! bitmap font at integer scale makes stroke width an exact, chosen number, costs
//! ~270 bytes, and adds no dependency.
//!
//! ## Who draws it
//!
//! **This module is the authority.** `sstv_send` (src-tauri) burns the plate into
//! the operator's RGB *after* validating the geometry and *before* handing it to
//! the encoder, so no webview bug, stale buffer or third-party caller can put an
//! unidentified picture on the air. `ui/src/sstvIdOverlay.ts` mirrors the table
//! and the geometry so the operator's preview is what actually goes out (the two
//! draw identical pixels, so the double draw is a no-op); `ui/src/sstv-id-overlay.test.ts`
//! parses THIS file and fails if the mirror drifts.

/// Glyph cell width in font pixels. Each row of a glyph is one `u8` whose low
/// [`GLYPH_W`] bits are the columns, **bit 4 leftmost**.
pub const GLYPH_W: u32 = 5;
/// Glyph cell height in font pixels (one `u8` per row).
pub const GLYPH_H: u32 = 7;

/// Horizontal scale as a fraction of the mode's `line_pixels`. 0.015 → 5 px on
/// the 320-wide modes, 8 on 512, 10 on 640, 12 on 800. Stroke width and the
/// minimum intra-glyph gap are both exactly this, i.e. at or above the 20 dB
/// demod smear in every mode and at the 10 dB smear in the worst of them
/// (Martin 2, 6.3 px of 320).
///
/// **Pinned by measurement, and the measurement said something worth recording.**
/// The `#[ignore]`d `sx_sweep` in `tests/id_legibility.rs` reads the call back
/// across five fractions × 15 modes × 3 SNRs (45 round trips each). Result —
/// misreads, then worst contrast margin over all 45:
///
/// | fraction | sx on 320 | misreads | worst margin |
/// |---|---|---|---|
/// | 0.006 | 2 | 0 | 123.0 |
/// | 0.009 | 3 | **1** (PD-50 @10 dB → `"J/ J4 "`) | 75.2 |
/// | 0.012 | 4 | 0 | 196.2 |
/// | **0.015** | **5** | **0** | **209.3** |
/// | 0.019 | 7 | 0 | 222.3 |
///
/// Two things in that table decided this constant, and neither is the obvious one.
///
/// **Read-back is not monotonic in stroke width.** 0.009 misreads where 0.006 does
/// not, because the dominant failure at small scales is sub-cell registration
/// against the decoder's horizontal roll (see the test's module header), not smear.
/// So "the smallest a matched filter can still read" is the wrong criterion twice
/// over: it is unstable, and a matched filter is far more capable than the human
/// this ident exists for.
///
/// **The margin is monotonic, and it is the number that tracks human readability.**
/// It does not top out at 0.015 — 0.019 is better, by 13 of 255. This is therefore
/// a judgement and not an optimum: 0.015 clears the guards' 96 bar by more than
/// double while the plate costs ~12 % of frame height on a 320×256 mode, and the
/// next step up buys 6 % more margin for about a fifth more of the operator's
/// picture. If a real on-air report says the call is hard to read, 0.019 is the
/// change to make, and this table is why.
pub const ID_STROKE_FRACTION: f64 = 0.015;
/// Floor on the horizontal scale, for the narrowest raster we might ever carry.
pub const ID_MIN_SX: u32 = 3;
/// Gap between glyph cells, in units of the horizontal scale.
pub const ID_GAP_CELLS: u32 = 1;
/// Padding between the glyph run and the plate edge, in units of the horizontal
/// scale — and also the plate's inset from the picture's top-left corner.
pub const ID_PAD_CELLS: u32 = 1;

/// 5×7 uppercase bitmap font: `A–Z`, `0–9`, `/`, `-`, and space. One `u8` per
/// row, bit 4 = leftmost column. `ui/src/sstv-id-overlay.test.ts` parses this
/// table and compares the TS mirror byte-for-byte — and parses it in a way that
/// does not care how `cargo fmt` chooses to lay it out, which it had to learn the
/// hard way when the formatter exploded all 39 rows across six lines each.
///
/// Shapes are chosen for separation under smear as much as for looks: `0` keeps
/// its interior diagonal so it cannot collapse onto `O`, `1` keeps its foot and
/// flag so it cannot collapse onto `I`, and `2`/`Z` differ in the first and last
/// rows rather than only in the middle.
pub const GLYPHS: [(char, [u8; 7]); 39] = [
    (
        'A',
        [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'B',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
    ),
    (
        'C',
        [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
    ),
    (
        'D',
        [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
    ),
    (
        'E',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        'F',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        'G',
        [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
    ),
    (
        'H',
        [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'I',
        [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
    ),
    (
        'J',
        [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
    ),
    (
        'K',
        [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        'L',
        [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        'M',
        [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'N',
        [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'O',
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        'P',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        'Q',
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
    ),
    (
        'R',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        'S',
        [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
    ),
    (
        'T',
        [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        'U',
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        'V',
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
    ),
    (
        'W',
        [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
    ),
    (
        'X',
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
    ),
    (
        'Y',
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        'Z',
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
    ),
    (
        '0',
        [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
    ),
    (
        '1',
        [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
    ),
    (
        '2',
        [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
    ),
    (
        '3',
        [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
    ),
    (
        '4',
        [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
    ),
    (
        '5',
        [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
    ),
    (
        '6',
        [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        '7',
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
    ),
    (
        '8',
        [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        '9',
        [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
    ),
    (
        '/',
        [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
    ),
    (
        '-',
        [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
    ),
    (
        ' ',
        [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
    ),
];

/// The 5×7 bitmap for one character, or `None` if the font has no glyph for it.
/// Lowercase is folded to uppercase; nothing else is substituted.
#[must_use]
pub fn glyph(c: char) -> Option<[u8; 7]> {
    let up = c.to_ascii_uppercase();
    GLYPHS.iter().find(|(g, _)| *g == up).map(|(_, bits)| *bits)
}

/// The operator's callsign reduced to characters the font can draw: uppercased,
/// anything outside `A–Z 0–9 / -` dropped, and clipped to `max_chars`.
///
/// Dropping rather than substituting is deliberate — a `?` box in a callsign is
/// worse than a shorter call, and every character a real callsign can contain
/// (including the `/` of a portable suffix) is in the font.
#[must_use]
pub fn normalize_call(call: &str, max_chars: usize) -> String {
    call.trim()
        .chars()
        .map(|c| c.to_ascii_uppercase())
        .filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '/' || *c == '-')
        .take(max_chars)
        .collect()
}

/// Where the ID plate sits in a picture, in destination-raster pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdPlate {
    /// Horizontal glyph scale — one font pixel is `sx` picture pixels wide.
    /// This is also the stroke width, the intra-glyph gap and the plate padding.
    pub sx: u32,
    /// Vertical glyph scale — one font pixel is `sy` picture pixels tall.
    /// Half `sx` (rounded up): vertical resolution is exact on the wire, so
    /// height is the axis to give back.
    pub sy: u32,
    /// Plate left edge, picture pixels.
    pub x: u32,
    /// Plate top edge, picture pixels.
    pub y: u32,
    /// Plate width, picture pixels.
    pub w: u32,
    /// Plate height, picture pixels.
    pub h: u32,
    /// The characters actually drawn (normalized, and clipped to what fits).
    pub call: String,
}

/// Plate geometry for a picture of `width`×`height` carrying `call`, or `None`
/// when there is nothing to draw ([`normalize_call`] emptied the call) or the
/// picture is too small to carry even one character.
///
/// **Position: top-left, inset [`ID_PAD_CELLS`]·`sx` from both edges.** Top
/// rather than bottom because a truncated over loses the *tail* — an early Stop,
/// QSB at the end, or the far end's decoder giving up costs the last lines, and an
/// ID down there goes with them. Inset rather than flush because Robot 24/36's
/// row 0 carries no Cb at all (slowrx behaviour, see `encode.rs`) and line 0 is
/// where a receiver's sync settling shows.
///
/// The scale steps **down** from [`ID_STROKE_FRACTION`] only if the run will not
/// fit the picture width — a long portable call (`VP2E/KD9TAW`) on a 320-wide
/// mode — and never below 2, after which characters are dropped instead. Shrinking
/// before truncating keeps the whole call readable for as long as possible; the
/// call is the point.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
pub fn plate_for(width: u32, height: u32, call: &str) -> Option<IdPlate> {
    let full = normalize_call(call, 32);
    if full.is_empty() || width == 0 || height == 0 {
        return None;
    }
    let ideal = (ID_STROKE_FRACTION * f64::from(width)).ceil() as u32;
    let mut sx = ideal.max(ID_MIN_SX);
    // Shrink to fit before dropping characters.
    let want = full.chars().count() as u32;
    while sx > 2 && !fits_width(sx, want, width) {
        sx -= 1;
    }
    // Still too wide at the floor → clip the call to the characters that fit.
    let n_max = max_chars(sx, width);
    if n_max == 0 {
        return None;
    }
    let call = normalize_call(&full, n_max as usize);
    let n = call.chars().count() as u32;
    let sy = sx.div_ceil(2);
    let w = plate_w(sx, n);
    let h = GLYPH_H * sy + 2 * ID_PAD_CELLS * sx;
    if ID_PAD_CELLS * sx + h > height {
        return None;
    }
    Some(IdPlate {
        sx,
        sy,
        x: ID_PAD_CELLS * sx,
        y: ID_PAD_CELLS * sx,
        w,
        h,
        call,
    })
}

/// Plate width for `n` glyphs at horizontal scale `sx`: the glyph run
/// (`n` cells with `n-1` gaps) plus padding both sides.
fn plate_w(sx: u32, n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    sx * (GLYPH_W * n + ID_GAP_CELLS * (n - 1) + 2 * ID_PAD_CELLS)
}

/// Does an `n`-glyph plate at scale `sx` fit `width`, keeping the plate's inset
/// clear on BOTH sides (the inset is symmetric so the plate does not read as
/// having fallen off the right edge on a short raster)?
fn fits_width(sx: u32, n: u32, width: u32) -> bool {
    plate_w(sx, n) + 2 * ID_PAD_CELLS * sx <= width
}

/// How many glyphs fit a picture `width` at scale `sx`, allowing for the plate's
/// own inset from both picture edges.
fn max_chars(sx: u32, width: u32) -> u32 {
    let avail = width.saturating_sub(2 * ID_PAD_CELLS * sx);
    // avail >= sx·(5n + (n−1) + 2·pad)  ⇒  n ≤ (avail/sx + gap − 2·pad) / (5 + gap)
    let cells = avail / sx;
    if cells < GLYPH_W + 2 * ID_PAD_CELLS {
        return 0;
    }
    (cells + ID_GAP_CELLS - 2 * ID_PAD_CELLS) / (GLYPH_W + ID_GAP_CELLS)
}

/// Burn the ID plate into a row-major RGB picture, in place.
///
/// ⚠️ **Composited onto the DESTINATION raster, after any crop or resample.**
/// The operator's drag box moves the source *behind* the frame; this writes in
/// destination coordinates afterwards, so there is no crop rectangle that can
/// reach it and no resample that can shrink its strokes. Do not "simplify" this
/// by drawing into the source and letting the scaler carry it: text drawn at
/// 4032 px and downscaled 12× becomes a sub-pixel grey smear — present in the
/// bitmap and not an identification.
///
/// No-op when [`plate_for`] has nothing to draw.
pub fn draw_id(rgb: &mut [[u8; 3]], width: u32, height: u32, call: &str) {
    let Some(p) = plate_for(width, height, call) else {
        return;
    };
    if rgb.len() < (width as usize) * (height as usize) {
        return;
    }
    let put = |rgb: &mut [[u8; 3]], x: u32, y: u32, v: [u8; 3]| {
        if x < width && y < height {
            rgb[(y as usize) * (width as usize) + (x as usize)] = v;
        }
    };
    // The plate: solid black, so the ID is independent of the picture behind it
    // and spans the mode's whole 800 Hz deviation against the glyphs.
    for y in p.y..p.y + p.h {
        for x in p.x..p.x + p.w {
            put(rgb, x, y, [0, 0, 0]);
        }
    }
    // The glyphs: solid white, integer-scaled — never antialiased. Every lit
    // font pixel becomes an exact sx×sy block, which is what makes the stroke
    // width a chosen number rather than a hope.
    let gx0 = p.x + ID_PAD_CELLS * p.sx;
    let gy0 = p.y + ID_PAD_CELLS * p.sx;
    for (i, ch) in p.call.chars().enumerate() {
        let Some(bits) = glyph(ch) else { continue };
        #[allow(clippy::cast_possible_truncation)]
        let cell_x = gx0 + (i as u32) * (GLYPH_W + ID_GAP_CELLS) * p.sx;
        for (r, row) in bits.iter().enumerate() {
            for c in 0..GLYPH_W {
                if row & (1 << (GLYPH_W - 1 - c)) == 0 {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation)]
                let y0 = gy0 + (r as u32) * p.sy;
                let x0 = cell_x + c * p.sx;
                for y in y0..y0 + p.sy {
                    for x in x0..x0 + p.sx {
                        put(rgb, x, y, [255, 255, 255]);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::modespec::{for_mode, SstvMode};

    /// The 15 shipped rasters and the geometry the formula gives each of them.
    /// Pinned as VALUES, not recomputed from the formula, so a change to the
    /// formula has to be argued for here as well as written there. The TS mirror
    /// pins the same table in `ui/src/sstv-id-overlay.test.ts`.
    const EXPECT: [(u32, u32, u32, u32); 5] = [
        // (line_pixels, sx, sy, plate height)
        (320, 5, 3, 31),
        (512, 8, 4, 44),
        (640, 10, 5, 55),
        (800, 12, 6, 66),
        // The ID_MIN_SX floor, for a raster narrower than anything we ship.
        (160, 3, 2, 20),
    ];

    #[test]
    fn geometry_matches_the_pinned_table() {
        for (w, sx, sy, h) in EXPECT {
            let p = plate_for(w, 256, "KD9TAW").expect("plate fits");
            assert_eq!((p.sx, p.sy, p.h), (sx, sy, h), "raster {w} wide");
        }
    }

    #[test]
    fn every_shipped_mode_gets_a_plate_that_fits() {
        for mode in [
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
        ] {
            let spec = for_mode(mode);
            let p = plate_for(spec.line_pixels, spec.image_lines, "KD9TAW")
                .unwrap_or_else(|| panic!("{mode:?}: no plate"));
            assert!(
                p.x + p.w <= spec.line_pixels && p.y + p.h <= spec.image_lines,
                "{mode:?}: plate {}×{} at ({},{}) escapes {}×{}",
                p.w,
                p.h,
                p.x,
                p.y,
                spec.line_pixels,
                spec.image_lines
            );
            // The stroke is at or above the 20 dB demod smear for every mode
            // (window 12 samples at 11 025 Hz over the mode's pixel period), which
            // is the whole reason the scale is a fraction of width.
            assert!(
                f64::from(p.sx) / f64::from(spec.line_pixels) >= 0.0125,
                "{mode:?}: stroke {} px is under 1.25 % of picture width",
                p.sx
            );
            assert_eq!(p.call, "KD9TAW");
        }
    }

    #[test]
    fn a_long_portable_call_shrinks_before_it_truncates() {
        // 11 characters on the narrowest shipped raster: sx=5 would need
        // 5·(55+10+2) = 335 px of 320, so the scale steps down rather than
        // dropping the suffix that says where the operator actually is.
        let p = plate_for(320, 256, "VP2E/KD9TAW").expect("plate");
        assert_eq!(p.call, "VP2E/KD9TAW");
        assert!(p.sx < 5, "expected a shrink, got sx={}", p.sx);
        assert!(p.x + p.w <= 320);
    }

    #[test]
    fn no_call_draws_nothing() {
        assert!(plate_for(320, 256, "").is_none());
        assert!(plate_for(320, 256, "   ").is_none());
        // Punctuation a callsign cannot contain is dropped, not substituted.
        assert!(plate_for(320, 256, "!!!").is_none());
        let mut rgb = vec![[7u8, 8, 9]; 320 * 256];
        draw_id(&mut rgb, 320, 256, "");
        assert!(rgb.iter().all(|p| *p == [7, 8, 9]));
    }

    #[test]
    fn normalize_keeps_only_what_the_font_can_draw() {
        assert_eq!(normalize_call(" kd9taw ", 32), "KD9TAW");
        assert_eq!(normalize_call("KD9TAW/P", 32), "KD9TAW/P");
        assert_eq!(normalize_call("K.D9-T!AW", 32), "KD9-TAW");
        assert_eq!(normalize_call("KD9TAW", 3), "KD9");
    }

    #[test]
    fn the_plate_is_black_and_the_glyphs_are_white() {
        let mut rgb = vec![[128u8, 64, 200]; 320 * 256];
        draw_id(&mut rgb, 320, 256, "KD9TAW");
        let p = plate_for(320, 256, "KD9TAW").expect("plate");
        let at = |x: u32, y: u32| rgb[(y as usize) * 320 + (x as usize)];
        // Plate corner: padding, so black.
        assert_eq!(at(p.x, p.y), [0, 0, 0]);
        // Untouched picture outside the plate.
        assert_eq!(at(p.x + p.w + 1, p.y), [128, 64, 200]);
        assert_eq!(at(0, 0), [128, 64, 200]);
        // 'K' column 0 row 0 is lit → white block at the glyph origin.
        let gx = p.x + p.sx;
        let gy = p.y + p.sx;
        assert_eq!(at(gx, gy), [255, 255, 255]);
        assert_eq!(at(gx + p.sx - 1, gy + p.sy - 1), [255, 255, 255]);
        // Only black and white land inside the plate — a monochrome plate is
        // exactly neutral chroma, which is what makes PD/Robot subsampling
        // lossless over it.
        for y in p.y..p.y + p.h {
            for x in p.x..p.x + p.w {
                let v = at(x, y);
                assert!(v == [0, 0, 0] || v == [255, 255, 255], "({x},{y}) = {v:?}");
            }
        }
    }

    #[test]
    fn the_font_covers_every_character_a_callsign_can_hold() {
        for c in ('A'..='Z').chain('0'..='9').chain(['/', '-']) {
            assert!(glyph(c).is_some(), "no glyph for {c:?}");
        }
        assert!(glyph('a').is_some(), "lowercase folds to uppercase");
        assert!(glyph('#').is_none());
    }

    #[test]
    fn glyph_bitmaps_are_distinct() {
        // Two identical bitmaps would be an unreadable ID that every guard below
        // still passes (the matched filter would just pick the other one).
        for (i, (ca, a)) in GLYPHS.iter().enumerate() {
            for (cb, b) in GLYPHS.iter().skip(i + 1) {
                assert_ne!(a, b, "{ca:?} and {cb:?} have the same bitmap");
            }
        }
    }
}
