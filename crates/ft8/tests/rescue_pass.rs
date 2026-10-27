//! The rescue pass after the full pass from the early pass's residual ([`ft8::decode_frame_carry`]).
//! Subtracting the early decodes at WSJT-X's 47 step damages a few signals under or beside them
//! that the full pass finds from the raw audio; the rescue runs that raw full pass once more and
//! hands on only what the period has not reported, so the period keeps every decode the raw full
//! pass finds and every decode the residual adds. It saves only its new decodes into the a7 table,
//! and can be given up ([`Progress::Rescue`]).
//!
//! The tests here share the modem's a7 table across calls, so they take [`LOCK`] in turn.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Instant;

use ft8::{decode_frame_a7_streaming, decode_frame_carry, subtract_early, Carry, Decode, Progress};

const NMAX: usize = ft8::NMAX;

/// One test at a time: they read the a7 table between calls.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The noise and the stations' levels, as a seeded generator.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }
    fn f(&mut self) -> f32 {
        (self.next() >> 29) as f32 / (1u64 << 24) as f32
    }
}

/// A period's audio: each (message, Hz, DT, SNR in 2500 Hz) over near-Gaussian noise of unit
/// variance (a sum of 12 uniform draws), scaled into 16 bits.
fn mix(sigs: &[(&str, f32, f32, f32)], seed: u64) -> Vec<i16> {
    let mut r = Rng(seed);
    let mut x = vec![0f32; NMAX];
    for v in x.iter_mut() {
        *v = (0..12).map(|_| r.f()).sum::<f32>() - 6.0;
    }
    for &(m, f0, dt, snr) in sigs {
        let w = ft8::gen_wave(&ft8::encode(m), ft8::SAMPLE_RATE, f0);
        let amp = (2.0 * 10f32.powf(snr / 10.0) * 2500.0 / 6000.0).sqrt();
        let o = ((0.5 + dt) * ft8::SAMPLE_RATE) as usize;
        for (i, v) in w.iter().enumerate() {
            if let Some(s) = x.get_mut(o + i) {
                *s += amp * v;
            }
        }
    }
    x.iter()
        .map(|v| (v * 300.0).round().clamp(-32768.0, 32767.0) as i16)
        .collect()
}

/// A weak report 2 Hz from a strong CQ that starts as late (DT 0.6), with a weaker CQ 10 Hz up
/// starting on time: the 47 step subtracts the on-time CQ while the late one is still in the audio
/// (WSJT-X leaves late starters to the full pass), and the remnant it leaves can bury the report.
/// From the raw audio the full pass subtracts the strong CQ first and finds the report.
const UNDER: &str = "SP7QG SP7ZG R-08";
fn crowded(seed: u64) -> Vec<i16> {
    mix(
        &[
            ("AA6EEO OH5WKX JA22", 591.0, 0.02, -10.7),
            ("CQ UA4WO LI20", 644.1, 0.63, 3.5),
            (UNDER, 646.2, 0.6, -8.3),
            ("CQ YO5OG HR40", 656.8, 0.05, -10.1),
        ],
        seed,
    )
}

fn front(all: &[i16], n: usize) -> Vec<i16> {
    let mut f = vec![0i16; NMAX];
    f[..n].copy_from_slice(&all[..n]);
    f
}

/// Who is listening: blind, or in a QSO with AP.
#[derive(Clone, Copy)]
struct Station {
    mycall: &'static str,
    hiscall: &'static str,
    nqso: i32,
    nfqso: i32,
}
const BLIND: Station = Station {
    mycall: "",
    hiscall: "",
    nqso: 0,
    nfqso: 0,
};
const IN_QSO: Station = Station {
    mycall: "W9ZZZ",
    hiscall: "K8ZZZ",
    nqso: 3,
    nfqso: 1500,
};

/// What one period's passes gave.
struct Period {
    /// The early pass's decodes.
    early: Vec<Decode>,
    /// The full pass's list.
    full: Vec<Decode>,
    /// Handed on by the full pass as it ran: (decode, during the rescue).
    streamed: Vec<(Decode, bool)>,
    /// How often [`Progress::Rescue`] was asked.
    rescue_asked: usize,
    /// When the full pass's last sync candidate was tried, and the rescue first asked.
    last_candidate: Option<Instant>,
    first_ask: Option<Instant>,
}

