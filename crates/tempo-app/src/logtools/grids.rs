//! Blank grids: contacts logged with no grid, filled from a grid Nexus knows for the station, only
//! those the operator ticks (operator decision, 2026-10-27).
//!
//! **Where a grid comes from, first found wins:**
//!
//! 1. **Heard on the air** ([`GridFrom::Heard`]): a grid the station itself SENT — its CQ, or a
//!    call to anyone with its grid — as Nexus decoded it, in the decode log Nexus keeps (ALL.TXT,
//!    [`heard_grids_in`]) or this session (`Roster::sent_grid_heard`), and **only within 24 hours
//!    of the contact** ([`heard_at`], `tempo_core::roster::SENT_GRID_MAX_AGE_SECS`; operator
//!    decision, 2026-10-27): the station's own word about where it was then, so it counts as any
//!    logged grid. A grid heard further from the contact says where a rover, a portable station or
//!    one that has moved was on another day, and is never offered as heard.
//! 2. **Home QTH, from the callbook** ([`GridFrom::Callbook`]): the grid a callbook lookup
//!    already made answered for exactly this call (the lookup cache; nothing is looked up here),
//!    only for a call with no stroke and no park, and only on a contact from the last 12 months —
//!    the log's own callbook rule ([`callbook::fill_from_callbook`], [`super::LOCATION_FIELDS`]).
//!    It is filled through that rule with the map-only mark
//!    ([`callbook::fill_gridless_from_callbook`]): for the distance and the map only, counted
//!    toward no grid award, filling no GRID need, sent to no service
//!    (`tempo_core::logbook::counted_grid`).
//!
//! **What it changes:** the grid of a contact that still has none, and the distance to it (ADIF
//! `DISTANCE`, where the contact has none and its own grid is known), as the log funnel fills it
//! when a contact is logged (`crate::engine::distance_km`). Never a call,
//! a stamp or a confirmation: a contact already uploaded keeps its upload marks, as with every fix
//! here (the module header), so nothing is sent again and each service keeps what it was sent.
//! ONE run of the history, undone whole; a contact given a grid since it was offered is left out.

use super::*;
use std::io::BufRead;
use tempo_core::message::Msg;

/// What the stations sent, by call: each run of one grid as `(first heard, last heard, grid)`, in
/// the order heard ([`heard_grids_in`]).
pub type HeardGrids = HashMap<String, Vec<(u64, u64, String)>>;

/// The grid each station last sent this session, by call, with the UTC second it was heard
/// (`Engine::sent_grids_heard`).
pub type SessionGrids = HashMap<String, (u64, String)>;

/// Where an offered grid came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridFrom {
    /// The station sent it on the air, and Nexus heard it.
    Heard,
    /// The station's home QTH, from a callbook lookup already made.
    Callbook,
}

/// One contact logged with no grid, and the grid Nexus knows for its station.
#[derive(Debug, Clone, PartialEq)]
pub struct GridOffer {
    pub id: RecordId,
    pub call: String,
    pub when_unix: u64,
    pub band: String,
    pub mode: String,
    pub grid: String,
    pub source: GridFrom,
    /// A service already has the contact: it keeps what it was sent.
    pub uploaded: bool,
}

/// What the offer reads of every contact: enough to choose the blank ones and say what they are,
/// and the park the callbook rule asks after. The upload stamps say which a service already has.
const GRIDLESS: Narrow = Narrow {
    columns: &[
        "call",
        "band",
        "mode",
        "when_unix",
        "grid",
        "ota_their_program",
        "ota_their_ref",
    ],
    uploads: true,
};

/// Whether a service already has the contact ([`tempo_core::logbook::UploadOutcome::is_sent`]).
fn uploaded(r: &QsoRecord) -> bool {
    let u = &r.upload;
    [&u.lotw, &u.eqsl, &u.qrz, &u.clublog]
        .into_iter()
        .any(|s| s.as_ref().is_some_and(|s| s.outcome.is_sent()))
}

