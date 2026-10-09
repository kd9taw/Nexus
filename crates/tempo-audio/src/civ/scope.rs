//! Icom scope-waveform assembly (CI-V command `0x27`) — pure, no I/O.
//!
//! With waveform output enabled, the radio streams each scope sweep as a numbered burst of
//! `27 00` frames: the first frame of a burst carries a header (center/fixed mode, the RF
//! position, an out-of-range flag), the rest carry the waveform points (one byte each,
//! display height up to the radio's own top: `0xA0`, or `0xC8` on an IC-7610 or IC-7760 —
//! see `point_max`). [`ScopeAssembler`] reassembles bursts into complete
//! sweeps normalized to the waterfall's 0..1 row contract, tagged with the absolute RF span,
//! and [`publish_sweep`] hands each one to the spectrum feed as one frame.
//!
//! Scope bytes never collide with CI-V framing (`FE`/`FD`/`FB`/`FA`): sequence counters are
//! BCD, frequencies/spans are BCD, and waveform points top out at `0xC8` — so the ordinary
//! [`super::frame::FrameSplitter`] splits scope frames safely and hands them here.
//!
//! Byte layout per the official Icom CI-V reference (IC-7300 §19, command 27; shared by the
//! 7300 family): waveform output streams only while `27 10` (scope ON) and `27 11` (wave
//! data output) are both ON; each sweep is divided into 11 frames over USB, the 1st carrying
//! only the header (`00`=center/`01`=fixed, frequency, span-or-edges, out-of-range — data
//! omitted when out of range), the rest the points; data range 0–160, length 475; the span
//! table (2500="2.5k" … 500000="500k") is the ± half-width. The manual's own example frame
//! is a fixture test below. The IC-9700 (its own CI-V guide, A7508-3EX-1) uses the SAME frame
//! layout, point count, and height range; its dual-receiver differences are only which scope
//! the stream carries (`27 12`, pinned to Main by [`scope_stream_frames`]) and the rig-side
//! precondition that wave output over USB needs CI-V USB baud 115200 (it NAKs `27 11 01` at
//! lower rates — the "scope never streams" trap). The IC-7610 (A7380-7EX-4, PDF p. 15) has
//! the same frame layout with two different numbers: 15 frames over USB (the header, 13 of
//! 50 points, then 39) and data range 0–200, length 689 — which is why the scale is per model.
//! The IC-905 (A7711-9EX-2, PDF p. 29) has the 7300's layout, except on its 10 GHz band, where
//! the frequency, or each edge, is 12 digits (6 bytes) instead of 10 — see `ten_ghz_points`.

use super::commands::IcomModel;
use super::frame::{bcd_to_freq, Frame};

/// The top of this radio's waveform scale: the value its strongest point is sent as. Each one
/// is the "Data range" in item 7 of the radio's own "Scope waveform data" (`27 00`) entry:
///
/// | Model | Top | Points | Source |
/// |---|---|---|---|
/// | IC-7300 | 160 (`A0`) | 475 | Full Manual A7292-4EX-12, PDF p. 172 (printed 19-14) |
/// | IC-7610 | **200** (`C8`) | 689 | CI-V Reference Guide A7380-7EX-4, PDF p. 15 |
/// | IC-9700 | 160 | 475 | CI-V Reference Guide A7508-3EX-4, PDF p. 26 |
/// | IC-705 | 160 | 475 | CI-V Reference Guide A7560-8EX-6, PDF p. 29 |
/// | IC-905 | 160 (`A0`) | 475 | CI-V Reference Guide A7711-9EX-2, PDF p. 29 |
/// | IC-7760 | **200** (`C8`) | 689 | CI-V Reference Guide A7788-8EX-2, PDF p. 25 |
/// | IC-7300MK2 | 160 (`A0`) | 475 | CI-V Reference Guide rev 0, PDF p. 24 |
///
/// Read against 160, every IC-7610 point from 161 to 200 drew at full height: the strongest
/// fifth of its scale came out flat. A radio the caller did not name (`None`) keeps 160, the
/// top of every native Icom but the 7610 and the 7760. No wildcard arm, so a model added to
/// [`IcomModel`] has to be read against its own guide first.
fn point_max(model: Option<IcomModel>) -> u8 {
    match model {
        Some(IcomModel::Ic7610 | IcomModel::Ic7760) => 200,
        Some(
            IcomModel::Ic7300
            | IcomModel::Ic9700
            | IcomModel::Ic705
            | IcomModel::Ic905
            | IcomModel::Ic7300Mk2,
        )
        | None => 160,
    }
}

