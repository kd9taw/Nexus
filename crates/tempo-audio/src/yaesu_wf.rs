//! FT-710 waterfall over the radio's internal FT4222 USB→SPI bridge.
//!
//! WHAT THIS IS. The FT-710 contains an FT4222 USB→SPI bridge. With *SCU-LAN10* enabled in the
//! radio's menu it appears as a THIRD USB function beside the CAT bridge and the codec, on the
//! same internal hub:
//!
//! ```text
//! IOUSBHostDevice@02400000                 ← the FT-710's internal hub
//!   ├─ CP2105 Dual USB to UART @02410000   ← CAT
//!   ├─ USB Audio Device        @02420000   ← the codec
//!   └─ FT4222                  @02430000   ← this  (FTDI, VID 0x0403, PID 0x601C)
//! ```
//!
//! It is neither a serial port nor a CAT command, and both of those were ruled out by measurement
//! before this module existed: `SS` (SPECTRUM SCOPE) reads and writes scope SETTINGS only — the
//! official FT-710 CAT manual has no waveform command at all — and nothing arrives unsolicited on
//! either virtual COM port. Measured on ON8ST's station 2026-08-17: 40 frames of 4096 B in 0.48 s
//! (84/s), 850 live bins. Polarity, width, bin-to-frequency alignment and orientation are all
//! verified against a known broadcast carrier — the row runs LOW to HIGH left to right (checked by
//! moving the dial 15 kHz and confirming the signal stayed put). See FORK.md for the measurements
//! and for the one residual: about 1.1 kHz of error at 15 kHz off centre, which is quantified but
//! not yet explained.
//!
//! WHY THE SPLIT IN THIS FILE. Everything that decides what the bins MEAN is pure and tested here
//! ([`parse_wf1`], [`span_hz`], [`sweep_edges`]). The only part that needs FTDI's closed-source
//! library sits behind the off-by-default `yaesu-wf` feature and behind [`WaterfallSource`], so
//! the parsing is exercised by the suite on every platform and the FFI is the one thing a
//! licensing decision gates. **No FTDI binary is vendored into this repo** — see FORK.md: D2XX /
//! LibFT4222 is closed source and Nexus is GPL-3.0-only, which is an open question, not a settled
//! one. Fork-local until it is answered.
//!
//! HOW IT MEETS THE REST OF THE APP. The output shape is deliberately the same as the CI-V
//! scope's [`crate::civ::scope::ScopeSweep`]: bins normalised 0..1 plus the absolute RF span, so
//! the existing `rigscope` pane and `SpectrumFeed` take it with no new UI or DSP. Which RADIO a
//! bridge belongs to is answered by [`crate::usbtopo`] — the FT4222 shares its parent hub with
//! that rig's CAT port and codec, the same evidence that already labels the codec.

/// One SPI read from the bridge. Fixed size; the radio does not frame or delimit.
pub const FRAME_BYTES: usize = 4096;

/// The 16 bytes that END every frame: `FF 01 EE 01`, four times.
///
/// MEASURED, not documented anywhere: identical in all 200 frames of a bench capture (station
/// FT-710, 2026-08-20, CENTER, 200 kHz), and the 64-byte slice ending the frame occurs exactly ONCE
/// per frame — so it is a usable delimiter rather than a pattern that happens to appear.
///
/// It exists here because the SPI side has NO framing of its own. `SPIMaster_SingleRead` hands over
/// a byte pipe; nothing says where a frame begins, and our 4096-byte read window drifts against the
/// radio's frame boundary on its own (measured: aligned for 48 frames, then misaligned for 51, with
/// no pause and no operator action in between). Reading bins out of an unaligned window puts every
/// bin at the wrong frequency, which is the "garbled spectrum" an operator sees.
pub const FRAME_TRAILER: [u8; 16] = [
    0xff, 0x01, 0xee, 0x01, 0xff, 0x01, 0xee, 0x01, 0xff, 0x01, 0xee, 0x01, 0xff, 0x01, 0xee, 0x01,
];

/// Find the end of the LAST complete frame in `window`, or `None`.
///
/// "Complete" means the trailer has a whole frame's worth of bytes in front of it. Searching for the
/// LAST one keeps latency down when a read has caught up on more than one frame.
pub fn frame_end(window: &[u8]) -> Option<usize> {
    let mut found = None;
    let mut from = 0;
    while let Some(rel) = window[from..]
        .windows(FRAME_TRAILER.len())
        .position(|w| w == FRAME_TRAILER)
    {
        let end = from + rel + FRAME_TRAILER.len();
        if end >= FRAME_BYTES {
            found = Some(end);
        }
        from += rel + 1;
        if from + FRAME_TRAILER.len() > window.len() {
            break;
        }
    }
    found
}
/// Receiver 1's waterfall line: `uint8` per bin, at the start of the frame.
pub const WF1_OFFSET: usize = 0;
/// Usable bins in receiver 1's line — the width of one waterfall line.
///
/// ⚠️ 850, NOT 852, and the two are not interchangeable. The layout reserves 852 bytes (see
/// [`WF2_OFFSET`], which follows at 852), but the LAST TWO are structurally zero: measured across
/// 30 consecutive frames on 2026-08-18, bins 850 and 851 were 0 in every one while 844-849 varied
/// normally.
///
/// Including them is not a rounding error, it is a FALSE SIGNAL: raw 0 inverts to full scale (see
/// [`parse_wf1`]), so both would appear as a permanent pair of strong spikes at the top edge of
/// every span — a carrier that is always there, moves when the operator retunes, and does not
/// exist. The published `ratmandu/YaesuWFTesting` layout says 852 and does not mention this.
pub const WF1_BINS: usize = 850;
/// Bytes the layout reserves for receiver 1's line, including the two trailing zero bytes that
/// [`WF1_BINS`] excludes — this is the stride to the next segment, not a usable width.
pub const WF1_STRIDE: usize = 852;
/// Receiver 2's line. Present in the layout for the FTDX101 series; unused on the FT-710.
pub const WF2_OFFSET: usize = 852;
/// AF-FFT for receiver 1 (`uint8`), after both waterfall lines.
pub const AF1_FFT_OFFSET: usize = 1704;
/// AF-FFT bin count.
pub const AF1_FFT_BINS: usize = 192;
/// AF oscilloscope for receiver 1 (`uint8`, 128 = zero line).
pub const AF1_SCOPE_OFFSET: usize = 1896;
/// AF oscilloscope sample count.
pub const AF1_SCOPE_SAMPLES: usize = 400;

/// Receiver 1's waterfall line, normalised to 0..1 the way [`ScopeSweep::row`] is — **inverted**,
/// because the radio sends it upside down.
///
/// ⚠️ LOW BYTES ARE STRONG SIGNALS. Measured against a known strong broadcast carrier on
/// 9.410 MHz, dial centred, 20 frames averaged (2026-08-18): the centre bin read **109** while the
/// noise floor across the rest of the row sat at **185-190**. The signal is a TROUGH in the raw
/// bytes, not a peak. An earlier capture on a quiet band agrees from the other side — mean 184
/// with nothing present, i.e. "quiet" is high.
///
/// Publishing the bytes as-is would have drawn every band upside down: signals as holes in a
/// bright ceiling. Nothing would have errored, the waterfall would simply have been inverted, and
/// the AGC would have stretched it into something that looks like a plausible display. This is the
/// one thing in this module that no amount of reading the protocol notes would have caught —
/// neither `YaesuWFTesting` nor the wfview-derived layout says which way up the bytes run.
///
/// `None` for a short frame rather than a padded one: a truncated SPI read is a transport fault,
/// and half a line rendered as if it were a full one is worse than a dropped frame — the operator
/// would be reading a spectrum whose right-hand side is silence that is not on the air.
///
/// [`ScopeSweep::row`]: crate::civ::scope::ScopeSweep::row
pub fn parse_wf1(raw: &[u8]) -> Option<Vec<f32>> {
    if raw.len() < WF1_OFFSET + WF1_BINS {
        return None;
    }
    Some(
        raw[WF1_OFFSET..WF1_OFFSET + WF1_BINS]
            .iter()
            .map(|&b| f32::from(255 - b) / 255.0)
            .collect(),
    )
}

/// The scope span in Hz for the `P3` code of `SS<P1>5;` (SPECTRUM SCOPE ▸ SPAN).
///
/// Straight from Yaesu's FT-710 CAT manual, and read off the radio rather than assumed — the app
/// asks `SS05;` and gets e.g. `SS0570000;` = code 7 = 200 kHz. `None` for a code the manual does
/// not define: guessing a span silently mis-scales the whole waterfall, which looks like a
/// mistuned radio rather than a software fault.
pub fn span_hz(code: u8) -> Option<f64> {
    Some(match code {
        b'0' => 1_000.0,
        b'1' => 2_000.0,
        b'2' => 5_000.0,
        b'3' => 10_000.0,
        b'4' => 20_000.0,
        b'5' => 50_000.0,
        b'6' => 100_000.0,
        b'7' => 200_000.0,
        b'8' => 500_000.0,
        b'9' => 1_000_000.0,
        _ => return None,
    })
}

/// The `SS` SPAN code for a span in Hz, or `None` when the rig has no such step.
///
/// The inverse of [`span_hz`], and deliberately EXACT: the radio's ladder is a fixed set of ten
/// steps (FT-710 CAT reference, `SS` P2=5), and asking for 15 kHz is not a request the rig can
/// honour. Rounding to a neighbour would leave the app showing one width while the radio swept
/// another — the same class of quiet mismatch this module exists to refuse.
pub fn span_code_for_hz(hz: u32) -> Option<u8> {
    (b'0'..=b'9').find(|&c| span_hz(c) == Some(hz as f64))
}

