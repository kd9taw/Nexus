//! Shared desktop band-picker list. Transmit badges are display only; the
//! native transmit privilege gate remains authoritative.

pub fn licensed_bands(
    class: crate::settings::LicenseClass,
    mode: crate::settings::OperatingMode,
) -> Vec<crate::bandplan::BandChannel> {
    use crate::bandplan::BandChannel;
    use crate::settings::OperatingMode;
    // Band, UI group, and a LISTENING dial — somewhere sensible to park when this class has
    // no transmit segment here (#184, akhepcat: "there are no restrictions on receiving").
    // These are calling/activity frequencies, not segment starts, because a receive-only row
    // has no segment to start at.
    const BANDS: &[(&str, &str, f64)] = &[
        ("160m", "HF", 1.845),
        ("80m", "HF", 3.573),
        ("40m", "HF", 7.074),
        ("30m", "HF", 10.136),
        ("20m", "HF", 14.074),
        ("17m", "HF", 18.100),
        ("15m", "HF", 21.074),
        ("12m", "HF", 24.915),
        ("10m", "HF", 28.074),
        ("6m", "VHF", 50.313),
        // 4 m is IARU Region 1 only — the US has no allocation at any class (#75). It sits
        // here rather than being left to the FT dropdown because the privilege filter below
        // is what decides who sees it: a US class holds no 4 m segment and never sees the
        // row, while the non-US `Open` class does, in SSB and CW as well as in FT8.
        ("4m", "VHF", 70.200),
        ("2m", "VHF", 144.174),
        ("1.25m", "VHF", 222.100),
        ("70cm", "UHF", 432.174),
        // Batch 3: the named microwave bands. Per-class privilege filtering below keeps
        // each operator's dropdown honest automatically — a band whose class holds no
        // segment (9 cm for every US class) is omitted for them and present for Open.
        ("33cm", "UHF", 903.100),
        ("23cm", "UHF", 1296.100),
        ("13cm", "UHF", 2304.100),
        ("9cm", "UHF", 3400.100),
        ("6cm", "UHF", 5760.100),
        ("3cm", "UHF", 10368.100),
        ("1.25cm", "UHF", 24192.100),
    ];
    let mut out = Vec::new();
    for (band, group, rx_dial) in BANDS {
        // PHONE goes through THE phone home (`privileges::phone_home`), which lifts an LSB
        // home clear of the segment edge — the bare edge is a dial the transmit gate refuses,
        // and this command recomputing it from `segment_start` is how the dropdown used to
        // publish an unkeyable 7.1250 for 40 m. Sideband comes from the same answer.
        // CW parks in the ACTIVITY window (14.030, not the dead 14.000 edge), clamped to the
        // licensed segment start so it never drops below privileges; digital on the start.
        // Sideband is digital-safe USB for both (the rig-mode policy forces CW in the CW
        // section regardless of this field).
        let home = if matches!(mode, OperatingMode::Phone) {
            crate::privileges::phone_home(class, band)
        } else {
            crate::privileges::segment_start(class, band, mode).map(|seg| {
                let dial = if matches!(mode, OperatingMode::Cw) {
                    crate::bandplan::cw_activity_mhz(band).map_or(seg, |a| a.max(seg))
                } else {
                    seg
                };
                (dial, "USB")
            })
        };
        // ⚠️ A BAND WITH NO TRANSMIT SEGMENT IS LISTED, NOT DROPPED (#184, akhepcat).
        //
        // This used to `if let Some(..)` and skip, which applied a TRANSMIT rule to a TUNING
        // list: no licence restricts listening, and the radio itself tunes there quite
        // happily. A US General could not select 4 m at all — not "could listen but not
        // key" — which is neither what the rules say nor what the rig does.
        //
        // So the row is emitted either way; `tx` carries which it is, and the UI marks the
        // receive-only ones. Nothing here reaches the transmit gate:
        // `privileges::tx_allowed` is untouched and still refuses the over, with the licence
        // reason, exactly as before.
        let (dial, sideband) = match home {
            Some((dial, sideband)) => (dial, sideband),
            None => (*rx_dial, "USB"),
        };
        // ⚠️ ASK THE GATE, do not re-derive it. The obvious spelling — "we found a segment
        // start, therefore transmit is allowed" — is WRONG for the `Open` class: it holds no
        // segments above 23 cm, yet `tx_allowed` short-circuits Open to true (it is the
        // non-US / undeclared class and is trusted), so that spelling labelled a non-US
        // operator's own microwave bands receive-only. Reading the real gate keeps this flag
        // and the refusal in agreement by construction rather than by duplicated logic.
        let tx = crate::privileges::tx_allowed(class, dial, mode);
        out.push(BandChannel {
            band: band.to_string(),
            group: group.to_string(),
            dial_mhz: dial,
            mode: sideband.to_string(),
            label: format!("{band} · {dial:.3} MHz"),
            note: String::new(),
            tx,
        });
    }
    out
}
