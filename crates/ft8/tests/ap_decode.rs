//! Behavioural proof that a-priori (AP) decoding actually fires and recovers
//! frames the no-context path misses — exercising the SAME golden WSJT-X decoder
//! the engine feeds real MyCall/DxCall/nQSOProgress/nfqso into.

use ft8::{decode_frame, decode_frame_a7, encode, gen_wave, NMAX, SAMPLE_RATE};

/// Unit-variance Gaussian (LCG + Box-Muller) — matches tempo-core's `Awgn` so the
/// SNR convention is identical, without a cross-crate dep.
struct Awgn {
    state: u64,
}
impl Awgn {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }
    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 32) as u32
    }
    fn u01(&mut self) -> f64 {
        (self.next_u32() as f64 + 1.0) / (u32::MAX as f64 + 2.0)
    }
    fn sample(&mut self) -> f32 {
        let u1 = self.u01();
        let u2 = self.u01();
        ((-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()) as f32
    }
}

fn snr_to_scale(snr_db: f32, fs: f32) -> f32 {
    let bw_ratio = 2500.0 / (fs / 2.0);
    (2.0 * bw_ratio).sqrt() * 10f32.powf(0.05 * snr_db)
}

/// Build a noisy 15 s int16 frame containing `msg` at carrier 1500 Hz, SNR
/// `snr_db`, AWGN seed `seed`.
fn noisy_frame(msg: &str, snr_db: f32, seed: u64) -> Vec<i16> {
    noisy_frame_at(msg, snr_db, seed, 1500.0)
}

/// As [`noisy_frame`] but at an explicit audio carrier `f0`.
fn noisy_frame_at(msg: &str, snr_db: f32, seed: u64, f0: f32) -> Vec<i16> {
    let tones = encode(msg);
    let wave = gen_wave(&tones, SAMPLE_RATE, f0);
    let sig = snr_to_scale(snr_db, SAMPLE_RATE);
    let mut noise = Awgn::new(seed);
    let noff = 6_000usize; // FT8 TX starts 0.5 s into the slot
    let mut buf = vec![0f32; NMAX];
    for (i, &s) in wave.iter().enumerate() {
        if noff + i < NMAX {
            buf[noff + i] = sig * s;
        }
    }
    for s in buf.iter_mut() {
        *s += noise.sample();
    }
    buf.iter()
        .map(|&x| (x * 100.0).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16)
        .collect()
}

/// AP recovers frames the no-context decoder cannot, and the recovery is
/// attributed to an actual AP pass (`nap > 0`), not a coincidental standalone
/// decode. This drives the SAME golden WSJT-X `ft8b` the engine feeds real
/// MyCall/DxCall/nQSOProgress into, so it is the behavioural proof of the wiring.
///
/// Operating point −22 dB (see `explore_ap_margin`): on this RR73-addressed-to-me
/// message the deepest AP case (iaptype 6, all 77 bits known) recovers ~every
/// seed while the no-context path recovers none — a several-dB gain. Carrier is
/// 1500 Hz with nfqso = 1500, so the deep AP window is centred on it.
#[test]
fn ap_recovers_frames_the_no_context_path_cannot() {
    let msg = "KD9TAW W1AW RR73"; // RR73 to me → iaptype 6 (all 77 ap bits)
    let seeds = 12u64;
    let (mut ap, mut ap_via_ap_pass, mut noap) = (0u32, 0u32, 0u32);
    for seed in 0..seeds {
        let iwave = noisy_frame(msg, -22.0, seed);
        // AP context: responder awaiting RR73 → nQSOProgress = 3; nfqso on carrier.
        let decs = decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3, 1500, true, false);
        if let Some(d) = decs.iter().find(|d| d.message == msg) {
            ap += 1;
            if d.nap > 0 {
                ap_via_ap_pass += 1; // recovery explicitly credited to an AP pass
            }
        }
        if decode_frame(&iwave, 200, 2900, 3, "", "", 0, 0, true, false)
            .iter()
            .any(|d| d.message == msg)
        {
            noap += 1;
        }
    }
    assert_eq!(
        noap, 0,
        "the no-context decoder must NOT recover this frame at -22 dB, got {noap}/{seeds}"
    );
    assert!(
        ap >= 9,
        "AP must recover the frame in most seeds, got {ap}/{seeds}"
    );
    assert!(
        ap_via_ap_pass >= 9,
        "recoveries must be credited to an AP pass (nap>0), got {ap_via_ap_pass}/{ap}"
    );
}

