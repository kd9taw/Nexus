//! The amplifier status poll thread — one serial link, read once a second, into the snapshot.
//!
//! # READ-ONLY, and that is the whole design
//!
//! The only bytes this thread ever puts on the wire are `SPE_CMD_STATUS` (0x90) and the six
//! Elecraft read verbs `^OS ^BN ^TM ^VI ^WS ^FL`. There is no write surface here and none is
//! planned: SPE's command set is front-panel KEYSTROKES — relative steps and toggles whose
//! meaning depends on a state we learn a poll late — so every write is a guess about a state
//! that has already moved. See `crate::amplifier`'s header for the worked example (Hamlib's
//! "standby" byte *is* the switch-off byte).
//!
//! ⛔ **NOTHING HERE GATES A TRANSMISSION, AND NOTHING HERE MAY EVER BECOME A STOP.** Putting an
//! amplifier in standby is not a way to stop a transmission — the exciter keeps keying and the
//! drive passes straight through — so no reading this thread produces may enter a cockpit's
//! stop-line census, and no TX decision may be conditioned on one. It is a display.
//!
//! # Why its own thread
//!
//! One poll is 0.5–2.4 s of BLOCKING serial (SPE: a write plus two `read_exact` at a 500 ms
//! budget; KPA: six sequential verbs at 400 ms each). The radio loop ticks every 20 ms and caps
//! its whole heavy CAT read-back at 250 ms, asking `have_budget()` before every single read.
//! A reader that long inside that budget does not fail loudly — it silently STARVES the readers
//! after it, which is exactly what `\dump_caps` did to the split read it was meant to qualify.
//! So the poll lives here, off the loop, and every byte of serial I/O happens with the engine
//! mutex RELEASED.
//!
//! # v1 polls the ACTIVE radio's amplifier only
//!
//! A second link is a second exclusive port hold and a second failure surface. The engine cache
//! is already keyed by radio id, so this extends to a pool the way the radio monitor did —
//! nothing here has to change shape for SO2R, only the reconcile.
//!
//! ⚠️ NEEDS-BENCH: both codecs are written from vendor specs and no field below has ever been
//! read from a real amplifier.

use std::time::Duration;

use tempo_app::dto::AmpStatusDto;

use crate::amplifier::{KpaStatus, SpeAlarm, SpeStatus, SpeWarning};

/// Wire tag for the SPE Expert family.
pub const FAMILY_SPE: &str = "spe";
/// Wire tag for the Elecraft KPA family.
pub const FAMILY_KPA: &str = "kpa";

/// Poll cadence, flat for both families.
///
/// The KPA needs six sequential round-trips per reading at a 400 ms per-verb budget, so a
/// faster cadence risks a poll not finishing before the next begins. Delivery rides the UI's
/// existing 300 ms snapshot poll, so the operator sees a reading at most ~1.3 s old — no slower
/// than the amplifier's own front panel, and faster than anything on it changes.
pub const POLL: Duration = Duration::from_millis(1000);

/// Longest a failed-open backoff ever waits.
const MAX_BACKOFF_MS: u64 = 60_000;

/// How long to wait before retrying an amplifier that failed to open, after `failures` in a row.
///
/// ⭐ WITHOUT THIS, AN AMPLIFIER THAT IS SWITCHED OFF COSTS A PORT SWEEP EVERY SECOND FOREVER.
/// `KpaLink::open` tries all four documented data rates at a 250 ms probe budget — up to ~1 s of
/// blocking and four port opens per attempt — because the KPA remembers its own rate and there
/// is nothing to assume. The radio monitor's `retry_after_ms` documents the identical trap from
/// the other side (a rigctld spawned and killed every 850 ms, forever), and this is the same
/// curve: 1 s, 2 s, 4 s … capped. A successful open clears it, so an amplifier switched on
/// mid-session is adopted within one backoff.
pub fn backoff_ms(failures: u32) -> u64 {
    if failures == 0 {
        return 0;
    }
    let step = failures.min(6) - 1;
    (1000u64 << step).min(MAX_BACKOFF_MS)
}

