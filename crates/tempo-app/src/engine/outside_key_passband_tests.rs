//! A program outside Nexus is judged across the whole audio passband it could send.
//!
//! A CAT broker client or a TCI program keys the transmitter with audio Nexus never hears, at an
//! offset Nexus does not know: a data program puts its signal anywhere from 0 to about 3 kHz
//! above the carrier on an upper sideband (below it on a lower one). Judging the carrier, or
//! Nexus's own frame at Nexus's own TX offset, let a program tuned just under an edge put its
//! signal inside the next segment (operator ruling, 2026-10-25: the whole passband must lie inside
//! the class's privileges for the mode, every class, every band edge). The Phone section judges
//! its own phone passband, CW the carrier, as for Nexus's own transmissions, which keep their own
//! judgement.

use super::tx_gate_table::station;
use super::*;
use tempo_net::tci::server::{KeyKind, Mic};
use tempo_net::tci::wire::Word;

/// Nexus's own TX offset in these tests: low, so Nexus's own data frame (300–350 Hz) sits well
/// inside wherever the program's 3 kHz passband does not, and the two judgements can disagree.
const OWN_OFFSET_HZ: f32 = 300.0;

/// One edge, swept from the side the passband reaches it from.
struct Edge {
    class: &'static str,
    section: &'static str,
    band: &'static str,
    /// The section's sideband (the commanded word follows it).
    sideband: &'static str,
    /// The privilege edge, MHz.
    edge: f64,
    /// kHz from the edge, on the side away from it, and what each key gets there, as `T`/`F`:
    /// the outside key's verdict, and Nexus's own (the control).
    sweep: &'static [(f64, char, char)],
}

/// An upper-sideband passband under an edge: 5 kHz under keys; 3 kHz under the passband's top
/// reaches the edge, which is outside, as a segment's top is; 2 and 0.1 kHz cross it. Nexus's own
/// frame keys until 0.1 kHz.
const UNDER: &[(f64, char, char)] = &[
    (5.0, 'T', 'T'),
    (3.0, 'F', 'T'),
    (2.0, 'F', 'T'),
    (0.1, 'F', 'F'),
];

/// A lower-sideband passband over an edge: 3 kHz over, the passband's bottom is the edge, which
/// is inside; 2.9 kHz and closer it crosses.
const OVER: &[(f64, char, char)] = &[
    (5.0, 'T', 'T'),
    (3.0, 'T', 'T'),
    (2.9, 'F', 'T'),
    (2.0, 'F', 'T'),
    (0.1, 'F', 'F'),
];

/// Phone: the gate's own 2.8 kHz phone passband, for the program and for Nexus alike.
const PHONE_UNDER: &[(f64, char, char)] = &[
    (5.0, 'T', 'T'),
    (3.0, 'T', 'T'),
    (2.0, 'F', 'F'),
    (0.1, 'F', 'F'),
];

/// CW: the carrier, for the program and for Nexus alike.
const CW_INSIDE: &[(f64, char, char)] = &[
    (5.0, 'T', 'T'),
    (3.0, 'T', 'T'),
    (2.0, 'T', 'T'),
    (0.1, 'T', 'T'),
    (-0.1, 'F', 'F'),
];

const EDGES: &[Edge] = &[
    // Technician: data below 28.300, voice only above it.
    Edge {
        class: "technician",
        section: "digital",
        band: "10m",
        sideband: "USB",
        edge: 28.300,
        sweep: UNDER,
    },
    // General: the data segment's top on 20 m.
    Edge {
        class: "general",
        section: "digital",
        band: "20m",
        sideband: "USB",
        edge: 14.150,
        sweep: UNDER,
    },
    // Extra: CW and data end at 3.600, phone above.
    Edge {
        class: "extra",
        section: "digital",
        band: "80m",
        sideband: "USB",
        edge: 3.600,
        sweep: UNDER,
    },
    // The top of 6 m.
    Edge {
        class: "extra",
        section: "digital",
        band: "6m",
        sideband: "USB",
        edge: 54.000,
        sweep: UNDER,
    },
    // LSB mirror: CW only below 50.100, every mode above.
    Edge {
        class: "technician",
        section: "digital",
        band: "6m",
        sideband: "LSB",
        edge: 50.100,
        sweep: OVER,
    },
    // LSB mirror: a General's data floor on 20 m, Extra-only below it.
    Edge {
        class: "general",
        section: "digital",
        band: "20m",
        sideband: "LSB",
        edge: 14.025,
        sweep: OVER,
    },
    // Keyboard (PSK31) under a General's 20 m data top: the program's passband, not PSK's tone.
    Edge {
        class: "general",
        section: "keyboard",
        band: "20m",
        sideband: "USB",
        edge: 14.150,
        sweep: UNDER,
    },
    // Phone at the top of 20 m: the gate's own phone passband, unchanged.
    Edge {
        class: "extra",
        section: "phone",
        band: "20m",
        sideband: "USB",
        edge: 14.350,
        sweep: PHONE_UNDER,
    },
    // CW from a General's 20 m floor: the carrier, unchanged.
    Edge {
        class: "general",
        section: "cw",
        band: "20m",
        sideband: "USB",
        edge: 14.025,
        sweep: CW_INSIDE,
    },
];

