//! The mailbox journal — the two facts about a message that the blob cannot tell you.
//!
//! [`super::mailbox`] makes the blob authoritative and `index.json` a cache re-derivable from it,
//! and that design is the reason a lost or mangled index costs one rebuild and never a message.
//! It has a boundary: **two facts about a message are not in the blob and cannot be derived from
//! it.**
//!
//! * **When this station received it.** The blob's `Date:` is the *sender's* clock, and
//!   [`super::mailbox::Index`] is ordered by MID by construction. Nothing on disk knows arrival
//!   order, so a mailbox list cannot show newest-first without being told.
//! * **Whether the operator has read it.** Nothing about a blob changes when it is opened.
//!
//! Everything else stays derived — [`super::mailbox`]'s rule, quoted there from [`crate::store`]:
//! *state that can be derived is not journaled, it is derived.* There are exactly two [`Event`]
//! variants and adding a third needs an argument that one of the two rules above does not already
//! answer. A "session outcome" or "delivery attempt" row is telemetry: unbounded in rows per
//! message, and nothing reads it.
//!
//! # This is a record, not a cache — which changes what may be done to it
//!
//! `index.json` may be deleted at any time and rebuilt in full. **The journal may not**: a lost
//! line costs an arrival time and a read flag permanently. So:
//!
//! * every [`Journal::append`] ends in `sync_all`, because a mail store that reports a message
//!   stored and loses when it arrived has lied about half of it;
//! * a line that will not parse is **skipped and counted**, never fatal — one torn line from a
//!   crash mid-append must not cost every other row in the file;
//! * nothing in this module deletes the file. [`Journal::compact`] rewrites it through a temp
//!   file and a rename, and only ever drops rows it can name a reason for.
//!
//! # NDJSON, and the same Latin-1 codec as the index
//!
//! One JSON object per line, appended. Append-only is what makes a crash cost at most the last
//! line; a re-serialised whole file would put every row at risk on every write. MIDs are encoded
//! with [`super::mailbox`]'s `byte_str` codec — the identical one, not a second copy — because a
//! MID spelled differently here from the index would never compare equal to the index row it
//! names, and [`super::restore`]'s reconcile is exactly that comparison.
//!
//! # Concurrency: this process's threads, not a second process
//!
//! A [`std::sync::Mutex`] serialises every append and compaction, for [`super::mailbox`]'s reason
//! and with [`super::mailbox`]'s limit: it does **not** cover a second Nexus process on the same
//! mailbox, and no file lock is taken (a lock file's own failure mode — a crash leaves it held —
//! is worse than the race, and the app runs one instance).

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

/// The journal's filename, at the mailbox root beside `index.json`.
pub const JOURNAL_FILE: &str = "journal.ndjson";

/// The size above which a startup restore compacts the journal.
///
/// One row is on the order of 60 bytes, so a megabyte is roughly seventeen thousand rows — far
/// more than a mailbox of hundreds of messages produces, and small enough that replaying it costs
/// nothing measurable at launch. It is a housekeeping threshold, not a bound: the file is
/// **never** truncated for being over it, only rewritten with its dead rows dropped.
pub const MAX_JOURNAL_BYTES: u64 = 1024 * 1024;

/// Serialises every journal write **within this process** — see the module header's concurrency
/// section for what that covers and what it does not.
///
/// One lock for every journal rather than one per root, for [`super::mailbox`]'s reason: it is
/// held over nothing contended, and a lock keyed by root would have to be a map that is itself
/// locked to reach.
static JOURNAL_LOCK: Mutex<()> = Mutex::new(());

/// Takes [`JOURNAL_LOCK`], ignoring poisoning.
///
/// A panic while it is held leaves no broken in-memory invariant to inherit — the guarded value is
/// `()`. Propagating the poison would turn one panicking append into a mailbox that can never
/// journal again, which loses arrival times permanently: strictly worse than the race the poison
/// is warning about.
fn lock() -> MutexGuard<'static, ()> {
    JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// One journaled fact about one message.
///
/// `#[serde(tag = "ev")]` so a row is self-describing on disk and a future variant is additive:
/// an older build replaying a newer journal skips the rows it does not know rather than failing
/// the whole file, which is the same tolerance the torn-line rule buys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ev", rename_all = "lowercase")]
pub enum Event {
    /// This station received the message at `at` (unix seconds, **our** clock, not the sender's).
    Received {
        /// The message identifier, encoded exactly as the index encodes it.
        #[serde(with = "super::mailbox::byte_str")]
        mid: Vec<u8>,
        /// Unix seconds.
        at: i64,
    },
    /// The operator opened the message at `at` (unix seconds).
    Read {
        /// The message identifier, encoded exactly as the index encodes it.
        #[serde(with = "super::mailbox::byte_str")]
        mid: Vec<u8>,
        /// Unix seconds.
        at: i64,
    },
}

