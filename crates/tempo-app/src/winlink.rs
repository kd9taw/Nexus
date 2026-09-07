//! The Winlink session driver — where spec Layer A (the protocol) meets Layer B (the socket).
//!
//! `tempo_core::winlink::b2f::Session` speaks B2F over a byte buffer and knows nothing about
//! sockets; `tempo_net::wl2k` owns a socket and knows nothing about B2F (it cannot — `tempo-net`
//! is a leaf over `std` with no `tempo-core` dependency). [`Driver`] is the join: it owns the
//! session, implements [`tempo_net::wl2k::ByteSession`], answers the peer's proposals from this
//! station's own mailbox, and writes every received message to disk.
//!
//! Because the seam is a trait rather than a socket, the **same** driver will be handed to the
//! ARDOP transport with no change here — which is the whole reason the telnet path can settle the
//! wire questions with no radio involved.
//!
//! # The ordering rule, expressed as code
//!
//! [`Driver::new`] runs [`restore::restore`] **before** the session exists. That is
//! `tempo_core::winlink::restore`'s five-step order, and this is the one place it can be got
//! wrong: a session built first would answer the CMS "I do not have it" about mail already on
//! disk, the CMS would be told `+`, and it would send the message again — over an ARQ link, the
//! whole message a second time.
//!
//! # A storage failure is a failed message, never a dropped socket
//!
//! `assemble_b2`, the mailbox write and the journal append can each fail (a full disk, a
//! read-only directory). None of them takes the session down: the transfer that is still in
//! flight, and the mail the CMS is still holding, are worth more than the one message that could
//! not be stored. A failure becomes a loud trace and [`SessionLog::failed`]; the session goes on
//! and the operator is told.
//!
//! # What bounds memory here, and the rule that is meant to stop the next one
//!
//! [`SessionLog`] is filled from the wire: one trace per protocol line, and `b2f` traces every
//! line it does not recognise, which before a SID is *every* line a peer chooses to send. It was
//! the **fourth** unbounded peer-driven accumulation found in this programme — three in
//! `b2f::Session`, closed together by one bound at that type's door, and then this one a layer up,
//! holding what that bounded engine emitted. Bounding a producer does not bound its consumer.
//!
//! So the bound here is the same shape, deliberately: [`MAX_LOG_HELD_BYTES`] checked at
//! [`SessionLog`]'s own recording doors, over an exhaustive [`SessionLog::held_bytes`].
//!
//! **The rule, which is the part meant to outlive this fix.** A fourth cap is not an answer to
//! "what stops the fifth". This is:
//!
//! > **A type that accumulates from the wire owns its own bound.** Concretely, and all three
//! > parts are load-bearing:
//! >
//! > 1. Its growing fields are **private**, so every write is a call it defines. That is the
//! >    *door* — one place, not one per caller. `pub` fields have no door, which is exactly how a
//! >    bounded engine's output became an unbounded log.
//! > 2. It has a `held_bytes()` that **destructures itself exhaustively, with no `..`**, in
//! >    `capacity()` and never `len()`, in O(1). A field added to it does not compile until
//! >    someone says how many peer bytes it holds.
//! > 3. Whatever **copies** it out copies the delta, not the whole thing. A bounded buffer cloned
//! >    once per chunk is O(n²) in CPU: the cap stops the memory exhaustion and hands back a
//! >    denial of service in its place, which is not a fix. See [`SessionLog::mirror_into`].
//!
//! Applied, this file's `Driver` holds nothing of its own that the peer can grow — its `session`
//! and `log` each answer for themselves, and `state`, `mailbox`, `journal` and `root` are sized by
//! the local disk. `size_of::<Driver>()` is what it costs, and if that stops being true the field
//! that changed it needs its own bound, not a note here.
//!
//! ⚠️ **The rule is a convention a reviewer applies, not something the compiler checks** — only
//! part 2 is enforced, and only once someone has written the first `held_bytes`. What it buys is
//! that "is this bounded?" has one answer per type and one place to look for it.
//!
//! # Nothing here touches the transmit path
//!
//! This is the internet path: TCP to the CMS. No PTT, no modem, no audio device, no keying, no
//! frequency and no privilege check appears in this file, and none of the programme's ten
//! TX-safety invariants is exercised by any line of it. A green test here is evidence about none
//! of them.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use tempo_core::winlink::b2f::{self, Action};
use tempo_core::winlink::fbb::{HaveState, Proposal};
use tempo_core::winlink::journal::{self, Event};
use tempo_core::winlink::mailbox::{self, Mailbox};
use tempo_core::winlink::message::assemble_b2;
use tempo_core::winlink::restore::{self, MailboxState};
use tempo_core::winlink::ClientConfig;

