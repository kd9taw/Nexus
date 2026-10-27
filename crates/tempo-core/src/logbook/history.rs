//! Every change to a contact, kept beside the log: when, what it changed (each field, before and
//! after), and where the change came from — and what it takes to undo one.
//!
//! # Where it is written, and why there
//!
//! In the store, by [`super::sqlite::LogDb::apply`], **in the transaction that writes the rows**.
//! Every change any part of the app makes reaches the store through that one statement path (the
//! writer thread's chunks), so nothing can change a contact without its history, and a crash
//! leaves both or neither: a chunk's rows and its history commit together. The row as it stood
//! is read there too — the store's own row, in the same transaction — so the record says what the
//! change actually did to what the store held, never what a caller believed it held.
//!
//! The change's origin ([`Note`]) is decided by the app, which knows who asked; it rides the
//! change to the writer. A change with no note writes no history: the one-time conversion of
//! `log.adi` (that is the log's starting point, not a change to it) and a store with no app
//! attached (tests that drive the writer directly).
//!
//! # What is kept, and how compactly
//!
//! One row per changed contact per change ([`qso_history`](DDL)), and one row per change
//! ([`history_run`](DDL)) holding what every contact it touched shares: its run id, its time and
//! its source.
//!
//! - **An edit** (and a merge, a fill): only the fields that changed, as two ADIF fragments —
//!   the values before and after, each `<TAG:n>value` in the log's own spelling. A field absent
//!   from one side was absent from the contact on that side.
//!   A merged contest contact's directed columns are one field of their own here
//!   ([`CONTEST_COLUMNS_TAG`], history only), never their plain tags, so a value moved between a
//!   column and the record's own field is a change, and its undo puts each back where it was.
//! - **A delete**: the contact whole, as the operator's own log writes it
//!   ([`super::adif_record_own_log`]), so it can be put back exactly — its id, its stamps and
//!   every field another logger left in it.
//!   As the history sees it ([`history_adif`]): a merged contest contact's columns under
//!   [`CONTEST_COLUMNS_TAG`], so it is put back with them where they were. ⚠️ An OLDER build
//!   that puts back a contact this build deleted does not know the tag and keeps it in the
//!   contact's passthrough, from where its writers send it on.
//! - **A new contact**: the event, and the upload stamps it arrived with (the `after` fragment;
//!   none when it carried none) — its content is the contact itself. The stamps are a reference
//!   point only: an undo of the change that added it can tell a contact uploaded since.
//!
//! **Upload stamps are not history** ([`UPLOAD_TAGS`]). A stamp is the app's own bookkeeping of
//! what a service holds, not something the operator wrote; recording one would make "mark all
//! as uploaded" a 150,000-row history entry, and undoing one would quietly owe the contact to the
//! service again. A change that moves only stamps writes no history at all.
//!
//! # Undo
//!
//! An undo is a change like any other, made through the same write path and recorded with source
//! [`Source::Undo`] and a link to the entry it undoes, so it can itself be undone (a redo).
//! What it does to the contact, as it stands now ([`undo`]):
//!
//! - a new contact: removed — **unless it has been changed since** (a later change of any kind
//!   but a delete by any other run, [`changed_after`]; or an upload since,
//!   [`Entry::uploaded_since`]), which keeps it, named in the answer;
//! - a deleted one: put back, whole, under its own id — **unless it was logged again after the
//!   delete**: a contact added since ([`created_after`], not itself put back by an undo) that the
//!   log's own duplicate rule calls the same one, which the app checks against its index when it
//!   makes the undo, and refuses as [`CannotUndo::Duplicate`]. A contact that was in the log
//!   beside it before the delete never blocks it: the log held both;
//! - an edit: each field it changed goes back, **but only a field still holding what the change
//!   wrote**. A field changed again since keeps the later value; an undo whose every field was
//!   changed since is refused rather than made as nothing. A blank station call is not put back
//!   where it would move the contact under another call than the one it names ([`revert`]);
//! - an entry of a kind this build does not know (a later build wrote it): refused, never taken
//!   for an edit.
//!
//! An undo never touches an upload stamp, with the one exception the edit rule already makes: an
//! undo that changes the callsign, or the station call the contact is uploaded under, clears the
//! stamps, as a corrected call does ([`super::edited`]), so the contact is owed to the services
//! under the call it now carries.
//! Nothing here sends anything anywhere.
//!
//! # How long it is kept
//!
//! An edit's fields, a new contact, a merge and a fill are kept for as long as the log is. A
//! delete's whole contact is the bulk of the history (about 1 KB a contact, against tens of bytes
//! for an edit), so it is kept for a number of days the operator sets ([`SNAPSHOT_DAYS`] by
//! default) and then taken out in bounded chunks by the writer, between changes ([`trim`]).
//!
//! # Older builds
//!
//! Both tables are new and the store's `schema_version` does not move, so a build from before
//! them opens the store as it always did and never reads them. What that build changes is not
//! recorded, and an undo made afterwards meets the contact as that build left it — which the
//! field-by-field rule above already answers.

use std::collections::{BTreeSet, HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};

use super::sqlite::{Result, RowWrite};
use super::{QsoRecord, RecordId};

/// The two tables, applied by every open after the store's own. `IF NOT EXISTS`, so an existing
/// store gains them on its first open by this build and nothing else about it changes.
///
/// `qso_history.run_id` names its `history_run` row without a foreign key, and `qso_id` names a
/// contact without one: a history row outlives the contact it describes (that is what a restore
/// reads), and a store written by an older build may hold contacts whose history never began.
pub(crate) const DDL: &str = "
CREATE TABLE IF NOT EXISTS history_run (
  id      INTEGER PRIMARY KEY,
  run     TEXT    NOT NULL UNIQUE,  -- the run id: one change, or one job's changes
  at_ms   INTEGER NOT NULL,         -- when it was made, Unix milliseconds
  source  TEXT    NOT NULL          -- who made it: Source::code
);
CREATE TABLE IF NOT EXISTS qso_history (
  seq     INTEGER PRIMARY KEY,      -- order of changes, across every contact
  run_id  INTEGER NOT NULL,         -- history_run.id
  qso_id  TEXT    NOT NULL,         -- the contact's id (APP_NEXUS_ID)
  kind    INTEGER NOT NULL,         -- Kind::code
  undoes  INTEGER,                  -- the seq this change undid, if it was an undo
  before  TEXT,                     -- edit: the changed fields as they were; delete: the contact
  after   TEXT                      -- edit: the changed fields as they became
);
";

