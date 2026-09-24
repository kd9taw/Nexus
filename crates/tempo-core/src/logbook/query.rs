//! The questions the UI asks of the log, answered the way the UI answered them — SPEC-2 v3's
//! **C17a**, the engine half of C17b's `LogSource`.
//!
//! Every view that shows log data used to hold the whole log and compute what it showed from it.
//! C17b turned each of those computations into a QUESTION (`ui/src/features/logAnswers.ts`) and
//! froze today's answer to every question over one fixture log as goldens
//! (`ui/src/features/__fixtures__/log-query/`). This module is the Rust port of the functions those
//! answers came from — the Logbook's order and search (`logQuery.ts`), one call's history and a
//! roster's summary (`callHistory.ts`), an entity's slots, the band map's worked calls — and it is
//! held to those goldens.
//!
//! # The port is of the JAVASCRIPT, not of what it meant
//!
//! A careless port changes answers silently, so the details that differ from Rust's defaults are
//! each ported deliberately:
//!
//! - **`trim()`** is ECMAScript's ([`js_trim`]): it trims U+FEFF and not U+0085, where
//!   `str::trim` does the opposite.
//! - **`toUpperCase()` / `toLowerCase()`** are Unicode's full case mappings — `ß` → `SS`, `ſ` →
//!   `S`, `ı` → `I`, a final sigma — which `str::to_uppercase` / `to_lowercase` also are. An
//!   ASCII-only fold (`to_ascii_uppercase`, SQLite's `upper()`) is wrong for non-ASCII text.
//! - **String comparison** is UTF-16 code unit order ([`utf16_cmp`]), which differs from Rust's
//!   byte (code point) order between a character above U+FFFF and one in U+E000..U+FFFF.
//! - **`fmtUtc`** ([`fmt_utc`]) is `Date`'s: an unpadded year, and `NaN` fields past the last
//!   instant a `Date` can hold.
//! - **Sorting** is stable, with the time as the tie-break and the WHOLE comparison negated for a
//!   descending order — so a full tie keeps log order in both directions.
//! - **`??`** falls back on a missing value only, never on an empty string.
//! - **`length` and `slice`** count UTF-16 code units ([`grid_square`]).
//!
//! # What is here, and what is not
//!
//! The functions here are pure: they take rows in LOG ORDER and answer from them. Which rows
//! they are handed — the whole log, or a store's candidates for one call (SPEC-2 v3 P2: SQL
//! narrows, Rust decides) — is the caller's business; what the rows then mean is decided here,
//! once. [`reference`] answers every question from a whole log, exactly as `answerFrom` does, and
//! is the oracle the engine's own paths are held to.

use super::{QsoRecord, UploadOutcome};
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

// ── JavaScript's string semantics ──────────────────────────────────────────────────────────────

/// Whether ECMAScript's `trim()` removes `c`: WhiteSpace (TAB, VT, FF, ZWNBSP U+FEFF and every
/// `Zs` space) or a LineTerminator (LF, CR, U+2028, U+2029). NOT U+0085, which `str::trim`
/// removes.
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'
            | '\u{A}'
            | '\u{B}'
            | '\u{C}'
            | '\u{D}'
            | '\u{20}'
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `s.trim()`, as JavaScript trims.
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// `s.toUpperCase()`: Unicode's full uppercase mapping, which Rust's `to_uppercase` is too.
pub fn js_upper(s: &str) -> String {
    if s.is_ascii() {
        s.to_ascii_uppercase()
    } else {
        s.to_uppercase()
    }
}

/// `s.toLowerCase()`: Unicode's full lowercase mapping, final sigma included, which Rust's
/// `to_lowercase` is too.
pub fn js_lower(s: &str) -> String {
    if s.is_ascii() {
        s.to_ascii_lowercase()
    } else {
        s.to_lowercase()
    }
}

/// The first UTF-16 code unit of `c`: the character itself below U+10000, its high surrogate
/// above.
fn first_unit(c: char) -> u32 {
    let c = c as u32;
    if c < 0x1_0000 {
        c
    } else {
        0xD800 + ((c - 0x1_0000) >> 10)
    }
}

/// How JavaScript's `<` orders two strings: by UTF-16 code units, lexicographically. It agrees
/// with Rust's byte order everywhere except between a character above U+FFFF (a surrogate pair,
/// whose first unit is 0xD800..0xDBFF) and one in U+E000..U+FFFF.
pub fn utf16_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars(), b.chars());
    loop {
        match (x.next(), y.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(p), Some(q)) if p == q => {}
            // Two different characters: their first units decide, unless both are pairs with the
            // same high surrogate — and then their code points order as their low surrogates do.
            (Some(p), Some(q)) => {
                return first_unit(p)
                    .cmp(&first_unit(q))
                    .then_with(|| (p as u32).cmp(&(q as u32)))
            }
        }
    }
}

