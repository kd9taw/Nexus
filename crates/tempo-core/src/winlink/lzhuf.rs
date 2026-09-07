// This file is a Rust port of the LZHUF codec in ARSFI's Winlink-Compression
// (`WinlinkSupport.vb`) — <https://github.com/ARSFI/Winlink-Compression> — which is
// distributed under the BSD 3-Clause License. The ~450-line codec is a port of *expression*,
// not a table of protocol facts, so that licence's notice is reproduced here verbatim from the
// upstream `License.txt` (the `.vb` file itself carries no per-file notice; this header restores
// it). BSD-3-Clause is permissive and combines one-way-inbound into Nexus's GPL-3.0-only whole,
// under which this port is redistributed. See the `NOTICE` entry "Winlink LZHUF compression".
//
//              Copyright (c) Amateur Radio Safety Foundation, Inc.
//                           All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
//
//     * Redistributions of source code must retain the above copyright notice,
//       this list of conditions and the following disclaimer.
//     * Redistributions in binary form must reproduce the above copyright notice,
//       this list of conditions and the following disclaimer in the
//       documentation and/or other materials provided with the distribution.
//     * Neither the name of Amateur Radio Safety Foundation, Inc. (ARSFI),
//       Winlink Global Radio Email(R), nor the names of its contributors
//       may be used to endorse or promote products derived from this software
//       without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
// ARE DISCLAIMED. IN NO EVENT SHALL THE AUTHORS OR CONTRIBUTORS BE LIABLE FOR
// ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
// OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
//
// The upstream lineage recorded in `WinlinkSupport.vb`, kept here because a port that loses it
// loses the only credit those authors were ever given: LZHUF.C English version 1.0, based on the
// Japanese version of 29-NOV-1988 — LZSS by Haruhiko Okumura, adaptive Huffman coding by Haruyasu
// Yoshizaki, edited and translated to English by Kenji Rikitake, converted to Turbo Pascal 5.0 by
// Peter Sawatzki with assistance of Wayne Sullivan, ported to C# (2008) and then VB (2010) by
// Peter Woods. (The LZH patent, US 4,906,991, has expired.)

//! LZHUF — the adaptive-Huffman-over-LZSS codec Winlink uses for compressed B2F bodies.
//!
//! LZSS with an adaptive Huffman coder over it, in the exact form Winlink forwarding requires.
//! The compressed image is an interoperability fact, so this is a **port of a specific
//! implementation** (ARSFI's, see the licence header above) rather than an independent codec:
//! every routine below keeps the reference's structure and name so a future reader can diff the
//! two. Where this file deliberately departs from ARSFI it says so at the point of departure.
//!
//! # The wire image
//!
//! [`compress`] emits, and [`decompress`] expects, the FBB **B2** image Winlink puts on the wire:
//!
//! ```text
//! byte 0..2    CRC16, little-endian          <- the 2-byte prefix; see LZHUF_CRC_OVER_COMPRESSED
//! byte 2..6    uncompressed length, u32 LE   <- the "textSize" header
//! byte 6..     the adaptive-Huffman bit stream
//! ```
//!
//! The CRC covers **bytes 2.. of the image** — the length header plus the bit stream, i.e. the
//! compressed form, not the plaintext. See [`LZHUF_CRC_OVER_COMPRESSED`] for how that was settled
//! and what is still owed to a live CMS.
//!
//! # Parameters — and one correction to the plan
//!
//! Ring buffer **N = 2048**, lookahead **F = 60**, threshold 2. The programme spec and this task's
//! brief both say "4096-window"; that is the *original* LZHUF (and the Turbo Pascal version this
//! lineage came through), **not** the Winlink variant. ARSFI's source is explicit about the change
//! — `Const N As Int32 = 2048`, with the comment "Note, was 4096 in the original pascal version" —
//! and LA5NTA's independent Go implementation of the same wire format also uses `_N = 2048`. N is
//! a wire parameter (it sets how far a match may reach back and therefore how the position field
//! is read), so 4096 here would produce an image no Winlink peer could decode. Fixed at 2048, and
//! [`pinned_window_is_2048_not_4096`](tests::pinned_window_is_2048_not_4096) guards it.
//!
//! # What a round-trip through this module does and does not prove
//!
//! It proves the port is **internally invertible**. It does not prove a CMS can read the output:
//! a codec that emitted only literals, or that used a 4096-byte window, round-trips against itself
//! perfectly and is still unreadable on the air. The tests below therefore include checks that
//! cannot pass on self-consistency alone — an externally published CRC check value, the wire
//! header laid out byte by byte, a compression-ratio floor that a literals-only coder fails, and
//! the parameter pins. The real oracle is still the first live CMS connect.

/// Whether the 2-byte CRC16 prefix covers the **compressed** image (`true`) or the uncompressed
/// source (`false`).
///
/// This is spec §7's "undecidable from documents" wire fact: Winlink's own paper and the open
/// reference word the coverage differently, and **a self-round-trip cannot settle it** — the port
/// round-trips perfectly under either choice. It stays behind this one constant so that the
/// shipping code has exactly one place to change.
///
/// # Flipping it also means editing tests in two files and re-deriving a fixture
///
/// An earlier wording here promised the flip was "a one-line change with no other edit anywhere".
/// The wording that replaced it promised the damage stopped at five tests in this file. Both were
/// false, and it is worth saying plainly, because the moment it matters is a live bench where a
/// red suite reads as "the codec broke". **Re-measure before trusting any count below** — the b2f
/// half grows whenever a session test is added. With the constant set to `false`,
/// `cargo test -p tempo-core --no-fail-fast` last measured twelve failures across two targets.
///
/// **In this file: five, each needing a different repair.**
///
/// * `pinned_image_for_a_fixed_plaintext` and
///   `a_body_that_rebuilds_the_tree_round_trips_and_pins_its_image` fail on their pinned images —
///   both pin the 2-byte prefix as part of the image, and the prefix is exactly what the flip
///   changes. Both pins must be re-derived from the flipped build.
/// * `a_corrupted_body_is_reported_as_crc_and_the_intact_one_is_not`,
///   `a_cut_image_with_a_repaired_crc_is_reported_as_truncated` and
///   `a_length_header_that_lands_inside_a_match_is_reported_as_malformed` fail because the
///   `reseal` test helper hard-codes `crc16(body)` to seal a *tampered* body so that the guard
///   after the CRC is the one under test. Under `false` the checked CRC is over the plaintext, and
///   a tampered body has no plaintext to seal against — the helper cannot simply route through
///   [`crc_of`], it has to be rethought along with the three tests, or those three have to be
///   confined to the compressed-CRC reading.
///
/// **In `tests/winlink_b2f.rs`: every session replay whose body reaches [`decompress`]** — seven
/// when last measured. Six report `Lzhuf(Crc)` where the session should have succeeded or failed
/// later for its own reason; the seventh asserts on *where* the session stopped consuming, and a
/// session that fails at the CRC stops before the transcript's last byte. The replays that do not
/// go red are the ones failing earlier — a bad `EOT` checksum, or a compressed length that
/// disagrees with its proposal — because [`decompress`] is never reached.
///
/// Their single cause lives in a third file that is not Rust at all:
/// `tests/fixtures/winlink/session1.trace` pins a compressed image sealed under the *current*
/// setting, so under `false` `decompress` checks crc16(plaintext) against that prefix and rejects
/// the body. Repairing them means re-deriving the fixture twice over — the image's 2-byte CRC
/// prefix, and then the record's `EOT` checksum, which covers the STX data bytes that prefix
/// lives in.
///
/// The shipping code needs no edit: `compress`/`decompress` both branch on the constant already,
/// and `wire_header_is_crc_le_then_length_le_then_stream` is written as a branch so it holds
/// either way.
///
/// **It is no longer a guess.** Two independent primary sources and three captured images agree
/// that the CRC covers the compressed image:
///
/// 1. ARSFI's own source computes the CRC in `putc` while *encoding* (over the bytes it writes,
///    i.e. the compressed image) and in `getc` while *decoding* (over the bytes it reads, i.e. the
///    same compressed image) — the `EncDec` flag selects which side is instrumented, and both
///    sides land on the compressed form.
/// 2. LA5NTA's independent Go implementation of the same format documents and computes
///    "a CRC16 checksum of the compressed data prepended (B2F option)".
/// 3. Checked against three real `.lzh` images published as test data with their plaintexts,
///    including a captured Winlink B2F message: for every one, CRC-16/XMODEM over bytes 2.. of the
///    image reproduces the stored prefix exactly, and CRC over the plaintext does not.
///
/// ⚠️ **NEEDS-BENCH — Batch 5.** What none of that establishes is that a live CMS accepts what
/// *this* port produces. The first live CMS connect is the oracle; if a server rejects a
/// compressed proposal, this constant is the first thing to try flipping, and the Batch-5 capture
/// is what settles it. Do not infer anything from a passing round-trip here.
pub const LZHUF_CRC_OVER_COMPRESSED: bool = true;