/// The raw CAT string that SETS the scope span. `SS<P1=0><P2=5><P3=code><P4..P7=0>;`
///
/// The rig does not answer a set (measured), so this goes out through `Rig::send_raw_set`.
pub fn set_span_command(code: u8) -> String {
    format!("SS05{}0000;", code as char)
}

/// A MENU item, addressed the way `EX` addresses one: three two-digit numbers.
///
/// The FT-710's menu is reachable over CAT — `EX<P1><P2><P3>;` reads, appending a value writes —
/// and that matters more than it looks. The two settings this module used to tell operators to go
/// and change by hand are BOTH in there, so Nexus can check them instead of guessing, and offer to
/// change them instead of instructing. Verified on the bench, 2026-08-20.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExItem {
    pub p1: u8,
    pub p2: u8,
    pub p3: u8,
}

/// `0: OFF  1: ON`. What makes the FT4222 spectrum bridge appear on USB at all.
pub const EX_SCU_LAN10: ExItem = ExItem { p1: 3, p2: 1, p3: 26 };
/// `0: OFF  1: ON`. NOT required for the bridge: measured OFF on a station whose waterfall was
/// working, which is why the "turn the external display on" advice was withdrawn.
pub const EX_EXT_DISPLAY: ExItem = ExItem { p1: 4, p2: 4, p3: 1 };
/// `0: FILTER  1: CARRIER POINT`. Which point the sweep is centred on, and therefore whether this
/// module's centred placement is right: on FILTER the centre sits at the filter's centre, offset
/// from the carrier by roughly half the passband — about 1.5 kHz on SSB, invisible across a 200 kHz
/// span and gross across a 5 kHz one.
pub const EX_SCOPE_CTR: ExItem = ExItem { p1: 4, p2: 2, p3: 2 };

/// The CAT string that READS a menu item: `EX030126;` for SCU-LAN10.
pub fn ex_read_command(item: ExItem) -> String {
    format!("EX{:02}{:02}{:02};", item.p1, item.p2, item.p3)
}

/// The CAT string that SETS one. The value is the menu's own `P4` text (`"1"`, `"00"`, `"-25"` …),
/// passed through rather than interpreted: the menu chart gives a different width per item and
/// guessing one here would be a silent way to write the wrong thing.
pub fn ex_set_command(item: ExItem, value: &str) -> String {
    format!("EX{:02}{:02}{:02}{value};", item.p1, item.p2, item.p3)
}

/// The value out of an `EX` answer, checked against the item that was asked for.
///
/// The address is re-checked for the same reason `parse_ss_reply` checks `P2`: on a link that can
/// interleave, taking the tail of "whatever came back" would read one menu item's value as
/// another's — and these are all small integers, so the wrong answer would look perfectly valid.
pub fn parse_ex_reply(reply: &str, item: ExItem) -> Option<String> {
    let body = reply.trim().trim_end_matches('\0').strip_prefix("EX")?;
    let body = body.strip_suffix(';')?;
    let (addr, value) = body.split_at_checked(6)?;
    (addr == format!("{:02}{:02}{:02}", item.p1, item.p2, item.p3) && !value.is_empty())
        .then(|| value.to_string())
}

/// The band a dial sits in, as `(lo_hz, hi_hz)` — the edges a FIX window has to cover.
///
/// IARU REGION 1 edges, because that is where this fork's radio is. Region 2/3 are wider on 40 m and
/// 80 m, so an operator there would see the top of those bands cut off — the table is the one thing
/// to change, and it is deliberately a table rather than arithmetic for exactly that reason.
///
/// Needed because the FT-710 reports its FIX window nowhere (unlike Icom, whose scope puts its fixed
/// edges in the frame header). If the window has to be derived rather than read, the band is the only
/// honest thing to derive it from: it is what the operator means by "the whole band".
pub fn band_edges_hz(dial_hz: f64) -> Option<(f64, f64)> {
    const BANDS: [(f64, f64); 11] = [
        (1_810_000.0, 2_000_000.0),   // 160 m
        (3_500_000.0, 3_800_000.0),   // 80 m
        (5_351_500.0, 5_366_500.0),   // 60 m
        (7_000_000.0, 7_200_000.0),   // 40 m
        (10_100_000.0, 10_150_000.0), // 30 m
        (14_000_000.0, 14_350_000.0), // 20 m
        (18_068_000.0, 18_168_000.0), // 17 m
        (21_000_000.0, 21_450_000.0), // 15 m
        (24_890_000.0, 24_990_000.0), // 12 m
        (28_000_000.0, 29_700_000.0), // 10 m
        (50_000_000.0, 52_000_000.0), // 6 m
    ];
    BANDS
        .iter()
        .find(|(lo, hi)| dial_hz >= *lo && dial_hz <= *hi)
        .copied()
}

/// The span rung Nexus ASKS the radio for in FIX: the narrowest that covers the tuned band.
///
/// Only a request. The window is drawn with whatever span the radio reports — see
/// `auto_fix_start`, and the difference between those two is not academic: pairing an ideal span
/// with a real one draws a window of the wrong WIDTH, which is a misplaced spectrum that looks
/// entirely plausible.
pub fn auto_fix_span_code(dial_hz: f64) -> Option<u8> {
    let (lo, hi) = band_edges_hz(dial_hz)?;
    let need = hi - lo;
    // Codes ascend in span, so the first that fits is the narrowest — 850 bins across as little
    // spectrum as the band allows.
    (b'0'..=b'9').find(|&c| span_hz(c).is_some_and(|s| s >= need))
}

/// Where a FIX window starts, for the span ACTUALLY in force: the tuned band's LOWER EDGE.
///
/// (This line used to say "the band's centre, minus half that span". That was the rule this
/// function was written from and it was wrong on the air — see below. Left named here because a
/// stale summary above a corrected body is how the wrong rule gets restored by the next reader.)
///
/// MEASURED ON THE RADIO, no longer assumed. The FIX start cannot be read over CAT — it is settable
/// only by a long press on the front panel, checked against all 56 CAT commands and the whole `EX`
/// menu — so Nexus has to derive it. It used to derive the window it would WANT: the band centred,
/// `centre - span/2`. The FT-710 does not do that. It starts the window at the BAND EDGE, read off
/// the rig's own scale on two bands whose predictions differ (ON8ST, 2026-08-20):
///
///   20 m, 500 kHz span:  centre-based 13.925-14.425   radio 14.000-14.500   (75 kHz out)
///   80 m, 500 kHz span:  centre-based  3.400- 3.900   radio  3.500- 4.000   (100 kHz out)
///
/// Both bands were chosen because the two rules disagree there. 40 m, 30 m, 17 m and 12 m cannot
/// tell them apart at all — their width equals the covering rung, so `lo == centre - span/2` — and
/// a test on one of those would have "confirmed" whichever rule was already written.
///
/// The consequence of the old rule was not a blank panadapter but a WRONG one: every signal drawn
/// 75-100 kHz from where it actually is, with nothing in the protocol able to reveal it.
///
/// Taking the span as an ARGUMENT is what keeps it consistent: the bins always arrive at the rig's
/// span, so the start has to be computed from the same number, whatever `auto_fix_span_code` may
/// have asked for and whether or not the radio has honoured it yet.
pub fn auto_fix_start(dial_hz: f64, span_code: u8) -> Option<f64> {
    let (lo, _hi) = band_edges_hz(dial_hz)?;
    // The span code still has to be a real rung: an unknown one means we do not know the width,
    // and a start without a width places nothing.
    span_hz(span_code)?;
    Some(lo)
}

/// Where the sweep sits relative to the dial. The operator-facing choice, three ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopePosition {
    Center,
    Cursor,
    Fix,
}

/// The `SS` P3 MODE code for a position, KEEPING the display family the rig is already in.
///
/// The FT-710 has three families — 3DSS (`0 1 2`), W/F EXPAND (`3 6 9`) and W/F NORMAL (`4 7 A`) —
/// and each carries all three positions. Mapping every request onto W/F NORMAL would quietly drag a
/// 3DSS operator out of 3DSS for asking to centre the sweep, so the family comes from `current`.
/// An unrecognised current code falls back to W/F NORMAL, which is the family this app can place.
pub fn mode_code_for(pos: ScopePosition, current: u8) -> u8 {
    let family: [u8; 3] = match current {
        b'0' | b'1' | b'2' => [b'0', b'1', b'2'],
        b'3' | b'6' | b'9' => [b'3', b'6', b'9'],
        _ => [b'4', b'7', b'A'],
    };
    match pos {
        ScopePosition::Center => family[0],
        ScopePosition::Cursor => family[1],
        ScopePosition::Fix => family[2],
    }
}

/// Which position a MODE code names, for showing the operator where the sweep currently sits.
pub fn position_of(code: u8) -> Option<ScopePosition> {
    Some(match code {
        b'0' | b'3' | b'4' => ScopePosition::Center,
        b'1' | b'6' | b'7' => ScopePosition::Cursor,
        b'2' | b'9' | b'A' => ScopePosition::Fix,
        _ => return None,
    })
}

/// The raw CAT string that SETS the scope mode — same shape, `P2=6`. `4` is W/F CENTER (NORMAL),
/// the only family whose sweep edges this module can place on the band.
pub fn set_mode_command(code: u8) -> String {
    format!("SS06{}0000;", code as char)
}

