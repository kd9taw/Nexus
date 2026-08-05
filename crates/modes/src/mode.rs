//! The pluggable [`Mode`] abstraction.
//!
//! Everything mode-specific lives behind this one trait: T/R timing, the decode
//! frame size, waveform synthesis, message decode, the waterfall passband, and
//! capability flags. FT8, FT4, and FT1 are the concrete implementations shipped
//! today; a future mode (e.g. CX1) becomes a new `impl Mode` with **no changes**
//! to the rest of the nerve-center scaffolding (spots, map, log, UI), which talk
//! to modes only through this interface.

use crate::decode::Decode;

/// Identity of a concrete mode (for selection, serialization, display). Carries
/// the per-mode timing metadata (slot length, frame size) so callers can size
/// clocks/buffers without constructing a [`Mode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModeKind {
    Ft8,
    Ft4,
    /// WSJT-X FST4 / FST4W. **RECEIVE-ONLY** — see [`Capabilities::tx`].
    ///
    /// Carries its T/R period for the same reason `Q65` does: the frame is
    /// `period_s * 12000` samples, so the period has to reach
    /// [`Self::frame_samples`] and [`Self::slot_secs`].
    ///
    /// `wspr` selects **FST4W**, the WSPR-like beacon mode (50-bit messages, no
    /// AP decoding), over FST4's QSO mode. They are separate modes on the air and
    /// separate operator selections, but one decoder and one frame contract —
    /// hence a flag rather than a second variant duplicating the sizing.
    ///
    /// `period_s` ∈ {15, 30, 60, 120, 300, 900, 1800}. FST4W's standard beacon
    /// intervals are 120/300/900/1800.
    Fst4 {
        period_s: u16,
        wspr: bool,
    },
    /// WSJT-X Q65 — transmits and receives; see [`Capabilities::tx`].
    ///
    /// Carries its T/R period and submode because both are part of the mode's
    /// identity AND its buffer contract: the frame is `period_s * 12000` samples
    /// and the submode sets the tone spacing. Putting them here rather than in a
    /// side channel is what keeps [`Self::frame_samples`] and [`Self::slot_secs`]
    /// pure functions of the kind, so every consumer that sizes a capture buffer
    /// or drives slot timing from a `ModeKind` stays correct with no signature
    /// change.
    ///
    /// `period_s` ∈ {15, 30, 60, 120, 300}; `submode` ∈ 0..=4 for A..E. EME on
    /// VHF/UHF works Q65-60A/B/C; 6 m meteor/ionoscatter uses 30; 15 is
    /// troposcatter.
    Q65 {
        period_s: u16,
        submode: u8,
    },
    /// WSJT-X MSK144 — the METEOR-SCATTER mode. **RECEIVE-ONLY** — see
    /// [`Capabilities::tx`].
    ///
    /// Carries its T/R period for the same reason FST4 and Q65 do: the frame is
    /// `period_s * 12000` samples. `period_s` ∈ {5, 10, 15, 30}; 15 is the 6 m
    /// workhorse.
    Msk144 {
        period_s: u16,
    },
    /// WSJT JT65 — the classic weak-signal / EME mode. **RECEIVE-ONLY**.
    ///
    /// Carries only its submode: unlike FST4, Q65 and MSK144, JT65 has a single
    /// fixed 60 s T/R period, so the frame length is constant. `submode` is 0/1/2
    /// for A/B/C (tone spacing 1x/2x/4x).
    Jt65 {
        submode: u8,
    },
    /// WSPR — the propagation-BEACON mode. **RECEIVE-ONLY**.
    ///
    /// Carries nothing: one fixed 2-minute interval, one message form. It is
    /// also the only mode here whose decodes are not QSO traffic — they are
    /// propagation reports, and they report an ABSOLUTE frequency in MHz rather
    /// than an audio offset.
    Wspr,
    TempoFast,
}

impl ModeKind {
    /// All modes shipped today, in display order.
    pub const ALL: [ModeKind; 8] = [
        ModeKind::Ft8,
        ModeKind::Ft4,
        ModeKind::FST4_15,
        ModeKind::Q65_30A,
        ModeKind::MSK144_15,
        ModeKind::JT65A,
        ModeKind::Wspr,
        ModeKind::TempoFast,
    ];

    /// FST4 at 15 s, QSO mode — the combination the C ABI used to be pinned to,
    /// kept as `ALL`'s one representative FST4 entry.
    pub const FST4_15: ModeKind = ModeKind::Fst4 {
        period_s: 15,
        wspr: false,
    };

    /// MSK144 at 15 s — the period 6 m meteor scatter actually runs on, and
    /// `ALL`'s representative MSK144 entry.
    pub const MSK144_15: ModeKind = ModeKind::Msk144 { period_s: 15 };

    /// JT65A — the default submode, and `ALL`'s representative JT65 entry.
    pub const JT65A: ModeKind = ModeKind::Jt65 { submode: 0 };

    /// JT65 submodes A/B/C, passed as 0/1/2.
    pub const JT65_SUBMODES: u8 = 3;

    /// The T/R periods MSK144 supports, in seconds.
    pub const MSK144_PERIODS: [u16; 4] = [5, 10, 15, 30];

    /// The T/R periods FST4/FST4W support, in seconds.
    pub const FST4_PERIODS: [u16; 7] = [15, 30, 60, 120, 300, 900, 1800];

    /// Q65-30A: the combination the C ABI used to be pinned to, kept as the
    /// default so `ALL` has one representative Q65 entry rather than 25.
    pub const Q65_30A: ModeKind = ModeKind::Q65 {
        period_s: 30,
        submode: 0,
    };

    /// The T/R periods Q65 supports, in seconds.
    pub const Q65_PERIODS: [u16; 5] = [15, 30, 60, 120, 300];
    /// Q65 submodes A..E, passed as 0..=4.
    pub const Q65_SUBMODES: u8 = 5;

    /// Every valid Q65 period/submode combination.
    ///
    /// Separate from [`Self::ALL`] on purpose: `ALL` is the display-order list of
    /// mode families and would be swamped by 25 Q65 rows, but the capability
    /// invariants still have to hold for every one of them.
    pub fn q65_all() -> impl Iterator<Item = ModeKind> {
        Self::Q65_PERIODS.into_iter().flat_map(|period_s| {
            (0..Self::Q65_SUBMODES).map(move |submode| ModeKind::Q65 { period_s, submode })
        })
    }

    /// Every valid FST4/FST4W period and mode combination.
    pub fn fst4_all() -> impl Iterator<Item = ModeKind> {
        Self::FST4_PERIODS
            .into_iter()
            .flat_map(|period_s| [false, true].map(move |wspr| ModeKind::Fst4 { period_s, wspr }))
    }

    /// Every valid MSK144 period.
    pub fn msk144_all() -> impl Iterator<Item = ModeKind> {
        Self::MSK144_PERIODS
            .into_iter()
            .map(|period_s| ModeKind::Msk144 { period_s })
    }

    /// Every JT65 submode.
    pub fn jt65_all() -> impl Iterator<Item = ModeKind> {
        (0..Self::JT65_SUBMODES).map(|submode| ModeKind::Jt65 { submode })
    }

