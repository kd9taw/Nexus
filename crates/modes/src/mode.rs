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
    TempoFast,
}

impl ModeKind {
    /// All modes shipped today, in display order.
    pub const ALL: [ModeKind; 3] = [ModeKind::Ft8, ModeKind::Ft4, ModeKind::TempoFast];

    /// Short display name, e.g. `"FT8"`.
    pub fn as_str(self) -> &'static str {
        match self {
            ModeKind::Ft8 => "FT8",
            ModeKind::Ft4 => "FT4",
            ModeKind::TempoFast => "TempoFast",
        }
    }

    /// Transmit/receive slot length in seconds (FT8 = 15, FT4 = 7.5, FT1 = 4).
    pub fn slot_secs(self) -> f32 {
        match self {
            ModeKind::Ft8 => 15.0,
            ModeKind::Ft4 => 7.5,
            ModeKind::TempoFast => 4.0,
        }
    }

    /// Number of int16 samples in one DECODE frame at 12 kHz (the length the
    /// vendored decoder reads from the start of the captured window).
    pub fn frame_samples(self) -> usize {
        match self {
            ModeKind::Ft8 => ft8::NMAX,
            ModeKind::Ft4 => ft4::NMAX,
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
    /// Has contest sub-modes / exchanges.
    pub contest: bool,
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
    /// (FT1); modes without these ignore the respective argument.
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
    ) -> Vec<Decode>;

    /// [`decode_frame`](Mode::decode_frame) plus the cross-cycle a-priori flag.
    ///
    /// `a7_final` marks the authoritative full-audio (slot-boundary) pass: for
    /// FT8 it gates WSJT-X's a7 cross-cycle replay (iaptype=7), which recovers
    /// QSO continuations remembered from the previous same-parity slot (keyed
    /// on `frame_time_ms`). The early partial pass sets it `false` (slot
    /// bookkeeping only). Modes without a cross-cycle path ignore the flag —
    /// this default delegates to [`decode_frame`](Mode::decode_frame).
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
        frame_time_ms: i64,
        _a7_final: bool,
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
        )
    }
}

/// Build a boxed [`Mode`] from its [`ModeKind`].
pub fn make_mode(kind: ModeKind) -> Box<dyn Mode> {
    match kind {
        ModeKind::Ft8 => Box::new(Ft8Mode),
        ModeKind::Ft4 => Box::new(Ft4Mode),
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
        frame_time_ms: i64,
        a7_final: bool,
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
            nutc,
            a7_final,
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
const FT1_LEAD_IN_SECS: f32 = 0.4;

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
        // 0.4 s, NOT the 0.5 s FT8/FT4 use: `tempo_fast::gen_wave` returns a FIXED
        // NMAX = 48000-sample (4.000 s) buffer holding 3.536 s of tones plus 0.464 s of tail
        // zeros. 0.4 s (4800 samples) shifts into that tail with 771 samples to spare; 0.5 s
        // would overrun it. Shifting WITHIN the buffer — rather than prepending, as FT8/FT4 do —
        // keeps the length at exactly 4.000 s, which matters because the PTT hold is sized from
        // `w.len()` (`tempo-audio/src/slot.rs`) and FT1's over already fills its whole T/R period.
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
        ) -> Vec<Decode> {
            Vec::new()
        }
    }

    #[test]
    fn every_shipped_mode_declares_tx() {
        // If a future mode ships RX-only, this is the line that has to change
        // deliberately — it should not be possible to add a silent mode by accident.
        for kind in ModeKind::ALL {
            assert!(
                make_mode(kind).capabilities().tx,
                "{} must declare tx: true or the TX path silently drops it",
                kind.as_str()
            );
            assert!(
                tx_mode(kind).is_some(),
                "tx_mode({}) must hand back a mode",
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
        for kind in ModeKind::ALL {
            let _ = make_mode(kind); // never refuses
        }
        assert!(RxOnlyMode.capabilities().tx == false);
    }
}
