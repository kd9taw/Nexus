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
//! Both halves are implemented below.
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

//! # The SOH/STX/EOT framing
//!
//! Once an `FS` line has accepted a proposal, the body crosses as a stream of binary records. The
//! framing is the FBB forward protocol's own (f6fbb.org/fbbdoc/docfwpro.htm), which B2F carries
//! over unchanged:
//!
//! ```text
//! <SOH> <len> <title> NUL <offset> NUL       one header block, opening a file
//! <STX> <len> <data ...>                     zero or more data blocks
//! <EOT> <checksum>                           end of file
//! ```
//!
//! Three facts, and each has an obvious reading that is wrong:
//!
//! * **`<len>` is one byte, and `0x00` means 256** — a full block, not an empty one. A block
//!   carrying no bytes is unrepresentable, which is why `0x00` was free to mean the maximum. An
//!   implementation that reads it as zero silently splits every full block in the transfer.
//! * **The checksum covers the STX data bytes and nothing else** — not the SOH header, not the
//!   markers, not the length bytes. It is the **two's complement** of their sum, so the receiver's
//!   check is `Σ data + checksum ≡ 0 (mod 256)`. Widening the sum to the whole wire record is the
//!   easy mistake, and it is invisible against a sender that made the same one.
//! * **A block whose checksum fails delivers nothing.** Its data blocks parsed cleanly and
//!   completely before the EOT arrived, so a framer that hands frames on as it parses them looks
//!   entirely correct until the first corrupt transfer, and then delivers corrupt mail. [`Framer`]
//!   therefore holds a block back until its own EOT vouches for it — [`Framer::feed`] returns
//!   nothing before then — which is the spec's second negative control (§5).
//!
//! ⚠️ **Write to the implementation, not to the 2017 ARDOP host-interface PDF** (spec §7). That
//! document disagrees with every shipped implementation on data framing — `D:`/`d:` prefixes
//! ardopcf does not implement, and a 3-byte tag counted inside the length. The vendor-spec rule
//! cuts the other way here, and the two rules above are pinned by property tests and by a fixture
//! whose checksum was derived outside this codebase.
//!
//! What the framer deliberately does **not** do: interpret the SOH payload (the title and offset
//! belong to the session that proposed the message, not to a framer), and bound the size of a
//! transfer. The wire buffer is self-bounding — a record is at most 258 bytes, so at most 257 can
//! sit incomplete — but the accumulated block grows with the message. The number that bounds it is
//! the proposal's `c-size`, which the session holds; duplicating a guess at it here would refuse a
//! legal transfer on a limit no protocol document states.

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
    /// The `EOT` checksum does not match the two's complement of the sum of the block's data
    /// bytes, so the block is corrupt and **none of it may be delivered** — see [`Framer`].
    ///
    /// Distinct from [`ProposalChecksum`](FbbError::ProposalChecksum) because the two are
    /// different negative controls over different bytes (spec §5) and a session answers them
    /// differently: a bad proposal block is re-proposable, a bad body block failed a transfer.
    EotChecksum,
    /// A line did not parse: wrong proposal code, wrong field count, a size that is not ASCII
    /// decimal or does not fit `u32`, an empty MID, or a malformed message type.
    ///
    /// Also raised by [`Framer`] for a byte that is not one of the three framing markers, which is
    /// the same fault at the other layer: the stream is not what it claims to be.
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

/// The FBB framing markers, as ASCII control codes and under the protocol's own names.
const SOH: u8 = 0x01;
const STX: u8 = 0x02;
const EOT: u8 = 0x04;

/// The largest payload one block can carry — and the size a `0x00` length byte means.
const MAX_BLOCK: usize = 256;

/// One complete record off the SOH/STX/EOT wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// An `SOH` header block: the payload verbatim, `<title> NUL <offset> NUL`.
    ///
    /// Handed on **unparsed and uninterpreted**. The title binds the bytes that follow to one of
    /// the MIDs this station accepted, and the offset is the `!offset` resume the `FS` answer may
    /// have asked for — both are the session's to read, and a framer that split them here would be
    /// guessing at a grammar it has no other reason to know. It is carried rather than dropped
    /// because dropping it is the silent loss: the data blocks alone cannot say which message they
    /// are, and a receiver that inferred it from proposal order would mis-file mail the first time
    /// a sender skipped one.
    Header(Vec<u8>),
    /// An `STX` data block payload — the bytes the checksum covers, in wire order.
    Data(Vec<u8>),
    /// `EOT`, and its checksum verified. Never emitted for a block that failed: see [`Framer`].
    Eot,
}