    /// Display name for an MSK144 period, e.g. `"MSK144-15"`.
    fn msk144_name(period_s: u16) -> &'static str {
        match period_s {
            5 => "MSK144-5",
            10 => "MSK144-10",
            15 => "MSK144-15",
            30 => "MSK144-30",
            _ => "MSK144",
        }
    }

    /// Display name for an FST4/FST4W period, e.g. `"FST4-60"` / `"FST4W-300"`.
    ///
    /// A table for the same reason as [`Self::q65_name`]: keeps [`Self::as_str`]
    /// returning `&'static str`.
    fn fst4_name(period_s: u16, wspr: bool) -> &'static str {
        const QSO: [&str; 7] = [
            "FST4-15",
            "FST4-30",
            "FST4-60",
            "FST4-120",
            "FST4-300",
            "FST4-900",
            "FST4-1800",
        ];
        const BEACON: [&str; 7] = [
            "FST4W-15",
            "FST4W-30",
            "FST4W-60",
            "FST4W-120",
            "FST4W-300",
            "FST4W-900",
            "FST4W-1800",
        ];
        let Some(i) = Self::FST4_PERIODS.iter().position(|&p| p == period_s) else {
            return if wspr { "FST4W" } else { "FST4" };
        };
        if wspr {
            BEACON[i]
        } else {
            QSO[i]
        }
    }

    /// Display name for a Q65 period/submode pair, e.g. `"Q65-60B"`.
    ///
    /// A lookup table rather than `format!` so [`Self::as_str`] can keep returning
    /// `&'static str` — changing that signature would ripple through every caller
    /// for the sake of five characters. An out-of-range pair yields the bare
    /// family name: better a vaguer label than one asserting a period that is not
    /// what the decoder was handed.
    fn q65_name(period_s: u16, submode: u8) -> &'static str {
        const NAMES: [[&str; 5]; 5] = [
            ["Q65-15A", "Q65-15B", "Q65-15C", "Q65-15D", "Q65-15E"],
            ["Q65-30A", "Q65-30B", "Q65-30C", "Q65-30D", "Q65-30E"],
            ["Q65-60A", "Q65-60B", "Q65-60C", "Q65-60D", "Q65-60E"],
            ["Q65-120A", "Q65-120B", "Q65-120C", "Q65-120D", "Q65-120E"],
            ["Q65-300A", "Q65-300B", "Q65-300C", "Q65-300D", "Q65-300E"],
        ];
        let Some(pi) = Self::Q65_PERIODS.iter().position(|&p| p == period_s) else {
            return "Q65";
        };
        match NAMES[pi].get(submode as usize) {
            Some(n) => n,
            None => "Q65",
        }
    }

    /// Short display name, e.g. `"FT8"`.
    pub fn as_str(self) -> &'static str {
        match self {
            ModeKind::Ft8 => "FT8",
            ModeKind::Ft4 => "FT4",
            // FST4/FST4W name themselves by period the way WSJT-X does: "FST4-60",
            // "FST4W-300". The bare family name would not say which slot clock the
            // operator is on, and FST4W at 120 s vs 900 s are different operating
            // decisions.
            ModeKind::Fst4 { period_s, wspr } => Self::fst4_name(period_s, wspr),
            // Period AND submode letter: "Q65" alone does not identify a signal on
            // the air, and an operator reading the label needs both. A table
            // rather than a format! so this can stay &'static str — 25 entries is
            // small, and an unsupported combination falls back to the bare family
            // name rather than pretending to a precision it does not have.
            ModeKind::Q65 { period_s, submode } => Self::q65_name(period_s, submode),
            ModeKind::Msk144 { period_s } => Self::msk144_name(period_s),
            ModeKind::Wspr => "WSPR",
            ModeKind::Jt65 { submode } => match submode {
                0 => "JT65A",
                1 => "JT65B",
                2 => "JT65C",
                _ => "JT65",
            },
            ModeKind::TempoFast => "TempoFast",
        }
    }

    /// Transmit/receive slot length in seconds (FT8 = 15, FT4 = 7.5, FT1 = 4).
    pub fn slot_secs(self) -> f32 {
        match self {
            ModeKind::Ft8 => 15.0,
            ModeKind::Ft4 => 7.5,
            // FST4 supports 15/30/60/120/300/900/1800 s upstream; the C ABI pins 15.
            ModeKind::Fst4 { period_s, .. } => f32::from(period_s),
            // The whole reason the period lives in the kind: slot timing follows it.
            ModeKind::Q65 { period_s, .. } => f32::from(period_s),
            ModeKind::Msk144 { period_s } => f32::from(period_s),
            // JT65 has ONE period, unlike the other parametric modes here.
            ModeKind::Jt65 { .. } => 60.0,
            // WSPR's interval is 2 minutes; the decoder reads 114 s of it.
            ModeKind::Wspr => f32::from(wspr::PERIOD_S),
            ModeKind::TempoFast => 4.0,
        }
    }

    /// Number of int16 samples in one DECODE frame at 12 kHz (the length the
    /// vendored decoder reads from the start of the captured window).
    pub fn frame_samples(self) -> usize {
        match self {
            ModeKind::Ft8 => ft8::NMAX,
            ModeKind::Ft4 => ft4::NMAX,
            ModeKind::Fst4 { period_s, .. } => fst4::nmax(period_s),
            // ... and so does the buffer contract: period*12000 samples.
            ModeKind::Q65 { period_s, .. } => q65::nmax(period_s),
            ModeKind::Msk144 { period_s } => msk144::nmax(period_s),
            // The full 60 s: the Fortran dummy is explicit-shape even though only
            // the first 52 s are read.
            ModeKind::Jt65 { .. } => jt65::NMAX,
            // 114 s, NOT the full 120 s interval — see the wspr crate docs.
            ModeKind::Wspr => wspr::NMAX,
            ModeKind::TempoFast => tempo_fast::NMAX,
        }
    }

    /// Number of samples to CAPTURE per slot = the full T/R period at 12 kHz. For
    /// FT8/FT1 this equals `frame_samples` (decode frame == slot); for FT4 the slot
    /// (7.5 s = 90000) is LONGER than the decode frame (6.048 s = NMAX), so the RX
    /// ring must hold the WHOLE slot — the decoder then reads its HEAD (leading
    /// Costas sync). Capturing only NMAX keeps the slot TAIL and amputates sync.
    pub fn capture_samples(self) -> usize {
        (self.slot_secs() * tempo_fast::SAMPLE_RATE) as usize
    }
}

/// Per-mode capability flags. Drives UI affordances and operating logic so the
/// generic engine need not special-case mode names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// This mode can TRANSMIT.
    ///
    /// Every mode shipped today sets this. It exists so a DECODE-ONLY mode can
    /// be added without becoming transmit-capable by accident: before this flag,
    /// `encode`/`gen_wave` were required trait methods and every [`ModeKind`]
    /// reachable from a `Tier` could key the radio by construction, so there was
    /// nothing to withhold.
    ///
    /// Enforced by [`tx_mode`], which is the only sanctioned way to obtain a mode
    /// for transmitting. Note `Default` gives `false`: a mode that forgets to
    /// declare itself is silent, which is the safe direction.
    pub tx: bool,
    /// Supports DXpedition Fox/Hound (multi-stream) operation.
    pub fox_hound: bool,
    /// Supports incremental-redundancy HARQ (cross-frame joint decode).
    pub ir_harq: bool,
    /// Supports free-text messages.
    pub free_text: bool,
    /// This mode is a **BEACON**: it transmits on a schedule and has no QSO
    /// sequence at all.
    ///
    /// Distinct from `tx`, and both flags are needed. WSPR and FST4W can transmit
    /// (`tx: true`) but there is nothing to sequence — the payload is callsign,
    /// grid and power, with no exchange and no addressing. Without this flag the
    /// auto-sequencer would happily arm a CQ run on a beacon tier and try to work
    /// the stations it hears, which is meaningless. The operating layer routes
    /// these through a transmit-percentage scheduler instead.
    pub beacon_only: bool,
    /// Has contest sub-modes / exchanges.
    pub contest: bool,
    /// This mode's STANDARD messages embed the station identity — the callsign,
    /// and for CQ/Tx1 the grid. TX must refuse to build one from a blank or
    /// invalid identity: pack77's free-text fallback happily encodes `"CQ "`,
    /// so without this gate the rig keys an unidentified transmission (§97.119).
    ///
    /// ⭐ `Default` sets this `true` — the FAIL-CLOSED direction, deliberately
    /// opposite to every other flag here. The gate used to be an `Ft8 | Ft4`
    /// allow-list in the engine, which failed OPEN for all six tiers that later
    /// gained TX; a mode that forgets to declare must GET the gate, not escape
    /// it. Free-text modes (FT1) opt out explicitly.
    pub structured_identity: bool,
    /// Runs the WSJT-X-style EARLY decode pass (decode the partial capture a few
    /// seconds before the slot boundary). FT8/FT4 only today — the other
    /// decoders take full frames.
    pub early_decode: bool,
    /// WSJT-X Split Operation applies: a TX audio offset outside 1500–2000 Hz
    /// reduces into that window with a compensating dial shift.
    pub split_reduce: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Capabilities {
            tx: false,
            fox_hound: false,
            ir_harq: false,
            free_text: false,
            beacon_only: false,
            contest: false,
            // The one deliberate `true`: identity-gating fails CLOSED — see the
            // field doc. Everything else defaults to "can't", which is safe.
            structured_identity: true,
            early_decode: false,
            split_reduce: false,
        }
    }
}

/// A weak-signal digital mode: the unit of pluggability for the whole app.
///
/// Implementors delegate to their modem crate (`ft8`/`ft4`/`ft1`). The trait is
/// object-safe so the engine can hold a `Box<dyn Mode>` and swap it at runtime.
pub trait Mode: Send + Sync {
    /// Which concrete mode this is.
    fn kind(&self) -> ModeKind;

