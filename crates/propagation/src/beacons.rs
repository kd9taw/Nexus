//! One-way transmissions that must never score as a need: the NCDXF/IARU beacon
//! ladder and the ARRL W1AW code-practice / bulletin schedule.
//!
//! **The trap this exists to close.** 14.100 MHz carries the NCDXF/IARU International
//! Beacon Project — eighteen beacons in eighteen DXCC entities, each taking a 10-second
//! slot so every one of them transmits on every band once every three minutes, day and
//! night. Any evidence source pointed at that frequency therefore offers 4U1UN as an
//! all-time-new entity forever, then VE8AT, then W6WX, and around again. The same is
//! true of W1AW: its code practice and bulletins are one-way broadcasts, so a W1AW row
//! on the Needed board is a QSY into a station that will never answer.
//!
//! A beacon being audible is genuine, valuable propagation evidence — so this module
//! **only suppresses SCORING**. Beacons still appear in the spot firehose and on the
//! band map, badged for what they are. Never suppress the display; that is the same
//! "show it, label it, never score it" doctrine the spot-trust work follows.
//!
//! ## Two deliberately different match rules
//!
//! The rules differ because the frequencies differ in kind, and getting this wrong in
//! either direction costs the operator a real contact:
//!
//! - **NCDXF: frequency alone, any callsign.** The IBP frequencies are beacon-exclusive
//!   by IARU band plan (14.099–14.101 is reserved for the project), so anything decoded
//!   or spotted there is the ladder — or a mis-copy of it. Matching on frequency also
//!   catches a garbled beacon callsign, which a call list never would.
//! - **W1AW: frequency AND the call must be exactly `W1AW`.** The voice bulletin
//!   frequencies (14.290, 7.290, …) sit in busy phone segments where real DX operates,
//!   so a frequency-only rule would suppress genuine contacts. `W1AW/4` and friends are
//!   deliberately NOT matched: portable W1AW operations are ordinary two-way contacts
//!   worth working, unlike the Newington bulletin transmissions.
//!
//! Note what is NOT done here: the beacon CALLSIGNS are never suppressed on their own.
//! 4U1UN is both an IBP beacon and the United Nations HQ station — a real, workable DXCC
//! entity. Suppressing the call everywhere would hide an ATNO; suppressing the frequency
//! hides only the beacon. [`NCDXF_CALLS`] is here to document the ladder (and to pin
//! that rule by test), not to gate anything.
//!
//! Sources, both fetched and verified 2026-07-30:
//! - NCDXF/IARU IBP schedule — <https://www.ncdxf.org/beacon/beaconschedule.html>
//! - W1AW operating schedule — <https://www.arrl.org/w1aw-operating-schedule>
//!
//! The UI mirrors the beacon ladder (call + QTH + slot arithmetic, for the beacon
//! monitor) in `ui/src/features/beacons.ts`; the two lists must agree.

use serde::{Deserialize, Serialize};

/// What kind of one-way transmission a frequency match identified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BeaconKind {
    /// An NCDXF/IARU International Beacon Project slot.
    Ncdxf,
    /// An ARRL W1AW code-practice or bulletin transmission.
    W1aw,
}

impl BeaconKind {
    /// Full description for a tooltip / evidence line.
    pub fn label(self) -> &'static str {
        match self {
            BeaconKind::Ncdxf => "NCDXF/IARU beacon — one-way, not workable",
            BeaconKind::W1aw => "W1AW bulletin / code practice — one-way, not workable",
        }
    }
    /// Short badge text for a dense row.
    pub fn short(self) -> &'static str {
        match self {
            BeaconKind::Ncdxf => "BEACON",
            BeaconKind::W1aw => "W1AW",
        }
    }
}

/// The eighteen NCDXF/IARU beacons, in transmission order — and eighteen distinct DXCC
/// entities, which is precisely why an unfiltered evidence source on 14.100 reads as an
/// endless supply of new ones. Documentation and test fixture only: the classifier below
/// deliberately does not consult this list (see the module header).
pub const NCDXF_CALLS: [&str; 18] = [
    "4U1UN", "VE8AT", "W6WX", "KH6RS", "ZL6B", "VK6RBP", "JA2IGY", "RR9O", "VR2B", "4S7B", "ZS6DN",
    "5Z4B", "4X6TU", "OH2B", "CS3B", "LU4AA", "OA4B", "YV5B",
];

/// The five International Beacon Project frequencies (MHz). Each beacon transmits on
/// each of these for 10 seconds, cycling every three minutes.
pub const NCDXF_MHZ: [f64; 5] = [14.100, 18.110, 21.150, 24.930, 28.200];

/// W1AW code-practice and bulletin frequencies (MHz), as published by ARRL. Grouped by
/// service; 50.350 and 147.555 carry all three services and so appear once.
///
/// `rustfmt::skip` because reflowing this collapses the per-service grouping comments onto
/// the wrong rows, which turns a correct table into a misleading one.
#[rustfmt::skip]
pub const W1AW_MHZ: [f64; 22] = [
    // Code practice + CW bulletins (sent at 18 wpm).
    1.8025, 3.5815, 7.0475, 14.0475, 18.0775, 21.0675, 28.0675,
    // Digital bulletins (Baudot / PSK31 / MFSK16 on a rotating schedule).
    3.5975, 7.095, 14.095, 18.1025, 21.095, 28.095,
    // Voice bulletins (7.290 is AM double-sideband full-carrier).
    1.855, 3.99, 7.29, 14.29, 18.16, 21.39, 28.59,
    // 6 m + 2 m, carried on all three services.
    50.350, 147.555,
];