/// The history's indexes, built with the store's own ([`super::sqlite::LogDb::build_indexes`]),
/// never by the tables' open: the one-time conversion opens a store with its tables only, and a
/// check made of a store another window is still converting must write nothing to it.
pub(crate) const INDEXES: &str = "
CREATE INDEX IF NOT EXISTS qso_history_qso ON qso_history(qso_id);
CREATE INDEX IF NOT EXISTS qso_history_run ON qso_history(run_id);
CREATE INDEX IF NOT EXISTS qso_history_deleted ON qso_history(seq) WHERE kind = 2;
";

/// The tags an upload stamp is written as. Never compared, never recorded, never undone.
pub const UPLOAD_TAGS: [&str; 4] = [
    "APP_TEMPO_UL_LOTW",
    "APP_TEMPO_UL_EQSL",
    "APP_TEMPO_UL_QRZ",
    "APP_TEMPO_UL_CLUBLOG",
];

/// Who made a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// The operator, at this station: a logged contact, an edit, a mark, a delete, a Logbook tool.
    Operator,
    /// An ADIF import (the Logbook's, or a companion program's logged contact).
    Import,
    /// The background fill of country and state.
    Fill,
    /// A LoTW confirmation download.
    Lotw,
    /// An eQSL InBox download.
    Eqsl,
    /// QRZ's logbook download.
    Qrz,
    /// A pota.app export's park references.
    Pota,
    /// The contest log's merge into the general log.
    Contest,
    /// A contact logged from a Remote client.
    Remote,
    /// Another program writing `log.adi` beside a session that keeps it.
    File,
    /// An undo (or a redo) made from the history.
    Undo,
    /// The one-time copy of merged contest contacts' received grid, name and QTH into their own
    /// fields ([`super::with_contest_columns_copied`]): one run, listed for a whole-run undo.
    ContestColumns,
}

impl Source {
    /// The word stored for it — a token, never translated.
    pub fn code(self) -> &'static str {
        match self {
            Source::Operator => "operator",
            Source::Import => "import",
            Source::Fill => "fill",
            Source::Lotw => "lotw",
            Source::Eqsl => "eqsl",
            Source::Qrz => "qrz",
            Source::Pota => "pota",
            Source::Contest => "contest",
            Source::Remote => "remote",
            Source::File => "file",
            Source::Undo => "undo",
            Source::ContestColumns => "contest-columns",
        }
    }
}

/// What a change did to one contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// It was added.
    Create,
    /// Fields of it changed, by the operator or an undo.
    Edit,
    /// It was taken out of the log.
    Delete,
    /// Fields of it changed by a download or an import that matched it.
    Merge,
    /// An empty field of it was filled.
    Fill,
    /// A kind this build does not know: written by a later one. Shown, never undone — what it
    /// did to the contact is that build's to say.
    Unknown(i64),
}

impl Kind {
    fn code(self) -> i64 {
        match self {
            Kind::Create => 0,
            Kind::Edit => 1,
            Kind::Delete => 2,
            Kind::Merge => 3,
            Kind::Fill => 4,
            Kind::Unknown(code) => code,
        }
    }

    fn of_code(code: i64) -> Kind {
        match code {
            0 => Kind::Create,
            1 => Kind::Edit,
            2 => Kind::Delete,
            3 => Kind::Merge,
            4 => Kind::Fill,
            _ => Kind::Unknown(code),
        }
    }

    /// The word for it the UI keys its labels on.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Create => "create",
            Kind::Edit => "edit",
            Kind::Delete => "delete",
            Kind::Merge => "merge",
            Kind::Fill => "fill",
            Kind::Unknown(_) => "unknown",
        }
    }
}

/// Where a change came from: what every contact it touches shares. The app makes one per
/// change (or one per job, for a job written in several changes) and it rides the change to
/// the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// The run this change belongs to — unique across every window and session sharing the
    /// store, so a job's changes can be undone together.
    pub run: String,
    /// When, Unix milliseconds.
    pub at_ms: i64,
    /// Who.
    pub source: Source,
    /// What a change to a contact the log already holds is called: [`Kind::Edit`],
    /// [`Kind::Merge`] or [`Kind::Fill`]. A contact added or taken out is that, whatever this is.
    pub edit: Kind,
    /// For an undo: the entry it undoes, by the contact it changes.
    pub undoes: HashMap<RecordId, i64>,
}

/// One entry of a contact's history, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its place in the order of every change.
    pub seq: i64,
    /// The contact.
    pub qso_id: RecordId,
    /// What the change did to it.
    pub kind: Kind,
    /// The change's run.
    pub run: String,
    /// When, Unix milliseconds.
    pub at_ms: i64,
    /// Who: a [`Source::code`] (or a word a later build wrote).
    pub source: String,
    /// The entry this one undid, if it was an undo.
    pub undoes: Option<i64>,
    /// An edit: the changed fields as they were. A delete: the contact, whole.
    pub before: Option<String>,
    /// An edit: the changed fields as they became.
    pub after: Option<String>,
}

/// One field an entry changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    /// The ADIF tag.
    pub tag: String,
    /// Its value before, `None` where the contact did not carry it.
    pub old: Option<String>,
    /// Its value after, `None` where the change took it away.
    pub new: Option<String>,
}

impl Entry {
    /// The fields this entry changed, in tag order. A delete lists every field of the contact it
    /// took out, with no new value; a new contact lists nothing.
    pub fn changes(&self) -> Vec<FieldChange> {
        let before: HashMap<String, String> = fields(self.before.as_deref().unwrap_or(""))
            .into_iter()
            .collect();
        let after: HashMap<String, String> = fields(self.after.as_deref().unwrap_or(""))
            .into_iter()
            .collect();
        let tags: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
        tags.into_iter()
            .map(|tag| FieldChange {
                tag: tag.clone(),
                old: before.get(tag).cloned(),
                new: after.get(tag).cloned(),
            })
            .collect()
    }