/// Days since 1970-01-01 → (year, month, day), proleptic Gregorian — the civil calendar
/// JavaScript's `Date` uses (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The last instant a JavaScript `Date` can hold, in seconds: 8.64e15 ms.
const JS_DATE_MAX_SECS: u64 = 8_640_000_000_000;

/// `fmtUtc(whenUnix)` — `YYYY-MM-DD HH:MMZ` in UTC, as `Date` formats it: the year unpadded, and
/// `NaN` for every field of an instant past the range a `Date` holds.
pub fn fmt_utc(when_unix: u64) -> String {
    if when_unix > JS_DATE_MAX_SECS {
        return "NaN-NaN-NaN NaN:NaNZ".to_string();
    }
    let (days, secs) = (when_unix / 86_400, when_unix % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y}-{m:02}-{d:02} {:02}:{:02}Z",
        secs / 3_600,
        secs % 3_600 / 60
    )
}

// ── the UI's own keys ──────────────────────────────────────────────────────────────────────────

/// `modeKey(mode)`: a mode's slot identity — the sidebands fold to SSB and the two BPSK spellings
/// to PSK, and nothing else folds (`callHistory.ts`).
pub fn mode_key(mode: &str) -> String {
    let m = js_upper(js_trim(mode));
    match m.as_str() {
        "USB" | "LSB" => "SSB".to_string(),
        "BPSK31" => "PSK31".to_string(),
        "BPSK63" => "PSK63".to_string(),
        _ => m,
    }
}

/// The UI's band table (`ui/src/band.ts` `BAND_RANGES`), low to high: `(lo, hi, label)` in MHz.
const UI_BANDS: [(f64, f64, &str); 22] = [
    (1.8, 2.0, "160m"),
    (3.5, 4.0, "80m"),
    (5.3, 5.41, "60m"),
    (7.0, 7.3, "40m"),
    (10.1, 10.15, "30m"),
    (14.0, 14.35, "20m"),
    (18.068, 18.168, "17m"),
    (21.0, 21.45, "15m"),
    (24.89, 24.99, "12m"),
    (28.0, 29.7, "10m"),
    (50.0, 54.0, "6m"),
    (70.0, 71.0, "4m"),
    (144.0, 148.0, "2m"),
    (222.0, 225.0, "1.25m"),
    (420.0, 450.0, "70cm"),
    (902.0, 928.0, "33cm"),
    (1240.0, 1300.0, "23cm"),
    (2300.0, 2450.0, "13cm"),
    (3300.0, 3500.0, "9cm"),
    (5650.0, 5925.0, "6cm"),
    (10000.0, 10500.0, "3cm"),
    (24000.0, 24250.0, "1.25cm"),
];

/// `bandLabelForMhz(mhz)`: the first band whose edges hold `mhz`, or `""`.
fn ui_band_for_mhz(mhz: f64) -> &'static str {
    if !mhz.is_finite() {
        return "";
    }
    UI_BANDS
        .iter()
        .find(|(lo, hi, _)| mhz >= *lo && mhz <= *hi)
        .map_or("", |(_, _, label)| label)
}

/// `bandRangeForLabel(label) !== null`: whether `label` names a band of the UI's table.
fn ui_band_known(label: &str) -> bool {
    UI_BANDS.iter().any(|(_, _, l)| *l == label)
}

/// A frequency as the UI reads it: JSON carries no NaN or infinity (serde writes `null`), and
/// JavaScript reads that `null` as 0 wherever it compares it.
fn ui_mhz(mhz: f64) -> f64 {
    if mhz.is_finite() {
        mhz
    } else {
        0.0
    }
}

/// `bandKey(q)`: a row's band slot — the stored token when it names a real band, else the band
/// its own frequency is in, upper-cased; `None` when neither can be known (`callHistory.ts`).
pub fn band_key(band: &str, freq_mhz: f64) -> Option<String> {
    let token = js_trim(band);
    if !token.is_empty() && ui_band_known(&js_lower(token)) {
        return Some(js_upper(token));
    }
    let mhz = ui_mhz(freq_mhz);
    if mhz > 0.0 {
        let derived = ui_band_for_mhz(mhz);
        if !derived.is_empty() {
            return Some(js_upper(derived));
        }
    }
    None
}

/// `entityKey(q)`: the award identity a row is compared on — the resolved entity when there is
/// one, else the stored country — trimmed and upper-cased. `??` falls back on a missing entity
/// only, so a resolved `""` would stand as it is.
pub fn entity_key(entity: Option<&str>, country: Option<&str>) -> String {
    js_upper(js_trim(entity.or(country).unwrap_or("")))
}

