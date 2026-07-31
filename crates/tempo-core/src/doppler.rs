//! Satellite Doppler correction — the pure core.
//!
//! Everything here is arithmetic over caller-supplied numbers: a transponder,
//! a range-rate from [`crate::sat::range_rate`], and the operator's own tuning.
//! No radio, no clock, no I/O — so the behaviour that decides where you
//! transmit is unit-testable on the bench instead of only over a live bird.
//!
//! # The two corrections are not symmetric
//!
//! A satellite RECEDING at `ṙ` shifts what you HEAR down and what it HEARS up:
//!
//! - **Downlink** — the bird transmits `f_down`; you must listen at
//!   `f_down × (1 − ṙ/c)`.
//! - **Uplink** — the bird listens at `f_up`; you must transmit at
//!   `f_up × (1 + ṙ/c)` so the shift on the way up lands your signal on its
//!   receiver.
//!
//! Both, always. Correcting only the downlink is the classic half-Doppler
//! mistake: your audio sounds right while you drift off the far end of the
//! passband and nobody answers.
//!
//! # Inverting transponders
//!
//! A linear INVERTING transponder mirrors its passband: tuning UP the downlink
//! means your uplink must go DOWN, and the sidebands swap (LSB up / USB down).
//! Every RS-44/AO-7-class bird works this way. Getting it wrong doesn't just
//! sound wrong — it puts you on top of a different QSO entirely, which is why
//! [`Transponder::invert`] is per-transponder data (from SatNOGS) and never a
//! global setting.
//!
//! # Operator-follow — the part that makes it usable
//!
//! Nobody parks on a computed centre frequency. You tune the DOWNLINK to chase
//! a station drifting through the passband, and your uplink has to follow so
//! you both stay in the same conversation. So the engine's state is not a
//! frequency: it is the operator's OFFSET from transponder centre
//! ([`DopplerState::offset_hz`]), which survives while Doppler moves the
//! absolute frequencies underneath it.

/// Speed of light, m/s — the same constant on both legs.
const C_M_S: f64 = 299_792_458.0;

/// The transponder being worked, reduced to what Doppler needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transponder {
    /// Centre of the uplink passband (Hz). `0` = downlink-only (a beacon).
    pub uplink_centre_hz: u64,
    /// Centre of the downlink passband (Hz).
    pub downlink_centre_hz: u64,
    /// Linear INVERTING transponder — see the module header.
    pub invert: bool,
    /// Half-width of the passband (Hz), for clamping the operator's tuning.
    /// `0` = a channel (FM repeater / beacon): no tuning inside it.
    pub half_width_hz: u64,
}

impl Transponder {
    /// A single-channel transponder (FM repeater, beacon): no passband, no
    /// inversion, nothing to tune inside.
    pub fn channel(uplink_hz: u64, downlink_hz: u64) -> Self {
        Transponder {
            uplink_centre_hz: uplink_hz,
            downlink_centre_hz: downlink_hz,
            invert: false,
            half_width_hz: 0,
        }
    }
}

/// The operator's position within the transponder, carried across the pass.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DopplerState {
    /// Where the operator has tuned RELATIVE to the downlink centre (Hz,
    /// signed). This — not an absolute frequency — is what persists as Doppler
    /// slides the band underneath.
    pub offset_hz: i64,
}

/// What the radio should be set to, right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// Where to LISTEN (Hz).
    pub downlink_hz: u64,
    /// Where to TRANSMIT (Hz). Equals the downlink for a beacon-only
    /// transponder (uplink centre 0) — callers must not key in that case.
    pub uplink_hz: u64,
    /// Doppler applied to each leg (Hz, signed) — for display. Operators read
    /// these to sanity-check a pass, and they are the numbers to diff against
    /// a reference controller.
    pub downlink_shift_hz: i64,
    pub uplink_shift_hz: i64,
}

