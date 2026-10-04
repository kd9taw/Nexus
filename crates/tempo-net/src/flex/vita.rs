//! The VITA-49 display streams: panadapter FFT frames (class `0x8003`) and waterfall tiles
//! (`0x8004`), decoded from a packet's payload and assembled into whole frames.
//!
//! The 28-byte VITA-49 header is read by [`crate::flexvita::parse_vita`], which follows the
//! standard's header flags; on the radio's packets the payload starts at byte 28, where upstream
//! reads it. This module starts at the payload.
//!
//! # FFT frames
//!
//! A 12-byte sub-header (`start_bin`, `num_bins`, `bin_size`, `total_bins`, then `frame_index`)
//! precedes `num_bins` big-endian `u16` bins, and a frame spans several packets. **A bin is a
//! pixel row counted from the top of the display, not a magnitude:** row 0 is the pan's
//! `max_dbm`, row `y_pixels − 1` its `min_dbm`, so
//! `dBm = max_dbm − row / (y_pixels − 1) × (max_dbm − min_dbm)` ([`row_to_dbm`]). The fraction
//! of the pan's window a row stands for, 1 at the top and 0 at the bottom, is
//! `1 − row / (y_pixels − 1)` ([`row_level`]). `y_pixels`, `min_dbm` and `max_dbm` are the pan's,
//! from its status.
//!
//! [`FftAssembler`] completes a frame only when every bin has arrived ([`BinCoverage`]), never
//! from a packet count, and never reads past a short payload ([`bounded_payload_bins`]). After an
//! `x_pixels` increase, 4.2.18 firmware can send one frame whose new bins are all zero, which
//! would read as a wall at `max_dbm`; [`has_zero_filled_growth_suffix`] and
//! [`GrowthSuffixGuard`] hold that frame back, and fill the new bins with the floor only if the
//! radio keeps sending it.
//!
//! # Waterfall tiles
//!
//! A 36-byte sub-header (the frame's low frequency and bin bandwidth as `i64` Hz × 2²⁰, line
//! duration, width, height, timecode, auto-black level, total bins, first bin) precedes the
//! tile. Only the first row is read (the height is normally 1). A value is `i16 / 128`, an
//! intensity on the radio's own scale, **not dBm**: the noise floor sits near 96–106 and HF peaks
//! near 110–115, so a waterfall's colours calibrate apart from the FFT's dBm axis.
//! [`TileAssembler`] assembles rows the way [`FftAssembler`] assembles frames.
//!
//! PORTED from AetherSDR (https://github.com/aethersdr/AetherSDR, GPL-3.0; the upstream file
//! carries no per-file header, the licence is the repository's), the FFT and waterfall-tile
//! decoders and frame assembly of `src/core/backends/flex/PanadapterStream.h` and
//! `src/core/backends/flex/PanadapterStream.cpp`, with `src/core/VitaBinCoverage.h` and
//! `src/core/VitaTileFrequency.h`, at commit `32fa50e4896a846a6970fa3f443bd49d667c139d`
//! (2026-10-03), translated from C++/Qt to Rust; the radio's default display size is a fact from
//! `src/models/RadioModel.cpp`, and the encodings are as `docs/architecture/vita49-format.md`
//! records them. Deliberate differences: the decoders are pure functions of a payload, split from
//! the socket, the stream registry and the Qt signals; a frame keeps its rows, and the conversion
//! to a level or to dBm is a separate step; up to three frames of a stream assemble at once, so a
//! fragment of one frame arriving after the next frame has begun costs nothing (upstream restarts
//! its single frame), and a frame that has completed takes no further fragment, so a duplicated
//! packet cannot publish it twice; a fragment that does not fit its frame is dropped before it
//! can displace a frame in progress; the dBm range's pending-echo handshake is not ported (frames
//! are published on the window's own scale until a calibrated axis is built). Recorded in the
//! repo-root NOTICE (AetherSDR entry).

/// The panadapter FFT packet class.
pub const FFT_CLASS: u16 = 0x8003;
/// The waterfall tile packet class.
pub const WATERFALL_CLASS: u16 = 0x8004;

/// The FFT sub-header's length.
pub const FFT_SUBHEADER_BYTES: usize = 12;
/// The waterfall tile sub-header's length.
pub const TILE_SUBHEADER_BYTES: usize = 36;

/// A display dimension at or below this is the radio's default or reset size, not one a client
/// set: a new pan is `xpixels=50 ypixels=20` until a client sets its own, and a profile load or a
/// reconnect can put it back. Bins encoded on that scale must not be read on another, so a
/// client re-sets its size when it sees one (upstream `kDefaultPanDimensionThreshold`).
pub const DEFAULT_DISPLAY_MAX: u32 = 100;

/// The number of whole bins a payload actually holds: never more than the sub-header declares,
/// and never more than the bytes that arrived. A header can declare a full fragment while the
/// datagram ends early; the missing tail must never be read or counted.
pub fn bounded_payload_bins(declared: usize, bytes_per_bin: usize, available: usize) -> usize {
    if declared == 0 || bytes_per_bin == 0 || available < bytes_per_bin {
        return 0;
    }
    declared.min(available / bytes_per_bin)
}

/// Which bins of a frame have arrived, counted once each. A packet count cannot prove a frame
/// complete: a duplicated or overlapping fragment repeats positions while leaving a gap elsewhere.
#[derive(Debug, Clone, Default)]
pub struct BinCoverage {
    received: Vec<bool>,
    unique: usize,
}

impl BinCoverage {
    /// Start over for a frame of `total` bins.
    pub fn reset(&mut self, total: usize) {
        self.received.clear();
        self.received.resize(total, false);
        self.unique = 0;
    }