// ── the Logbook's order and search ─────────────────────────────────────────────────────────────

/// A sortable Logbook column (`LogSortKey`). `band` sorts by frequency, as `freq` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    Call,
    Country,
    Band,
    Freq,
    Mode,
    Sent,
    Rcvd,
    Time,
    Park,
    Qsl,
}

/// What the Logbook list shows: the rows the search and the chip let through, in the order the
/// sorted column gives (`LogQuery`). The same four fields name an order everywhere.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogQuery {
    pub sort: SortKey,
    pub asc: bool,
    /// The search box as typed: trimmed and lower-cased when matched, never before.
    pub search: String,
    pub needs_confirm_only: bool,
}

impl LogQuery {
    /// Newest first, nothing filtered: the view the Logbook opens on (`DEFAULT_LOG_QUERY`) — the
    /// one order the store's `qso_recent` index gives directly.
    pub fn is_default(&self) -> bool {
        self.sort == SortKey::Time
            && !self.asc
            && self.search.is_empty()
            && !self.needs_confirm_only
    }
}

/// A row's value in a sorted column (`sortVal`).
#[derive(Debug, Clone, PartialEq)]
enum SortVal {
    Text(Box<str>),
    Num(f64),
}

fn sort_val(r: &QsoRecord, k: SortKey) -> SortVal {
    let text = |s: &str| SortVal::Text(js_upper(s).into_boxed_str());
    match k {
        SortKey::Call => text(&r.call),
        SortKey::Country => text(r.country.as_deref().unwrap_or("")),
        SortKey::Band | SortKey::Freq => SortVal::Num(ui_mhz(r.freq_mhz)),
        SortKey::Mode => text(&r.mode),
        SortKey::Sent => text(r.rst_sent.as_deref().unwrap_or("")),
        SortKey::Rcvd => text(r.rst_rcvd.as_deref().unwrap_or("")),
        SortKey::Time => SortVal::Num(r.when_unix as f64),
        SortKey::Park => text(
            r.ota
                .their_ref
                .as_deref()
                .or(r.ota.my_ref.as_deref())
                .unwrap_or(""),
        ),
        SortKey::Qsl => SortVal::Num(if r.award_confirmed {
            2.0
        } else if r.confirmed {
            1.0
        } else {
            0.0
        }),
    }
}