/// The tuning for this instant.
///
/// `range_rate_km_s` is positive while the satellite RECEDES (the convention
/// [`crate::sat::range_rate`] pins with a test).
pub fn tuning(t: &Transponder, state: DopplerState, range_rate_km_s: f64) -> Tuning {
    // Fraction of c. Range-rate arrives in km/s; c is in m/s.
    let beta = (range_rate_km_s * 1000.0) / C_M_S;

    // The operator's chosen spot within the passband, clamped to it. A channel
    // (half_width 0) pins the offset to zero: there is nothing to tune inside.
    let offset = clamp_offset(state.offset_hz, t.half_width_hz);

    // Downlink: what the operator wants to hear, before Doppler.
    let want_down = add_offset(t.downlink_centre_hz, offset);
    // Uplink: the SAME position in the passband, mirrored when inverting.
    let up_offset = if t.invert { -offset } else { offset };
    let want_up = add_offset(t.uplink_centre_hz, up_offset);

    // Receding ⇒ hear it low, transmit high.
    let down_corrected = shift(want_down, -beta);
    let up_corrected = shift(want_up, beta);

    Tuning {
        downlink_hz: down_corrected,
        uplink_hz: if t.uplink_centre_hz == 0 {
            down_corrected
        } else {
            up_corrected
        },
        downlink_shift_hz: down_corrected as i64 - want_down as i64,
        uplink_shift_hz: up_corrected as i64 - want_up as i64,
    }
}

/// Adopt an operator's manual DOWNLINK tuning: convert the dial they just set
/// into the passband offset the engine carries, given the Doppler in force at
/// that instant. This is the operator-follow entry point — the uplink then
/// tracks them automatically on the next [`tuning`] call.
///
/// Returns the state unchanged when the transponder is a channel (nothing to
/// tune inside) so a stray dial nudge on an FM bird can't drag the uplink.
pub fn follow_downlink(
    t: &Transponder,
    tuned_downlink_hz: u64,
    range_rate_km_s: f64,
) -> DopplerState {
    if t.half_width_hz == 0 {
        return DopplerState::default();
    }
    DopplerState {
        offset_hz: clamp_offset(
            raw_offset_for(t, tuned_downlink_hz, range_rate_km_s),
            t.half_width_hz,
        ),
    }
}

/// The same adoption, but it REFUSES a dial that is not inside the passband.
///
/// The difference matters at the one call site that watches the operator's VFO
/// knob. [`follow_downlink`] clamps, which is right when a caller has already
/// decided the operator is tuning *within* the transponder — but a knob QSY to
/// another band is also "a new dial", and clamping that would slam the offset
/// to a passband edge and drag the uplink with it. A satellite passband is a
/// few tens of kHz wide; anything outside it is the operator leaving, not
/// tuning, and the honest answer is to decline rather than to guess.
///
/// `None` for a channel (an FM bird has nothing to tune inside) or an
/// out-of-band dial.
pub fn follow_downlink_in_band(
    t: &Transponder,
    tuned_downlink_hz: u64,
    range_rate_km_s: f64,
) -> Option<DopplerState> {
    if t.half_width_hz == 0 {
        return None;
    }
    let offset = raw_offset_for(t, tuned_downlink_hz, range_rate_km_s);
    (offset.unsigned_abs() <= t.half_width_hz).then_some(DopplerState { offset_hz: offset })
}

/// Undo the Doppler the operator was hearing to recover the bird-frame
/// frequency, then express it as an offset from the passband centre. Unclamped
/// on purpose — the callers above differ precisely in what they do when it
/// falls outside.
fn raw_offset_for(t: &Transponder, tuned_downlink_hz: u64, range_rate_km_s: f64) -> i64 {
    let beta = (range_rate_km_s * 1000.0) / C_M_S;
    let bird_frame = shift(tuned_downlink_hz, beta); // inverse of the -beta in `tuning`
    bird_frame as i64 - t.downlink_centre_hz as i64
}

/// The sideband pair for a transponder, given the downlink mode the satellite
/// database reports. An inverting transponder swaps the uplink sideband — the
/// single most-missed detail in satellite operating.
pub fn uplink_mode_for(downlink_mode: &str, invert: bool) -> String {
    let m = downlink_mode.trim().to_ascii_uppercase();
    if !invert {
        return m;
    }
    match m.as_str() {
        "USB" => "LSB".to_string(),
        "LSB" => "USB".to_string(),
        // CW/FM/data modes have no sideband to mirror.
        _ => m,
    }
}

