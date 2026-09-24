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
//! change is applied by taking the rows it touched out ([`HotIndex::catch_up`] reads them from
//! the picture the log was in before the change) and putting them back as they are now: work in
//! proportion to the change, not to the log. Four ways to catch up, cheapest first:
//!
//! - **Nothing an index reads moved** — an upload stamp, a QSL-sent mark, an id: the log's
//!   `index_rev` has not passed the revision the index holds, so it only moves on (O(1)).
//! - **Rows were appended** — a logged contact: the new rows alone are added.
//! - **Anything else, with the rows as they stood before it** — an edit, a delete, a merge, a
//!   purge: the two pictures are walked side by side, and only the rows whose pointer changed
//!   are compared, and only those whose indexed fields changed are taken out and put back.
//! - **Anything else, without that picture** — a log loaded or replaced, a resolver changed: the
//!   index is rebuilt. That is a pass over the whole log, counted by `LOG_SWEEPS` and, in debug
//!   builds, by [`HOT_REBUILDS`], so a test can pin where it may and may not happen.
//!
//! # Order
//!
//! Two answers depend on LOG ORDER — the newest row of a station decides the partner's grid —
//! and positions shift when a row is deleted. So every row carries an order key the index hands
//! out: the key of the row at each position is kept beside the log ([`HotIndex::catch_up`]
//! walks it with the rows), a row that stays keeps its key, and a row added or moved gets a key
//! past every key so far, which is where the walk puts it in the log. Keys increase with
//! position, always; the debug check below says so.
//!
//! # What it reads, and nothing else
//!
//! The call, band, mode, time, grid, propagation mode, park references (theirs and mine), the
//! award-grade confirmation and the contest exchange — [`Project`], the one place those fields
//! are read. A change touching none of them is skipped because the two projections compare
//! equal, so the list of fields an index reads and the list the skip compares cannot drift.
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

use super::{Logbook, QsoRecord, WorkedSince};
use crate::contest::DupeRule;
use crate::message::base_call;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

// Whole-index rebuilds, DEBUG BUILDS ONLY — per thread, like `LOG_SWEEPS`, because the test
// harness runs tests in parallel. A rebuild is a pass over the whole log; the tests that pin
// "a contact, a stamp or an edit costs none" read this. (Plain comments: doc comments cannot
// attach through `thread_local!`.)
#[cfg(debug_assertions)]
thread_local! {
    pub static HOT_REBUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Up to this many rows, a debug build checks the whole index after every change it applies:
/// every row's entry against the row, and every count against a recount from the rows. Past
/// it the check would make a debug build with a lifetime log crawl; the parity tests run far
/// below it.
#[cfg(debug_assertions)]
const VERIFY_ROWS: usize = 1_000;

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
/// "last worked" read, and the entity the row was counted under.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    order: u64,
    band: u32,
    mode: u32,
    when: u64,
    grid: Option<Box<str>>,
    entity: Option<u32>,
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
}

/// The revision and length of the log the index holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct At {
    rev: u64,
    len: usize,
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

/// The hot index set. See the module header.
#[derive(Debug, Default)]
pub struct HotIndex {
    /// The log the index holds; `None` until the first catch-up, and after [`Self::invalidate`].
    at: Option<At>,
    /// The order key of the row at each position of the log.
    order: Vec<u64>,
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

    /// Forget the log: the next catch-up rebuilds. For a change the index cannot follow row by
    /// row — a new DXCC resolver re-keys every row's entity.
    pub fn invalidate(&mut self) {
        self.at = None;
    }

