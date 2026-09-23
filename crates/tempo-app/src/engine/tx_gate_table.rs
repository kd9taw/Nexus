//! ⛔ TODAY'S TRANSMIT-GATE DECISIONS, PINNED OVER A TABLE OF STATION STATES.
//!
//! The dual-receiver programme is about to put a second receiver into the engine, and its
//! first ruling (D1) says the licence gate should one day judge whichever receiver actually
//! TRANSMITS. That switch is a separate step with its own yes. Until it is taken, adding the
//! receiver model must not move a single decision, and "it doesn't touch `tx_allowed`" is a
//! claim about a diff, not about behaviour. This table is the behaviour.
//!
//! Every row builds a station state through the verbs the product itself uses (a band pick, a
//! section change, a split the radio loop acknowledged, a satellite pass) and records what
//! three consumers read:
//!
//! - `tx_allowed` — the key-time licence gate every transmit path ANDs in, read both through
//!   the verb and through the snapshot the cockpit's lock indicator renders;
//! - `tx_emission_mhz` — the frequency the lock names;
//! - `phone_seg_lo/hi` — the band strip's "where you may talk" shade.
//!
//! The expected values were measured on the tree BEFORE the receiver model existed, and each
//! one was checked against the privilege tables by hand. The rows cover single-receiver radios,
//! dual-receiver radios in ordinary operation, and satellite cross-band pairs, because those
//! are the three places a second receiver could plausibly leak into the answer.
//!
//! The same rows are then judged the D1 way ([`Engine::tx_source_verdict`], against the receiver
//! that transmits), and [`D1_DIFFERS`] pins every row where that answer parts from the gate's —
//! the difference the maintainer rules on before the gate is ever switched.

use super::*;
use crate::dualrx::ReceiverId;
use crate::settings::SatVfoMap;
use tempo_core::doppler::{DownlinkClass, Transponder};

/// RS-44 as the satellite tests elsewhere in the engine carry it: an INVERTING linear
/// transponder, 2 m up and 70 cm down. On an IC-9700 that pair is Main = downlink, Sub = uplink.
pub(super) const RS44: Transponder = Transponder {
    uplink_centre_hz: 145_965_000,
    downlink_centre_hz: 435_640_000,
    invert: true,
    half_width_hz: 30_000,
};

/// A CONSTRUCTED inverting pair whose uplink sits 1 kHz above the 2 m CW-only edge (144.100).
/// No real bird uplinks there; it exists to put a sideband-sensitive emission next to a
/// segment edge, which is the only place the uplink's sideband can change a licence answer.
pub(super) const EDGE_BIRD: Transponder = Transponder {
    uplink_centre_hz: 144_101_000,
    downlink_centre_hz: 435_640_000,
    invert: true,
    half_width_hz: 30_000,
};

/// A V/V transponder (2 m in, 2 m out): the pass a Main/Sub Icom works on its A/B split,
/// because Main and Sub cannot share a band.
pub(super) const VV_BIRD: Transponder = Transponder {
    uplink_centre_hz: 145_990_000,
    downlink_centre_hz: 145_950_000,
    invert: false,
    half_width_hz: 15_000,
};

/// A station on `model` with `class` privileges, in `section`, tuned to `mhz` on `band`.
pub(super) fn station(
    model: u32,
    class: &str,
    section: &str,
    mhz: f64,
    band: &str,
    sideband: &str,
) -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.ensure_radio_profiles();
    e.settings.rig_model = model;
    e.settings.sync_active_from_flat();
    e.set_license_class(class);
    e.set_operating_mode(section, false);
    e.set_frequency(mhz, band, sideband);
    e
}

/// A satellite pass on `model`, the way the Satellites view and the radio loop build one:
/// Main = downlink / Sub = uplink confirmed for every radio, the native CI-V daemon serving,
/// the transponder picked, the nominal legs queued. The section is set FIRST because a section
/// change releases a satellite's hold on the rig mode.
pub(super) fn sat_pass(model: u32, class: &str, section: &str, tp: Transponder) -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.ensure_radio_profiles();
    e.settings.rig_model = model;
    e.settings.rig_conn = "serial".to_string();
    e.settings.rig_addr = String::new();
    e.settings.icom_native_cat = true;
    e.settings.sync_active_from_flat();
    let ids: Vec<u32> = e.settings.radios.iter().map(|p| p.id).collect();
    for id in ids {
        e.settings.confirm_sat_uplink(id, SatVfoMap::MainDownSubUp);
    }
    e.set_license_class(class);
    e.set_operating_mode(section, false);
    e.set_sat_transponder(Some(("TEST|linear".into(), 0, tp)));
    e.sat_tune_nominal(DownlinkClass::Usb, 1_000_000);
    e
}