/// Reassembles the SOH/STX/EOT stream from arbitrary chunk boundaries, delivering only what an
/// `EOT` has vouched for.
///
/// The shape is `tempo_net::aprsis::Session`'s — an internal `buf` the caller extends with
/// whatever bytes arrived, and a `feed` that drains every complete unit the buffer now holds — so
/// a transfer unit-tests from a `Vec<u8>` with no socket, and the chunk-boundary bug class FlexCat
/// paid for is a property test rather than a field report.
///
/// Two rules make it more than a splitter, and both are the module header's third fact:
///
/// 1. **Nothing is delivered before its `EOT`.** Parsed blocks accumulate unpublished; the `EOT`
///    that verifies moves them, together, into the delivery buffer. There is no window in which a
///    caller can act on data the checksum has not covered.
/// 2. **A checksum failure is terminal.** FBB has no resynchronisation marker, so a framer that
///    has been handed a corrupt transfer cannot honestly claim to know where the next record
///    begins; it latches the refusal and returns it to every later [`feed`](Framer::feed). What
///    *did* verify before the failure stays retrievable through
///    [`take_delivered`](Framer::take_delivered) — that is why the delivery buffer outlives
///    `feed`'s `Result` instead of being only its return value.
#[derive(Debug, Default)]
pub struct Framer {
    /// Wire bytes not yet consumed by a complete record. Self-bounding: a record is at most
    /// `2 + MAX_BLOCK` bytes, so at most `2 + MAX_BLOCK - 1` can sit here incomplete.
    buf: Vec<u8>,
    /// Blocks parsed since the last `EOT`, held back until one vouches for them.
    pending: Vec<Frame>,
    /// Running `Σ` of the data bytes in `pending`, which the `EOT` checksum must complement.
    sum: u8,
    /// Verified frames the caller has not collected yet.
    delivered: Vec<Frame>,
    /// The latched refusal, if the stream has failed. See rule 2 above.
    failed: Option<FbbError>,
}

impl Framer {
    /// A framer at the start of a stream.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds received bytes; returns every frame the stream has now vouched for.
    ///
    /// An empty `Ok` is the ordinary answer to a chunk that did not complete a block — including a
    /// chunk that completed several data blocks whose `EOT` has not arrived. An `Err` is terminal:
    /// see rule 2 on [`Framer`], and collect anything already verified with
    /// [`take_delivered`](Framer::take_delivered).
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, FbbError> {
        if let Some(err) = self.failed {
            return Err(err);
        }
        self.buf.extend_from_slice(chunk);
        // `?` on purpose: an early return leaves `delivered` holding whatever earlier blocks
        // verified, for the caller that handles the error to collect.
        self.drain()?;
        Ok(self.take_delivered())
    }

    /// Takes the verified frames collected so far, leaving the buffer empty.
    ///
    /// [`feed`](Framer::feed) drains this on its way out, so after a successful feed there is
    /// nothing here. It is public for the caller that took an `Err`: the frames a *previous* `EOT`
    /// vouched for are still good, and this is how they are collected.
    pub fn take_delivered(&mut self) -> Vec<Frame> {
        std::mem::take(&mut self.delivered)
    }