    /// Whether the contact a new-contact entry added carries other upload stamps now than it
    /// arrived with: uploaded (or marked uploaded) since. `false` for any other entry.
    pub fn uploaded_since(&self, current: &QsoRecord) -> bool {
        self.kind == Kind::Create
            && fields(self.after.as_deref().unwrap_or("")) != fields(&uploads_of(current))
    }

    /// The contact a delete took out, as it was. `None` for any other entry.
    pub fn deleted_contact(&self) -> Option<QsoRecord> {
        if self.kind != Kind::Delete {
            return None;
        }
        let text = self.before.as_deref()?;
        let mut rec = rebuilt(text)?;
        rec.id = Some(self.qso_id);
        Some(rec)
    }
}

/// An ADIF record's fields, in order, as the importer reads them: the tag upper-cased, the value
/// the `len` bytes after it. Up to `<EOR>`; a header is not expected (a fragment has none).
pub fn fields(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(end) = text[i..].find('>').map(|e| i + e) else {
            break;
        };
        let tag = &text[i + 1..end];
        i = end + 1;
        if tag.eq_ignore_ascii_case("EOR") {
            break;
        }
        let mut parts = tag.splitn(3, ':');
        let name = parts.next().unwrap_or("").to_ascii_uppercase();
        let len: usize = parts
            .next()
            .and_then(|l| l.trim().parse().ok())
            .unwrap_or(0);
        let stop = i.saturating_add(len);
        out.push((name, text.get(i..stop).unwrap_or("").to_string()));
        i = stop;
    }
    out
}

/// ⭐ **The history's own tag for a contest exchange's directed columns**
/// ([`super::ContestFields::adif`]): every one of them, in their order, as one value
/// ([`crate::contest::carrier::encode_pairs`]). History only — it is never written to the log's
/// file, an export or an upload, and never left in a record ([`rebuilt`] takes it back out).
///
/// Why it exists: the operator's own ADIF writes those columns under their plain tags, the same
/// tags the record's own fields write (`GRIDSQUARE`, `NAME`, `QTH`, `STATE`) and its passthrough
/// writes (`CQZ`), and reads them back into the fields. So a change that moved a value from a
/// contest column into a field — or replaced a contest `STATE` beside the field's own — wrote the
/// same tags before and after and recorded nothing, and an undo rebuilt from those tags put every
/// contest column into a field. Here the columns are a field of their own, so both are exact.
///
/// It is in an edit's fragments whenever the contact had contest columns on either side — with
/// the same value on both when they did not change — which is how [`revert`] tells an entry this
/// build recorded from one an older build did ([`old_format_kept`]).
pub const CONTEST_COLUMNS_TAG: &str = "APP_NEXUS_C_ADIF";

/// The prefix of every history-only tag: what no ADIF the log writes may ever carry.
pub const HISTORY_ONLY_PREFIX: &str = "APP_NEXUS_C_";

/// ⚠️ **A history-only tag found in a record's passthrough is taken out of it** — every tag of
/// [`HISTORY_ONLY_PREFIX`] — and the contest columns [`CONTEST_COLUMNS_TAG`] holds are put back
/// where a record keeps them, where it holds none of its own. Only a build from before this tag
/// leaves it there: one that puts back a contact this build deleted, or undoes an entry that took
/// a contact's last contest column out, keeps the tag it does not know as a field it does not
/// model. Read back here, the contact is healed — its columns where they belong, and the tag
/// never handed to a writer (which skips it as well).
pub(crate) fn heal_history_only(r: &mut QsoRecord) {
    if !r
        .extra
        .iter()
        .any(|(k, _)| k.starts_with(HISTORY_ONLY_PREFIX))
    {
        return;
    }
    let stray = r
        .extra
        .iter()
        .find(|(k, _)| k == CONTEST_COLUMNS_TAG)
        .map(|(_, v)| crate::contest::carrier::decode_pairs(v));
    r.extra.retain(|(k, _)| !k.starts_with(HISTORY_ONLY_PREFIX));
    if let Some(columns) = stray.filter(|c| !c.is_empty()) {
        if r.contest.as_deref().is_none_or(|c| c.adif.is_empty()) {
            r.contest.get_or_insert_with(Default::default).adif = columns;
        }
    }
}

/// A contact's ADIF as the history sees it: the operator's own log's, with a contest exchange's
/// directed columns under [`CONTEST_COLUMNS_TAG`] instead of their plain tags — so the plain tags
/// are exactly the record's own fields and its passthrough.
pub(crate) fn history_adif(r: &QsoRecord) -> String {
    let Some(c) = r.contest.as_deref().filter(|c| !c.adif.is_empty()) else {
        return super::adif_record_own_log(r);
    };
    let mut plain = r.clone();
    if let Some(block) = plain.contest.as_deref_mut() {
        block.adif.clear();
    }
    let mut out = super::adif_record_own_log(&plain);
    let at = out.rfind("<EOR>").unwrap_or(out.len());
    out.insert_str(
        at,
        &super::field(
            CONTEST_COLUMNS_TAG,
            &crate::contest::carrier::encode_pairs(&c.adif),
        ),
    );
    out
}

/// The record a history ADIF text describes ([`history_adif`]): the text read as the log reads
/// ADIF, with the contest columns [`CONTEST_COLUMNS_TAG`] holds put back where a record keeps
/// them, in their order, and the tag itself kept out of the record.
fn rebuilt(text: &str) -> Option<QsoRecord> {
    let mut f = fields(text);
    let columns = f
        .iter()
        .position(|(tag, _)| tag == CONTEST_COLUMNS_TAG)
        .map(|i| f.remove(i).1);
    let mut plain = String::new();
    for (tag, value) in &f {
        plain.push_str(&super::field(tag, value));
    }
    plain.push_str("<EOR>\n");
    let mut row = super::parse_adif(&plain).into_iter().next()?;
    if let Some(columns) = columns {
        let adif = crate::contest::carrier::decode_pairs(&columns);
        if !adif.is_empty() {
            row.contest.get_or_insert_with(Default::default).adif = adif;
        }
    }
    Some(row)
}