/// `av < bv ? -1 : av > bv ? 1 : 0` — JavaScript's `<` and `>`, which call two values neither
/// less nor greater a tie.
fn sort_val_cmp(a: &SortVal, b: &SortVal) -> Ordering {
    match (a, b) {
        (SortVal::Text(a), SortVal::Text(b)) => utf16_cmp(a, b),
        (SortVal::Num(a), SortVal::Num(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
        // One column gives one kind of value.
        _ => Ordering::Equal,
    }
}

/// Whether the Logbook shows `r` under `query` (`matchesLogQuery`, verbatim): not an
/// award-confirmed contact under the chip, and the search in any of seven fields — the formatted
/// time among them, whose trailing `Z` is why a search for "z" finds every row.
pub fn matches(r: &QsoRecord, query: &LogQuery) -> bool {
    shown(
        r,
        query.needs_confirm_only,
        &js_lower(js_trim(&query.search)),
    )
}

/// [`matches`], with the search already trimmed and lower-cased (`t`).
fn shown(r: &QsoRecord, needs_confirm_only: bool, t: &str) -> bool {
    if needs_confirm_only && r.award_confirmed {
        return false;
    }
    if t.is_empty() {
        return true;
    }
    let has = |s: &str| js_lower(s).contains(t);
    has(&r.call)
        || r.country.as_deref().is_some_and(has)
        || r.grid.as_deref().is_some_and(has)
        || has(&r.band)
        || has(&r.mode)
        || has(&mode_key(&r.mode))
        || has(&fmt_utc(r.when_unix))
}

/// The rows `query` shows, as their handles in display order — the order vector (`logOrder`,
/// SPEC-2 v3 §4.3). `rows` are `(handle, row)` in LOG ORDER; a row the query does not show is
/// left out. See [`OrderBuilder`], which this is.
pub fn order<H, R>(rows: impl IntoIterator<Item = (H, R)>, query: &LogQuery) -> Vec<H>
where
    R: Borrow<QsoRecord>,
{
    let mut b = OrderBuilder::new(query);
    for (h, r) in rows {
        b.push(h, r.borrow());
    }
    b.finish()
}

/// An order vector built as the rows go by, so a pass over a lifetime log keeps only what the
/// order needs of each row it shows — its handle, its value in the sorted column, its time —
/// and never the rows. Filter, then a stable sort on the column with the time as the
/// tie-break, the whole comparison negated for a descending order — so a full tie keeps log
/// order either way (`logOrder`).
pub struct OrderBuilder<H> {
    sort: SortKey,
    asc: bool,
    needs_confirm_only: bool,
    /// The search, trimmed and lower-cased once.
    t: String,
    kept: Vec<(H, SortVal, u64)>,
}

impl<H> OrderBuilder<H> {
    pub fn new(query: &LogQuery) -> Self {
        Self {
            sort: query.sort,
            asc: query.asc,
            needs_confirm_only: query.needs_confirm_only,
            t: js_lower(js_trim(&query.search)),
            kept: Vec::new(),
        }
    }

    /// The next row, in LOG ORDER after every row pushed before it.
    pub fn push(&mut self, h: H, r: &QsoRecord) {
        if shown(r, self.needs_confirm_only, &self.t) {
            self.kept.push((h, sort_val(r, self.sort), r.when_unix));
        }
    }

    /// The handles of the rows shown, in display order.
    pub fn finish(mut self) -> Vec<H> {
        let asc = self.asc;
        self.kept.sort_by(|a, b| {
            let cmp = sort_val_cmp(&a.1, &b.1).then(a.2.cmp(&b.2));
            if asc {
                cmp
            } else {
                cmp.reverse()
            }
        });
        self.kept.into_iter().map(|(h, _, _)| h).collect()
    }
}

// ── one call's history, a roster's summary, the band map ───────────────────────────────────────

/// Prior contacts with one call (`CallHistory`). `qsos` are positions in the rows the history was
/// computed from, in log order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallHistory {
    pub qsos: Vec<usize>,
    pub count: usize,
    pub worked_before: bool,
    pub dupe_this_band: bool,
    /// `None` when never worked.
    pub last_unix: Option<u64>,
    pub confirmed_count: usize,
    /// Distinct bands worked, as logged, first-seen order.
    pub bands: Vec<String>,
    /// Distinct modes worked, as logged, first-seen order.
    pub modes: Vec<String>,
}

/// The call a history is of: trimmed and upper-cased as the UI compares calls. A row belongs to
/// it when its own call, trimmed and upper-cased, is the same string.
pub fn history_call(call: &str) -> String {
    js_upper(js_trim(call))
}

/// `callHistory(log, call, band, mode, matchMode)` over `rows` in log order. `band` is the
/// operating band for the dupe cue (`""` skips it); `match_mode` also asks the mode to match,
/// through the mode fold on both sides.
pub fn call_history<R: Borrow<QsoRecord>>(
    rows: &[R],
    call: &str,
    band: &str,
    mode: &str,
    match_mode: bool,
) -> CallHistory {
    let c = history_call(call);
    if c.is_empty() {
        return CallHistory::default();
    }
    let qsos: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| history_call(&(*r).borrow().call) == c)
        .map(|(i, _)| i)
        .collect();
    if qsos.is_empty() {
        return CallHistory::default();
    }
    let (want_band, want_mode) = (js_lower(js_trim(band)), mode_key(mode));
    let mut h = CallHistory {
        count: qsos.len(),
        worked_before: true,
        last_unix: Some(0),
        ..CallHistory::default()
    };
    for &i in &qsos {
        let q = rows[i].borrow();
        if !q.band.is_empty() && !h.bands.contains(&q.band) {
            h.bands.push(q.band.clone());
        }
        if !q.mode.is_empty() && !h.modes.contains(&q.mode) {
            h.modes.push(q.mode.clone());
        }
        if Some(q.when_unix) > h.last_unix {
            h.last_unix = Some(q.when_unix);
        }
        if q.confirmed {
            h.confirmed_count += 1;
        }
        if !band.is_empty()
            && js_lower(js_trim(&q.band)) == want_band
            && (!match_mode || mode_key(&q.mode) == want_mode)
        {
            h.dupe_this_band = true;
        }
    }
    h.qsos = qsos;
    h
}

/// What the JS8 roster shows for one worked call (`CallSummary`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallSummary {
    pub count: usize,
    pub last_unix: Option<u64>,
    /// The most recent contact's grid, name and comment, trimmed.
    pub grid: String,
    pub name: String,
    pub comment: String,
}

