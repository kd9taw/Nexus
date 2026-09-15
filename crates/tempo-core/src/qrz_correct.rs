//! **Correct at QRZ** — the operator-invoked repair of ONE contact already in the QRZ logbook.
//!
//! # Why this exists
//!
//! Phone contacts Nexus uploaded before 2026-09-15 carry no sideband at QRZ, because nothing in
//! Nexus recorded one until then (`logbook::adif_submode`'s `USB`/`LSB` entries). The ordinary
//! push cannot repair them: Nexus sends an `ACTION=INSERT`, QRZ answers `Duplicate`, and QRZ's
//! copy stays as it was. The only API that overwrites is `ACTION=INSERT&OPTION=REPLACE`, and
//! QRZ's own spec labels it with a warning. So this is deliberately not a sync, not a button
//! that fixes everything, and not something a timer can reach.
//!
//! # What QRZ documents, and what it does not
//!
//! * A duplicate is **callsign + band + mode + date/time within ±30 minutes**, UTC, and
//!   **submodes are folded into their parent mode** for matching — QRZ names the SSB/USB/LSB
//!   case itself. So adding `SUBMODE:USB` to a record whose `MODE` stays `SSB` changes none of
//!   the four keys.
//! * A QRZ confirmation is **derived continuously**, never stored: "Manual confirmation or
//!   confirmation override is not possible." An upload can neither create nor destroy one. The
//!   hazard is therefore not "replacing a confirmed QSO" — it is **changing a match key**, which
//!   is what the refusals below are all about.
//! * A replace that MISSES **inserts a duplicate**, and QRZ's answer says which happened:
//!   `RESULT=REPLACE` overwrote, `RESULT=OK` inserted. This module treats `OK` as a **failure**.
//! * **You cannot address a replace by `logid`** — the ADIF content match is the only aim there
//!   is (tested against a live account by another client, and consistent with the spec). That is
//!   precisely why the match-key refusal has to live here: there is no targeted call to fall
//!   back on.
//! * **Whether REPLACE rebuilds the row from the uploaded ADIF or merges into it is UNDOCUMENTED**
//!   — by QRZ, and by every other client examined. That single unknown is what shapes the
//!   mechanism: [`plan_correction`] starts from **QRZ's own stored record**, changes only the
//!   fields on an explicit allow-list, and sends the whole thing back. If REPLACE rebuilds, the
//!   rebuild is from QRZ's own bytes; if it merges, nothing is lost either. It is the only hedge
//!   available, and it is the mechanism rather than an optimisation.
//!
//! # No network here
//!
//! Everything in this module is pure except for what it asks a [`QrzLogbook`] to carry, and the
//! default [`NoTransport`] refuses. That is not decoration: it is how the test suite drives the
//! whole sequence — a missed replace, a scrubbed key, a preserved field — without a QRZ account
//! and without a socket.
//!
//! ⚠️ **The API key never reaches a message.** QRZ echoes the failing request back in `REASON`,
//! and the request body carries the key, so every reason this module surfaces goes through
//! [`scrub_key`] first. See its note for why that is stricter than the ordinary push path.

use crate::logbook::{adif_record, adif_submode, QsoRecord};
use crate::qrz::{
    parse_delete_response, parse_fetch, parse_push_response, parse_status_response,
    QrzMissRecovery, QrzPushResult,
};

/// QRZ's duplicate window, in minutes. Documented on QRZ's ADIF-import and confirmations pages.
pub const MATCH_WINDOW_MINUTES: i64 = 30;

/// The fields this action may change: the phone sideband and the exact-protocol note that rides
/// with it. **`MODE`, `BAND`, `CALL`, `QSO_DATE` and `TIME_ON` are deliberately absent** — they
/// are QRZ's match keys, and a correction that moved one would be aiming at a different record.
/// Everything QRZ holds outside this list is sent back exactly as QRZ gave it.
pub const SIDEBAND_FIELDS: &[&str] = &["SUBMODE", "APP_TEMPO_MODE"];

/// Which of QRZ's four match keys a correction would have moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKey {
    Callsign,
    Band,
    Mode,
    Time,
}

impl MatchKey {
    fn name(self) -> &'static str {
        match self {
            MatchKey::Callsign => "callsign",
            MatchKey::Band => "band",
            MatchKey::Mode => "mode",
            MatchKey::Time => "date/time",
        }
    }
}

/// Why the action will not proceed. Every one of these is checked BEFORE anything is written at
/// QRZ; the first three are checked before anything is even read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// More (or fewer) than one contact was selected. This action is one at a time, always.
    NotOneSelection(usize),
    /// The contact has no known time of day, so it cannot be aimed at a ±30-minute window.
    NoTime,
    /// The contact carries no `STATION_CALLSIGN` — named in QRZ's own insertion requirement.
    NoStationCallsign,
    /// The contact's date is outside the target logbook's configured range; QRZ rejects those.
    OutsideBookRange {
        date: String,
        start: Option<String>,
        end: Option<String>,
    },
    /// The correction would move one of QRZ's match keys.
    MatchKeyChanged(MatchKey),
    /// QRZ holds no record of this contact, so there is nothing to correct (a plain push, not a
    /// replace, is what an absent record needs).
    NoQrzRecord,
    /// More than one QRZ record fits the match keys; a replace cannot say which.
    Ambiguous(usize),
    /// QRZ's copy already says what the correction would say.
    NothingToChange,
    /// No transport was installed — the refusal a test build gets instead of a socket.
    NoTransport,
    /// The transport failed. Category text only; the transport never stringifies its URL.
    Transport(String),
    /// QRZ answered the read and refused it (bad key, etc.). Reason already scrubbed.
    QrzRefusedRead(Option<String>),
}