/// How many points a sweep carries on a radio with a 10 GHz band, or `None` for a radio without
/// one. Only the IC-905 has one, and there its scope frequencies are 12 digits (6 bytes) instead
/// of 10 (5 bytes): "When the 10 GHz band is selected, the Center frequency is 12 digits (6 bytes)
/// from 100 GHz to 1 Hz", and in the Fixed and scroll modes "the each Edge frequency is 12 digits
/// (6 bytes)" (CI-V Reference Guide A7711-9EX-2, PDF p. 29, `27 00` item 5). Its sweep is "Data
/// length: 475" (item 7), which is what tells a network sweep's 12-digit header from a 10-digit
/// one ([`parse_waveform`]). Every other radio here stops below 10 GHz, where its guide gives 10
/// digits. No wildcard arm, as in [`point_max`].
fn ten_ghz_points(model: Option<IcomModel>) -> Option<usize> {
    match model {
        Some(IcomModel::Ic905) => Some(475),
        Some(
            IcomModel::Ic7300
            | IcomModel::Ic7610
            | IcomModel::Ic9700
            | IcomModel::Ic705
            | IcomModel::Ic7760
            | IcomModel::Ic7300Mk2,
        )
        | None => None,
    }
}
/// Cap accumulated points per sweep — a corrupt total can't grow the buffer unbounded.
const MAX_POINTS: usize = 4096;

/// One completed scope sweep, normalized for [`tempo_app::dto::Spectrum`].
#[derive(Debug, Clone, PartialEq)]
pub struct ScopeSweep {
    /// Waveform points normalized 0..1 (the UI's AGC/LUT does the display stretch).
    pub row: Vec<f32>,
    /// Absolute RF span the row covers.
    pub lo_hz: f64,
    pub hi_hz: f64,
}

/// The CI-V frames that enable/disable the scope waveform stream to the controller:
/// `27 10` turns the scope itself on (harmless if already on), `27 11` switches the
/// waveform-data output to the CI-V port. Sent on every enable transition — both are
/// idempotent on the radio.
///
/// Per the official IC-9700 CI-V reference (A7508-3EX-1): `27 10`/`27 11` take a bare
/// on/off byte on every model (no Main/Sub prefix); on dual-receiver rigs (IC-9700,
/// IC-7610) which of the two scopes the stream carries is selected GLOBALLY via `27 12`
/// (00=Main / 01=Sub), so the enable sequence pins Main first — the assembler renders the
/// Main scope and drops Sub sweeps. Single-receiver rigs don't have `27 12`; a NAK from
/// them is harmless (internal request, no reply consumer), so it's sent only to known
/// dual-scope addresses. NOTE the rig-side precondition (same reference, `27 11` footnote):
/// wave output over USB requires CI-V USB Port = "Unlink from [REMOTE]" AND CI-V USB baud
/// 115200 — at lower baud the rig NAKs `27 11 01` (verified on an IC-9700 at 57600).
pub fn scope_stream_frames(radio: u8, on: bool) -> Vec<Frame> {
    let b = u8::from(on);
    if on {
        let mut frames = Vec::with_capacity(3);
        if scope_is_dual(radio) {
            frames.push(Frame::command(radio, 0x27, &[0x12, 0x00])); // target = Main scope
        }
        frames.push(Frame::command(radio, 0x27, &[0x10, 0x01]));
        frames.push(Frame::command(radio, 0x27, &[0x11, b]));
        frames
    } else {
        // Leave the scope display itself as the operator had it; just stop the stream.
        vec![Frame::command(radio, 0x27, &[0x11, b])]
    }
}

/// True for dual-receiver Icoms (two scopes: Main + Sub) — IC-7610 (0x98), IC-9700 (0xA2) and
/// IC-7760 (0xB2) by CI-V default address. Their `27 14/15/19` scope-CONTROL commands take a
/// leading Main/Sub selector byte that single-scope rigs (IC-7300/705/905/7300MK2) omit; the
/// IC-7760's `27 14`, `27 17` and `27 19` formats carry it as `00=MAIN, 01=SUB` (A7788-8EX-2 PDF
/// pp. 25–26), and it selects its scope with `27 12` (p. 17).
pub fn scope_is_dual(radio: u8) -> bool {
    matches!(radio, 0x98 | 0xA2 | 0xB2)
}

/// One decimal from a BCD byte pair position (two digits per byte).
fn bcd_byte(b: u8) -> u32 {
    let hi = (b >> 4) as u32;
    let lo = (b & 0x0F) as u32;
    (if hi > 9 { 0 } else { hi }) * 10 + if lo > 9 { 0 } else { lo }
}

/// The header carried by the FIRST frame of each sweep burst.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SweepHeader {
    /// Absolute RF edges the sweep covers.
    lo_hz: f64,
    hi_hz: f64,
    /// The radio flags the sweep invalid while retuning (out-of-range) — dropped.
    out_of_range: bool,
}

