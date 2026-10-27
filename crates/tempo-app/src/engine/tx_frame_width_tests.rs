//! ⛔ THE LICENCE GATE JUDGES A DATA FRAME ACROSS ITS WHOLE WIDTH, WHERE IT KEYS (operator
//! rulings, 2026-10-15: "Judge the whole width, JS8 and FT." and "Judge the heartbeat's own spot.").
//!
//! The gate judged a JS8, FT8 or FT4 frame at its LOWEST tone, the dial plus the TX offset, so a
//! frame whose top crossed a privilege or segment edge still keyed. A US General at 5.363 MHz keyed
//! every offset under 3500 Hz, and from 3450 Hz up the top of a Normal frame sat on or above the
//! 5366.5 kHz top of the 60 m segment. Each frame is now judged from its lowest tone up the width its
//! mode occupies: FT8 50 Hz, FT4 83.3 Hz, JS8 25, 50, 80, 160 or 250 Hz by speed.
//!
//! FT2 kept the lowest tone after that, and its frame is the widest of them: a General at 5.363 MHz
//! keyed it from 3334 Hz up with its top on or above 5366.5 kHz. It is judged across its 166.7 Hz
//! too.
//!
//! A JS8 heartbeat or HB ACK keys on its own spot in 500–1000 Hz, while the gate judged the
//! operator's TX offset; on a dial just under a segment's bottom the spot fell below it with the
//! gate saying allowed. The frame is now judged at the offset it keys on, when it is planned and
//! again where the gate re-checks it at the key.
//!
//! Both only ever refuse more. The sweep below holds that over every US class and data range, and a
//! frame away from every edge keys exactly as before.

use super::*;
use crate::settings::OperatingMode;
use modes::Js8Speed;

/// What the JS8 cockpit says when a refusal outside the licence's privileges drops what was waiting
/// to go out.
const OUTSIDE_PRIVILEGES: &str = "JS8 not sent: this frequency is outside your license \
     privileges, so what was waiting to go out was dropped, not held for later.";

/// The width each mode occupies above its lowest tone: its tones times their spacing, as WSJT-X's
/// User Guide gives FT8 (8 × 6.25 Hz) and FT4 (4 × 20.83 Hz), and as JS8Call gives each JS8 speed
/// (`JS8::Submode::bandwidth`, 8 × the speed's spacing), which its waterfall draws from the TX
/// offset up.
const FT8_WIDTH: f64 = 50.0;
const FT4_WIDTH: f64 = 4.0 * 12_000.0 / 576.0;
/// FT2's, Decodium's FT4 at half the symbol time: its four tones, 0 to 3 (`genft2.f90`), each a
/// symbol rate apart (`gen_ft2wave.f90`'s modulation index of 1), and a symbol is 288 samples at
/// 12 kHz (`ft2_params.f90`). Written out from the Fortran, apart from the gate's own constants.
const FT2_WIDTH: f64 = 4.0 * 12_000.0 / 288.0;
const JS8_WIDTHS: [(Js8Speed, f64); 5] = [
    (Js8Speed::Slow, 25.0),
    (Js8Speed::Normal, 50.0),
    (Js8Speed::Fast, 80.0),
    (Js8Speed::Turbo, 160.0),
    (Js8Speed::Ultra, 250.0),
];

/// A US General on JS8 at `speed`, tuned to `dial` with the TX offset at `offset`, TX on, and
/// heartbeat networking on.
fn js8_general(dial: f64, speed: Js8Speed, offset: f32) -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.js8_hb_networking = true;
    e.js8_apply_station_config();
    e.js8_enter();
    e.js8_set_speed(speed.index()).expect("the speed");
    e.set_license_class("general");
    let band = crate::bandplan::band_for_dial(dial).expect("the dial is in a band");
    e.set_frequency(dial, band, "USB");
    e.set_tx_offset(offset);
    // A band change turns TX off; the operator turns it on.
    e.set_tx_enabled(true);
    assert_eq!(
        (e.settings.dial_mhz, e.tx_offset_hz(), e.js8_tx_speed()),
        (dial, offset, speed),
        "premise: the station as asked"
    );
    e
}

