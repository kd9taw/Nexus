//! The B2F session engine — `Session::feed(&[u8]) -> Vec<Action>`.
//!
//! The whole handshake as one accumulate-and-drain state machine, in the shape
//! `tempo_net::cluster` and `tempo_net::aprsis` already use: bytes in, actions out, an internal
//! buffer holding whatever partial unit the last chunk ended mid-way through. SID exchange →
//! secure login → FC proposals → FS answers → SOH/STX/EOT transfer → LZHUF decompress → parsed
//! message, driven entirely by what the buffer now holds.
//!
//! **The session never learns which transport it is on.** It has no socket, no clock and no
//! filesystem: telnet to a CMS and ARDOP over the air feed it the identical bytes, which is why
//! the telnet path can settle the undecidable wire questions with no radio involved, and why the
//! entire handshake unit-tests from two `Vec<u8>`.
//!
//! **Bytes, never `String`** — [`super`]'s rule, inherited from `tempo_net::aprsis`'s module
//! header. Every line here is a `&[u8]`, and the only place text is manufactured is
//! [`Action::Trace`], which is a log line and nothing else: a `Trace` is never parsed, compared,
//! or written back to the wire, so its lossy UTF-8 rendering cannot corrupt anything.
//!
//! # The wire, in the order it happens
//!
//! ```text
//! <  [WL2K-5.0-B2FWIHJM$]            the peer announces itself
//! <  ;PQ: 41913235                   the secure-login challenge
//! >  ;FW: N0CALL                     |
//! >  [Nexus-1.0-B2FHM$]              | one block: who we forward for, who we are,
//! >  ;PR: 74706169                   | the challenge answered, the identity line,
//! >  ; WL2K DE N0CALL                | and "nothing to propose"
//! >  FF                              |
//! <  FC EM ABCDEFGHIJKL 598 365 0    the peer offers a message
//! <  F> 76                           and closes the block with its checksum
//! >  FS +                            one answer per proposal, in order: send it
//! <  <SOH>…<STX>…<EOT><cks>          the body, binary-framed, LZHUF-compressed
//! <  FF                              nothing more from the peer
//! >  FQ                              nor from us: end the session
//! ```
//!
//! # Six readings, not citations
//!
//! B2F's published grammar does not settle everything a state machine has to decide. As in
//! [`super::sid`] and [`super::fbb`], each decision below is confined to one small function so a
//! live connect can settle it by editing that function and nothing else. **None of them is
//! settled by fixture #1** — that transcript is constructed, so it agrees with whatever this file
//! does. Fixture #2, captured at the first live CMS connect, is the oracle.
//!
//! 1. **When the peer's greeting is over** ([`Session::greeting_line`]). There is no
//!    end-of-greeting marker, and no clock here to notice the peer has gone quiet. The rule is
//!    therefore: the greeting ends on whichever of *the SID* and *the `;PQ:` challenge* arrives
//!    **second**, or on the first FBB command line after a SID. That is order-independent, which
//!    matters because the two published orderings disagree about whether a CMS sends its SID or
//!    its challenge first, and `greeting_order_does_not_change_the_session` pins it.
//!    ⚠️ A peer that sends a SID, never challenges, and then waits silently would hang this
//!    session. No CMS or RMS gateway does that (both challenge); the ARDOP P2P peer that could is
//!    Batch 11's, and the fix there is one condition in this function.
//! 2. **What the challenge *is*** ([`pq_challenge`]). `;PQ:` then the token, ASCII-whitespace
//!    trimmed off both ends. [`super::secure`] warns that any normalisation on the way into the
//!    digest is a silent wrong answer, so this is the one place that decides, and it decides the
//!    minimum: the separating space after the colon is framing, everything else is the challenge.
//! 3. **The `F>` block is summed over the bytes as received** ([`Session::handle_proposal`]).
//!    B2F terminates lines with a bare CR, and the sum is over the `FC` lines including that CR.
//!    A peer that sent CRLF would have its LFs excluded here. ⚠️ Whether such a peer includes them
//!    in its own sum is undecidable from the documents and no observed peer sends CRLF in a
//!    proposal block — but this is the first thing to look at if a live connect fails on
//!    [`fbb::FbbError::ProposalChecksum`].
//! 4. **Who the identity line names** ([`Session::greeting_block`]). `; <target> DE <call>` is an
//!    FBB *comment* line: the far end ignores it, and identity is really enforced by `;FW:`, the
//!    SID and the CMS's own account check (spec §2.8). Its target is taken from the peer's own SID
//!    product field — `WL2K` for a CMS — rather than from an invented constant, so the line says
//!    what the peer actually called itself.
//! 5. **What our SID advertises** ([`SID_FLAGS`]). See that constant.
//! 6. **What a bracketed line means once the greeting is over** ([`Session::handle_sid`]). Every
//!    `[`-leading line is a SID candidate, and one that does not parse is fatal **only until our
//!    own greeting block has gone out**. The two halves are decided differently on purpose.
//!    *Before* it, a bracketed line is the one place the peer's SID can appear, so refusing there
//!    is what puts the fault where it is still legible — the lenient alternative leaves the
//!    session waiting for a SID that already arrived garbled, and it dies later as "FBB command
//!    before the peer's SID", naming a line that was never the problem. *After* it, the SID has
//!    been accepted and the peer is a peer: a CMS puts banner and MOTD text on this same stream,
//!    so a second bracketed line is text, and failing the session over it — with mail still
//!    outstanding and the transport told to close — is exactly the forward-compatibility trap
//!    [`super::sid`] documents at length and the `_ =>` arm of [`Session::handle_line`] already
//!    refuses to fall into. Code and comment disagreed here until this reading was written down.
//!    ⚠️ A peer that puts a *bracketed banner* line before its SID is still refused by this rule.
//!    No observed CMS does, and if one does, the fix is one condition in this function: trace the
//!    unparseable line in both states and let reading 1 close the greeting.
//!
//! # What bounds memory, and what bounds the protocol
//!
//! This stream is remote-controlled — the transport underneath is a socket to a CMS that may be
//! buggy, MITM'd or hostile — and every buffer in [`Session`] fills from it. Three review rounds
//! each found one of those buffers growing without a limit and closed it with a cap of its own,
//! which is the shape that finds the fourth one the same way. The memory bound is therefore not a
//! cap per buffer.
//!
//! **One bound, at the door.** [`Session::feed`] is the only way a peer byte enters this type, so
//! that is where it goes: [`Session::held_bytes`] — everything the session is holding on the
//! peer's behalf, over every field it owns — is checked against [`MAX_SESSION_HELD_BYTES`] on
//! every pass of `feed`'s drain loop, and over it the session fails and consumes nothing further.
//!
//! What makes that cover buffers nobody has written yet: [`Session::held_bytes`] destructures
//! `Session` **exhaustively, with no `..`**, so a field added to the struct does not compile until
//! that function says how many peer bytes it holds. The next buffer is bounded because it cannot
//! be added silently — not because someone notices it in review.
//!
//! ## The number is retained heap, not bytes off the wire
//!
//! Those two are different numbers, and a round that bounded the second believed it had bounded
//! the first. The peer's cost to this process is the memory this process keeps, and three things
//! put memory outside a count of peer data bytes:
//!
//! * **Containers cost more than their contents.** A [`fbb::Frame`] holding one payload byte costs
//!   `size_of::<Frame>()` in the framer's vector plus an allocation of its own, and the peer picks
//!   the block size that decides how many there are. A transfer at [`MAX_PROPOSAL_C_SIZE`] framed
//!   in 8-byte blocks is 1,310,785 wire bytes and charges 5,245,207 — four times its own volume
//!   (measured 2026-09-07). The *factor* is asserted rather than left to prose:
//!   `the_door_charges_the_frames_a_peer_makes_us_hold_not_its_data_bytes` requires the door to
//!   refuse a one-byte-block flood before a quarter of the ceiling has arrived on the wire, which
//!   is a charge over 4× the volume. So every term here is a `capacity()` and never a `len()` — a
//!   drained `Vec` still owns its buffer — and every collection is charged its per-element
//!   overhead as well as its elements' own heap.
//! * **A check after an allocation is not a bound.** The peer chooses `u-size`, and this coder
//!   expands by up to `F` bytes per symbol: measured, its best case is 47.9× and a 120,000-byte
//!   image of a long run unpacks to 5,751,076 bytes. So a proposal well inside the compressed cap
//!   can name tens of megabytes of plaintext, and decompressing first hands that allocation over
//!   before the comparison that would have refused it. Every peer-chosen size is judged before the
//!   memory exists instead: [`MAX_PROPOSAL_U_SIZE`] at proposal time, and
//!   [`lzhuf::decompress_bounded`] refusing an image whose own header declares more than the
//!   proposal did.
//! * **Giving up has to release.** A session that has failed still owns every buffer it filled
//!   unless it drops them, and `clear()` does not: [`Session::close`] replaces them.
//!
//! Two properties of the accounting are deliberate. It **over-states rather than measures**: a
//! held [`Proposal`] is charged [`PROPOSAL_HELD_BYTES`], the most one can hold rather than what it
//! does hold. And every term is **O(1)**, which is the reason for the first: an accounting that
//! walked the proposal vector on every line would be quadratic in the length of a block the peer
//! chooses, which turns a memory bound into a CPU one. [`fbb::Framer`] keeps running counters for
//! the same reason.
//!
//! ⚠️ **What the number bounds is what the session *retains* when the check next runs. The
//! transient of delivering a message is larger, and nothing in this file bounds it.** At the
//! instant a transfer completes, the compressed `image`, the `plain` it unpacks to and the parsed
//! [`Message`] are all live at once, and the check does not run between them. Two revisions of
//! this paragraph gave that transient an arithmetic bound — [`MAX_PROPOSAL_C_SIZE`] plus twice
//! [`MAX_PROPOSAL_U_SIZE`], 17.0 MiB — and both were one message *shape* generalised. Measured
//! 2026-09-07, requested bytes, a counting allocator on the current code:
//!
//! * 8,388,608 plaintext bytes as one flat body peak at 17,086,031 (16.3 MiB), **2.04×** the
//!   plaintext. That is the shape the 17 MiB arithmetic was read off.
//! * 8,304,746 plaintext bytes as 1,384,119 header lines — image 173,091, inside every ceiling
//!   here, and `FS +` is the right answer to it — peak at 161,707,585 (154.2 MiB), **19.5×** the
//!   plaintext and **9.1× the bound this paragraph used to state**.
//!
//! The term that scales is the parsed [`Message`], not the session: `A: B\r\n` is six plaintext
//! bytes and becomes a `(Vec<u8>, Vec<u8>)` in a vector, with two allocations of its own. **That
//! is [`super::message`]'s to bound and it is being tracked as its own piece of work** — the
//! numbers here exist so that nobody re-derives the 17 MiB and believes it. What this file
//! guarantees is the *retained* figure, and the ceiling covers that.
//!
//! **The per-site limits that remain are protocol correctness, not the memory story.** Both would
//! still be here if memory were free, and neither is what keeps this session finite:
//!
//! * [`Session::transfer_ceiling`] — a transfer may not run past the compressed size its own `FC`
//!   line declared, because a peer that streams `STX` blocks past it is no longer sending the
//!   message it proposed. Exceeding it is [`SessionError::Protocol`] and never a truncation: a
//!   truncated body would be a *shorter* message delivered as if it were the whole one.
//! * [`MAX_PROPOSAL_C_SIZE`] and [`MAX_PROPOSAL_U_SIZE`] — a proposal declaring more compressed
//!   bytes than a Winlink account may hold, or more uncompressed bytes than that could plausibly
//!   unpack to, is not a message that could have reached a CMS to be forwarded to us. Both are
//!   refused at **proposal** time, answered `-` with the reason traced, so the outcome is a
//!   legible refusal of one message rather than a session killed mid-stream. The second is also
//!   the bound on the largest allocation this file makes, which is why it is checked one line
//!   before the transfer is accepted rather than one line after it is decompressed.
//!
//! # What a legitimate peer needs, and where the budget comes from
//!
//! FBB bounds a proposal block at the *sender*: "A proposal can handle up to five FB command
//! lines", and the block is sized by "a parameter \[...\] in INIT.SRV file to tell the maximum
//! size of the message block. It is set by default to 10KB"
//! (<https://www.f6fbb.org/fbbdoc/docfwpro.htm>, read 2026-09-07). ARSFI's B2F document states no
//! count of its own, only that "All of the parts of up to five messages are combined into a single
//! 'file' that is then compressed as a unit before transmission" (<https://winlink.org/B2F>, read
//! 2026-09-07).
//!
//! ⚠️ **Five is what a legitimate peer sends, not a number to refuse a session over.** It is a
//! sender's rule in a sender's document, and the reference implementation of this protocol —
//! `wl2k-go`, the library Pat is built on — reads it the same way: it caps its **own sender** at
//! five, truncating the block in `Session.sendOutbound` (<https://github.com/la5nta/wl2k-go>, HEAD
//! `efde6fbc`, `fbb/b2f.go:26` `MaxBlockSize = 5`, applied at `fbb/b2f.go:112`), and enforces no
//! count at all on **receive** — its `case "FA", "FB", "FC", "FD"` arm appends (`fbb/b2f.go:233`)
//! and `writeProposalsAnswer` (`fbb/b2f.go:304`) has no length test. All four line numbers read
//! 2026-09-07. ⚠️ An earlier revision of this paragraph said "enforces no proposal count in either
//! direction" and cited `fbb/wl2k.go` and `fbb/handshake.go`: both halves were wrong — the count
//! exists on the sending side, and neither named file mentions it (`grep -n
//! 'MaxBlockSize\|len(proposals)' fbb/wl2k.go fbb/handshake.go` is empty). The design conclusion
//! is unchanged and is better argued by the correct facts: senders cap at five, receivers cap at
//! nothing. Made a cap *here*, it would be [`super::fbb`]'s trap 1 in a new costume — a receiver
//! refusing a block for being longer than one document's example destroys mail the peer then stops
//! offering. So the number sizes the budget instead of becoming a limit.
//!
//! The largest thing a legitimate session holds is one transfer in flight, and what that costs
//! depends on how the sender frames it. Measured 2026-09-07, on a message whose image is exactly
//! [`MAX_PROPOSAL_C_SIZE`] — 1,048,576 bytes — delivered end to end:
//!
//! | block size | charged at the peak | of the ceiling |
//! |---|---|---|
//! | 256 ([`super::fbb`]'s maximum) | 1,182,003 | 14.1 % |
//! | 125 (`wl2k-go`'s sender) | 1,574,994 | 18.8 % |
//! | 16 | 3,147,867 | 37.5 % |
//! | 8 | 5,245,207 | 62.5 % |
//! | 7 | refused at the door | — |
//!
//! Beside that sits the block's bookkeeping: five proposals are **832** charged bytes — the
//! vector's `capacity()` of 8 times [`PROPOSAL_HELD_BYTES`], not its `len()` of 5, which is this
//! section's own rule applied to itself — and an open FBB-legal block of five charges 1,278 all
//! told. Both are asserted by `the_session_ceiling_bounds_every_buffer_at_the_door`. The worst
//! *legitimate* session measured is five cap-sized messages at `wl2k-go`'s framing through a
//! 64 KiB reader: **1,703,188 charged bytes, 20.3 % of the ceiling, 4.93× headroom**. Erring
//! generous is deliberate: a few unnecessary MiB cost the operator nothing they can see, and
//! refusing legitimate mail is the one outcome an email transport may not have.
//!
//! ⚠️ **What this ceiling refuses that it used to carry** is a transfer *at the compressed cap*
//! framed in blocks of seven bytes or fewer, because that framing costs more `Frame` slots than
//! the message has payload. Eight bytes is the smallest that still delivers there, and
//! `a_transfer_at_the_compressed_ceiling_is_carried_down_to_eight_byte_blocks` asserts both sides
//! of that boundary so it cannot drift in prose again. **It is a property of the transfer and not
//! of the framing alone**: the charge scales with the message, so at Winlink's own published
//! 120,000-byte maximum — the largest that can actually reach a CMS mailbox — every legal block
//! size **down to one byte** delivers, charging 5,156,288 (61.5 %). That is
//! `a_message_at_winlinks_published_maximum_is_carried_at_every_legal_block_size`, and it is why
//! no real mail is at stake in the paragraph above: the refusal band lives entirely inside the
//! 8.7× of compressed headroom [`MAX_PROPOSAL_C_SIZE`] holds against a future Winlink limit, at
//! block sizes fifteen times smaller than the smallest any implementation sends.
//!
//! ## Which numbers here are test-backed
//!
//! Everything above is either asserted by a named test or labelled as a dated measurement, and
//! the distinction is the point: four review rounds each corrected a figure in this header and
//! wrote a new one, so a figure worth stating precisely is now worth an assertion. **Asserted**,
//! and therefore red if it stops being true: 832 and 1,278; the 4× charge-to-wire factor;
//! delivery at 8-byte blocks and refusal at 7 at the compressed cap; delivery at every block size
//! down to one byte at Winlink's 120,000; acceptance at 120,000 compressed and at 5,751,076
//! uncompressed; that a closed session holds zero. **Measured on 2026-09-07 and not asserted**
//! (reproduce before relying on one): every charged-byte figure quoted above, including the table
//! and the 1,703,188 worst legitimate session; the 47.9× best compression ratio, which is where
//! the 5,751,076 comes from; and the two live-heap transients. Each was taken in a debug build on
//! this machine with a private `CARGO_TARGET_DIR`, which is the second half of reproducing one.
//!
//! # What this engine does not do yet
//!
//! It **proposes nothing**: its greeting block ends in `FF` and it answers every inbound `FF` with
//! `FQ`. Outbound mail arrives with the mailbox (Task 2.9) and the composer above it; the seam is
//! the `have` callback in [`Session::close_proposal_block`], which today answers
//! [`fbb::HaveState::No`] for every MID because there is no store to ask. That is a stub with a
//! name, not a silent gap: when the mailbox lands, one closure changes and the `!offset` resume
//! the `FS` table already implements starts working.

