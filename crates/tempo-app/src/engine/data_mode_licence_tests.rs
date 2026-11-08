//! The licence gate judges MSK144 where it transmits, and holds two US data limits by mode.
//!
//! - MSK144 keys 1000 and 2000 Hz above the dial (`msk144::TX_CENTRE_HZ` is 1500) whatever the
//!   TX marker says, so the gate judges that signal, dial + 1500 ± 1395 Hz (its -26 dB main lobe),
//!   as well as the marker it judged before.
//! - 47 CFR 97.307(f)(3) and (f)(14) cap data at 2.8 kHz below 28.3 MHz: Q65-30E and Q65-15D
//!   (about 3.47 kHz) are refused there for a US class.
//! - 97.307(f)(3) caps 2200 m and 630 m at 300 baud: MSK144 (2000 Bd) is refused there.
//!
//! Each of those only refuses more. The sweep at the bottom holds that across every class, band
//! edge, mode, offset and sideband: nothing keys now that was refused before, and nothing but
//! MSK144 and the two wide Q65 submodes changes at all. The lock names the rule, through the
//! snapshot's `dataModeRefusal`, only where the mode is the whole reason.

use super::tx_gate_table::station;
use super::*;

/// Every US class, as the operator's verb names it (`privileges::US_CLASSES` in order).
const US: [&str; 5] = ["novice", "technician", "general", "advanced", "extra"];
/// Every US class that holds 6 m and 2 m (a Novice holds neither).
const VHF_CLASSES: [&str; 4] = ["technician", "general", "advanced", "extra"];
/// Every US class that holds 2200 m, 630 m, 60 m and 20 m data.
const HF_CLASSES: [&str; 3] = ["general", "advanced", "extra"];

/// A Digital station as `class` on `tier` at `dial` (`sideband`), its TX marker at `offset`.
/// The tier first, because entering MSK144 parks the marker on 1500; then the dial and the
/// marker, each checked.
fn digital_on(class: &str, tier: Tier, dial: f64, sideband: &str, offset: f32) -> Engine {
    let band = crate::bandplan::band_for_dial(dial).unwrap_or("");
    let mut e = station(3073, class, "digital", dial, band, sideband);
    e.set_tier(tier);
    e.set_frequency(dial, band, sideband);
    e.set_tx_offset(offset);
    assert_eq!(e.app.tier(), tier, "precondition: the tier is {tier:?}");
    assert!(
        (e.settings.dial_mhz - dial).abs() < 1e-9,
        "precondition: the tune landed on {dial}"
    );
    assert_eq!(
        e.tx_offset_hz(),
        offset,
        "precondition: the marker is at {offset}"
    );
    e
}

/// [`digital_on`] on USB.
fn digital(class: &str, tier: Tier, dial: f64, offset: f32) -> Engine {
    digital_on(class, tier, dial, "USB", offset)
}

/// [`digital`] on Q65 at `period_s` and `submode` (0..=4 for A..E).
fn q65(class: &str, period_s: u16, submode: u8, dial: f64, offset: f32) -> Engine {
    let mut e = digital(class, Tier::Q65, dial, offset);
    e.settings.q65_period_s = period_s;
    e.settings.q65_submode = submode;
    e
}

/// The gate as it judged before this change: the same frame, with no rule of its mode's.
fn as_before(e: &Engine) -> bool {
    e.tx_allowed_for(DataFrame {
        mode: None,
        ..e.tx_data_frame()
    })
}

