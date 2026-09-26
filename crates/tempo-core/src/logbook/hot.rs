//! The hot index set — SPEC-2 v3's §4.2, step C13. What the radio loop, the snapshot and the
//! log path ask of the log on every tick, poll and contact, answered from memory that follows
//! the log's changes, instead of from a pass over the log.
//!
//! # What it answers, and the old answer each is held to
//!
//! Every lookup here replaced a scan, and the scan it replaced is kept as its ORACLE: the parity
//! test at the bottom of this file drives random sequences of every kind of change and, after
//! each, asks both.
//!
//! | question | the old answer (the oracle) |
//! |---|---|
//! | is this contact a duplicate? | [`super::dedup::scan_for_duplicate`] |
//! | the grid we logged for a partner | the NEWEST row, in log order, whose call is the partner's (`same_call`), then its grid if not blank — no fall-back to an older row |
//! | when a station was last worked | the latest `when_unix` among its rows (a non-empty base call) |
//! | worked before (B4), call scope | [`Logbook::worked_call_set`] |
//! | B4, band and band·mode scope | [`Logbook::worked_band_set`], both folds |
//! | the contest session's sweep | [`Logbook::worked_keys_since`] |
//! | NEW GRID / DXCC / BAND, confirmed, NEW PARK badges | a full rebuild of the station's worked index |
//! | the activation's contact count | the rows whose `MY_SIG_INFO` is that reference |
//!
//! # How it is kept
//!
//! Every answer is a set COUNTED by the rows that contribute to it, or a list of rows, so a
//! change is applied by taking the rows it touched out and putting them back as they are now:
//! work in proportion to the change, not to the log. A change reaches the index ONE way: as the
//! rows it took out and put in, `(before, after)` pairs ([`HotIndex::follow`], a pair at a time
//! through [`HotIndex::apply`]). A row is found by its id among its station's rows, so no pair
//! needs to know where in the log its row sits, and a pair whose row changed nothing the index
//! reads costs one comparison.
//!
//! Two other ways in, and neither is a change:
//!
//! - **A build** — a log loaded or attached, a resolver changed, another window's changes taken
//!   in: a pass over the whole log — from the store ([`HotIndex::from_store`]) or from rows in
//!   hand ([`HotIndex::from_rows`]) — counted, in debug builds, by [`HOT_REBUILDS`], so a test
//!   can pin where it may and may not happen.
//! - **A catch-up** ([`HotIndex::catch_up`]) — for a write that reached a [`Logbook`] without
//!   being followed: a write no index reads moves the index on, appends are put in as a change's
//!   are, and anything else is a build. The station never takes one (SPEC-2 v3 C19): its index
//!   holds the log as it holds it, and a debug build names a write that went around it. Kept for
//!   the model's own tests until the log in memory goes; debug builds count it
//!   ([`HOT_CATCH_UPS`]).
//!
//! # Order
//!
//! Two answers depend on LOG ORDER — the newest row of a station decides the partner's grid —
//! so every row carries an order key, and keys increase with log order, always; the debug check
//! below says so. A build keys each row by its place in the log, which is the order of the
//! store's rowids (log order IS rowid order, SPEC-2 v3 P5). A row changed in place keeps its key,
//! and so its place. A row put in gets a key past every key so far — this window's append
//! counter — which is where it lands in the log. A row the log moved (one inserted mid-log, and
//! every row after it) arrives taken out and put back in, and so goes behind every other row,
//! which is where the log now has it.
//!
//! # What it reads, and nothing else
//!
//! The call, band, mode, time, grid, propagation mode, park references (theirs and mine), the
//! award-grade confirmation and the contest exchange — [`Project`], the one place those fields
//! are read — and the row's id, by which a change finds it. A change touching none of them is
//! skipped because the two projections (and ids) compare equal, so the list of fields an index
//! reads and the list the skip compares cannot drift.
//!
//! Two answers need what tempo-core cannot compute — the band a badge is keyed on (the app's
//! band plan) and a call's DXCC entity (cty.dat) — and take them through [`HotKeys`]. The
//! entity a row was counted under is kept with the row, so a row leaves under the entity it
//! came in with whatever the resolver says later; a new resolver is a rebuild.
//!
//! # Never the disk
//!
//! Like the duplicate guard it absorbed, this has no store in scope and cannot wait on one: the
//! FT sequencer logs from the radio loop under the engine lock, and everything here runs there.

use super::sqlite::{self, LogDb};
use super::{Logbook, QsoRecord, RecordId, WorkedSince};
use crate::contest::DupeRule;
use crate::message::base_call;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

// DEBUG BUILDS ONLY — per thread, like `LOG_SWEEPS`, because the test harness runs tests in
// parallel. (Plain comments: doc comments cannot attach through `thread_local!`.)
//
// `HOT_REBUILDS`: whole-index builds — from a log, from rows, from the store. A build is a pass
// over the whole log; the tests that pin "a contact, a stamp or an edit costs none", and "the
// attach installs the index the open built instead of building one", read this.
//
// `HOT_CATCH_UPS`: writes the index had to find out about from the log instead of being told —
// see `HotIndex::catch_up`. The tests that pin "every write the station makes is followed" read
// this.
#[cfg(debug_assertions)]
thread_local! {
    pub static HOT_REBUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub static HOT_CATCH_UPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Up to this many rows, a debug build checks the whole index after every change it applies:
/// every row's entry against the row, and every count against a recount from the rows. Past
/// it the check would make a debug build with a lifetime log crawl; the parity tests run far
/// below it.
#[cfg(debug_assertions)]
pub const VERIFY_ROWS: usize = 1_000;

/// What the badge half of the index needs from outside tempo-core.
pub trait HotKeys {
    /// The band a badge is keyed on — the app's canonical band, lower-cased.
    fn band_key(&self, band: &str) -> String;
    /// A call's DXCC entity from the live resolver; `None` when it does not resolve or no
    /// resolver is wired.
    fn entity(&self, call: &str) -> Option<String>;
}

/// The 4-character Maidenhead field+square, upper-cased — the granularity grids are awarded at
/// (VUCC counts squares, not subsquares). The index and every lookup go through this one
/// function: a 6-character logged grid ("FN31PR") and a 4-character decode ("FN31") are the
/// same square only if both sides cut them the same way.
pub fn grid4(grid: &str) -> Option<String> {
    let g: String = grid.trim().to_ascii_uppercase().chars().take(4).collect();
    (g.len() == 4).then_some(g)
}

/// Whether a row is a satellite contact (`PROP_MODE=SAT`). Its grid earns Satellite-VUCC credit
/// only (the ARRL rule), so it stays out of the per-band terrestrial grid badges — mirrored by
/// the same exclusion in propagation's `LogNeeds::add_qso`.
fn is_satellite(prop_mode: Option<&str>) -> bool {
    prop_mode.is_some_and(|p| p.trim().eq_ignore_ascii_case("SAT"))
}

/// A contest exchange as a row holds it: received, then sent, slot → raw.
type Exchange<'a> = (&'a [(String, String)], &'a [(String, String)]);

/// Every field any index here reads, borrowed from one row — the only place they are read. Two
/// rows with equal projections contribute exactly the same keys.
#[derive(Debug, PartialEq)]
struct Project<'a> {
    call: &'a str,
    band: &'a str,
    mode: &'a str,
    when: u64,
    grid: Option<&'a str>,
    prop_mode: Option<&'a str>,
    their_ref: Option<&'a str>,
    my_ref: Option<&'a str>,
    award_confirmed: bool,
    /// The contest exchange, received and sent — what a session's dupe key may name.
    exchange: Option<Exchange<'a>>,
}

fn project(r: &QsoRecord) -> Project<'_> {
    Project {
        call: &r.call,
        band: &r.band,
        mode: &r.mode,
        when: r.when_unix,
        grid: r.grid.as_deref(),
        prop_mode: r.prop_mode.as_deref(),
        their_ref: r.ota.their_ref.as_deref(),
        my_ref: r.ota.my_ref.as_deref(),
        award_confirmed: r.award_confirmed,
        exchange: r
            .contest
            .as_deref()
            .map(|c| (c.rcvd.as_slice(), c.sent.as_slice())),
    }
}

/// A set counted by the rows that put each key in: a key is present while at least one row
/// contributes it, so a row can be taken out without asking whether another row holds the key.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Counted<K: Eq + Hash>(HashMap<K, u32>);

impl<K: Eq + Hash> Default for Counted<K> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<K: Eq + Hash> Counted<K> {
    fn add(&mut self, k: K) {
        *self.0.entry(k).or_insert(0) += 1;
    }
    fn remove(&mut self, k: &K) {
        match self.0.get_mut(k) {
            Some(n) if *n > 1 => *n -= 1,
            Some(_) => {
                self.0.remove(k);
            }
            // A row taking out a key it never put in: the index and the log have come apart.
            // The debug check after every change names where; a release build carries on with
            // the count it has rather than invent one.
            None => debug_assert!(false, "hot index: a key taken out that was never counted"),
        }
    }
    fn contains<Q>(&self, k: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.0.contains_key(k)
    }
    fn count<Q>(&self, k: &Q) -> u32
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.0.get(k).copied().unwrap_or(0)
    }
}

/// Strings the rows repeat — bands, modes, entities — held once and named by number.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Names {
    ids: HashMap<Box<str>, u32>,
    names: Vec<Box<str>>,
}

