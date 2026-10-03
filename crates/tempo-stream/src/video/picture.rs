//! One picture on its way from the window to the encoder: the captured BGRA, and the I420 the
//! encoder takes, at the size the page will show it.
//!
//! - **The size the page gets is the client area's, cut to even**, unless that is more than the
//!   page shows or the path carries. VP8's 4:2:0 wants even sides; dropping at most one column and
//!   one row keeps the page's input coordinates, which are fractions of the decoded frame, within a
//!   pixel of the window's own.
//! - **Scaled once, down and never up, to what the page shows** ([`Bound`], [`encoded_size`]): no
//!   larger than the page's picture area in its own device pixels, once it has said (the `view`
//!   control message), and no more pixels than the path carries: [`LAN_PIXELS`] when the page is on
//!   the shack's own network, [`INTERNET_PIXELS`] anywhere else, a relay included. Each pixel sent
//!   is the average of the window area it covers ([`scale`]), so text drawn at the window's own
//!   pixels is as sharp as the page's size allows, and the page shows it pixel for pixel. Before
//!   2026-10-03 a window past 2560×1600 was halved with a 2×2 average whatever the page showed, and
//!   the browser enlarged it again: a 3440×1440 window reached the page at 1720×720 and its text
//!   read soft in every browser.
//! - **The bit rate follows the pixels** ([`Bound::kbps`]), within the path's ceiling. Encoding
//!   cost follows the pixel count too, and the station's CPU belongs to the radio first, which is
//!   one more reason never to encode more than the page shows.
//! - **BT.601, limited range.** VP8 carries no colour description, and a browser's decoder assumes
//!   exactly that, so any other matrix would shift every colour on the page.
use std::time::Instant;

use yuv::{
    bgra_to_yuv420, YuvChromaSubsampling, YuvConversionMode, YuvPlanarImageMut, YuvRange,
    YuvStandardMatrix,
};

/// The most pixels a picture is encoded at for a page on the shack's own network: a 4K window
/// streams whole.
pub const LAN_PIXELS: u64 = 3840 * 2160;
/// The most pixels for a page anywhere else, a relay included: a larger window is scaled to fit.
pub const INTERNET_PIXELS: u64 = 2560 * 1600;
/// The bit rate's ceiling, in kbit/s, for a page on the shack's own network…
pub const LAN_KBPS: u32 = 10_000;
/// …and for one anywhere else.
pub const INTERNET_KBPS: u32 = 4_000;
/// The bit rate's floor, in kbit/s, for any picture.
pub const MIN_KBPS: u32 = 300;
/// The smallest side the station takes a page's picture area to have. A page that says less (its
/// window dragged nearly shut) still gets a picture it can show, and the frames the freshness rule
/// reads.
pub const MIN_VIEW: u32 = 240;
/// The smallest side worth encoding. A window dragged smaller than this sends nothing until it
/// grows again.
pub const MIN_SIDE: u32 = 16;

/// A captured picture of the window's client area: 8-bit BGRA, top row first.
pub struct Bgra {
    pub width: u32,
    pub height: u32,
    /// Bytes from one row to the next, at least `width * 4`.
    pub stride: usize,
    pub pixels: Vec<u8>,
    /// When the capture delivered it, on the station's clock.
    pub captured_at: Instant,
}

impl Bgra {
    /// Is the buffer as large as its size and stride say?
    fn whole(&self) -> bool {
        let row = self.width as usize * 4;
        self.stride >= row
            && self.height > 0
            && self.pixels.len() >= self.stride * (self.height as usize - 1) + row
    }
}

/// The encoder's input: planar 4:2:0, BT.601 limited range.
pub struct I420 {
    pub(crate) planes: YuvPlanarImageMut<'static, u8>,
}

impl I420 {
    pub fn width(&self) -> u32 {
        self.planes.width
    }

    pub fn height(&self) -> u32 {
        self.planes.height
    }

    /// The Y, U and V planes, each with its stride in bytes.
    pub fn planes(&self) -> [(&[u8], u32); 3] {
        [
            (self.planes.y_plane.borrow(), self.planes.y_stride),
            (self.planes.u_plane.borrow(), self.planes.u_stride),
            (self.planes.v_plane.borrow(), self.planes.v_stride),
        ]
    }
}

