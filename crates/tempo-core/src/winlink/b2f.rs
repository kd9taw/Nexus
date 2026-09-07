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
//! # The resource bound, in two halves
//!
//! [`fbb::Framer`] holds every data block back until an `EOT` vouches for it — that is what makes
//! "a corrupt block delivers nothing" true — so an inbound transfer accumulates in memory with
//! nothing inside the framer to stop it, and `fbb`'s module header says so out loud and delegates
//! the limit here: *"The number that bounds it is the proposal's `c-size`, which the session
//! holds."* This stream is remote-controlled — the transport underneath is a socket to a CMS that
//! may be buggy, MITM'd or hostile — so an unbounded `Vec` on the far end of one is the exposure,
//! and it takes two bounds to close, not one.
//!
//! 1. **A transfer may not run past what its own proposal declared.**
//!    [`Session::transfer_ceiling`] is that number, and [`Session::drain_binary`] checks it before
//!    each byte reaches the framer. A peer that streams `STX` blocks and never sends `EOT` is
//!    refused the moment it exceeds its own `FC` line. Exceeding it is [`SessionError::Protocol`]
//!    — the session ends — and never a truncation: a truncated body would be a *shorter* message
//!    delivered as if it were the whole one.
//! 2. **A proposal may not declare more than [`MAX_PROPOSAL_C_SIZE`].** Bound 1 on its own is
//!    peer-declared and therefore not a bound at all: `c-size` is a `u32` that costs the peer
//!    nothing to inflate, so `FC EM <mid> 4294967295 4294967295 0` sets bound 1 at
//!    `3 * 4294967295 + 260` — 12 GiB of wire bytes it may stream, accumulating, before bound 1
//!    can fire. The absolute cap is what no peer-supplied number can raise, and it is applied at
//!    **proposal** time — the slot is answered `-` and the body never starts — so the outcome is
//!    a legible refusal in the trace rather than a session killed mid-stream.
//!
//! Together they are what makes bound 1's ceiling finite: [`Session::transfer_ceiling`] reads a
//! `c-size` that acceptance has already capped.
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

/// Longest FBB command line accepted before the stream is called a desync. Every line in the
/// grammar is short — the longest realistic one is an `FS` answer for a very large block — so a
/// buffer this size with no CR in it means we are no longer aligned to the protocol, not that a
/// long line is in flight. Matching `aprsis`'s bound, and unlike `aprsis` this is a refusal
/// rather than a silent buffer reset: there is no packet stream here to resynchronise into.
const MAX_LINE: usize = 512;