/// Early pass, 47 step, full pass (and its rescue), as the radio loop runs them, for slot key
/// `nutc`. `give_up(n)` answers the n-th [`Progress::Rescue`] (0 = before the rescue starts).
fn carried(
    all: &[i16],
    nutc: i32,
    st: Station,
    carry: &mut Carry,
    give_up: &mut dyn FnMut(usize) -> bool,
) -> Period {
    let early = decode_frame_carry(
        &front(all, 41 * 3456),
        200,
        2900,
        3,
        st.mycall,
        st.hiscall,
        st.nqso,
        st.nfqso,
        st.nfqso,
        nutc,
        false,
        true,
        true,
        false,
        &mut |_| false,
        carry,
    );
    subtract_early(&front(all, ft8::RESIDUAL_SAMPLES), 3, carry, &mut || false);
    assert!(carry.has_residual(), "premise: the 47 step made a residual");
    let mut streamed = Vec::new();
    let mut asked = 0;
    let mut last_candidate = None;
    let mut first_ask = None;
    let full = decode_frame_carry(
        &front(all, 50 * 3456),
        200,
        2900,
        3,
        st.mycall,
        st.hiscall,
        st.nqso,
        st.nfqso,
        st.nfqso,
        nutc,
        true,
        false,
        true,
        false,
        &mut |p| match p {
            Progress::Found(d) => {
                streamed.push((d.clone(), asked > 0));
                false
            }
            Progress::Rescue => {
                first_ask.get_or_insert_with(Instant::now);
                asked += 1;
                give_up(asked - 1)
            }
            Progress::Candidate => {
                last_candidate = Some(Instant::now());
                false
            }
        },
        carry,
    );
    Period {
        early,
        full,
        streamed,
        rescue_asked: asked,
        last_candidate,
        first_ask,
    }
}

/// Today's full pass: the raw audio, every decode.
fn raw(all: &[i16], nutc: i32, st: Station) -> Vec<Decode> {
    decode_frame_a7_streaming(
        &front(all, 50 * 3456),
        200,
        2900,
        3,
        st.mycall,
        st.hiscall,
        st.nqso,
        st.nfqso,
        st.nfqso,
        nutc,
        true,
        false,
        true,
        false,
        &mut |_| false,
    )
}

fn messages(d: &[Decode]) -> Vec<&str> {
    d.iter().map(|d| d.message.as_str()).collect()
}

/// Every field, bit for bit.
fn same(a: &[Decode], b: &[Decode]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.message == b.message
                && a.snr == b.snr
                && a.dt.to_bits() == b.dt.to_bits()
                && a.freq.to_bits() == b.freq.to_bits()
                && a.nap == b.nap
                && a.qual.to_bits() == b.qual.to_bits()
                && a.sync.to_bits() == b.sync.to_bits()
        })
}

/// What `ft8_a7_save` keeps a decode under (the vendored `ft8_a7.f90`): nothing for a message
/// with '/' or '<', else one entry.
fn saved_by_a7(msg: &str) -> bool {
    !msg.contains('/') && !msg.contains('<') && !msg.starts_with("CQ_")
}