/// Is the scope's `P3` MODE code one of the CENTER modes — i.e. is the span centred on the dial?
///
/// `SS<P1>6;` reports it. CENTER (`3`, `4` for W/F; `0` for 3DSS) means the row is symmetric about
/// the dial. CURSOR and FIX are NOT, and this module refuses to guess their edges: in FIX the
/// window is pinned to a band edge the CAT protocol does not report, so a centred assumption would
/// place every signal at the wrong frequency — a wrong answer that looks authoritative.
pub fn mode_is_centered(code: u8) -> bool {
    matches!(code, b'0' | b'3' | b'4')
}

/// Absolute row edges `(lo_hz, hi_hz)` for a dial and span, or `None` when they cannot be known.
///
/// `None` for a non-CENTER mode, for an undefined span code, and for a dial that would put the row
/// below 0 Hz. Every one of those is "we do not know where this row sits", and the honest response
/// is to render no row rather than a mislabelled one.
pub fn sweep_edges(dial_hz: f64, span_code: u8, mode_code: u8) -> Option<(f64, f64)> {
    sweep_edges_anchored(dial_hz, span_code, mode_code, None, None)
}

/// Absolute row edges, with an optional ANCHOR for a sweep that is not centred on the dial.
///
/// `anchor_hz` is where the window sits — see `SweepMeta::center_hz`. With one, a CURSOR sweep is
/// placeable: the window is static and the dial moves across it. Two things are still refused, and
/// both would otherwise draw an authoritative wrong answer:
///
/// * FIX, always. Its window jumps to a per-band preset when entered ("switching in and out of fix,
///   they do [move]"), and no `SS` sub-command reports it — so an anchor observed at a CENTER→FIX
///   transition would be the wrong window from the first frame.
/// * A CURSOR sweep whose dial has left the window. The radio does something at that edge and we
///   cannot see what; the honest answer is that we no longer know where the window is.
pub fn sweep_edges_anchored(
    dial_hz: f64,
    span_code: u8,
    mode_code: u8,
    anchor_hz: Option<f64>,
    fix_start_hz: Option<f64>,
) -> Option<(f64, f64)> {
    let span = span_hz(span_code)?;
    // FIX first, because it is the one position whose window is given by an EDGE. The rig's own
    // scale reads `start` → `start + span`, so once the start is known the span finishes it — no
    // centring, and no dial in the arithmetic at all: in FIX the operator may tune right out of the
    // window and it stays where it is.
    if matches!(position_of(mode_code), Some(ScopePosition::Fix)) {
        let start = fix_start_hz?;
        return (start >= 0.0).then_some((start, start + span));
    }
    let centered = mode_is_centered(mode_code);
    let cursor = matches!(position_of(mode_code), Some(ScopePosition::Cursor));
    let center = match (centered, cursor, anchor_hz) {
        (true, _, _) => dial_hz,
        (false, true, Some(a)) => a,
        _ => return None, // CURSOR with no anchor established, or a mode we do not know
    };
    if !centered && (dial_hz - center).abs() > span / 2.0 {
        return None; // the dial has left the window — where it went next is not ours to guess
    }
    let half = span / 2.0;
    // A window that would run below 0 Hz is not a window. Checked on the CENTRE, not the dial:
    // in CURSOR they are different numbers and the dial is the one that moves.
    (center - half >= 0.0).then_some((center - half, center + half))
}

/// A source of raw 4096-byte frames.
///
/// A trait so the parsing above is testable without hardware AND so the FTDI dependency has one
/// place to live. `Err` is a transport fault; the caller drops the frame and keeps the previous
/// picture rather than blanking the pane on a single hiccup.
pub trait WaterfallSource {
    fn read_frame(&mut self) -> std::io::Result<Vec<u8>>;
}

/// A canned source for tests: cycles the frames it was given.
#[derive(Debug)]
pub struct MockWaterfall {
    frames: Vec<Vec<u8>>,
    next: usize,
}

impl MockWaterfall {
    pub fn new(frames: Vec<Vec<u8>>) -> Self {
        Self { frames, next: 0 }
    }
    /// A frame whose receiver-1 line ramps 0..255 across the bins — a shape whose normalisation
    /// is checkable at both ends and in the middle.
    pub fn ramp() -> Self {
        let mut f = vec![0u8; FRAME_BYTES];
        for (i, b) in f[WF1_OFFSET..WF1_OFFSET + WF1_BINS].iter_mut().enumerate() {
            *b = ((i * 255) / (WF1_BINS - 1)) as u8;
        }
        Self::new(vec![f])
    }
}

impl WaterfallSource for MockWaterfall {
    fn read_frame(&mut self) -> std::io::Result<Vec<u8>> {
        if self.frames.is_empty() {
            return Err(std::io::Error::other("no frames"));
        }
        let f = self.frames[self.next % self.frames.len()].clone();
        self.next += 1;
        Ok(f)
    }
}

/// What one pump attempt did — three outcomes, because two of them are NOT the same failure.
///
/// A dropped frame is transient (a short SPI read, a hiccup) and the caller must KEEP the last
/// picture: blanking the pane on one bad read is a flicker the operator reads as a dying radio.
/// `Unavailable` is persistent — the scope is in a mode whose edges CAT does not report — and
/// there the stale row must be CLEARED, or the pane keeps showing a band that is no longer what
/// the rig is looking at. The CI-V path draws the same line with `clear_rf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pumped {
    /// A row reached the feed.
    Published,
    /// Transient read fault — keep whatever is on screen.
    Dropped,
    /// The row cannot be placed on the band at all — the caller should clear the RF feed.
    Unavailable,
}

/// What the radio has to tell us over CAT before a row can be placed on the band.
///
/// The bins arrive over SPI with no metadata at all — no centre, no span, no mode. Those three
/// come from `SS<P1>5;` / `SS<P1>6;` and the dial, which is why this transport is useless on its
/// own and pairs with the CAT link rather than replacing it.
#[derive(Debug, Clone, Copy)]
pub struct SweepMeta {
    pub dial_hz: f64,
    /// Where the sweep is CENTRED, when that is not the dial — the CURSOR case.
    ///
    /// `None` means "centred on the dial", which is what a CENTER mode does. `Some(hz)` is an
    /// anchor the owner ESTABLISHED rather than read: switching CENTER → CURSOR leaves the window
    /// exactly where it was (operator, bench, 2026-08-20 — "going to cursor from center, the band
    /// edges don't move"), so the anchor is the dial at the instant of that transition. The CAT
    /// protocol reports no such value; this is knowable only because the transition is observed, or
    /// caused, by us.
    pub center_hz: Option<f64>,
    /// Where a FIX sweep STARTS — its left edge, not its centre.
    ///
    /// Deliberately a second field rather than reuse of `center_hz`: a centre and a left edge are
    /// different quantities, and conflating them puts every signal half a span out — which on the
    /// FT-710's own scale (`start` → `start + span`, operator 2026-08-20) is exactly the mistake
    /// available to anyone who assumes "anchor" means the middle.
    ///
    /// The radio reports this nowhere: the start is set by a LONG PRESS on FIX, a front-panel-only
    /// action, and the whole `EX` menu was searched for it without result. So it is stated by the
    /// operator and held against the band it was stated on.
    pub fix_start_hz: Option<f64>,
    /// `P3` of `SS<P1>5;` — the SPAN code, as the ASCII byte the radio sent.
    pub span_code: u8,
    /// `P3` of `SS<P1>6;` — the MODE code, as the ASCII byte the radio sent.
    pub mode_code: u8,
}

/// The feed label for rows from this bridge.
///
/// ⚠️ MUST match `isRfScopeSource` in `ui/src/waterfall.ts`. That predicate decides whether a row
/// spans ABSOLUTE RF Hz or demodulated audio Hz, and an unknown label falls through to the audio
/// reading — so a row would be drawn against a 0–4000 Hz axis with dB-scaled thresholds. Nothing
/// errors; the waterfall is simply wrong, which is the failure this constant exists to prevent.
pub const SOURCE: &str = "yaesu";

/// Read one frame and publish it as an RF row.
///
/// Takes the feed rather than the engine: the CI-V scope learned this the hard way — going through
/// the engine mutex starved the panadapter on the same lock that starved the audio row.
pub fn pump(
    src: &mut dyn WaterfallSource,
    feed: &tempo_app::engine::SpectrumFeed,
    meta: SweepMeta,
) -> Pumped {
    let Some((lo_hz, hi_hz)) =
        sweep_edges_anchored(
            meta.dial_hz,
            meta.span_code,
            meta.mode_code,
            meta.center_hz,
            meta.fix_start_hz,
        )
    else {
        return Pumped::Unavailable;
    };
    let Ok(raw) = src.read_frame() else {
        return Pumped::Dropped;
    };
    let Some(row) = parse_wf1(&raw) else {
        return Pumped::Dropped;
    };
    feed.publish_rf(tempo_app::dto::Spectrum {
        row,
        lo_hz,
        hi_hz,
        source: SOURCE.to_string(),
    });
    Pumped::Published
}