/// Which operator-facing reason token an amplifier error becomes.
///
/// ⭐ CLASSIFIED BY `ErrorKind`, NEVER BY MATCHING THE MESSAGE TEXT. The link layer sets the kind
/// deliberately for exactly this: `Unsupported` is the EXPERT 1K-FA — a working link speaking a
/// protocol Nexus does not decode, which must never read as "no amplifier" to an operator whose
/// amplifier is right there and switched on. `InvalidData` is a frame that arrived and failed
/// its checksum or its shape, which is a different fact from silence. Everything else is the
/// port being held by something else, or nothing answering.
pub fn reason_for(e: &std::io::Error) -> &'static str {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::Unsupported => "wrongModel",
        K::InvalidData => "malformed",
        // The port opened for somebody else first. On Windows a held COM port is "Access is
        // denied"; on Unix an exclusive open of a busy tty is EBUSY.
        K::PermissionDenied | K::ResourceBusy | K::AddrInUse => "portBusy",
        _ => "noAnswer",
    }
}

/// The camelCase wire tag for an SPE alarm.
///
/// ⚠️ `Unknown` is a FAULT, not a gap. A later firmware adding an alarm letter must not read to
/// the operator as "no alarm" — the failure direction of a status decoder in front of a kilowatt
/// has to be toward reporting a fault, not toward silence. `alarm_raised` below is taken from
/// `is_raised()`, which counts `Unknown`, and the UI colours from that rather than from this tag.
pub fn alarm_tag(a: SpeAlarm) -> &'static str {
    match a {
        SpeAlarm::None => "none",
        SpeAlarm::SwrExceedingLimits => "swrExceedingLimits",
        SpeAlarm::AmplifierProtection => "amplifierProtection",
        SpeAlarm::InputOverdriving => "inputOverdriving",
        SpeAlarm::ExcessOverheating => "excessOverheating",
        SpeAlarm::CombinerFault => "combinerFault",
        SpeAlarm::Unknown(_) => "unknown",
    }
}

/// The camelCase wire tag for an SPE warning. Same `Unknown` rule as [`alarm_tag`].
pub fn warning_tag(w: SpeWarning) -> &'static str {
    match w {
        SpeWarning::None => "none",
        SpeWarning::AlarmAmplifier => "alarmAmplifier",
        SpeWarning::NoSelectedAntenna => "noSelectedAntenna",
        SpeWarning::SwrAntenna => "swrAntenna",
        SpeWarning::NoValidBand => "noValidBand",
        SpeWarning::PowerLimitExceeded => "powerLimitExceeded",
        SpeWarning::Overheating => "overheating",
        SpeWarning::AtuNotAvailable => "atuNotAvailable",
        SpeWarning::TuningWithNoPower => "tuningWithNoPower",
        SpeWarning::AtuBypassed => "atuBypassed",
        SpeWarning::PowerSwitchHeldByRemote => "powerSwitchHeldByRemote",
        SpeWarning::CombinerOverheating => "combinerOverheating",
        SpeWarning::CombinerFault => "combinerFault",
        SpeWarning::Unknown(_) => "unknown",
    }
}

/// A zero SWR is NOT a 1:1 match — it is the amplifier saying it has no reading.
///
/// The rule is already Elecraft's, written into `kpa_parse_ws`: `^WS` "reads `000` when not
/// transmitting", so a zero comes back as `None` rather than as a number no antenna can produce.
/// SPE's §5 says the same thing in different words ("Zero on receive") and this applies it, so
/// the pane never prints `0.0:1` — a reading that would look like a perfect match.
fn swr_or_none(v: f32) -> Option<f32> {
    (v > 0.0).then_some(v)
}