/// Why an LZHUF image could not be decompressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LzhufError {
    /// The CRC16 prefix does not match the image. The body is corrupt and none of it may be
    /// delivered — see [`LZHUF_CRC_OVER_COMPRESSED`] for what the CRC covers.
    Crc,
    /// The image is shorter than its own header, or the decoder had to consume bits past the end
    /// of the image to satisfy the declared length. Distinct from [`Malformed`](LzhufError::Malformed)
    /// because it says the image was *cut*, not that it disagrees with itself.
    Truncated,
    /// The image is internally inconsistent: a copy ran past the declared uncompressed length.
    ///
    /// A well-formed image cannot do this — the encoder clamps every match to the bytes actually
    /// remaining (`if match_length > len { match_length = len }`), so the copies sum to exactly the
    /// length in the header. An overrun therefore means the stream and its header disagree, and
    /// returning the extra bytes would hand a caller a message a byte longer than the sender sent.
    Malformed,
}

// ---------------------------------------------------------------------------------------------
// Parameters. ARSFI `WinlinkSupport.vb`, the `Const` block.
// ---------------------------------------------------------------------------------------------

/// Ring-buffer (sliding-dictionary) size. **2048 in the Winlink variant** — see the module header.
const N: usize = 2048;
/// Lookahead-buffer size: the longest match that can be encoded.
const F: usize = 60;
/// Matches this long or shorter are cheaper as literals.
const THRESHOLD: usize = 2;
/// "No such node" for the binary search trees. Deliberately equal to `N`, so every tree array has
/// one scratch slot at index `N` that a NIL child can be written through harmlessly.
const NIL: usize = N;
/// Alphabet size: 256 literals plus the match-length codes `F - THRESHOLD` of them.
const N_CHAR: usize = 256 - THRESHOLD + F;
/// Size of the Huffman node table.
const T: usize = N_CHAR * 2 - 1;
/// Index of the Huffman root.
const ROOT: usize = T - 1;
/// The root frequency at which the tree is rebuilt with halved counts.
const MAX_FREQ: u32 = 0x8000;
/// The fixed part of the image: 2 bytes of CRC16 plus the 4-byte uncompressed length.
const HEADER_LEN: usize = 6;

/// Code lengths for the upper 6 bits of a match position. ARSFI `p_len`.
#[rustfmt::skip]
const P_LEN: [u8; 64] = [
    0x03, 0x04, 0x04, 0x04, 0x05, 0x05, 0x05, 0x05,
    0x05, 0x05, 0x05, 0x05, 0x06, 0x06, 0x06, 0x06,
    0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06,
    0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
    0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
    0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
    0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08,
    0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08,
];

/// Code values for the upper 6 bits of a match position, left-justified in a byte. ARSFI `p_code`.
#[rustfmt::skip]
const P_CODE: [u8; 64] = [
    0x00, 0x20, 0x30, 0x40, 0x50, 0x58, 0x60, 0x68,
    0x70, 0x78, 0x80, 0x88, 0x90, 0x94, 0x98, 0x9C,
    0xA0, 0xA4, 0xA8, 0xAC, 0xB0, 0xB4, 0xB8, 0xBC,
    0xC0, 0xC2, 0xC4, 0xC6, 0xC8, 0xCA, 0xCC, 0xCE,
    0xD0, 0xD2, 0xD4, 0xD6, 0xD8, 0xDA, 0xDC, 0xDE,
    0xE0, 0xE2, 0xE4, 0xE6, 0xE8, 0xEA, 0xEC, 0xEE,
    0xF0, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7,
    0xF8, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF,
];

/// ARSFI carries `d_code` and `d_len` as two more hand-written 256-entry tables. They are not
/// independent data: `(P_CODE, P_LEN)` is a complete prefix code over all 256 byte values, and the
/// decode tables are just its inverse — for each entry `j`, every byte in
/// `P_CODE[j] .. P_CODE[j] + 2^(8 - P_LEN[j])` decodes to `j` with length `P_LEN[j]`. Deriving
/// them here rather than transcribing 512 more hex constants removes 512 chances to mistype one;
/// [`derived_decode_tables_match_the_reference`](tests::derived_decode_tables_match_the_reference)
/// pins the result against values read off ARSFI's listing, and
/// [`decode_tables_cover_every_byte_exactly_once`](tests::decode_tables_cover_every_byte_exactly_once)
/// proves the code is complete (if it were not, the derivation would leave holes).
const fn derive_decode_tables() -> ([u8; 256], [u8; 256]) {
    let mut code = [0u8; 256];
    let mut len = [0u8; 256];
    let mut j = 0;
    while j < 64 {
        let start = P_CODE[j] as usize;
        let span = 1usize << (8 - P_LEN[j]);
        let mut b = 0;
        while b < span {
            code[start + b] = j as u8;
            len[start + b] = P_LEN[j];
            b += 1;
        }
        j += 1;
    }
    (code, len)
}

const DECODE_TABLES: ([u8; 256], [u8; 256]) = derive_decode_tables();
/// Inverse of [`P_CODE`]: the position index a leading byte decodes to. ARSFI `d_code`.
const D_CODE: [u8; 256] = DECODE_TABLES.0;
/// Inverse of [`P_LEN`]: the total code length of that leading byte. ARSFI `d_len`.
const D_LEN: [u8; 256] = DECODE_TABLES.1;

// ---------------------------------------------------------------------------------------------
// CRC16. ARSFI `DoCRC` + `CRCTable`.
// ---------------------------------------------------------------------------------------------

/// The CRC-16/XMODEM table (polynomial 0x1021, MSB first), generated rather than transcribed.
/// ARSFI ships the same 256 values as a literal table.
const fn crc16_table() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = (i as u16) << 8;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 0x8000 != 0 {
                (c << 1) ^ 0x1021
            } else {
                c << 1
            };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

const CRC16_TABLE: [u16; 256] = crc16_table();