/// The uplink the pass queued on the split, in Hz.
fn queued_uplink_hz(e: &Engine) -> u64 {
    e.split_tx_mhz()
        .map(|m| (m * 1e6).round() as u64)
        .expect("the pick put the uplink on the split")
}

/// What the radio loop does with a queued satellite split when `cat` is serving, reduced to
/// the engine calls it makes: ask which VFO the split rides, then either report the rig's
/// acknowledgement — naming the receiver it wrote, as the loop does — or report the refusal.
/// Returns the VFO the split rode, if it was sent.
pub(super) fn loop_applies_sat_split(e: &mut Engine, cat: SatCatBackend) -> Option<&'static str> {
    let up = queued_uplink_hz(e);
    e.rig_dial_applied(e.settings.dial_hz());
    match e.sat_split_tx_vfo(up, cat) {
        Ok(vfo) => {
            let rx = if vfo == "Sub" {
                ReceiverId::Sub
            } else {
                ReceiverId::Main
            };
            e.rig_split_applied_on(up, rx);
            Some(vfo)
        }
        Err(_) => {
            e.split_rejected(up as f64 / 1e6);
            None
        }
    }
}

/// One station state and the decisions the gate made for it before the receiver model.
struct Row {
    name: &'static str,
    build: fn() -> Engine,
    tx_allowed: bool,
    emission_mhz: f64,
    phone_seg: Option<(f64, f64)>,
}