    /// Short display name (defaults to [`ModeKind::as_str`]).
    fn name(&self) -> &'static str {
        self.kind().as_str()
    }

    /// Transmit/receive slot length in seconds (delegates to [`ModeKind`]).
    fn slot_secs(&self) -> f32 {
        self.kind().slot_secs()
    }

    /// Number of int16 samples in one decode frame at 12 kHz (delegates to
    /// [`ModeKind`]).
    fn frame_samples(&self) -> usize {
        self.kind().frame_samples()
    }

    /// Audio passband `(lo, hi)` in Hz for the waterfall and decode search.
    fn passband(&self) -> (f32, f32) {
        (200.0, 2900.0)
    }

    /// Capability flags for this mode.
    fn capabilities(&self) -> Capabilities;

    /// Encode a message (≤ 37 chars) into channel tones; empty on bad input.
    ///
    /// Defaults to empty so a DECODE-ONLY mode need not implement it. A mode that
    /// leaves this defaulted must also report `Capabilities { tx: false, .. }`;
    /// [`tx_mode`] is what keeps the two consistent, by refusing to hand out a
    /// mode for transmitting unless it declared `tx`.
    fn encode(&self, _msg: &str) -> Vec<i32> {
        Vec::new()
    }

    /// Synthesize the TX audio waveform for the given tones at carrier `f0`. The
    /// returned buffer is **slot-positioned** — it includes the mode's leading silence
    /// (FT8/FT4 start 0.5 s into the slot) so the radio loop can play it straight at the
    /// slot boundary without the over going out early.
    ///
    /// Defaults to empty for the same reason as [`Mode::encode`]. An empty wave is
    /// also the safe failure: the radio loop sizes its PTT hold from the returned
    /// buffer length, so nothing is keyed.
    fn gen_wave(&self, _itone: &[i32], _fsample: f32, _f0: f32) -> Vec<f32> {
        Vec::new()
    }

    /// Decode every signal in a [`frame_samples`](Mode::frame_samples)-long
    /// int16 frame at 12 kHz. `nfa..=nfb` is the audio search range; `ndepth`
    /// the decode aggressiveness (≤ 0 ⇒ 3); `mycall`/`hiscall` enable a-priori
    /// decoding (pass `""` if unknown). `nfqso` is the QSO/RX audio frequency
    /// (Hz) being worked — WSJT-X's nfqso, which centers the deep AP passes and
    /// sync (FT8/FT4); pass 0 / out-of-band for band-center. `frame_time_ms` is a
    /// monotonic timestamp for this frame, used by modes with cross-frame IR-HARQ
    /// (FT1). `ap` is WSJT-X "Enable AP" (FT8 only — the vendored FT4 decoder
    /// has no such flag) and `ap_cq_only` restricts AP to the CQ hypothesis
    /// (FT8 + FT4); `(true, false)` is stock. Modes without these ignore the
    /// respective argument.
    #[allow(clippy::too_many_arguments)] // mirrors the modem decode ABI
    fn decode_frame(
        &self,
        iwave: &[i16],
        nfa: i32,
        nfb: i32,
        ndepth: i32,
        mycall: &str,
        hiscall: &str,
        nqso_progress: i32,
        nfqso: i32,
        frame_time_ms: i64,
        ap: bool,
        ap_cq_only: bool,
    ) -> Vec<Decode>;

    /// [`decode_frame`](Mode::decode_frame) plus the cross-cycle a-priori flag
    /// and the two WSJT-X pass-shape inputs.
    ///
    /// `a7_final` marks the authoritative full-audio (slot-boundary) pass: for
    /// FT8 it gates WSJT-X's a7 cross-cycle replay (iaptype=7), which recovers
    /// QSO continuations remembered from the previous same-parity slot (keyed
    /// on `frame_time_ms`). The early partial pass sets it `false` (slot
    /// bookkeeping only).
    ///
    /// `nftx` is the operator's TX audio offset — WSJT-X's `nftx`
    /// (`mainwindow.cpp:3722`), the SECOND deep-AP window alongside `nfqso`
    /// (`ft8b.f90:305`). Pass `nfqso` when RX and TX are not split.
    ///
    /// `partial` marks WSJT-X's early decode (`nzhsym = 41`,
    /// `mainwindow.cpp:1878`), which upstream runs deliberately cheap — a higher
    /// sync floor and no AP passes — because the authoritative pass re-decodes
    /// the same audio moments later. It is NOT `!a7_final`: the F6 review
    /// re-decode is also `a7_final = false` but runs over the retained FULL
    /// audio, and must keep its AP passes.
    ///
    /// Modes without a cross-cycle path ignore all three — this default
    /// delegates to [`decode_frame`](Mode::decode_frame).
    #[allow(clippy::too_many_arguments)] // mirrors the modem decode ABI
    fn decode_frame_a7(
        &self,
        iwave: &[i16],
        nfa: i32,
        nfb: i32,
        ndepth: i32,
        mycall: &str,
        hiscall: &str,
        nqso_progress: i32,
        nfqso: i32,
        _nftx: i32,
        frame_time_ms: i64,
        _a7_final: bool,
        _partial: bool,
        ap: bool,
        ap_cq_only: bool,
    ) -> Vec<Decode> {
        self.decode_frame(
            iwave,
            nfa,
            nfb,
            ndepth,
            mycall,
            hiscall,
            nqso_progress,
            nfqso,
            frame_time_ms,
            ap,
            ap_cq_only,
        )
    }
}

/// Build a boxed [`Mode`] from its [`ModeKind`].
pub fn make_mode(kind: ModeKind) -> Box<dyn Mode> {
    match kind {
        ModeKind::Ft8 => Box::new(Ft8Mode),
        ModeKind::Ft4 => Box::new(Ft4Mode),
        ModeKind::Fst4 { period_s, wspr } => Box::new(Fst4Mode { period_s, wspr }),
        ModeKind::Q65 { period_s, submode } => Box::new(Q65Mode { period_s, submode }),
        ModeKind::Msk144 { period_s } => Box::new(Msk144Mode { period_s }),
        ModeKind::Jt65 { submode } => Box::new(Jt65Mode { submode }),
        ModeKind::Wspr => Box::new(WsprMode),
        ModeKind::TempoFast => Box::new(Ft1Mode),
    }
}

/// Build a mode **for transmitting**, or `None` if the mode cannot transmit.
///
/// The only sanctioned path to a mode that will be handed to `encode`/`gen_wave`.
/// [`make_mode`] stays the RX/general constructor and is unrestricted.
///
/// This exists so a decode-only mode can ship. Previously `encode` and `gen_wave`
/// were required trait methods with no default, so every [`ModeKind`] reachable
/// from a `Tier` was transmit-capable by construction and "RX-only" was not
/// expressible — there was nothing to withhold. Adding a receive-only mode without
/// this would have made it silently keyable.
///
/// It can only ever PREVENT keying: it does not alter timing, waveform, or the
/// decision to transmit, and a mode declaring `tx: true` is returned unchanged.
pub fn tx_mode(kind: ModeKind) -> Option<Box<dyn Mode>> {
    let m = make_mode(kind);
    if m.capabilities().tx {
        Some(m)
    } else {
        None
    }
}

/// Standard WSJT-X **FT8** — 15 s T/R, 8-GFSK, the dominant HF digital mode.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ft8Mode;