/// ⭐ THE RESCUE KEEPS EVERY DECODE THE RAW FULL PASS FINDS. Over twelve noise draws of the crowded
/// scene the period (early pass + full pass and rescue) has every message today's raw full pass
/// has, and every one the full pass from the residual has; the full pass from the residual alone
/// loses the report under the late CQ in at least one draw, and the rescue hands it on. No message
/// is handed on twice, and none the early pass had.
#[test]
fn the_rescue_keeps_every_decode_the_raw_full_pass_finds() {
    let _l = lock();
    let mut lost_alone = 0;
    for seed in 1..=12u64 {
        let all = crowded(seed);
        ft8::a7_reset();
        let today: HashSet<String> = raw(&all, 0, BLIND).into_iter().map(|d| d.message).collect();
        ft8::a7_reset();
        let alone = carried(&all, 0, BLIND, &mut Carry::default(), &mut |_| true);
        ft8::a7_reset();
        let p = carried(&all, 0, BLIND, &mut Carry::default(), &mut |_| false);
        let alone_set: HashSet<&str> = messages(&alone.early)
            .into_iter()
            .chain(messages(&alone.full))
            .collect();
        let period: HashSet<&str> = messages(&p.early)
            .into_iter()
            .chain(messages(&p.full))
            .collect();
        for m in &today {
            assert!(
                period.contains(m.as_str()),
                "seed {seed}: the raw full pass's {m:?} is kept: {period:?}"
            );
        }
        for m in &alone_set {
            assert!(
                period.contains(m),
                "seed {seed}: the residual's {m:?} is kept"
            );
        }
        if today.contains(UNDER) && !alone_set.contains(UNDER) {
            lost_alone += 1;
            assert!(
                p.streamed.iter().any(|(d, r)| d.message == UNDER && *r),
                "seed {seed}: the rescue hands the report on"
            );
        }
        let mut handed = HashSet::new();
        for (d, _) in &p.streamed {
            assert!(
                handed.insert(d.message.as_str()),
                "seed {seed}: {:?} handed on twice",
                d.message
            );
            assert!(
                !messages(&p.early).contains(&d.message.as_str()),
                "seed {seed}: the early pass's {:?} handed on again",
                d.message
            );
        }
        assert!(
            same(&p.full[..alone.full.len()], &alone.full),
            "seed {seed}: the full pass's own list comes first, as without the rescue"
        );
        assert_eq!(
            p.full.len(),
            period.len(),
            "seed {seed}: no message twice in the list"
        );
    }
    assert!(
        lost_alone >= 1,
        "premise: the full pass from the residual alone loses the report in some draw"
    );
}

/// A rescue given up before it starts leaves the full pass's result alone, field for field, and
/// hands nothing more on: it is asked before its first sync candidate, within a fraction of the
/// time that candidate takes after the full pass's last one. Given up after its first candidate,
/// it is asked no more.
#[test]
fn a_rescue_given_up_stops_there() {
    let _l = lock();
    let all = crowded(6);
    ft8::a7_reset();
    let alone = carried(&all, 0, BLIND, &mut Carry::default(), &mut |_| true);
    let before = alone.first_ask.unwrap() - alone.last_candidate.unwrap();
    assert_eq!(alone.rescue_asked, 1, "asked once, before it starts");
    assert!(
        alone.streamed.iter().all(|(_, r)| !r),
        "given up before it starts: nothing from the rescue"
    );
    let mut whole_at = Vec::new();
    ft8::a7_reset();
    let whole = carried(&all, 0, BLIND, &mut Carry::default(), &mut |_| {
        whole_at.push(Instant::now());
        false
    });
    let first = whole_at[1] - whole_at[0];
    assert!(
        before * 4 < first,
        "asked before the rescue tries a candidate: {before:?} after the full pass's last one, \
         against {first:?} for the rescue's first candidate"
    );
    assert!(
        whole.full.len() > alone.full.len(),
        "control: the whole rescue adds a decode here: {:?}",
        messages(&whole.full)
    );
    assert!(
        whole.rescue_asked > 2,
        "control: the whole rescue is asked after every candidate"
    );
    ft8::a7_reset();
    let cut = carried(&all, 0, BLIND, &mut Carry::default(), &mut |n| n >= 1);
    assert_eq!(
        cut.rescue_asked, 2,
        "asked before it starts and after its first candidate, then stopped"
    );
    assert!(
        same(&cut.full[..alone.full.len()], &alone.full),
        "what the full pass found stands"
    );
}

