//! The Logbook's clean-up tools, as rules on records: which contacts are one contact logged twice,
//! what merging them keeps, what a bulk edit sets or clears, and the formats a check holds a
//! contact's grid and references to. Pure — the app reads the rows, decides with these, and
//! makes the change through the log's one write path, recorded in the history
//! (`tempo_app::logtools`).
//!
//! # Duplicates ([`dupe_groups`])
//!
//! Two contacts are one logged twice by **the log's own duplicate window**
//! ([`super::dedup::DEDUP_WINDOW_SECS`]) — the same band and mode — but on the **full logged
//! call**, case and padding aside, never the base call the logging guard compares: `W1AW/KH6` and
//! `W1AW/KL7` are two DXCC entities, `W1AW/4` and `W1AW/5` two call areas, and a merge of them
//! would hand one the other's confirmation. The wider "possible" tier asks the same question
//! within [`POSSIBLE_WINDOW_SECS`]. A group is measured from its FIRST contact, so it spans no
//! more than its window: a station worked every 45 minutes all afternoon is several groups, never
//! one that chains for hours.
//!
//! **Copies that differ in an identity field are two contacts** ([`distinct`]): the station call
//! (a portable `/P` aside: `KD9TAW/P` is `KD9TAW`, operator ruling), the operator, either grid, the operator's park or summit, the propagation mode or satellite.
//! A club log's two calls, a rover's grid corner and a satellite pass beside a simplex contact
//! are each owed their own record. A copy holding NONE of a field is no difference — that is the
//! gap a merge fills — but a copy naming no station call is under the call in Settings, as its
//! upload was; and a grid is one grid at any precision (`FN31` and `FN31PR`). [`split_distinct`]
//! splits a window's group on them, leaving out a copy that could belong to either of two, and the
//! app refuses a merge of copies that differ in one, whatever the operator chose.
//!
//! # A merge keeps everything it can ([`merge`])
//!
//! The kept contact is one of the group, its identity (id, call, band, mode, time) as it is. Every
//! other field is the union of the copies:
//!
//! - a field one copy holds and the kept one lacks is taken;
//! - **confirmations are unioned** — each channel's, `confirmed`, `award_confirmed`, credit granted
//!   and submitted — never taken off, and **never taken from a copy logged under another call**;
//! - **upload stamps are the kept copy's own**, never another copy's: neither a stamp nor the
//!   `LOTW_QSL_SENT` mark a stamp is read back from. A stamp is not history, so a stamp the merge
//!   gave the kept contact would outlive an undo of the merge and claim an upload that contact
//!   never had. The price: a service only a deleted copy reached is owed the merged contact, and
//!   the next upload sends it. Where the kept copy's time is the uploaded copy's (one contact
//!   imported twice) the service already holds that QSO and answers Duplicate; where the copies
//!   were logged minutes apart, a service that matches on the time may keep both, and nothing here
//!   can take one back. **One exception:** a copy whose station call is the merged contact's, or
//!   the kept copy's, but for a portable `/P` (`KD9TAW/P` beside `KD9TAW`) is the same QSO to these
//!   tools but another record to LoTW, which can delete neither. A service only it was sent to
//!   keeps its stamp on the merged contact, whichever spelling the merge puts on it and whichever
//!   copy is kept (a kept copy naming no station call too), so the next upload never signs the QSO
//!   a second time under the other one. A copy spelled as both the merged contact and the kept
//!   copy are takes the rule above. An undo of the merge leaves a carried stamp, as a `/P` edit
//!   leaves its stamps: the same station call, and the restored copy carries its own;
//! - a field two copies hold DIFFERENT values for is a [`Conflict`]: the merged contact takes the
//!   value the caller chose, else the kept copy's (else the first copy's that has one). A conflict
//!   is **guarded** — it must be chosen, never defaulted — when it is the contest exchange, the
//!   QSL-sent mark (a mark the operator CLEARED is a value too, so a copy marked sent never brings
//!   it back), operator-written text (COMMENT, NOTES), the worked station's park or summit
//!   (`SIG`), an identity field ([`IDENTITY`]), or a value a confirmed copy carries: a
//!   confirmation's own detail (a grid, a state, a zone LoTW sent) is never overwritten without
//!   the operator's say;
//! - a side's passthrough references (the other programme's reference `take_ota_side` parks in
//!   the passthrough) are never dropped unseen. Where the copies hold the same side (or one holds
//!   none) they are unioned, and a reference two copies hold differently is guarded. Where the
//!   sides differ the side is a guarded conflict, and its references come from the copy whose
//!   side was chosen, never from another: written after the side, the next read would take a
//!   left-over one for it. **A hunter's two-fer** — a summit on one copy, a park on the other —
//!   is no choice: both are right, so the merged contact keeps the summit as its side and the park
//!   parked beside it, as a read of such a record holds them ([`with_twofer_parked`]).
//!
//! The copies that are not kept are deleted by the merge, whole, into the history: nothing a
//! copy held is lost, and the merge is undone as one run.
//!
//! [`merge`] names every field of [`QsoRecord`] in one destructuring, without `..`: a field added
//! to the record does not compile here until it is given a rule, so no field is merged by being
//! forgotten.
//!
//! # A bulk edit ([`changed`])
//!
//! Sets or clears the fields in [`BULK_FIELDS`] — the operator's own station details, the
//! propagation mode, and the operator's park or summit. **Never the worked call**: by the edit
//! rule ([`super::edited`]) it keeps every confirmation but a changed station call's (below), and
//! queues nothing to any service.
//!
//! **One exception for the stamps: a changed STATION_CALLSIGN.** It is the call both upload paths
//! sign and book the contact under — the call in Settings where it names none — so a contact
//! uploaded under the old one is not on any service under the new one. Its upload stamps, and the
//! `LOTW_QSL_SENT` mark a stamp is read back from, are cleared exactly as a corrected call's are
//! ([`super::edited`]), so the next upload sends it under the call it now carries ([`resends`]).
//! Unlike a corrected call it is not queued here — no clean-up tool sends anything. **And the
//! services' confirmations go with the stamps** (operator ruling): LoTW, eQSL and QRZ each matched
//! the contact under the old station call, so their confirmations, LoTW's and eQSL's received
//! dates, eQSL's Authenticity-Guaranteed mark and LoTW's DXCC are taken off, and
//! `confirmed`/`award_confirmed` are read again from what is left ([`unconfirm_electronic`]). A
//! paper card is the operator's own record and stays, with its date and route; so do the credit
//! codes, which record what an award desk granted and what the operator submitted. Club Log keeps
//! no confirmation on the contact, only its stamp. The preview counts what is taken off
//! ([`confirmations_cleared`]), and the run's undo puts it back.
//!
//! Filling in a blank station call, or clearing one, is no change of call, whatever the call in
//! Settings is: no stamp or confirmation is cleared. Only one call replaced by another is a
//! change, and a portable `/P` added or taken off is not ([`resends`]). But a contact naming no
//! station call is uploaded under the call in Settings, so a fill with another call, or a clear of
//! a call that is not the Settings call, does change the call it goes to a service not yet sent
//! it under: the preview counts those ([`moves`]) for the operator to acknowledge.
//!
//! The marks that say Nexus or a callbook filled a field come off a field it changes, as an edit
//! takes them off. A park or summit is set only on a contact whose side holds no reference or one
//! of the same programme: another programme's reference is never replaced by it, and such a
//! contact is left as it is.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::callbook::{self, CALLBOOK_FILLED_FIELD};
use super::{
    canonical, extra_value, self_filled, set_extra, set_self_filled, QslRcvd, QsoRecord, RecordId,
    UploadState, SELF_FILLED_FIELD,
};

