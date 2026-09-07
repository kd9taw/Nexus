//! B2F session replay. The golden transcript is replayed WHOLE, one byte at a time, and in
//! seeded-random splits — the chunk-boundary bug class FlexCat paid for. The five negative
//! controls ship in this same commit as the engine, because passing fixtures prove nothing alone.
//!
//! What the golden replays can and cannot prove is written into the fixture's own header and is
//! worth repeating here: `session1.trace` is a **constructed** transcript, not a live capture, so
//! a green run here says this engine is self-consistent across every chunk boundary and says
//! nothing about what a CMS will accept. The oracle is fixture #2, captured at the first live
//! connect. The seven controls below are the part that does not depend on the fixture being
//! right: each one perturbs a byte that a correct engine MUST reject, so a passing control is
//! evidence about the engine rather than about the transcript. (Five shipped with the engine;
//! controls 6 and 7 pin the two proposal-size cross-checks, which review found untested.)

use tempo_core::winlink::message::{Attachment, Message};
use tempo_core::winlink::{b2f, fbb, secure, sid, ClientConfig};

/// The callsign the fixture's outbound side was computed for. Changing it invalidates the
/// `;FW:`, `;` identity and `;PR:` records in one go.
const CALLSIGN: &str = "N0CALL";
/// The password the fixture's `;PR:` record was computed for. Not a credential for anything —
/// see the fixture header.
const PASSWORD: &str = "NEXUSTEST";
/// The `;PQ:` challenge value carried by the fixture, needed by the `;PR:` control.
const CHALLENGE: &[u8] = b"41913235";

// ---------------------------------------------------------------------------------------------
// The fixture
// ---------------------------------------------------------------------------------------------

/// One direction-marked record from the trace file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    /// `true` when the peer sent it to us, `false` when we are expected to send it.
    inbound: bool,
    bytes: Vec<u8>,
}

/// Parses `session1.trace`. The encoding is documented in the fixture's own header: `#` and empty
/// lines are comments, every other line is `<`/`>` then a space then lowercase hex.
fn records() -> Vec<Record> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/winlink/session1.trace"
    );
    let text = std::fs::read_to_string(path).expect("fixture is missing");
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (dir, hex) = line.split_at(1);
        let inbound = match dir {
            "<" => true,
            ">" => false,
            _ => panic!("line {}: record must start with '<' or '>': {line}", n + 1),
        };
        let hex = hex.trim();
        assert!(
            hex.len() % 2 == 0 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
            "line {}: record body must be an even number of hex digits",
            n + 1
        );
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        out.push(Record { inbound, bytes });
    }
    assert!(!out.is_empty(), "fixture parsed to no records at all");
    out
}

/// Every inbound record, concatenated — the byte stream the engine is fed.
fn inbound_of(recs: &[Record]) -> Vec<u8> {
    recs.iter()
        .filter(|r| r.inbound)
        .flat_map(|r| r.bytes.clone())
        .collect()
}

/// Every outbound record, concatenated — the byte stream the engine must produce. Concatenated
/// rather than compared record by record, deliberately: how the engine batches its writes is not
/// a protocol fact and the fixture must not pin it.
fn outbound_of(recs: &[Record]) -> Vec<u8> {
    recs.iter()
        .filter(|r| !r.inbound)
        .flat_map(|r| r.bytes.clone())
        .collect()
}

/// Replaces the first record matching `pick` with `bytes`. Used by the negative controls, which
/// are all "the golden transcript with exactly one thing wrong".
fn with_record(recs: &[Record], pick: impl Fn(&Record) -> bool, bytes: Vec<u8>) -> Vec<Record> {
    let mut out = recs.to_vec();
    let at = out.iter().position(&pick).expect("no record matched");
    out[at].bytes = bytes;
    out
}

/// The binary transfer record — the one inbound record that is not a CR-terminated command line.
fn is_transfer(r: &Record) -> bool {
    r.inbound && r.bytes.first() == Some(&0x01)
}

// ---------------------------------------------------------------------------------------------
// The replay harness
// ---------------------------------------------------------------------------------------------

/// Drives a fresh [`b2f::Session`] over `chunks` and collects every action, in order.
fn replay(chunks: impl Iterator<Item = Vec<u8>>) -> Vec<b2f::Action> {
    replay_with(PASSWORD, chunks).0
}

