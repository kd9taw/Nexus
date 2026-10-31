//! WSJT-X's FT8 full pass starts from the early pass's residual (`lib/ft8_decode.f90`: the early
//! decodes' tones kept at :229-231, the 47 step subtracting them at :124-149, the full pass
//! starting from that residual and carrying the early decodes at :121 and :152-162): a station the
//! early pass decoded is gone from the full pass's audio, so what it hid is found in the first
//! subtraction pass and handed on first, and no early decode is searched for or handed on again.
//! [`ft8::Carry`] is that state, owned by the caller and checked against the audio.

use ft8::{decode_frame_a7_streaming, decode_frame_carry, subtract_early, Carry, Decode, Progress};

const NMAX: usize = ft8::NMAX;
const MYCALL: &str = "W9ZZZ";
const HISCALL: &str = "K8ZZZ";
const REPLY: &str = "W9ZZZ K8ZZZ RR73";
const STRONG: &str = "CQ K2DEF FN42";

/// A period's audio: a strong CQ on 1500 Hz at DT 0, and on the same 1500 Hz a weaker reply
/// starting `rdt` s later, the strong station hiding it from the first subtraction pass; light
/// uniform noise. `seed` varies the noise only.
fn scene(reply_amp: f32, rdt: f32, noise: f32, seed: u64) -> Vec<i16> {
    scene_with(reply_amp, rdt, noise, seed, &[])
}

/// [`scene`] with more stations: (message, Hz, DT, amplitude).
fn scene_with(
    reply_amp: f32,
    rdt: f32,
    noise: f32,
    seed: u64,
    more: &[(&str, f32, f32, f32)],
) -> Vec<i16> {
    scene_at(0.0, reply_amp, rdt, noise, seed, more)
}

