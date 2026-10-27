//! TCI audio frames, at both ends of a TCI connection: the samples a binary frame carries, read
//! into Nexus's audio and cut out of it (TCI 2.0 §3.4 and §4.3). Nexus as the server: a program's
//! transmit frames become 12 kHz mono for the program's over ([`FromProgram`]), and the receive
//! audio becomes the program's frames ([`ToProgram`]). Nexus as a client: a server's receive audio
//! becomes 12 kHz mono ([`FromServer`]), and Nexus's transmit audio is pulled at the server's rate
//! for each TX_CHRONO ([`ToServer`], [`ChronoAsk`]). Formats, rates, `length`, sanitising and the
//! resampling between Nexus's rate and the other end's, both ways. Pure: no socket, no engine, no
//! clock of its own.
//!
//! # What lives here, and what lives in `tempo_net::tci::frame`
//!
//! The 64-byte header, its checks, TX_CHRONO and the block encoder are `frame`'s. This crate does
//! not depend on tempo-net (the phone client builds on tempo-codec alone), so nothing here reads or
//! writes a header: a caller reads one with `frame::RawHeader::read` and hands [`FromProgram`] its
//! fields in a [`TxHeader`], and encodes [`ToProgram`]'s frames with `frame::encode_block`. The
//! spec's numbers the two modules share are restated here from the spec: the format codes
//! ([`Format`]), the audio rates ([`RATES`]) and the payload bound ([`MAX_PAYLOAD`]).
//!
//! # `length`
//!
//! A frame's `length` counts sample values, frames times channels: `frame`'s reading of the spec,
//! and how WSJT-X, MSHV, the Go client and AetherSDR write and read it. A program's receive frames
//! hold [`Stream::values`] values, and the server's TX_CHRONO asks for the same number. A client
//! reads a server's TX_CHRONO in values too, unless the server's dialect counts frames
//! ([`ChronoAsk::in_values`], [`ChronoAsk::in_frames`]).
//!
//! # A program's transmit frames
//!
//! [`FromProgram::accept`] takes a frame only when its header is the program's own stream:
//! receiver 0, codec 0, the program's rate, and the program's format. The header's code 4 is read
//! as float32 for a float32 stream, in the exact size only: it is the Go client's float32, and
//! WSJT-X's header comment calls it float64, whose `length` values take exactly the bytes of twice
//! as many float32. The header's `channels` is not read: WSJT-X never writes it, so the program's
//! own setting counts. `length` must claim at least one whole frame, and the payload must be
//! exactly `length` values or exactly twice that (WSJT-X sizes its answer for twice and fills the
//! first half); anything else is refused as bad and counted. A frame that passes is charged to a
//! budget of [`TX_BURST_SECS`], refilled at [`TX_SPEED`] times real time, for the `length` values
//! it hands on, and one the budget cannot pay for is dropped whole. Only those values are read:
//! each sanitised, the channels averaged to mono, resampled to [`NEXUS_RATE`] and clamped again. So
//! a frame that claims little can never deliver much, and one that claims nothing is not free.
//!
//! What the clients send (read for behaviour in their source; nothing copied):
//! - WSJT-X 3.0.2 answers each TX_CHRONO with the chrono's receiver, rate, format and `length`,
//!   and a payload of `length` × 2 floats of which it writes the first `length`, left and right
//!   equal (`Transceiver/TCITransceiver.cpp:869-891`, `readAudioData` `:1529-1545`). Its
//!   `Data_Stream` has no `channels` field (`TCITransceiver.hpp:24-35`), so that word is whatever
//!   the buffer held.
//! - MSHV answers in the chrono's format with exactly `length` values, at 48, 24 or 12 kHz, and
//!   writes `channels` for protocol 1.9 and later (`HvRigCat/network/network.cpp:336-406`).
//! - The Go client (github.com/ftl/tci, `client/message.go:136-158`) writes format 4 and exactly
//!   the values it was asked for.
//!
//! # Receive audio to a program
//!
//! [`ToProgram::push`] resamples Nexus's mono receive audio to the program's rate and cuts it into
//! frames of exactly [`Stream::values`] values, every channel the same audio: WSJT-X and MSHV read
//! channel 0, and WSJT-X takes whole stereo frames only (`TCITransceiver.cpp:929`). Less than a
//! frame waits for the next push; nothing more does.
//!
//! # Sanitising
//!
//! Every sample this module hands on, either way, is finite and within ±1.0: a value that is not
//! finite is silence, and the rest is clamped to full scale. It is done
//! before a filter, where one NaN would spoil every output whose window holds it, and again after
//! it, since a filter overshoots full scale on clipped audio. The operator's TX level applies
//! later, as on Nexus's own overs.
//!
//! # Resampling
//!
//! [`CaptureResampler`], the filter every other audio route in the tree uses: stateful, free of
//! drift, an anti-alias cutoff at 4.5 kHz, and an identity between equal rates. Its window keeps
//! the last fewer-than-64 input samples until more arrive, as on those routes. A change of rate or
//! stream starts a new filter.
//!
//! # Bounds
//!
//! Nothing is sized from a number in a header until it is checked: a transmit frame is read only
//! up to [`MAX_PAYLOAD`] bytes, a TX_CHRONO's ask is checked with `checked_mul` against
//! [`MAX_ANSWER_VALUES`] before anything is allocated for it, a client's pull never holds more than
//! that, and a server's receive block is read only as far as a frame can hold ([`MAX_VALUES`]).

use std::collections::VecDeque;
use std::iter::repeat_n;
use std::ops::RangeInclusive;
use std::time::Instant;

use crate::capture_resample::CaptureResampler;

/// Nexus's own audio rate: the modem's, and what a program's over is fed at.
pub const NEXUS_RATE: u32 = 12_000;

/// The audio rates a TCI stream may run at (§4.3 `AUDIO_SAMPLERATE`; `frame::AudioRate::HZ`).
pub const RATES: [u32; 4] = [8_000, 12_000, 24_000, 48_000];

/// `audio_stream_samples` as the spec allows it (§4.3), in values a frame.
pub const SAMPLES: RangeInclusive<u32> = 100..=2048;

/// The rates Nexus's receive audio may arrive at: the receive feed's (`receive_audio.rs`).
pub const SOURCE_RATES: RangeInclusive<u32> = 8_000..=192_000;

/// The largest payload read, in bytes (`frame::MAX_PAYLOAD`).
pub const MAX_PAYLOAD: usize = 64 * 1024;

/// The most values one frame can carry: [`MAX_PAYLOAD`] of int16.
pub const MAX_VALUES: usize = MAX_PAYLOAD / 2;

/// The most values a TX_CHRONO's answer may hold: what fits [`MAX_PAYLOAD`] in any format, about
/// a sixth of a second of 48 kHz stereo. Every server read asks for 2048 or fewer.
pub const MAX_ANSWER_VALUES: usize = MAX_PAYLOAD / 4;

/// A program's transmit audio is taken at most this many times real time.
pub const TX_SPEED: f64 = 2.0;

/// How many seconds of a program's transmit audio may arrive at once.
pub const TX_BURST_SECS: f64 = 0.5;

/// The header's format code the Go client writes for float32 (outside the spec's 0 to 3).
const GO_FLOAT32: u32 = 4;

/// Nexus's transmit audio pulled at a time, at [`NEXUS_RATE`]: about 21 ms.
const PULL: usize = 256;

/// A sample format (§3.4 `SampleType`), by the header's code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Int16,
    /// Three bytes a value, packed little-endian, as MSHV writes it.
    Int24,
    Int32,
    Float32,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Int16, Format::Int24, Format::Int32, Format::Float32];

    /// The header's `format` (`frame::SampleFormat::code`).
    pub fn code(self) -> u32 {
        match self {
            Format::Int16 => 0,
            Format::Int24 => 1,
            Format::Int32 => 2,
            Format::Float32 => 3,
        }
    }

    pub fn from_code(code: u32) -> Option<Format> {
        Format::ALL.into_iter().find(|f| f.code() == code)
    }

    /// Bytes a value.
    pub fn bytes(self) -> usize {
        match self {
            Format::Int16 => 2,
            Format::Int24 => 3,
            Format::Int32 | Format::Float32 => 4,
        }
    }

    /// One value from exactly [`bytes`](Self::bytes) little-endian bytes, full scale ±1.0 for the
    /// integers; a float as it came.
    fn read(self, b: &[u8]) -> f32 {
        match (self, b) {
            (Format::Int16, &[x0, x1]) => f32::from(i16::from_le_bytes([x0, x1])) / 32_768.0,
            // The three bytes in the top of an i32, shifted down: the sign comes along.
            (Format::Int24, &[x0, x1, x2]) => {
                (i32::from_le_bytes([0, x0, x1, x2]) >> 8) as f32 / 8_388_608.0
            }
            (Format::Int32, &[x0, x1, x2, x3]) => {
                i32::from_le_bytes([x0, x1, x2, x3]) as f32 / 2_147_483_648.0
            }
            (Format::Float32, &[x0, x1, x2, x3]) => f32::from_le_bytes([x0, x1, x2, x3]),
            _ => 0.0,
        }
    }
}