// ── duplicates ─────────────────────────────────────────────────────────────────────────────────

/// The "possible duplicate" tier's window: the same station, band and mode within an hour.
pub const POSSIBLE_WINDOW_SECS: u64 = 3_600;

/// What the duplicate scan reads of a contact.
#[derive(Debug, Clone)]
pub struct DupeKey {
    pub id: RecordId,
    pub call: String,
    pub band: String,
    pub mode: String,
    pub when_unix: u64,
}

/// A logged call as the duplicate finder compares it: the FULL call, trimmed, case aside.
pub fn call_key(call: &str) -> String {
    call.trim().to_ascii_uppercase()
}

/// The groups of contacts that are one contact logged more than once: the same full call
/// ([`call_key`]), band and mode (case aside), each within `window_secs` of the group's FIRST
/// contact. Each group in time order, two or more contacts; the groups newest first. A contact
/// with no call is in none.
pub fn dupe_groups(keys: Vec<DupeKey>, window_secs: u64) -> Vec<Vec<RecordId>> {
    type Slot = (String, String, String);
    let mut keyed: Vec<(Slot, u64, RecordId)> = keys
        .into_iter()
        .filter(|k| !k.call.trim().is_empty())
        .map(|k| {
            let slot = (
                call_key(&k.call),
                k.band.to_ascii_lowercase(),
                k.mode.to_ascii_lowercase(),
            );
            (slot, k.when_unix, k.id)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut groups: Vec<(u64, Vec<RecordId>)> = Vec::new();
    let mut current: Vec<RecordId> = Vec::new();
    let mut first = 0;
    let mut latest = 0;
    for (i, (slot, when, id)) in keyed.iter().enumerate() {
        // Measured from the group's first contact: a group spans no more than the window.
        let joins = i > 0 && keyed[i - 1].0 == *slot && when - first <= window_secs;
        if !joins {
            if current.len() > 1 {
                groups.push((latest, std::mem::take(&mut current)));
            }
            current.clear();
            first = *when;
        }
        current.push(*id);
        latest = *when;
    }
    if current.len() > 1 {
        groups.push((latest, current));
    }
    groups.sort_by(|a, b| b.0.cmp(&a.0));
    groups.into_iter().map(|(_, g)| g).collect()
}

/// The identity fields: copies holding different values for one are two contacts ([`distinct`]),
/// and a merge's conflict on one is guarded. By the tag a merge's conflict names it.
pub const IDENTITY: [&str; 6] = [
    "STATION_CALLSIGN",
    "OPERATOR",
    "MY_GRIDSQUARE",
    "GRIDSQUARE",
    "MY_SIG",
    "PROP_MODE",
];

/// The first field in which `a` and `b` are two contacts, not one: the call ([`call_key`]), the
/// band or mode (case aside), or an identity field ([`IDENTITY`]) both hold and hold differently,
/// case aside. A field one of them leaves empty is no difference. The station call is the one each
/// is uploaded under ([`station_call`]), so a copy naming none is under `settings_call`, the call
/// in Settings. A grid is one grid at any precision ([`same_locator`]). `None`: they may be one
/// contact.
pub fn distinct(a: &QsoRecord, b: &QsoRecord, settings_call: Option<&str>) -> Option<&'static str> {
    if call_key(&a.call) != call_key(&b.call) {
        return Some("CALL");
    }
    if !a.band.trim().eq_ignore_ascii_case(b.band.trim()) {
        return Some("BAND");
    }
    if !a.mode.trim().eq_ignore_ascii_case(b.mode.trim()) {
        return Some("MODE");
    }
    let differ = |x: &Option<String>, y: &Option<String>| match (text(x), text(y)) {
        (Some(x), Some(y)) => !x.eq_ignore_ascii_case(&y),
        _ => false,
    };
    let grids_differ = |x: &Option<String>, y: &Option<String>| match (text(x), text(y)) {
        (Some(x), Some(y)) => !same_locator(&x, &y),
        _ => false,
    };
    let fields: [(&'static str, bool); 7] = [
        (
            "STATION_CALLSIGN",
            matches!(
                (station_call(a, settings_call), station_call(b, settings_call)),
                (Some(x), Some(y)) if x != y
            ),
        ),
        ("OPERATOR", differ(&a.operator, &b.operator)),
        ("MY_GRIDSQUARE", grids_differ(&a.my_grid, &b.my_grid)),
        ("GRIDSQUARE", grids_differ(&a.grid, &b.grid)),
        // The operator's side: its programme and its reference (MY_SIG and MY_SIG_INFO).
        (
            "MY_SIG",
            match (
                side(&a.ota.my_program, &a.ota.my_ref),
                side(&b.ota.my_program, &b.ota.my_ref),
            ) {
                (Some(x), Some(y)) => x.0 != y.0,
                _ => false,
            },
        ),
        ("PROP_MODE", differ(&a.prop_mode, &b.prop_mode)),
        ("PROP_MODE", differ(&a.sat_name, &b.sat_name)),
    ];
    fields.into_iter().find(|(_, d)| *d).map(|(tag, _)| tag)
}

/// The call `r` is uploaded under, as the clean-up tools compare it: its STATION_CALLSIGN, else
/// `settings_call`, the call in Settings ([`crate::reconcile::lotw_own_call`]), trimmed, case
/// aside, a portable `/P` taken off ([`without_portable_p`]). `None` when neither names one.
pub fn station_call(r: &QsoRecord, settings_call: Option<&str>) -> Option<String> {
    crate::reconcile::lotw_own_call(r, settings_call).map(without_portable_p)
}

/// `call` without a portable `/P` at its end (operator ruling): `KD9TAW/P` is the same station call
/// as `KD9TAW` to the duplicate finder and to a change of station call. Only `/P` — `/M`, `/MM`,
/// `/QRP`, a prefix (`W1/KD9TAW`) or any other suffix is another call.
pub fn without_portable_p(call: String) -> String {
    match call.strip_suffix("/P") {
        Some(base) if !base.is_empty() => base.to_string(),
        _ => call,
    }
}

/// Whether two locators are one grid: the shorter, case aside, begins the longer and ends on a
/// pair (`FN31` and `FN31PR`; one logger writes the square, another the subsquare). `FN31PR` and
/// `FN31PS`, or `FN31` and `FN32`, are two.
fn same_locator(x: &str, y: &str) -> bool {
    let (x, y) = (x.to_ascii_uppercase(), y.to_ascii_uppercase());
    let (short, long) = if x.len() <= y.len() { (x, y) } else { (y, x) };
    short.len() % 2 == 0 && long.starts_with(&short)
}

/// One window's group split into the groups that may be one contact each: every copy joins the
/// first group it is [`distinct`] from no member of, in order. Only the groups of two or more.
///
/// A copy that could be a copy of either of two copies that are two contacts (one leaving blank
/// the grid in which the other two differ) is in no group: which one it was logged for is the
/// operator's to say, and a merge into the wrong one would hand it the other's confirmation.
pub fn split_distinct(copies: Vec<QsoRecord>, settings_call: Option<&str>) -> Vec<Vec<QsoRecord>> {
    let one = |a: &QsoRecord, b: &QsoRecord| distinct(a, b, settings_call).is_none();
    let ambiguous: Vec<bool> = copies
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let fits: Vec<&QsoRecord> = copies
                .iter()
                .enumerate()
                .filter(|&(j, m)| j != i && one(m, r))
                .map(|(_, m)| m)
                .collect();
            fits.iter()
                .enumerate()
                .any(|(k, x)| fits[k + 1..].iter().any(|y| !one(x, y)))
        })
        .collect();
    let mut groups: Vec<Vec<QsoRecord>> = Vec::new();
    for (r, _) in copies.into_iter().zip(ambiguous).filter(|(_, a)| !a) {
        match groups.iter_mut().find(|g| g.iter().all(|m| one(m, &r))) {
            Some(g) => g.push(r),
            None => groups.push(vec![r]),
        }
    }
    groups.retain(|g| g.len() > 1);
    groups
}