/// Parse one `27 00` waveform frame's data (everything after the `27` command byte,
/// i.e. `data[0] == 0x00`). Returns `(sequence, total, header-if-first, points)`.
///
/// Layout (after the `00` sub-command):
/// `[main/sub] [seq BCD] [seq-total BCD]` then, in the first frame of a burst:
/// `[mode 00=center|01=fixed] [freq 5B BCD] [span-or-upper 5B BCD] [oor]`;
/// in every later frame: waveform points, one byte each. On a radio's 10 GHz band the
/// frequency, or each edge, is 6 bytes ([`ten_ghz_points`], that radio's sweep length, or
/// `None` for a radio without the band).
fn parse_waveform(
    data: &[u8],
    ten_ghz_points: Option<usize>,
) -> Option<(u32, u32, Option<SweepHeader>, &[u8])> {
    // data[0] = 0x00 sub-command, [1] = main(00)/sub(01) receiver, [2] = seq, [3] = total.
    // MAIN receiver only: on a dual-watch IC-9700 the sub receiver's sweeps interleave on
    // the same command — mixing the two bursts would corrupt both.
    if data.len() < 4 || data[0] != 0x00 || data[1] != 0x00 {
        return None;
    }
    let seq = bcd_byte(data[2]);
    let total = bcd_byte(data[3]);
    if seq == 0 || total == 0 || seq > total {
        return None;
    }
    if seq == 1 {
        // Header frame: mode + position + span/edge + out-of-range flag.
        if data.len() < 16 {
            return None;
        }
        // Mode byte: 00=Center, 01=Fixed, 02=Scroll-C (center-style fields),
        // 03=Scroll-F (fixed-style fields). Anything newer/unknown → drop the sweep
        // rather than misread its fields.
        let center_style = match data[4] {
            0x00 | 0x02 => true,
            0x01 | 0x03 => false,
            _ => return None,
        };
        // How many bytes each frequency takes: 5, except on a 10 GHz band, where the center
        // frequency is 6 and the span keeps its 5, or each edge is 6 (A7711-9EX-2 PDF p. 29,
        // `27 00` item 5). Nothing in the header names the band, so its length says which:
        // over USB the first division carries the header alone, and over the network the one
        // division carries the header and the whole sweep, or the header alone when the sweep
        // is out of range (items 2, 3, 6 and 7).
        let (wa, wb) = match ten_ghz_points {
            Some(points) => {
                let wide = if center_style { (6, 5) } else { (6, 6) };
                let header = 1 + wide.0 + wide.1 + 1; // the mode, the two, out of range
                let after = data.len() - 4;
                if after == header || after == header + points {
                    wide
                } else {
                    (5, 5)
                }
            }
            None => (5, 5),
        };
        let a = bcd_to_freq(&data[5..5 + wa]) as f64;
        let b = bcd_to_freq(&data[5 + wa..5 + wa + wb]) as f64;
        let oor = 5 + wa + wb;
        let (lo, hi) = if center_style {
            // Center: center frequency ± span (the span value is the ± half-width).
            (a - b, a + b)
        } else {
            // Fixed: lower edge, upper edge. An `F` in the lower edge's top digit, the high
            // nibble of its last byte, means the edge is NEGATIVE and the other digits are its
            // absolute value. That is the 1 GHz digit in 10 digits: A7380-7EX-4 (IC-7610) PDF
            // p. 15, the IC-7300 Full Manual A7292-4EX-12 PDF p. 172, A7560-8EX-6 (IC-705) PDF
            // p. 29; and the 100 GHz digit in 12: A7711-9EX-2 (IC-905) PDF p. 29. `bcd_to_freq`
            // reads the F as 0, so `a` is already that absolute value; only the sign was lost.
            let lo = if data[4 + wa] >> 4 == 0x0F { -a } else { a };
            (lo, b)
        };
        let header = SweepHeader {
            lo_hz: lo,
            hi_hz: hi,
            out_of_range: data[oor] != 0x00,
        };
        Some((seq, total, Some(header), &data[oor + 1..]))
    } else {
        Some((seq, total, None, &data[4..]))
    }
}

/// Reassembles waveform bursts into [`ScopeSweep`]s. Feed every `cmd == 0x27` frame;
/// a completed, in-range sweep pops out. Missed/out-of-order frames drop the burst and
/// resync on the next header — a lossy stream degrades to a lower frame rate, never to
/// a corrupted row.
#[derive(Debug)]
pub struct ScopeAssembler {
    header: Option<SweepHeader>,
    points: Vec<u8>,
    next_seq: u32,
    total: u32,
    /// The top of this radio's scale (`point_max`): a point is divided by it.
    point_max: f32,
    /// This radio's sweep length if it has a 10 GHz band (`ten_ghz_points`).
    ten_ghz_points: Option<usize>,
}

impl ScopeAssembler {
    /// An assembler for `model`'s sweeps, scaled to that radio's own range.
    pub fn new(model: Option<IcomModel>) -> Self {
        ScopeAssembler {
            header: None,
            points: Vec::new(),
            next_seq: 0,
            total: 0,
            point_max: f32::from(point_max(model)),
            ten_ghz_points: ten_ghz_points(model),
        }
    }