impl Mode for Ft8Mode {
    fn kind(&self) -> ModeKind {
        ModeKind::Ft8
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            tx: true,
            fox_hound: true,
            ir_harq: false,
            free_text: true,
            contest: true,
            // A QSO mode: it has an exchange to sequence.
            beacon_only: false,
            structured_identity: true,
            early_decode: true,
            split_reduce: true,
        }
    }
    fn encode(&self, msg: &str) -> Vec<i32> {
        ft8::encode(msg)
    }
    fn gen_wave(&self, itone: &[i32], fsample: f32, f0: f32) -> Vec<f32> {
        // WSJT-X positions FT8 tones 0.5 s into the slot (the decoder's `xdt = t − 0.5`).
        // `ft8::gen_wave` returns only the bare 12.64 s tone stream, so we PREPEND the
        // 0.5 s lead-in here to return a slot-positioned waveform (the same contract FT4
        // already satisfies). Without it the radio loop plays the tones at the slot
        // boundary and the whole over goes out 0.5 s early — every receiver sees us at
        // DT ≈ −0.5 s, off-nominal and at the edge of the decode window.
        let lead = (0.5 * fsample).round().max(0.0) as usize;
        let tones = ft8::gen_wave(itone, fsample, f0);
        let mut wave = vec![0f32; lead + tones.len()];
        wave[lead..].copy_from_slice(&tones);
        wave
    }
    fn decode_frame(
        &self,
        iwave: &[i16],
        nfa: i32,
        nfb: i32,
        ndepth: i32,
        mycall: &str,
        hiscall: &str,
        nqso_progress: i32,
        nfqso: i32,
        _frame_time_ms: i64, // a7-inert legacy path (constant slot key)
        ap: bool,
        ap_cq_only: bool,
    ) -> Vec<Decode> {
        ft8::decode_frame(
            iwave,
            nfa,
            nfb,
            ndepth,
            mycall,
            hiscall,
            nqso_progress,
            nfqso,
            ap,
            ap_cq_only,
        )
        .into_iter()
        .map(Into::into)
        .collect()
    }
    fn decode_frame_a7(
        &self,
        iwave: &[i16],
        nfa: i32,
        nfb: i32,
        ndepth: i32,
        mycall: &str,
        hiscall: &str,
        nqso_progress: i32,
        nfqso: i32,
        nftx: i32,
        frame_time_ms: i64,
        a7_final: bool,
        partial: bool,
        ap: bool,
        ap_cq_only: bool,
    ) -> Vec<Decode> {
        // a7 slot key = slot UTC seconds-of-day. frame_time_ms is slot*15000 for
        // FT8, so /1000 gives slot seconds; rem_euclid keeps i32 valid past 2038
        // and preserves parity ((slot*15 mod 86400)/5 mod 2 == slot % 2, since
        // 86400 is an even multiple of 15). The engine calls a7_reset (via
        // modes::reset_ft8_a7) on band/QSO change.
        let nutc = (frame_time_ms / 1000).rem_euclid(86_400) as i32;
        ft8::decode_frame_a7(
            iwave,
            nfa,
            nfb,
            ndepth,
            mycall,
            hiscall,
            nqso_progress,
            nfqso,
            nftx,
            nutc,
            a7_final,
            partial,
            ap,
            ap_cq_only,
        )
        .into_iter()
        .map(Into::into)
        .collect()
    }
}

/// Standard WSJT-X **FT4** — 7.5 s T/R, 4-GFSK, the fast contest sibling.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ft4Mode;

impl Mode for Ft4Mode {
    fn kind(&self) -> ModeKind {
        ModeKind::Ft4
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            tx: true,
            fox_hound: false,
            ir_harq: false,
            free_text: true,
            contest: true,
            // A QSO mode: it has an exchange to sequence.
            beacon_only: false,
            structured_identity: true,
            early_decode: true,
            split_reduce: true,
        }
    }
    fn encode(&self, msg: &str) -> Vec<i32> {
        ft4::encode(msg)
    }
    fn gen_wave(&self, itone: &[i32], fsample: f32, f0: f32) -> Vec<f32> {
        // Same slot-positioning contract as FT8: WSJT-X's FT4 decoder reports
        // `xdt = t − 0.5` (ft4_decode.f90), i.e. the nominal signal start is
        // 0.5 s into the period — but `ft4::gen_wave` places the tones at t ≈ 0
        // (measured: signal 0.00–5.04 s of the 6.048 s buffer). Played at the
        // boundary that put every transmission at DT ≈ −0.5 for the whole band.
        // Prepend the lead-in so our FT4 goes out at the standard DT ≈ 0.
        let lead = (0.5 * fsample).round().max(0.0) as usize;
        let tones = ft4::gen_wave(itone, fsample, f0);
        let mut wave = vec![0f32; lead + tones.len()];
        wave[lead..].copy_from_slice(&tones);
        wave
    }
    fn decode_frame(
        &self,
        iwave: &[i16],
        nfa: i32,
        nfb: i32,
        ndepth: i32,
        mycall: &str,
        hiscall: &str,
        nqso_progress: i32,
        nfqso: i32,
        _frame_time_ms: i64, // FT4 has no cross-frame IR-HARQ
        _ap: bool,           // HONESTLY ignored: the vendored FT4 decoder has no AP
        // on/off flag — AP runs whenever ndepth > 1 (ft4_decode.f90). Wiring
        // this would be a placebo; the UI documents the FT8-only scope.
        ap_cq_only: bool,
    ) -> Vec<Decode> {
        ft4::decode_frame(
            iwave,
            nfa,
            nfb,
            ndepth,
            mycall,
            hiscall,
            nqso_progress,
            nfqso,
            ap_cq_only,
        )
        .into_iter()
        .map(Into::into)
        .collect()
    }
}

/// Slot lead-in for FT1, in seconds — how far into its 4 s T/R period the tones start.
///
/// Exists because FT1's decoder clamps its timing search at `istart >= 0` (see
/// `Ft1Mode::gen_wave`), so a signal at t=0 has no early-side margin at all. 0.4 s buys that
/// margin while still fitting inside `gen_wave`'s fixed 48000-sample buffer, whose 0.464 s of
/// trailing zeros is the entire budget available to shift into.
/// FT1's slot lead-in — the ONLY early-timing margin the mode has, and half of a fixed budget.
///
/// The buffer is 4.6432 s of room holding 3.5357 s of tones, so lead and post-tone tail come out
/// of one 464 ms allowance and trade one-for-one:
///
/// ```text
///   early tolerance = the lead          (the decoder's istart clamps at 0)
///   late tolerance  = ~900 ms - lead    (the coarse sweep's top)
///   post-tone tail  = 464 ms - lead     (what survives soundcard output latency)
/// ```
///
/// **0.300 s, chosen on measurement (`crates/tempo-fast/tests/ft1_lead_sweep.rs`, operator
/// decision 2026-08-05).** It was 0.400 s, which left a 64 ms tail — and because the PTT hold is
/// clamped to the slot boundary and the device output ring lags the buffer by the soundcard's
/// latency, ANY output latency above ~64 ms cut FT1's RF mid-tone. 20-100 ms cpal buffers are
/// ordinary, so the shipped value was inside the failure region on real hardware.
///
/// The sweep (12 trials/cell, AWGN, -4/-8/-12 dB) shows early tolerance tracks the lead to the
/// millisecond and is a CLIFF, not a slope — flat at 10-12/12 up to -lead, then 0 at the next step
/// out, because `tempofast_decode.f90` cannot search before sample 0. The shape is identical at
/// all three SNRs, so it is timing and not sensitivity:
///
/// ```text
///   lead  tail |  -450 -400 -350 -300 -250 -200 -150  -100    0  +100  +250  +400
///    150   314 |     0    0    0    0    0    0   12    11   10    12    11    12
///    250   214 |     0    0    0    0   12   11   11    10   12    12    11    11
///    300   164 |     0    0    0   12   11   11   10    12   12    11    12    12   <- here
///    400    64 |     0   12   11   11   10   12   12    12   11    11    11    11
/// ```
///
/// So this costs 100 ms of early tolerance (a peer more than 300 ms fast is now lost, where 400 ms
/// was tolerated) and buys 100 ms of tail — 164 ms, comfortably clear of any normal audio buffer.
/// It also IMPROVES the late side, which is `900 ms - lead`: at -12 dB the old 400 ms lead already
/// fell to 5/12 at +400 ms where 300 ms holds 6/12 and shorter leads do better still.
///
/// ⚠️ DO NOT RAISE THIS WITHOUT RE-READING THE TAIL. Every millisecond added here is taken from
/// the margin protecting the end of the signal, and that failure is invisible in a loopback test —
/// it only appears on hardware with real output latency.
pub(crate) const FT1_LEAD_IN_SECS: f32 = 0.3;

/// **FT1** (KD9TAW) — 4 s T/R, 4-CPM turbo, with IR-HARQ. Tempo's native mode.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ft1Mode;