use std::collections::VecDeque;

use super::fbb::{self, FbbError, Frame, Framer, HaveState, Proposal};
use super::lzhuf;
use super::message::{self, Message};
use super::secure;
use super::sid::{self, Sid};
use super::ClientConfig;

/// The product field of our SID. Deliberately not the app's name-plus-version: nothing in B2F
/// branches on it, and it exists so an operator reading another client's log can tell who they
/// were talking to.
pub const SID_PRODUCT: &[u8] = b"Nexus";

/// The version field of our SID — **this protocol implementation's version, not the app's
/// release number.** Wiring `CARGO_PKG_VERSION` in here would put the release number on the wire
/// at the cost of breaking the golden transcript on every release, for a field no peer reads.
/// It moves when the B2F implementation's behaviour moves, and then fixture #1 moves with it.
pub const SID_VERSION: &[u8] = b"1.0";

/// The flag letters our SID advertises: `B2` (message format) + `F` (FBB forwarding) + `H` + `M`.
///
/// **Reading 5.** `B2` and `F` are the two capabilities this implementation actually requires and
/// implements — [`super::sid`] refuses a peer that lacks either. `H` and `M` are carried because
/// this is verbatim the flag set RMS Express advertises (`[RMS Express-1.5.45.0-B2FHM$]`), and
/// RMS Express is the client the CMS is built and tested against, which is the strongest interop
/// evidence available offline. ⚠️ Their exact meanings are not established from the documents
/// available here, so this constant advertises two letters whose semantics we cannot state. If a
/// live CMS ever behaves as though we claimed something we do not do, narrowing this to `B2F` is
/// the first experiment and touches nothing else.
pub const SID_FLAGS: &[u8] = b"B2FHM";

/// Longest FBB command line this session will hold in order to interpret it, in bytes. A line
/// over it is **discarded, not fatal**. Matching `aprsis`'s bound.
///
/// Every line in the grammar is short. Measured, the longest a peer sends is its `;FW:`
/// forwarding list, and twenty callsigns of it are 164 bytes; a CMS SID is 20, a `;PQ:` challenge
/// 13, and an `FC` line carrying a MID at [`MAX_MID`] is 78.
/// `an_over_long_line_is_discarded_however_it_arrives` carries all four and asserts they are
/// interpreted, so shrinking this constant under any real line reddens the gate rather than
/// silently dropping mail.
///
/// **Discarded and traced, because a CMS puts banner and MOTD text on this same stream.**
/// [`Session::handle_line`]'s last arm exists precisely so that a line we do not recognise cannot
/// end the session; a length check *above* that arm overrode it, and a review round shipped one —
/// a 512-byte MOTD line that arrived whole went from traced-and-ignored to session-fatal, which
/// is refusing a working server for saying hello. What this constant is for is memory, and a line
/// can be bounded without being fatal: the bytes are counted, dropped and traced, and the session
/// resumes at the next CR. ⚠️ The one line whose *meaning* that changes is an over-long `FC`:
/// dropping it leaves a hole in what [`fbb::fb_checksum`] covers, so its block fails at the `F>`
/// rather than quietly proposing one message fewer.
///
/// **Applied to the line, not only to a CR-less buffer.** It was once the second: a line longer
/// than this was refused when its CR had not arrived yet and accepted when it had, so the same
/// wire passed or failed on where the caller's read boundaries happened to land. The rule here
/// makes the outcome independent of chunking, which everything else in this file already is.
///
/// ⚠️ **It is not what bounds [`PROPOSAL_HELD_BYTES`]**, whatever an earlier revision of this doc
/// claimed. [`MAX_MID`] at [`Session::handle_proposal`] is, and a mutation says so: revert this
/// rule to the CR-less-only one and
/// `a_proposal_whose_mid_is_over_the_maximum_is_deferred_not_retained` stays green — only the line
/// test reddens. This is defence in depth over one buffer, not the bound on a retained field.
const MAX_LINE: usize = 512;

/// Longest MID this session will retain from a proposal, in bytes.
///
/// Winlink MIDs are twelve characters — `wl2k-go` has `MaxMIDLength = 12` (`fbb/mid.go:14`) and
/// annotates the field it writes as "Max 12 characters" (`fbb/b2f.go:120`), both read 2026-09-07 —
/// so this is generous by 5×. It is here because
/// [`PROPOSAL_HELD_BYTES`] is a *flat* charge: a per-proposal cost that did not bound the one
/// variable-length field a [`Proposal`] owns would be a number, not a bound. A longer MID is
/// **deferred**, not rejected — [`Session::handle_proposal`]'s existing not-understood path, for
/// [`super::fbb`]'s trap 1 reason: `-` tells a peer to stop offering a message, and something we
/// declined to read is not something we know is unwanted.
const MAX_MID: usize = 64;

/// The most bytes this session will hold on the peer's behalf, over every buffer it owns.
///
/// **The memory bound, and the only one** — see the module header for why it is one accounting at
/// [`Session::feed`] rather than a cap per buffer, and for the arithmetic that makes 8 MiB
/// generous: the worst transfer a proposal may legally open is just over 3 MiB, and a proposal
/// block costs kilobytes beside it.
///
/// **Retained heap, not bytes off the wire** — see the module header for why the distinction is
/// the whole point, and for the transient inside one drain that this number does not cover.
///
/// One consequence is worth stating where the constant is: because the charge is honest about
/// container overhead, a peer that streams a transfer in absurdly small blocks is refused here
/// rather than quietly costing several times what it appeared to. The block sizes real senders
/// use are 125 (`wl2k-go`, `fbb/b2f.go:30`, "Paclink-unix uses 250, protocol maximum is 255", read
/// 2026-09-07) up to [`super::fbb`]'s 256, and a transfer at [`MAX_PROPOSAL_C_SIZE`] is delivered
/// at every block size down to **8 bytes**; **7 and under are refused there**, with the trace
/// [`Session::feed`] emits rather than silently. Nothing sends those, and the boundary is a
/// property of the transfer size rather than of the framing — at Winlink's own 120,000-byte
/// maximum every block size down to one byte delivers. Both halves are asserted; see the module
/// header's test-backed list.
const MAX_SESSION_HELD_BYTES: usize = 8 * 1024 * 1024;

/// What one slot of [`Session::proposals`] or [`Session::accepted`] is charged against
/// [`MAX_SESSION_HELD_BYTES`] — the vector's own element plus the heap that element owns.
///
/// A `Proposal`'s fields are `kind: u8`, `u_size: u32`, `c_size: u32` and `mid: Vec<u8>`, so `mid`
/// is the only heap and [`MAX_MID`] is what bounds it. That bound is enforced where the proposal
/// is retained ([`Session::handle_proposal`]) rather than inferred from what some other function
/// would have refused — the previous reading of this constant depended on [`Session::drain_line`]
/// and was false by orders of magnitude, because that refusal only applied to a buffer with no CR
/// in it. `a_proposal_whose_mid_is_over_the_maximum_is_deferred_not_retained` is what holds the
/// charge up, and it stays green under every mutation of [`Session::drain_line`], which is the
/// evidence that the dependence really is gone rather than merely re-worded.
///
/// Charged flat, and multiplied by `capacity()` rather than `len()`, because
/// [`Session::held_bytes`] must be O(1): walking the vector on every line would make a block
/// quadratic in a length the peer picks, which is the same exhaustion by a different resource.
const PROPOSAL_HELD_BYTES: usize = MAX_MID + size_of::<Proposal>();

/// The largest compressed message size this station will accept a proposal for, in bytes.
///
/// **A protocol-correctness refusal, not the memory bound** (module header). A proposal declaring
/// more than a Winlink account may hold describes a message that could not have reached a CMS to
/// be forwarded to us, so it is refused for being invalid — and it would be refused here if memory
/// were free. What keeps this session's footprint finite is [`MAX_SESSION_HELD_BYTES`].
///
/// Winlink's own limit is **120,000 bytes compressed**: the per-account `MAX SIZE` option is
/// documented, verbatim, as "a numerical value setting the size in bytes (compressed) of the
/// largest message you will accept. 120000 Bytes is maximum, and the default."
/// (<https://winlink.org/content/how_change_your_account_settings_option_message_useroptions>,
/// read 2026-09-07). A message larger than that cannot reach a CMS mailbox, so it cannot
/// legitimately be proposed to us either.
///
/// This constant is **1 MiB, roughly nine times that**, and the headroom is deliberate. Winlink
/// raising its own limit must not turn into Nexus silently dropping messages, so this is sized to
/// absorb that without a release, and refusing real mail is the one outcome an email transport may
/// not have. ⚠️ The absorption is 8.7× only up to a compression ratio of 8: past that
/// [`MAX_PROPOSAL_U_SIZE`] is the constant that bites first, since 8 MiB of plaintext is what a
/// 1 MiB image at ratio 8 unpacks to. The cost of the generosity is bounded twice over:
/// [`Session::transfer_ceiling`] then sits at `3 * 1048576 + 260`, and
/// [`MAX_SESSION_HELD_BYTES`] holds whatever this is set to —
/// `a_transfer_at_the_compressed_ceiling_is_carried_down_to_eight_byte_blocks` is what asserts
/// that second half, at the framing a real sender uses and the smallest one that fits.
pub const MAX_PROPOSAL_C_SIZE: u32 = 1024 * 1024;

/// The largest **uncompressed** message size this station will accept a proposal for, in bytes.
///
/// [`MAX_PROPOSAL_C_SIZE`]'s pair, and the one that bounds an allocation rather than a buffer.
/// `u-size` is a peer-chosen `u32` and LZHUF expands by up to `F` (60) bytes per symbol, so
/// without this a proposal well inside the compressed cap — answered `FS +`, and delivered as a
/// perfectly valid message — decompresses into tens of megabytes before anything compares the
/// result to what the `FC` line promised. Measured 2026-09-07 on a long-run body: this coder's
/// best ratio is **47.9×**, so a 1 MiB image unpacks to about 50 MB. `super::lzhuf`'s own
/// `a_long_run_compresses_by_orders_of_magnitude` is the same shape at a smaller size.
///
/// Refused at **proposal** time for [`MAX_PROPOSAL_C_SIZE`]'s reason — a legible `-` for one
/// message beats a session killed mid-stream — which also puts the decision before the allocation
/// instead of after it. [`Session::complete_transfer`] then passes this proposal's own `u-size`
/// into [`lzhuf::decompress_bounded`], so the image's internal length header cannot exceed what
/// the `FC` line already promised and was already judged against this constant.
///
/// **8 MiB, eight times the compressed cap, and no message a CMS can hold can reach it.** Winlink's
/// own documented maximum is 120,000 bytes *compressed* (see [`MAX_PROPOSAL_C_SIZE`]), and the
/// most plaintext this coder can produce from an image that size is **5,751,076 bytes** — measured
/// 2026-09-07 by bisection on a long-run body, its own best case at 47.9×. That is 1.46× under
/// this ceiling, so the answer is not "it would have to expand improbably far" but "it cannot",
/// and `the_ceilings_admit_the_largest_message_a_cms_account_can_hold` asserts acceptance at that
/// pair of literal sizes rather than at whatever these constants happen to say. It is not a
/// compression-ratio limit and must not be read as one: it is a statement about the largest
/// plaintext this station will hold, which is the resource actually at stake.
pub const MAX_PROPOSAL_U_SIZE: u32 = 8 * 1024 * 1024;

/// The `kind` given to a proposal line that could not be read at all.
///
/// Any byte that is not [`fbb::PROPOSAL_CODE_FC`] makes [`fbb::fs_answer`] answer `=` — defer,
/// never reject — which is that module's trap 1: `-` tells the peer the message is *answered* and
/// it stops offering it, so rejecting something we merely failed to parse destroys mail. This byte
/// never reaches the wire; it exists only to select the deferring branch.
const UNREADABLE_PROPOSAL: u8 = b'?';

/// What the session wants done, in the order it wants it done.
///
/// Deliberately not `Result`-shaped. A failure is an [`Action::Failed`] *in the stream* rather
/// than a return type, so the actions produced before the failure survive it and their order
/// relative to the failure is preserved — which is exactly what the second negative control has
/// to see: a corrupt EOT must produce a failure **and no [`Action::Received`] anywhere before
/// it**, and that is one assertion over one vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Bytes to write to the transport, verbatim.
    Send(Vec<u8>),
    /// A fully received, checksum-verified, decompressed and parsed inbound message.
    Received(Message),
    /// A human-readable protocol event, for the log and the telemetry pane. Display only: nothing
    /// parses a `Trace`, and its bytes-to-text rendering is lossy on purpose.
    Trace(String),
    /// The session is complete; the transport may close.
    Done,
    /// The session failed and will consume nothing further. The transport may close.
    Failed(SessionError),
}