// ── merge ──────────────────────────────────────────────────────────────────────────────────────

/// A field the copies of a contact hold different values for.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    /// The field: its ADIF name, or for a pair that goes together the name of its first —
    /// `DXCC` (with `COUNTRY`), `FREQ` (with `FREQ_RX`), `PROP_MODE` (with `SAT_NAME`), `MY_SIG`
    /// and `SIG` (a side's programme with its reference), `QSL_SENT`, `CONTEST` (the whole
    /// exchange).
    pub tag: String,
    /// Each copy holding a value, and the value as shown.
    pub values: Vec<(RecordId, String)>,
    /// Must be chosen: never defaulted (see the module header).
    pub guarded: bool,
    /// The copy whose value the merged contact takes.
    pub chosen: RecordId,
}

/// What merging the copies of a contact makes: the contact, and the fields they disagreed on.
#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    pub record: QsoRecord,
    pub conflicts: Vec<Conflict>,
}

impl Merged {
    /// The guarded conflicts `choices` does not choose for: the merge must not be made until
    /// each is chosen.
    pub fn unchosen(&self, choices: &HashMap<String, RecordId>) -> Vec<&str> {
        self.conflicts
            .iter()
            .filter(|c| c.guarded && !choices.contains_key(&c.tag))
            .map(|c| c.tag.as_str())
            .collect()
    }
}