impl Refusal {
    /// One operator-facing sentence. Never contains the API key (the only free text that reaches
    /// here has already been through [`scrub_key`]).
    pub fn sentence(&self) -> String {
        match self {
            Refusal::NotOneSelection(n) => {
                format!("Correct at QRZ works on one contact at a time; {n} are selected.")
            }
            Refusal::NoTime => "This contact has no time of day recorded. QRZ matches on a \
                 ±30-minute window, so without a time the correction could land on the wrong \
                 contact — or create a duplicate."
                .to_string(),
            Refusal::NoStationCallsign => {
                "This contact has no station callsign recorded, and QRZ names the sending \
                 station's callsign in its own requirements for accepting a record. Add it in \
                 the logbook first."
                    .to_string()
            }
            Refusal::OutsideBookRange { date, start, end } => format!(
                "This contact is dated {date}, outside the date range of the QRZ logbook this \
                 API key opens ({} to {}). QRZ rejects those outright — check you are using the \
                 key for the right callsign's logbook.",
                start.as_deref().unwrap_or("the book's start"),
                end.as_deref().unwrap_or("the book's end"),
            ),
            Refusal::MatchKeyChanged(k) => format!(
                "This correction would change the {}, which is one of the four details QRZ \
                 matches on. QRZ cannot be told which record to overwrite — it finds it by those \
                 details — so changing one would not correct the record, it would add a second \
                 one. Fix the contact at QRZ by hand instead.",
                k.name()
            ),
            Refusal::NoQrzRecord => "QRZ holds no matching record for this contact, so there is \
                 nothing to correct. Use the ordinary QRZ push to upload it."
                .to_string(),
            Refusal::Ambiguous(n) => format!(
                "QRZ holds {n} records that match this contact's callsign, band, mode and time. \
                 A correction cannot say which one it means, so nothing was sent."
            ),
            Refusal::NothingToChange => {
                "QRZ's copy of this contact already carries everything this correction would \
                 set. Nothing was sent."
                    .to_string()
            }
            Refusal::NoTransport => {
                "No connection to QRZ is available, so nothing was read and nothing was sent."
                    .to_string()
            }
            Refusal::Transport(t) => format!("Could not reach QRZ: {t}"),
            Refusal::QrzRefusedRead(r) => match r {
                Some(r) => format!("QRZ refused to read the contact back: {r}"),
                None => "QRZ refused to read the contact back.".to_string(),
            },
        }
    }
}

// ----- the raw ADIF record, kept exactly as QRZ sent it ---------------------------------

/// One ADIF record as a list of `(NAME, value)` pairs **in the order they arrived**, with
/// nothing interpreted and nothing dropped.
///
/// This exists instead of `logbook::parse_adif` on purpose. That parser produces a
/// [`QsoRecord`], which is Nexus's model of a contact — a round trip through it normalises what
/// Nexus understands and sweeps the rest into `extra`. For sending QRZ's own bytes back to QRZ,
/// "what Nexus understands" is exactly the wrong filter: the field that matters most here is the
/// one Nexus does not model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawAdif {
    fields: Vec<(String, String)>,
}

impl RawAdif {
    /// The pairs, in QRZ's order.
    pub fn fields(&self) -> &[(String, String)] {
        &self.fields
    }

    /// The first value for `name` (case-insensitive), trimmed.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim())
    }

    /// Set `name`, replacing the first occurrence in place (so field order is preserved) or
    /// appending when it is new.
    pub fn set(&mut self, name: &str, value: &str) {
        match self
            .fields
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
        {
            Some(slot) => slot.1 = value.to_string(),
            None => self
                .fields
                .push((name.to_ascii_uppercase(), value.to_string())),
        }
    }

    /// Re-emit as ADIF, `<EOR>`-terminated. Lengths are byte counts, as everywhere else in this
    /// tree (`logbook::adif_record`).
    pub fn to_adif(&self) -> String {
        let mut out = String::new();
        for (k, v) in &self.fields {
            out.push_str(&format!("<{}:{}>{}", k, v.len(), v));
        }
        out.push_str("<EOR>\n");
        out
    }

    fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

/// Split an ADIF payload into raw records. Header (everything through `<EOH>`) is skipped.
///
/// Deliberately tolerant in the same ways `logbook::parse_adif` is — a declared length is
/// attacker-controllable, so the scan uses saturating arithmetic and clamps rather than
/// wrapping (a wrapped index walks backwards and hangs the process).
pub fn split_records(adif: &str) -> Vec<RawAdif> {
    let body = match adif.to_ascii_uppercase().find("<EOH>") {
        Some(i) => &adif[i + 5..],
        None => adif,
    };
    let mut out = Vec::new();
    let mut cur = RawAdif::default();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(rel) = body[i..].find('>') else {
            break;
        };
        let end = i + rel;
        let tag = &body[i + 1..end];
        i = end + 1;
        if tag.eq_ignore_ascii_case("EOR") {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        let mut parts = tag.splitn(3, ':');
        let name = parts.next().unwrap_or("").trim().to_ascii_uppercase();
        let len: usize = parts
            .next()
            .and_then(|l| l.trim().parse().ok())
            .unwrap_or(0);
        let stop = i.saturating_add(len);
        let val = body.get(i..stop).unwrap_or("").to_string();
        i = stop;
        if !name.is_empty() {
            cur.fields.push((name, val));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ----- match keys ------------------------------------------------------------------------

/// QRZ's mode key: the **parent** mode, because QRZ folds submodes into their parent for
/// matching and names `SSB`/`USB`/`LSB` as the example. Reuses the tree's one submode table, so
/// the two cannot drift.
fn parent_mode(mode: &str) -> String {
    let m = mode.trim();
    match adif_submode(m) {
        Some((parent, _)) => parent.to_ascii_uppercase(),
        None => m.to_ascii_uppercase(),
    }
}

/// Minutes since the ADIF epoch for `QSO_DATE` + `TIME_ON`, or `None` when either is missing or
/// unreadable. `TIME_ON` may be `HHMM` or `HHMMSS` — both are legal ADIF.
fn minutes_of(rec: &RawAdif) -> Option<i64> {
    let date = rec.get("QSO_DATE")?;
    let time = rec.get("TIME_ON")?;
    if date.len() < 8 || time.len() < 4 {
        return None;
    }
    let num = |s: &str, a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, m, d) = (num(date, 0, 4)?, num(date, 4, 6)?, num(date, 6, 8)?);
    let (hh, mm) = (num(time, 0, 2)?, num(time, 2, 4)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 {
        return None;
    }
    // Days from civil (Howard Hinnant) — the same algorithm `qrz::fetch_since_date` inverts.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 1440 + hh * 60 + mm)
}

/// Compare two records on QRZ's four match keys. `Ok(())` means a replace built from `ours`
/// would still find `theirs`.
///
/// Exposed so the guard can be exercised in both directions: a submode-only change passes, a
/// mode change is refused. A guard only ever shown not to fire is half a test.
pub fn check_match_keys(theirs: &RawAdif, ours: &RawAdif) -> Result<(), Refusal> {
    let same = |k: &str| match (theirs.get(k), ours.get(k)) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        (None, None) => true,
        _ => false,
    };
    if !same("CALL") {
        return Err(Refusal::MatchKeyChanged(MatchKey::Callsign));
    }
    if !same("BAND") {
        return Err(Refusal::MatchKeyChanged(MatchKey::Band));
    }
    let (tm, om) = (
        theirs.get("MODE").map(parent_mode),
        ours.get("MODE").map(parent_mode),
    );
    if tm != om {
        return Err(Refusal::MatchKeyChanged(MatchKey::Mode));
    }
    match (minutes_of(theirs), minutes_of(ours)) {
        (Some(a), Some(b)) if (a - b).abs() <= MATCH_WINDOW_MINUTES => Ok(()),
        (None, None) => Ok(()),
        _ => Err(Refusal::MatchKeyChanged(MatchKey::Time)),
    }
}

// ----- the plan --------------------------------------------------------------------------

/// A correction that has passed every refusal: QRZ's own record, with the allowed fields set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correction {
    /// The contact's callsign, as QRZ holds it.
    pub call: String,
    /// `YYYY-MM-DD`, as QRZ holds it.
    pub date: String,
    /// `HHMMZ`, as QRZ holds it.
    pub time: String,
    /// `(field, QRZ's value or None, the value this correction would set)` — what the operator
    /// is being asked to approve, and nothing else changes.
    pub changes: Vec<(String, Option<String>, String)>,
    /// QRZ's whole record with those changes applied, ready for `OPTION=REPLACE`.
    pub adif: String,
    /// The record in the operator's words, for the confirmation, the log line and any delete.
    pub subject: String,
}

impl Correction {
    /// The confirmation text: the callsign, the date, and exactly what will change.
    pub fn confirmation(&self) -> String {
        let mut s = format!(
            "Correct {} at QRZ — the contact of {} at {}.\n\nThis will change:\n",
            self.call, self.date, self.time
        );
        for (field, was, now) in &self.changes {
            match was {
                Some(w) => s.push_str(&format!("  {field}: {w} → {now}\n")),
                None => s.push_str(&format!("  {field}: (not set) → {now}\n")),
            }
        }
        s.push_str(
            "\nEverything else is sent back to QRZ exactly as QRZ holds it. QRZ has no undo.",
        );
        s
    }
}

/// The logbook's configured date range, from `ACTION=STATUS`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BookRange {
    pub start: Option<String>,
    pub end: Option<String>,
}

