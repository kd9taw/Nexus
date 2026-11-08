//! Shared desktop band-picker list. Transmit badges are display only; the
//! native transmit privilege gate remains authoritative.

pub fn licensed_bands(
    class: crate::settings::LicenseClass,
    mode: crate::settings::OperatingMode,
) -> Vec<crate::bandplan::BandChannel> {
    licensed_bands_for(class, mode, 0)
}

/// [`licensed_bands`] for the radio with Hamlib model `rig_model` (0 = unknown): the rows the
/// Phone/CW menu lists and the band pick lands on, whose 3 cm home follows the radio
/// ([`crate::bandplan::activity_home_mhz`]).
pub fn licensed_bands_for(
    class: crate::settings::LicenseClass,
    mode: crate::settings::OperatingMode,
    rig_model: u32,
) -> Vec<crate::bandplan::BandChannel> {
    use crate::bandplan::BandChannel;
    use crate::settings::{LicenseClass, OperatingMode};
    // Band and UI group, low band to high.
    const BANDS: &[(&str, &str)] = &[
        ("160m", "HF"),
        ("80m", "HF"),
        // 60 m: `Open` parks in the WRC-15 segment; a US class lists it at the same dial, where
        // the gate judges its channelised privileges as it does everywhere else.
        ("60m", "HF"),
        ("40m", "HF"),
        ("30m", "HF"),
        ("20m", "HF"),
        ("17m", "HF"),
        ("15m", "HF"),
        ("12m", "HF"),
        ("10m", "HF"),
        ("6m", "VHF"),
        // 4 m is IARU Region 1 only — the US has no allocation at any class (#75). A US class
        // sees it receive-only; the non-US `Open` class keys it, in SSB and CW as well as FT8.
        ("4m", "VHF"),
        ("2m", "VHF"),
        ("1.25m", "VHF"),
        ("70cm", "UHF"),
        // Batch 3: the named microwave bands. A band whose class holds no segment (9 cm for
        // every US class) is listed receive-only for them.
        ("33cm", "UHF"),
        ("23cm", "UHF"),
        ("13cm", "UHF"),
        ("9cm", "UHF"),
        ("6cm", "UHF"),
        ("3cm", "UHF"),
        ("1.25cm", "UHF"),
    ];
    // PHONE goes through THE phone home (`privileges::phone_home`), which lifts an LSB home
    // clear of the segment edge — the bare edge is a dial the transmit gate refuses, and this
    // command recomputing it from `segment_start` is how the dropdown used to publish an
    // unkeyable 7.1250 for 40 m. Sideband comes from the same answer.
    // CW parks in the ACTIVITY window (14.030, not the dead 14.000 edge) through THE CW home
    // (`privileges::cw_home`), which section entry reads too; digital on the start.
    // Sideband is digital-safe USB for both (the rig-mode policy forces CW in the CW
    // section regardless of this field).
    let home = |class: LicenseClass, band: &str| {
        if matches!(mode, OperatingMode::Phone) {
            crate::privileges::phone_home_for(class, band, rig_model)
        } else if matches!(mode, OperatingMode::Cw) {
            crate::privileges::cw_home_for(class, band, rig_model).map(|dial| (dial, "USB"))
        } else {
            crate::privileges::segment_start(class, band, mode).map(|seg| (seg, "USB"))
        }
    };
    let mut out = Vec::new();
    for (band, group) in BANDS {
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
        //
        // A receive-only row LISTENS where `Open` would park — the worldwide parking table,
        // so a Technician picking 20 m in Phone hears the phone segment, not the FT8 hole —
        // and the band pick lands on exactly this row (`Engine::band_pick_default`), so a
        // listed band is never a pick that does nothing. `Open` parks on every band listed
        // here (`open_parks_inside_the_amateur_allocation_of_every_region`), so no row is lost.
        let Some((dial, sideband)) = home(class, band).or_else(|| home(LicenseClass::Open, band))
        else {
            continue;
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
