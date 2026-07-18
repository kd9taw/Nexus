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