/// One decoded SPE status as the UI sees it.
///
/// ⚠️ TWO VALUES ARE DELIBERATELY DROPPED, and both absences are the honest reading:
/// `band_index` (its ladder is an inference from two published endpoints, and a raw index tells
/// an operator nothing their rig does not already show), and the temperature's UNIT — §5 says
/// "Temp in °C or F" and the amplifier reports whatever its own front panel is set to, so
/// `temp_celsius` is FALSE here and the pane must print no scale letter.
pub fn spe_dto(s: &SpeStatus) -> AmpStatusDto {
    AmpStatusDto {
        family: FAMILY_SPE.to_string(),
        // Raw, exactly as it arrived: an id we do not recognise is a newer amplifier, not a bad
        // frame. This is what lets a 1.5K-FA report itself without a code change.
        model: s.model.clone(),
        linked: true,
        reason: String::new(),
        operate: Some(s.operate),
        transmitting: Some(s.transmitting),
        output_watts: Some(s.output_watts),
        swr: swr_or_none(s.swr_antenna),
        swr_atu: swr_or_none(s.swr_atu),
        volts: Some(s.volts),
        amps: Some(s.amps),
        temp: Some(s.temp_upper),
        temp_celsius: false,
        alarm: alarm_tag(s.alarm).to_string(),
        alarm_raised: s.alarm.is_raised(),
        warning: warning_tag(s.warning).to_string(),
        warning_raised: s.warning.is_raised(),
        kpa_fault: None,
    }
}

/// One decoded KPA reading as the UI sees it.
///
/// `temp_celsius` is TRUE and only here: `^TM` is documented Celsius, 0–150, so this is the one
/// family whose temperature may carry a scale letter on screen.
///
/// The KPA has no alarm or warning channel — it has ONE fault register — so `alarm` carries the
/// uniform "this amplifier is reporting a fault" signal (`"fault"` / `"none"`) with the
/// identifier itself in `kpa_fault`, and `warning` stays EMPTY, which is how the wire says "this
/// family does not report that channel" as distinct from "it reports no warning".
pub fn kpa_dto(s: &KpaStatus) -> AmpStatusDto {
    AmpStatusDto {
        family: FAMILY_KPA.to_string(),
        // The KPA does not report a model id on any polled verb, and guessing one from the verb
        // set would be a fabrication. Empty is the honest answer.
        model: String::new(),
        linked: true,
        reason: String::new(),
        operate: Some(s.operate),
        // Not reported by any verb in the polled set. `None`, never `Some(false)` — a confident
        // "not transmitting" from an amplifier that never said so is exactly the class of
        // fabricated reading this whole path refuses.
        transmitting: None,
        output_watts: Some(s.output_watts),
        swr: s.swr,
        swr_atu: None,
        volts: Some(s.volts),
        amps: Some(s.amps),
        temp: Some(s.temp_c as i16),
        temp_celsius: true,
        alarm: if s.has_fault() { "fault" } else { "none" }.to_string(),
        alarm_raised: s.has_fault(),
        warning: String::new(),
        warning_raised: false,
        kpa_fault: Some(s.fault),
    }
}

/// The port-owning half. Needs `serial` for the links themselves and `device` for the process
/// shutdown flag; neither alone is enough, and src-tauri's `radio` feature turns on both.
#[cfg(all(feature = "device", feature = "serial"))]
mod imp {
    use super::{backoff_ms, kpa_dto, reason_for, spe_dto, FAMILY_KPA, FAMILY_SPE, POLL};
    use crate::amplifier::{KpaLink, SpeLink};
    use crate::service::SHUTDOWN;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use tempo_app::engine::{engine_lock, Engine};

    /// One open amplifier, whichever family it is.
    enum Link {
        Spe(SpeLink),
        Kpa(KpaLink),
    }

    /// What the poll thread is currently configured for.
    type Cfg = (u32, String, String);