/// Why a B2F session failed.
///
/// Four of the five variants are another module's error carried through unchanged, because the
/// session is not a better judge of an FBB checksum than [`super::fbb`] is, and because the
/// negative controls name those errors specifically. The fifth is for the desyncs no sub-parser
/// can see, each carrying a fixed string rather than a formatted one so the type stays `Copy`
/// and a caller can match on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// The peer's SID was malformed, or lacked `F` or `B2`. The session sends nothing at all in
    /// this case — see [`Session::handle_sid`].
    Sid(sid::SidError),
    /// An FBB fault: the `F>` proposal-block checksum, an EOT checksum, or unframeable bytes in
    /// the binary phase.
    Fbb(FbbError),
    /// A received body did not decompress.
    Lzhuf(lzhuf::LzhufError),
    /// A body decompressed but is not a B2 message.
    Message(message::MessageError),
    /// A desync in the session grammar itself: the sizes a transfer arrived at disagree with the
    /// proposal that offered it, a block closed with no proposals in it, or the stream stopped
    /// looking like FBB command lines.
    Protocol(&'static str),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Sid(err) => write!(f, "SID refused: {err}"),
            SessionError::Fbb(err) => write!(f, "FBB fault: {err:?}"),
            SessionError::Lzhuf(err) => write!(f, "body did not decompress: {err:?}"),
            SessionError::Message(err) => write!(f, "body is not a B2 message: {err:?}"),
            SessionError::Protocol(why) => write!(f, "B2F desync: {why}"),
        }
    }
}

impl std::error::Error for SessionError {}

/// Which side of the forwarding session this is.
///
/// One variant today, and it is not a placeholder: everything below assumes the *calling* side —
/// we greet second, we answer proposals, we close with `FQ`. A `Role::Server` would invert all
/// three, which is why the role is a parameter rather than an assumption baked into the flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The station that placed the call: to a CMS over telnet, or to an RMS gateway over ARDOP.
    Client,
}

/// Where in the handshake the session is. Private: a caller drives the session with bytes and
/// reads [`Action`]s, and exposing this would invite branching on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Reading the peer's opening lines; our block has not gone out.
    Greeting,
    /// Line-oriented FBB command mode — proposals, answers, turn control.
    Line,
    /// The binary SOH/STX/EOT phase, for the bodies this session accepted.
    Binary,
    /// Done or failed. Consumes nothing further.
    Closed,
}

/// The B2F session machine.
///
/// **Derives no `Debug`, for [`ClientConfig`]'s reason:** it holds the account password for as
/// long as the session lasts, because a `;PQ:` challenge can arrive after the greeting and has to
/// be answerable then. A derived `Debug` would put that password into every log line and
/// `assert_eq!` failure that happened to carry a session.
pub struct Session {
    /// This station's callsign, as it goes into `;FW:` and the identity line.
    callsign: Vec<u8>,
    /// The account password, bytes. Fed to [`secure::pr_response`] and nowhere else.
    password: Vec<u8>,
    /// Which side we are. Read into the opening trace; see [`Role`].
    role: Role,
    state: State,
    /// Wire bytes not yet consumed by a complete line or binary record.
    ///
    /// A `VecDeque` and not a `Vec`, for one reason: everything here consumes from the **front**,
    /// and `Vec::drain(..n)` memmoves the tail behind it. One line at a time out of a `Vec` made a
    /// single `feed` quadratic in its chunk — measured at roughly 4× the time for 2× the chunk of
    /// short lines, and linear after — because the peer picks the line length and so picks how
    /// many memmoves its bytes cost. (The shape is the claim; the absolute milliseconds are not
    /// restated here, because they moved by 8× between two machines measuring the same run.) A
    /// ring buffer's front drain moves the head and nothing else, which makes the same work linear
    /// without a cursor field to keep in step.
    buf: VecDeque<u8>,
    /// How many bytes of an over-long line have been dropped so far; `0` when the session is not
    /// inside one. Carrying it across calls is what makes [`MAX_LINE`]'s discard independent of
    /// where the caller's read boundaries fall: a line too long to interpret is dropped to the
    /// next CR however many `feed`s it takes to get there. It counts bytes already gone, so it
    /// holds nothing and [`Session::held_bytes`] charges it nothing.
    dropping_line: usize,
    /// The peer's SID once it has arrived and been accepted.
    peer_sid: Option<Sid>,
    /// The `;PQ:` challenge value, if the peer has issued one.
    challenge: Option<Vec<u8>>,
    /// Whether our greeting block has gone out. Guards the late-challenge path.
    greeted: bool,
    /// The raw bytes of the proposal lines in the block currently open, as received — the input
    /// to [`fbb::fb_checksum`]. See reading 3.
    block: Vec<u8>,
    /// The proposals in the block currently open, in the order proposed.
    proposals: Vec<Proposal>,
    /// The proposals we answered "send it" to, in order — one is retired per completed transfer.
    accepted: VecDeque<Proposal>,
    /// The SOH/STX/EOT reassembler for the binary phase.
    framer: Framer,
    /// The compressed body of the transfer in flight, accumulated across its STX blocks.
    image: Vec<u8>,
    /// Wire bytes fed to the framer since the transfer in flight opened, against
    /// [`Session::transfer_ceiling`]. Counted here rather than in [`Framer`] because the number
    /// that bounds it — the proposal's `c-size` — is held here; see the module header.
    ///
    /// ⚠️ **Wire bytes, so it is not a memory term** and [`Session::held_bytes`] does not charge
    /// it. It used to stand in for `image` and `framer` on the argument that every byte in either
    /// was counted through here — true of the bytes, false of the memory, and false several times
    /// over once the peer chose one-byte blocks. Both are now charged for what they hold.
    binary_bytes: u64,
}

impl Session {
    /// A session that has not yet seen a byte from the peer.
    ///
    /// Emits nothing: unlike `aprsis`, whose client speaks first, a B2F caller waits for the
    /// peer's SID — it cannot answer a challenge it has not been given, and it must not send its
    /// callsign to a peer whose SID it has not accepted (see `sid_without_b2_refuses`).
    pub fn new(cfg: &ClientConfig, role: Role) -> Self {
        Session {
            callsign: cfg.callsign.as_bytes().to_vec(),
            password: cfg.password.as_bytes().to_vec(),
            role,
            state: State::Greeting,
            buf: VecDeque::new(),
            dropping_line: 0,
            peer_sid: None,
            challenge: None,
            greeted: false,
            block: Vec::new(),
            proposals: Vec::new(),
            accepted: VecDeque::new(),
            framer: Framer::new(),
            image: Vec::new(),
            binary_bytes: 0,
        }
    }