/// The TCI program the Phone rows key from.
const PROGRAM: u64 = 1;

/// A station as `class` at `dial`, armed, Nexus's own TX offset [`OWN_OFFSET_HZ`], with the CAT
/// broker's and the TCI server's transmit switches on and the TCI program's last `modulation:`
/// the section's sideband.
fn armed(class: &str, section: &str, band: &str, sideband: &str, dial: f64) -> Engine {
    let mut e = station(3073, class, section, dial, band, sideband);
    assert!(
        (e.settings.dial_mhz - dial).abs() < 1e-9,
        "precondition: tuned to {dial}"
    );
    e.set_tx_offset(OWN_OFFSET_HZ);
    e.settings.cat_broker_ptt = true;
    e.settings.tci_server_tx = true;
    e.set_tx_enabled(true);
    e.tci_note_modulation(
        PROGRAM,
        &Word::new(&sideband.to_ascii_lowercase()).expect("a word"),
    );
    e
}

/// The broker's key, granted and read as keyed; and its release, never refused.
fn broker_keys(e: &mut Engine) -> bool {
    let keyed = e.broker_ptt(true) && e.broker_ptt_active() && e.manual_ptt();
    assert!(e.broker_ptt(false), "the release is never refused");
    assert!(!e.manual_ptt(), "and it unkeys");
    keyed
}

/// ⛔ AN OUTSIDE KEY IS ALLOWED ONLY WHERE THE PROGRAM'S WHOLE PASSBAND IS INSIDE THE LICENCE.
/// Edge by edge for several classes, from the side the passband reaches each edge: in Digital the
/// broker's key is judged across 0–3 kHz on the commanded side, so a dial up to 3 kHz from the
/// edge is refused where Nexus's own frame keys. CONTROLS: Nexus's own judgement at every dial is
/// unchanged (its own frame at 300 Hz); the Phone section judges the program as Nexus's own voice,
/// TCI microphone key and over included; CW judges the carrier.
#[test]
fn an_outside_key_is_judged_across_the_programs_whole_passband_at_every_edge() {
    let (mut wrong, mut discriminating) = (Vec::new(), 0);
    for edge in EDGES {
        let lsb = edge.sideband == "LSB";
        for &(khz, outside, own) in edge.sweep {
            // Away from the edge: under it on USB, over it on LSB; CW sits over its floor.
            let away = if lsb || edge.section == "cw" {
                1.0
            } else {
                -1.0
            };
            let dial = edge.edge + away * khz / 1000.0;
            let at = || armed(edge.class, edge.section, edge.band, edge.sideband, dial);
            let (want_outside, want_own) = (outside == 'T', own == 'T');
            let label = format!(
                "{} {} {} at {dial:.4} ({khz} kHz from {})",
                edge.class, edge.section, edge.sideband, edge.edge
            );
            let mut e = at();
            if e.tx_allowed() != want_own {
                wrong.push(format!("{label}: Nexus's own key {}", e.tx_allowed()));
            }
            if e.outside_key_passband_allowed() != want_outside {
                wrong.push(format!("{label}: the passband verdict"));
            }
            if broker_keys(&mut e) != want_outside {
                wrong.push(format!("{label}: the broker's key"));
            }
            if edge.section == "phone" {
                let mut e = at();
                let mic = e
                    .tci_key(PROGRAM, KeyKind::Mic(Mic::Selected), false)
                    .is_ok()
                    && e.manual_ptt();
                if mic != want_outside {
                    wrong.push(format!("{label}: the TCI microphone key {mic}"));
                }
                let mut e = at();
                let over = e.tci_key(PROGRAM, KeyKind::Tci, true).is_ok();
                if over != want_outside {
                    wrong.push(format!("{label}: the TCI over {over}"));
                }
            }
            discriminating += usize::from(want_own && !want_outside);
        }
    }
    assert!(wrong.is_empty(), "outside keys at the edges: {wrong:#?}");
    assert_eq!(
        discriminating, 14,
        "the sweep must reach the dials where the two judgements differ"
    );
}