/// A field's text, trimmed; `None` when blank.
fn text(v: &Option<String>) -> Option<String> {
    v.as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// A field — or a pair that goes together — as the merge compares and copies it: its value as
/// compared and as shown (`None` when the copy holds none), and the copy of it from one record
/// into another.
struct Part {
    tag: &'static str,
    get: fn(&QsoRecord) -> Option<(String, String)>,
    put: fn(&mut QsoRecord, &QsoRecord),
}

/// A value compared as it is shown.
fn same(v: Option<String>) -> Option<(String, String)> {
    v.map(|v| (v.clone(), v))
}

/// A value compared case aside — a grid, a call.
fn caseless(v: Option<String>) -> Option<(String, String)> {
    v.map(|v| (v.to_ascii_uppercase(), v))
}

fn parts() -> Vec<Part> {
    vec![
        Part {
            tag: "GRIDSQUARE",
            get: |r| caseless(text(&r.grid)),
            put: |o, s| o.grid = s.grid.clone(),
        },
        Part {
            tag: "DXCC",
            get: |r| {
                let country = text(&r.country);
                (r.dxcc.is_some() || country.is_some()).then(|| {
                    let shown = match (r.dxcc, &country) {
                        (Some(n), Some(c)) => format!("{n} {c}"),
                        (Some(n), None) => n.to_string(),
                        (None, Some(c)) => c.clone(),
                        (None, None) => String::new(),
                    };
                    (shown.to_ascii_uppercase(), shown)
                })
            },
            put: |o, s| {
                o.dxcc = s.dxcc;
                o.country = s.country.clone();
            },
        },
        Part {
            tag: "STATE",
            get: |r| caseless(text(&r.state)),
            put: |o, s| o.state = s.state.clone(),
        },
        Part {
            tag: "NAME",
            get: |r| same(text(&r.name)),
            put: |o, s| o.name = s.name.clone(),
        },
        Part {
            tag: "QTH",
            get: |r| same(text(&r.qth)),
            put: |o, s| o.qth = s.qth.clone(),
        },
        Part {
            tag: "COMMENT",
            get: |r| same(text(&r.comment)),
            put: |o, s| o.comment = s.comment.clone(),
        },
        Part {
            tag: "NOTES",
            get: |r| same(text(&r.notes)),
            put: |o, s| o.notes = s.notes.clone(),
        },
        Part {
            tag: "RST_SENT",
            get: |r| same(text(&r.rst_sent)),
            put: |o, s| o.rst_sent = s.rst_sent.clone(),
        },
        Part {
            tag: "RST_RCVD",
            get: |r| same(text(&r.rst_rcvd)),
            put: |o, s| o.rst_rcvd = s.rst_rcvd.clone(),
        },
        Part {
            tag: "TX_PWR",
            get: |r| same(r.tx_power.map(|p| p.to_string())),
            put: |o, s| o.tx_power = s.tx_power,
        },
        Part {
            tag: "FREQ",
            get: |r| {
                let rx = r
                    .freq_rx_mhz
                    .map(|f| format!(" / {f:.6}"))
                    .unwrap_or_default();
                (r.freq_mhz > 0.0 || r.freq_rx_mhz.is_some())
                    .then(|| format!("{:.6}{rx}", r.freq_mhz))
                    .map(|v| (v.clone(), v))
            },
            put: |o, s| {
                o.freq_mhz = s.freq_mhz;
                o.freq_rx_mhz = s.freq_rx_mhz;
            },
        },
        Part {
            tag: "TIME_OFF",
            get: |r| same(r.time_off_unix.map(|t| t.to_string())),
            // An end time before the kept contact's start is no end time of it.
            put: |o, s| {
                o.time_off_unix = s.time_off_unix.filter(|&off| off >= o.when_unix);
            },
        },
        Part {
            tag: "PROP_MODE",
            get: |r| {
                let (p, s) = (text(&r.prop_mode), text(&r.sat_name));
                (p.is_some() || s.is_some()).then(|| {
                    let shown = format!("{} {}", p.unwrap_or_default(), s.unwrap_or_default())
                        .trim()
                        .to_string();
                    (shown.to_ascii_uppercase(), shown)
                })
            },
            put: |o, s| {
                o.prop_mode = s.prop_mode.clone();
                o.sat_name = s.sat_name.clone();
            },
        },
        Part {
            tag: "OPERATOR",
            get: |r| caseless(text(&r.operator)),
            put: |o, s| o.operator = s.operator.clone(),
        },
        Part {
            tag: "STATION_CALLSIGN",
            get: |r| caseless(text(&r.station_callsign)),
            put: |o, s| o.station_callsign = s.station_callsign.clone(),
        },
        Part {
            tag: "MY_GRIDSQUARE",
            get: |r| caseless(text(&r.my_grid)),
            put: |o, s| o.my_grid = s.my_grid.clone(),
        },
        Part {
            tag: "MY_RIG",
            get: |r| same(text(&r.my_rig)),
            put: |o, s| o.my_rig = s.my_rig.clone(),
        },
        Part {
            tag: "MY_SIG",
            get: |r| side(&r.ota.my_program, &r.ota.my_ref),
            put: |o, s| {
                o.ota.my_program = s.ota.my_program.clone();
                o.ota.my_ref = s.ota.my_ref.clone();
            },
        },
        Part {
            tag: "SIG",
            get: |r| side(&r.ota.their_program, &r.ota.their_ref),
            put: |o, s| {
                o.ota.their_program = s.ota.their_program.clone();
                o.ota.their_ref = s.ota.their_ref.clone();
            },
        },
        Part {
            tag: "IOTA",
            get: |r| caseless(text(&r.ota.iota)),
            put: |o, s| o.ota.iota = s.ota.iota.clone(),
        },
        Part {
            tag: "QSL_SENT",
            // A mark the operator CLEARED is a value too: a copy marked sent never fills it.
            get: |r| {
                if !r.qsl_sent.sent {
                    return r.qsl_sent.cleared_unix.map(|t| {
                        let shown = format!("N {}", super::query::fmt_utc(t));
                        (shown.clone(), shown)
                    });
                }
                r.qsl_sent.sent.then(|| {
                    let via = r
                        .qsl_sent
                        .via
                        .map(|v| format!(" {v:?}"))
                        .unwrap_or_default();
                    let date = r
                        .qsl_sent
                        .date_unix
                        .map(|d| format!(" {}", super::query::fmt_utc(d)))
                        .unwrap_or_default();
                    let shown = format!("Y{via}{date}");
                    (shown.clone(), shown)
                })
            },
            put: |o, s| o.qsl_sent = s.qsl_sent,
        },
        Part {
            tag: "CONTEST",
            get: |r| {
                r.contest.as_deref().map(|c| {
                    let sent = c
                        .stx_string
                        .clone()
                        .or(c.stx.map(|n| n.to_string()))
                        .unwrap_or_else(|| exchange(&c.sent));
                    let rcvd = c
                        .srx_string
                        .clone()
                        .or(c.srx.map(|n| n.to_string()))
                        .unwrap_or_else(|| exchange(&c.rcvd));
                    (
                        format!("{c:?}"),
                        format!("{} {sent} / {rcvd}", c.contest_id)
                            .trim()
                            .to_string(),
                    )
                })
            },
            put: |o, s| o.contest = s.contest.clone(),
        },
    ]
}

/// One side's park, summit or other programme, as compared and shown.
fn side(program: &Option<String>, reference: &Option<String>) -> Option<(String, String)> {
    let (p, r) = (text(program), text(reference));
    (p.is_some() || r.is_some()).then(|| {
        let shown = format!("{} {}", p.unwrap_or_default(), r.unwrap_or_default())
            .trim()
            .to_string();
        (shown.to_ascii_uppercase(), shown)
    })
}

/// A contest exchange's slots, as shown.
fn exchange(slots: &[(String, String)]) -> String {
    slots
        .iter()
        .map(|(_, v)| v.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The passthrough fields that are marks about OTHER fields: they follow the values they mark,
/// never merged as values of their own.
fn is_mark(name: &str) -> bool {
    name == SELF_FILLED_FIELD || name == CALLBOOK_FILLED_FIELD
}

/// How much a copy holds: the default copy to keep is the fullest.
fn fullness(r: &QsoRecord) -> usize {
    let held = parts().iter().filter(|p| (p.get)(r).is_some()).count();
    let confirmed = [
        r.qsl_rcvd.card,
        r.qsl_rcvd.lotw,
        r.qsl_rcvd.eqsl,
        r.qsl_rcvd.qrz,
    ]
    .into_iter()
    .filter(|c| *c)
    .count();
    held + r.extra.iter().filter(|(k, _)| !is_mark(k)).count() + 2 * confirmed
}

/// How many services a copy has been sent to ([`super::UploadOutcome::is_sent`]).
fn sent_to(r: &QsoRecord) -> usize {
    let u = &r.upload;
    [&u.lotw, &u.eqsl, &u.qrz, &u.clublog]
        .into_iter()
        .filter(|s| s.as_ref().is_some_and(|s| s.outcome.is_sent()))
        .count()
}

/// The copy a merge keeps unless the operator picks another: the fullest — the most fields,
/// confirmations counted twice — then, of equally full ones, the one sent to the most services
/// (a merge keeps only the kept copy's stamps, so keeping the sent one sends nothing again), and
/// then the first in log order.
pub fn suggested_keep(copies: &[QsoRecord]) -> usize {
    let rank = |r: &QsoRecord| (fullness(r), sent_to(r));
    let mut best = 0;
    for (i, r) in copies.iter().enumerate() {
        if rank(r) > rank(&copies[best]) {
            best = i;
        }
    }
    best
}

/// Whether a copy carries a confirmation: what it holds is a confirmation's detail.
fn confirmed(r: &QsoRecord) -> bool {
    r.confirmed || r.award_confirmed || r.qsl_rcvd.any()
}

/// Whether a conflict on `tag` must be chosen whoever holds it (see the module header).
fn always_guarded(tag: &str) -> bool {
    matches!(tag, "CONTEST" | "QSL_SENT" | "COMMENT" | "NOTES" | "SIG") || IDENTITY.contains(&tag)
}

/// Each side's passthrough references, by the part that holds the side: where the copies' sides
/// differ they follow the copy whose side the merge took; where they agree they are unioned.
const SIDE_EXTRAS: [(&str, [&str; 4]); 2] = [
    (
        "MY_SIG",
        ["MY_SIG", "MY_SIG_INFO", "MY_SOTA_REF", "MY_POTA_REF"],
    ),
    ("SIG", ["SIG", "SIG_INFO", "SOTA_REF", "POTA_REF"]),
];

/// The passthrough field an upload stamp is read back from: the kept copy's own, like a stamp.
const STAMP_EXTRA: &str = "LOTW_QSL_SENT";

/// The copies, a hunter's two-fer's parked as one record holds it: where one copy names a summit
/// for the worked station and another names a park (or another programme's reference), both are
/// right, and the record keeps both the way a read of it does — the summit as the side (a summit
/// always takes the side, `take_ota_side`) and the other reference beside it in the passthrough,
/// as its own side writes it (`ota_fields`). So the copies not on a summit come in with their side
/// parked, and the merge unions the parked references as it does any side's: none is dropped, and
/// two that differ are a guarded choice. Copies naming no summit come in as they are.
fn with_twofer_parked(copies: &[QsoRecord]) -> Vec<QsoRecord> {
    let summit = |r: &QsoRecord| {
        text(&r.ota.their_program).is_some_and(|p| p.eq_ignore_ascii_case("SOTA"))
            && text(&r.ota.their_ref).is_some()
    };
    let twofer = copies.iter().any(summit);
    copies
        .iter()
        .map(|r| {
            let written = super::ota_fields(
                "SIG",
                "SIG_INFO",
                "SOTA_REF",
                "POTA_REF",
                &r.ota.their_program,
                &r.ota.their_ref,
            );
            if !twofer || summit(r) || written.is_empty() {
                return r.clone();
            }
            let mut parked = r.clone();
            parked.ota.their_program = None;
            parked.ota.their_ref = None;
            for (k, v) in super::history::fields(&written) {
                set_extra(&mut parked, &k, Some(v));
            }
            parked
        })
        .collect()
}

/// ★ The copies of one contact merged into the copy `keep` (see the module header). `copies` in
/// log order; `choices` names, for a conflicting field, the copy whose value the merged contact
/// takes. The merged contact keeps `copies[keep]`'s id.
pub fn merge(copies: &[QsoRecord], keep: usize, choices: &HashMap<String, RecordId>) -> Merged {
    let copies = &with_twofer_parked(copies);
    let kept = &copies[keep];
    // The kept copy first: its value is the default wherever it has one.
    let order: Vec<&QsoRecord> = std::iter::once(kept)
        .chain(
            copies
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != keep)
                .map(|(_, r)| r),
        )
        .collect();
    let id_of = |r: &QsoRecord| r.id.expect("a contact the log holds carries an id");
    let pick = |tag: &str, holding: &[&QsoRecord]| -> usize {
        choices
            .get(tag)
            .and_then(|want| holding.iter().position(|r| id_of(r) == *want))
            .unwrap_or(0)
    };
    let mut out = kept.clone();
    let mut conflicts = Vec::new();
    // A copy logged under another call is another contact: named, guarded, and never a source of
    // confirmation or credit (below).
    let call = call_key(&kept.call);
    if order.iter().any(|r| call_key(&r.call) != call) {
        conflicts.push(Conflict {
            tag: "CALL".to_string(),
            values: order.iter().map(|r| (id_of(r), r.call.clone())).collect(),
            guarded: true,
            chosen: id_of(kept),
        });
    }
    // The copy each side was taken from, by its part's tag, where the copies' sides differed.
    let mut side_from: HashMap<&str, RecordId> = HashMap::new();

    for part in parts() {
        let holding: Vec<(&QsoRecord, (String, String))> = order
            .iter()
            .filter_map(|r| (part.get)(r).map(|v| (*r, v)))
            .collect();
        let Some((_, first)) = holding.first() else {
            continue;
        };
        let differ = holding.iter().any(|(_, v)| v.0 != first.0);
        let records: Vec<&QsoRecord> = holding.iter().map(|(r, _)| *r).collect();
        let from = if differ { pick(part.tag, &records) } else { 0 };
        (part.put)(&mut out, records[from]);
        if differ {
            side_from.insert(part.tag, id_of(records[from]));
            let guarded = always_guarded(part.tag) || records.iter().any(|r| confirmed(r));
            conflicts.push(Conflict {
                tag: part.tag.to_string(),
                values: holding
                    .iter()
                    .map(|(r, v)| (id_of(r), v.1.clone()))
                    .collect(),
                guarded,
                chosen: id_of(records[from]),
            });
        }
    }

    // The passthrough fields, one by one, as the parts are: the marks follow the values below.
    let mut names: Vec<&str> = order
        .iter()
        .flat_map(|r| r.extra.iter().map(|(k, _)| k.as_str()))
        .filter(|k| !is_mark(k))
        .collect();
    names.sort_unstable();
    names.dedup();
    let mut extra: Vec<(String, String)> = Vec::new();
    for name in names {
        // The copies this field may come from: the kept one alone for the stamp's mark, the one
        // whose side was taken for a side's reference where the sides differed, else every copy.
        let side = SIDE_EXTRAS
            .iter()
            .find(|(_, names)| names.contains(&name))
            .map(|(tag, _)| *tag);
        let source = if name == STAMP_EXTRA {
            Some(id_of(kept))
        } else {
            side.and_then(|tag| side_from.get(tag).copied())
        };
        let from_copies: Vec<&QsoRecord> = order
            .iter()
            .copied()
            .filter(|r| source.is_none_or(|id| id_of(r) == id))
            .collect();
        let holding: Vec<(&QsoRecord, &str)> = from_copies
            .iter()
            .filter_map(|r| extra_value(r, name).map(|v| (*r, v)))
            .filter(|(_, v)| !v.is_empty())
            .collect();
        let Some(&(_, first)) = holding.first() else {
            // Held empty by every copy that holds it: kept as the kept copy holds it, or as the
            // first holds it.
            if let Some((k, v)) = from_copies
                .iter()
                .find_map(|r| r.extra.iter().find(|(k, _)| k == name))
            {
                extra.push((k.clone(), v.clone()));
            }
            continue;
        };
        let differ = holding.iter().any(|(_, v)| *v != first);
        let records: Vec<&QsoRecord> = holding.iter().map(|(r, _)| *r).collect();
        let from = if differ { pick(name, &records) } else { 0 };
        let value = holding[from].1;
        extra.push((name.to_string(), value.to_string()));
        if differ {
            conflicts.push(Conflict {
                tag: name.to_string(),
                values: holding
                    .iter()
                    .map(|(r, v)| (id_of(r), v.to_string()))
                    .collect(),
                // A reference is never dropped unseen.
                guarded: side.is_some() || records.iter().any(|r| confirmed(r)),
                chosen: id_of(records[from]),
            });
        }
    }
    extra.sort_by(|a, b| a.0.cmp(&b.0));
    out.extra = extra;
    // The marks: a field is marked filled by Nexus or by a callbook when a copy that marked it
    // holds the value the merged contact now holds.
    let mut own: Vec<String> = Vec::new();
    let mut looked: Vec<String> = Vec::new();
    for r in &order {
        for tag in self_filled(r) {
            if mark_value(r, tag) == mark_value(&out, tag) && mark_value(r, tag).is_some() {
                own.push(tag.to_string());
            }
        }
        for tag in callbook::callbook_filled(r) {
            if mark_value(r, tag) == mark_value(&out, tag) && mark_value(r, tag).is_some() {
                looked.push(tag.to_string());
            }
        }
    }
    set_self_filled(&mut out, own);
    looked.sort();
    looked.dedup();
    set_extra(
        &mut out,
        CALLBOOK_FILLED_FIELD,
        (!looked.is_empty()).then(|| looked.join(",")),
    );

    // ★ Every field of the record, named — no `..`: a field added to `QsoRecord` does not compile
    // here until it has a rule. The rules, field by field:
    let QsoRecord {
        // The kept copy's identity: the contact the merge keeps.
        id: _,
        call: _,
        band: _,
        mode: _,
        when_unix: _,
        time_known: _,
        // The parts above.
        grid: _,
        country: _,
        state: _,
        freq_mhz: _,
        freq_rx_mhz: _,
        rst_sent: _,
        rst_rcvd: _,
        name: _,
        qth: _,
        comment: _,
        notes: _,
        tx_power: _,
        time_off_unix: _,
        qsl_sent: _,
        ota: _,
        dxcc: _,
        prop_mode: _,
        sat_name: _,
        operator: _,
        station_callsign: _,
        my_grid: _,
        my_rig: _,
        contest: _,
        // The passthrough, above.
        extra: _,
        // Unioned, below — from copies under the kept copy's call only.
        confirmed: _,
        award_confirmed: _,
        qsl_rcvd: _,
        credit_granted: _,
        credit_submitted: _,
        // The kept copy's own: never another copy's but a `/P` spelling's (the module header).
        upload: _,
    } = &out;
    for r in order[1..].iter().filter(|r| call_key(&r.call) == call) {
        out.confirmed |= r.confirmed;
        out.award_confirmed |= r.award_confirmed;
        out.qsl_rcvd.merge(r.qsl_rcvd);
        out.credit_granted.extend(r.credit_granted.iter().cloned());
        out.credit_submitted
            .extend(r.credit_submitted.iter().cloned());
    }
    for list in [&mut out.credit_granted, &mut out.credit_submitted] {
        list.sort();
        list.dedup();
    }
    // A copy whose station call is the merged contact's, or the kept copy's, but for a portable
    // `/P` is the same QSO here and another record on LoTW: its upload's stamp comes to the merged
    // contact, so the next upload does not sign it again under the spelling the merge put on it.
    let spelled = |r: &QsoRecord| text(&r.station_callsign).map(|c| c.to_ascii_uppercase());
    let apart = |x: &(Option<String>, Option<String>), r: &QsoRecord| {
        x.0.is_some() && station_call(r, None) == x.0 && spelled(r) != x.1
    };
    let merged = (station_call(&out, None), spelled(&out));
    let kept = (station_call(kept, None), spelled(kept));
    for r in order[1..]
        .iter()
        .filter(|r| call_key(&r.call) == call && (apart(&merged, r) || apart(&kept, r)))
    {
        let (o, u) = (&mut out.upload, &r.upload);
        for (mine, theirs) in [
            (&mut o.lotw, &u.lotw),
            (&mut o.eqsl, &u.eqsl),
            (&mut o.qrz, &u.qrz),
            (&mut o.clublog, &u.clublog),
        ] {
            let sent =
                |s: &Option<super::UploadStatus>| s.as_ref().is_some_and(|s| s.outcome.is_sent());
            if !sent(mine) && sent(theirs) {
                mine.clone_from(theirs);
            }
        }
    }
    Merged {
        record: out,
        conflicts,
    }
}

/// The value a fill mark names, as [`callbook`] and the self-filled list read it.
fn mark_value(r: &QsoRecord, tag: &str) -> Option<String> {
    let v = match tag {
        "NAME" => r.name.as_deref(),
        "QTH" => r.qth.as_deref(),
        "STATE" => r.state.as_deref(),
        "GRIDSQUARE" => r.grid.as_deref(),
        "MY_GRIDSQUARE" => r.my_grid.as_deref(),
        "IOTA" => r.ota.iota.as_deref(),
        _ => extra_value(r, tag),
    };
    v.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_ascii_uppercase)
}

// ── bulk edit and fixes ────────────────────────────────────────────────────────────────────────

/// A field the clean-up tools set or clear, by its ADIF name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Field {
    MyGridsquare,
    MyRig,
    Operator,
    StationCallsign,
    PropMode,
    SatName,
    TxPwr,
    MyPotaRef,
    MySotaRef,
    // The check's fixes, beside the above.
    Gridsquare,
    Band,
    Dxcc,
    Iota,
    PotaRef,
    SotaRef,
    TimeOff,
}

