//! The FBB forwarding protocol — `FC` proposals, the `F>` block checksum, `FS` answers, and the
//! SOH/STX/EOT binary framer.
//!
//! Two halves of one wire language. The **proposal** half is line-oriented ASCII: `FC` lines offer
//! messages by MID with their uncompressed and compressed sizes, `F>` closes the block with a
//! two-hex-digit checksum over it, and the peer's `FS` line answers each proposal in order —
//! including the `!offset` form that resumes a transfer already partly held. The **framing** half
//! is binary: SOH/STX records with a length byte (`0x00` meaning 256), terminated by EOT carrying
//! a two's-complement checksum over the data, and a frame whose checksum fails must deliver
//! nothing at all rather than deliver something corrupt.
//!
//! Bytes, never `String`: a length byte, a checksum byte and an LZHUF body are all free to be any
//! value at all, and decoding them as UTF-8 corrupts them inbound and — far worse — outbound.
//!
//! The proposal half is implemented below; the binary framer lands beside it.
//!
//! # The `FC` proposal line
//!
//! ```text
//! FC <type> <MID> <u-size> <c-size> <offset>\r
//! FC EM ABCDEFGHIJKL 1234 900 0\r
//! ```
//!
//! Six space-separated fields, single spaces, terminated by a bare CR (`\r`) — not CRLF. The
//! fields, from the ARSFI B2F document (winlink.org/B2F) and the FBB forward protocol
//! (f6fbb.org/fbbdoc/docfwpro.htm):
//!
//! * **`FC`** — the proposal *code*. `F` then one code byte: `C` is the B2 compressed proposal,
//!   and B2F also names `FA` (ASCII) and `FB` (binary), which "may be intermixed" with it in one
//!   block. Their field grammars differ from `FC`'s, so [`parse_proposal`] handles `FC` only and
//!   refuses the others as [`FbbError::Malformed`] rather than mis-slicing them; the code byte
//!   still survives into [`Proposal::kind`] so that a proposal built elsewhere can be answered
//!   safely (see the `FS` table below). Nexus proposes and accepts only `FC`.
//! * **`<type>`** — the message type: one or two alphanumerics. B2F names `EM` (encapsulated
//!   message) and `CM` (Winlink control message). Validated for shape and then **discarded**:
//!   nothing in the transfer branches on it (the B2 message carries its own headers), and a
//!   `Proposal` that stored it would be inventing a field the interface does not have. If a
//!   consumer ever needs `EM` vs `CM`, it is one field added here, not a re-parse elsewhere.
//! * **`<MID>`** — the message identifier, B2F's "max 12 characters". Kept verbatim as bytes and
//!   **not length-checked**: refusing a thirteen-character MID would refuse a working gateway for
//!   naming a message in a way we did not predict, the forward-compatibility trap
//!   [`super::sid`] documents at length. Empty is refused — that is a mis-split, not a MID.
//! * **`<u-size>` / `<c-size>`** — uncompressed and compressed sizes, ASCII decimal. Both are
//!   needed: `c-size` is how many body bytes to expect on the wire, `u-size` is what they
//!   decompress to, and a progress bar that used the wrong one lies by the compression ratio.
//! * **`<offset>`** — optional sixth field, the FBB fragmented-file offset. Every implementation
//!   observed writes a literal `0`, and the FBB document says of the equivalent header field
//!   "in version 5.12, this parameter is not utilized and is always equal to zero". This parser
//!   requires it to be decimal if present and then **ignores its value**. ⚠️ A peer that sent a
//!   nonzero offset here would be proposing a resume Nexus does not implement, and this parser
//!   would silently drop that intent. Recorded as a known gap rather than guessed at in code.
//!
//! # The `F>` block checksum
//!
//! A proposal block is one or more `FC` lines followed by `F> HH\r`, where `HH` is two uppercase
//! ASCII hex digits of a single byte: **the sum of every byte of the proposal lines, including
//! each line's terminating CR, negated modulo 256** — the two's complement, so that
//! `Σ(block bytes) + checksum ≡ 0 (mod 256)`. The `F>` line itself is not part of the sum.
//!
//! [`fb_checksum`] is that byte and nothing more. It takes the block **exactly as it goes on the
//! wire**, CRs included, and the caller renders `F> {:02X}\r` and compares or emits it — the
//! split is deliberate: the byte is the protocol fact worth property-testing, while the hex
//! rendering is presentation, and folding them together would make the primitive untestable
//! against an independent implementation.
//!
//! # The `FS` answer table
//!
//! ```text
//! FS +-!1234\r
//! ```
//!
//! `FS ` then **one answer per proposal, in the order proposed**, then CR. The answer characters,
//! verbatim from the FBB forward protocol and its version-1 extension:
//!
//! | Sent | Also accepted inbound | Meaning |
//! |------|----------------------|---------|
//! | `+`  | `Y` `y`              | "I need this message" — send it. |
//! | `-`  | `N` `n` `R` `r`      | "I don't want this message" — do not send it; it is answered. |
//! | `=`  | `L` `l` `H` `h`      | "I defer this message" — do not send it now, **offer it again**. |
//! | `!n` | `A` `a`              | Accept, resuming at byte offset `n` — send from `n` onward. |
//!
//! [`fs_answer`] emits the four left-hand forms; the right-hand column is what a peer may send
//! back and belongs to the session that parses an inbound `FS`.
//!
//! Two rules in [`fs_answer`] are worth their own sentence, because both are cases where the
//! obvious answer is the damaging one:
//!
//! 1. **A proposal code we do not implement is deferred, never rejected.** `-` tells the peer the
//!    message is answered and it stops offering it — for a proposal we merely failed to *parse*,
//!    that silently destroys mail. `=` keeps it queued for a session that can take it.
//! 2. **An offset the wire cannot carry falls back to `+`.** The FBB offset field is one to six
//!    ASCII digits, and a receiver given more than six ignores the offset entirely and sends from
//!    byte zero — so `!1000000` reads as "start over" at the far end while this end waits for a
//!    resume, which is a desync rather than a slow transfer. `+` says "start over" out loud.