/// The most bytes a [`SessionLog`] will hold on the peer's behalf, over every field it owns.
///
/// **The memory bound for this layer**, the same shape and for the same reason as
/// `b2f`'s `MAX_SESSION_HELD_BYTES`: see the module header for why the bound is one accounting at
/// one door rather than a cap per field.
///
/// 1 MiB is generous by three orders of magnitude against every legitimate session measured
/// 2026-09-07 on this code, `SessionLog::held_bytes` in charged bytes:
///
/// | session | charged | of the ceiling |
/// |---|---|---|
/// | the golden transcript, one message delivered end to end | 997 | 0.10 % |
/// | 1,000 messages stored, two traces and a MID each | 210,728 | 20.1 % |
///
/// Erring generous is deliberate, and it is a different judgement from `b2f`'s: over the ceiling
/// that session *fails*, because a peer that made it hold too much is not sending mail. This log
/// is diagnostics, so over the ceiling it merely stops recording — see [`SessionLog::refuse`].
const MAX_LOG_HELD_BYTES: usize = 1024 * 1024;

/// Charged room reserved for the one line [`SessionLog::refuse`] writes when the ceiling is hit.
///
/// The ceiling is a hard bound on [`SessionLog::held_bytes`], so the note that says the log is
/// full has to fit *inside* it or the bound is not a bound. Its real cost is asserted against this
/// number by `the_log_full_note_fits_the_room_reserved_for_it`, so the reserve cannot rot as the
/// wording changes.
const LOG_FULL_NOTE_BUDGET: usize = 256;

/// What one session did, for the status pane and the log.
///
/// Deliberately **not** a `Result`: a session can store three messages, fail to store a fourth,
/// and go on to store a fifth. Only a flat record of what happened can say that.
///
/// **The fields are private and every one of them is peer-driven** — see the module header's
/// memory rule. Writing goes through [`SessionLog::trace`], [`SessionLog::arrived`] and
/// [`SessionLog::fail`], which is the door [`MAX_LOG_HELD_BYTES`] is enforced at; reading goes
/// through the accessors below.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionLog {
    /// Protocol traces in order, as `b2f` produced them, plus this driver's own storage notes.
    traces: Vec<String>,
    /// The MID of every message stored this session, in arrival order. Bounded, so it can be
    /// **shorter** than [`SessionLog::stored`] — which is the count, and is never short.
    received: Vec<Vec<u8>>,
    /// The first thing that went irrecoverably wrong, rendered. `None` is the good case.
    failed: Option<String>,
    /// Messages written to the mailbox this session, counted whether or not the MID was retained.
    ///
    /// A `usize`, so the peer cannot grow it: the operator's "N message(s) stored" must stay true
    /// on the far side of the ceiling, and a length that stopped growing would quietly under-report
    /// mail that really is on disk.
    stored: usize,
    /// Running total of the heap the elements of `traces` and `received` own.
    ///
    /// A counter rather than a walk, because [`SessionLog::held_bytes`] runs on every recorded
    /// line: summing the vector each time would make a peer's line count quadratic in CPU, which
    /// is the same exhaustion by a different resource (`b2f`'s `held_bytes` is O(1) for this
    /// reason too).
    elems: usize,
    /// Whether the ceiling has already refused something, so the note is written exactly once.
    full: bool,
}