    pub fn push(&mut self, f: &Frame) -> Option<ScopeSweep> {
        if f.cmd != 0x27 {
            return None;
        }
        let (seq, total, header, points) = parse_waveform(&f.data, self.ten_ghz_points)?;
        if seq == 1 {
            // A new burst always resets the assembler (implicitly drops a partial one).
            self.header = header;
            self.points.clear();
            self.points.extend_from_slice(points);
            self.next_seq = 2;
            self.total = total;
        } else {
            // Continuation: must be the frame we expect, in the burst we're building.
            if self.header.is_none() || seq != self.next_seq || total != self.total {
                self.reset();
                return None;
            }
            if self.points.len() + points.len() > MAX_POINTS {
                self.reset();
                return None;
            }
            self.points.extend_from_slice(points);
            self.next_seq += 1;
        }
        // Burst complete?
        if seq == self.total {
            let header = self.header.take()?;
            let points = std::mem::take(&mut self.points);
            self.reset();
            if header.out_of_range || points.is_empty() || header.hi_hz <= header.lo_hz {
                return None;
            }
            let row = points
                .iter()
                .map(|&p| (f32::from(p) / self.point_max).min(1.0))
                .collect();
            return Some(ScopeSweep {
                row,
                lo_hz: header.lo_hz,
                hi_hz: header.hi_hz,
            });
        }
        None
    }

    fn reset(&mut self) {
        self.header = None;
        self.points.clear();
        self.next_seq = 0;
        self.total = 0;
    }
}

/// Publish one completed sweep to the spectrum feed: ONE call per sweep, and the feed stamps it
/// with the next frame number, which is how a reader polling faster than the radio sweeps tells a
/// new sweep from the last one again.
///
/// - The scale is RELATIVE: a point is the rig's display height (0–160, or 0–200 on an
///   IC-7610) against its own REF level, which Nexus sets but never reads back, so no dB axis
///   can be claimed for it.
/// - The slice is 0, the Main receiver: the stream is pinned to the Main scope
///   ([`scope_stream_frames`]) and `parse_waveform` drops the Sub's sweeps.
pub fn publish_sweep(feed: &tempo_app::engine::SpectrumFeed, sweep: ScopeSweep) {
    feed.publish_rf_frame(
        tempo_app::dto::Spectrum {
            row: sweep.row,
            lo_hz: sweep.lo_hz,
            hi_hz: sweep.hi_hz,
            source: "civ".into(),
        },
        tempo_app::dto::SpectrumScale::Relative,
        Some(0),
    );
}

#[cfg(test)]
mod tests {
    use super::super::frame::{freq_to_bcd, Frame};
    use super::*;

    /// Build a `27 00` waveform frame as the radio would send it.
    fn wf_frame(seq: u8, total: u8, body: &[u8]) -> Frame {
        // BCD-encode seq/total (two decimal digits per byte).
        let bcd = |v: u8| ((v / 10) << 4) | (v % 10);
        let mut data = vec![0x00, 0x00, bcd(seq), bcd(total)];
        data.extend_from_slice(body);
        Frame {
            to: 0xE0,
            from: 0xA2,
            cmd: 0x27,
            data,
        }
    }

    /// A center-mode header body: center 145 MHz, span ±25 kHz, in range.
    fn center_header() -> Vec<u8> {
        let mut b = vec![0x00]; // center mode
        b.extend_from_slice(&freq_to_bcd(145_000_000));
        b.extend_from_slice(&freq_to_bcd(25_000));
        b.push(0x00); // in range
        b
    }

    #[test]
    fn assembles_a_three_frame_center_mode_sweep() {
        let mut asm = ScopeAssembler::new(Some(IcomModel::Ic9700));
        assert!(asm.push(&wf_frame(1, 3, &center_header())).is_none());
        assert!(asm.push(&wf_frame(2, 3, &[0, 40, 80])).is_none());
        let sweep = asm.push(&wf_frame(3, 3, &[120, 160])).expect("complete");
        assert_eq!(sweep.row.len(), 5);
        assert_eq!(sweep.row[0], 0.0);
        assert!((sweep.row[1] - 0.25).abs() < 0.01);
        assert_eq!(sweep.row[4], 1.0);
        // Center ± span → absolute RF edges.
        assert_eq!(sweep.lo_hz, 144_975_000.0);
        assert_eq!(sweep.hi_hz, 145_025_000.0);
    }

    #[test]
    fn fixed_mode_header_uses_edges_directly() {
        let mut asm = ScopeAssembler::new(Some(IcomModel::Ic9700));
        let mut hdr = vec![0x01]; // fixed mode
        hdr.extend_from_slice(&freq_to_bcd(144_000_000)); // lower
        hdr.extend_from_slice(&freq_to_bcd(144_500_000)); // upper
        hdr.push(0x00);
        assert!(asm.push(&wf_frame(1, 2, &hdr)).is_none());
        let sweep = asm.push(&wf_frame(2, 2, &[10, 20])).expect("complete");
        assert_eq!(sweep.lo_hz, 144_000_000.0);
        assert_eq!(sweep.hi_hz, 144_500_000.0);
    }