/// The table. Grouped as the brief for this stage asks: single-receiver radios, dual-receiver
/// radios in normal operation, satellite cross-band pairs.
fn rows() -> Vec<Row> {
    vec![
        // ── Single-receiver radios ─────────────────────────────────────────────────────
        Row {
            name: "IC-7300 FT8 on 20 m, General",
            build: || station(3073, "general", "digital", 14.074, "20m", "USB"),
            tx_allowed: true,
            emission_mhz: 14.074,
            phone_seg: Some((14.225, 14.350)),
        },
        Row {
            name: "FTDX3000 phone at 14.200, below the General phone floor",
            build: || station(1037, "general", "phone", 14.200, "20m", "USB"),
            tx_allowed: false,
            emission_mhz: 14.200,
            phone_seg: Some((14.225, 14.350)),
        },
        Row {
            name: "IC-7300 digital 3.601 LSB — the offset lands back in the 80 m data segment",
            build: || station(3073, "general", "digital", 3.601, "80m", "LSB"),
            tx_allowed: true,
            emission_mhz: 3.601,
            phone_seg: Some((3.800, 4.000)),
        },
        Row {
            name: "IC-7300 digital 3.601 USB — the offset lands above the data ceiling",
            build: || station(3073, "general", "digital", 3.601, "80m", "USB"),
            tx_allowed: false,
            emission_mhz: 3.601,
            phone_seg: Some((3.800, 4.000)),
        },
        Row {
            name: "FTDX10 split: RX 14.015 (Extra CW), TX 14.026 acknowledged, General",
            build: || {
                let mut e = station(1042, "general", "cw", 14.015, "20m", "USB");
                e.rig_split_applied(14_026_000);
                e
            },
            tx_allowed: true,
            emission_mhz: 14.026,
            phone_seg: Some((14.225, 14.350)),
        },
        Row {
            name: "FTDX10 split reversed: RX 14.030 legal, TX 14.015 acknowledged, General",
            build: || {
                let mut e = station(1042, "general", "cw", 14.030, "20m", "USB");
                e.rig_split_applied(14_015_000);
                e
            },
            tx_allowed: false,
            emission_mhz: 14.015,
            phone_seg: Some((14.225, 14.350)),
        },
        Row {
            name: "IC-7300 phone 14.345 with +3 kHz XIT — the clarifier carries it over the top",
            build: || {
                let mut e = station(3073, "general", "phone", 14.345, "20m", "USB");
                e.request_xit(3_000);
                e
            },
            tx_allowed: false,
            emission_mhz: 14.348,
            phone_seg: Some((14.225, 14.350)),
        },
        Row {
            name: "IC-7300 reports split, operator never opted in — unverified, refused",
            build: || {
                let mut e = station(3073, "general", "digital", 14.074, "20m", "USB");
                e.observe_rig_split(true, Some(14_030_000));
                e
            },
            tx_allowed: false,
            emission_mhz: 14.074,
            phone_seg: Some((14.225, 14.350)),
        },
        Row {
            name: "IC-7300 reports split, opted in and fresh — judged at the reported TX",
            build: || {
                let mut e = station(3073, "general", "digital", 14.074, "20m", "USB");
                e.settings.split_detect_enabled = true;
                e.observe_rig_split(true, Some(14_160_000));
                e
            },
            tx_allowed: false,
            emission_mhz: 14.160,
            phone_seg: Some((14.225, 14.350)),
        },
        Row {
            name: "IC-7300, Open class (no US privilege model), phone at 14.200",
            build: || station(3073, "open", "phone", 14.200, "20m", "USB"),
            tx_allowed: true,
            emission_mhz: 14.200,
            phone_seg: None,
        },
        // ── Dual-receiver radios, normal operation ─────────────────────────────────────
        Row {
            name: "IC-7610 phone 14.250, General",
            build: || station(3078, "general", "phone", 14.250, "20m", "USB"),
            tx_allowed: true,
            emission_mhz: 14.250,
            phone_seg: Some((14.225, 14.350)),
        },
        Row {
            name: "IC-7610 phone 14.200, General",
            build: || station(3078, "general", "phone", 14.200, "20m", "USB"),
            tx_allowed: false,
            emission_mhz: 14.200,
            phone_seg: Some((14.225, 14.350)),
        },
        Row {
            name: "FTDX101D CW pile-up split: RX 14.020 (Extra), TX 14.025 acknowledged, General",
            build: || {
                let mut e = station(1040, "general", "cw", 14.020, "20m", "USB");
                e.rig_split_applied(14_025_000);
                e
            },
            tx_allowed: true,
            emission_mhz: 14.025,
            phone_seg: Some((14.225, 14.350)),
        },
        Row {
            name: "IC-9700 SSB 144.200 on 2 m, Technician",
            build: || station(3081, "technician", "phone", 144.200, "2m", "USB"),
            tx_allowed: true,
            emission_mhz: 144.200,
            phone_seg: Some((144.1, 148.0)),
        },
        Row {
            name: "TS-990S FT8 7.074, Technician (no 40 m data privilege)",
            build: || station(2039, "technician", "digital", 7.074, "40m", "USB"),
            tx_allowed: false,
            emission_mhz: 7.074,
            phone_seg: None,
        },
        // ── Satellite cross-band pairs ─────────────────────────────────────────────────
        Row {
            name: "IC-9700 RS-44 phone, native CI-V: uplink rides Sub, General",
            build: || {
                let mut e = sat_pass(3081, "general", "phone", RS44);
                assert_eq!(
                    loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv),
                    Some("Sub")
                );
                e
            },
            tx_allowed: true,
            emission_mhz: 145.965,
            phone_seg: Some((420.0, 450.0)),
        },
        Row {
            name: "IC-9700 RS-44 digital, native CI-V: uplink rides Sub, General",
            build: || {
                let mut e = sat_pass(3081, "general", "digital", RS44);
                assert_eq!(
                    loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv),
                    Some("Sub")
                );
                e
            },
            tx_allowed: true,
            emission_mhz: 145.965,
            phone_seg: Some((420.0, 450.0)),
        },
        Row {
            name: "IC-9700 constructed edge bird (up 144.101, inverting) digital, General",
            build: || {
                let mut e = sat_pass(3081, "general", "digital", EDGE_BIRD);
                assert_eq!(
                    loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv),
                    Some("Sub")
                );
                e
            },
            tx_allowed: true,
            emission_mhz: 144.101,
            phone_seg: Some((420.0, 450.0)),
        },
        // Added with the D1 comparison below, and measured the same way: the gate code these
        // rows exercise is unchanged from the base. The one state in which the uplink's
        // sideband is unknown — the operator took the mode back mid-pass, so Nexus stops
        // commanding one (`sat_mode_released`). Every shipped caller of the override sets it for
        // PHONE (the mode picker, a memory recall into Phone, Remote's phone mode), so in the
        // Digital section it is reachable only through the bare `set_sideband_override` command.
        Row {
            name: "IC-9700 edge bird digital, uplink mode released mid-pass",
            build: || {
                let mut e = sat_pass(3081, "general", "digital", EDGE_BIRD);
                assert_eq!(
                    loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv),
                    Some("Sub")
                );
                e.request_sideband_override(Some("USB"));
                e
            },
            tx_allowed: true,
            emission_mhz: 144.101,
            phone_seg: Some((420.0, 450.0)),
        },
        Row {
            name: "IC-9700 RS-44, Open class",
            build: || {
                let mut e = sat_pass(3081, "open", "phone", RS44);
                assert_eq!(
                    loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv),
                    Some("Sub")
                );
                e
            },
            tx_allowed: true,
            emission_mhz: 145.965,
            phone_seg: None,
        },
        Row {
            name: "IC-9700 V/V pass: same band, so the uplink rides VFO B on Main",
            build: || {
                let mut e = sat_pass(3081, "general", "phone", VV_BIRD);
                assert_eq!(
                    loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv),
                    Some("VFOB")
                );
                e
            },
            tx_allowed: true,
            emission_mhz: 145.990,
            phone_seg: Some((144.1, 148.0)),
        },
        Row {
            name: "IC-9700 RS-44 served by Hamlib: the Sub split is refused, nothing confirmed",
            build: || {
                let mut e = sat_pass(3081, "general", "phone", RS44);
                assert_eq!(loop_applies_sat_split(&mut e, SatCatBackend::Hamlib), None);
                e
            },
            tx_allowed: true,
            emission_mhz: 435.640,
            phone_seg: Some((420.0, 450.0)),
        },
        Row {
            name: "IC-905 (second receiver unread) RS-44, native CI-V: uplink rides Sub",
            build: || {
                let mut e = sat_pass(3090, "general", "phone", RS44);
                assert_eq!(
                    loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv),
                    Some("Sub")
                );
                e
            },
            tx_allowed: true,
            emission_mhz: 145.965,
            phone_seg: Some((420.0, 450.0)),
        },
    ]
}