/// Where the page is, as far as the picture goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    /// The station sends the picture to a private IPv4 address: the page is on the shack's own
    /// network.
    Lan,
    /// Anywhere else: the internet, or a relay.
    Internet,
}

/// The most a picture may be encoded at, and the bit rate it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bound {
    /// The page's picture area, in its own device pixels; `u32::MAX` until it says.
    pub width: u32,
    pub height: u32,
    /// The path's pixel budget.
    pub pixels: u64,
    /// The path's bit rate ceiling, in kbit/s.
    pub max_kbps: u32,
}

impl Bound {
    /// For a page on `path` whose picture area is `view`. A path the session does not know yet is
    /// read as the internet, the smaller budget; a page that has not said its view is bounded by
    /// the path alone.
    pub fn new(path: Option<Path>, view: Option<(u32, u32)>) -> Self {
        let (pixels, max_kbps) = match path {
            Some(Path::Lan) => (LAN_PIXELS, LAN_KBPS),
            Some(Path::Internet) | None => (INTERNET_PIXELS, INTERNET_KBPS),
        };
        let (width, height) = view.map_or((u32::MAX, u32::MAX), |(w, h)| {
            (w.max(MIN_VIEW), h.max(MIN_VIEW))
        });
        Self {
            width,
            height,
            pixels,
            max_kbps,
        }
    }

    /// The bit rate for a `width`×`height` picture: 1 kbit/s per 1000 pixels (2 Mbit/s at
    /// 1920×1080), never below [`MIN_KBPS`] nor above the path's ceiling.
    pub fn kbps(&self, width: u32, height: u32) -> u32 {
        let pixels = u64::from(width) * u64::from(height);
        (pixels / 1000).clamp(u64::from(MIN_KBPS), u64::from(self.max_kbps)) as u32
    }
}

impl Default for Bound {
    fn default() -> Self {
        Self::new(None, None)
    }
}

/// The size a `width`×`height` window is encoded at within `bound`: cut to even, then scaled down,
/// keeping its shape, until it fits the page's view and the path's pixels. `None` only for a window
/// with a side below [`MIN_SIDE`]. However small the bound, a window that can be encoded keeps at
/// least that much of each side, so a page never loses its picture to the size it reported.
pub fn encoded_size(width: u32, height: u32, bound: &Bound) -> Option<(u32, u32)> {
    let (w, h) = (u64::from(width & !1), u64::from(height & !1));
    let min = u64::from(MIN_SIDE);
    if w < min || h < min {
        return None;
    }
    // The view first, by whichever side it reaches first (compared cross-multiplied, so no
    // rounding decides it), then the path's pixels.
    let (bw, bh) = (u64::from(bound.width), u64::from(bound.height));
    let (mut sw, mut sh) = if w <= bw && h <= bh {
        (w, h)
    } else if bw * h <= bh * w {
        (bw, h * bw / w)
    } else {
        (w * bh / h, bh)
    };
    if sw * sh > bound.pixels {
        let k = (bound.pixels as f64 / (sw * sh) as f64).sqrt();
        sw = (sw as f64 * k) as u64;
        sh = (sh as f64 * k) as u64;
        // Floating point may land a hair over the budget; the budget is a bound.
        while sw * sh > bound.pixels {
            sw -= 1;
        }
    }
    let side = |v: u64, whole: u64| (v & !1).clamp(min, whole) as u32;
    Some((side(sw, w), side(sh, h)))
}

/// Fixed-point shares: the weights of one output pixel sum to exactly this, down the columns (so
/// the first pass is 16-bit arithmetic, which a processor does many lanes at a time)…
const ROWS: u32 = 1 << 8;
/// …and along each row.
const COLUMNS: u32 = 1 << 14;

/// One axis of a resize: for each output position, the first source pixel it covers and the share
/// of it, and of each of the `n - 1` after it, the output has, in `one`ths (zero past those it
/// covers).
struct Axis {
    n: usize,
    first: Vec<usize>,
    weights: Vec<u32>,
}