/// The proposal code Nexus speaks: `FC`, the B2 compressed proposal.
///
/// Exposed because [`Proposal::kind`] is a public field a consumer has to fill in, and a bare
/// `b'C'` at that call site says nothing about which of `FA`/`FB`/`FC` it means.
pub const PROPOSAL_CODE_FC: u8 = b'C';

/// The largest offset an `FS` answer can carry: the FBB offset field is one to six ASCII digits.
///
/// Not a limit Nexus chose — see rule 2 in the module header for what happens above it.
const MAX_FS_OFFSET: u32 = 999_999;

/// One proposal from an `FC` line: a message the peer offers, named by MID and sized twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// The proposal code byte — the byte after `F`. [`PROPOSAL_CODE_FC`] (`b'C'`) for everything
    /// [`parse_proposal`] returns; other values reach [`fs_answer`] only from a `Proposal` built
    /// elsewhere, and are deferred rather than answered.
    pub kind: u8,
    /// The message identifier, verbatim bytes. Not length-checked, not decoded — see the module
    /// header.
    pub mid: Vec<u8>,
    /// Uncompressed size in bytes: what the body decompresses to.
    pub u_size: u32,
    /// Compressed size in bytes: what actually crosses the wire, and therefore what a transfer
    /// counts down.
    pub c_size: u32,
}

/// What this station already holds of a proposed message, as [`fs_answer`] needs to know it.
///
/// Deliberately *not* an answer character: the caller answers "what do I have", and the mapping
/// from that to `+`/`-`/`!n` — including the two traps in the module header — stays in one place
/// where it can be tested, instead of being re-derived at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaveState {
    /// Nothing held. Answered `+`.
    No,
    /// Held in full. Answered `-`.
    Yes,
    /// Held in part, this many bytes of the **compressed** body. Answered `!n`, subject to the
    /// six-digit ceiling.
    Partial(u32),
}

/// Why an FBB proposal exchange was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FbbError {
    /// The `F> HH` value does not match [`fb_checksum`] over the proposal block that preceded it,
    /// so the block is corrupt and no proposal in it may be acted on.
    ///
    /// Raised by the session that owns both halves of the comparison — this module supplies the
    /// checksum byte, and the block and its `F>` line are the session's to hold.
    ProposalChecksum,
    /// A line did not parse: wrong proposal code, wrong field count, a size that is not ASCII
    /// decimal or does not fit `u32`, an empty MID, or a malformed message type.
    Malformed,
}