/// [`replay`], plus the password to log in with and the session's final `wants_close()`.
fn replay_with(password: &str, chunks: impl Iterator<Item = Vec<u8>>) -> (Vec<b2f::Action>, bool) {
    let cfg = ClientConfig {
        callsign: CALLSIGN.to_string(),
        password: password.to_string(),
    };
    let mut session = b2f::Session::new(&cfg, b2f::Role::Client);
    let mut actions = Vec::new();
    for chunk in chunks {
        actions.extend(session.feed(&chunk));
    }
    (actions, session.wants_close())
}

/// Every byte the session asked to write, concatenated.
fn sent(actions: &[b2f::Action]) -> Vec<u8> {
    actions
        .iter()
        .filter_map(|a| match a {
            b2f::Action::Send(bytes) => Some(bytes.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Every message the session delivered.
fn delivered(actions: &[b2f::Action]) -> Vec<Message> {
    actions
        .iter()
        .filter_map(|a| match a {
            b2f::Action::Received(msg) => Some(msg.clone()),
            _ => None,
        })
        .collect()
}

/// Every failure the session reported.
fn failures(actions: &[b2f::Action]) -> Vec<b2f::SessionError> {
    actions
        .iter()
        .filter_map(|a| match a {
            b2f::Action::Failed(err) => Some(*err),
            _ => None,
        })
        .collect()
}

/// A deterministic byte-stream splitter: xorshift32 chunk sizes in `1..=max`, so a seed names an
/// exact set of boundaries and a failure is reproducible from the seed alone.
fn seeded_splits(stream: &[u8], seed: u32, max: u32) -> Vec<Vec<u8>> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1) | 1;
    let mut out = Vec::new();
    let mut i = 0;
    while i < stream.len() {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let take = (state % max + 1) as usize;
        let end = (i + take).min(stream.len());
        out.push(stream[i..end].to_vec());
        i = end;
    }
    out
}

/// The one message the golden transcript carries, spelled out rather than re-derived: a test that
/// decompressed the fixture to build its own expectation would agree with any codec at all.
fn golden_message() -> Message {
    let crlf = |lines: &[&str]| lines.join("\r\n").into_bytes();
    Message {
        mid: b"ABCDEFGHIJKL".to_vec(),
        headers: vec![
            (b"Date".to_vec(), b"2026/09/06 12:00".to_vec()),
            (b"Type".to_vec(), b"Private".to_vec()),
            (b"From".to_vec(), b"SMTP:netcontrol@example.com".to_vec()),
            (b"To".to_vec(), b"N0CALL".to_vec()),
            (b"Subject".to_vec(), b"Net check-in".to_vec()),
            (b"Mbo".to_vec(), b"WL2K".to_vec()),
        ],
        body: crlf(&[
            "Net control: all stations please check in on",
            "146.520 simplex at 1900Z. KD9TAW is NCS.",
            "",
        ]),
        attachments: vec![Attachment {
            name: b"RMS_Express_Form_ICS213.xml".to_vec(),
            data: crlf(&[
                "<?xml version=\"1.0\"?>",
                "<RMS_Express_Form>",
                "  <form_parameters>",
                "    <display_form>ICS213.html</display_form>",
                "  </form_parameters>",
                "  <variables>",
                "    <Subject>Net check-in</Subject>",
                "    <Message>Net control: all stations please check in on 146.520 simplex at 1900Z.</Message>",
                "  </variables>",
                "</RMS_Express_Form>",
                "",
            ]),
        }],
    }
}

/// The assertions every successful replay of the golden transcript must satisfy, in one place so
/// the three chunkings cannot drift apart.
fn assert_golden(actions: &[b2f::Action], recs: &[Record], how: &str) {
    assert_eq!(failures(actions), Vec::new(), "{how}: session failed");
    assert_eq!(
        sent(actions),
        outbound_of(recs),
        "{how}: outbound byte stream does not match the transcript"
    );
    assert_eq!(
        delivered(actions),
        vec![golden_message()],
        "{how}: delivered messages do not match the transcript"
    );
    assert_eq!(
        actions.last(),
        Some(&b2f::Action::Done),
        "{how}: the session did not end with Done"
    );
}

// ---------------------------------------------------------------------------------------------
// The golden transcript, replayed three ways
// ---------------------------------------------------------------------------------------------

#[test]
fn golden_replays_whole() {
    let recs = records();
    let (actions, closed) = replay_with(PASSWORD, std::iter::once(inbound_of(&recs)));
    assert_golden(&actions, &recs, "whole");
    assert!(closed, "the transport should be told it may close");
}

#[test]
fn golden_replays_one_byte_at_a_time() {
    let recs = records();
    let stream = inbound_of(&recs);
    let actions = replay(stream.iter().map(|b| vec![*b]));
    assert_golden(&actions, &recs, "one byte at a time");
}

#[test]
fn golden_replays_seeded_random_splits() {
    let recs = records();
    let stream = inbound_of(&recs);
    for seed in 0..64 {
        let splits = seeded_splits(&stream, seed, 97);
        // The harness's own positive control: a degenerate splitter would turn this test into a
        // second copy of one of the two above and prove nothing new.
        assert!(
            splits.len() > 1 && splits.iter().any(|chunk| chunk.len() > 1),
            "seed {seed}: the splitter produced no real chunk boundaries"
        );
        let actions = replay(splits.into_iter());
        assert_golden(&actions, &recs, &format!("seed {seed}"));
    }
}

// ---------------------------------------------------------------------------------------------
// The five negative controls (spec §5)
// ---------------------------------------------------------------------------------------------

/// Control 1. A single flipped byte inside the `FC` line must be caught by the `F>` block
/// checksum, at the proposal exchange, before any body crosses.
#[test]
fn perturbed_proposal_fails_on_proposal_checksum() {
    let recs = records();
    let recs = with_record(
        &recs,
        |r| r.bytes.starts_with(b"FC "),
        b"FC EM ABCDEFGHIJKM 598 365 0\r".to_vec(),
    );
    let actions = replay(std::iter::once(inbound_of(&recs)));
    assert_eq!(
        failures(&actions),
        vec![b2f::SessionError::Fbb(fbb::FbbError::ProposalChecksum)],
        "a perturbed FC line must fail on the F> checksum"
    );
    assert!(
        delivered(&actions).is_empty(),
        "nothing may be delivered from a corrupt proposal block"
    );
}

/// Control 2. A corrupt EOT checksum must fail **and must deliver nothing** — the whole reason
/// the framer holds a block back until its own EOT vouches for it.
#[test]
fn bad_eot_checksum_fails_and_does_not_deliver() {
    let recs = records();
    let mut transfer = recs
        .iter()
        .find(|r| is_transfer(r))
        .expect("no binary transfer record in the fixture")
        .bytes
        .clone();
    let last = transfer.len() - 1;
    transfer[last] ^= 0x01;
    let recs = with_record(&recs, is_transfer, transfer);
    let actions = replay(std::iter::once(inbound_of(&recs)));
    assert_eq!(
        failures(&actions),
        vec![b2f::SessionError::Fbb(fbb::FbbError::EotChecksum)],
        "a corrupt EOT checksum must fail the transfer"
    );
    assert!(
        delivered(&actions).is_empty(),
        "a block whose EOT checksum failed must deliver nothing"
    );
}

/// Control 3. A peer that forwards but does not offer B2 must be refused **at the handshake**.
/// Both halves: the parser refuses the line, and the session refuses the peer without ever
/// answering it — a refusal that still sent our callsign and `;PR:` would have logged in first.
#[test]
fn sid_without_b2_refuses() {
    assert!(matches!(
        sid::parse_sid(b"[FBB-7.00i-B1FHM$]\r"),
        Err(sid::SidError::MissingB2)
    ));

    let recs = records();
    let recs = with_record(
        &recs,
        |r| r.bytes.starts_with(b"[WL2K"),
        b"[FBB-7.00i-B1FHM$]\r".to_vec(),
    );
    let actions = replay(std::iter::once(inbound_of(&recs)));
    assert_eq!(
        failures(&actions),
        vec![b2f::SessionError::Sid(sid::SidError::MissingB2)],
        "a B1-only peer must be refused"
    );
    assert!(
        sent(&actions).is_empty(),
        "a refused peer must not be answered at all"
    );
    assert!(delivered(&actions).is_empty());
}

/// Control 4. Flags nobody has documented must not refuse a working peer — the
/// forward-compatibility trap `sid.rs` exists to avoid. The session must run to completion and
/// produce byte-identical output.
#[test]
fn sid_with_invented_flags_proceeds() {
    assert!(sid::parse_sid(b"[WL2K-9.9-B2FWIHJMQXZ$]\r").is_ok());

    let recs = records();
    let perturbed = with_record(
        &recs,
        |r| r.bytes.starts_with(b"[WL2K"),
        b"[WL2K-9.9-B2FWIHJMQXZ$]\r".to_vec(),
    );
    let actions = replay(std::iter::once(inbound_of(&perturbed)));
    assert_golden(&actions, &recs, "invented flags");
}

/// Control 5 — the integration half of `secure.rs`'s `one_byte_salt_change_changes_pr`.
///
/// The salt-byte perturbation itself runs in `secure.rs`, because `pr_with_salt` is private on
/// purpose: nothing outside that module may choose a salt. What can be proved *here*, and is what
/// this control is for, is that the session's `;PR:` is produced by that exact derivation and not
/// by a constant, a cached token or a second copy of the arithmetic that could have dropped the
/// salt on the way. Both live inputs are then perturbed one at a time, so a session that ignored
/// either would fail here even though the unit control passed.
#[test]
fn one_byte_salt_change_changes_pr() {
    let recs = records();
    let actions = replay(std::iter::once(inbound_of(&recs)));
    let pr = pr_token(&sent(&actions)).expect("the session sent no ;PR: line");
    assert_eq!(
        pr,
        secure::pr_response(CHALLENGE, PASSWORD.as_bytes()),
        "the session's ;PR: must be exactly what secure::pr_response derives"
    );

    // One byte of the password changed — a session holding a constant would not notice.
    let (other, _) = replay_with("NEXUSTES!", std::iter::once(inbound_of(&recs)));
    assert_ne!(
        pr_token(&sent(&other)),
        Some(pr.clone()),
        "a one-byte password change must change ;PR:"
    );

    // One byte of the challenge changed — a session that answered before reading it would not.
    let recs = with_record(
        &records(),
        |r| r.bytes.starts_with(b";PQ:"),
        b";PQ: 41913236\r".to_vec(),
    );
    let (other, _) = replay_with(PASSWORD, std::iter::once(inbound_of(&recs)));
    assert_ne!(
        pr_token(&sent(&other)),
        Some(pr),
        "a one-byte challenge change must change ;PR:"
    );
}

/// The eight bytes after `;PR: ` in an outbound stream.
fn pr_token(stream: &[u8]) -> Option<Vec<u8>> {
    let at = stream.windows(5).position(|w| w == b";PR: ")?;
    let from = at + 5;
    Some(stream[from..from + 8].to_vec())
}

/// The golden transcript with the `FC` line replaced and its `F>` checksum **recomputed over the
/// new line**, so the proposal block still passes its own integrity check and the only thing
/// wrong with the session is the pair of numbers `complete_transfer` cross-checks. Without the
/// recomputation these two controls would be a second copy of control 1 and would say nothing
/// about the size checks at all.
fn replay_with_proposal(fc: &[u8]) -> Vec<b2f::Action> {
    let recs = with_record(&records(), |r| r.bytes.starts_with(b"FC "), fc.to_vec());
    let recs = with_record(
        &recs,
        |r| r.bytes.starts_with(b"F> "),
        format!("F> {:02X}\r", fbb::fb_checksum(fc)).into_bytes(),
    );
    replay(std::iter::once(inbound_of(&recs)))
}

/// Control 6. The `c-size` the peer proposed is how many body bytes the transfer must actually
/// carry. The transcript's body is 365 bytes; a proposal claiming 366 must be refused rather than
/// delivered, because a body that is not the length its own proposal declared is a desync — the
/// framer verified the bytes it was given, not that they were all of them.
///
/// The exact reason string is pinned deliberately: it is what distinguishes this check from the
/// `u-size` one below, and a test that accepted either would still pass with this one deleted.
#[test]
fn a_c_size_that_disagrees_with_the_body_fails_before_delivery() {
    let actions = replay_with_proposal(b"FC EM ABCDEFGHIJKL 598 366 0\r");
    assert_eq!(
        failures(&actions),
        vec![b2f::SessionError::Protocol(
            "compressed body length disagrees with its FC proposal"
        )],
        "a body that is not its proposal's compressed length must fail the session"
    );
    assert!(
        delivered(&actions).is_empty(),
        "nothing may be delivered from a transfer whose length disagrees with its proposal"
    );
}

/// Control 7 — the outer truncation check `message.rs` delegates here.
///
/// That module's header records the one corruption it cannot see from inside a message: "a
/// declared length that is short by exactly the amount that leaves a CRLF at the cut is
/// undetectable ... The FBB proposal's `<u-size>` is the outer check for that, and it lives in
/// the session, not here." This is that check. The transcript decompresses to 598 bytes; a
/// proposal claiming 599 must be refused, or a silently truncated message is delivered and filed
/// with nothing anywhere having noticed.
#[test]
fn a_u_size_that_disagrees_with_the_decompressed_body_fails_before_delivery() {
    let actions = replay_with_proposal(b"FC EM ABCDEFGHIJKL 599 365 0\r");
    assert_eq!(
        failures(&actions),
        vec![b2f::SessionError::Protocol(
            "decompressed body length disagrees with its FC proposal"
        )],
        "a body that is not its proposal's uncompressed length must fail the session"
    );
    assert!(
        delivered(&actions).is_empty(),
        "a message whose size disagrees with its proposal must not be delivered"
    );
}

// ---------------------------------------------------------------------------------------------
// Two more the engine has to survive
// ---------------------------------------------------------------------------------------------

/// The order of the peer's SID and its `;PQ:` challenge is not established from the documents
/// (see the `b2f` module header), so the engine must produce the same session either way.
#[test]
fn greeting_order_does_not_change_the_session() {
    let recs = records();
    let mut swapped = recs.clone();
    swapped.swap(0, 1);
    assert!(swapped[0].bytes.starts_with(b";PQ:") && swapped[1].bytes.starts_with(b"[WL2K"));
    let actions = replay(std::iter::once(inbound_of(&swapped)));
    assert_golden(&actions, &recs, "challenge before SID");
}

/// `wants_close` is the transport's signal, so it must be false for exactly as long as the
/// session still has something to say.
#[test]
fn wants_close_only_after_the_session_ends() {
    let recs = records();
    let cfg = ClientConfig {
        callsign: CALLSIGN.to_string(),
        password: PASSWORD.to_string(),
    };
    let mut session = b2f::Session::new(&cfg, b2f::Role::Client);
    assert!(!session.wants_close(), "a fresh session must stay open");
    let stream = inbound_of(&recs);
    let mut ended = None;
    for (i, byte) in stream.iter().enumerate() {
        session.feed(&[*byte]);
        if session.wants_close() {
            ended = Some(i);
            break;
        }
    }
    assert_eq!(
        ended,
        Some(stream.len() - 1),
        "the session must close on the last transcript byte and not before"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════════════════
// The mailbox's storage claim, proved rather than assumed.
//
// `mailbox.rs`'s module header says each received message is written "exactly as it went over
// the wire". The driver that will store received mail is handed a PARSED `Message` by
// `Action::Received`, not the plaintext, so what it can actually write is `assemble_b2(&msg)` —
// a re-serialisation. The claim is therefore true only if that round trip is the identity, and
// these two tests are what makes that a fact instead of a hope.
// ═══════════════════════════════════════════════════════════════════════════════════════════

#[test]
fn a_received_message_re_assembles_to_the_bytes_it_arrived_as() {
    let recs = records();
    let actions = replay(std::iter::once(inbound_of(&recs)));
    let received: Vec<&tempo_core::winlink::message::Message> = actions
        .iter()
        .filter_map(|a| match a {
            b2f::Action::Received(m) => Some(m),
            _ => None,
        })
        .collect();
    assert!(
        !received.is_empty(),
        "the fixture delivers no message to test"
    );
    for msg in received {
        let re = tempo_core::winlink::message::assemble_b2(msg).expect("assemble");
        let back = tempo_core::winlink::message::parse_b2(&re).expect("parse");
        assert_eq!(&back, msg, "assemble->parse is not the identity");
    }
}

#[test]
fn parsing_and_re_assembling_a_b2_blob_returns_the_same_bytes() {
    // The stronger claim, and the one the mailbox actually rests on: for a real B2 blob,
    // assemble_b2(parse_b2(x)) == x, byte for byte. If this is red the mailbox is storing a
    // NORMALISED copy and the design has to change, not the assertion.
    //
    // Note the lengths: `Body: 5` for "hello", and the CRLF after it is the SEPARATOR, not part
    // of the body (module header, "why every part is followed by CRLF"). A `Body: 7` that
    // swallowed the separator is `Malformed`, correctly, and was this test's first draft.
    let blob: &[u8] = b"Mid: ABCDEFGHIJKL\r\n\
                        Date: 2026/09/07 12:00\r\n\
                        From: SMTP:someone@example.com\r\n\
                        To: N0CALL\r\n\
                        Subject: hello\r\n\
                        Body: 5\r\n\
                        File: 4 a.txt\r\n\
                        \r\n\
                        hello\r\n\
                        abcd\r\n";
    let msg = tempo_core::winlink::message::parse_b2(blob).expect("parse");
    let re = tempo_core::winlink::message::assemble_b2(&msg).expect("assemble");
    assert_eq!(
        String::from_utf8_lossy(&re),
        String::from_utf8_lossy(blob),
        "re-assembly is not byte-identical — the mailbox would be storing a normalised copy"
    );
}