/// The `P3` byte out of an `SS` reply, checked against the sub-command that was asked for.
///
/// The radio answers `SS<P1><P2><P3><P4..P7>;` — e.g. `SS0570000;` for `SS05;` (span code 7 =
/// 200 kHz). Verified on an FT-710 2026-08-17.
///
/// The `P2` check is the point of this function existing rather than an index into the string: the
/// span and mode reads are two commands with identically-shaped replies, and on a link that can
/// interleave, taking byte 4 of "whatever came back" would silently read the span code as a mode.
/// A mode of `7` is CURSOR — which the caller then refuses to place — so the failure would look
/// like a scope that mysteriously stopped working rather than a crossed reply.
pub fn parse_ss_reply(reply: &str, expect_p2: u8) -> Option<u8> {
    let b = reply.trim().as_bytes();
    // "SS" + P1 + P2 + P3 … ';' — at least 6 bytes before the P3 can exist.
    if b.len() < 6 || &b[0..2] != b"SS" || b[3] != expect_p2 {
        return None;
    }
    Some(b[4])
}

/// The metadata the reader thread needs, shared with whoever owns the CAT link.
///
/// `None` = not established yet (or no longer trustworthy). The reader treats that as
/// [`Pumped::Unavailable`] and clears the RF feed rather than publishing rows it cannot place.
pub type SharedMeta = std::sync::Arc<std::sync::Mutex<Option<SweepMeta>>>;

/// A running reader: a thread that pumps frames into the feed until dropped.
///
/// WHY A THREAD AND NOT THE RADIO POLL LOOP. The loop's heavy tick is `RIG_POLL_MS` = 750 ms, and
/// one frame per tick is ~1.3 rows/s — a waterfall that scrolls once a second is not a waterfall.
/// A read costs 12 ms measured, so a dedicated reader at ~15/s is cheap and never blocks CAT: the
/// bridge is a SEPARATE USB function from the CAT port, which is the whole reason this is
/// possible at all.
///
/// Lifecycle mirrors [`crate::flexspectrum::FlexSpectrum`] deliberately — that is the existing
/// answer in this codebase to "a second device that feeds the spectrum": the owner holds it in an
/// `Option` beside a KEY, and a rig switch drops it (stopping the thread) before starting the one
/// Does this Hamlib model have the internal FT4222 bridge?
///
/// Only the FT-710 (1049) is confirmed — measured on hardware 2026-08-17. The FTX-1 (1051) is NOT
/// included: it has not been checked, and claiming a bridge that is not there would put a
/// "enable SCU-LAN10" instruction in front of an operator whose radio has no such menu.
pub fn model_has_ft4222(model: u32) -> bool {
    model == 1049
}

/// Whether this build actually has the FT4222 transport compiled in.
///
/// Exists so the caller can tell the TWO reasons for a silent scope apart. Without it both look
/// identical from `open_default_source`, and the operator gets told to check a menu setting on a
/// build that could not have used the bridge whatever the menu said — which is the worst kind of
/// wrong answer, because it is actionable and it does not work.
pub const TRANSPORT_COMPILED: bool = cfg!(feature = "yaesu-wf");

/// What to tell the operator when the transport is not in this build.
///
/// NAMES THE LIBRARY AND WHERE TO GET IT. The alternative — the feature quietly not appearing —
/// leaves someone with the right radio and the right cable no way to discover that one download
/// stands between them and a working panadapter.
///
/// It is not bundled, and that is a licence decision rather than an oversight: LibFT4222 and the
/// D2XX driver under it are FTDI's closed-source binaries. Shipping them inside the .app would
/// make the distributed whole a combined work with a GPL-incompatible part, and Nexus is
/// GPL-3.0-only. macOS ships the DriverKit *serial* driver (`/dev/cu.usbserial-*`), which is a
/// different thing entirely and does not provide the SPI interface this needs.
pub const YAESU_WF_NO_LIBRARY: &str = "The FT-710 scope needs FTDI's LibFT4222, which macOS does \
     not ship and which Nexus cannot redistribute (it is closed source; Nexus is GPL-3.0-only). \
     Install it from https://ftdichip.com/software-examples/ft4222h-software-examples/ and run a \
     build made with `--features yaesu-wf`. Everything else works without it — this affects the \
     RF panadapter only.";

/// Open the bridge, or `None` when it is not available for any reason.
///
/// `None` is the ORDINARY answer, not an error path: without the `yaesu-wf` feature there is no
/// transport compiled in at all, and with it the device is absent until SCU-LAN10 is enabled. The
/// caller turns `None` into the operator-facing instruction.
pub fn open_default_source() -> Option<Box<dyn WaterfallSource + Send>> {
    #[cfg(feature = "yaesu-wf")]
    {
        match ft4222::Ft4222Waterfall::open(0) {
            Ok(src) => Some(Box::new(src) as Box<dyn WaterfallSource + Send>),
            Err(e) => {
                crate::civ::diag::note(&format!("yaesu waterfall: FT4222 open failed: {e}"));
                None
            }
        }
    }
    #[cfg(not(feature = "yaesu-wf"))]
    {
        None
    }
}

/// the new radio needs. Getting that wrong is how a scope keeps streaming the previous radio's
/// band, which is the dual-radio fault this project has already been bitten by elsewhere.
pub struct YaesuWaterfall {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
    /// Rows that actually reached the feed. The owner watches this to tell two failures apart that
    /// look identical from outside — the bridge never opened, versus it opened and sends nothing.
    /// On an FT-710 those have DIFFERENT cures in the radio's own EX menu, so collapsing them into
    /// "no spectrum" would send the operator to the wrong setting.
    published: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl YaesuWaterfall {
    /// Start reading. `interval` paces the reads; the thread exits on drop.
    pub fn start(
        mut src: Box<dyn WaterfallSource + Send>,
        feed: tempo_app::engine::SpectrumFeed,
        meta: SharedMeta,
        interval: std::time::Duration,
    ) -> Self {
        use std::sync::atomic::Ordering;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_thread = stop.clone();
        let published = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let published_thread = published.clone();
        let join = std::thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                // Copied out under the lock, never held across the SPI read: the CAT side must
                // never wait on a USB transaction to publish a new span.
                let m = meta.lock().ok().and_then(|g| *g);
                match m {
                    Some(m) => match pump(src.as_mut(), &feed, m) {
                        Pumped::Published => {
                            published_thread.fetch_add(1, Ordering::Relaxed);
                        }
                        Pumped::Dropped => {}
                        Pumped::Unavailable => feed.clear_rf(),
                    },
                    // Metadata not established: no row can be placed, so nothing stale may stay.
                    None => feed.clear_rf(),
                }
                std::thread::sleep(interval);
            }
        });
        Self {
            stop,
            join: Some(join),
            published,
        }
    }

    /// How many rows have reached the feed since this reader started. Monotonic; 0 means the
    /// transport opened but the radio has sent nothing usable yet.
    pub fn published(&self) -> u64 {
        self.published.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Drop for YaesuWaterfall {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
        // NOT clearing the feed here: the owner decides what replaces this source. A rig switch
        // starts another reader immediately, and blanking in between is a flicker.
    }
}

/// The FT4222 transport — the ONE part that needs FTDI's closed-source library.
///
/// ⚠️ **FORK-LOCAL, AND OFF BY DEFAULT.** `LibFT4222`/`D2XX` are closed-source binaries and Nexus
/// is GPL-3.0-only; that question is open (FORK.md), so nothing is vendored here and the feature
/// is not enabled in any build. Everything above this module is licence-clean and tested.
///
/// The SPI configuration is not written from memory: the constants below were READ OUT of the
/// `ft4222` Python wrapper that was proven against the radio on 2026-08-17, because a wrong CPOL
/// or clock divider yields plausible-looking garbage rather than an error — the worst failure mode
/// available here.
#[cfg(feature = "yaesu-wf")]
pub mod ft4222 {
    use super::{WaterfallSource, FRAME_BYTES};
    use std::os::raw::{c_int, c_void};

    // Verified against the working wrapper, not the header from memory.
    const SPI_IO_SINGLE: u8 = 1;
    const CLK_DIV_16: u8 = 4;
    const CPOL_IDLE_HIGH: u8 = 1;
    const CPHA_CLK_TRAILING: u8 = 1;
    const SS0: u8 = 1;
    const SYS_CLK_48: u8 = 2;

    type Handle = *mut c_void;

    // TWO LIBRARIES, not one, and the split is not obvious from the API's naming: opening and
    // closing a device are D2XX calls (`FT_*`), while everything SPI is LibFT4222 (`FT4222_*`).
    // Linking only `ft4222` compiles and then fails at link time on `_FT_Open`/`_FT_Close` alone —
    // which reads like a missing library rather than a missing SECOND one.
    #[link(name = "ftd2xx")]
    unsafe extern "C" {
        fn FT_Open(device: c_int, handle: *mut Handle) -> u32;
        fn FT_Close(handle: Handle) -> u32;
    }

    #[link(name = "ft4222")]
    unsafe extern "C" {
        fn FT4222_SetClock(handle: Handle, rate: u8) -> u32;
        fn FT4222_SPIMaster_Init(
            handle: Handle,
            io_line: u8,
            clock: u8,
            cpol: u8,
            cpha: u8,
            sso_map: u8,
        ) -> u32;
        fn FT4222_SPIMaster_SingleRead(
            handle: Handle,
            buffer: *mut u8,
            bytes_to_read: u16,
            size_transferred: *mut u16,
            is_end_transaction: bool,
        ) -> u32;
    }

    /// An opened bridge. Closes on drop.
    pub struct Ft4222Waterfall {
        handle: Handle,
        /// Bytes read but not yet cut into a frame. The pipe has no framing of its own, so a frame
        /// boundary is found rather than assumed — see `FRAME_TRAILER`.
        acc: Vec<u8>,
    }