/// ⛔ A HELD OUTSIDE KEY DROPS WHEN A RETUNE CARRIES THE PASSBAND ACROSS AN EDGE, and its release
/// is never refused. A General's broker key in Digital at 14.140 is held; a retune to 14.148
/// leaves Nexus's own frame inside the data segment, so only the program's passband crosses
/// 14.150. The key is still latched, so the mask on read is what drops it.
#[test]
fn a_held_outside_key_drops_when_a_retune_carries_the_passband_across_an_edge() {
    let mut e = armed("general", "digital", "20m", "USB", 14.140);
    assert!(
        e.broker_ptt(true) && e.manual_ptt(),
        "precondition: the broker keys at 14.140"
    );
    e.set_frequency(14.148, "20m", "USB");
    assert!(
        e.tx_allowed(),
        "precondition: Nexus's own frame keys at 14.148"
    );
    assert!(
        e.broker_ptt_active(),
        "precondition: the retune leaves the key latched"
    );
    assert!(!e.manual_ptt(), "the broker's held key drops");
    assert!(!e.broker_ptt(true), "and a re-assert is refused");
    assert!(e.broker_ptt(false), "the release is never refused");
    assert!(!e.broker_ptt_active());
}

/// One radio-loop tick of the programs' keys, as the loop polls it before it reads the PTT.
fn loop_tick(e: &mut Engine) {
    let _ = e.poll_tci(Instant::now(), 0.0, 0);
}

/// ⛔ A BROKER KEY THE LICENCE DROPPED DOES NOT COME BACK BY ITSELF. A General's broker key in
/// Digital at 14.140 is held; a retune to 14.148 carries the passband across 14.150, so the loop's
/// tick ends the latch; back at 14.140 the radio stays unkeyed until the program sends a new key,
/// which keys. CONTROL: a tick while the key is allowed leaves it held.
#[test]
fn a_broker_key_the_licence_dropped_stays_unkeyed_until_a_new_key() {
    let mut e = armed("general", "digital", "20m", "USB", 14.140);
    assert!(
        e.broker_ptt(true) && e.manual_ptt(),
        "precondition: the broker keys at 14.140"
    );
    loop_tick(&mut e);
    assert!(
        e.broker_ptt_active() && e.manual_ptt(),
        "control: a tick inside the licence keeps the key"
    );
    e.set_frequency(14.148, "20m", "USB");
    assert!(!e.manual_ptt(), "the mask unkeys at 14.148");
    loop_tick(&mut e);
    assert!(!e.broker_ptt_active(), "the tick ends the latch");
    e.set_frequency(14.140, "20m", "USB");
    loop_tick(&mut e);
    assert!(
        !e.broker_ptt_active() && !e.manual_ptt(),
        "back at 14.140 the radio stays unkeyed with no new key"
    );
    assert!(
        e.broker_ptt(true) && e.manual_ptt(),
        "a new key from the program keys"
    );
}