/// When the engine is allowed to move the dial — the difference between
/// working a bird by ear and running a slot-timed digital mode through one.
///
/// # Why these are not the same operating model
///
/// **Manual modes (SSB / CW / FM)** carry no timing contract. The operator is
/// listening continuously, the far end drifts continuously, and steering the
/// dial *during* a transmission is exactly what every satellite controller
/// does — it is how your signal stays on the transponder for the length of an
/// over. [`TxPolicy::Continuous`].
///
/// **Slot modes (FT8/FT4-family)** are the opposite case, and not because of a
/// software guard — because of the waveform. An FT8 tone is ~6 Hz wide and the
/// whole signal ~50 Hz; the decoder assumes a coherent carrier for the full
/// 12.6 s transmission. Near TCA on a 435 MHz LEO downlink the Doppler RATE
/// reaches roughly 100 Hz/s, so the physical shift alone smears a single over
/// by well over a kilohertz — and if we ALSO step the dial underneath the
/// waveform we are playing, we add our own discontinuities on top of it. The
/// far end then decodes neither.
///
/// So for slot modes the engine goes quiet for the duration of a keyed over and
/// re-corrects between them ([`TxPolicy::FreezeDuringOver`]). That is the most
/// a fixed-waveform transmit path can honestly do: it removes OUR contribution
/// to the smear, leaves the physical Doppler (which nothing at this layer can
/// remove), and keeps every over internally coherent.
///
/// The practical consequence is worth stating plainly rather than hiding: FT
/// modes work well through GEO birds (QO-100-class), where Doppler is small and
/// slow, and remain marginal through fast LEO passes regardless of what the
/// software does. That is a property of the mode, not of this implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxPolicy {
    /// Steer continuously, including while keyed — SSB, CW, FM.
    Continuous,
    /// Hold the dial still for the length of a keyed over; correct between
    /// overs — the slot-timed digital modes.
    FreezeDuringOver,
}

impl TxPolicy {
    /// The policy an operating mode implies. Digital (slot) modes freeze;
    /// everything the operator keys by hand steers continuously.
    pub fn for_slot_mode(is_slot_timed: bool) -> Self {
        if is_slot_timed {
            TxPolicy::FreezeDuringOver
        } else {
            TxPolicy::Continuous
        }
    }

    /// May the engine move the dial right now?
    ///
    /// `keyed` is "a transmission is physically in flight". Note the asymmetry
    /// with the TX-safety stamp: a Doppler step is an INTENDED, engine-authored,
    /// in-passband move, not an operator QSY — the two must never be conflated,
    /// which is why the engine reports its authority explicitly rather than
    /// letting the dial value speak for itself.
    pub fn may_steer(self, keyed: bool) -> bool {
        match self {
            TxPolicy::Continuous => true,
            TxPolicy::FreezeDuringOver => !keyed,
        }
    }
}

/// What was last actually SENT to the radio, so the next correction can decide
/// whether it is worth sending at all.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SentTuning {
    pub downlink_hz: u64,
    pub uplink_hz: u64,
    /// Monotonic ms of the last write (any clock, caller-supplied).
    pub at_ms: u64,
    /// False until the first write, so the pass always begins with one.
    pub sent: bool,
}

/// A rate-limit decision: what to write now, or nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Correction {
    pub downlink_hz: Option<u64>,
    pub uplink_hz: Option<u64>,
}

impl Correction {
    pub fn is_empty(&self) -> bool {
        self.downlink_hz.is_none() && self.uplink_hz.is_none()
    }
}

/// The operator's two brakes on how often the radio is written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SteerLimits {
    /// Corrections smaller than this are not worth a CAT write.
    pub min_shift_hz: u32,
    /// Minimum gap between writes (ms).
    pub min_interval_ms: u32,
}

/// Which legs the operator's VFO mapping actually puts under Doppler control.
/// A receive-only mapping must never yield a transmit frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Legs {
    pub downlink: bool,
    pub uplink: bool,
}