/// `callsSummary(log, calls)` over `rows` in log order: for each of `calls` worked at least once
/// — any band, any mode — its count, last time, and the most recent contact's grid, name and
/// comment ("most recent" is the FIRST row with the greatest time). Calls never worked are
/// absent; a call asked twice is answered once, where it was first asked.
pub fn calls_summary<R: Borrow<QsoRecord>>(
    rows: &[R],
    calls: &[String],
) -> Vec<(String, CallSummary)> {
    let mut out: Vec<(String, CallSummary)> = Vec::new();
    for call in calls {
        let h = call_history(rows, call, "", "", false);
        if !h.worked_before {
            continue;
        }
        let last = h
            .qsos
            .iter()
            .map(|&i| rows[i].borrow())
            .reduce(|a, b| if b.when_unix > a.when_unix { b } else { a })
            .expect("a worked call has a contact");
        let trimmed = |s: &Option<String>| js_trim(s.as_deref().unwrap_or("")).to_string();
        let summary = CallSummary {
            count: h.count,
            last_unix: h.last_unix,
            grid: trimmed(&last.grid),
            name: trimmed(&last.name),
            comment: trimmed(&last.comment),
        };
        match out.iter_mut().find(|(c, _)| c == call) {
            Some((_, s)) => *s = summary,
            None => out.push((call.clone(), summary)),
        }
    }
    out
}

/// The band map's key for a call: `call.toUpperCase()`, UNTRIMMED — a trailing space is a
/// different call.
pub fn worked_key(call: &str) -> String {
    js_upper(call)
}

// ── an entity's slots ──────────────────────────────────────────────────────────────────────────

/// An entity's worked slots (`EntitySlots`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitySlots {
    pub worked_ever: bool,
    /// Band slots ([`band_key`]), first-seen order.
    pub bands_worked: Vec<String>,
    /// Mode slots ([`mode_key`]), first-seen order.
    pub modes_worked: Vec<String>,
    /// A contact of this entity has a band that cannot be placed at all.
    pub band_unknown: bool,
}

/// Whether an entity is new, and its slots (`EntityAnswer`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityAnswer {
    pub new_entity: bool,
    pub slots: EntitySlots,
}

/// Every entity's slots, built row by row in log order — the engine's answer to the entity
/// question, kept and grown as the log grows (an appended row only ever adds to its entity's
/// first-seen lists, where [`reference`] recomputes them).
#[derive(Debug, Clone, Default)]
pub struct EntityIndex {
    slots: HashMap<String, EntitySlots>,
}

impl EntityIndex {
    /// Count one row: `entity` is its resolved entity (`None` when its call does not resolve), in
    /// log order after every row counted so far.
    pub fn add(&mut self, entity: Option<&str>, r: &QsoRecord) {
        let key = entity_key(entity, r.country.as_deref());
        if key.is_empty() {
            return;
        }
        let s = self.slots.entry(key).or_default();
        s.worked_ever = true;
        match band_key(&r.band, r.freq_mhz) {
            Some(b) => {
                if !s.bands_worked.contains(&b) {
                    s.bands_worked.push(b);
                }
            }
            None => {
                if !js_trim(&r.band).is_empty() || ui_mhz(r.freq_mhz) > 0.0 {
                    s.band_unknown = true;
                }
            }
        }
        let m = mode_key(&r.mode);
        if !m.is_empty() && !s.modes_worked.contains(&m) {
            s.modes_worked.push(m);
        }
    }

    /// `{ newEntity: isNewEntity(log, entity), slots: entitySlots(log, entity) }`.
    pub fn answer(&self, entity: &str) -> EntityAnswer {
        let c = js_upper(js_trim(entity));
        if c.is_empty() {
            return EntityAnswer::default();
        }
        match self.slots.get(&c) {
            Some(s) => EntityAnswer {
                new_entity: false,
                slots: s.clone(),
            },
            None => EntityAnswer {
                new_entity: true,
                slots: EntitySlots::default(),
            },
        }
    }
}

// ── the folds: squares, the globe's dots and bands, the LoTW backlog, the statistics ──────────

/// A question answered by one pass over the whole log in log order: the squares worked, the
/// Logbook globe's dots and its bands, the LoTW backlog, the statistics' counts. The engine runs
/// it over the store's rows, narrow; [`reference::fold`] over a whole log.
pub trait LogFold {
    type Answer;
    /// The next row, in LOG ORDER after every row added before it.
    fn add(&mut self, r: &QsoRecord);
    fn finish(self) -> Self::Answer;
}

/// `s.length`: UTF-16 code units.
fn js_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// `s.slice(0, n)`: the first `n` UTF-16 code units of `s`. A character above U+FFFF that the cut
/// splits leaves JavaScript half of it — a lone surrogate, which no Rust string can hold — so it
/// stands here as U+FFFD. Only a grid with such a character in its fourth place meets this, and
/// neither view can place that square on a map.
fn js_slice(s: &str, n: usize) -> String {
    let mut out = String::new();
    let mut units = 0;
    for c in s.chars() {
        let w = c.len_utf16();
        if units + w > n {
            if units < n {
                out.push('\u{FFFD}');
            }
            break;
        }
        out.push(c);
        units += w;
    }
    out
}