/// The operator AP controls provably reach ft8b — the honest-knob proof for the
/// decode-config surface. Same operating point as
/// [`ap_recovers_frames_the_no_context_path_cannot`]: a −22 dB RR73 addressed to
/// me, recoverable ONLY by the deep AP passes (iaptype ≥ 3). Therefore:
/// - `ap = false` (WSJT-X "Enable AP" off, ft8b `lft8apon`): AP passes 5-8 never
///   run — the frame must NOT decode even with full MyCall/DxCall context.
/// - `ap_cq_only = true` (ft8b `lapcqonly`): AP is restricted to the CQ
///   hypothesis (iaptype 1), and an RR73 is not a CQ — the frame must NOT
///   decode either.
///
/// If either assertion fails, the flag renders in the UI but never reaches the
/// decoder — the placebo-knob failure mode this surface forbids.
#[test]
fn ap_off_and_cq_only_provably_gate_the_ap_passes() {
    let msg = "KD9TAW W1AW RR73"; // needs iaptype 6 — deepest AP
    let seeds = 12u64;
    let (mut ap_off_hits, mut cq_only_hits) = (0u32, 0u32);
    for seed in 0..seeds {
        let iwave = noisy_frame(msg, -22.0, seed);
        // Full AP context supplied but AP switched OFF.
        if decode_frame(
            &iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3, 1500, false, false,
        )
        .iter()
        .any(|d| d.message == msg)
        {
            ap_off_hits += 1;
        }
        // AP on but restricted to the CQ hypothesis only.
        if decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3, 1500, true, true)
            .iter()
            .any(|d| d.message == msg)
        {
            cq_only_hits += 1;
        }
    }
    assert_eq!(
        ap_off_hits, 0,
        "ap = false must suppress every deep-AP recovery (got {ap_off_hits}/{seeds}) — \
         the flag is not reaching ft8b's lft8apon"
    );
    assert_eq!(
        cq_only_hits, 0,
        "ap_cq_only = true must suppress non-CQ AP recovery (got {cq_only_hits}/{seeds}) — \
         the flag is not reaching ft8b's lapcqonly"
    );
}

/// The deep AP passes (iaptype>=3 — the MyCall+DxCall masks that give the big
/// gain) only fire within ~napwid (75 Hz) of nfqso. So nfqso MUST track the
/// worked station's carrier, or the gain is stuck at band-center. Proof at a
/// carrier 850 Hz off centre: nfqso-on-carrier recovers it; nfqso-at-band-center
/// (0 → ~1550) does not. This is the behavioural proof of the nfqso plumbing.
#[test]
fn nfqso_steers_deep_ap_to_an_off_center_carrier() {
    let msg = "KD9TAW W1AW RR73";
    let f0 = 2400.0f32; // ~850 Hz above band center (~1550)
    let seeds = 12u64;
    let (mut steered, mut centered) = (0u32, 0u32);
    for seed in 0..seeds {
        let iwave = noisy_frame_at(msg, -22.0, seed, f0);
        // nfqso ON the carrier → deep AP fires there.
        if decode_frame(
            &iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3, f0 as i32, true, false,
        )
        .iter()
        .any(|d| d.message == msg)
        {
            steered += 1;
        }
        // nfqso = 0 → C-ABI falls back to band center; the off-center station is
        // outside the deep-AP window, so iaptype>=3 never fires for it.
        if decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3, 0, true, false)
            .iter()
            .any(|d| d.message == msg)
        {
            centered += 1;
        }
    }
    assert!(
        steered >= 9,
        "nfqso on the carrier must recover the off-center frame ({steered}/{seeds})"
    );
    assert!(
        centered <= 1,
        "band-center nfqso must NOT deep-AP an 850 Hz-off station ({centered}/{seeds})"
    );
    assert!(
        steered > centered,
        "steering nfqso must strictly out-recover band-center ({steered} vs {centered})"
    );
}