/// A full pass that was given up is not rescued: the rescue is never asked, and nothing more is
/// handed on after the full pass stopped.
#[test]
fn a_full_pass_given_up_is_not_rescued() {
    let _l = lock();
    let all = crowded(6);
    let mut carry = Carry::default();
    ft8::a7_reset();
    let _ = decode_frame_carry(
        &front(&all, 41 * 3456),
        200,
        2900,
        3,
        "",
        "",
        0,
        0,
        0,
        0,
        false,
        true,
        true,
        false,
        &mut |_| false,
        &mut carry,
    );
    subtract_early(
        &front(&all, ft8::RESIDUAL_SAMPLES),
        3,
        &mut carry,
        &mut || false,
    );
    assert!(carry.has_residual(), "premise: the 47 step made a residual");
    let mut rescue = 0;
    let mut tried = 0;
    let _ = decode_frame_carry(
        &front(&all, 50 * 3456),
        200,
        2900,
        3,
        "",
        "",
        0,
        0,
        0,
        0,
        true,
        false,
        true,
        false,
        &mut |p| match p {
            Progress::Candidate => {
                tried += 1;
                true
            }
            Progress::Rescue => {
                rescue += 1;
                false
            }
            Progress::Found(_) => false,
        },
        &mut carry,
    );
    assert_eq!(tried, 1, "the full pass stopped at its first candidate");
    assert_eq!(rescue, 0, "and was not rescued");
}

/// ⭐ THE a7 TABLE SAVES EACH STATION ONCE. The rescue decodes the period's stations again from the
/// raw audio; it saves only those the period had not reported, so the table holds each station of
/// the period once, as today's raw full pass's does.
#[test]
fn the_rescue_saves_each_station_into_the_a7_table_once() {
    let _l = lock();
    for seed in [6u64, 7] {
        let all = crowded(seed);
        ft8::a7_reset();
        let today = raw(&all, 0, BLIND);
        assert_eq!(
            ft8::a7_saved(),
            today.iter().filter(|d| saved_by_a7(&d.message)).count(),
            "control: the raw full pass saves each of its decodes once"
        );
        ft8::a7_reset();
        let p = carried(&all, 0, BLIND, &mut Carry::default(), &mut |_| false);
        assert!(
            p.full.len() >= 3,
            "premise: several stations: {:?}",
            messages(&p.full)
        );
        assert_eq!(
            ft8::a7_saved(),
            p.full.iter().filter(|d| saved_by_a7(&d.message)).count(),
            "seed {seed}: the period's stations, each once: {:?}",
            messages(&p.full)
        );
    }
}