    // SAFETY: `handle` is an opaque D2XX handle (`*mut c_void`), which is why the compiler refuses
    // `Send` by default — a raw pointer says nothing about who may touch it.
    //
    // Sending this value is sound because OWNERSHIP MOVES, it is never shared:
    //   * the handle is written exactly once, in `open`, and the struct is neither `Clone` nor
    //     `Copy`, so no second value can ever refer to the same handle;
    //   * every use goes through `&mut self` (`read_frame`), so D2XX is only ever called from
    //     whichever single thread owns the value at that moment;
    //   * `Drop` closes it, on that same owning thread.
    //
    // This is `Send` and deliberately NOT `Sync`: the reader thread takes the value and keeps it.
    // Two threads calling into one FT4222 handle concurrently is exactly what would be unsound, and
    // `Sync` is what would permit it.
    unsafe impl Send for Ft4222Waterfall {}

    impl Ft4222Waterfall {
        /// Open interface A of the bridge and put it in SPI-master mode.
        ///
        /// `device` is the index in FTDI's device list; interface A is the data interface (B
        /// exists and carries nothing we want). The first open on a freshly-appeared bridge was
        /// observed to hang for minutes once and be instant every time after, so a caller must
        /// treat this as a blocking call worth a timeout — that is the caller's job, because
        /// D2XX offers no timeout of its own here.
        pub fn open(device: i32) -> std::io::Result<Self> {
            let mut handle: Handle = std::ptr::null_mut();
            let st = unsafe { FT_Open(device, &mut handle) };
            if st != 0 || handle.is_null() {
                return Err(std::io::Error::other(format!("FT_Open failed: {st}")));
            }
            let me = Self {
                handle,
                acc: Vec::with_capacity(2 * FRAME_BYTES),
            };
            let st = unsafe {
                FT4222_SPIMaster_Init(
                    handle,
                    SPI_IO_SINGLE,
                    CLK_DIV_16,
                    CPOL_IDLE_HIGH,
                    CPHA_CLK_TRAILING,
                    SS0,
                )
            };
            if st != 0 {
                return Err(std::io::Error::other(format!(
                    "SPIMaster_Init failed: {st}"
                )));
            }
            let st = unsafe { FT4222_SetClock(handle, SYS_CLK_48) };
            if st != 0 {
                return Err(std::io::Error::other(format!("SetClock failed: {st}")));
            }
            Ok(me)
        }
    }

    impl WaterfallSource for Ft4222Waterfall {
        /// Read until a WHOLE frame can be cut out on its trailer.
        ///
        /// The transport is a byte pipe with no framing (see `FRAME_TRAILER`), so a fixed-size read
        /// is not a frame — it is 4096 bytes starting wherever the pipe happens to be. This keeps a
        /// small accumulator and returns the last complete frame in it, which is what makes a bin
        /// index mean a frequency. A read that contains no trailer yields `Err`, and the caller
        /// treats that as a dropped frame: the previous picture stays, which is the right answer for
        /// a transient, and a persistent one shows as a frozen scope rather than a plausible lie.
        fn read_frame(&mut self) -> std::io::Result<Vec<u8>> {
            self.read_once()
        }
    }

    /// Bytes per SPI read: TWO frames.
    ///
    /// A frame is cut on its trailer out of a pipe with no framing, so a read of exactly one frame's
    /// length contains a whole frame only when the boundary happens to fall right — measured, that
    /// is about half the time (184 frames from 400 single-frame reads, and a bounded retry only
    /// lifted it to 201 for up to three times the traffic, because the failures are correlated).
    /// Reading two frames' worth means a complete frame is present whatever the rotation.
    const READ_BYTES: usize = 2 * FRAME_BYTES;

    impl Ft4222Waterfall {
        fn read_once(&mut self) -> std::io::Result<Vec<u8>> {
            let mut buf = vec![0u8; READ_BYTES];
            let mut got: u16 = 0;
            let st = unsafe {
                FT4222_SPIMaster_SingleRead(
                    self.handle,
                    buf.as_mut_ptr(),
                    READ_BYTES as u16,
                    &mut got,
                    // isEndTransaction = TRUE — chip-select is de-asserted at the end of every read.
                    //
                    // It was `false`, carried over from the Python wrapper that first proved this
                    // transport. With CS held asserted the transaction never ends, so a GAP IN
                    // READING leaves the slave mid-word and everything after it arrives shifted by
                    // one BIT — measured on the bench (station FT-710, 2026-08-20): the frame
                    // trailer `FF 01 EE 01…` came back as `7F 80 F7 00…`, the same bits one place
                    // right, and it never recovered. The app takes exactly such a gap whenever the
                    // rig's scope is not in a CENTER mode, because `pump` returns before reading.
                    // With `true`, three 3-second pauses cost nothing: 300 of 300 frames aligned.
                    true,
                )
            };
            if st != 0 {
                return Err(std::io::Error::other(format!("SingleRead failed: {st}")));
            }
            buf.truncate(got as usize);
            self.acc.extend_from_slice(&buf);
            // Bound the accumulator. Two frames is enough to cut one out at any rotation; more than
            // that means we are not finding trailers at all, and holding megabytes of a stream we
            // cannot parse helps nobody.
            let cap = 3 * FRAME_BYTES;
            if self.acc.len() > cap {
                let drop_to = self.acc.len() - cap;
                self.acc.drain(..drop_to);
            }
            match super::frame_end(&self.acc) {
                Some(end) => {
                    let frame = self.acc[end - FRAME_BYTES..end].to_vec();
                    self.acc.drain(..end); // consumed — never re-cut the same frame
                    Ok(frame)
                }
                None => Err(std::io::Error::other(
                    "no frame trailer in the stream yet — dropping this read",
                )),
            }
        }
    }

    impl Drop for Ft4222Waterfall {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe { FT_Close(self.handle) };
            }
        }
    }
}

#[cfg(test)]
mod tests {

    /// The absent-library message must NAME the library and say where to get it.
    ///
    /// This is the whole difference between "the feature quietly did not appear" and an operator
    /// who can act. Pinned by content rather than left to prose drift: someone with the right
    /// radio and the right cable must be able to read this one string and know what to do.
    #[test]
    fn the_absent_library_message_names_the_library_and_where_to_get_it() {
        let m = YAESU_WF_NO_LIBRARY;
        assert!(m.contains("LibFT4222"), "names the library: {m}");
        assert!(m.contains("https://"), "carries a link: {m}");
        assert!(m.contains("ftdichip.com"), "and the link is FTDI's: {m}");
        // Says what is NOT affected, so it does not read as "the app is broken".
        assert!(
            m.contains("panadapter") || m.contains("Everything else works"),
            "scopes the impact: {m}"
        );
    }

    /// A build without the feature must SAY it has no transport, not merely fail to open.
    ///
    /// `TRANSPORT_COMPILED` is what lets the caller answer the right question. If this ever
    /// reported true on a default build, the operator would be sent to the radio's EX menu to fix
    /// something the menu cannot fix.
    #[test]
    fn a_default_build_reports_no_transport_and_opens_nothing() {
        if cfg!(feature = "yaesu-wf") {
            assert!(TRANSPORT_COMPILED, "the feature is on, so the transport is compiled");
            return;
        }
        assert!(!TRANSPORT_COMPILED, "no feature means no transport");
        assert!(
            open_default_source().is_none(),
            "and nothing can be opened without one"
        );
    }
    use super::*;

    #[test]
    fn a_full_frame_yields_one_normalised_line_per_bin() {
        let mut src = MockWaterfall::ramp();
        let raw = src.read_frame().unwrap();
        let row = parse_wf1(&raw).expect("a full frame parses");
        assert_eq!(row.len(), WF1_BINS);
        // The ramp runs 0..255 in the RAW bytes, and the parser inverts, so the first bin is
        // full scale and the last is the floor. Asserting it this way round is what would fail if
        // the inversion were ever dropped.
        assert!((row[0] - 1.0).abs() < 1e-6, "raw 0 = strongest = 1.0");
        assert!(
            (row[WF1_BINS - 1] - 0.0).abs() < 1e-6,
            "raw 255 = weakest = 0.0"
        );
        assert!(
            row.iter().all(|v| (0.0..=1.0).contains(v)),
            "row stays in 0..1"
        );
    }