/// What the three consumers read for `e`: the gate verb, and the snapshot fields the cockpit
/// renders. The verb and the snapshot must agree, or the lock indicator is lying.
fn decisions(e: &Engine) -> (bool, f64, Option<(f64, f64)>) {
    let s = e.snapshot();
    assert_eq!(
        s.radio.tx_allowed,
        e.tx_allowed(),
        "the snapshot's lock and the key-time gate disagree"
    );
    (
        e.tx_allowed(),
        s.radio
            .tx_emission_mhz
            .expect("the snapshot always names the judged frequency"),
        s.radio.phone_seg_lo.zip(s.radio.phone_seg_hi),
    )
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn same_seg(a: Option<(f64, f64)>, b: Option<(f64, f64)>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some((al, ah)), Some((bl, bh))) => close(al, bl) && close(ah, bh),
        _ => false,
    }
}

/// ⛔ THE GATE'S DECISIONS ARE TODAY'S, ROW FOR ROW.
///
/// All rows are checked before anything is reported, so a regression names every state it
/// moved rather than the first.
#[test]
fn the_transmit_gate_decides_every_row_exactly_as_it_did_before_the_receiver_model() {
    let mut moved = Vec::new();
    for row in rows() {
        let e = (row.build)();
        let (allowed, emission, seg) = decisions(&e);
        if allowed != row.tx_allowed
            || !close(emission, row.emission_mhz)
            || !same_seg(seg, row.phone_seg)
        {
            moved.push(format!(
                "{}: now ({allowed}, {emission}, {seg:?}), was ({}, {}, {:?})",
                row.name, row.tx_allowed, row.emission_mhz, row.phone_seg
            ));
        }
    }
    assert!(moved.is_empty(), "the gate moved:\n{}", moved.join("\n"));
}

/// ⚠️ THE CONTROL FOR THE TABLE ABOVE: the rows must disagree with EACH OTHER, or the table
/// could pass on a gate that answers the same thing everywhere. Both answers of the gate, and
/// more than one segment and emission, must appear.
#[test]
fn the_gate_table_is_not_uniform() {
    let rows = rows();
    assert!(rows.iter().any(|r| r.tx_allowed) && rows.iter().any(|r| !r.tx_allowed));
    assert!(rows.iter().any(|r| r.phone_seg.is_none()));
    assert!(rows
        .iter()
        .any(|r| r.phone_seg.is_some_and(|(lo, _)| close(lo, 420.0))));
    assert!(rows
        .iter()
        .any(|r| r.phone_seg.is_some_and(|(lo, _)| close(lo, 14.225))));
}

// ── D1's input: which receiver transmits ─────────────────────────────────────────────────