/// The fields a bulk edit offers. Never the call: see the module header.
pub const BULK_FIELDS: [Field; 9] = [
    Field::MyGridsquare,
    Field::MyRig,
    Field::Operator,
    Field::StationCallsign,
    Field::PropMode,
    Field::SatName,
    Field::TxPwr,
    Field::MyPotaRef,
    Field::MySotaRef,
];

/// One field set (`Some`) or cleared (`None`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Change {
    pub field: Field,
    pub value: Option<String>,
}

/// A field's value on a contact as the tools compare it — what a fix expects to find.
pub fn value_of(r: &QsoRecord, field: Field) -> Option<String> {
    let of_side = |program: &Option<String>, reference: &Option<String>, want: &str| {
        program
            .as_deref()
            .filter(|p| p.eq_ignore_ascii_case(want))
            .and(reference.clone())
    };
    let v = match field {
        Field::MyGridsquare => r.my_grid.clone(),
        Field::MyRig => r.my_rig.clone(),
        Field::Operator => r.operator.clone(),
        Field::StationCallsign => r.station_callsign.clone(),
        Field::PropMode => r.prop_mode.clone(),
        Field::SatName => r.sat_name.clone(),
        Field::TxPwr => r.tx_power.map(|p| p.to_string()),
        Field::MyPotaRef => of_side(&r.ota.my_program, &r.ota.my_ref, "POTA"),
        Field::MySotaRef => of_side(&r.ota.my_program, &r.ota.my_ref, "SOTA"),
        Field::Gridsquare => r.grid.clone(),
        Field::Band => Some(r.band.clone()),
        Field::Dxcc => r.dxcc.map(|n| n.to_string()),
        Field::Iota => r.ota.iota.clone(),
        Field::PotaRef => of_side(&r.ota.their_program, &r.ota.their_ref, "POTA"),
        Field::SotaRef => of_side(&r.ota.their_program, &r.ota.their_ref, "SOTA"),
        Field::TimeOff => r.time_off_unix.map(|t| t.to_string()),
    };
    v.filter(|v| !v.is_empty())
}