/// The broker's refusal names the edge and how far the program's audio could reach, on the side
/// the radio sends it: up from the carrier on USB, down on LSB. None where the passband fits.
#[test]
fn the_passband_refusal_says_where_the_programs_audio_could_reach() {
    let e = armed("general", "digital", "20m", "USB", 14.148);
    let why = e.outside_key_passband_refusal().expect("refused at 14.148");
    assert_eq!(
        why,
        "outside your license privileges at the segment edge: the program's audio could reach \
         14.1510 MHz"
    );
    let e = armed("general", "digital", "20m", "LSB", 14.027);
    let why = e.outside_key_passband_refusal().expect("refused at 14.027");
    assert!(why.contains("at the segment edge"), "{why:?}");
    assert!(why.contains("could reach 14.0240 MHz"), "{why:?}");
    // The top of 6 m is the band's edge, not a segment's.
    let e = armed("extra", "digital", "6m", "USB", 53.999);
    let why = e.outside_key_passband_refusal().expect("refused at 53.999");
    assert!(why.contains("at the band edge"), "{why:?}");
    assert!(why.contains("could reach 54.0020 MHz"), "{why:?}");
    // The side the radio reports, where the commanded side fits: the reach is on the reported side.
    let mut e = armed("general", "digital", "20m", "USB", 14.026);
    e.observe_rig_mode("PKTLSB".into());
    let why = e
        .outside_key_passband_refusal()
        .expect("refused at 14.026 on PKTLSB");
    assert!(why.contains("could reach 14.0230 MHz"), "{why:?}");
    let e = armed("general", "digital", "20m", "USB", 14.140);
    assert_eq!(e.outside_key_passband_refusal(), None, "control: inside");
}

/// The four 60 m channels' dials, each a channel's carrier, 1.5 kHz under its centre.
const SIXTY_CHANNEL_DIALS: [f64; 4] = [5.3305, 5.3465, 5.3715, 5.4035];

/// ⛔ ONLY ON A 60 M CHANNEL'S DIAL IS AN OUTSIDE KEY JUDGED AT THE CHANNEL (operator rulings,
/// 2026-10-25): a channel is 2.8 kHz wide, so a 3 kHz passband never fits one, and the rule
/// applies to the channel's upper-sideband dial, its phone carrier, within 50 Hz. A General's or
/// an Extra's broker key in Digital keys at each dial and 40 Hz either side of it; 60 Hz off it,
/// and at a channel's centre or 1 kHz under its top (common mis-set dials, where Nexus's own frame
/// still keys), the passband rule refuses it. A lower-sideband read-back on a dial is refused.
/// The 5.3515–5.3665 segment keeps the passband rule: 3 kHz under its top refused where Nexus's
/// own frame keys, 4 kHz under it granted.
#[test]
fn only_on_a_60m_channel_dial_is_an_outside_key_judged_at_the_channel() {
    let mut wrong = Vec::new();
    for class in ["general", "extra"] {
        for dial in SIXTY_CHANNEL_DIALS {
            for off_hz in [0.0, -40.0, 40.0] {
                let at = dial + off_hz / 1e6;
                let mut e = armed(class, "digital", "60m", "USB", at);
                if !(e.tx_allowed() && e.outside_key_passband_allowed() && broker_keys(&mut e)) {
                    wrong.push(format!("{class} on the channel dial at {at:.6}: refused"));
                }
            }
            // 60 Hz off the dial, the channel's centre, 1 kHz under its top: Nexus's own frame
            // keys, the program's passband leaves the channel.
            for at in [dial - 60e-6, dial + 60e-6, dial + 0.0015, dial + 0.0025] {
                let mut e = armed(class, "digital", "60m", "USB", at);
                if !e.tx_allowed() {
                    wrong.push(format!(
                        "{class} at {at:.6}: premise, Nexus's own key refused"
                    ));
                }
                if e.outside_key_passband_allowed() || broker_keys(&mut e) {
                    wrong.push(format!("{class} off the channel dial at {at:.6}: granted"));
                }
            }
            let mut e = armed(class, "digital", "60m", "USB", dial);
            e.observe_rig_mode("PKTLSB".into());
            if e.outside_key_passband_allowed() || broker_keys(&mut e) {
                wrong.push(format!("{class} at {dial} on a PKTLSB read-back: granted"));
            }
        }
        // Off every channel: between the 5.3320 and 5.3480 channels, and a hair under a carrier.
        for dial in [5.3400, 5.3300] {
            let mut e = armed(class, "digital", "60m", "USB", dial);
            if e.outside_key_passband_allowed() || broker_keys(&mut e) {
                wrong.push(format!("{class} off-channel at {dial}: granted"));
            }
        }
        // The segment's top, by the passband: 5.3635 reaches 5.3665, the segment's top.
        let mut e = armed(class, "digital", "60m", "USB", 5.3635);
        if !e.tx_allowed() || e.outside_key_passband_allowed() || broker_keys(&mut e) {
            wrong.push(format!(
                "{class} at 5.3635 under the segment's top: not the passband"
            ));
        }
        let mut e = armed(class, "digital", "60m", "USB", 5.3625);
        if !(e.outside_key_passband_allowed() && broker_keys(&mut e)) {
            wrong.push(format!("{class} at 5.3625 inside the segment: refused"));
        }
    }
    // CONTROL: a 20 m edge point keeps the passband rule.
    let mut e = armed("general", "digital", "20m", "USB", 14.148);
    if !e.tx_allowed() || e.outside_key_passband_allowed() || broker_keys(&mut e) {
        wrong.push("general at 14.148: not the passband".into());
    }
    assert!(wrong.is_empty(), "60 m outside keys: {wrong:#?}");
}