/// A contact's fields as the history sees them ([`history_adif`]), upload stamps left out.
fn content_fields(r: &QsoRecord) -> Vec<(String, String)> {
    let mut f = fields(&history_adif(r));
    f.retain(|(tag, _)| !UPLOAD_TAGS.contains(&tag.as_str()));
    f
}

/// A contact's revision as a caller that showed it compares it: a hash of every field the
/// operator's own log writes, upload stamps aside. Any change to what the contact holds — the
/// contest exchange, the station, the passthrough, a confirmation — moves it; a stamp landing does
/// not.
pub fn content_key(r: &QsoRecord) -> String {
    let mut text = String::new();
    for (tag, value) in content_fields(r) {
        text.push_str(&super::field(&tag, &value));
    }
    format!("{:016x}", super::id::fnv1a64(text.as_bytes()))
}

/// A contact's upload stamps, as the fragment a new contact's entry keeps them in: empty when it
/// carries none.
fn uploads_of(r: &QsoRecord) -> String {
    fields(&super::adif_record_own_log(r))
        .into_iter()
        .filter(|(tag, _)| UPLOAD_TAGS.contains(&tag.as_str()))
        .map(|(tag, value)| super::field(&tag, &value))
        .collect()
}

/// What changed between two versions of a contact, as the two fragments an edit stores — or
/// `None` when no field but an upload stamp did.
pub fn diff(old: &QsoRecord, new: &QsoRecord) -> Option<(String, String)> {
    let a = content_fields(old);
    let b = content_fields(new);
    let am: HashMap<&str, &str> = a.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let bm: HashMap<&str, &str> = b.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let mut before = String::new();
    let mut after = String::new();
    let tags: BTreeSet<&str> = am.keys().chain(bm.keys()).copied().collect();
    for tag in tags {
        let (x, y) = (am.get(tag), bm.get(tag));
        if x == y {
            continue;
        }
        if let Some(v) = x {
            before.push_str(&super::field(tag, v));
        }
        if let Some(v) = y {
            after.push_str(&super::field(tag, v));
        }
    }
    if before.is_empty() && after.is_empty() {
        return None;
    }
    // The contest columns ride every entry of a contact that has them, changed or not: the mark
    // of an entry this build recorded ([`old_format_kept`]).
    let columns = CONTEST_COLUMNS_TAG;
    if am.get(columns) == bm.get(columns) {
        if let Some(v) = am.get(columns) {
            before.push_str(&super::field(columns, v));
            after.push_str(&super::field(columns, v));
        }
    }
    Some((before, after))
}

/// Writes the history of one chunk of a change, inside the chunk's transaction, a contact at a
/// time: nothing is held beyond the contact being written, so a purge of the whole log costs the
/// memory of one of its rows, not of the log ([`super::sqlite::LogDb::apply`] reads the rows it
/// takes out a bounded chunk at a time). Its run's row is written with its first entry, so a
/// change that recorded nothing leaves no run either.
pub(crate) struct Recorder<'a> {
    conn: &'a Connection,
    note: &'a Note,
    /// The ids the chunk writes: a row taken out and written again is an edit, not a delete.
    written: &'a HashSet<RecordId>,
    run_id: Option<i64>,
}

impl<'a> Recorder<'a> {
    pub(crate) fn new(
        conn: &'a Connection,
        note: &'a Note,
        written: &'a HashSet<RecordId>,
    ) -> Recorder<'a> {
        Recorder {
            conn,
            note,
            written,
            run_id: None,
        }
    }

    /// `old`, as the store held it, taken out by the chunk.
    pub(crate) fn deleted(&mut self, old: &QsoRecord) -> Result<()> {
        match old.id {
            Some(id) if !self.written.contains(&id) => {
                self.entry(id, Kind::Delete, Some(&history_adif(old)), None)
            }
            _ => Ok(()),
        }
    }

    /// The rows the chunk writes, against `stored`, the rows the store held before it by id: a
    /// row it did not hold is new, one it did is an edit of the fields that changed.
    pub(crate) fn written(
        &mut self,
        stored: &HashMap<RecordId, QsoRecord>,
        upsert: &[RowWrite],
    ) -> Result<()> {
        for w in upsert {
            let Some(id) = w.rec.id else { continue };
            match stored.get(&id) {
                None => {
                    let uploads = uploads_of(&w.rec);
                    let uploads = (!uploads.is_empty()).then_some(uploads.as_str());
                    self.entry(id, Kind::Create, None, uploads)?
                }
                Some(old) => {
                    if let Some((before, after)) = diff(old, &w.rec) {
                        self.entry(id, self.note.edit, Some(&before), Some(&after))?;
                    }
                }
            }
        }
        Ok(())
    }

    fn entry(
        &mut self,
        id: RecordId,
        kind: Kind,
        before: Option<&str>,
        after: Option<&str>,
    ) -> Result<()> {
        let run_id = match self.run_id {
            Some(run_id) => run_id,
            None => {
                self.conn.execute(
                    "INSERT INTO history_run (run, at_ms, source) VALUES (?1, ?2, ?3)
                     ON CONFLICT(run) DO NOTHING",
                    params![self.note.run, self.note.at_ms, self.note.source.code()],
                )?;
                let run_id: i64 = self.conn.query_row(
                    "SELECT id FROM history_run WHERE run = ?1",
                    [&self.note.run],
                    |r| r.get(0),
                )?;
                *self.run_id.insert(run_id)
            }
        };
        self.conn
            .prepare_cached(
                "INSERT INTO qso_history (run_id, qso_id, kind, undoes, before, after)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?
            .execute(params![
                run_id,
                id.to_string(),
                kind.code(),
                self.note.undoes.get(&id),
                before,
                after
            ])?;
        Ok(())
    }
}

const SELECT: &str = "SELECT h.seq, h.qso_id, h.kind, r.run, r.at_ms, r.source, h.undoes, \
                      h.before, h.after FROM qso_history h JOIN history_run r ON r.id = h.run_id";