/// A program's audio stream as it set it with `audio_samplerate`, `audio_stream_sample_type`,
/// `audio_stream_channels` and `audio_stream_samples` (§4.3): the shape of the receive frames it
/// is sent, and how its transmit frames are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stream {
    rate: u32,
    format: Format,
    channels: usize,
    frames: usize,
}

impl Stream {
    /// The spec's defaults (§4.3), which WSJT-X relies on: 48 kHz float32 stereo, 2048 values a
    /// frame.
    pub const DEFAULT: Stream = Stream {
        rate: 48_000,
        format: Format::Float32,
        channels: 2,
        frames: 1024,
    };

    /// `None` outside the spec: a rate in [`RATES`], one or two channels, `samples` in
    /// [`SAMPLES`]. A sample count that is not whole frames is cut to whole frames (2047 in stereo
    /// is 2046 values), since WSJT-X reads whole frames only.
    pub fn new(rate: u32, format: Format, channels: u32, samples: u32) -> Option<Stream> {
        if !RATES.contains(&rate) || !SAMPLES.contains(&samples) {
            return None;
        }
        let channels = match channels {
            1 => 1,
            2 => 2,
            _ => return None,
        };
        Some(Stream {
            rate,
            format,
            channels,
            frames: samples as usize / channels,
        })
    }

    pub fn rate(self) -> u32 {
        self.rate
    }

    pub fn format(self) -> Format {
        self.format
    }

    pub fn channels(self) -> usize {
        self.channels
    }

    /// Frames in each receive frame.
    pub fn frames(self) -> usize {
        self.frames
    }

    /// Values in each receive frame: its `length`, and the `length` of each TX_CHRONO.
    pub fn values(self) -> usize {
        self.frames * self.channels
    }
}

/// Fit for Nexus's audio: silence for a value that is not finite, full scale for one beyond it.
fn sanitize(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// The fields of a transmit frame's header this module reads, unchecked: `frame::RawHeader`'s, as
/// the caller read them. The header's `channels` is not among them: WSJT-X leaves it unset, so the
/// program's own setting counts. The `type` (2, transmit audio) is the caller's to have routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxHeader {
    pub receiver: u32,
    pub sample_rate: u32,
    pub format: u32,
    pub codec: u32,
    pub length: u32,
}

/// Why a program's transmit frame is refused as bad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bad {
    /// Not receiver 0, the one receiver Nexus serves.
    Receiver(u32),
    /// Compressed: the spec implements no codec.
    Codec(u32),
    /// Not the program's rate.
    Rate(u32),
    /// Not the program's format (nor code 4 for float32, in the exact size).
    Format(u32),
    /// More than [`MAX_PAYLOAD`] bytes of samples.
    Long(usize),
    /// `length` 0: a frame that claims nothing.
    Empty,
    /// `length` is not a whole number of frames.
    Ragged { length: u32 },
    /// The payload is neither `length` values nor twice that.
    Length { length: u32, payload: usize },
}

/// Why a transmit frame was not handed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxDrop {
    /// Refused, counted as bad.
    Bad(Bad),
    /// A good frame the budget could not pay for: dropped whole, never queued.
    OverBudget,
}

/// What [`FromProgram`] took and refused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TxTally {
    pub accepted: u64,
    pub bad: u64,
    pub over_budget: u64,
    /// Values handed on, every one of them paid for.
    pub values: u64,
    /// The last refusal.
    pub last_bad: Option<Bad>,
}

/// Seconds of transmit audio a program may still send: [`TX_BURST_SECS`] at first, refilled at
/// [`TX_SPEED`] times real time.
#[derive(Debug, Clone, Copy, Default)]
struct Budget {
    seconds: f64,
    last: Option<Instant>,
}

impl Budget {
    /// Pay for `cost` seconds of audio at `now`, or refuse.
    fn take(&mut self, cost: f64, now: Instant) -> bool {
        self.seconds = match self.last {
            None => TX_BURST_SECS,
            Some(last) => (self.seconds
                + now.saturating_duration_since(last).as_secs_f64() * TX_SPEED)
                .min(TX_BURST_SECS),
        };
        self.last = Some(self.last.map_or(now, |last| last.max(now)));
        if cost <= self.seconds {
            self.seconds -= cost;
            true
        } else {
            false
        }
    }
}

/// The bytes of a transmit frame that are read (the values its `length` claims) and the format
/// to read them in; or why the frame is bad.
fn check(stream: Stream, header: TxHeader, payload: &[u8]) -> Result<(Format, &[u8]), Bad> {
    if header.receiver != 0 {
        return Err(Bad::Receiver(header.receiver));
    }
    if header.codec != 0 {
        return Err(Bad::Codec(header.codec));
    }
    if header.sample_rate != stream.rate() {
        return Err(Bad::Rate(header.sample_rate));
    }
    let (format, exact) = match Format::from_code(header.format) {
        Some(f) if f == stream.format() => (f, false),
        None if header.format == GO_FLOAT32 && stream.format() == Format::Float32 => {
            (Format::Float32, true)
        }
        _ => return Err(Bad::Format(header.format)),
    };
    if payload.len() > MAX_PAYLOAD {
        return Err(Bad::Long(payload.len()));
    }
    let length = header.length;
    let mismatch = Bad::Length {
        length,
        payload: payload.len(),
    };
    let Ok(values) = usize::try_from(length) else {
        return Err(mismatch);
    };
    if values == 0 {
        return Err(Bad::Empty);
    }
    if !values.is_multiple_of(stream.channels()) {
        return Err(Bad::Ragged { length });
    }
    // Exactly the claim, or exactly twice it (WSJT-X's); never a byte read past the payload.
    let claimed = values.checked_mul(format.bytes());
    let twice = claimed.and_then(|c| c.checked_mul(2));
    let fits = claimed == Some(payload.len()) || (!exact && twice == Some(payload.len()));
    claimed
        .filter(|_| fits)
        .and_then(|c| payload.get(..c))
        .map(|data| (format, data))
        .ok_or(mismatch)
}

/// Interleaved values in `format`, one value per frame: each sanitised, the channels averaged.
fn mono(format: Format, data: &[u8], channels: usize) -> Vec<f32> {
    let size = format.bytes();
    data.chunks_exact(size * channels)
        .map(|frame| {
            frame
                .chunks_exact(size)
                .map(|b| sanitize(format.read(b)))
                .sum::<f32>()
                / channels as f32
        })
        .collect()
}

/// One program's transmit frames, as Nexus the server reads them: 12 kHz mono for its over. Keep
/// one a program, across its overs (the budget is the program's); [`reset`](Self::reset) at each
/// over's start.
#[derive(Default)]
pub struct FromProgram {
    /// The stream the filter was built for, and the filter.
    filter: Option<(Stream, CaptureResampler)>,
    budget: Budget,
    tally: TxTally,
}

impl FromProgram {
    /// One transmit frame, read with the program's `stream` as it stands (a change starts a new
    /// filter; the budget stays): its `length` values at [`NEXUS_RATE`], mono and sanitised, or why
    /// it was dropped. `payload` is the frame after its 64-byte header; `now` prices the budget.
    pub fn accept(
        &mut self,
        stream: Stream,
        header: TxHeader,
        payload: &[u8],
        now: Instant,
    ) -> Result<Vec<f32>, TxDrop> {
        let (format, data) = match check(stream, header, payload) {
            Ok(read) => read,
            Err(bad) => {
                self.tally.bad = self.tally.bad.saturating_add(1);
                self.tally.last_bad = Some(bad);
                return Err(TxDrop::Bad(bad));
            }
        };
        // Charged for what is handed on: the frames of `data`, which are the claim and only it.
        let frames = data.len() / (format.bytes() * stream.channels());
        if !self
            .budget
            .take(frames as f64 / f64::from(stream.rate()), now)
        {
            self.tally.over_budget = self.tally.over_budget.saturating_add(1);
            return Err(TxDrop::OverBudget);
        }
        let samples = mono(format, data, stream.channels());
        if !matches!(&self.filter, Some((s, _)) if *s == stream) {
            self.filter = None;
        }
        let (_, filter) = self
            .filter
            .get_or_insert_with(|| (stream, CaptureResampler::new(stream.rate(), NEXUS_RATE)));
        let out: Vec<f32> = filter.process(&samples).into_iter().map(sanitize).collect();
        self.tally.accepted = self.tally.accepted.saturating_add(1);
        self.tally.values = self
            .tally
            .values
            .saturating_add((frames * stream.channels()) as u64);
        Ok(out)
    }

