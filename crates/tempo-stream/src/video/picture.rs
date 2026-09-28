//! One picture on its way from the window to the encoder: the captured BGRA, and the I420 the
//! encoder takes.
//!
//! - **The size the page gets is the client area's, cut to even.** VP8's 4:2:0 wants even sides;
//!   dropping at most one column and one row keeps the page's input coordinates, which are
//!   fractions of the decoded frame, within a pixel of the window's own.
//! - **A very large window is halved.** Past [`MAX_PIXELS`] (2560×1600) the picture is halved with
//!   a 2×2 average, as often as it takes: a 4K window streams at 1920×1080. Encoding cost and the
//!   bit rate both follow the pixel count, and the station's CPU belongs to the radio first.
//! - **BT.601, limited range.** VP8 carries no colour description, and a browser's decoder assumes
//!   exactly that, so any other matrix would shift every colour on the page.
use std::time::Instant;

use yuv::{
    bgra_to_yuv420, YuvChromaSubsampling, YuvConversionMode, YuvPlanarImageMut, YuvRange,
    YuvStandardMatrix,
};

/// The most pixels the stream is encoded at; a larger window is halved until it fits.
pub const MAX_PIXELS: u64 = 2560 * 1600;
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

/// The size a `width`×`height` window is encoded at, and how many times it is halved to get there.
/// `None` when a side is below [`MIN_SIDE`].
pub fn encoded_size(width: u32, height: u32) -> Option<(u32, u32, u32)> {
    let (mut w, mut h, mut halvings) = (width, height, 0);
    while u64::from(w) * u64::from(h) > MAX_PIXELS {
        w /= 2;
        h /= 2;
        halvings += 1;
    }
    let (w, h) = (w & !1, h & !1);
    (w >= MIN_SIDE && h >= MIN_SIDE).then_some((w, h, halvings))
}

/// Half the picture in each direction, each output pixel the rounded average of a 2×2 block.
fn halve(picture: &Bgra) -> Bgra {
    let (w, h) = (picture.width / 2, picture.height / 2);
    let out_row = w as usize * 4;
    let mut pixels = vec![0u8; out_row * h as usize];
    for (y, out) in pixels.chunks_exact_mut(out_row).enumerate() {
        let top = &picture.pixels[2 * y * picture.stride..];
        let bottom = &picture.pixels[(2 * y + 1) * picture.stride..];
        for (i, value) in out.iter_mut().enumerate() {
            let (x, channel) = (i / 4, i % 4);
            let left = 8 * x + channel;
            let sum = u16::from(top[left])
                + u16::from(top[left + 4])
                + u16::from(bottom[left])
                + u16::from(bottom[left + 4]);
            *value = ((sum + 2) / 4) as u8;
        }
    }
    Bgra {
        width: w,
        height: h,
        stride: out_row,
        pixels,
        captured_at: picture.captured_at,
    }
}

/// The captured picture as the encoder takes it, or `None` if there is nothing worth encoding (a
/// side too small, or a buffer shorter than its own size says).
pub fn to_i420(picture: &Bgra) -> Option<I420> {
    if !picture.whole() {
        return None;
    }
    let (width, height, halvings) = encoded_size(picture.width, picture.height)?;
    let mut halved: Option<Bgra> = None;
    for _ in 0..halvings {
        halved = Some(halve(halved.as_ref().unwrap_or(picture)));
    }
    let source = halved.as_ref().unwrap_or(picture);
    let mut planes = YuvPlanarImageMut::alloc(width, height, YuvChromaSubsampling::Yuv420);
    bgra_to_yuv420(
        &mut planes,
        &source.pixels,
        source.stride as u32,
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

    #[test]
    fn sizes_are_even_and_a_large_window_is_halved() {
        assert_eq!(encoded_size(1201, 721), Some((1200, 720, 0)));
        assert_eq!(encoded_size(2560, 1600), Some((2560, 1600, 0)));
        assert_eq!(encoded_size(3840, 2160), Some((1920, 1080, 1)));
        assert_eq!(encoded_size(7680, 4320), Some((1920, 1080, 2)));
        assert_eq!(encoded_size(15, 400), None);
        assert_eq!(encoded_size(400, 17), Some((400, 16, 0)));
        let picture = to_i420(&solid(3840, 2160, [10, 20, 30, 255])).unwrap();
        assert_eq!((picture.width(), picture.height()), (1920, 1080));
    }

    /// The halving averages: a one-pixel checkerboard of black and white halves to mid-grey, where
    /// dropping every other pixel would give all black or all white.
    #[test]
    fn halving_averages_rather_than_drops() {
        let (w, h) = (3000u32, 2000u32);
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let v = if (x + y) % 2 == 0 { 0 } else { 255 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let picture = to_i420(&Bgra {
            width: w,
            height: h,
            stride: w as usize * 4,
            pixels,
            captured_at: Instant::now(),
        })
        .unwrap();
        assert_eq!((picture.width(), picture.height()), (1500, 1000));
        let y = picture.planes()[0].0[5000];
        assert!((120..=135).contains(&y), "mid-grey expected, got {y}");
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
