//! The binary stream frames (spec §3.4), from the server's side: a header of sixteen `uint32`
//! fields (64 bytes), then the samples.
//!
//! Written for the simulator from the spec, independently of Nexus's client codec.
//!
//! **What the spec leaves loose, and what the simulator does**:
//! 1. **Byte order** is not stated. Every implementation read writes little-endian; so does this.
//! 2. **`length`** is "the number of samples". WSJT-X, AetherSDR and the Go client read it as
//!    sample values (frames × channels); the spec's own advice on minimum sizes only fits frames.
//!    The simulator writes values, or frames under `Fault::LengthAsFrames`, and judges every frame
//!    a client sends by which reading its payload fits ([`fit`]), never by assuming one.
//! 3. **`format` and `channels`** are not always set right by clients: WSJT-X leaves `channels`
//!    unset in its transmit frames, and the Go client writes `4` for float32. The simulator
//!    reports what arrived ([`Header`] as received) instead of guessing past it.
//!
//! Integer samples are full-scale signed values (`i16::MAX` etc. for 1.0); float32 is ±1.0.

/// Bytes in a frame's header.
pub const HEADER_LEN: usize = 64;

/// The stream types (spec §3.4, `enum StreamType`).
pub mod kind {
    /// The receiver's IQ.
    pub const IQ: u32 = 0;
    /// The receiver's audio.
    pub const RX_AUDIO: u32 = 1;
    /// Audio for the transmitter, from a client.
    pub const TX_AUDIO: u32 = 2;
    /// The server asking a client for transmit audio: a header with no samples.
    pub const TX_CHRONO: u32 = 3;
    /// The line-out audio.
    pub const LINE_OUT: u32 = 4;
}

/// A sample format (spec §3.4, `enum SampleType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleType {
    Int16,
    Int24,
    Int32,
    Float32,
}

impl SampleType {
    /// Every format, in code order.
    pub const ALL: [SampleType; 4] = [
        SampleType::Int16,
        SampleType::Int24,
        SampleType::Int32,
        SampleType::Float32,
    ];

    /// The header's `format` code.
    pub fn code(self) -> u32 {
        match self {
            SampleType::Int16 => 0,
            SampleType::Int24 => 1,
            SampleType::Int32 => 2,
            SampleType::Float32 => 3,
        }
    }

    /// The format a header's code names; `None` for any other code.
    pub fn from_code(code: u32) -> Option<SampleType> {
        SampleType::ALL.into_iter().find(|t| t.code() == code)
    }

    /// Bytes per sample value.
    pub fn bytes(self) -> usize {
        match self {
            SampleType::Int16 => 2,
            SampleType::Int24 => 3,
            SampleType::Int32 | SampleType::Float32 => 4,
        }
    }

    /// The name `audio_stream_sample_type` uses.
    pub fn name(self) -> &'static str {
        match self {
            SampleType::Int16 => "int16",
            SampleType::Int24 => "int24",
            SampleType::Int32 => "int32",
            SampleType::Float32 => "float32",
        }
    }

    /// The format `audio_stream_sample_type` names, in any case.
    pub fn from_name(name: &str) -> Option<SampleType> {
        SampleType::ALL
            .into_iter()
            .find(|t| t.name().eq_ignore_ascii_case(name))
    }
}

/// A frame's header, field for field (spec §3.4, `struct Stream`). `kind` is the spec's `type`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Header {
    pub receiver: u32,
    pub sample_rate: u32,
    pub format: u32,
    pub codec: u32,
    pub crc: u32,
    pub length: u32,
    pub kind: u32,
    pub channels: u32,
    pub reserved: [u32; 8],
}

impl Header {
    /// The 64 header bytes, little-endian.
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let fields = [
            self.receiver,
            self.sample_rate,
            self.format,
            self.codec,
            self.crc,
            self.length,
            self.kind,
            self.channels,
        ];
        let mut out = [0u8; HEADER_LEN];
        for (i, v) in fields.iter().chain(self.reserved.iter()).enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// The header at the start of `bytes`; `None` when there are fewer than 64 bytes.
    pub fn decode(bytes: &[u8]) -> Option<Header> {
        let head = bytes.get(..HEADER_LEN)?;
        let word = |i: usize| {
            let mut b = [0u8; 4];
            b.copy_from_slice(&head[i * 4..i * 4 + 4]);
            u32::from_le_bytes(b)
        };
        let mut reserved = [0u32; 8];
        for (i, r) in reserved.iter_mut().enumerate() {
            *r = word(8 + i);
        }
        Some(Header {
            receiver: word(0),
            sample_rate: word(1),
            format: word(2),
            codec: word(3),
            crc: word(4),
            length: word(5),
            kind: word(6),
            channels: word(7),
            reserved,
        })
    }

    /// The header and `payload` as one frame.
    pub fn frame(&self, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        out.extend_from_slice(&self.encode());
        out.extend_from_slice(payload);
        out
    }
}

/// Sample values as they go on the wire in `format`, little-endian. Values beyond ±1.0 are
/// clipped.
pub fn encode_samples(format: SampleType, samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * format.bytes());
    for &s in samples {
        let s = if s.is_nan() { 0.0 } else { s.clamp(-1.0, 1.0) };
        match format {
            SampleType::Float32 => out.extend_from_slice(&s.to_le_bytes()),
            SampleType::Int16 => out.extend_from_slice(
                &((f64::from(s) * f64::from(i16::MAX)).round() as i16).to_le_bytes(),
            ),
            SampleType::Int24 => {
                let v = (f64::from(s) * 8_388_607.0).round() as i32;
                out.extend_from_slice(&v.to_le_bytes()[..3]);
            }
            SampleType::Int32 => out.extend_from_slice(
                &((f64::from(s) * f64::from(i32::MAX)).round() as i32).to_le_bytes(),
            ),
        }
    }
    out
}