    /// A NEGATIVE LOWER EDGE. In the Fixed and Scroll-F modes the radio sends the two edges, and
    /// an `F` in the lower edge's 1 GHz digit means the edge is below 0 Hz, its other digits the
    /// absolute value. Read as a 0, the F put the sweep over a positive span the radio was not
    /// showing.
    #[test]
    fn an_f_in_the_lower_edges_1_ghz_digit_is_a_negative_edge() {
        // −20 kHz: the absolute value, 20 kHz, with the 1 GHz digit (the fifth byte's high
        // nibble) set to F.
        let mut lower = freq_to_bcd(20_000);
        lower[4] |= 0xF0;
        assert_eq!(lower, [0x00, 0x00, 0x02, 0x00, 0xF0]);
        for mode in [0x01u8, 0x03] {
            // Fixed, and Scroll-F (fixed-style edges).
            let mut hdr = vec![mode];
            hdr.extend_from_slice(&lower);
            hdr.extend_from_slice(&freq_to_bcd(480_000));
            hdr.push(0x00);
            let mut asm = ScopeAssembler::new(Some(IcomModel::Ic7300));
            assert!(asm.push(&wf_frame(1, 2, &hdr)).is_none());
            let sweep = asm.push(&wf_frame(2, 2, &[0, 80, 160])).expect("complete");
            assert_eq!(
                (sweep.lo_hz, sweep.hi_hz),
                (-20_000.0, 480_000.0),
                "mode {mode:02X}"
            );
            assert_eq!(sweep.row, [0.0, 0.5, 1.0], "mode {mode:02X}");
        }
        // Control: a 1 GHz digit that is a digit. An IC-9700's 23 cm fixed scope keeps the
        // positive edges it has.
        let mut hdr = vec![0x01];
        hdr.extend_from_slice(&freq_to_bcd(1_240_000_000));
        hdr.extend_from_slice(&freq_to_bcd(1_300_000_000));
        hdr.push(0x00);
        let mut asm = ScopeAssembler::new(Some(IcomModel::Ic9700));
        assert!(asm.push(&wf_frame(1, 2, &hdr)).is_none());
        let sweep = asm.push(&wf_frame(2, 2, &[10, 20])).expect("complete");
        assert_eq!(
            (sweep.lo_hz, sweep.hi_hz),
            (1_240_000_000.0, 1_300_000_000.0)
        );
    }

    #[test]
    fn a_missed_frame_drops_the_burst_and_resyncs_on_the_next_header() {
        let mut asm = ScopeAssembler::new(Some(IcomModel::Ic9700));
        assert!(asm.push(&wf_frame(1, 3, &center_header())).is_none());
        // Frame 2 lost; frame 3 arrives → burst dropped, no bogus sweep.
        assert!(asm.push(&wf_frame(3, 3, &[1, 2])).is_none());
        // The next full burst still assembles.
        assert!(asm.push(&wf_frame(1, 2, &center_header())).is_none());
        assert!(asm.push(&wf_frame(2, 2, &[5, 6])).is_some());
    }

    #[test]
    fn out_of_range_sweeps_are_dropped() {
        let mut asm = ScopeAssembler::new(Some(IcomModel::Ic9700));
        let mut hdr = vec![0x00];
        hdr.extend_from_slice(&freq_to_bcd(145_000_000));
        hdr.extend_from_slice(&freq_to_bcd(25_000));
        hdr.push(0x01); // OUT of range (mid-retune)
        assert!(asm.push(&wf_frame(1, 2, &hdr)).is_none());
        assert!(asm.push(&wf_frame(2, 2, &[1, 2, 3])).is_none(), "dropped");
    }

    #[test]
    fn parses_the_manuals_own_example_header_frame() {
        // The IC-7300 CI-V reference's worked example (§19, command 27 00):
        //   27 00 | 00 | 01 | 11 | 01 | 00 00 00 14 00 | 00 00 35 14 00 | 00
        // = main scope, division 1 of 11, FIXED mode, lower edge 14.000 MHz,
        //   upper edge 14.350 MHz, in range — a fixed 20 m band scope.
        let f = Frame {
            to: 0xE0,
            from: 0x94,
            cmd: 0x27,
            data: vec![
                0x00, 0x00, 0x01, 0x11, 0x01, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x35, 0x14,
                0x00, 0x00,
            ],
        };
        let (seq, total, header, points) =
            parse_waveform(&f.data, ten_ghz_points(Some(IcomModel::Ic7300))).expect("parses");
        assert_eq!(seq, 1);
        assert_eq!(total, 11, "BCD 0x11 = 11 divisions over USB");
        let h = header.expect("first frame carries the header");
        assert_eq!(h.lo_hz, 14_000_000.0);
        assert_eq!(h.hi_hz, 14_350_000.0);
        assert!(!h.out_of_range);
        assert!(points.is_empty(), "the 1st division has no waveform data");
    }