/// ⛔ THE RECEIVER A SPLIT RIDES NEVER REACHES THE GATE.
///
/// The radio loop names the receiver in the very call that grants a split confirmation, so the
/// grant site now writes one more field. Every row holding a confirmed split is rebuilt with
/// that record flipped (Main ↔ Sub) and must decide identically — while the flip must visibly
/// move [`Engine::tx_source`], or this would be a mutation that changed nothing.
#[test]
fn the_receiver_a_split_rides_never_reaches_the_gate() {
    let mut flipped = Vec::new();
    for row in rows() {
        let e = (row.build)();
        let Some(hz) = e.tx_split_confirmed_hz else {
            continue;
        };
        let mut twin = (row.build)();
        let other = match twin.tx_split_confirmed_rx {
            ReceiverId::Main => ReceiverId::Sub,
            ReceiverId::Sub => ReceiverId::Main,
        };
        twin.rig_split_applied_on(hz, other);
        assert_ne!(
            twin.tx_source(),
            e.tx_source(),
            "{}: the flip must move the transmit source",
            row.name
        );
        assert_eq!(decisions(&twin), decisions(&e), "{}", row.name);
        flipped.push(e.tx_source());
    }
    assert!(
        flipped.contains(&ReceiverId::Main) && flipped.contains(&ReceiverId::Sub),
        "the table must carry confirmed splits on both receivers: {flipped:?}"
    );
}

/// D1 — THE TRANSMIT SOURCE: Main, except while an acknowledged split rides the Sub band.
#[test]
fn the_transmit_source_is_main_until_an_acknowledged_split_rides_the_sub() {
    // Simplex on a two-receiver radio: Main transmits.
    let e = station(3078, "general", "phone", 14.250, "20m", "USB");
    assert_eq!(e.tx_source(), ReceiverId::Main, "simplex");

    // A pile-up split rides Main's VFO B.
    let mut e = station(1040, "general", "cw", 14.020, "20m", "USB");
    e.rig_split_applied(14_025_000);
    assert_eq!(e.tx_source(), ReceiverId::Main, "a VFO B split is Main's");

    // A rig-reported split names no receiver, and nothing here wrote it: Main.
    let mut e = station(3081, "general", "digital", 144.174, "2m", "USB");
    e.settings.split_detect_enabled = true;
    e.observe_rig_split(true, Some(144_180_000));
    assert_eq!(e.tx_source(), ReceiverId::Main, "a split the rig reported");

    // RS-44 on the native daemon: satellite mode transmits out of the Sub band.
    let mut e = sat_pass(3081, "general", "phone", RS44);
    assert_eq!(
        loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv),
        Some("Sub")
    );
    assert_eq!(e.tx_source(), ReceiverId::Sub, "the uplink rides the Sub");

    // A later split on VFO B is Main's: the record is rewritten with every grant, never left
    // over from the one before.
    e.rig_split_applied(145_965_000);
    assert_eq!(
        e.tx_source(),
        ReceiverId::Main,
        "rewritten by the next grant"
    );

    // A QSY voids the confirmation, and with it the Sub as the source.
    let mut e = sat_pass(3081, "general", "phone", RS44);
    loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv);
    assert_eq!(e.tx_source(), ReceiverId::Sub);
    e.set_frequency(435.700, "70cm", "USB");
    assert_eq!(e.tx_source(), ReceiverId::Main, "the radio moved");

    // Same rig, a V/V bird: Main and Sub cannot share 2 m, so the pass rides VFO B on Main.
    let mut e = sat_pass(3081, "general", "phone", VV_BIRD);
    assert_eq!(
        loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv),
        Some("VFOB")
    );
    assert_eq!(e.tx_source(), ReceiverId::Main, "V/V rides VFO B");

    // Served by Hamlib the Sub split is refused and nothing is confirmed: Main.
    let mut e = sat_pass(3081, "general", "phone", RS44);
    assert_eq!(loop_applies_sat_split(&mut e, SatCatBackend::Hamlib), None);
    assert_eq!(e.tx_source(), ReceiverId::Main, "a refused Sub split");

    // ⚠️ THE RIG DECIDES WHERE IT TRANSMITS, NOT THE CAPABILITY TABLE: an IC-905 on a pass
    // transmits out of its Sub band although no manual has been read for a second receiver.
    let mut e = sat_pass(3090, "general", "phone", RS44);
    assert_eq!(
        loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv),
        Some("Sub")
    );
    assert_eq!(e.tx_source(), ReceiverId::Sub, "IC-905 uplink");
}

// ── D1: the same table, judged against the transmit source ─────────────────────────────