/// a7-inert decode (constant `nutc`, `a7_final = false` — see `decode_frame`'s
/// doc for why both are load-bearing) with the two pass-shape inputs exposed.
#[allow(clippy::too_many_arguments)]
fn decode_shaped(
    iwave: &[i16],
    mycall: &str,
    hiscall: &str,
    nqso_progress: i32,
    nfqso: i32,
    nftx: i32,
    partial: bool,
) -> Vec<ft8::Decode> {
    decode_frame_a7(
        iwave,
        200,
        2900,
        3,
        mycall,
        hiscall,
        nqso_progress,
        nfqso,
        nftx,
        0,
        false,
        partial,
        true,
        false,
    )
}

/// WSJT-X's deep AP fires in TWO windows, not one: `nfqso ± napwid` **and**
/// `nftx ± napwid`. `ft8b.f90:305` cycles a candidate only when it is outside
/// BOTH, and `mainwindow.cpp:3722` feeds nftx straight from the Tx-frequency
/// spin box — so whenever the operator has "Hold Tx Freq" on and RX/TX are
/// split, upstream still gets the deep masks on its own transmit frequency.
/// That is not a corner case: it is where the station answering your CQ calls.
///
/// The control here is tighter than
/// [`nfqso_steers_deep_ap_to_an_off_center_carrier`]'s: nfqso is IDENTICAL in
/// both arms (so sync8's own nfqso weighting is held constant) and only nftx
/// moves. If nftx does not reach ft8b, the two arms are the same decode and the
/// off-window carrier is unrecoverable in both.
#[test]
fn nftx_opens_a_second_deep_ap_window_on_the_tx_offset() {
    let msg = "KD9TAW W1AW RR73"; // needs iaptype 6 — gated by the napwid windows
    let nfqso = 1500i32; // where we listen
    let f0 = 2400.0f32; // the caller answers 900 Hz away, on OUR tx offset
    let seeds = 12u64;
    let (mut split, mut collapsed) = (0u32, 0u32);
    for seed in 0..seeds {
        let iwave = noisy_frame_at(msg, -22.0, seed, f0);
        // Hold Tx Freq on: we transmit at 2400 while listening at 1500.
        if decode_shaped(&iwave, "KD9TAW", "W1AW", 3, nfqso, f0 as i32, false)
            .iter()
            .any(|d| d.message == msg)
        {
            split += 1;
        }
        // nftx collapsed onto nfqso — one window, which is what shipped before
        // this parameter existed and what an unsplit operator still gets.
        if decode_shaped(&iwave, "KD9TAW", "W1AW", 3, nfqso, nfqso, false)
            .iter()
            .any(|d| d.message == msg)
        {
            collapsed += 1;
        }
    }
    assert!(
        split >= 9,
        "the tx-offset window must recover the caller answering on our tx freq \
         ({split}/{seeds}) — nftx is not reaching ft8b.f90:305"
    );
    assert!(
        collapsed <= 1,
        "with both windows on nfqso, a 900 Hz-away caller must NOT deep-AP \
         ({collapsed}/{seeds}) — the arms are not actually differing"
    );
}