    /// Spawn the amplifier status poll thread — call once at startup, beside the RX threads.
    pub fn spawn_amp_poll(engine: Arc<Mutex<Engine>>) {
        std::thread::Builder::new()
            .name("amp-poll".into())
            .spawn(move || run(engine))
            .expect("spawn amp-poll");
    }

    fn run(engine: Arc<Mutex<Engine>>) {
        let mut link: Option<Link> = None;
        // (radio id, family, port) the link above was opened for. `None` = nothing configured.
        let mut applied: Option<Cfg> = None;
        let mut open_failures: u32 = 0;
        let mut retry_after = Instant::now();
        let mut reason = "noAnswer";

        loop {
            if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
                // Drops the port explicitly rather than at unwind, so a restart within the same
                // process does not race a still-open exclusive handle.
                drop(link);
                return;
            }
            std::thread::sleep(POLL);

            // ONE brief lock: read the active radio's amplifier config, and — when there is
            // none — drop its cache in the same guard rather than taking the mutex twice.
            let cfg: Option<Cfg> = {
                let mut e = engine_lock(&engine);
                let want = e.settings().active_profile().map(|p| {
                    (
                        p.id,
                        p.amp_model.trim().to_lowercase(),
                        p.amp_port.trim().to_string(),
                    )
                });
                match want {
                    Some((id, model, port)) if !model.is_empty() && !port.is_empty() => {
                        Some((id, model, port))
                    }
                    other => {
                        // Unconfigured, on this radio or on no radio at all. The snapshot then
                        // carries no `amp`, which is what makes every amplifier surface render
                        // NOTHING rather than an empty frame.
                        if let Some((id, _, _)) = other {
                            e.forget_amp(id);
                        }
                        None
                    }
                }
            };

            let Some(cfg) = cfg else {
                // The disarmed cost of this whole thread: one settings read and a sleep. That is
                // the state of almost every station, and it is the rule the RX threads state.
                if let Some((old, _, _)) = applied.take() {
                    engine_lock(&engine).forget_amp(old);
                }
                link = None;
                open_failures = 0;
                continue;
            };

            // RECONCILE. A changed pair drops the old link FIRST and only then opens: the port
            // is exclusive-open, so a same-port reopen while the old handle lives fails with
            // "Access is denied" on Windows. A plain `= None` suffices — `SerialPort` drops
            // synchronously and this is the only thread that touches it.
            if applied.as_ref() != Some(&cfg) {
                link = None;
                open_failures = 0;
                retry_after = Instant::now();
                // The radio changed under us: the previous radio's reading is not this radio's,
                // and leaving it in the cache would show it again on a switch back.
                if let Some((old, _, _)) = applied.replace(cfg.clone()) {
                    if old != cfg.0 {
                        engine_lock(&engine).forget_amp(old);
                    }
                }
            }
            let (id, family, port) = cfg;

            if link.is_none() {
                if Instant::now() < retry_after {
                    // Still backing off. Keep saying why — the operator must not see the reason
                    // blink out while we wait.
                    engine_lock(&engine).observe_amp_miss(id, &family, reason);
                    continue;
                }
                // OFF THE LOCK. `KpaLink::open` sweeps four data rates at 250 ms apiece.
                let opened = match family.as_str() {
                    FAMILY_SPE => SpeLink::open(&port).map(Link::Spe),
                    FAMILY_KPA => KpaLink::open(&port).map(|(l, _baud)| Link::Kpa(l)),
                    // A family string no build of Nexus writes. Not silence: an amplifier is
                    // configured and this cannot speak to it.
                    _ => Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "unknown amplifier family",
                    )),
                };
                match opened {
                    Ok(l) => {
                        link = Some(l);
                        open_failures = 0;
                    }
                    Err(e) => {
                        open_failures = open_failures.saturating_add(1);
                        reason = reason_for(&e);
                        retry_after = Instant::now()
                            + std::time::Duration::from_millis(backoff_ms(open_failures));
                        engine_lock(&engine).observe_amp_miss(id, &family, reason);
                        continue;
                    }
                }
            }

            // THE POLL ITSELF, ENTIRELY OFF THE LOCK — up to 2.4 s of blocking serial. Holding
            // the engine mutex across it would stall the 20 ms radio loop AND `get_snapshot`.
            let read = match link.as_mut() {
                Some(Link::Spe(l)) => l.poll().map(|s| spe_dto(&s)),
                Some(Link::Kpa(l)) => l.poll().map(|s| kpa_dto(&s)),
                None => continue,
            };

            // Re-lock only to hand over the result.
            match read {
                Ok(dto) => engine_lock(&engine).observe_amp_status(id, dto),
                Err(e) => {
                    reason = reason_for(&e);
                    // Drop the link so the next cycle reopens it. A desynced or unplugged port
                    // is not recovered by asking it again on the same handle, and the backoff
                    // stops the reopen from becoming a port sweep every second.
                    link = None;
                    open_failures = open_failures.saturating_add(1);
                    retry_after = Instant::now()
                        + std::time::Duration::from_millis(backoff_ms(open_failures));
                    engine_lock(&engine).observe_amp_miss(id, &family, reason);
                }
            }
        }
    }
}

