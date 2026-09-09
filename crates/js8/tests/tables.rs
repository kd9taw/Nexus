//! Table gate for the transcribed LDPC(174,87) and Costas tables.
//!
//! WHY: a wrong integer in Mn/Nm fails SILENTLY as decoder under-performance (the code still
//! "works", ~1 dB worse), and a wrong generator row or colorder entry makes every frame
//! undecodable by JS8Call while our own loopback stays green. So the two independent
//! transcriptions are proved against EACH OTHER: G/colorder come from WSJT-X's
//! `ldpc_174_87_params.f90`, Mn/Nm/nrw from JS8Call's `bpdecode174.f90`; if
//! G·Hᵀ = 0 for every unit message they agree. The generator multiply here is written
//! independently of `js8::phy::ldpc::encode87` (which B1.6 tests against this one).
//! No network, no upstream text: `scripts/gen-js8-ldpc-tables.mjs` did the fetch and the
//! provenance sha256s ride in the generated file as constants pinned here.

use js8::phy::ldpc_tables::{
    COLORDER, GENERATOR, MN, NM, NRW, SOURCE_JS8CALL_BPDECODE_SHA256, SOURCE_WSJTX_PARAMS_SHA256,
};

const N: usize = 174;
const M: usize = 87;

/// H[j][i] = 1 iff bit i takes part in check j — built from NM (rows) only.
fn h_from_nm() -> Vec<[u8; N]> {
    let mut h = vec![[0u8; N]; M];
    for j in 0..M {
        for k in 0..NRW[j] as usize {
            h[j][NM[j][k] as usize] = 1;
        }
    }
    h
}

/// The same matrix built from MN (columns) only.
fn h_from_mn() -> Vec<[u8; N]> {
    let mut h = vec![[0u8; N]; M];
    for (i, checks) in MN.iter().enumerate() {
        for &c in checks {
            h[c as usize][i] = 1;
        }
    }
    h
}

/// Reference encoder written from encode174.f90:35-47 — NOT `js8::phy::ldpc::encode87`.
fn reference_codeword(msg: &[u8; M]) -> [u8; N] {
    let mut itmp = [0u8; N];
    for (i, row) in GENERATOR.iter().enumerate() {
        let mut acc = 0u32;
        for (j, &m) in msg.iter().enumerate() {
            let g = (row[j / 8] >> (7 - j % 8)) & 1;
            acc += u32::from(g & m);
        }
        itmp[i] = (acc % 2) as u8;
    }
    itmp[M..].copy_from_slice(msg);
    let mut cw = [0u8; N];
    for i in 0..N {
        cw[COLORDER[i] as usize] = itmp[i];
    }
    cw
}

fn syndrome_is_zero(h: &[[u8; N]], cw: &[u8; N]) -> bool {
    h.iter().all(|row| {
        row.iter()
            .zip(cw)
            .map(|(&a, &b)| u32::from(a & b))
            .sum::<u32>()
            % 2
            == 0
    })
}

fn lcg(state: &mut u64) -> u32 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*state >> 33) as u32
}

#[test]
fn provenance_pins_are_the_verified_upstream_files() {
    // ldpc_174_87_params.f90 (WSJT-X, byte-identical in JS8Call @ a7ff1be0) and JS8Call's
    // lib/ft8/bpdecode174.f90 @ a7ff1be0 — sha256 verified 2026-09-05 against GitHub raw.
    assert_eq!(
        SOURCE_WSJTX_PARAMS_SHA256,
        "98f802adb997b856801366c12567d9af0c089fdc96b1523e012508adb05f8650"
    );
    assert_eq!(
        SOURCE_JS8CALL_BPDECODE_SHA256,
        "c185d6e5716b7f18c5010e29361d46df47708bc383aabd9f36b8dd1b086a6c5f"
    );
}

#[test]
#[allow(clippy::needless_range_loop)] // indexing COLORDER by the on-air position, not the value
fn colorder_is_a_permutation_that_leaves_the_message_half_in_place() {
    let mut seen = [false; N];
    for &c in &COLORDER {
        assert!(!seen[c as usize], "duplicate colorder entry {c}");
        seen[c as usize] = true;
    }
    assert!(seen.iter().all(|&s| s));
    // encode174.f90:45-47: itmp = [parity | message]; cw[colorder[i]] = itmp[i]; the message
    // half must land at cw[87..174) in order, i.e. colorder[87..] is the identity.
    for i in M..N {
        assert_eq!(COLORDER[i] as usize, i, "colorder[{i}]");
    }
    let mut low: Vec<u8> = COLORDER[..M].to_vec();
    low.sort_unstable();
    assert_eq!(
        low,
        (0..M as u8).collect::<Vec<_>>(),
        "colorder[0..87) permutes the parity half"
    );
}

#[test]
#[allow(clippy::needless_range_loop)] // indexing NM[j][k] with two independent counters
fn nrw_counts_the_used_nm_entries_and_the_rest_are_poisoned() {
    for j in 0..M {
        let n = NRW[j] as usize;
        assert!((5..=7).contains(&n), "check {j} has {n} neighbours");
        for k in 0..n {
            assert!((NM[j][k] as usize) < N, "check {j} slot {k}");
        }
        for k in n..7 {
            assert_eq!(
                NM[j][k],
                u8::MAX,
                "check {j} slot {k} past NRW must be the poison value"
            );
        }
    }
}

#[test]
fn parity_check_from_nm_equals_parity_check_from_mn() {
    let a = h_from_nm();
    let b = h_from_mn();
    assert_eq!(
        a, b,
        "Nm (per-check) and Mn (per-bit) describe different matrices"
    );
    // Regular column weight 3 (bpdecode174.f90: "3 checks per bit"), rows 5/6/7.
    for i in 0..N {
        assert_eq!(a.iter().map(|r| u32::from(r[i])).sum::<u32>(), 3, "bit {i}");
    }
}

#[test]
fn generator_times_parity_check_transpose_is_zero() {
    let h = h_from_nm();
    // Every unit message: exercises every generator row bit-column and every colorder entry.
    for k in 0..M {
        let mut msg = [0u8; M];
        msg[k] = 1;
        let cw = reference_codeword(&msg);
        assert!(
            syndrome_is_zero(&h, &cw),
            "unit message e_{k} is not a codeword"
        );
    }
    // And 200 random messages.
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    for t in 0..200 {
        let mut msg = [0u8; M];
        for m in msg.iter_mut() {
            *m = (lcg(&mut state) & 1) as u8;
        }
        let cw = reference_codeword(&msg);
        assert!(
            syndrome_is_zero(&h, &cw),
            "random message {t} is not a codeword"
        );
        assert_eq!(&cw[M..], &msg[..], "message half must be systematic");
    }
}

#[test]
fn costas_rows_are_permutations_of_the_seven_tones() {
    for (name, table) in [
        ("ORIGINAL", js8::phy::costas::COSTAS_ORIGINAL),
        ("MODIFIED", js8::phy::costas::COSTAS_MODIFIED),
    ] {
        for row in table {
            let mut sorted = row;
            sorted.sort_unstable();
            assert_eq!(sorted, [0, 1, 2, 3, 4, 5, 6], "{name} row {row:?}");
        }
    }
}
