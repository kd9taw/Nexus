//! Record identity: `APP_NEXUS_ID`, the id a record keeps for as long as the log holds it,
//! so a change can be addressed to THE row instead of to a position or a guess at its fields.
//!
//! Two kinds, and a record never changes kind:
//!
//! - **Minted**, `<posid>:<nonce>:<seq>`, for a row this build creates. `posid` is the
//!   station's position id (`fd_position_id`, 0 while the profile has none), `nonce` is drawn
//!   once per [`Minter`] (one per open log) from the OS-seeded hasher keys, and `seq` counts
//!   from 1. Two machines running one copied settings file share a posid; they collide only
//!   if their nonces match too, about n²/2⁶⁵ for n sessions.
//! - **Provisional**, `~<hash>` or `~<hash>.<k>`, for a row that arrived without one: a
//!   pre-1.14 log, another logger's file, a row another build appended. `hash` is FNV-1a-64 of
//!   the record's own text as the loader decoded it (after the previous `<EOR>` through this
//!   one, trimmed) and `k` its ordinal among equal hashes in file order. That is a function of
//!   the file alone, so two instances reading one file agree without talking. The first
//!   rewrite persists it as it is, and every later load reads it back; it is never converted
//!   to a minted id.
//!
//!   ⚠️ The hash covers the text AS DECODED (lossy UTF-8). A future change to the loader's
//!   decoding would move the provisional id of a row that was never persisted. The first
//!   rewrite persists every id, and two instances holding different ids for one row converge
//!   on the smaller at their next reconcile ([`RecordId::adopt`]), so the window is bounded,
//!   but it exists.
//!
//! Assigning an id never writes the file: an id rides the next append or rewrite of its row.

use std::collections::{HashMap, HashSet};

/// See the module header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordId {
    Minted { posid: u32, nonce: u64, seq: u32 },
    Provisional { hash: u64, ordinal: u32 },
}

impl RecordId {
    /// The id two instances holding different ids for ONE row both settle on: the smaller in
    /// the text form, the one both of them can see. Any total order would converge; this one
    /// also puts a minted id ahead of a provisional one (`~` sorts after every hex digit).
    pub fn adopt(a: RecordId, b: RecordId) -> RecordId {
        if a.to_string() <= b.to_string() {
            a
        } else {
            b
        }
    }
}

impl std::fmt::Display for RecordId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            RecordId::Minted { posid, nonce, seq } => write!(f, "{posid:08x}:{nonce:016x}:{seq}"),
            RecordId::Provisional { hash, ordinal: 0 } => write!(f, "~{hash:016x}"),
            RecordId::Provisional { hash, ordinal } => write!(f, "~{hash:016x}.{ordinal}"),
        }
    }
}

/// Only the canonical text parses, so text identity IS id identity: two spellings of one id
/// cannot exist, and "the same id twice in a file" is a string comparison.
impl std::str::FromStr for RecordId {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        fn hex<T>(
            s: &str,
            digits: usize,
            parse: fn(&str, u32) -> Result<T, std::num::ParseIntError>,
        ) -> Result<T, ()> {
            if s.len() == digits && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                parse(s, 16).map_err(|_| ())
            } else {
                Err(())
            }
        }
        // A count from 1, in plain decimal: no sign, no leading zero.
        fn count(s: &str) -> Result<u32, ()> {
            match s.as_bytes() {
                [b'1'..=b'9', rest @ ..] if rest.iter().all(u8::is_ascii_digit) => {
                    s.parse().map_err(|_| ())
                }
                _ => Err(()),
            }
        }
        if let Some(rest) = s.strip_prefix('~') {
            let (hash, ordinal) = match rest.split_once('.') {
                Some((hash, k)) => (hash, count(k)?),
                None => (rest, 0),
            };
            return Ok(RecordId::Provisional {
                hash: hex(hash, 16, u64::from_str_radix)?,
                ordinal,
            });
        }
        let mut parts = s.split(':');
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(posid), Some(nonce), Some(seq), None) => Ok(RecordId::Minted {
                posid: hex(posid, 8, u32::from_str_radix)?,
                nonce: hex(nonce, 16, u64::from_str_radix)?,
                seq: count(seq)?,
            }),
            _ => Err(()),
        }
    }
}

/// Mints ids for the rows one open log creates. See the module header.
#[derive(Debug)]
pub struct Minter {
    posid: u32,
    nonce: u64,
    next_seq: u32,
}