impl Event {
    /// The MID this event is about, whichever variant it is.
    pub fn mid(&self) -> &[u8] {
        match self {
            Event::Received { mid, .. } | Event::Read { mid, .. } => mid,
        }
    }
}

/// The result of replaying the journal: every row that parsed, and how many did not.
///
/// `skipped` is surfaced rather than swallowed because it is the only evidence a torn or corrupt
/// journal leaves. A restore that repaired silently would make a losing mailbox look healthy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Replay {
    /// Every row that parsed, in file order.
    pub events: Vec<Event>,
    /// Rows that did not parse and were skipped.
    pub skipped: usize,
}

/// A journal rooted at a mailbox directory. Cheap and infallible to make — it touches no disk
/// until asked to, matching [`super::mailbox::Mailbox`].
#[derive(Debug, Clone)]
pub struct Journal {
    /// The mailbox root. The journal file sits directly in it.
    root: PathBuf,
}

/// Names the journal of the mailbox rooted at `root`. Creates nothing.
pub fn open(root: &Path) -> Journal {
    Journal {
        root: root.to_path_buf(),
    }
}

impl Journal {
    /// The journal file's path.
    fn path(&self) -> PathBuf {
        self.root.join(JOURNAL_FILE)
    }

    /// Appends one event, durably.
    ///
    /// One `write_all` for the whole line including its newline, because a split write is what
    /// makes a torn line more likely than it needs to be; then `sync_all`, because the whole point
    /// of this file is that it is not re-derivable.
    pub fn append(&self, ev: &Event) -> io::Result<()> {
        let _guard = lock();
        std::fs::create_dir_all(&self.root)?;
        let mut line = serde_json::to_vec(ev)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        line.push(b'\n');
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())?;
        f.write_all(&line)?;
        f.sync_all()
    }

    /// Replays every row in file order.
    ///
    /// A missing file is an empty replay, not an error: a first launch, or a mailbox that has
    /// never received anything. A row that will not parse — including a trailing fragment with no
    /// newline, which is what a crash mid-append leaves — is skipped and counted.
    pub fn replay(&self) -> io::Result<Replay> {
        let raw = match std::fs::read(self.path()) {
            Ok(r) => r,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Replay::default()),
            Err(e) => return Err(e),
        };
        let mut out = Replay::default();
        // `split` (not `split_terminator`) so a trailing fragment with no newline becomes its own
        // element and is counted as skipped; a file ending in '\n' yields a final empty element,
        // which is ignored below rather than counted.
        for line in raw.split(|&b| b == b'\n') {
            if line.is_empty() {
                continue;
            }
            match serde_json::from_slice::<Event>(line) {
                Ok(ev) => out.events.push(ev),
                Err(_) => out.skipped += 1,
            }
        }
        Ok(out)
    }

    /// Rewrites the journal keeping only the rows that still say something, and returns how many
    /// rows were dropped.
    ///
    /// Three rules, and each names its reason:
    /// * the **first** `Received` per MID survives — it is the real arrival; a later one is a
    ///   duplicate from a re-store and would move the message in a newest-first list;
    /// * the **last** `Read` per MID survives — read state is a latch, and the most recent
    ///   statement of it is the true one;
    /// * a row whose MID is not in `live` is dropped — its blob is gone, so nothing can ever ask
    ///   about it again.
    ///
    /// Output order is the surviving rows' original file order, so a compaction does not reorder
    /// a journal and a second compaction of the same file is a no-op.
    pub fn compact(&self, live: &[Vec<u8>]) -> io::Result<usize> {
        let replay = self.replay()?;
        let live: std::collections::BTreeSet<&[u8]> = live.iter().map(|m| m.as_slice()).collect();

        // Index of the row to keep for each (MID, kind). Computed over the whole replay before
        // anything is written, so "first Received" and "last Read" are decided against the file
        // as a whole rather than incrementally.
        let mut keep_received: std::collections::BTreeMap<&[u8], usize> =
            std::collections::BTreeMap::new();
        let mut keep_read: std::collections::BTreeMap<&[u8], usize> =
            std::collections::BTreeMap::new();
        for (i, ev) in replay.events.iter().enumerate() {
            if !live.contains(ev.mid()) {
                continue;
            }
            match ev {
                Event::Received { mid, .. } => {
                    keep_received.entry(mid.as_slice()).or_insert(i);
                }
                Event::Read { mid, .. } => {
                    keep_read.insert(mid.as_slice(), i);
                }
            }
        }
        let keep: std::collections::BTreeSet<usize> = keep_received
            .values()
            .chain(keep_read.values())
            .copied()
            .collect();

        let mut body = Vec::new();
        for (i, ev) in replay.events.iter().enumerate() {
            if !keep.contains(&i) {
                continue;
            }
            let mut line = serde_json::to_vec(ev)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            line.push(b'\n');
            body.extend_from_slice(&line);
        }
        let dropped = replay.events.len() - keep.len();

        let _guard = lock();
        // Temp file + fsync + rename, `mailbox::write_atomic`'s shape. Not that function: it is
        // private to `mailbox.rs`, and widening another item to share four lines would trade a
        // real coupling for a small duplication.
        let path = self.path();
        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&body)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &path)?;
        Ok(dropped)
    }

    /// The journal's size in bytes, or 0 if it does not exist — the input to the restore's
    /// compaction decision.
    pub fn len_bytes(&self) -> u64 {
        std::fs::metadata(self.path()).map(|m| m.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("wl-journal-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn appended_events_replay_in_order() {
        let root = tmp("order");
        let j = open(&root);
        j.append(&Event::Received {
            mid: b"AAA1".to_vec(),
            at: 100,
        })
        .unwrap();
        j.append(&Event::Received {
            mid: b"BBB2".to_vec(),
            at: 200,
        })
        .unwrap();
        j.append(&Event::Read {
            mid: b"AAA1".to_vec(),
            at: 300,
        })
        .unwrap();
        let r = j.replay().unwrap();
        assert_eq!(r.skipped, 0);
        assert_eq!(
            r.events,
            vec![
                Event::Received {
                    mid: b"AAA1".to_vec(),
                    at: 100
                },
                Event::Received {
                    mid: b"BBB2".to_vec(),
                    at: 200
                },
                Event::Read {
                    mid: b"AAA1".to_vec(),
                    at: 300
                },
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_journal_replays_empty_rather_than_failing() {
        // A first launch, and a mailbox that has never received anything. Not an error.
        let root = tmp("absent");
        let r = open(&root).replay().unwrap();
        assert_eq!(r, Replay::default());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_torn_last_line_is_skipped_and_every_other_row_survives() {
        // THE CRASH CASE. A power loss mid-append leaves a partial line. The journal is not
        // re-derivable, so losing the whole file over one bad line would lose every arrival time
        // the mailbox has.
        let root = tmp("torn");
        let j = open(&root);
        j.append(&Event::Received {
            mid: b"AAA1".to_vec(),
            at: 100,
        })
        .unwrap();
        j.append(&Event::Received {
            mid: b"BBB2".to_vec(),
            at: 200,
        })
        .unwrap();
        let path = root.join(JOURNAL_FILE);
        let mut raw = std::fs::read(&path).unwrap();
        raw.extend_from_slice(b"{\"ev\":\"received\",\"mid\":\"CCC");
        std::fs::write(&path, &raw).unwrap();

        let r = j.replay().unwrap();
        assert_eq!(
            r.skipped, 1,
            "the torn line must be counted, not silently dropped"
        );
        assert_eq!(r.events.len(), 2, "both whole rows must survive: {r:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn compaction_keeps_one_row_per_mid_per_kind_and_drops_the_dead() {
        let root = tmp("compact");
        let j = open(&root);
        for at in [10, 20, 30] {
            j.append(&Event::Received {
                mid: b"AAA1".to_vec(),
                at,
            })
            .unwrap();
        }
        j.append(&Event::Read {
            mid: b"AAA1".to_vec(),
            at: 40,
        })
        .unwrap();
        j.append(&Event::Received {
            mid: b"GONE".to_vec(),
            at: 50,
        })
        .unwrap();

        let dropped = j.compact(&[b"AAA1".to_vec()]).unwrap();
        assert_eq!(dropped, 3, "two superseded Received rows and one dead MID");
        let r = j.replay().unwrap();
        assert_eq!(
            r.events,
            vec![
                Event::Received {
                    mid: b"AAA1".to_vec(),
                    at: 10
                },
                Event::Read {
                    mid: b"AAA1".to_vec(),
                    at: 40
                },
            ],
            "compaction keeps the FIRST arrival (the real one) and the LAST read"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_mid_with_a_non_ascii_byte_round_trips_through_the_index_codec() {
        // The index stores byte-strings as Latin-1; the journal must agree exactly, or a
        // reconcile comparing the two never matches.
        let root = tmp("latin1");
        let j = open(&root);
        let mid = vec![0x41u8, 0xB0, 0x5A];
        j.append(&Event::Received {
            mid: mid.clone(),
            at: 1,
        })
        .unwrap();
        assert_eq!(
            j.replay().unwrap().events[0],
            Event::Received { mid, at: 1 }
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
