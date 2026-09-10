//! Shared native passband and band-boundary decisions for CAT retuning.

pub(crate) fn passband_for(md: &str) -> i32 {
    match md.trim().to_ascii_uppercase().as_str() {
        "PKTUSB" | "PKTLSB" => 3000,
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
/// changed. So in FT8, where `passband_for` deliberately forces 3 kHz, every plain dial move
/// (a spot click, a Needed pick, a section QSY) re-sent `M PKTUSB 3000`: a DATA-filter switch
/// and a Width-display pop per QSY, on a rig that was already exactly where we wanted it.
///
/// The 3 kHz force itself is NOT removable and this must not be gated on `mode_changed` alone.
/// It exists because a rig recalls a narrow per-band DATA filter — 600 Hz on the FTDX10 that
/// prompted it, which clips FT8 — and it recalls it on a BAND change, which routinely arrives
/// with the mode UNCHANGED. So the gate is "in-band, dial-only": send `-1`
/// (`RIG_PASSBAND_NOCHANGE`) only when the mode did not change AND the band did not change;
/// keep the width in every other case.
pub(crate) fn retune_passband(md: &str, mode_changed: bool, prev_dial: u64, dial: u64) -> i32 {
    if !mode_changed && same_named_band(prev_dial, dial) {
        -1
    } else {
        passband_for(md)
    }
}