/// Parses one `FC` proposal line. See the module header for the grammar and every refusal.
///
/// Accepts the line with or without its terminator (`\r`, `\n` or `\r\n`), because a framer that
/// splits on CR and one that hands over the CR are both reasonable and this is not the place to
/// care which. Spaces are single and significant: a doubled space shifts every field, so it is a
/// refusal rather than something to normalise.
pub fn parse_proposal(line: &[u8]) -> Result<Proposal, FbbError> {
    let body = trim_terminator(line);

    // `F`, the code byte, then the separating space. Checked before the split so that a line that
    // is not a proposal at all is refused on what it is, rather than on a field count.
    let (&[b'F', kind, b' '], rest) = body.split_at_checked(3).ok_or(FbbError::Malformed)? else {
        return Err(FbbError::Malformed);
    };
    if kind != PROPOSAL_CODE_FC {
        return Err(FbbError::Malformed);
    }

    // Single spaces, and a doubled one yields an empty field that the checks below refuse — the
    // module header's reason for not normalising runs of whitespace away.
    let fields: Vec<&[u8]> = rest.split(|&b| b == b' ').collect();
    if fields.len() != 4 && fields.len() != 5 {
        return Err(FbbError::Malformed);
    }

    // The message type: shape-checked, then dropped. See the module header.
    let msg_type = fields[0];
    if msg_type.is_empty()
        || msg_type.len() > 2
        || !msg_type.iter().all(u8::is_ascii_alphanumeric)
    {
        return Err(FbbError::Malformed);
    }

    let mid = fields[1];
    if mid.is_empty() {
        return Err(FbbError::Malformed);
    }

    let u_size = parse_decimal(fields[2])?;
    let c_size = parse_decimal(fields[3])?;
    // The trailing offset: validated so a garbled tail is caught here rather than mistaken for a
    // short line, and then discarded — Nexus does not implement the resume it would ask for.
    if let Some(offset) = fields.get(4) {
        parse_decimal(offset)?;
    }

    Ok(Proposal {
        kind,
        mid: mid.to_vec(),
        u_size,
        c_size,
    })
}

/// Strips any mixture of trailing CR and LF, so a framer that keeps the terminator, one that
/// strips it, and one that hands over `\r\n` all reach the same parse.
fn trim_terminator(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    while end > 0 && (line[end - 1] == b'\r' || line[end - 1] == b'\n') {
        end -= 1;
    }
    &line[..end]
}

/// ASCII decimal to `u32`, refusing an empty field, any non-digit, and anything that does not fit.
///
/// Hand-rolled rather than `str::parse` because the field is bytes and may not be UTF-8: going
/// through `from_utf8` would turn a wire error into a *different* wire error and add a decode this
/// module exists to avoid. The overflow refusal is a real boundary — `u32::MAX` itself parses.
fn parse_decimal(field: &[u8]) -> Result<u32, FbbError> {
    if field.is_empty() || !field.iter().all(u8::is_ascii_digit) {
        return Err(FbbError::Malformed);
    }
    field.iter().try_fold(0u32, |acc, &digit| {
        acc.checked_mul(10)
            .and_then(|shifted| shifted.checked_add(u32::from(digit - b'0')))
            .ok_or(FbbError::Malformed)
    })
}

/// The `F>` checksum byte over a proposal block: `Σ bytes`, negated modulo 256.
///
/// `proposals` is the block exactly as it goes on the wire — every `FC` line **including its
/// terminating CR**, and nothing else. The caller renders it as `F> {:02X}\r`.
pub fn fb_checksum(proposals: &[u8]) -> u8 {
    let sum = proposals
        .iter()
        .fold(0u8, |acc, &byte| acc.wrapping_add(byte));
    // Two's complement of the sum, which is what makes `Σ bytes + checksum ≡ 0 (mod 256)` hold at
    // the receiver. `0 - sum` rather than `!sum + 1` so the definition reads as the arithmetic.
    0u8.wrapping_sub(sum)
}