/// Half-width of the frequency match (MHz). 1 kHz either side: it is the width the IARU
/// band plan reserves around an IBP frequency (14.099–14.101 for 14.100), and it is about
/// as tightly as a human cluster spot can be trusted. Wider would start suppressing real
/// stations on the shared W1AW voice frequencies.
const WINDOW_MHZ: f64 = 0.001;

fn near(freq_mhz: f64, table: &[f64]) -> bool {
    table.iter().any(|&f| (freq_mhz - f).abs() <= WINDOW_MHZ)
}

/// Identify a beacon or bulletin transmission from a spot's callsign and frequency.
/// `Some(kind)` means "display it, badge it, do not score it".
///
/// See the module header for why NCDXF matches on frequency alone while W1AW also
/// requires the callsign.
pub fn classify(call: &str, freq_mhz: f64) -> Option<BeaconKind> {
    if near(freq_mhz, &NCDXF_MHZ) {
        return Some(BeaconKind::Ncdxf);
    }
    if call.trim().eq_ignore_ascii_case("W1AW") && near(freq_mhz, &W1AW_MHZ) {
        return Some(BeaconKind::W1aw);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ibp_ladder_is_eighteen_entities_on_five_bands() {
        // The shape of the problem: 18 calls × 5 bands, every three minutes, forever.
        assert_eq!(NCDXF_CALLS.len(), 18);
        assert_eq!(NCDXF_MHZ.len(), 5);
        // Transmission order starts at the UN and the list is duplicate-free.
        assert_eq!(NCDXF_CALLS[0], "4U1UN");
        let mut sorted: Vec<&str> = NCDXF_CALLS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 18, "a beacon call is listed twice");
    }

    #[test]
    fn every_beacon_call_on_a_beacon_frequency_is_suppressed() {
        // The 4U1UN-as-an-ATNO-forever case, for all eighteen and all five bands.
        for call in NCDXF_CALLS {
            for mhz in NCDXF_MHZ {
                assert_eq!(
                    classify(call, mhz),
                    Some(BeaconKind::Ncdxf),
                    "{call} on {mhz} must be recognised as a beacon"
                );
            }
        }
    }

    #[test]
    fn an_ibp_frequency_suppresses_any_call() {
        // Frequency alone, because a garbled beacon copy is still a beacon — and nothing
        // else is licensed to run there.
        assert_eq!(classify("4X6T", 14.100), Some(BeaconKind::Ncdxf));
        assert_eq!(classify("", 28.200), Some(BeaconKind::Ncdxf));
    }

    #[test]
    fn a_beacon_call_off_a_beacon_frequency_still_scores() {
        // 4U1UN is ALSO the United Nations HQ station — a real, workable entity. The
        // callsign must never be suppressed on its own or this hides an ATNO.
        assert_eq!(classify("4U1UN", 14.025), None);
        assert_eq!(classify("OH2B", 7.005), None);
        assert_eq!(classify("VE8AT", 21.250), None);
    }

    #[test]
    fn the_match_window_is_one_khz_either_side() {
        assert_eq!(classify("4U1UN", 14.0995), Some(BeaconKind::Ncdxf));
        assert_eq!(classify("4U1UN", 14.1005), Some(BeaconKind::Ncdxf));
        // Just outside: the IARU reservation ends, real stations begin.
        assert_eq!(classify("W3LPL", 14.0975), None);
        assert_eq!(classify("W3LPL", 14.1025), None);
    }

    #[test]
    fn w1aw_bulletins_are_suppressed_on_their_own_frequencies() {
        assert_eq!(classify("W1AW", 14.0475), Some(BeaconKind::W1aw));
        assert_eq!(classify("W1AW", 18.0775), Some(BeaconKind::W1aw));
        assert_eq!(classify("w1aw", 7.0475), Some(BeaconKind::W1aw));
        assert_eq!(classify(" W1AW ", 28.0675), Some(BeaconKind::W1aw));
    }

    #[test]
    fn w1aw_elsewhere_and_other_calls_on_w1aw_frequencies_still_score() {
        // Off its bulletin frequencies W1AW is an ordinary contact.
        assert_eq!(classify("W1AW", 14.025), None);
        // A portable W1AW operation is a real two-way contact, not a bulletin.
        assert_eq!(classify("W1AW/4", 14.0475), None);
        // 14.290 is a busy phone frequency — only W1AW itself is the bulletin.
        assert_eq!(classify("K1ABC", 14.29), None);
        // 7.0475 is both the W1AW 40 m CW bulletin AND the 40 m FT4 watering hole; an
        // FT4 station there must still score.
        assert_eq!(classify("DL1ABC", 7.0475), None);
    }

    #[test]
    fn beacon_kinds_carry_operator_facing_text() {
        assert_eq!(BeaconKind::Ncdxf.short(), "BEACON");
        assert_eq!(BeaconKind::W1aw.short(), "W1AW");
        for k in [BeaconKind::Ncdxf, BeaconKind::W1aw] {
            assert!(
                k.label().contains("not workable"),
                "the label must say why it is suppressed: {}",
                k.label()
            );
        }
    }
}