impl Minter {
    /// A minter whose nonce appears in none of `taken`: the nonces of the minted ids the
    /// loaded file already carries, under any posid.
    pub fn new(posid: u32, taken: &HashSet<u64>) -> Self {
        Self::drawing(posid, taken, draw_nonce)
    }

    fn drawing(posid: u32, taken: &HashSet<u64>, mut draw: impl FnMut() -> u64) -> Self {
        let nonce = std::iter::repeat_with(&mut draw)
            .find(|n| !taken.contains(n))
            .expect("repeat_with never ends");
        Self {
            posid,
            nonce,
            next_seq: 1,
        }
    }

    pub fn set_posid(&mut self, posid: u32) {
        self.posid = posid;
    }

    /// The position id the ids it mints from here carry.
    pub fn posid(&self) -> u32 {
        self.posid
    }

    /// A minter for a log whose rows carry `ids`: under `posid`, and clear of the nonce of every
    /// minted id among them — the station's, for a log it did not load itself (SPEC-2 v3 C19: the
    /// store's rows, read in the pass that builds the hot index).
    pub fn clear_of<'a>(posid: u32, ids: impl IntoIterator<Item = &'a RecordId>) -> Self {
        Self::new(posid, &minted_nonces(ids))
    }

    pub fn mint(&mut self) -> RecordId {
        if self.next_seq == u32::MAX {
            // Four billion rows in one session: start a fresh run under a new nonce rather
            // than wrap onto ids already handed out.
            *self = Self::drawing(self.posid, &HashSet::from([self.nonce]), draw_nonce);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        RecordId::Minted {
            posid: self.posid,
            nonce: self.nonce,
            seq,
        }
    }
}

impl Default for Minter {
    fn default() -> Self {
        Self::new(0, &HashSet::new())
    }
}

/// A copy of a log mints under a nonce of its own, or the two would hand out the same ids.
impl Clone for Minter {
    fn clone(&self) -> Self {
        Self::drawing(self.posid, &HashSet::from([self.nonce]), draw_nonce)
    }
}

/// The nonce of every minted id among `ids` — what a minter for the log they name draws clear of.
/// A provisional id was minted by no one, and names none.
fn minted_nonces<'a>(ids: impl IntoIterator<Item = &'a RecordId>) -> HashSet<u64> {
    ids.into_iter()
        .filter_map(|id| match id {
            RecordId::Minted { nonce, .. } => Some(*nonce),
            RecordId::Provisional { .. } => None,
        })
        .collect()
}