    /// Mark `count` bins from `first`. `false` when the range does not fit the frame or brings no
    /// bin that had not already arrived.
    pub fn mark_range(&mut self, first: usize, count: usize) -> bool {
        let Some(end) = first.checked_add(count) else {
            return false;
        };
        if count == 0 || end > self.received.len() {
            return false;
        }
        let mut new_bin = false;
        for seen in &mut self.received[first..end] {
            if !*seen {
                *seen = true;
                self.unique += 1;
                new_bin = true;
            }
        }
        new_bin
    }

    /// Every bin has arrived (and the frame has at least one).
    pub fn is_complete(&self) -> bool {
        !self.received.is_empty() && self.unique == self.received.len()
    }

    pub fn unique_bins(&self) -> usize {
        self.unique
    }

    pub fn total_bins(&self) -> usize {
        self.received.len()
    }
}

/// Whether a completed frame is the radio's placeholder after an `x_pixels` increase: the bins
/// it had before are real and nearly every new one is zero. Zero is the top row, `max_dbm`, so
/// accepting the frame would draw a wall across the new part of the pan. Only a real growth of
/// at least 16 bins counts, and only when the old part is not itself mostly zero, so a strong
/// signal that saturates the whole pan is never taken for the placeholder.
pub fn has_zero_filled_growth_suffix(rows: &[u16], previous_accepted: usize) -> bool {
    if previous_accepted == 0 || previous_accepted >= rows.len() {
        return false;
    }
    let suffix = rows.len() - previous_accepted;
    if suffix < 16 {
        return false;
    }
    let prefix_zeros = rows[..previous_accepted]
        .iter()
        .filter(|&&r| r == 0)
        .count();
    let suffix_zeros = rows[previous_accepted..]
        .iter()
        .filter(|&&r| r == 0)
        .count();
    suffix_zeros * 10 >= suffix * 9 && prefix_zeros * 10 < previous_accepted
}

/// What to do with a completed frame, from [`GrowthSuffixGuard::observe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrowthAction {
    /// An ordinary frame.
    Accept,
    /// The growth placeholder: hold it back.
    Reject,
    /// The placeholder again, past the bounded wait: emit the old part and draw the new bins at
    /// the floor, so a radio that keeps sending it cannot freeze the pan at its old width.
    EmitWithFloorSuffix,
}

/// Holds back the growth placeholder ([`has_zero_filled_growth_suffix`]). A FLEX-8600 on 4.2.18
/// normally sends at most one, so rejecting it is right for the common case; after
/// [`Self::REJECTED_FRAMES_BEFORE_FLOOR_FALLBACK`] in a row at one width the frame is emitted
/// with its new bins at the floor, and the first properly filled frame resets the guard.
#[derive(Debug, Clone, Default)]
pub struct GrowthSuffixGuard {
    rejected_total: usize,
    consecutive_rejected: u32,
}

impl GrowthSuffixGuard {
    pub const REJECTED_FRAMES_BEFORE_FLOOR_FALLBACK: u32 = 3;

    /// Fold in one completed frame of `total` bins.
    pub fn observe(&mut self, zero_filled_growth: bool, total: usize) -> GrowthAction {
        if !zero_filled_growth || total == 0 {
            self.reset();
            return GrowthAction::Accept;
        }
        if total != self.rejected_total {
            self.rejected_total = total;
            self.consecutive_rejected = 1;
        } else {
            self.consecutive_rejected += 1;
        }
        if self.consecutive_rejected <= Self::REJECTED_FRAMES_BEFORE_FLOOR_FALLBACK {
            GrowthAction::Reject
        } else {
            GrowthAction::EmitWithFloorSuffix
        }
    }

    pub fn reset(&mut self) {
        self.rejected_total = 0;
        self.consecutive_rejected = 0;
    }

    pub fn consecutive_rejected(&self) -> u32 {
        self.consecutive_rejected
    }
}

fn be_u16(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at + 2)?;
    Some(u16::from_be_bytes([s[0], s[1]]))
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn be_i64(b: &[u8], at: usize) -> Option<i64> {
    let s = b.get(at..at + 8)?;
    Some(i64::from_be_bytes([
        s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
    ]))
}

/// The payload without its 4-byte VITA trailer, when the header says one follows.
fn without_trailer(payload: &[u8], has_trailer: bool) -> Option<&[u8]> {
    if has_trailer {
        payload.get(..payload.len().checked_sub(4)?)
    } else {
        Some(payload)
    }
}

/// One FFT packet: a run of rows of one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FftFragment {
    pub start_bin: u16,
    pub total_bins: u16,
    pub frame_index: u32,
    /// The rows that actually arrived ([`bounded_payload_bins`]): at most `num_bins` of them.
    pub rows: Vec<u16>,
}

/// Decode an FFT packet's payload (the bytes after the 28-byte header, trailer included when the
/// header says one follows). `None` for a sub-header that declares no bins, no width or a bin
/// size other than two bytes, and for a payload that carries no whole bin.
pub fn decode_fft(payload: &[u8], has_trailer: bool) -> Option<FftFragment> {
    let body = without_trailer(payload, has_trailer)?;
    let start_bin = be_u16(body, 0)?;
    let num_bins = be_u16(body, 2)?;
    let bin_size = be_u16(body, 4)?;
    let total_bins = be_u16(body, 6)?;
    let frame_index = be_u32(body, 8)?;
    // Flex FFT bins are u16; any other size is not a frame this decoder can read.
    if num_bins == 0 || total_bins == 0 || bin_size != 2 {
        return None;
    }
    let data = &body[FFT_SUBHEADER_BYTES..];
    let count = bounded_payload_bins(usize::from(num_bins), 2, data.len());
    if count == 0 {
        return None;
    }
    let rows = data[..count * 2]
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    Some(FftFragment {
        start_bin,
        total_bins,
        frame_index,
        rows,
    })
}