/// CRC-16/XMODEM: polynomial 0x1021, init 0x0000, no reflection, no final xor.
///
/// This is ARSFI's `DoCRC` recurrence exactly (`crc = (crc << 8) ^ table[(crc >> 8) ^ byte]`) with
/// the same zero seed. It is a published, named CRC, which is what lets
/// [`crc16_matches_the_published_check_value`](tests::crc16_matches_the_published_check_value) pin
/// it against a constant derived outside this codebase instead of against itself.
fn crc16(data: &[u8]) -> u16 {
    data.iter().fold(0u16, |crc, &b| {
        (crc << 8) ^ CRC16_TABLE[usize::from((crc >> 8) as u8 ^ b)]
    })
}

// ---------------------------------------------------------------------------------------------
// The adaptive Huffman tree, shared by both directions. ARSFI `StartHuff` / `reconst` / `update`.
// ---------------------------------------------------------------------------------------------

/// The adaptive-Huffman state. Encoder and decoder run *identical* copies of this: the tree is
/// rebuilt from the symbol stream on both sides and never transmitted, which is why a single
/// divergence in [`update`](Huffman::update) desynchronises everything after it rather than
/// corrupting one byte.
struct Huffman {
    /// Node frequencies, kept sorted ascending. `freq[T]` is a sentinel above every real count.
    freq: [u32; T + 1],
    /// Parent of each node. Entries `T..T + N_CHAR` locate the leaf for each symbol.
    prnt: [usize; T + N_CHAR],
    /// Left child of each internal node (the right child is `son[i] + 1`); `>= T` means a leaf.
    /// One slot longer than ARSFI's `T` so that a corrupt stream reaching `son[T]` reads a benign
    /// zero instead of panicking on a network path.
    son: [usize; T + 1],
}

