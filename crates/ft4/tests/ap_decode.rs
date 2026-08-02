//! Behavioural proof that the FT4 `lapcqonly` operator control actually reaches
//! the vendored WSJT-X `ft4_decoder%decode` — the FT4 half of the AP knob whose
//! FT8 half is pinned by `ft8/tests/ap_decode.rs`.
//!
//! FT4 has NO AP on/off flag (AP runs whenever `ndepth > 1`), so CQ-only is the
//! only decoder-honest AP restriction there is, and the only thing to pin.

use ft4::{decode_frame, encode, gen_wave, NMAX, SAMPLE_RATE};

struct Rng(u64);
impl Rng {
    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn gauss(&mut self) -> f32 {
        let u1 = (self.next_f64() + 1e-12).min(1.0);
        let u2 = self.next_f64();
        ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
    }
}

/// One encoded FT4 message at `f0`, scaled to `snr_db` (WSJT-X 2500 Hz
/// convention), from sample 0 (FT4 `gen_wave` fills the frame), + AWGN.
/// Identical construction to `decode_parity.rs`'s `frame_with`.
fn frame_with(msg: &str, f0: f32, snr_db: f32, seed: u64) -> Vec<i16> {
    let wave = gen_wave(&encode(msg), SAMPLE_RATE, f0);
    let bw_ratio = 2500.0 / (SAMPLE_RATE / 2.0);
    let sig = (2.0 * bw_ratio).sqrt() * 10f32.powf(0.05 * snr_db);
    let mut dd = vec![0f32; NMAX];
    for (i, &w) in wave.iter().enumerate() {
        if i < NMAX {
            dd[i] += sig * w;
        }
    }
    let mut rng = Rng(seed);
    dd.iter()
        .map(|&s| (((s + rng.gauss()) * 100.0).clamp(-32768.0, 32767.0)) as i16)
        .collect()
}

/// `ap_cq_only` provably reaches the vendored FT4 decoder — the honest-knob
/// proof, and the regression barrier a SEVERED wire has to trip.
///
/// A test that only asserts "decodes in both flag states" pins ABI marshalling
/// and nothing else: hard-wiring `lapcqonly=.false.` in `ft4_cabi.f90` would
/// pass it. This one is behavioural. Operating point (see `explore_ft4_ap_cq_margin`):
/// an "RR73 addressed to me" at a marginal SNR, carrier 1500 Hz with nfqso on it
/// and nQSOProgress = 3. In `ft4_decode.f90` that schedules AP passes
/// `naptypes(3,1:2) = (3,6)`, and iaptype 6 forces all 77 bits — the only way
/// this frame comes back. With `lapcqonly` set, the same file caps `npasses = 4`
/// and forces `iaptype = 1` (CQ), and an RR73 is not a CQ, so the frame must
/// stay lost.
///
/// Measured at −19 dB / 24 seeds: full AP 21/24, CQ-only 1/24. Sever the wire
/// anywhere between the Rust arg and `ft4_decoder%decode` and the two columns
/// collapse into each other, failing the strict `>` assertion.
#[test]
fn ft4_ap_cq_only_provably_restricts_the_ap_passes() {
    let msg = "KD9TAW W1AW RR73"; // needs iaptype 6 — the deepest FT4 AP
    let seeds = 24u64;
    let (mut full_ap, mut full_ap_via_ap_pass, mut cq_only) = (0u32, 0u32, 0u32);
    for seed in 0..seeds {
        let iwave = frame_with(msg, 1500.0, -19.0, seed);
        // Full AP: the deep hypotheses (iaptype 3, then 6) are live.
        let decs = decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3, 1500, false);
        if let Some(d) = decs.iter().find(|d| d.message == msg) {
            full_ap += 1;
            if d.nap > 0 {
                full_ap_via_ap_pass += 1; // credited to an AP pass, not luck
            }
        }
        // Same audio, same AP context, AP restricted to the CQ hypothesis.
        if decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3, 1500, true)
            .iter()
            .any(|d| d.message == msg)
        {
            cq_only += 1;
        }
    }
    assert!(
        full_ap >= 16,
        "full AP must recover this frame in most seeds, got {full_ap}/{seeds} \
         (operating point drifted — re-run explore_ft4_ap_cq_margin)"
    );
    assert!(
        full_ap_via_ap_pass >= 16,
        "recoveries must be credited to an AP pass (nap>0), got {full_ap_via_ap_pass}/{full_ap}"
    );
    assert!(
        cq_only <= 3,
        "ap_cq_only = true must suppress non-CQ AP recovery, got {cq_only}/{seeds} — \
         the flag is not reaching ft4_decode.f90's lapcqonly"
    );
    assert!(
        full_ap > cq_only,
        "CQ-only must strictly under-recover full AP ({full_ap} vs {cq_only}) — \
         equal columns mean the wire is severed and the flag changes nothing"
    );
}

/// EXPLORATION ONLY (ignored): recovery of a deep-AP-only FT4 frame with AP full
/// vs restricted to CQ, across SNR — how the operating point above was chosen.
///   cargo test -p ft4 --test ap_decode explore_ft4_ap_cq_margin -- --ignored --nocapture
#[test]
#[ignore]
fn explore_ft4_ap_cq_margin() {
    let msg = "KD9TAW W1AW RR73";
    let seeds = 24u64;
    for &snr in &[-16.0f32, -17.0, -18.0, -18.5, -19.0, -19.5, -20.0, -21.0] {
        let (mut full_ap, mut cq_only, mut no_ctx) = (0u32, 0u32, 0u32);
        for seed in 0..seeds {
            let iwave = frame_with(msg, 1500.0, snr, seed);
            if decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3, 1500, false)
                .iter()
                .any(|d| d.message == msg)
            {
                full_ap += 1;
            }
            if decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3, 1500, true)
                .iter()
                .any(|d| d.message == msg)
            {
                cq_only += 1;
            }
            if decode_frame(&iwave, 200, 2900, 3, "", "", 0, 0, false)
                .iter()
                .any(|d| d.message == msg)
            {
                no_ctx += 1;
            }
        }
        println!(
            "SNR {snr:>6} dB:  full-AP {full_ap:2}/{seeds}   CQ-only {cq_only:2}/{seeds}   no-context {no_ctx:2}/{seeds}"
        );
    }
}