/// The pan's display window in dBm: the top row is `max_dbm`, the bottom row `min_dbm`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DbmRange {
    pub min_dbm: f32,
    pub max_dbm: f32,
}

impl DbmRange {
    /// The lowest `min_dbm` taken.
    pub const FLOOR_DBM: f32 = -180.0;
    /// The narrowest window taken, in dB.
    pub const MIN_SPAN_DB: f32 = 10.0;

    /// A window, held to [`Self::FLOOR_DBM`] at the bottom and at least [`Self::MIN_SPAN_DB`]
    /// tall, as upstream holds the range it decodes with.
    pub fn new(min_dbm: f32, max_dbm: f32) -> Self {
        let min_dbm = min_dbm.max(Self::FLOOR_DBM);
        DbmRange {
            min_dbm,
            max_dbm: max_dbm.max(min_dbm + Self::MIN_SPAN_DB),
        }
    }
}

/// The row clamped onto the display, as a float: a row at or past the bottom is a clipped
/// sample at the floor, not evidence of a taller display.
fn clamped_row(row: u16, y_pixels: u32) -> (f32, f32) {
    let y = y_pixels.max(2) as f32;
    (f32::from(row).clamp(0.0, y - 1.0), y)
}

/// Where a row sits in the pan's window: 1 at the top (`max_dbm`), 0 at the bottom (`min_dbm`).
pub fn row_level(row: u16, y_pixels: u32) -> f32 {
    let (row, y) = clamped_row(row, y_pixels);
    1.0 - row / (y - 1.0)
}

/// The dBm a row stands for in a window of `y_pixels` rows.
pub fn row_to_dbm(row: u16, y_pixels: u32, range: DbmRange) -> f32 {
    let (row, y) = clamped_row(row, y_pixels);
    let dbm = range.max_dbm - row / (y - 1.0) * (range.max_dbm - range.min_dbm);
    dbm.clamp(range.min_dbm, range.max_dbm)
}

/// A whole FFT frame, as the radio encoded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FftFrame {
    pub frame_index: u32,
    /// One row per bin, counted from the top of the display.
    pub rows: Vec<u16>,
    /// From this bin on, the frame is the growth placeholder emitted after the bounded wait
    /// ([`GrowthAction::EmitWithFloorSuffix`]): those bins are drawn at the floor, never read.
    pub floor_from: Option<usize>,
}

impl FftFrame {
    fn row(&self, bin: usize, y_pixels: u32) -> u16 {
        match self.floor_from {
            Some(from) if bin >= from => u16::try_from(y_pixels.max(2) - 1).unwrap_or(u16::MAX),
            _ => self.rows[bin],
        }
    }

    /// Each bin's place in the pan's window, 0 (bottom) to 1 (top) ([`row_level`]).
    pub fn levels(&self, y_pixels: u32) -> Vec<f32> {
        (0..self.rows.len())
            .map(|bin| row_level(self.row(bin, y_pixels), y_pixels))
            .collect()
    }

    /// Each bin in dBm ([`row_to_dbm`]).
    pub fn dbm(&self, y_pixels: u32, range: DbmRange) -> Vec<f32> {
        (0..self.rows.len())
            .map(|bin| row_to_dbm(self.row(bin, y_pixels), y_pixels, range))
            .collect()
    }
}

/// How many frames of one stream may be part-assembled at once. Reordering on a LAN is a packet
/// or two deep, not a frame's worth; three frames of 2048 bins is a few kilobytes, and the oldest
/// is given up rather than the set grown.
const IN_FLIGHT: usize = 3;

/// One frame being assembled.
#[derive(Debug)]
struct Partial<T, M> {
    index: u32,
    values: Vec<T>,
    coverage: BinCoverage,
    /// What the fragment that began the frame said about the whole of it.
    meta: M,
}

/// The frames of one stream being assembled, keyed by frame index and width (a fragment that
/// reuses an index with another width is another frame, never a write past the end of this one).
#[derive(Debug)]
struct Frames<T, M> {
    inflight: Vec<Partial<T, M>>,
    /// The frames that most recently completed, oldest first: a late duplicate of one of them is
    /// not the start of a new frame.
    completed: Vec<(u32, usize)>,
}

impl<T, M> Default for Frames<T, M> {
    fn default() -> Self {
        Frames {
            inflight: Vec::new(),
            completed: Vec::new(),
        }
    }
}

impl<T: Copy + Default, M> Frames<T, M> {
    /// Fold one fragment in: `values` for bins `first..` of frame `index`, `total` bins wide.
    /// The values and what the first fragment said, once every bin of the frame has arrived.
    fn push(
        &mut self,
        index: u32,
        total: usize,
        first: usize,
        values: &[T],
        meta: impl FnOnce() -> M,
    ) -> Option<(Vec<T>, M)> {
        let end = first.checked_add(values.len())?;
        if values.is_empty() || end > total || self.completed.contains(&(index, total)) {
            return None;
        }
        let at = match self
            .inflight
            .iter()
            .position(|p| p.index == index && p.values.len() == total)
        {
            Some(at) => at,
            None => {
                if self.inflight.len() >= IN_FLIGHT {
                    self.inflight.remove(0); // the oldest unfinished frame gives up its place
                }
                let mut coverage = BinCoverage::default();
                coverage.reset(total);
                self.inflight.push(Partial {
                    index,
                    values: vec![T::default(); total],
                    coverage,
                    meta: meta(),
                });
                self.inflight.len() - 1
            }
        };
        let partial = &mut self.inflight[at];
        if !partial.coverage.mark_range(first, values.len()) {
            return None;
        }
        partial.values[first..end].copy_from_slice(values);
        if !partial.coverage.is_complete() {
            return None;
        }
        let done = self.inflight.remove(at);
        if self.completed.len() >= IN_FLIGHT {
            self.completed.remove(0);
        }
        self.completed.push((index, total));
        Some((done.values, done.meta))
    }
}