/// The first JS8 period after now.
fn next_slot(e: &Engine) -> u64 {
    let period_ms = u64::from(e.js8_tx_speed().period_s()) * 1000;
    tempo_core::timing::now_unix_ms() as u64 / period_ms + 1
}

/// Key one JS8 frame at `slot` as the radio loop does at a boundary (plan, build, commit), and say
/// the offset it keyed at, or `None` when nothing keyed.
fn key_frame(e: &mut Engine, slot: u64) -> Option<f32> {
    let plan = e.plan_tx(slot)?;
    let TxWaveform::Js8 { f0, .. } = &plan.waveform else {
        panic!("a JS8 plan carries the typed waveform");
    };
    let f0 = *f0;
    let wave = plan.waveform.build();
    (!e.commit_tx(&plan, wave, slot).is_empty()).then_some(f0)
}

/// The operator sends a one-frame message, and the next period keys it or does not.
fn send_one_frame(e: &mut Engine) -> Option<f32> {
    e.js8_send(None, "HI".into()).expect("queues");
    let s = next_slot(e);
    key_frame(e, s)
}

/// Fill the band activity so that every heartbeat spot from 500 Hz up to (not including) `to` Hz is
/// taken: a station heard now every 50 Hz from 525 Hz, each within 25 Hz of a spot. JS8Call's
/// free-spot search then cannot pick below `to`, and with every spot taken it takes 500 Hz.
fn take_heartbeat_spots_below(e: &mut Engine, to: i32) {
    let now = tempo_core::timing::now_unix_ms() as u64;
    for offset in (525..to).step_by(50) {
        e.js8_band_activity.insert(offset, now);
    }
}

/// A US General at 5.363 MHz, an FT tier, the TX offset at `offset`, calling CQ with TX on.
fn ft_general(tier: Tier, offset: f32) -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(tier);
    e.set_license_class("general");
    e.set_frequency(5.363, "60m", "USB");
    e.set_tx_offset(offset);
    e.start_cq(None).expect("Call CQ");
    e.set_tx_enabled(true);
    assert_eq!(e.tx_offset_hz(), offset, "premise: the offset as asked");
    e
}

/// Whether the CQ keys in the station's first two TX periods.
fn ft_keys(e: &mut Engine) -> bool {
    (0..4).any(|slot| !e.poll_tx(slot).is_empty())
}

/// JS8Call 3.0.3's 60 m dial, 5.363 MHz, as a US General: 3500 Hz above it is 5366.5 kHz, the top
/// of the 5351.5–5366.5 kHz segment. Per speed the last offset that keys and the first that is
/// refused sit the speed's width below 3500 Hz, a frame reaching 5366.5 kHz itself being outside
/// (the segment ends there, as a phone passband's does). The refusal locks the cockpit and says why.
#[test]
fn per_js8_speed_on_60_m_the_first_refused_offset_is_the_speeds_width_below_the_segment_top() {
    // (speed, the last offset that keys, the first that is refused)
    let want = [
        (Js8Speed::Slow, 3474.0, 3475.0),
        (Js8Speed::Normal, 3449.0, 3450.0),
        (Js8Speed::Fast, 3419.0, 3420.0),
        (Js8Speed::Turbo, 3339.0, 3340.0),
        (Js8Speed::Ultra, 3249.0, 3250.0),
    ];
    for ((speed, last, first), (wspeed, width)) in want.iter().zip(JS8_WIDTHS) {
        assert_eq!(*speed, wspeed, "the rows line up");
        assert_eq!(*last + 1.0, *first, "{speed:?}: one hertz apart");
        assert_eq!(
            f64::from(*first) + width,
            3500.0,
            "{speed:?}: the top at 5366.5 kHz"
        );
    }
    let got: Vec<_> = want
        .iter()
        .map(|&(speed, last, first)| {
            let keys = send_one_frame(&mut js8_general(5.363, speed, last));
            let mut e = js8_general(5.363, speed, first);
            let refused = send_one_frame(&mut e);
            let locked = !e.snapshot().radio.tx_allowed;
            e.js8_tick(tempo_core::timing::now_unix_ms() as u64);
            let why = e.js8_state().last_error;
            (speed, keys, refused, locked, why)
        })
        .collect();
    let want: Vec<_> = want
        .iter()
        .map(|&(speed, last, _)| {
            (
                speed,
                Some(last),
                None,
                true,
                Some(OUTSIDE_PRIVILEGES.to_string()),
            )
        })
        .collect();
    assert_eq!(
        got, want,
        "(speed, keyed at the last offset, keyed at the first refused, cockpit locked, why)"
    );
}