    /// Feeds received bytes; returns every action the stream has now produced, in wire order.
    ///
    /// Chunk boundaries are irrelevant by construction — whatever does not complete a unit stays
    /// in [`buf`](Session::buf) — and that is asserted rather than asserted-to: the golden
    /// transcript replays whole, one byte at a time, and in 64 seeded random splits, and all three
    /// must produce the identical action stream.
    ///
    /// **This is the door, so the memory bound is here** (module header): every peer byte this
    /// session will ever hold arrives through this function, and [`Session::held_bytes`] is
    /// checked against [`MAX_SESSION_HELD_BYTES`] once per pass of the loop below — which is after
    /// every unit that can grow a buffer, because a drain that grew nothing does not loop again.
    ///
    /// The chunk is charged **before it is copied in**, so the ceiling doubles as the largest
    /// chunk a caller may hand this session. Checking only after the copy left the one hole the
    /// door could not see: a 64 MiB chunk with no line terminator in it peaked at 128 MiB — the
    /// caller's copy and ours — and *then* refused it. Nothing today reads that big (the sibling
    /// socket readers in `tempo_net` read 4 KiB), so this is the contract being true rather than a
    /// behaviour change anyone can observe.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Action> {
        let mut out = Vec::new();
        if self.state == State::Closed {
            return out;
        }
        if self.refuse_over_ceiling(self.held_bytes().saturating_add(chunk.len()), &mut out) {
            return out;
        }
        self.buf.extend(chunk);
        // Alternates between the two readers until neither can make progress: a line can put the
        // session into the binary phase and a completed transfer can put it back, both in the
        // middle of one chunk.
        loop {
            if self.refuse_over_ceiling(self.held_bytes(), &mut out) {
                return out;
            }
            let progressed = match self.state {
                State::Closed => return out,
                State::Binary => self.drain_binary(&mut out),
                State::Greeting | State::Line => self.drain_line(&mut out),
            };
            if !progressed {
                return out;
            }
        }
    }

    /// Fails the session when `held` is over the ceiling, and says so. Returns whether it did.
    ///
    /// One function because there are two call sites — the incoming chunk and the drain loop — and
    /// they must refuse the same way: a second copy of this would be the place a later edit
    /// changed one message, or one ceiling, and not the other.
    fn refuse_over_ceiling(&mut self, held: usize, out: &mut Vec<Action>) -> bool {
        if held <= MAX_SESSION_HELD_BYTES {
            return false;
        }
        // Traced as well as failed: the failure is a fixed string (`SessionError` is `Copy`), and
        // the number is the part an operator reading the log needs.
        out.push(Action::Trace(format!(
            "refusing the peer: {held} bytes held on its behalf, over this session's \
             {MAX_SESSION_HELD_BYTES}-byte ceiling"
        )));
        self.fail(
            out,
            SessionError::Protocol(
                "the peer made this session hold more than any forwarding block needs: FBB \
                 proposes at most five messages per block and transfers them one at a time",
            ),
        );
        true
    }

    /// Everything this session is holding on the peer's behalf, in charged bytes.
    ///
    /// **Exhaustive on purpose: no `..`, and every field named.** That is the whole mechanism by
    /// which [`MAX_SESSION_HELD_BYTES`] covers a buffer nobody has added yet — a new field on
    /// [`Session`] does not compile until this function says what it holds, so the fourth
    /// unbounded `Vec` cannot be introduced quietly. Do not replace the pattern with `self.buf`
    /// and friends, however much shorter it reads.
    ///
    /// **`capacity()`, never `len()`, and every container charged its own overhead.** That is the
    /// module header's first point at the site that has to obey it: a `Vec` that grew to a
    /// megabyte and was drained still owns the megabyte, and a vector of a million one-byte frames
    /// costs `size_of::<Frame>()` a million times over before any payload is counted. Charging
    /// data bytes instead understated this session's real cost several times over — measured at
    /// 4× for a cap-sized transfer in 8-byte blocks, and
    /// `the_door_charges_the_frames_a_peer_makes_us_hold_not_its_data_bytes` asserts a factor of
    /// at least four on the flood it refuses.
    ///
    /// Every term is an upper bound and every term is O(1); see the module header for why both
    /// matter. Ours-not-theirs fields charge nothing, and that is a claim about the field, not an
    /// exemption: [`Session::callsign`] and [`Session::password`] come from [`ClientConfig`], and
    /// the scalars hold no bytes at all.
    ///
    /// ⚠️ What the compiler enforces is a **decision, not a correct answer**. Adding a field is
    /// `error[E0027]: pattern does not mention field` — measured, by adding a `Vec<u8>` to
    /// [`Session`] and watching `cargo check -p tempo-core` refuse to build it — but rustc's own
    /// suggested fix is `<field>: _`, which silences it while charging nothing. That is the
    /// remaining hole and it is a human one: the pattern makes ignoring a buffer something someone
    /// had to type, which is all a compiler can do here.
    fn held_bytes(&self) -> usize {
        let Session {
            // Ours, from `ClientConfig` — the peer never writes these.
            callsign: _,
            password: _,
            // Scalars: no buffer behind them.
            role: _,
            state: _,
            greeted: _,
            // Peer bytes not yet consumed by a line or a binary record.
            buf,
            // A count of bytes already dropped, not bytes held: see the field.
            dropping_line: _,
            // The peer's SID line, kept in three pieces.
            peer_sid,
            // The `;PQ:` token as it arrived. Replaced, never appended to.
            challenge,
            // The proposal block open right now, as received.
            block,
            // Parsed from `block`'s lines, and outliving it in `accepted`.
            proposals,
            accepted,
            // The binary phase, charged for what each holds. `framer` answers for itself because
            // its per-frame overhead is the expensive half and only it can total that in O(1).
            framer,
            image,
            // Wire bytes against `transfer_ceiling`, not a buffer: see the field.
            binary_bytes: _,
        } = self;
        buf.capacity()
            + peer_sid.as_ref().map_or(0, |sid| {
                size_of::<Sid>()
                    + sid.product.capacity()
                    + sid.version.capacity()
                    + sid.flags.capacity()
            })
            + challenge.as_ref().map_or(0, Vec::capacity)
            + block.capacity()
            + (proposals.capacity() + accepted.capacity()) * PROPOSAL_HELD_BYTES
            + framer.held_bytes()
            + image.capacity()
    }

    /// Whether the transport may close: the session is finished, or has failed and will consume
    /// nothing further.
    pub fn wants_close(&self) -> bool {
        self.state == State::Closed
    }

    // -- line phase ---------------------------------------------------------------------------

    /// Consumes one complete CR-terminated line, if the buffer holds one. Returns whether it did.
    ///
    /// [`MAX_LINE`] is applied to the line itself, not only to a buffer with no CR in it: see that
    /// constant for why the CR-less-only reading made the outcome depend on the caller's read
    /// boundaries, and for why an over-long line is dropped rather than fatal.
    fn drain_line(&mut self, out: &mut Vec<Action>) -> bool {
        let cr = self.buf.iter().position(|&b| b == b'\r');
        // Over-long in either shape — a CR past the limit, or no CR and already past it — or the
        // rest of one whose CR has still not arrived.
        if self.dropping_line > 0 || cr.map_or(self.buf.len() > MAX_LINE, |at| at >= MAX_LINE) {
            return self.drop_over_long_line(cr, out);
        }
        let Some(at) = cr else {
            return false;
        };
        // The copy is what lets `handle_line` take `&mut self`; a ring buffer's front drain is
        // `O(at)`, so the cost is the line and not the tail behind it.
        let raw: Vec<u8> = self.buf.drain(..=at).collect();
        self.handle_line(trim_line(&raw), out);
        true
    }

    /// Drops a line too long to interpret, and says how long it was. Returns whether the buffer
    /// moved on to the next line.
    ///
    /// Nothing of the line is kept: [`MAX_LINE`] exists to bound what this session holds, and
    /// holding a prefix in order to report it would be the same cost with a smaller number on it.
    /// While the CR is still outstanding the buffer is emptied and the count carried in
    /// [`Session::dropping_line`], so an over-long line costs the same whether it arrives whole or
    /// in pieces — and the session resumes at the next line rather than ending over one.
    fn drop_over_long_line(&mut self, cr: Option<usize>, out: &mut Vec<Action>) -> bool {
        let Some(at) = cr else {
            self.dropping_line = self.dropping_line.saturating_add(self.buf.len());
            self.buf.clear();
            return false;
        };
        let dropped = self.dropping_line.saturating_add(at + 1);
        self.buf.drain(..=at);
        self.dropping_line = 0;
        out.push(Action::Trace(format!(
            "ignored an over-long line: {dropped} bytes, over the {MAX_LINE}-byte maximum"
        )));
        true
    }

    /// Dispatches one line by what it is. Order matters only in that the greeting has to be
    /// closed out before an FBB command is acted on.
    fn handle_line(&mut self, line: &[u8], out: &mut Vec<Action>) {
        if line.is_empty() {
            return;
        }
        match line[0] {
            b'[' => self.handle_sid(line, out),
            b';' => self.handle_control(line, out),
            b'F' => {
                if self.state == State::Greeting && !self.greeting_line(out) {
                    return;
                }
                self.handle_command(line, out);
            }
            // A CMS puts banner and MOTD text on the same stream. Refusing the session over a
            // line we do not recognise would refuse a working server for saying hello, which is
            // the forward-compatibility trap `sid.rs` documents at length. The integrity checks
            // that matter — both checksums — are unaffected by anything ignored here.
            _ => out.push(Action::Trace(format!(
                "ignored non-protocol line: {}",
                show(line)
            ))),
        }
    }

    /// The peer's SID. Refusing here is the whole point of [`super::sid`]: a peer that cannot
    /// carry B2 is refused at the handshake, where the reason is still legible, and **before we
    /// have told it our callsign** — a refusal that had already sent `;FW:` and `;PR:` would have
    /// logged in to a peer we then declined to talk to.
    ///
    /// **Reading 6** is the other half of that sentence: *at the handshake*. Once our greeting
    /// block has gone out the peer's SID is behind us, and a `[`-leading line that does not parse
    /// is banner or MOTD text on the same stream — traced and dropped, exactly as the unbracketed
    /// text in [`Session::handle_line`]'s last arm is. See the module header for why the boundary
    /// is `greeted` and not, say, `peer_sid.is_some()`.
    fn handle_sid(&mut self, line: &[u8], out: &mut Vec<Action>) {
        match sid::parse_sid(line) {
            Err(_) if self.greeted => out.push(Action::Trace(format!(
                "ignored bracketed line after the greeting: {}",
                show(line)
            ))),
            Err(err) => self.fail(out, SessionError::Sid(err)),
            Ok(parsed) => {
                out.push(Action::Trace(format!(
                    "peer SID: product={} version={} flags={} (we are {:?})",
                    show(&parsed.product),
                    show(&parsed.version),
                    show(&parsed.flags),
                    self.role
                )));
                let second = self.challenge.is_some();
                self.peer_sid = Some(parsed);
                // Reading 1: whichever of SID / challenge lands second ends the greeting.
                if second {
                    self.greeting_line(out);
                }
            }
        }
    }

    /// A `;`-prefixed line. In FBB these are comments, so everything that is not the challenge is
    /// traced and dropped — including the peer's own `;FW:` and any `;PM:`-style extension.
    fn handle_control(&mut self, line: &[u8], out: &mut Vec<Action>) {
        let Some(challenge) = pq_challenge(line) else {
            out.push(Action::Trace(format!("peer comment: {}", show(line))));
            return;
        };
        out.push(Action::Trace(format!(
            "secure-login challenge: {}",
            show(&challenge)
        )));
        self.challenge = Some(challenge);
        if self.greeted {
            // Reading 1's other half: a challenge that arrives after we have already greeted is
            // answered on its own line. `;PR:` is a comment to a peer that never asked, so the
            // recovery cannot desync one that did not want it.
            let pr = self.pr_line();
            out.push(Action::Send(pr));
        } else if self.peer_sid.is_some() {
            self.greeting_line(out);
        }
    }

    /// Closes the peer's greeting and sends ours. Returns whether the session may continue —
    /// `false` means an FBB command arrived before any SID, which is a desync we cannot answer
    /// (the identity line has no target and the peer's capabilities are unknown).
    fn greeting_line(&mut self, out: &mut Vec<Action>) -> bool {
        if self.peer_sid.is_none() {
            self.fail(
                out,
                SessionError::Protocol("FBB command before the peer's SID"),
            );
            return false;
        }
        if !self.greeted {
            let block = self.greeting_block();
            out.push(Action::Send(block));
            self.greeted = true;
        }
        self.state = State::Line;
        true
    }

    /// Our whole opening block, as one write.
    ///
    /// Five lines, and spec §2.8's structural identity is three of them: `;FW:` names the station
    /// we forward for, the SID names what we can carry, and the `; <target> DE <call>` comment
    /// (reading 4) puts both callsigns in the transcript. The block ends in `FF` because this
    /// implementation proposes nothing — see the module header's last section.
    fn greeting_block(&self) -> Vec<u8> {
        let mut block = Vec::new();
        block.extend_from_slice(b";FW: ");
        block.extend_from_slice(&self.callsign);
        block.push(b'\r');

        block.push(b'[');
        block.extend_from_slice(SID_PRODUCT);
        block.push(b'-');
        block.extend_from_slice(SID_VERSION);
        block.push(b'-');
        block.extend_from_slice(SID_FLAGS);
        block.extend_from_slice(b"$]\r");

        if self.challenge.is_some() {
            block.extend_from_slice(&self.pr_line());
        }

        block.extend_from_slice(b"; ");
        // Reading 4: the target is whatever the peer called itself, never an invented constant.
        if let Some(peer) = &self.peer_sid {
            block.extend_from_slice(&peer.product);
        }
        block.extend_from_slice(b" DE ");
        block.extend_from_slice(&self.callsign);
        block.push(b'\r');

        block.extend_from_slice(b"FF\r");
        block
    }

    /// The `;PR:` line for the challenge in hand. Empty when there is none.
    ///
    /// The derivation is [`secure::pr_response`]'s and is not restated here — the session's whole
    /// job is to route the challenge and the password into it, which is what the fifth negative
    /// control checks at this boundary.
    fn pr_line(&self) -> Vec<u8> {
        let Some(challenge) = &self.challenge else {
            return Vec::new();
        };
        let mut line = Vec::from(b";PR: ");
        line.extend_from_slice(&secure::pr_response(challenge, &self.password));
        line.push(b'\r');
        line
    }

    /// An FBB command line: turn control, a proposal, or a block close.
    fn handle_command(&mut self, line: &[u8], out: &mut Vec<Action>) {
        // `F>` and `FS ` are checked before the generic `F<code> ` proposal shape, because `>`
        // and `S` are both syntactically valid proposal codes and would otherwise be deferred.
        if line == b"FQ" {
            out.push(Action::Trace("peer ended the session (FQ)".into()));
            self.finish(out);
        } else if line == b"FF" {
            // We propose nothing, so an inbound "nothing more from me" ends the session. When the
            // mailbox lands this is where an outbound proposal block goes instead.
            out.push(Action::Trace(
                "peer has nothing more (FF); closing with FQ".into(),
            ));
            out.push(Action::Send(b"FQ\r".to_vec()));
            self.finish(out);
        } else if line.starts_with(b"F>") {
            self.close_proposal_block(line, out);
        } else if line.starts_with(b"FS ") {
            // We proposed nothing, so there is nothing for an inbound FS to answer. Traced rather
            // than refused: it costs nothing and a refusal here would end a working session.
            out.push(Action::Trace(format!(
                "unexpected FS answer (this session proposed nothing): {}",
                show(line)
            )));
        } else if line.len() > 2 && line[2] == b' ' {
            self.handle_proposal(line, out);
        } else {
            out.push(Action::Trace(format!(
                "ignored unknown FBB command: {}",
                show(line)
            )));
        }
    }

    /// One proposal line. Its bytes join the block being summed **as received** (reading 3)
    /// whether or not it parses, because the checksum covers the wire and not our reading of it.
    fn handle_proposal(&mut self, line: &[u8], out: &mut Vec<Action>) {
        self.block.extend_from_slice(line);
        self.block.push(b'\r');
        match fbb::parse_proposal(line) {
            // The MID is the one variable-length thing a retained `Proposal` owns, and
            // `PROPOSAL_HELD_BYTES` charges a flat [`MAX_MID`] for it. Checked here, where the
            // proposal is retained, so the charge is bounded by this function and not by a reading
            // of another one. Deferred rather than rejected: see `MAX_MID`.
            Ok(proposal) if proposal.mid.len() > MAX_MID => {
                out.push(Action::Trace(format!(
                    "deferring a proposal whose MID is {} bytes, over the {MAX_MID}-byte maximum",
                    proposal.mid.len()
                )));
                self.proposals.push(Proposal {
                    kind: UNREADABLE_PROPOSAL,
                    mid: Vec::new(),
                    u_size: 0,
                    c_size: 0,
                });
            }
            Ok(proposal) => {
                out.push(Action::Trace(format!(
                    "offered {} ({} bytes, {} compressed)",
                    show(&proposal.mid),
                    proposal.u_size,
                    proposal.c_size
                )));
                self.proposals.push(proposal);
            }
            Err(_) => {
                // `FA`/`FB` proposals (B2F allows them intermixed with `FC`) and anything
                // genuinely garbled land here. Both get a placeholder whose only job is to make
                // `fs_answer` defer this slot and keep the answer count aligned with the block —
                // a dropped slot would shift every answer after it onto the wrong message.
                out.push(Action::Trace(format!(
                    "proposal not understood; deferring it: {}",
                    show(line)
                )));
                self.proposals.push(Proposal {
                    kind: if line[1] == fbb::PROPOSAL_CODE_FC {
                        UNREADABLE_PROPOSAL
                    } else {
                        line[1]
                    },
                    mid: Vec::new(),
                    u_size: 0,
                    c_size: 0,
                });
            }
        }
    }

    /// `F> HH` — verify the block, then answer it.
    ///
    /// The `have` callback is the mailbox seam (module header): today it says "nothing held" for
    /// every MID, so every readable `FC` proposal is accepted. The answer line is then *read back*
    /// to decide how many transfers to expect, rather than re-deriving that from the same
    /// `have` decisions — the peer acts on the bytes we sent, so those bytes are what the count
    /// must come from.
    fn close_proposal_block(&mut self, line: &[u8], out: &mut Vec<Action>) {
        if self.proposals.is_empty() {
            self.fail(
                out,
                SessionError::Protocol("F> closed a proposal block with no proposals in it"),
            );
            return;
        }
        let Some(claimed) = parse_hex_byte(line[2..].trim_ascii()) else {
            self.fail(
                out,
                SessionError::Protocol("F> checksum is not two hex digits"),
            );
            return;
        };
        if claimed != fbb::fb_checksum(&self.block) {
            self.fail(out, SessionError::Fbb(FbbError::ProposalChecksum));
            return;
        }

        let proposals = std::mem::take(&mut self.proposals);
        // A fresh `Vec`, not `clear()`: the block is finished with, and a cleared one would go on
        // owning whatever the largest block this session saw needed.
        self.block = Vec::new();
        for proposal in proposals.iter() {
            if let Some(why) = over_ceiling(proposal) {
                out.push(Action::Trace(format!(
                    "refusing {}: its FC line declares {} compressed and {} uncompressed bytes; \
                     {why} (ceilings {MAX_PROPOSAL_C_SIZE} and {MAX_PROPOSAL_U_SIZE})",
                    show(&proposal.mid),
                    proposal.c_size,
                    proposal.u_size,
                )));
            }
        }
        let answer = fbb::fs_answer(&proposals, |proposal| {
            if over_ceiling(proposal).is_some() {
                HaveState::Unwanted
            } else {
                HaveState::No
            }
        });
        let Some(wanted) = read_fs_answer(&answer, proposals.len()) else {
            self.fail(
                out,
                SessionError::Protocol("the FS answer does not match the block it answers"),
            );
            return;
        };
        self.accepted = proposals
            .into_iter()
            .zip(wanted)
            .filter_map(|(proposal, send)| send.then_some(proposal))
            .collect();
        out.push(Action::Trace(format!(
            "answered the block: {} ({} transfer(s) to come)",
            show(&answer),
            self.accepted.len()
        )));
        out.push(Action::Send(answer));
        if !self.accepted.is_empty() {
            self.state = State::Binary;
            // Belt and braces, and **no test covers it**: measured, deleting this line leaves the
            // whole crate green. Every path that reaches here has `binary_bytes` already at zero —
            // it starts there and `complete_transfer` puts it back — and the only way it would not
            // is a failed transfer, which ends the session. Kept because entering the binary phase
            // is the natural place to say the count starts at zero, not because it is load-bearing.
            // The reset that *is* load-bearing is `complete_transfer`'s, gated by
            // `two_transfers_in_one_block_are_each_counted_against_their_own_proposal`.
            self.binary_bytes = 0;
        }
    }

    // -- binary phase -------------------------------------------------------------------------

    /// Feeds the buffer to the framer **one byte at a time**, stopping the instant the last
    /// accepted transfer completes.
    ///
    /// Byte at a time on purpose, and it is not an oversight: [`Framer`] consumes everything it
    /// is handed, so a chunk that carried the final `EOT` *and* the `FF` line behind it would
    /// have the `F` refused as an unframeable marker. Nothing else can find that boundary — the
    /// binary phase ends where the accepted-transfer count runs out, and only the framer knows
    /// when a record is complete. A transfer is bounded by the proposal's `c-size` — by the count
    /// below, which is where that bound is taken — so the cost is one call per body byte on a
    /// path that already ran a Huffman decoder over the same bytes.
    ///
    /// The count is checked **before** the byte is handed on, so nothing past the ceiling ever
    /// reaches the framer's buffer: the peer chooses how much it sends, and this end chooses how
    /// much of it it is willing to hold. See the module header's resource-bound section.
    fn drain_binary(&mut self, out: &mut Vec<Action>) -> bool {
        if self.buf.is_empty() {
            return false;
        }
        let mut consumed = 0usize;
        while consumed < self.buf.len() && self.state == State::Binary {
            let byte = self.buf[consumed];
            consumed += 1;
            self.binary_bytes += 1;
            if self.binary_bytes > self.transfer_ceiling() {
                self.fail(
                    out,
                    SessionError::Protocol(
                        "a transfer ran past the compressed size its FC proposal declared",
                    ),
                );
                break;
            }
            match self.framer.feed(&[byte]) {
                Ok(frames) => {
                    for frame in frames {
                        self.handle_frame(frame, out);
                        if self.state != State::Binary {
                            break;
                        }
                    }
                }
                Err(err) => self.fail(out, SessionError::Fbb(err)),
            }
        }
        // `close()` empties the buffer itself, so after a failure there is no prefix left to
        // drain — and draining one out of a buffer that is already gone panics. (This is not a
        // hypothetical: `bad_eot_checksum_fails_and_does_not_deliver` found exactly that, which
        // is the whole argument for shipping the negative controls with the engine.)
        if self.state != State::Closed {
            self.buf.drain(..consumed);
        }
        true
    }

    /// The most wire bytes a **legal** transfer of the proposal now in flight can take.
    ///
    /// Worst legal case rather than likely case, on purpose. FBB framing permits a data block as
    /// short as one byte (`0x00` in the length byte means 256, so an *empty* block is
    /// unrepresentable, but a one-byte block is not), and each one costs its marker and its
    /// length byte — three wire bytes per data byte. Around those sit one `SOH` record (marker,
    /// length, and a payload of at most 256) and the two-byte `EOT`. A peer sending full blocks
    /// lands near `c_size + 260`, so the slack is real; it is the price of not refusing a legal
    /// transfer for choosing small blocks, on a limit no protocol document states. What the
    /// number has to be is *finite and ours*, not tight.
    ///
    /// No proposal outstanding yields the framing-only floor. That state should be unreachable —
    /// [`Session::complete_transfer`] leaves the binary phase when the last accepted proposal is
    /// retired — and if it ever is reached, the bound holds there too rather than opening.
    ///
    /// **This is a protocol-correctness check, not the memory bound** (module header). `c_size` is
    /// a peer-supplied `u32`, so `3 * c_size` reaches 12 GiB for a peer that simply lies in its
    /// `FC` line, and what this function refuses is a *transfer that is not the message it
    /// proposed* — a fault worth naming even on a machine with infinite memory. The session's
    /// footprint is bounded by [`MAX_SESSION_HELD_BYTES`] at [`Session::feed`] whatever this
    /// number says, so a future change that accepted a proposal without
    /// [`MAX_PROPOSAL_C_SIZE`]'s test would lose a legible refusal, not the bound.
    fn transfer_ceiling(&self) -> u64 {
        let c_size = self.accepted.front().map_or(0, |p| u64::from(p.c_size));
        3 * c_size + 258 + 2
    }

    /// One verified frame. Nothing reaches here that an `EOT` has not already vouched for, which
    /// is why the delivery below can be unconditional.
    fn handle_frame(&mut self, frame: Frame, out: &mut Vec<Action>) {
        match frame {
            Frame::Header(payload) => {
                let (title, offset) = split_soh_header(&payload);
                out.push(Action::Trace(format!(
                    "transfer opening: title={} offset={}",
                    show(title),
                    show(offset)
                )));
                // The title is a cross-check, not the message's identity — `parse_b2` lifts the
                // authoritative `Mid:` out of the body itself, so a mismatch is worth saying out
                // loud and is not worth failing a transfer that will name itself in a moment.
                if let Some(expected) = self.accepted.front() {
                    if !title.is_empty() && title != expected.mid.as_slice() {
                        out.push(Action::Trace(format!(
                            "transfer title {} does not match the proposal it answers ({})",
                            show(title),
                            show(&expected.mid)
                        )));
                    }
                }
            }
            Frame::Data(data) => self.image.extend_from_slice(&data),
            Frame::Eot => self.complete_transfer(out),
        }
    }

    /// An `EOT` verified: unwrap the body it closed and deliver the message.
    ///
    /// Both proposal sizes are checked against what actually arrived. That is not belt-and-braces:
    /// [`super::message`]'s header records that a `Body:`/`File:` length short by exactly the
    /// amount that leaves a CRLF at the cut is undetectable from inside one message, and names the
    /// proposal's `u-size` as the outer check for it — this is that check, and it lives here
    /// because the session is the only layer holding both numbers.
    fn complete_transfer(&mut self, out: &mut Vec<Action>) {
        let Some(proposal) = self.accepted.pop_front() else {
            self.fail(
                out,
                SessionError::Protocol("a transfer completed with no proposal outstanding"),
            );
            return;
        };
        let image = std::mem::take(&mut self.image);
        // The next transfer in the block is counted from zero against its own proposal.
        self.binary_bytes = 0;
        if image.len() as u64 != u64::from(proposal.c_size) {
            self.fail(
                out,
                SessionError::Protocol("compressed body length disagrees with its FC proposal"),
            );
            return;
        }
        // Bounded by the `u-size` this proposal declared, which `over_ceiling` already judged
        // against `MAX_PROPOSAL_U_SIZE` before the transfer was accepted. The bound goes *into*
        // the decompressor rather than being applied to what it returns: the image carries its own
        // uncompressed length, the peer picks it, and the equality test below cannot un-allocate
        // the tens of megabytes it disagrees with. `LzhufError::TooLarge` is a peer that
        // contradicted its own `FC` line, and it fails the transfer for the same reason the length
        // test does.
        let plain = match lzhuf::decompress_bounded(&image, proposal.u_size as usize) {
            Ok(plain) => plain,
            Err(err) => return self.fail(out, SessionError::Lzhuf(err)),
        };
        if plain.len() as u64 != u64::from(proposal.u_size) {
            self.fail(
                out,
                SessionError::Protocol("decompressed body length disagrees with its FC proposal"),
            );
            return;
        }
        let msg = match message::parse_b2(&plain) {
            Ok(msg) => msg,
            Err(err) => return self.fail(out, SessionError::Message(err)),
        };
        out.push(Action::Trace(format!(
            "received {} ({} bytes, {} attachment(s))",
            show(&msg.mid),
            plain.len(),
            msg.attachments.len()
        )));
        out.push(Action::Received(msg));
        if self.accepted.is_empty() {
            // Back to line mode for the peer's `FF`/`FQ`, or its next proposal block.
            self.state = State::Line;
        }
    }

    // -- ending -------------------------------------------------------------------------------

    /// Ends the session cleanly.
    fn finish(&mut self, out: &mut Vec<Action>) {
        self.close();
        out.push(Action::Done);
    }

    /// Ends the session on `err`. Everything unconsumed is dropped: after a checksum failure or a
    /// desync we cannot honestly say where the next unit begins, and a later `feed` must not be
    /// able to deliver from a stream whose alignment is no longer known — [`Framer`]'s rule 2, at
    /// the session's own scale.
    fn fail(&mut self, out: &mut Vec<Action>, err: SessionError) {
        self.close();
        out.push(Action::Failed(err));
    }

    /// The shared half of both endings.
    ///
    /// **Fresh containers, not `clear()`.** `clear()` drops the elements and keeps the allocation,
    /// so a session that gave up went on owning every byte it had been made to hold — tens of
    /// megabytes of it, most in `framer`, which the previous version of this function did not name
    /// at all. After this, a closed session's `held_bytes()` is zero, which is the assertion
    /// `a_closed_session_holds_nothing` makes and the reason for naming every peer-filled field
    /// here rather than the big ones.
    fn close(&mut self) {
        self.state = State::Closed;
        self.buf = VecDeque::new();
        self.dropping_line = 0;
        self.peer_sid = None;
        self.challenge = None;
        self.block = Vec::new();
        self.proposals = Vec::new();
        self.accepted = VecDeque::new();
        self.framer = Framer::new();
        self.image = Vec::new();
    }
}