impl Axis {
    fn new(from: u32, to: u32, one: u32) -> Self {
        let (from64, to64) = (u64::from(from), u64::from(to));
        // In units of 1/to of a source pixel, a source pixel spans `to` and output `o` spans
        // [o·from, (o+1)·from): exact integers, and at most ceil(from/to) + 1 source pixels.
        let n = from.div_ceil(to) as usize + 1;
        let mut first = Vec::with_capacity(to as usize);
        let mut weights = vec![0u32; to as usize * n];
        for (o, shares) in weights.chunks_exact_mut(n).enumerate() {
            let (start, end) = (o as u64 * from64, (o as u64 + 1) * from64);
            let lo = start / to64;
            for i in lo..=(end - 1) / to64 {
                let covered = end.min((i + 1) * to64) - start.max(i * to64);
                shares[(i - lo) as usize] =
                    ((covered * u64::from(one) + from64 / 2) / from64) as u32;
            }
            // The rounding lands on the largest share, so the shares sum to exactly `one` and a
            // flat picture keeps its colour.
            let sum: u32 = shares.iter().sum();
            let largest = (0..n).max_by_key(|&i| shares[i]).unwrap_or(0);
            shares[largest] = shares[largest] + one - sum;
            first.push(lo as usize);
        }
        Self { n, first, weights }
    }
}

/// One plane of `size` at `stride` bytes a row, resized to `to` into `out` (stride `to.0`): each
/// output pixel the average of the source area it covers. Down the columns first, into one row at 8
/// more bits (at most 255 · 2⁸, so it fits 16 bits), then along that row.
fn resize(src: &[u8], stride: usize, size: (u32, u32), out: &mut [u8], to: (u32, u32)) {
    let (rows, cols) = (
        Axis::new(size.1, to.1, ROWS),
        Axis::new(size.0, to.0, COLUMNS),
    );
    let width = size.0 as usize;
    // Room past the row for the last outputs' zero shares.
    let mut row = vec![0u16; width + cols.n];
    for (r, line) in out
        .chunks_exact_mut(to.0 as usize)
        .take(to.1 as usize)
        .enumerate()
    {
        let sum = &mut row[..width];
        sum.fill(0);
        let shares = &rows.weights[r * rows.n..][..rows.n];
        for (k, &share) in shares.iter().enumerate() {
            // A zero share may lie past the last source row: it is never read.
            if share == 0 {
                continue;
            }
            let source = &src[(rows.first[r] + k) * stride..][..width];
            let share = share as u16;
            for (s, &p) in sum.iter_mut().zip(source) {
                *s += u16::from(p) * share;
            }
        }
        for (x, pixel) in line.iter_mut().enumerate() {
            let at = cols.first[x];
            let total: u32 = cols.weights[x * cols.n..][..cols.n]
                .iter()
                .zip(&row[at..at + cols.n])
                .map(|(&share, &v)| share * u32::from(v))
                .sum();
            *pixel = ((total + (1 << 21)) >> 22).min(255) as u8;
        }
    }
}

/// `source` at `width`×`height`, both even and neither larger than the source's: on each plane,
/// each output pixel is the average of the source area it covers.
pub fn scale(source: &I420, width: u32, height: u32) -> I420 {
    let mut planes = YuvPlanarImageMut::alloc(width, height, YuvChromaSubsampling::Yuv420);
    let [(y, ys), (u, us), (v, vs)] = source.planes();
    let full = (source.width(), source.height());
    let half = (full.0.div_ceil(2), full.1.div_ceil(2));
    let to = (width / 2, height / 2);
    resize(
        y,
        ys as usize,
        full,
        planes.y_plane.borrow_mut(),
        (width, height),
    );
    resize(u, us as usize, half, planes.u_plane.borrow_mut(), to);
    resize(v, vs as usize, half, planes.v_plane.borrow_mut(), to);
    I420 { planes }
}