/// FT8 and FT4 at 5.363 MHz as a US General: a CQ whose lowest tone is inside the segment but whose
/// top reaches 5366.5 kHz is refused, and the cockpit shows TX locked; one hertz lower it keys.
#[test]
fn an_ft8_or_ft4_cq_whose_top_reaches_the_segment_edge_is_refused_and_just_inside_keys() {
    // (tier, the last offset that keys, the first that is refused): FT8's 50 Hz, FT4's 83.3 Hz.
    let rows = [(Tier::Ft8, 3449.0, 3450.0), (Tier::Ft4, 3416.0, 3417.0)];
    for (tier, last, first) in rows {
        let width = if tier == Tier::Ft8 {
            FT8_WIDTH
        } else {
            FT4_WIDTH
        };
        assert!(
            f64::from(last) + width < 3500.0 && f64::from(first) + width >= 3500.0,
            "{tier:?}: the top under 5366.5 kHz at the last offset, on it at the first"
        );
    }
    let got: Vec<_> = rows
        .iter()
        .map(|&(tier, last, first)| {
            let keys = ft_keys(&mut ft_general(tier, last));
            let mut e = ft_general(tier, first);
            let refused = ft_keys(&mut e);
            (tier, keys, refused, e.snapshot().radio.tx_allowed)
        })
        .collect();
    assert_eq!(
        got,
        [
            (Tier::Ft8, true, false, false),
            (Tier::Ft4, true, false, false)
        ],
        "(tier, keyed just inside, keyed with the top on the edge, the cockpit's TX allowed)"
    );
}

/// FT2 at 5.363 MHz as a US General: a CQ whose lowest tone is inside the segment but whose top
/// reaches 5366.5 kHz is refused, and one hertz lower it keys. The cockpit locks TX for the licence:
/// no connection refuses it and the phone mode is not the rule, so the lock says this frequency is
/// outside the privileges, and the frequency it names is the dial.
#[test]
fn an_ft2_cq_whose_top_reaches_the_segment_edge_is_refused_with_the_licence_lock_and_just_inside_keys(
) {
    // The last offset that keys and the first that is refused: FT2's 166.7 Hz below 3500 Hz.
    let (last, first) = (3333.0_f32, 3334.0_f32);
    assert!(
        f64::from(last) + FT2_WIDTH < 3500.0 && f64::from(first) + FT2_WIDTH >= 3500.0,
        "the top under 5366.5 kHz at the last offset, on or above it at the first"
    );
    assert!(
        crate::privileges::tx_allowed(
            crate::settings::LicenseClass::General,
            5.363 + f64::from(first) / 1e6,
            OperatingMode::Digital
        ),
        "premise: the lowest tone at the first refused offset is inside the segment"
    );
    let keys = ft_keys(&mut ft_general(Tier::Ft2, last));
    let mut e = ft_general(Tier::Ft2, first);
    let refused = ft_keys(&mut e);
    let radio = e.snapshot().radio;
    assert_eq!(
        (
            keys,
            refused,
            radio.tx_allowed,
            radio.tx_refusal,
            radio.phone_mode_refused,
            radio.tx_emission_mhz
        ),
        (true, false, false, None, false, Some(5.363)),
        "(keyed at the last offset, keyed at the first refused, the cockpit's TX allowed, the \
         connection's refusal, the phone-mode lock, the frequency the lock names)"
    );
}