    /// THE PARSER AGAINST A REAL RADIO, not a shape I invented. Captured from an FT-710 on
    /// 2026-08-17 with the SCU-LAN10 option on; the synthetic ramp above proves the arithmetic,
    /// this proves the OFFSETS — a layout error would pass the ramp and fail here.
    #[test]
    fn a_real_ft710_frame_parses_into_a_plausible_spectrum() {
        let raw = include_bytes!("../tests/fixtures/ft710_wf_frame.bin");
        assert_eq!(
            raw.len(),
            FRAME_BYTES,
            "the radio sends fixed 4096-byte frames"
        );
        let row = parse_wf1(raw).expect("a real frame parses");
        assert_eq!(row.len(), WF1_BINS);

        // A live band is neither flat nor saturated: there is a noise floor well above zero and
        // peaks below full scale. Flat would mean we are reading a dead region of the frame.
        // THIS ALSO GUARDS THE INVERSION, and that is why the bounds are tight. The fixture is a
        // QUIET band (raw bytes average 177 — mostly "nothing here"), so after inverting it must
        // read DARK: a low mean. Drop the inversion and the same bytes give ~0.69, which fails
        // here. A loose range would have let that through, and an upside-down waterfall is not
        // something a test should be able to miss.
        let mean = row.iter().sum::<f32>() / row.len() as f32;
        let max = row.iter().cloned().fold(f32::MIN, f32::max);
        assert!(
            (0.10..0.50).contains(&mean),
            "a quiet band must read dark after inversion: mean {mean}"
        );
        assert!(max > mean, "no peaks above the floor — wrong offset?");
        assert!(row.iter().all(|v| (0.0..=1.0).contains(v)));

        // The two trailing bytes are structurally zero and MUST be outside the row: inverted they
        // are full scale, i.e. a permanent phantom carrier at the top of every span.
        assert_eq!(
            (raw[850], raw[851]),
            (0, 0),
            "bins 850/851 are the padding this row deliberately excludes"
        );
        assert_eq!(WF1_BINS, 850);
        assert_eq!(WF1_STRIDE, 852, "the layout stride is unchanged");

        // The 144-byte parameter block is NOT usable on this model: 128 zeroes then a repeating
        // `ff 01 ee 01` idle pattern. Pinned so that if a future frame does carry frequencies,
        // this test fails and says so rather than the block being quietly assumed empty forever.
        let params = &raw[3952..FRAME_BYTES];
        assert!(
            params[..128].iter().all(|&b| b == 0)
                && params[128..] == [0xff, 0x01, 0xee, 0x01].repeat(4),
            "the parameter block changed shape — re-derive it before relying on it"
        );
    }

    /// THE ENGINE CONNECTION: a frame in, an RF row out, with the edges CAT supplied.
    #[test]
    fn a_frame_reaches_the_spectrum_feed_as_an_absolute_rf_row() {
        let feed = tempo_app::engine::SpectrumFeed::default();
        let mut src = MockWaterfall::ramp();
        let meta = SweepMeta {
            dial_hz: 14_100_000.0,
            center_hz: None,
            fix_start_hz: None,
            span_code: b'7', // 200 kHz — what the radio reported
            mode_code: b'4', // W/F CENTER (NORMAL) — likewise
        };
        assert_eq!(pump(&mut src, &feed, meta), Pumped::Published);

        let row = feed.row().expect("a row reached the feed");
        assert_eq!(row.row.len(), WF1_BINS);
        assert_eq!(row.lo_hz, 14_000_000.0);
        assert_eq!(row.hi_hz, 14_200_000.0);
        assert_eq!(
            row.source, SOURCE,
            "the label the UI's isRfScopeSource must recognise"
        );
    }

    /// The two failures are NOT interchangeable, and the distinction is what the caller acts on:
    /// a transient read fault keeps the last picture, an unplaceable row clears it.
    #[test]
    fn a_transient_fault_is_dropped_but_an_unplaceable_row_is_unavailable() {
        let feed = tempo_app::engine::SpectrumFeed::default();
        let meta = SweepMeta {
            dial_hz: 14_100_000.0,
            center_hz: None,
            fix_start_hz: None,
            span_code: b'7',
            mode_code: b'4',
        };

        // A source with nothing to give = transient.
        let mut empty = MockWaterfall::new(vec![]);
        assert_eq!(pump(&mut empty, &feed, meta), Pumped::Dropped);
        // A short frame is the same class.
        let mut short = MockWaterfall::new(vec![vec![0u8; 10]]);
        assert_eq!(pump(&mut short, &feed, meta), Pumped::Dropped);

        // A CURSOR/FIX scope mode cannot be placed at all — and the read must not even be
        // attempted, since the answer would be discarded.
        let mut src = MockWaterfall::ramp();
        let cursor = SweepMeta {
            mode_code: b'7',
            ..meta
        };
        assert_eq!(pump(&mut src, &feed, cursor), Pumped::Unavailable);
    }

    /// The replies the radio actually sent, and the crossed-reply case the P2 check exists for.
    #[test]
    fn an_ss_reply_yields_its_p3_only_for_the_subcommand_asked_for() {
        // Captured from the FT-710 on 2026-08-17.
        assert_eq!(parse_ss_reply("SS0570000;", b'5'), Some(b'7')); // span  = 200 kHz
        assert_eq!(parse_ss_reply("SS0640000;", b'6'), Some(b'4')); // mode  = W/F CENTER (NORMAL)
        assert_eq!(parse_ss_reply("SS0000000;", b'0'), Some(b'0')); // speed = SLOW1

        // A span reply must NOT satisfy a mode read. Byte 4 of this string is '7', which as a
        // MODE code means CURSOR — so without the check the scope would quietly refuse to draw.
        assert_eq!(parse_ss_reply("SS0570000;", b'6'), None);

        // Rubbish and truncation are refused rather than indexed into.
        assert_eq!(parse_ss_reply("?;", b'5'), None);
        assert_eq!(parse_ss_reply("SS05", b'5'), None);
        assert_eq!(parse_ss_reply("", b'5'), None);
        assert_eq!(parse_ss_reply("FA014100000;", b'5'), None);
    }

    /// The reader thread, end to end: rows appear while it runs, and it stops on drop.
    #[test]
    fn the_reader_publishes_while_it_runs_and_stops_when_dropped() {
        use std::time::Duration;
        let feed = tempo_app::engine::SpectrumFeed::default();
        let meta: SharedMeta = std::sync::Arc::new(std::sync::Mutex::new(Some(SweepMeta {
            dial_hz: 14_100_000.0,
            center_hz: None,
            fix_start_hz: None,
            span_code: b'7',
            mode_code: b'4',
        })));
        let reader = YaesuWaterfall::start(
            Box::new(MockWaterfall::ramp()),
            feed.clone(),
            meta.clone(),
            Duration::from_millis(1),
        );
        // Give it a few cycles — deliberately generous so a loaded machine does not make this
        // flaky, the mistake the pipe-buffer test taught.
        std::thread::sleep(Duration::from_millis(120));
        let row = feed.row().expect("the reader published a row");
        assert_eq!(row.source, SOURCE);
        assert_eq!((row.lo_hz, row.hi_hz), (14_000_000.0, 14_200_000.0));

        drop(reader); // joins; a leaked thread would keep writing after the owner let go
        let after = feed.row();
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(
            feed.row().map(|r| r.row.len()),
            after.map(|r| r.row.len()),
            "nothing may be published after the reader is dropped"
        );
    }

    /// Metadata we do not have must CLEAR the feed, not leave the last row standing: the row would
    /// claim a band the radio is no longer looking at.
    #[test]
    fn unknown_metadata_clears_the_feed_rather_than_leaving_a_stale_row() {
        use std::time::Duration;
        let feed = tempo_app::engine::SpectrumFeed::default();
        let meta: SharedMeta = std::sync::Arc::new(std::sync::Mutex::new(Some(SweepMeta {
            dial_hz: 14_100_000.0,
            center_hz: None,
            fix_start_hz: None,
            span_code: b'7',
            mode_code: b'4',
        })));
        let _reader = YaesuWaterfall::start(
            Box::new(MockWaterfall::ramp()),
            feed.clone(),
            meta.clone(),
            Duration::from_millis(1),
        );
        std::thread::sleep(Duration::from_millis(80));
        assert!(feed.row().is_some(), "a row was flowing first");

        *meta.lock().unwrap() = None; // the CAT side lost the span/mode
        std::thread::sleep(Duration::from_millis(80));
        assert!(
            feed.row().is_none(),
            "an unplaceable row must be cleared, not left on screen"
        );
    }

    /// A short read must be DROPPED, not padded. Half a line rendered as a whole one shows
    /// silence where the band is, which reads as a dead radio rather than a lost frame.
    #[test]
    fn a_truncated_frame_is_refused_rather_than_padded() {
        assert!(parse_wf1(&vec![0u8; WF1_BINS - 1]).is_none());
        // Exactly enough for the line is fine even if the rest of the frame is missing.
        assert!(parse_wf1(&vec![0u8; WF1_BINS]).is_some());
    }

    /// The span table is Yaesu's, and every code the manual defines must resolve — a missing
    /// entry would silently mis-scale the whole row.
    #[test]
    fn every_documented_span_code_resolves_and_others_do_not() {
        let expect = [
            (b'0', 1_000.0),
            (b'1', 2_000.0),
            (b'2', 5_000.0),
            (b'3', 10_000.0),
            (b'4', 20_000.0),
            (b'5', 50_000.0),
            (b'6', 100_000.0),
            (b'7', 200_000.0),
            (b'8', 500_000.0),
            (b'9', 1_000_000.0),
        ];
        for (code, hz) in expect {
            assert_eq!(span_hz(code), Some(hz), "code {}", code as char);
        }
        // The radio answered `SS0570000;` on 2026-08-17 — code 7, 200 kHz.
        assert_eq!(span_hz(b'7'), Some(200_000.0));
        for bad in [b'A', b'x', 0u8] {
            assert_eq!(span_hz(bad), None, "undefined codes must not be guessed");
        }
    }

    /// CENTER modes are the only ones whose edges follow from the dial. CURSOR and FIX are pinned
    /// to something CAT does not report, so they must yield no row rather than a wrong one.
    #[test]
    fn only_centered_modes_get_edges() {
        // W/F CENTER (NORMAL) = 4 — what the radio reported (`SS0640000;`).
        assert_eq!(
            sweep_edges(14_100_000.0, b'7', b'4'),
            Some((14_000_000.0, 14_200_000.0))
        );
        // 3DSS CENTER (0) centres on the dial too.
        assert_eq!(
            sweep_edges(14_100_000.0, b'7', b'0'),
            Some((14_000_000.0, 14_200_000.0))
        );
        for &cursor_or_fix in b"12679A" {
            assert_eq!(
                sweep_edges(14_100_000.0, b'7', cursor_or_fix),
                None,
                "mode {} is not centred on the dial",
                cursor_or_fix as char
            );
        }
        // An undefined span is refused even in a centred mode.
        assert_eq!(sweep_edges(14_100_000.0, b'Z', b'4'), None);
        // A span wider than the dial would put the row below 0 Hz.
        assert_eq!(sweep_edges(100_000.0, b'9', b'4'), None);
    }

