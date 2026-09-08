//! The Winlink secure-login challenge-response (`;PQ:` → `;PR:`).
//!
//! The CMS answers the SID exchange with a `;PQ:` challenge; the client replies `;PR:` with an
//! eight-digit token derived from that challenge, the account password, and a fixed 64-byte
//! protocol salt. Getting it wrong is a refused login, nothing subtler.
//!
//! **What the MD5 here is for, stated plainly so nobody "upgrades" it.** It *authenticates a
//! Winlink account to the CMS*, and that is all. It is **not** an integrity check, **not** a
//! secrecy primitive, and nothing anywhere in Nexus may treat it as either. MD5 being broken as a
//! hash is true and irrelevant: the algorithm is the wire format the far end speaks, so swapping
//! in a modern digest would not make an operator safer — it would make every operator unable to
//! log in. For the same reason [`WINLINK_SALT`] is a **public protocol constant, not a key**: it
//! is published in every open Winlink client, it is in this file in the clear on purpose, and
//! finding it here is not a leak. The one real secret in the exchange is the password, which is
//! why it moves through here as bytes and is never logged, formatted, or copied to the heap.
//!
//! ⚠️ **The exact derivation is undecidable from documents.** There is no published specification
//! for `;PR:`. What is implemented is the wl2k-go reading (LA5NTA, MIT — see `NOTICE`), which
//! reached wl2k-go from paclink-unix. Every choice it makes — the salt bytes, the order the three
//! inputs are hashed in, which four digest bytes are used, the six-bit mask, the byte order, the
//! decimal formatting and the eight-character truncation — is confined to [`pr_with_salt`] and
//! [`reduce_digest_wl2kgo`], so **the first live CMS connect settles it by editing two small
//! functions and nothing else.** Until that connect happens, a green test in this module proves
//! self-consistency and says nothing at all about on-air correctness.
//!
//! Bytes, not `String`, throughout — the module-level rule in [`super`], and it is load-bearing
//! twice over here: the challenge is whatever the CMS put on the wire, and the password is an
//! operator secret. Neither is guaranteed UTF-8, and a lossy decode on the way into a digest is a
//! login failure with no visible cause.

use md5::{Digest, Md5};

/// The fixed 64-byte Winlink secure-login salt, transcribed verbatim from wl2k-go's
/// `fbb/secure.go` (`winlinkSecureSalt`, whose own comment records that it came from
/// paclink-unix). A public protocol constant, **not** a key — see the module header.
///
/// `pub` for two reasons: the salt-sensitivity negative control perturbs a copy of it, and an
/// operator debugging a refused login against another client needs to be able to compare bytes.
///
/// Its *value* carries no test. The negative control proves that changing a byte changes the
/// answer — it cannot prove these are the right bytes, and no offline test can. Only a live CMS
/// can say that.
pub const WINLINK_SALT: [u8; 64] = [
    77, 197, 101, 206, 190, 249, 93, 200, 51, 243, 93, 237, 71, 94, 239, 138, 68, 108, 70, 185,
    225, 137, 217, 16, 51, 122, 193, 48, 194, 195, 198, 175, 172, 169, 70, 84, 61, 62, 104, 186,
    114, 52, 61, 168, 66, 129, 192, 208, 187, 249, 232, 193, 41, 113, 41, 45, 240, 16, 29, 228,
    208, 228, 61, 20,
];

/// Number of ASCII decimal digits in a `;PR:` token.
const PR_DIGITS: usize = 8;

/// The `;PR:` response for a `;PQ:` challenge — exactly eight ASCII decimal digits, ready to be
/// written straight after the `;PR:` prefix.
///
/// `challenge` is the raw bytes of the `;PQ:` value **as they arrived**: no trimming, no case
/// folding, no UTF-8 round-trip. The digest is taken over exactly what the CMS sent, so any
/// normalisation a caller does on the way in is a silent wrong answer. `password` is
/// [`ClientConfig::password`](super::ClientConfig) as bytes.
pub fn pr_response(challenge: &[u8], password: &[u8]) -> Vec<u8> {
    pr_with_salt(&WINLINK_SALT, challenge, password)
}

/// [`pr_response`] with the salt supplied explicitly, so the negative control can run the real
/// derivation over a perturbed copy of a compile-time constant.
///
/// **Undecidable choice #1 — the feed order.** wl2k-go digests the single concatenation
/// `challenge ++ password ++ salt`. Feeding the three slices to one [`Md5`] in that order is the
/// same byte stream, and it avoids materialising a joined copy of the password on the heap where
/// a later allocator reuse could expose it.
fn pr_with_salt(salt: &[u8; 64], challenge: &[u8], password: &[u8]) -> Vec<u8> {
    let mut md5 = Md5::new();
    md5.update(challenge);
    md5.update(password);
    md5.update(salt);
    let digest: [u8; 16] = md5.finalize().into();

    reduce_digest_wl2kgo(&digest)
}