impl Mode for Ft1Mode {
    fn kind(&self) -> ModeKind {
        ModeKind::TempoFast
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            tx: true,
            fox_hound: false,
            ir_harq: true,
            free_text: true,
            contest: true,
            // A QSO mode: it has an exchange to sequence.
            beacon_only: false,
            // The one deliberate opt-OUT: TempoFast is a free-text chat mode —
            // its frames are not grid-bound and carry no standard CQ/Tx1 shape.
            structured_identity: false,
            early_decode: false,
            split_reduce: false,
        }
    }
    fn encode(&self, msg: &str) -> Vec<i32> {
        tempo_fast::encode(msg)
    }
    fn gen_wave(&self, itone: &[i32], fsample: f32, f0: f32) -> Vec<f32> {
        // ⚠️ SLOT LEAD-IN — an on-air fix, not cosmetics. Read this before removing it.
        //
        // FT1's decoder cannot search for an EARLY signal at all. `tempofast_decode.f90`'s
        // coarse sweep is `do istart=0,200,4` and its fine pass is
        // `do istart=max(0,ibest_all-5),…` — both hard-clamped at zero. The two modes that work
        // both search negative: FT8's `sync8.f90` declares `sync2d(NH1,-JZ:JZ)` and sweeps
        // `do j=-JZ,+JZ` about a nominal +0.5 s start, and FT4's `ft4_decode.f90` starts at
        // `ibmin=-344`.
        //
        // Until now FT1 alone returned the raw waveform, putting its first symbol at sample 0 —
        // sitting exactly ON that clamp, with ZERO margin on the early side. Any ordinary
        // timing error in the early direction (a peer's PC a few hundred ms off UTC, audio-clock
        // rate error, keying jitter) fell straight off a cliff: measured 0/5 decodes at just
        // −50 ms early, against 5/5 at 0 and 5/5 out to +500 ms.
        //
        // On air (KD9TAW ↔ N9UM, 6 m, 2026-07-26) that lost roughly half of all frames in each
        // direction — enough that single-frame messages got through and multi-frame ones never
        // reassembled. FT8 was unaffected on the same radios, because its symmetric search
        // absorbs the same error.
        //
        // NOT the 0.5 s FT8/FT4 use: `tempo_fast::gen_wave` returns a FIXED NMAX = 48000-sample
        // (4.000 s) buffer holding 3.536 s of tones plus 0.464 s of tail zeros, and the lead is
        // shifted WITHIN that buffer rather than prepended — which keeps the length at exactly
        // 4.000 s. That matters because the PTT hold is sized from `w.len()`
        // (`tempo-audio/src/slot.rs`), so a longer buffer would not buy time, it would only push
        // the clamp. See `FT1_LEAD_IN_SECS` for why the value is 0.300 and what it trades.
        let tones = tempo_fast::gen_wave(itone, fsample, f0);
        let lead = (FT1_LEAD_IN_SECS * fsample).round().max(0.0) as usize;
        if lead == 0 || lead >= tones.len() {
            return tones;
        }
        let keep = tones.len() - lead;
        debug_assert!(
            tones[keep..].iter().all(|&s| s == 0.0),
            "the lead-in shift must only ever push trailing SILENCE off the end — if this trips, \
             gen_wave's tail-zero budget shrank and the signal is being clipped"
        );
        let mut wave = vec![0f32; tones.len()];
        wave[lead..].copy_from_slice(&tones[..keep]);
        wave
    }
    fn decode_frame(
        &self,
        iwave: &[i16],
        nfa: i32,
        nfb: i32,
        ndepth: i32,
        mycall: &str,
        hiscall: &str,
        nqso_progress: i32,
        _nfqso: i32, // FT1 uses IR-HARQ, not WSJT-X nfqso-windowed AP
        frame_time_ms: i64,
        _ap: bool, // WSJT-X FT8 AP control — FT1 has its own IR-HARQ, no WSJT-X AP
        _ap_cq_only: bool,
    ) -> Vec<Decode> {
        // FT1's decoder keys cross-frame IR-HARQ combining off frame_time_ms; the
        // caller resets HARQ buffers (tempo_fast::harq_reset) on band/QSO change.
        tempo_fast::decode_frame(
            iwave,
            nfa,
            nfb,
            ndepth,
            mycall,
            hiscall,
            nqso_progress,
            frame_time_ms,
        )
        .into_iter()
        .map(Into::into)
        .collect()
    }
}

#[cfg(test)]
mod tx_capability_tests {
    use super::*;

    /// A receive-only mode: implements decode, leaves `encode`/`gen_wave` defaulted,
    /// and declares `tx: false`. This is the shape FST4 will take when it lands
    /// RX-only, so the guard is tested against the real intended use rather than a
    /// hypothetical.
    struct RxOnlyMode;