// A test-only tally of [`Huffman::reconst`] executions on the current thread.
//
// The rebuild fires once per `MAX_FREQ / 2` symbols or so, which is far past any small fixture,
// so a test that has stopped reaching it looks exactly like one that still does. That is how the
// routine came to be executed by no test at all. Counting it lets the fixture below *assert* that
// the rebuild ran rather than infer it from a byte count. Each Rust test runs on its own thread,
// so the tally is per-test and needs no synchronisation.
#[cfg(test)]
thread_local! {
    static RECONST_COUNT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

impl Huffman {
    /// ARSFI `StartHuff`: the initial flat tree, every symbol at frequency 1.
    fn new() -> Self {
        let mut h = Huffman {
            freq: [0; T + 1],
            prnt: [0; T + N_CHAR],
            son: [0; T + 1],
        };
        for i in 0..N_CHAR {
            h.freq[i] = 1;
            h.son[i] = i + T;
            h.prnt[i + T] = i;
        }
        let mut i = 0;
        let mut j = N_CHAR;
        while j <= ROOT {
            h.freq[j] = h.freq[i] + h.freq[i + 1];
            h.son[j] = i;
            h.prnt[i] = j;
            h.prnt[i + 1] = j;
            i += 2;
            j += 1;
        }
        h.freq[T] = 0xFFFF;
        h.prnt[ROOT] = 0;
        h
    }

    /// ARSFI `reconst`: halve every leaf count and rebuild, called when the root count reaches
    /// [`MAX_FREQ`]. This is what keeps the counts bounded, and it must happen at exactly the same
    /// symbol on both sides or the trees diverge.
    fn reconst(&mut self) {
        #[cfg(test)]
        RECONST_COUNT.with(|c| c.set(c.get() + 1));

        // Collect the leaves into the front of the table with halved counts. The source array is
        // sorted ascending and halving is monotone, so the collected prefix is sorted too — which
        // the insertion below relies on.
        let mut j = 0;
        for i in 0..T {
            if self.son[i] >= T {
                self.freq[j] = (self.freq[i] + 1) >> 1;
                self.son[j] = self.son[i];
                j += 1;
            }
        }

        // Rebuild the internal nodes, inserting each new parent at its sorted position.
        let mut i = 0;
        let mut j = N_CHAR;
        while j < T {
            let f = self.freq[i] + self.freq[i + 1];
            self.freq[j] = f;
            let mut k = j - 1;
            // `f >= freq[0]` because `freq` is ascending and `f` is a sum of two of its entries,
            // so this stops at `k == 0` at the very latest. The `k > 0` guard is belt-and-braces:
            // it can only fire if that invariant has already been broken, and stopping beats
            // wrapping a `usize` into an index panic.
            while k > 0 && f < self.freq[k] {
                k -= 1;
            }
            k += 1;
            // ARSFI replaces the reference's two `move()` calls with this shift; same effect.
            let mut n = j;
            while n > k {
                self.freq[n] = self.freq[n - 1];
                self.son[n] = self.son[n - 1];
                n -= 1;
            }
            self.freq[k] = f;
            self.son[k] = i;
            i += 2;
            j += 1;
        }

        // Re-point every parent at its (possibly moved) node.
        for i in 0..T {
            let k = self.son[i];
            self.prnt[k] = i;
            if k < T {
                self.prnt[k + 1] = i;
            }
        }
    }

    /// ARSFI `update`: bump a symbol's count and restore the sibling property by swapping the node
    /// with the last node of equal frequency, walking to the root.
    fn update(&mut self, c: usize) {
        if self.freq[ROOT] == MAX_FREQ {
            self.reconst();
        }
        let mut c = self.prnt[c + T];
        loop {
            self.freq[c] += 1;
            let k = self.freq[c];

            // If the order is disturbed, exchange nodes.
            let mut n = c + 1;
            if k > self.freq[n] {
                // `freq[T]` is the 0xFFFF sentinel, so this walk cannot run off the end.
                while k > self.freq[n + 1] {
                    n += 1;
                }
                self.freq[c] = self.freq[n];
                self.freq[n] = k;

                let i = self.son[c];
                self.prnt[i] = n;
                if i < T {
                    self.prnt[i + 1] = n;
                }
                let j = self.son[n];
                self.son[n] = i;

                self.prnt[j] = c;
                if j < T {
                    self.prnt[j + 1] = c;
                }
                self.son[c] = j;

                c = n;
            }
            c = self.prnt[c];
            if c == 0 {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Encoder. ARSFI `Encode` + `InsertNode` / `DeleteNode` / `EncodeChar` / `EncodePosition`.
// ---------------------------------------------------------------------------------------------

/// The LZSS front end plus the bit packer. `lson`/`rson`/`dad` are ARSFI's binary search trees
/// over the ring buffer — 256 of them, one per leading byte, rooted at `N + 1 + byte`.
struct Encoder {
    huff: Huffman,
    /// The ring buffer, `N` bytes plus the `F - 1` byte wrap-around copy the match loop reads
    /// through (so a match that straddles the wrap compares as one contiguous run).
    text_buf: [u8; N + F],
    /// Left/right children and parent. Sized `N + 257` (not ARSFI's `N + 1` for `lson`) so that
    /// the root slots `N + 1 ..= N + 256` are addressable in both arrays: `dad[p]` can name a root,
    /// and writing through the unreachable `lson[root]` branch is then a harmless store rather
    /// than an out-of-bounds panic.
    lson: [usize; N + 257],
    rson: [usize; N + 257],
    dad: [usize; N + 1],
    /// Best match found by the last [`insert_node`](Encoder::insert_node), as a back-distance − 1.
    match_position: usize,
    match_length: usize,
    /// The 16-bit bit-packing accumulator and how many of its top bits are pending.
    put_buf: u32,
    put_len: u32,
    out: Vec<u8>,
}

impl Encoder {
    fn new(capacity_hint: usize) -> Self {
        let mut e = Encoder {
            huff: Huffman::new(),
            text_buf: [0; N + F],
            lson: [0; N + 257],
            rson: [0; N + 257],
            dad: [0; N + 1],
            match_position: 0,
            match_length: 0,
            put_buf: 0,
            put_len: 0,
            out: Vec::with_capacity(capacity_hint),
        };
        // ARSFI `InitTree`.
        for i in N + 1..=N + 256 {
            e.rson[i] = NIL;
        }
        for i in 0..N {
            e.dad[i] = NIL;
        }
        e
    }

    /// ARSFI `InsertNode`: insert `r` into its tree, recording the longest match found on the way
    /// down. The walk is a plain BST descent keyed on the `F` bytes at each node.
    fn insert_node(&mut self, r: usize) {
        let mut geq = true;
        let mut p = N + 1 + usize::from(self.text_buf[r]);
        self.rson[r] = NIL;
        self.lson[r] = NIL;
        self.match_length = 0;
        loop {
            if geq {
                if self.rson[p] == NIL {
                    self.rson[p] = r;
                    self.dad[r] = p;
                    return;
                }
                p = self.rson[p];
            } else {
                if self.lson[p] == NIL {
                    self.lson[p] = r;
                    self.dad[r] = p;
                    return;
                }
                p = self.lson[p];
            }

            let mut i = 1;
            while i < F && self.text_buf[r + i] == self.text_buf[p + i] {
                i += 1;
            }
            // ARSFI's `geq = (textBuf(r+i) >= textBuf(p+i)) OrElse (i = F)`, with the `i == F` case
            // hoisted so the equal-through-the-whole-lookahead case never reads past the run.
            geq = i == F || self.text_buf[r + i] >= self.text_buf[p + i];

            if i > THRESHOLD {
                if i > self.match_length {
                    self.match_position = back_distance(r, p);
                    self.match_length = i;
                    if self.match_length >= F {
                        break;
                    }
                }
                if i == self.match_length {
                    let c = back_distance(r, p);
                    if c < self.match_position {
                        self.match_position = c;
                    }
                }
            }
        }

        // Splice `r` into `p`'s place and unlink `p`.
        self.dad[r] = self.dad[p];
        self.lson[r] = self.lson[p];
        self.rson[r] = self.rson[p];
        self.dad[self.lson[p]] = r;
        self.dad[self.rson[p]] = r;
        if self.rson[self.dad[p]] == p {
            self.rson[self.dad[p]] = r;
        } else {
            self.lson[self.dad[p]] = r;
        }
        self.dad[p] = NIL;
    }

    /// ARSFI `DeleteNode`.
    fn delete_node(&mut self, p: usize) {
        if self.dad[p] == NIL {
            return; // not in the tree
        }
        let q = if self.rson[p] == NIL {
            self.lson[p]
        } else if self.lson[p] == NIL {
            self.rson[p]
        } else {
            let mut q = self.lson[p];
            if self.rson[q] != NIL {
                while self.rson[q] != NIL {
                    q = self.rson[q];
                }
                self.rson[self.dad[q]] = self.lson[q];
                self.dad[self.lson[q]] = self.dad[q];
                self.lson[q] = self.lson[p];
                self.dad[self.lson[p]] = q;
            }
            self.rson[q] = self.rson[p];
            self.dad[self.rson[p]] = q;
            q
        };
        self.dad[q] = self.dad[p];
        if self.rson[self.dad[p]] == p {
            self.rson[self.dad[p]] = q;
        } else {
            self.lson[self.dad[p]] = q;
        }
        self.dad[p] = NIL;
    }

    /// ARSFI `Putcode`: append the top `n` bits of `c` to the output.
    fn put_code(&mut self, n: u32, c: u32) {
        debug_assert!(n <= 16, "a Huffman code longer than the 16-bit accumulator");
        self.put_buf = (self.put_buf | (c >> self.put_len)) & 0xFFFF;
        self.put_len += n;
        if self.put_len >= 8 {
            self.out.push((self.put_buf >> 8) as u8);
            self.put_len -= 8;
            if self.put_len >= 8 {
                self.out.push((self.put_buf & 0xFF) as u8);
                self.put_len -= 8;
                self.put_buf = (c << (n - self.put_len)) & 0xFFFF;
            } else {
                self.put_buf = (self.put_buf & 0xFF) << 8;
            }
        }
    }

    /// ARSFI `EncodeChar`: walk leaf → root, emitting the path (0 = left son, 1 = right).
    fn encode_char(&mut self, c: usize) {
        let mut code: u32 = 0;
        let mut len: u32 = 0;
        let mut k = self.huff.prnt[c + T];
        loop {
            code >>= 1;
            // An odd node address is the bigger brother, i.e. a 1 bit.
            if k & 1 != 0 {
                code += 0x8000;
            }
            len += 1;
            k = self.huff.prnt[k];
            if k == ROOT {
                break;
            }
        }
        self.put_code(len, code);
        self.huff.update(c);
    }

    /// ARSFI `EncodePosition`: the upper 6 bits through [`P_CODE`], the lower 6 verbatim.
    ///
    /// With `N = 2048` a position is 11 bits, so `c >> 6` never exceeds 31 and the upper half of
    /// [`P_CODE`]/[`P_LEN`] is dead code in the Winlink variant. Both tables are kept whole because
    /// they are the reference's, and a reader diffing the two files should find them identical.
    fn encode_position(&mut self, c: usize) {
        let i = c >> 6;
        self.put_code(u32::from(P_LEN[i]), u32::from(P_CODE[i]) << 8);
        self.put_code(6, ((c & 0x3F) as u32) << 10);
    }

    /// ARSFI `EncodeEnd`: flush the partial byte, zero-padded.
    fn encode_end(&mut self) {
        if self.put_len > 0 {
            self.out.push((self.put_buf >> 8) as u8);
        }
    }
}

/// The back-distance ARSFI writes as `((r - p) And (N - 1)) - 1`.
///
/// `p` is always a node already in the tree and `r` has just been removed from it, so the masked
/// difference is in `1 ..= N - 1` and the result is in `0 ..= N - 2`. The wrapping arithmetic and
/// the second mask make the function total anyway: they are exact for every reachable input and
/// keep an unreachable one from wrapping a `usize` into a panic.
fn back_distance(r: usize, p: usize) -> usize {
    (r.wrapping_sub(p) & (N - 1)).wrapping_sub(1) & (N - 1)
}

/// Compress `plain` into the FBB B2 image described in the module header.
///
/// # Panics
///
/// If `plain` is longer than `u32::MAX`. The image's length header is 4 bytes, so the format
/// cannot express more; a Winlink account's whole quota is measured in tens of kilobytes, so this
/// is a not-a-real-input assertion rather than an error case worth threading through the return
/// type.
pub fn compress(plain: &[u8]) -> Vec<u8> {
    let text_size = u32::try_from(plain.len())
        .expect("LZHUF images carry a 4-byte length; a message this large cannot be sent");

    // ARSFI writes the length header through the same `putc` the bit packer uses, so the CRC it
    // accumulates covers the header as well as the stream. Ours is computed at the end over the
    // same bytes, which is the same thing said once instead of on every byte.
    let mut body = Vec::with_capacity(plain.len() / 2 + HEADER_LEN + 16);
    body.extend_from_slice(&text_size.to_le_bytes());

    if !plain.is_empty() {
        let mut e = Encoder::new(plain.len() / 2 + 16);
        let mut in_ptr = 0usize;

        // Pre-fill the dictionary with spaces and the lookahead with the first F bytes.
        let mut s = 0usize;
        let mut r = N - F;
        for i in 0..r {
            e.text_buf[i] = b' ';
        }
        let mut len = 0usize;
        while len < F && in_ptr < plain.len() {
            e.text_buf[r + len] = plain[in_ptr];
            in_ptr += 1;
            len += 1;
        }
        for i in 1..=F {
            e.insert_node(r - i);
        }
        e.insert_node(r);

        loop {
            if e.match_length > len {
                e.match_length = len;
            }
            if e.match_length <= THRESHOLD {
                e.match_length = 1;
                e.encode_char(usize::from(e.text_buf[r]));
            } else {
                e.encode_char(255 - THRESHOLD + e.match_length);
                e.encode_position(e.match_position);
            }
            let last_match_length = e.match_length;

            // Slide the window forward over the bytes just encoded, pulling in new input.
            let mut i = 0usize;
            while i < last_match_length && in_ptr < plain.len() {
                i += 1;
                e.delete_node(s);
                let c = plain[in_ptr];
                in_ptr += 1;
                e.text_buf[s] = c;
                // Keep the wrap-around copy in sync so a match spanning the wrap compares right.
                if s < F - 1 {
                    e.text_buf[s + N] = c;
                }
                s = (s + 1) & (N - 1);
                r = (r + 1) & (N - 1);
                e.insert_node(r);
            }
            // Input exhausted: drain the lookahead without refilling it.
            while i < last_match_length {
                i += 1;
                e.delete_node(s);
                s = (s + 1) & (N - 1);
                r = (r + 1) & (N - 1);
                len -= 1;
                if len > 0 {
                    e.insert_node(r);
                }
            }

            if len == 0 {
                break;
            }
        }
        e.encode_end();
        body.extend_from_slice(&e.out);
    }

    let mut image = Vec::with_capacity(body.len() + 2);
    image.extend_from_slice(&crc_of(&body, plain).to_le_bytes());
    image.extend_from_slice(&body);
    image
}

/// The one place the CRC's coverage is decided, on both the write and the read path.
/// See [`LZHUF_CRC_OVER_COMPRESSED`].
fn crc_of(compressed_body: &[u8], plain: &[u8]) -> u16 {
    if LZHUF_CRC_OVER_COMPRESSED {
        crc16(compressed_body)
    } else {
        crc16(plain)
    }
}

// ---------------------------------------------------------------------------------------------
// Decoder. ARSFI `DecodeWork` + `GetBit` / `GetByte` / `DecodeChar` / `DecodePosition`.
// ---------------------------------------------------------------------------------------------

/// ARSFI's `GetBit`/`GetByte` over a slice, with one addition: it counts what it has consumed.
///
/// The reference reads zeros past the end of the input, which turns a cut-short image into a
/// plausible-looking stream of fabricated symbols. Comparing bits consumed against bits actually
/// supplied ([`over_read`](BitReader::over_read)) is what makes that detectable, and it doubles as
/// the bound on how much output a forged length header can make this loop produce.
struct BitReader<'a> {
    data: &'a [u8],
    /// How many real bytes have been pulled out of `data`.
    pos: usize,
    buf: u32,
    len: u32,
    /// How many bits have been handed out.
    bits: u64,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader {
            data,
            pos: 0,
            buf: 0,
            len: 0,
            bits: 0,
        }
    }

    /// Top the 16-bit window up to more than 8 bits, zero-filling past the end as ARSFI's `getc`
    /// does.
    fn fill(&mut self) {
        while self.len <= 8 {
            let c = if self.pos < self.data.len() {
                let b = self.data[self.pos];
                self.pos += 1;
                u32::from(b)
            } else {
                0
            };
            self.buf = (self.buf | (c << (8 - self.len))) & 0xFFFF;
            self.len += 8;
        }
    }

    fn get_bit(&mut self) -> usize {
        self.fill();
        let v = (self.buf >> 15) & 1;
        self.buf = (self.buf << 1) & 0xFFFF;
        self.len -= 1;
        self.bits += 1;
        v as usize
    }

    fn get_byte(&mut self) -> usize {
        self.fill();
        let v = (self.buf >> 8) & 0xFF;
        self.buf = (self.buf << 8) & 0xFFFF;
        self.len -= 8;
        self.bits += 8;
        v as usize
    }

    /// True once a bit has been handed out that the image did not supply.
    ///
    /// For a well-formed image this is always false: the decoder consumes exactly the bits the
    /// encoder emitted, and `8 * pos` counts every bit of every byte it pulled — the look-ahead
    /// can be *ahead*, but it can never be behind.
    fn over_read(&self) -> bool {
        self.bits > 8 * self.pos as u64
    }
}

/// The decoder's half of the shared state: the same Huffman tree, and the same ring buffer.
struct Decoder {
    huff: Huffman,
    text_buf: [u8; N],
}

impl Decoder {
    fn new() -> Self {
        let mut d = Decoder {
            huff: Huffman::new(),
            text_buf: [0; N],
        };
        // The encoder pre-filled `0 .. N - F` with spaces, so the decoder must too: a match near
        // the start of a message can reach back into that region, and it must find the same bytes.
        for i in 0..N - F {
            d.text_buf[i] = b' ';
        }
        d
    }

    /// ARSFI `DecodeChar`: walk root → leaf, one bit per level.
    fn decode_char(&mut self, br: &mut BitReader<'_>) -> usize {
        let mut c = self.huff.son[ROOT];
        while c < T {
            c = self.huff.son[c + br.get_bit()];
        }
        c -= T;
        self.huff.update(c);
        c
    }

    /// ARSFI `DecodePosition`: the upper 6 bits from [`D_CODE`], the rest verbatim.
    fn decode_position(&mut self, br: &mut BitReader<'_>) -> usize {
        let mut i = br.get_byte();
        let c = usize::from(D_CODE[i]) << 6;
        let mut j = u32::from(D_LEN[i]) - 2;
        while j > 0 {
            j -= 1;
            i = ((i << 1) | br.get_bit()) & 0xFFFF;
        }
        c | (i & 0x3F)
    }
}

/// Decompress an FBB B2 image produced by [`compress`] or by a Winlink peer.
///
/// Returns [`LzhufError::Crc`] before doing any work when the prefix does not match — the CRC
/// covers the compressed image, so it can be checked up front, and checking it first means the
/// structural errors below only ever describe an image that is intact but self-contradictory.
pub fn decompress(image: &[u8]) -> Result<Vec<u8>, LzhufError> {
    // ARSFI's `Encode` returns a zero-length buffer for empty input — it takes an early exit that
    // drops the 4-byte length header it had already written, and the CRC prefix with it. This port
    // does not reproduce that (see `compress`: an empty message still gets a well-formed image),
    // but a peer running the reference can put one on the wire, and an empty image can only ever
    // have meant an empty message.
    if image.is_empty() {
        return Ok(Vec::new());
    }
    if image.len() < HEADER_LEN {
        return Err(LzhufError::Truncated);
    }

    let supplied = u16::from_le_bytes([image[0], image[1]]);
    let body = &image[2..];
    if LZHUF_CRC_OVER_COMPRESSED && crc16(body) != supplied {
        return Err(LzhufError::Crc);
    }

    let text_size = u32::from_le_bytes([body[0], body[1], body[2], body[3]]) as usize;
    let stream = &body[4..];
    let plain = decode_stream(stream, text_size)?;

    if !LZHUF_CRC_OVER_COMPRESSED && crc16(&plain) != supplied {
        return Err(LzhufError::Crc);
    }
    Ok(plain)
}

fn decode_stream(stream: &[u8], text_size: usize) -> Result<Vec<u8>, LzhufError> {
    if text_size == 0 {
        return Ok(Vec::new());
    }
    // Trust the length header only as far as the stream can back it up. Every output byte costs at
    // least one bit and a single symbol yields at most `F` of them, so this bounds what a forged
    // header can make us allocate, without capping any real message.
    let capacity = text_size.min(stream.len().saturating_mul(F).saturating_add(F));

    let mut d = Decoder::new();
    let mut br = BitReader::new(stream);
    let mut out: Vec<u8> = Vec::with_capacity(capacity);
    let mut r = N - F;

    while out.len() < text_size {
        if br.over_read() {
            return Err(LzhufError::Truncated);
        }
        let c = d.decode_char(&mut br);
        if c < 256 {
            out.push(c as u8);
            d.text_buf[r] = c as u8;
            r = (r + 1) & (N - 1);
        } else {
            // A match: `c - 255 + THRESHOLD` bytes, copied from `pos + 1` bytes back. The masking
            // mirrors ARSFI's signed `(r - pos - 1) And (N - 1)`; a corrupt stream can decode a
            // position of up to 4095, which the mask folds back into the ring rather than panicking.
            let pos = d.decode_position(&mut br);
            let from = r.wrapping_sub(pos).wrapping_sub(1) & (N - 1);
            let run = c - 255 + THRESHOLD;
            for k in 0..run {
                let b = d.text_buf[(from + k) & (N - 1)];
                out.push(b);
                d.text_buf[r] = b;
                r = (r + 1) & (N - 1);
            }
        }
    }

    if br.over_read() {
        return Err(LzhufError::Truncated);
    }
    if out.len() != text_size {
        return Err(LzhufError::Malformed);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use md5::{Digest, Md5};
    use proptest::prelude::*;

    // -----------------------------------------------------------------------------------------
    // The round-trip tests. These prove the port is invertible **against itself** and nothing
    // more — see the module header. Everything after them is the part that can fail while these
    // pass.
    // -----------------------------------------------------------------------------------------

    #[test]
    fn roundtrip_empty() {
        assert_eq!(decompress(&compress(b"")).unwrap(), b"");
    }

    #[test]
    fn roundtrip_one_byte() {
        assert_eq!(decompress(&compress(b"A")).unwrap(), b"A");
    }

    #[test]
    fn roundtrip_64k_repeat() {
        let v = vec![b'Z'; 64 * 1024];
        assert_eq!(decompress(&compress(&v)).unwrap(), v);
    }

    proptest! {
        #[test]
        fn roundtrip_incompressible(data in proptest::collection::vec(any::<u8>(), 0..8192)) {
            prop_assert_eq!(decompress(&compress(&data)).unwrap(), data);
        }
    }

    /// A message long enough to wrap the 2048-byte ring several times, with a period that is not a
    /// factor of `N`, so matches are forced to reach across the wrap and through the `F - 1` byte
    /// mirror at the end of `text_buf`. A window bookkeeping bug that the short cases miss shows up
    /// here as a garbled tail.
    #[test]
    fn roundtrip_across_the_ring_wrap() {
        let unit = b"The quick brown fox jumps over the lazy dog 0123456789. ";
        let mut v = Vec::new();
        while v.len() < 6 * N {
            v.extend_from_slice(unit);
        }
        assert_eq!(decompress(&compress(&v)).unwrap(), v);
    }

    /// Byte-for-byte round-trip over every length from 0 to a little past the `F`-byte lookahead,
    /// which is where the encoder's two drain loops and the "match longer than what is left"
    /// clamp all interact.
    #[test]
    fn roundtrip_every_length_around_the_lookahead() {
        let src: Vec<u8> = (0..200u32).map(|i| (i % 7) as u8 + b'a').collect();
        for n in 0..=(2 * F + 5) {
            let slice = &src[..n];
            assert_eq!(
                decompress(&compress(slice)).unwrap(),
                slice,
                "round-trip failed at length {n}"
            );
        }
    }

    // -----------------------------------------------------------------------------------------
    // Checks a self-consistent-but-wrong codec fails.
    // -----------------------------------------------------------------------------------------

    /// Pins [`crc16`] against the published check value for CRC-16/XMODEM — the value every
    /// implementation of that named CRC produces for `"123456789"`, derived entirely outside this
    /// codebase. Without this, the CRC is only ever compared with itself.
    #[test]
    fn crc16_matches_the_published_check_value() {
        assert_eq!(crc16(b"123456789"), 0x31C3);
        // And it is genuinely seeded at zero, not at 0xFFFF (which would be CCITT-FALSE and give
        // 0x29B1 for the same input).
        assert_eq!(crc16(&[]), 0x0000);
    }

    /// The window is the Winlink 2048, not the original LZHUF 4096. Both the programme spec and
    /// this task's brief say 4096; ARSFI's source and LA5NTA's independent implementation both say
    /// 2048, and it is a wire parameter — an image built with the wrong one decodes to garbage on
    /// a real peer while round-tripping perfectly here.
    #[test]
    fn pinned_window_is_2048_not_4096() {
        assert_eq!(N, 2048, "the Winlink LZHUF ring buffer is 2048, not 4096");
        assert_eq!(F, 60);
        assert_eq!(THRESHOLD, 2);
        // The derived alphabet, spelled out so a change to any of the three above is visible.
        assert_eq!(N_CHAR, 314);
        assert_eq!(T, 627);
        assert_eq!(ROOT, 626);
    }

    /// The image layout, byte by byte: CRC16 little-endian, then the uncompressed length as a
    /// little-endian `u32`, then the stream. A codec that agreed with itself but put the length
    /// first, or the CRC big-endian, passes every round-trip test above and no peer can read it.
    #[test]
    fn wire_header_is_crc_le_then_length_le_then_stream() {
        let plain = b"Nexus Winlink B2F compressed body";
        let image = compress(plain);

        assert!(image.len() > HEADER_LEN);
        assert_eq!(
            &image[2..6],
            &(plain.len() as u32).to_le_bytes(),
            "bytes 2..6 must be the uncompressed length, little-endian"
        );
        let prefix = u16::from_le_bytes([image[0], image[1]]);
        // The seam, read both ways round: whichever way LZHUF_CRC_OVER_COMPRESSED is set, the
        // prefix must be the CRC of that input and must NOT be the CRC of the other one. Written
        // as a branch rather than a pin so that *this* test survives the flip untouched. Others
        // do not, here and in tests/winlink_b2f.rs — LZHUF_CRC_OVER_COMPRESSED's own doc says
        // which and why.
        if LZHUF_CRC_OVER_COMPRESSED {
            assert_eq!(
                prefix,
                crc16(&image[2..]),
                "bytes 0..2 must be the CRC16 of everything after them, little-endian"
            );
            assert_ne!(prefix, crc16(plain), "the CRC is over the compressed image");
        } else {
            assert_eq!(
                prefix,
                crc16(plain),
                "bytes 0..2 must be the CRC16 of the plaintext"
            );
            assert_ne!(prefix, crc16(&image[2..]));
        }
    }

    /// The LZSS front end is actually engaged. A codec that emitted every byte as a literal
    /// round-trips perfectly and is useless: 64 KB of one byte must not cost anything like 64 KB.
    /// The threshold is deliberately loose — this is a "did the matcher run at all" control, not a
    /// ratio benchmark: 64 KB of one byte is ~1.4 KB here, which is within a few percent of what
    /// this codec can do at all (1092 maximum-length matches at roughly ten bits each), while a
    /// literals-only coder would land near 40 KB.
    #[test]
    fn a_long_run_compresses_by_orders_of_magnitude() {
        let v = vec![b'Z'; 64 * 1024];
        let image = compress(&v);
        assert!(
            image.len() < v.len() / 40,
            "64 KB of one byte compressed to {} bytes; the match finder is not running",
            image.len()
        );
    }

    /// Ten thousand bytes of English text must at least halve. Same control as above against a
    /// realistic body rather than a degenerate run.
    #[test]
    fn prose_compresses_to_less_than_half() {
        let unit = b"Field Day is the single most popular on-the-air event held annually in the US and Canada. ";
        let mut v = Vec::new();
        while v.len() < 10_000 {
            v.extend_from_slice(unit);
        }
        let image = compress(&v);
        assert!(
            image.len() < v.len() / 2,
            "compressed to {} bytes",
            image.len()
        );
    }

    /// The derived decode tables against values read off ARSFI's `d_code`/`d_len` listings. These
    /// are the boundaries of every code-length band, which is where a derivation would go wrong.
    #[test]
    fn derived_decode_tables_match_the_reference() {
        for (byte, code, len) in [
            (0usize, 0x00u8, 3u8),
            (31, 0x00, 3),
            (32, 0x01, 4),
            (47, 0x01, 4),
            (48, 0x02, 4),
            (79, 0x03, 4),
            (80, 0x04, 5),
            (143, 0x0B, 5),
            (144, 0x0C, 6),
            (147, 0x0C, 6),
            (148, 0x0D, 6),
            (191, 0x17, 6),
            (192, 0x18, 7),
            (193, 0x18, 7),
            (194, 0x19, 7),
            (239, 0x2F, 7),
            (240, 0x30, 8),
            (255, 0x3F, 8),
        ] {
            assert_eq!(D_CODE[byte], code, "D_CODE[{byte}]");
            assert_eq!(D_LEN[byte], len, "D_LEN[{byte}]");
        }
    }

    /// `(P_CODE, P_LEN)` must be a *complete* prefix code over all 256 byte values — every byte
    /// claimed by exactly one entry. If it were not, `derive_decode_tables` would silently leave
    /// zeroed holes that decode to position 0 with length 3.
    #[test]
    fn decode_tables_cover_every_byte_exactly_once() {
        let mut claimed = [false; 256];
        let mut total = 0usize;
        for j in 0..64 {
            let span = 1usize << (8 - P_LEN[j]);
            total += span;
            for b in 0..span {
                let byte = P_CODE[j] as usize + b;
                assert!(!claimed[byte], "byte {byte} claimed twice");
                claimed[byte] = true;
                assert_eq!(D_CODE[byte] as usize, j);
                assert_eq!(D_LEN[byte], P_LEN[j]);
            }
        }
        assert_eq!(total, 256, "the position code does not tile the byte range");
        assert!(claimed.iter().all(|&c| c));
    }

    // -----------------------------------------------------------------------------------------
    // Negative controls: each error must fire, and must not fire on the intact image.
    // -----------------------------------------------------------------------------------------

    /// Recompute the CRC prefix over a body that has been tampered with, so the *next* guard is
    /// the one under test rather than the CRC.
    fn reseal(body: &[u8]) -> Vec<u8> {
        let mut image = Vec::with_capacity(body.len() + 2);
        image.extend_from_slice(&crc16(body).to_le_bytes());
        image.extend_from_slice(body);
        image
    }

    #[test]
    fn a_corrupted_body_is_reported_as_crc_and_the_intact_one_is_not() {
        let plain = b"Winlink B2F body, long enough to span several code words.".repeat(8);
        let image = compress(&plain);
        assert_eq!(decompress(&image).unwrap(), plain, "positive control");

        // Every single-byte change anywhere after the CRC prefix must be caught.
        for i in 2..image.len() {
            let mut bad = image.clone();
            bad[i] ^= 0x01;
            assert_eq!(
                decompress(&bad),
                Err(LzhufError::Crc),
                "a flipped bit at offset {i} was not caught by the CRC"
            );
        }
        // And a change to the prefix itself.
        let mut bad = image.clone();
        bad[0] ^= 0x80;
        assert_eq!(decompress(&bad), Err(LzhufError::Crc));
    }

    #[test]
    fn a_cut_image_with_a_repaired_crc_is_reported_as_truncated() {
        let plain = b"Winlink B2F body, long enough to span several code words.".repeat(8);
        let image = compress(&plain);
        let body = &image[2..];

        // Keep the length header, throw away most of the stream, and repair the CRC so the image
        // is intact-but-short rather than merely corrupt.
        let cut = reseal(&body[..4 + (body.len() - 4) / 4]);
        assert_eq!(decompress(&cut), Err(LzhufError::Truncated));

        // Shorter than its own 6-byte header.
        assert_eq!(decompress(&[0u8; 5]), Err(LzhufError::Truncated));
        // Positive control: the uncut image with the same reseal is still fine.
        assert_eq!(decompress(&reseal(body)).unwrap(), plain);
    }

    #[test]
    fn a_length_header_that_lands_inside_a_match_is_reported_as_malformed() {
        let plain = b"AAAAAAAAAABBBBBBBBBBAAAAAAAAAABBBBBBBBBB".repeat(4);
        let image = compress(&plain);
        let body = &image[2..];

        // Walk every shorter declared length. Each must either decode to exactly that many bytes
        // or be refused as Malformed — never silently return a different count.
        let mut malformed_seen = 0;
        for declared in 1..plain.len() as u32 {
            let mut forged = body.to_vec();
            forged[0..4].copy_from_slice(&declared.to_le_bytes());
            match decompress(&reseal(&forged)) {
                Ok(out) => assert_eq!(
                    out.len(),
                    declared as usize,
                    "declared {declared} but returned {} bytes",
                    out.len()
                ),
                Err(LzhufError::Malformed) => malformed_seen += 1,
                Err(other) => panic!("declared {declared}: unexpected {other:?}"),
            }
        }
        assert!(
            malformed_seen > 0,
            "the overrun guard never fired; it is not being exercised"
        );

        // Positive control: the true length still decodes.
        assert_eq!(decompress(&image).unwrap(), plain);
    }

    /// A declared length larger than the stream can supply must be refused, not padded out with
    /// symbols decoded from zeros.
    #[test]
    fn a_length_header_larger_than_the_stream_is_reported_as_truncated() {
        let plain = b"a short body";
        let image = compress(plain);
        let mut forged = image[2..].to_vec();
        forged[0..4].copy_from_slice(&100_000u32.to_le_bytes());
        assert_eq!(decompress(&reseal(&forged)), Err(LzhufError::Truncated));
    }

    /// ARSFI's encoder emits a zero-length buffer for empty input (it takes an early exit that
    /// drops the header it had just written). This port emits a well-formed image instead, but
    /// must still read the reference's form.
    #[test]
    fn the_reference_empty_image_reads_as_an_empty_message() {
        assert_eq!(decompress(&[]).unwrap(), Vec::<u8>::new());
        // This port's own form of the same thing.
        let image = compress(b"");
        assert_eq!(image.len(), HEADER_LEN);
        assert_eq!(&image[2..], &0u32.to_le_bytes());
        assert_eq!(decompress(&image).unwrap(), Vec::<u8>::new());
    }

    // Decompression of arbitrary bytes must never panic — the decoder sits on a network path and
    // a peer can send anything. It may return any error, or (vanishingly rarely) succeed.
    proptest! {
        #[test]
        fn arbitrary_bytes_never_panic(data in proptest::collection::vec(any::<u8>(), 0..512)) {
            let _ = decompress(&data);
            // Same again with a valid CRC, so the structural guards are the ones under test.
            let mut sealed = Vec::with_capacity(data.len() + 2);
            sealed.extend_from_slice(&crc16(&data).to_le_bytes());
            sealed.extend_from_slice(&data);
            let _ = decompress(&sealed);
        }
    }

    /// A pinned image for a fixed plaintext, so that any future change to the coder — a table
    /// edit, a reordered `update`, a different `N` — shows up as a diff here rather than as a
    /// message no CMS can read.
    ///
    /// **This vector is not self-consistency.** It was emitted by this port *after* the port was
    /// verified bit-exact, in both directions, against three externally produced `.lzh` images
    /// published with their plaintexts by LA5NTA's independent Go implementation
    /// (<https://github.com/la5nta/wl2k-go>, `lzhuf/testdata`) — 1.5 KB of prose, 100 KB of
    /// decimal digits, and a 31 KB captured Winlink B2F message. Every byte of all three images,
    /// CRC prefix included, matched. Those fixtures are deliberately **not** vendored here: doing
    /// so would add a third-party test-data item to `NOTICE`, which is operator-gated. Re-running
    /// that cross-check needs nothing but the four files and a dozen lines of test.
    #[test]
    fn pinned_image_for_a_fixed_plaintext() {
        let plain: &[u8] =
            b"Nexus LZHUF golden vector.\r\nSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS\r\n0123456789 0123456789\r\n";
        #[rustfmt::skip]
        const IMAGE: [u8; 58] = [
            0x4D, 0x60, 0x53, 0x00, 0x00, 0x00, 0xED, 0x7C, 0x41, 0x00, 0x7F, 0xFA,
            0xCE, 0xC7, 0x9B, 0xA9, 0xE1, 0xE9, 0x60, 0x7C, 0xFF, 0x7F, 0x8F, 0x86,
            0x27, 0xEA, 0xD0, 0x15, 0x9F, 0x78, 0x05, 0xCF, 0xFB, 0x75, 0x99, 0xCB,
            0x77, 0xE9, 0x80, 0x15, 0x95, 0x7B, 0xCD, 0xEE, 0xFB, 0x7F, 0xC0, 0xE0,
            0xF0, 0xB8, 0x7C, 0x4E, 0x2D, 0xA9, 0x30, 0x54, 0xC9, 0x80,
        ];
        assert_eq!(compress(plain), IMAGE, "the compressed image moved");
        assert_eq!(decompress(&IMAGE).unwrap(), plain);
    }

    // -----------------------------------------------------------------------------------------
    // The adaptive-Huffman tree rebuild.
    //
    // Everything above this line runs entirely on the *initial* tree. [`Huffman::reconst`] fires
    // only when the root count reaches `MAX_FREQ`, i.e. after `MAX_FREQ - N_CHAR` = 32454 encoded
    // symbols, and nothing above comes within an order of magnitude of that: 64 KB of one byte is
    // ~1100 symbols, the ring-wrap body ~1200, the golden vector ~90, and the round-trip proptest
    // caps at 8192 *bytes*. The rebuild is the one routine here that both peers must run at the
    // identical symbol, so it is also the one whose divergence corrupts every byte after it rather
    // than one — and it was the one routine no test executed.
    // -----------------------------------------------------------------------------------------

    /// A deterministic body long enough to rebuild the Huffman tree three times.
    ///
    /// Two properties are being bought, and both drive the shape:
    ///
    /// * **Low compressibility keeps it small.** A byte the matcher cannot cover costs one symbol,
    ///   so symbols track bytes about 1:1 and the second rebuild arrives at ~52000 bytes. 80000
    ///   gives it 50% headroom and buys a third rebuild; the test asserts the count rather than
    ///   trusting this arithmetic.
    /// * **A 32-symbol alphabet makes short matches common**, so `insert_node` repeatedly finds
    ///   two candidates of *equal* length and its closest-match tie-break decides between them.
    ///   Over a 95-symbol alphabet that branch is effectively never taken, and disabling it does
    ///   not move the image at any size — an alphabet this narrow is what turns the tie-break into
    ///   something the pin below can see. It is also realistic: RFC 4648 base32 is the shape of an
    ///   encoded attachment body.
    ///
    /// Generated rather than vendored: a third-party fixture this size would be a NOTICE item, and
    /// this one is reproducible by anyone from these five lines.
    fn tree_rebuild_body() -> Vec<u8> {
        const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let mut x: u32 = 0x1234_5678;
        (0..80_000)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                // The top byte, not the low bits: an LCG's low bits have short periods.
                ALPHABET[((x >> 24) & 31) as usize]
            })
            .collect()
    }

    /// Round-trips a body that crosses [`Huffman::reconst`], and pins the image it produces.
    ///
    /// Both halves are load-bearing and neither substitutes for the other:
    ///
    /// * The **round trip** proves the two trees stay in step across a rebuild — a rebuild that
    ///   ran on one side and not the other garbles everything after it. The `RECONST_COUNT`
    ///   assertions state outright that the rebuild ran, so a later edit that stops reaching it
    ///   fails here instead of silently restoring the coverage gap this test exists to close.
    /// * The **pinned image** is what catches a rebuild that is symmetric but wrong. Encoder and
    ///   decoder share one [`Huffman`], so a change to [`MAX_FREQ`] or to `reconst` itself changes
    ///   both sides identically: it round-trips perfectly while putting bytes on the air that no
    ///   Winlink peer can read. That is not hypothetical — `MAX_FREQ` at `0x7FFF` (the rebuild one
    ///   symbol early) or at `0x4000`, and `insert_node`'s tie-break disabled, all leave every
    ///   round trip in this file green and all three move this pin.
    ///
    /// The pin is a digest because the image is 51 KB. MD5 is a fixture identity here and nothing
    /// else — never integrity, secrecy, or any security decision (`secure.rs` carries the same
    /// caveat for the one place Winlink's protocol forces MD5 on us).
    #[test]
    fn a_body_that_rebuilds_the_tree_round_trips_and_pins_its_image() {
        let plain = tree_rebuild_body();

        RECONST_COUNT.with(|c| c.set(0));
        let image = compress(&plain);
        let rebuilds_encoding = RECONST_COUNT.with(|c| c.get());

        RECONST_COUNT.with(|c| c.set(0));
        let back = decompress(&image).unwrap();
        let rebuilds_decoding = RECONST_COUNT.with(|c| c.get());

        // Reported as a first-difference offset rather than through `assert_eq!`, which would dump
        // 80 KB of bytes twice on failure and bury the one number that locates the divergence.
        if back != plain {
            let at = back.iter().zip(&plain).position(|(a, b)| a != b);
            panic!(
                "the round trip across the tree rebuild is not exact: {} bytes out for {} in, \
                 first difference at {at:?}",
                back.len(),
                plain.len()
            );
        }
        assert!(
            rebuilds_encoding >= 2,
            "the fixture reached {rebuilds_encoding} tree rebuilds, not at least 2; `reconst` is \
             back to being executed by nothing"
        );
        assert_eq!(
            rebuilds_decoding, rebuilds_encoding,
            "the decoder rebuilt its tree a different number of times than the encoder did"
        );

        let mut md5 = Md5::new();
        md5.update(&image);
        let digest: String = md5.finalize().iter().map(|b| format!("{b:02x}")).collect();
        // The length is the cheap half and it is not sufficient: a 100 KB variant of this fixture
        // survives the tie-break mutation with its length unchanged and only the digest moving.
        assert_eq!(image.len(), 51884, "the compressed image moved (length)");
        assert_eq!(
            digest, "373b8cd9516ff4937c71fb34508fb749",
            "the compressed image moved (contents)"
        );
    }
}