impl Names {
    fn id(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.ids.get(s) {
            return id;
        }
        let id = u32::try_from(self.names.len()).expect("fewer than 2^32 distinct names");
        self.names.push(s.into());
        self.ids.insert(s.into(), id);
        id
    }
    fn name(&self, id: u32) -> &str {
        &self.names[id as usize]
    }
}

/// One row, as the per-station lists hold it: what the duplicate guard, the partner's grid and
/// "last worked" read, the entity the row was counted under, and the id a change finds it by.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    id: Option<RecordId>,
    order: u64,
    band: u32,
    mode: u32,
    when: u64,
    grid: Option<Box<str>>,
    entity: Option<u32>,
    /// The row's fields as it was counted, as one number ([`digest`]) — what tells whether the
    /// index holds a row as a change read it ([`HotIndex::holds`]).
    digest: u64,
}

/// A row's projection as one number, the contest exchange left out — what tells whether the
/// index holds a row exactly as it was counted ([`HotIndex::holds`]). The exchange is left out
/// because a build from the store reads none ([`STORE_COLUMNS`]); the session's rows are opened
/// from whole records ([`HotIndex::install_session`]).
fn digest(p: &Project<'_>) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        p.call,
        p.band,
        p.mode,
        p.when,
        p.grid,
        p.prop_mode,
        p.their_ref,
        p.my_ref,
        p.award_confirmed,
    )
        .hash(&mut h);
    h.finish()
}

/// The open contest session's half of the dupe index: the rows since `cutoff`, keyed by `rule`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Session {
    cutoff: u64,
    rule: DupeRule,
    exact: Counted<Vec<String>>,
    calls: Counted<String>,
}

impl Session {
    fn new(cutoff: u64, rule: DupeRule) -> Self {
        Self {
            cutoff,
            rule,
            exact: Counted::default(),
            calls: Counted::default(),
        }
    }

    /// The session's sweep of `rows`: each from `cutoff` on, keyed by `rule`.
    fn swept<'a>(
        cutoff: u64,
        rule: DupeRule,
        rows: impl IntoIterator<Item = &'a QsoRecord>,
    ) -> Self {
        let mut s = Self::new(cutoff, rule);
        for r in rows.into_iter().filter(|r| r.when_unix >= cutoff) {
            let (call, exact) = session_keys(&project(r), &rule);
            s.calls.add(call);
            if let Some(k) = exact {
                s.exact.add(k);
            }
        }
        s
    }
}

#[derive(Clone, Copy)]
enum Delta {
    Add,
    Remove,
}

impl Delta {
    fn apply<K: Eq + Hash>(self, set: &mut Counted<K>, k: K) {
        match self {
            Delta::Add => set.add(k),
            Delta::Remove => set.remove(&k),
        }
    }
}

/// The store columns [`HotIndex::from_store`] reads: every field [`Project`] reads, where the
/// award-grade confirmation is the card and LoTW channels the decoder reads it from. The id is
/// always read. The contest exchange is not: only a contest session's sweep reads it, and a
/// build starts with no session.
pub const STORE_COLUMNS: &[&str] = &[
    "call",
    "band",
    "mode",
    "when_unix",
    "grid",
    "prop_mode",
    "ota_their_ref",
    "ota_my_ref",
    "qsl_card_rcvd_raw",
    "lotw_rcvd_raw",
];

/// One row's part in a change: the row as it stood before (`None` for a row put in) and as it
/// stands after (`None` for a row taken out). A change to the log is a list of these, in log
/// order, handed to [`HotIndex::follow`].
pub type RowPair = (Option<Arc<QsoRecord>>, Option<Arc<QsoRecord>>);

/// The hot index set. See the module header.
#[derive(Debug, Default)]
pub struct HotIndex {
    /// The revision of the log the index holds; `None` until it is built, after
    /// [`Self::invalidate`], and after a change it could not follow — until it is built again.
    at: Option<u64>,
    /// How many rows it holds.
    rows: usize,
    /// The order key the next row put in gets: past every key handed out so far.
    next_order: u64,
    /// Base call → its rows, in log order. The duplicate guard's predicate, the partner's grid
    /// and "last worked" read these.
    by_base: HashMap<String, Vec<Row>>,
    bands: Names,
    modes: Names,
    entities: Names,
    /// B4: `call.to_ascii_uppercase()`, untrimmed — [`Logbook::worked_call_set`]'s key.
    calls: Counted<String>,
    /// B4 by band: `(CALL, BAND)`.
    call_band: Counted<(String, String)>,
    /// B4 by band and mode: `(CALL, BAND\u{1}MODE)`. Both kept, so the fold is a lookup.
    call_band_mode: Counted<(String, String)>,
    /// `(grid4, band key)` of every terrestrial row with a grid.
    grids: Counted<(String, String)>,
    /// `(entity, band key)` of every row whose call resolves.
    worked: Counted<(String, String)>,
    /// The entity alone — worked on any band.
    worked_any: Counted<String>,
    /// `(entity, band key)` of every award-confirmed row whose call resolves.
    confirmed: Counted<(String, String)>,
    /// Every park or summit reference on the hunter side, split and upper-cased.
    parks: Counted<String>,
    /// Every `MY_SIG_INFO` reference, as stored.
    activations: Counted<String>,
    /// The contest session's sweep, once a session asks for it.
    session: Option<Session>,
}

impl HotIndex {
    /// An index of `log`, built in one pass — the launch's build, and the tests' reference.
    pub fn build(log: &Logbook, keys: &dyn HotKeys) -> Self {
        let mut index = Self::default();
        index.rebuild(log, keys);
        index
    }

    /// Forget the log: the index is built again whole — for the station, from the store, by
    /// its freshness poll (SPEC-2 v3 C19). For an index a panic may have left half-changed, or
    /// one that could not follow a change row by row.
    pub fn invalidate(&mut self) {
        self.at = None;
    }

    /// The revision of the log the index holds; `None` while it holds none.
    pub fn at(&self) -> Option<u64> {
        self.at
    }

    /// How many rows the index holds: every row of the log it holds, which is how the purge
    /// counts what it takes (SPEC-2 v3 C19).
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// The id of every row the index holds, in no particular order — what the purge names as
    /// the rows it takes out (SPEC-2 v3 C19), so a purge the store refused and sends again takes
    /// out those rows and never one logged after it.
    pub fn ids(&self) -> Vec<RecordId> {
        self.by_base
            .values()
            .flatten()
            .filter_map(|row| row.id)
            .collect()
    }

    /// Whether the index holds `r` as it counted it: a row with `r`'s id, under `r`'s station,
    /// counted from the same fields ([`digest`]). What a change is checked by before it is
    /// followed (SPEC-2 v3 C19): its pairs can be followed only from rows the index holds as the
    /// change read them, and a row another window changed since the index took it in is not one.
    pub fn holds(&self, r: &QsoRecord) -> bool {
        let p = project(r);
        let d = digest(&p);
        r.id.is_some()
            && self
                .by_base
                .get(&base_call(p.call))
                .is_some_and(|rows| rows.iter().any(|row| row.id == r.id && row.digest == d))
    }

    /// ★ The purge, followed as ONE change: every row taken out at once, which took the log from
    /// revision `from` to revision `to` — told with no row pairs, so the purge of a 500,000-row
    /// log costs the index one reset and not a removal per row. The index empties and keeps its
    /// order keys and the contest session's rule, as [`Self::follow`] empties it for a change
    /// that takes out every row. Applied only to the log it describes, as `follow` is.
    pub fn purge(&mut self, from: u64, to: u64) {
        match self.at {
            Some(at) if at == to => return,
            Some(at) if at == from => {}
            _ => {
                self.at = None;
                return;
            }
        }
        self.empty();
        self.at = Some(to);
    }

    /// The contest session the index keeps a sweep for — its start and its rule — if one was
    /// opened. What an index built again elsewhere must open again, from whole rows, to answer
    /// as this one does ([`Self::install_session`]).
    pub fn session_key(&self) -> Option<(u64, DupeRule)> {
        self.session.as_ref().map(|s| (s.cutoff, s.rule))
    }

    /// The index of the log the store holds, built from its rows in one pass — the launch's
    /// build, made before the engine is locked (SPEC-2 v3 C19). Each row is keyed by its rowid,
    /// which is the store's log order, and a row put in later gets a key past the last of them.
    ///
    /// It reads [`STORE_COLUMNS`] of each row and nothing else, through the store's one decoder,
    /// so a row reads here exactly as a whole-record load reads it. Run it inside one read
    /// transaction ([`LogDb::in_one_snapshot`]) to build it from one picture of the store. The
    /// index names no log until it is installed as one ([`Self::holding`]).
    pub fn from_store(db: &LogDb, keys: &dyn HotKeys) -> sqlite::Result<HotIndex> {
        #[cfg(debug_assertions)]
        HOT_REBUILDS.with(|c| c.set(c.get() + 1));
        let mut index = Self::default();
        let mut last = 0;
        db.each_narrow_after(STORE_COLUMNS, 0, &mut |rowid, r| {
            index.put(r, u64::from(rowid), keys);
            last = rowid;
        })?;
        index.next_order = u64::from(last) + 1;
        Ok(index)
    }