/// Assembles one pan's FFT fragments into frames: per-bin coverage, then the growth guard.
#[derive(Debug, Default)]
pub struct FftAssembler {
    frames: Frames<u16, ()>,
    /// The width of the last frame accepted as an ordinary frame.
    last_accepted_total: usize,
    growth: GrowthSuffixGuard,
}

impl FftAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold in a fragment. The frame it completes, unless that frame is the growth placeholder
    /// being held back.
    pub fn push(&mut self, fragment: &FftFragment) -> Option<FftFrame> {
        let (rows, ()) = self.frames.push(
            fragment.frame_index,
            usize::from(fragment.total_bins),
            usize::from(fragment.start_bin),
            &fragment.rows,
            || (),
        )?;
        let placeholder = has_zero_filled_growth_suffix(&rows, self.last_accepted_total);
        let floor_from = match self.growth.observe(placeholder, rows.len()) {
            GrowthAction::Reject => return None,
            GrowthAction::EmitWithFloorSuffix => Some(self.last_accepted_total),
            GrowthAction::Accept => {
                self.last_accepted_total = rows.len();
                None
            }
        };
        Some(FftFrame {
            frame_index: fragment.frame_index,
            rows,
            floor_from,
        })
    }
}

/// A waterfall tile's frequencies, in MHz. The wire carries FlexLib's `VitaFrequency`, Hz × 2²⁰
/// as an `i64`, and it is always decoded as that: guessing the encoding from the magnitude broke
/// transverter tiles above 1 GHz and tiles that overhang below 0 Hz near DC.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileFrequency {
    pub low_mhz: f64,
    pub bin_bw_mhz: f64,
}

/// Hz × 2²⁰ per MHz.
const VITA_FREQUENCY_PER_MHZ: f64 = 1_048_576.0 * 1e6;

/// Decode a tile's frame low frequency and bin bandwidth.
pub fn tile_frequency(low_raw: i64, bin_bw_raw: i64) -> TileFrequency {
    TileFrequency {
        low_mhz: low_raw as f64 / VITA_FREQUENCY_PER_MHZ,
        bin_bw_mhz: bin_bw_raw as f64 / VITA_FREQUENCY_PER_MHZ,
    }
}

/// One waterfall tile packet: part of one row of the radio's waterfall.
#[derive(Debug, Clone, PartialEq)]
pub struct TileFragment {
    /// The low edge of the whole row and the width of one bin.
    pub frequency: TileFrequency,
    /// The radio's `line_duration`, as the tile reports it.
    pub line_duration: u32,
    pub width: u16,
    pub height: u16,
    /// The row's number: the fragments of one row share it.
    pub timecode: u32,
    pub auto_black: u32,
    pub total_bins: u16,
    pub first_bin: u16,
    /// The first row's intensities (`i16 / 128`), from `first_bin` on.
    pub values: Vec<f32>,
}

/// Decode a waterfall tile's payload (the bytes after the 28-byte header, trailer included when
/// the header says one follows). `None` for a tile with no width or height, one whose payload
/// does not hold a whole row, or one that starts past the end of its row.
pub fn decode_tile(payload: &[u8], has_trailer: bool) -> Option<TileFragment> {
    let body = without_trailer(payload, has_trailer)?;
    let frequency = tile_frequency(be_i64(body, 0)?, be_i64(body, 8)?);
    let line_duration = be_u32(body, 16)?;
    let width = be_u16(body, 20)?;
    let height = be_u16(body, 22)?;
    let timecode = be_u32(body, 24)?;
    let auto_black = be_u32(body, 28)?;
    let total_bins = be_u16(body, 32)?;
    let first_bin = be_u16(body, 34)?;
    if width == 0 || height == 0 {
        return None;
    }
    let data = &body[TILE_SUBHEADER_BYTES..];
    if data.len() < usize::from(width) * 2 {
        return None; // not even one whole row
    }
    let count =
        usize::from(width).min(usize::from(total_bins).checked_sub(usize::from(first_bin))?);
    if count == 0 {
        return None;
    }
    let values = data[..count * 2]
        .chunks_exact(2)
        .map(|c| f32::from(i16::from_be_bytes([c[0], c[1]])) / 128.0)
        .collect();
    Some(TileFragment {
        frequency,
        line_duration,
        width,
        height,
        timecode,
        auto_black,
        total_bins,
        first_bin,
        values,
    })
}

/// One whole row of the radio's waterfall.
#[derive(Debug, Clone, PartialEq)]
pub struct TileRow {
    pub timecode: u32,
    pub low_mhz: f64,
    /// `low_mhz` plus one bin bandwidth per bin.
    pub high_mhz: f64,
    pub auto_black: u32,
    /// Intensities on the radio's own scale (`i16 / 128`), not dBm.
    pub values: Vec<f32>,
}

/// Assembles one waterfall's tiles into rows, with the same per-bin coverage as
/// [`FftAssembler`].
#[derive(Debug, Default)]
pub struct TileAssembler {
    frames: Frames<f32, (TileFrequency, u32)>,
}

impl TileAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold in a tile. The row it completes, with the frequencies and auto-black level of the
    /// row's first tile.
    pub fn push(&mut self, tile: &TileFragment) -> Option<TileRow> {
        let (values, (frequency, auto_black)) = self.frames.push(
            tile.timecode,
            usize::from(tile.total_bins),
            usize::from(tile.first_bin),
            &tile.values,
            || (tile.frequency, tile.auto_black),
        )?;
        Some(TileRow {
            timecode: tile.timecode,
            low_mhz: frequency.low_mhz,
            high_mhz: frequency.low_mhz + frequency.bin_bw_mhz * values.len() as f64,
            auto_black,
            values,
        })
    }
}