/// The captured picture as the encoder takes it, at the window's own size cut to even, or `None` if
/// there is nothing worth encoding (a side too small, or a buffer shorter than its own size says).
/// [`encoded_size`] and [`scale`] then fit it to what the page shows.
pub fn to_i420(picture: &Bgra) -> Option<I420> {
    if !picture.whole() {
        return None;
    }
    let (width, height) = (picture.width & !1, picture.height & !1);
    if width < MIN_SIDE || height < MIN_SIDE {
        return None;
    }
    let mut planes = YuvPlanarImageMut::alloc(width, height, YuvChromaSubsampling::Yuv420);
    bgra_to_yuv420(
        &mut planes,
        &picture.pixels,
        picture.stride as u32,
        YuvRange::Limited,
        YuvStandardMatrix::Bt601,
        YuvConversionMode::Balanced,
    )
    .ok()?;
    Some(I420 { planes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, bgra: [u8; 4]) -> Bgra {
        Bgra {
            width,
            height,
            stride: width as usize * 4,
            pixels: bgra.repeat((width * height) as usize),
            captured_at: Instant::now(),
        }
    }

    /// A grey picture whose luma is `luma(x, y)`.
    fn drawn(width: u32, height: u32, luma: impl Fn(u32, u32) -> u8) -> I420 {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let v = luma(x, y);
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        to_i420(&Bgra {
            width,
            height,
            stride: width as usize * 4,
            pixels,
            captured_at: Instant::now(),
        })
        .unwrap()
    }

    /// The limited-range BT.601 values a browser's decoder turns back into these colours. Each is
    /// within 2 of the textbook value (Y = 16 + 219·Y′, Cb/Cr = 128 + 224·C′).
    #[test]
    fn colours_come_out_as_a_browser_reads_them() {
        for (bgra, (y, u, v)) in [
            ([0, 0, 0, 255], (16, 128, 128)),
            ([255, 255, 255, 255], (235, 128, 128)),
            // Pure red, green and blue: the matrix is BT.601, not BT.709 (red's Y would be 63).
            ([0, 0, 255, 255], (82, 90, 240)),
            ([0, 255, 0, 255], (145, 54, 34)),
            ([255, 0, 0, 255], (41, 240, 110)),
        ] {
            let picture = to_i420(&solid(32, 16, bgra)).unwrap();
            let [(yp, _), (up, _), (vp, _)] = picture.planes();
            for (got, want, plane) in [(yp[0], y, "Y"), (up[0], u, "U"), (vp[0], v, "V")] {
                assert!(
                    got.abs_diff(want) <= 2,
                    "{bgra:?} {plane}: got {got}, want {want}"
                );
            }
        }
    }

    /// The channel order: BGRA in, not RGBA. CONTROL: the same bytes read as RGBA would put the
    /// blue in V's place, so red and blue must come out different.
    #[test]
    fn blue_and_red_are_not_swapped() {
        let blue = to_i420(&solid(16, 16, [255, 0, 0, 255])).unwrap();
        let red = to_i420(&solid(16, 16, [0, 0, 255, 255])).unwrap();
        let (bu, ru) = (blue.planes()[1].0[0], red.planes()[1].0[0]);
        assert!(bu > 200 && ru < 100, "blue U {bu}, red U {ru}");
    }

    /// The row stride is honoured: padding at the end of each row never reaches the picture.
    #[test]
    fn padding_past_the_row_is_ignored() {
        let (w, h) = (16u32, 16u32);
        let stride = w as usize * 4 + 64;
        let mut pixels = vec![0u8; stride * h as usize];
        for row in pixels.chunks_exact_mut(stride) {
            row[..w as usize * 4].copy_from_slice(&[255, 255, 255, 255].repeat(w as usize));
            // White picture, black padding.
        }
        let picture = to_i420(&Bgra {
            width: w,
            height: h,
            stride,
            pixels,
            captured_at: Instant::now(),
        })
        .unwrap();
        assert!(picture.planes()[0].0.iter().all(|&y| y >= 233));
    }

    /// The operator's case, 2026-10-03: "resolution looks a little fuzzy". A 3440×1440 window
    /// streamed to a browser on the shack's own network went out halved, at 1720×720, and every
    /// browser enlarged it again. On that path it goes whole.
    #[test]
    fn a_wide_window_on_the_shacks_network_is_encoded_whole() {
        let lan = Bound::new(Some(Path::Lan), None);
        assert_eq!(encoded_size(3440, 1440, &lan), Some((3440, 1440)));
        assert_eq!(encoded_size(3840, 2160, &lan), Some((3840, 2160)));
        // Past the network's budget, scaled to fit it: here exactly a half.
        assert_eq!(encoded_size(7680, 4320, &lan), Some((3840, 2160)));
    }

    /// Anywhere else the budget is 2560×1600's pixels, and a larger window is scaled to fit it,
    /// keeping its shape, rather than halved. Until the session knows the path, that is the bound.
    #[test]
    fn a_wide_window_on_the_internet_is_scaled_to_the_budget_not_halved() {
        let internet = Bound::new(Some(Path::Internet), None);
        assert_eq!(encoded_size(3440, 1440, &internet), Some((3128, 1308)));
        assert_eq!(encoded_size(3840, 2160, &internet), Some((2698, 1516)));
        assert_eq!(encoded_size(2560, 1600, &internet), Some((2560, 1600)));
        assert_eq!(Bound::new(None, None), internet);
        for (w, h) in [(3440, 1440), (3840, 2160), (5120, 1440)] {
            let (sw, sh) = encoded_size(w, h, &internet).unwrap();
            assert!(u64::from(sw) * u64::from(sh) <= INTERNET_PIXELS, "{w}x{h}");
        }
    }

    /// The page's picture area bounds the picture, by whichever side it reaches first: the station
    /// scales once, and the page shows it pixel for pixel. Never up: a window smaller than the page's
    /// view goes at its own size.
    #[test]
    fn the_pages_view_bounds_the_picture() {
        let lan = |view| Bound::new(Some(Path::Lan), Some(view));
        // The 3440×1440 window, seen in a 1920×1080 browser, a 2560×1440 one and a Retina laptop.
        assert_eq!(
            encoded_size(3440, 1440, &lan((1920, 896))),
            Some((1920, 802))
        );
        assert_eq!(
            encoded_size(3440, 1440, &lan((2560, 1256))),
            Some((2560, 1070))
        );
        assert_eq!(
            encoded_size(3440, 1440, &lan((2880, 1590))),
            Some((2880, 1204))
        );
        // Limited by the view's height.
        assert_eq!(
            encoded_size(1920, 1080, &lan((1920, 600))),
            Some((1066, 600))
        );
        // Never enlarged.
        assert_eq!(
            encoded_size(1920, 1080, &lan((2560, 1256))),
            Some((1920, 1080))
        );
        // The view and the path both bound it: a 5K page on the internet gets the path's budget.
        let far = Bound::new(Some(Path::Internet), Some((5120, 2880)));
        assert_eq!(encoded_size(3440, 1440, &far), Some((3128, 1308)));
    }

    /// However small the page says its picture area is, a window that can be encoded keeps a
    /// picture: the freshness rule needs its frames, and a blind page has no authority.
    #[test]
    fn a_tiny_view_never_takes_the_picture_away() {
        let tiny = Bound::new(Some(Path::Lan), Some((1, 1)));
        assert_eq!((tiny.width, tiny.height), (MIN_VIEW, MIN_VIEW));
        assert_eq!(encoded_size(3440, 1440, &tiny), Some((240, 100)));
        // A sliver of a window keeps at least the smallest side worth encoding.
        assert_eq!(encoded_size(3440, 40, &tiny), Some((240, 16)));
    }

    #[test]
    fn sizes_are_even_and_a_window_too_small_has_none() {
        let any = Bound::new(Some(Path::Lan), None);
        assert_eq!(encoded_size(1201, 721, &any), Some((1200, 720)));
        assert_eq!(encoded_size(15, 400, &any), None);
        assert_eq!(encoded_size(400, 17, &any), Some((400, 16)));
        let picture = to_i420(&solid(1201, 721, [10, 20, 30, 255])).unwrap();
        assert_eq!((picture.width(), picture.height()), (1200, 720));
    }

    #[test]
    fn the_bit_rate_follows_the_picture_and_the_path() {
        let (lan, internet) = (
            Bound::new(Some(Path::Lan), None),
            Bound::new(Some(Path::Internet), None),
        );
        assert_eq!(internet.kbps(1920, 1080), 2073);
        assert_eq!(lan.kbps(1920, 1080), 2073);
        assert_eq!(internet.kbps(320, 180), MIN_KBPS);
        assert_eq!(internet.kbps(2560, 1600), 4000);
        assert_eq!(internet.kbps(3440, 1440), INTERNET_KBPS);
        assert_eq!(lan.kbps(3440, 1440), 4953);
        assert_eq!(lan.kbps(3840, 2160), 8294);
    }

    /// Scaling averages the area each output pixel covers: a one-pixel checkerboard comes out grey
    /// at two thirds of its size (each pixel 4/9 or 5/9 white), where dropping pixels would leave
    /// black (16) and white (235).
    #[test]
    fn scaling_averages_rather_than_drops() {
        let board = drawn(300, 198, |x, y| if (x + y) % 2 == 0 { 0 } else { 255 });
        let small = scale(&board, 200, 132);
        assert_eq!((small.width(), small.height()), (200, 132));
        let [(y, stride), ..] = small.planes();
        for row in 0..132 {
            for x in 0..200 {
                let v = y[row * stride as usize + x];
                assert!((110..=141).contains(&v), "({x}, {row}): {v}");
            }
        }
    }

    /// A flat picture keeps its colour, on every plane, at any size: the shares sum to one.
    #[test]
    fn a_flat_picture_keeps_its_colour() {
        let flat = to_i420(&solid(1720, 720, [40, 160, 220, 255])).unwrap();
        let [(y0, _), (u0, _), (v0, _)] = flat.planes();
        let (y0, u0, v0) = (y0[0], u0[0], v0[0]);
        for (w, h) in [(960, 402), (1278, 534), (1564, 654), (240, 100)] {
            let small = scale(&flat, w, h);
            let [(y, _), (u, _), (v, _)] = small.planes();
            assert!(y.iter().all(|&p| p == y0), "{w}x{h} Y");
            assert!(
                u.iter().all(|&p| p == u0) && v.iter().all(|&p| p == v0),
                "{w}x{h} UV"
            );
        }
    }

    /// An edge that falls on the output grid stays an edge: at exactly half the size, a black half
    /// and a white half come out black and white with nothing between them, and a 2×2 block is its
    /// own average, as the halving it replaces gave.
    #[test]
    fn an_edge_on_the_output_grid_stays_sharp() {
        let half = drawn(64, 32, |x, _| if x < 32 { 16 } else { 235 });
        let small = scale(&half, 32, 16);
        let [(y, stride), ..] = small.planes();
        let row = &y[5 * stride as usize..][..32];
        let (black, white) = (half.planes()[0].0[0], half.planes()[0].0[63]);
        assert!(row[..16].iter().all(|&v| v == black), "{row:?}");
        assert!(row[16..].iter().all(|&v| v == white), "{row:?}");
        let blocks = drawn(16, 16, |x, y| {
            [[0, 60, 200, 255], [20, 100, 50, 90]][y as usize % 2][x as usize % 4]
        });
        let [(src, s), ..] = blocks.planes();
        let halved = scale(&blocks, 8, 8);
        let [(out, _), ..] = halved.planes();
        let mean = |x: usize, y: usize| {
            let at = |dx: usize, dy: usize| u32::from(src[(y + dy) * s as usize + x + dx]);
            ((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8
        };
        assert_eq!([out[0], out[1]], [mean(0, 0), mean(2, 0)]);
    }

    #[test]
    fn a_short_buffer_or_a_tiny_window_encodes_nothing() {
        let mut short = solid(32, 32, [0, 0, 0, 255]);
        short.pixels.truncate(100);
        assert!(to_i420(&short).is_none());
        assert!(to_i420(&solid(8, 8, [0, 0, 0, 255])).is_none());
        // CONTROL: the same picture, whole, encodes.
        assert!(to_i420(&solid(32, 32, [0, 0, 0, 255])).is_some());
    }
}
