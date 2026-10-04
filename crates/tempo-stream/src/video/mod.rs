//! The stream's picture (S1, S2): the station's Nexus window, captured, converted to I420 and
//! encoded as VP8, on threads of its own below the radio's priority.
//!
//! - [`picture`] — the captured BGRA, the I420 the encoder takes, and the size and bit rate it is
//!   sent at: no larger than the page shows, within what the path carries (every platform).
//! - [`rate`] — what each frame is sent at when the session estimates the link: the bit rate
//!   follows the estimate, and a weak link steps the frame rate and then the size down (every
//!   platform).
//! - [`pipeline`] — the encoder thread: drop-not-queue, 30 frames a second at most, a still window
//!   twice a second, keyframes when the page asks (every platform, tested with a stand-in encoder).
//! - `capture` — Windows Graphics Capture of the one window (Windows).
//! - `vp8` — libvpx through a C shim (Windows).
//!
//! On any other platform [`Video::start`] answers [`Unavailable`]; admission refuses a stream there
//! long before it is asked.
pub mod picture;
pub mod pipeline;
pub mod rate;

#[cfg(windows)]
mod capture;
#[cfg(windows)]
pub mod vp8;

pub use pipeline::Encoded;

/// This station cannot stream its window: not Windows, no Windows Graphics Capture, no window, or
/// the capture or encoder thread could not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unavailable;

/// Can this station stream `window` (its main window's handle) at all?
pub fn available(window: isize) -> bool {
    #[cfg(windows)]
    {
        capture::available(window)
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        false
    }
}

/// One streamed session's picture, running. Dropping it stops the capture, then the encoder.
pub struct Video {
    // Declared first, so it stops first: nothing more reaches the mailbox once the encoder goes.
    #[cfg(windows)]
    _capture: capture::Capture,
    pipeline: pipeline::Pipeline,
}

impl Video {
    /// Capture and encode `window`, the station's main window: the only window a stream ever
    /// captures (security test A1).
    pub fn start(window: isize) -> Result<Self, Unavailable> {
        #[cfg(windows)]
        {
            use std::sync::Arc;
            let mailbox = Arc::new(pipeline::Mailbox::default());
            let make: pipeline::MakeEncoder = Box::new(|width, height, kbps| {
                vp8::Vp8::open(width, height, kbps)
                    .map(|e| Box::new(e) as Box<dyn pipeline::Encode>)
            });
            let pipeline = pipeline::Pipeline::start(
                mailbox.clone(),
                Box::new(move || capture::showing(window)),
                make,
                capture::lower_priority,
            )
            .map_err(|_| Unavailable)?;
            let capture = capture::Capture::start(window, mailbox).map_err(|_| Unavailable)?;
            Ok(Self {
                _capture: capture,
                pipeline,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = window;
            Err(Unavailable)
        }
    }

    /// The frames encoded since the last call, oldest first.
    pub fn take(&self) -> Vec<Encoded> {
        self.pipeline.take()
    }

    /// Where the page is, how large it shows the picture, in its own device pixels, and what the
    /// link to it carries, in kbit/s (the session's estimate): from the next frame on, the picture
    /// is encoded no larger than the view, within the path's budget ([`picture::Bound`]), at what
    /// the link carries ([`rate`]). `None` for what is not known (or, for the link, not estimated).
    pub fn fit(
        &self,
        path: Option<picture::Path>,
        view: Option<(u32, u32)>,
        estimate: Option<u32>,
    ) {
        self.pipeline
            .set_bound(picture::Bound::new(path, view).within(estimate));
    }

    /// The link the whole picture would need to be sent at its own budget, in kbit/s: what the
    /// session asks its estimation to look for. `None` before the first frame.
    pub fn wanted(&self) -> Option<u32> {
        self.pipeline.wanted()
    }

    /// The page asked for a keyframe.
    pub fn request_keyframe(&self) {
        self.pipeline.request_keyframe();
    }

    /// Has the picture ended for good (the window closed, or a thread stopped)?
    pub fn ended(&self) -> bool {
        #[cfg(windows)]
        let capture_ended = self._capture.ended();
        #[cfg(not(windows))]
        let capture_ended = false;
        capture_ended || self.pipeline.ended()
    }
}