/// What the lock line reads: the snapshot's `dataModeRefusal` as (`rule`, `mode`).
fn told(e: &Engine) -> Option<(String, String)> {
    let radio = serde_json::to_value(e.snapshot().radio).expect("the snapshot serializes");
    let why = radio.get("dataModeRefusal")?;
    let field = |k: &str| {
        why.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    Some((field("rule"), field("mode")))
}

#[test]
fn msk144_with_its_marker_dragged_up_is_judged_where_it_transmits() {
    // Dial 50.0965 with the marker at 4000 judges 50.1005, inside the data segment, while the
    // signal sits in 50.0966-50.0994: the 50.0-50.1 CW-only segment. The same at 144.1.
    let mut wrong = Vec::new();
    for class in VHF_CLASSES {
        for dial in [50.0965, 144.0965] {
            let e = digital(class, Tier::Msk144, dial, 4000.0);
            if e.tx_allowed() {
                wrong.push(format!("{class} {dial:.4} marker 4000 keyed"));
            }
            if told(&e) != Some(("fixedCentre".into(), "MSK144-15".into())) {
                wrong.push(format!("{class} {dial:.4} marker 4000 told {:?}", told(&e)));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "MSK144 must be judged at 1500 Hz: {wrong:#?}"
    );
}

#[test]
fn msk144_with_its_marker_parked_is_judged_across_its_width() {
    // Dial 50.0990 with the marker on 1500 judges 50.1005, but the signal's -26 dB main lobe runs
    // from 50.099105, under 50.1. The same at 144.1.
    let mut wrong = Vec::new();
    for class in VHF_CLASSES {
        for dial in [50.0990, 144.0990] {
            let e = digital(class, Tier::Msk144, dial, 1500.0);
            if e.tx_allowed() {
                wrong.push(format!("{class} {dial:.4} marker 1500 keyed"));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "MSK144's low side must be judged: {wrong:#?}"
    );
}

#[test]
fn msk144_at_the_top_of_6_m_is_judged_across_its_width() {
    // Dial 53.9985 with the marker at 200 judges 53.9987; the signal runs to 54.0013.
    let mut wrong = Vec::new();
    for class in VHF_CLASSES {
        let e = digital(class, Tier::Msk144, 53.9985, 200.0);
        if e.tx_allowed() {
            wrong.push(format!("{class} 53.9985 marker 200 keyed"));
        }
    }
    assert!(
        wrong.is_empty(),
        "MSK144's top must stay under 54 MHz: {wrong:#?}"
    );
}

#[test]
fn msk144_is_judged_1395_hz_either_side_of_its_centre_to_the_hertz() {
    // The width itself, at both 6 m edges. The signal runs from dial + 105 Hz to dial + 2895 Hz,
    // so the lowest dial that keeps it above 50.1 MHz is 50.099895, and the highest that keeps it
    // under 54.0 is 53.997105: one hertz either side of each keys and is refused. A half-width of
    // 1000 or 1600 Hz moves both edges by hundreds of hertz and turns this red.
    let mut wrong = Vec::new();
    for class in VHF_CLASSES {
        for (dial, keys) in [
            (50.099896, true),
            (50.099894, false),
            (53.997104, true),
            (53.997106, false),
        ] {
            // Each marker lands inside the segment on every one of these dials, so only the
            // signal's span decides.
            for marker in [200.0, 1500.0] {
                let e = digital(class, Tier::Msk144, dial, marker);
                if e.tx_allowed() != keys {
                    wrong.push(format!(
                        "{class} {dial:.6} marker {marker}: keyed={}",
                        !keys
                    ));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "MSK144's ±1395 Hz edges moved: {wrong:#?}"
    );
}

#[test]
fn msk144_on_630_m_lsb_is_named_for_its_symbol_rate_where_its_signal_fits() {
    // Dial 0.4790 LSB: the dial is the band top, but the signal sits below it, 0.476105-0.478895
    // MHz, wholly inside 630 m. Only the 300-baud cap refuses it, so the lock names that, not the
    // fixed centre, whose "outside your license privileges" would be false here.
    let mut wrong = Vec::new();
    for class in HF_CLASSES {
        let e = digital_on(class, Tier::Msk144, 0.4790, "LSB", 1500.0);
        if e.tx_allowed() {
            wrong.push(format!("{class} 0.4790 LSB keyed"));
        }
        if told(&e) != Some(("tooFast".into(), "MSK144-15".into())) {
            wrong.push(format!("{class} 0.4790 LSB told {:?}", told(&e)));
        }
    }
    assert!(
        wrong.is_empty(),
        "the reason must be where the signal is: {wrong:#?}"
    );
}

#[test]
fn msk144_is_refused_on_2200_m_and_630_m_for_every_us_class() {
    // 2000 baud against the 300-baud cap. On 2200 m the signal is also wider than the band; on
    // 630 m it fits, so there the symbol rate is the whole reason and the lock names it.
    let mut wrong = Vec::new();
    for class in HF_CLASSES {
        let e = digital(class, Tier::Msk144, 0.1360, 1500.0);
        if e.tx_allowed() {
            wrong.push(format!("{class} 2200 m 0.1360 keyed"));
        }
        let e = digital(class, Tier::Msk144, 0.4745, 1500.0);
        if e.tx_allowed() {
            wrong.push(format!("{class} 630 m 0.4745 keyed"));
        }
        if told(&e) != Some(("tooFast".into(), "MSK144-15".into())) {
            wrong.push(format!("{class} 630 m 0.4745 told {:?}", told(&e)));
        }
        // The control: a narrow mode keys on the same 630 m dial.
        if !digital(class, Tier::Fst4, 0.4745, 1500.0).tx_allowed() {
            wrong.push(format!("{class} 630 m FST4 refused"));
        }
    }
    // Open is not a US class and is trusted, as everywhere.
    if !digital("open", Tier::Msk144, 0.4745, 1500.0).tx_allowed() {
        wrong.push("open 630 m refused".into());
    }
    assert!(wrong.is_empty(), "300 baud on 2200 m and 630 m: {wrong:#?}");
}

#[test]
fn q65_wider_than_2_8_khz_is_refused_below_28_3_mhz() {
    let mut wrong = Vec::new();
    for class in HF_CLASSES {
        // 20 m, 60 m's segment and 2200 m: Q65-30E and Q65-15D refused, named.
        for (dial, offset) in [(14.074, 1000.0), (5.3570, 200.0), (0.1357, 200.0)] {
            for (period, sub, name) in [(30, 4, "Q65-30E"), (15, 3, "Q65-15D")] {
                let e = q65(class, period, sub, dial, offset);
                if e.tx_allowed() {
                    wrong.push(format!("{class} {dial:.4} {name} keyed"));
                }
                if told(&e) != Some(("tooWide".into(), name.into())) {
                    wrong.push(format!("{class} {dial:.4} {name} told {:?}", told(&e)));
                }
            }
        }
        // Q65-30D, 1.73 kHz, still keys on 20 m and tells nothing.
        let e = q65(class, 30, 3, 14.074, 1000.0);
        if !e.tx_allowed() || told(&e).is_some() {
            wrong.push(format!(
                "{class} 14.074 Q65-30D: {} {:?}",
                e.tx_allowed(),
                told(&e)
            ));
        }
    }
    // 6 m has no such cap: Q65-30E keys there.
    for class in VHF_CLASSES {
        if !q65(class, 30, 4, 50.275, 1500.0).tx_allowed() {
            wrong.push(format!("{class} 50.275 Q65-30E refused"));
        }
    }
    // A Technician's 10 m data is under the cap too.
    if q65("technician", 30, 4, 28.120, 1000.0).tx_allowed() {
        wrong.push("technician 28.120 Q65-30E keyed".into());
    }
    if !q65("open", 30, 4, 14.074, 1000.0).tx_allowed() {
        wrong.push("open 14.074 Q65-30E refused".into());
    }
    assert!(wrong.is_empty(), "the 2.8 kHz data cap: {wrong:#?}");
}

#[test]
fn a_frequency_refusal_keeps_the_frequency_sentence() {
    // MSK144 deep in 50.0-50.1: the marker is refused too, so the frequency is the reason.
    let e = digital("general", Tier::Msk144, 50.050, 1500.0);
    assert!(!e.tx_allowed(), "precondition: refused");
    assert_eq!(told(&e), None, "the frequency is the reason, not the mode");
    // Q65-30E in the 20 m phone segment: refused for the frequency as before.
    let e = q65("general", 30, 4, 14.200, 1000.0);
    assert!(!e.tx_allowed(), "precondition: refused");
    assert_eq!(told(&e), None, "the frequency is the reason, not the mode");
    // And nothing at all while transmit is allowed.
    let e = digital("general", Tier::Msk144, 50.260, 1500.0);
    assert!(e.tx_allowed(), "precondition: allowed");
    assert_eq!(told(&e), None);
}

#[test]
fn every_plan_dial_still_keys_at_its_default_offset() {
    // Every calling dial of every tier, for every US class, at the marker the tier parks on: if
    // the gate allowed it before, it still does. MSK144's dials are named outright.
    let mut wrong = Vec::new();
    for class in US {
        for tier in Tier::ALL {
            for c in crate::bandplan::band_plan_for(tier)
                .into_iter()
                .filter(|c| c.tx)
            {
                let sideband = if c.mode.eq_ignore_ascii_case("LSB") {
                    "LSB"
                } else {
                    "USB"
                };
                let e = digital_on(class, tier, c.dial_mhz, sideband, 1500.0);
                if as_before(&e) && !e.tx_allowed() {
                    wrong.push(format!("{class} {tier:?} {:.6} refused", c.dial_mhz));
                }
            }
        }
    }
    for class in VHF_CLASSES {
        for dial in [50.260, 50.380, 144.150, 144.360, 432.360] {
            if !digital(class, Tier::Msk144, dial, 1500.0).tx_allowed() {
                wrong.push(format!("{class} MSK144 {dial:.3} refused"));
            }
        }
    }
    assert!(wrong.is_empty(), "a plan dial stopped keying: {wrong:#?}");
}

#[test]
fn nothing_keys_now_that_was_refused_before() {
    // Every class (Open too), every digital tier and every Q65 submode at 15 s and 30 s, on and
    // around every data edge any US class has, at three markers and on both sidebands. A case is
    // allowed now only if it was allowed before; it differs from before only for MSK144 and the
    // two wide Q65 submodes under a US class; and some cases do differ, so the sweep is not vacuous.
    let mut edges: Vec<f64> = crate::privileges::US_CLASSES
        .iter()
        .flat_map(|&c| crate::privileges::allowed_spans(c, crate::settings::OperatingMode::Digital))
        .flatten()
        .flat_map(|(lo, hi)| [lo, hi])
        .collect();
    edges.sort_by(f64::total_cmp);
    edges.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let dials: Vec<f64> = edges
        .iter()
        .flat_map(|&edge| {
            [
                -3000.0, -2900.0, -1500.0, -200.0, 0.0, 200.0, 1500.0, 2900.0, 3000.0,
            ]
            .map(|hz: f64| edge + hz / 1e6)
        })
        .filter(|&f| f > 0.13)
        .collect();
    let mut kinds: Vec<(Tier, Option<(u16, u8)>)> = Tier::ALL
        .iter()
        .filter(|&&t| t != Tier::Q65)
        .map(|&t| (t, None))
        .collect();
    for period in [15u16, 30] {
        for sub in 0u8..5 {
            kinds.push((Tier::Q65, Some((period, sub))));
        }
    }
    let mut looser = Vec::new();
    let mut elsewhere = Vec::new();
    let mut stricter = 0usize;
    let mut judged = 0usize;
    for class in US.into_iter().chain(["open"]) {
        let mut e = station(3073, class, "digital", 14.074, "20m", "USB");
        for &(tier, q) in &kinds {
            e.set_tier(tier);
            if let Some((period, sub)) = q {
                e.settings.q65_period_s = period;
                e.settings.q65_submode = sub;
            }
            let wide_q65 = matches!(q, Some((30, 4)) | Some((15, 3)) | Some((15, 4)));
            let changes = class != "open" && (tier == Tier::Msk144 || wide_q65);
            for &dial in &dials {
                let band = crate::bandplan::band_for_dial(dial).unwrap_or("");
                for sideband in ["USB", "LSB"] {
                    e.set_frequency(dial, band, sideband);
                    for offset in [200.0, 1500.0, 4000.0] {
                        e.set_tx_offset(offset);
                        let (now, before) = (e.tx_allowed(), as_before(&e));
                        judged += 1;
                        let case =
                            || format!("{class} {tier:?} {q:?} {dial:.6} {sideband} {offset}");
                        if now && !before {
                            looser.push(case());
                        } else if now != before {
                            if changes {
                                stricter += 1;
                            } else {
                                elsewhere.push(case());
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(looser.is_empty(), "keyed now, refused before: {looser:#?}");
    assert!(
        elsewhere.is_empty(),
        "changed outside MSK144 and wide Q65: {elsewhere:#?}"
    );
    assert!(
        stricter > 0,
        "no case of {judged} refused more: the sweep proves nothing"
    );
}