impl BookRange {
    /// `true` when `yyyymmdd` is inside the range. An unreported bound does not constrain.
    fn accepts(&self, yyyymmdd: &str) -> bool {
        let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
        let d = digits(yyyymmdd);
        if d.len() != 8 {
            return true;
        }
        let before = self
            .start
            .as_deref()
            .map(digits)
            .filter(|s| s.len() == 8)
            .is_some_and(|s| d < s);
        let after = self
            .end
            .as_deref()
            .map(digits)
            .filter(|s| s.len() == 8)
            .is_some_and(|s| d > s);
        !before && !after
    }
}

/// Plan the correction of ONE contact, or say why it will not happen.
///
/// `local` is Nexus's (corrected) record, `qrz_adif` is what `ACTION=FETCH&OPTION=CALL:…` gave
/// back, `selected` is how many contacts the operator had selected, and `correctable` is the
/// allow-list of fields that may be changed — [`SIDEBAND_FIELDS`] in the shipped path. The
/// allow-list is a parameter rather than a constant so the match-key guard can be driven from
/// the outside with a field that MUST trip it.
pub fn plan_correction(
    local: &QsoRecord,
    qrz_adif: &str,
    book: &BookRange,
    selected: usize,
    correctable: &[&str],
) -> Result<Correction, Refusal> {
    // 1 — one at a time, always. Checked first so bulk never reaches the network.
    if selected != 1 {
        return Err(Refusal::NotOneSelection(selected));
    }
    let mine = single_record(&adif_record(local))?;
    // 2 — no time means no ±30-minute aim. `adif_record` omits TIME_ON when the time is not
    //     actually known, so its absence here IS the "date-only import" case.
    if mine.get("TIME_ON").is_none_or(str::is_empty) {
        return Err(Refusal::NoTime);
    }
    // 3 — QRZ's own insertion requirement names the sending station's callsign.
    if mine.get("STATION_CALLSIGN").is_none_or(str::is_empty) {
        return Err(Refusal::NoStationCallsign);
    }
    // 4 — the logbook an API key opens has a configured date range, and QRZ rejects anything
    //     outside it. A callsign change opens a second book, so this is a live possibility.
    let date = mine.get("QSO_DATE").unwrap_or_default().to_string();
    if !book.accepts(&date) {
        return Err(Refusal::OutsideBookRange {
            date: pretty_date(&date),
            start: book.start.clone(),
            end: book.end.clone(),
        });
    }

    // 5 — find QRZ's own copy, by QRZ's own four keys.
    let theirs = pick_record(&mine, qrz_adif)?;

    // 6 — build the outgoing record from THEIRS, changing only what the allow-list permits.
    let mut outgoing = theirs.clone();
    let mut changes = Vec::new();
    for field in correctable {
        let Some(want) = mine.get(field).filter(|v| !v.is_empty()) else {
            continue;
        };
        let was = theirs.get(field).map(str::to_string);
        if was.as_deref() == Some(want) {
            continue;
        }
        outgoing.set(field, want);
        changes.push((
            (*field).to_ascii_uppercase(),
            was.filter(|w| !w.is_empty()),
            want.to_string(),
        ));
    }
    if changes.is_empty() {
        return Err(Refusal::NothingToChange);
    }

    // 7 — the structural guard, restated as a check. The allow-list already makes a match-key
    //     change unrepresentable in the shipped path; this is what makes it REFUSED rather than
    //     merely unlikely, and it is the direction a test can force.
    check_match_keys(&theirs, &outgoing)?;

    let call = theirs.get("CALL").unwrap_or_default().to_string();
    let date = pretty_date(theirs.get("QSO_DATE").unwrap_or_default());
    let time = pretty_time(theirs.get("TIME_ON").unwrap_or_default());
    Ok(Correction {
        subject: format!("{call} on {date} at {time}"),
        call,
        date,
        time,
        changes,
        adif: outgoing.to_adif(),
    })
}

/// Choose QRZ's copy of the contact, or say which key stopped it.
fn pick_record(mine: &RawAdif, qrz_adif: &str) -> Result<RawAdif, Refusal> {
    let all = split_records(qrz_adif);
    if all.is_empty() {
        return Err(Refusal::NoQrzRecord);
    }
    let hits: Vec<RawAdif> = all
        .iter()
        .filter(|r| check_match_keys(r, mine).is_ok())
        .cloned()
        .collect();
    match hits.len() {
        1 => Ok(hits.into_iter().next().expect("len checked")),
        0 => {
            // Name the key that stopped it, off the record that came closest — a bare "no
            // match" leaves the operator with nothing to act on.
            let near = all
                .iter()
                .find_map(|r| check_match_keys(r, mine).err())
                .unwrap_or(Refusal::NoQrzRecord);
            Err(near)
        }
        n => Err(Refusal::Ambiguous(n)),
    }
}

