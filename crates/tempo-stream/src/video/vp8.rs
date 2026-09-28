//! The VP8 encoder: libvpx, through `vp8_shim.c`.
//!
//! libvpx is linked statically from the pinned source build (scripts/build-windows-cross.sh). The
//! shim is compiled against that build's own headers, so the only thing Rust knows about libvpx is
//! the shim's five functions, declared below.
use std::ffi::{c_char, c_int, c_longlong, c_uint, c_ulong, CStr};
use std::ptr::NonNull;
use std::time::Duration;

use super::picture::I420;
use super::pipeline::{Encode, Packet};

/// The shim's encoder, opaque to Rust.
#[repr(C)]
struct Raw {
    _private: [u8; 0],
}

extern "C" {
    fn nexus_vp8_version() -> *const c_char;
    fn nexus_vp8_open(width: c_uint, height: c_uint, kbps: c_uint) -> *mut Raw;
    #[allow(clippy::too_many_arguments)]
    fn nexus_vp8_encode(
        enc: *mut Raw,
        y: *const u8,
        y_stride: c_int,
        u: *const u8,
        u_stride: c_int,
        v: *const u8,
        v_stride: c_int,
        pts_ms: c_longlong,
        duration_ms: c_ulong,
        keyframe: c_int,
    ) -> c_int;
    fn nexus_vp8_next(
        enc: *mut Raw,
        data: *mut *const u8,
        size: *mut usize,
        keyframe: *mut c_int,
    ) -> c_int;
    fn nexus_vp8_close(enc: *mut Raw);
}

/// The linked libvpx's version string, e.g. `v1.17.0`.
pub fn version() -> String {
    // SAFETY: libvpx returns a pointer to a static, NUL-terminated string.
    unsafe { CStr::from_ptr(nexus_vp8_version()) }
        .to_string_lossy()
        .into_owned()
}

/// The bit rate for a picture size: 1 Mbit/s per million pixels (2 Mbit/s at 1920×1080), never
/// below 300 kbit/s nor above 4 Mbit/s.
pub fn kbps(width: u32, height: u32) -> u32 {
    let pixels = u64::from(width) * u64::from(height);
    (pixels / 1000).clamp(300, 4000) as u32
}

/// One VP8 encoder, for one picture size.
pub struct Vp8 {
    raw: NonNull<Raw>,
    width: u32,
    height: u32,
    last: Option<Duration>,
}

// SAFETY: a libvpx encoder context has no affinity to the thread that made it; `Vp8` is used from
// one thread at a time (`&mut self` on every call) and freed exactly once, in `Drop`.
unsafe impl Send for Vp8 {}

impl Vp8 {
    /// An encoder for `width`×`height` pictures, or `None` if libvpx refused the configuration.
    pub fn open(width: u32, height: u32) -> Option<Self> {
        // SAFETY: plain values in; a null return is handled.
        let raw = unsafe { nexus_vp8_open(width, height, kbps(width, height)) };
        NonNull::new(raw).map(|raw| Self {
            raw,
            width,
            height,
            last: None,
        })
    }
}

impl Encode for Vp8 {
    fn encode(&mut self, picture: &I420, at: Duration, keyframe: bool) -> Option<Vec<Packet>> {
        if (picture.width(), picture.height()) != (self.width, self.height) {
            return None;
        }
        let [(y, ys), (u, us), (v, vs)] = picture.planes();
        // The planes must hold the whole picture at their strides before libvpx reads them.
        let (cw, ch) = (
            self.width.div_ceil(2) as usize,
            self.height.div_ceil(2) as usize,
        );
        let fits = |plane: &[u8], stride: u32, w: usize, h: usize| {
            stride as usize >= w && plane.len() >= stride as usize * h.saturating_sub(1) + w
        };
        if !fits(y, ys, self.width as usize, self.height as usize)
            || !fits(u, us, cw, ch)
            || !fits(v, vs, cw, ch)
        {
            return None;
        }
        let duration = self
            .last
            .map_or(Duration::from_millis(33), |last| at.saturating_sub(last));
        self.last = Some(at);
        // SAFETY: `raw` is a live encoder; each plane pointer is valid for the reads checked above
        // and outlives the call.
        let status = unsafe {
            nexus_vp8_encode(
                self.raw.as_ptr(),
                y.as_ptr(),
                ys as c_int,
                u.as_ptr(),
                us as c_int,
                v.as_ptr(),
                vs as c_int,
                at.as_millis() as c_longlong,
                duration.as_millis().max(1) as c_ulong,
                c_int::from(keyframe),
            )
        };
        if status != 0 {
            return None;
        }
        let mut packets = Vec::new();
        loop {
            let (mut data, mut size, mut key) = (std::ptr::null(), 0usize, 0);
            // SAFETY: `raw` is live; the out-pointers are valid locals.
            let more = unsafe { nexus_vp8_next(self.raw.as_ptr(), &mut data, &mut size, &mut key) };
            if more == 0 {
                break;
            }
            if data.is_null() {
                continue;
            }
            // SAFETY: libvpx hands out `size` readable bytes at `data`, valid until the next
            // encode on this context; they are copied here before anything else touches it.
            let bytes = unsafe { std::slice::from_raw_parts(data, size) }.to_vec();
            packets.push(Packet {
                data: bytes,
                keyframe: key != 0,
            });
        }
        Some(packets)
    }
}