/// Builds the `FS` answer line for a block of proposals, in order, terminated by CR.
///
/// `have` is asked, per proposal, what this station already holds of that MID; the answer table
/// and its two traps are in the module header. The returned line is complete and ready to write.
///
/// An empty `proposals` slice yields `FS \r`. The protocol never sends that — a peer with nothing
/// to offer sends `FF` and no block at all — so the session is responsible for not asking.
pub fn fs_answer(proposals: &[Proposal], have: impl Fn(&[u8]) -> HaveState) -> Vec<u8> {
    let mut line = Vec::with_capacity(b"FS \r".len() + proposals.len());
    line.extend_from_slice(b"FS ");
    for proposal in proposals {
        if proposal.kind != PROPOSAL_CODE_FC {
            // Trap 1: defer, never reject. A reject answers the message for good.
            line.push(b'=');
            continue;
        }
        match have(&proposal.mid) {
            HaveState::Yes => line.push(b'-'),
            HaveState::No => line.push(b'+'),
            // Trap 2: nothing held, and an offset the six-digit field cannot carry, both mean
            // "send it from the start" — and `+` is the only way to say that without a desync.
            HaveState::Partial(0) => line.push(b'+'),
            HaveState::Partial(offset) if offset > MAX_FS_OFFSET => line.push(b'+'),
            HaveState::Partial(offset) => {
                line.push(b'!');
                line.extend_from_slice(offset.to_string().as_bytes());
            }
        }
    }
    line.push(b'\r');
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A well-formed one-proposal block, terminated the way the wire terminates it.
    const BLOCK: &[u8] = b"FC EM FOOBAR123 1234 900 0\r";

    /// The spec's negative control: a one-byte change to a proposal block must be visible in the
    /// `F>` checksum. A checksum that ignored its input — or ignored the byte that moved — passes
    /// every round-trip test in this module and fails this one.
    #[test]
    fn a_perturbed_proposal_block_fails_on_checksum() {
        let good = fb_checksum(BLOCK);
        for index in 0..BLOCK.len() {
            let mut bad = BLOCK.to_vec();
            bad[index] ^= 0x01;
            assert_ne!(
                fb_checksum(&bad),
                good,
                "a one-byte change at {index} left the checksum unmoved"
            );
        }
    }

    /// Pins the checksum against a value computed outside this codebase (by hand and again in
    /// Python) over the block above, in the exact `F> HH` rendering a caller emits. Every other
    /// checksum test here is self-consistent; this one is not.
    #[test]
    fn pinned_block_checksum_renders_as_two_hex_digits() {
        assert_eq!(format!("F> {:02X}\r", fb_checksum(BLOCK)), "F> 56\r");
        let two = b"FC EM ABCDEFGHIJKL 1234 900 0\rFC EM ZZZZZZZZZZZZ 10 8 0\r";
        assert_eq!(format!("F> {:02X}\r", fb_checksum(two)), "F> 8E\r");
    }

    proptest! {
        /// The shipped checksum against an independent implementation of the documented rule.
        #[test]
        fn fb_checksum_matches_independent_impl(data in proptest::collection::vec(any::<u8>(), 0..512)) {
            let sum = data.iter().fold(0u8, |a, &b| a.wrapping_add(b));
            let independent = 0u8.wrapping_sub(sum);
            prop_assert_eq!(fb_checksum(&data), independent);
        }

        /// The receiver's side of the same fact, as the FBB document states it: "the sum of the
        /// data and the checksum received, modulo 256, shall be equal to zero".
        #[test]
        fn block_plus_checksum_is_zero_mod_256(data in proptest::collection::vec(any::<u8>(), 0..512)) {
            let sum = data.iter().fold(0u8, |a, &b| a.wrapping_add(b));
            prop_assert_eq!(sum.wrapping_add(fb_checksum(&data)), 0u8);
        }
    }

    #[test]
    fn parses_a_six_field_proposal() {
        let p = parse_proposal(b"FC EM ABCDEFGHIJKL 1234 900 0\r").expect("well-formed FC line");
        assert_eq!(
            p,
            Proposal {
                kind: PROPOSAL_CODE_FC,
                mid: b"ABCDEFGHIJKL".to_vec(),
                u_size: 1234,
                c_size: 900,
            }
        );
    }

    /// The trailing offset field is optional, the terminator is optional, and `\r\n` is tolerated
    /// as well as a bare `\r` — three framers, one parse.
    #[test]
    fn tolerates_missing_offset_and_every_terminator() {
        let expected = Proposal {
            kind: PROPOSAL_CODE_FC,
            mid: b"ABC".to_vec(),
            u_size: 7,
            c_size: 5,
        };
        for line in [
            &b"FC EM ABC 7 5"[..],
            &b"FC EM ABC 7 5 0"[..],
            &b"FC EM ABC 7 5 0\r"[..],
            &b"FC EM ABC 7 5 0\r\n"[..],
            &b"FC EM ABC 7 5 0\n"[..],
        ] {
            assert_eq!(
                parse_proposal(line).as_ref(),
                Ok(&expected),
                "failed on {line:?}"
            );
        }
    }

    /// `CM` is B2F's other documented type, and a one-character type is inside the "1-2
    /// alphanumeric" rule. Both parse; neither changes the result, because the type is discarded.
    #[test]
    fn accepts_every_documented_message_type() {
        for line in [
            &b"FC CM ABC 7 5 0"[..],
            &b"FC E ABC 7 5 0"[..],
            &b"FC 9Z ABC 7 5 0"[..],
        ] {
            assert!(parse_proposal(line).is_ok(), "refused {line:?}");
        }
    }

    #[test]
    fn refuses_malformed_proposals() {
        let cases: [(&[u8], &str); 13] = [
            (b"", "empty"),
            (b"FC", "code only"),
            (b"FC ", "no fields"),
            (b"XC EM ABC 7 5 0", "not an F line"),
            (b"FA EM ABC 7 5 0", "FA grammar is not FC's"),
            (b"FB EM ABC 7 5 0", "FB grammar is not FC's"),
            (b"FC_EM ABC 7 5 0", "no space after the code"),
            (b"FC EM ABC 7", "one field short"),
            (b"FC EM ABC 7 5 0 9", "one field long"),
            (b"FC EM  7 5 0", "empty MID from a doubled space"),
            (b"FC EMM ABC 7 5 0", "three-character message type"),
            (b"FC EM ABC 7 five 0", "non-decimal compressed size"),
            (b"FC EM ABC 4294967296 5 0", "uncompressed size overflows u32"),
        ];
        for (line, why) in cases {
            assert_eq!(
                parse_proposal(line),
                Err(FbbError::Malformed),
                "should have refused ({why}): {line:?}"
            );
        }
    }

    /// The upper bound of `u32` itself must still parse — the overflow refusal above is a real
    /// boundary, not an off-by-one that quietly refuses legal sizes.
    #[test]
    fn accepts_the_largest_representable_size() {
        let p = parse_proposal(b"FC EM ABC 4294967295 4294967295 0").expect("u32::MAX is legal");
        assert_eq!(p.u_size, u32::MAX);
        assert_eq!(p.c_size, u32::MAX);
    }

    fn fc(mid: &[u8]) -> Proposal {
        Proposal {
            kind: PROPOSAL_CODE_FC,
            mid: mid.to_vec(),
            u_size: 100,
            c_size: 80,
        }
    }

    /// The answer table, all four forms at once, in proposal order.
    #[test]
    fn answers_each_proposal_in_order() {
        let props = [fc(b"NEW"), fc(b"HELD"), fc(b"PART"), fc(b"ALSONEW")];
        let line = fs_answer(&props, |mid| match mid {
            b"HELD" => HaveState::Yes,
            b"PART" => HaveState::Partial(1234),
            _ => HaveState::No,
        });
        assert_eq!(line, b"FS +-!1234+\r".to_vec());
    }

    /// A positive control on the closure's argument: `have` must be asked about the MID of the
    /// proposal it is answering, and about every one of them. An implementation that answered
    /// from the first proposal, or from a constant, passes the ordering test above.
    #[test]
    fn have_is_asked_about_every_mid() {
        let props = [fc(b"AAA"), fc(b"BBB"), fc(b"CCC")];
        let seen = std::cell::RefCell::new(Vec::new());
        let line = fs_answer(&props, |mid| {
            seen.borrow_mut().push(mid.to_vec());
            if mid == b"BBB" {
                HaveState::Yes
            } else {
                HaveState::No
            }
        });
        assert_eq!(
            seen.into_inner(),
            vec![b"AAA".to_vec(), b"BBB".to_vec(), b"CCC".to_vec()]
        );
        assert_eq!(line, b"FS +-+\r".to_vec());
    }

    /// Trap 2 from the module header, both sides of the six-digit boundary. `Partial(0)` is not a
    /// partial at all and must not go out as `!0`.
    #[test]
    fn offsets_the_wire_cannot_carry_fall_back_to_accept() {
        let props = [fc(b"A"), fc(b"B"), fc(b"C"), fc(b"D")];
        let line = fs_answer(&props, |mid| match mid {
            b"A" => HaveState::Partial(0),
            b"B" => HaveState::Partial(1),
            b"C" => HaveState::Partial(MAX_FS_OFFSET),
            _ => HaveState::Partial(MAX_FS_OFFSET + 1),
        });
        assert_eq!(line, b"FS +!1!999999+\r".to_vec());
    }

    /// Trap 1 from the module header: a proposal code this build does not implement is deferred,
    /// so the peer offers it again, instead of rejected, which would answer it for good.
    #[test]
    fn an_unhandled_proposal_code_is_deferred_not_rejected() {
        let props = [
            Proposal {
                kind: b'A',
                mid: b"ASCII".to_vec(),
                u_size: 10,
                c_size: 10,
            },
            fc(b"OURS"),
        ];
        let line = fs_answer(&props, |_| HaveState::No);
        assert_eq!(line, b"FS =+\r".to_vec());
    }

    /// Documented degenerate case, pinned so it cannot drift into something that looks like a
    /// real line.
    #[test]
    fn an_empty_block_answers_with_no_characters() {
        assert_eq!(fs_answer(&[], |_| HaveState::No), b"FS \r".to_vec());
    }
}
