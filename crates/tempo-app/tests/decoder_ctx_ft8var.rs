//! Per-chain decoder context for the JTDX-style FT8 decoder (WSJT-X 3.0.2's multithreaded one,
//! `lib/ft8var`): `decoder_ctx.rs`'s acceptance gate, for ft8var's own state.
//!
//! ft8var keeps a radio's QSO across periods in state of its own, separate from the standard
//! decoder's: its own callsign hash tables (packjt77's `*var` half), the per-parity message
//! tables, the average DT, saved signals. `tempo_ctx_t` carries all of it per chain
//! (`libtempo/ft8_cabi.f90`, `modem-state-manifest.toml` GROUP P). Same shape as the standard
//! decoder's test, and as there, the negative control is what makes it worth anything:
//!
//! * a fresh context per chain decodes exactly what no context decodes (one chain);
//! * two chains with disjoint callsign populations, decoded on interleaved periods, each decode
//!   what they decode alone, and never the other's calls;
//! * the same with ONE shared context must cross-contaminate.
//!
//! One thread (Easy is single-thread): deterministic.

use std::sync::{Arc, Mutex};

use tempo_app::dto::Tier;
use tempo_app::engine::{run_decode_job, DecodePass, Engine};
use tempo_app::settings::{DecoderStyle, JtdxPreset, Settings};
use tempo_core::tempo_fast::{self, DecoderCtx};

static SERIAL: Mutex<()> = Mutex::new(());

const TX_START: usize = 6_000;

type Row = (String, i32);

fn wipe_process_modem_state() {
    DecoderCtx::new().scoped(|| {});
}

fn fresh_ctx() -> Arc<Mutex<DecoderCtx>> {
    Arc::new(Mutex::new(DecoderCtx::new()))
}

/// A station on 20 m FT8 whose full pass is the JTDX-style decoder, Easy (one thread).
fn chain() -> Engine {
    let mut eng = Engine::with_settings(Settings {
        mycall: "W9XYZ".to_string(),
        mygrid: "EN37".to_string(),
        band: "20m".to_string(),
        dial_mhz: 14.074,
        sideband: "USB".to_string(),
        ft8_decoder: DecoderStyle::Jtdx,
        jtdx_preset: JtdxPreset::Easy,
        ..Settings::default()
    });
    eng.set_tier(Tier::Ft8);
    eng.set_frequency(14.074, "20m", "USB");
    eng.set_rx_offset(1500.0);
    eng
}

/// `decoder_ctx.rs`'s frame: `msg` at `f0` over a deterministic noise floor, ~+40 dB.
fn frame(msg: &str, f0: f32, mut seed: u32) -> Vec<f32> {
    let mode = modes::make_mode(modes::ModeKind::Ft8);
    let tones = mode.encode(msg);
    assert!(!tones.is_empty(), "{msg} must encode");
    let wave = mode.gen_wave(&tones, tempo_fast::SAMPLE_RATE, f0);
    let n = mode.frame_samples();
    let mut out = vec![0f32; n];
    for s in out.iter_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        *s = (((seed >> 8) % 31) as f32 - 15.0) / 32767.0;
    }
    for (i, &v) in wave.iter().enumerate() {
        if TX_START + i < n {
            out[TX_START + i] += v * 0.0305;
        }
    }
    out
}

fn decode(
    eng: &Engine,
    audio: &[f32],
    slot: u64,
    ctx: Option<&Arc<Mutex<DecoderCtx>>>,
) -> Vec<Row> {
    let mut job = eng.build_decode_job(audio.to_vec(), slot, DecodePass::Boundary);
    if let Some(ctx) = ctx {
        job = job.with_ctx(ctx.clone());
    }
    run_decode_job(job)
        .decodes()
        .iter()
        .map(|d| (d.message.clone(), d.nap))
        .collect()
}

const A_COMPOUND: &str = "PJ4/K1ABC";
const A_PLAIN: &str = "W1AW";
const B_COMPOUND: &str = "KH1/N7GHI";
const B_PLAIN: &str = "K2DEF";