impl SessionLog {
    /// Protocol traces in order.
    pub fn traces(&self) -> &[String] {
        &self.traces
    }

    /// The MIDs retained this session. ⚠️ Bounded — [`SessionLog::stored`] is the count.
    pub fn received(&self) -> &[Vec<u8>] {
        &self.received
    }

    /// The first thing that went irrecoverably wrong. `None` is the good case.
    pub fn failed(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    /// How many messages this session wrote to the mailbox. Never short.
    pub fn stored(&self) -> usize {
        self.stored
    }

    /// Everything this log is holding on the peer's behalf, in charged bytes.
    ///
    /// **Exhaustive on purpose: no `..`, and every field named** — `b2f::Session::held_bytes`'s
    /// mechanism, one layer up, and for the identical reason. A field added to [`SessionLog`] does
    /// not compile until this function says what it holds, so the *fifth* unbounded accumulation
    /// cannot be introduced quietly. The module header states the rule this is one instance of.
    ///
    /// **`capacity()`, never `len()`**, and every container charged its own per-element overhead:
    /// a `Vec` that grew and was drained still owns its buffer, and a vector of a thousand
    /// one-character traces costs `size_of::<String>()` a thousand times before a byte of text is
    /// counted. Every term is O(1).
    ///
    /// ⚠️ What the compiler enforces is a **decision, not a correct answer** — rustc's suggested
    /// fix for a missing field is `<field>: _`, which silences it while charging nothing. The
    /// pattern makes ignoring a buffer something someone had to type, which is all it can do.
    pub fn held_bytes(&self) -> usize {
        let SessionLog {
            // The two peer-driven vectors. Their spines are charged here; the heap their elements
            // own is `elems`, below, because only a running counter keeps this O(1).
            traces,
            received,
            // One `String`, replaced and never appended to.
            failed,
            // Scalars: no buffer behind them.
            stored: _,
            full: _,
            elems,
        } = self;
        elems
            + traces.capacity() * size_of::<String>()
            + received.capacity() * size_of::<Vec<u8>>()
            + failed.as_ref().map_or(0, String::capacity)
    }

    /// Records one trace line. **The door for `traces`.**
    pub fn trace(&mut self, note: impl Into<String>) {
        let note = note.into();
        if self.refuse(note.capacity() + size_of::<String>()) {
            return;
        }
        self.elems += note.capacity();
        self.traces.push(note);
    }

    /// Records that a message was written to the mailbox. **The door for `received`.**
    ///
    /// The count is taken first and unconditionally: the ceiling may cost the operator a MID in
    /// the list, never a message in the tally.
    pub fn arrived(&mut self, mid: Vec<u8>) {
        self.stored = self.stored.saturating_add(1);
        if self.refuse(mid.capacity() + size_of::<Vec<u8>>()) {
            return;
        }
        self.elems += mid.capacity();
        self.received.push(mid);
    }

    /// Records something that went irrecoverably wrong: traced, and latched into `failed`.
    ///
    /// First-wins, because the first failure is the one that explains the rest. `failed` is
    /// exempt from the ceiling by construction — it is one `String`, written at most once, so it
    /// is not something a peer can repeat — but it *is* charged, so `held_bytes` stays honest.
    pub fn fail(&mut self, note: impl Into<String>) {
        let note = note.into();
        if self.failed.is_none() {
            self.failed = Some(note.clone());
        }
        self.trace(note);
    }

    /// Whether the ceiling refuses `more` charged bytes, writing the one note that says so.
    ///
    /// **Over the ceiling this log stops recording; it does not fail the session.** That is the
    /// deliberate difference from `b2f::Session::refuse_over_ceiling`, and the reason is what the
    /// two things are: the session is the mail transfer, so a peer that overran it is not
    /// delivering mail and is refused. This is the *diagnostic record of* that transfer, and
    /// dropping a socket with mail outstanding because the log filled up would be the log
    /// deciding the session's outcome — the module header's rule that a storage failure is never
    /// a dropped socket, applied to the one thing that is not even storage.
    ///
    /// The note is written once and inside the ceiling ([`LOG_FULL_NOTE_BUDGET`]), because an
    /// operator must not read a truncated log as a complete one.
    fn refuse(&mut self, more: usize) -> bool {
        // A latch, not a high-water mark. Without it the log reopens for anything small enough to
        // fit in the slack the refused entry left — measured: a full log went on accepting MIDs,
        // so `nothing further is recorded` was false and the tail of the log became a function of
        // entry sizes rather than of when the ceiling was reached.
        if self.full {
            return true;
        }
        let held = self
            .held_bytes()
            .saturating_add(more)
            .saturating_add(self.spine_growth_headroom())
            .saturating_add(LOG_FULL_NOTE_BUDGET);
        if held <= MAX_LOG_HELD_BYTES {
            return false;
        }
        self.full = true;
        let note = format!(
            "the session log reached its {MAX_LOG_HELD_BYTES}-byte ceiling; \
             nothing further is recorded, and the session goes on"
        );
        self.elems += note.capacity();
        self.traces.push(note);
        true
    }

    /// What one more recorded entry could add to [`SessionLog::held_bytes`] *beyond its own cost*.
    ///
    /// ⚠️ **Without this the ceiling is not a ceiling.** `Vec::push` on a full vector reallocates
    /// to twice the capacity (to four elements from empty), so a single push can add the whole
    /// current spine charge again — at the sizes this log reaches, ~390 KiB in one step. A door
    /// that only charged the entry would wave through the entry that doubles the vector, and
    /// `held_bytes()` would then sit ~40 % over the number it was checked against. It passed the
    /// flood test anyway, on the arithmetic of one particular line length; that is luck, not a
    /// bound.
    ///
    /// The cost is that the log fills a little early, because the reserve is held back the whole
    /// time. Measured 2026-09-07 over the line widths
    /// `a_peer_that_floods_junk_lines_cannot_grow_the_session_log` sweeps, the charge it stops
    /// recording at: 835,828 (1- and 7-byte lines), 851,602 (40), 949,971 (200), 998,953 (480) —
    /// **80 % to 95 % of the ceiling**. Diagnostics do not need the last fifth of a megabyte, and
    /// a ceiling that can be exceeded is worth less than one that cannot.
    ///
    /// Without it, the same sweep reached **1,126,817** charged bytes at 200-byte lines. A single
    /// line width does not find that: at 40 bytes the bound held on the arithmetic of where the
    /// vector happened to double, which is why the test sweeps.
    fn spine_growth_headroom(&self) -> usize {
        (self.traces.capacity() + 4) * size_of::<String>()
            + (self.received.capacity() + 4) * size_of::<Vec<u8>>()
    }

    /// Appends everything recorded since the last call into `into`, leaving it an exact copy.
    ///
    /// **This is what keeps mirroring linear.** `into` is written after every chunk off the
    /// socket (`tempo_net::wl2k::pump` reads 4 KiB at a time), and a whole-log clone there is
    /// O(n²) in the number of chunks. Measured 2026-09-07 on this code, banner lines through a
    /// `Driver` and a shared `SessionLog` at the pump's own chunk size — *before* is the shipped
    /// pair, unbounded log and whole-log clone; *after* is the ceiling and this function:
    ///
    /// | peer bytes | chunks | before | after | log held, before → after |
    /// |---|---|---|---|---|
    /// | 4 MiB | 1,025 | 3.64 s | 0.14 s | 21,911,712 → 851,602 |
    /// | 8 MiB | 2,049 | 15.74 s | 0.27 s | 43,823,316 → 851,602 |
    /// | 16 MiB | 4,097 | 63.06 s | 0.53 s | 87,646,524 → 851,602 |
    ///
    /// Doubling the input quadrupled the time before and doubled it after, which is the shape of
    /// the defect rather than one slow number: the peer chose how long its own bytes took to
    /// process, and 16 MiB of banner text — nothing a CMS could not send — cost a minute of CPU
    /// and 84 MiB of retained log. **Bounding the log alone would not have fixed it**: a 1 MiB
    /// ceiling cloned once per chunk is still 4,097 near-megabyte copies. The *after* column is
    /// flat in both dimensions, which is the point — neither figure is a function of how much the
    /// peer sent.
    ///
    /// No cursor is carried, because `into` *is* the cursor: it is an exact copy as of the last
    /// call, so its own lengths say what has already been copied. That is sound only because both
    /// vectors are **append-only** — an entry once recorded is never removed or rewritten, and
    /// past the ceiling nothing is appended at all — so a position stays a position. `into` being
    /// longer than `self` means it was never a mirror of it; that copies nothing rather than
    /// panicking on the slice.
    pub fn mirror_into(&self, into: &mut SessionLog) {
        let SessionLog {
            traces,
            received,
            failed,
            stored,
            elems,
            full,
        } = self;
        into.traces
            .extend_from_slice(traces.get(into.traces.len()..).unwrap_or(&[]));
        into.received
            .extend_from_slice(received.get(into.received.len()..).unwrap_or(&[]));
        into.failed.clone_from(failed);
        into.stored = *stored;
        into.elems = *elems;
        into.full = *full;
    }
}

/// Owns one B2F session and the mailbox it delivers into.
pub struct Driver {
    /// The protocol engine. Fed by [`Driver::feed`], never by anything else.
    session: b2f::Session,
    /// The blob store received mail is written to.
    mailbox: Mailbox,
    /// The arrival/read record. One row appended per stored message.
    journal: journal::Journal,
    /// The mailbox state this session opened against — the answer to `have`.
    state: MailboxState,
    /// What has happened so far.
    log: SessionLog,
    /// The clock, injected so a test can pin an arrival time.
    now: fn() -> i64,
    /// The mailbox root, kept for the error messages that name it.
    root: PathBuf,
}

impl Driver {
    /// Restores the mailbox, then builds a session that answers proposals from it.
    ///
    /// Fails only if the mailbox cannot be restored at all — at which point opening a session
    /// would be answering the CMS from a store whose contents are unknown, which is exactly the
    /// double-download the restore exists to prevent.
    pub fn new(cfg: &ClientConfig, root: &Path, now: fn() -> i64) -> io::Result<Driver> {
        // ORDERING (see tempo_core::winlink::restore): the mailbox is restored BEFORE the session
        // exists, so `have` cannot be asked a question the store has not finished answering.
        let state = restore::restore(root)?;
        let held: BTreeSet<Vec<u8>> = state.index.entries.iter().map(|e| e.mid.clone()).collect();
        let mut session = b2f::Session::new(cfg, b2f::Role::Client);
        session.set_have(Box::new(move |p: &Proposal| {
            // `Yes`, never `Unwanted`: fbb.rs is explicit that the two are the same wire byte and
            // different decisions, and we do have it.
            //
            // `Partial` is deliberately not used. The `!offset` resume needs a partial-blob store
            // to resume from, and answering `Partial(n)` with nothing behind it would ask the CMS
            // to send from an offset this station cannot use.
            if held.contains(&p.mid) {
                HaveState::Yes
            } else {
                HaveState::No
            }
        }));
        Ok(Driver {
            session,
            mailbox: mailbox::open(root),
            journal: journal::open(root),
            state,
            log: SessionLog::default(),
            now,
            root: root.to_path_buf(),
        })
    }