/// The grid a callbook answer gives `r`, by the log's own rule, or none: the answer's grid filled
/// into a copy of `r` exactly as a logged contact takes it, and only where that marks it as the
/// callbook's (the station's own record, no park). A stroked call takes none here, whatever its
/// answer (operator decision, 2026-10-27).
fn callbook_grid(r: &QsoRecord, answer: &CallbookAnswer, now_unix: u64) -> Option<String> {
    if r.call.contains('/') || !location_current(r, now_unix) {
        return None;
    }
    let mut copy = QsoRecord::clone(r);
    copy.grid = None;
    callbook::fill_from_callbook(
        &mut copy,
        &CallbookAnswer {
            call: answer.call.clone(),
            grid: answer.grid.clone(),
            ..CallbookAnswer::default()
        },
    );
    if !callbook::is_callbook_filled(&copy, "GRIDSQUARE") {
        return None;
    }
    copy.grid
}

/// ★ Every contact logged with no grid for which a grid is known. `heard(calls)` answers what the
/// decode log holds for those calls ([`heard_grids_in`]) and `session(calls)` the grid each sent
/// this session, with when it was heard; of both, the grid in effect at the contact's time is
/// taken, if heard within 24 hours of it ([`heard_at`]); else `kept(call key)`, the callbook answer
/// a lookup already made. Each is asked once. Read with the Engine lock released; changes nothing.
/// How many contacts have no grid, and the offers.
pub fn grid_offers(
    engine: &Mutex<Engine>,
    heard: &mut dyn FnMut(&HashSet<String>) -> HeardGrids,
    session: &mut dyn FnMut(&HashSet<String>) -> SessionGrids,
    kept: &mut dyn FnMut(&str) -> Option<CallbookAnswer>,
    now_unix: u64,
) -> Result<(usize, Vec<GridOffer>), String> {
    let mut blank = Vec::new();
    each_row(engine, GRIDLESS, &mut |r| {
        if r.id.is_some()
            && !r.call.trim().is_empty()
            && r.grid.as_deref().is_none_or(|g| g.trim().is_empty())
        {
            blank.push(r.clone());
        }
    })?;
    let calls: HashSet<String> = blank
        .iter()
        .map(|r| r.call.trim().to_ascii_uppercase())
        .collect();
    let mut decoded = if calls.is_empty() {
        HashMap::new()
    } else {
        heard(&calls)
    };
    if !calls.is_empty() {
        for (call, (at, grid)) in session(&calls) {
            let runs = decoded.entry(call).or_default();
            runs.push((at, at, grid));
            runs.sort_by_key(|(first, _, _)| *first);
        }
    }
    let mut answers: HashMap<String, Option<CallbookAnswer>> = HashMap::new();
    let mut offers = Vec::new();
    for r in &blank {
        let call = r.call.trim().to_ascii_uppercase();
        let found = decoded
            .get(&call)
            .and_then(|sent| heard_at(sent, r.when_unix))
            .map(str::to_string)
            .filter(|g| tempo_core::message::is_valid_grid(g.trim()))
            .map(|g| (g, GridFrom::Heard))
            .or_else(|| {
                let key = cleanup::call_key(&r.call);
                let answer = answers.entry(key.clone()).or_insert_with(|| kept(&key));
                answer
                    .as_ref()
                    .and_then(|a| callbook_grid(r, a, now_unix))
                    .map(|g| (g, GridFrom::Callbook))
            });
        if let (Some(id), Some((grid, source))) = (r.id, found) {
            offers.push(GridOffer {
                id,
                call: r.call.clone(),
                when_unix: r.when_unix,
                band: r.band.clone(),
                mode: r.mode.clone(),
                grid,
                source,
                uploaded: uploaded(r),
            });
        }
    }
    Ok((blank.len(), offers))
}