#[cfg(all(feature = "device", feature = "serial"))]
pub use imp::spawn_amp_poll;

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_app::dto::AMP_REASONS;

    fn spe_fixture() -> SpeStatus {
        SpeStatus {
            model: "15K".into(),
            operate: true,
            transmitting: true,
            bank: None,
            input: 1,
            band_index: 5,
            tx_antenna: 1,
            atu: crate::amplifier::SpeAtu::Tunable,
            rx_antenna: None,
            power_level: crate::amplifier::SpePowerLevel::High,
            output_watts: 1200,
            swr_atu: 1.1,
            swr_antenna: 1.4,
            volts: 48.0,
            amps: 32.5,
            temp_upper: 41,
            temp_lower: 38,
            temp_combiner: 30,
            warning: SpeWarning::None,
            alarm: SpeAlarm::None,
        }
    }

    /// ⭐ THE ONE THAT MATTERS. An alarm letter this firmware reports and the spec does not list
    /// must reach the operator as a FAULT. Under serde's default enum tagging `SpeAlarm` would
    /// have serialised as a bare string for its unit variants and as `{"unknown":"Z"}` for the
    /// newtype — one Rust type with two JSON shapes — and a TypeScript string union would have
    /// compiled, never matched the object form, and fallen through to its default branch:
    /// silence, in front of a kilowatt, which is the exact inversion the enum was written to
    /// prevent.
    #[test]
    fn an_unrecognised_alarm_code_reaches_the_ui_raised_and_never_as_none() {
        let mut s = spe_fixture();
        s.alarm = SpeAlarm::Unknown('Z');
        let d = spe_dto(&s);
        assert_eq!(d.alarm, "unknown", "one flat tag, one JSON shape");
        assert!(
            d.alarm_raised,
            "an unknown alarm code is a FAULT, not silence"
        );
        assert_ne!(d.alarm, "none");

        // Same for a warning letter.
        let mut s = spe_fixture();
        s.warning = SpeWarning::Unknown('Q');
        let d = spe_dto(&s);
        assert_eq!(d.warning, "unknown");
        assert!(d.warning_raised);

        // CONTROL, and it must NOT be raised: the documented "no alarms" letter, so the flag is
        // reading the alarm and not simply always true.
        let d = spe_dto(&spe_fixture());
        assert_eq!(d.alarm, "none");
        assert!(!d.alarm_raised);
        assert!(!d.warning_raised);
    }

    /// Every tag is a camelCase token a UI switches on, never a sentence it renders — a Rust
    /// `format!` is invisible to the hardcoded-string guard and cannot be translated.
    #[test]
    fn every_alarm_and_warning_tag_is_a_switchable_token() {
        let alarms = [
            SpeAlarm::None,
            SpeAlarm::SwrExceedingLimits,
            SpeAlarm::AmplifierProtection,
            SpeAlarm::InputOverdriving,
            SpeAlarm::ExcessOverheating,
            SpeAlarm::CombinerFault,
            SpeAlarm::Unknown('?'),
        ];
        for a in alarms {
            let t = alarm_tag(a);
            assert!(!t.is_empty() && !t.contains(' ') && t.is_ascii(), "{t}");
            // `is_raised()` is the amplifier's own judgement and the ONLY thing the UI colours
            // from; a tag comparison would go quiet on a letter nobody has seen yet.
            assert_eq!(a.is_raised(), t != "none", "tag and flag must agree: {t}");
        }
        let warnings = [
            SpeWarning::None,
            SpeWarning::AlarmAmplifier,
            SpeWarning::NoSelectedAntenna,
            SpeWarning::SwrAntenna,
            SpeWarning::NoValidBand,
            SpeWarning::PowerLimitExceeded,
            SpeWarning::Overheating,
            SpeWarning::AtuNotAvailable,
            SpeWarning::TuningWithNoPower,
            SpeWarning::AtuBypassed,
            SpeWarning::PowerSwitchHeldByRemote,
            SpeWarning::CombinerOverheating,
            SpeWarning::CombinerFault,
            SpeWarning::Unknown('?'),
        ];
        for w in warnings {
            let t = warning_tag(w);
            assert!(!t.is_empty() && !t.contains(' ') && t.is_ascii(), "{t}");
            assert_eq!(w.is_raised(), t != "none", "tag and flag must agree: {t}");
        }
    }

    /// The 1.5K-FA — the amplifier actually on the bench — must arrive with its own model id
    /// intact, without appearing anywhere in this crate as a recognised constant. The decoder
    /// keeps the id RAW on purpose: an id we do not know is a newer amplifier, not a bad frame.
    #[test]
    fn an_unlisted_model_id_passes_through_raw() {
        for id in ["13K", "15K", "20K", "99Z"] {
            let mut s = spe_fixture();
            s.model = id.into();
            assert_eq!(spe_dto(&s).model, id);
        }
    }

    /// ⚠️ NO SCALE LETTER ON AN SPE TEMPERATURE. §5 says "Temp in °C or F" — the amplifier
    /// reports whatever its own front panel is set to and the wire does not say which. A guessed
    /// °C is a false statement half the time.
    #[test]
    fn only_the_kpa_temperature_is_known_to_be_celsius() {
        assert!(!spe_dto(&spe_fixture()).temp_celsius);
        assert_eq!(spe_dto(&spe_fixture()).temp, Some(41));

        let k = kpa_dto(&kpa_fixture());
        assert!(k.temp_celsius, "^TM is documented Celsius, 0-150");
        assert_eq!(k.temp, Some(52));
    }

    fn kpa_fixture() -> KpaStatus {
        KpaStatus {
            operate: true,
            band_index: 5,
            temp_c: 52,
            volts: 48.0,
            amps: 32.5,
            output_watts: 480,
            swr: Some(1.5),
            fault: 0,
        }
    }

    /// A zero SWR is the amplifier saying it has no reading, not a perfect match. Elecraft says
    /// so outright (`^WS` reads `000` off air) and SPE says "Zero on receive"; printing `0.0:1`
    /// would be a fabricated reading that looks like the best possible one.
    #[test]
    fn a_zero_swr_is_absence_not_a_perfect_match() {
        let mut s = spe_fixture();
        s.swr_antenna = 0.0;
        s.swr_atu = 0.0;
        let d = spe_dto(&s);
        assert_eq!(d.swr, None);
        assert_eq!(d.swr_atu, None);
        // CONTROL: a real SWR is carried through, so the filter is not swallowing readings.
        let d = spe_dto(&spe_fixture());
        assert_eq!(d.swr, Some(1.4));
        assert_eq!(d.swr_atu, Some(1.1));
    }

    /// What the KPA does not report must be `None`, never a confident `false`/`0`.
    #[test]
    fn the_kpa_reports_no_transmitting_flag_and_says_so() {
        let d = kpa_dto(&kpa_fixture());
        assert_eq!(d.transmitting, None, "no polled verb reports it");
        assert_eq!(d.swr_atu, None, "the KPA has no pre-ATU SWR");
        assert_eq!(d.model, "", "no polled verb reports a model id");
        assert_eq!(d.warning, "", "empty = this family has no warning channel");
        assert!(!d.alarm_raised);

        // A fault is the KPA's own judgement and drives the same flag the SPE alarm does.
        let mut s = kpa_fixture();
        s.fault = 4;
        let d = kpa_dto(&s);
        assert!(d.alarm_raised, "a KPA fault must colour like an SPE alarm");
        assert_eq!(d.alarm, "fault");
        assert_eq!(d.kpa_fault, Some(4));
    }

    /// ⭐ AN EXPERT 1K-FA IS NOT "NO AMPLIFIER". It is a working link speaking the other SPE
    /// dialect, and its owner must be told that rather than sent to check their cabling.
    #[test]
    fn error_kinds_map_to_the_four_reason_tokens() {
        use std::io::{Error, ErrorKind as K};
        let cases = [
            (K::Unsupported, "wrongModel"),
            (K::InvalidData, "malformed"),
            (K::PermissionDenied, "portBusy"),
            (K::ResourceBusy, "portBusy"),
            (K::TimedOut, "noAnswer"),
            (K::NotFound, "noAnswer"),
            (K::BrokenPipe, "noAnswer"),
        ];
        for (kind, want) in cases {
            let got = reason_for(&Error::new(kind, "x"));
            assert_eq!(got, want, "{kind:?}");
            assert!(
                AMP_REASONS.contains(&got),
                "{got} is not in the wire vocabulary"
            );
        }
        // CONTROL: the mapping discriminates. If every kind returned the same token the loop
        // above would pass just as happily.
        assert_ne!(
            reason_for(&Error::new(K::Unsupported, "x")),
            reason_for(&Error::new(K::TimedOut, "x"))
        );
    }

    /// ⭐ AN AMPLIFIER THAT IS SWITCHED OFF MUST NOT COST A PORT SWEEP EVERY SECOND. Without the
    /// backoff the KPA's four-rate sweep — four port opens at 250 ms — reruns forever at 1 Hz.
    #[test]
    fn the_failed_open_backoff_grows_and_is_capped() {
        assert_eq!(backoff_ms(0), 0, "a healthy link waits for nothing");
        assert_eq!(backoff_ms(1), 1_000);
        assert_eq!(backoff_ms(2), 2_000);
        assert_eq!(backoff_ms(3), 4_000);
        assert_eq!(backoff_ms(6), 32_000);
        // Capped, and it STAYS capped rather than growing without bound or wrapping.
        for n in [7u32, 20, 1000, u32::MAX] {
            assert!(
                backoff_ms(n) <= MAX_BACKOFF_MS,
                "{n} failures waited {} ms",
                backoff_ms(n)
            );
        }
        assert_eq!(backoff_ms(u32::MAX), 32_000);
    }

    /// A successful reading is `linked` with no reason; the reason field is for failures only.
    #[test]
    fn a_successful_reading_carries_no_reason() {
        for d in [spe_dto(&spe_fixture()), kpa_dto(&kpa_fixture())] {
            assert!(d.linked);
            assert_eq!(d.reason, "");
            assert!(!d.family.is_empty());
        }
    }
}