#[cfg(test)]
mod tests {
    //! Translated from upstream's `panadapter_dbm_range_test.cpp` (the bin-coverage, payload-bound,
    //! growth-suffix and guard blocks; the dBm-range handshake blocks test `setDbmRange` and
    //! `gui/DbmRangeTransition.h`, which are not ported) and `vita_tile_frequency_test.cpp` (all of
    //! it). The rest are Nexus's: the decoders against packets built byte by byte, the known-level
    //! frame, and the reassembly cases the shipped decoder was fixed for (Flex audit 2026-08-17).
    use super::*;
    use crate::flexvita::parse_vita;

    // ── Packets, built byte by byte as the radio lays them out ──────────────────────────────

    /// A 28-byte header: extension data with a stream id, the class id (Flex OUI, `SL`, class),
    /// TSI 3 and TSF 1 with zero timestamps; optionally a trailer flag.
    fn packet(stream_id: u32, class: u16, trailer: bool, payload: &[u8]) -> Vec<u8> {
        let words = (28 + payload.len()).div_ceil(4) + usize::from(trailer);
        let mut w0: u32 = (3 << 28) | (1 << 27) | (3 << 22) | (1 << 20) | words as u32;
        if trailer {
            w0 |= 1 << 26;
        }
        let mut p = Vec::new();
        p.extend_from_slice(&w0.to_be_bytes());
        p.extend_from_slice(&stream_id.to_be_bytes());
        p.extend_from_slice(&0x0000_1C2Du32.to_be_bytes());
        p.extend_from_slice(&((0x534Cu32 << 16) | u32::from(class)).to_be_bytes());
        p.extend_from_slice(&[0u8; 12]);
        p.extend_from_slice(payload);
        p.resize(words * 4 - if trailer { 4 } else { 0 }, 0);
        if trailer {
            p.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        }
        p
    }