fn entry_of(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<Entry>> {
    let id: String = row.get(1)?;
    let Ok(qso_id) = id.parse::<RecordId>() else {
        return Ok(None);
    };
    Ok(Some(Entry {
        seq: row.get(0)?,
        qso_id,
        kind: Kind::of_code(row.get(2)?),
        run: row.get(3)?,
        at_ms: row.get(4)?,
        source: row.get(5)?,
        undoes: row.get(6)?,
        before: row.get(7)?,
        after: row.get(8)?,
    }))
}

fn entries(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> Result<Vec<Entry>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(p, entry_of)?;
    let mut out = Vec::new();
    for r in rows {
        out.extend(r?);
    }
    Ok(out)
}

/// One contact's history, oldest first.
pub(crate) fn of_qso(conn: &Connection, id: RecordId) -> Result<Vec<Entry>> {
    entries(
        conn,
        &format!("{SELECT} WHERE h.qso_id = ?1 ORDER BY h.seq"),
        [id.to_string()],
    )
}

/// One entry.
pub(crate) fn entry(conn: &Connection, seq: i64) -> Result<Option<Entry>> {
    Ok(entries(conn, &format!("{SELECT} WHERE h.seq = ?1"), [seq])?
        .into_iter()
        .next())
}

/// Every entry of one run, in the order they were made.
pub(crate) fn of_run(conn: &Connection, run: &str) -> Result<Vec<Entry>> {
    entries(
        conn,
        &format!("{SELECT} WHERE r.run = ?1 ORDER BY h.seq"),
        [run],
    )
}

/// The contacts deleted and not put back since, newest delete first: for each, its latest delete.
pub(crate) fn deleted(conn: &Connection, limit: usize) -> Result<Vec<Entry>> {
    entries(
        conn,
        &format!(
            "{SELECT} WHERE h.kind = 2 \
             AND NOT EXISTS (SELECT 1 FROM qso q WHERE q.id = h.qso_id) \
             AND h.seq = (SELECT MAX(x.seq) FROM qso_history x \
                          WHERE x.qso_id = h.qso_id AND x.kind = 2) \
             ORDER BY h.seq DESC LIMIT ?1"
        ),
        [i64::try_from(limit).unwrap_or(i64::MAX)],
    )
}

/// The contacts `run` added that a later change by another run touched since — an edit, a
/// merge, a contact added again under the same id (deleted, then imported again from a Nexus
/// export), or a change of a kind a later build wrote: anything but a delete — each with the latest
/// such change's place in the order. What an undo of the run must not take out without a word
/// ([`CannotUndo::ChangedSince`]). Not counted: an undo (a redo of the operator's own), and the
/// fill job's country and state and the copy of a contest column into its field
/// ([`Source::ContestColumns`]), which the log derives from the contact itself. An upload since is
/// not history: [`Entry::uploaded_since`] answers that.
pub(crate) fn changed_after(conn: &Connection, run: &str) -> Result<HashMap<RecordId, i64>> {
    let sql = format!(
        "SELECT h.qso_id, MAX(h.seq) FROM qso_history c \
         JOIN history_run cr ON cr.id = c.run_id \
         JOIN qso_history h ON h.qso_id = c.qso_id AND h.seq > c.seq \
         JOIN history_run r ON r.id = h.run_id \
         WHERE cr.run = ?1 AND c.kind = {create} AND h.kind <> {delete} AND r.run <> ?1 \
           AND r.source NOT IN ('{undo}', '{fill}', '{copy}') \
         GROUP BY h.qso_id",
        create = Kind::Create.code(),
        delete = Kind::Delete.code(),
        undo = Source::Undo.code(),
        fill = Source::Fill.code(),
        copy = Source::ContestColumns.code(),
    );
    ids_with_seq(conn, &sql, [run])
}

/// The entries of `run` that stand undone now: the latest undo linked to one is not itself
/// undone (a redo), and so on along the chain — an undo, its redo, an undo of that redo. What a
/// whole run's undo does not name as refused, as the operator already took those changes back
/// one at a time.
/// Read off each contact's own history: an undo is linked to an entry of the contact it changes.
pub(crate) fn undone_in(conn: &Connection, run: &str) -> Result<HashSet<i64>> {
    let mut latest: HashMap<i64, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT u.undoes, MAX(u.seq) FROM qso_history u \
             WHERE u.undoes IS NOT NULL AND u.qso_id IN ( \
               SELECT c.qso_id FROM qso_history c JOIN history_run cr ON cr.id = c.run_id \
               WHERE cr.run = ?1) \
             GROUP BY u.undoes",
        )?;
        let rows = stmt.query_map([run], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (undoes, seq) = row?;
            latest.insert(undoes, seq);
        }
    }
    let mut stmt = conn.prepare(
        "SELECT h.seq FROM qso_history h JOIN history_run r ON r.id = h.run_id WHERE r.run = ?1",
    )?;
    let seqs = stmt.query_map([run], |r| r.get::<_, i64>(0))?;
    let mut undone = HashSet::new();
    for seq in seqs {
        let seq = seq?;
        // Each link is later than what it undoes, so the chain ends; an odd length is undone.
        let (mut at, mut links) = (seq, 0usize);
        while let Some(&next) = latest.get(&at).filter(|&&next| next > at) {
            at = next;
            links += 1;
        }
        if links % 2 == 1 {
            undone.insert(seq);
        }
    }
    Ok(undone)
}

/// The contacts added to the log after the change `after_seq` — logged, imported, downloaded or
/// taken in — each with the latest such entry's place in the order. A contact an undo put back is
/// not one of them (its entry undoes another): it was in the log before. What a restore of a
/// contact deleted at `after_seq` or later may be blocked by: the same contact, logged again since
/// ([`CannotUndo::Duplicate`]). A contact the log held beside it before the delete is never here.
pub(crate) fn created_after(conn: &Connection, after_seq: i64) -> Result<HashMap<RecordId, i64>> {
    let sql = format!(
        "SELECT qso_id, MAX(seq) FROM qso_history \
         WHERE seq > ?1 AND kind = {create} AND undoes IS NULL GROUP BY qso_id",
        create = Kind::Create.code(),
    );
    ids_with_seq(conn, &sql, [after_seq])
}

fn ids_with_seq(
    conn: &Connection,
    sql: &str,
    p: impl rusqlite::Params,
) -> Result<HashMap<RecordId, i64>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(p, |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (id, seq) = row?;
        if let Ok(id) = id.parse::<RecordId>() {
            out.insert(id, seq);
        }
    }
    Ok(out)
}