/// One contact the operator ticked: its grid, and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct GridPick {
    pub id: RecordId,
    pub grid: String,
    pub source: GridFrom,
}

/// ★ Fill the ticked contacts' grids, and no other, as ONE run of the history: a heard grid as the
/// contact's own, a callbook grid through the log's callbook rule, with its mark. A contact given a
/// grid since it was offered is left out (`changed`), and so is a contact gone since; a callbook
/// grid the rule would not give the contact as it now stands (a park added since) is left out too.
/// A grid that is no Maidenhead locator is refused whole, changing nothing. Never a call, a stamp
/// or a confirmation; nothing is sent anywhere.
pub fn fill_grids(
    engine: &Mutex<Engine>,
    picks: &[GridPick],
) -> (Result<ToolRun, String>, Durability) {
    if let Some(bad) = picks
        .iter()
        .find(|p| !tempo_core::message::is_valid_grid(p.grid.trim()))
    {
        return (
            Err(format!("'{}' is not a grid locator.", bad.grid)),
            Durability::default(),
        );
    }
    let mut seen = HashSet::new();
    let picks: Vec<&GridPick> = picks.iter().filter(|p| seen.insert(p.id)).collect();
    let by_id: HashMap<RecordId, &GridPick> = picks.iter().map(|p| (p.id, *p)).collect();
    let ids: Vec<RecordId> = picks.iter().map(|p| p.id).collect();
    let parts: Vec<Vec<RecordId>> = ids.chunks(CHUNK).map(<[RecordId]>::to_vec).collect();
    run_in_parts(engine, &parts, "log_grid_fill", |k, rows| {
        let mut pairs = Vec::new();
        let mut refused = Vec::new();
        for id in &parts[k] {
            let Some(row) = rows.get(id) else {
                refused.push((*id, "gone"));
                continue;
            };
            if row.grid.as_deref().is_some_and(|g| !g.trim().is_empty()) {
                refused.push((*id, "changed"));
                continue;
            }
            let pick = by_id[id];
            let grid = pick.grid.trim().to_string();
            let mut after = QsoRecord::clone(row);
            match pick.source {
                GridFrom::Heard => after.grid = Some(grid),
                GridFrom::Callbook => {
                    if row.call.contains('/') {
                        refused.push((*id, "changed"));
                        continue;
                    }
                    // For the distance and the map only, as an FT8/FT4 contact logged with no
                    // grid takes it (`fill_gridless_from_callbook`).
                    callbook::fill_gridless_from_callbook(
                        &mut after,
                        &CallbookAnswer {
                            call: row.call.clone(),
                            grid: Some(grid),
                            ..CallbookAnswer::default()
                        },
                    );
                    if !callbook::is_callbook_filled(&after, "GRIDSQUARE") {
                        refused.push((*id, "changed"));
                        continue;
                    }
                }
            }
            if let Some(km) = crate::engine::distance_km(&after) {
                tempo_core::logbook::fill_extra(&mut after, "DISTANCE", km.to_string());
            }
            pairs.push((Arc::clone(row), Some(Arc::new(after))));
        }
        (pairs, refused)
    })
}

// ── the grids Nexus heard ────────────────────────────────────────────────────────────────────────

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// An ALL.TXT line's `yyMMdd_hhmmss`, as Unix seconds (years 2000–2099, as WSJT-X writes them).
fn all_txt_time(stamp: &str) -> Option<u64> {
    let b = stamp.as_bytes();
    if b.len() != 13
        || b[6] != b'_'
        || !stamp[..6]
            .bytes()
            .chain(stamp[7..].bytes())
            .all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let n = |r: std::ops::Range<usize>| stamp[r].parse::<i64>().ok();
    let (y, mo, d) = (2000 + n(0..2)?, n(2..4)?, n(4..6)?);
    let (h, mi, s) = (n(7..9)?, n(9..11)?, n(11..13)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    u64::try_from(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s).ok()
}

/// A column of numbers on an ALL.TXT line: the report, DT or audio offset, or two of them run
/// together.
fn is_number(word: &str) -> bool {
    !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_digit() || b"+-.".contains(&b))
}