/// A heartbeat keys on its own spot in 500–1000 Hz. On a dial 700 Hz under the 60 m segment's
/// bottom, 5.3508 MHz, the operator's 1500 Hz is 5352.3 kHz, inside, and a spot at 500 Hz is
/// 5351.3 kHz, below 5351.5: that heartbeat is refused and the cockpit says why. The same dial with
/// the spot inside (from 725 Hz up) keys the heartbeat there.
#[test]
fn a_heartbeat_whose_own_spot_is_below_the_segment_bottom_is_refused_and_inside_it_keys() {
    let mut e = js8_general(5.3508, Js8Speed::Normal, 1500.0);
    assert!(e.tx_allowed(), "premise: the operator's offset is inside");
    take_heartbeat_spots_below(&mut e, 1000);
    e.js8_heartbeat_now().expect("sent now");
    let s = next_slot(&e);
    let below = key_frame(&mut e, s);
    let why = e.js8_state().last_error;
    let mut e = js8_general(5.3508, Js8Speed::Normal, 1500.0);
    take_heartbeat_spots_below(&mut e, 725);
    e.js8_heartbeat_now().expect("sent now");
    let s = next_slot(&e);
    let inside = key_frame(&mut e, s);
    assert!(
        inside.is_some_and(|f0| (725.0..1000.0).contains(&f0)),
        "control: on a spot inside the segment the heartbeat keys there: {inside:?}"
    );
    assert_eq!(
        (below, why.as_deref()),
        (None, Some(OUTSIDE_PRIVILEGES)),
        "on its 500 Hz spot the heartbeat is refused, and the cockpit says why"
    );
}

/// The gate re-checks a planned over at the key while a satellite pass owns the dial (Doppler moves
/// it between the plan and the key). There it judges the frame where it keys too: a heartbeat
/// planned on its 500 Hz spot at 144.0996 MHz, 144.1001 MHz and inside a General's 2 m data, is
/// refused when the dial has moved to 144.0994 MHz, which puts the spot at 144.0999, under the
/// 144.1 MHz edge of the CW-only segment, although the operator's 1500 Hz is still inside. A move
/// that keeps the spot inside keys it.
#[test]
fn under_a_pass_a_heartbeat_moved_off_its_privilege_between_plan_and_key_is_refused_at_the_key() {
    let keyed_after = |doppler_to: f64| {
        let mut e = js8_general(144.0996, Js8Speed::Normal, 1500.0);
        e.sat_dial_owner = Some(("test pass".into(), 0));
        take_heartbeat_spots_below(&mut e, 1000);
        e.js8_heartbeat_now().expect("sent now");
        let s = next_slot(&e);
        let plan = e.plan_tx(s).expect("planned: the spot is inside");
        let TxWaveform::Js8 { f0, .. } = plan.waveform else {
            panic!("a JS8 plan");
        };
        assert_eq!(f0, 500.0, "premise: the heartbeat's spot");
        // Doppler moves the dial while the wave is built, with the engine unlocked.
        e.settings.dial_mhz = doppler_to;
        assert!(
            e.tx_allowed(),
            "premise: the operator's offset stays inside"
        );
        let wave = plan.waveform.build();
        !e.commit_tx(&plan, wave, s).is_empty()
    };
    assert!(keyed_after(144.0998), "control: the spot still inside keys");
    assert!(
        !keyed_after(144.0994),
        "the spot moved under 144.1 MHz is refused at the key"
    );
}

/// Away from every edge every frame keys exactly as before: at 5.363 MHz + 1500 Hz a General's JS8
/// frame keys at 1500 Hz at every speed, FT8, FT4 and FT2 call CQ, and at 14.078 MHz a heartbeat
/// keys on its own spot.
#[test]
fn away_from_every_edge_every_frame_keys_as_before() {
    for (speed, _) in JS8_WIDTHS {
        assert_eq!(
            send_one_frame(&mut js8_general(5.363, speed, 1500.0)),
            Some(1500.0),
            "{speed:?}"
        );
    }
    for tier in [Tier::Ft8, Tier::Ft4, Tier::Ft2] {
        assert!(ft_keys(&mut ft_general(tier, 1500.0)), "{tier:?}");
    }
    let mut e = js8_general(14.078, Js8Speed::Normal, 1500.0);
    e.js8_heartbeat_now().expect("sent now");
    let s = next_slot(&e);
    let f0 = key_frame(&mut e, s);
    assert!(
        f0.is_some_and(|f| (500.0..1000.0).contains(&f)),
        "the heartbeat keys on its spot: {f0:?}"
    );
}

