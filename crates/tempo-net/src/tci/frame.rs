//! The TCI binary frame codec: one WebSocket binary message, a 64-byte header and its samples
//! (§3.4), decoded strictly and encoded in both directions (a client's transmit audio; a server's
//! receive audio, IQ, line out and TX_CHRONO).
//!
//! # The header
//!
//! Sixteen `uint32`, 64 bytes, in the order of the spec's `struct Stream`:
//!
//! | Offset | Field | Here |
//! |---|---|---|
//! | 0 | `receiver` | a [`Trx`], below [`MAX_TRX`](super::MAX_TRX) |
//! | 4 | `sample_rate` | Hz: an [`AudioRate`] for audio and TX_CHRONO, an [`IqRate`] for IQ |
//! | 8 | `format` | [`SampleFormat`]: 0 int16, 1 int24, 2 int32, 3 float32 |
//! | 12 | `codec` | 0: compression is not implemented (§3.4), so anything else is refused |
//! | 16 | `crc` | 0 in the spec and not implemented: not read, written as 0 |
//! | 20 | `length` | the number of samples; see below |
//! | 24 | `type` | 0 IQ, 1 receive audio, 2 transmit audio, 3 TX_CHRONO, 4 line out |
//! | 28 | `channels` | [`Channels`]: 1 or 2 |
//! | 32 | `reserv[8]` | not read, written as 0 |
//!
//! # What the spec leaves loose, decided here
//!
//! 1. **Byte order** is not stated. Every implementation read writes little-endian: WSJT-X and
//!    AetherSDR copy the struct on x86, and the Go client (github.com/ftl/tci) says
//!    `binary.LittleEndian`. Little-endian, both ways.
//! 2. **`length`** is "the number of samples", and for IQ the spec divides it by `channels` to count
//!    complex samples, so it counts values, frames times channels ([`LengthMeaning::Values`]);
//!    WSJT-X, AetherSDR, piHPSDR and the Go client all write it so. But the spec's minimum-size
//!    advice (512 samples at 48 kHz for 10 ms, §4.3 `AUDIO_STREAM_SAMPLES`) and its 16384-byte
//!    `data` field fit only a count of frames ([`LengthMeaning::Frames`]). A block is accepted when
//!    its payload is exactly `length` values or exactly `length` frames, and refused when it is
//!    neither ([`FrameError::Length`]); [`Block::reading`] says which one fitted. A TX_CHRONO has
//!    no samples to tell by, so how much it asks for is read with the dialect's meaning
//!    ([`Chrono::frames`], [`Dialect::chrono_length`](super::Dialect::chrono_length)).
//! 3. **`format` and `channels`** are never guessed: a value the spec does not define is refused.
//!    Two clients write such values (WSJT-X leaves `channels` unset in its transmit frames, and the
//!    Go client writes format 4 for float32); as a client Nexus reads only servers' frames.
//!
//! # Samples
//!
//! [`Block::samples`] reads every format as `f32` at full scale ±1.0. A float32 sample that is not
//! finite reads as 0.0 and one beyond ±[`MAX_FLOAT_SAMPLE`] is clamped there: a hostile or broken
//! peer's NaN or 3e38 must not reach a filter, a level meter or an FFT. Encoding clamps every
//! sample to full scale and writes a non-finite one as silence, so Nexus never sends a NaN or an
//! over-range sample to a transmitter.
//!
//! The decoder never panics and never allocates: a [`Block`] borrows its samples from the message.
//! A frame is at most [`MAX_FRAME`] bytes.

use std::fmt;

use super::Trx;

/// The header: sixteen `uint32`.
pub const HEADER_LEN: usize = 64;

/// The largest payload read or written, in bytes: four times the spec's `data[16384]`, and
/// AetherSDR's own 64 KiB message cap.
pub const MAX_PAYLOAD: usize = 64 * 1024;

/// The largest frame read or written.
pub const MAX_FRAME: usize = HEADER_LEN + MAX_PAYLOAD;

/// The most a TX_CHRONO may ask for, in `length` units: four times the spec's largest
/// `AUDIO_STREAM_SAMPLES` (2048). AetherSDR asks for 2048.
pub const MAX_CHRONO_LENGTH: u32 = 8192;

/// A float32 sample's largest magnitude once read. Full scale is 1.0; this leaves room for a peer
/// that scales float audio like int16 (to 32768) and still keeps every sum of squares finite.
pub const MAX_FLOAT_SAMPLE: f32 = 1.0e6;

const IQ: u32 = 0;
const RX_AUDIO: u32 = 1;
const TX_AUDIO: u32 = 2;
const TX_CHRONO: u32 = 3;
const LINE_OUT: u32 = 4;

/// What a block of samples is (§3.4 `StreamType`, without TX_CHRONO, which is a [`Chrono`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockKind {
    /// The receiver's IQ, `type` 0.
    Iq,
    /// Receive audio, `type` 1.
    RxAudio,
    /// Transmit audio, a client's answer to a TX_CHRONO, `type` 2.
    TxAudio,
    /// The line-out audio, `type` 4.
    LineOut,
}

impl BlockKind {
    pub const ALL: [BlockKind; 4] = [
        BlockKind::Iq,
        BlockKind::RxAudio,
        BlockKind::TxAudio,
        BlockKind::LineOut,
    ];

    /// The `type` field.
    pub fn code(self) -> u32 {
        match self {
            BlockKind::Iq => IQ,
            BlockKind::RxAudio => RX_AUDIO,
            BlockKind::TxAudio => TX_AUDIO,
            BlockKind::LineOut => LINE_OUT,
        }
    }

    fn from_code(code: u32) -> Option<BlockKind> {
        BlockKind::ALL.into_iter().find(|k| k.code() == code)
    }

    /// Whether a block of this kind may run at `hz`: an [`IqRate`] for IQ, an [`AudioRate`] for
    /// the rest.
    pub fn allows_rate(self, hz: u32) -> bool {
        match self {
            BlockKind::Iq => IqRate::new(hz).is_some(),
            _ => AudioRate::new(hz).is_some(),
        }
    }
}

