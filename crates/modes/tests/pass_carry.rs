//! A native source keeps WSJT-X's FT8 carry between the passes of a period ([`PassCarry`]): the
//! early pass's decodes, then the 47 step's residual, which the full pass starts from. It is the
//! source's own, and a mode change starts a new one.

use modes::{DecodeRequest, DecodeSink, ModeKind, NativeSource, SignalSource};

const NMAX: usize = 180_000;

/// A strong CQ on 1500 Hz at DT 0 and, a second later on the same frequency, a weak reply it
/// hides from the first subtraction pass over raw audio; light noise.
fn hidden_reply() -> Vec<i16> {
    let mut x = vec![0f32; NMAX];
    let mut s = 0x5EED_1234_u64;
    for v in x.iter_mut() {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *v = ((s >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 4.0;
    }
    for (msg, t0, amp) in [("CQ K2DEF FN42", 0.5, 1.0), ("W9ZZZ K8ZZZ RR73", 1.5, 0.08)] {
        let w = ft8::gen_wave(&ft8::encode(msg), ft8::SAMPLE_RATE, 1500.0);
        let o = (t0 * ft8::SAMPLE_RATE) as usize;
        for (i, v) in w.iter().enumerate() {
            if let Some(slot) = x.get_mut(o + i) {
                *slot += amp * v;
            }
        }
    }
    x.iter()
        .map(|v| (v * 1000.0).round().clamp(-32768.0, 32767.0) as i16)
        .collect()
}

fn front(all: &[i16], n: usize) -> Vec<i16> {
    let mut f = vec![0i16; NMAX];
    f[..n].copy_from_slice(&all[..n]);
    f
}

fn req(iwave: &[i16], partial: bool) -> DecodeRequest<'_> {
    DecodeRequest {
        iwave,
        nfa: 200,
        nfb: 2900,
        ndepth: 3,
        mycall: "W9ZZZ",
        hiscall: "K8ZZZ",
        nqso_progress: 3,
        nfqso: 1500,
        nftx: 1500,
        frame_time_ms: 15_000,
        ap: true,
        ap_cq_only: false,
        activity: false,
        partial,
    }
}

#[derive(Default)]
struct Found(Vec<String>);
impl DecodeSink for Found {
    fn found(&mut self, d: &modes::Decode) {
        self.0.push(d.message.trim().to_string());
    }
}

/// Early pass, 47 step, then (after `between`) the full pass: what the full pass handed on.
fn period(between: impl FnOnce(&mut NativeSource)) -> Vec<String> {
    let all = hidden_reply();
    modes::reset_ft8_a7();
    let mut src = NativeSource::from_kind(ModeKind::Ft8);
    let early = front(&all, 41 * 3456);
    let _ = src.decode_streaming(&req(&early, true), false, &mut Found::default());
    assert_eq!(
        src.carry().ft8_early_len(),
        1,
        "premise: the early CQ carried"
    );
    let at47 = front(&all, 47 * 3456);
    let _ = src.subtract_early(&req(&at47, false), &mut Found::default());
    assert!(src.carry().ft8_has_residual(), "premise: the residual made");
    between(&mut src);
    let full = front(&all, 50 * 3456);
    let mut found = Found::default();
    let _ = src.decode_streaming(&req(&full, false), true, &mut found);
    assert!(
        !src.carry().ft8_has_residual(),
        "the full pass took the carry"
    );
    found.0
}

#[test]
fn the_full_pass_starts_from_the_sources_residual_and_a_mode_change_drops_it() {
    assert_eq!(
        period(|_| {}),
        vec!["W9ZZZ K8ZZZ RR73"],
        "from the residual: only the reply, the early CQ not again"
    );
    assert_eq!(
        period(|src| {
            src.set_mode(modes::make_mode(ModeKind::Ft4));
            src.set_mode(modes::make_mode(ModeKind::Ft8));
            assert_eq!(
                src.carry().ft8_early_len(),
                0,
                "a mode change starts a new carry"
            );
        }),
        vec!["CQ K2DEF FN42", "W9ZZZ K8ZZZ RR73"],
        "after a mode change: raw audio, the CQ handed on again"
    );
}
