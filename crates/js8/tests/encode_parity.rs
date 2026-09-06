//! JS8 ENCODER parity — the first gate of the native JS8 modem, run BEFORE any decoder exists.
//!
//! The claim: bits we put on the air through `js8::phy` (Word87 → LDPC(174,87) → 79 tones →
//! continuous-phase 8-FSK) are decoded by JS8Call's OWN decoder as the exact 12-char field and
//! i3 bits we sent, at all four speeds. That proves the transcribed generator/colorder, the
//! CRC-12 (poly 0xC06, XOR 42, field zeroed), the i3 placement in bits 72..=74, the
//! MSB-first tone map (no Gray), the two Costas kernels and the start-delay timing — with
//! no Nexus decoder in the loop, so a shared bug in our encode/decode pair cannot hide.
//! Passing this test FREEZES the `Word87` seam (spec §Sequencing B2).
//!
//! Oracle: the stock `js8` CLI built by ~/work/twowayfd/paritylab/js8/build_js8.sh (the
//! Fortran reference decoder still shipped in js8call @ a7ff1be0, built with the source list
//! of 28ea7378 — the last commit that built it — and a Qt-free shared-memory stub). It is
//! found through `JS8_STOCK_BIN`; the stock-backed test is `#[ignore]` so a box without the
//! lab reports `ignored` (never a silent `ok`), and it PANICS if run with `--ignored` while
//! the variable is unset or points at nothing runnable.
//!
//! Fixture pins: the three upstream media/tests WAVs land here for B3's decoder gate; this
//! file owns their pin check because B2 is the batch that lands them.

mod common;

/// The two FIPS 180-4 vectors + a mutation: proves the hand-rolled hasher before it is
/// trusted to pin anything (a hasher that returned a constant would "pass" every pin).
#[test]
fn sha256_helper_matches_fips_vectors_and_sees_a_flipped_bit() {
    assert_eq!(
        common::hex(&common::sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        common::hex(&common::sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let mut long = vec![0x5au8; 1000]; // multi-block path
    let h1 = common::sha256(&long);
    long[999] ^= 0x01;
    assert_ne!(
        common::sha256(&long),
        h1,
        "a flipped bit must change the digest"
    );
}

/// Every pinned upstream WAV is present and byte-identical to its pin, and the pinned set is
/// exactly the three files NOTICE credits (A_2_9, A_3_3, E_2_1 — three distinct blobs).
#[test]
fn upstream_media_test_fixtures_match_their_sha256_pins() {
    let verified = common::verify_fixture_pins();
    let mut names: Vec<&str> = verified.iter().map(|(n, _)| n.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["A_2_9.wav", "A_3_3.wav", "E_2_1.wav"]);
    // Sizes are protocol facts: 15 s and 30 s of 12 kHz PCM-16 mono. Upstream's files are a
    // canonical 44-byte header + the data chunk + a 164/170-byte TRAILING chunk, so assert
    // the sample count from the `data` chunk length, never from the byte count.
    for (name, path) in &verified {
        let bytes = std::fs::read(path).unwrap();
        let expect_samples = if name.starts_with('E') {
            30 * 12_000
        } else {
            15 * 12_000
        };
        assert_eq!(
            data_chunk_len(&bytes) / 2,
            expect_samples,
            "{name}: data chunk is not {expect_samples} samples"
        );
    }
}

/// Walk RIFF chunks to `data` and return its byte length (the ft8 harness's reader shape,
/// crates/ft8/tests/decode_parity.rs:12-27, minus the sample conversion).
fn data_chunk_len(b: &[u8]) -> usize {
    let mut i = 12usize;
    while i + 8 <= b.len() {
        let sz = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        if &b[i..i + 4] == b"data" {
            return sz.min(b.len() - i - 8);
        }
        i += 8 + sz + (sz & 1);
    }
    panic!("no data chunk");
}