    /// A new over: what the filter holds of the last one is dropped. The budget and the tally stay.
    pub fn reset(&mut self) {
        self.filter = None;
    }

    pub fn tally(&self) -> TxTally {
        self.tally
    }
}

/// Nexus's receive audio, cut into one program's frames: mono at the source's rate in; frames of
/// exactly [`Stream::values`] interleaved values out, at the program's rate, every channel the same
/// audio, sanitised. Each goes out through `frame::encode_block` (receiver 0, receive audio, the
/// stream's rate, format and channels, `length` in values).
#[derive(Default)]
pub struct ToProgram {
    /// The source rate and stream the filter and the waiting audio belong to, and the filter.
    filter: Option<((u32, Stream), CaptureResampler)>,
    /// Mono at the program's rate, less than a frame of it.
    waiting: Vec<f32>,
}

impl ToProgram {
    /// Receive audio at `source_rate` (one of [`SOURCE_RATES`]; nothing otherwise) for a program
    /// whose stream is `stream` as it stands (a change starts afresh): every whole frame it
    /// completes.
    pub fn push(&mut self, source_rate: u32, stream: Stream, mono: &[f32]) -> Vec<Vec<f32>> {
        if !SOURCE_RATES.contains(&source_rate) {
            self.filter = None;
            self.waiting.clear();
            return Vec::new();
        }
        let key = (source_rate, stream);
        if !matches!(&self.filter, Some((k, _)) if *k == key) {
            self.filter = None;
            self.waiting.clear();
        }
        let (_, filter) = self
            .filter
            .get_or_insert_with(|| (key, CaptureResampler::new(source_rate, stream.rate())));
        let clean: Vec<f32> = mono.iter().map(|&v| sanitize(v)).collect();
        self.waiting
            .extend(filter.process(&clean).into_iter().map(sanitize));
        let per = stream.frames();
        let whole = self.waiting.len() / per * per;
        let frames: Vec<Vec<f32>> = self.waiting[..whole]
            .chunks_exact(per)
            .map(|frame| {
                frame
                    .iter()
                    .flat_map(|&v| repeat_n(v, stream.channels()))
                    .collect()
            })
            .collect();
        self.waiting.drain(..whole);
        frames
    }

    /// Mono at the program's rate waiting for the rest of its frame.
    #[cfg(test)]
    fn waiting(&self) -> usize {
        self.waiting.len()
    }
}

/// A server's receive audio, as Nexus the client reads it: 12 kHz mono for the decoders and the
/// waterfall.
#[derive(Default)]
pub struct FromServer {
    filter: Option<(u32, CaptureResampler)>,
}

impl FromServer {
    /// One receive block's channel 0 (`frame::Block::channel(0)`: the real audio, in DIGU and DIGL
    /// too) at `rate`, as 12 kHz mono, sanitised. A new rate starts a new filter; a rate TCI does
    /// not have gives nothing. At most [`MAX_VALUES`] are read.
    pub fn push(&mut self, rate: u32, channel0: impl IntoIterator<Item = f32>) -> Vec<f32> {
        if !RATES.contains(&rate) {
            self.filter = None;
            return Vec::new();
        }
        if !matches!(&self.filter, Some((r, _)) if *r == rate) {
            self.filter = None;
        }
        let (_, filter) = self
            .filter
            .get_or_insert_with(|| (rate, CaptureResampler::new(rate, NEXUS_RATE)));
        let clean: Vec<f32> = channel0
            .into_iter()
            .take(MAX_VALUES)
            .map(sanitize)
            .collect();
        filter.process(&clean).into_iter().map(sanitize).collect()
    }
}

/// What a server's TX_CHRONO asks for, checked: at least one frame, one or two channels, and no
/// more values than [`MAX_ANSWER_VALUES`]. The only way to size an answer from a chrono's `length`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChronoAsk {
    frames: usize,
    channels: usize,
}

/// Why a TX_CHRONO's ask is not answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskRefused {
    /// Not one or two channels.
    Channels(u32),
    /// Less than one frame.
    Empty,
    /// More than [`MAX_ANSWER_VALUES`].
    TooLong { length: u32 },
}

impl ChronoAsk {
    /// `length` counted in values, frames times channels: every server read.
    pub fn in_values(length: u32, channels: u32) -> Result<ChronoAsk, AskRefused> {
        ChronoAsk::checked(length, channels, false)
    }

    /// `length` counted in frames, for a dialect that counts so.
    pub fn in_frames(length: u32, channels: u32) -> Result<ChronoAsk, AskRefused> {
        ChronoAsk::checked(length, channels, true)
    }

    fn checked(length: u32, channels: u32, in_frames: bool) -> Result<ChronoAsk, AskRefused> {
        let count = match channels {
            1 => 1usize,
            2 => 2,
            _ => return Err(AskRefused::Channels(channels)),
        };
        let too_long = AskRefused::TooLong { length };
        let Ok(length_n) = usize::try_from(length) else {
            return Err(too_long);
        };
        let frames = if in_frames {
            length_n
        } else {
            length_n / count
        };
        match frames.checked_mul(count) {
            Some(values) if values > MAX_ANSWER_VALUES => Err(too_long),
            Some(_) if frames == 0 => Err(AskRefused::Empty),
            Some(_) => Ok(ChronoAsk {
                frames,
                channels: count,
            }),
            None => Err(too_long),
        }
    }

    pub fn frames(self) -> usize {
        self.frames
    }

    pub fn channels(self) -> usize {
        self.channels
    }

    /// The answer's values: frames times channels.
    pub fn values(self) -> usize {
        self.frames * self.channels
    }
}

/// Nexus's transmit audio, pulled at a server's rate for each TX_CHRONO, as Nexus the client sends
/// it. Keep one a connection; [`reset`](Self::reset) at each over's start and at a stop.
#[derive(Default)]
pub struct ToServer {
    filter: Option<(u32, CaptureResampler)>,
    /// Resampled to the server's rate, not yet handed out.
    ready: VecDeque<f32>,
}

impl ToServer {
    /// Mono at `rate` into `out`, pulling Nexus's 12 kHz transmit audio from `source` as needed:
    /// how many were written (at most [`MAX_ANSWER_VALUES`]); the rest of `out` is the caller's
    /// silence. `source` fills the slice it is given and returns how many it wrote, 0 when it has
    /// nothing. A new rate starts afresh; a rate TCI does not have gives nothing.
    pub fn fill(
        &mut self,
        rate: u32,
        out: &mut [f32],
        source: &mut dyn FnMut(&mut [f32]) -> usize,
    ) -> usize {
        if !RATES.contains(&rate) {
            self.reset();
            return 0;
        }
        if !matches!(&self.filter, Some((r, _)) if *r == rate) {
            self.reset();
        }
        let (_, filter) = self
            .filter
            .get_or_insert_with(|| (rate, CaptureResampler::new(NEXUS_RATE, rate)));
        let want = out.len().min(MAX_ANSWER_VALUES);
        let mut chunk = [0.0f32; PULL];
        while self.ready.len() < want {
            let n = source(&mut chunk).min(PULL);
            if n == 0 {
                break;
            }
            for v in &mut chunk[..n] {
                *v = sanitize(*v);
            }
            self.ready
                .extend(filter.process(&chunk[..n]).into_iter().map(sanitize));
        }
        let n = self.ready.len().min(want);
        for (slot, v) in out.iter_mut().zip(self.ready.drain(..n)) {
            *slot = v;
        }
        n
    }

    /// A TX_CHRONO's answer at `rate`: exactly [`ChronoAsk::values`] values, every channel the
    /// same audio, silence past what `source` had. For `frame::Chrono::answer`.
    pub fn answer(
        &mut self,
        rate: u32,
        ask: ChronoAsk,
        source: &mut dyn FnMut(&mut [f32]) -> usize,
    ) -> Vec<f32> {
        let mut mono = vec![0.0f32; ask.frames()];
        self.fill(rate, &mut mono, source);
        mono.iter()
            .flat_map(|&v| repeat_n(v, ask.channels()))
            .collect()
    }