/// ⛔ THE RADIO'S OWN REPORTED SIDE COUNTS. A General in Digital, commanded and stored USB at
/// 14.026, whose radio reports PKTLSB (a front-panel flip the loop does not re-assert): the
/// program's audio would go out at 14.023–14.026, under the General floor of 14.025, so the key is
/// refused, and a held one ends on the next tick. CONTROLS: a USB-side read-back keys; a read-back
/// that names no emission (RTTY) keys as before; Nexus's own key is unchanged throughout. In Phone, an
/// Extra at 14.152 commanded USB whose radio reports LSB is refused (the voice would reach under
/// 14.150), and with a USB read-back keys.
#[test]
fn the_radios_reported_side_is_judged_as_well_as_the_commanded_one() {
    let mut wrong = Vec::new();
    for (readback, keys) in [
        ("PKTLSB", false),
        ("LSB", false),
        ("DATA-L", false),
        ("PKTUSB", true),
        ("USB", true),
        ("DATA-U", true),
        ("RTTY", true),
    ] {
        let mut e = armed("general", "digital", "20m", "USB", 14.026);
        e.observe_rig_mode(readback.into());
        if !e.tx_allowed() {
            wrong.push(format!("{readback}: premise, Nexus's own key refused"));
        }
        if e.outside_key_passband_allowed() != keys || broker_keys(&mut e) != keys {
            wrong.push(format!("{readback} at 14.026: want {keys}"));
        }
    }
    let mut e = armed("general", "digital", "20m", "USB", 14.026);
    assert!(e.broker_ptt(true), "precondition: the key is held");
    e.observe_rig_mode("PKTLSB".into());
    assert!(!e.manual_ptt(), "the flip unkeys");
    loop_tick(&mut e);
    assert!(!e.broker_ptt_active(), "and the tick ends the latch");
    for (readback, keys) in [("LSB", false), ("USB", true)] {
        let mut e = armed("extra", "phone", "20m", "USB", 14.152);
        e.observe_rig_mode(readback.into());
        if !e.tx_allowed() || broker_keys(&mut e) != keys {
            wrong.push(format!("phone {readback} at 14.152: want {keys}"));
        }
    }
    assert!(wrong.is_empty(), "read-back sides: {wrong:#?}");
}