    impl Mode for RxOnlyMode {
        fn kind(&self) -> ModeKind {
            ModeKind::Ft8 // borrowed identity; only capabilities matter here
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                tx: false,
                ..Default::default()
            }
        }
        #[allow(clippy::too_many_arguments)]
        fn decode_frame(
            &self,
            _iwave: &[i16],
            _nfa: i32,
            _nfb: i32,
            _ndepth: i32,
            _mycall: &str,
            _hiscall: &str,
            _nqso_progress: i32,
            _nfqso: i32,
            _frame_time_ms: i64,
            _ap: bool,
            _ap_cq_only: bool,
        ) -> Vec<Decode> {
            Vec::new()
        }
    }

    /// Modes that ship RECEIVE-ONLY. Adding to this list is the deliberate act
    /// that makes a mode silent; a mode that is silent WITHOUT being listed here
    /// fails `tx_capability_is_declared_not_inherited` below.
    /// The modes that cannot transmit.
    ///
    /// A predicate rather than a const slice: the data-carrying kinds would need
    /// every combination listed and would silently miss any that were forgotten.
    /// `matches!` covers a whole family at once.
    ///
    /// Shrinking as encoders land: Q65, then FST4, then the two BEACONS (WSPR and
    /// FST4W) once the transmit-percentage scheduler existed, then MSK144, then
    /// JT65. Beacons are `tx: true` AND `beacon_only: true` — transmit-capable, but
    /// never handed to the QSO sequencer.
    fn rx_only(_kind: ModeKind) -> bool {
        // EMPTY — every shipped mode transmits. JT65 was listed here in 0.19.17
        // alone, as a mitigation for a Windows crash fixed in 0.19.18 (xcor.f90).
        // Kept as a predicate, not deleted: it is the deliberate act that makes the
        // next receive-only mode silent, and the two-sided test below enforces it.
        false
    }

    #[test]
    fn tx_capability_is_declared_not_inherited() {
        // The original form of this test asserted every shipped mode declares tx.
        // FST4 is the first that does not, and the test failing on it was the
        // intended behaviour — a silent mode must not be addable by accident. The
        // invariant is now two-sided: RX_ONLY is exactly the set that cannot
        // transmit, and every other mode can.
        for kind in ModeKind::ALL
            .into_iter()
            .chain(ModeKind::q65_all())
            .chain(ModeKind::fst4_all())
            .chain(ModeKind::msk144_all())
            .chain(ModeKind::jt65_all())
            .chain(std::iter::once(ModeKind::Wspr))
        {
            let rx_only = rx_only(kind);
            let caps_tx = make_mode(kind).capabilities().tx;
            assert_eq!(
                caps_tx,
                !rx_only,
                "{} declares tx={caps_tx} but rx_only() says rx_only={rx_only} — \
                 update rx_only() deliberately, or fix the mode's Capabilities",
                kind.as_str()
            );
            assert_eq!(
                tx_mode(kind).is_some(),
                !rx_only,
                "tx_mode({}) disagrees with its own Capabilities.tx",
                kind.as_str()
            );
        }
    }

    #[test]
    fn fst4_and_fst4w_both_transmit_but_only_one_is_a_qso_mode() {
        // FST4 and FST4W share a ModeKind and DIVERGE on `beacon_only`, which is the
        // distinction most likely to be lost by a change to either. Both halves
        // pinned. `tx` alone is NOT enough to describe them: both key the radio, but
        // only FST4 has an exchange for the sequencer to run.
        for kind in ModeKind::fst4_all() {
            let label = kind.as_str();
            let wspr = matches!(kind, ModeKind::Fst4 { wspr: true, .. });
            let m = make_mode(kind);
            let caps = m.capabilities();

            assert!(caps.tx, "{label} should transmit");
            assert!(tx_mode(kind).is_some(), "tx_mode must allow {label}");
            assert_eq!(
                caps.beacon_only, wspr,
                "{label}: beacon_only must follow the wspr flag"
            );
            assert_eq!(
                caps.free_text, !wspr,
                "{label}: the 50-bit beacon payload has no free-text form"
            );

            let msg = if wspr {
                "KD9TAW EN52 30"
            } else {
                "CQ KD9TAW EN52"
            };
            let tones = m.encode(msg);
            assert_eq!(tones.len(), fst4::NN, "{label} encodes 160 symbols");
            assert!(
                tones.iter().all(|&t| (0..=3).contains(&t)),
                "{label} is 4-FSK: every tone must be 0..3"
            );
        }
    }

    #[test]
    fn the_two_fst4_codes_produce_different_symbols_for_the_same_period() {
        // `iwspr` selects the LDPC code — (240,101)/77-bit vs (240,74)/50-bit — and
        // BOTH emit 160 symbols, so a mix-up transmits a well-formed frame the far
        // end cannot read. Nothing about the length would catch it.
        let qso = make_mode(ModeKind::Fst4 {
            period_s: 60,
            wspr: false,
        })
        .encode("CQ KD9TAW EN52");
        let beacon = make_mode(ModeKind::Fst4 {
            period_s: 60,
            wspr: true,
        })
        .encode("KD9TAW EN52 30");
        assert_eq!(qso.len(), beacon.len());
        assert_ne!(qso, beacon, "the two codes must not coincide");
    }

    #[test]
    fn every_beacon_mode_declares_itself_one_and_no_qso_mode_does() {
        // The flag exists so the auto-sequencer can never arm a CQ run on a mode
        // with no exchange. Assert the whole set both ways rather than spot-checks.
        for kind in ModeKind::ALL
            .into_iter()
            .chain(ModeKind::q65_all())
            .chain(ModeKind::fst4_all())
            .chain(ModeKind::msk144_all())
            .chain(ModeKind::jt65_all())
        {
            let caps = make_mode(kind).capabilities();
            let expect_beacon = matches!(kind, ModeKind::Wspr | ModeKind::Fst4 { wspr: true, .. });
            assert_eq!(
                caps.beacon_only,
                expect_beacon,
                "{} declares beacon_only={}",
                kind.as_str(),
                caps.beacon_only
            );
            if caps.beacon_only {
                assert!(
                    caps.tx,
                    "{}: a beacon must be able to transmit",
                    kind.as_str()
                );
                assert!(
                    !caps.free_text,
                    "{}: a beacon payload carries call/grid/power only",
                    kind.as_str()
                );
            }
        }
    }

    #[test]
    fn the_ft1_lead_and_tail_are_what_the_slot_clamp_assumes() {
        // ⭐ THE GEOMETRY GUARD, measured off the REAL waveform rather than restated from a
        // constant. `tempo-audio/src/slot.rs` sizes FT1's PTT hold from `w.len()` and asserts a
        // ~164 ms post-tone tail using its own mirrored copy of the lead — so a change to
        // `FT1_LEAD_IN_SECS` here, without a matching change there, would leave both files
        // internally consistent and both wrong. This test is what makes that impossible: it
        // reads where the tones actually start and stop in the buffer that goes on the air.
        //
        // The tail is not cosmetic. The PTT hold is clamped to the slot boundary and the device
        // output ring lags the buffer by the soundcard's latency, so the tail IS the margin
        // protecting the end of the signal. At the former 400 ms lead it was 64 ms, inside the
        // failure region for ordinary 20-100 ms audio buffers (operator decision 2026-08-05;
        // the trade is measured in crates/tempo-fast/tests/ft1_lead_sweep.rs).
        let m = Ft1Mode;
        let w = m.gen_wave(&tempo_fast::encode("CQ W9XYZ EN37"), 12_000.0, 1500.0);
        assert_eq!(w.len(), 48_000, "FT1's buffer must stay exactly 4.000 s");

        let first = w
            .iter()
            .position(|&v| v != 0.0)
            .expect("the wave has tones");
        let last = w
            .iter()
            .rposition(|&v| v != 0.0)
            .expect("the wave has tones");
        let lead_ms = first as f64 / 12.0;
        let tail_ms = (w.len() - 1 - last) as f64 / 12.0;

        assert!(
            (lead_ms - 300.0).abs() < 1.0,
            "FT1's tones must start ~300 ms into the buffer (the early-timing margin), got \
             {lead_ms:.1} ms"
        );
        assert!(
            (tail_ms - 164.0).abs() < 2.0,
            "FT1 must keep ~164 ms of tail after the tones — this is the soundcard-latency \
             margin `slot.rs` asserts and the reason the lead was shortened; got {tail_ms:.1} ms"
        );
        // And the pair must still fit the fixed frame, which is the constraint that makes this
        // a trade at all: lead + tones + tail == 4.000 s, always.
        assert!(
            lead_ms + tail_ms < 470.0,
            "lead + tail must fit the 464 ms the tones leave; got {:.1} ms",
            lead_ms + tail_ms
        );
    }

    #[test]
    fn a_malformed_tone_vector_never_reaches_the_air() {
        // The defaulted-trait backstop applies to transmit-capable modes too: a
        // caller handing gen_wave the wrong symbol count must get silence, not a
        // partial over. 3 tones is not an FST4 frame.
        for kind in ModeKind::fst4_all().filter(|k| !matches!(k, ModeKind::Fst4 { wspr: true, .. }))
        {
            let m = make_mode(kind);
            assert!(
                m.gen_wave(&[1, 2, 3], 12000.0, 1500.0).is_empty(),
                "{} produced a waveform from 3 tones",
                kind.as_str()
            );
        }
    }

    #[test]
    fn rx_only_mode_cannot_produce_a_waveform() {
        let m = RxOnlyMode;
        assert!(!m.capabilities().tx);
        // The defaulted trait methods are the backstop if a caller bypasses tx_mode:
        // an empty wave keys nothing, because the radio loop sizes its PTT hold from
        // the returned buffer length.
        assert!(m.encode("CQ KD9TAW EN52").is_empty());
        assert!(m.gen_wave(&[1, 2, 3], 12000.0, 1500.0).is_empty());
    }

    #[test]
    fn tx_mode_is_the_gate_not_make_mode() {
        // make_mode stays unrestricted (RX/general construction); tx_mode is what
        // enforces the capability. Both must remain true for the split to mean
        // anything.
        for kind in ModeKind::ALL
            .into_iter()
            .chain(ModeKind::q65_all())
            .chain(ModeKind::fst4_all())
            .chain(ModeKind::msk144_all())
            .chain(ModeKind::jt65_all())
            .chain(std::iter::once(ModeKind::Wspr))
        {
            let _ = make_mode(kind); // never refuses
        }
        assert!(!RxOnlyMode.capabilities().tx);
    }
}

/// WSJT-X **FST4** — 15 s T/R, 4-GFSK, LDPC(240,101)+CRC-24. **RECEIVE-ONLY.**
///
/// The first mode in this tree that cannot transmit. `encode`/`gen_wave` are left
/// defaulted (empty) and `Capabilities.tx` is false, so [`tx_mode`] refuses to hand
/// it to the transmit path. That is enforced, not conventional: `engine.rs`'s wave
/// builder goes through `tx_mode` and abandons the over when it returns `None`.
///
/// Upstream FST4 offers 15/30/60/120/300/900/1800 s periods. The C ABI pins 15 s
/// because `fst4_decode` sizes its frame from the period, so a selectable period
/// needs both a per-period entry point and a `ModeKind` that can carry one.
#[derive(Debug, Clone, Copy)]
pub struct Fst4Mode {
    /// T/R period in seconds: 15, 30, 60, 120, 300, 900 or 1800.
    pub period_s: u16,
    /// FST4W beacon mode (50-bit, no AP) instead of FST4's QSO mode.
    pub wspr: bool,
}

impl Default for Fst4Mode {
    /// FST4 at 15 s — the combination the C ABI was originally pinned to.
    fn default() -> Self {
        Self {
            period_s: 15,
            wspr: false,
        }
    }
}

impl Mode for Fst4Mode {
    fn kind(&self) -> ModeKind {
        ModeKind::Fst4 {
            period_s: self.period_s,
            wspr: self.wspr,
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            tx: true,
            fox_hound: false,
            ir_harq: false,
            // FST4W's 50-bit beacon payload has no free-text form.
            free_text: !self.wspr,
            contest: false,
            // ⭐ SPLIT ON `wspr` — the reason FST4 and FST4W share a ModeKind but
            // not a behaviour. FST4 is a QSO mode with a full exchange; FST4W is a
            // beacon carrying callsign/grid/power on a schedule. Both transmit; only
            // one has anything for the sequencer to do.
            beacon_only: self.wspr,
            // Both shapes embed callsign+grid — identity-gated either way.
            structured_identity: true,
            early_decode: false,
            split_reduce: false,
        }
    }

    fn encode(&self, msg: &str) -> Vec<i32> {
        // `iwspr` selects the LDPC CODE, not just the message shape — (240,101)
        // with a 77-bit payload vs (240,74) with the 50-bit beacon payload. Both
        // yield 160 symbols, so passing the wrong one transmits a well-formed frame
        // the far end cannot read.
        fst4::encode(msg, self.wspr).unwrap_or_default()
    }