    #[test]
    fn sub_receiver_sweeps_are_ignored() {
        // A dual-watch IC-9700 interleaves sub-receiver sweeps (main/sub byte 01) on the
        // same command — mixing them into the main burst would corrupt both.
        let mut asm = ScopeAssembler::new(Some(IcomModel::Ic9700));
        let mut data = vec![0x00, 0x01, 0x01, 0x02]; // sub receiver, seq 1 of 2
        data.extend_from_slice(&center_header());
        let sub = Frame {
            to: 0xE0,
            from: 0xA2,
            cmd: 0x27,
            data,
        };
        assert!(asm.push(&sub).is_none());
        // A main-receiver burst still assembles cleanly around it.
        assert!(asm.push(&wf_frame(1, 2, &center_header())).is_none());
        assert!(asm.push(&sub).is_none());
        assert!(asm.push(&wf_frame(2, 2, &[5, 6])).is_some());
    }

    #[test]
    fn scroll_modes_parse_with_their_base_styles_and_unknown_modes_drop() {
        // Mode 02 = Scroll-C carries CENTER-style fields (center ± span); 03 = Scroll-F
        // carries FIXED-style edges; an unknown mode byte must drop, not misread.
        let mut asm = ScopeAssembler::new(Some(IcomModel::Ic9700));
        let mut hdr = vec![0x02]; // Scroll-C
        hdr.extend_from_slice(&freq_to_bcd(145_000_000));
        hdr.extend_from_slice(&freq_to_bcd(25_000));
        hdr.push(0x00);
        assert!(asm.push(&wf_frame(1, 2, &hdr)).is_none());
        let sweep = asm
            .push(&wf_frame(2, 2, &[1, 2]))
            .expect("scroll-C assembles");
        assert_eq!(sweep.lo_hz, 144_975_000.0, "center-style math for Scroll-C");
        assert_eq!(sweep.hi_hz, 145_025_000.0);

        let mut bad = vec![0x07]; // unknown future mode
        bad.extend_from_slice(&freq_to_bcd(145_000_000));
        bad.extend_from_slice(&freq_to_bcd(25_000));
        bad.push(0x00);
        assert!(asm.push(&wf_frame(1, 2, &bad)).is_none());
        assert!(
            asm.push(&wf_frame(2, 2, &[1, 2])).is_none(),
            "unknown mode dropped"
        );
    }

    #[test]
    fn enable_disable_frames() {
        // Single-receiver rig (IC-7300, 0x94): no 27 12 — it doesn't have that command.
        let on = scope_stream_frames(0x94, true);
        assert_eq!(on.len(), 2);
        assert_eq!(on[0].data, vec![0x10, 0x01]); // scope on
        assert_eq!(on[1].data, vec![0x11, 0x01]); // waveform output on
        let off = scope_stream_frames(0x94, false);
        assert_eq!(off.len(), 1);
        assert_eq!(off[0].data, vec![0x11, 0x00]); // stream off, display untouched
    }

    #[test]
    fn dual_scope_rigs_pin_the_main_scope_first() {
        // IC-9700 (0xA2) / IC-7610 (0x98): the stream carries whichever scope 27 12 targets,
        // so the enable pins Main — the assembler renders Main and drops Sub sweeps.
        for addr in [0xA2u8, 0x98] {
            let on = scope_stream_frames(addr, true);
            assert_eq!(on.len(), 3, "addr {addr:#04x}");
            assert_eq!(on[0].data, vec![0x12, 0x00]); // target = Main scope
            assert_eq!(on[1].data, vec![0x10, 0x01]);
            assert_eq!(on[2].data, vec![0x11, 0x01]);
            // Disable is unchanged — one frame, display untouched.
            assert_eq!(scope_stream_frames(addr, false).len(), 1);
        }
    }

    #[test]
    fn garbage_and_foreign_subcommands_are_ignored() {
        let mut asm = ScopeAssembler::new(Some(IcomModel::Ic9700));
        // A 27 14 (mode set ack echo) or short/foreign frame must not panic or emit.
        let foreign = Frame {
            to: 0xE0,
            from: 0xA2,
            cmd: 0x27,
            data: vec![0x14, 0x00, 0x01],
        };
        assert!(asm.push(&foreign).is_none());
        let short = Frame {
            to: 0xE0,
            from: 0xA2,
            cmd: 0x27,
            data: vec![0x00],
        };
        assert!(asm.push(&short).is_none());
    }