/// A Maidenhead locator of 2, 4, 6 or 8 characters, case aside: field letters A–R, square
/// digits, subsquare letters A–X, extended square digits.
pub fn valid_grid(g: &str) -> bool {
    let b = g.as_bytes();
    if !matches!(b.len(), 2 | 4 | 6 | 8) {
        return false;
    }
    b.iter().enumerate().all(|(i, c)| match i {
        0 | 1 => matches!(c.to_ascii_uppercase(), b'A'..=b'R'),
        2 | 3 | 6 | 7 => c.is_ascii_digit(),
        _ => matches!(c.to_ascii_uppercase(), b'A'..=b'X'),
    })
}

/// A POTA park reference: a 1–4 character prefix with a letter, a hyphen, 4 or 5 digits
/// (`US-1234`, `K-0001`, `VE-12345`). Several, comma-separated, for a two-fer.
pub fn valid_pota(r: &str) -> bool {
    each_ref(r, |one| {
        let Some((prefix, num)) = one.split_once('-') else {
            return false;
        };
        (1..=4).contains(&prefix.len())
            && prefix
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            && prefix.bytes().any(|c| c.is_ascii_uppercase())
            && (4..=5).contains(&num.len())
            && num.bytes().all(|c| c.is_ascii_digit())
    })
}