/// WSJT-X's EARLY decode is a deliberately CHEAP pass, and the cheapness is the
/// point: it runs 2.6 s before the boundary pass re-decodes the same audio, and
/// an early pass that overruns is what makes the boundary decode land late and
/// cost the whole period. Upstream buys that with two mechanisms, both keyed on
/// the same `nzhsym = 41` (`mainwindow.cpp:1878`, `m_earlyDecode` at
/// `mainwindow.h:533`): the sync floor rises 1.3 → 2.0 (`ft8_decode.f90:178`)
/// and the AP passes 5-8 are switched off outright (`ft8b.f90:275`,
/// `if(nzhsym.lt.50) npasses=4`).
///
/// So this is the observable, and it is a NEGATIVE on purpose: a frame that only
/// the deep AP masks can reach must NOT come back from the early pass, while the
/// identical audio on the full-frame pass must. Paired with a strong signal that
/// survives both, which is what separates "upstream's cheap pass" from "the
/// early pass is broken".
#[test]
fn the_early_pass_drops_the_ap_passes_exactly_as_wsjtx_does() {
    let msg = "KD9TAW W1AW RR73"; // recoverable at -22 dB ONLY via iaptype 6
    let loud = "CQ W9XYZ EN52"; // ordinary strong signal, no AP needed
    let seeds = 12u64;
    let (mut full_ap, mut early_ap) = (0u32, 0u32);
    let (mut full_loud, mut early_loud) = (0u32, 0u32);
    for seed in 0..seeds {
        let iwave = noisy_frame(msg, -22.0, seed);
        if decode_shaped(&iwave, "KD9TAW", "W1AW", 3, 1500, 1500, false)
            .iter()
            .any(|d| d.message == msg)
        {
            full_ap += 1;
        }
        if decode_shaped(&iwave, "KD9TAW", "W1AW", 3, 1500, 1500, true)
            .iter()
            .any(|d| d.message == msg)
        {
            early_ap += 1;
        }

        let strong = noisy_frame(loud, 0.0, seed);
        if decode_shaped(&strong, "", "", 0, 1500, 1500, false)
            .iter()
            .any(|d| d.message == loud)
        {
            full_loud += 1;
        }
        if decode_shaped(&strong, "", "", 0, 1500, 1500, true)
            .iter()
            .any(|d| d.message == loud)
        {
            early_loud += 1;
        }
    }
    assert!(
        full_ap >= 9,
        "the full-frame pass must still deep-AP this frame ({full_ap}/{seeds})"
    );
    assert_eq!(
        early_ap, 0,
        "the early pass must run NO AP passes ({early_ap}/{seeds}) — nzhsym is \
         not reaching ft8b's npasses clamp (ft8b.f90:275)"
    );
    assert_eq!(
        (full_loud, early_loud),
        (seeds as u32, seeds as u32),
        "a strong signal must decode on BOTH passes — the early pass is cheaper, \
         not broken"
    );
}

/// EXPLORATION ONLY (ignored): with nfqso left at band center (0), does deep AP
/// fire across the band? Demonstrates the limitation the nfqso plumbing fixes —
/// recovery should peak near ~1550 Hz and fall off away from it. Run with:
///   cargo test -p ft8 --test ap_decode explore_ap_vs_frequency -- --ignored --nocapture
#[test]
#[ignore]
fn explore_ap_vs_frequency() {
    let msg = "KD9TAW W1AW RR73"; // needs iaptype 6 (gated by napwid around nfqso)
    let seeds = 12u64;
    for &f0 in &[700.0f32, 1100.0, 1500.0, 1900.0, 2300.0, 2600.0] {
        let (mut centered, mut steered) = (0u32, 0u32);
        for seed in 0..seeds {
            let iwave = noisy_frame_at(msg, -22.0, seed, f0);
            if decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3, 0, true, false)
                .iter()
                .any(|d| d.message == msg)
            {
                centered += 1;
            }
            if decode_frame(
                &iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3, f0 as i32, true, false,
            )
            .iter()
            .any(|d| d.message == msg)
            {
                steered += 1;
            }
        }
        println!("carrier {f0:>6} Hz:  nfqso=center {centered:2}/{seeds}   nfqso=carrier {steered:2}/{seeds}");
    }
}

/// EXPLORATION ONLY (ignored): print AP-vs-no-AP recovery across an SNR band so we
/// can pick a marginal operating point. Run with:
///   cargo test -p ft8 --test ap_decode explore_ap_margin -- --ignored --nocapture
#[test]
#[ignore]
fn explore_ap_margin() {
    let msg = "KD9TAW W1AW RR73"; // RR73 to me → deepest AP (iaptype 6, all 77 bits)
    for &snr in &[-18.0f32, -20.0, -21.0, -22.0, -23.0, -24.0, -25.0, -26.0] {
        let (mut ap, mut noap) = (0u32, 0u32);
        let seeds = 12u64;
        for seed in 0..seeds {
            let iwave = noisy_frame(msg, snr, seed);
            if decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3, 1500, true, false)
                .iter()
                .any(|d| d.message == msg)
            {
                ap += 1;
            }
            if decode_frame(&iwave, 200, 2900, 3, "", "", 0, 0, true, false)
                .iter()
                .any(|d| d.message == msg)
            {
                noap += 1;
            }
        }
        println!("SNR {snr:>6} dB:  AP {ap:2}/{seeds}   no-AP {noap:2}/{seeds}");
    }
}