/// The 4-character square a contact's grid names, as the maps and the Logbook's globe key it
/// (`workedGridSet`, `qsoGridCounts`): trimmed and upper-cased, cut to four UTF-16 units — `None`
/// when shorter. Nothing checks that it IS a square; neither view did.
pub fn grid_square(grid: Option<&str>) -> Option<String> {
    let g = js_upper(js_trim(grid.unwrap_or("")));
    (js_len(&g) >= 4).then(|| js_slice(&g, 4))
}

/// `workedGridSet(log)`: every square worked, first-seen order (`workedGrids`).
#[derive(Debug, Default)]
pub struct WorkedGrids {
    seen: HashSet<String>,
    squares: Vec<String>,
}

impl LogFold for WorkedGrids {
    type Answer = Vec<String>;

    fn add(&mut self, r: &QsoRecord) {
        if let Some(g) = grid_square(r.grid.as_deref()) {
            if self.seen.insert(g.clone()) {
                self.squares.push(g);
            }
        }
    }

    fn finish(self) -> Vec<String> {
        self.squares
    }
}

/// One worked square on the Logbook's globe (`QsoGridCount`): its contacts, and the band of its
/// most recent one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridCount {
    pub grid: String,
    pub n: usize,
    pub band: String,
}

/// `qsoGridCounts(log, band)` (`gridPoints`): the squares worked on `band` — compared exactly as
/// logged; `"all"` pools every band — first-seen order. A square's band is its most recent
/// contact's: the LAST of equal times.
#[derive(Debug)]
pub struct GridCounts {
    band: String,
    at: HashMap<String, usize>,
    squares: Vec<(GridCount, u64)>,
}

impl GridCounts {
    pub fn new(band: &str) -> Self {
        Self {
            band: band.to_string(),
            at: HashMap::new(),
            squares: Vec::new(),
        }
    }
}

impl LogFold for GridCounts {
    type Answer = Vec<GridCount>;

    fn add(&mut self, r: &QsoRecord) {
        if self.band != "all" && r.band != self.band {
            return;
        }
        let Some(grid) = grid_square(r.grid.as_deref()) else {
            return;
        };
        match self.at.get(&grid) {
            Some(&i) => {
                let (square, when) = &mut self.squares[i];
                square.n += 1;
                if r.when_unix >= *when {
                    *when = r.when_unix;
                    square.band = r.band.clone();
                }
            }
            None => {
                self.at.insert(grid.clone(), self.squares.len());
                let square = GridCount {
                    grid,
                    n: 1,
                    band: r.band.clone(),
                };
                self.squares.push((square, r.when_unix));
            }
        }
    }

    fn finish(self) -> Vec<GridCount> {
        self.squares.into_iter().map(|(square, _)| square).collect()
    }
}

/// The bands in the log, exactly as logged, first-seen order (`bandsInLog`): every one that is
/// not the empty string.
#[derive(Debug, Default)]
pub struct BandsInLog {
    seen: HashSet<String>,
    bands: Vec<String>,
}

impl LogFold for BandsInLog {
    type Answer = Vec<String>;

    fn add(&mut self, r: &QsoRecord) {
        if !r.band.is_empty() && self.seen.insert(r.band.clone()) {
            self.bands.push(r.band.clone());
        }
    }

    fn finish(self) -> Vec<String> {
        self.bands
    }
}

/// The Logbook's LoTW backlog (`lotwBacklog`): the contacts its "Upload to LoTW" button counts —
/// not award-confirmed, and never sent to LoTW or bounced by it — by whether the time of day is
/// known (`unsent`) or not (`timeless`: LoTW matches on time, so it can never confirm one).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LotwBacklog {
    pub unsent: usize,
    pub timeless: usize,
}

impl LogFold for LotwBacklog {
    type Answer = LotwBacklog;

    fn add(&mut self, r: &QsoRecord) {
        let eligible = !r.award_confirmed
            && r.upload.lotw.as_ref().is_none_or(|s| {
                matches!(s.outcome, UploadOutcome::Rejected | UploadOutcome::AuthFail)
            });
        if eligible && r.time_known {
            self.unsent += 1;
        } else if eligible {
            self.timeless += 1;
        }
    }

    fn finish(self) -> LotwBacklog {
        self
    }
}

/// A labelled count (`Tally`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelCount {
    pub label: String,
    pub count: usize,
}

/// How many contacts carry each confirmation channel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QslCounts {
    pub card: usize,
    pub lotw: usize,
    pub eqsl: usize,
}