    /// Consumes every complete record in `buf`, publishing each verified block.
    ///
    /// Returns `Ok(())` when the buffer holds only an incomplete record — a short read is not a
    /// fault — and stops on the first record that is corrupt or unframeable.
    fn drain(&mut self) -> Result<(), FbbError> {
        loop {
            let Some(&marker) = self.buf.first() else {
                return Ok(());
            };
            match marker {
                SOH | STX => {
                    let Some(&len_byte) = self.buf.get(1) else {
                        return Ok(());
                    };
                    // `0x00` is a full block, never an empty one — the module header's first fact.
                    let len = if len_byte == 0 {
                        MAX_BLOCK
                    } else {
                        usize::from(len_byte)
                    };
                    if self.buf.len() < 2 + len {
                        return Ok(());
                    }
                    let payload: Vec<u8> = self.buf.drain(..2 + len).skip(2).collect();
                    if marker == STX {
                        // Only these bytes are summed. Widening this to the record, or to the SOH
                        // header, is the mistake the pinned fixture exists to catch.
                        for &byte in &payload {
                            self.sum = self.sum.wrapping_add(byte);
                        }
                        self.pending.push(Frame::Data(payload));
                    } else {
                        self.pending.push(Frame::Header(payload));
                    }
                }
                EOT => {
                    let Some(&checksum) = self.buf.get(1) else {
                        return Ok(());
                    };
                    self.buf.drain(..2);
                    let sum = self.sum;
                    self.sum = 0;
                    let block = std::mem::take(&mut self.pending);
                    // The receiver's form of the rule, verbatim from the FBB document: "the sum of
                    // the data and the checksum received, modulo 256, shall be equal to zero".
                    if sum.wrapping_add(checksum) != 0 {
                        return Err(self.fail(FbbError::EotChecksum));
                    }
                    self.delivered.extend(block);
                    self.delivered.push(Frame::Eot);
                }
                // Not a marker: the stream is not FBB framing, or we are no longer aligned to it.
                _ => return Err(self.fail(FbbError::Malformed)),
            }
        }
    }

    /// Latches `err` and discards everything unverified, so no later call can deliver from a
    /// stream whose alignment is no longer known. `delivered` is untouched — it holds only frames
    /// an `EOT` already vouched for.
    fn fail(&mut self, err: FbbError) -> FbbError {
        self.failed = Some(err);
        self.buf.clear();
        self.pending.clear();
        self.sum = 0;
        err
    }
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

#[cfg(test)]
mod framer_tests {
    use super::*;
    use proptest::prelude::*;

    /// The `SOH` header payload the test framer emits: an FBB title/offset header, NUL-separated.
    /// Its bytes sum to 586, so a checksum that wrongly folded the header in would come out
    /// 0xA2 instead of 0xEC on the pinned block below — the header's exclusion is *observable*.
    const TEST_HEADER: &[u8] = b"TESTMID\x000\x00";

    /// Frames `body` the way an FBB sender does: one `SOH` header block, then `STX` data blocks of
    /// at most 256 bytes (a full block writes length `0x00`), then `EOT` carrying the
    /// two's-complement checksum over the **data bytes only**.
    ///
    /// ⚠️ This helper folds the checksum with the same arithmetic the shipped [`Framer`] verifies
    /// with, so every property test built on it is self-consistent by construction. The pinned
    /// wire bytes in [`pinned_wire_bytes_match_an_independently_derived_checksum`] are what make
    /// the value itself falsifiable.
    fn build_stx_block(body: &[u8]) -> Vec<u8> {
        let mut out = vec![SOH, TEST_HEADER.len() as u8];
        out.extend_from_slice(TEST_HEADER);
        for chunk in body.chunks(MAX_BLOCK) {
            out.push(STX);
            // The wire has no way to say 256 in one byte, so 0x00 means a full block.
            out.push(if chunk.len() == MAX_BLOCK {
                0
            } else {
                chunk.len() as u8
            });
            out.extend_from_slice(chunk);
        }
        let sum = body.iter().fold(0u8, |a, &b| a.wrapping_add(b));
        out.push(EOT);
        out.push(0u8.wrapping_sub(sum));
        out
    }

    /// Every `Frame::Data` payload in order, which for a single verified block is the message.
    fn data_of(frames: &[Frame]) -> Vec<u8> {
        frames
            .iter()
            .flat_map(|f| match f {
                Frame::Data(d) => d.clone(),
                Frame::Header(_) | Frame::Eot => Vec::new(),
            })
            .collect()
    }

    /// The spec's second negative control (§5): a bad EOT checksum MUST fail and MUST NOT deliver.
    /// The data blocks parsed cleanly and completely before the EOT arrived, so an implementation
    /// that handed frames on as it parsed them passes every round-trip test here and fails this.
    #[test]
    fn a_bad_eot_checksum_fails_and_delivers_nothing() {
        let mut framer = Framer::new();
        let mut stream = build_stx_block(b"hello");
        let n = stream.len();
        stream[n - 1] ^= 0x01; // corrupt the EOT checksum byte
        let out = framer.feed(&stream);
        assert_eq!(out, Err(FbbError::EotChecksum), "bad EOT checksum must fail");
        assert!(
            framer.take_delivered().is_empty(),
            "a bad EOT must not deliver"
        );
    }