fn single_record(adif: &str) -> Result<RawAdif, Refusal> {
    split_records(adif)
        .into_iter()
        .next()
        .ok_or(Refusal::NoQrzRecord)
}

fn pretty_date(yyyymmdd: &str) -> String {
    match (yyyymmdd.get(0..4), yyyymmdd.get(4..6), yyyymmdd.get(6..8)) {
        (Some(y), Some(m), Some(d)) => format!("{y}-{m}-{d}"),
        _ => yyyymmdd.to_string(),
    }
}

fn pretty_time(hhmmss: &str) -> String {
    match hhmmss.get(0..4) {
        Some(hhmm) => format!("{hhmm}Z"),
        None => hhmmss.to_string(),
    }
}

// ----- the transport seam -----------------------------------------------------------------

/// The four QRZ Logbook calls this action can make, and **no others**.
///
/// It is deliberately not "post any body you like". A generic seam would let a delete be issued
/// from anywhere that can format a string; these four name what each call is for, and
/// [`QrzLogbook::delete_missed_replace`] can only be handed a [`QrzMissRecovery`], which has one
/// constructor and it is a replace that missed. The live implementation lives in
/// `propagation::live::qrz`; every implementation in the test suite is offline, so no test in
/// this tree contacts QRZ.
///
/// Each method returns QRZ's raw response text; the parsing is this module's.
pub trait QrzLogbook {
    /// `ACTION=STATUS` — the book's counts and configured date range.
    fn status(&self, api_key: &str) -> Result<String, String>;
    /// `ACTION=FETCH&OPTION=CALL:<call>` — QRZ's own stored copies of one contact.
    fn fetch_callsign(&self, api_key: &str, callsign: &str) -> Result<String, String>;
    /// `ACTION=INSERT&OPTION=REPLACE` — the correction itself.
    fn replace(&self, api_key: &str, adif: &str) -> Result<String, String>;
    /// `ACTION=DELETE&LOGIDS=<one id>` — the miss-recovery path, and nothing else.
    fn delete_missed_replace(
        &self,
        api_key: &str,
        recovery: &QrzMissRecovery,
    ) -> Result<String, String>;
}

/// The refusal a build with no transport installed gets instead of a socket.
pub struct NoTransport;

const NO_TRANSPORT: &str = "no QRZ transport is installed in this build";

impl QrzLogbook for NoTransport {
    fn status(&self, _api_key: &str) -> Result<String, String> {
        Err(NO_TRANSPORT.to_string())
    }
    fn fetch_callsign(&self, _api_key: &str, _callsign: &str) -> Result<String, String> {
        Err(NO_TRANSPORT.to_string())
    }
    fn replace(&self, _api_key: &str, _adif: &str) -> Result<String, String> {
        Err(NO_TRANSPORT.to_string())
    }
    fn delete_missed_replace(
        &self,
        _api_key: &str,
        _recovery: &QrzMissRecovery,
    ) -> Result<String, String> {
        Err(NO_TRANSPORT.to_string())
    }
}

/// Replace every occurrence of the API key with a marker.
///
/// **Why this is stricter than the ordinary push path.** QRZ echoes the failing request back
/// inside `REASON`, and the request body carries `KEY=…`. The ordinary push surfaces QRZ's
/// reason as-is to a toast that dies with the session; this action writes its outcome to the
/// connection log as well, which is printed and kept, so the key is scrubbed before anything
/// else happens to the text. Both the raw key and its percent-encoded form are matched, since
/// what comes back is an echo of a form-encoded body.
pub fn scrub_key(text: &str, api_key: &str) -> String {
    let key = api_key.trim();
    if key.is_empty() {
        return text.to_string();
    }
    let mut out = text.replace(key, "«api key»");
    let encoded = pct_encode(key);
    if encoded != key {
        out = out.replace(&encoded, "«api key»");
    }
    out
}

/// Percent-encode as `qrz::pct` does, so the scrubber matches an echo of the wire form.
fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ----- the action ------------------------------------------------------------------------

/// What happened when a correction was actually sent.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// `RESULT=REPLACE` — QRZ's matcher agreed, and QRZ's copy is corrected.
    Replaced {
        subject: String,
        logid: Option<String>,
    },
    /// `RESULT=OK` — **a failure.** QRZ's matcher did NOT agree: it inserted a second record
    /// instead of overwriting the first. `recovery`, when present, is the authority to delete
    /// exactly that new record and nothing else.
    MissedAndDuplicated {
        subject: String,
        recovery: Option<QrzMissRecovery>,
    },
    /// QRZ refused the write. Reason already scrubbed.
    Rejected {
        subject: String,
        reason: Option<String>,
    },
    /// Nothing was sent.
    Refused(Refusal),
}

impl Outcome {
    /// `true` only for a replace that actually replaced. `RESULT=OK` is not a success here.
    pub fn is_success(&self) -> bool {
        matches!(self, Outcome::Replaced { .. })
    }

    /// One operator-facing sentence. Never contains the API key.
    pub fn sentence(&self) -> String {
        match self {
            Outcome::Replaced { subject, .. } => {
                format!("QRZ's copy of {subject} is corrected.")
            }
            Outcome::MissedAndDuplicated { subject, recovery } => {
                let mut s = format!(
                    "QRZ did NOT correct {subject}. Its matcher did not recognise the record as \
                     the same contact, so instead of overwriting it, QRZ added a SECOND copy."
                );
                match recovery {
                    Some(r) => s.push_str(&format!(
                        " The new record is QRZ log id {}. Nexus can delete exactly that record \
                         — {} — and leave the original alone. Nothing has been deleted yet.",
                        r.logid(),
                        r.subject(),
                    )),
                    None => s.push_str(
                        " QRZ did not say which record it added, so the duplicate has to be \
                         removed by hand in the QRZ logbook.",
                    ),
                }
                s
            }
            Outcome::Rejected { subject, reason } => match reason {
                Some(r) => format!("QRZ refused to correct {subject}: {r}"),
                None => format!("QRZ refused to correct {subject}."),
            },
            Outcome::Refused(r) => r.sentence(),
        }
    }
}