/// ⭐ THE a7 REPLAY OVER SEVERAL PERIODS OF A QSO, with the carry and the rescue as the radio loop
/// runs them and the table kept from period to period. The partner's report is heard in the first
/// period; in the next same-parity period the partner's RR73 is too weak for the direct passes and
/// only the a7 replay finds it, once; each period's table holds each station once; and the period
/// has every decode today's raw full pass finds over the same periods.
#[test]
fn the_a7_replay_over_a_qso_finds_the_weak_continuation_once() {
    let _l = lock();
    let others = [
        ("K1ABC DL2XYZ JO62", "K1ABC DL2XYZ R-11", 700.0, 0.2),
        ("CQ JA1QRP PM95", "CQ JA1QRP PM95", 1100.0, 0.1),
        ("VK3OOP G4WWE IO91", "VK3OOP G4WWE RR73", 2200.0, 0.3),
    ];
    let periods = |rr73_snr: f32| -> Vec<Vec<i16>> {
        let first = [
            ("W9ZZZ K8ZZZ -09", 1500.0, 0.1, -6.0),
            (others[0].0, others[0].2, others[0].3, -8.0),
            (others[1].0, others[1].2, others[1].3, -4.0),
            (others[2].0, others[2].2, others[2].3, -10.0),
        ];
        let second = [
            ("W9ZZZ K8ZZZ RR73", 1500.0, 0.1, rr73_snr),
            (others[0].1, others[0].2, others[0].3, -9.0),
            (others[1].1, others[1].2, others[1].3, -5.0),
            (others[2].1, others[2].2, others[2].3, -12.0),
        ];
        vec![mix(&first, 0x41), mix(&second, 0x42)]
    };
    // The RR73's level: too weak for the direct passes, found by the a7 replay.
    let snr = -26.0;
    let audio = periods(snr);
    ft8::a7_reset();
    let today: Vec<Vec<Decode>> = audio
        .iter()
        .enumerate()
        .map(|(i, a)| raw(a, 30 * i as i32, IN_QSO))
        .collect();
    let rr73 = |d: &[Decode]| {
        d.iter()
            .filter(|d| d.message == "W9ZZZ K8ZZZ RR73")
            .map(|d| d.nap)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rr73(&today[1]),
        vec![7],
        "premise: today's full pass finds the RR73 by the a7 replay only: {:?}",
        messages(&today[1])
    );
    ft8::a7_reset();
    let alone = raw(&audio[1], 30, IN_QSO);
    assert!(
        rr73(&alone).is_empty(),
        "control: without the first period's report in the table, the RR73 is not found"
    );

    ft8::a7_reset();
    let mut carry = Carry::default();
    for (i, a) in audio.iter().enumerate() {
        let p = carried(a, 30 * i as i32, IN_QSO, &mut carry, &mut |_| false);
        let got: Vec<&str> = messages(&p.early)
            .into_iter()
            .chain(messages(&p.full))
            .collect();
        for d in &today[i] {
            assert!(
                got.contains(&d.message.as_str()),
                "period {i}: today's {:?} is kept: {got:?}",
                d.message
            );
        }
        let unique: HashSet<&str> = got.iter().copied().collect();
        assert_eq!(
            ft8::a7_saved(),
            unique.iter().filter(|m| saved_by_a7(m)).count(),
            "period {i}: the table holds each station once: {got:?}"
        );
        if i == 1 {
            assert_eq!(
                rr73(&p.full),
                vec![7],
                "the RR73, by the a7 replay, once: {got:?}"
            );
            assert_eq!(
                p.streamed
                    .iter()
                    .filter(|(d, _)| d.message == "W9ZZZ K8ZZZ RR73")
                    .count(),
                1,
                "handed on once"
            );
        }
    }
}

/// A busy band over four same-parity periods: fifty QSOs, each a step further each period ("A B
/// grid", "A B R-nn", "A B RR73", "A B 73"), the later steps weak enough that many are found only
/// by the a7 replay from the period before, and our own partner on 1500 Hz. The audio of the QSO
/// sets `seqs` (ascending) of a seeded run, each as its four periods' frames.
fn busy_qso_sets(seed: u64, seqs: &[usize]) -> Vec<Vec<Vec<i16>>> {
    const PFX: [&str; 20] = [
        "K", "W", "N", "DL", "G", "F", "JA", "VK", "PY", "LU", "OH", "SM", "EA", "I", "UA", "SP",
        "OK", "HA", "YO", "ON",
    ];
    let mut r = Rng(seed);
    let letter = |r: &mut Rng, n: u64| (b'A' + (r.next() % n) as u8) as char;
    let call = |r: &mut Rng| {
        let p = PFX[(r.next() as usize) % PFX.len()];
        let (a, b, c) = (letter(r, 26), letter(r, 26), letter(r, 26));
        format!("{p}{}{a}{b}{c}", r.next() % 10)
    };
    let grid = |r: &mut Rng| {
        let (a, b) = (letter(r, 18), letter(r, 18));
        format!("{a}{b}{}{}", r.next() % 10, r.next() % 10)
    };
    let mut sets = Vec::new();
    let mut out: Vec<Vec<i16>> = Vec::new();
    for s in 0..=*seqs.iter().max().unwrap_or(&0) {
        let st: Vec<(String, String, String, f32, f32)> = (0..50)
            .map(|_| {
                let (a, b, g) = (call(&mut r), call(&mut r), grid(&mut r));
                (a, b, g, 200.0 + r.f() * 2650.0, r.f() * 1.6 - 0.3)
            })
            .collect();
        out.clear();
        for p in 0..4 {
            let mut sigs: Vec<(String, f32, f32, f32)> = Vec::new();
            for (i, (a, b, g, f0, dt)) in st.iter().enumerate() {
                let m = match p {
                    0 => format!("{a} {b} {g}"),
                    1 => format!("{a} {b} R-{:02}", 5 + i % 15),
                    2 => format!("{a} {b} RR73"),
                    _ => format!("{a} {b} 73"),
                };
                let snr = if p == 0 {
                    -12.0 + r.f() * 14.0
                } else {
                    -24.0 + r.f() * 16.0
                };
                sigs.push((m, *f0, *dt, snr));
            }
            let reply = [
                "W9ZZZ K8ZZZ EN52",
                "W9ZZZ K8ZZZ R-15",
                "W9ZZZ K8ZZZ RR73",
                "W9ZZZ K8ZZZ 73",
            ];
            let rsnr = if p == 0 { -10.0 } else { -22.0 + r.f() * 4.0 };
            sigs.push((reply[p].to_string(), 1500.0, 0.1, rsnr));
            // The noise continues the same generator, period after period.
            let mut x = vec![0f32; NMAX];
            for v in x.iter_mut() {
                *v = (0..12).map(|_| r.f()).sum::<f32>() - 6.0;
            }
            if !seqs.contains(&s) {
                continue;
            }
            for (m, f0, dt, snr) in &sigs {
                let w = ft8::gen_wave(&ft8::encode(m), ft8::SAMPLE_RATE, *f0);
                let amp = (2.0 * 10f32.powf(snr / 10.0) * 2500.0 / 6000.0).sqrt();
                let o = ((0.5 + dt) * ft8::SAMPLE_RATE) as usize;
                for (i, v) in w.iter().enumerate() {
                    if let Some(s) = x.get_mut(o + i) {
                        *s += amp * v;
                    }
                }
            }
            out.push(
                x.iter()
                    .map(|v| (v * 300.0).round().clamp(-32768.0, 32767.0) as i16)
                    .collect(),
            );
        }
        if seqs.contains(&s) {
            sets.push(std::mem::take(&mut out));
        }
    }
    sets
}