/// Period 0 teaches each chain its own compound call (ft8var's own hash tables); period 1
/// references it by hash and must resolve; period 2 references the OTHER chain's compound call
/// by hash, which a correct decoder prints as `<...>`.
fn bands() -> (Vec<Vec<f32>>, Vec<Vec<f32>>) {
    (
        vec![
            frame(&format!("CQ {A_COMPOUND}"), 1500.0, 0x1111_1111),
            frame(
                &format!("<{A_COMPOUND}> {A_PLAIN} RR73"),
                1700.0,
                0x2222_2222,
            ),
            frame(
                &format!("<{B_COMPOUND}> {A_PLAIN} RR73"),
                1700.0,
                0x3333_3333,
            ),
        ],
        vec![
            frame(&format!("CQ {B_COMPOUND}"), 1500.0, 0x4444_4444),
            frame(
                &format!("<{B_COMPOUND}> {B_PLAIN} RR73"),
                1700.0,
                0x5555_5555,
            ),
            frame(
                &format!("<{A_COMPOUND}> {B_PLAIN} RR73"),
                1700.0,
                0x6666_6666,
            ),
        ],
    )
}

fn solo(slots: &[Vec<f32>]) -> Vec<Vec<Row>> {
    wipe_process_modem_state();
    let eng = chain();
    slots
        .iter()
        .enumerate()
        .map(|(slot, audio)| decode(&eng, audio, slot as u64 * 2, None))
        .collect()
}

#[allow(clippy::type_complexity)]
fn interleaved(
    a: &[Vec<f32>],
    b: &[Vec<f32>],
    ctx_a: &Arc<Mutex<DecoderCtx>>,
    ctx_b: &Arc<Mutex<DecoderCtx>>,
) -> (Vec<Vec<Row>>, Vec<Vec<Row>>) {
    let eng = chain();
    let (mut out_a, mut out_b) = (Vec::new(), Vec::new());
    for slot in 0..a.len() {
        out_a.push(decode(&eng, &a[slot], slot as u64 * 2, Some(ctx_a)));
        out_b.push(decode(&eng, &b[slot], slot as u64 * 2, Some(ctx_b)));
    }
    (out_a, out_b)
}

fn mentions(rows: &[Vec<Row>], call: &str) -> bool {
    rows.iter().flatten().any(|(m, _)| m.contains(call))
}

#[test]
fn a_fresh_context_decodes_what_no_context_decodes() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (a, _) = bands();
    let without = solo(&a);
    wipe_process_modem_state();
    let eng = chain();
    let ctx = fresh_ctx();
    let with: Vec<Vec<Row>> = a
        .iter()
        .enumerate()
        .map(|(slot, audio)| decode(&eng, audio, slot as u64 * 2, Some(&ctx)))
        .collect();
    assert!(
        mentions(&without, A_COMPOUND),
        "the session must decode: {without:?}"
    );
    assert_eq!(without, with, "a context changed what one chain decodes");
}

#[test]
fn two_chains_do_not_cross_contaminate_and_a_shared_context_does() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (a, b) = bands();
    let solo_a = solo(&a);
    let solo_b = solo(&b);
    assert!(
        mentions(&solo_a, A_COMPOUND) && !mentions(&solo_a, B_COMPOUND),
        "chain A alone knows only its own compound call; got {solo_a:?}"
    );
    assert!(
        solo_a[2].iter().any(|(m, _)| m.contains("<...>")),
        "chain A alone prints <...> for the call it never heard; got {:?}",
        solo_a[2]
    );

    wipe_process_modem_state();
    let (got_a, got_b) = interleaved(&a, &b, &fresh_ctx(), &fresh_ctx());
    assert!(
        !mentions(&got_a, B_COMPOUND) && !mentions(&got_b, A_COMPOUND),
        "the chains cross-contaminated: A {got_a:?} B {got_b:?}"
    );
    assert_eq!(solo_a, got_a, "chain A's decodes changed beside chain B");
    assert_eq!(solo_b, got_b, "chain B's decodes changed beside chain A");

    // NEGATIVE CONTROL: one context for both must contaminate, or the above proves nothing.
    wipe_process_modem_state();
    let shared = fresh_ctx();
    let (bad_a, bad_b) = interleaved(&a, &b, &shared, &shared);
    assert!(
        mentions(&bad_a, B_COMPOUND) || mentions(&bad_b, A_COMPOUND),
        "NEGATIVE CONTROL FAILED: a shared context did not cross-contaminate. A {bad_a:?} B \
         {bad_b:?}"
    );
}
