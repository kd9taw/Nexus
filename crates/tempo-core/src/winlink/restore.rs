//! The ordered startup restore — index, journal, reconcile, compact, in that order.
//!
//! [`super::mailbox`] owns what is on disk and [`super::journal`] owns the two facts the disk
//! cannot supply. Neither is usable on its own: the index knows every message and nothing about
//! when it arrived; the journal knows arrivals and nothing about whether the blob is still there.
//! This module joins them, once, at launch, and produces the [`MailboxState`] every reader works
//! from.
//!
//! # The order, and why each step is where it is
//!
//! 1. **Load the index; on any error, rebuild it from the blobs** and record
//!    [`Repairs::index_rebuilt`]. The index first, because everything after it is keyed by the
//!    set of MIDs that actually have blobs.
//! 2. **Replay the journal.** Second, because a journal row is only meaningful against an index
//!    that says the blob exists.
//! 3. **Reconcile, both directions, counting each.** A journal row whose MID is not in the index
//!    is dropped ([`Repairs::journal_rows_without_blob`]) — the blob may have been removed out of
//!    band, and the journal is not authoritative about existence. An index entry with no
//!    `Received` row is listed with **no arrival time**
//!    ([`Repairs::blobs_without_journal`]).
//! 4. **Compact if the journal is over [`super::journal::MAX_JOURNAL_BYTES`]**, passing the
//!    index's MID list as the live set. After the reconcile, so compaction acts on facts rather
//!    than guesses.
//! 5. **Only now is the state usable. No session may open before this returns.**
//!
//! # Why the ordering is load-bearing, mechanically
//!
//! `b2f::Session`'s `have` callback answers the CMS's proposals from this state. A session opened
//! before step 1 answers `HaveState::No` for a message it already holds, the CMS is told `+`, and
//! it sends the message again — over an ARQ link later on, that is the whole message a second
//! time, on the air, on the operator's clock. That is the observable cost, and it is what the
//! ordering buys.
//!
//! # ⚠️ A blob with no journal row gets NO arrival time, not a fabricated one
//!
//! Stamping "now" would sort a two-year-old message to the top of a newest-first list, and would
//! then be indistinguishable from a real arrival forever — a lie that cannot be found later.
//! [`MailboxState::arrived`] simply has no entry for that MID and the UI renders the arrival as
//! unknown. The absence is the honest answer and it is self-correcting: nothing overwrites it
//! with a guess.
//!
//! For the same reason such a message is **read** by default, not unread: a badge the operator
//! cannot explain, on a message whose arrival this station cannot account for, is noise that
//! never clears.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use super::journal::{self, Event, MAX_JOURNAL_BYTES};
use super::mailbox::{self, Index};

/// What the restore had to repair, and what it could not.
///
/// Every field is a count rather than a flag-and-a-log, because these are the numbers a
/// diagnostics pane shows and a bug report needs. A silent repair makes a losing mailbox look
/// healthy, which is the failure this struct exists to prevent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Repairs {
    /// `index.json` was missing or unreadable and was re-derived from the blobs.
    pub index_rebuilt: bool,
    /// Journal lines that would not parse (a torn write, or a row from a newer build).
    pub journal_lines_skipped: usize,
    /// Journal rows naming a MID with no blob. Dropped from the state.
    pub journal_rows_without_blob: usize,
    /// Blobs with no `Received` row: listed, with no arrival time and not unread.
    pub blobs_without_journal: usize,
    /// Rows dropped by the compaction in step 4, if it ran.
    pub journal_rows_dropped_by_compaction: usize,
}

/// Everything a reader needs about the mailbox, as of one restore.
///
/// `BTreeMap`/`BTreeSet` rather than the hash variants so two restores of one mailbox are
/// bit-identical — which is what makes `restore_is_idempotent_and_does_not_depend_on_the_index_cache`
/// a real comparison rather than a comparison of two orderings of one set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailboxState {
    /// Every message on disk, ascending by MID (the index's own invariant).
    pub index: Index,
    /// MID → unix seconds this station received it. **A MID may be absent**; see the module
    /// header.
    pub arrived: BTreeMap<Vec<u8>, i64>,
    /// MIDs with a `Received` row and no later `Read` row.
    pub unread: BTreeSet<Vec<u8>>,
    /// What had to be repaired to produce this.
    pub repairs: Repairs,
}