    fn gen_wave(&self, itone: &[i32], fsample: f32, f0: f32) -> Vec<f32> {
        // hmod = 1: upstream's x2/x4 Tone Spacing is a config option for unusual
        // paths, not a default. Exposing it means a settings field and a matching
        // RX-side choice, so it stays at standard spacing until asked for.
        let Some(tones) = fst4::gen_wave(itone, self.period_s, 1, fsample, f0) else {
            return Vec::new();
        };
        // Slot-positioned per the trait contract. 1.0 s at every period EXCEPT
        // FST4-15, which starts at 0.5 s — `fst4::lead_in_secs` carries the two
        // upstream citations. A flat 1.0 s here still decodes, just 0.5 s late.
        let lead = (fst4::lead_in_secs(self.period_s) * fsample)
            .round()
            .max(0.0) as usize;
        let mut wave = vec![0f32; lead + tones.len()];
        wave[lead..].copy_from_slice(&tones);
        wave
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_frame(
        &self,
        iwave: &[i16],
        nfa: i32,
        nfb: i32,
        ndepth: i32,
        mycall: &str,
        hiscall: &str,
        nqso_progress: i32,
        nfqso: i32,
        _frame_time_ms: i64, // FST4 has no cross-frame state (no a7, no IR-HARQ)
        _ap: bool,           // FT8/FT4 operator AP controls — this wrapper's C ABI pins its own
        _ap_cq_only: bool,
    ) -> Vec<Decode> {
        fst4::decode_frame(
            iwave,
            self.period_s,
            self.wspr,
            nfa,
            nfb,
            ndepth,
            mycall,
            hiscall,
            nqso_progress,
            nfqso,
        )
        .into_iter()
        .map(Into::into)
        .collect()
    }
}

/// WSJT-X **Q65-30A**, receive-only.
///
/// The weak-signal mode for EME, VHF+ scatter and other Doppler-smeared paths:
/// 65-tone FSK carrying 78 bits through a Q-ary Repeat-Accumulate LDPC code over
/// GF(64). The second mode in this tree that cannot transmit — `encode`/`gen_wave`
/// are left defaulted (empty) and `Capabilities.tx` is false, so [`tx_mode`]
/// refuses to hand it to the transmit path.
///
/// Carries its T/R period and submode: all 5 × 5 combinations are reachable, and
/// both are part of the buffer contract (`frame_samples` = `period_s * 12000`).
/// EME on VHF/UHF works Q65-60A/B/C, which is why this is parametric rather than
/// pinned to the 30 s the ABI originally exposed.
#[derive(Debug, Clone, Copy)]
pub struct Q65Mode {
    /// T/R period in seconds: 15, 30, 60, 120 or 300.
    pub period_s: u16,
    /// Submode 0..=4 for A..E (tone spacing).
    pub submode: u8,
}

impl Default for Q65Mode {
    /// Q65-30A — the combination the C ABI was originally pinned to.
    fn default() -> Self {
        Self {
            period_s: 30,
            submode: 0,
        }
    }
}

impl Mode for Q65Mode {
    fn kind(&self) -> ModeKind {
        ModeKind::Q65 {
            period_s: self.period_s,
            submode: self.submode,
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            tx: true,
            fox_hound: false,
            ir_harq: false,
            free_text: true,
            // The Q65 contest path is deliberately not wired: it depends on caller
            // history that used to arrive through a file the headless build removed,
            // because that file let one chain's callers reach another.
            contest: false,
            structured_identity: true,
            early_decode: false,
            split_reduce: false,
            // A QSO mode: it has an exchange to sequence.
            beacon_only: false,
        }
    }

    fn encode(&self, msg: &str) -> Vec<i32> {
        // Empty on a message that will not pack — the trait's documented safe
        // failure, and `gen_wave` below turns it into an empty waveform, so the
        // radio loop keys nothing rather than transmitting garbage.
        q65::encode(msg).unwrap_or_default()
    }

    fn gen_wave(&self, itone: &[i32], fsample: f32, f0: f32) -> Vec<f32> {
        // Slot-positioned, per the trait contract: PREPEND the lead-in silence so
        // the radio loop can play this straight at the slot boundary.
        //
        // ⭐ The lead-in is 0.5 s at 15/30 s and 1.0 s at 60 s+ — not the flat 0.5 s
        // FT8 uses. `q65::lead_in_secs` carries the citations. Getting it wrong puts
        // every receiver's DT off by half a second at the periods EME actually runs.
        let Some(tones) = q65::gen_wave(itone, self.period_s, self.submode, fsample, f0) else {
            return Vec::new();
        };
        let lead = (q65::lead_in_secs(self.period_s) * fsample)
            .round()
            .max(0.0) as usize;
        let mut wave = vec![0f32; lead + tones.len()];
        wave[lead..].copy_from_slice(&tones);
        wave
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_frame(
        &self,
        iwave: &[i16],
        nfa: i32,
        nfb: i32,
        ndepth: i32,
        mycall: &str,
        hiscall: &str,
        nqso_progress: i32,
        nfqso: i32,
        _frame_time_ms: i64, // Q65 has no cross-frame state here: the ABI pins
        // lclearave, so message averaging never spans calls.
        _ap: bool, // FT8/FT4 operator AP controls — Q65's C ABI pins its own
        _ap_cq_only: bool,
    ) -> Vec<Decode> {
        q65::decode_frame(
            iwave,
            self.period_s,
            self.submode,
            nfa,
            nfb,
            ndepth,
            mycall,
            hiscall,
            // hisgrid: the trait's decode_frame carries no grid, and Q65 is the only
            // mode whose AP layer wants one (q65_set_list builds its candidate list
            // from mycall+hiscall+hisgrid). Passing "" costs some q3 list-decode
            // reach and costs nothing else — every other decode path is unaffected.
            // Wiring it through means widening the trait signature for all modes,
            // which is worth doing only if grid-AP measurably helps.
            "",
            nqso_progress,
            nfqso,
        )
        .into_iter()
        .map(Into::into)
        .collect()
    }
}

/// WSJT-X **MSK144** — the meteor-scatter mode. Receive-only.
///
/// 72 ms frames repeated through the period, so one ionised trail lasting a tenth
/// of a second can carry a whole message. Third receive-only mode in this tree:
/// `encode`/`gen_wave` are left defaulted (empty) and `Capabilities.tx` is false,
/// so [`tx_mode`] refuses to hand it to the transmit path.
#[derive(Debug, Clone, Copy)]
pub struct Msk144Mode {
    /// T/R period in seconds: 5, 10, 15 or 30.
    pub period_s: u16,
}

impl Default for Msk144Mode {
    /// 15 s — the period 6 m meteor scatter runs on.
    fn default() -> Self {
        Self { period_s: 15 }
    }
}

impl Mode for Msk144Mode {
    fn kind(&self) -> ModeKind {
        ModeKind::Msk144 {
            period_s: self.period_s,
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            tx: true,
            fox_hound: false,
            ir_harq: false,
            free_text: true,
            contest: false,
            structured_identity: true,
            early_decode: false,
            split_reduce: false,
            // A QSO mode: it has an exchange to sequence. ⚠️ But a METEOR-SCATTER
            // one — the far end may hear a single 72 ms frame out of ~200 and a
            // contact can take many minutes of apparent silence. The sequencer's
            // FT8-tuned patience (cq_max_calls, the stalled-QSO detector) is the
            // part that still wants operator judgement on the air.
            beacon_only: false,
        }
    }

    fn encode(&self, msg: &str) -> Vec<i32> {
        // May return 40 (MSK40 shorthand) rather than 144 — gen_wave accepts both.
        msk144::encode(msg).unwrap_or_default()
    }

    fn gen_wave(&self, itone: &[i32], fsample: f32, f0: f32) -> Vec<f32> {
        // ⭐ f0 IS IGNORED. MSK144 does not follow the operator's TX offset: the
        // signal is 1000 Hz wide (two tones at centre ±500) and fills a normal SSB
        // passband, so there is nowhere to move it. Upstream hardcodes the same
        // centre (mainwindow.cpp:12763, f0=1000 with 1000 Hz spacing → tones at
        // 1000/2000 Hz). Honouring a 2800 Hz offset here would put the upper tone
        // outside the transmit filter.
        let _ = f0;
        // The frame is repeated to fill the over — gen_wave owns that, because the
        // repetition IS the mode. No lead-in: MSK144 keys from the slot boundary
        // (msk144sim starts at sample 1), unlike Q65/FST4/WSPR.
        msk144::gen_wave(itone, self.period_s, fsample, msk144::TX_CENTRE_HZ).unwrap_or_default()
    }

    // encode() and gen_wave() are DELIBERATELY not implemented — the trait defaults
    // return empty, which is the safe failure if anything bypasses tx_mode.