/// ⛔ IN A CW-ONLY SEGMENT AN OUTSIDE KEY IS ALLOWED ONLY WHILE THE RADIO IS IN CW (operator ruling,
/// 2026-10-25). Class by class at its CW-only segments, in the CW section with the radio's own
/// keyer (commanded CW): no read-back or a CW one keys; a USB or DATA read-back, or the soundcard
/// keyer's commanded DATA submode, is refused. CONTROL: just above the 6 m and 2 m CW-only
/// segments, out of CW, the key is allowed; a passband from just under 50.1 that reaches into the
/// segment is refused; Nexus's own CW is allowed everywhere here.
#[test]
fn in_a_cw_only_segment_an_outside_key_needs_the_radio_in_cw() {
    use crate::settings::CwKeyerBackend;
    let cases: &[(&str, f64, &str)] = &[
        ("technician", 3.550, "80m"),
        ("technician", 7.050, "40m"),
        ("technician", 21.100, "15m"),
        ("technician", 50.050, "6m"),
        ("technician", 144.050, "2m"),
        ("novice", 7.050, "40m"),
        ("novice", 21.100, "15m"),
        ("general", 50.050, "6m"),
        ("advanced", 144.050, "2m"),
        ("extra", 50.050, "6m"),
    ];
    let mut wrong = Vec::new();
    let mut check = |class: &str, mhz: f64, band: &str, how: &str, want: bool| {
        let mut e = armed(class, "cw", band, "USB", mhz);
        match how {
            "no read-back" => {}
            "soundcard" => {
                e.settings.cw_keyer = CwKeyerBackend::Soundcard;
                e.settings.sync_active_from_flat();
            }
            readback => e.observe_rig_mode(readback.into()),
        }
        if !e.tx_allowed() {
            wrong.push(format!(
                "{class} {how} at {mhz}: premise, Nexus's own CW refused"
            ));
        }
        let got = e.outside_key_passband_allowed() && broker_keys(&mut e);
        if got != want {
            wrong.push(format!("{class} {how} at {mhz}: want {want}, got {got}"));
        }
    };
    for &(class, mhz, band) in cases {
        for how in ["no read-back", "CW", "CWR"] {
            check(class, mhz, band, how, true);
        }
        for how in ["USB", "PKTUSB", "DATA-U", "soundcard"] {
            check(class, mhz, band, how, false);
        }
    }
    for class in ["technician", "general", "extra"] {
        check(class, 50.105, "6m", "USB", true);
        check(class, 144.105, "2m", "PKTUSB", true);
        // A passband that reaches into 50.0–50.1 from either side.
        check(class, 50.098, "6m", "USB", false);
        check(class, 50.102, "6m", "LSB", false);
    }
    assert!(wrong.is_empty(), "CW-only segments: {wrong:#?}");
}

/// The broker's refusal in a CW-only segment out of CW says so.
#[test]
fn the_cw_only_refusal_says_the_radio_is_not_in_cw() {
    let mut e = armed("technician", "cw", "40m", "USB", 7.050);
    e.observe_rig_mode("USB".into());
    assert_eq!(
        e.outside_key_passband_refusal().as_deref(),
        Some(
            "outside your license privileges: only CW is allowed here, and the radio is not in CW"
        )
    );
}

/// ⛔ AN AM-FAMILY OR FM READ-BACK IS JUDGED TOO (only ever refusing more). A General in Digital,
/// commanded and stored USB at 14.026, whose radio reports AM, DSB or SAM: the lower sideband
/// reaches under the 14.025 floor, so the key is refused, as on an LSB read-back; an FM read-back
/// is judged as the gate's phone model judges FM, and a General may not send phone at 14.026.
/// At 14.100 an AM read-back still keys (both sides inside). In Phone, an Extra at 14.152 on an AM
/// read-back is refused (the lower side reaches under 14.150) and keys at 14.200. CONTROL: FM
/// simplex at 146.52 on an FM read-back keys; Nexus's own key is unchanged throughout.
#[test]
fn an_am_or_fm_read_back_is_judged_across_what_it_sends() {
    let mut wrong = Vec::new();
    let mut check = |class: &str, section: &str, band: &str, sb: &str, mhz: f64, rb: &str, want| {
        let mut e = armed(class, section, band, sb, mhz);
        e.observe_rig_mode(rb.into());
        if !e.tx_allowed() {
            wrong.push(format!(
                "{class} {section} {rb} at {mhz}: premise, Nexus's own key"
            ));
        }
        let got = e.outside_key_passband_allowed();
        if got != want || broker_keys(&mut e) != want {
            wrong.push(format!(
                "{class} {section} {rb} at {mhz}: want {want}, got {got}"
            ));
        }
    };
    for rb in ["AM", "DSB", "SAM", "AMN", "FM", "FMN", "PKTFM"] {
        check("general", "digital", "20m", "USB", 14.026, rb, false);
    }
    for rb in ["AM", "DSB", "SAM"] {
        check("general", "digital", "20m", "USB", 14.100, rb, true);
    }
    check("extra", "phone", "20m", "USB", 14.152, "AM", false);
    check("extra", "phone", "20m", "USB", 14.200, "AM", true);
    check("technician", "phone", "2m", "FM", 146.52, "FM", true);
    check("technician", "digital", "2m", "FM", 146.52, "FM", true);
    assert!(wrong.is_empty(), "AM and FM read-backs: {wrong:#?}");
}

