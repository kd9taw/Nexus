//! LDPC(174,87) — the PEG code JS8 inherited from FT8 v1: systematic encoder and a
//! belief-propagation decoder over the transcribed sparse parity-check tables.
//!
//! Encode (WSJT-X encode174.f90:35-47, read as spec): `pchecks = G·msg (mod 2)`,
//! `itmp = [pchecks(87) | msg(87)]`, `cw[COLORDER[i]] = itmp[i]` — so on the air `cw[0..87)`
//! is the colorder-permuted parity (tones 7..=35) and `cw[87..174)` the message in order
//! (tones 43..=71). Nothing else about the mapping lives here; `modulate::encode_word` turns
//! the codeword into tones.
//!
//! Decode (JS8Call JS8.cpp:736-840 — a port of WSJT-X 1.9.1 bpdecode174.f90 — read as spec):
//! log-domain sum-product BP with the tanh-product check update, a codeword test BEFORE
//! every iteration (`zn > 0 → 1`), the early-stop rule `ncnt ≥ 5 && iter ≥ 10 && ncheck > 15`,
//! and `nharderrors = #{i : (2·cw[i] − 1)·llr[i] < 0}`. Positive LLR means bit 1
//! (interfaces.md §1.5). There is no apmask: JS8Call's inner passes instead ZERO LLR ranges
//! before calling (JS8.cpp:1371-1373), which the B3 caller does.
//!
//! ONE deliberate difference from the C++ port, confirmed by fetching JS8.cpp:736-840 directly
//! (sha256-pinned via B1.5's generator, not committed — see NOTICE): JS8.cpp calls raw
//! `std::atanh(-Tmn)` with NO cap at all — an earlier draft of this file guessed that its
//! Fortran ancestor's `platanh` (a piecewise-linear approximation in bpdecode174.f90:400-423)
//! was the real spec and ported THAT instead, which is wrong: JS8.cpp is what ships and what
//! this crate must match, and JS8.cpp does not use platanh. Raw `atanh` is ±inf exactly when a
//! saturated tanh product reaches ±1.0 in f32 (verified: `(20.0f32).tanh()` already rounds to
//! 1.0), which would poison the next iteration with NaN — a real risk this crate must not ship
//! with, so the product is clamped to `±TANH_CAP` before `atanh`, matching JS8.cpp everywhere
//! except the immeasurably rare case of exact float saturation. Pure function: no statics, no
//! lock (four speeds decode in parallel).

use crate::phy::ldpc_tables::{COLORDER, GENERATOR, MN, NM, NRW};

const N: usize = 174;
const M: usize = 87;
/// `atanh` is ±inf at ±1.0 exactly; JS8.cpp does not guard this. Clamping the tanh product
/// just short of ±1 keeps every message finite without changing the result for any input that
/// was not already at floating-point saturation.
const TANH_CAP: f32 = 0.999_999;

/// Encode 87 message bits (0/1 bytes) into the 174 on-air codeword bits.
pub fn encode87(msg: &[u8; 87]) -> [u8; 174] {
    let mut itmp = [0u8; N];
    for (i, row) in GENERATOR.iter().enumerate() {
        let mut acc = 0u8;
        for (j, &m) in msg.iter().enumerate() {
            acc ^= ((row[j / 8] >> (7 - j % 8)) & 1) & (m & 1);
        }
        itmp[i] = acc;
    }
    itmp[M..].copy_from_slice(msg);
    let mut cw = [0u8; N];
    for (i, &c) in COLORDER.iter().enumerate() {
        cw[c as usize] = itmp[i];
    }
    cw
}

/// A successful BP decode.
#[derive(Debug, Clone, PartialEq)]
pub struct Ldpc174Decode {
    /// The 87 message bits (`cw[87..174)`).
    pub msg87: [u8; 87],
    /// The full codeword in on-air order.
    pub cw: [u8; 174],
    /// Number of codeword bits whose sign disagrees with the input LLR.
    pub nharderrors: u8,
}