    /// What this session has done so far.
    pub fn log(&self) -> &SessionLog {
        &self.log
    }

    /// The mailbox state this session opened against.
    pub fn state(&self) -> &MailboxState {
        &self.state
    }

    /// Writes one received message to the mailbox and journals its arrival.
    ///
    /// Stores `assemble_b2(&msg)` — a re-serialisation, because `Action::Received` carries a
    /// parsed message and not the plaintext. That is only faithful to `mailbox.rs`'s "exactly as
    /// it went over the wire" because the round trip is byte-identical, which is proved by
    /// `parsing_and_re_assembling_a_b2_blob_returns_the_same_bytes` in tempo-core's B2F
    /// integration test rather than assumed here.
    ///
    /// Every failure below is recorded and swallowed; see the module header.
    fn store(&mut self, msg: tempo_core::winlink::message::Message) {
        let mid = msg.mid.clone();
        let shown = String::from_utf8_lossy(&mid).into_owned();
        let blob = match assemble_b2(&msg) {
            Ok(b) => b,
            Err(e) => return self.storage_failed(&shown, &format!("could not re-assemble: {e:?}")),
        };
        if let Err(e) = self.mailbox.store(&mid, &blob) {
            return self.storage_failed(
                &shown,
                &format!("could not write it to {}: {e}", self.root.display()),
            );
        }
        if let Err(e) = self.journal.append(&Event::Received {
            mid: mid.clone(),
            at: (self.now)(),
        }) {
            // The blob IS stored; only its arrival time was lost. Said precisely, because
            // "message lost" and "arrival time lost" are very different news.
            return self.storage_failed(
                &shown,
                &format!("stored, but its arrival time could not be journaled: {e}"),
            );
        }
        self.log
            .trace(format!("stored {shown} ({} bytes)", blob.len()));
        self.log.arrived(mid);
    }

