//! Mutation-tracked record storage for consistent reads in small chunks.
//! Contents remain private here so every write passes through DerefMut.
//!
//! The same choke point keeps the log's REVISION, and four narrower watermarks beside it, each
//! naming the revision at which one KIND of change last happened ([`OpClass`]). A result cached
//! against a watermark cannot outlive what it was built from, and a cache whose inputs a change
//! did not touch survives that change instead of being thrown away: an upload stamp moves no
//! index, so the worked-before sets and the awards fold stand. A reader holding the first `n`
//! rows of an older revision can still be told whether those rows stand
//! ([`Records::appended_only_since`]).
//!
//! Each record is held behind an `Arc`, so a SNAPSHOT of the log is a copy of pointers
//! ([`super::Logbook::snapshot`]): a reader takes one under the lock and does its real work
//! after releasing it, without cloning a single record. A write copies only the record it
//! touches (`Arc::make_mut`, via [`super::StoredRecord`]), and only while a snapshot still
//! shares it.
use super::QsoRecord;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Where every revision comes from: ONE counter for the whole process, not one per log. A
/// per-log count would restart when the engine's log is replaced by a fresh load, and the new
/// log's first writes would repeat revisions the old one had already handed out, so a cache
/// keyed on them would answer for the wrong log. From one counter, a revision names one state
/// of one log. Two logs share a revision only when one is an untouched clone of the other,
/// and then they hold the same records.
///
/// The count starts at the process's start time in MICROSECONDS, not at 1, so a revision an
/// earlier run of the app handed out (to a UI that kept its copy across a restart, say) cannot
/// name a state of this run's log: that run's revisions all lie below this run's start unless
/// it wrote more than once per microsecond it was alive. Microseconds since 1970 stay below
/// 2^53 until the 2250s, so a revision survives a round trip through a JS number exactly.
static NEXT_REVISION: std::sync::LazyLock<AtomicU64> = std::sync::LazyLock::new(|| {
    let start = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_micros() as u64);
    AtomicU64::new(start.max(1))
});

fn next_revision() -> u64 {
    // Relaxed is enough: `fetch_add` is a single atomic step, so no value is handed out twice
    // and a later step always returns a larger one.
    NEXT_REVISION.fetch_add(1, Ordering::Relaxed)
}

/// What a write did to the records, which is what says which watermarks it moves.
///
/// The classes exist to let a cache survive a change that cannot affect it. Naming a change
/// WIDER than it is costs a rebuild; naming it narrower serves a stale answer, so a write whose
/// class is not known is [`Structural`](OpClass::Structural) — the widest — and the honesty of
/// each claim is checked in debug builds where the op is applied.
///
/// | class | `revision` | `content_rev` | `index_rev` | `key_rev` | `shape_rev` |
/// |---|---|---|---|---|---|
/// | `IdOnly` | ✓ | ✓ | | | |
/// | `Stamp` | ✓ | ✓ | | | |
/// | `Upgrade` | ✓ | ✓ | ✓ | | |
/// | `Append` | ✓ | | ✓ | ✓ | |
/// | `Key` | ✓ | ✓ | ✓ | ✓ | ✓ |
/// | `Structural` | ✓ | ✓ | ✓ | ✓ | ✓ |
///
/// `Append` is the one class that leaves `content_rev` alone, and that is the whole point of
/// it: the rows held before an append are still held, so a reader that has them needs only the
/// tail. `Key` and `Structural` move the same watermarks and differ in what they promise about
/// POSITIONS — an edit leaves every row where it was, a delete does not — which is what an
/// id → position index reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpClass {
    /// Only the ids of held rows changed (adoption). Identity is not a field of the contact,
    /// so nothing derived from the log's CONTENT moves — but the rows are not the rows a
    /// reader holding an earlier revision has, so this is not an append.
    IdOnly,
    /// An upload/QSL-service stamp on a held row: a field no index, key or fold reads.
    Stamp,
    /// Held rows gained confirmation / credit / OTA state — monotone upgrades an index over
    /// the log's content reads, leaving every row's identity and position alone.
    Upgrade,
    /// Rows appended at the end, and nothing else touched.
    Append,
    /// A held row's identifying fields changed (an edit): its keys move, and a plan built on
    /// an earlier snapshot can no longer be rebased over it.
    Key,
    /// Rows were removed or reordered, so positions moved as well. The fallback for a write
    /// this module cannot see the shape of.
    Structural,
}