    // ── Framing: the SPI side has none, so a frame boundary is FOUND ─────────────────────────────
    //
    // Bench measurement behind these (station FT-710, 2026-08-20), because none of it is guessable:
    // a fixed-size read is not a frame. With the shipped code, three 3-second gaps in reading left
    // 100 of 400 frames valid — the rest shifted by one BIT, because chip-select was never
    // de-asserted — and the read window drifts against the radio's frame boundary even with no gap
    // at all (aligned 48 frames, then misaligned 51, untouched). Cutting on the trailer and reading
    // two frames' worth per read gave 400 of 400, checked against a signature the cut does not use.

    fn framed(payload: u8) -> Vec<u8> {
        let mut f = vec![payload; FRAME_BYTES];
        f[FRAME_BYTES - FRAME_TRAILER.len()..].copy_from_slice(&FRAME_TRAILER);
        f
    }

    #[test]
    fn a_whole_frame_is_cut_at_its_trailer() {
        let w = framed(0x22);
        assert_eq!(frame_end(&w), Some(FRAME_BYTES));
    }

    #[test]
    fn a_trailer_without_a_frame_in_front_of_it_is_not_a_frame() {
        // The rotation case, and the one that makes the difference between a correct row and a
        // plausible wrong one: the trailer arrived, but the bins that belong to it did not. Cutting
        // here would splice the head of this frame onto whatever preceded it.
        let mut w = vec![0x11u8; 100];
        w.extend_from_slice(&FRAME_TRAILER);
        assert_eq!(frame_end(&w), None);
    }

    #[test]
    fn the_last_complete_frame_wins_when_a_read_caught_up_on_two() {
        // Two frames' worth is read per call, so this is the ordinary case, not an edge one. Taking
        // the LAST keeps the waterfall showing the newest sweep instead of one frame of latency.
        let mut w = framed(0x33);
        w.extend_from_slice(&framed(0x44));
        assert_eq!(frame_end(&w), Some(2 * FRAME_BYTES));
        let cut = &w[frame_end(&w).unwrap() - FRAME_BYTES..frame_end(&w).unwrap()];
        assert_eq!(cut[0], 0x44, "the newest frame, not the older one");
    }

    #[test]
    fn a_stream_with_no_trailer_yields_nothing_rather_than_a_guess() {
        assert_eq!(frame_end(&vec![0x55u8; 3 * FRAME_BYTES]), None);
    }

    #[test]
    fn a_frame_split_across_two_reads_is_still_found() {
        // What a real read looks like: a partial frame, then a whole one. The whole one must be
        // recovered, and its first byte must be the new frame's, not the tail of the partial.
        let mut w = vec![0x66u8; 1_234]; // tail of an earlier frame, no trailer of its own
        w.extend_from_slice(&framed(0x77));
        let end = frame_end(&w).expect("the complete frame is found");
        assert_eq!(end, 1_234 + FRAME_BYTES);
        assert_eq!(w[end - FRAME_BYTES], 0x77, "cut starts at the frame, not in the tail");
    }


    // ── Commanding the radio's own span ─────────────────────────────────────────────────────────
    //
    // Operator, 2026-08-20: "I expect the panadapter in the app to reflect the panadapter in the
    // radio (its settings and width)." Cropping the row client-side does the opposite — it shows a
    // narrower window of the SAME coarse sweep, ~42 of 850 bins stretched across the panel.

    #[test]
    fn every_rung_of_the_rigs_ladder_round_trips() {
        // The ladder is the radio's, not ours: 1, 2, 5, 10, 20, 50, 100, 200, 500 kHz and 1 MHz
        // (FT-710 CAT reference, SS P2=5). A code that survives Hz and back is a code the rig has.
        for c in b'0'..=b'9' {
            let hz = span_hz(c).expect("every code 0-9 is a documented span");
            assert_eq!(span_code_for_hz(hz as u32), Some(c), "code {}", c as char);
        }
    }

    #[test]
    fn a_span_the_radio_cannot_sweep_is_refused_not_rounded() {
        // The whole point of an exact map. 15 kHz sits between two rungs; rounding it would leave
        // the app drawing one width while the rig swept another, and nothing would report the
        // difference — the app's axis would simply be wrong.
        assert_eq!(span_code_for_hz(15_000), None);
        assert_eq!(span_code_for_hz(0), None);
        assert_eq!(span_code_for_hz(2_000_000), None);
        // And the ones it CAN: the two the old client-side presets came closest to.
        assert_eq!(span_code_for_hz(10_000), Some(b'3'));
        assert_eq!(span_code_for_hz(50_000), Some(b'5'));
        // And the doubling the caller does: the engine's span request is a ± HALF-width (Icom
        // CI-V 27 15), while `SS` P2=5 names the full span. A UI chip labelled 200k sends 100k.
        assert_eq!(span_code_for_hz(100_000u32 * 2), Some(b'7'), "±100k is the 200 kHz rung");
        assert_eq!(span_code_for_hz(5_000u32 * 2), Some(b'3'), "±5k is the 10 kHz rung");
    }

    #[test]
    fn a_set_command_has_the_shape_the_radio_answers_a_read_with() {
        // `SS<P1><P2><P3><P4..P7>;` — P4..P7 fixed at 0. The read of the same field comes back in
        // exactly this shape (`SS0570000;` measured on the bench), which is the cheapest available
        // check that the set is well formed.
        assert_eq!(set_span_command(b'7'), "SS0570000;");
        assert_eq!(set_span_command(b'3'), "SS0530000;");
        // Mode 4 = W/F CENTER (NORMAL), the family whose edges `sweep_edges` can place.
        assert_eq!(set_mode_command(b'4'), "SS0640000;");
        // A set is the read's shape with P3 filled in, so parsing it back must yield the code —
        // the same function the reply path uses, pointed at what we are about to send.
        assert_eq!(parse_ss_reply(&set_span_command(b'7'), b'5'), Some(b'7'));
        assert_eq!(parse_ss_reply(&set_mode_command(b'4'), b'6'), Some(b'4'));
    }


    // ── Placing a CURSOR sweep ──────────────────────────────────────────────────────────────────

    #[test]
    fn an_anchored_cursor_window_stays_put_while_the_dial_moves_across_it() {
        // 200 kHz span anchored at 14.150; the dial moves 20 kHz and the EDGES do not.
        let at = |dial: f64| sweep_edges_anchored(dial, b'7', b'7', Some(14_150_000.0), None);
        let a = at(14_150_000.0).expect("anchored cursor is placeable");
        let b = at(14_170_000.0).expect("still placeable after tuning");
        assert_eq!(a, b, "the window is fixed; the dial is what moves");
        assert_eq!(a, (14_050_000.0, 14_250_000.0));
    }

    #[test]
    fn a_cursor_sweep_with_no_anchor_is_refused() {
        // Before the transition is seen — e.g. Nexus started with the rig already in CURSOR — the
        // window is unknown, and unknown must not be drawn.
        assert_eq!(sweep_edges_anchored(14_150_000.0, b'7', b'7', None, None), None);
    }

    #[test]
    fn a_dial_that_has_left_the_cursor_window_makes_it_unknown_again() {
        // At the edge the radio does something — shift, re-centre, stop — and nothing tells us
        // which. Half of 200 kHz is 100 kHz, so 100.1 kHz away is outside.
        assert!(sweep_edges_anchored(14_249_000.0, b'7', b'7', Some(14_150_000.0), None).is_some());
        assert_eq!(sweep_edges_anchored(14_251_000.0, b'7', b'7', Some(14_150_000.0), None), None);
    }

    #[test]
    fn fix_is_refused_when_its_start_has_not_been_stated() {
        // A cursor ANCHOR is not a FIX start — they are a centre and a left edge. Handing the
        // anchor in must not make FIX placeable; only a stated start does that.
        for code in [b'2', b'9', b'A'] {
            assert_eq!(sweep_edges_anchored(14_150_000.0, b'7', code, Some(14_150_000.0), None), None);
        }
    }

    #[test]
    fn a_center_sweep_ignores_any_anchor_and_follows_the_dial() {
        let with = sweep_edges_anchored(14_150_000.0, b'7', b'4', Some(7_000_000.0), None);
        let without = sweep_edges_anchored(14_150_000.0, b'7', b'4', None, None);
        assert_eq!(with, without);
        assert_eq!(with, Some((14_050_000.0, 14_250_000.0)));
    }


    // ── The EX menu, which turned out to be reachable over CAT ──────────────────────────────────
    //
    // Every shape below was READ OFF the radio (bench, 2026-08-20), not derived from the chart:
    //   EX030126;  -> EX0301261;   SCU-LAN10 = ON
    //   EX040401;  -> EX0404010;   EXT DISPLAY = OFF  (and the waterfall was working)
    //   EX040202;  -> EX0402021;   SCOPE CTR = CARRIER POINT
    // The middle one is why an operator message got withdrawn rather than reworded.