    /// Records a storage failure without ending the session.
    fn storage_failed(&mut self, mid: &str, why: &str) {
        self.log.fail(format!("{mid}: {why}"));
    }
}

impl tempo_net::wl2k::ByteSession for Driver {
    fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for action in self.session.feed(chunk) {
            match action {
                Action::Send(bytes) => out.push(bytes),
                Action::Trace(t) => self.log.trace(t),
                Action::Received(msg) => self.store(msg),
                Action::Done => self.log.trace("session complete"),
                Action::Failed(e) => self.log.fail(e.to_string()),
            }
        }
        out
    }

    fn wants_close(&self) -> bool {
        self.session.wants_close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_net::wl2k::ByteSession as _;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wl-driver-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The inbound side of tempo-core's golden transcript, concatenated.
    ///
    /// Read from the fixture rather than restated here, so this test and the B2F integration test
    /// are looking at the same bytes. ⚠️ It is a **constructed** transcript (its own header says
    /// so), so what it proves is that this driver consumes what that engine produces — not
    /// anything about a real CMS.
    fn fixture_inbound() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tempo-core/tests/fixtures/winlink/session1.trace"
        );
        let text = std::fs::read_to_string(path).expect("fixture is missing");
        let mut out = Vec::new();
        for line in text.lines() {
            let line = line.trim_end();
            if !line.starts_with('<') {
                continue;
            }
            let hex = line[1..].trim();
            out.extend(
                (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("fixture hex")),
            );
        }
        assert!(!out.is_empty(), "fixture parsed to no inbound bytes");
        out
    }

    fn cfg() -> ClientConfig {
        ClientConfig {
            callsign: "N0CALL".into(),
            // The fixture's stated parameter. Not a credential for anything.
            password: "NEXUSTEST".into(),
        }
    }

    #[test]
    fn a_session_stores_what_it_receives_and_then_declines_it_the_second_time() {
        // The `have` seam proving itself end to end. The second half is the one that would
        // silently not work: an unwired seam still stores the message the first time.
        let root = tmp("driver");
        let inbound = fixture_inbound();

        let mut d = Driver::new(&cfg(), &root, || 1_700_000_000).unwrap();
        let sent = d.feed(&inbound).concat();
        let sent = String::from_utf8_lossy(&sent).into_owned();
        assert!(
            sent.contains("FS +"),
            "first session should accept the offer: {sent:?}"
        );
        assert_eq!(d.log().failed(), None, "traces: {:?}", d.log().traces());
        assert_eq!(d.log().received().len(), 1, "nothing was stored");
        let mid = d.log().received()[0].clone();
        assert!(root
            .join("messages")
            .join(format!("{}.b2f", String::from_utf8_lossy(&mid)))
            .exists());
        assert_eq!(
            journal::open(&root).replay().unwrap().events.len(),
            1,
            "the arrival was not journaled"
        );

        let mut again = Driver::new(&cfg(), &root, || 1_700_000_100).unwrap();
        // The restore ran before the session, so the seam already knows about the stored message.
        assert_eq!(again.state().index.entries.len(), 1);
        assert_eq!(
            again.state().arrived.get(mid.as_slice()),
            Some(&1_700_000_000),
            "the first session's arrival time did not survive"
        );
        let sent2 = String::from_utf8_lossy(&again.feed(&inbound).concat()).into_owned();
        assert!(
            sent2.contains("FS -"),
            "the second session re-downloaded mail it already holds: {sent2:?}"
        );
        assert!(
            again.log().received().is_empty(),
            "nothing should have been stored the second time: {:?}",
            again.log().traces()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_mailbox_that_cannot_be_written_fails_the_message_and_not_the_session() {
        // A full or read-only disk. The transfer in flight and the mail still on the CMS are
        // worth more than the one message that could not be stored, so the socket stays up and
        // the operator is told.
        let root = tmp("readonly");
        let inbound = fixture_inbound();
        let mut d = Driver::new(&cfg(), &root, || 1_700_000_000).unwrap();

        // Make `messages/` unwritable AFTER the driver exists, so the restore itself succeeds and
        // it is the store that fails. A plain file where the directory must go is portable and
        // needs no permission bits (which a root-run CI ignores).
        let messages = root.join("messages");
        if messages.is_dir() {
            // The restore's rebuild already created it, and it is empty at this point.
            std::fs::remove_dir(&messages).unwrap();
        }
        std::fs::write(&messages, b"not a directory").unwrap();

        let sent = String::from_utf8_lossy(&d.feed(&inbound).concat()).into_owned();
        assert!(
            sent.contains("FS +"),
            "the session must still have answered the block: {sent:?}"
        );
        assert!(
            d.log().failed().is_some(),
            "a storage failure must be reported: {:?}",
            d.log().traces()
        );
        assert!(
            d.log().received().is_empty(),
            "nothing was stored, so nothing may be listed as received"
        );
        assert!(
            !d.log().traces().iter().any(|t| t.contains("NEXUSTEST")),
            "the account password reached a trace: {:?}",
            d.log().traces()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The fourth unbounded peer-driven accumulation, at the layer that reintroduced it.
    ///
    /// `b2f` traces every line it does not recognise, and before a SID that is every line a peer
    /// chooses to send — so a peer that says nothing but banner text still writes one `String`
    /// per line into this log, through a session whose own ceiling never trips because it drains
    /// each line as it arrives. Bounding the producer did not bound the consumer.
    #[test]
    fn a_peer_that_floods_junk_lines_cannot_grow_the_session_log() {
        // Swept over line widths on purpose. The bound has to hold whatever the peer's lines
        // cost, and a single width can pass on the arithmetic of where a `Vec` happens to double
        // — which is what `SessionLog::spine_growth_headroom` exists for, and what one width
        // hid while that function was missing.
        for width in [1usize, 7, 40, 200, 480] {
            let root = tmp(&format!("flood{width}"));
            let mut d = Driver::new(&cfg(), &root, || 1_700_000_000).unwrap();

            // Enough banner lines to charge several times the ceiling if nothing refuses them,
            // and every one of them a line a real CMS is allowed to send.
            let mut junk = Vec::new();
            let lines = 4 * MAX_LOG_HELD_BYTES / (width + size_of::<String>() + 32);
            for i in 0..lines {
                junk.extend_from_slice(format!("{:width$}\r", i % 10).as_bytes());
            }
            d.feed(&junk);

            assert!(
                d.log().held_bytes() <= MAX_LOG_HELD_BYTES,
                "at {width}-byte lines the peer grew the session log to {} charged bytes, over \
                 the {MAX_LOG_HELD_BYTES}-byte ceiling",
                d.log().held_bytes()
            );
            assert!(
                d.log()
                    .traces()
                    .iter()
                    .any(|t| t.contains("reached its") && t.contains("ceiling")),
                "at {width}-byte lines the log stopped recording without saying so, so a \
                 truncated log reads as a whole one"
            );
            // And it stopped recording rather than ending the session: the log is diagnostics,
            // and a full one must not drop a socket with mail outstanding.
            assert_eq!(
                d.log().failed(),
                None,
                "a full log failed the session; see SessionLog::refuse"
            );
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// [`LOG_FULL_NOTE_BUDGET`] is what makes the ceiling a real bound rather than a bound plus
    /// however long that sentence happens to be. Measured against the note itself, so rewording
    /// it past the reserve is a red test and not a silently wider ceiling.
    #[test]
    fn the_log_full_note_fits_the_room_reserved_for_it() {
        let mut log = SessionLog::default();
        while !log.full {
            log.trace("x".repeat(400));
        }
        let note = log.traces().last().expect("the note is the last line");
        assert!(
            note.contains("ceiling"),
            "the last line is not the full note: {note:?}"
        );
        let charged = note.capacity() + size_of::<String>();
        assert!(
            charged <= LOG_FULL_NOTE_BUDGET,
            "the full note charges {charged}, over the {LOG_FULL_NOTE_BUDGET} reserved for it"
        );
        assert!(
            log.held_bytes() <= MAX_LOG_HELD_BYTES,
            "the note itself pushed the log over its own ceiling"
        );
    }

    /// The count of stored mail is not the length of the MID list, and this is why.
    ///
    /// A message is on disk before its MID is offered to the log. If the ceiling could shorten
    /// the operator's "N message(s) stored", a full log would under-report real mail.
    #[test]
    fn the_ceiling_costs_a_mid_in_the_list_never_a_message_in_the_count() {
        let mut log = SessionLog::default();
        while !log.full {
            log.trace("x".repeat(400));
        }
        let listed = log.received().len();
        log.arrived(b"ABCDEFGHIJKL".to_vec());
        log.arrived(b"MNOPQRSTUVWX".to_vec());
        assert_eq!(log.stored(), 2, "a stored message went uncounted");
        assert_eq!(
            log.received().len(),
            listed,
            "the MID list grew past the ceiling"
        );
    }

    /// The other half of the finding: a bounded log cloned once per chunk is O(n²) in CPU, which
    /// trades a memory exhaustion for a denial of service rather than fixing one.
    ///
    /// Pinned by identity rather than by a clock: a whole-log clone rebuilds every `String`, so
    /// the first line's heap buffer moves. Appending the delta leaves it exactly where it was.
    #[test]
    fn mirroring_the_log_copies_only_what_is_new() {
        let mut src = SessionLog::default();
        for i in 0..64 {
            src.trace(format!("line {i}"));
        }
        let mut mirror = SessionLog::default();
        src.mirror_into(&mut mirror);
        assert_eq!(mirror, src, "the first mirror is not an exact copy");
        let first = mirror.traces()[0].as_ptr();

        src.trace("one more line");
        src.arrived(b"ABCDEFGHIJKL".to_vec());
        src.mirror_into(&mut mirror);
        assert_eq!(mirror, src, "the second mirror is not an exact copy");
        assert_eq!(
            mirror.traces()[0].as_ptr(),
            first,
            "the mirror re-copied lines it already held — that is the whole-log clone"
        );
    }
}
