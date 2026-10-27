//! Shared native passband and band-boundary decisions for CAT retuning.

/// The width (Hz) to send with a mode, or `-1` (`RIG_PASSBAND_NOCHANGE`): the rig keeps its own.
///
/// ⛔ THE DATA MODES SEND NO WIDTH (#82, #107; operator ruling: match WSJT-X). Every
/// `rig_set_mode` WSJT-X makes passes `RIG_PASSBAND_NOCHANGE` (`HamlibTransceiver.cpp:929`,
/// `:1009`, `:1087`), so the DATA filter is the operator's, set at the radio. Nexus used to force
/// 3 kHz here (a radio recalling a 600 Hz DATA filter clips FT8), and it cost more than it saved:
/// a Flex over SmartSDR CAT answered with its own 6 kHz default (#82), and an FTdx10 operator who
/// wanted 4 kHz was held to 3 (#107). A width the OPERATOR commands (the cockpit's passband
/// control, `Rig::set_passband`) is a separate path and still goes out.
pub(crate) fn passband_for(md: &str) -> i32 {
    match md.trim().to_ascii_uppercase().as_str() {
        // AM is DOUBLE-sideband: the carrier sits in the middle with a sideband either side, so
        // an SSB-width filter cuts half the signal off and the audio comes out thin and distorted.
        // 6 kHz is the AM filter every HF rig that has one offers. Rigs that round to their
        // nearest own filter are fine — the read-back check treats a nearby width as the radio
        // doing its job, not a fault.
        "AM" => 6000,
        _ => -1,
    }
}

/// Are `a` and `b` (Hz) on the SAME NAMED amateur band — i.e. does a retune between them
/// cross no band boundary?
///
/// ⚠️ `None == None` is NOT "in-band": two dials the band plan cannot name (47 GHz+, or one
/// named and one not) may sit on different band registers inside the rig, and reading that
/// equality as same-band is how a band-dependent correction gets skipped exactly where the
/// rig's band memory is least predictable. Only two EQUAL NAMED bands count. A `0` — the
/// "no dial pushed yet" sentinel — is unnamed, so it is never the same band as anything.
pub(crate) fn same_named_band(a: u64, b: u64) -> bool {
    let band_of = |hz: u64| tempo_app::bandplan::band_for_dial(hz as f64 / 1e6);
    matches!((band_of(a), band_of(b)), (Some(x), Some(y)) if x == y)
}

/// The passband to send WITH the mode on an operator force retune: does this retune have to
/// re-command the width, or may it leave the rig's filter where the operator put it?
///
/// THE BUG (#67). The force path sent [`passband_for`]'s width on EVERY retune — it consulted
/// only "is `md` non-empty", never whether anything about the mode or the band had actually
/// changed — so every plain dial move re-sent the width: a filter switch and a Width-display pop
/// per QSY, on a rig that was already exactly where we wanted it. (The DATA modes it was found
/// in carry no width at all now — see [`passband_for`]; AM's 6 kHz is what this still gates.)
///
/// Not gated on `mode_changed` alone: a rig recalls a per-band filter on a BAND change, which
/// routinely arrives with the mode UNCHANGED. So the gate is "in-band, dial-only": send `-1`
/// (`RIG_PASSBAND_NOCHANGE`) only when the mode did not change AND the band did not change;
/// keep the width in every other case.
pub(crate) fn retune_passband(md: &str, mode_changed: bool, prev_dial: u64, dial: u64) -> i32 {
    if !mode_changed && same_named_band(prev_dial, dial) {
        -1
    } else {
        passband_for(md)
    }
}