/// [`scene_with`], the strong CQ at DT `strong_dt` (the reply `rdt` after it).
fn scene_at(
    strong_dt: f32,
    reply_amp: f32,
    rdt: f32,
    noise: f32,
    seed: u64,
    more: &[(&str, f32, f32, f32)],
) -> Vec<i16> {
    let mut x = vec![0f32; NMAX];
    let mut s = seed;
    for v in x.iter_mut() {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *v = ((s >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * noise;
    }
    let mut add = |msg: &str, f0: f32, t0: f32, amp: f32| {
        let w = ft8::gen_wave(&ft8::encode(msg), ft8::SAMPLE_RATE, f0);
        let o = (t0 * ft8::SAMPLE_RATE) as usize;
        for (i, v) in w.iter().enumerate() {
            if let Some(slot) = x.get_mut(o + i) {
                *slot += amp * v;
            }
        }
    };
    add(STRONG, 1500.0, 0.5 + strong_dt, 1.0);
    add(REPLY, 1500.0, 0.5 + strong_dt + rdt, reply_amp);
    for &(msg, f0, dt, amp) in more {
        add(msg, f0, 0.5 + dt, amp);
    }
    x.iter()
        .map(|v| (v * 1000.0).round().clamp(-32768.0, 32767.0) as i16)
        .collect()
}

/// The scene the reply is found in by both full passes, after the strong station from raw audio
/// and first from the residual.
fn hidden_reply() -> Vec<i16> {
    scene(0.08, 1.0, 4.0, 0x5EED_1234)
}

/// The first `n` samples of `all`, zero after: the frame the radio loop hands each pass.
fn front(all: &[i16], n: usize) -> Vec<i16> {
    let mut f = vec![0i16; NMAX];
    f[..n].copy_from_slice(&all[..n]);
    f
}

fn early(all: &[i16]) -> Vec<i16> {
    front(all, 41 * 3456)
}
fn at47(all: &[i16]) -> Vec<i16> {
    front(all, ft8::RESIDUAL_SAMPLES)
}
fn full(all: &[i16]) -> Vec<i16> {
    front(all, 50 * 3456)
}

/// One pass through `decode_frame_carry`: (returned, streamed).
fn pass(
    frame: &[i16],
    partial: bool,
    a7_final: bool,
    carry: &mut Carry,
) -> (Vec<Decode>, Vec<Decode>) {
    let (got, streamed, _) = pass_counted(frame, partial, a7_final, carry);
    (got, streamed)
}

/// [`pass`], also telling how many sync candidates the pass had tried before each decode it
/// handed on.
fn pass_counted(
    frame: &[i16],
    partial: bool,
    a7_final: bool,
    carry: &mut Carry,
) -> (Vec<Decode>, Vec<Decode>, Vec<usize>) {
    let mut streamed = Vec::new();
    let mut tried = 0;
    let mut at = Vec::new();
    let got = decode_frame_carry(
        frame,
        200,
        2900,
        3,
        MYCALL,
        HISCALL,
        3,
        1500,
        1500,
        0,
        a7_final,
        partial,
        true,
        false,
        false,
        &mut |p| {
            match p {
                Progress::Found(d) => {
                    streamed.push(d.clone());
                    at.push(tried);
                }
                Progress::Candidate => tried += 1,
                // The full pass from the residual alone: the rescue after it is tested below.
                Progress::Rescue => return true,
            }
            false
        },
        carry,
    );
    (got, streamed, at)
}

/// The full pass as it ran before: raw audio, no carry. (returned, streamed).
fn raw_full(frame: &[i16]) -> (Vec<Decode>, Vec<Decode>) {
    let mut streamed = Vec::new();
    let got = decode_frame_a7_streaming(
        frame,
        200,
        2900,
        3,
        MYCALL,
        HISCALL,
        3,
        1500,
        1500,
        0,
        true,
        false,
        true,
        false,
        false,
        &mut |p| {
            if let Progress::Found(d) = p {
                streamed.push(d.clone());
            }
            false
        },
    );
    (got, streamed)
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

/// Early pass, 47 step, full pass, as the radio loop runs them: (early, full returned, full
/// streamed, the candidates the full pass had tried before each, decodes the step reached).
fn period(all: &[i16]) -> (Vec<Decode>, Vec<Decode>, Vec<Decode>, Vec<usize>, usize) {
    ft8::a7_reset();
    let mut carry = Carry::default();
    let (e, _) = pass(&early(all), true, false, &mut carry);
    let reached = subtract_early(&at47(all), 3, &mut carry, &mut || false);
    assert!(carry.has_residual(), "premise: the 47 step made a residual");
    let (f, s, at) = pass_counted(&full(all), false, true, &mut carry);
    (e, f, s, at, reached)
}

/// The RX frequency's candidates come first (`sync8.f90:155-163`): a reply there that nothing
/// hides is found within the first few, before the rest of the band is searched.
const FIRST_FEW: usize = 3;

/// ⭐ THE ORDER. The early pass decodes the strong CQ; the reply hidden under it is found by the
/// full pass. From raw audio the full pass first decodes the strong CQ again and hands it on, and
/// finds the reply only in its second subtraction pass, after the whole band's candidates. From
/// the residual the CQ is gone: the reply is found among the first candidates, the first decode the
/// pass hands on and the only one, and the early decode stands first in the pass's list exactly as
/// the early pass returned it.
#[test]
fn the_full_pass_on_the_residual_hands_on_new_decodes_first_and_never_an_early_one_again() {
    let all = hidden_reply();
    let (e, f, s, at, reached) = period(&all);
    assert_eq!(
        messages(&e),
        vec![STRONG],
        "premise: the early pass has the strong CQ only"
    );
    assert_eq!(reached, 1, "the 47 step reached the one early decode");
    assert_eq!(
        messages(&s),
        vec![REPLY],
        "from the residual: the reply is handed on first, and the early CQ is not handed on again"
    );
    assert!(
        at[0] <= FIRST_FEW,
        "the reply is found among the RX frequency's first candidates, after {} tried",
        at[0]
    );
    assert!(
        same(&f[..1], &e),
        "the early decode opens the full pass's list, as the early pass returned it"
    );
    assert_eq!(
        messages(&f),
        vec![STRONG, REPLY],
        "the pass's list: the early decode, then the new one"
    );

    // Control: the same full pass from raw audio re-finds the CQ first.
    ft8::a7_reset();
    let mut c = Carry::default();
    let _ = pass(&early(&all), true, false, &mut c);
    let (_, rs, rat) = pass_counted(&full(&all), false, true, &mut Carry::default());
    assert_eq!(
        messages(&rs),
        vec![STRONG, REPLY],
        "control: from raw audio the early CQ is handed on again, before the reply"
    );
    assert!(
        rat[1] > 50,
        "control: from raw audio the reply comes after the band's candidates, at {}",
        rat[1]
    );
}

/// What the residual buys: a reply the strong station hides from every pass over raw audio is
/// found from the residual.
#[test]
fn a_reply_only_the_residual_reveals_is_found() {
    let all = scene(0.15, 0.3, 8.0, 0x5EED_1234);
    let (e, _, s, _, _) = period(&all);
    assert_eq!(
        messages(&e),
        vec![STRONG],
        "premise: the early pass has the strong CQ only"
    );
    assert_eq!(messages(&s), vec![REPLY], "from the residual: the reply");
    ft8::a7_reset();
    let (rf, _) = raw_full(&full(&all));
    assert!(
        !messages(&rf).contains(&REPLY),
        "control: from raw audio the reply stays hidden, got {:?}",
        messages(&rf)
    );
}

/// A residual is spliced only onto the audio it was made from. A full pass over other audio
/// (another period, a band change, a ring that skipped) decodes that audio raw, exactly as the
/// full pass always did; so does one whose 47 step met audio the early pass did not decode.
#[test]
fn a_residual_is_never_spliced_onto_other_audio() {
    let a = hidden_reply();
    let mut b = a.clone();
    b[1000] = b[1000].wrapping_add(1); // one sample of the first 13.536 s differs
    let raw_b = {
        ft8::a7_reset();
        raw_full(&full(&b))
    };

    // Early and 47 step on A, full pass on B.
    ft8::a7_reset();
    let mut carry = Carry::default();
    let _ = pass(&early(&a), true, false, &mut carry);
    let _ = subtract_early(&at47(&a), 3, &mut carry, &mut || false);
    assert!(carry.has_residual(), "premise: a residual of A");
    let (f, s) = pass(&full(&b), false, true, &mut carry);
    assert!(
        same(&f, &raw_b.0),
        "B decoded raw, field for field: {:?}",
        messages(&f)
    );
    assert!(same(&s, &raw_b.1), "and handed on as raw");
    assert!(
        !carry.has_residual() && carry.early_len() == 0,
        "the full pass took the carry"
    );

    // Early on A, 47 step on B: no residual, B's full pass raw.
    ft8::a7_reset();
    let mut carry = Carry::default();
    let _ = pass(&early(&a), true, false, &mut carry);
    assert_eq!(subtract_early(&at47(&b), 3, &mut carry, &mut || false), 0);
    assert!(
        !carry.has_residual() && carry.early_len() == 0,
        "no carry from another audio"
    );
    let (f, _) = pass(&full(&b), false, true, &mut carry);
    assert!(same(&f, &raw_b.0), "B decoded raw");
}

/// The full pass takes the carry: a second full pass over the same audio starts from raw audio.
/// The F6 re-decode (neither early nor final) leaves it for the full pass.
#[test]
fn the_full_pass_takes_the_carry_and_a_redecode_leaves_it() {
    let all = hidden_reply();
    ft8::a7_reset();
    let raw = raw_full(&full(&all));

    ft8::a7_reset();
    let mut carry = Carry::default();
    let _ = pass(&early(&all), true, false, &mut carry);
    let _ = subtract_early(&at47(&all), 3, &mut carry, &mut || false);
    // A re-decode of another period's audio in between.
    let (_, rs) = pass(&full(&scene(0.3, 0.0, 4.0, 7)), false, false, &mut carry);
    assert!(!rs.is_empty(), "premise: the re-decode decoded");
    assert!(carry.has_residual(), "the re-decode left the carry");
    let (_, s) = pass(&full(&all), false, true, &mut carry);
    assert_eq!(
        messages(&s),
        vec![REPLY],
        "the full pass still started from the residual"
    );

    // Again, with nothing carried: raw.
    ft8::a7_reset();
    let (f2, s2) = pass(&full(&all), false, true, &mut carry);
    assert!(
        same(&f2, &raw.0) && same(&s2, &raw.1),
        "the second full pass decoded raw audio"
    );
}

/// No carry at depth 1 (WSJT-X runs no early pass there, `ft8_decode.f90:102-105`), nor from an
/// early pass that decoded nothing; an early pass replaces what was carried.
#[test]
fn what_an_early_pass_leaves() {
    let all = hidden_reply();
    let mut carry = Carry::default();
    ft8::a7_reset();
    let _ = decode_frame_carry(
        &early(&all),
        200,
        2900,
        1,
        MYCALL,
        HISCALL,
        3,
        1500,
        1500,
        0,
        false,
        true,
        true,
        false,
        false,
        &mut |_| false,
        &mut carry,
    );
    assert_eq!(carry.early_len(), 0, "depth 1: nothing carried");

    let _ = pass(&early(&all), true, false, &mut carry);
    assert_eq!(carry.early_len(), 1, "depth 3: the early decode carried");
    let silence = vec![0i16; NMAX];
    let (e, _) = pass(&silence, true, false, &mut carry);
    assert!(e.is_empty(), "premise: nothing in silence");
    assert_eq!(
        carry.early_len(),
        0,
        "an early pass that decoded nothing carries nothing"
    );
    assert_eq!(subtract_early(&at47(&all), 3, &mut carry, &mut || false), 0);
    assert!(!carry.has_residual());
}

/// What WSJT-X's 47 step leaves, its full pass subtracts first (`ft8_decode.f90:157-160`): a
/// decode that ends after the step's 13.536 s (dt 0.396 s or later, `:135`), and any the step did
/// not reach before its 14.3 s bail-out (`:140-145`). Here the strong CQ starts 0.6 s late, so the
/// step leaves it, and the step gives up after it, before the second early decode. The full pass
/// still finds the reply under the CQ among its first candidates.
#[test]
fn what_the_47_step_left_the_full_pass_subtracts() {
    const OTHER: &str = "CQ N1XYZ FN31";
    let all = scene_at(
        0.6,
        0.08,
        0.6,
        4.0,
        0x5EED_1234,
        &[(OTHER, 800.0, 0.0, 1.0)],
    );
    ft8::a7_reset();
    let mut carry = Carry::default();
    let (e, _) = pass(&early(&all), true, false, &mut carry);
    assert_eq!(
        messages(&e),
        vec![STRONG, OTHER],
        "premise: both CQs, the RX frequency's first"
    );
    let mut asked = 0;
    let reached = subtract_early(&at47(&all), 3, &mut carry, &mut || {
        asked += 1;
        true
    });
    assert_eq!(
        (reached, asked),
        (1, 1),
        "asked after the first early decode, and given up"
    );
    assert!(carry.has_residual(), "what was done is kept");
    assert_eq!(
        carry.early_subtracted(),
        0,
        "the step left the late CQ, and never reached the other"
    );
    let (_, s, at) = pass_counted(&full(&all), false, true, &mut carry);
    assert_eq!(messages(&s), vec![REPLY], "only the reply is handed on");

    // Not given up: the step subtracts the other CQ (DT 0) and leaves the late one (DT 0.6).
    ft8::a7_reset();
    let mut whole = Carry::default();
    let _ = pass(&early(&all), true, false, &mut whole);
    assert_eq!(subtract_early(&at47(&all), 3, &mut whole, &mut || false), 2);
    assert_eq!(
        whole.early_subtracted(),
        1,
        "only the CQ that ends inside 13.536 s"
    );
    assert!(
        at[0] <= FIRST_FEW,
        "the full pass subtracted the CQ the step left: the reply among the first candidates, after {}",
        at[0]
    );
}