/// A SOTA summit reference: association, `/`, a two-character region, a hyphen, three digits
/// (`W7W/LC-001`, `G/LD-001`, `VK3/VE-001`).
pub fn valid_sota(r: &str) -> bool {
    each_ref(r, |one| {
        let Some((assoc, rest)) = one.split_once('/') else {
            return false;
        };
        let Some((region, num)) = rest.split_once('-') else {
            return false;
        };
        (1..=4).contains(&assoc.len())
            && assoc
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            && region.len() == 2
            && region
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            && num.len() == 3
            && num.bytes().all(|c| c.is_ascii_digit())
    })
}

/// An IOTA reference: a continent (AF, AN, AS, EU, NA, OC, SA), a hyphen, three digits.
pub fn valid_iota(r: &str) -> bool {
    super::valid_iota(r).is_some_and(|v| v == r)
}

fn each_ref(r: &str, one: impl Fn(&str) -> bool) -> bool {
    !r.is_empty() && r.split(',').all(|p| one(p.trim()))
}

/// A reference spelled the way its format wants it, where that is all that is wrong: trimmed,
/// upper case, a space or underscore read as the hyphen, and a POTA reference missing its hyphen
/// given one (`us1234` → `US-1234`). `None` when even that is not a valid reference.
pub fn respelled(r: &str, valid: fn(&str) -> bool) -> Option<String> {
    let parts: Vec<String> = r
        .split(',')
        .map(|p| {
            let p = p.trim().to_ascii_uppercase().replace([' ', '_'], "-");
            if valid(&p) {
                return p;
            }
            // A park missing its hyphen: the letters, then the digits.
            let at = p.find(|c: char| c.is_ascii_digit()).unwrap_or(0);
            let (prefix, num) = p.split_at(at);
            if !prefix.is_empty()
                && prefix.bytes().all(|c| c.is_ascii_uppercase())
                && num.bytes().all(|c| c.is_ascii_digit())
            {
                format!("{prefix}-{num}")
            } else {
                p
            }
        })
        .collect();
    let out = parts.join(",");
    valid(&out).then_some(out)
}

/// Why a change cannot be made as asked, before any contact is read: a value of the wrong form.
fn checked_value(field: Field, value: &str) -> Result<String, String> {
    let v = value.trim();
    let bad = |what: &str| Err(format!("'{v}' is not {what}."));
    match field {
        Field::MyGridsquare | Field::Gridsquare if !valid_grid(v) => bad("a grid square"),
        Field::TxPwr if !v.parse::<f64>().is_ok_and(|w| w.is_finite() && w >= 0.0) => {
            bad("a power in watts")
        }
        Field::Dxcc if v.parse::<u32>().is_err() => bad("a DXCC entity number"),
        Field::TimeOff if v.parse::<u64>().is_err() => bad("a time"),
        // A two-fer may be typed with ';' between its parks; it is stored with ','.
        Field::MyPotaRef | Field::PotaRef
            if !valid_pota(&v.replace(';', ",").to_ascii_uppercase()) =>
        {
            bad("a POTA park reference")
        }
        Field::MySotaRef | Field::SotaRef if !valid_sota(&v.to_ascii_uppercase()) => {
            bad("a SOTA summit reference")
        }
        Field::Iota if !valid_iota(&v.to_ascii_uppercase()) => bad("an IOTA reference"),
        Field::Band if v.is_empty() => bad("a band"),
        _ if v.is_empty() => Err("A value to set cannot be blank; clear the field instead.".into()),
        Field::Operator | Field::StationCallsign | Field::PropMode => Ok(v.to_ascii_uppercase()),
        // A two-fer as the activation stamps one: the parks upper case, comma-separated with no
        // spaces, each once.
        Field::MyPotaRef | Field::PotaRef => {
            let mut parks: Vec<String> = Vec::new();
            for park in v.split([',', ';']).map(|p| p.trim().to_ascii_uppercase()) {
                if !parks.contains(&park) {
                    parks.push(park);
                }
            }
            Ok(parks.join(","))
        }
        Field::MySotaRef | Field::SotaRef | Field::Iota => Ok(v.to_ascii_uppercase()),
        _ => Ok(v.to_string()),
    }
}

/// `changes` in the form they are made: each value checked and spelled as stored. `Err` names the
/// first value that is not of its field's form.
pub fn checked(changes: &[Change]) -> Result<Vec<Change>, String> {
    changes
        .iter()
        .map(|c| {
            Ok(Change {
                field: c.field,
                value: c
                    .value
                    .as_deref()
                    .map(|v| checked_value(c.field, v))
                    .transpose()?,
            })
        })
        .collect()
}

/// Set one side's programme reference: on a side holding none, or one of the same programme;
/// another programme's is left as it is. A clear takes only that programme's reference.
fn set_side(
    program: &mut Option<String>,
    reference: &mut Option<String>,
    want: &str,
    value: &Option<String>,
) {
    let mine = program
        .as_deref()
        .is_some_and(|p| p.eq_ignore_ascii_case(want));
    let empty = text(program).is_none() && text(reference).is_none();
    match value {
        Some(v) if mine || empty => {
            *program = Some(want.to_string());
            *reference = Some(v.clone());
        }
        None if mine => {
            *program = None;
            *reference = None;
        }
        _ => {}
    }
}

/// Whether a change from `before` to `after` changes the call the contact is uploaded under: one
/// STATION_CALLSIGN replaced by ANOTHER ([`station_call`]: trimmed, case and a portable `/P`
/// aside). Its stamps go, and the next upload sends it again under the call it now carries.
///
/// A blank station call filled in, or a station call cleared, is no change, whatever the call in
/// Settings is. A contact naming no station call was uploaded under whatever call
/// was in Settings then, which Nexus does not record: the fill records the call it was made under,
/// and clearing it never owes it to the services under a call it may never have been made under.
pub fn resends(before: &QsoRecord, after: &QsoRecord) -> bool {
    matches!(
        (station_call(before, None), station_call(after, None)),
        (Some(x), Some(y)) if x != y
    )
}