/// Strips the line's terminator: any `\n` left over from a preceding CRLF at the front, and the
/// `\r` this line was split on at the back.
fn trim_line(raw: &[u8]) -> &[u8] {
    let mut line = raw;
    while line.first() == Some(&b'\n') {
        line = &line[1..];
    }
    if line.last() == Some(&b'\r') {
        line = &line[..line.len() - 1];
    }
    line
}

/// **Reading 2.** The `;PQ:` challenge value, or `None` if this is not a challenge line.
///
/// The only normalisation is ASCII whitespace at both ends — the space after the colon is the
/// field separator, not payload. [`super::secure`] is explicit that anything more is a silent
/// wrong answer at the digest, so this function is deliberately the only place that decides, and
/// it decides as little as possible.
fn pq_challenge(line: &[u8]) -> Option<Vec<u8>> {
    let value = line.strip_prefix(b";PQ:")?;
    Some(value.trim_ascii().to_vec())
}

/// Why a proposal declares more than this station will accept, or `None` if it does not.
///
/// One function rather than the condition written twice, because the trace and the `FS` answer
/// are two consumers of one decision: written out at both sites, a later edit that relaxed the
/// comparison in the answer alone would leave the session tracing "refusing" and then answering
/// `+`. It returns the reason for the same purpose one step on — the trace names which of the two
/// sizes was refused, and it cannot name a different one from the one the answer acted on.
///
/// See [`MAX_PROPOSAL_C_SIZE`] and [`MAX_PROPOSAL_U_SIZE`].
fn over_ceiling(proposal: &Proposal) -> Option<&'static str> {
    if proposal.c_size > MAX_PROPOSAL_C_SIZE {
        Some("over the compressed ceiling")
    } else if proposal.u_size > MAX_PROPOSAL_U_SIZE {
        Some("over the uncompressed ceiling")
    } else {
        None
    }
}

/// Two ASCII hex digits to a byte. Upper and lower case both accepted: the FBB document writes
/// `F>` checksums in upper case and every observed peer does too, but case is presentation and
/// refusing `f> 7a` would fail a session over nothing.
fn parse_hex_byte(field: &[u8]) -> Option<u8> {
    let [high, low] = field else { return None };
    let digit = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    Some(digit(*high)? << 4 | digit(*low)?)
}

/// Reads an `FS` answer line back into one "send it" flag per proposal, or `None` if it does not
/// describe exactly `count` answers.
///
/// `!offset` is one answer spanning several bytes, which is the only reason this is a parser
/// rather than a byte count.
fn read_fs_answer(answer: &[u8], count: usize) -> Option<Vec<bool>> {
    let body = answer.strip_prefix(b"FS ")?.strip_suffix(b"\r")?;
    let mut flags = Vec::with_capacity(count);
    let mut i = 0;
    while i < body.len() {
        match body[i] {
            b'+' => {
                i += 1;
                flags.push(true);
            }
            b'!' => {
                i += 1;
                while i < body.len() && body[i].is_ascii_digit() {
                    i += 1;
                }
                flags.push(true);
            }
            b'-' | b'=' => {
                i += 1;
                flags.push(false);
            }
            _ => return None,
        }
    }
    (flags.len() == count).then_some(flags)
}

/// Splits an SOH payload into `(title, offset)`. The payload is `<title> NUL <offset> NUL`; a
/// payload that is not shaped that way yields whatever prefixes are there, because this is only
/// ever used for a trace and a cross-check and must not be able to fail a transfer.
fn split_soh_header(payload: &[u8]) -> (&[u8], &[u8]) {
    let mut fields = payload.split(|&b| b == 0);
    (fields.next().unwrap_or(&[]), fields.next().unwrap_or(&[]))
}