/// The sample values in `payload`, read as `format`; `None` when the payload is not a whole number
/// of samples.
pub fn decode_samples(format: SampleType, payload: &[u8]) -> Option<Vec<f32>> {
    let size = format.bytes();
    if !payload.len().is_multiple_of(size) {
        return None;
    }
    Some(
        payload
            .chunks_exact(size)
            .map(|b| match format {
                SampleType::Float32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                SampleType::Int16 => {
                    f32::from(i16::from_le_bytes([b[0], b[1]])) / f32::from(i16::MAX)
                }
                SampleType::Int24 => {
                    // Sign-extend the 24-bit value through the top byte of an i32.
                    let v = i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8;
                    (f64::from(v) / 8_388_607.0) as f32
                }
                SampleType::Int32 => {
                    (f64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])) / f64::from(i32::MAX))
                        as f32
                }
            })
            .collect(),
    )
}

/// Which reading of a header's `length` a payload of `values` sample values fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// `length` counts sample values (frames × channels): WSJT-X's, AetherSDR's and the Go
    /// client's reading.
    Values,
    /// `length` counts frames: the spec's minimum-size advice.
    Frames,
    /// One channel, where both readings are the same.
    Both,
    /// Neither: the frame is wrong for its own header.
    Neither,
}

/// Which reading of `length` (with `channels`) a payload of `values` sample values fits. A
/// `channels` of 0 (unset, as WSJT-X leaves it) can only fit as values.
pub fn fit(length: u32, channels: u32, values: usize) -> Fit {
    let length = length as usize;
    let as_values = values == length;
    let as_frames = channels > 0 && values == length.saturating_mul(channels as usize);
    match (as_values, as_frames) {
        (true, true) => Fit::Both,
        (true, false) => Fit::Values,
        (false, true) => Fit::Frames,
        (false, false) => Fit::Neither,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_round_trips_and_is_little_endian() {
        let h = Header {
            receiver: 1,
            sample_rate: 48_000,
            format: SampleType::Float32.code(),
            codec: 0,
            crc: 0,
            length: 2048,
            kind: kind::RX_AUDIO,
            channels: 2,
            reserved: [0, 0, 0, 0, 0, 0, 0, 0xDEAD_BEEF],
        };
        let bytes = h.encode();
        assert_eq!(&bytes[4..8], &48_000u32.to_le_bytes());
        assert_eq!(&bytes[20..24], &[0x00, 0x08, 0x00, 0x00]); // length 2048
        assert_eq!(&bytes[60..64], &[0xEF, 0xBE, 0xAD, 0xDE]);
        assert_eq!(Header::decode(&bytes), Some(h));
        assert_eq!(Header::decode(&bytes[..63]), None);
        let framed = h.frame(&[1, 2, 3]);
        assert_eq!(framed.len(), HEADER_LEN + 3);
        assert_eq!(Header::decode(&framed), Some(h));
    }

    #[test]
    fn every_format_round_trips_within_its_resolution() {
        let samples = [0.0f32, 0.5, -0.5, 1.0, -1.0, 0.123_456, 2.0, f32::NAN];
        for format in SampleType::ALL {
            let bytes = encode_samples(format, &samples);
            assert_eq!(bytes.len(), samples.len() * format.bytes(), "{format:?}");
            let back = decode_samples(format, &bytes).unwrap();
            let tolerance = match format {
                SampleType::Int16 => 1.0 / 32_767.0,
                _ => 1e-6,
            };
            for (i, (&a, &b)) in samples.iter().zip(&back).enumerate() {
                let want = if a.is_nan() { 0.0 } else { a.clamp(-1.0, 1.0) };
                assert!(
                    (want - b).abs() <= tolerance,
                    "{format:?} [{i}]: {a} -> {b}"
                );
            }
            assert_eq!(SampleType::from_code(format.code()), Some(format));
            assert_eq!(
                SampleType::from_name(&format.name().to_uppercase()),
                Some(format)
            );
        }
        assert_eq!(
            SampleType::from_code(4),
            None,
            "the Go client's float32 code"
        );
        assert_eq!(decode_samples(SampleType::Int24, &[0; 7]), None);
        // A negative 24-bit value sign-extends.
        assert!(
            (decode_samples(SampleType::Int24, &[0x01, 0x00, 0x80]).unwrap()[0] + 1.0).abs() < 1e-6
        );
    }

    #[test]
    fn length_fits_values_frames_both_or_neither() {
        assert_eq!(fit(2048, 2, 2048), Fit::Values);
        assert_eq!(fit(1024, 2, 2048), Fit::Frames);
        assert_eq!(fit(512, 1, 512), Fit::Both);
        assert_eq!(fit(2048, 2, 1000), Fit::Neither);
        // Channels unset, as WSJT-X leaves them: only the values reading can be checked.
        assert_eq!(fit(2048, 0, 2048), Fit::Values);
        assert_eq!(fit(1024, 0, 2048), Fit::Neither);
        // No overflow at the extremes.
        assert_eq!(fit(u32::MAX, u32::MAX, 16), Fit::Neither);
    }
}