/// Whether a change from `before` to `after` sends the contact to a service under another call
/// though it is no change of call ([`resends`]): a blank station call filled with a call other
/// than `settings_call`, the call in Settings, or a station call other than it cleared — so the
/// contact is uploaded under another call ([`station_call`], a portable `/P` aside) — while a
/// service has not been sent it yet. Its stamps stay, so a service already sent it is not sent it
/// again; every other service gets it under the new call, and LoTW cannot take one back.
pub fn moves(before: &QsoRecord, after: &QsoRecord, settings_call: Option<&str>) -> bool {
    let u = &after.upload;
    let owed = [&u.lotw, &u.eqsl, &u.qrz, &u.clublog]
        .into_iter()
        .any(|s| !s.as_ref().is_some_and(|s| s.outcome.is_sent()));
    !resends(before, after)
        && owed
        && matches!(
            (station_call(before, settings_call), station_call(after, settings_call)),
            (Some(x), Some(y)) if x != y
        )
}

/// What a service matched under a station call, beside its confirmation flag: LoTW's and eQSL's
/// received dates, eQSL's Authenticity-Guaranteed mark and LoTW's DXCC. A paper card's date and
/// route (`QSLRDATE`, `QSL_RCVD_VIA`) are not among them.
pub(super) const ELECTRONIC_EXTRAS: [&str; 4] = [
    "LOTW_QSLRDATE",
    "EQSL_QSLRDATE",
    super::EQSL_AG_FIELD,
    super::LOTW_DXCC_FIELD,
];

/// A contact's confirmations by service. An older record holding one only in
/// `award_confirmed`/`confirmed`, with no per-service flag, is read as the log writes it: an award
/// confirmation as LoTW's, any other as eQSL's.
fn by_service(r: &QsoRecord) -> QslRcvd {
    let q = r.qsl_rcvd;
    if q.any() {
        return q;
    }
    QslRcvd {
        lotw: r.award_confirmed,
        eqsl: r.confirmed && !r.award_confirmed,
        ..QslRcvd::default()
    }
}

/// Take off `rec` the confirmations the services matched under its old station call (the module
/// header): LoTW's, eQSL's and QRZ's ([`by_service`]), with [`ELECTRONIC_EXTRAS`]. A paper card
/// stays, and so do the credit codes: what an award desk granted and the operator submitted.
fn unconfirm_electronic(rec: &mut QsoRecord) {
    let q = by_service(rec);
    if !(q.lotw || q.eqsl || q.qrz) {
        return;
    }
    rec.qsl_rcvd = QslRcvd {
        card: q.card,
        ..QslRcvd::default()
    };
    rec.confirmed = rec.qsl_rcvd.any();
    rec.award_confirmed = rec.qsl_rcvd.award();
    rec.extra
        .retain(|(k, _)| !ELECTRONIC_EXTRAS.contains(&k.as_str()));
}

/// How many of `before`'s LoTW, eQSL and QRZ confirmations `after` no longer holds: what a changed
/// station call took off, as the preview counts it.
pub fn confirmations_cleared(before: &QsoRecord, after: &QsoRecord) -> usize {
    let (b, a) = (by_service(before), by_service(after));
    [(b.lotw, a.lotw), (b.eqsl, a.eqsl), (b.qrz, a.qrz)]
        .into_iter()
        .filter(|&(was, now)| was && !now)
        .count()
}

/// ★ The contact `row` with `changes` made (each already [`checked`]), as an edit makes them: the
/// fill marks off a field changed, a side's left-over references gone with the side, the band in
/// the log's one form. `country` names the `COUNTRY` a DXCC number gives the contact, as every
/// change of the number writes it. `None` when nothing about the contact changes.
pub fn changed(
    row: &QsoRecord,
    changes: &[Change],
    country: &dyn Fn(&str, Option<u32>) -> Option<String>,
) -> Option<QsoRecord> {
    let mut rec = row.clone();
    for c in changes {
        let v = c.value.clone();
        match c.field {
            Field::MyGridsquare => rec.my_grid = v,
            Field::MyRig => rec.my_rig = v,
            Field::Operator => rec.operator = v,
            Field::StationCallsign => rec.station_callsign = v,
            Field::PropMode => rec.prop_mode = v,
            Field::SatName => rec.sat_name = v,
            Field::TxPwr => rec.tx_power = v.and_then(|w| w.parse().ok()),
            Field::MyPotaRef => set_side(&mut rec.ota.my_program, &mut rec.ota.my_ref, "POTA", &v),
            Field::MySotaRef => set_side(&mut rec.ota.my_program, &mut rec.ota.my_ref, "SOTA", &v),
            Field::PotaRef => set_side(
                &mut rec.ota.their_program,
                &mut rec.ota.their_ref,
                "POTA",
                &v,
            ),
            Field::SotaRef => set_side(
                &mut rec.ota.their_program,
                &mut rec.ota.their_ref,
                "SOTA",
                &v,
            ),
            Field::Gridsquare => rec.grid = v,
            Field::Band => rec.band = v.unwrap_or_default(),
            Field::Dxcc => {
                rec.dxcc = v.and_then(|n| n.parse().ok());
                rec.country = country(&rec.call, rec.dxcc).or(rec.country.clone());
            }
            Field::Iota => rec.ota.iota = v,
            Field::TimeOff => rec.time_off_unix = v.and_then(|t| t.parse().ok()),
        }
    }
    // What an edit does beside its fields (`super::edited`), for the fields these change.
    let grid_of = |g: &Option<String>| g.as_deref().map(|g| g.trim().to_ascii_uppercase());
    if grid_of(&rec.my_grid) != grid_of(&row.my_grid) {
        let tags = self_filled(&rec)
            .into_iter()
            .filter(|t| *t != "MY_GRIDSQUARE")
            .map(str::to_string)
            .collect();
        set_self_filled(&mut rec, tags);
    }
    callbook::unmark_edited(row, &mut rec);
    // A changed station call: the stamps go, as a corrected call's do (the module header).
    if resends(row, &rec) {
        rec.upload = UploadState::default();
        rec.extra.retain(|(k, _)| k != STAMP_EXTRA);
        unconfirm_electronic(&mut rec);
    }
    if (&rec.ota.my_program, &rec.ota.my_ref) != (&row.ota.my_program, &row.ota.my_ref) {
        rec.extra.retain(|(k, _)| {
            !matches!(
                k.as_str(),
                "MY_SIG" | "MY_SIG_INFO" | "MY_SOTA_REF" | "MY_POTA_REF"
            )
        });
    }
    if (&rec.ota.their_program, &rec.ota.their_ref) != (&row.ota.their_program, &row.ota.their_ref)
    {
        rec.extra
            .retain(|(k, _)| !matches!(k.as_str(), "SIG" | "SIG_INFO" | "SOTA_REF" | "POTA_REF"));
    }
    canonical::contact(&mut rec);
    (rec != *row).then_some(rec)
}

#[cfg(test)]
mod tests;