    /// Drop what is held: a new over, or a stop.
    pub fn reset(&mut self) {
        self.filter = None;
        self.ready.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;
    use std::time::Duration;

    fn stream(rate: u32, format: Format, channels: u32, samples: u32) -> Stream {
        Stream::new(rate, format, channels, samples).expect("a stream inside the spec")
    }

    /// The header a program writes for `length` values of its own stream.
    fn header(stream: Stream, length: usize) -> TxHeader {
        TxHeader {
            receiver: 0,
            sample_rate: stream.rate(),
            format: stream.format().code(),
            codec: 0,
            length: u32::try_from(length).expect("a test length"),
        }
    }

    /// `samples` as a program writes them in `format`, little-endian; integers at full scale ±1.0,
    /// floats as given (a program may send anything in a float).
    fn payload(format: Format, samples: &[f32]) -> Vec<u8> {
        let mut out = Vec::new();
        for &v in samples {
            let v32 = v.clamp(-1.0, 1.0);
            match format {
                Format::Int16 => {
                    let x = (v32 * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16;
                    out.extend_from_slice(&x.to_le_bytes());
                }
                Format::Int24 => {
                    let x = (v32 * 8_388_608.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
                    out.extend_from_slice(&x.to_le_bytes()[..3]);
                }
                Format::Int32 => {
                    let x = (f64::from(v32) * 2_147_483_648.0)
                        .round()
                        .clamp(f64::from(i32::MIN), f64::from(i32::MAX))
                        as i32;
                    out.extend_from_slice(&x.to_le_bytes());
                }
                Format::Float32 => out.extend_from_slice(&v.to_le_bytes()),
            }
        }
        out
    }

    /// A whole binary message as a program sends it: the spec's header (§3.4: receiver,
    /// sample_rate, format, codec, crc, length, type 2, channels, eight reserved words), then the
    /// payload.
    fn message(head: TxHeader, channels: u32, reserved: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for w in [
            head.receiver,
            head.sample_rate,
            head.format,
            head.codec,
            0,
            head.length,
            2,
            channels,
        ] {
            out.extend_from_slice(&w.to_le_bytes());
        }
        for _ in 0..8 {
            out.extend_from_slice(&reserved.to_le_bytes());
        }
        out.extend_from_slice(payload);
        out
    }

    /// What the server's caller hands on from a message: the fields `frame::RawHeader::read`
    /// returns, at the spec's offsets, and the samples after the 64 bytes.
    fn read_message(message: &[u8]) -> (TxHeader, &[u8]) {
        let word = |i: usize| {
            u32::from_le_bytes(message[i * 4..i * 4 + 4].try_into().expect("four bytes"))
        };
        (
            TxHeader {
                receiver: word(0),
                sample_rate: word(1),
                format: word(2),
                codec: word(3),
                length: word(5),
            },
            &message[64..],
        )
    }

    fn tone(rate: u32, hz: f64, amp: f64, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (amp * (2.0 * PI * hz * i as f64 / f64::from(rate)).sin()) as f32)
            .collect()
    }

    fn interleave(mono: &[f32], channels: usize) -> Vec<f32> {
        mono.iter().flat_map(|&v| repeat_n(v, channels)).collect()
    }

    /// The amplitude of `hz` in `samples` (Goertzel); exact when `hz` falls on a bin, so callers
    /// pass whole cycles.
    fn amplitude(samples: &[f32], hz: f64, rate: u32) -> f64 {
        let n = samples.len();
        assert!(n > 0, "no samples to measure");
        let k = (0.5 + n as f64 * hz / f64::from(rate)).floor();
        let coeff = 2.0 * (2.0 * PI * k / n as f64).cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in samples {
            let s = f64::from(x) + coeff * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        2.0 * (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0).sqrt() / n as f64
    }

    /// `samples` without `edge` at each end, cut to whole cycles of `cycle` samples.
    fn middle(samples: &[f32], edge: usize, cycle: usize) -> &[f32] {
        assert!(
            samples.len() > 2 * edge + cycle,
            "{} samples",
            samples.len()
        );
        let mid = &samples[edge..samples.len() - edge];
        &mid[..mid.len() / cycle * cycle]
    }

    fn db(a: f64, b: f64) -> f64 {
        20.0 * (a / b).log10()
    }

    /// A 1 kHz tone of amplitude 0.5 at `rate`, measured: within `tolerance_db` of 0.5.
    fn assert_tone(samples: &[f32], rate: u32, tolerance_db: f64, what: &str) {
        let cycle = (rate / 1000) as usize;
        let amp = amplitude(middle(samples, 4 * cycle * 8, cycle), 1000.0, rate);
        assert!(
            db(amp, 0.5).abs() < tolerance_db,
            "{what}: 1 kHz at {amp:.4}, {:.2} dB from 0.5",
            db(amp, 0.5)
        );
    }

    #[test]
    fn a_program_s_stream_is_inside_the_spec_and_whole_frames() {
        assert_eq!(
            Stream::new(48_000, Format::Float32, 2, 2048),
            Some(Stream::DEFAULT)
        );
        assert_eq!(
            (Stream::DEFAULT.values(), Stream::DEFAULT.frames()),
            (2048, 1024)
        );
        for rate in [0, 11_025, 44_100, 96_000] {
            assert_eq!(Stream::new(rate, Format::Int16, 2, 2048), None, "{rate}");
        }
        for channels in [0, 3, u32::MAX] {
            assert_eq!(Stream::new(48_000, Format::Int16, channels, 2048), None);
        }
        for samples in [0, 99, 2049, u32::MAX] {
            assert_eq!(Stream::new(48_000, Format::Int16, 2, samples), None);
        }
        // 2047 values in stereo is not whole frames: 1023 frames, 2046 values.
        let odd = stream(24_000, Format::Int24, 2, 2047);
        assert_eq!((odd.frames(), odd.values()), (1023, 2046));
        let least = stream(8_000, Format::Int32, 1, 100);
        assert_eq!((least.frames(), least.values()), (100, 100));
        for f in Format::ALL {
            assert_eq!(Format::from_code(f.code()), Some(f));
        }
        assert_eq!(Format::from_code(4), None, "4 is no format of the spec's");
    }

    #[test]
    fn each_format_reads_at_full_scale_as_the_spec_and_mshv_pack_it() {
        let read = |format: Format, bytes: &[u8]| {
            // 12 kHz mono: no filter between the bytes and what is handed on.
            let s = stream(12_000, format, 1, 100);
            FromProgram::default()
                .accept(s, header(s, 1), bytes, Instant::now())
                .expect("one value")
        };
        assert_eq!(read(Format::Int16, &[0x00, 0x80]), [-1.0]);
        assert_eq!(read(Format::Int16, &[0x00, 0x40]), [0.5]);
        assert_eq!(read(Format::Int16, &[0xff, 0x7f]), [32_767.0 / 32_768.0]);
        // int24: three bytes, low first, the sign in the top one (MSHV's packing,
        // `network.cpp:309-311` and `:388-390`).
        assert_eq!(read(Format::Int24, &[0x00, 0x00, 0x80]), [-1.0]);
        assert_eq!(read(Format::Int24, &[0x00, 0x00, 0xc0]), [-0.5]);
        assert_eq!(read(Format::Int24, &[0x00, 0x00, 0x40]), [0.5]);
        assert_eq!(
            read(Format::Int24, &[0xff, 0xff, 0xff]),
            [-1.0 / 8_388_608.0]
        );
        assert_eq!(read(Format::Int32, &[0, 0, 0, 0x80]), [-1.0]);
        assert_eq!(read(Format::Int32, &[0, 0, 0, 0x40]), [0.5]);
        assert_eq!(read(Format::Float32, &0.25f32.to_le_bytes()), [0.25]);
    }

    /// A sample that is not finite is silence, and one past full scale is clamped, before
    /// it reaches a transmitter.
    #[test]
    fn samples_that_are_not_finite_are_silence_and_out_of_range_ones_are_clamped() {
        let s = stream(12_000, Format::Float32, 1, 100);
        let sent = [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            3.0e38,
            -7.5,
            1.5,
            -1.0,
            0.25,
        ];
        let got = FromProgram::default()
            .accept(
                s,
                header(s, sent.len()),
                &payload(Format::Float32, &sent),
                Instant::now(),
            )
            .expect("eight values");
        assert_eq!(got, [0.0, 0.0, 0.0, 1.0, -1.0, 1.0, -1.0, 0.25]);
        // Each side is made fit before the two are averaged, so a NaN on one side does not
        // silence the other.
        let s = stream(12_000, Format::Float32, 2, 100);
        let sent = [f32::NAN, 0.5, 0.25, 0.75, 2.0, -2.0, 1.0, 0.0];
        let got = FromProgram::default()
            .accept(
                s,
                header(s, sent.len()),
                &payload(Format::Float32, &sent),
                Instant::now(),
            )
            .expect("four frames");
        assert_eq!(got, [0.25, 0.5, 0.0, 0.5]);
    }

    /// A filter overshoots full scale on clipped audio; what reaches the over never does.
    #[test]
    fn what_the_filter_makes_of_clipped_audio_is_clamped_again() {
        // Full-scale square wave, 750 Hz at 48 kHz, stereo.
        let square: Vec<f32> = (0..9600)
            .map(|i| if (i / 32) % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        // The positive control: the filter alone goes past full scale on it.
        let raw = CaptureResampler::new(48_000, NEXUS_RATE).process(&square);
        let peak = raw.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 1.0, "the filter alone peaks at {peak}");
        let s = Stream::DEFAULT;
        let mut from = FromProgram::default();
        let mut got = Vec::new();
        for chunk in interleave(&square, 2).chunks(s.values()) {
            got.extend(
                from.accept(
                    s,
                    header(s, chunk.len()),
                    &payload(Format::Float32, chunk),
                    Instant::now(),
                )
                .expect("accepted"),
            );
        }
        let peak = got.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert_eq!(peak, 1.0, "clamped after the filter");
    }

    /// A frame that carries less than it claims is refused, counted, and never read past
    /// its end; so is one that carries more, except WSJT-X's twice.
    #[test]
    fn a_short_frame_is_refused_counted_and_never_panics() {
        let now = Instant::now();
        for format in Format::ALL {
            for channels in [1u32, 2] {
                let s = stream(48_000, format, channels, 2048);
                let length = 6 * channels as usize;
                let one = length * format.bytes();
                let mut from = FromProgram::default();
                let sizes = 0..=2 * one + format.bytes();
                for size in sizes.clone() {
                    let got = from.accept(s, header(s, length), &vec![0u8; size], now);
                    if size == one || size == 2 * one {
                        assert!(got.is_ok(), "{format:?} x{channels}: {size} bytes fits");
                    } else {
                        assert_eq!(
                            got,
                            Err(TxDrop::Bad(Bad::Length {
                                length: length as u32,
                                payload: size
                            })),
                            "{format:?} x{channels}: {size} bytes for {length} values"
                        );
                    }
                }
                let tally = from.tally();
                assert_eq!(tally.accepted, 2, "{format:?} x{channels}");
                assert_eq!(
                    tally.bad,
                    sizes.count() as u64 - 2,
                    "{format:?} x{channels}"
                );
                assert_eq!(tally.values, 2 * length as u64, "{format:?} x{channels}");
            }
        }
    }

    /// Security review (1): a frame whose `length` is 0 is refused as bad, whatever it carries. It
    /// would cost nothing, so a stream of them would buy calls for free.
    #[test]
    fn a_frame_that_claims_nothing_is_refused_as_bad() {
        let s = Stream::DEFAULT;
        let mut from = FromProgram::default();
        let now = Instant::now();
        for size in [0usize, 8, 8192] {
            assert_eq!(
                from.accept(s, header(s, 0), &vec![0u8; size], now),
                Err(TxDrop::Bad(Bad::Empty)),
                "length 0 with {size} bytes"
            );
        }
        let tally = from.tally();
        assert_eq!(
            (tally.accepted, tally.bad, tally.values, tally.last_bad),
            (0, 3, 0, Some(Bad::Empty))
        );
    }

    /// Security review (1): the budget was charged by min(header length, payload values), so a
    /// frame claiming a tiny `length` with a big payload paid little and delivered much. Such a
    /// frame is refused as bad: it buys nothing and costs nothing.
    #[test]
    fn a_tiny_claim_with_a_big_payload_is_refused_and_buys_nothing() {
        let s = Stream::DEFAULT;
        let now = Instant::now();
        let big = payload(Format::Float32, &vec![0.5; 4096]);
        let mut from = FromProgram::default();
        assert_eq!(
            from.accept(s, header(s, 2), &big, now),
            Err(TxDrop::Bad(Bad::Length {
                length: 2,
                payload: 16_384
            }))
        );
        for _ in 0..1000 {
            assert!(from.accept(s, header(s, 2), &big, now).is_err());
        }
        let tally = from.tally();
        assert_eq!(
            (tally.accepted, tally.values, tally.bad),
            (0, 0, 1001),
            "a thousand tiny claims bought no audio"
        );
        // And spent none of the budget: half a second of real frames still passes.
        let frame = payload(Format::Float32, &vec![0.1; 2048]);
        let admitted = (0..30)
            .filter(|_| from.accept(s, header(s, 2048), &frame, now).is_ok())
            .count();
        assert_eq!(admitted, 23);
    }

    /// Security review (1): only the `length` values paid for are handed on, never the rest of
    /// WSJT-X's twice-sized payload.
    #[test]
    fn only_the_values_paid_for_are_handed_on() {
        let s = stream(12_000, Format::Float32, 1, 100);
        let sent = [0.1, 0.2, 0.3, 0.4, 0.9, -0.9, 0.9, -0.9];
        let mut from = FromProgram::default();
        assert_eq!(
            from.accept(
                s,
                header(s, 4),
                &payload(Format::Float32, &sent),
                Instant::now()
            ),
            Ok(vec![0.1, 0.2, 0.3, 0.4])
        );
        assert_eq!(from.tally().values, 4);
    }

    /// Security review (1): charged by what is delivered. Half a second at once, then twice real
    /// time; WSJT-X's twice-sized frames cost what they hand on, no more and no less; a frame over
    /// budget hands on nothing; a refused frame costs nothing.
    #[test]
    fn transmit_audio_is_charged_for_what_is_delivered() {
        let s = Stream::DEFAULT;
        let t0 = Instant::now();
        let frame = interleave(&tone(48_000, 1000.0, 0.5, 1024), 2);
        let single = payload(Format::Float32, &frame);
        let mut double = single.clone();
        double.extend(payload(Format::Float32, &vec![0.9; 2048]));
        for (what, bytes) in [("single", &single), ("WSJT-X's twice-sized", &double)] {
            let mut from = FromProgram::default();
            // 2048 values of 48 kHz stereo is 21.33 ms; 23 of them are 0.4907 s.
            let admitted = (0..30)
                .filter(|_| from.accept(s, header(s, 2048), bytes, t0).is_ok())
                .count();
            assert_eq!(admitted, 23, "{what}");
            let tally = from.tally();
            assert_eq!(
                tally.values,
                23 * 2048,
                "{what}: handed on exactly what was paid for"
            );
            assert_eq!(tally.over_budget, 7, "{what}");
            // Half a frame's time refills a whole frame, at twice real time.
            let t1 = t0 + Duration::from_micros(10_667);
            assert!(from.accept(s, header(s, 2048), bytes, t1).is_ok(), "{what}");
            assert_eq!(
                from.accept(s, header(s, 2048), bytes, t1),
                Err(TxDrop::OverBudget),
                "{what}"
            );
            assert_eq!(from.tally().values, 24 * 2048, "{what}");
        }
        // Refused frames are not charged.
        let mut from = FromProgram::default();
        for _ in 0..100 {
            let _ = from.accept(s, header(s, 0), &[], t0);
            let _ = from.accept(s, header(s, 3), &single, t0);
        }
        let admitted = (0..30)
            .filter(|_| from.accept(s, header(s, 2048), &single, t0).is_ok())
            .count();
        assert_eq!(admitted, 23);
        // A program quiet for a while has not saved up: half a second at once is the ceiling.
        let later = t0 + Duration::from_secs(10);
        let admitted = (0..30)
            .filter(|_| from.accept(s, header(s, 2048), &single, later).is_ok())
            .count();
        assert_eq!(admitted, 23, "after ten quiet seconds");
    }

    #[test]
    fn a_frame_off_the_program_s_stream_is_refused() {
        let now = Instant::now();
        let s = Stream::DEFAULT;
        let four = payload(Format::Float32, &[0.0; 4]);
        let h = header(s, 4);
        let mut from = FromProgram::default();
        let mut refused = |head: TxHeader, bytes: &[u8]| match from.accept(s, head, bytes, now) {
            Err(TxDrop::Bad(bad)) => bad,
            other => panic!("{head:?}: {other:?}"),
        };
        assert_eq!(
            refused(TxHeader { receiver: 1, ..h }, &four),
            Bad::Receiver(1)
        );
        assert_eq!(refused(TxHeader { codec: 1, ..h }, &four), Bad::Codec(1));
        assert_eq!(
            refused(
                TxHeader {
                    sample_rate: 24_000,
                    ..h
                },
                &four
            ),
            Bad::Rate(24_000)
        );
        for code in [0, 1, 2, 5, u32::MAX] {
            assert_eq!(
                refused(TxHeader { format: code, ..h }, &four),
                Bad::Format(code)
            );
        }
        assert_eq!(
            refused(header(s, 3), &four[..12]),
            Bad::Ragged { length: 3 }
        );
        // Code 4 is the Go client's float32: read in the exact size only.
        let go = TxHeader { format: 4, ..h };
        assert!(FromProgram::default().accept(s, go, &four, now).is_ok());
        let eight = payload(Format::Float32, &[0.0; 8]);
        assert_eq!(
            refused(go, &eight),
            Bad::Length {
                length: 4,
                payload: 32
            }
        );
        let int16 = stream(48_000, Format::Int16, 2, 2048);
        assert_eq!(
            FromProgram::default().accept(int16, go, &four, now),
            Err(TxDrop::Bad(Bad::Format(4)))
        );
        // No more than MAX_PAYLOAD bytes are read, even when `length` describes them.
        let mono = stream(48_000, Format::Float32, 1, 2048);
        let most = vec![0u8; MAX_PAYLOAD];
        assert!(FromProgram::default()
            .accept(mono, header(mono, MAX_PAYLOAD / 4), &most, now)
            .is_ok());
        let over = vec![0u8; MAX_PAYLOAD + 4];
        assert_eq!(
            FromProgram::default().accept(mono, header(mono, MAX_PAYLOAD / 4 + 1), &over, now),
            Err(TxDrop::Bad(Bad::Long(MAX_PAYLOAD + 4)))
        );
    }

    /// Every format, every rate, both channel counts: a 1 kHz tone reaches Nexus's rate at its
    /// level, and the time it took on the program's side.
    #[test]
    fn every_format_rate_and_channel_count_reaches_nexus_s_rate() {
        let now = Instant::now();
        for format in Format::ALL {
            for rate in RATES {
                for channels in [1u32, 2] {
                    let s = stream(rate, format, channels, 2048);
                    let n = rate as usize * 2 / 5;
                    let sent = interleave(&tone(rate, 1000.0, 0.5, n), channels as usize);
                    let mut from = FromProgram::default();
                    let mut got = Vec::new();
                    for chunk in sent.chunks(s.values()) {
                        got.extend(
                            from.accept(s, header(s, chunk.len()), &payload(format, chunk), now)
                                .expect("accepted"),
                        );
                    }
                    let what = format!("{format:?} at {rate} Hz x{channels}");
                    assert_tone(&got, NEXUS_RATE, 0.5, &what);
                    // 0.4 s at 12 kHz, less what the filter still holds (63 samples in).
                    let held = 64 * NEXUS_RATE as usize / rate as usize + 1;
                    assert!(
                        got.len() <= 4800 && got.len() + held >= 4800,
                        "{what}: {} samples",
                        got.len()
                    );
                }
            }
        }
    }

    /// WSJT-X's answer to a TX_CHRONO, as it builds it: the chrono's receiver, rate, format and
    /// `length`, a `channels` word it never writes, and twice the payload with the first half
    /// written. What reaches the over is exactly what the first halves alone give.
    #[test]
    fn wsjtx_s_answer_is_read_from_its_first_half_at_the_program_s_settings() {
        let s = Stream::DEFAULT;
        let now = Instant::now();
        let sent = interleave(&tone(48_000, 1000.0, 0.5, 48_000 * 2 / 5), 2);
        let stale: Vec<f32> = (0..2048)
            .map(|i| match i % 4 {
                0 => f32::NAN,
                1 => 1.0e30,
                2 => -1.0,
                _ => 0.9,
            })
            .collect();
        let mut wsjtx = FromProgram::default();
        let mut reference = FromProgram::default();
        let (mut got, mut want) = (Vec::new(), Vec::new());
        for chunk in sent.chunks(2048) {
            let mut bytes = payload(Format::Float32, chunk);
            bytes.extend(payload(Format::Float32, &stale[..chunk.len()]));
            let head = TxHeader {
                receiver: 0,
                sample_rate: 48_000,
                format: 3,
                codec: 0,
                length: chunk.len() as u32,
            };
            let msg = message(head, 0xa5a5_a5a5, 0x5a5a_5a5a, &bytes);
            let (head, samples) = read_message(&msg);
            got.extend(wsjtx.accept(s, head, samples, now).expect("WSJT-X's frame"));
            want.extend(
                reference
                    .accept(s, head, &payload(Format::Float32, chunk), now)
                    .expect("the first half alone"),
            );
        }
        assert_eq!(got, want, "nothing of the second half reaches the over");
        assert_tone(&got, NEXUS_RATE, 0.5, "WSJT-X");
        assert_eq!(wsjtx.tally().values, sent.len() as u64);
    }

    /// MSHV's answer: the chrono's format, exactly `length` values, at 48, 24 or 12 kHz, with
    /// `channels` written (protocol 1.9 and later).
    #[test]
    fn mshv_s_answer_is_read_in_each_of_its_formats_and_rates() {
        let now = Instant::now();
        for format in Format::ALL {
            for rate in [48_000, 24_000, 12_000] {
                let s = stream(rate, format, 2, 2048);
                let sent = interleave(&tone(rate, 1000.0, 0.5, rate as usize * 2 / 5), 2);
                let mut from = FromProgram::default();
                let mut got = Vec::new();
                for chunk in sent.chunks(2048) {
                    let msg = message(header(s, chunk.len()), 2, 0, &payload(format, chunk));
                    let (head, samples) = read_message(&msg);
                    got.extend(from.accept(s, head, samples, now).expect("MSHV's frame"));
                }
                assert_tone(
                    &got,
                    NEXUS_RATE,
                    0.5,
                    &format!("MSHV {format:?} at {rate} Hz"),
                );
            }
        }
    }

    /// What WSJT-X reads (the defaults it never changes) and what MSHV sets for itself: whole
    /// frames of the size asked, both channels the same audio, at the program's rate.
    #[test]
    fn wsjtx_and_mshv_get_whole_frames_of_the_size_they_set() {
        let src = tone(12_000, 1000.0, 0.5, 12_000);
        for (who, s) in [
            ("WSJT-X", Stream::DEFAULT),
            ("MSHV", stream(24_000, Format::Int16, 2, 1024)),
            ("a mono program", stream(8_000, Format::Int32, 1, 160)),
        ] {
            let frames = ToProgram::default().push(NEXUS_RATE, s, &src);
            assert!(!frames.is_empty(), "{who}");
            for f in &frames {
                assert_eq!(f.len(), s.values(), "{who}");
                for pair in f.chunks_exact(s.channels()) {
                    assert!(pair.iter().all(|&v| v == pair[0]), "{who}: {pair:?}");
                }
            }
            // A second's audio, less what the filter holds and the part frame still waiting.
            let rate = s.rate() as usize;
            let made = frames.len() * s.frames();
            assert!(
                made <= rate && made + s.frames() + 64 * rate / 12_000 >= rate,
                "{who}: {made} frames' worth"
            );
            let left: Vec<f32> = frames
                .iter()
                .flat_map(|f| f.iter().step_by(s.channels()).copied())
                .collect();
            assert_tone(&left, s.rate(), 0.5, who);
        }
    }

    #[test]
    fn receive_frames_do_not_depend_on_how_the_audio_arrives() {
        let s = stream(24_000, Format::Float32, 2, 1000);
        let src = tone(44_100, 1000.0, 0.5, 44_100);
        let whole = ToProgram::default().push(44_100, s, &src);
        let mut to = ToProgram::default();
        let mut pieces = Vec::new();
        let mut at = 0;
        for (i, size) in [1usize, 7, 441, 4410, 13, 999, 2205]
            .iter()
            .cycle()
            .enumerate()
        {
            if at >= src.len() {
                break;
            }
            let end = (at + size + i % 3).min(src.len());
            pieces.extend(to.push(44_100, s, &src[at..end]));
            assert!(to.waiting() < s.frames(), "less than a frame waits");
            at = end;
        }
        assert_eq!(pieces, whole);
        // Nothing in, nothing out.
        assert!(to.push(44_100, s, &[]).is_empty());
    }

    #[test]
    fn a_receive_sample_that_is_not_finite_is_silence_and_spoils_nothing_else() {
        let s = stream(24_000, Format::Float32, 1, 480);
        let mut src = tone(48_000, 1000.0, 0.5, 9600);
        let mut clean = src.clone();
        for i in [1000, 2000, 2001, 5000] {
            src[i] = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::NAN][i % 4];
            clean[i] = 0.0;
        }
        src[7000] = 40.0;
        clean[7000] = 1.0;
        let got = ToProgram::default().push(48_000, s, &src);
        let want = ToProgram::default().push(48_000, s, &clean);
        assert_eq!(got, want);
        assert!(got
            .iter()
            .flatten()
            .all(|v| v.is_finite() && v.abs() <= 1.0));
    }

    #[test]
    fn receive_audio_from_outside_the_feed_s_rates_or_after_a_change_starts_afresh() {
        let s = Stream::DEFAULT;
        let mut to = ToProgram::default();
        for rate in [0, 7_999, 192_001, u32::MAX] {
            assert!(to.push(rate, s, &[0.5; 4800]).is_empty(), "{rate}");
        }
        // Some audio waits for its frame; a change of stream drops it, and the new stream's frames
        // are the new stream's size only.
        assert_eq!(to.push(48_000, s, &[0.25; 1500]).len(), 1);
        assert!(to.waiting() > 0);
        let mshv = stream(48_000, Format::Int16, 1, 500);
        let frames = to.push(48_000, mshv, &[0.25; 600]);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].len(), 500);
        assert_eq!(to.waiting(), 100);
        assert!(to.push(0, mshv, &[0.25; 600]).is_empty());
        assert_eq!(to.waiting(), 0, "a bad rate drops what waited");
    }