/// The Statistics dashboard COUNTED and not yet ordered (`LogStatCounts`): each tally in
/// first-seen order, every entity. The window orders them (`finishLogStats`): its ties are broken
/// by `localeCompare`, in the webview's own locale, and the top twelve entities are cut after that.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogStatCounts {
    pub total: usize,
    pub unique_calls: usize,
    pub confirmed: usize,
    pub award_confirmed: usize,
    pub dxcc_entities: usize,
    pub by_band: Vec<LabelCount>,
    pub by_mode: Vec<LabelCount>,
    pub by_year: Vec<LabelCount>,
    pub by_state: Vec<LabelCount>,
    pub entities: Vec<LabelCount>,
    pub hour_utc: [usize; 24],
    pub hour_unknown: usize,
    pub qsl: QslCounts,
}

/// Labels counted in first-seen order, each under the label as first seen — by the label itself
/// (`tallyBy`), or, where `fold` is set, by its upper case (`tallyByCI`).
#[derive(Debug, Default)]
struct Tallies {
    fold: bool,
    at: HashMap<String, usize>,
    counts: Vec<LabelCount>,
}

impl Tallies {
    fn by_upper_case() -> Self {
        Self {
            fold: true,
            ..Self::default()
        }
    }

    /// Count `label`, trimmed; a blank one is not counted.
    fn add(&mut self, label: &str) {
        let label = js_trim(label);
        if label.is_empty() {
            return;
        }
        let key = if self.fold {
            js_upper(label)
        } else {
            label.to_string()
        };
        match self.at.get(&key) {
            Some(&i) => self.counts[i].count += 1,
            None => {
                self.at.insert(key, self.counts.len());
                self.counts.push(LabelCount {
                    label: label.to_string(),
                    count: 1,
                });
            }
        }
    }
}

/// The US-family entities WAS counts states for (`US_ENTITIES`).
const US_ENTITIES: [&str; 3] = ["UNITED STATES", "ALASKA", "HAWAII"];

/// The fifty WAS codes (`WAS_STATES`).
const WAS_STATES: [&str; 50] = [
    "AK", "AL", "AR", "AZ", "CA", "CO", "CT", "DE", "FL", "GA", "HI", "IA", "ID", "IL", "IN", "KS",
    "KY", "LA", "MA", "MD", "ME", "MI", "MN", "MO", "MS", "MT", "NC", "ND", "NE", "NH", "NJ", "NM",
    "NV", "NY", "OH", "OK", "OR", "PA", "RI", "SC", "SD", "TN", "TX", "UT", "VA", "VT", "WA", "WI",
    "WV", "WY",
];

/// `wasState(q)`: a contact's state as a WAS code, only for a US-family STORED country.
fn was_state(r: &QsoRecord) -> Option<String> {
    let country = js_upper(js_trim(r.country.as_deref().unwrap_or("")));
    if !US_ENTITIES.contains(&country.as_str()) {
        return None;
    }
    let code = js_upper(js_trim(r.state.as_deref()?));
    WAS_STATES.contains(&code.as_str()).then_some(code)
}

/// `phoneModeLabel(mode)`: the sidebands fold into SSB, and nothing else folds — case included.
fn phone_mode_label(mode: &str) -> String {
    let m = js_trim(mode);
    match js_upper(m).as_str() {
        "USB" | "LSB" => "SSB".to_string(),
        _ => m.to_string(),
    }
}

/// `countLogStats(log)`, a row at a time (`statistics`): the dashboard's counts, with each
/// contact's entity its call's live one (`resolve`, once per distinct call in the pass) and the
/// stored country where it has none.
pub struct LogStatCounter<'a> {
    resolve: &'a dyn Fn(&str) -> Option<String>,
    entity_of: HashMap<String, Option<String>>,
    counts: LogStatCounts,
    calls: HashSet<String>,
    countries: HashSet<String>,
    bands: Tallies,
    modes: Tallies,
    years: Tallies,
    states: Tallies,
    entities: Tallies,
}

impl<'a> LogStatCounter<'a> {
    pub fn new(resolve: &'a dyn Fn(&str) -> Option<String>) -> Self {
        Self {
            resolve,
            entity_of: HashMap::new(),
            counts: LogStatCounts::default(),
            calls: HashSet::new(),
            countries: HashSet::new(),
            bands: Tallies::default(),
            modes: Tallies::default(),
            years: Tallies::default(),
            states: Tallies::default(),
            entities: Tallies::by_upper_case(),
        }
    }
}

