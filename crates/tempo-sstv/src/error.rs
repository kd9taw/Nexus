//! Error types for the slowrx crate.
//!
//! Library-only failure modes — anything I/O or codec-shaped belongs to
//! the caller (CLI, examples, integration tests use their own wrappers).

/// Crate-wide error type.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Caller-supplied sample rate is outside the supported range.
    #[error("invalid sample rate: {got} (must be > 0 and ≤ 192000)")]
    InvalidSampleRate {
        /// The rate the caller passed.
        got: u32,
    },
    /// Source image passed to [`crate::encode::encode_image`] is not the
    /// target mode's exact geometry (`line_pixels × image_lines`).
    #[error("image {got_w}×{got_h} ({got_len} px) does not match {mode} ({want_w}×{want_h})")]
    ImageDimensionMismatch {
        /// Human-readable target mode name.
        mode: &'static str,
        /// Required width (the mode's `line_pixels`).
        want_w: u32,
        /// Required height (the mode's `image_lines`).
        want_h: u32,
        /// Supplied image width.
        got_w: u32,
        /// Supplied image height.
        got_h: u32,
        /// Supplied pixel-buffer length.
        got_len: usize,
    },
}

/// Convenient `Result` alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_sample_rate_renders_with_value() {
        let e = Error::InvalidSampleRate { got: 0 };
        assert_eq!(
            e.to_string(),
            "invalid sample rate: 0 (must be > 0 and ≤ 192000)"
        );
    }
}