    /// The index of `rows` — a log's rows in log order, each keyed by its place — built where
    /// they already are, before any lock that guards the log they will become is taken (the
    /// launch's rows, loaded by the store's open). The index names no log until it is installed
    /// as one ([`Self::holding`]).
    pub fn from_rows<'a>(
        rows: impl IntoIterator<Item = &'a QsoRecord>,
        keys: &dyn HotKeys,
    ) -> Self {
        #[cfg(debug_assertions)]
        HOT_REBUILDS.with(|c| c.set(c.get() + 1));
        let mut index = Self::default();
        index.put_all(rows, keys);
        index
    }

    /// Open the contest session's sweep from `rows` — every row the log holds from `cutoff` on,
    /// and any from before it, which are passed over — read somewhere else (the store, off the
    /// lock: SPEC-2 v3 C19). From here it is followed row by row, as a sweep built by
    /// [`Self::worked_since`] is, and asking for this session sweeps nothing.
    ///
    /// ⚠️ `rows` must be the rows of the log the index holds: a row it lacks, or one it holds
    /// that `rows` lacks, is a session counted wrong until the next sweep.
    pub fn install_session<'a>(
        &mut self,
        cutoff: u64,
        rule: DupeRule,
        rows: impl IntoIterator<Item = &'a QsoRecord>,
    ) {
        self.session = Some(Session::swept(cutoff, rule, rows));
    }

    /// This index, as the index of the log at `revision` — how an index built somewhere else (the
    /// store, off the lock) is installed as the one a log follows from here.
    pub fn holding(mut self, revision: u64) -> Self {
        self.at = Some(revision);
        self
    }

    /// Whether `other` gives every answer this index gives: the same rows under each station, in
    /// the same order, and the same keys counted the same number of times — whatever order keys
    /// and name numbers each happened to hand out. What shows an index built one way is the one
    /// built another (SPEC-2 v3 C19: from the store, and from the log in memory).
    pub fn answers_as(&self, other: &HotIndex) -> bool {
        type Station<'a> = Vec<(
            Option<RecordId>,
            &'a str,
            &'a str,
            u64,
            Option<&'a str>,
            Option<&'a str>,
        )>;
        fn stations(ix: &HotIndex) -> HashMap<&str, Station<'_>> {
            ix.by_base
                .iter()
                .map(|(base, rows)| {
                    let rows = rows
                        .iter()
                        .map(|r| {
                            (
                                r.id,
                                ix.bands.name(r.band),
                                ix.modes.name(r.mode),
                                r.when,
                                r.grid.as_deref(),
                                r.entity.map(|e| ix.entities.name(e)),
                            )
                        })
                        .collect();
                    (base.as_str(), rows)
                })
                .collect()
        }
        self.rows == other.rows
            && stations(self) == stations(other)
            && self.calls == other.calls
            && self.call_band == other.call_band
            && self.call_band_mode == other.call_band_mode
            && self.grids == other.grids
            && self.worked == other.worked
            && self.worked_any == other.worked_any
            && self.confirmed == other.confirmed
            && self.parks == other.parks
            && self.activations == other.activations
            && self.session == other.session
    }

    /// ★ Follow one change to the log — the one way a change reaches the index. `pairs` are the
    /// rows it took out and put in, in log order ([`RowPair`]), and it took the log from revision
    /// `from` to revision `to`.
    ///
    /// Applied only to the log they describe. An index already at `to` has the change. One at
    /// neither `from` nor `to` holds some other state of the log, which these pairs cannot bring
    /// up to date: it lets go of the log, to be built again whole — as does one handed a row to
    /// take out that it does not hold.
    pub fn follow(&mut self, pairs: &[RowPair], from: u64, to: u64, keys: &dyn HotKeys) {
        match self.at {
            Some(at) if at == to => return,
            Some(at) if at == from => {}
            _ => {
                self.at = None;
                return;
            }
        }
        // Every row taken out and none put in: the log was emptied. One reset, not a removal
        // per row.
        if !pairs.is_empty()
            && pairs.len() == self.rows
            && pairs.iter().all(|(b, a)| b.is_some() && a.is_none())
        {
            self.empty();
        } else {
            for (before, after) in pairs {
                if !self.apply(before.as_deref(), after.as_deref(), keys) {
                    self.at = None;
                    return;
                }
            }
        }
        self.at = Some(to);
    }

    /// One row's part in a change — see [`Self::follow`], through which a change reaches here.
    ///
    /// `(None, Some)` puts a row in with a key past every other: it is the log's newest.
    /// `(Some, None)` takes a row out. `(Some, Some)` is a row changed in place: it keeps its key,
    /// and so its place in the log, and costs one comparison when nothing the index reads of it
    /// changed, its id included. False when the row to take out is not one the index holds: the
    /// index has come apart from the log.
    pub fn apply(
        &mut self,
        before: Option<&QsoRecord>,
        after: Option<&QsoRecord>,
        keys: &dyn HotKeys,
    ) -> bool {
        let order = match (before, after) {
            (None, None) => return true,
            (None, Some(_)) => self.next_key(),
            (Some(b), a) => {
                if a.is_some_and(|a| a.id == b.id && project(a) == project(b)) {
                    return true;
                }
                match self.take(b, keys) {
                    Some(order) => order,
                    None => return false,
                }
            }
        };
        if let Some(a) = after {
            self.put(a, order, keys);
        }
        true
    }

    /// Bring the index up to `log` as it stands, after a write that reached the log without being
    /// followed — see the module header. A write no index reads moves the index on; appends are
    /// put in as a change's are; anything else is a rebuild, as is a log swapped for an older
    /// copy of itself. SPEC-2 v3 C19: this goes with the in-memory log.
    ///
    /// ⚠️ A write that changes only ids moves no index watermark ([`super::OpClass::IdOnly`]), so
    /// it is moved past here as a stamp is, though the index finds rows by id: an id change must
    /// be followed.
    pub fn catch_up(&mut self, log: &Logbook, keys: &dyn HotKeys) {
        let rev = log.revision();
        if self.at == Some(rev) {
            return;
        }
        #[cfg(debug_assertions)]
        HOT_CATCH_UPS.with(|c| c.set(c.get() + 1));
        let Some(at) = self.at else {
            return self.rebuild(log, keys);
        };
        let rows = log.records();
        // A log only moves forward: a revision below the index's is a different log (an
        // older copy swapped in), and nothing the index holds can be trusted against it.
        let forward = at <= rev;
        if forward && log.index_rev() <= at && rows.len() == self.rows {
            // Only writes no index reads since — stamps, QSL-sent marks.
            self.at = Some(rev);
        } else if forward && log.appended_only_since(at) && self.rows <= rows.len() {
            for r in &rows[self.rows..] {
                self.apply(None, Some(r), keys);
            }
            self.at = Some(rev);
        } else {
            return self.rebuild(log, keys);
        }
        #[cfg(debug_assertions)]
        if rows.len() <= VERIFY_ROWS {
            self.verify(log, keys);
        }
    }

    fn next_key(&mut self) -> u64 {
        let k = self.next_order;
        self.next_order += 1;
        k
    }

    /// Every row, from scratch, each keyed by its place in the log.
    fn rebuild(&mut self, log: &Logbook, keys: &dyn HotKeys) {
        super::note_log_sweep();
        #[cfg(debug_assertions)]
        HOT_REBUILDS.with(|c| c.set(c.get() + 1));
        let session = self.session.take().map(|s| Session::new(s.cutoff, s.rule));
        *self = Self {
            session,
            ..Self::default()
        };
        self.put_all(log.records().iter().map(|r| &**r), keys);
        self.at = Some(log.revision());
    }

    /// Put in `rows`, in log order, each keyed past every key so far.
    fn put_all<'a>(&mut self, rows: impl IntoIterator<Item = &'a QsoRecord>, keys: &dyn HotKeys) {
        for r in rows {
            let k = self.next_key();
            self.put(r, k, keys);
        }
    }

    /// The log was emptied: every count to nothing, the session's rule and the key counter kept.
    fn empty(&mut self) {
        let session = self.session.take().map(|s| Session::new(s.cutoff, s.rule));
        let next_order = self.next_order;
        *self = Self {
            session,
            next_order,
            ..Self::default()
        };
    }

    /// Put `r` in, under the order key `order`.
    fn put(&mut self, r: &QsoRecord, order: u64, keys: &dyn HotKeys) {
        let p = project(r);
        let entity = keys.entity(p.call);
        let row = Row {
            id: r.id,
            order,
            band: self.bands.id(p.band),
            mode: self.modes.id(p.mode),
            when: p.when,
            grid: p.grid.map(Into::into),
            entity: entity.as_deref().map(|e| self.entities.id(e)),
            digest: digest(&p),
        };
        let rows = self.by_base.entry(base_call(p.call)).or_default();
        let at = rows.partition_point(|r| r.order < order);
        rows.insert(at, row);
        self.rows += 1;
        self.count(&p, entity.as_deref(), keys, Delta::Add);
    }

    /// Take `r` out — found by its id among its station's rows, as the index put it in — and
    /// hand back its order key. `None` when the index holds no such row, or `r` carries no id to
    /// find it by: every row a log holds has one.
    fn take(&mut self, r: &QsoRecord, keys: &dyn HotKeys) -> Option<u64> {
        let p = project(r);
        let base = base_call(p.call);
        let found = r.id.and_then(|id| {
            let rows = self.by_base.get_mut(&base)?;
            let at = rows.iter().position(|row| row.id == Some(id))?;
            let row = rows.remove(at);
            if rows.is_empty() {
                self.by_base.remove(&base);
            }
            Some(row)
        });
        let Some(row) = found else {
            debug_assert!(
                false,
                "hot index: no row {:?} under {base:?} to take out",
                r.id
            );
            return None;
        };
        self.rows -= 1;
        let entity = row.entity.map(|e| self.entities.name(e).to_string());
        self.count(&p, entity.as_deref(), keys, Delta::Remove);
        Some(row.order)
    }

    /// Put `p`'s keys in, or take them out, of every counted set.
    fn count(&mut self, p: &Project<'_>, entity: Option<&str>, keys: &dyn HotKeys, d: Delta) {
        // B4 — exactly `worked_call_set` / `worked_band_set`'s keys.
        let call = p.call.to_ascii_uppercase();
        d.apply(
            &mut self.call_band,
            (call.clone(), Logbook::band_key(p.band, p.mode, false)),
        );
        d.apply(
            &mut self.call_band_mode,
            (call.clone(), Logbook::band_key(p.band, p.mode, true)),
        );
        d.apply(&mut self.calls, call);
        // The badges — exactly the station's worked index.
        let band = keys.band_key(p.band);
        if let Some(g4) = p
            .grid
            .filter(|_| !is_satellite(p.prop_mode))
            .and_then(grid4)
        {
            d.apply(&mut self.grids, (g4, band.clone()));
        }
        if let Some(e) = entity {
            // Award-grade (LoTW or a card), not eQSL or QRZ — what the awards screens count.
            if p.award_confirmed {
                d.apply(&mut self.confirmed, (e.to_string(), band.clone()));
            }
            d.apply(&mut self.worked, (e.to_string(), band));
            d.apply(&mut self.worked_any, e.to_string());
        }
        // Parks are not per band: a reference is hunted once, on any band. A two-fer
        // ("US-0001,US-0002") is a contact with each park, split on the separators the
        // activator export reads.
        if let Some(refs) = p.their_ref {
            for park in refs.split([',', ';']).map(str::trim) {
                if !park.is_empty() {
                    d.apply(&mut self.parks, park.to_uppercase());
                }
            }
        }
        if let Some(r) = p.my_ref {
            d.apply(&mut self.activations, r.to_string());
        }
        if let Some(s) = self.session.as_mut() {
            if p.when >= s.cutoff {
                let (call, exact) = session_keys(p, &s.rule);
                d.apply(&mut s.calls, call);
                if let Some(k) = exact {
                    d.apply(&mut s.exact, k);
                }
            }
        }
    }

    /// Whether `rec` is a contact the log already holds, by the duplicate guard's predicate —
    /// asked of the rows sharing `rec`'s base call, which are the only rows its call clause can
    /// accept (see [`super::dedup`]). The same answer [`super::dedup::scan_for_duplicate`]
    /// gives.
    pub fn is_duplicate(&self, rec: &QsoRecord) -> bool {
        self.by_base.get(&base_call(&rec.call)).is_some_and(|rows| {
            rows.iter().any(|r| {
                super::dedup::is_recent_duplicate_in_slot(
                    self.bands.name(r.band),
                    self.modes.name(r.mode),
                    r.when,
                    rec,
                )
            })
        })
    }

    /// The grid logged on the NEWEST row, in log order, whose call is `call`'s station
    /// (`same_call`) — if that row's grid is not blank. An older row's grid is never reached
    /// for: a rover's most recent square is the answer, or there is none.
    pub fn newest_grid(&self, call: &str) -> Option<String> {
        self.by_base
            .get(&base_call(call))?
            .last()?
            .grid
            .as_deref()
            .filter(|g| !g.trim().is_empty())
            .map(str::to_string)
    }

    /// Unix seconds of the most recent contact with `call`'s station, on any band or mode.
    /// `None` for a call with no base (blank, or the unresolved `<...>`), as it always was.
    pub fn last_worked(&self, call: &str) -> Option<u64> {
        let base = base_call(call);
        if base.is_empty() {
            return None;
        }
        self.by_base.get(&base)?.iter().map(|r| r.when).max()
    }

    /// B4, call scope: `call_upper` (a call, ASCII upper-cased) is in the log.
    pub fn worked_call(&self, call_upper: &str) -> bool {
        self.calls.contains(call_upper)
    }

    /// Every B4 call key: each distinct call in the log, ASCII upper-cased and untrimmed. A read
    /// of the set, for a caller that must see the keys themselves (SPEC-2 v3 C17a).
    pub fn worked_call_keys(&self) -> impl Iterator<Item = &str> {
        self.calls.0.keys().map(String::as_str)
    }

    /// B4, band scope: `(call_upper, band_key)` is in the log, where `band_key` is
    /// [`Logbook::band_key`] of the band (and, under `fold_mode`, the mode).
    pub fn worked_call_band(&self, call_upper: &str, band_key: &str, fold_mode: bool) -> bool {
        let set = if fold_mode {
            &self.call_band_mode
        } else {
            &self.call_band
        };
        set.contains(&(call_upper.to_string(), band_key.to_string()))
    }

    /// NEW GRID's question: `grid`'s square is worked on the band keyed `band_key`.
    pub fn grid_worked_on(&self, grid: &str, band_key: &str) -> bool {
        grid4(grid).is_some_and(|g4| self.grids.contains(&(g4, band_key.to_string())))
    }

    /// `entity` is worked on the band keyed `band_key`.
    pub fn entity_worked_on(&self, entity: &str, band_key: &str) -> bool {
        self.worked
            .contains(&(entity.to_string(), band_key.to_string()))
    }

    /// `entity` is confirmed (award-grade) on the band keyed `band_key`.
    pub fn entity_confirmed_on(&self, entity: &str, band_key: &str) -> bool {
        self.confirmed
            .contains(&(entity.to_string(), band_key.to_string()))
    }

    /// `entity` is worked on any band — the all-time (ATNO) question.
    pub fn entity_worked_ever(&self, entity: &str) -> bool {
        self.worked_any.contains(entity)
    }

    /// `reference` (trimmed and upper-cased by the caller) is on the hunter side of the log.
    pub fn park_in_log(&self, reference: &str) -> bool {
        self.parks.contains(reference)
    }

    /// How many rows carry `reference` as their `MY_SIG_INFO`, compared exactly.
    pub fn activation_count(&self, reference: &str) -> usize {
        self.activations.count(reference) as usize
    }

    /// ★ The contest session's sweep — [`Logbook::worked_keys_since`]'s answer — as it was
    /// opened from whole rows read elsewhere ([`Self::install_session`]) and followed row by row
    /// since. It sweeps nothing (SPEC-2 v3 C19): `None` when no session is open for `cutoff` and
    /// `rule` and the log holds a row from `cutoff` on — the caller opens one from the store's
    /// rows, or refuses. A log with no row from `cutoff` on needs no rows read: its sweep is
    /// empty, whatever the rule keys, and it is opened here — a Field Day entered before its
    /// first contact.
    pub fn session(&mut self, cutoff: u64, rule: &DupeRule) -> Option<WorkedSince> {
        let open = self
            .session
            .as_ref()
            .is_some_and(|s| s.cutoff == cutoff && s.rule == *rule);
        if !open {
            if self.by_base.values().flatten().any(|r| r.when >= cutoff) {
                return None;
            }
            self.session = Some(Session::new(cutoff, *rule));
        }
        let s = self.session.as_ref().expect("open above");
        Some(WorkedSince {
            exact: s.exact.0.keys().cloned().collect(),
            worked_this_session: s.calls.0.keys().cloned().collect(),
        })
    }

    /// The contest session's sweep of `log` — [`Logbook::worked_keys_since`]'s answer. Built
    /// from `log` once, when a session opens or its start or rule changes, and followed row by
    /// row from then on. `log` must be the log the index holds (a caller catches up first).
    /// SPEC-2 v3 C19: the station asks [`Self::session`], which sweeps nothing; this goes with
    /// the log in memory.
    pub fn worked_since(&mut self, log: &Logbook, cutoff: u64, rule: &DupeRule) -> WorkedSince {
        debug_assert_eq!(
            self.at,
            Some(log.revision()),
            "hot index: a session asked of a log the index has not caught up with"
        );
        if !self
            .session
            .as_ref()
            .is_some_and(|s| s.cutoff == cutoff && s.rule == *rule)
        {
            super::note_log_sweep();
            self.session = Some(Session::swept(
                cutoff,
                *rule,
                log.records().iter().map(|r| &**r),
            ));
        }
        let s = self.session.as_ref().expect("set above");
        WorkedSince {
            exact: s.exact.0.keys().cloned().collect(),
            worked_this_session: s.calls.0.keys().cloned().collect(),
        }
    }

    /// DEBUG BUILDS: the index checked against the log it says it holds — every row's entry,
    /// found by id, against the row, with keys rising in log order, and every count against a
    /// recount from the rows under the entities they were counted with (so the check asks no
    /// resolver anything). Panics on the first difference.
    #[cfg(debug_assertions)]
    pub fn verify(&self, log: &Logbook, keys: &dyn HotKeys) {
        let rows = log.records();
        assert_eq!(
            (self.at, self.rows),
            (Some(log.revision()), rows.len()),
            "hot index: holds a log other than this one"
        );
        let mut recount = Self {
            bands: self.bands.clone(),
            modes: self.modes.clone(),
            entities: self.entities.clone(),
            session: self
                .session
                .as_ref()
                .map(|s| Session::new(s.cutoff, s.rule)),
            ..Self::default()
        };
        let mut last = None;
        for r in rows {
            let p = project(r);
            let held = self
                .by_base
                .get(&base_call(p.call))
                .and_then(|b| b.iter().find(|row| row.id == r.id))
                .unwrap_or_else(|| panic!("hot index: no entry for the row {:?}", r.call));
            assert!(
                last < Some(held.order),
                "hot index: order keys out of log order at {:?}",
                r.call
            );
            last = Some(held.order);
            let entity = held.entity.map(|e| self.entities.name(e).to_string());
            recount
                .by_base
                .entry(base_call(p.call))
                .or_default()
                .push(Row {
                    id: r.id,
                    order: held.order,
                    band: recount.bands.id(p.band),
                    mode: recount.modes.id(p.mode),
                    when: p.when,
                    grid: p.grid.map(Into::into),
                    entity: entity.as_deref().map(|e| recount.entities.id(e)),
                    digest: digest(&p),
                });
            recount.count(&p, entity.as_deref(), keys, Delta::Add);
        }
        assert_eq!(
            recount.by_base, self.by_base,
            "hot index: the station lists"
        );
        assert_eq!(recount.calls, self.calls, "hot index: B4 calls");
        assert_eq!(recount.call_band, self.call_band, "hot index: B4 by band");
        assert_eq!(
            recount.call_band_mode, self.call_band_mode,
            "hot index: B4 by band and mode"
        );
        assert_eq!(recount.grids, self.grids, "hot index: grids");
        assert_eq!(recount.worked, self.worked, "hot index: entities");
        assert_eq!(
            recount.worked_any, self.worked_any,
            "hot index: entities, any band"
        );
        assert_eq!(recount.confirmed, self.confirmed, "hot index: confirmed");
        assert_eq!(recount.parks, self.parks, "hot index: parks");
        assert_eq!(
            recount.activations, self.activations,
            "hot index: activations"
        );
        assert_eq!(recount.session, self.session, "hot index: the session");
    }
}