/// ⛔ NEVER LESS. Over every US class, the bottom and top edge of every range it may send data in,
/// both sidebands, FT8, FT4, FT2 and every JS8 speed, and TX offsets across each edge, the gate
/// allows a frame only where it allowed the frame's lowest tone (the whole of what it judged
/// before), and it allows every frame whose whole width is privileged, as before. Where the two
/// part, the frame leaves the privilege, and every mode parts on both sidebands.
#[test]
fn the_gate_never_allows_a_frame_it_refused_before_and_allows_every_frame_inside_as_before() {
    let tiers: Vec<(Tier, Option<Js8Speed>, f64)> = [
        (Tier::Ft8, None, FT8_WIDTH),
        (Tier::Ft4, None, FT4_WIDTH),
        (Tier::Ft2, None, FT2_WIDTH),
    ]
    .into_iter()
    .chain(JS8_WIDTHS.map(|(speed, width)| (Tier::Js8, Some(speed), width)))
    .collect();
    let mut parted: std::collections::BTreeMap<String, usize> = Default::default();
    let mut cases = 0usize;
    for class in crate::privileges::US_CLASSES {
        for &(tier, speed, width) in &tiers {
            let label = format!("{tier:?} {speed:?}");
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            match speed {
                Some(speed) => {
                    e.js8_enter();
                    e.js8_set_speed(speed.index()).expect("the speed");
                }
                None => e.set_tier(tier),
            }
            e.settings.license_class = class;
            let spans = crate::privileges::allowed_spans(class, OperatingMode::Digital)
                .expect("a US class has data ranges");
            // The top of the frame crosses an edge 3500 Hz from the dial (offsets from a width below
            // it to just past it), and the lowest tone crosses one 300 Hz from it.
            let top: Vec<f32> = ((3500.0 - width).floor() as i32 - 3..=3503)
                .map(|o| o as f32)
                .chain([1500.0])
                .collect();
            let low: Vec<f32> = (290..=310).map(|o| o as f32).chain([1500.0]).collect();
            for (lo, hi) in spans {
                // USB: the frame sits above the dial; LSB: below it.
                let edges = [
                    ("USB", hi - 0.0035, &top),
                    ("USB", lo - 0.0003, &low),
                    ("LSB", lo + 0.0035, &top),
                    ("LSB", hi + 0.0003, &low),
                ];
                for (sb, dial, offsets) in edges {
                    let Some(band) = crate::bandplan::band_for_dial(dial) else {
                        continue;
                    };
                    e.set_frequency(dial, band, sb);
                    let dial = e.settings.dial_mhz;
                    let lsb = e.settings.sideband.eq_ignore_ascii_case("LSB");
                    assert_eq!(lsb, sb == "LSB", "premise: the sideband");
                    // The lowest tone and every 10 Hz up to the top, as the point gate judges each.
                    let point = |hz: f64| {
                        let f = if lsb {
                            dial - hz / 1e6
                        } else {
                            dial + hz / 1e6
                        };
                        crate::privileges::tx_allowed(class, f, OperatingMode::Digital)
                    };
                    for &off in offsets.iter() {
                        e.set_tx_offset(off);
                        assert_eq!(e.tx_offset_hz(), off, "premise: the offset");
                        let off = f64::from(off);
                        let before = point(off);
                        let whole = (0..=(width / 10.0) as u32)
                            .all(|k| point(off + 10.0 * f64::from(k)))
                            && point(off + width);
                        let now = e.tx_allowed();
                        let what = format!("{class:?} {label} {sb} dial {dial} offset {off}");
                        assert!(
                            !now || before,
                            "{what}: allowed where the lowest tone was refused"
                        );
                        assert!(
                            !(before && whole) || now,
                            "{what}: refused a frame wholly inside"
                        );
                        if before && !now {
                            assert!(!whole, "{what}");
                            *parted.entry(format!("{label} {sb}")).or_default() += 1;
                        }
                        cases += 1;
                    }
                }
            }
        }
    }
    assert!(cases > 10_000, "the sweep judged {cases} frames");
    // On each sideband: above the dial a frame's top crosses a range's top; below it, its bottom
    // crosses a range's bottom.
    for (tier, speed, _) in tiers {
        for sb in ["USB", "LSB"] {
            let label = format!("{tier:?} {speed:?} {sb}");
            assert!(
                parted.get(&label).is_some_and(|&n| n > 0),
                "{label}: the width refused nothing the lowest tone allowed: {parted:?}"
            );
        }
    }
}