/// ⛔ ON A 60 M CHANNEL DIAL ONLY AN UPPER-SIDEBAND READ-BACK, OR NONE, KEYS. A General's broker
/// key in Digital at each channel dial is refused on an AM, DSB, SAM, FM, CW, RTTY or LSB-side
/// read-back, and the refusal says so; USB, PKTUSB, DATA-U or no read-back keys.
#[test]
fn on_a_60m_channel_dial_only_an_upper_sideband_read_back_keys() {
    let mut wrong = Vec::new();
    for dial in SIXTY_CHANNEL_DIALS {
        for (rb, want) in [
            (None, true),
            (Some("USB"), true),
            (Some("PKTUSB"), true),
            (Some("DATA-U"), true),
            (Some("AM"), false),
            (Some("DSB"), false),
            (Some("SAM"), false),
            (Some("FM"), false),
            (Some("CW"), false),
            (Some("RTTY"), false),
            (Some("PKTLSB"), false),
            (Some("DATA-L"), false),
        ] {
            let mut e = armed("general", "digital", "60m", "USB", dial);
            if let Some(rb) = rb {
                e.observe_rig_mode(rb.into());
            }
            if !e.tx_allowed() {
                wrong.push(format!(
                    "{rb:?} at {dial}: premise, Nexus's own key refused"
                ));
            }
            let why = e.outside_key_passband_refusal();
            if !want
                && why.as_deref()
                    != Some(
                        "outside your license privileges: on a 60 m channel the radio must be in \
                         upper sideband",
                    )
            {
                wrong.push(format!("{rb:?} at {dial}: refusal {why:?}"));
            }
            if e.outside_key_passband_allowed() != want || broker_keys(&mut e) != want {
                wrong.push(format!("{rb:?} at {dial}: want {want}"));
            }
        }
    }
    assert!(wrong.is_empty(), "60 m dial read-backs: {wrong:#?}");
}

/// ⛔ A RETUNE NEXUS COMMANDS DOES NOT CLEAR THE READ-BACK THE GATE JUDGES. A Technician in the CW
/// section at 7.030 whose radio reports USB is refused; Nexus retunes to 7.031 and the loop clears
/// the displayed read-back until its next poll: a broker key in that window is still refused, on
/// the last reading. The display copy is cleared. CONTROLS: a CAT drop clears both, and the key
/// is judged on the commanded CW as before; the next poll's CW read-back keys.
#[test]
fn a_retune_keeps_the_last_read_back_for_an_outside_key() {
    let mut e = armed("technician", "cw", "40m", "USB", 7.030);
    e.observe_rig_mode("USB".into());
    assert!(e.tx_allowed(), "premise: Nexus's own CW keys");
    assert!(
        !e.outside_key_passband_allowed(),
        "precondition: refused on USB"
    );
    e.set_frequency(7.031, "40m", "USB");
    e.clear_rig_mode_for_retune();
    assert_eq!(e.snapshot().radio.rig_mode, None, "the display copy clears");
    assert!(
        !e.outside_key_passband_allowed() && !broker_keys(&mut e),
        "a key in the window is judged on the last read-back"
    );
    e.observe_rig_mode("CW".into());
    assert!(
        broker_keys(&mut e),
        "control: the next poll's CW read-back keys"
    );
    e.observe_rig_mode("USB".into());
    e.clear_rig_mode();
    assert!(
        broker_keys(&mut e),
        "control: a CAT drop clears the gate's copy too"
    );
}

/// Age the rig's split reading by `secs`, and the start of a hold on it, as the clock moving on
/// would: both are unix seconds, so moving them back is the same as time passing.
fn age_split(e: &mut Engine, secs: u64) {
    if let Some((_, _, at)) = e.observed_split.as_mut() {
        *at -= secs;
    }
    if let Some(since) = e.observed_split_held_since.as_mut() {
        *since -= secs;
    }
}