/// A read token becomes obsolete before any mutable access to the vector. The
/// wrapper covers indexing, mutable slices and all Vec methods, including future
/// mutation sites. It does not change records, ADIF, reconciliation or saves.
/// A reader retains its Arc, so a later allocation cannot reuse that identity.
#[derive(Debug, Clone)]
pub(super) struct Records {
    values: Vec<Arc<QsoRecord>>,
    token: Arc<()>,
    /// Moves on every write (see the module header).
    revision: u64,
    /// The last write that was not an append at the end — so the rows held at any revision
    /// from here up are still held, unchanged and in order.
    content_rev: u64,
    /// The last write a derived index over the rows' content must be rebuilt (or, for an
    /// append, extended) for.
    index_rev: u64,
    /// The last write that moved a row's identifying fields, by adding, editing or removing
    /// one — what a key-based index (worked-before, dedup) reads.
    key_rev: u64,
    /// The last write a plan built on an earlier snapshot cannot be rebased over: a row edited
    /// or removed. An append can be rebased over (the plan re-checks its adds against the
    /// tail) and a stamp or an upgrade can be re-applied (both are monotone), so neither moves
    /// this.
    shape_rev: u64,
}
impl Records {
    pub(super) fn read_token(&self) -> Arc<()> {
        self.token.clone()
    }
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }
    pub(super) fn content_rev(&self) -> u64 {
        self.content_rev
    }
    pub(super) fn index_rev(&self) -> u64 {
        self.index_rev
    }
    pub(super) fn key_rev(&self) -> u64 {
        self.key_rev
    }
    pub(super) fn shape_rev(&self) -> u64 {
        self.shape_rev
    }
    /// Mutable access to the records under a class the caller VOUCHES FOR: the narrower the
    /// class, the more derived state survives the write. [`DerefMut`] is the same access with
    /// no claim made, and costs the widest one.
    pub(super) fn write_as(&mut self, class: OpClass) -> &mut Vec<Arc<QsoRecord>> {
        self.mark(class);
        &mut self.values
    }
    /// Take the next revision and move the watermarks this class moves (see [`OpClass`]).
    /// Every write in this module ends here, so a watermark can only move with one.
    fn mark(&mut self, class: OpClass) -> u64 {
        use OpClass::*;
        self.obsolete_read_token();
        self.revision = next_revision();
        if class != Append {
            self.content_rev = self.revision;
        }
        if matches!(class, Upgrade | Append | Key | Structural) {
            self.index_rev = self.revision;
        }
        if matches!(class, Append | Key | Structural) {
            self.key_rev = self.revision;
        }
        if matches!(class, Key | Structural) {
            self.shape_rev = self.revision;
        }
        self.revision
    }
    /// Whether every write since these records stood at `revision` was an append: the rows
    /// held then are, unchanged and in order, the first rows held now. False for any revision
    /// they did not hold after their last rewrite, including one from their future or from a
    /// log they replaced.
    pub(super) fn appended_only_since(&self, revision: u64) -> bool {
        self.content_rev <= revision && revision <= self.revision
    }
    /// Append one record at the end, the one write that is not a rewrite. An inherent method,
    /// so `records.push(..)` resolves here before it could reach `Vec::push` through
    /// `DerefMut`.
    pub(super) fn push(&mut self, rec: QsoRecord) {
        self.values.push(Arc::new(rec));
        self.mark(OpClass::Append);
    }
    fn obsolete_read_token(&mut self) {
        // With no retained reader/clone there is no old identity to invalidate.
        // Ordinary logging therefore adds no token allocation per contact.
        if Arc::strong_count(&self.token) > 1 {
            self.token = Arc::new(());
        }
    }
}
impl Default for Records {
    fn default() -> Self {
        Vec::new().into()
    }
}
impl From<Vec<QsoRecord>> for Records {
    fn from(values: Vec<QsoRecord>) -> Self {
        // A new log is a rewrite of whatever came before it: no earlier revision can
        // claim its rows, and nothing built against one survives.
        let revision = next_revision();
        Self {
            values: values.into_iter().map(Arc::new).collect(),
            token: Arc::new(()),
            revision,
            content_rev: revision,
            index_rev: revision,
            key_rev: revision,
            shape_rev: revision,
        }
    }
}
impl std::ops::Deref for Records {
    type Target = Vec<Arc<QsoRecord>>;
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}
/// The unclassified write. This wrapper cannot see what the caller does with the `&mut Vec` it
/// hands out, so the write counts as the widest class there is; a caller that knows better says
/// so through [`Records::write_as`]. Counting a write wider than it was costs a rebuild, never a
/// stale answer — which is the direction this has to err in.
impl std::ops::DerefMut for Records {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.mark(OpClass::Structural);
        &mut self.values
    }
}
impl<'a> IntoIterator for &'a Records {
    type Item = &'a Arc<QsoRecord>;
    type IntoIter = std::slice::Iter<'a, Arc<QsoRecord>>;
    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}