/// Should we write to the radio right now, and what?
///
/// Two independent brakes, both operator-set, because a satellite pass is the
/// one time an app is tempted to spam a serial bus:
///
/// - **`min_shift_hz`** — a correction smaller than this is inaudible and not
///   worth a CAT write. At 435 MHz, 20 Hz is well inside the tuning step most
///   radios use, and writing it anyway just fights the operator's knob.
/// - **`min_interval_ms`** — the radio is a serial device sharing a bus with
///   meters, mode and PTT. One write a second tracks a LEO pass fine.
///
/// The FIRST correction of a pass always goes out (`sent = false`), or the
/// radio would sit on an uncorrected frequency until the shift happened to grow
/// past the threshold.
///
/// `steer` is [`TxPolicy::may_steer`] — a frozen over returns nothing at all,
/// so a slot-mode transmission is never stepped underneath.
pub fn correction(
    want: Tuning,
    last: SentTuning,
    now_ms: u64,
    limits: SteerLimits,
    legs: Legs,
    steer: bool,
) -> Correction {
    let none = Correction {
        downlink_hz: None,
        uplink_hz: None,
    };
    if !steer {
        return none;
    }
    if last.sent && now_ms.saturating_sub(last.at_ms) < u64::from(limits.min_interval_ms) {
        return none;
    }
    let worth = |want: u64, was: u64| -> bool {
        !last.sent || want.abs_diff(was) >= u64::from(limits.min_shift_hz)
    };
    Correction {
        downlink_hz: (legs.downlink && worth(want.downlink_hz, last.downlink_hz))
            .then_some(want.downlink_hz),
        uplink_hz: (legs.uplink && worth(want.uplink_hz, last.uplink_hz))
            .then_some(want.uplink_hz),
    }
}

/// Apply a fractional shift to a frequency, rounding to the nearest Hz.
fn shift(hz: u64, beta: f64) -> u64 {
    let v = hz as f64 * (1.0 + beta);
    if v <= 0.0 {
        return 0;
    }
    v.round() as u64
}

fn add_offset(centre: u64, offset: i64) -> u64 {
    if offset >= 0 {
        centre.saturating_add(offset as u64)
    } else {
        centre.saturating_sub(offset.unsigned_abs())
    }
}