/// What D1 answers for one row of the table.
struct D1Answer {
    row: &'static str,
    tx_allowed: bool,
    emission_mhz: f64,
    phone_seg: Option<(f64, f64)>,
}

/// ⭐ THE DIFF TABLE — every row where D1's verdict ([`Engine::tx_source_verdict`]) decides
/// differently from the gate today, with what D1 would answer. D1 and today agree on every row
/// not listed, by assertion.
///
/// Read it as: switching the gate to D1 moves the band strip's phone shade on every cross-band
/// pass (Main's 70 cm segment becomes the uplink's 2 m one), never moves the judged frequency,
/// and changes a licence answer in two states. Both are on the constructed edge bird and in the
/// Digital section, where the gate judges the data offset on the DIAL's sideband and D1 on the
/// uplink's:
/// - the uplink's sideband unknown (the mode taken back mid-pass): D1 judges both halves;
/// - ⚠️ the uplink's KNOWN sideband, since `2d4300ad` mirrors the data submode on an inverting
///   transponder (PKTUSB ↔ PKTLSB): the uplink goes up in PKTLSB, so its data carrier sits one
///   offset BELOW the 144.101 dial, inside the 2 m CW-only segment. That row read `true` when
///   the table was pinned, because the uplink was then commanded PKTUSB, the downlink's word.
const D1_DIFFERS: &[D1Answer] = &[
    D1Answer {
        row: "IC-9700 RS-44 phone, native CI-V: uplink rides Sub, General",
        tx_allowed: true,
        emission_mhz: 145.965,
        phone_seg: Some((144.1, 148.0)),
    },
    D1Answer {
        row: "IC-9700 RS-44 digital, native CI-V: uplink rides Sub, General",
        tx_allowed: true,
        emission_mhz: 145.965,
        phone_seg: Some((144.1, 148.0)),
    },
    // `true` until `2d4300ad` was merged: the uplink is now commanded PKTLSB, so D1 judges the
    // data carrier 1.5 kHz below 144.101. The gate still allows this row (the table above).
    D1Answer {
        row: "IC-9700 constructed edge bird (up 144.101, inverting) digital, General",
        tx_allowed: false,
        emission_mhz: 144.101,
        phone_seg: Some((144.1, 148.0)),
    },
    D1Answer {
        row: "IC-9700 edge bird digital, uplink mode released mid-pass",
        tx_allowed: false,
        emission_mhz: 144.101,
        phone_seg: Some((144.1, 148.0)),
    },
    D1Answer {
        row: "IC-905 (second receiver unread) RS-44, native CI-V: uplink rides Sub",
        tx_allowed: true,
        emission_mhz: 145.965,
        phone_seg: Some((144.1, 148.0)),
    },
];

/// D1 JUDGES THE TRANSMIT SOURCE, and parts from today's gate only where the Sub transmits.
///
/// A Main-sourced verdict IS the gate's (it judges the same receiver), so every difference must
/// come from a Sub-sourced row; and a Sub-sourced row may still agree (the Open class has no
/// segments to differ over), which is why the Sub-sourced count is asserted separately.
#[test]
fn d1_judges_the_transmit_source_and_parts_from_today_only_where_the_sub_transmits() {
    let rows = rows();
    for d in D1_DIFFERS {
        assert!(
            rows.iter().any(|r| r.name == d.row),
            "the diff table names a row the table does not have: {}",
            d.row
        );
    }
    let mut sub_sourced = Vec::new();
    for row in &rows {
        let e = (row.build)();
        let v = e.tx_source_verdict();
        assert_eq!(v.source, e.tx_source(), "{}", row.name);
        if v.source == ReceiverId::Sub {
            sub_sourced.push(row.name);
        }
        let today = decisions(&e);
        let d1 = (v.tx_allowed, v.emission_mhz, v.phone_seg);
        match D1_DIFFERS.iter().find(|d| d.row == row.name) {
            Some(d) => {
                assert_eq!(v.source, ReceiverId::Sub, "{}", row.name);
                assert!(
                    d1.0 == d.tx_allowed
                        && close(d1.1, d.emission_mhz)
                        && same_seg(d1.2, d.phone_seg),
                    "{}: D1 now answers {d1:?}",
                    row.name
                );
                assert_ne!(d1, today, "{}: listed as differing, but agrees", row.name);
            }
            None => assert_eq!(d1, today, "{}: D1 must agree with the gate here", row.name),
        }
    }
    assert_eq!(
        sub_sourced.len(),
        6,
        "the Sub transmits on six rows, listed or not: {sub_sourced:?}"
    );
}