    /// Nexus's receive audio out to a program and its own audio back in, in every format, rate and
    /// channel count: the tone survives at its level.
    #[test]
    fn receive_audio_round_trips_through_a_program_in_every_format_and_rate() {
        let now = Instant::now();
        let src = tone(NEXUS_RATE, 1000.0, 0.5, 5400);
        for format in Format::ALL {
            for rate in RATES {
                for channels in [1u32, 2] {
                    let s = stream(rate, format, channels, 512);
                    let frames = ToProgram::default().push(NEXUS_RATE, s, &src);
                    let mut from = FromProgram::default();
                    let mut back = Vec::new();
                    for f in &frames {
                        back.extend(
                            from.accept(s, header(s, f.len()), &payload(format, f), now)
                                .expect("its own frame"),
                        );
                    }
                    assert_tone(
                        &back,
                        NEXUS_RATE,
                        1.0,
                        &format!("{format:?} at {rate} Hz x{channels}"),
                    );
                }
            }
        }
    }

    #[test]
    fn a_server_s_receive_audio_reaches_nexus_s_rate_from_any_tci_rate() {
        for rate in RATES {
            let mut from = FromServer::default();
            let src = tone(rate, 1000.0, 0.5, rate as usize / 2);
            let got: Vec<f32> = src
                .chunks(1024)
                .flat_map(|c| from.push(rate, c.iter().copied()))
                .collect();
            assert_tone(&got, NEXUS_RATE, 0.5, &format!("a server at {rate} Hz"));
        }
        let mut from = FromServer::default();
        for rate in [0, 11_025, 96_000, u32::MAX] {
            assert!(from.push(rate, [0.5; 100]).is_empty(), "{rate}");
        }
        // What a server sends is made fit, before and after the filter.
        let wild: Vec<f32> = (0..4800)
            .map(|i| [f32::NAN, 1.0e30, -1.0e30, f32::INFINITY, 0.5][i % 5])
            .collect();
        assert!(from
            .push(48_000, wild)
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1.0));
        // A sample that is not finite is silence before the filter, so it spoils nothing near it.
        let mut src = tone(48_000, 1000.0, 0.5, 9600);
        let mut clean = src.clone();
        for (i, v) in [(1000, f32::NAN), (2001, f32::INFINITY), (5000, f32::NAN)] {
            src[i] = v;
            clean[i] = 0.0;
        }
        src[7000] = 40.0;
        clean[7000] = 1.0;
        assert_eq!(
            FromServer::default().push(48_000, src),
            FromServer::default().push(48_000, clean)
        );
        // A block is read only as far as a frame can hold.
        let got = FromServer::default().push(48_000, std::iter::repeat_n(0.25, 1_000_000));
        assert!(got.len() <= MAX_VALUES / 4, "{} samples", got.len());
    }

    /// Security review (2): a TX_CHRONO's `length` sized an allocation on the client side. Its
    /// ask is checked, with `checked_mul`, against what a frame can carry before anything is sized
    /// from it.
    #[test]
    fn a_chrono_s_ask_is_checked_before_anything_is_sized_from_it() {
        let ask = ChronoAsk::in_values(2048, 2).expect("AetherSDR's ask");
        assert_eq!((ask.frames(), ask.values()), (1024, 2048));
        assert_eq!(
            ChronoAsk::in_values(u32::MAX, 2),
            Err(AskRefused::TooLong { length: u32::MAX })
        );
        assert_eq!(
            ChronoAsk::in_frames(u32::MAX, 2),
            Err(AskRefused::TooLong { length: u32::MAX })
        );
        let most = MAX_ANSWER_VALUES as u32;
        assert_eq!(
            ChronoAsk::in_values(most, 2).map(ChronoAsk::values),
            Ok(MAX_ANSWER_VALUES)
        );
        assert_eq!(
            ChronoAsk::in_values(most + 2, 2),
            Err(AskRefused::TooLong { length: most + 2 })
        );
        assert_eq!(
            ChronoAsk::in_frames(most / 2, 2).map(ChronoAsk::values),
            Ok(MAX_ANSWER_VALUES)
        );
        assert_eq!(
            ChronoAsk::in_frames(most / 2 + 1, 2),
            Err(AskRefused::TooLong {
                length: most / 2 + 1
            })
        );
        assert_eq!(ChronoAsk::in_values(0, 2), Err(AskRefused::Empty));
        assert_eq!(
            ChronoAsk::in_values(1, 2),
            Err(AskRefused::Empty),
            "half a frame"
        );
        for channels in [0, 3, u32::MAX] {
            assert_eq!(
                ChronoAsk::in_values(2048, channels),
                Err(AskRefused::Channels(channels))
            );
        }
    }

    /// A pull that never ends, into a slice of any size: no more than a frame can carry is held
    /// or handed out.
    #[test]
    fn a_client_pull_never_holds_more_than_a_frame_can_carry() {
        let mut asked = 0usize;
        let mut source = |out: &mut [f32]| {
            asked += out.len();
            out.fill(0.25);
            out.len()
        };
        let mut out = vec![0.0f32; 1_000_000];
        let n = ToServer::default().fill(48_000, &mut out, &mut source);
        assert_eq!(n, MAX_ANSWER_VALUES);
        assert!(
            asked <= MAX_ANSWER_VALUES / 4 + 64 + 2 * PULL,
            "pulled {asked} samples"
        );
    }

    /// Nexus's 12 kHz transmit audio, pulled for each TX_CHRONO, arrives at the server's rate as
    /// the tone it was; an answer is exactly the ask, both channels the same, then silence.
    #[test]
    fn the_client_answers_each_chrono_with_nexus_s_audio_at_the_server_s_rate() {
        for rate in RATES {
            let src = tone(NEXUS_RATE, 1000.0, 0.5, 6000);
            let mut at = 0;
            let mut source = |out: &mut [f32]| {
                let n = out.len().min(src.len() - at);
                out[..n].copy_from_slice(&src[at..at + n]);
                at += n;
                n
            };
            let mut to = ToServer::default();
            let mut sent = Vec::new();
            loop {
                let mut out = vec![0.0f32; 1024];
                let n = to.fill(rate, &mut out, &mut source);
                if n == 0 {
                    break;
                }
                sent.extend_from_slice(&out[..n]);
            }
            assert_tone(&sent, rate, 0.5, &format!("to a server at {rate} Hz"));
            let held = 64 * rate as usize / NEXUS_RATE as usize + 1;
            assert!(
                sent.len() <= rate as usize / 2 && sent.len() + held >= rate as usize / 2,
                "{rate}: {} samples",
                sent.len()
            );
        }
        // 100 samples at 12 kHz is about 150 at 48 kHz once the filter has filled: the rest of
        // the answer is silence.
        let mut left = 100usize;
        let mut source = |out: &mut [f32]| {
            let n = out.len().min(left);
            out[..n].fill(0.5);
            left -= n;
            n
        };
        let ask = ChronoAsk::in_values(2048, 2).expect("a stereo ask");
        let answer = ToServer::default().answer(48_000, ask, &mut source);
        assert_eq!(answer.len(), 2048);
        assert!(answer.chunks_exact(2).all(|p| p[0] == p[1]));
        let sounding = answer.iter().rposition(|&v| v != 0.0).map_or(0, |i| i + 1);
        assert!((100..=400).contains(&sounding), "{sounding}");
        // Nexus's audio is made fit before the filter as well, so a NaN spoils nothing near it.
        let pull_all = |samples: Vec<f32>| {
            let mut at = 0;
            let mut source = |out: &mut [f32]| {
                let n = out.len().min(samples.len() - at);
                out[..n].copy_from_slice(&samples[at..at + n]);
                at += n;
                n
            };
            let mut out = vec![0.0f32; 4 * samples.len()];
            let n = ToServer::default().fill(48_000, &mut out, &mut source);
            out.truncate(n);
            out
        };
        let mut src = tone(NEXUS_RATE, 1000.0, 0.5, 2400);
        let mut clean = src.clone();
        src[500] = f32::NAN;
        clean[500] = 0.0;
        src[1500] = 9.0;
        clean[1500] = 1.0;
        assert_eq!(pull_all(src), pull_all(clean));
        // A new rate drops what was made for the old one.
        let mut to = ToServer::default();
        let mut out = [0.0f32; 100];
        let mut half = |out: &mut [f32]| {
            out.fill(0.5);
            out.len()
        };
        assert_eq!(to.fill(48_000, &mut out, &mut half), 100);
        let mut quarter = |out: &mut [f32]| {
            out.fill(0.25);
            out.len()
        };
        assert_eq!(to.fill(24_000, &mut out, &mut quarter), 100);
        assert!(
            out.iter().all(|&v| (v - 0.25).abs() < 0.01),
            "audio made for 48 kHz handed out at 24 kHz: {out:?}"
        );
        // Nothing to send, or a rate TCI does not have: nothing written.
        let mut none = |_: &mut [f32]| 0usize;
        assert_eq!(
            ToServer::default().fill(48_000, &mut [0.0; 64], &mut none),
            0
        );
        let mut endless = |out: &mut [f32]| {
            out.fill(0.5);
            out.len()
        };
        assert_eq!(
            ToServer::default().fill(44_100, &mut [0.0; 64], &mut endless),
            0
        );
    }

    /// splitmix64, from a fixed seed: a failure is a case number to replay.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }

        fn pick<T: Copy>(&mut self, from: &[T]) -> T {
            from[self.below(from.len())]
        }

        /// A sample's bytes: anything, often a float that is not finite or past full scale.
        fn bytes(&mut self, n: usize) -> Vec<u8> {
            let mut out = Vec::with_capacity(n);
            while out.len() < n {
                let word = match self.below(8) {
                    0 => f32::NAN.to_bits(),
                    1 => f32::INFINITY.to_bits(),
                    2 => f32::NEG_INFINITY.to_bits(),
                    3 => f32::MAX.to_bits(),
                    _ => self.next() as u32,
                };
                out.extend_from_slice(&word.to_le_bytes());
            }
            out.truncate(n);
            out
        }
    }

    /// The fuzz: transmit frames with headers near the program's stream and payloads of any
    /// size and content. Never a panic; a frame handed on had a header of the program's stream
    /// and a payload of its claim or twice it, and hands on finite samples within full scale;
    /// every frame is counted once.
    #[test]
    fn any_transmit_frame_never_panics_and_hands_on_only_what_it_paid_for() {
        let mut rng = Rng(0x7c1_5e2d);
        let streams: Vec<Stream> = Format::ALL
            .into_iter()
            .flat_map(|f| RATES.into_iter().map(move |r| (f, r)))
            .flat_map(|(f, r)| [1u32, 2].map(|c| stream(r, f, c, [100, 2047, 2048][c as usize])))
            .collect();
        let mut readers: Vec<FromProgram> =
            streams.iter().map(|_| FromProgram::default()).collect();
        let mut calls = vec![0u64; streams.len()];
        let mut now = Instant::now();
        for case in 0..4096 {
            let i = rng.below(streams.len());
            let s = streams[i];
            let head = TxHeader {
                receiver: rng.pick(&[0, 0, 0, 0, 0, 1, u32::MAX]),
                sample_rate: if rng.below(4) > 0 {
                    s.rate()
                } else {
                    rng.pick(&[0, 8_000, 12_000, 24_000, 44_100, 48_000, u32::MAX])
                },
                format: if rng.below(4) > 0 {
                    s.format().code()
                } else {
                    rng.pick(&[0, 1, 2, 3, 4, 5, u32::MAX])
                },
                codec: if rng.below(8) > 0 {
                    0
                } else {
                    rng.pick(&[1, 2, u32::MAX])
                },
                length: match rng.below(8) {
                    0 => rng.pick(&[0, 1, 2, 3, 4, 7]),
                    1 => s.values() as u32,
                    2 => rng.next() as u32,
                    3 => u32::MAX,
                    _ => rng.below(700) as u32,
                },
            };
            let size_of = |values: u64| values.saturating_mul(s.format().bytes() as u64);
            let claimed = size_of(u64::from(head.length));
            let size = match rng.below(8) {
                0 | 1 if claimed <= MAX_PAYLOAD as u64 => claimed as usize,
                2 if claimed * 2 <= MAX_PAYLOAD as u64 => claimed as usize * 2,
                3 if claimed <= MAX_PAYLOAD as u64 => {
                    (claimed as usize + rng.pick(&[1usize, 2, 3])).saturating_sub(rng.below(5))
                }
                4 => rng.below(MAX_PAYLOAD + 9),
                5 => 0,
                _ => rng.below(2100),
            };
            let bytes = rng.bytes(size);
            let before = readers[i].tally();
            let got = readers[i].accept(s, head, &bytes, now);
            calls[i] += 1;
            let after = readers[i].tally();
            match got {
                Ok(out) => {
                    assert_eq!(after.accepted, before.accepted + 1, "case {case}");
                    assert_eq!(
                        after.values,
                        before.values + u64::from(head.length),
                        "case {case}: handed on its claim"
                    );
                    assert_eq!(
                        (head.receiver, head.codec, head.sample_rate),
                        (0, 0, s.rate()),
                        "case {case}"
                    );
                    let one = claimed as usize;
                    assert!(
                        size == one || (size == 2 * one && head.format == s.format().code()),
                        "case {case}: {size} bytes for a claim of {one}"
                    );
                    assert!(
                        out.iter().all(|v| v.is_finite() && v.abs() <= 1.0),
                        "case {case}"
                    );
                }
                Err(TxDrop::Bad(bad)) => {
                    assert_eq!(after.bad, before.bad + 1, "case {case}");
                    assert_eq!(after.last_bad, Some(bad), "case {case}");
                    assert_eq!(after.values, before.values, "case {case}");
                }
                Err(TxDrop::OverBudget) => {
                    assert_eq!(after.over_budget, before.over_budget + 1, "case {case}");
                    assert_eq!(after.values, before.values, "case {case}");
                }
            }
            if rng.below(4) == 0 {
                now += Duration::from_micros(rng.below(60_000) as u64);
            }
        }
        for (reader, n) in readers.iter().zip(calls) {
            let t = reader.tally();
            assert_eq!(t.accepted + t.bad + t.over_budget, n);
        }
    }

    /// The fuzz, the other three ways: receive audio of any rate, size and content cut for a
    /// program, read from a server, and transmit audio pulled for any ask. Never a panic, never a
    /// sample out of bounds, never a frame of the wrong size.
    #[test]
    fn any_audio_either_way_never_panics_and_stays_in_bounds() {
        let mut rng = Rng(0x5e7_a0d1);
        let wild = |rng: &mut Rng, n: usize| -> Vec<f32> {
            (0..n)
                .map(|_| match rng.below(10) {
                    0 => f32::NAN,
                    1 => f32::INFINITY,
                    2 => -1.0e30,
                    3 => 7.5,
                    _ => (rng.below(2001) as f32 - 1000.0) / 1000.0,
                })
                .collect()
        };
        let in_bounds = |v: &f32| v.is_finite() && v.abs() <= 1.0;
        let mut to = ToProgram::default();
        let mut from = FromServer::default();
        let mut pull = ToServer::default();
        let mut s = Stream::DEFAULT;
        let mut source_rate = 48_000;
        let mut rate = 48_000;
        for case in 0..1500 {
            if rng.below(32) == 0 {
                s = stream(
                    rng.pick(&RATES),
                    rng.pick(&Format::ALL),
                    rng.pick(&[1, 2]),
                    rng.pick(&[100, 101, 512, 2047, 2048]),
                );
                source_rate =
                    rng.pick(&[0, 8_000, 11_025, 12_000, 44_100, 48_000, 96_000, 192_001]);
                rate = rng.pick(&[0, 8_000, 12_000, 24_000, 44_100, 48_000]);
            }
            let n = rng.below(3000);
            let audio = wild(&mut rng, n);
            for frame in to.push(source_rate, s, &audio) {
                assert_eq!(frame.len(), s.values(), "case {case}");
                assert!(frame.iter().all(in_bounds), "case {case}");
            }
            assert!(to.waiting() < s.frames(), "case {case}");
            assert!(
                from.push(rate, audio.iter().copied()).iter().all(in_bounds),
                "case {case}"
            );
            let mut out = vec![0.0f32; rng.below(5000)];
            let mut left = rng.below(1500);
            let mut source = |out: &mut [f32]| {
                let n = out.len().min(left).min(rng.below(PULL + 64));
                for (slot, v) in out[..n].iter_mut().zip(audio.iter().cycle()) {
                    *slot = *v;
                }
                left -= n;
                n
            };
            let n = pull.fill(rate, &mut out, &mut source);
            assert!(n <= out.len().min(MAX_ANSWER_VALUES), "case {case}");
            assert!(out[..n].iter().all(in_bounds), "case {case}");
            let length = match rng.below(4) {
                0 => rng.next() as u32,
                _ => rng.below(20_000) as u32,
            };
            let channels = rng.pick(&[0, 1, 2, 2, 3]);
            if let Ok(ask) = ChronoAsk::in_values(length, channels) {
                assert!(ask.frames() >= 1, "case {case}");
                assert!(ask.values() <= MAX_ANSWER_VALUES, "case {case}");
                assert_eq!(ask.values(), ask.frames() * ask.channels(), "case {case}");
            }
            if let Ok(ask) = ChronoAsk::in_frames(length, channels) {
                assert!(ask.values() <= MAX_ANSWER_VALUES, "case {case}");
                let mut none = |_: &mut [f32]| 0usize;
                assert_eq!(pull.answer(rate, ask, &mut none).len(), ask.values());
            }
        }
    }
}