/// A row's two keys in a contest session — [`Logbook::worked_keys_since`]'s, verbatim: the
/// trimmed, upper-cased call for "worked this session", and the rule's exact dupe key when the
/// row can supply every component the rule names.
fn session_keys(p: &Project<'_>, rule: &DupeRule) -> (String, Option<Vec<String>>) {
    let (rcvd, sent) = p.exchange.unwrap_or((&[][..], &[][..]));
    (
        p.call.trim().to_ascii_uppercase(),
        rule.key_of_pairs(
            p.call,
            p.band,
            crate::contest::mode_class(p.mode),
            rcvd,
            sent,
            // Terrestrial, as the sweep has always keyed a general-log row — see
            // `worked_keys_since` for why a satellite row is not keyed under its bird.
            crate::contest::SatKey::default(),
        ),
    )
}

// Debug builds only: the tests read the rebuild and sweep counters and the debug check, which a
// release build compiles away.
#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;
    use crate::logbook::dedup::scan_for_duplicate;

    /// The pairs of a change nobody described row by row: `before`, the log's rows as they stood
    /// just before it, walked beside `after` as `writer::Change::between` walks them. A row that
    /// is the same pointer is unchanged and makes no pair; a row matched by id is a row changed
    /// in place; a row of `before` not where the walk expects it was taken out; and every row
    /// past the end of `before` was put in, in order. A row inserted mid-log makes the rows after
    /// it come out and go back in behind every other row — where the log now holds them.
    ///
    /// SPEC-2 v3 C19: every change the station makes hands over its own pairs; these tests drive
    /// the index from the log model's changes, which do not.
    fn pairs_between(before: &[Arc<QsoRecord>], after: &[Arc<QsoRecord>]) -> Vec<RowPair> {
        let mut pairs = Vec::new();
        let (mut i, mut j) = (0, 0);
        // An emptied log took every row out: nothing is walked, and every row of `before` goes.
        while !after.is_empty() && i < before.len() && j < after.len() {
            let (b, a) = (&before[i], &after[j]);
            if b.id == a.id {
                if !Arc::ptr_eq(b, a) {
                    pairs.push((Some(Arc::clone(b)), Some(Arc::clone(a))));
                }
                i += 1;
                j += 1;
            } else {
                pairs.push((Some(Arc::clone(b)), None));
                i += 1;
            }
        }
        pairs.extend(before[i..].iter().map(|b| (Some(Arc::clone(b)), None)));
        pairs.extend(after[j..].iter().map(|a| (None, Some(Arc::clone(a)))));
        pairs
    }
    use crate::logbook::{parse_adif, ContestFields, LogOp, OpClass, QslVia, UploadService};
    use crate::message::same_call;
    use proptest::prelude::*;
    use std::collections::{HashMap, HashSet};

    /// A band key and an entity for every call, deterministic, and deliberately unlike the
    /// B4 keys: the badge half must be keyed by these, not by the raw band.
    struct Keys;
    impl HotKeys for Keys {
        fn band_key(&self, band: &str) -> String {
            band.trim().to_ascii_lowercase()
        }
        fn entity(&self, call: &str) -> Option<String> {
            // Two letters of the base call name the entity; a call too short resolves to none.
            let base = base_call(call);
            (base.len() >= 3).then(|| format!("E-{}", &base[..2]))
        }
    }

    // ---- the ORACLES: the answers as the code before C13 computed them, verbatim ----------

    /// `Engine::dx_grid_resolved`'s log leg, verbatim.
    fn old_newest_grid(log: &Logbook, dxcall: &str) -> Option<String> {
        log.records()
            .iter()
            .rev()
            .find(|r| same_call(&r.call, dxcall))
            .and_then(|r| r.grid.clone())
            .filter(|g| !g.trim().is_empty())
    }

    /// `StationCore::grid4`, verbatim.
    fn old_grid4(grid: &str) -> Option<String> {
        let g: String = grid.trim().to_ascii_uppercase().chars().take(4).collect();
        (g.len() == 4).then_some(g)
    }

    /// `StationCore::activation_qso_count`, verbatim.
    fn old_activation_count(log: &Logbook, reference: &str) -> usize {
        log.records()
            .iter()
            .filter(|r| r.ota.my_ref.as_deref() == Some(reference))
            .count()
    }

    #[derive(Debug, Default, PartialEq)]
    struct OldIndex {
        worked_grids: HashSet<(String, String)>,
        worked_entities: HashSet<(String, String)>,
        confirmed_entities: HashSet<(String, String)>,
        worked_parks: HashSet<String>,
        last_worked: HashMap<String, u64>,
    }

    /// `StationCore::refresh_worked_index`, rebuilding, verbatim.
    fn old_worked_index(log: &Logbook) -> OldIndex {
        let mut o = OldIndex::default();
        for r in log.records() {
            let base = base_call(&r.call);
            if !base.is_empty() {
                let at = o.last_worked.entry(base).or_insert(r.when_unix);
                *at = (*at).max(r.when_unix);
            }
            if let Some(refs) = &r.ota.their_ref {
                for p in refs.split([',', ';']) {
                    let p = p.trim();
                    if !p.is_empty() {
                        o.worked_parks.insert(p.to_uppercase());
                    }
                }
            }
            let band = Keys.band_key(&r.band);
            if let Some(g) = &r.grid {
                let sat = r
                    .prop_mode
                    .as_deref()
                    .is_some_and(|p| p.trim().eq_ignore_ascii_case("SAT"));
                if !sat {
                    if let Some(g4) = old_grid4(g) {
                        o.worked_grids.insert((g4, band.clone()));
                    }
                }
            }
            if let Some(entity) = Keys.entity(&r.call) {
                if r.award_confirmed {
                    o.confirmed_entities.insert((entity.clone(), band.clone()));
                }
                o.worked_entities.insert((entity, band));
            }
        }
        o
    }

    fn keys<K: Clone + Eq + Hash>(c: &Counted<K>) -> HashSet<K> {
        c.0.keys().cloned().collect()
    }

    // ---- what the log is made of ----------------------------------------------------------

    /// Call spellings the base-call rule has to see through: portable both ways, a compound,
    /// the hashed wrapper and its unresolved form, case, stray spaces, a call with no entity.
    const CALLS: &[&str] = &[
        "W1AW",
        "w1aw",
        "W1AW/P",
        "KH6/W1AW",
        "<W1AW>",
        " W1AW ",
        "K1ABC",
        "K1ABC/MM",
        "VP2E/AA9A",
        "AA9A",
        "<...>",
        "DL1ZZZ/4",
        "dl1zzz",
        "",
        "K1",
        "N0OLD",
    ];
    const BANDS: &[&str] = &["20m", "20M", "40m", "", "2m", " 6m "];
    const MODES: &[&str] = &["FT8", "ft8", "FT4", "CW", "SSB"];
    const GRIDS: &[Option<&str>] = &[
        None,
        Some(""),
        Some("  "),
        Some("FN31"),
        Some("fn31pr"),
        Some("JO3"),
        Some("EM12"),
        Some(" IO91 "),
    ];
    const PROPS: &[Option<&str>] = &[None, None, Some("SAT"), Some(" sat "), Some("TR")];
    const THEIR_REFS: &[Option<&str>] = &[
        None,
        None,
        Some("US-0001"),
        Some("US-0001,US-0002"),
        Some(" us-0003 ; K-0001"),
        Some(""),
    ];
    const MY_REFS: &[Option<&str>] = &[None, None, Some("US-1234"), Some("us-1234")];
    const T0: u64 = 1_788_000_000;

    const FD: DupeRule = DupeRule {
        by_call: true,
        by_band: true,
        by_mode_class: true,
        by_fields: &[],
        by_sent_fields: &[],
        mode_class_groups: &[],
        log_dupes: false,
        satellite_is_a_band: false,
        fm_satellite_once: false,
    };
    const QSO_PARTY: DupeRule = DupeRule {
        by_fields: &["QTH"],
        ..FD
    };

    #[derive(Debug, Clone)]
    struct Fields {
        call: usize,
        band: usize,
        mode: usize,
        dt: i64,
        grid: usize,
        prop: usize,
        their: usize,
        mine: usize,
        confirmed: bool,
        qth: Option<&'static str>,
    }

    fn contact(f: &Fields) -> QsoRecord {
        let mut r = parse_adif("<CALL:4>W1AW<BAND:3>20m<MODE:3>FT8<EOR>").remove(0);
        r.call = CALLS[f.call].to_string();
        r.band = BANDS[f.band].to_string();
        r.mode = MODES[f.mode].to_string();
        r.when_unix = (T0 as i64 + f.dt) as u64;
        r.grid = GRIDS[f.grid].map(str::to_string);
        r.prop_mode = PROPS[f.prop].map(str::to_string);
        r.ota.their_ref = THEIR_REFS[f.their].map(str::to_string);
        r.ota.my_ref = MY_REFS[f.mine].map(str::to_string);
        r.award_confirmed = f.confirmed;
        r.contest = f.qth.map(|q| {
            Box::new(ContestFields {
                rcvd: vec![("QTH".into(), q.into())],
                ..ContestFields::default()
            })
        });
        r
    }

    fn arb_fields() -> impl Strategy<Value = Fields> {
        (
            (0..CALLS.len(), 0..BANDS.len(), 0..MODES.len()),
            prop_oneof![
                Just(0i64),
                Just(299),
                Just(300),
                Just(301),
                Just(-300),
                Just(-301),
                -2_000i64..2_000,
            ],
            (0..GRIDS.len(), 0..PROPS.len(), 0..THEIR_REFS.len()),
            (0..MY_REFS.len(), any::<bool>()),
            prop_oneof![Just(None), Just(Some("FRAN")), Just(Some("COOK"))],
        )
            .prop_map(
                |((call, band, mode), dt, (grid, prop, their), (mine, confirmed), qth)| Fields {
                    call,
                    band,
                    mode,
                    dt,
                    grid,
                    prop,
                    their,
                    mine,
                    confirmed,
                    qth,
                },
            )
    }

    fn arb_contact() -> impl Strategy<Value = QsoRecord> {
        arb_fields().prop_map(|f| contact(&f))
    }

    /// Every kind of change the app makes to the log: appends (one, and an import of several),
    /// an edit that may move every key a row has, a delete, a purge, stamps and marks that must
    /// NOT reach the index, the upgrades that must (a confirmation, park references, a
    /// satellite tag, a country backfill), the report merge and the disk reconcile, and a
    /// replacement of the whole log (a reload).
    #[derive(Debug, Clone)]
    enum Step {
        Add(QsoRecord),
        Import(Vec<QsoRecord>),
        Edit(usize, QsoRecord),
        Delete(usize),
        Clear,
        Stamp(usize),
        QslSent(usize),
        QslCard(usize),
        SatTag(usize, bool),
        Confirm(usize, bool),
        Refs(usize, usize, usize),
        Backfill(usize),
        Merge(Vec<QsoRecord>),
        Reconcile(Vec<QsoRecord>),
        Reload,
    }

    fn arb_step() -> impl Strategy<Value = Step> {
        prop_oneof![
            6 => arb_contact().prop_map(Step::Add),
            2 => prop::collection::vec(arb_contact(), 1..4).prop_map(Step::Import),
            3 => (any::<usize>(), arb_contact()).prop_map(|(i, r)| Step::Edit(i, r)),
            2 => any::<usize>().prop_map(Step::Delete),
            1 => Just(Step::Clear),
            2 => any::<usize>().prop_map(Step::Stamp),
            1 => any::<usize>().prop_map(Step::QslSent),
            1 => any::<usize>().prop_map(Step::QslCard),
            1 => (any::<usize>(), any::<bool>()).prop_map(|(i, on)| Step::SatTag(i, on)),
            1 => (any::<usize>(), any::<bool>()).prop_map(|(i, on)| Step::Confirm(i, on)),
            1 => (any::<usize>(), 0..THEIR_REFS.len(), 0..MY_REFS.len())
                .prop_map(|(i, t, m)| Step::Refs(i, t, m)),
            1 => any::<usize>().prop_map(Step::Backfill),
            1 => prop::collection::vec(arb_contact(), 1..3).prop_map(Step::Merge),
            1 => prop::collection::vec(arb_contact(), 1..3).prop_map(Step::Reconcile),
            1 => Just(Step::Reload),
        ]
    }

    fn adif_of(rows: &[QsoRecord]) -> String {
        let mut t = crate::logbook::adif_header();
        for r in rows {
            t.push_str(&crate::logbook::adif_record_own_log(r));
        }
        t
    }

    fn run(log: &mut Logbook, step: Step) {
        let n = log.len();
        match step {
            Step::Add(r) => {
                log.add(r);
            }
            Step::Import(rows) => {
                log.import_adif(&adif_of(&rows));
            }
            Step::Edit(i, r) if n > 0 => {
                log.update_record(i % n, r);
            }
            Step::Delete(i) if n > 0 => {
                log.delete(i % n);
            }
            Step::Clear => {
                log.clear();
            }
            Step::Stamp(i) if n > 0 => {
                let id = log.records()[i % n].id.expect("id");
                log.apply(LogOp::Stamp {
                    id,
                    service: UploadService::Qrz,
                    status: crate::logbook::UploadStatus {
                        outcome: crate::logbook::UploadOutcome::Accepted,
                        when_unix: 1,
                        detail: None,
                    },
                });
            }
            Step::QslSent(i) if n > 0 => {
                log.mark_qsl_sent(i % n, Some(QslVia::Bureau), 1);
            }
            Step::QslCard(i) if n > 0 => {
                log.mark_qsl_card(i % n, true);
            }
            Step::SatTag(i, on) if n > 0 => {
                log.set_sat_tag(i % n, on.then_some("SO-50"));
            }
            Step::Confirm(i, on) if n > 0 => {
                let rows = log.records_mut(OpClass::Upgrade);
                Arc::make_mut(&mut rows[i % n]).award_confirmed = on;
            }
            Step::Refs(i, t, m) if n > 0 => {
                let rows = log.records_mut(OpClass::Upgrade);
                let r = Arc::make_mut(&mut rows[i % n]);
                r.ota.their_ref = THEIR_REFS[t].map(str::to_string);
                r.ota.my_ref = MY_REFS[m].map(str::to_string);
            }
            Step::Backfill(i) if n > 0 => {
                let rows = log.records_mut(OpClass::Upgrade);
                Arc::make_mut(&mut rows[i % n]).country = Some("X".into());
            }
            Step::Merge(rows) => {
                log.merge_report(&adif_of(&rows));
            }
            Step::Reconcile(rows) => {
                log.reconcile_disk(&adif_of(&rows));
            }
            Step::Reload => {
                let rows: Vec<QsoRecord> = log.records().iter().map(|r| (**r).clone()).collect();
                *log = Logbook::from_store(rows);
            }
            _ => {}
        }
    }

    /// Every answer the index gives, against its oracle, for every probe.
    fn assert_parity(
        index: &mut HotIndex,
        log: &Logbook,
        probes: &[QsoRecord],
        session: (u64, &DupeRule),
    ) -> Result<(), TestCaseError> {
        // The duplicate guard — the FT gate.
        for p in probes {
            prop_assert_eq!(
                index.is_duplicate(p),
                scan_for_duplicate(log, p),
                "dedup: {:?} {} {} @{} against {} rows",
                p.call,
                p.band,
                p.mode,
                p.when_unix,
                log.len()
            );
        }
        // Per station: the partner's grid and "last worked".
        let old = old_worked_index(log);
        for call in CALLS
            .iter()
            .copied()
            .chain(probes.iter().map(|p| p.call.as_str()))
        {
            prop_assert_eq!(
                index.newest_grid(call),
                old_newest_grid(log, call),
                "dx-grid: {:?}",
                call
            );
            prop_assert_eq!(
                index.last_worked(call),
                old.last_worked.get(&base_call(call)).copied(),
                "last worked: {:?}",
                call
            );
        }
        // B4, all three sets, whole.
        prop_assert_eq!(keys(&index.calls), log.worked_call_set(), "B4 calls");
        prop_assert_eq!(
            keys(&index.call_band),
            log.worked_band_set(false),
            "B4 by band"
        );
        prop_assert_eq!(
            keys(&index.call_band_mode),
            log.worked_band_set(true),
            "B4 by band and mode"
        );
        // The badges, whole.
        prop_assert_eq!(keys(&index.grids), old.worked_grids, "grids");
        prop_assert_eq!(keys(&index.worked), old.worked_entities.clone(), "entities");
        prop_assert_eq!(
            keys(&index.worked_any),
            old.worked_entities
                .iter()
                .map(|(e, _)| e.clone())
                .collect::<HashSet<_>>(),
            "entities, any band"
        );
        prop_assert_eq!(keys(&index.confirmed), old.confirmed_entities, "confirmed");
        prop_assert_eq!(keys(&index.parks), old.worked_parks, "parks");
        // The activation count, for every reference the log could hold.
        for r in MY_REFS.iter().flatten() {
            prop_assert_eq!(
                index.activation_count(r),
                old_activation_count(log, r),
                "activation {:?}",
                r
            );
        }
        // The contest session.
        prop_assert_eq!(
            index.worked_since(log, session.0, session.1),
            log.worked_keys_since(session.0, session.1),
            "session sweep"
        );
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

        /// ★ THE PARITY PROPERTY. Two indexes follow a random sequence of every kind of change:
        /// one handed each change as the rows it took out and put in — its pairs, found by id,
        /// the ONLY way it hears of a change (SPEC-2 v3 C19) — and one handed nothing, so it
        /// catches up only through the paths that need no pairs and rebuilds otherwise. After
        /// every change, every answer each gives is its oracle's — the scans the code asked
        /// before C13 — and the first never let go of the log, so it never had to be rebuilt.
        #[test]
        fn every_answer_is_the_old_scans_after_every_kind_of_change(
            steps in prop::collection::vec(arb_step(), 1..40),
            probes in prop::collection::vec(arb_contact(), 1..10),
            party in any::<bool>(),
            cutoff in prop_oneof![Just(0u64), Just(T0 - 1_000), Just(T0), Just(T0 + 1_000)],
        ) {
            let rule = if party { QSO_PARTY } else { FD };
            let mut log = Logbook::new();
            let mut followed = HotIndex::build(&log, &Keys);
            let mut caught_up = HotIndex::build(&log, &Keys);
            for step in steps {
                let before = (log.revision(), log.records().to_vec());
                run(&mut log, step);
                let pairs = pairs_between(&before.1, log.records());
                followed.follow(&pairs, before.0, log.revision(), &Keys);
                prop_assert_eq!(
                    followed.at,
                    Some(log.revision()),
                    "the index handed each change's pairs followed it, and never let go"
                );
                caught_up.catch_up(&log, &Keys);
                assert_parity(&mut followed, &log, &probes, (cutoff, &rule))?;
                assert_parity(&mut caught_up, &log, &probes, (cutoff, &rule))?;
                followed.verify(&log, &Keys);
                caught_up.verify(&log, &Keys);
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

        /// ★ THE LAUNCH'S BUILD (SPEC-2 v3 C19). The index built from the store — each row keyed
        /// by its rowid, gaps and all — answers exactly as the index built from the same rows
        /// loaded whole: the log the launch holds is those rows, so the two must agree. The
        /// store is made the way a real one is: rows written, some taken out, more put in after
        /// the gaps.
        #[test]
        fn the_index_built_from_the_store_answers_as_the_one_built_from_its_rows(
            steps in prop::collection::vec(arb_step(), 1..40),
            gone in prop::collection::vec(any::<usize>(), 0..6),
            later in prop::collection::vec(arb_contact(), 0..4),
        ) {
            let mut log = Logbook::new();
            for step in steps {
                run(&mut log, step);
            }
            let mut db = LogDb::open_in_memory().expect("a store");
            db.insert_all(log.records().iter().map(|r| (&**r, sqlite::Resolved::default())))
                .expect("written");
            let n = log.len();
            let remove: Vec<RecordId> = gone
                .iter()
                .filter(|_| n > 0)
                .map(|i| log.records()[i % n].id.expect("an id"))
                .collect();
            for r in later {
                log.add(r);
            }
            let upsert: Vec<sqlite::RowWrite> = log.records()[n..]
                .iter()
                .map(|r| sqlite::RowWrite::new(Arc::clone(r)))
                .collect();
            db.apply(sqlite::Batch {
                remove: &remove,
                upsert: &upsert,
                ..sqlite::Batch::default()
            })
            .expect("changed");

            let copy = Logbook::from_store(db.load_all().expect("loaded"));
            let from_rows = HotIndex::build(&copy, &Keys);
            let from_store = db
                .in_one_snapshot(|db| HotIndex::from_store(db, &Keys))
                .expect("built");
            prop_assert!(
                from_store.answers_as(&from_rows),
                "built from the store: {from_store:?}\nbuilt from its rows: {from_rows:?}"
            );
            // And installed as the index of that log, it follows the next change as the other does.
            let (mut installed, mut built) = (from_store.holding(copy.revision()), from_rows);
            let mut after = copy.clone();
            let before = (after.revision(), after.records().to_vec());
            after.add(row("K1ABC", "40m", Some("EM12"), 7_000));
            let pairs = pairs_between(&before.1, after.records());
            installed.follow(&pairs, before.0, after.revision(), &Keys);
            built.follow(&pairs, before.0, after.revision(), &Keys);
            prop_assert!(installed.answers_as(&built), "after a contact logged on top");
            installed.verify(&after, &Keys);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

        /// ★ THE SESSION OPENED FROM ROWS READ ELSEWHERE (SPEC-2 v3 C19). A contest session
        /// opened from rows read somewhere else — every row from a bound at or before its start —
        /// answers exactly as the sweep of the log does, sweeps nothing when it is asked, and is
        /// followed as that sweep is through every kind of change that comes after.
        #[test]
        fn a_session_opened_from_rows_read_elsewhere_is_the_sweep_of_the_log(
            before in prop::collection::vec(arb_step(), 0..30),
            after in prop::collection::vec(arb_step(), 0..20),
            party in any::<bool>(),
            cutoff in prop_oneof![Just(0u64), Just(T0 - 1_000), Just(T0), Just(T0 + 1_000)],
            slack in 0u64..3_000,
        ) {
            let rule = if party { QSO_PARTY } else { FD };
            let mut log = Logbook::new();
            for step in before {
                run(&mut log, step);
            }
            let mut index = HotIndex::build(&log, &Keys);
            let bound = cutoff.saturating_sub(slack);
            let read: Vec<QsoRecord> = log
                .records()
                .iter()
                .filter(|r| r.when_unix >= bound)
                .map(|r| QsoRecord::clone(r))
                .collect();
            index.install_session(cutoff, rule, &read);
            crate::logbook::LOG_SWEEPS.with(|c| c.set(0));
            let opened = index.worked_since(&log, cutoff, &rule);
            prop_assert_eq!(
                crate::logbook::LOG_SWEEPS.with(|c| c.get()),
                0,
                "asked for, the session sweeps nothing"
            );
            prop_assert_eq!(opened, log.worked_keys_since(cutoff, &rule), "the sweep of the log");
            for step in after {
                let before = (log.revision(), log.records().to_vec());
                run(&mut log, step);
                let pairs = pairs_between(&before.1, log.records());
                index.follow(&pairs, before.0, log.revision(), &Keys);
                prop_assert_eq!(
                    index.worked_since(&log, cutoff, &rule),
                    log.worked_keys_since(cutoff, &rule),
                    "followed through every change"
                );
                index.verify(&log, &Keys);
            }
        }
    }

    /// The POSITIVE CONTROLS for `answers_as`: it tells apart two indexes whose counts differ,
    /// and two whose station lists hold the same rows in a different order — which is what
    /// decides the partner's grid.
    #[test]
    fn answers_as_tells_a_different_count_and_a_different_order_apart() {
        let mut log = Logbook::new();
        log.add(row("W1AW", "20m", Some("FN31"), 0));
        log.add(row("W1AW/P", "40m", Some("EM12"), 60));
        let index = HotIndex::build(&log, &Keys);
        assert!(index.answers_as(&HotIndex::build(&log, &Keys)), "itself");
        let mut counted = HotIndex::build(&log, &Keys);
        counted.calls.add("K1ABC".into());
        assert!(!index.answers_as(&counted), "a count");
        let mut reversed = Logbook::new();
        for r in log.records().iter().rev() {
            reversed.add(QsoRecord::clone(r));
        }
        assert!(
            !index.answers_as(&HotIndex::build(&reversed, &Keys)),
            "the same rows in the other order"
        );
    }

    fn row(call: &str, band: &str, grid: Option<&str>, dt: i64) -> QsoRecord {
        contact(&Fields {
            call: CALLS
                .iter()
                .position(|c| *c == call)
                .expect("a listed call"),
            band: BANDS
                .iter()
                .position(|b| *b == band)
                .expect("a listed band"),
            mode: 0,
            dt,
            grid: GRIDS
                .iter()
                .position(|g| *g == grid)
                .expect("a listed grid"),
            prop: 0,
            their: 0,
            mine: 0,
            confirmed: false,
            qth: None,
        })
    }

    /// The partner's grid is the NEWEST row's in LOG order — not the latest time, not the
    /// newest row that has a grid — and a deleted or edited newest row hands the answer to the
    /// row before it, whatever that row holds.
    #[test]
    fn the_partners_grid_follows_log_order_through_deletes_and_edits() {
        let mut log = Logbook::new();
        // Log order: FN31, then (logged later, contacted earlier) EM12, then no grid at all.
        log.add(row("W1AW", "20m", Some("FN31"), 0));
        log.add(row("W1AW/P", "40m", Some("EM12"), -1_000));
        log.add(row("w1aw", "20m", None, 500));
        let mut index = HotIndex::build(&log, &Keys);
        assert_eq!(index.newest_grid("W1AW"), None, "the newest row has none");
        assert_eq!(index.newest_grid("W1AW"), old_newest_grid(&log, "W1AW"));

        let step = |log: &mut Logbook, index: &mut HotIndex, change: &dyn Fn(&mut Logbook)| {
            let before = (log.revision(), log.records().to_vec());
            change(log);
            let pairs = pairs_between(&before.1, log.records());
            index.follow(&pairs, before.0, log.revision(), &Keys);
        };
        step(&mut log, &mut index, &|log| {
            log.delete(2);
        });
        assert_eq!(index.newest_grid("W1AW").as_deref(), Some("EM12"));
        // An edit that moves the newest row to another station hands its place back.
        step(&mut log, &mut index, &|log| {
            log.update_record(1, row("K1ABC", "20m", Some("FN31"), 0));
        });
        assert_eq!(index.newest_grid("W1AW").as_deref(), Some("FN31"));
        assert_eq!(index.newest_grid("K1ABC").as_deref(), Some("FN31"));
        // …and an edit that moves a row IN puts it back at its own place in the log.
        step(&mut log, &mut index, &|log| {
            log.update_record(0, row("K1ABC", "40m", Some("EM12"), 0));
        });
        assert_eq!(
            index.newest_grid("K1ABC").as_deref(),
            Some("FN31"),
            "row 0 is older in the log than row 1, whichever was edited last"
        );
        assert_eq!(index.newest_grid("K1ABC"), old_newest_grid(&log, "K1ABC"));
        assert_eq!(index.newest_grid("W1AW"), None, "no W1AW rows left");
    }

    /// Which way each kind of change reaches the index: a stamp nobody followed costs nothing, an
    /// append nobody followed adds its rows, a change handed its pairs is followed, and one
    /// without them rebuilds — as does a log swapped for an older copy of itself, which no pairs
    /// describe.
    #[test]
    fn each_kind_of_change_takes_its_own_catch_up() {
        let mut log = Logbook::new();
        log.add(row("W1AW", "20m", Some("FN31"), 0));
        let mut index = HotIndex::build(&log, &Keys);
        HOT_REBUILDS.with(|c| c.set(0));
        crate::logbook::LOG_SWEEPS.with(|c| c.set(0));

        // A stamp: nothing an index reads.
        let id = log.records()[0].id.expect("id");
        log.apply(LogOp::Stamp {
            id,
            service: UploadService::Eqsl,
            status: crate::logbook::UploadStatus {
                outcome: crate::logbook::UploadOutcome::Accepted,
                when_unix: 1,
                detail: None,
            },
        });
        index.catch_up(&log, &Keys);
        assert_eq!(index.at, Some(log.revision()));
        // An append, after the stamp: the new row alone.
        log.add(row("K1ABC", "40m", Some("EM12"), 60));
        index.catch_up(&log, &Keys);
        assert!(index.worked_call("K1ABC"));
        // An edit, handed its pairs: followed.
        let before = (log.revision(), log.records().to_vec());
        log.update_record(0, row("AA9A", "20m", Some("FN31"), 0));
        let pairs = pairs_between(&before.1, log.records());
        assert_eq!(pairs.len(), 1, "one row changed, one pair");
        index.follow(&pairs, before.0, log.revision(), &Keys);
        assert!(index.worked_call("AA9A") && !index.worked_call("W1AW"));
        assert_eq!(
            (
                HOT_REBUILDS.with(|c| c.get()),
                crate::logbook::LOG_SWEEPS.with(|c| c.get())
            ),
            (0, 0),
            "a stamp, an append and an edit handed its pairs: no pass over the log"
        );

        // An edit WITHOUT its pairs: rebuilt, and right.
        let older = log.clone();
        log.update_record(1, row("N0OLD", "20m", None, 0));
        index.catch_up(&log, &Keys);
        assert_eq!(HOT_REBUILDS.with(|c| c.get()), 1, "no pairs: rebuilt");
        assert!(index.worked_call("N0OLD") && !index.worked_call("K1ABC"));
        // A log swapped for an older copy of itself: its revision is behind the index's, so
        // nothing the index holds is trusted.
        log = older;
        index.catch_up(&log, &Keys);
        assert_eq!(HOT_REBUILDS.with(|c| c.get()), 2, "an older log: rebuilt");
        assert!(index.worked_call("K1ABC") && !index.worked_call("N0OLD"));
    }

    /// A change's pairs are applied only to the log they describe. Handed pairs from a state it
    /// does not hold — a change it never heard of came first — the index lets go of the log and
    /// is rebuilt at its next catch-up, rather than apply pairs that do not fit what it holds;
    /// and an index a catch-up already brought past a change does not count its pairs again.
    #[test]
    fn a_change_is_followed_only_from_the_log_it_describes() {
        let mut log = Logbook::new();
        log.add(row("W1AW", "20m", Some("FN31"), 0));
        log.add(row("K1ABC", "40m", None, 60));
        let mut index = HotIndex::build(&log, &Keys);
        // A change the index never hears of…
        log.update_record(0, row("AA9A", "20m", None, 0));
        // …then one it is handed the pairs of, from a state it does not hold.
        let before = (log.revision(), log.records().to_vec());
        log.update_record(1, row("N0OLD", "20m", None, 0));
        let pairs = pairs_between(&before.1, log.records());
        index.follow(&pairs, before.0, log.revision(), &Keys);
        assert_eq!(
            index.at, None,
            "pairs from a log it does not hold: let go, not applied"
        );
        HOT_REBUILDS.with(|c| c.set(0));
        index.catch_up(&log, &Keys);
        assert_eq!(
            HOT_REBUILDS.with(|c| c.get()),
            1,
            "rebuilt at the next catch-up"
        );
        assert!(index.worked_call("AA9A") && index.worked_call("N0OLD"));
        assert!(!index.worked_call("W1AW") && !index.worked_call("K1ABC"));

        // A change a catch-up already took in: its pairs change nothing.
        let before = (log.revision(), log.records().to_vec());
        log.add(row("W1AW", "40m", None, 120));
        index.catch_up(&log, &Keys);
        let pairs = pairs_between(&before.1, log.records());
        index.follow(&pairs, before.0, log.revision(), &Keys);
        index.verify(&log, &Keys);
    }

    /// A row whose id changes — two instances settling on one id ([`RecordId::adopt`]) — is the
    /// same row. Handed as one pair, it keeps its key, and so its place and the partner's grid its
    /// place decides; and the index finds it by the new id from then on.
    #[test]
    fn a_row_whose_id_changes_keeps_its_place_and_is_found_by_the_new_id() {
        let mut log = Logbook::new();
        log.add(row("W1AW", "20m", Some("FN31"), 0));
        log.add(row("W1AW/P", "40m", Some("EM12"), -1_000));
        let mut index = HotIndex::build(&log, &Keys);
        let before = (log.revision(), log.records().to_vec());
        Arc::make_mut(&mut log.records_mut(OpClass::IdOnly)[0]).id = Some(RecordId::Provisional {
            hash: 7,
            ordinal: 0,
        });
        let pairs = vec![(
            Some(Arc::clone(&before.1[0])),
            Some(Arc::clone(&log.records()[0])),
        )];
        index.follow(&pairs, before.0, log.revision(), &Keys);
        index.verify(&log, &Keys);
        assert_eq!(
            index.newest_grid("W1AW").as_deref(),
            Some("EM12"),
            "the renamed row is still the older of the two"
        );
        // Found by its new id: an edit through it moves it to another station.
        let before = (log.revision(), log.records().to_vec());
        log.update_record(0, row("K1ABC", "20m", None, 0));
        let pairs = pairs_between(&before.1, log.records());
        index.follow(&pairs, before.0, log.revision(), &Keys);
        index.verify(&log, &Keys);
        assert!(index.worked_call("K1ABC") && !index.worked_call("W1AW"));
    }

    /// The session's sweep is built once for a session and followed after that: a contact
    /// logged in the session is counted without a second pass, and a new rule or start
    /// rebuilds it.
    #[test]
    fn the_session_sweep_is_built_once_and_then_followed() {
        let mut log = Logbook::new();
        log.add(row("N0OLD", "20m", None, -5_000));
        log.add(row("W1AW", "20m", None, 0));
        let mut index = HotIndex::build(&log, &Keys);
        crate::logbook::LOG_SWEEPS.with(|c| c.set(0));
        let w = index.worked_since(&log, T0 - 1_000, &FD);
        assert!(w.worked_this_session.contains("W1AW") && !w.worked_this_session.contains("N0OLD"));
        assert_eq!(
            crate::logbook::LOG_SWEEPS.with(|c| c.get()),
            1,
            "built once"
        );

        log.add(row("K1ABC", "40m", None, 60));
        index.catch_up(&log, &Keys);
        let w = index.worked_since(&log, T0 - 1_000, &FD);
        assert!(w.worked_this_session.contains("K1ABC"));
        assert_eq!(w, log.worked_keys_since(T0 - 1_000, &FD));
        crate::logbook::LOG_SWEEPS.with(|c| c.set(0));
        let _ = index.worked_since(&log, T0 - 1_000, &FD);
        assert_eq!(
            crate::logbook::LOG_SWEEPS.with(|c| c.get()),
            0,
            "followed, not swept again"
        );
        let _ = index.worked_since(&log, T0 - 10_000, &FD);
        assert_eq!(
            crate::logbook::LOG_SWEEPS.with(|c| c.get()),
            1,
            "a new session start is a new sweep"
        );
    }

    /// The debug check is live: an index that has come apart from its log is caught. The
    /// POSITIVE CONTROL for every `verify` the parity property passed through.
    #[test]
    #[should_panic(expected = "hot index: B4 calls")]
    fn the_debug_check_catches_an_index_apart_from_its_log() {
        let mut log = Logbook::new();
        log.add(row("W1AW", "20m", Some("FN31"), 0));
        let mut index = HotIndex::build(&log, &Keys);
        index.calls.add("K1ABC".into());
        index.verify(&log, &Keys);
    }
}