/// How long a whole contact a delete kept stays in the history, by default: 90 days. An edit's
/// fields are kept for as long as the log is.
pub const SNAPSHOT_DAYS: u32 = 90;

/// Take out of the history, in one transaction of the caller's, up to `limit` of the whole
/// contacts deletes kept ([`Kind::Delete`]) that were made before `before_ms`, and the runs left
/// with no entry. How many it took out: fewer than `limit` means none is left to take.
///
/// Only a delete's entry holds a whole contact; every other kind — an edit's fields, a new
/// contact, a merge, a fill — is never touched, however old. A contact deleted that long ago is
/// then gone from Recently deleted, and an undo of the run that deleted it finds nothing to put
/// back.
pub(crate) fn trim(conn: &Connection, before_ms: i64, limit: usize) -> Result<usize> {
    let picked: Vec<(i64, i64)> = {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT h.seq, h.run_id FROM qso_history h JOIN history_run r ON r.id = h.run_id \
             WHERE h.kind = {delete} AND r.at_ms < ?1 ORDER BY h.seq LIMIT ?2",
            delete = Kind::Delete.code()
        ))?;
        let rows = stmt.query_map(
            params![before_ms, i64::try_from(limit).unwrap_or(i64::MAX)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut drop_entry = conn.prepare_cached("DELETE FROM qso_history WHERE seq = ?1")?;
    for (seq, _) in &picked {
        drop_entry.execute([seq])?;
    }
    let runs: BTreeSet<i64> = picked.iter().map(|(_, run)| *run).collect();
    let mut drop_run = conn.prepare_cached(
        "DELETE FROM history_run WHERE id = ?1 \
         AND NOT EXISTS (SELECT 1 FROM qso_history WHERE run_id = ?1)",
    )?;
    for run in runs {
        drop_run.execute([run])?;
    }
    Ok(picked.len())
}

/// A run, as a toast or a menu names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    /// Its id.
    pub run: String,
    /// Who made it.
    pub source: String,
    /// When.
    pub at_ms: i64,
    /// How many contacts it changed.
    pub contacts: u64,
    /// Whether undoing it now takes a contact out of the log: a contact it added is still there.
    pub removes: bool,
}

/// SQL: whether the run `r.id` added a contact the log still holds, which its undo would take out
/// ([`RunSummary::removes`]).
fn added_still_in() -> String {
    format!(
        "EXISTS (SELECT 1 FROM qso_history a WHERE a.run_id = r.id AND a.kind = {create} \
         AND EXISTS (SELECT 1 FROM qso q WHERE q.id = a.qso_id))",
        create = Kind::Create.code()
    )
}

/// The latest run made by `source` (any source when `None`) that changed a contact.
pub(crate) fn latest_run(conn: &Connection, source: Option<&str>) -> Result<Option<RunSummary>> {
    let found = conn
        .query_row(
            &format!(
                "SELECT r.run, r.source, r.at_ms, \
                        (SELECT COUNT(*) FROM qso_history h WHERE h.run_id = r.id), {removes} \
                 FROM history_run r \
                 WHERE (?1 IS NULL OR r.source = ?1) \
                   AND EXISTS (SELECT 1 FROM qso_history h WHERE h.run_id = r.id) \
                 ORDER BY r.id DESC LIMIT 1",
                removes = added_still_in()
            ),
            [source],
            |r| {
                Ok(RunSummary {
                    run: r.get(0)?,
                    source: r.get(1)?,
                    at_ms: r.get(2)?,
                    contacts: r.get::<_, i64>(3)?.max(0) as u64,
                    removes: r.get(4)?,
                })
            },
        )
        .optional()?;
    Ok(found)
}

/// The runs a whole-run undo can still be asked for after its toast has gone, newest first, at
/// most `limit`, made at `since_ms` or later: a purge (the operator's run of two or more deletes
/// and nothing else) with a contact it took out still out of the log, an import of two or more
/// contacts with a contact it added still in it, and a whole run's undo of two or more contacts
/// that took one out still out or put one back still in (undoing it is the redo). Read off the log
/// as it stands, so a run already undone, or put back a contact at a time, is not among them; a
/// purge whose deleted contacts the history no longer keeps ([`trim`]) is gone with them. A
/// companion program's one logged contact is an import of one, and an undo of one contact is one
/// too: neither is listed. The one-time copy of contest columns into their fields
/// ([`Source::ContestColumns`]) is listed, however few contacts it changed, while a contact it
/// changed is in the log and that change not undone, and counts those contacts: it is one change the
/// operator never asked for, and one Undo of it is the way back. No other fill is ever listed.
/// Every run counts distinct contacts.
pub(crate) fn undoable_runs(
    conn: &Connection,
    since_ms: i64,
    limit: usize,
) -> Result<Vec<RunSummary>> {
    let sql = format!(
        "SELECT run, source, at_ms, n, removes FROM ( \
           SELECT r.id, r.run, r.source, r.at_ms, \
                  CASE WHEN r.source = '{copy}' THEN \
                    (SELECT COUNT(DISTINCT h.qso_id) FROM qso_history h WHERE h.run_id = r.id \
                     AND EXISTS (SELECT 1 FROM qso q WHERE q.id = h.qso_id) \
                     AND NOT EXISTS (SELECT 1 FROM qso_history u \
                                     WHERE u.qso_id = h.qso_id AND u.undoes = h.seq)) \
                  ELSE (SELECT COUNT(DISTINCT c.qso_id) FROM qso_history c WHERE c.run_id = r.id) \
                  END AS n, \
                  {removes} AS removes, \
                  EXISTS (SELECT 1 FROM qso_history h WHERE h.run_id = r.id AND h.kind = {delete} \
                          AND NOT EXISTS (SELECT 1 FROM qso q WHERE q.id = h.qso_id)) AS puts_back \
           FROM history_run r \
           WHERE r.at_ms >= ?1 \
             AND r.source IN ('{operator}', '{import}', '{undo}', '{copy}')) r \
         WHERE (r.source = '{copy}' AND n > 0) OR (n >= 2 AND ( \
           (r.source = '{operator}' AND puts_back \
            AND NOT EXISTS (SELECT 1 FROM qso_history o WHERE o.run_id = r.id AND o.kind <> {delete})) \
           OR (r.source = '{import}' AND removes) \
           OR (r.source = '{undo}' AND (removes OR puts_back)))) \
         ORDER BY r.id DESC LIMIT ?2",
        removes = added_still_in(),
        operator = Source::Operator.code(),
        import = Source::Import.code(),
        undo = Source::Undo.code(),
        copy = Source::ContestColumns.code(),
        delete = Kind::Delete.code(),
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        params![since_ms, i64::try_from(limit).unwrap_or(i64::MAX)],
        |r| {
            Ok(RunSummary {
                run: r.get(0)?,
                source: r.get(1)?,
                at_ms: r.get(2)?,
                contacts: r.get::<_, i64>(3)?.max(0) as u64,
                removes: r.get(4)?,
            })
        },
    )?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// What undoing an entry does to its contact, as it now stands.
#[derive(Debug, Clone, PartialEq)]
pub enum Undo {
    /// Take the contact out: the entry added it.
    Remove(RecordId),
    /// Put the contact back, whole: the entry deleted it.
    Restore(QsoRecord),
    /// The contact, with the entry's fields put back where they still hold what it wrote.
    /// `kept` names the fields changed again since, which keep the later value; `held` the fields
    /// left as they are because putting them back would put the contact under another call.
    Revert {
        /// The contact as the undo leaves it.
        row: QsoRecord,
        /// Fields left as they are, because a later change moved them.
        kept: Vec<String>,
        /// Fields left as they are, because putting them back would move the contact under a
        /// call it was not made under: a blank STATION_CALLSIGN, while the call in Settings is
        /// another call than the one it holds.
        held: Vec<String>,
    },
}

/// Why an entry cannot be undone now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CannotUndo {
    /// The contact is no longer in the log.
    Gone,
    /// The contact is in the log already: there is nothing to put back.
    AlreadyThere,
    /// Every field the entry changed has been changed again since — or, for a contact the entry
    /// added, the contact has been edited or confirmed since, and stays.
    ChangedSince,
    /// The contact it would put back was logged again after it was deleted, under another id: a
    /// contact added since ([`created_after`]) with the same station, band and mode within the
    /// log's duplicate window ([`super::dedup::is_recent_duplicate`]). Put back, it would be a
    /// duplicate. The operator can still put it back anyway.
    Duplicate,
    /// The entry is of a kind this build does not know ([`Kind::Unknown`]).
    UnknownKind,
}

