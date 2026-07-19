//! # tempo-sstv
//!
//! Nexus SSTV receiver **and transmitter** core. The receiver is vendored
//! from the MIT `slowrx` crate v0.5.3
//! (<https://github.com/jasonherald/slowrx.rs>, commit `aa384b4`) — itself a
//! pure-Rust port of [slowrx](https://github.com/windytan/slowrx) by Oona
//! Räisänen (OH2EIQ). Significant portions of the algorithms are translated
//! from the C source. See the [NOTICE file] for full attribution and license
//! preservation, and `README.md` for what Nexus changed (crate rename,
//! CLI dropped, PD50/90/160/290 mode-table additions).
//!
//! ## Status
//!
//! Upstream `0.5.3` + Nexus PD additions. PD50/90/120/160/180/240/290 +
//! Robot 24/36/72 + Scottie 1 / Scottie 2 / Scottie DX + Martin 1 /
//! Martin 2 decoding from raw audio. PD120/PD180 validated against ARISS Dec-2017;
//! Robot 36 validated against the ARISS Fram2 corpus (see
//! `tests/ariss_fram2_validation.md`). Scottie and Martin families
//! are synthetic round-trip-validated only — no Scottie or Martin
//! reference WAVs available. The public API is
//! `#[non_exhaustive]`-protected for additive growth as future
//! mode-family epics land. See
//! <https://github.com/jasonherald/slowrx.rs/issues/9> for the V2 roadmap.
//!
//! The **transmitter** ([`encode`]) is original Nexus code: it synthesizes
//! the full on-air transmission — standard two-segment calibration/VIS
//! header + per-mode scanlines — for all 15 modes, directly at the caller's
//! sample rate. Every mode is TX↔RX self-loopback-validated against the
//! decoder (`tests/tx_loopback.rs`). See [`encode_image`] and
//! [`tx_duration_secs`].
//!
//! ## Example
//!
//! ```
//! # use tempo_sstv::Error;
//! use tempo_sstv::SstvDecoder;
//!
//! // Construct a decoder at the caller's audio sample rate.
//! let mut decoder = SstvDecoder::new(44_100)?;
//!
//! // Feed audio chunks; consume any events that come back.
//! let audio = vec![0.0_f32; 1024];
//! let _events = decoder.process(&audio);
//! # Ok::<(), Error>(())
//! ```
//!
//! [NOTICE file]: https://github.com/jasonherald/slowrx.rs/blob/main/NOTICE.md

#![warn(missing_docs)]

pub(crate) mod demod;
pub(crate) mod dsp;

pub mod decoder;
pub mod encode;
pub(crate) mod encode_pd;
pub(crate) mod encode_robot;
pub(crate) mod encode_scottie;
pub mod error;
pub(crate) mod fsk;
pub mod image;
pub mod mode_pd;
pub mod mode_robot;
pub mod mode_scottie;
pub mod modespec;
pub mod resample;
#[allow(dead_code)]
pub(crate) mod snr;
pub(crate) mod sync;
pub(crate) mod tone;
pub mod vis;

pub use crate::decoder::{SstvDecoder, SstvEvent};
pub use crate::encode::{encode_image, tx_duration_secs, SourceImage};
pub use crate::error::{Error, Result};
pub use crate::image::SstvImage;
pub use crate::modespec::{
    for_mode, lookup as lookup_vis, ChannelLayout, ModeSpec, SstvMode, SyncPosition,
};
pub use crate::resample::{Resampler, WORKING_SAMPLE_RATE_HZ};

/// Test-support — exposed under the `test-support` feature for integration
/// tests in this crate (e.g., `tests/roundtrip.rs`). NOT part of the stable
/// public API; will be hidden behind `#[doc(hidden)]` until V1 publishes.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod __test_support {
    pub mod vis {
        pub use crate::vis::tests::synth_vis;
    }
    pub mod mode_pd {
        pub use crate::demod::ycbcr_to_rgb;

        /// Thin wrapper around the working-rate PD scanline encoder
        /// (`crate::encode_pd::encode_pd`). `__test_support` is the sole
        /// consumer-facing path for the synthetic PD encoder (#86 B10).
        #[doc(hidden)]
        #[must_use]
        pub fn encode_pd(mode: crate::modespec::SstvMode, ycrcb: &[[u8; 3]]) -> Vec<f32> {
            crate::encode_pd::encode_pd(mode, ycrcb)
        }
    }
    pub mod mode_robot {
        /// Thin wrapper around the working-rate Robot scanline encoder
        /// (`crate::encode_robot::encode_robot`). `__test_support` is the sole
        /// consumer-facing path for the synthetic Robot encoder (#86 B10).
        #[doc(hidden)]
        #[must_use]
        pub fn encode_robot(mode: crate::modespec::SstvMode, ycrcb: &[[u8; 3]]) -> Vec<f32> {
            crate::encode_robot::encode_robot(mode, ycrcb)
        }
    }
    pub mod mode_scottie {
        /// Thin wrapper around the working-rate Scottie/Martin scanline encoder
        /// (`crate::encode_scottie::encode_scottie`). `__test_support` is the
        /// sole consumer-facing path for the synthetic Scottie/Martin encoder
        /// (#86 B10).
        #[doc(hidden)]
        #[must_use]
        pub fn encode_scottie(mode: crate::modespec::SstvMode, rgb: &[[u8; 3]]) -> Vec<f32> {
            crate::encode_scottie::encode_scottie(mode, rgb)
        }
    }
}