    fn fft_payload(start: u16, total: u16, frame: u32, rows: &[u16]) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&start.to_be_bytes());
        p.extend_from_slice(&(rows.len() as u16).to_be_bytes());
        p.extend_from_slice(&2u16.to_be_bytes());
        p.extend_from_slice(&total.to_be_bytes());
        p.extend_from_slice(&frame.to_be_bytes());
        for r in rows {
            p.extend_from_slice(&r.to_be_bytes());
        }
        p
    }

    fn fragment(frame: u32, start: u16, total: u16, rows: &[u16]) -> FftFragment {
        decode_fft(&fft_payload(start, total, frame, rows), false).expect("a fragment")
    }

    #[allow(clippy::too_many_arguments)]
    fn tile_payload(
        low_raw: i64,
        bw_raw: i64,
        width: u16,
        timecode: u32,
        auto_black: u32,
        total: u16,
        first: u16,
        values: &[i16],
    ) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&low_raw.to_be_bytes());
        p.extend_from_slice(&bw_raw.to_be_bytes());
        p.extend_from_slice(&80u32.to_be_bytes()); // line duration
        p.extend_from_slice(&width.to_be_bytes());
        p.extend_from_slice(&1u16.to_be_bytes()); // height
        p.extend_from_slice(&timecode.to_be_bytes());
        p.extend_from_slice(&auto_black.to_be_bytes());
        p.extend_from_slice(&total.to_be_bytes());
        p.extend_from_slice(&first.to_be_bytes());
        for v in values {
            p.extend_from_slice(&v.to_be_bytes());
        }
        p
    }

    /// VitaFrequency raw (Hz × 2²⁰) for a frequency in MHz.
    fn vita_raw(mhz: f64) -> i64 {
        (mhz * 1e6 * 1_048_576.0).round() as i64
    }

    // ── panadapter_dbm_range_test.cpp: coverage, bounds, growth ─────────────────────────────

    #[test]
    fn coverage_counts_each_bin_once() {
        let mut c = BinCoverage::default();
        c.reset(8);
        assert!(c.mark_range(0, 4));
        assert_eq!(c.unique_bins(), 4);
        assert!(!c.is_complete());
        assert!(!c.mark_range(0, 4), "the same bins again bring nothing new");
        assert_eq!(c.unique_bins(), 4);
        assert!(!c.is_complete());
        assert!(c.mark_range(2, 4), "an overlap that brings two new bins");
        assert_eq!(c.unique_bins(), 6);
        assert!(!c.is_complete());
        assert!(c.mark_range(4, 4));
        assert_eq!(c.unique_bins(), 8);
        assert!(c.is_complete());
        c.reset(10);
        assert_eq!(c.unique_bins(), 0);
        assert_eq!(c.total_bins(), 10);
        assert!(!c.is_complete());
        assert!(!c.mark_range(8, 4), "a range past the end is refused whole");
        assert!(!c.mark_range(0, 0));
        assert!(!c.mark_range(usize::MAX, 2), "no overflow");
    }

    #[test]
    fn a_payload_never_yields_more_bins_than_arrived() {
        assert_eq!(bounded_payload_bins(256, 2, 512), 256);
        assert_eq!(bounded_payload_bins(256, 2, 510), 255);
        assert_eq!(bounded_payload_bins(256, 2, 1), 0);
        assert_eq!(bounded_payload_bins(256, 0, 512), 0);
        assert_eq!(bounded_payload_bins(0, 2, 512), 0);
    }

    #[test]
    fn the_growth_placeholder_is_told_from_a_saturated_frame() {
        let mut clean = vec![300u16; 16];
        clean.resize(32, 350);
        assert!(!has_zero_filled_growth_suffix(&clean, 16));

        let mut placeholder = vec![300u16; 32];
        placeholder[16..].fill(0);
        assert!(has_zero_filled_growth_suffix(&placeholder, 16));

        assert!(
            !has_zero_filled_growth_suffix(&[0u16; 32], 16),
            "a frame that is all top rows is a strong signal, not the placeholder"
        );
        assert!(!has_zero_filled_growth_suffix(&placeholder, 0));
        assert!(!has_zero_filled_growth_suffix(&placeholder, 32));
    }

    #[test]
    fn the_growth_guard_rejects_three_then_fills_with_the_floor() {
        let mut g = GrowthSuffixGuard::default();
        for _ in 0..GrowthSuffixGuard::REJECTED_FRAMES_BEFORE_FLOOR_FALLBACK {
            assert_eq!(g.observe(true, 32), GrowthAction::Reject);
        }
        assert_eq!(g.observe(true, 32), GrowthAction::EmitWithFloorSuffix);
        assert_eq!(g.consecutive_rejected(), 4);
        assert_eq!(g.observe(false, 32), GrowthAction::Accept);
        assert_eq!(g.consecutive_rejected(), 0);
        assert_eq!(g.observe(true, 48), GrowthAction::Reject);
        assert_eq!(g.consecutive_rejected(), 1);
    }

    // ── vita_tile_frequency_test.cpp ────────────────────────────────────────────────────────

    fn near(what: &str, got: f64, want: f64, tol: f64) {
        assert!(
            (got - want).abs() <= tol,
            "{what}: got {got:.9} MHz, want {want:.9} MHz"
        );
    }

    #[test]
    fn tile_frequencies_are_always_vita_fixed_point() {
        let f = tile_frequency(vita_raw(14.0), vita_raw(0.001));
        near("14 MHz low", f.low_mhz, 14.0, 1e-6);
        near("14 MHz bin", f.bin_bw_mhz, 0.001, 1e-9);
        near(
            "28 MHz IF",
            tile_frequency(vita_raw(28.0), vita_raw(0.002)).low_mhz,
            28.0,
            1e-6,
        );
        // Either side of 1 GHz, where a magnitude guess read the value as plain Hz.
        near(
            "1000.015 MHz",
            tile_frequency(vita_raw(1000.015), vita_raw(0.003)).low_mhz,
            1000.015,
            1e-3,
        );
        near(
            "1000.016 MHz",
            tile_frequency(vita_raw(1000.016), vita_raw(0.003)).low_mhz,
            1000.016,
            1e-3,
        );
        for mhz in [1296.0, 2304.1, 2400.0] {
            near(
                "microwave",
                tile_frequency(vita_raw(mhz), 0).low_mhz,
                mhz,
                1e-2,
            );
        }
        near(
            "2200 m",
            tile_frequency(vita_raw(0.1357), vita_raw(0.00001)).low_mhz,
            0.1357,
            1e-4,
        );
        // A tile at a 0 Hz display edge starts below zero: -30,112 Hz with 375 Hz bins.
        let f = tile_frequency(-31_574_720_512, 393_216_000);
        near("negative DC overhang", f.low_mhz, -0.030112, 1e-9);
        near("near-DC bin", f.bin_bw_mhz, 0.000375, 1e-12);
        // -3.2 kHz with 6 kHz bins.
        let f = tile_frequency(-3_355_443_200, 6_291_456_000);
        near("-3.2 kHz overhang", f.low_mhz, -0.0032, 1e-12);
        near("6 kHz bin", f.bin_bw_mhz, 0.006, 1e-12);
    }

    // ── The FFT decoder ──────────────────────────────────────────────────────────────────────

    #[test]
    fn an_fft_packet_decodes_after_the_28_byte_header() {
        let dg = packet(
            0x4000_0000,
            FFT_CLASS,
            false,
            &fft_payload(4, 8, 7, &[1, 2, 3]),
        );
        let v = parse_vita(&dg).expect("a VITA packet");
        assert_eq!(v.packet_class, Some(FFT_CLASS));
        assert_eq!(
            v.stream_id,
            Some(0x4000_0000),
            "the FFT stream id is the pan id"
        );
        assert_eq!(
            dg.len() - v.payload.len(),
            28,
            "the payload starts at byte 28"
        );
        let f = decode_fft(v.payload, v.has_trailer).expect("an FFT fragment");
        assert_eq!(
            f,
            FftFragment {
                start_bin: 4,
                total_bins: 8,
                frame_index: 7,
                rows: vec![1, 2, 3],
            }
        );
    }

    #[test]
    fn the_trailer_and_padding_are_never_read_as_bins() {
        // Three rows pad to a whole word; the trailer follows. Neither is a bin.
        let dg = packet(
            0x4000_0000,
            FFT_CLASS,
            true,
            &fft_payload(0, 3, 1, &[5, 6, 7]),
        );
        let v = parse_vita(&dg).unwrap();
        assert!(v.has_trailer);
        assert_eq!(decode_fft(v.payload, true).unwrap().rows, vec![5, 6, 7]);
    }

    #[test]
    fn a_short_fft_payload_yields_only_the_bins_that_arrived() {
        let mut p = fft_payload(0, 8, 1, &[9, 9, 9, 9]);
        p.truncate(p.len() - 3); // the datagram ends inside the third bin
        let f = decode_fft(&p, false).expect("two whole bins arrived");
        assert_eq!(f.rows, vec![9, 9]);
        // A declared fragment with no whole bin is nothing.
        let p = fft_payload(0, 8, 1, &[9]);
        assert_eq!(decode_fft(&p[..13], false), None);
    }

    #[test]
    fn an_fft_sub_header_the_decoder_cannot_read_is_refused() {
        let ok = fft_payload(0, 4, 1, &[1, 2]);
        assert!(decode_fft(&ok, false).is_some());
        let mut wide = ok.clone();
        wide[4..6].copy_from_slice(&4u16.to_be_bytes()); // 4-byte bins
        assert_eq!(decode_fft(&wide, false), None);
        let mut no_total = ok.clone();
        no_total[6..8].copy_from_slice(&0u16.to_be_bytes());
        assert_eq!(decode_fft(&no_total, false), None);
        let mut no_bins = ok.clone();
        no_bins[2..4].copy_from_slice(&0u16.to_be_bytes());
        assert_eq!(decode_fft(&no_bins, false), None);
        assert_eq!(decode_fft(&ok[..11], false), None, "a cut sub-header");
    }

    // ── Rows to levels and dBm ───────────────────────────────────────────────────────────────

    /// THE ROW IS COUNTED FROM THE TOP. A frame with a carrier at row 140 and the floor at row
    /// 420 of a 701-row display, on a -140..-40 dBm window, reads -60 dBm and -100 dBm: 0.8 and
    /// 0.4 of the window. The top row is the strongest reading, not the weakest.
    #[test]
    fn a_known_level_frame_reads_its_level_and_its_dbm() {
        const Y: u32 = 701;
        let window = DbmRange::new(-140.0, -40.0);
        let frame = FftFrame {
            frame_index: 1,
            rows: vec![420, 420, 140, 420, 0, 700, 900],
            floor_from: None,
        };
        let dbm = frame.dbm(Y, window);
        for (bin, want) in [(0, -100.0), (2, -60.0), (4, -40.0), (5, -140.0)] {
            assert!(
                (dbm[bin] - want).abs() < 1e-3,
                "bin {bin}: {} dBm",
                dbm[bin]
            );
        }
        assert!(
            (dbm[6] - -140.0).abs() < 1e-3,
            "a row past the bottom is a clipped floor sample"
        );
        let levels = frame.levels(Y);
        for (bin, want) in [(0, 0.4), (2, 0.8), (4, 1.0), (5, 0.0), (6, 0.0)] {
            assert!(
                (levels[bin] - want).abs() < 1e-6,
                "bin {bin}: {}",
                levels[bin]
            );
        }
        // The level is the dBm's place in the window, whatever the window.
        for (level, dbm) in levels.iter().zip(&dbm) {
            let from_dbm = (dbm - window.min_dbm) / (window.max_dbm - window.min_dbm);
            assert!((level - from_dbm).abs() < 1e-5);
        }
    }

    #[test]
    fn the_window_is_held_to_its_floor_and_its_narrowest_span() {
        assert_eq!(DbmRange::new(-200.0, -40.0).min_dbm, -180.0);
        assert_eq!(DbmRange::new(-100.0, -100.0).max_dbm, -90.0);
        assert_eq!(
            DbmRange::new(-130.0, -40.0),
            DbmRange {
                min_dbm: -130.0,
                max_dbm: -40.0
            }
        );
        // A one-row display is read as two rows, never divided by zero.
        assert_eq!(row_level(0, 1), 1.0);
        assert_eq!(row_level(5, 0), 0.0);
    }

    // ── FFT frame assembly ───────────────────────────────────────────────────────────────────

    #[test]
    fn a_frame_completes_when_every_bin_has_arrived() {
        let mut a = FftAssembler::new();
        assert_eq!(a.push(&fragment(7, 0, 4, &[1, 2])), None);
        let frame = a
            .push(&fragment(7, 2, 4, &[3, 4]))
            .expect("covered end to end");
        assert_eq!(frame.rows, vec![1, 2, 3, 4]);
        assert_eq!(frame.floor_from, None);
    }

    /// A DUPLICATED FRAGMENT CANNOT COMPLETE A FRAME. Counting fragments, the same half twice
    /// would complete a four-bin frame whose other half is still zero, and a zero row is the TOP
    /// of the display: a false wall at `max_dbm`, not a quiet band.
    #[test]
    fn a_duplicated_fragment_cannot_complete_a_half_covered_frame() {
        let mut a = FftAssembler::new();
        let half = fragment(3, 0, 4, &[9, 9]);
        assert_eq!(a.push(&half), None);
        assert_eq!(
            a.push(&half),
            None,
            "the same two bins twice is still half a frame"
        );
        let frame = a.push(&fragment(3, 2, 4, &[7, 8])).expect("the other half");
        assert_eq!(frame.rows, vec![9, 9, 7, 8]);
    }

    /// A FRAME THAT HAS COMPLETED TAKES NOTHING MORE. A packet duplicated in transit after its
    /// frame completed must not begin that frame again, which would publish it twice.
    #[test]
    fn a_completed_frame_is_never_completed_twice() {
        let mut a = FftAssembler::new();
        let whole = fragment(5, 0, 2, &[4, 4]);
        assert!(a.push(&whole).is_some());
        assert_eq!(a.push(&whole), None);
        assert!(
            a.push(&fragment(6, 0, 2, &[4, 4])).is_some(),
            "the next frame stands"
        );
    }

    /// REORDERING ACROSS FRAMES COSTS AT MOST THE FRAME IT TOUCHES. With one frame in flight,
    /// each frame's tail arriving after the next frame's head (ordinary UDP) completes none of
    /// them: a permanently blank pan (Flex audit 2026-08-17).
    #[test]
    fn frames_still_complete_when_their_fragments_interleave() {
        let mut a = FftAssembler::new();
        let mut frames = 0;
        for n in 0..6u32 {
            assert_eq!(a.push(&fragment(n, 0, 4, &[300, 300])), None);
            if n > 0 && a.push(&fragment(n - 1, 2, 4, &[300, 300])).is_some() {
                frames += 1;
            }
        }
        assert_eq!(frames, 5, "every frame whose tail arrived");
    }

    #[test]
    fn frames_in_progress_are_bounded() {
        let mut a = FftAssembler::new();
        for n in 0..50u32 {
            assert_eq!(a.push(&fragment(n, 0, 4, &[1])), None);
        }
        assert_eq!(a.frames.inflight.len(), IN_FLIGHT);
    }

    /// A fragment that does not fit its frame is dropped before it can take the place of a frame
    /// in progress, and a fragment reusing a frame index with another width is another frame.
    #[test]
    fn a_fragment_that_does_not_fit_displaces_nothing() {
        let mut a = FftAssembler::new();
        assert_eq!(a.push(&fragment(1, 0, 4, &[1, 1])), None);
        for n in 10..20u32 {
            assert_eq!(a.push(&fragment(n, 3, 4, &[1, 1])), None, "past the end");
        }
        assert_eq!(
            a.push(&fragment(1, 0, 8, &[2; 8])).map(|f| f.rows.len()),
            Some(8)
        );
        let frame = a
            .push(&fragment(1, 2, 4, &[1, 1]))
            .expect("frame 1 survived");
        assert_eq!(frame.rows, vec![1, 1, 1, 1]);
    }

    fn whole(frame: u32, rows: Vec<u16>) -> FftFragment {
        let total = rows.len() as u16;
        fragment(frame, 0, total, &rows)
    }

    /// The x_pixels growth placeholder is held back three times, then emitted with its new bins
    /// at the floor; a properly filled wider frame is accepted at once and resets the guard.
    #[test]
    fn the_growth_placeholder_never_draws_a_wall() {
        const Y: u32 = 700;
        let mut a = FftAssembler::new();
        let narrow = whole(0, vec![500; 32]);
        assert!(a.push(&narrow).is_some());
        let mut placeholder = vec![500u16; 64];
        placeholder[32..].fill(0);
        for n in 1..=3 {
            assert_eq!(
                a.push(&whole(n, placeholder.clone())),
                None,
                "held back {n}"
            );
        }
        let emitted = a
            .push(&whole(4, placeholder.clone()))
            .expect("after the wait");
        assert_eq!(emitted.floor_from, Some(32));
        let levels = emitted.levels(Y);
        assert!(levels[..32]
            .iter()
            .all(|l| (l - (1.0 - 500.0 / 699.0)).abs() < 1e-6));
        assert!(
            levels[32..].iter().all(|&l| l == 0.0),
            "the new bins sit at the floor"
        );

        let filled = a
            .push(&whole(5, vec![480; 64]))
            .expect("a real wider frame");
        assert_eq!(filled.floor_from, None);
        assert_eq!(a.growth.consecutive_rejected(), 0);
        assert_eq!(a.last_accepted_total, 64);
    }

    // ── Waterfall tiles ──────────────────────────────────────────────────────────────────────

    #[test]
    fn a_tile_decodes_its_frequencies_and_intensities() {
        let payload = tile_payload(
            vita_raw(14.0),
            vita_raw(0.0001953125),
            4,
            9,
            100,
            4,
            0,
            &[100 * 128, 104 * 128, 115 * 128, -128],
        );
        let dg = packet(0x4200_0000, WATERFALL_CLASS, false, &payload);
        let v = parse_vita(&dg).unwrap();
        assert_eq!(v.packet_class, Some(WATERFALL_CLASS));
        let t = decode_tile(v.payload, v.has_trailer).expect("a tile");
        near("low", t.frequency.low_mhz, 14.0, 1e-9);
        near("bin", t.frequency.bin_bw_mhz, 0.0001953125, 1e-12);
        assert_eq!(
            (t.width, t.height, t.timecode, t.auto_black),
            (4, 1, 9, 100)
        );
        assert_eq!((t.total_bins, t.first_bin, t.line_duration), (4, 0, 80));
        assert_eq!(
            t.values,
            vec![100.0, 104.0, 115.0, -1.0],
            "i16 / 128, not dBm"
        );
    }

    #[test]
    fn a_tile_the_decoder_cannot_read_is_refused() {
        let ok = tile_payload(0, 1, 4, 1, 0, 4, 0, &[1, 2, 3, 4]);
        assert!(decode_tile(&ok, false).is_some());
        assert_eq!(
            decode_tile(&ok[..ok.len() - 1], false),
            None,
            "less than a row"
        );
        let past = tile_payload(0, 1, 4, 1, 0, 4, 4, &[1, 2, 3, 4]);
        assert_eq!(
            decode_tile(&past, false),
            None,
            "first bin at the end of the row"
        );
        let beyond = tile_payload(0, 1, 4, 1, 0, 4, 9, &[1, 2, 3, 4]);
        assert_eq!(decode_tile(&beyond, false), None, "first bin past the row");
        let mut flat = ok.clone();
        flat[22..24].copy_from_slice(&0u16.to_be_bytes());
        assert_eq!(decode_tile(&flat, false), None, "no height");
        assert_eq!(decode_tile(&ok[..35], false), None, "a cut sub-header");
        // A tile that runs past its row is read only to the row's end.
        let tail = tile_payload(0, 1, 4, 1, 0, 6, 4, &[1, 2, 3, 4]);
        assert_eq!(decode_tile(&tail, false).unwrap().values.len(), 2);
    }

    #[test]
    fn tiles_assemble_into_a_row_with_coverage() {
        let mut a = TileAssembler::new();
        let low = vita_raw(7.0);
        let bin = vita_raw(0.001);
        let tile = |first: u16, values: &[i16]| {
            decode_tile(
                &tile_payload(low, bin, values.len() as u16, 3, 15, 4, first, values),
                false,
            )
            .unwrap()
        };
        assert_eq!(a.push(&tile(0, &[256, 256])), None);
        assert_eq!(
            a.push(&tile(0, &[256, 256])),
            None,
            "a duplicate is still half a row"
        );
        let row = a.push(&tile(2, &[512, 512])).expect("the whole row");
        assert_eq!(row.timecode, 3);
        assert_eq!(row.auto_black, 15);
        assert_eq!(row.values, vec![2.0, 2.0, 4.0, 4.0]);
        near("row low", row.low_mhz, 7.0, 1e-9);
        near("row high", row.high_mhz, 7.004, 1e-9);
        assert_eq!(a.push(&tile(2, &[512, 512])), None, "the row is complete");
    }
}