/// Where the message starts on an ALL.TXT line split into words: after the stamp, the dial, Rx/Tx
/// and the mode, past the report, DT and audio offset. Those are fixed-width fields with no space
/// between them, so a wide value runs two together (a DT of -10.5 after a report of -12 is
/// `-12-10.5`): the numbers are skipped as they come, up to three, never counted on as three.
fn message_start(words: &[&str]) -> usize {
    4 + words
        .iter()
        .skip(4)
        .take(3)
        .take_while(|w| is_number(w))
        .count()
}

/// Whether a decode's words name one of `wanted` (calls as decoded, upper-cased): asked before
/// the message is parsed, so the lines of every other station cost a lookup per word.
fn names_one(words: &[&str], wanted: &HashSet<String>) -> bool {
    words.iter().any(|w| {
        wanted.contains(*w)
            || (w.bytes().any(|b| b.is_ascii_lowercase())
                && wanted.contains(&w.to_ascii_uppercase()))
    })
}

/// What the stations in `wanted` (calls as decoded, upper-cased) SENT in the decode log `reader`
/// holds — ALL.TXT, in WSJT-X's line format ([`crate::alltxt`]): each received CQ, or call to
/// anyone, carrying the sender's grid, as runs of one grid `(first heard, last heard, grid)` in
/// the order heard. A line that is not such a decode (a transmission, a beacon spot, free text, a
/// line of another format) is passed over. Read one line at a time: the file can be large, and a
/// line naming none of `wanted` is passed over before its message is parsed.
pub fn heard_grids_in(reader: impl BufRead, wanted: &HashSet<String>) -> HeardGrids {
    let mut out = HeardGrids::new();
    for line in reader.split(b'\n') {
        let Ok(line) = line else { break };
        let line = String::from_utf8_lossy(&line);
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.len() < 5 || words[2] != "Rx" {
            continue;
        }
        let message = &words[message_start(&words)..];
        if message.is_empty() || !names_one(message, wanted) {
            continue;
        }
        let Some(when) = all_txt_time(words[0]) else {
            continue;
        };
        let (sender, grid) = match Msg::parse(&message.join(" ")) {
            Msg::Cq { de, grid, .. } | Msg::Grid { de, grid, .. } => (de, grid),
            _ => continue,
        };
        let sender = sender.to_ascii_uppercase();
        if grid.trim().is_empty() || !wanted.contains(&sender) {
            continue;
        }
        let runs = out.entry(sender).or_default();
        match runs.last_mut() {
            Some((first, last, g)) if g.eq_ignore_ascii_case(&grid) => {
                *first = (*first).min(when);
                *last = (*last).max(when);
            }
            _ => runs.push((when, when, grid)),
        }
    }
    out
}

/// Of the grids a station sent ([`heard_grids_in`]), the one in effect at `when`, and only one
/// heard within 24 hours of it (`tempo_core::roster::SENT_GRID_MAX_AGE_SECS`): the last run begun
/// at or before it, else the first begun after it. None heard that near: none.
pub fn heard_at(sent: &[(u64, u64, String)], when: u64) -> Option<&str> {
    // How far `when` is from a run: none inside it, else to its nearer end.
    let near = |first: u64, last: u64| {
        let gap = if when < first {
            first - when
        } else {
            when.saturating_sub(last)
        };
        gap <= tempo_core::roster::SENT_GRID_MAX_AGE_SECS
    };
    sent.iter()
        .rev()
        .find(|(first, last, _)| *first <= when && near(*first, *last))
        .or_else(|| {
            sent.iter()
                .find(|(first, last, _)| *first > when && near(*first, *last))
        })
        .map(|(_, _, g)| g.as_str())
}