/// 64 bits from the OS-seeded hasher keys (every `RandomState` has fresh ones), mixed with the
/// process id and the clock. No new dependency; ids are identity, not secrets.
fn draw_nonce() -> u64 {
    use std::hash::{BuildHasher, Hash, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    std::process::id().hash(&mut h);
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
        .hash(&mut h);
    h.finish()
}

/// FNV-1a, 64-bit: fixed forever, because a provisional id is only stable while its hash is.
pub(super) fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Settle the ids of a file's records, in file order, as a function of the file alone.
///
/// A row keeps the id it carries unless an earlier row already holds it, in which case the
/// first holder keeps it and this row counts as carrying none. Every row left without one
/// gets a provisional id from `span` (its own text): the hash, and the first ordinal from its
/// place among equal hashes that is still free. Returns the nonces of the minted ids kept, so
/// the open log's [`Minter`] can steer clear of them.
pub(super) fn settle_file_ids<'a>(
    rows: impl IntoIterator<Item = (&'a mut Option<RecordId>, &'a str)>,
) -> HashSet<u64> {
    let rows: Vec<_> = rows.into_iter().collect();
    let mut taken: HashSet<RecordId> = HashSet::new();
    let mut nonces = HashSet::new();
    let mut unset = Vec::new();
    for (i, (id, _)) in rows.iter().enumerate() {
        match **id {
            Some(held) if taken.insert(held) => {
                if let RecordId::Minted { nonce, .. } = held {
                    nonces.insert(nonce);
                }
            }
            _ => unset.push(i),
        }
    }
    let mut seen: HashMap<u64, u32> = HashMap::new();
    let mut rows = rows;
    for i in unset {
        let (id, span) = &mut rows[i];
        let hash = fnv1a64(span.trim().as_bytes());
        let next = seen.entry(hash).or_insert(0);
        let mut ordinal = *next;
        while taken.contains(&RecordId::Provisional { hash, ordinal }) {
            ordinal += 1;
        }
        *next = ordinal + 1;
        let settled = RecordId::Provisional { hash, ordinal };
        taken.insert(settled);
        **id = Some(settled);
    }
    nonces
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical text is the ONLY spelling that parses, which is what lets "the same id
    /// twice in one file" be a string comparison: no second spelling of one id can exist.
    #[test]
    fn only_the_canonical_text_parses_and_it_round_trips() {
        let ids = [
            RecordId::Minted {
                posid: 0,
                nonce: 1,
                seq: 1,
            },
            RecordId::Minted {
                posid: 0xdead_beef,
                nonce: u64::MAX,
                seq: u32::MAX,
            },
            RecordId::Provisional {
                hash: 0,
                ordinal: 0,
            },
            RecordId::Provisional {
                hash: 0x0123_4567_89ab_cdef,
                ordinal: 7,
            },
        ];
        for id in ids {
            let text = id.to_string();
            assert_eq!(text.parse::<RecordId>(), Ok(id), "round trip of {text}");
        }
        assert_eq!(
            ids[0].to_string(),
            "00000000:0000000000000001:1",
            "fixed widths, lower-case hex, a decimal count"
        );
        assert_eq!(ids[2].to_string(), "~0000000000000000");
        assert_eq!(ids[3].to_string(), "~0123456789abcdef.7");
        for bad in [
            "",
            "~",
            "0:1:1",                            // unpadded posid
            "00000000:1:1",                     // unpadded nonce
            "00000000:0000000000000001:0",      // a count starts at 1
            "00000000:0000000000000001:01",     // ...with no leading zero
            "00000000:0000000000000001:+1",     // ...and no sign
            "00000000:0000000000000001:1:1",    // one field too many
            "00000000:0000000000000001",        // one too few
            "0000000G:0000000000000001:1",      // not hex
            "00000000:0000000000000001:1 ",     // no surrounding space
            "0000000A:0000000000000001:1",      // upper-case hex is a second spelling
            "~000000000000000",                 // 15 digits
            "~00000000000000000",               // 17
            "~0000000000000000.0",              // ordinal 0 is written without the suffix
            "~0000000000000000.",               // ...and never bare
            "~0000000000000000.1.2",            // one ordinal
            "00000000:0000000000000001:1extra", // no trailing rubbish
        ] {
            assert_eq!(bad.parse::<RecordId>(), Err(()), "{bad:?} must not parse");
        }
    }

    /// R1. Two logs open under one posid — the operator's copied settings file on two
    /// machines — hand out 200_000 ids between them without a single collision, because each
    /// drew its own nonce.
    #[test]
    fn two_minters_sharing_a_posid_hand_out_no_id_twice() {
        const EACH: usize = 100_000;
        let run = || {
            std::thread::spawn(|| {
                let mut m = Minter::new(0x1234_5678, &HashSet::new());
                (0..EACH).map(|_| m.mint()).collect::<Vec<_>>()
            })
        };
        let (a, b) = (run(), run());
        let mut all: HashSet<RecordId> = a.join().unwrap().into_iter().collect();
        all.extend(b.join().unwrap());
        assert_eq!(all.len(), 2 * EACH, "every id distinct");
    }

    /// A nonce the loaded file already carries is redrawn — the one case where a fresh draw
    /// would be a real collision, since those rows are already minted under it.
    #[test]
    fn a_minter_draws_past_every_nonce_the_file_already_holds() {
        let taken = HashSet::from([7, 8, 9]);
        let mut draws = [7, 9, 8, 11, 12].into_iter();
        let m = Minter::drawing(0, &taken, || draws.next().unwrap());
        assert_eq!(m.nonce, 11, "the first free draw, not the first draw");
        assert_eq!(draws.next(), Some(12), "and it stopped drawing there");
    }

    /// The station's minter for a log it did not load itself (SPEC-2 v3 C19): drawn clear of the
    /// nonce of every minted id the log's rows carry — and of nothing a provisional id's hash
    /// happens to equal.
    #[test]
    fn a_minter_for_a_log_is_drawn_clear_of_every_nonce_its_rows_carry() {
        let ids = [
            RecordId::Minted {
                posid: 1,
                nonce: 7,
                seq: 1,
            },
            RecordId::Provisional {
                hash: 11,
                ordinal: 0,
            },
            RecordId::Minted {
                posid: 2,
                nonce: 9,
                seq: 4,
            },
        ];
        let taken = minted_nonces(&ids);
        assert_eq!(taken, HashSet::from([7, 9]));
        let mut draws = [9, 7, 11].into_iter();
        let m = Minter::drawing(3, &taken, || draws.next().unwrap());
        assert_eq!(
            (m.posid, m.nonce),
            (3, 11),
            "the first draw no row's id carries"
        );
    }

    /// A copied log mints under a nonce of its own: two Logbooks cloned from one must not
    /// hand out the same ids to different contacts.
    #[test]
    fn a_copied_log_mints_under_a_nonce_of_its_own() {
        let mut a = Minter::new(5, &HashSet::new());
        let mut b = a.clone();
        assert_ne!(a.nonce, b.nonce);
        assert_ne!(a.mint(), b.mint(), "...so their first ids differ");
        assert_eq!(b.posid, 5, "the position is the station's, and is kept");
    }

    /// Four billion rows in one session: the sequence starts again under a NEW nonce rather
    /// than wrapping onto ids already handed out.
    #[test]
    fn a_run_that_exhausts_its_sequence_starts_a_new_one() {
        let mut m = Minter::new(0, &HashSet::new());
        let (first, was) = (m.mint(), m.nonce);
        m.next_seq = u32::MAX;
        let after = m.mint();
        assert_ne!(m.nonce, was, "a fresh run");
        assert_eq!(
            after,
            RecordId::Minted {
                posid: 0,
                nonce: m.nonce,
                seq: 1
            },
            "counting from 1 again"
        );
        assert_ne!(after, first);
    }

    /// One id twice in a file (a copied row, a merge gone wrong): the FIRST holder keeps it
    /// and the second is treated as carrying none, so the id stays unique within the file.
    #[test]
    fn a_duplicate_id_in_a_file_stays_with_its_first_holder() {
        let dup = RecordId::Minted {
            posid: 1,
            nonce: 2,
            seq: 3,
        };
        let mut ids = [Some(dup), Some(dup), None];
        let spans = ["first", "second", "third"];
        let nonces = settle_file_ids(ids.iter_mut().zip(spans));
        assert_eq!(ids[0], Some(dup), "the first holder keeps it");
        assert_eq!(
            ids[1],
            Some(RecordId::Provisional {
                hash: fnv1a64(b"second"),
                ordinal: 0
            }),
            "the second falls back to its own text"
        );
        assert_eq!(
            ids[2],
            Some(RecordId::Provisional {
                hash: fnv1a64(b"third"),
                ordinal: 0
            })
        );
        assert_eq!(nonces, HashSet::from([2]), "the nonce in use is reported");
    }

    /// Rows whose text is identical are told apart by their ordinal, and an ordinal another
    /// row already holds is skipped — the id is unique in the file either way.
    #[test]
    fn rows_that_read_alike_are_told_apart_by_their_ordinal() {
        let hash = fnv1a64(b"same");
        let mut ids = [
            None,
            None,
            Some(RecordId::Provisional { hash, ordinal: 2 }),
            None,
        ];
        let spans = ["same", " same ", "held", "same"];
        settle_file_ids(ids.iter_mut().zip(spans));
        assert_eq!(
            [ids[0], ids[1], ids[3]],
            [
                Some(RecordId::Provisional { hash, ordinal: 0 }),
                Some(RecordId::Provisional { hash, ordinal: 1 }),
                Some(RecordId::Provisional { hash, ordinal: 3 }),
            ],
            "surrounding space is not part of the row; ordinal 2 was taken"
        );
    }

    /// Two instances each gave one row an id; whichever pairs with whichever, both land on
    /// the same one, so they converge without talking.
    #[test]
    fn adoption_picks_the_same_id_from_either_side() {
        let minted = RecordId::Minted {
            posid: 0,
            nonce: 1,
            seq: 1,
        };
        let other = RecordId::Minted {
            posid: 0,
            nonce: 2,
            seq: 1,
        };
        let prov = RecordId::Provisional {
            hash: 0,
            ordinal: 0,
        };
        for (a, b) in [(minted, other), (minted, prov), (prov, other)] {
            assert_eq!(RecordId::adopt(a, b), RecordId::adopt(b, a), "commutative");
            let once = RecordId::adopt(a, b);
            assert_eq!(RecordId::adopt(once, b), once, "and idempotent");
        }
        assert_eq!(RecordId::adopt(minted, other), minted, "the smaller text");
        assert_eq!(
            RecordId::adopt(minted, prov),
            minted,
            "a minted id outranks a provisional one (`~` sorts last)"
        );
    }
}