/// A transport failure becomes a refusal: "no transport installed" is its own case so a build
/// without one says so instead of looking like a network outage. The text is scrubbed even
/// though the transport already refuses to stringify its URL — defence at the boundary that
/// surfaces the text, not only at the one that produces it.
fn transport_refusal(err: &str, api_key: &str) -> Refusal {
    if err.contains(NO_TRANSPORT) {
        Refusal::NoTransport
    } else {
        Refusal::Transport(scrub_key(err, api_key))
    }
}

/// Read the logbook's date range. A STATUS that fails does not block the correction — an
/// unreported range simply does not constrain (see [`BookRange::accepts`]).
fn read_book_range(qrz: &dyn QrzLogbook, api_key: &str) -> BookRange {
    match qrz.status(api_key) {
        Ok(body) => {
            let st = parse_status_response(&body);
            BookRange {
                start: st.start_date,
                end: st.end_date,
            }
        }
        Err(_) => BookRange::default(),
    }
}

/// Fetch QRZ's own copies and plan the correction — the preview behind the confirmation.
///
/// This is also step one of [`apply`], which runs it again rather than trusting a plan built
/// before the operator read the dialog: the record a replace is built from must be one QRZ just
/// handed us.
pub fn preview(
    qrz: &dyn QrzLogbook,
    api_key: &str,
    local: &QsoRecord,
    selected: usize,
) -> Result<Correction, Refusal> {
    if selected != 1 {
        return Err(Refusal::NotOneSelection(selected));
    }
    let book = read_book_range(qrz, api_key);
    let resp = qrz
        .fetch_callsign(api_key, &local.call)
        .map_err(|e| transport_refusal(&e, api_key))?;
    let fetched = parse_fetch(&resp);
    if !fetched.ok {
        return Err(Refusal::QrzRefusedRead(
            fetched.reason.map(|r| scrub_key(&r, api_key)),
        ));
    }
    plan_correction(local, &fetched.adif, &book, selected, SIDEBAND_FIELDS)
}

/// Send the correction. Fetches QRZ's copy again first, re-runs every refusal, and only then
/// writes.
pub fn apply(qrz: &dyn QrzLogbook, api_key: &str, local: &QsoRecord, selected: usize) -> Outcome {
    let plan = match preview(qrz, api_key, local, selected) {
        Ok(p) => p,
        Err(r) => return Outcome::Refused(r),
    };
    let resp = match qrz.replace(api_key, &plan.adif) {
        Ok(r) => r,
        Err(e) => return Outcome::Refused(transport_refusal(&e, api_key)),
    };
    let push = parse_push_response(&resp);
    let reason = push.reason.as_deref().map(|r| scrub_key(r, api_key));
    match push.result {
        QrzPushResult::Replace => Outcome::Replaced {
            subject: plan.subject,
            logid: push.logid,
        },
        // ⛔ NOT a success. QRZ says it INSERTED — its matcher disagreed with ours and the
        // logbook now holds two copies of this contact.
        QrzPushResult::Ok => Outcome::MissedAndDuplicated {
            recovery: QrzMissRecovery::from_missed_replace(&push, &plan.subject),
            subject: plan.subject,
        },
        _ => Outcome::Rejected {
            subject: plan.subject,
            reason,
        },
    }
}