/// What undoing `entry` does to its contact, `current` — the contact as the log now holds it, or
/// `None` where it holds none. `settings_call`: the call in Settings, the one a contact naming no
/// station call is uploaded under.
pub fn undo(
    entry: &Entry,
    current: Option<&QsoRecord>,
    settings_call: Option<&str>,
) -> std::result::Result<Undo, CannotUndo> {
    match entry.kind {
        Kind::Create => match current {
            Some(_) => Ok(Undo::Remove(entry.qso_id)),
            None => Err(CannotUndo::Gone),
        },
        Kind::Delete => match current {
            Some(_) => Err(CannotUndo::AlreadyThere),
            None => entry
                .deleted_contact()
                .map(Undo::Restore)
                .ok_or(CannotUndo::Gone),
        },
        Kind::Edit | Kind::Merge | Kind::Fill => {
            let current = current.ok_or(CannotUndo::Gone)?;
            revert(entry, current, settings_call)
        }
        Kind::Unknown(_) => Err(CannotUndo::UnknownKind),
    }
}

/// Whether an entry an older build recorded ([`CONTEST_COLUMNS_TAG`] absent from it) changed
/// `tag` where the contact now holds a contest column of that name. Such a build wrote the column
/// under the plain tag, in place of the field, so its entry for the tag may describe either: the
/// field is left as it is, as a field changed since, rather than given a value that was perhaps
/// the column's.
fn old_format_kept(tag: &str, current: &QsoRecord) -> bool {
    current
        .contest
        .as_deref()
        .is_some_and(|c| c.adif.iter().any(|(t, v)| t == tag && !v.trim().is_empty()))
}

/// How the History names a contest column ([`CONTEST_COLUMNS_TAG`] holds them all): `TAG
/// (contest)`, beside the record's own field of that tag.
pub fn contest_column_label(tag: &str) -> String {
    format!("{tag} (contest)")
}

/// [`revert`] of the contest columns ([`CONTEST_COLUMNS_TAG`]), a column at a time: each tag the
/// entry changed goes back where its values are still what the entry wrote, and is kept, named
/// ([`contest_column_label`]), where a later change moved it; a tag the entry did not change is
/// left as it is now. So a later change of one column (a State edit's STATE, say) never makes the
/// undo of another column's change drop it. Whether any column went back.
fn revert_columns(
    change: &FieldChange,
    now: &mut Vec<(String, String)>,
    kept: &mut Vec<String>,
) -> bool {
    use crate::contest::carrier::{decode_pairs, encode_pairs};
    let pairs = |v: Option<&str>| decode_pairs(v.unwrap_or(""));
    let at = now.iter().position(|(tag, _)| tag == CONTEST_COLUMNS_TAG);
    let (old, new) = (pairs(change.old.as_deref()), pairs(change.new.as_deref()));
    let mut columns = pairs(at.map(|i| now[i].1.as_str()));
    let values = |pairs: &[(String, String)], tag: &str| -> Vec<String> {
        pairs
            .iter()
            .filter(|(t, _)| t == tag)
            .map(|(_, v)| v.clone())
            .collect()
    };
    let mut tags: Vec<&str> = Vec::new();
    for (t, _) in old.iter().chain(&new) {
        if !tags.contains(&t.as_str()) {
            tags.push(t);
        }
    }
    let mut back = Vec::new();
    for tag in tags {
        let wrote = values(&new, tag);
        if values(&old, tag) == wrote {
            continue;
        }
        if values(&columns, tag) == wrote {
            back.push(tag);
        } else {
            kept.push(contest_column_label(tag));
        }
    }
    if back.is_empty() {
        return false;
    }
    if columns == new {
        // Nothing changed since: the columns as they were, in their order.
        columns = old;
    } else {
        for tag in back {
            let first = columns.iter().position(|(t, _)| t == tag);
            columns.retain(|(t, _)| t != tag);
            let was: Vec<(String, String)> =
                old.iter().filter(|(t, _)| t == tag).cloned().collect();
            // In its place: where it is now, else after the column it followed in the entry's
            // `old`, else first.
            let place = first.unwrap_or_else(|| {
                let before = old.iter().position(|(t, _)| t == tag).unwrap_or(0);
                old[..before]
                    .iter()
                    .rev()
                    .find_map(|(p, _)| columns.iter().rposition(|(t, _)| t == p))
                    .map_or(0, |i| i + 1)
            });
            let place = place.min(columns.len());
            columns.splice(place..place, was);
        }
    }
    match (at, columns.is_empty()) {
        (Some(i), true) => {
            now.remove(i);
        }
        (Some(i), false) => now[i].1 = encode_pairs(&columns),
        (None, true) => {}
        (None, false) => now.push((CONTEST_COLUMNS_TAG.to_string(), encode_pairs(&columns))),
    }
    true
}