/// Hold an offset inside the passband. Public because anything REPORTING the
/// operator's position has to clamp it exactly as [`tuning`] does, or a display
/// drawn from the reported offset can disagree with the frequencies computed
/// from the clamped one.
pub fn clamp_offset(offset: i64, half_width_hz: u64) -> i64 {
    let lim = half_width_hz as i64;
    offset.clamp(-lim, lim)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: Legs = Legs {
        downlink: true,
        uplink: true,
    };
    const DOWN_ONLY: Legs = Legs {
        downlink: true,
        uplink: false,
    };
    fn lim(min_shift_hz: u32, min_interval_ms: u32) -> SteerLimits {
        SteerLimits {
            min_shift_hz,
            min_interval_ms,
        }
    }

    /// RS-44-class linear inverting transponder (real-world shape).
    fn rs44() -> Transponder {
        Transponder {
            uplink_centre_hz: 145_965_000,
            downlink_centre_hz: 435_640_000,
            invert: true,
            half_width_hz: 30_000,
        }
    }

    #[test]
    fn both_legs_are_corrected_and_in_opposite_directions() {
        // Half-Doppler is the classic bug: audio sounds right while your uplink
        // drifts off the far end of the passband and nobody comes back to you.
        let t = rs44();
        let s = DopplerState::default();
        let receding = tuning(&t, s, 5.0);
        assert!(
            receding.downlink_shift_hz < 0,
            "receding ⇒ hear it LOW, got {}",
            receding.downlink_shift_hz
        );
        assert!(
            receding.uplink_shift_hz > 0,
            "receding ⇒ transmit HIGH, got {}",
            receding.uplink_shift_hz
        );
        let approaching = tuning(&t, s, -5.0);
        assert!(approaching.downlink_shift_hz > 0);
        assert!(approaching.uplink_shift_hz < 0);

        // Magnitudes: 435.64 MHz at 5 km/s ≈ 7.3 kHz; 145.965 MHz ≈ 2.4 kHz.
        assert!((receding.downlink_shift_hz + 7_265).abs() < 40);
        assert!((receding.uplink_shift_hz - 2_434).abs() < 40);
    }

    #[test]
    fn an_inverting_transponder_mirrors_the_operators_offset() {
        // THE inversion test. Tune 10 kHz UP the downlink and the uplink must
        // go 10 kHz DOWN — otherwise you land on a different QSO.
        let t = rs44();
        let up_the_band = DopplerState { offset_hz: 10_000 };
        let a = tuning(&t, up_the_band, 0.0);
        assert_eq!(a.downlink_hz, 435_650_000, "downlink follows the operator up");
        assert_eq!(a.uplink_hz, 145_955_000, "inverting ⇒ uplink goes DOWN");

        // A NON-inverting transponder moves both the same way.
        let mut lin = t;
        lin.invert = false;
        let b = tuning(&lin, up_the_band, 0.0);
        assert_eq!(b.downlink_hz, 435_650_000);
        assert_eq!(b.uplink_hz, 145_975_000, "non-inverting ⇒ uplink goes UP");
    }

    #[test]
    fn sidebands_swap_only_when_inverting() {
        assert_eq!(uplink_mode_for("USB", true), "LSB");
        assert_eq!(uplink_mode_for("LSB", true), "USB");
        assert_eq!(uplink_mode_for("USB", false), "USB");
        // CW and FM have no sideband to mirror.
        assert_eq!(uplink_mode_for("CW", true), "CW");
        assert_eq!(uplink_mode_for("FM", true), "FM");
        assert_eq!(uplink_mode_for(" usb ", true), "LSB", "case/space tolerant");
    }

    #[test]
    fn operator_follow_survives_doppler_moving_underneath() {
        // The behaviour that makes it a tool rather than a demo. The operator
        // chases a station 8 kHz up the passband EARLY in the pass (satellite
        // approaching fast); by TCA the Doppler has swung right through zero,
        // and their uplink must still be pointed at the same station.
        let t = rs44();
        let early_rate = -6.0; // approaching
        // What the operator hears the station on, early in the pass:
        let heard_at = tuning(&t, DopplerState { offset_hz: 8_000 }, early_rate).downlink_hz;
        // They tune the dial there manually; the engine adopts it.
        let state = follow_downlink(&t, heard_at, early_rate);
        assert!(
            (state.offset_hz - 8_000).abs() <= 2,
            "adopting a manual tune must recover the passband offset, got {}",
            state.offset_hz
        );

        // Later in the pass the geometry has reversed. The absolute frequencies
        // move a long way; the RELATIVE position must not.
        let late = tuning(&t, state, 6.0);
        let down_delta = late.downlink_hz as i64 - heard_at as i64;
        assert!(
            down_delta.abs() > 10_000,
            "precondition: Doppler really did move the band ({down_delta} Hz)"
        );
        // The uplink stayed mirrored around the same passband position.
        let expected_up_centre = 145_965_000i64 - 8_000;
        let up_bird_frame = (late.uplink_hz as i64) - late.uplink_shift_hz;
        assert!(
            (up_bird_frame - expected_up_centre).abs() <= 2,
            "uplink lost the operator's position: {up_bird_frame} vs {expected_up_centre}"
        );
    }

    #[test]
    fn a_channel_transponder_has_nothing_to_tune_inside() {
        // FM repeater: a stray dial nudge must not drag the uplink off channel.
        let t = Transponder::channel(145_990_000, 437_800_000);
        let s = follow_downlink(&t, 437_805_000, 0.0);
        assert_eq!(s.offset_hz, 0, "a channel pins the offset");
        let r = tuning(&t, DopplerState { offset_hz: 5_000 }, 0.0);
        assert_eq!(r.downlink_hz, 437_800_000);
        assert_eq!(r.uplink_hz, 145_990_000);
    }

    #[test]
    fn the_operator_cannot_tune_outside_the_passband() {
        let t = rs44();
        let r = tuning(&t, DopplerState { offset_hz: 999_000 }, 0.0);
        assert_eq!(
            r.downlink_hz,
            435_640_000 + 30_000,
            "clamped to the passband edge, never past it"
        );
    }

    #[test]
    fn a_beacon_never_produces_a_transmit_frequency_of_its_own() {
        // Downlink-only: uplink centre 0. The caller must not key, and the
        // engine must not invent an uplink somewhere arbitrary.
        let t = Transponder {
            uplink_centre_hz: 0,
            downlink_centre_hz: 435_300_000,
            invert: false,
            half_width_hz: 0,
        };
        let r = tuning(&t, DopplerState::default(), 3.0);
        assert_eq!(r.uplink_hz, r.downlink_hz);
        assert!(r.downlink_shift_hz < 0);
    }

    #[test]
    fn slot_modes_freeze_the_dial_while_keyed_manual_modes_do_not() {
        // The two operating models, pinned. Steering under a fixed waveform
        // adds our own discontinuities to a signal the far end must decode as
        // one coherent carrier; steering under a voice/CW over is exactly how
        // the signal stays on the transponder.
        let slot = TxPolicy::for_slot_mode(true);
        let manual = TxPolicy::for_slot_mode(false);
        assert_eq!(slot, TxPolicy::FreezeDuringOver);
        assert_eq!(manual, TxPolicy::Continuous);

        assert!(slot.may_steer(false), "between overs the slot mode re-corrects");
        assert!(!slot.may_steer(true), "never mid-over on a slot mode");
        assert!(manual.may_steer(true), "SSB/CW/FM steer while keyed — by design");
        assert!(manual.may_steer(false));
    }

    #[test]
    fn a_frozen_over_still_lands_on_the_right_frequency_when_it_starts() {
        // Freezing does not mean ignoring Doppler: the over is keyed at the
        // correction in force at key-down, and the NEXT over gets a fresh one.
        // (What it cannot fix is the physical drift during the over itself —
        // that is the mode's problem, not ours.)
        let t = rs44();
        let s = DopplerState::default();
        let at_key_down = tuning(&t, s, -4.0);
        let next_over = tuning(&t, s, 4.0);
        assert_ne!(
            at_key_down.uplink_hz, next_over.uplink_hz,
            "between overs the correction must actually change"
        );
        assert!(at_key_down.uplink_shift_hz < 0 && next_over.uplink_shift_hz > 0);
    }

    #[test]
    fn the_first_correction_of_a_pass_always_goes_out() {
        // Otherwise the radio sits on an uncorrected frequency until the shift
        // happens to grow past the threshold — at AOS that is the whole point.
        let t = rs44();
        let want = tuning(&t, DopplerState::default(), 0.5); // tiny shift
        let c = correction(want, SentTuning::default(), 0, lim(500, 1_000), BOTH, true);
        assert_eq!(c.downlink_hz, Some(want.downlink_hz));
        assert_eq!(c.uplink_hz, Some(want.uplink_hz));
    }

    #[test]
    fn rate_limits_hold_off_both_by_size_and_by_clock() {
        let t = rs44();
        let want = tuning(&t, DopplerState::default(), 5.0);
        let last = SentTuning {
            downlink_hz: want.downlink_hz - 5, // 5 Hz stale: not worth a write
            uplink_hz: want.uplink_hz - 5,
            at_ms: 10_000,
            sent: true,
        };
        // Too small AND too soon.
        assert!(correction(want, last, 10_200, lim(20, 1_000), BOTH, true).is_empty());
        // Interval satisfied, but the change is still under the threshold.
        assert!(correction(want, last, 12_000, lim(20, 1_000), BOTH, true).is_empty());
        // A real change, interval satisfied → both legs go.
        let moved = SentTuning {
            downlink_hz: want.downlink_hz - 900,
            uplink_hz: want.uplink_hz - 900,
            ..last
        };
        let c = correction(want, moved, 12_000, lim(20, 1_000), BOTH, true);
        assert!(c.downlink_hz.is_some() && c.uplink_hz.is_some());
    }

    #[test]
    fn a_frozen_over_writes_nothing_at_all() {
        // Slot mode, keyed: the dial must not move under the waveform.
        let t = rs44();
        let want = tuning(&t, DopplerState::default(), 5.0);
        let c = correction(
            want,
            SentTuning::default(),
            0,
            lim(20, 1_000),
            BOTH,
            TxPolicy::FreezeDuringOver.may_steer(true),
        );
        assert!(c.is_empty(), "no writes while a slot-mode over is in flight");
    }

    #[test]
    fn a_downlink_only_mapping_never_produces_a_transmit_frequency() {
        // Receive-only satellite listening must not write an uplink anywhere.
        let t = rs44();
        let want = tuning(&t, DopplerState::default(), 5.0);
        let c = correction(want, SentTuning::default(), 0, lim(20, 1_000), DOWN_ONLY, true);
        assert!(c.downlink_hz.is_some());
        assert_eq!(c.uplink_hz, None, "downlink-only must never key a frequency");
    }

    #[test]
    fn zero_range_rate_is_the_identity() {
        let t = rs44();
        let r = tuning(&t, DopplerState::default(), 0.0);
        assert_eq!(r.downlink_hz, 435_640_000);
        assert_eq!(r.uplink_hz, 145_965_000);
        assert_eq!(r.downlink_shift_hz, 0);
        assert_eq!(r.uplink_shift_hz, 0);
    }

    #[test]
    fn follow_in_band_declines_a_dial_that_left_the_transponder() {
        // The knob-watching call site cannot tell "tuning across the passband"
        // from "QSY to 20 m" — both are just a new dial. Clamping the second
        // one would slam the offset to a passband edge and drag the uplink
        // there, so the in-band variant declines instead of guessing.
        let t = rs44(); // 435.640 MHz centre, ±half_width
        let half = t.half_width_hz as i64;

        // Dead centre, no Doppler: offset 0, adopted.
        let s = follow_downlink_in_band(&t, t.downlink_centre_hz, 0.0).expect("centre is in band");
        assert_eq!(s.offset_hz, 0);

        // Just inside the top edge: adopted, and NOT clamped — the exact offset
        // is what the passband strip draws a cursor at.
        let inside = t.downlink_centre_hz + (half as u64) - 500;
        let s = follow_downlink_in_band(&t, inside, 0.0).expect("inside the edge is in band");
        assert_eq!(s.offset_hz, half - 500);

        // A whole band away: declined, not clamped to the edge.
        assert!(
            follow_downlink_in_band(&t, 14_074_000, 0.0).is_none(),
            "a knob QSY to 20 m must not be adopted as a passband offset"
        );
        // …where the clamping variant would have pinned it to an edge, which is
        // exactly the behaviour the knob path must not have.
        assert_eq!(follow_downlink(&t, 14_074_000, 0.0).offset_hz, -half);

        // A channel (FM bird) has nothing to tune inside.
        let fm = Transponder {
            half_width_hz: 0,
            ..t
        };
        assert!(follow_downlink_in_band(&fm, fm.downlink_centre_hz, 0.0).is_none());
    }

    #[test]
    fn follow_in_band_measures_against_the_dopplered_passband() {
        // The operator tunes the dial they HEAR, which Doppler has moved. The
        // in-band test must undo that shift first, or a perfectly ordinary
        // in-passband tune reads as out-of-band near AOS, when the shift is
        // biggest and chasing a station matters most.
        let t = rs44();
        let rate = -7.0; // approaching hard: ~+10 kHz on 70 cm
        let heard_centre = tuning(&t, DopplerState { offset_hz: 0 }, rate).downlink_hz;
        assert!(
            heard_centre > t.downlink_centre_hz + 5_000,
            "the test needs a shift big enough to matter"
        );
        let s = follow_downlink_in_band(&t, heard_centre, rate)
            .expect("the dial the operator actually hears the centre at is in band");
        assert!(s.offset_hz.abs() <= 20, "recovers ~centre, got {}", s.offset_hz);

        // The same raw number WITHOUT undoing Doppler would be off by the shift.
        let naive = heard_centre as i64 - t.downlink_centre_hz as i64;
        assert!(naive.abs() > 5_000, "guard: the naive answer really is wrong");
    }
}