    #[allow(clippy::too_many_arguments)]
    fn decode_frame(
        &self,
        iwave: &[i16],
        nfa: i32,
        nfb: i32,
        ndepth: i32,
        mycall: &str,
        hiscall: &str,
        _nqso_progress: i32,
        nfqso: i32,
        frame_time_ms: i64,
        _ap: bool, // FT8/FT4 operator AP controls — MSK144 has no AP path
        _ap_cq_only: bool,
    ) -> Vec<Decode> {
        // ⭐ frame_time_ms IS the nutc the C ABI needs. mskrtd uses it as the
        // period label: it is the UTC field of the decoder's output line and one
        // half of the duplicate-suppressor reset (`nutc00 != nutc0 || tsec <
        // tsec0`). Seconds are distinct per period at every supported period
        // (5 s minimum) and monotonic, which is all mskrtd compares.
        let nutc = (frame_time_ms / 1000) as i32;
        msk144::decode_frame(
            iwave,
            self.period_s,
            nutc,
            nfa,
            nfb,
            ndepth,
            mycall,
            hiscall,
            nfqso,
        )
        .into_iter()
        .map(Into::into)
        .collect()
    }
}

/// WSJT **JT65** — the classic weak-signal / EME mode. Receive-only.
///
/// 65-tone MFSK through a (63,12) Reed-Solomon code, carrying the LEGACY 72-bit
/// message layer (22-character decodes, not 37). One fixed 60 s T/R period, so
/// unlike FST4/Q65/MSK144 this carries only a submode.
#[derive(Debug, Clone, Copy)]
pub struct Jt65Mode {
    /// Submode 0/1/2 for A/B/C — tone spacing 1x/2x/4x.
    pub submode: u8,
}

impl Default for Jt65Mode {
    /// JT65A.
    fn default() -> Self {
        Self { submode: 0 }
    }
}

impl Mode for Jt65Mode {
    fn kind(&self) -> ModeKind {
        ModeKind::Jt65 {
            submode: self.submode,
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // TX is ON. It was disabled in 0.19.17 as a mitigation while Call CQ on
            // JT65 hard-crashed Windows with 0xC0000005; the cause is fixed in
            // 0.19.18 and the mitigation is lifted.
            //
            // The fault was never in the encoder or the TX path — it was the DECODE
            // that runs at the same slot boundary. `xcor.f90` left its `lagpk` peak-lag
            // dummy UNASSIGNED whenever the cross-correlation was everywhere zero or
            // NaN, and `sync65.f90:44` used it as an unguarded index into
            // `real ccfblue(-32:82)`. See the note in xcor.f90 for the full chain and
            // why JT65 alone was affected (sync65/xcor are JT65-only compilation
            // units — Q65-60 shares the slot and the sequencer but not this code).
            tx: true,
            fox_hound: false,
            ir_harq: false,
            // JT65's legacy packjt layer carries a 13-character free-text form.
            free_text: true,
            contest: false,
            // A QSO mode: it has an exchange to sequence.
            beacon_only: false,
            structured_identity: true,
            early_decode: false,
            split_reduce: false,
        }
    }

    fn encode(&self, msg: &str) -> Vec<i32> {
        // ⚠️ 22 characters, not 37 — JT65 predates 77-bit and uses `packjt`.
        jt65::encode(msg).unwrap_or_default()
    }

    fn gen_wave(&self, itone: &[i32], fsample: f32, f0: f32) -> Vec<f32> {
        let Some(tones) = jt65::gen_wave(itone, self.submode, fsample, f0) else {
            return Vec::new();
        };
        // Slot-positioned: 1.0 s into the minute, like the other 60 s modes.
        let lead = (jt65::LEAD_IN_SECS * fsample).round().max(0.0) as usize;
        let mut wave = vec![0f32; lead + tones.len()];
        wave[lead..].copy_from_slice(&tones);
        wave
    }

    // encode() and gen_wave() are DELIBERATELY not implemented.

    #[allow(clippy::too_many_arguments)]
    fn decode_frame(
        &self,
        iwave: &[i16],
        nfa: i32,
        nfb: i32,
        ndepth: i32,
        mycall: &str,
        hiscall: &str,
        _nqso_progress: i32,
        nfqso: i32,
        _frame_time_ms: i64, // JT65 has no cross-frame state here: clearave is pinned.
        _ap: bool,           // FT8/FT4 operator AP controls — JT65's C ABI pins its own
        _ap_cq_only: bool,
    ) -> Vec<Decode> {
        jt65::decode_frame(
            iwave,
            self.submode,
            nfa,
            nfb,
            ndepth,
            mycall,
            hiscall,
            // hisgrid: same trait limitation as Q65 — decode_frame carries no grid.
            // JT65's deep search would use it to build candidate messages; passing
            // "" narrows that path and costs nothing else.
            "",
            nfqso,
        )
        .into_iter()
        .map(Into::into)
        .collect()
    }
}

/// **WSPR** — the propagation-beacon mode. Receive-only.
///
/// The odd one out in this trait. Every other mode decodes QSO traffic and
/// reports an audio offset in Hz; WSPR decodes BEACONS and reports an absolute
/// frequency in MHz, because a spot is only meaningful with the band attached.
/// The shared [`Decode`] cannot carry that, so `freq` here is the audio offset
/// derived back from the absolute value — see the conversion in `decode.rs`.
#[derive(Debug, Clone, Copy, Default)]
pub struct WsprMode;

impl Mode for WsprMode {
    fn kind(&self) -> ModeKind {
        ModeKind::Wspr
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            tx: true,
            fox_hound: false,
            ir_harq: false,
            // WSPR's 50-bit layer carries "CALL GRID DBM" and nothing else.
            free_text: false,
            contest: false,
            // A beacon keys unattended on a schedule. `beacon_only` keeps the QSO
            // sequencer away from it — see Capabilities::beacon_only.
            beacon_only: true,
            // The payload IS the identity (published to wsprnet) — gated.
            structured_identity: true,
            early_decode: false,
            split_reduce: false,
        }
    }

    fn encode(&self, msg: &str) -> Vec<i32> {
        // `msg` must already be "CALL GRID DBM" — `wspr::message()` builds it. The
        // 50-bit payload has no other form, so anything else simply fails to encode
        // and yields the empty (silent) result.
        wspr::encode(msg)
            .map(|s| s.into_iter().map(i32::from).collect())
            .unwrap_or_default()
    }

    fn gen_wave(&self, itone: &[i32], fsample: f32, f0: f32) -> Vec<f32> {
        // Back to the u8 the synthesiser validates: any value outside 0..=3 is not
        // a WSPR symbol and must produce silence rather than a wrong tone.
        if itone.iter().any(|&t| !(0..=3).contains(&t)) {
            return Vec::new();
        }
        let sym: Vec<u8> = itone.iter().map(|&t| t as u8).collect();
        let Some(tones) = wspr::gen_wave(&sym, fsample, f0) else {
            return Vec::new();
        };
        // Slot-positioned: WSPR's modulation starts 1.0 s into the 2-minute window.
        let lead = (wspr::LEAD_IN_SECS * fsample).round().max(0.0) as usize;
        let mut wave = vec![0f32; lead + tones.len()];
        wave[lead..].copy_from_slice(&tones);
        wave
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_frame(
        &self,
        iwave: &[i16],
        _nfa: i32,
        _nfb: i32,
        ndepth: i32,
        _mycall: &str,
        _hiscall: &str,
        _nqso_progress: i32,
        _nfqso: i32,
        _frame_time_ms: i64,
        _ap: bool, // FT8/FT4 operator AP controls — WSPR has no AP path
        _ap_cq_only: bool,
    ) -> Vec<Decode> {
        // ⭐ nfa/nfb/mycall/hiscall/nfqso are IGNORED, and that is not an
        // oversight. WSPR searches its own fixed 200 Hz sub-band around the dial
        // frequency — there is no operator-selectable passband — and it has no
        // a-priori decoding, so callsigns buy nothing. Accepting them and
        // silently doing nothing would be worse than saying so here.
        //
        // dial_mhz is 0.0 because the shared Decode has nowhere to put an
        // absolute frequency; the app-layer path passes the real dial. A caller
        // using this trait method gets audio offsets in MHz, which the
        // conversion in decode.rs turns back into Hz.
        let quick = ndepth > 0 && ndepth < 2;
        // 3 passes: upstream's default. The third is the weak-signal pass.
        wspr::decode_frame(iwave, 0.0, quick, 3, true, false, false)
            .into_iter()
            .map(Into::into)
            .collect()
    }
}