/// Renders wire bytes for a log line. **Display only** — lossy, escaped, and never parsed,
/// compared or written back to the wire. Everything that has to survive round-trips stays bytes.
fn show(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).escape_debug().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_line_strips_both_terminators() {
        assert_eq!(trim_line(b"FF\r"), b"FF");
        assert_eq!(trim_line(b"\nFF\r"), b"FF");
        assert_eq!(trim_line(b"FF"), b"FF");
        assert_eq!(trim_line(b"\r"), b"");
    }

    /// The one normalisation reading 2 permits, and the one it does not: the separating space is
    /// framing, but the value itself is handed on byte for byte.
    #[test]
    fn pq_challenge_takes_the_token_and_nothing_else() {
        assert_eq!(pq_challenge(b";PQ: 41913235"), Some(b"41913235".to_vec()));
        assert_eq!(pq_challenge(b";PQ:41913235"), Some(b"41913235".to_vec()));
        assert_eq!(pq_challenge(b";PR: 41913235"), None);
        assert_eq!(pq_challenge(b"FF"), None);
    }

    #[test]
    fn hex_byte_accepts_either_case_and_refuses_the_rest() {
        assert_eq!(parse_hex_byte(b"7A"), Some(0x7a));
        assert_eq!(parse_hex_byte(b"7a"), Some(0x7a));
        assert_eq!(parse_hex_byte(b"00"), Some(0));
        assert_eq!(parse_hex_byte(b"7"), None);
        assert_eq!(parse_hex_byte(b"7G"), None);
        assert_eq!(parse_hex_byte(b""), None);
    }

    /// The `!offset` form is why the answer is parsed rather than counted: four answers here span
    /// eight bytes, and a byte count would expect eight transfers.
    #[test]
    fn fs_answers_are_counted_by_meaning_not_by_byte() {
        assert_eq!(
            read_fs_answer(b"FS +-=!1234\r", 4),
            Some(vec![true, false, false, true])
        );
        assert_eq!(read_fs_answer(b"FS +\r", 1), Some(vec![true]));
        // Wrong count, unknown character, and missing framing all refuse.
        assert_eq!(read_fs_answer(b"FS +\r", 2), None);
        assert_eq!(read_fs_answer(b"FS +?\r", 2), None);
        assert_eq!(read_fs_answer(b"+\r", 1), None);
    }

    #[test]
    fn soh_header_splits_into_title_and_offset() {
        assert_eq!(
            split_soh_header(b"ABCDEFGHIJKL\x000\x00"),
            (&b"ABCDEFGHIJKL"[..], &b"0"[..])
        );
        // Shapes that are not the grammar must degrade, never panic.
        assert_eq!(split_soh_header(b""), (&b""[..], &b""[..]));
        assert_eq!(split_soh_header(b"TITLE"), (&b"TITLE"[..], &b""[..]));
    }

    /// A peer that says `FF` before it has said who it is cannot be answered — the identity line
    /// has no target and its capabilities are unknown — so the session refuses rather than
    /// guessing. The positive control is right below it.
    #[test]
    fn a_command_before_the_sid_is_a_desync() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        let mut session = Session::new(&cfg, Role::Client);
        let actions = session.feed(b"FF\r");
        assert!(matches!(
            actions.last(),
            Some(Action::Failed(SessionError::Protocol(_)))
        ));
        assert!(session.wants_close());
    }

    /// The positive control for the test above: the same `FF`, after a SID, greets and closes
    /// cleanly. Without this, "refuses `FF`" could just as well be "refuses everything".
    #[test]
    fn the_same_command_after_a_sid_greets_and_closes() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        let mut session = Session::new(&cfg, Role::Client);
        let actions = session.feed(b"[WL2K-5.0-B2FWIHJM$]\rFF\r");
        let sent: Vec<u8> = actions
            .iter()
            .filter_map(|a| match a {
                Action::Send(bytes) => Some(bytes.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(
            sent,
            b";FW: N0CALL\r[Nexus-1.0-B2FHM$]\r; WL2K DE N0CALL\rFF\rFQ\r".to_vec(),
            "an unchallenged peer gets the same block without a ;PR: line"
        );
        assert_eq!(actions.last(), Some(&Action::Done));
        assert!(session.wants_close());
    }

    /// A challenge that arrives after the greeting has gone out is answered on its own line
    /// rather than dropped — reading 1's recovery path, which the golden transcript's ordering
    /// never exercises.
    #[test]
    fn a_late_challenge_is_still_answered() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "NEXUSTEST".into(),
        };
        let mut session = Session::new(&cfg, Role::Client);
        let mut actions = session.feed(b"[WL2K-5.0-B2FWIHJM$]\rFC EM AAAAAAAAAAAA 1 1 0\r");
        actions.extend(session.feed(b";PQ: 41913235\r"));
        let sent: Vec<u8> = actions
            .iter()
            .filter_map(|a| match a {
                Action::Send(bytes) => Some(bytes.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        assert!(
            sent.ends_with(b";PR: 74706169\r"),
            "the late challenge must still produce a ;PR: line: {sent:?}"
        );
    }

    /// A session greeted, challenged, and holding exactly one accepted proposal of `c_size`
    /// compressed bytes — the state the binary phase begins in. The `F>` checksum is computed
    /// here rather than written out, because a wrong one would fail the block before the state
    /// under test is reached, and the assertion below is what proves it was not.
    fn session_awaiting_a_body(c_size: u32) -> Session {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        let mut session = Session::new(&cfg, Role::Client);
        session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
        let fc = format!("FC EM AAAAAAAAAAAA 10 {c_size} 0\r").into_bytes();
        session.feed(&fc);
        let actions = session.feed(format!("F> {:02X}\r", fbb::fb_checksum(&fc)).as_bytes());
        assert!(
            actions.contains(&Action::Send(b"FS +\r".to_vec())),
            "the proposal must have been accepted for this fixture to mean anything: {actions:?}"
        );
        session
    }

    /// [`Session::transfer_ceiling`]: a peer that streams `STX` blocks and never sends the `EOT`
    /// that would let `complete_transfer` compare lengths is no longer sending the message it
    /// proposed, and this is where that is said. It is also the reason the check is not `fbb`'s to
    /// take — `Framer` holds every block back until an `EOT` vouches for it, and the number those
    /// bytes are measured against is the proposal's, which only the session holds.
    ///
    /// Both directions, because a guard that refuses everything would pass the second half alone:
    /// the control feeds a body that fits and must be taken quietly, and the golden replay (a
    /// real 365-byte transfer, ceiling 1355) is the same control at full size.
    #[test]
    fn a_transfer_that_runs_past_its_c_size_is_refused() {
        let mut session = session_awaiting_a_body(5);

        // Control: an `SOH` header and a short data block — 119 wire bytes against a ceiling of
        // 275 — are inside what the proposal permits, and nothing may object to them.
        let mut opening = vec![0x01u8, 15];
        opening.extend_from_slice(b"AAAAAAAAAAAA\x000\x00");
        opening.push(0x02);
        opening.push(100);
        opening.extend(std::iter::repeat_n(b'x', 100));
        let actions = session.feed(&opening);
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Action::Failed(_) | Action::Done)),
            "a body inside its proposal's size must not be refused: {actions:?}"
        );
        assert!(!session.wants_close());

        // The finding: full data blocks, no `EOT`, for as long as the peer cares to send them.
        // The reviewer fed 160,000 of these (40 MB) and the session took every byte; the loop
        // stops at 4,000 only so that the mutation this test is written for fails fast.
        let mut full = vec![0x02u8, 0x00];
        full.extend(std::iter::repeat_n(0xAAu8, 256));
        let mut fed = 0usize;
        let mut failure = None;
        for _ in 0..4_000 {
            let actions = session.feed(&full);
            fed += full.len();
            if let Some(Action::Failed(err)) = actions.last() {
                failure = Some(*err);
                break;
            }
        }
        assert_eq!(
            failure,
            Some(SessionError::Protocol(
                "a transfer ran past the compressed size its FC proposal declared"
            )),
            "an unbounded transfer must be a protocol failure, not a truncation"
        );
        assert!(session.wants_close(), "the transport must be told to close");
        // The point of the bound is the number, not the refusal: a session that only noticed at
        // the end of the loop would pass every assertion above and still be the defect.
        assert!(
            fed < 1024,
            "the refusal must land near the proposal's own ceiling, not after {fed} bytes"
        );
    }

    /// Reading 6, both directions. Before our greeting goes out a bracketed line is the peer's
    /// SID and a malformed one is fatal; after it, the SID is behind us and a bracketed line is
    /// banner text a CMS put on the same stream — ending the session there would drop mail that
    /// is still outstanding.
    #[test]
    fn a_bracketed_line_is_fatal_during_the_greeting_and_ignored_after_it() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };

        let mut session = Session::new(&cfg, Role::Client);
        session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
        let actions = session.feed(b"[Winlink CMS MOTD]\r");
        assert!(
            !actions.iter().any(|a| matches!(a, Action::Failed(_))),
            "a bracketed line after the greeting must not fail the session: {actions:?}"
        );
        assert!(matches!(actions.last(), Some(Action::Trace(_))));
        assert!(
            !session.wants_close(),
            "the transport must stay open for the mail that has not arrived yet"
        );
        // And the session is still a session, not merely un-failed.
        assert_eq!(session.feed(b"FF\r").last(), Some(&Action::Done));

        let mut session = Session::new(&cfg, Role::Client);
        let actions = session.feed(b"[Winlink CMS MOTD]\r");
        assert!(
            matches!(
                actions.last(),
                Some(Action::Failed(SessionError::Sid(sid::SidError::Malformed)))
            ),
            "the peer's opening bracketed line is its SID, and a malformed one is refused where \
             the reason is still legible: {actions:?}"
        );
        assert!(session.wants_close());
    }

    /// The caller half of the `fbb` module header's positional-answer rule, which is this
    /// session: a proposal line [`fbb::parse_proposal`] refuses still owns a slot in the `FS`
    /// answer.
    ///
    /// `handle_proposal`'s obvious shape — keep the `Ok`s and drop the rest — answers this
    /// three-line block `FS +\r`. The peer would read that single `+` as accepting proposal #1
    /// and send `AAAAAAAAAAAA`'s body, this end would decode it as `BBBBBBBBBBBB`, and
    /// `BBBBBBBBBBBB` would be treated as answered and never sent. Nothing on the wire reports
    /// that, which is why the count is asserted here rather than left to the transfer to reveal.
    #[test]
    fn an_unparseable_proposal_line_still_occupies_its_answer_slot() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        let mut session = Session::new(&cfg, Role::Client);
        session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");

        // B2F allows `FA`/`FB` intermixed with `FC`. The last line is a garbled `FC` — a second
        // refusal by a different route, so a fix that only special-cased foreign codes is caught
        // too. The checksum is taken over all three lines exactly as sent, refused or not.
        let mut block = Vec::new();
        block.extend_from_slice(b"FA EM AAAAAAAAAAAA 100 80 0\r");
        block.extend_from_slice(b"FC EM BBBBBBBBBBBB 200 150 0\r");
        block.extend_from_slice(b"FC EM CCCCCCCCCCCC 7 five 0\r");
        session.feed(&block);
        let actions = session.feed(format!("F> {:02X}\r", fbb::fb_checksum(&block)).as_bytes());

        let sent: Vec<Vec<u8>> = actions
            .iter()
            .filter_map(|action| match action {
                Action::Send(bytes) => Some(bytes.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            sent,
            vec![b"FS =+=\r".to_vec()],
            "three proposal lines must be answered by three characters, with the one readable \
             proposal in the slot it was proposed in: {actions:?}"
        );
    }

    /// One `SOH`/`STX`/`EOT` transfer on the wire: header record, then the image in `block`-byte
    /// data blocks, then the `EOT` and its checksum over the data bytes alone.
    ///
    /// `block` is a parameter because the sender picks it and what it costs this end is the whole
    /// subject of [`MAX_SESSION_HELD_BYTES`]: 256 is [`super::fbb`]'s maximum, 125 is `wl2k-go`'s
    /// sender, and 1 is the smallest the framing can express.
    fn transfer_records(mid: &[u8], image: &[u8], block: usize) -> Vec<u8> {
        assert!(
            (1..=256).contains(&block),
            "FBB data blocks are 1..=256 bytes"
        );
        let mut out = Vec::new();
        let mut header = mid.to_vec();
        header.push(0);
        header.push(b'0');
        header.push(0);
        out.push(0x01);
        out.push(header.len() as u8);
        out.extend_from_slice(&header);
        let mut sum = 0u8;
        for chunk in image.chunks(block) {
            out.push(0x02);
            // `0x00` in the length byte means 256, which is why a full block is not 0x100.
            out.push(if chunk.len() == 256 {
                0
            } else {
                chunk.len() as u8
            });
            out.extend_from_slice(chunk);
            for &byte in chunk {
                sum = sum.wrapping_add(byte);
            }
        }
        out.push(0x04);
        out.push(0u8.wrapping_sub(sum));
        out
    }

    /// A B2 message with `mid` and `body`, as the plaintext and the LZHUF image a proposal for it
    /// would declare.
    fn body_and_image(mid: &[u8], body: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let plain = message::assemble_b2(&Message {
            mid: mid.to_vec(),
            headers: vec![],
            body: body.to_vec(),
            attachments: vec![],
        })
        .expect("the fixture message must be representable");
        let image = lzhuf::compress(&plain);
        (plain, image)
    }

    /// `len` bytes that LZHUF cannot shrink (xorshift64, fixed seed), so a fixture's image size is
    /// set by the body length rather than by how well the coder happens to do on prose.
    fn incompressible(len: usize) -> Vec<u8> {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        std::iter::repeat_with(|| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 24) as u8
        })
        .take(len)
        .collect()
    }

    /// Offers one message and feeds its whole transfer in `block`-byte data blocks, returning
    /// everything the session did.
    ///
    /// The wire goes in in 4 KiB chunks because that is what the sibling socket readers in
    /// `tempo_net` hand a session (`cluster.rs`, `aprsis.rs`), so a delivery here is the delivery
    /// a real caller gets, boundaries and all — and not a single `feed` of the whole transfer,
    /// which no caller performs and which would hide any chunk-boundary dependence.
    fn deliver_at_block_size(
        mid: &[u8],
        plain_len: usize,
        image: &[u8],
        block: usize,
    ) -> Vec<Action> {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        let mut session = Session::new(&cfg, Role::Client);
        let mut actions = session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
        let fc = format!(
            "FC EM {} {plain_len} {} 0\r",
            String::from_utf8_lossy(mid),
            image.len()
        )
        .into_bytes();
        actions.extend(session.feed(&fc));
        actions.extend(session.feed(format!("F> {:02X}\r", fbb::fb_checksum(&fc)).as_bytes()));
        for chunk in transfer_records(mid, image, block).chunks(4096) {
            actions.extend(session.feed(chunk));
        }
        actions
    }

    /// The MID of every message `actions` delivered, in order.
    fn delivered(actions: &[Action]) -> Vec<Vec<u8>> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Received(msg) => Some(msg.mid.clone()),
                _ => None,
            })
            .collect()
    }

    /// Why the session failed, if it did.
    fn failure(actions: &[Action]) -> Option<SessionError> {
        actions.iter().find_map(|a| match a {
            Action::Failed(err) => Some(*err),
            _ => None,
        })
    }

    /// **Two accepted proposals in one block** — a CMS holding two messages, which is the ordinary
    /// case and the one no other test in this crate covers: every fixture and every control above
    /// carries exactly one proposal per block.
    ///
    /// It exists for one line. [`Session::complete_transfer`] resets `binary_bytes` to zero so the
    /// next transfer is counted against *its own* proposal; with that line gone the second
    /// transfer inherits the first one's total, and a perfectly legal message is refused with
    /// `"a transfer ran past the compressed size its FC proposal declared"` — a false refusal that
    /// ends the session with mail dropped, which is exactly the failure
    /// [`Session::transfer_ceiling`]'s slack is written to avoid. The whole suite stayed green
    /// against that mutation before this test existed.
    ///
    /// The two messages are deliberately lopsided, and the assertion below pins why: the mutation
    /// is only observable when the first transfer's wire bytes exceed the *second* proposal's
    /// entire ceiling, so a future change to the codec that evened the sizes out would quietly
    /// stop this test from discriminating anything.
    #[test]
    fn two_transfers_in_one_block_are_each_counted_against_their_own_proposal() {
        // Deliberately incompressible: prose of this length collapses to an image far too small
        // for the first transfer to overrun the second proposal's ceiling, and the assertion below
        // is what caught that.
        let (plain_a, image_a) = body_and_image(b"AAAAAAAAAAAA", &incompressible(1500));
        let (plain_b, image_b) = body_and_image(b"BBBBBBBBBBBB", b"ok");

        let wire_a = transfer_records(b"AAAAAAAAAAAA", &image_a, 256);
        let ceiling_b = 3 * image_b.len() + 258 + 2;
        assert!(
            wire_a.len() > ceiling_b,
            "this control only discriminates the reset while the first transfer's {} wire bytes \
             exceed the second proposal's whole ceiling of {}",
            wire_a.len(),
            ceiling_b
        );

        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        let mut session = Session::new(&cfg, Role::Client);
        session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");

        let mut block =
            format!("FC EM AAAAAAAAAAAA {} {} 0\r", plain_a.len(), image_a.len()).into_bytes();
        block.extend_from_slice(
            format!("FC EM BBBBBBBBBBBB {} {} 0\r", plain_b.len(), image_b.len()).as_bytes(),
        );
        session.feed(&block);
        let actions = session.feed(format!("F> {:02X}\r", fbb::fb_checksum(&block)).as_bytes());
        assert!(
            actions.contains(&Action::Send(b"FS ++\r".to_vec())),
            "both proposals must be accepted for this test to mean anything: {actions:?}"
        );

        let mut actions = session.feed(&wire_a);
        actions.extend(session.feed(&transfer_records(b"BBBBBBBBBBBB", &image_b, 256)));

        let failures: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                Action::Failed(err) => Some(*err),
                _ => None,
            })
            .collect();
        assert!(
            failures.is_empty(),
            "two legal transfers in one block must both be taken: {failures:?}"
        );
        let delivered: Vec<Vec<u8>> = actions
            .iter()
            .filter_map(|a| match a {
                Action::Received(msg) => Some(msg.mid.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            delivered,
            vec![b"AAAAAAAAAAAA".to_vec(), b"BBBBBBBBBBBB".to_vec()],
            "both messages must be delivered, in the order they were proposed"
        );
    }

    /// A greeted and challenged session that has answered one `FC` line proposing `c_size`
    /// compressed bytes, with the actions its `F>` produced.
    fn answer_a_proposal_of(c_size: u64) -> (Session, Vec<Action>) {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        let mut session = Session::new(&cfg, Role::Client);
        session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
        let fc = format!("FC EM AAAAAAAAAAAA 10 {c_size} 0\r").into_bytes();
        session.feed(&fc);
        let actions = session.feed(format!("F> {:02X}\r", fbb::fb_checksum(&fc)).as_bytes());
        (session, actions)
    }

    /// [`MAX_PROPOSAL_C_SIZE`], both sides of it.
    ///
    /// The refusal has to land at **proposal** time, before a byte of body is asked for, because
    /// that is where it is legible: one message answered `-` with a traced reason, rather than a
    /// session killed mid-stream over a number the peer chose. `FC EM <mid> 4294967295 4294967295
    /// 0` costs a hostile or buggy CMS one line, and it is answered here and not later.
    ///
    /// Both directions, because a cap that refused everything would pass the second half alone:
    /// a proposal at exactly the ceiling is legal and must be accepted.
    #[test]
    fn a_proposal_over_the_absolute_ceiling_is_refused_at_proposal_time() {
        // Control: exactly at the cap is a legal message and is accepted.
        let (_, actions) = answer_a_proposal_of(u64::from(MAX_PROPOSAL_C_SIZE));
        assert!(
            actions.contains(&Action::Send(b"FS +\r".to_vec())),
            "a proposal at exactly the ceiling must be accepted: {actions:?}"
        );

        // The finding: one byte over, and a `u32::MAX` liar, are both answered `-`.
        for c_size in [u64::from(MAX_PROPOSAL_C_SIZE) + 1, u64::from(u32::MAX)] {
            let (mut session, actions) = answer_a_proposal_of(c_size);
            assert!(
                actions.contains(&Action::Send(b"FS -\r".to_vec())),
                "a proposal declaring {c_size} compressed bytes must be answered `-`: {actions:?}"
            );
            assert!(
                session.accepted.is_empty() && session.state != State::Binary,
                "a refused proposal must not open the binary phase: {:?}",
                session.state
            );
            assert!(
                actions
                    .iter()
                    .any(|a| matches!(a, Action::Trace(t) if t.contains("ceiling"))),
                "the refusal must be legible in the trace, not silent: {actions:?}"
            );
            // Still a session: the refusal answers one message, it does not fail the peer.
            assert_eq!(session.feed(b"FF\r").last(), Some(&Action::Done));
        }
    }

    /// [`MAX_SESSION_HELD_BYTES`], both directions — the memory bound, and the finding that
    /// motivated moving it to the door.
    ///
    /// `block` and `proposals` grow once per proposal line and nothing looked at either until an
    /// `F>` arrived, so a peer that streams `FC` lines and never closes its block accumulated for
    /// as long as it cared to send. Measured with the ceiling check removed from [`Session::feed`]
    /// — which is the behaviour this test was written against — 200,000 of the line below left
    /// `block` at 5,400,000 bytes and `proposals` at 200,000 entries, with the session still in
    /// `State::Line` and `wants_close()` false. Which buffer that was is not the point: the bound
    /// under test is the total at the door, so it fires the same way for the next buffer.
    ///
    /// Both directions, because a ceiling that refused everything would pass the second half on
    /// its own: the control is the FBB per-block maximum of five proposals, which must be answered
    /// in full and must leave the session nowhere near the ceiling.
    ///
    /// Measured discrimination: with that check removed, `cargo test -p tempo-core` reports
    /// exactly one failure — this test, at the `panic!` below, having fed all 200,000 lines.
    #[test]
    fn the_session_ceiling_bounds_every_buffer_at_the_door() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };

        // Control: five proposals, what FBB says one block carries, are answered in full.
        let mut session = Session::new(&cfg, Role::Client);
        session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
        let mut block = Vec::new();
        for i in 0..5 {
            block.extend_from_slice(format!("FC EM AAAAAAAAAAA{i} 10 10 0\r").as_bytes());
        }
        session.feed(&block);
        let held = session.held_bytes();
        // Read before the `F>`: `close_proposal_block` takes `proposals` and leaves a fresh one.
        let proposals_capacity = session.proposals.capacity();
        let actions = session.feed(format!("F> {:02X}\r", fbb::fb_checksum(&block)).as_bytes());
        assert!(
            actions.contains(&Action::Send(b"FS +++++\r".to_vec())),
            "a five-proposal block is legitimate and must be answered in full: {actions:?}"
        );
        // The header quotes both of these. 1,278 is what an open FBB-legal block charges all told;
        // it moved from 2,918 when the accounting changed from data bytes to retained heap
        // (`PROPOSAL_HELD_BYTES` now bounds the one field a `Proposal` owns rather than charging a
        // whole `MAX_LINE`, and `buf`/`block` are charged their capacity rather than their
        // length). 832 is the proposals' share of it, and it is asserted separately because the
        // header stated it as 520 for three rounds — five times `PROPOSAL_HELD_BYTES`, which is
        // `len()` arithmetic in the one paragraph that exists to say the charge is `capacity()`.
        assert_eq!(
            held, 1_278,
            "the charged cost of an open five-proposal block"
        );
        assert_eq!(
            (proposals_capacity, session.accepted.capacity()),
            (8, 8),
            "five proposals live in a container of eight, which is why the charge is not 5 × \
             PROPOSAL_HELD_BYTES"
        );
        assert_eq!(
            session.accepted.capacity() * PROPOSAL_HELD_BYTES,
            832,
            "the charged cost of five accepted proposals"
        );

        // The finding: `FC` lines for as long as the peer cares to send them, and no `F>`.
        let mut session = Session::new(&cfg, Role::Client);
        session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
        let line = b"FC EM AAAAAAAAAAAA 10 10 0\r";
        let mut failure = None;
        let mut lines = 0usize;
        // One line per `feed`, so the refusal is located to the line rather than to a chunk. The
        // cap is 200,000 because that is where the pre-fix measurement above stopped, not because
        // anything is expected to survive that long.
        for _ in 0..200_000 {
            let actions = session.feed(line);
            lines += 1;
            if let Some(Action::Failed(err)) = actions.last() {
                failure = Some((*err, actions));
                break;
            }
        }
        let Some((SessionError::Protocol(why), actions)) = failure else {
            panic!("a peer that never closes its block must be refused; it sent {lines} lines");
        };
        assert!(
            why.contains("five messages per block"),
            "the refusal must name what a legitimate block holds: {why}"
        );
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::Trace(t) if t.contains("ceiling"))),
            "the number held must be legible in the trace, not only in the failure: {actions:?}"
        );
        assert!(session.wants_close(), "the transport must be told to close");
        // The point is the ceiling, not the refusal: a session that only noticed once the peer
        // stopped would satisfy every assertion above and still be the defect.
        assert!(
            lines * line.len() < MAX_SESSION_HELD_BYTES,
            "the peer got {} wire bytes into us before the {MAX_SESSION_HELD_BYTES}-byte ceiling \
             fired",
            lines * line.len()
        );
    }

    /// The block sizes a peer picks, and the accounting that charges them.
    ///
    /// The finding this was written for: `held_bytes` counted peer *data bytes*, so a transfer
    /// delivered as one-byte `STX` blocks — three wire bytes each, all of them inside
    /// [`Session::transfer_ceiling`] — cost one `fbb::Frame` apiece and held tens of megabytes
    /// while the counter read a few. `failed=false`, `wants_close()=false`, and nothing ever took
    /// it back.
    ///
    /// Both directions, and the control is the point: the **same wire budget** in the 256-byte
    /// blocks a real sender uses must not be refused. What is under test is that the ceiling
    /// follows the memory the peer's framing choice costs, not the volume it sent — a ceiling that
    /// refused on volume would pass the first half and fail the second.
    ///
    /// Measured discrimination: put `framer` back to `framer: _` charging nothing — the accounting
    /// this replaced — and this test goes red; restore it and it goes green.
    #[test]
    fn the_door_charges_the_frames_a_peer_makes_us_hold_not_its_data_bytes() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        // At `MAX_PROPOSAL_C_SIZE` the transfer ceiling is 3 MiB + 260 wire bytes, so neither half
        // below can reach it: whatever refuses is the door.
        let open = |session: &mut Session| {
            session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
            let fc = format!("FC EM AAAAAAAAAAAA 10 {MAX_PROPOSAL_C_SIZE} 0\r").into_bytes();
            session.feed(&fc);
            let actions = session.feed(format!("F> {:02X}\r", fbb::fb_checksum(&fc)).as_bytes());
            assert!(
                actions.contains(&Action::Send(b"FS +\r".to_vec())),
                "the transfer must be accepted for either half to mean anything: {actions:?}"
            );
        };

        // The finding: one-byte blocks, and no `EOT` to make the framer let go.
        let mut session = Session::new(&cfg, Role::Client);
        open(&mut session);
        let mut wire = 0usize;
        let mut failure = None;
        for _ in 0..400_000 {
            let actions = session.feed(&[0x02, 0x01, b'A']);
            wire += 3;
            if let Some(Action::Failed(err)) = actions.last() {
                failure = Some(*err);
                break;
            }
        }
        let Some(SessionError::Protocol(why)) = failure else {
            panic!("a peer holding {wire} wire bytes of one-byte frames was not refused");
        };
        assert!(
            why.contains("five messages per block"),
            "the refusal must be the session ceiling, not the transfer ceiling: {why}"
        );
        assert!(session.wants_close(), "the transport must be told to close");
        // The point: refused on memory, long before the wire volume looks like anything. A
        // ceiling that only counted data bytes would have let all 400,000 blocks through.
        assert!(
            wire < MAX_SESSION_HELD_BYTES / 4,
            "the peer sent {wire} wire bytes before the {MAX_SESSION_HELD_BYTES}-byte ceiling \
             fired; that is data-byte accounting, not heap accounting"
        );

        // Control: the same wire budget in 256-byte blocks is what a real transfer looks like and
        // must be carried without complaint.
        let mut session = Session::new(&cfg, Role::Client);
        open(&mut session);
        let mut block = vec![0x02u8, 0x00];
        block.extend_from_slice(&[b'A'; 256]);
        let mut sent = 0usize;
        while sent < wire {
            let actions = session.feed(&block);
            sent += block.len();
            assert!(
                !actions.iter().any(|a| matches!(a, Action::Failed(_))),
                "{sent} wire bytes of full-size blocks must not be refused: {actions:?}"
            );
        }
        assert!(!session.wants_close(), "the control session is still open");
    }

    /// [`MAX_PROPOSAL_U_SIZE`], and that it is judged **before** anything is decompressed.
    ///
    /// The finding: [`Session::complete_transfer`] decompressed into a local and only then
    /// compared the result to the peer's `u-size`. A legal image well under
    /// [`MAX_PROPOSAL_C_SIZE`] — answered `FS +`, delivered as an `Action::Received` — expands by
    /// up to 47.9× and so allocated tens of megabytes before the comparison it was waiting for. A
    /// check performed after the allocation is not a bound, so the number is now judged at
    /// proposal time, where refusing costs one `FS -`.
    ///
    /// Both directions: a proposal at exactly the ceiling is legal and must be accepted.
    ///
    /// Measured discrimination: delete [`over_ceiling`]'s `u_size` arm and this test goes red;
    /// restore it and it goes green.
    #[test]
    fn a_proposal_over_the_uncompressed_ceiling_is_refused_before_anything_is_decompressed() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        let answer = |u_size: u64| {
            let mut session = Session::new(&cfg, Role::Client);
            session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
            let fc = format!("FC EM AAAAAAAAAAAA {u_size} 10 0\r").into_bytes();
            session.feed(&fc);
            let actions = session.feed(format!("F> {:02X}\r", fbb::fb_checksum(&fc)).as_bytes());
            (session, actions)
        };

        // Control: exactly at the ceiling is a legal message and is accepted. ⚠️ This control is
        // written in terms of the constant it tests, so it slides with it and can never see one
        // set too low — `the_ceilings_admit_the_largest_message_a_cms_account_can_hold` is the
        // control that does not move, and it is what fails if this ceiling is tightened.
        let (_, actions) = answer(u64::from(MAX_PROPOSAL_U_SIZE));
        assert!(
            actions.contains(&Action::Send(b"FS +\r".to_vec())),
            "a proposal at exactly the uncompressed ceiling must be accepted: {actions:?}"
        );

        for u_size in [u64::from(MAX_PROPOSAL_U_SIZE) + 1, u64::from(u32::MAX)] {
            let (mut session, actions) = answer(u_size);
            assert!(
                actions.contains(&Action::Send(b"FS -\r".to_vec())),
                "a proposal declaring {u_size} uncompressed bytes must be answered `-`: {actions:?}"
            );
            assert!(
                session.accepted.is_empty() && session.state != State::Binary,
                "a refused proposal must not open the binary phase: {:?}",
                session.state
            );
            assert!(
                actions
                    .iter()
                    .any(|a| matches!(a, Action::Trace(t) if t.contains("uncompressed ceiling"))),
                "the refusal must name which ceiling it was, not only that there was one: \
                 {actions:?}"
            );
            // Still a session: the refusal answers one message, it does not fail the peer.
            assert_eq!(session.feed(b"FF\r").last(), Some(&Action::Done));
        }
    }

    /// **The ceilings, measured against Winlink rather than against themselves.**
    ///
    /// The finding this exists for is about the *gate*, not the bound: every "both directions"
    /// control above answers `answer(MAX_PROPOSAL_U_SIZE)` or feeds "the same wire budget" the
    /// finding half established, so each control is written in terms of the constant it tests and
    /// slides with it. Measured: [`MAX_PROPOSAL_U_SIZE`] could be set to **4096** — refusing
    /// essentially every real message — and all 833 tests of this crate stayed green. The two
    /// constants are `pub`, so the next consumer to tighten one ships the narrowing with a green
    /// gate and the operator finds out when a served agency's traffic stops arriving.
    ///
    /// So the numbers here are **literals that do not move**, and every one of them is a fact
    /// about Winlink or about this coder rather than about this file:
    ///
    /// * **120,000 bytes compressed** — Winlink's own published per-account maximum, cited in full
    ///   at [`MAX_PROPOSAL_C_SIZE`]. The largest message that can reach a CMS mailbox at all.
    /// * **5,751,076 bytes uncompressed** — what an image that size unpacks to at the best ratio
    ///   this LZHUF variant reaches (47.9×, measured 2026-09-07 by bisection on a long-run body).
    ///   The largest plaintext any CMS-legal message can arrive as.
    ///
    /// Both are accepted, and a message that really does expand 47× is carried end to end, so a
    /// ceiling set anywhere below reality reddens this test with a message that names reality.
    #[test]
    fn the_ceilings_admit_the_largest_message_a_cms_account_can_hold() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        // The proposal a CMS could legitimately make at both of Winlink's own limits at once.
        let mut session = Session::new(&cfg, Role::Client);
        session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
        let fc = b"FC EM ABCDEFGHIJKL 5751076 120000 0\r".to_vec();
        session.feed(&fc);
        let actions = session.feed(format!("F> {:02X}\r", fbb::fb_checksum(&fc)).as_bytes());
        assert!(
            actions.contains(&Action::Send(b"FS +\r".to_vec())),
            "the largest message a Winlink account can hold — 120,000 bytes compressed, unpacking \
             to 5,751,076 at this coder's best ratio — must be accepted, whatever the ceilings \
             happen to be set to: {actions:?}"
        );

        // And one that really does expand that far, carried end to end. A long run is the shape
        // LZHUF codes best, so this is the ratio a proposal can legitimately declare — not a
        // contrived one.
        let (plain, image) = body_and_image(b"ABCDEFGHIJKL", &vec![b'Z'; 1_000_000]);
        assert!(
            plain.len() > 40 * image.len(),
            "this fixture only exercises a high ratio while it has one: {} plaintext bytes from \
             an image of {}",
            plain.len(),
            image.len()
        );
        let actions = deliver_at_block_size(b"ABCDEFGHIJKL", plain.len(), &image, 256);
        assert_eq!(
            failure(&actions),
            None,
            "a legitimate high-ratio message must not fail"
        );
        assert_eq!(
            delivered(&actions),
            vec![b"ABCDEFGHIJKL".to_vec()],
            "a message that expands {:.0}× must be delivered: {actions:?}",
            plain.len() as f64 / image.len() as f64
        );
    }

    /// **A message at Winlink's published maximum, at every block size the framing permits.**
    ///
    /// The reality anchor for [`MAX_SESSION_HELD_BYTES`] and [`MAX_PROPOSAL_C_SIZE`] together, and
    /// the reason the module header can say the small-block refusal costs no real mail. 120,000
    /// bytes is Winlink's own per-account maximum; one-byte `STX` blocks are the most wasteful
    /// framing B2F can express, three wire bytes per data byte. The two together are the worst
    /// thing a *legitimate* peer can do to this session, and it charges 5,156,288 of the
    /// 8,388,608-byte ceiling — 61.5 %, measured 2026-09-07.
    ///
    /// Measured discrimination: [`MAX_SESSION_HELD_BYTES`] at 4 MiB, or [`MAX_PROPOSAL_C_SIZE`] at
    /// anything under 120,000, and this test goes red naming the block size that stopped working.
    #[test]
    fn a_message_at_winlinks_published_maximum_is_carried_at_every_legal_block_size() {
        // 119,700 incompressible bytes land the image just under Winlink's cap; the assertions
        // are what say so, because the coder — not this fixture — decides the exact size.
        let (plain, image) = body_and_image(b"ABCDEFGHIJKL", &incompressible(119_700));
        assert!(
            image.len() <= 120_000 && image.len() > 119_000,
            "this fixture is only Winlink's maximum while its image is just under 120,000; it is \
             {} — re-tune the body length",
            image.len()
        );

        for block in [256usize, 125, 8, 1] {
            let actions = deliver_at_block_size(b"ABCDEFGHIJKL", plain.len(), &image, block);
            assert_eq!(
                failure(&actions),
                None,
                "a {}-byte message at Winlink's own maximum must not be refused in {block}-byte \
                 blocks",
                image.len()
            );
            assert_eq!(
                delivered(&actions),
                vec![b"ABCDEFGHIJKL".to_vec()],
                "the message must arrive in {block}-byte blocks"
            );
        }
    }

    /// **A transfer at [`MAX_PROPOSAL_C_SIZE`] is carried at the framings that fit, and the one
    /// that does not is named.**
    ///
    /// The coherence check between the two ceilings: this station answers `FS +` to a proposal at
    /// the compressed cap, so it has to be able to *carry* one at the framing a real sender uses.
    /// A [`MAX_SESSION_HELD_BYTES`] too small for what [`MAX_PROPOSAL_C_SIZE`] admits is the worst
    /// failure this file can have — the message is accepted and then the session dies mid-stream,
    /// with the peer told nothing.
    ///
    /// It also pins the boundary the module header states, which two rounds got wrong in prose:
    /// **8-byte blocks deliver, 7-byte blocks are refused**, at this transfer size. If a later
    /// change moves that boundary, this test is where it says so, and the header's paragraph moves
    /// with it.
    #[test]
    fn a_transfer_at_the_compressed_ceiling_is_carried_down_to_eight_byte_blocks() {
        // 1,047,800 incompressible bytes land the image just under the compressed ceiling.
        let (plain, image) = body_and_image(b"ABCDEFGHIJKL", &incompressible(1_047_800));
        assert!(
            image.len() <= MAX_PROPOSAL_C_SIZE as usize
                && image.len() > MAX_PROPOSAL_C_SIZE as usize - 4096,
            "this fixture is only at the compressed ceiling while its image is just under \
             {MAX_PROPOSAL_C_SIZE}; it is {} — re-tune the body length",
            image.len()
        );

        // 125 is `wl2k-go`'s sender; 8 is the smallest framing this ceiling leaves room for.
        for block in [125usize, 8] {
            let actions = deliver_at_block_size(b"ABCDEFGHIJKL", plain.len(), &image, block);
            assert_eq!(
                failure(&actions),
                None,
                "a transfer this station answered `FS +` to must be carriable in {block}-byte \
                 blocks: accepting a proposal and then failing the session is the one outcome \
                 worse than refusing it"
            );
            assert_eq!(
                delivered(&actions),
                vec![b"ABCDEFGHIJKL".to_vec()],
                "the message must arrive in {block}-byte blocks"
            );
        }

        // The other side of the boundary, and it is the door that says so rather than the
        // transfer ceiling: one byte less per block costs another 32-byte `Frame` slot each.
        let actions = deliver_at_block_size(b"ABCDEFGHIJKL", plain.len(), &image, 7);
        let Some(SessionError::Protocol(why)) = failure(&actions) else {
            panic!(
                "7-byte blocks at the compressed ceiling are over the session ceiling and must be \
                 refused there; if this now delivers, the header's stated boundary has moved and \
                 both it and this test need re-measuring: {actions:?}"
            );
        };
        assert!(
            why.contains("five messages per block"),
            "the refusal must be the session ceiling, not the transfer ceiling: {why}"
        );
        assert!(
            delivered(&actions).is_empty(),
            "a refused framing must not also deliver"
        );
    }

    /// [`MAX_MID`], which is what makes [`PROPOSAL_HELD_BYTES`] a bound rather than a number.
    ///
    /// The finding: the flat charge was justified by [`Session::drain_line`]'s [`MAX_LINE`]
    /// refusal, which only applied to a buffer holding no CR — so a `Proposal` could retain a
    /// megabytes-long MID while being charged a fraction of it. Two things close that, and this
    /// test covers the one in this function; `an_over_long_line_is_discarded_however_it_arrives`
    /// covers the other.
    ///
    /// **Deferred, not rejected** — [`super::fbb`]'s trap 1. `=` leaves the peer free to offer the
    /// message again; `-` would tell it the message is answered and destroy mail over a field
    /// length. Both directions, because a check that deferred everything would pass the first half.
    ///
    /// Measured discrimination: disable the `mid.len() > MAX_MID` arm of
    /// [`Session::handle_proposal`] and this test goes red; restore it and it goes green.
    #[test]
    fn a_proposal_whose_mid_is_over_the_maximum_is_deferred_not_retained() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        let answer = |mid_len: usize| {
            let mut session = Session::new(&cfg, Role::Client);
            session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
            let mid = "A".repeat(mid_len);
            let fc = format!("FC EM {mid} 10 10 0\r").into_bytes();
            assert!(fc.len() < MAX_LINE, "the line itself must be acceptable");
            let mut actions = session.feed(&fc);
            // What the open block retains, which is what `PROPOSAL_HELD_BYTES` charges for.
            let retained = session.proposals.iter().map(|p| p.mid.len()).sum::<usize>();
            actions.extend(session.feed(format!("F> {:02X}\r", fbb::fb_checksum(&fc)).as_bytes()));
            (session, retained, actions)
        };

        // The reality control, and it is a literal: a Winlink MID is twelve characters
        // (`wl2k-go`'s `MaxMIDLength = 12`), so any tightening of `MAX_MID` under 12 destroys
        // every real message and must redden here rather than sliding with the constant.
        let (session, retained, actions) = answer(12);
        assert_eq!(
            retained, 12,
            "a real 12-character Winlink MID is retained in full"
        );
        assert!(
            actions.contains(&Action::Send(b"FS +\r".to_vec())),
            "a real 12-character Winlink MID must be accepted: {actions:?}"
        );
        assert_eq!(
            session.accepted.front().map(|p| p.mid.len()),
            Some(12),
            "the accepted proposal must carry the MID it was offered"
        );

        // Control: exactly `MAX_MID` is accepted and kept whole.
        let (session, retained, actions) = answer(MAX_MID);
        assert_eq!(retained, MAX_MID, "a legal MID is retained in full");
        assert!(
            actions.contains(&Action::Send(b"FS +\r".to_vec())),
            "a MID at exactly the maximum must be accepted: {actions:?}"
        );
        assert_eq!(
            session.accepted.front().map(|p| p.mid.len()),
            Some(MAX_MID),
            "the accepted proposal must carry the MID it was offered"
        );

        // The finding: one byte over is deferred, and nothing over-long is retained.
        let (session, retained, actions) = answer(MAX_MID + 1);
        assert!(
            actions.contains(&Action::Send(b"FS =\r".to_vec())),
            "an over-long MID must be deferred, never rejected: {actions:?}"
        );
        assert!(
            session.accepted.is_empty(),
            "a deferred proposal must not be accepted"
        );
        assert_eq!(
            retained, 0,
            "an over-long MID must not be retained at all; PROPOSAL_HELD_BYTES charges \
             {PROPOSAL_HELD_BYTES} whatever it holds"
        );
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::Trace(t) if t.contains("over the"))),
            "the deferral must be legible in the trace, not silent: {actions:?}"
        );
    }

    /// [`MAX_LINE`] applies to the line rather than to a buffer that happens to hold no CR, and an
    /// over-long line is **dropped, not fatal**.
    ///
    /// Two findings, one rule. The first: the same 600-byte line failed the session when its CR
    /// had not arrived yet and was parsed when it had, so a peer's line length was judged by where
    /// the *caller's* read boundaries fell. The second, from the round that fixed the first: it
    /// resolved the inconsistency toward *always fatal*, which took a CMS banner or MOTD line of
    /// 512 bytes or more — arriving whole, inside one 4 KiB read — from traced-and-ignored to a
    /// dropped connection. Refusing a working server for saying hello is exactly the trap
    /// [`Session::handle_line`]'s last arm exists to avoid, and a length check above that arm
    /// silently overrode it.
    ///
    /// Three things are asserted here, and the reality control is the first because the constant
    /// has no meaning without it:
    ///
    /// 1. Every line a real session must *interpret* is carried: a CMS SID, a `;PQ:` challenge, a
    ///    `;FW:` list of twenty callsigns (164 bytes, the longest a peer sends), an `FC` line
    ///    whose MID is at [`MAX_MID`] (78), and the block that answers them. Shrink [`MAX_LINE`]
    ///    under any of those and this half reddens.
    /// 2. An over-long line is discarded and traced, the session survives it, and it survives it
    ///    identically whole or one byte at a time.
    /// 3. The memory this constant exists for is still bounded: a mebibyte of CR-less bytes leaves
    ///    the session holding kilobytes, not a mebibyte.
    ///
    /// Measured discrimination: make the over-long arm of [`Session::drain_line`] fail again and
    /// half 2 goes red; revert it to the CR-less-only rule and the split case of half 3 goes red.
    #[test]
    fn an_over_long_line_is_discarded_however_it_arrives() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        // A real CMS opening: SID, challenge, a forwarding list for twenty stations, and a
        // proposal carrying the longest MID this session will retain.
        let mut fw = b";FW:".to_vec();
        for i in 0..20 {
            fw.extend_from_slice(format!(" KD9TA{i:02}").as_bytes());
        }
        assert_eq!(
            fw.len(),
            164,
            "the ;FW: control is only a control at its real length"
        );
        let fc = format!("FC EM {} 10 10 0\r", "A".repeat(MAX_MID)).into_bytes();
        assert_eq!(fc.len(), 79, "the FC control carries a MID at MAX_MID");

        let mut session = Session::new(&cfg, Role::Client);
        let mut actions = session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
        fw.push(b'\r');
        actions.extend(session.feed(&fw));
        actions.extend(session.feed(&fc));
        actions.extend(session.feed(format!("F> {:02X}\r", fbb::fb_checksum(&fc)).as_bytes()));
        assert_eq!(failure(&actions), None, "no real line may fail a session");
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Action::Trace(t) if t.contains("over-long"))),
            "no line a real peer sends may be dropped for length: {actions:?}"
        );
        assert!(
            actions.contains(&Action::Send(b"FS +\r".to_vec())),
            "the block those lines make up must still be answered: {actions:?}"
        );

        // A banner line too long to interpret: dropped, traced, and the session goes on to work.
        // Whole and one byte at a time, because the arrival shape used to decide the outcome.
        for whole in [true, false] {
            let mut session = Session::new(&cfg, Role::Client);
            session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
            let mut line = vec![b'x'; 600];
            line.push(b'\r');
            let mut actions = Vec::new();
            if whole {
                actions.extend(session.feed(&line));
            } else {
                for byte in &line {
                    actions.extend(session.feed(&[*byte]));
                }
            }
            assert_eq!(
                failure(&actions),
                None,
                "a 601-byte MOTD line must not end the session (whole={whole}): {actions:?}"
            );
            assert!(
                actions
                    .iter()
                    .any(|a| matches!(a, Action::Trace(t) if t.contains("601 bytes"))),
                "the dropped line must be traced with its length (whole={whole}): {actions:?}"
            );
            // Still a session, not merely un-failed: the next line is read as a line.
            assert_eq!(session.feed(b"FF\r").last(), Some(&Action::Done));
        }

        // Control: the longest line the limit permits is interpreted, whole or split — so the
        // dropping above is a length decision and not a refusal of everything long.
        for whole in [true, false] {
            let mut session = Session::new(&cfg, Role::Client);
            session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
            let mut line = vec![b'x'; MAX_LINE - 1];
            line.push(b'\r');
            let mut actions = Vec::new();
            if whole {
                actions.extend(session.feed(&line));
            } else {
                for byte in &line {
                    actions.extend(session.feed(&[*byte]));
                }
            }
            assert_eq!(
                failure(&actions),
                None,
                "a line at the limit (whole={whole})"
            );
            assert!(
                actions
                    .iter()
                    .any(|a| matches!(a, Action::Trace(t) if t.contains("ignored non-protocol"))),
                "a line at the limit must reach `handle_line` (whole={whole}): {actions:?}"
            );
        }

        // And the memory the constant exists for: a peer that never sends a CR is holding this
        // session's buffer open, and it must not be able to grow it.
        let mut session = Session::new(&cfg, Role::Client);
        session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
        for _ in 0..256 {
            let actions = session.feed(&[b'x'; 4096]);
            assert_eq!(failure(&actions), None, "a CR-less flood is not a failure");
        }
        assert!(
            session.held_bytes() < 64 * 1024,
            "a mebibyte of CR-less bytes must leave the session holding kilobytes, not the \
             mebibyte: {} held",
            session.held_bytes()
        );
    }

    /// A session that has given up holds nothing.
    ///
    /// The finding: [`Session::close`] cleared five buffers and never touched `framer`, and
    /// `clear()` keeps a `Vec`'s capacity anyway — so a session reporting `wants_close()` true
    /// still held every buffer the peer had filled, for as long as its caller kept it. Its own doc
    /// said "everything unconsumed is dropped"; the largest thing was not.
    ///
    /// The assertion before the failure is the control: without it this test would pass just as
    /// well against a session that had never held anything.
    ///
    /// Measured discrimination: put [`Session::close`] back to five `clear()` calls that do not
    /// name `framer` and this test goes red; restore it and it goes green.
    #[test]
    fn a_closed_session_holds_nothing() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        let mut session = Session::new(&cfg, Role::Client);
        session.feed(b"[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
        let fc = format!("FC EM AAAAAAAAAAAA 10 {MAX_PROPOSAL_C_SIZE} 0\r").into_bytes();
        session.feed(&fc);
        session.feed(format!("F> {:02X}\r", fbb::fb_checksum(&fc)).as_bytes());
        for _ in 0..20_000 {
            session.feed(&[0x02, 0x01, b'A']);
        }
        let before = session.held_bytes();
        assert!(
            before > 500_000,
            "the session must actually be holding something before it lets go: {before}"
        );

        // `0x03` is not a framing marker: the framer latches `Malformed` and the session fails.
        let actions = session.feed(&[0x03]);
        assert!(
            matches!(actions.last(), Some(Action::Failed(SessionError::Fbb(_)))),
            "the malformed byte must fail the session: {actions:?}"
        );
        assert!(session.wants_close());
        assert_eq!(
            session.held_bytes(),
            0,
            "a closed session must not go on owning the peer's buffers"
        );
    }

    /// Nothing is consumed after the session ends — a closed session that kept parsing could
    /// deliver a message from bytes that arrived after a checksum failure.
    #[test]
    fn a_closed_session_consumes_nothing_more() {
        let cfg = ClientConfig {
            callsign: "N0CALL".into(),
            password: "pw".into(),
        };
        let mut session = Session::new(&cfg, Role::Client);
        session.feed(b"[WL2K-5.0-B2FWIHJM$]\rFQ\r");
        assert!(session.wants_close());
        assert!(session
            .feed(b"FC EM AAAAAAAAAAAA 1 1 0\rF> 00\r")
            .is_empty());
    }
}