/// ⭐ EVERY DECODE OF TODAY'S FULL PASS, PERIOD AFTER PERIOD, with the a7 table carried as on the
/// air. The next period's a7 replay starts from what the table holds for each station, and a
/// replay that starts a little off can miss a weak continuation. The table must hold what the raw
/// full pass saved: the dt and frequency it measured on the whole audio, for a station the early
/// pass found first, and for one the replay over the residual found. Over three sets of four
/// periods of fifty QSOs each period then has every decode today's raw full pass finds; the full
/// pass from the residual alone misses some.
#[test]
fn every_decode_of_todays_full_pass_period_after_period() {
    let _l = lock();
    let mut lost_alone = 0;
    for (set, periods) in busy_qso_sets(20_261_023, &[6, 12, 18]).iter().enumerate() {
        lost_alone += kept_period_after_period(set, periods);
    }
    assert!(
        lost_alone >= 1,
        "premise: the full pass from the residual alone misses one of today's decodes"
    );
}

/// [`every_decode_of_todays_full_pass_period_after_period`] over one QSO set: how many of today's
/// decodes the full pass from the residual alone missed.
fn kept_period_after_period(set: usize, periods: &[Vec<i16>]) -> usize {
    ft8::a7_reset();
    let today: Vec<Vec<Decode>> = periods
        .iter()
        .enumerate()
        .map(|(i, a)| raw(a, 30 * i as i32, IN_QSO))
        .collect();
    let mut lost_alone = 0;
    for rescue in [false, true] {
        ft8::a7_reset();
        let mut carry = Carry::default();
        for (i, a) in periods.iter().enumerate() {
            let p = carried(a, 30 * i as i32, IN_QSO, &mut carry, &mut |_| !rescue);
            let got: HashSet<&str> = messages(&p.early)
                .into_iter()
                .chain(messages(&p.full))
                .collect();
            for d in &today[i] {
                if got.contains(d.message.as_str()) {
                    continue;
                }
                assert!(
                    !rescue,
                    "set {set}, period {i}: today's {:?} (nap {}) is kept",
                    d.message, d.nap
                );
                lost_alone += 1;
            }
        }
    }
    lost_alone
}