    #[test]
    fn a_menu_item_reads_with_three_two_digit_fields() {
        assert_eq!(ex_read_command(EX_SCU_LAN10), "EX030126;");
        assert_eq!(ex_read_command(EX_EXT_DISPLAY), "EX040401;");
        assert_eq!(ex_read_command(EX_SCOPE_CTR), "EX040202;");
    }

    #[test]
    fn a_set_is_the_read_with_the_value_appended() {
        assert_eq!(ex_set_command(EX_SCU_LAN10, "1"), "EX0301261;");
        assert_eq!(ex_set_command(EX_SCOPE_CTR, "1"), "EX0402021;");
    }

    #[test]
    fn a_reply_yields_its_value_and_survives_the_nul_the_radio_sends() {
        // The raw-CAT path is NUL-terminated, so the parser has to tolerate it — that terminator is
        // what broke every raw read in this app until 2026-08-20.
        assert_eq!(parse_ex_reply("EX0301261;", EX_SCU_LAN10).as_deref(), Some("1"));
        assert_eq!(parse_ex_reply("EX0301261;\0", EX_SCU_LAN10).as_deref(), Some("1"));
        assert_eq!(parse_ex_reply("EX0404010;", EX_EXT_DISPLAY).as_deref(), Some("0"));
    }

    #[test]
    fn a_reply_for_a_different_menu_item_is_refused() {
        // The point of checking the address. These values are all small integers, so one item's
        // answer read as another's would look perfectly valid — and on a link that can interleave,
        // that is not hypothetical. SCU-LAN10 reading "0" from EXT DISPLAY's reply would send the
        // operator to power-cycle a radio whose setting was already on.
        assert_eq!(parse_ex_reply("EX0404010;", EX_SCU_LAN10), None);
        assert_eq!(parse_ex_reply("EX0402021;", EX_EXT_DISPLAY), None);
        // And malformed answers yield nothing rather than a guess.
        assert_eq!(parse_ex_reply("EX030126;", EX_SCU_LAN10), None, "no value at all");
        assert_eq!(parse_ex_reply("?;", EX_SCU_LAN10), None);
        assert_eq!(parse_ex_reply("", EX_SCU_LAN10), None);
    }


    // ── Placing a FIX sweep ─────────────────────────────────────────────────────────────────────
    //
    // Operator, 2026-08-20: "in fix the scale reads start to start plus span." That is the whole
    // geometry, and the reason the start is a SEPARATE field from the cursor anchor: one is a left
    // edge and the other a centre, and reading one as the other is a half-span error that looks
    // entirely reasonable on screen.

    #[test]
    fn a_stated_fix_start_plus_the_span_is_the_window() {
        // 10 kHz span starting at 14.070 → 14.070–14.080. Not centred on anything.
        assert_eq!(
            sweep_edges_anchored(14_074_000.0, b'3', b'A', None, Some(14_070_000.0)),
            Some((14_070_000.0, 14_080_000.0))
        );
    }

    #[test]
    fn a_fix_window_does_not_move_when_the_dial_leaves_it() {
        // The difference from CURSOR, and it is not an oversight: in FIX the operator may tune
        // right out of the window and the window stays put, so the dial is not in the arithmetic.
        let a = sweep_edges_anchored(14_074_000.0, b'3', b'A', None, Some(14_070_000.0));
        let b = sweep_edges_anchored(14_200_000.0, b'3', b'A', None, Some(14_070_000.0));
        assert_eq!(a, b);
    }

    #[test]
    fn every_fix_family_uses_the_stated_start() {
        // 3DSS FIX, W/F FIX (EXPAND) and W/F FIX (NORMAL) are the same geometry.
        for code in [b'2', b'9', b'A'] {
            assert_eq!(
                sweep_edges_anchored(7_100_000.0, b'2', code, None, Some(7_050_000.0)),
                Some((7_050_000.0, 7_055_000.0)),
                "code {}", code as char
            );
        }
    }

    #[test]
    fn a_cursor_anchor_is_never_read_as_a_fix_start() {
        // The half-span trap, pinned. If FIX read the anchor as a centre, this would come back
        // 14.145–14.155 instead of nothing.
        assert_eq!(sweep_edges_anchored(14_150_000.0, b'3', b'A', Some(14_150_000.0), None), None);
    }


    #[test]
    fn a_fix_sweep_with_a_stated_start_actually_publishes_a_row() {
        // The chain, not just the geometry: metadata carrying a FIX mode AND a stated start must
        // make `pump` PUBLISH, not report the row unplaceable. The operator saw the panadapter stay
        // on sound-card audio after stating a start (2026-08-20), and the arithmetic below was
        // already passing its own tests — so the gap, if any, is between them.
        let feed = tempo_app::engine::SpectrumFeed::default();
        let mut src = MockWaterfall::ramp();
        let meta = SweepMeta {
            dial_hz: 14_074_000.0,
            center_hz: None,
            fix_start_hz: Some(14_070_000.0),
            span_code: b'3', // 10 kHz
            mode_code: b'A', // W/F FIX (NORMAL)
        };
        assert_eq!(pump(&mut src, &feed, meta), Pumped::Published);
        let row = feed.row().expect("a row reached the feed");
        assert_eq!((row.lo_hz, row.hi_hz), (14_070_000.0, 14_080_000.0));
        assert_eq!(row.source, SOURCE);
    }

    #[test]
    fn a_fix_sweep_without_a_start_is_reported_unavailable_not_published() {
        let feed = tempo_app::engine::SpectrumFeed::default();
        let mut src = MockWaterfall::ramp();
        let meta = SweepMeta {
            dial_hz: 14_074_000.0,
            center_hz: None,
            fix_start_hz: None,
            span_code: b'3',
            mode_code: b'A',
        };
        assert_eq!(pump(&mut src, &feed, meta), Pumped::Unavailable);
    }


    // ── The FIX window, derived with no operator interaction ────────────────────────────────────
    //
    // Operator, 2026-08-20: "I do not want any user interaction/clicking to get the spectrum working
    // in fix mode. When I select fix mode, I want you to set the scope to the center of the tuned
    // band, with the minimal span that is needed to be able to show the full band. The start is the
    // center minus half of the minimal needed span."

    #[test]
    fn twenty_metres_asks_for_the_narrowest_span_that_covers_it() {
        // 14.000-14.350 is 350 kHz wide: 200 kHz does not cover it, 500 kHz does.
        assert_eq!(auto_fix_span_code(14_074_000.0), Some(b'8'), "500 kHz");
        // And at that span the window starts at the BAND EDGE, as the radio does: 14.000-14.500.
        let start = auto_fix_start(14_074_000.0, b'8').expect("placeable");
        assert_eq!(start, 14_000_000.0, "the radio starts FIX at the band edge");
        assert!(start <= 14_000_000.0 && start + 500_000.0 >= 14_350_000.0, "band inside");
    }

    #[test]
    fn the_start_follows_the_span_actually_in_force() {
        // THE BUG THIS PINS. The bins arrive at whatever span the RADIO has, so a start computed
        // from the span we merely asked for draws a window of the wrong width — and it looks
        // entirely plausible. Observed on the bench: a 500 kHz-derived start (13.925) paired with
        // the rig's real 200 kHz span drew 13.925-14.125, which is not the 20 m band by 225 kHz.
        // The start is the band edge whatever the span; the SPAN still decides the width, which is
        // what the bins arrive at. A start paired with the wrong width is the fault this pins.
        for code in [b'5', b'6', b'7', b'8', b'9'] {
            assert_eq!(
                auto_fix_start(14_074_000.0, code),
                Some(14_000_000.0),
                "band edge at span {}",
                code as char
            );
        }
    }

    #[test]
    fn a_narrow_band_asks_for_a_narrow_span_rather_than_the_same_one_everywhere() {
        // 30 m is 50 kHz wide, so it takes the 50 kHz rung — 850 bins across 50 kHz instead of 500,
        // which is the whole point of asking for the MINIMAL covering span.
        assert_eq!(auto_fix_span_code(10_136_000.0), Some(b'5'), "50 kHz");
        assert_eq!(auto_fix_start(10_136_000.0, b'5'), Some(10_100_000.0));
    }

    #[test]
    fn every_band_in_the_table_is_coverable_from_its_edge() {
        for dial in [1_850_000.0, 3_600_000.0, 7_100_000.0, 14_074_000.0, 21_074_000.0, 24_915_000.0] {
            let (lo, hi) = band_edges_hz(dial).expect("a band");
            let code = auto_fix_span_code(dial).expect("coverable");
            let start = auto_fix_start(dial, code).expect("placeable");
            let end = start + span_hz(code).unwrap();
            assert_eq!(start, lo, "the window starts at the band edge");
            assert!(end >= hi, "band {lo}-{hi} inside {start}-{end}");
        }
        // 10 m (1.7 MHz) and 6 m (2 MHz) exceed the 1 MHz top rung: no window covers them, and
        // saying so beats drawing a third of the band as though it were all of it.
        assert_eq!(auto_fix_span_code(28_074_000.0), None, "10 m exceeds every rung");
        assert_eq!(auto_fix_span_code(50_313_000.0), None, "6 m exceeds every rung");
    }

    #[test]
    fn a_dial_outside_every_ham_band_yields_no_window() {
        assert_eq!(band_edges_hz(9_410_000.0), None, "a broadcast station is not a band");
        assert_eq!(auto_fix_span_code(9_410_000.0), None);
        assert_eq!(auto_fix_start(9_410_000.0, b'7'), None);
    }

}