    /// ONE COMPLETED SWEEP, ONE FRAME. The scope draws a new row only when the frame number
    /// advances, so a sweep published without one would never be drawn and a sweep numbered twice
    /// would scroll a copy. A burst the radio flags out of range never becomes a sweep, so it
    /// takes no number either.
    #[test]
    fn each_completed_sweep_is_one_numbered_frame() {
        let feed = tempo_app::engine::SpectrumFeed::default();
        let newest = |last| {
            feed.scope_frame_after(0.0, 0.0, Default::default(), last, || {
                unreachable!("a sweep was published, so the fallback is never asked")
            })
        };
        let mut asm = ScopeAssembler::new(Some(IcomModel::Ic9700));
        assert!(asm.push(&wf_frame(1, 2, &center_header())).is_none());
        publish_sweep(
            &feed,
            asm.push(&wf_frame(2, 2, &[0, 80, 160])).expect("complete"),
        );
        let first = newest(0).expect("the sweep reached the feed");
        assert_eq!(first.seq, 1);
        assert_eq!(first.source, "civ");
        assert_eq!(first.scale, tempo_app::dto::SpectrumScale::Relative);
        assert_eq!(first.slice, Some(0), "the Main scope");
        assert_eq!(first.bins, vec![0.0, 0.5, 1.0]);
        assert_eq!((first.lo_hz, first.hi_hz), (144_975_000.0, 145_025_000.0));
        assert_eq!(
            newest(first.seq),
            None,
            "a re-read of the same sweep is not a new frame"
        );

        // Mid-retune: the radio flags the burst out of range, and nothing reaches the feed.
        let mut oor = center_header();
        *oor.last_mut().unwrap() = 0x01;
        assert!(asm.push(&wf_frame(1, 2, &oor)).is_none());
        assert!(asm.push(&wf_frame(2, 2, &[1, 2, 3])).is_none());
        assert_eq!(newest(first.seq), None, "no sweep, no frame");

        assert!(asm.push(&wf_frame(1, 2, &center_header())).is_none());
        publish_sweep(
            &feed,
            asm.push(&wf_frame(2, 2, &[40, 40, 40])).expect("complete"),
        );
        let second = newest(first.seq).expect("the next sweep is a new frame");
        assert_eq!(second.seq, first.seq + 1, "one sweep, one step");
    }

    /// ⭐ OVER THE NETWORK A SWEEP IS ONE FRAME, and the guides give its size: "When data is sent
    /// to the controller (PC) using the RF deck's [LAN] port, all data is sent together", one
    /// division of 704 bytes on the IC-7760 (A7788-8EX-2 PDF p. 25), 490 on the IC-7300MK2
    /// (rev 0, PDF p. 24): the 15 header bytes and the 689 or 475 points. The assembler takes
    /// such a burst as it is, `seq == total == 1`, and each radio's points are read on its own
    /// scale (200 on the 7760 and 7610, 160 on the rest).
    #[test]
    fn a_network_sweep_is_one_frame_read_on_each_radios_own_scale() {
        for (model, points, top, len) in [
            (IcomModel::Ic7760, 689usize, 200u8, 704usize),
            (IcomModel::Ic7610, 689, 200, 704),
            (IcomModel::Ic7300Mk2, 475, 160, 490),
            (IcomModel::Ic9700, 475, 160, 490),
            (IcomModel::Ic705, 475, 160, 490),
            (IcomModel::Ic905, 475, 160, 490),
        ] {
            let mut body = center_header();
            // A ramp from 0 to the radio's own top: its last point is full height.
            body.extend((0..points).map(|i| (i * usize::from(top) / (points - 1)) as u8));
            let f = wf_frame(1, 1, &body);
            // The division as the guide sizes it: everything after the `00` sub-command.
            assert_eq!(
                f.data.len() - 1,
                len,
                "{model:?}: the guide's division length"
            );
            let sweep = ScopeAssembler::new(Some(model))
                .push(&f)
                .unwrap_or_else(|| panic!("{model:?}: a one-frame sweep completes"));
            assert_eq!(sweep.row.len(), points, "{model:?}");
            assert_eq!(
                sweep.row.last().copied(),
                Some(1.0),
                "{model:?}: the top is full height"
            );
            let half = sweep.row[points / 2];
            assert!(
                (half - 0.5).abs() < 0.01,
                "{model:?}: half way up is 0.5, got {half}"
            );
            assert_eq!((sweep.lo_hz, sweep.hi_hz), (144_975_000.0, 145_025_000.0));
        }
    }