    /// Bring the index up to `log` as it stands. `before` is the log's rows as they stood at the
    /// revision named with them — the picture a change started from — when the caller has it:
    /// with it, any change is applied row by row; without it, only a change no index reads and
    /// an append are, and anything else rebuilds. See the module header.
    pub fn catch_up(
        &mut self,
        log: &Logbook,
        before: Option<(u64, &[Arc<QsoRecord>])>,
        keys: &dyn HotKeys,
    ) {
        let rows = log.records();
        let rev = log.revision();
        let Some(at) = self.at else {
            return self.rebuild(log, keys);
        };
        if at.rev == rev {
            return;
        }
        // A log only moves forward: a revision below the index's is a different log (an
        // older copy swapped in), and nothing the index holds can be trusted against it.
        let forward = at.rev <= rev;
        if forward && log.index_rev() <= at.rev && rows.len() == at.len {
            // Only writes no index reads since — stamps, QSL-sent marks, ids.
            self.at = Some(At { rev, len: at.len });
        } else if forward && log.appended_only_since(at.rev) && at.len <= rows.len() {
            for r in &rows[at.len..] {
                let k = self.next_key();
                self.add(&project(r), k, keys);
                self.order.push(k);
            }
            self.at = Some(At {
                rev,
                len: rows.len(),
            });
        } else if let Some((_, before)) =
            before.filter(|(from, b)| *from == at.rev && b.len() == at.len)
        {
            self.walk(before, rows, keys);
            self.at = Some(At {
                rev,
                len: rows.len(),
            });
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

    /// Every row, from scratch.
    fn rebuild(&mut self, log: &Logbook, keys: &dyn HotKeys) {
        super::note_log_sweep();
        #[cfg(debug_assertions)]
        HOT_REBUILDS.with(|c| c.set(c.get() + 1));
        let session = self.session.take().map(|s| Session::new(s.cutoff, s.rule));
        *self = Self {
            session,
            ..Self::default()
        };
        let rows = log.records();
        self.order.reserve(rows.len());
        for r in rows {
            let k = self.next_key();
            self.add(&project(r), k, keys);
            self.order.push(k);
        }
        self.at = Some(At {
            rev: log.revision(),
            len: rows.len(),
        });
    }

    /// `before` → `after`, side by side, as [`super::writer::Change::between`] walks them: a
    /// row that is the same pointer is unchanged; one matched by id is taken out and put back
    /// only if what the index reads of it changed, keeping its order key; a row of `before`
    /// not where the walk expects it is taken out, and every row past the end of `before` is
    /// put in with a new key. Right even when a row was inserted mid-log: the rows after it are
    /// taken out and put back, in order, with keys past every other. Every row a log holds has
    /// an id; rows without one still come out right, only re-counted from the first row a
    /// delete shifted.
    fn walk(&mut self, before: &[Arc<QsoRecord>], after: &[Arc<QsoRecord>], keys: &dyn HotKeys) {
        if after.is_empty() {
            return self.empty();
        }
        let held = std::mem::take(&mut self.order);
        let mut order = Vec::with_capacity(after.len());
        let (mut i, mut j) = (0, 0);
        while i < before.len() && j < after.len() {
            let (b, a, k) = (&before[i], &after[j], held[i]);
            if b.id == a.id {
                if !Arc::ptr_eq(b, a) {
                    let (was, now) = (project(b), project(a));
                    if was != now {
                        self.remove(&was, k, keys);
                        self.add(&now, k, keys);
                    }
                }
                order.push(k);
                i += 1;
                j += 1;
            } else {
                self.remove(&project(b), k, keys);
                i += 1;
            }
        }
        for (b, &k) in before[i..].iter().zip(&held[i..]) {
            self.remove(&project(b), k, keys);
        }
        for a in &after[j..] {
            let k = self.next_key();
            self.add(&project(a), k, keys);
            order.push(k);
        }
        self.order = order;
    }

    /// The log was emptied: every count to nothing, the session's rule kept.
    fn empty(&mut self) {
        let session = self.session.take().map(|s| Session::new(s.cutoff, s.rule));
        let next_order = self.next_order;
        *self = Self {
            session,
            next_order,
            ..Self::default()
        };
    }

    fn add(&mut self, p: &Project<'_>, order: u64, keys: &dyn HotKeys) {
        let entity = keys.entity(p.call);
        let row = Row {
            order,
            band: self.bands.id(p.band),
            mode: self.modes.id(p.mode),
            when: p.when,
            grid: p.grid.map(Into::into),
            entity: entity.as_deref().map(|e| self.entities.id(e)),
        };
        let rows = self.by_base.entry(base_call(p.call)).or_default();
        let at = rows.partition_point(|r| r.order < order);
        rows.insert(at, row);
        self.count(p, entity.as_deref(), keys, Delta::Add);
    }

    fn remove(&mut self, p: &Project<'_>, order: u64, keys: &dyn HotKeys) {
        let base = base_call(p.call);
        let Some(rows) = self.by_base.get_mut(&base) else {
            debug_assert!(
                false,
                "hot index: no rows for {base:?} to take a row out of"
            );
            return;
        };
        let Ok(at) = rows.binary_search_by_key(&order, |r| r.order) else {
            debug_assert!(
                false,
                "hot index: no row with order key {order} under {base:?}"
            );
            return;
        };
        let row = rows.remove(at);
        if rows.is_empty() {
            self.by_base.remove(&base);
        }
        let entity = row.entity.map(|e| self.entities.name(e).to_string());
        self.count(p, entity.as_deref(), keys, Delta::Remove);
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

    /// The contest session's sweep of `log` — [`Logbook::worked_keys_since`]'s answer. Built
    /// from `log` once, when a session opens or its start or rule changes, and followed row by
    /// row from then on. `log` must be the log the index holds (a caller catches up first).
    pub fn worked_since(&mut self, log: &Logbook, cutoff: u64, rule: &DupeRule) -> WorkedSince {
        debug_assert_eq!(
            self.at.map(|at| at.rev),
            Some(log.revision()),
            "hot index: a session asked of a log the index has not caught up with"
        );
        if !self
            .session
            .as_ref()
            .is_some_and(|s| s.cutoff == cutoff && s.rule == *rule)
        {
            super::note_log_sweep();
            let mut s = Session::new(cutoff, *rule);
            for r in log.records().iter().filter(|r| r.when_unix >= cutoff) {
                let (call, exact) = session_keys(&project(r), rule);
                s.calls.add(call);
                if let Some(k) = exact {
                    s.exact.add(k);
                }
            }
            self.session = Some(s);
        }
        let s = self.session.as_ref().expect("set above");
        WorkedSince {
            exact: s.exact.0.keys().cloned().collect(),
            worked_this_session: s.calls.0.keys().cloned().collect(),
        }
    }

    /// DEBUG BUILDS: the index checked against the log it says it holds — every row's entry
    /// against the row, in log order, and every count against a recount from the rows under
    /// the entities they were counted with (so the check asks no resolver anything). Panics
    /// on the first difference.
    #[cfg(debug_assertions)]
    pub fn verify(&self, log: &Logbook, keys: &dyn HotKeys) {
        let rows = log.records();
        let at = self.at.expect("hot index: verified before it was built");
        assert_eq!(
            (at.rev, at.len, self.order.len()),
            (log.revision(), rows.len(), rows.len()),
            "hot index: holds a log other than this one"
        );
        assert!(
            self.order.windows(2).all(|w| w[0] < w[1]),
            "hot index: order keys out of log order"
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
        for (r, &k) in rows.iter().zip(&self.order) {
            let p = project(r);
            let held = self
                .by_base
                .get(&base_call(p.call))
                .and_then(|b| b.iter().find(|row| row.order == k))
                .unwrap_or_else(|| panic!("hot index: no entry for the row {:?}", r.call));
            let entity = held.entity.map(|e| self.entities.name(e).to_string());
            recount
                .by_base
                .entry(base_call(p.call))
                .or_default()
                .push(Row {
                    order: k,
                    band: recount.bands.id(p.band),
                    mode: recount.modes.id(p.mode),
                    when: p.when,
                    grid: p.grid.map(Into::into),
                    entity: entity.as_deref().map(|e| recount.entities.id(e)),
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
        /// one handed the rows as they stood before each change, as the station hands them
        /// (so every change is applied row by row), and one handed nothing (so it catches up
        /// only through the paths that need no picture, and rebuilds otherwise). After every
        /// change, every answer each gives is its oracle's — the scans the code asked before
        /// C13 — and the first never rebuilt once.
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
            HOT_REBUILDS.with(|c| c.set(0));
            for step in steps {
                let before = (log.revision(), log.records().to_vec());
                run(&mut log, step);
                followed.catch_up(&log, Some((before.0, &before.1)), &Keys);
                let followed_rebuilds = HOT_REBUILDS.with(|c| c.get());
                caught_up.catch_up(&log, None, &Keys);
                HOT_REBUILDS.with(|c| c.set(followed_rebuilds));
                assert_parity(&mut followed, &log, &probes, (cutoff, &rule))?;
                assert_parity(&mut caught_up, &log, &probes, (cutoff, &rule))?;
                followed.verify(&log, &Keys);
                caught_up.verify(&log, &Keys);
            }
            prop_assert_eq!(
                HOT_REBUILDS.with(|c| c.get()),
                0,
                "the index handed each change's picture never rebuilt"
            );
        }
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
            index.catch_up(log, Some((before.0, &before.1)), &Keys);
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

    /// Which catch-up each kind of change takes: a stamp costs nothing, an append adds its
    /// rows, a change handed its picture walks, and one without it rebuilds — as does a log
    /// swapped for an older copy of itself, which no picture describes.
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
        index.catch_up(&log, None, &Keys);
        assert_eq!(index.at.map(|a| a.rev), Some(log.revision()));
        // An append, after the stamp: the new row alone.
        log.add(row("K1ABC", "40m", Some("EM12"), 60));
        index.catch_up(&log, None, &Keys);
        assert!(index.worked_call("K1ABC"));
        // An edit, handed its picture: walked.
        let before = (log.revision(), log.records().to_vec());
        log.update_record(0, row("AA9A", "20m", Some("FN31"), 0));
        index.catch_up(&log, Some((before.0, &before.1)), &Keys);
        assert!(index.worked_call("AA9A") && !index.worked_call("W1AW"));
        assert_eq!(
            (
                HOT_REBUILDS.with(|c| c.get()),
                crate::logbook::LOG_SWEEPS.with(|c| c.get())
            ),
            (0, 0),
            "a stamp, an append and an edit handed its picture: no pass over the log"
        );

        // An edit WITHOUT its picture: rebuilt, and right.
        let older = log.clone();
        log.update_record(1, row("N0OLD", "20m", None, 0));
        index.catch_up(&log, None, &Keys);
        assert_eq!(HOT_REBUILDS.with(|c| c.get()), 1, "no picture: rebuilt");
        assert!(index.worked_call("N0OLD") && !index.worked_call("K1ABC"));
        // A log swapped for an older copy of itself: its revision is behind the index's, so
        // nothing the index holds is trusted.
        log = older;
        index.catch_up(&log, None, &Keys);
        assert_eq!(HOT_REBUILDS.with(|c| c.get()), 2, "an older log: rebuilt");
        assert!(index.worked_call("K1ABC") && !index.worked_call("N0OLD"));
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
        index.catch_up(&log, None, &Keys);
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