impl LogFold for LogStatCounter<'_> {
    type Answer = LogStatCounts;

    fn add(&mut self, r: &QsoRecord) {
        let resolve = self.resolve;
        let entity = self
            .entity_of
            .entry(r.call.clone())
            .or_insert_with(|| resolve(&r.call));
        let c = &mut self.counts;
        c.total += 1;
        self.calls.insert(js_upper(js_trim(&r.call)));
        // `q.entity ?? q.country`: a missing entity only.
        let named = js_trim(entity.as_deref().or(r.country.as_deref()).unwrap_or(""));
        if !named.is_empty() {
            self.countries.insert(js_upper(named));
        }
        self.entities.add(named);
        c.confirmed += usize::from(r.confirmed);
        c.award_confirmed += usize::from(r.award_confirmed);
        c.qsl.card += usize::from(r.qsl_rcvd.card);
        c.qsl.lotw += usize::from(r.qsl_rcvd.lotw);
        c.qsl.eqsl += usize::from(r.qsl_rcvd.eqsl);
        // A time with no time of day is a date-only import's, judged on the number JavaScript
        // holds; a time past the range a `Date` holds has no hour and no year.
        if (r.when_unix as f64) % 86_400.0 == 0.0 {
            c.hour_unknown += 1;
        } else if r.when_unix <= JS_DATE_MAX_SECS {
            c.hour_utc[(r.when_unix % 86_400 / 3_600) as usize] += 1;
        }
        if r.when_unix <= JS_DATE_MAX_SECS {
            let (year, _, _) = civil_from_days((r.when_unix / 86_400) as i64);
            self.years.add(&year.to_string());
        }
        self.bands.add(&r.band);
        self.modes.add(&phone_mode_label(&r.mode));
        if let Some(state) = was_state(r) {
            self.states.add(&state);
        }
    }

    fn finish(self) -> LogStatCounts {
        LogStatCounts {
            unique_calls: self.calls.len(),
            dxcc_entities: self.countries.len(),
            by_band: self.bands.counts,
            by_mode: self.modes.counts,
            by_year: self.years.counts,
            by_state: self.states.counts,
            entities: self.entities.counts,
            ..self.counts
        }
    }
}

/// The oracle: every question answered from a whole log in log order, exactly as
/// `logAnswers.ts`'s `answerFrom` computes it — no index, no cache, no store. The engine's own
/// paths are held to it; it is held to the goldens.
pub mod reference {
    use super::*;

    /// `callHistory` over the whole log.
    pub fn call_history<R: Borrow<QsoRecord>>(
        log: &[R],
        call: &str,
        band: &str,
        mode: &str,
        match_mode: bool,
    ) -> super::CallHistory {
        super::call_history(log, call, band, mode, match_mode)
    }

    /// `{ newEntity: isNewEntity(log, entity), slots: entitySlots(log, entity) }`, each row's
    /// entity from `entity_of` — the two functions ported line by line, not through
    /// [`EntityIndex`].
    pub fn entity<R: Borrow<QsoRecord>>(
        log: &[R],
        entity: &str,
        entity_of: &dyn Fn(&QsoRecord) -> Option<String>,
    ) -> EntityAnswer {
        let c = js_upper(js_trim(entity));
        if c.is_empty() {
            return EntityAnswer::default();
        }
        let key = |q: &QsoRecord| entity_key(entity_of(q).as_deref(), q.country.as_deref());
        let new_entity = !log.iter().any(|q| key(q.borrow()) == c);
        let mut slots = EntitySlots::default();
        for q in log {
            let q = q.borrow();
            if key(q) != c {
                continue;
            }
            slots.worked_ever = true;
            let b = band_key(&q.band, q.freq_mhz);
            let m = mode_key(&q.mode);
            match b {
                None => {
                    if !js_trim(&q.band).is_empty() || ui_mhz(q.freq_mhz) > 0.0 {
                        slots.band_unknown = true;
                    }
                }
                Some(b) if !slots.bands_worked.contains(&b) => slots.bands_worked.push(b),
                Some(_) => {}
            }
            if !m.is_empty() && !slots.modes_worked.contains(&m) {
                slots.modes_worked.push(m);
            }
        }
        EntityAnswer { new_entity, slots }
    }

    /// `workedCalls`: the asked calls some row's call upper-cases to, untrimmed, in the order
    /// asked.
    pub fn worked_calls<R: Borrow<QsoRecord>>(log: &[R], calls: &[String]) -> Vec<String> {
        let worked: std::collections::HashSet<String> =
            log.iter().map(|r| worked_key(&r.borrow().call)).collect();
        calls
            .iter()
            .filter(|c| worked.contains(&worked_key(c)))
            .cloned()
            .collect()
    }

    /// The order vector of the whole log, as log positions.
    pub fn order<R: Borrow<QsoRecord>>(log: &[R], query: &LogQuery) -> Vec<usize> {
        super::order(log.iter().map(|r| r.borrow()).enumerate(), query)
    }

    /// A fold ([`LogFold`]) over the whole log.
    pub fn fold<R: Borrow<QsoRecord>, F: LogFold>(log: &[R], mut fold: F) -> F::Answer {
        for r in log {
            fold.add(r.borrow());
        }
        fold.finish()
    }
}

#[cfg(test)]
mod tests;