/// **Undecidable choice #2 — the digest→token reduction.** Isolated and named so that a live CMS
/// disagreement is a one-function edit rather than a hunt through the session engine.
///
/// wl2k-go reads the digest's first four bytes as a little-endian 32-bit value with the most
/// significant byte masked to six bits, prints it as zero-padded decimal, and keeps the last
/// eight characters. Twelve of the sixteen digest bytes are therefore never consulted; that is
/// the reference's behaviour, transcribed rather than corrected.
///
/// Two consequences that each look like a bug and are neither:
/// * The six-bit mask caps the value at `0x3FFF_FFFF`, so it can never be negative. wl2k-go's
///   `int32` and this `u32` print identically, and there is no overflow to reason about.
/// * `0x3FFF_FFFF` is 1_073_741_823 — **ten** digits — so the trailing-eight rule genuinely
///   discards leading digits. Both halves of the formatting are live in production, and the rare
///   half is the dangerous one: over a uniform 30-bit value roughly 91% of digests exceed eight
///   digits and are truncated, while about 0.9% fall short and are zero-padded. A vector set that
///   happens to sample only the common half would let a broken pad ship and then panic on roughly
///   one login in a hundred, so the pinned vectors below carry one of each on purpose.
fn reduce_digest_wl2kgo(digest: &[u8; 16]) -> Vec<u8> {
    // Most significant byte first, so the fold shifts the remaining three in underneath it —
    // little-endian over `digest[0..4]`, exactly wl2k-go's descending `i = 2, 1, 0` loop.
    let masked_high = u32::from(digest[3] & 0x3f);
    let value = digest[..3]
        .iter()
        .rev()
        .fold(masked_high, |acc, &byte| (acc << 8) | u32::from(byte));

    // `{:08}` guarantees at least PR_DIGITS characters, so the slice below cannot underflow; the
    // output is ASCII digits only, so byte slicing and character slicing agree.
    let decimal = format!("{value:0width$}", width = PR_DIGITS);
    decimal.as_bytes()[decimal.len() - PR_DIGITS..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The negative control the spec requires: the salt must actually reach the digest. It fails
    /// loudly if a future edit drops the salt from the feed, or feeds a constant in its place —
    /// which is the whole failure mode a green self-consistency suite would otherwise hide.
    #[test]
    fn one_byte_salt_change_changes_pr() {
        let challenge = b"ABC123";
        let password = b"secretpass";
        let base = pr_with_salt(&WINLINK_SALT, challenge, password);
        let mut tweaked = WINLINK_SALT;
        tweaked[0] ^= 0x01;
        let changed = pr_with_salt(&tweaked, challenge, password);
        assert_ne!(base, changed, "a one-byte salt change must change ;PR:");
    }

    #[test]
    fn pr_is_deterministic() {
        let c = b"ZZ9ZZ9";
        let p = b"hunter2";
        assert_eq!(pr_response(c, p), pr_response(c, p));
    }

    /// The other two inputs must reach the digest too — a `;PR:` that ignored the challenge would
    /// hand every session the same token and would pass the salt control unharmed.
    #[test]
    fn challenge_and_password_each_change_pr() {
        let base = pr_response(b"ABC123", b"secretpass");
        assert_ne!(
            base,
            pr_response(b"ABC124", b"secretpass"),
            "a changed challenge must change ;PR:"
        );
        assert_ne!(
            base,
            pr_response(b"ABC123", b"secretpast"),
            "a changed password must change ;PR:"
        );
    }

    /// Shape, over inputs including a non-UTF-8 challenge and password — the bytes-not-`String`
    /// rule is only real if the API actually accepts bytes a `String` could not hold.
    #[test]
    fn pr_is_always_eight_ascii_digits() {
        let cases: [(&[u8], &[u8]); 5] = [
            (b"ABC123", b"secretpass"),
            (b"ZZ9ZZ9", b"hunter2"),
            (b"N0CALL", b""),
            (b"\xff\xfe\x00CHAL", b"p\xc3\xa4ss"),
            // Reduces to 9_489_147 — seven digits, so this is the ~0.9% zero-padding path that
            // the other four (all nine-digit, all truncating) cannot reach.
            (b"C0", b"pw"),
        ];
        for (challenge, password) in cases {
            let pr = pr_response(challenge, password);
            assert_eq!(
                pr.len(),
                PR_DIGITS,
                "wrong length for {challenge:?}: {pr:?}"
            );
            assert!(
                pr.iter().all(u8::is_ascii_digit),
                "non-digit in ;PR: for {challenge:?}: {pr:?}"
            );
        }
    }

    /// Pins the derivation, so an accidental change to the byte order, the mask or the formatting
    /// is loud instead of silent — the sensitivity controls above would all still pass through any
    /// of those.
    ///
    /// These vectors were produced by an independent transliteration of wl2k-go's
    /// `secureLoginResponse` (its own arithmetic, its own MD5, its own `%08d`) and agree with this
    /// implementation. **They pin the wl2k-go reading, not CMS truth.** If the first live connect
    /// disagrees, the fix is [`reduce_digest_wl2kgo`] / [`pr_with_salt`] and then these values —
    /// deliberately, in that order, never the other way round.
    #[test]
    fn pinned_wl2kgo_vectors() {
        let vectors: [(&[u8], &[u8], &[u8]); 5] = [
            // Four nine-digit reductions — the truncating path.
            (b"ABC123", b"secretpass", b"28762506"),
            (b"ZZ9ZZ9", b"hunter2", b"82124384"),
            (b"N0CALL", b"", b"59231567"),
            (b"\xff\xfe\x00CHAL", b"p\xc3\xa4ss", b"14724529"),
            // And one seven-digit reduction — the zero-padding path, which no vector above
            // exercises. Its leading `0` is the whole point of the case.
            (b"C0", b"pw", b"09489147"),
        ];
        for (challenge, password, expected) in vectors {
            assert_eq!(
                pr_response(challenge, password),
                expected,
                "derivation drifted for challenge {challenge:?}"
            );
        }
    }
}