    /// Assemble one IC-905 sweep from a header `body` both ways the radio sends one, and check its
    /// span: over USB the header alone opens the burst and the points follow, and over the network
    /// one division carries the header and all 475 points ("Data length: 475"; A7711-9EX-2 PDF
    /// p. 29, `27 00` items 2, 3 and 7).
    fn assert_ic905_sweeps(body: &[u8], span: (f64, f64), what: &str) {
        let mut asm = ScopeAssembler::new(Some(IcomModel::Ic905));
        assert!(asm.push(&wf_frame(1, 2, body)).is_none(), "{what}");
        let usb = asm
            .push(&wf_frame(2, 2, &[0, 80, 160]))
            .unwrap_or_else(|| panic!("{what} over USB: no sweep"));
        assert_eq!((usb.lo_hz, usb.hi_hz), span, "{what} over USB");
        assert_eq!(
            usb.row,
            [0.0, 0.5, 1.0],
            "{what} over USB: a header byte read as a point"
        );
        let mut one = body.to_vec();
        one.extend((0..475).map(|i| (i % 161) as u8));
        let net = ScopeAssembler::new(Some(IcomModel::Ic905))
            .push(&wf_frame(1, 1, &one))
            .unwrap_or_else(|| panic!("{what} over the network: no sweep"));
        assert_eq!((net.lo_hz, net.hi_hz), span, "{what} over the network");
        assert_eq!(
            net.row.len(),
            475,
            "{what} over the network: the guide's data length"
        );
    }

    /// ⭐ ON THE IC-905'S 10 GHz BAND THE CENTER FREQUENCY IS 12 DIGITS. A7711-9EX-2 PDF p. 29,
    /// `27 00` item 5: "When the 10 GHz band is selected, the Center frequency is 12 digits (6
    /// bytes) from 100 GHz to 1 Hz"; the span stays the Scope span settings' 10 (`27 15` items
    /// 2–6, the same page). These are those bytes, 1 Hz first and the 100 GHz and 10 GHz digits
    /// last, the order of the Scope Fixed edge frequency settings (`27 1E`, PDF p. 30). Read as 10
    /// digits, the 10 GHz digit fell into the span: a 10368 MHz sweep drew at 368 MHz, and the
    /// out-of-range byte became a point.
    #[test]
    fn the_ic905s_10_ghz_center_frequency_is_12_digits() {
        let body = [
            0x00, // Center mode
            0x00, 0x00, 0x20, 0x68, 0x03, 0x01, // center 10368.200000 MHz
            0x00, 0x00, 0x05, 0x00, 0x00, // span ± 50 kHz
            0x00, // in range
        ];
        assert_ic905_sweeps(&body, (10_368_150_000.0, 10_368_250_000.0), "center");
        // Control: below 10 GHz the IC-905 sends 10 digits, and its 2 m header reads as before.
        let mut asm = ScopeAssembler::new(Some(IcomModel::Ic905));
        assert!(asm.push(&wf_frame(1, 2, &center_header())).is_none());
        let sweep = asm.push(&wf_frame(2, 2, &[0, 80, 160])).expect("2 m");
        assert_eq!((sweep.lo_hz, sweep.hi_hz), (144_975_000.0, 145_025_000.0));
        assert_eq!(sweep.row, [0.0, 0.5, 1.0]);
    }

    /// ⭐ …AND IN THE FIXED AND SCROLL-F MODES EACH EDGE IS. A7711-9EX-2 PDF p. 29, `27 00` item
    /// 5: "When the Higher Edge or Lower Edge frequency is in the 10 GHz band, the each Edge
    /// frequency is 12 digits (6 bytes) from 100 GHz to 1 Hz", laid out as the Scope Fixed edge
    /// frequency settings' range 06 (`27 1E`, PDF p. 30). Read as 10 digits, the out-of-range byte
    /// came from inside the upper edge, so this sweep was dropped as out of range.
    #[test]
    fn the_ic905s_10_ghz_edges_are_12_digits_each() {
        for mode in [0x01u8, 0x03] {
            let body = [
                mode, // Fixed, or Scroll-F
                0x00, 0x00, 0x00, 0x68, 0x03, 0x01, // lower edge 10368.000000 MHz
                0x00, 0x00, 0x50, 0x68, 0x03, 0x01, // upper edge 10368.500000 MHz
                0x00, // in range
            ];
            let what = format!("mode {mode:02X}");
            assert_ic905_sweeps(&body, (10_368_000_000.0, 10_368_500_000.0), &what);
        }
    }

    /// The IC-7760 has two scopes, like the IC-7610: it selects one with `27 12` (A7788-8EX-2
    /// PDF p. 17) and names Main or Sub in its scope-control formats (pp. 25–26). The
    /// IC-7300MK2 has one: its `27 12` is "Main only" (rev 0, PDF p. 15).
    #[test]
    fn the_7760_pins_its_main_scope_and_the_7300mk2_has_one() {
        let on = scope_stream_frames(0xB2, true);
        assert_eq!(on.len(), 3);
        assert_eq!(on[0].data, vec![0x12, 0x00]);
        assert!(scope_is_dual(0xB2));
        let mk2 = scope_stream_frames(0xB6, true);
        assert_eq!(mk2.len(), 2);
        assert!(!scope_is_dual(0xB6));
    }
}