/// Runs the ordered restore over the mailbox rooted at `root`.
///
/// Errors only when the filesystem itself refuses (the blob directory cannot be read). A missing
/// mailbox, a missing index, a missing journal and a torn journal are all ordinary and produce a
/// state plus a [`Repairs`] count.
pub fn restore(root: &Path) -> io::Result<MailboxState> {
    let mb = mailbox::open(root);
    let mut repairs = Repairs::default();

    // 1 ── the index, or a rebuild from the blobs.
    let index = match mb.load_index() {
        Ok(ix) => ix,
        Err(_) => {
            repairs.index_rebuilt = true;
            mb.rebuild_index()?
        }
    };
    let live: BTreeSet<&[u8]> = index.entries.iter().map(|e| e.mid.as_slice()).collect();

    // 2 ── the journal.
    let j = journal::open(root);
    let replay = j.replay()?;
    repairs.journal_lines_skipped = replay.skipped;

    // 3 ── reconcile, both directions.
    let mut arrived: BTreeMap<Vec<u8>, i64> = BTreeMap::new();
    let mut read_at: BTreeMap<Vec<u8>, i64> = BTreeMap::new();
    for ev in &replay.events {
        if !live.contains(ev.mid()) {
            repairs.journal_rows_without_blob += 1;
            continue;
        }
        match ev {
            // FIRST arrival wins: a later `Received` for the same MID is a re-store, and taking
            // it would move an old message to the top of a newest-first list. Same rule as
            // `journal::compact`, and they must agree or compaction would change the answer.
            Event::Received { mid, at } => {
                arrived.entry(mid.clone()).or_insert(*at);
            }
            // LAST read wins: read state is a latch, and the most recent statement is the true
            // one.
            Event::Read { mid, at } => {
                read_at.insert(mid.clone(), *at);
            }
        }
    }
    repairs.blobs_without_journal = index
        .entries
        .iter()
        .filter(|e| !arrived.contains_key(&e.mid))
        .count();

    // Unread = has an arrival and has never been read.
    //
    // Deliberately NOT `read_at < arrived`. A stale-read guard reads like prudence and is dead
    // logic here: arrival is FIRST-wins, so the arrival in hand is the earliest one on file, and
    // a `Read` row older than it would mean the operator read the message before it existed.
    // Adding the comparison left every test green under mutation, which is how it was found to
    // be unreachable rather than merely untested; an unreachable branch that looks like a safety
    // check is worse than its absence, because the next reader trusts it.
    let unread: BTreeSet<Vec<u8>> = arrived
        .keys()
        .filter(|mid| !read_at.contains_key(*mid))
        .cloned()
        .collect();

    // 4 ── compact, on facts rather than guesses.
    if j.len_bytes() > MAX_JOURNAL_BYTES {
        let mids: Vec<Vec<u8>> = index.entries.iter().map(|e| e.mid.clone()).collect();
        repairs.journal_rows_dropped_by_compaction = j.compact(&mids)?;
    }

    Ok(MailboxState {
        index,
        arrived,
        unread,
        repairs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::winlink::journal::{self, Event};
    use crate::winlink::mailbox;
    use crate::winlink::message::{assemble_b2, Message};

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("wl-restore-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A minimal, real B2 blob for `mid`.
    fn blob(mid: &[u8], subject: &str) -> Vec<u8> {
        assemble_b2(&Message {
            mid: mid.to_vec(),
            headers: vec![(b"Subject".to_vec(), subject.as_bytes().to_vec())],
            body: b"hello\r\n".to_vec(),
            attachments: Vec::new(),
        })
        .expect("assemble")
    }

    #[test]
    fn restore_is_idempotent_and_does_not_depend_on_the_index_cache() {
        // THE POSITIVE CONTROL, and it is mailbox.rs's own control extended over the journal:
        // an implementation that secretly trusted index.json cannot produce the same answer with
        // the cache deleted.
        let root = tmp("idem");
        let mb = mailbox::open(&root);
        mb.store(b"AAA1", &blob(b"AAA1", "one")).unwrap();
        mb.store(b"BBB2", &blob(b"BBB2", "two")).unwrap();
        let j = journal::open(&root);
        j.append(&Event::Received {
            mid: b"AAA1".to_vec(),
            at: 111,
        })
        .unwrap();
        j.append(&Event::Received {
            mid: b"BBB2".to_vec(),
            at: 222,
        })
        .unwrap();
        j.append(&Event::Read {
            mid: b"AAA1".to_vec(),
            at: 333,
        })
        .unwrap();

        let first = restore(&root).unwrap();
        let second = restore(&root).unwrap();
        assert_eq!(first, second, "restore is not idempotent");

        std::fs::remove_file(root.join(mailbox::INDEX_JSON)).unwrap();
        let third = restore(&root).unwrap();
        assert!(
            third.repairs.index_rebuilt,
            "a deleted cache must be rebuilt, and said so"
        );
        assert_eq!(third.index, first.index, "the blobs are the record");
        assert_eq!(third.arrived, first.arrived);
        assert_eq!(third.unread, first.unread);
        assert_eq!(
            third.unread,
            [b"BBB2".to_vec()].into_iter().collect(),
            "AAA1 has a later Read row; BBB2 does not"
        );
        assert_eq!(third.arrived.get(b"AAA1".as_slice()), Some(&111));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_journal_row_whose_blob_is_gone_is_dropped_and_counted() {
        let root = tmp("orphan-row");
        let mb = mailbox::open(&root);
        mb.store(b"AAA1", &blob(b"AAA1", "one")).unwrap();
        let j = journal::open(&root);
        j.append(&Event::Received {
            mid: b"AAA1".to_vec(),
            at: 1,
        })
        .unwrap();
        j.append(&Event::Received {
            mid: b"GONE".to_vec(),
            at: 2,
        })
        .unwrap();

        let st = restore(&root).unwrap();
        assert_eq!(st.repairs.journal_rows_without_blob, 1);
        assert!(!st.arrived.contains_key(b"GONE".as_slice()));
        assert_eq!(st.index.entries.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_blob_with_no_journal_row_is_listed_with_no_arrival_time_and_is_not_unread() {
        // The migration case: a mailbox that predates the journal, or a blob copied in by hand.
        // Inventing "now" would sort an old message to the top of a newest-first list forever.
        let root = tmp("orphan-blob");
        let mb = mailbox::open(&root);
        mb.store(b"AAA1", &blob(b"AAA1", "one")).unwrap();

        let st = restore(&root).unwrap();
        assert_eq!(st.repairs.blobs_without_journal, 1);
        assert_eq!(st.index.entries.len(), 1, "it is still listed");
        assert!(
            !st.arrived.contains_key(b"AAA1".as_slice()),
            "no invented timestamp"
        );
        assert!(
            st.unread.is_empty(),
            "a message with no accountable arrival must not badge"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_torn_journal_line_is_counted_and_the_rest_of_the_restore_succeeds() {
        let root = tmp("torn-restore");
        let mb = mailbox::open(&root);
        mb.store(b"AAA1", &blob(b"AAA1", "one")).unwrap();
        let j = journal::open(&root);
        j.append(&Event::Received {
            mid: b"AAA1".to_vec(),
            at: 7,
        })
        .unwrap();
        let p = root.join(journal::JOURNAL_FILE);
        let mut raw = std::fs::read(&p).unwrap();
        raw.extend_from_slice(b"{\"ev\":\"rec");
        std::fs::write(&p, raw).unwrap();

        let st = restore(&root).unwrap();
        assert_eq!(st.repairs.journal_lines_skipped, 1);
        assert_eq!(st.arrived.get(b"AAA1".as_slice()), Some(&7));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_mailbox_restores_to_an_empty_state_without_creating_junk() {
        let root = tmp("empty");
        let st = restore(&root).unwrap();
        assert!(st.index.entries.is_empty());
        assert!(st.arrived.is_empty());
        assert!(st.unread.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_oversized_journal_is_compacted_and_the_answer_does_not_change() {
        // Step 4 of the order, and the assertion that matters is not that it shrank: it is that
        // compaction is invisible to the state. A compaction that changed an arrival time would
        // be a repair that damaged the record it was repairing.
        let root = tmp("compact");
        let mb = mailbox::open(&root);
        mb.store(b"AAA1", &blob(b"AAA1", "one")).unwrap();
        let j = journal::open(&root);
        j.append(&Event::Received {
            mid: b"AAA1".to_vec(),
            at: 500,
        })
        .unwrap();
        // Enough duplicate rows to cross MAX_JOURNAL_BYTES, plus one for a MID with no blob,
        // which compaction should drop. Written in ONE go rather than through `append`: every
        // append fsyncs, and twenty-odd thousand fsyncs is minutes on a real disk. The lines are
        // still produced by the journal's own serialiser, so this is the same bytes `append`
        // would have written and not a second spelling of the format.
        {
            // Rows until the file is over the threshold, MEASURED as they are built. A row count
            // derived from a guessed row size lands just under (38 bytes a row, not the 40 the
            // arithmetic assumed) and the test then asserts on a journal that never compacted.
            let mut bulk: Vec<u8> = Vec::new();
            let mut at = 1000i64;
            while (bulk.len() as u64) <= journal::MAX_JOURNAL_BYTES {
                bulk.extend_from_slice(
                    &serde_json::to_vec(&Event::Read {
                        mid: b"AAA1".to_vec(),
                        at,
                    })
                    .unwrap(),
                );
                bulk.push(b'\n');
                at += 1;
            }
            bulk.extend_from_slice(
                &serde_json::to_vec(&Event::Received {
                    mid: b"GONE".to_vec(),
                    at: 9,
                })
                .unwrap(),
            );
            bulk.push(b'\n');
            let mut existing = std::fs::read(root.join(journal::JOURNAL_FILE)).unwrap();
            existing.extend_from_slice(&bulk);
            std::fs::write(root.join(journal::JOURNAL_FILE), &existing).unwrap();
        }
        assert!(
            j.len_bytes() > journal::MAX_JOURNAL_BYTES,
            "the fixture did not get big enough to trigger compaction: {} bytes",
            j.len_bytes()
        );

        let before = restore(&root).unwrap();
        assert!(
            before.repairs.journal_rows_dropped_by_compaction > 0,
            "nothing was compacted: {:?}",
            before.repairs
        );
        assert!(
            j.len_bytes() < journal::MAX_JOURNAL_BYTES,
            "the journal is still over the threshold after compaction"
        );

        let after = restore(&root).unwrap();
        assert_eq!(after.arrived, before.arrived, "compaction moved an arrival");
        assert_eq!(after.unread, before.unread, "compaction moved read state");
        assert_eq!(after.index, before.index);
        assert_eq!(
            after.repairs.journal_rows_dropped_by_compaction, 0,
            "a compacted journal must not need compacting again"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