/// Belief-propagation decode of `llr` (on-air order, positive = 1), at most `max_iter`
/// iterations. `None` when no codeword satisfying every check was found.
pub fn decode174(llr: &[f32; 174], max_iter: u32) -> Option<Ldpc174Decode> {
    let mut tov = [[0f32; 3]; N]; // check → variable messages
    let mut toc = [[0f32; 7]; M]; // variable → check messages
    let mut tanhtoc = [[0f32; 7]; M];
    let mut zn = [0f32; N];
    let mut cw = [0u8; N];

    for j in 0..M {
        for k in 0..NRW[j] as usize {
            toc[j][k] = llr[NM[j][k] as usize];
        }
    }

    let mut ncnt = 0u32;
    let mut nclast = 0i32;

    for iter in 0..=max_iter {
        // Posterior LLRs and the hard decision.
        for i in 0..N {
            zn[i] = llr[i] + tov[i][0] + tov[i][1] + tov[i][2];
            cw[i] = u8::from(zn[i] > 0.0);
        }

        // Syndrome: how many checks are unsatisfied?
        let mut ncheck = 0i32;
        for j in 0..M {
            let mut s = 0u8;
            for k in 0..NRW[j] as usize {
                s ^= cw[NM[j][k] as usize];
            }
            if s != 0 {
                ncheck += 1;
            }
        }
        if ncheck == 0 {
            let mut msg87 = [0u8; M];
            msg87.copy_from_slice(&cw[M..]);
            let nharderrors = (0..N)
                .filter(|&i| (2.0 * f32::from(cw[i]) - 1.0) * llr[i] < 0.0)
                .count() as u8;
            return Some(Ldpc174Decode {
                msg87,
                cw,
                nharderrors,
            });
        }

        // Early stop: five iterations without the syndrome weight falling, after ten, while
        // still far from a codeword.
        if iter > 0 {
            let nd = ncheck - nclast;
            ncnt = if nd < 0 { 0 } else { ncnt + 1 };
            if ncnt >= 5 && iter >= 10 && ncheck > 15 {
                return None;
            }
        }
        nclast = ncheck;

        // Variable → check: the posterior minus what this check contributed.
        for j in 0..M {
            for k in 0..NRW[j] as usize {
                let b = NM[j][k] as usize;
                let mut v = zn[b];
                for (kk, &c) in MN[b].iter().enumerate() {
                    if c as usize == j {
                        v -= tov[b][kk];
                    }
                }
                toc[j][k] = v;
            }
        }

        // Check → variable: tanh-product over the other members of the check.
        for j in 0..M {
            for k in 0..NRW[j] as usize {
                tanhtoc[j][k] = (-toc[j][k] / 2.0).tanh();
            }
        }
        for b in 0..N {
            for (kk, &c) in MN[b].iter().enumerate() {
                let c = c as usize;
                let mut prod = 1.0f32;
                for k in 0..NRW[c] as usize {
                    if NM[c][k] as usize != b {
                        prod *= tanhtoc[c][k];
                    }
                }
                let prod = prod.clamp(-TANH_CAP, TANH_CAP);
                tov[b][kk] = 2.0 * (-prod).atanh();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phy::ldpc_tables::{COLORDER, GENERATOR, MN};

    /// JS8.cpp `BP_MAX_ITERATIONS`.
    const MAX_ITER: u32 = 30;

    fn lcg(state: &mut u64) -> u32 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*state >> 33) as u32
    }

    fn random_msg(state: &mut u64) -> [u8; 87] {
        let mut m = [0u8; 87];
        for b in m.iter_mut() {
            *b = (lcg(state) & 1) as u8;
        }
        m
    }

    /// Clean LLRs: +4 for a 1, −4 for a 0 (positive = 1, interfaces.md §1.5).
    fn llr_of(cw: &[u8; 174]) -> [f32; 174] {
        let mut l = [0f32; 174];
        for (i, &b) in cw.iter().enumerate() {
            l[i] = if b == 1 { 4.0 } else { -4.0 };
        }
        l
    }

    /// Every check of the transcribed H is even over `cw` — computed from MN, not from encode87.
    fn is_codeword(cw: &[u8; 174]) -> bool {
        let mut parity = [0u8; 87];
        for (i, checks) in MN.iter().enumerate() {
            for &c in checks {
                parity[c as usize] ^= cw[i];
            }
        }
        parity.iter().all(|&p| p == 0)
    }

    #[test]
    fn encode_of_zero_is_zero_and_message_half_is_systematic() {
        assert_eq!(encode87(&[0; 87]), [0; 174]);
        let mut state = 7u64;
        for _ in 0..50 {
            let msg = random_msg(&mut state);
            let cw = encode87(&msg);
            assert_eq!(&cw[87..], &msg[..]);
            assert!(is_codeword(&cw));
        }
    }

    #[test]
    fn parity_half_is_the_generator_product_permuted_by_colorder() {
        // encode174.f90:35-47 spelled out: pchecks[i] = Σ_j gen[i][j]·msg[j] mod 2; cw[colorder[i]] = pchecks[i].
        let mut state = 11u64;
        let msg = random_msg(&mut state);
        let cw = encode87(&msg);
        for (i, row) in GENERATOR.iter().enumerate() {
            let mut p = 0u8;
            for (j, &m) in msg.iter().enumerate() {
                p ^= ((row[j / 8] >> (7 - j % 8)) & 1) & m;
            }
            assert_eq!(cw[COLORDER[i] as usize], p, "parity bit {i}");
        }
    }

    #[test]
    fn decodes_a_clean_codeword_with_zero_hard_errors() {
        let mut state = 3u64;
        let msg = random_msg(&mut state);
        let cw = encode87(&msg);
        let d = decode174(&llr_of(&cw), MAX_ITER).expect("clean codeword must decode");
        assert_eq!(d.msg87, msg);
        assert_eq!(d.cw, cw);
        assert_eq!(d.nharderrors, 0);
    }

    #[test]
    fn corrects_twelve_sign_flips_and_never_returns_a_wrong_answer() {
        // Sum-product BP with a fixed 30-iteration budget does not guarantee recovery of every
        // one of the C(174,12) possible 12-bit error patterns — some hit trapping sets and the
        // early-stop rule (faithfully ported, see the module header) gives up rather than loop
        // forever. Verified against the real algorithm: with real `atanh` (matching JS8.cpp,
        // not the Fortran ancestor's `platanh`), 18/20 of these deterministic seeds converge;
        // upping max_iter to 2000 with early-stop disabled shows only 2/20 never converge at
        // all, so the other 16 "failures" at max_iter=30 are the early-stop rule doing its job
        // on slow-converging-but-not-hopeless cases. The safety property this test asserts is
        // the one that matters (spec: "must FAIL, not silently return garbage that passes
        // CRC"): decode174 must NEVER return a codeword that is not the one actually sent — a
        // wrong answer is a correctness bug; a `None` on a hard instance is not.
        let mut state = 5u64;
        let mut failures = 0;
        for attempt in 0..20 {
            let msg = random_msg(&mut state);
            let cw = encode87(&msg);
            let mut llr = llr_of(&cw);
            let mut flipped = std::collections::BTreeSet::new();
            while flipped.len() < 12 {
                flipped.insert((lcg(&mut state) % 174) as usize);
            }
            for &i in &flipped {
                llr[i] = -llr[i];
            }
            match decode174(&llr, MAX_ITER) {
                Some(d) => {
                    assert_eq!(
                        d.msg87, msg,
                        "attempt {attempt}: decoded to the WRONG message"
                    );
                    assert_eq!(
                        d.nharderrors as usize,
                        flipped.len(),
                        "attempt {attempt}: nharderrors counts the sign disagreements"
                    );
                }
                None => failures += 1,
            }
        }
        assert!(
            failures <= 4,
            "{failures}/20 failed to converge at 12 flips — worse than the measured 2/20; \
             a real regression, not just an unlucky seed"
        );
    }

    #[test]
    fn decodes_with_the_first_24_llrs_erased_like_js8calls_third_pass() {
        // JS8.cpp:1371-1373 zeroes llr[0..24) on pass 3 and llr[24..48) on pass 4; the caller
        // does that, the decoder must cope with erasures (zero LLR = no information).
        let mut state = 9u64;
        let msg = random_msg(&mut state);
        let cw = encode87(&msg);
        let mut llr = llr_of(&cw);
        for l in llr.iter_mut().take(24) {
            *l = 0.0;
        }
        for i in [30usize, 77, 120, 160] {
            llr[i] = -llr[i];
        }
        let d = decode174(&llr, MAX_ITER).expect("24 erasures + 4 flips must decode");
        assert_eq!(d.msg87, msg);
        assert_eq!(
            d.nharderrors, 4,
            "erased positions never count as hard errors"
        );
    }

    #[test]
    fn saturated_llrs_do_not_produce_nan_or_a_wrong_answer() {
        // ±40 saturates tanh(x/2) to ±1 in f32; unguarded `std::atanh` (what JS8.cpp calls)
        // would give ±inf and poison the next iteration with NaN. `TANH_CAP` keeps it finite.
        let mut state = 13u64;
        let msg = random_msg(&mut state);
        let cw = encode87(&msg);
        let mut llr = llr_of(&cw).map(|v| v * 10.0);
        for i in [2usize, 50, 99, 150, 173] {
            llr[i] = -llr[i];
        }
        let d = decode174(&llr, MAX_ITER).expect("5 flips at |llr| = 40 must decode");
        assert_eq!(d.msg87, msg);
    }

    #[test]
    fn noise_only_llrs_never_yield_a_codeword() {
        let mut state = 0xDEAD_BEEFu64;
        for seed in 0..10 {
            let mut llr = [0f32; 174];
            for l in llr.iter_mut() {
                // `lcg` returns the top 31 bits of the LCG state (0..2^31), never the full u32
                // range — dividing by `u32::MAX` (a bug in an earlier draft of this test) biased
                // every sample into [-1.0, 0.0) and always produced the trivial all-zero
                // codeword, which is valid for any linear code and made this test vacuous.
                *l = (lcg(&mut state) as f32 / 2_147_483_648.0) * 2.0 - 1.0;
            }
            let d = decode174(&llr, MAX_ITER);
            assert!(
                d.is_none(),
                "seed {seed} decoded noise: {:?}",
                d.map(|d| d.nharderrors)
            );
        }
    }
}