impl Drop for Vp8 {
    fn drop(&mut self) {
        // SAFETY: `raw` came from `nexus_vp8_open` and is closed only here.
        unsafe { nexus_vp8_close(self.raw.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::picture::{to_i420, Bgra};
    use std::time::Instant;

    fn picture(width: u32, height: u32, shade: u8) -> I420 {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                // A gradient with a moving bar, so frames differ.
                let v = ((x + y) as u8).wrapping_add(shade);
                pixels.extend_from_slice(&[v, v / 2, 255 - v, 255]);
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

    /// The linked libvpx is the pinned one.
    #[test]
    fn the_linked_libvpx_is_the_pinned_release() {
        assert_eq!(version(), "v1.17.0");
    }

    /// A real VP8 stream: the first frame is a keyframe carrying VP8's start code and the picture's
    /// size (RFC 6386 §9.1), later frames are inter frames, and a keyframe comes when asked.
    #[test]
    fn frames_are_vp8_and_a_keyframe_comes_when_asked() {
        let (w, h) = (640u32, 360u32);
        let mut enc = Vp8::open(w, h).expect("libvpx refused a plain configuration");
        let first = enc.encode(&picture(w, h, 0), Duration::ZERO, true).unwrap();
        assert_eq!(first.len(), 1);
        let key = &first[0];
        assert!(key.keyframe);
        // Frame tag bit 0 is 0 for a keyframe; then the start code 9d 01 2a and the 14-bit sizes.
        assert_eq!(key.data[0] & 1, 0);
        assert_eq!(&key.data[3..6], &[0x9d, 0x01, 0x2a]);
        let width = u16::from_le_bytes([key.data[6], key.data[7]]) & 0x3fff;
        let height = u16::from_le_bytes([key.data[8], key.data[9]]) & 0x3fff;
        assert_eq!((u32::from(width), u32::from(height)), (w, h));

        let next = enc
            .encode(&picture(w, h, 7), Duration::from_millis(33), false)
            .unwrap();
        assert!(
            !next[0].keyframe,
            "control: an ordinary frame is an inter frame"
        );
        assert_eq!(next[0].data[0] & 1, 1);

        let asked = enc
            .encode(&picture(w, h, 14), Duration::from_millis(66), true)
            .unwrap();
        assert!(asked[0].keyframe);
    }

    /// The twice-a-second floor for a still window stays a small part of the stream's budget.
    /// The first resends are larger (the encoder sharpens what its size-capped keyframe left
    /// soft); measured on a hard synthetic picture at 1280×720, the resends then settle near
    /// 5 KB, under a tenth of the 921 kbit/s the size is given. Asserted with room: each settled
    /// resend under a quarter of the budget's share for its half second.
    #[test]
    fn a_still_window_costs_a_small_part_of_the_budget() {
        let (w, h) = (1280u32, 720u32);
        let mut enc = Vp8::open(w, h).unwrap();
        let still = picture(w, h, 3);
        let mut sizes = Vec::new();
        for n in 0..30u64 {
            let at = Duration::from_millis(500 * n);
            let packets = enc.encode(&still, at, n == 0).unwrap();
            sizes.push(packets.iter().map(|p| p.data.len()).sum::<usize>());
        }
        let half_second = kbps(w, h) as usize * 1000 / 8 / 2;
        let settled = &sizes[20..];
        assert!(
            settled.iter().all(|&s| s < half_second / 4),
            "settled resends {settled:?} against a half-second budget of {half_second} bytes"
        );
    }

    /// A picture of another size is refused rather than read out of bounds.
    #[test]
    fn a_picture_of_the_wrong_size_is_refused() {
        let mut enc = Vp8::open(640, 360).unwrap();
        assert!(enc
            .encode(&picture(320, 180, 0), Duration::ZERO, true)
            .is_none());
        // CONTROL: the right size encodes.
        assert!(enc
            .encode(&picture(640, 360, 0), Duration::ZERO, true)
            .is_some());
    }

    #[test]
    fn the_bit_rate_follows_the_picture() {
        assert_eq!(kbps(1920, 1080), 2073);
        assert_eq!(kbps(320, 180), 300);
        assert_eq!(kbps(2560, 1600), 4000);
    }
}
