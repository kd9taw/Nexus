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

/// What one session did, for the status pane and the log.
///
/// Deliberately **not** a `Result`: a session can store three messages, fail to store a fourth,
/// and go on to store a fifth. Only a flat record of what happened can say that.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionLog {
    /// Protocol traces in order, as `b2f` produced them, plus this driver's own storage notes.
    pub traces: Vec<String>,
    /// The MID of every message stored this session, in arrival order.
    pub received: Vec<Vec<u8>>,
    /// The first thing that went irrecoverably wrong, rendered. `None` is the good case.
    pub failed: Option<String>,
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
            .traces
            .push(format!("stored {shown} ({} bytes)", blob.len()));
        self.log.received.push(mid);
    }

    /// Records a storage failure without ending the session.
    fn storage_failed(&mut self, mid: &str, why: &str) {
        let note = format!("{mid}: {why}");
        self.log.traces.push(note.clone());
        if self.log.failed.is_none() {
            self.log.failed = Some(note);
        }
    }
}

impl tempo_net::wl2k::ByteSession for Driver {
    fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for action in self.session.feed(chunk) {
            match action {
                Action::Send(bytes) => out.push(bytes),
                Action::Trace(t) => self.log.traces.push(t),
                Action::Received(msg) => self.store(msg),
                Action::Done => self.log.traces.push("session complete".into()),
                Action::Failed(e) => {
                    let note = e.to_string();
                    self.log.traces.push(note.clone());
                    if self.log.failed.is_none() {
                        self.log.failed = Some(note);
                    }
                }
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
        assert_eq!(d.log().failed, None, "traces: {:?}", d.log().traces);
        assert_eq!(d.log().received.len(), 1, "nothing was stored");
        let mid = d.log().received[0].clone();
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
            again.log().received.is_empty(),
            "nothing should have been stored the second time: {:?}",
            again.log().traces
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
            d.log().failed.is_some(),
            "a storage failure must be reported: {:?}",
            d.log().traces
        );
        assert!(
            d.log().received.is_empty(),
            "nothing was stored, so nothing may be listed as received"
        );
        assert!(
            !d.log().traces.iter().any(|t| t.contains("NEXUSTEST")),
            "the account password reached a trace: {:?}",
            d.log().traces
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