/// A General in Digital at 14.074 with split detect on, the radio on a front-panel split sending
/// at 14.075, the reading fresh.
fn on_a_rig_split() -> Engine {
    let mut e = armed("general", "digital", "20m", "USB", 14.074);
    e.settings.split_detect_enabled = true;
    e.observe_rig_split(true, Some(14_075_000));
    assert!(e.tx_allowed(), "precondition: a fresh rig split keys");
    e
}

/// ⛔ A RIG-REPORTED SPLIT DOES NOT GO STALE WHILE A KEY NEXUS APPLIES HOLDS OFF THE POLL THAT READS
/// IT (operator ruling, 2026-10-25), here another program's key through the broker. A broker's
/// FT8 over (12.6 s) on a front-panel split, fresh at the key: the loop holds the reading's age
/// while keyed, so the key stays held and the loop's tick does not end it. After the over the
/// reading has the age it had at the key, the loop is told to read the split again, and with no
/// read it ages again and refuses after the TTL; a read keys. The operator's own key (Nexus's
/// gate, `tx_allowed`) holds the same way.
#[test]
fn a_rig_split_reading_does_not_age_while_nexus_is_keyed() {
    let mut e = on_a_rig_split();
    assert!(
        e.broker_ptt(true) && e.manual_ptt(),
        "precondition: the broker keys"
    );
    assert!(!e.hold_observed_split_age(true), "the loop sees the key");
    for _ in 0..13 {
        age_split(&mut e, 1);
        loop_tick(&mut e);
        assert!(
            e.broker_ptt_active() && e.manual_ptt() && e.tx_allowed(),
            "the over runs to its end"
        );
    }
    assert!(e.broker_ptt(false), "the over ends");
    assert!(
        e.hold_observed_split_age(false),
        "the loop reads the split again after the over"
    );
    assert!(e.tx_allowed(), "the reading has the age it had at the key");
    age_split(&mut e, OBSERVED_SPLIT_TTL_SECS + 1);
    assert!(
        !e.tx_allowed(),
        "unkeyed, with no read, it ages and refuses"
    );
    e.observe_rig_split(true, Some(14_075_000));
    assert!(
        e.broker_ptt(true) && e.manual_ptt(),
        "the poll's read refreshes it, and the next over keys"
    );
}

/// ⛔ A READING ALREADY STALE AT THE KEY STILL REFUSES, held or not: the hold keeps its age, it
/// never makes it younger.
#[test]
fn a_rig_split_reading_stale_at_the_key_still_refuses() {
    let mut e = on_a_rig_split();
    age_split(&mut e, OBSERVED_SPLIT_TTL_SECS + 1);
    assert!(!e.tx_allowed(), "stale before the key");
    assert!(!e.broker_ptt(true), "the broker's key is refused");
    e.hold_observed_split_age(true);
    age_split(&mut e, 5);
    assert!(!e.tx_allowed(), "still stale under a hold");
    e.hold_observed_split_age(false);
    assert!(!e.tx_allowed(), "and after it");
}

/// ⛔ THE HOLD IS CAPPED: a reading fresh at the key stays fresh for at most
/// `OBSERVED_SPLIT_HOLD_MAX_SECS` of the key, then ages again, so a key that never releases on a
/// front-panel split (a program's hung PTT through the broker) is refused that cap plus the TTL
/// after it went down, and the loop's tick ends it. Checked one second either side, so a tick of
/// the real clock mid-test cannot flip it. The reading keeps that age after the key ends.
#[test]
fn a_rig_split_reading_held_past_the_cap_ages_again_and_refuses() {
    let mut e = on_a_rig_split();
    assert!(
        e.broker_ptt(true) && e.manual_ptt(),
        "precondition: the broker keys"
    );
    e.hold_observed_split_age(true);
    for _ in 0..OBSERVED_SPLIT_HOLD_MAX_SECS + OBSERVED_SPLIT_TTL_SECS - 1 {
        age_split(&mut e, 1);
        loop_tick(&mut e);
        assert!(e.manual_ptt(), "keyed up to the cap plus the TTL");
    }
    age_split(&mut e, 2);
    loop_tick(&mut e);
    assert!(
        !e.tx_allowed() && !e.manual_ptt(),
        "refused past the cap plus the TTL, and the key ends"
    );
    e.hold_observed_split_age(false);
    assert!(!e.tx_allowed(), "and still stale once the key is gone");
}