/// A sample format (§3.4 `SampleType`, and the words of `AUDIO_STREAM_SAMPLE_TYPE`, §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleFormat {
    Int16,
    /// Three bytes a sample, packed. The spec says "24-bit"; the packing is not stated and no
    /// server was read sending it. Nexus asks for float32, the default.
    Int24,
    Int32,
    Float32,
}

impl SampleFormat {
    pub const ALL: [SampleFormat; 4] = [
        SampleFormat::Int16,
        SampleFormat::Int24,
        SampleFormat::Int32,
        SampleFormat::Float32,
    ];

    /// The `format` field.
    pub fn code(self) -> u32 {
        match self {
            SampleFormat::Int16 => 0,
            SampleFormat::Int24 => 1,
            SampleFormat::Int32 => 2,
            SampleFormat::Float32 => 3,
        }
    }

    pub fn from_code(code: u32) -> Option<SampleFormat> {
        SampleFormat::ALL.into_iter().find(|f| f.code() == code)
    }

    /// The word `audio_stream_sample_type` carries.
    pub fn word(self) -> &'static str {
        match self {
            SampleFormat::Int16 => "int16",
            SampleFormat::Int24 => "int24",
            SampleFormat::Int32 => "int32",
            SampleFormat::Float32 => "float32",
        }
    }

    /// The format a word names, in any case.
    pub fn from_word(word: &str) -> Option<SampleFormat> {
        SampleFormat::ALL
            .into_iter()
            .find(|f| f.word().eq_ignore_ascii_case(word))
    }

    /// Bytes per sample value.
    pub fn bytes(self) -> usize {
        match self {
            SampleFormat::Int16 => 2,
            SampleFormat::Int24 => 3,
            SampleFormat::Int32 | SampleFormat::Float32 => 4,
        }
    }

    /// One value from exactly [`bytes`](Self::bytes) little-endian bytes, at full scale.
    fn read(self, b: &[u8]) -> f32 {
        match (self, b) {
            (SampleFormat::Int16, &[x0, x1]) => f32::from(i16::from_le_bytes([x0, x1])) / 32_768.0,
            // The three bytes in the top of an i32, shifted down: the sign comes along.
            (SampleFormat::Int24, &[x0, x1, x2]) => {
                (i32::from_le_bytes([0, x0, x1, x2]) >> 8) as f32 / 8_388_608.0
            }
            (SampleFormat::Int32, &[x0, x1, x2, x3]) => {
                i32::from_le_bytes([x0, x1, x2, x3]) as f32 / 2_147_483_648.0
            }
            (SampleFormat::Float32, &[x0, x1, x2, x3]) => {
                let v = f32::from_le_bytes([x0, x1, x2, x3]);
                if v.is_finite() {
                    v.clamp(-MAX_FLOAT_SAMPLE, MAX_FLOAT_SAMPLE)
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    }

    /// One value, clamped to full scale (silence if not finite), little-endian.
    fn write(self, v: f32, out: &mut Vec<u8>) {
        let v = if v.is_finite() {
            v.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        match self {
            SampleFormat::Int16 => {
                let x = (v * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16;
                out.extend_from_slice(&x.to_le_bytes());
            }
            SampleFormat::Int24 => {
                let x = (v * 8_388_608.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
                out.extend_from_slice(&x.to_le_bytes()[..3]);
            }
            SampleFormat::Int32 => {
                let x = (f64::from(v) * 2_147_483_648.0)
                    .round()
                    .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
                out.extend_from_slice(&x.to_le_bytes());
            }
            SampleFormat::Float32 => out.extend_from_slice(&v.to_le_bytes()),
        }
    }
}

/// How many channels a stream carries (`channels`, and `AUDIO_STREAM_CHANNELS`, §4.3). IQ is two:
/// I and Q.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channels {
    One,
    Two,
}

impl Channels {
    pub fn count(self) -> usize {
        match self {
            Channels::One => 1,
            Channels::Two => 2,
        }
    }

    pub fn from_count(n: u32) -> Option<Channels> {
        match n {
            1 => Some(Channels::One),
            2 => Some(Channels::Two),
            _ => None,
        }
    }
}

impl fmt::Display for Channels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.count())
    }
}

/// An audio sample rate the spec supports: 8, 12, 24 or 48 kHz (§4.3 `AUDIO_SAMPLERATE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AudioRate(u32);

impl AudioRate {
    pub const HZ: [u32; 4] = [8_000, 12_000, 24_000, 48_000];

    pub fn new(hz: u32) -> Option<AudioRate> {
        AudioRate::HZ.contains(&hz).then_some(AudioRate(hz))
    }

    pub fn hz(self) -> u32 {
        self.0
    }
}

impl fmt::Display for AudioRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An IQ sample rate the spec supports: 48, 96, 192 or 384 kHz (§4.3 `IQ_SAMPLERATE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IqRate(u32);

impl IqRate {
    pub const HZ: [u32; 4] = [48_000, 96_000, 192_000, 384_000];

    pub fn new(hz: u32) -> Option<IqRate> {
        IqRate::HZ.contains(&hz).then_some(IqRate(hz))
    }

    pub fn hz(self) -> u32 {
        self.0
    }
}

impl fmt::Display for IqRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What a `length` field counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LengthMeaning {
    /// Sample values: frames times channels. The spec's IQ wording, and what every
    /// implementation read writes.
    Values,
    /// Frames: one value per channel each. What the spec's size advice fits.
    Frames,
}

/// The header's fields as they arrived, unchecked: for a log line about a refused frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawHeader {
    pub receiver: u32,
    pub sample_rate: u32,
    pub format: u32,
    pub codec: u32,
    pub crc: u32,
    pub length: u32,
    pub kind: u32,
    pub channels: u32,
}

impl RawHeader {
    /// The first [`HEADER_LEN`] bytes as a header; `None` when there are fewer.
    pub fn read(bytes: &[u8]) -> Option<RawHeader> {
        let head = bytes.get(..HEADER_LEN)?;
        let mut w = [0u32; 16];
        for (slot, b) in w.iter_mut().zip(head.chunks_exact(4)) {
            if let &[b0, b1, b2, b3] = b {
                *slot = u32::from_le_bytes([b0, b1, b2, b3]);
            }
        }
        Some(RawHeader {
            receiver: w[0],
            sample_rate: w[1],
            format: w[2],
            codec: w[3],
            crc: w[4],
            length: w[5],
            kind: w[6],
            channels: w[7],
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        for v in [
            self.receiver,
            self.sample_rate,
            self.format,
            self.codec,
            self.crc,
            self.length,
            self.kind,
            self.channels,
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&[0u8; 32]);
    }
}

/// A block's header, checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockHeader {
    pub trx: Trx,
    pub kind: BlockKind,
    /// Hz; one [`BlockKind::allows_rate`] admits.
    pub sample_rate: u32,
    pub format: SampleFormat,
    pub channels: Channels,
}

/// A block of samples, borrowed from the message it arrived in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block<'a> {
    pub header: BlockHeader,
    /// Which reading of `length` the payload fitted. `None` when both give the same size (one
    /// channel, or no samples), so the block says nothing about the server's meaning.
    pub reading: Option<LengthMeaning>,
    data: &'a [u8],
}

impl<'a> Block<'a> {
    /// The samples' bytes, exactly [`values`](Self::values) values.
    pub fn bytes(&self) -> &'a [u8] {
        self.data
    }

    /// How many sample values: frames times channels.
    pub fn values(&self) -> usize {
        self.data.len() / self.header.format.bytes()
    }

    /// How many frames (one value per channel each).
    pub fn frames(&self) -> usize {
        self.values() / self.header.channels.count()
    }

    /// Every value in order (channels interleaved), at full scale ±1.0.
    pub fn samples(&self) -> impl Iterator<Item = f32> + 'a {
        let format = self.header.format;
        self.data
            .chunks_exact(format.bytes())
            .map(move |b| format.read(b))
    }

    /// The values of one channel, 0 first; none for a channel the block does not have.
    pub fn channel(&self, channel: usize) -> impl Iterator<Item = f32> + 'a {
        let count = self.header.channels.count();
        let take = if channel < count { usize::MAX } else { 0 };
        self.samples().skip(channel).step_by(count).take(take)
    }
}

/// A TX_CHRONO: the server asks the client for transmit audio now (§3.4). It is sent without
/// waiting for the answers; a client with nothing to send may send nothing, but silence is
/// preferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chrono {
    pub trx: Trx,
    /// Hz; an [`AudioRate`].
    pub sample_rate: u32,
    pub format: SampleFormat,
    pub channels: Channels,
    /// As the server wrote it; at most [`MAX_CHRONO_LENGTH`].
    pub length: u32,
}

impl Chrono {
    /// How many frames the server asks for, `length` read with `meaning`. A values count that is
    /// not a whole number of frames is rounded down.
    pub fn frames(&self, meaning: LengthMeaning) -> usize {
        let length = self.length as usize;
        match meaning {
            LengthMeaning::Values => length / self.channels.count(),
            LengthMeaning::Frames => length,
        }
    }

    /// The transmit-audio frame that answers this request: this request's receiver, rate, format
    /// and channels, and exactly the frames it asks for (`samples` interleaved, frames times
    /// channels of them; pad with silence when there is less to send).
    pub fn answer(&self, meaning: LengthMeaning, samples: &[f32]) -> Result<Vec<u8>, FrameError> {
        let asked = self.frames(meaning) * self.channels.count();
        if samples.len() != asked {
            return Err(FrameError::AnswerSize {
                asked,
                got: samples.len(),
            });
        }
        let header = BlockHeader {
            trx: self.trx,
            kind: BlockKind::TxAudio,
            sample_rate: self.sample_rate,
            format: self.format,
            channels: self.channels,
        };
        encode_block(&header, meaning, samples)
    }

    /// The request as a server sends it: the header alone.
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        if AudioRate::new(self.sample_rate).is_none() {
            return Err(FrameError::Rate(self.sample_rate));
        }
        if self.length > MAX_CHRONO_LENGTH {
            return Err(FrameError::ChronoLength(self.length));
        }
        let mut out = Vec::with_capacity(HEADER_LEN);
        RawHeader {
            receiver: self.trx.get(),
            sample_rate: self.sample_rate,
            format: self.format.code(),
            codec: 0,
            crc: 0,
            length: self.length,
            kind: TX_CHRONO,
            channels: self.channels.count() as u32,
        }
        .write(&mut out);
        Ok(out)
    }
}

/// One binary message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame<'a> {
    Block(Block<'a>),
    Chrono(Chrono),
}

/// Why a frame is refused (or cannot be encoded). The frame is dropped; the session counts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// Fewer than [`HEADER_LEN`] bytes; the size.
    Short(usize),
    /// More than [`MAX_FRAME`] bytes; the size.
    Long(usize),
    /// A `type` the spec does not define.
    Kind(u32),
    /// A `receiver` at or past [`MAX_TRX`](super::MAX_TRX).
    Receiver(u32),
    /// A `codec` other than 0: compressed samples cannot be read.
    Codec(u32),
    /// A `format` the spec does not define.
    Format(u32),
    /// A `channels` other than 1 or 2.
    Channels(u32),
    /// A `sample_rate` the stream type does not allow.
    Rate(u32),
    /// The payload is neither `length` values nor `length` frames.
    Length {
        length: u32,
        channels: usize,
        payload: usize,
    },
    /// The payload is `length` values, but they are not a whole number of frames.
    Ragged { length: u32 },
    /// A TX_CHRONO asking for more than [`MAX_CHRONO_LENGTH`].
    ChronoLength(u32),
    /// Encoding: the samples are not a whole number of frames.
    Unaligned { samples: usize, channels: usize },
    /// Encoding an answer: not the number of samples the TX_CHRONO asked for.
    AnswerSize { asked: usize, got: usize },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for FrameError {}

/// Read one binary message. Pure; never panics; allocates nothing.
pub fn decode(bytes: &[u8]) -> Result<Frame<'_>, FrameError> {
    if bytes.len() > MAX_FRAME {
        return Err(FrameError::Long(bytes.len()));
    }
    let Some(raw) = RawHeader::read(bytes) else {
        return Err(FrameError::Short(bytes.len()));
    };
    let trx = Trx::new(raw.receiver).ok_or(FrameError::Receiver(raw.receiver))?;
    if raw.codec != 0 {
        return Err(FrameError::Codec(raw.codec));
    }
    let format = SampleFormat::from_code(raw.format).ok_or(FrameError::Format(raw.format))?;
    let channels = Channels::from_count(raw.channels).ok_or(FrameError::Channels(raw.channels))?;
    if raw.kind == TX_CHRONO {
        if AudioRate::new(raw.sample_rate).is_none() {
            return Err(FrameError::Rate(raw.sample_rate));
        }
        // The spec defines no payload for a TX_CHRONO and AetherSDR sends the header alone; one
        // that arrives with bytes after it (a whole `struct Stream`, say) is read for its header.
        if raw.length > MAX_CHRONO_LENGTH {
            return Err(FrameError::ChronoLength(raw.length));
        }
        return Ok(Frame::Chrono(Chrono {
            trx,
            sample_rate: raw.sample_rate,
            format,
            channels,
            length: raw.length,
        }));
    }
    let kind = BlockKind::from_code(raw.kind).ok_or(FrameError::Kind(raw.kind))?;
    if !kind.allows_rate(raw.sample_rate) {
        return Err(FrameError::Rate(raw.sample_rate));
    }
    let data = bytes.get(HEADER_LEN..).unwrap_or_default();
    let reading = fit(raw.length, channels, format, data.len())?;
    Ok(Frame::Block(Block {
        header: BlockHeader {
            trx,
            kind,
            sample_rate: raw.sample_rate,
            format,
            channels,
        },
        reading,
        data,
    }))
}

/// Which reading of `length` a payload of `payload` bytes fits (item 2 in the module header).
fn fit(
    length: u32,
    channels: Channels,
    format: SampleFormat,
    payload: usize,
) -> Result<Option<LengthMeaning>, FrameError> {
    let count = channels.count();
    let as_values = usize::try_from(length)
        .ok()
        .and_then(|l| l.checked_mul(format.bytes()));
    let as_frames = as_values.and_then(|v| v.checked_mul(count));
    match (as_values == Some(payload), as_frames == Some(payload)) {
        (true, true) => Ok(None),
        (true, false) if (length as usize).is_multiple_of(count) => Ok(Some(LengthMeaning::Values)),
        (true, false) => Err(FrameError::Ragged { length }),
        (false, true) => Ok(Some(LengthMeaning::Frames)),
        (false, false) => Err(FrameError::Length {
            length,
            channels: count,
            payload,
        }),
    }
}

/// One block of samples as bytes: `samples` interleaved (frames times channels of them) in
/// `header.format`, with `length` written as `meaning` counts. Each sample is clamped to full
/// scale and a non-finite one is written as silence.
pub fn encode_block(
    header: &BlockHeader,
    meaning: LengthMeaning,
    samples: &[f32],
) -> Result<Vec<u8>, FrameError> {
    let count = header.channels.count();
    if !samples.len().is_multiple_of(count) {
        return Err(FrameError::Unaligned {
            samples: samples.len(),
            channels: count,
        });
    }
    if !header.kind.allows_rate(header.sample_rate) {
        return Err(FrameError::Rate(header.sample_rate));
    }
    let payload = samples.len().saturating_mul(header.format.bytes());
    if payload > MAX_PAYLOAD {
        return Err(FrameError::Long(HEADER_LEN.saturating_add(payload)));
    }
    let length = match meaning {
        LengthMeaning::Values => samples.len(),
        LengthMeaning::Frames => samples.len() / count,
    };
    let mut out = Vec::with_capacity(HEADER_LEN + payload);
    RawHeader {
        receiver: header.trx.get(),
        sample_rate: header.sample_rate,
        format: header.format.code(),
        codec: 0,
        crc: 0,
        // At most MAX_PAYLOAD / 2 values, checked above.
        length: length as u32,
        kind: header.kind.code(),
        channels: count as u32,
    }
    .write(&mut out);
    for &v in samples {
        header.format.write(v, &mut out);
    }
    Ok(out)
}

/// Counts what [`decode`] accepted and refused, for the session to report ("reject a
/// frame that fits neither, and count it").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub accepted: u64,
    pub refused: u64,
    /// The last refusal.
    pub last: Option<FrameError>,
}

impl Tally {
    /// [`decode`], counted.
    pub fn decode<'a>(&mut self, bytes: &'a [u8]) -> Option<Frame<'a>> {
        match decode(bytes) {
            Ok(frame) => {
                self.accepted = self.accepted.saturating_add(1);
                Some(frame)
            }
            Err(e) => {
                self.refused = self.refused.saturating_add(1);
                self.last = Some(e);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tci::MAX_TRX;
    use proptest::prelude::*;

    fn trx(n: u32) -> Trx {
        Trx::new(n).expect("a receiver number below MAX_TRX")
    }

    /// A header in the spec's field order (§3.4), every field given.
    fn header(
        receiver: u32,
        rate: u32,
        format: u32,
        codec: u32,
        length: u32,
        kind: u32,
        channels: u32,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        RawHeader {
            receiver,
            sample_rate: rate,
            format,
            codec,
            crc: 0,
            length,
            kind,
            channels,
        }
        .write(&mut out);
        out
    }

    fn with_payload(mut head: Vec<u8>, payload: usize) -> Vec<u8> {
        head.resize(HEADER_LEN + payload, 0);
        head
    }

    fn block(frame: Result<Frame<'_>, FrameError>) -> Block<'_> {
        match frame {
            Ok(Frame::Block(b)) => b,
            other => panic!("not a block: {other:?}"),
        }
    }

    fn rx_audio(format: SampleFormat, channels: Channels) -> BlockHeader {
        BlockHeader {
            trx: trx(0),
            kind: BlockKind::RxAudio,
            sample_rate: 48_000,
            format,
            channels,
        }
    }

    #[test]
    fn the_header_is_sixteen_little_endian_words_in_the_spec_order() {
        // §3.4 `struct Stream`: receiver, sample_rate, format, codec, crc, length, type, channels,
        // reserv[8]; then the samples.
        let bytes = encode_block(
            &BlockHeader {
                trx: trx(1),
                kind: BlockKind::TxAudio,
                sample_rate: 48_000,
                format: SampleFormat::Float32,
                channels: Channels::Two,
            },
            LengthMeaning::Values,
            &[0.5, -0.5, 0.25, -0.25],
        )
        .expect("encodes");
        assert_eq!(bytes.len(), HEADER_LEN + 16);
        let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        assert_eq!(word(0), 1, "receiver");
        assert_eq!(word(1), 48_000, "sample_rate");
        assert_eq!(word(2), 3, "format: float32");
        assert_eq!(word(3), 0, "codec");
        assert_eq!(word(4), 0, "crc");
        assert_eq!(word(5), 4, "length: four values");
        assert_eq!(word(6), 2, "type: TX audio");
        assert_eq!(word(7), 2, "channels");
        assert!(bytes[32..64].iter().all(|&b| b == 0), "reserved");
        assert_eq!(&bytes[64..68], &0.5f32.to_le_bytes());
        // The same bytes read back.
        let raw = RawHeader::read(&bytes).expect("64 bytes");
        assert_eq!(
            (raw.receiver, raw.length, raw.kind, raw.channels),
            (1, 4, 2, 2)
        );
    }

    #[test]
    fn each_format_reads_at_full_scale() {
        let read = |format: SampleFormat, bytes: &[u8]| {
            let mut frame = header(0, 48_000, format.code(), 0, 1, RX_AUDIO, 1);
            frame.extend_from_slice(bytes);
            block(decode(&frame)).samples().collect::<Vec<_>>()
        };
        assert_eq!(read(SampleFormat::Int16, &[0x00, 0x80]), [-1.0]);
        assert_eq!(read(SampleFormat::Int16, &[0x00, 0x40]), [0.5]);
        assert_eq!(
            read(SampleFormat::Int16, &[0xff, 0x7f]),
            [32_767.0 / 32_768.0]
        );
        // int24: three bytes, the sign carried from the top one.
        assert_eq!(read(SampleFormat::Int24, &[0x00, 0x00, 0x80]), [-1.0]);
        assert_eq!(read(SampleFormat::Int24, &[0x00, 0x00, 0xc0]), [-0.5]);
        assert_eq!(
            read(SampleFormat::Int24, &[0xff, 0xff, 0xff]),
            [-1.0 / 8_388_608.0]
        );
        assert_eq!(read(SampleFormat::Int24, &[0x00, 0x00, 0x40]), [0.5]);
        assert_eq!(read(SampleFormat::Int32, &[0, 0, 0, 0x80]), [-1.0]);
        assert_eq!(read(SampleFormat::Int32, &[0, 0, 0, 0x40]), [0.5]);
        assert_eq!(read(SampleFormat::Float32, &0.25f32.to_le_bytes()), [0.25]);
    }

    #[test]
    fn a_float_that_is_not_finite_reads_as_silence_and_a_huge_one_is_clamped() {
        let mut frame = header(0, 48_000, 3, 0, 5, RX_AUDIO, 1);
        for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 3.0e38, -3.0e38] {
            frame.extend_from_slice(&v.to_le_bytes());
        }
        let got: Vec<f32> = block(decode(&frame)).samples().collect();
        assert_eq!(
            got,
            [0.0, 0.0, 0.0, MAX_FLOAT_SAMPLE, -MAX_FLOAT_SAMPLE],
            "MAX_FLOAT_SAMPLE bounds what a peer's float can put in a filter"
        );
        // A peer scaling float audio like int16 keeps its samples.
        let mut frame = header(0, 48_000, 3, 0, 1, RX_AUDIO, 1);
        frame.extend_from_slice(&(-32_768.0f32).to_le_bytes());
        assert_eq!(
            block(decode(&frame)).samples().collect::<Vec<_>>(),
            [-32_768.0]
        );
    }

    #[test]
    fn encoding_clamps_to_full_scale_and_sends_silence_for_a_non_finite_sample() {
        let bytes = encode_block(
            &BlockHeader {
                trx: trx(0),
                kind: BlockKind::TxAudio,
                sample_rate: 48_000,
                format: SampleFormat::Float32,
                channels: Channels::One,
            },
            LengthMeaning::Values,
            &[f32::NAN, f32::INFINITY, 2.0, -7.5, 0.5],
        )
        .expect("encodes");
        // On the wire, not just as the decoder reads it back (the decoder sanitizes too).
        let sent: Vec<f32> = bytes[HEADER_LEN..]
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(sent, [0.0, 0.0, 1.0, -1.0, 0.5]);
        let back: Vec<f32> = block(decode(&bytes)).samples().collect();
        assert_eq!(back, [0.0, 0.0, 1.0, -1.0, 0.5]);
        // The integer formats clamp the same way rather than wrapping.
        for format in [
            SampleFormat::Int16,
            SampleFormat::Int24,
            SampleFormat::Int32,
        ] {
            let bytes = encode_block(
                &rx_audio(format, Channels::One),
                LengthMeaning::Values,
                &[1.5, -1.5],
            )
            .expect("encodes");
            let back: Vec<f32> = block(decode(&bytes)).samples().collect();
            assert!(back[0] > 0.99 && back[0] <= 1.0, "{format:?}: {back:?}");
            assert_eq!(back[1], -1.0, "{format:?}");
        }
    }

    #[test]
    fn both_readings_of_length_are_accepted_when_the_payload_fits_one() {
        // Two channels of float32 and `length` 8: eight values (32 bytes) or eight frames (64).
        let values = with_payload(header(0, 48_000, 3, 0, 8, RX_AUDIO, 2), 32);
        let b = block(decode(&values));
        assert_eq!(b.reading, Some(LengthMeaning::Values));
        assert_eq!((b.values(), b.frames()), (8, 4));
        let frames = with_payload(header(0, 48_000, 3, 0, 8, RX_AUDIO, 2), 64);
        let b = block(decode(&frames));
        assert_eq!(b.reading, Some(LengthMeaning::Frames));
        assert_eq!((b.values(), b.frames()), (16, 8));
        // One channel: the readings agree, so the block says nothing about the server's meaning.
        let mono = with_payload(header(0, 48_000, 3, 0, 8, RX_AUDIO, 1), 32);
        assert_eq!(block(decode(&mono)).reading, None);
        // No samples at all is a block of nothing, not an error.
        let empty = header(0, 48_000, 3, 0, 0, RX_AUDIO, 2);
        let b = block(decode(&empty));
        assert_eq!((b.values(), b.reading), (0, None));
    }

    /// A frame of the wrong size is refused, counted, and never panics.
    #[test]
    fn a_frame_of_the_wrong_size_is_refused_counted_and_never_panics() {
        let mut tally = Tally::default();
        let mut sent = 0u64;
        for format in SampleFormat::ALL {
            for channels in [Channels::One, Channels::Two] {
                let bps = format.bytes();
                let n = channels.count();
                let length = 6u32;
                let good = [6 * bps, 6 * bps * n];
                for payload in 0..=6 * bps * n + 2 * bps {
                    let frame = with_payload(
                        header(0, 48_000, format.code(), 0, length, RX_AUDIO, n as u32),
                        payload,
                    );
                    let got = tally.decode(&frame);
                    sent += 1;
                    if good.contains(&payload) {
                        assert!(got.is_some(), "{format:?} x{n}: {payload} bytes fits");
                    } else {
                        assert!(
                            got.is_none(),
                            "{format:?} x{n}: {payload} bytes for length 6 must be refused"
                        );
                        assert_eq!(
                            tally.last,
                            Some(FrameError::Length {
                                length,
                                channels: n,
                                payload
                            }),
                            "{format:?} x{n}: {payload} bytes"
                        );
                    }
                }
            }
        }
        // 4 formats x 2 channel counts; the accepted frames are the readings that fit (one size
        // for mono, two for stereo). Every other frame was counted as refused.
        assert_eq!(tally.accepted, 4 * (1 + 2));
        assert_eq!(tally.refused, sent - tally.accepted);
    }

    /// The length check is exact both ways: a payload one byte past either reading, or a `length`
    /// that claims more than arrived, is refused rather than truncated or read past.
    #[test]
    fn a_length_that_claims_more_than_arrived_is_refused() {
        let frame = with_payload(header(0, 48_000, 3, 0, 4096, RX_AUDIO, 2), 16);
        assert_eq!(
            decode(&frame),
            Err(FrameError::Length {
                length: 4096,
                channels: 2,
                payload: 16
            })
        );
        // u32::MAX values would overflow a 32-bit size; it is simply not the payload.
        let frame = with_payload(header(0, 48_000, 0, 0, u32::MAX, RX_AUDIO, 2), 8);
        assert!(matches!(decode(&frame), Err(FrameError::Length { .. })));
    }

    #[test]
    fn values_that_are_not_a_whole_number_of_frames_are_refused() {
        // Seven float32 values in a stereo stream: three and a half frames.
        let frame = with_payload(header(0, 48_000, 3, 0, 7, RX_AUDIO, 2), 28);
        assert_eq!(decode(&frame), Err(FrameError::Ragged { length: 7 }));
    }

    #[test]
    fn a_frame_shorter_than_the_header_is_refused() {
        for n in [0, 1, 32, 63] {
            assert_eq!(decode(&vec![0u8; n]), Err(FrameError::Short(n)), "{n}");
        }
        assert_eq!(RawHeader::read(&[0u8; 63]), None);
    }

    #[test]
    fn a_frame_longer_than_max_frame_is_refused_even_when_its_length_fits() {
        // 20000 float32 values: 80000 bytes, a payload its own `length` describes exactly.
        let frame = with_payload(header(0, 48_000, 3, 0, 20_000, RX_AUDIO, 1), 80_000);
        assert_eq!(decode(&frame), Err(FrameError::Long(HEADER_LEN + 80_000)));
        // The largest frame that fits is read.
        let frame = with_payload(header(0, 48_000, 3, 0, 16_384, RX_AUDIO, 1), MAX_PAYLOAD);
        assert_eq!(block(decode(&frame)).values(), 16_384);
    }

    #[test]
    fn header_fields_outside_the_spec_are_refused() {
        let ok = |kind: u32| header(0, 48_000, 3, 0, 0, kind, 2);
        assert!(decode(&ok(RX_AUDIO)).is_ok());
        assert_eq!(
            decode(&header(MAX_TRX, 48_000, 3, 0, 0, RX_AUDIO, 2)),
            Err(FrameError::Receiver(MAX_TRX))
        );
        assert_eq!(
            decode(&header(255, 48_000, 3, 0, 0, RX_AUDIO, 2)),
            Err(FrameError::Receiver(255))
        );
        assert_eq!(
            decode(&header(0, 48_000, 3, 1, 0, RX_AUDIO, 2)),
            Err(FrameError::Codec(1))
        );
        // Format 4 is the Go client's float32 and WSJT-X's float64 comment: not the spec's.
        assert_eq!(
            decode(&header(0, 48_000, 4, 0, 0, RX_AUDIO, 2)),
            Err(FrameError::Format(4))
        );
        for channels in [0, 3, 0xdead_beef] {
            assert_eq!(
                decode(&header(0, 48_000, 3, 0, 0, RX_AUDIO, channels)),
                Err(FrameError::Channels(channels))
            );
        }
        assert_eq!(decode(&ok(5)), Err(FrameError::Kind(5)));
        // An audio stream at an IQ-only rate, IQ at an audio-only rate, and a rate of nothing.
        assert_eq!(
            decode(&header(0, 96_000, 3, 0, 0, RX_AUDIO, 2)),
            Err(FrameError::Rate(96_000))
        );
        assert_eq!(
            decode(&header(0, 12_000, 3, 0, 0, IQ, 2)),
            Err(FrameError::Rate(12_000))
        );
        assert_eq!(
            decode(&header(0, 0, 3, 0, 0, LINE_OUT, 1)),
            Err(FrameError::Rate(0))
        );
        assert_eq!(
            decode(&header(0, 0, 3, 0, 2048, TX_CHRONO, 2)),
            Err(FrameError::Rate(0))
        );
        // The crc is never read (the spec does not implement it).
        let mut crc = header(0, 48_000, 3, 0, 0, RX_AUDIO, 2);
        crc[16..20].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        assert!(decode(&crc).is_ok());
    }

    #[test]
    fn a_chrono_asks_for_frames_by_the_dialects_reading_and_its_answer_fits_it() {
        // AetherSDR's request: 48 kHz float32 stereo, `length` 2048, header only.
        let request = header(0, 48_000, 3, 0, 2048, TX_CHRONO, 2);
        let chrono = match decode(&request) {
            Ok(Frame::Chrono(c)) => c,
            other => panic!("{other:?}"),
        };
        assert_eq!(chrono.frames(LengthMeaning::Values), 1024);
        assert_eq!(chrono.frames(LengthMeaning::Frames), 2048);
        assert_eq!(chrono.encode().as_deref(), Ok(request.as_slice()));
        // The answer: a TX-audio frame on the same receiver, rate, format and channels, `length`
        // counted the same way, 1024 stereo frames of samples.
        let samples = vec![0.25f32; 2048];
        let answer = chrono
            .answer(LengthMeaning::Values, &samples)
            .expect("the size it asked for");
        let b = block(decode(&answer));
        assert_eq!(b.header.kind, BlockKind::TxAudio);
        assert_eq!(
            b.header,
            BlockHeader {
                kind: BlockKind::TxAudio,
                ..rx_audio(SampleFormat::Float32, Channels::Two)
            }
        );
        assert_eq!((b.frames(), b.reading), (1024, Some(LengthMeaning::Values)));
        assert_eq!(RawHeader::read(&answer).map(|h| h.length), Some(2048));
        // Read as frames, the same request wants twice as much; the answer says so.
        let answer = chrono
            .answer(LengthMeaning::Frames, &vec![0.0; 4096])
            .expect("the size it asked for");
        assert_eq!(RawHeader::read(&answer).map(|h| h.length), Some(2048));
        assert_eq!(block(decode(&answer)).frames(), 2048);
        // Anything but the size asked for is not an answer.
        assert_eq!(
            chrono.answer(LengthMeaning::Values, &samples[..2046]),
            Err(FrameError::AnswerSize {
                asked: 2048,
                got: 2046
            })
        );
        assert_eq!(
            chrono.answer(LengthMeaning::Values, &vec![0.0; 2050]),
            Err(FrameError::AnswerSize {
                asked: 2048,
                got: 2050
            })
        );
    }

    #[test]
    fn a_chrono_with_bytes_after_its_header_is_read_for_the_header() {
        // A server that sends a whole `struct Stream` (64 + 16384 bytes) for its request.
        let request = with_payload(header(1, 48_000, 3, 0, 512, TX_CHRONO, 2), 16_384);
        assert!(matches!(
            decode(&request),
            Ok(Frame::Chrono(Chrono { length: 512, .. }))
        ));
    }

    #[test]
    fn a_chrono_asking_for_more_than_max_chrono_length_is_refused() {
        let ask = |length: u32| {
            decode(&header(0, 48_000, 3, 0, length, TX_CHRONO, 2)).map(|f| match f {
                Frame::Chrono(c) => Some(c),
                Frame::Block(_) => None,
            })
        };
        assert!(matches!(ask(MAX_CHRONO_LENGTH), Ok(Some(_))));
        assert_eq!(
            ask(MAX_CHRONO_LENGTH + 1),
            Err(FrameError::ChronoLength(MAX_CHRONO_LENGTH + 1))
        );
        assert_eq!(ask(u32::MAX), Err(FrameError::ChronoLength(u32::MAX)));
        let chrono = Chrono {
            trx: trx(0),
            sample_rate: 48_000,
            format: SampleFormat::Float32,
            channels: Channels::Two,
            length: MAX_CHRONO_LENGTH + 1,
        };
        assert_eq!(
            chrono.encode(),
            Err(FrameError::ChronoLength(MAX_CHRONO_LENGTH + 1))
        );
    }

    #[test]
    fn encoding_refuses_a_payload_over_max_payload_and_samples_that_are_not_whole_frames() {
        let mono = rx_audio(SampleFormat::Float32, Channels::One);
        let most = vec![0.0f32; MAX_PAYLOAD / 4];
        assert_eq!(
            encode_block(&mono, LengthMeaning::Values, &most).map(|b| b.len()),
            Ok(MAX_FRAME)
        );
        let over = vec![0.0f32; MAX_PAYLOAD / 4 + 1];
        assert_eq!(
            encode_block(&mono, LengthMeaning::Values, &over),
            Err(FrameError::Long(MAX_FRAME + 4))
        );
        let stereo = rx_audio(SampleFormat::Float32, Channels::Two);
        assert_eq!(
            encode_block(&stereo, LengthMeaning::Values, &[0.0; 3]),
            Err(FrameError::Unaligned {
                samples: 3,
                channels: 2
            })
        );
        let iq_at_12k = BlockHeader {
            kind: BlockKind::Iq,
            sample_rate: 12_000,
            ..stereo
        };
        assert_eq!(
            encode_block(&iq_at_12k, LengthMeaning::Values, &[0.0; 2]),
            Err(FrameError::Rate(12_000))
        );
    }

    #[test]
    fn a_channel_reads_its_own_values_and_a_missing_channel_reads_none() {
        let bytes = encode_block(
            &rx_audio(SampleFormat::Float32, Channels::Two),
            LengthMeaning::Values,
            &[0.1, -0.1, 0.2, -0.2, 0.3, -0.3],
        )
        .expect("encodes");
        let b = block(decode(&bytes));
        assert_eq!(b.channel(0).collect::<Vec<_>>(), [0.1, 0.2, 0.3]);
        assert_eq!(b.channel(1).collect::<Vec<_>>(), [-0.1, -0.2, -0.3]);
        assert_eq!(b.channel(2).count(), 0);
        assert_eq!(b.channel(usize::MAX).count(), 0);
    }

    #[test]
    fn the_tally_counts_what_it_accepts_and_refuses() {
        let mut tally = Tally::default();
        assert!(tally
            .decode(&header(0, 48_000, 3, 0, 0, RX_AUDIO, 2))
            .is_some());
        assert!(tally.decode(&[0u8; 10]).is_none());
        assert!(tally
            .decode(&header(0, 48_000, 9, 0, 0, RX_AUDIO, 2))
            .is_none());
        assert_eq!(
            tally,
            Tally {
                accepted: 1,
                refused: 2,
                last: Some(FrameError::Format(9)),
            }
        );
    }

    /// A value on `format`'s grid: what it reads back exactly.
    fn on_grid(format: SampleFormat) -> BoxedStrategy<f32> {
        match format {
            SampleFormat::Int16 => (i16::MIN..=i16::MAX)
                .prop_map(|k| f32::from(k) / 32_768.0)
                .boxed(),
            SampleFormat::Int24 => (-8_388_608i32..=8_388_607)
                .prop_map(|k| k as f32 / 8_388_608.0)
                .boxed(),
            // An i32 whose low byte is zero has at most 24 significant bits, which an f32 holds.
            SampleFormat::Int32 => (-8_388_608i32..=8_388_607)
                .prop_map(|k| (k * 256) as f32 / 2_147_483_648.0)
                .boxed(),
            SampleFormat::Float32 => (-1.0f32..=1.0).boxed(),
        }
    }

    fn block_case() -> impl Strategy<Value = (BlockHeader, LengthMeaning, Vec<f32>)> {
        let kind = prop::sample::select(BlockKind::ALL.to_vec());
        let format = prop::sample::select(SampleFormat::ALL.to_vec());
        let channels = prop::sample::select(vec![Channels::One, Channels::Two]);
        let meaning = prop::sample::select(vec![LengthMeaning::Values, LengthMeaning::Frames]);
        (0..MAX_TRX, kind, format, channels, meaning, 0usize..4).prop_flat_map(
            |(t, kind, format, channels, meaning, rate)| {
                let sample_rate = match kind {
                    BlockKind::Iq => IqRate::HZ[rate],
                    _ => AudioRate::HZ[rate],
                };
                let header = BlockHeader {
                    trx: trx(t),
                    kind,
                    sample_rate,
                    format,
                    channels,
                };
                let frames = 0usize..64;
                (Just(header), Just(meaning), frames).prop_flat_map(move |(h, m, n)| {
                    (
                        Just(h),
                        Just(m),
                        prop::collection::vec(on_grid(format), n * channels.count()),
                    )
                })
            },
        )
    }

    /// A header with every field drawn near the spec's values, and a payload of any size.
    fn near_header() -> impl Strategy<Value = Vec<u8>> {
        (
            prop::sample::select(vec![0u32, 1, 15, 16, u32::MAX]),
            prop::sample::select(vec![0u32, 8_000, 12_000, 48_000, 96_000, 384_000, u32::MAX]),
            0u32..6,
            prop::sample::select(vec![0u32, 0, 0, 1]),
            prop::sample::select(vec![0u32, 1, 2, 7, 64, 2048, 8192, 8193, u32::MAX]),
            0u32..6,
            0u32..4,
            0usize..2100,
        )
            .prop_map(
                |(r, rate, format, codec, length, kind, channels, payload)| {
                    with_payload(
                        header(r, rate, format, codec, length, kind, channels),
                        payload,
                    )
                },
            )
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

        /// Every block that can be encoded reads back as itself: header, reading and samples.
        #[test]
        fn every_block_round_trips((header, meaning, samples) in block_case()) {
            let bytes = encode_block(&header, meaning, &samples).expect("encodes");
            let b = block(decode(&bytes));
            prop_assert_eq!(b.header, header);
            let expected = if header.channels == Channels::One || samples.is_empty() {
                None
            } else {
                Some(meaning)
            };
            prop_assert_eq!(b.reading, expected);
            prop_assert_eq!(b.samples().collect::<Vec<_>>(), samples);
        }

        /// Every request that can be encoded reads back as itself.
        #[test]
        fn every_chrono_round_trips(
            t in 0..MAX_TRX,
            rate in prop::sample::select(AudioRate::HZ.to_vec()),
            format in prop::sample::select(SampleFormat::ALL.to_vec()),
            channels in prop::sample::select(vec![Channels::One, Channels::Two]),
            length in 0..=MAX_CHRONO_LENGTH,
        ) {
            let chrono = Chrono { trx: trx(t), sample_rate: rate, format, channels, length };
            let bytes = chrono.encode().expect("encodes");
            prop_assert_eq!(decode(&bytes), Ok(Frame::Chrono(chrono)));
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 4096, ..ProptestConfig::default() })]

        /// The fuzz: any bytes at all. Never a panic; anything accepted is internally consistent.
        #[test]
        fn any_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..600)) {
            check(&bytes)?;
        }

        /// The fuzz, aimed: a header near the spec's values with a payload of any size.
        #[test]
        fn any_header_near_the_spec_never_panics(bytes in near_header()) {
            check(&bytes)?;
        }
    }

    /// What every accepted frame must satisfy, whatever the input.
    fn check(bytes: &[u8]) -> Result<(), TestCaseError> {
        match decode(bytes) {
            Ok(Frame::Block(b)) => {
                let bps = b.header.format.bytes();
                let n = b.header.channels.count();
                prop_assert_eq!(b.bytes().len(), b.values() * bps);
                prop_assert_eq!(b.values(), b.frames() * n);
                prop_assert!(b.bytes().len() <= MAX_PAYLOAD);
                prop_assert!(b.header.kind.allows_rate(b.header.sample_rate));
                let raw = RawHeader::read(bytes).expect("a header");
                let claimed = raw.length as usize;
                prop_assert!(b.values() == claimed || b.frames() == claimed);
                for v in b.samples() {
                    prop_assert!(v.is_finite() && v.abs() <= MAX_FLOAT_SAMPLE);
                }
            }
            Ok(Frame::Chrono(c)) => {
                prop_assert!(c.length <= MAX_CHRONO_LENGTH);
                prop_assert!(AudioRate::new(c.sample_rate).is_some());
            }
            Err(_) => {}
        }
        Ok(())
    }
}
