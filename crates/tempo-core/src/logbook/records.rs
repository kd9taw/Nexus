//! Mutation-tracked record storage for consistent reads in small chunks.
//! Contents remain private here so every write passes through DerefMut.
//!
//! The same choke point keeps the log's REVISION. Every write moves it, and every write that
//! is not an append at the end also marks the log REWRITTEN. So a result cached against a
//! revision cannot outlive the records it was built from, and a reader holding the first `n`
//! rows of an older revision can be told whether those rows still stand
//! ([`Records::appended_only_since`]).
use super::QsoRecord;
use std::sync::atomic::{AtomicU64, Ordering};

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

/// A read token becomes obsolete before any mutable access to the vector. The
/// wrapper covers indexing, mutable slices and all Vec methods, including future
/// mutation sites. It does not change records, ADIF, reconciliation or saves.
/// A reader retains its Arc, so a later allocation cannot reuse that identity.
#[derive(Debug, Clone)]
pub(super) struct Records {
    values: Vec<QsoRecord>,
    token: std::sync::Arc<()>,
    /// Moves on every write (see the module header).
    revision: u64,
    /// The revision of the last write that was not an append at the end. Every `DerefMut`
    /// counts, because this wrapper cannot see what the caller does with the `&mut Vec` it
    /// hands out; counting one that changed nothing costs a rebuild, never a stale answer.
    rewritten_at: u64,
}
impl Records {
    pub(super) fn read_token(&self) -> std::sync::Arc<()> {
        self.token.clone()
    }
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }
    /// Whether every write since these records stood at `revision` was an append: the rows
    /// held then are, unchanged and in order, the first rows held now. False for any revision
    /// they did not hold after their last rewrite, including one from their future or from a
    /// log they replaced.
    pub(super) fn appended_only_since(&self, revision: u64) -> bool {
        self.rewritten_at <= revision && revision <= self.revision
    }
    /// Append one record at the end, the one write that is not a rewrite. An inherent method,
    /// so `records.push(..)` resolves here before it could reach `Vec::push` through
    /// `DerefMut`.
    pub(super) fn push(&mut self, rec: QsoRecord) {
        self.obsolete_read_token();
        self.values.push(rec);
        self.revision = next_revision();
    }
    fn obsolete_read_token(&mut self) {
        // With no retained reader/clone there is no old identity to invalidate.
        // Ordinary logging therefore adds no token allocation per contact.
        if std::sync::Arc::strong_count(&self.token) > 1 {
            self.token = std::sync::Arc::new(());
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
        // claim its rows.
        let revision = next_revision();
        Self {
            values,
            token: std::sync::Arc::new(()),
            revision,
            rewritten_at: revision,
        }
    }
}
impl std::ops::Deref for Records {
    type Target = Vec<QsoRecord>;
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}
impl std::ops::DerefMut for Records {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.obsolete_read_token();
        self.revision = next_revision();
        self.rewritten_at = self.revision;
        &mut self.values
    }
}
impl<'a> IntoIterator for &'a Records {
    type Item = &'a QsoRecord;
    type IntoIter = std::slice::Iter<'a, QsoRecord>;
    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}