/// The largest compressed message size this station will accept a proposal for, in bytes.
///
/// **An absolute ceiling that no peer-supplied number can raise**, and the reason
/// [`Session::transfer_ceiling`] is a bound rather than an echo — see the module header's
/// resource-bound section for why one without the other closes nothing.
///
/// Winlink's own limit is **120,000 bytes compressed**: the per-account `MAX SIZE` option is
/// documented, verbatim, as "a numerical value setting the size in bytes (compressed) of the
/// largest message you will accept. 120000 Bytes is maximum, and the default."
/// (<https://winlink.org/content/how_change_your_account_settings_option_message_useroptions>,
/// read 2026-09-07). A message larger than that cannot reach a CMS mailbox, so it cannot
/// legitimately be proposed to us either.
///
/// This constant is **1 MiB, roughly nine times that**, and the headroom is deliberate. The cost
/// of being generous is bounded and small — bound 1 then sits at `3 * 1048576 + 260`, 3.0 MiB,
/// on a machine already running a waterfall — while the cost of being tight is refusing real
/// mail, which is the one outcome an email transport may not have. Winlink raising its own limit
/// must not turn into Nexus silently dropping messages, so this is sized to absorb that without a
/// release.
pub const MAX_PROPOSAL_C_SIZE: u32 = 1024 * 1024;

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
    buf: Vec<u8>,
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
            buf: Vec::new(),
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
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Action> {
        let mut out = Vec::new();
        if self.state == State::Closed {
            return out;
        }
        self.buf.extend_from_slice(chunk);
        // Alternates between the two readers until neither can make progress: a line can put the
        // session into the binary phase and a completed transfer can put it back, both in the
        // middle of one chunk.
        loop {
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

    /// Whether the transport may close: the session is finished, or has failed and will consume
    /// nothing further.
    pub fn wants_close(&self) -> bool {
        self.state == State::Closed
    }

    // -- line phase ---------------------------------------------------------------------------

    /// Consumes one complete CR-terminated line, if the buffer holds one. Returns whether it did.
    fn drain_line(&mut self, out: &mut Vec<Action>) -> bool {
        let Some(at) = self.buf.iter().position(|&b| b == b'\r') else {
            if self.buf.len() > MAX_LINE {
                self.fail(
                    out,
                    SessionError::Protocol("no CR within the maximum FBB line length"),
                );
            }
            return false;
        };
        let raw: Vec<u8> = self.buf.drain(..=at).collect();
        self.handle_line(trim_line(&raw), out);
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
        self.block.clear();
        for proposal in proposals.iter().filter(|p| over_ceiling(p)) {
            out.push(Action::Trace(format!(
                "refusing {}: its FC line declares {} compressed bytes, over the {}-byte ceiling",
                show(&proposal.mid),
                proposal.c_size,
                MAX_PROPOSAL_C_SIZE
            )));
        }
        let answer = fbb::fs_answer(&proposals, |proposal| {
            if over_ceiling(proposal) {
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
    /// **This number is finite because acceptance made it so, not because of anything here.**
    /// `c_size` is a peer-supplied `u32`; what keeps `3 * c_size` from reaching 12 GiB is that
    /// [`Session::close_proposal_block`] answers `-` to any proposal declaring more than
    /// [`MAX_PROPOSAL_C_SIZE`], so nothing above the cap ever enters `accepted` to be read here.
    /// A future change that accepts a proposal without that test re-opens the exposure, and this
    /// function will not notice.
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
        let plain = match lzhuf::decompress(&image) {
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
    fn close(&mut self) {
        self.state = State::Closed;
        self.buf.clear();
        self.block.clear();
        self.proposals.clear();
        self.accepted.clear();
        self.image.clear();
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

/// Whether a proposal declares more compressed bytes than this station will accept.
///
/// One function rather than the condition written twice, because the trace and the `FS` answer
/// are two consumers of one decision: written out at both sites, a later edit that relaxed the
/// comparison in the answer alone would leave the session tracing "refusing" and then answering
/// `+`. See [`MAX_PROPOSAL_C_SIZE`].
fn over_ceiling(proposal: &Proposal) -> bool {
    proposal.c_size > MAX_PROPOSAL_C_SIZE
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

    /// The module header's resource bound, and the reason it is not `fbb`'s to take: a peer that
    /// streams `STX` blocks and never sends the `EOT` that would let `complete_transfer` compare
    /// lengths is bounded by nothing else. `Framer` holds every block back until an `EOT` vouches
    /// for it, so those bytes are live memory, and the transport underneath is a socket to a CMS
    /// that may be buggy, MITM'd or hostile.
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

    /// One `SOH`/`STX`/`EOT` transfer on the wire: header record, then the image in 256-byte data
    /// blocks, then the `EOT` and its checksum over the data bytes alone.
    fn transfer_records(mid: &[u8], image: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut header = mid.to_vec();
        header.push(0);
        header.push(b'0');
        header.push(0);
        out.push(0x01);
        out.push(header.len() as u8);
        out.extend_from_slice(&header);
        let mut sum = 0u8;
        for chunk in image.chunks(256) {
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
        // Deliberately incompressible (xorshift64, fixed seed): prose of this length collapses to
        // an image far too small for the first transfer to overrun the second proposal's ceiling,
        // and the assertion below is what caught that.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let long: Vec<u8> = std::iter::repeat_with(|| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 24) as u8
        })
        .take(1500)
        .collect();
        let (plain_a, image_a) = body_and_image(b"AAAAAAAAAAAA", &long);
        let (plain_b, image_b) = body_and_image(b"BBBBBBBBBBBB", b"ok");

        let wire_a = transfer_records(b"AAAAAAAAAAAA", &image_a);
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
        actions.extend(session.feed(&transfer_records(b"BBBBBBBBBBBB", &image_b)));

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

    /// A proposal is answered against `FC` line for `c_size` compressed bytes, greeted and
    /// challenged, returning the actions the `F>` produced.
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

    /// [`MAX_PROPOSAL_C_SIZE`], both sides of it — bound 2 from the module header.
    ///
    /// Bound 1 ([`Session::transfer_ceiling`]) is `3 * c_size + 260`, and `c_size` is a `u32` the
    /// **peer** writes: `FC EM <mid> 4294967295 4294967295 0` costs a hostile or buggy CMS one
    /// line and sets bound 1 at 12 GiB of wire bytes. So the cap is what makes
    /// bound 1 a bound, and it has to be refused where the exposure is cheap — at the proposal,
    /// answered `-`, before a byte of body is asked for.
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