    /// Pins the wire bytes against a checksum derived outside this codebase — by hand and again in
    /// Python from the protocol rule alone (`(-Σ data) mod 256`), never from this module. `hello`
    /// sums to 532, 532 mod 256 = 20, and 256 - 20 = 236 = 0xEC.
    ///
    /// Two facts are pinned at once, and the second is the one that is easy to get wrong: the
    /// checksum covers the **STX data only**. Folding the `SOH` header's bytes in would give 0xA2,
    /// which this test proves the framer rejects.
    #[test]
    fn pinned_wire_bytes_match_an_independently_derived_checksum() {
        let good: &[u8] = &[
            0x01, 0x0A, 0x54, 0x45, 0x53, 0x54, 0x4D, 0x49, 0x44, 0x00, 0x30, 0x00, 0x02, 0x05,
            0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x04, 0xEC,
        ];
        assert_eq!(build_stx_block(b"hello"), good, "the helper drifted");

        let mut framer = Framer::new();
        let frames = framer.feed(good).expect("0xEC is the checksum over the data");
        assert_eq!(
            frames,
            vec![
                Frame::Header(TEST_HEADER.to_vec()),
                Frame::Data(b"hello".to_vec()),
                Frame::Eot
            ]
        );

        // 0xA2 is the checksum this block would carry if the SOH header counted. It must fail.
        let mut header_folded_in = good.to_vec();
        *header_folded_in.last_mut().expect("non-empty") = 0xA2;
        assert_eq!(
            Framer::new().feed(&header_folded_in),
            Err(FbbError::EotChecksum),
            "the SOH header must not count toward the EOT checksum"
        );
    }

    /// The length byte's `0x00 == 256` rule and its checksum, both pinned to bytes computed in
    /// Python: 256 copies of 0xAA sum to 43520, which is 0 mod 256, so the checksum is also 0x00.
    /// A full block is the one case where the length byte and the checksum are both the value an
    /// off-by-one implementation reads as "empty".
    #[test]
    fn a_full_block_writes_a_zero_length_byte_and_a_zero_checksum() {
        let body = vec![0xAAu8; 256];
        let framed = build_stx_block(&body);
        let tail = &framed[framed.len() - 260..];
        assert_eq!((tail[0], tail[1]), (STX, 0x00), "256 is written as 0x00");
        assert_eq!(
            (framed[framed.len() - 2], framed[framed.len() - 1]),
            (EOT, 0x00)
        );
        let frames = Framer::new().feed(&framed).expect("valid block");
        assert_eq!(data_of(&frames), body);
    }

    /// Nothing may be handed on before the EOT that vouches for it. Feeding a complete data block
    /// with the EOT withheld must deliver nothing at all.
    #[test]
    fn nothing_is_delivered_before_the_eot() {
        let framed = build_stx_block(b"hello");
        let mut framer = Framer::new();
        let frames = framer
            .feed(&framed[..framed.len() - 2])
            .expect("a truncated stream is incomplete, not corrupt");
        assert!(frames.is_empty(), "delivered before the EOT: {frames:?}");
        assert!(framer.take_delivered().is_empty());
    }

    /// A block already vouched for by its own EOT is not un-delivered by a later corrupt block —
    /// the caller that took the error can still collect what verified. This is why the delivery
    /// buffer outlives `feed`'s `Result`.
    #[test]
    fn a_verified_block_survives_a_later_bad_checksum() {
        let mut stream = build_stx_block(b"first");
        let mut bad = build_stx_block(b"second");
        let n = bad.len();
        bad[n - 1] ^= 0x01;
        stream.extend_from_slice(&bad);

        let mut framer = Framer::new();
        assert_eq!(framer.feed(&stream), Err(FbbError::EotChecksum));
        assert_eq!(data_of(&framer.take_delivered()), b"first".to_vec());
        assert!(framer.take_delivered().is_empty(), "drained twice");
    }