/// [`undo`] of an edit: each changed field back where it still holds what the edit wrote.
///
/// **An edit of the callsign, undone.** A corrected call ([`super::edited`]) clears the
/// contact's confirmations, credits, entity and state, its upload stamps and its
/// `LOTW_QSL_SENT`, and the edit records each of those fields but the stamps, which are never
/// history. Its undo therefore puts the call back with the confirmations and credits the old call
/// held, and leaves the stamps cleared and `LOTW_QSL_SENT` off: the contact is owed again to every
/// service under the call it now carries, and goes with the next upload. A service that already
/// holds it under that call answers that upload as a duplicate; nothing is claimed sent that was
/// not. Deliberate: the stamps belonged to the contact as it stood before
/// the undo, and a stamp is only ever written by a service's answer. (The app, which holds the
/// upload queue, puts a correction's marks back service by service, where it can show that the
/// service was not sent the corrected call: `tempo_app::loghistory`'s header.)
///
/// **A change of the station call, undone,** is the same: the call the contact is uploaded under
/// (its STATION_CALLSIGN, else `settings_call`, the call in Settings) moves back, so the stamps of
/// uploads made under the call it no longer carries are cleared with `LOTW_QSL_SENT`. The stamps
/// the change itself cleared are not put back — they were never history — so the next upload sends
/// the contact under the restored call, and a service already holding that QSO answers Duplicate.
/// A blank station call filled with the Settings call, undone, keeps its stamps: the call it is
/// uploaded under never moved. Undoing a clear of a station call, or redoing an undone fill, while
/// another call is in Settings does clear them, though the clear or the fill itself cleared none:
/// the contact moves back from the Settings call to its own, and goes again under its own call,
/// which the services answer as a duplicate. Keep it so: an upload under the Settings call may have
/// been made in between, and keeping its stamps would leave the contact never sent under its own.
///
/// **⛔ An undo never puts back a blank station call that would move the contact under another
/// call.** A contact naming none goes out under the call in Settings NOW, and nothing records the
/// call a blank contact was uploaded under. Old contacts uploaded under KA1OLD, filled with KA1OLD,
/// and the fill undone after Settings moved to KD9NEW would be owed to every service under KD9NEW,
/// and LoTW cannot take back a QSO it accepted. So where the blank would land the contact under
/// another call than the one its STATION_CALLSIGN names now, that call stays, with its stamps, and
/// the field is `held`. The Settings call the edit was made under is not what decides: in that
/// example it was KD9NEW too. Only the call the contact holds now is known to be one it was made
/// under. Putting back a call that is named is always allowed: it is a call the operator wrote.
fn revert(
    entry: &Entry,
    current: &QsoRecord,
    settings_call: Option<&str>,
) -> std::result::Result<Undo, CannotUndo> {
    let mut now = fields(&history_adif(current));
    let mut kept = Vec::new();
    let mut moved = false;
    let changes = entry.changes();
    let marked = changes.iter().any(|c| c.tag == CONTEST_COLUMNS_TAG);
    for change in changes {
        // The contest columns as they were and stayed: the entry's mark, not a change.
        if change.old == change.new {
            continue;
        }
        if change.tag == CONTEST_COLUMNS_TAG {
            moved |= revert_columns(&change, &mut now, &mut kept);
            continue;
        }
        if !marked && old_format_kept(&change.tag, current) {
            kept.push(change.tag);
            continue;
        }
        let at = now.iter().position(|(tag, _)| *tag == change.tag);
        let holds = at.map(|i| now[i].1.clone());
        if holds != change.new {
            kept.push(change.tag);
            continue;
        }
        moved = true;
        match (at, change.old) {
            (Some(i), Some(old)) => now[i].1 = old,
            (Some(i), None) => {
                now.remove(i);
            }
            (None, Some(old)) => now.push((change.tag, old)),
            (None, None) => {}
        }
    }
    if !moved {
        return Err(CannotUndo::ChangedSince);
    }
    let mut text = String::new();
    for (tag, value) in &now {
        text.push_str(&super::field(tag, value));
    }
    text.push_str("<EOR>\n");
    let mut row = rebuilt(&text).ok_or(CannotUndo::ChangedSince)?;
    row.id = current.id;
    let station = |r: &QsoRecord| super::cleanup::station_call(r, settings_call);
    let mut held = Vec::new();
    let named = |r: &QsoRecord| super::cleanup::station_call(r, None).is_some();
    if !named(&row) && named(current) && station(&row) != station(current) {
        row.station_callsign = current.station_callsign.clone();
        held.push("STATION_CALLSIGN".to_string());
    }
    // A stamp is never history, so it is never undone: the contact keeps the stamps it has —
    // unless the undo corrects its call, or the station call it is uploaded under, which owes it
    // to every service again, as a corrected call always has (`edited`), with the LoTW mark
    // another logger left in it.
    if row.call.trim().eq_ignore_ascii_case(current.call.trim())
        && station(&row) == station(current)
    {
        row.upload = current.upload.clone();
    } else {
        row.upload = super::UploadState::default();
        row.extra.retain(|(k, _)| k != "LOTW_QSL_SENT");
    }
    Ok(Undo::Revert { row, kept, held })
}

#[cfg(test)]
mod tests;