/// Delete the duplicate a missed replace created — the ONLY delete this application can issue,
/// because [`QrzMissRecovery`] has no other constructor.
///
/// QRZ's delete is permanent and has no undo. Returns the sentence to show and log, which names
/// the record that was removed.
pub fn recover_from_miss(
    qrz: &dyn QrzLogbook,
    api_key: &str,
    recovery: &QrzMissRecovery,
) -> Result<String, String> {
    let resp = qrz
        .delete_missed_replace(api_key, recovery)
        .map_err(|e| scrub_key(&e, api_key))?;
    let del = parse_delete_response(&resp);
    if del.ok {
        Ok(format!(
            "Deleted the duplicate QRZ added for {} (log id {}). The original record is \
             untouched.",
            recovery.subject(),
            recovery.logid(),
        ))
    } else {
        Err(match del.reason {
            Some(r) => format!(
                "QRZ would not delete the duplicate for {} (log id {}): {}",
                recovery.subject(),
                recovery.logid(),
                scrub_key(&r, api_key),
            ),
            None => format!(
                "QRZ would not delete the duplicate for {} (log id {}).",
                recovery.subject(),
                recovery.logid(),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qrz::{QrzPush, QrzPushResult};

    /// 2024-03-01 14:32:00 UTC.
    const WHEN: u64 = 1_709_303_520;
    const KEY: &str = "SECRET-LOGBOOK-KEY-1234";

    fn local(mode: &str) -> QsoRecord {
        QsoRecord {
            call: "W1AW".into(),
            grid: None,
            country: None,
            state: None,
            band: "20m".into(),
            freq_mhz: 14.250,
            freq_rx_mhz: None,
            mode: mode.into(),
            rst_sent: Some("59".into()),
            rst_rcvd: Some("59".into()),
            name: None,
            qth: None,
            comment: None,
            notes: None,
            tx_power: None,
            when_unix: WHEN,
            time_off_unix: None,
            confirmed: false,
            award_confirmed: false,
            qsl_rcvd: Default::default(),
            qsl_sent: Default::default(),
            credit_granted: Vec::new(),
            credit_submitted: Vec::new(),
            upload: Default::default(),
            ota: Default::default(),
            time_known: true,
            dxcc: None,
            prop_mode: None,
            sat_name: None,
            operator: None,
            my_grid: None,
            my_rig: None,
            station_callsign: Some("KD9TAW".into()),
            contest: None,
            extra: Vec::new(),
        }
    }

    /// QRZ's stored copy: SSB with no sideband, and `LOTW_QSL_RCVD` — a field QRZ holds from its
    /// own LoTW import that Nexus's outgoing record does not carry. That field is the whole
    /// point of the fetch-then-send-whole mechanism.
    fn qrz_copy(time_on: &str) -> String {
        format!(
            "<CALL:4>W1AW<BAND:3>20m<MODE:3>SSB<QSO_DATE:8>20240301<TIME_ON:{}>{}\
             <STATION_CALLSIGN:6>KD9TAW<LOTW_QSL_RCVD:1>Y<APP_QRZLOG_STATUS:1>C<EOR>\n",
            time_on.len(),
            time_on
        )
    }

    fn fetch_ok(adif: &str) -> String {
        let escaped = adif.replace('<', "&lt;").replace('>', "&gt;");
        format!("RESULT=OK&COUNT=1&ADIF={escaped}")
    }

    /// A transport that answers each of the four calls from a canned body, records what it was
    /// asked to do, and NEVER opens a socket.
    struct Canned {
        status: String,
        fetch: String,
        insert: String,
        delete: String,
        /// `(call name, the argument that matters)`, in order.
        did: std::cell::RefCell<Vec<(&'static str, String)>>,
    }

    impl Canned {
        fn new(fetch: String, insert: &str) -> Self {
            Canned {
                status: "RESULT=OK&COUNT=5&START_DATE=2000-01-01&END_DATE=2030-12-31".into(),
                fetch,
                insert: insert.into(),
                delete: "RESULT=OK&COUNT=1".into(),
                did: std::cell::RefCell::new(Vec::new()),
            }
        }
        fn arg_of(&self, what: &str) -> Option<String> {
            self.did
                .borrow()
                .iter()
                .find(|(k, _)| *k == what)
                .map(|(_, v)| v.clone())
        }
    }

    impl QrzLogbook for Canned {
        fn status(&self, _api_key: &str) -> Result<String, String> {
            self.did.borrow_mut().push(("status", String::new()));
            Ok(self.status.clone())
        }
        fn fetch_callsign(&self, _api_key: &str, callsign: &str) -> Result<String, String> {
            self.did.borrow_mut().push(("fetch", callsign.to_string()));
            Ok(self.fetch.clone())
        }
        fn replace(&self, _api_key: &str, adif: &str) -> Result<String, String> {
            self.did.borrow_mut().push(("replace", adif.to_string()));
            Ok(self.insert.clone())
        }
        fn delete_missed_replace(
            &self,
            _api_key: &str,
            recovery: &QrzMissRecovery,
        ) -> Result<String, String> {
            self.did
                .borrow_mut()
                .push(("delete", recovery.logid().to_string()));
            Ok(self.delete.clone())
        }
    }

    /// A transport that FAILS THE TEST if it is called at all — the positive control for "this
    /// refusal happens before anything reaches QRZ".
    struct NeverCalled;

    impl QrzLogbook for NeverCalled {
        fn status(&self, _api_key: &str) -> Result<String, String> {
            panic!("a refusal must not reach QRZ");
        }
        fn fetch_callsign(&self, _api_key: &str, _callsign: &str) -> Result<String, String> {
            panic!("a refusal must not reach QRZ");
        }
        fn replace(&self, _api_key: &str, _adif: &str) -> Result<String, String> {
            panic!("a refusal must not reach QRZ");
        }
        fn delete_missed_replace(
            &self,
            _api_key: &str,
            _recovery: &QrzMissRecovery,
        ) -> Result<String, String> {
            panic!("a refusal must not reach QRZ");
        }
    }

    fn plan(rec: &QsoRecord, qrz: &str, fields: &[&str]) -> Result<Correction, Refusal> {
        plan_correction(rec, qrz, &BookRange::default(), 1, fields)
    }

    // ----- the refusals, each shown to fire AND shown not to fire ------------------------

    #[test]
    fn refuses_more_than_one_selection_and_allows_exactly_one() {
        let r = local("USB");
        let qrz = qrz_copy("143200");
        assert_eq!(
            plan_correction(&r, &qrz, &BookRange::default(), 2, SIDEBAND_FIELDS),
            Err(Refusal::NotOneSelection(2))
        );
        assert_eq!(
            plan_correction(&r, &qrz, &BookRange::default(), 0, SIDEBAND_FIELDS),
            Err(Refusal::NotOneSelection(0))
        );
        // Control: one selection, same inputs, gets a plan.
        assert!(plan(&r, &qrz, SIDEBAND_FIELDS).is_ok());
    }

    #[test]
    fn a_bulk_selection_never_reaches_qrz() {
        let out = apply(&NeverCalled, KEY, &local("USB"), 7);
        assert_eq!(out, Outcome::Refused(Refusal::NotOneSelection(7)));
    }

    #[test]
    fn refuses_a_contact_with_no_known_time() {
        let mut r = local("USB");
        r.time_known = false;
        assert_eq!(
            plan(&r, &qrz_copy("143200"), SIDEBAND_FIELDS),
            Err(Refusal::NoTime)
        );
        // Control: the same contact with its time known is planned.
        r.time_known = true;
        assert!(plan(&r, &qrz_copy("143200"), SIDEBAND_FIELDS).is_ok());
    }

    #[test]
    fn refuses_a_contact_with_no_station_callsign() {
        let mut r = local("USB");
        r.station_callsign = None;
        assert_eq!(
            plan(&r, &qrz_copy("143200"), SIDEBAND_FIELDS),
            Err(Refusal::NoStationCallsign)
        );
        // Control: the same contact with one is planned.
        r.station_callsign = Some("KD9TAW".into());
        assert!(plan(&r, &qrz_copy("143200"), SIDEBAND_FIELDS).is_ok());
    }

    #[test]
    fn refuses_a_contact_outside_the_logbooks_date_range() {
        let r = local("USB");
        let qrz = qrz_copy("143200");
        let narrow = BookRange {
            start: Some("2025-01-01".into()),
            end: Some("2025-12-31".into()),
        };
        let err = plan_correction(&r, &qrz, &narrow, 1, SIDEBAND_FIELDS).unwrap_err();
        assert!(
            matches!(&err, Refusal::OutsideBookRange { date, .. } if date == "2024-03-01"),
            "{err:?}"
        );
        // Control: a range that contains the contact is planned.
        let wide = BookRange {
            start: Some("2000-01-01".into()),
            end: Some("2030-12-31".into()),
        };
        assert!(plan_correction(&r, &qrz, &wide, 1, SIDEBAND_FIELDS).is_ok());
    }

    #[test]
    fn refuses_a_change_that_moves_a_match_key_and_allows_a_submode_only_change() {
        let r = local("USB");
        let qrz = qrz_copy("143200");
        // `MODE` on the allow-list is the only way to build an outgoing record whose parent mode
        // differs from QRZ's — and it must be refused. (`local("USB")` emits MODE=SSB, so force
        // the divergence with a mode whose parent really is different.)
        let mut ft8 = local("FT8");
        ft8.band = "20m".into();
        assert_eq!(
            plan(&ft8, &qrz, &["MODE"]),
            Err(Refusal::MatchKeyChanged(MatchKey::Mode))
        );
        // Control, the shipped path: the sideband rides as SUBMODE, MODE stays SSB, and the
        // change is allowed.
        let ok = plan(&r, &qrz, SIDEBAND_FIELDS).expect("a submode-only change is allowed");
        assert_eq!(ok.changes[0].0, "SUBMODE");
        assert_eq!(ok.changes[0].2, "USB");
        assert!(ok.adif.contains("<MODE:3>SSB"), "{}", ok.adif);
    }

    #[test]
    fn check_match_keys_fires_on_each_key_and_passes_a_submode_addition() {
        let base = split_records(&qrz_copy("143200")).remove(0);
        for (field, value, key) in [
            ("CALL", "W1XYZ", MatchKey::Callsign),
            ("BAND", "40m", MatchKey::Band),
            ("MODE", "FT8", MatchKey::Mode),
            ("TIME_ON", "150300", MatchKey::Time), // +31 minutes
        ] {
            let mut moved = base.clone();
            moved.set(field, value);
            assert_eq!(
                check_match_keys(&base, &moved),
                Err(Refusal::MatchKeyChanged(key)),
                "moving {field} must be refused"
            );
        }
        // Control 1: adding a SUBMODE moves nothing.
        let mut sub = base.clone();
        sub.set("SUBMODE", "USB");
        assert_eq!(check_match_keys(&base, &sub), Ok(()));
        // Control 2: inside the ±30-minute window is still a match.
        let mut near = base.clone();
        near.set("TIME_ON", "150100"); // +29 minutes
        assert_eq!(check_match_keys(&base, &near), Ok(()));
    }

    #[test]
    fn a_qrz_record_outside_the_time_window_is_not_the_contact() {
        let r = local("USB");
        // +31 minutes: outside QRZ's window, so this is a different contact.
        assert_eq!(
            plan(&r, &qrz_copy("150300"), SIDEBAND_FIELDS),
            Err(Refusal::MatchKeyChanged(MatchKey::Time))
        );
        // Control: +29 minutes is inside it.
        assert!(plan(&r, &qrz_copy("150100"), SIDEBAND_FIELDS).is_ok());
    }

    #[test]
    fn refuses_when_qrz_holds_nothing_or_holds_two() {
        let r = local("USB");
        assert_eq!(plan(&r, "", SIDEBAND_FIELDS), Err(Refusal::NoQrzRecord));
        let two = format!("{}{}", qrz_copy("143200"), qrz_copy("143100"));
        assert_eq!(plan(&r, &two, SIDEBAND_FIELDS), Err(Refusal::Ambiguous(2)));
        // Control: one record is planned.
        assert!(plan(&r, &qrz_copy("143200"), SIDEBAND_FIELDS).is_ok());
    }

    #[test]
    fn refuses_when_qrz_already_says_it() {
        let r = local("USB");
        let already = "<CALL:4>W1AW<BAND:3>20m<MODE:3>SSB<SUBMODE:3>USB<APP_TEMPO_MODE:3>USB\
                       <QSO_DATE:8>20240301<TIME_ON:6>143200<EOR>\n";
        assert_eq!(
            plan(&r, already, SIDEBAND_FIELDS),
            Err(Refusal::NothingToChange)
        );
    }

    // ----- the send-QRZ's-own-record-back hedge -------------------------------------------

    #[test]
    fn the_outgoing_record_is_qrzs_own_with_only_the_correction_applied() {
        let plan = plan(&local("USB"), &qrz_copy("143200"), SIDEBAND_FIELDS).unwrap();
        // The field QRZ holds and Nexus's own outgoing record does not model as its own: it must
        // survive, because REPLACE may rebuild the row from exactly these bytes.
        assert!(plan.adif.contains("<LOTW_QSL_RCVD:1>Y"), "{}", plan.adif);
        assert!(
            plan.adif.contains("<APP_QRZLOG_STATUS:1>C"),
            "{}",
            plan.adif
        );
        // Positive control: that field is NOT in what Nexus would have sent on its own, so its
        // presence above is the fetch doing the work and not a coincidence.
        let ours = adif_record(&local("USB"));
        assert!(!ours.contains("LOTW_QSL_RCVD"), "{ours}");
        // And the match keys are byte-identical to QRZ's.
        assert!(plan.adif.contains("<CALL:4>W1AW"));
        assert!(plan.adif.contains("<BAND:3>20m"));
        assert!(plan.adif.contains("<MODE:3>SSB"));
        assert!(plan.adif.contains("<TIME_ON:6>143200"));
    }

    #[test]
    fn the_confirmation_names_the_call_the_date_and_the_change() {
        let text = plan(&local("USB"), &qrz_copy("143200"), SIDEBAND_FIELDS)
            .unwrap()
            .confirmation();
        assert!(text.contains("W1AW"), "{text}");
        assert!(text.contains("2024-03-01"), "{text}");
        assert!(text.contains("SUBMODE"), "{text}");
        assert!(text.contains("USB"), "{text}");
    }

    // ----- RESULT=OK is a failure ---------------------------------------------------------

    #[test]
    fn result_ok_is_a_failure_and_result_replace_is_the_success() {
        let missed = Canned::new(
            fetch_ok(&qrz_copy("143200")),
            "RESULT=OK&COUNT=1&LOGID=130877825",
        );
        let out = apply(&missed, KEY, &local("USB"), 1);
        assert!(!out.is_success(), "RESULT=OK must never read as success");
        let Outcome::MissedAndDuplicated { recovery, .. } = &out else {
            panic!("expected a miss, got {out:?}");
        };
        assert_eq!(recovery.as_ref().unwrap().logid(), "130877825");
        let said = out.sentence();
        assert!(said.contains("did NOT correct"), "{said}");
        assert!(said.contains("SECOND copy"), "{said}");
        assert!(said.contains("130877825"), "{said}");
        assert!(said.contains("Nothing has been deleted yet"), "{said}");

        // Control: the same flow with RESULT=REPLACE is the success.
        let hit = Canned::new(
            fetch_ok(&qrz_copy("143200")),
            "RESULT=REPLACE&COUNT=1&LOGID=130877825",
        );
        let ok = apply(&hit, KEY, &local("USB"), 1);
        assert!(ok.is_success(), "{ok:?}");
        assert!(ok.sentence().contains("is corrected"), "{}", ok.sentence());
    }

    #[test]
    fn the_record_sent_is_the_one_qrz_just_handed_back() {
        let t = Canned::new(fetch_ok(&qrz_copy("143200")), "RESULT=REPLACE&COUNT=1");
        assert!(apply(&t, KEY, &local("USB"), 1).is_success());
        // The contact was read back from QRZ by callsign first…
        assert_eq!(t.arg_of("fetch").as_deref(), Some("W1AW"));
        // …and what went up is QRZ's own record, carrying the field QRZ holds and Nexus does
        // not model, plus the correction.
        let sent = t.arg_of("replace").expect("a replace was sent");
        assert!(sent.contains("<LOTW_QSL_RCVD:1>Y"), "{sent}");
        assert!(sent.contains("<SUBMODE:3>USB"), "{sent}");
        // Control: Nexus's own record would NOT have carried that field.
        assert!(!adif_record(&local("USB")).contains("LOTW_QSL_RCVD"));
    }

    // ----- DELETE is reachable only from a miss -------------------------------------------

    #[test]
    fn a_delete_can_only_be_built_from_a_replace_that_missed() {
        let push = |result, logid: Option<&str>| QrzPush {
            result,
            logid: logid.map(str::to_string),
            count: 1,
            reason: None,
        };
        // The one case that yields an authority: RESULT=OK with a logid.
        assert!(
            QrzMissRecovery::from_missed_replace(&push(QrzPushResult::Ok, Some("42")), "W1AW")
                .is_some()
        );
        // Every other outcome yields none — a successful replace most of all.
        for r in [
            QrzPushResult::Replace,
            QrzPushResult::Duplicate,
            QrzPushResult::AuthFail,
            QrzPushResult::Fail,
        ] {
            assert!(
                QrzMissRecovery::from_missed_replace(&push(r, Some("42")), "W1AW").is_none(),
                "{r:?} must not authorise a delete"
            );
        }
        // Nor can one be built without a logid, or without naming what is being deleted.
        assert!(
            QrzMissRecovery::from_missed_replace(&push(QrzPushResult::Ok, None), "W1AW").is_none()
        );
        assert!(
            QrzMissRecovery::from_missed_replace(&push(QrzPushResult::Ok, Some("42")), "  ")
                .is_none()
        );
        assert!(QrzMissRecovery::from_missed_replace(
            &push(QrzPushResult::Ok, Some("4 2; DROP")),
            "x"
        )
        .is_none());
    }

    #[test]
    fn the_recovery_delete_names_exactly_one_record() {
        let t = Canned::new(
            fetch_ok(&qrz_copy("143200")),
            "RESULT=OK&COUNT=1&LOGID=130877825",
        );
        let Outcome::MissedAndDuplicated { recovery, .. } = apply(&t, KEY, &local("USB"), 1) else {
            panic!("expected a miss");
        };
        let r = recovery.unwrap();
        let said = recover_from_miss(&t, KEY, &r).unwrap();
        assert!(said.contains("130877825"), "{said}");
        assert!(said.contains("W1AW"), "{said}");
        // Exactly the record QRZ said it had just added, and nothing else.
        assert_eq!(t.arg_of("delete").as_deref(), Some("130877825"));
        // Control: a successful replace produces no authority to delete at all.
        let hit = Canned::new(
            fetch_ok(&qrz_copy("143200")),
            "RESULT=REPLACE&COUNT=1&LOGID=9",
        );
        assert!(apply(&hit, KEY, &local("USB"), 1).is_success());
        assert_eq!(hit.arg_of("delete"), None);
    }

    // ----- the API key reaches no message -------------------------------------------------

    #[test]
    fn the_api_key_appears_in_no_message_qrz_provokes() {
        // QRZ echoes the failing request back in REASON, and the request carried the key. This
        // is the shape that leaks if nothing scrubs.
        let echoed = format!(
            "RESULT=FAIL&REASON=Unable to add QSO: bad request KEY={KEY}&ACTION=INSERT&LOGID=9"
        );
        // POSITIVE CONTROL: the fixture really does carry the key, so a clean result below is
        // the scrubber working and not an inert test.
        assert!(echoed.contains(KEY), "the fixture must carry the key");

        let t = Canned::new(fetch_ok(&qrz_copy("143200")), &echoed);
        let out = apply(&t, KEY, &local("USB"), 1);
        assert!(!out.sentence().contains(KEY), "{}", out.sentence());
        assert!(!format!("{out:?}").contains(KEY), "{out:?}");

        // The same for a refused READ, and for a transport failure.
        let mut t2 = Canned::new(fetch_ok(&qrz_copy("143200")), "RESULT=OK");
        t2.fetch = format!("RESULT=FAIL&REASON=bad key KEY={KEY}");
        let out2 = apply(&t2, KEY, &local("USB"), 1);
        assert!(!out2.sentence().contains(KEY), "{}", out2.sentence());

        // And the scrubber itself, both directions.
        assert!(!scrub_key(&echoed, KEY).contains(KEY));
        assert!(scrub_key(&echoed, KEY).contains("«api key»"));
        assert_eq!(scrub_key("nothing to hide", KEY), "nothing to hide");
    }

    #[test]
    fn the_percent_encoded_key_is_scrubbed_too() {
        let key = "ab cd/ef";
        let echoed = "REASON=request was KEY=ab%20cd%2Fef&ACTION=INSERT";
        // Control: the encoded form is really there and the raw form is not.
        assert!(echoed.contains("ab%20cd%2Fef"));
        assert!(!echoed.contains(key));
        assert!(!scrub_key(echoed, key).contains("ab%20cd%2Fef"));
    }

    // ----- no test contacts QRZ -----------------------------------------------------------

    #[test]
    fn a_build_with_no_transport_refuses_instead_of_reaching_the_network() {
        let out = apply(&NoTransport, KEY, &local("USB"), 1);
        assert_eq!(out, Outcome::Refused(Refusal::NoTransport));
        assert!(
            out.sentence().contains("No connection to QRZ"),
            "{}",
            out.sentence()
        );
        assert_eq!(
            preview(&NoTransport, KEY, &local("USB"), 1),
            Err(Refusal::NoTransport)
        );
        // Control: the same call with a transport installed gets past the refusal.
        let t = Canned::new(fetch_ok(&qrz_copy("143200")), "RESULT=REPLACE&COUNT=1");
        assert!(apply(&t, KEY, &local("USB"), 1).is_success());
    }

    // ----- the raw ADIF splitter ----------------------------------------------------------

    #[test]
    fn raw_records_round_trip_unknown_fields_verbatim() {
        let src = "<CALL:4>W1AW<SOME_FIELD_WE_DO_NOT_MODEL:5>hello<EOR>\n";
        let recs = split_records(src);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].get("SOME_FIELD_WE_DO_NOT_MODEL"), Some("hello"));
        assert_eq!(recs[0].to_adif(), src);
        // Control: a header is skipped and two records are two.
        let two = format!("hdr<EOH>\n{src}{src}");
        assert_eq!(split_records(&two).len(), 2);
    }

    #[test]
    fn an_absurd_declared_length_clamps_instead_of_wrapping() {
        // The length comes off the wire, so it is hostile input.
        let recs = split_records("<CALL:18446744073709551615>W1AW<EOR>");
        assert!(recs.len() <= 1);
    }
}