    /// FBB has no resynchronisation marker, so a framer that has lost the stream stays lost: every
    /// later feed returns the same refusal rather than pretending the next byte begins a record.
    #[test]
    fn a_failed_framer_stays_failed() {
        let mut framer = Framer::new();
        let mut stream = build_stx_block(b"hello");
        let n = stream.len();
        stream[n - 1] ^= 0x01;
        assert_eq!(framer.feed(&stream), Err(FbbError::EotChecksum));
        assert_eq!(
            framer.feed(&build_stx_block(b"a perfectly good block")),
            Err(FbbError::EotChecksum)
        );
    }

    /// A byte that is not one of the three framing markers is a desync, not data.
    #[test]
    fn a_byte_that_is_not_a_framing_marker_is_refused() {
        let mut framer = Framer::new();
        assert_eq!(framer.feed(b"FF\r"), Err(FbbError::Malformed));
        assert_eq!(framer.feed(&build_stx_block(b"x")), Err(FbbError::Malformed));
    }

    /// The chunk-boundary bug class FlexCat already paid for (spec §5): the same stream fed one
    /// byte at a time must produce exactly the same frames, in the same order, as fed whole.
    #[test]
    fn one_byte_at_a_time_is_the_same_as_whole() {
        let body: Vec<u8> = (0..600u32).map(|i| (i % 251) as u8).collect();
        let framed = build_stx_block(&body);

        let whole = Framer::new().feed(&framed).expect("valid");
        let mut framer = Framer::new();
        let mut piecemeal = Vec::new();
        for byte in &framed {
            piecemeal.extend(framer.feed(&[*byte]).expect("valid"));
        }
        assert_eq!(piecemeal, whole);
        assert_eq!(data_of(&whole), body);
    }

    proptest! {
        /// `0x00` means 256, both ways: every length from 1 to 256 must survive framing and
        /// reframing byte for byte, and 256 is the only value that writes a `0x00` length byte.
        #[test]
        fn stx_length_byte_roundtrips_including_256(len in 1usize..=MAX_BLOCK) {
            let body = vec![0xAAu8; len];
            let framed = build_stx_block(&body);
            let mut framer = Framer::new();
            let frames = framer.feed(&framed).expect("valid block");
            prop_assert_eq!(data_of(&frames), body);
        }

        /// The wire fact the receiver checks, stated as the FBB document states it:
        /// `Σ data + eot_checksum ≡ 0 (mod 256)`, for bodies spanning one, two and three blocks.
        #[test]
        fn eot_checksum_is_twos_complement(body in proptest::collection::vec(any::<u8>(), 0..300)) {
            let sum = body.iter().fold(0u8, |a, &b| a.wrapping_add(b));
            let framed = build_stx_block(&body);
            let cksum = *framed.last().expect("non-empty");
            prop_assert_eq!(sum.wrapping_add(cksum), 0u8);
            prop_assert_eq!(data_of(&Framer::new().feed(&framed).expect("valid")), body);
        }

        /// A negative control on the checksum itself: a one-byte change anywhere from the first
        /// `STX` to the checksum must never deliver the original body. A checksum that ignored
        /// its input, or that summed the wrong bytes, passes the two properties above and fails
        /// this one.
        ///
        /// The `SOH` header is deliberately outside the range: FBB does not checksum it, so a
        /// change there IS delivered unchanged, and that is the protocol rather than a defect.
        #[test]
        fn any_single_byte_corruption_after_the_header_is_refused(
            body in proptest::collection::vec(any::<u8>(), 1..80),
            offset in 0usize..4096,
            mask in 1u8..=255,
        ) {
            let mut framed = build_stx_block(&body);
            let start = 2 + TEST_HEADER.len();
            let index = start + offset % (framed.len() - start);
            framed[index] ^= mask;
            let mut framer = Framer::new();
            // A corrupted length or marker byte leaves the record incomplete or unframeable
            // rather than merely corrupt — an empty delivery or a refusal. Either way the
            // original body must not come out the other side.
            let delivered = framer.feed(&framed).unwrap_or_default();
            prop_assert!(
                delivered.is_empty() || data_of(&delivered) != body,
                "a one-byte change at {} delivered the original body", index
            );
        }
    }
}
