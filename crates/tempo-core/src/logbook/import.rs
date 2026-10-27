//! An import, looked at before it is made and accounted for after: the file read in any of the
//! three formats a logger hands another ([`Format`]), its [`Preview`], and the no-loss
//! [`Report`].
//!
//! # One reader for the import, the preview and the report
//!
//! Every format comes to the log as ADIF: an `.adi` file as itself, an ADX file ([`adx`]) and a
//! Cabrillo log ([`cabrillo`]) as the ADIF their fields make. The import itself is the log's own
//! ([`super::Logbook::import_adif`]), unchanged. What is new is that the ADIF tokenizer the import
//! reads with is THIS module's ([`adif_records`]), so the preview and the report read the same
//! records, the same fields, and the same "last one wins" for a tag repeated in one record, as the
//! import does. A count the preview shows is the count the import will meet.
//!
//! # What "no loss" is measured against
//!
//! A field's fate ([`Fate`]) is not looked up in a hand-kept table of what the parser models. It
//! is MEASURED: each record is read as the import reads it ([`super::record_from`]) and written
//! back as the operator's own export writes it ([`super::adif_record_own_log`]), and each source
//! field is then found in what came back. Byte-equal is [`Fate::Stored`] (or
//! [`Fate::Passthrough`], for a field the log keeps verbatim without modelling it); present with
//! another spelling (`20M` → `20m`, `1234` → `123400`) is [`Fate::Normalized`]; absent is
//! [`Fate::Dropped`], with the reason. So a field this build silently lost would show up as a
//! dropped field in every report, rather than be missing from a list.
//!
//! The fates are "as the log stores the record as a new contact". A record the log already holds
//! (its own duplicate rule, [`super::dedup_key`]) is not stored again: the import upgrades the held
//! contact from it (confirmations, credits), and the report counts those records apart.

pub mod adx;
pub mod cabrillo;

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::QsoRecord;

/// The formats an import reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// ADIF's tagged text (`.adi`).
    Adif,
    /// ADIF's XML form (`.adx`).
    Adx,
    /// A Cabrillo contest log (`QSO:` lines).
    Cabrillo,
}

/// The format a file's text is in, read from the text itself: an XML declaration or an `<ADX>`
/// root is ADX, a first line `START-OF-LOG:` is Cabrillo, and anything else is ADIF (whose
/// header is free text, so it is the one format with no signature).
pub fn detect(text: &str) -> Format {
    let head = text.trim_start_matches('\u{feff}').trim_start();
    // By bytes: the text's first characters may be any, and a signature is ASCII.
    let starts = |s: &str, p: &str| {
        s.as_bytes()
            .get(..p.len())
            .is_some_and(|h| h.eq_ignore_ascii_case(p.as_bytes()))
    };
    if starts(head, "<?xml") || starts(head, "<ADX") {
        Format::Adx
    } else if head
        .lines()
        .next()
        .is_some_and(|l| starts(l.trim(), "START-OF-LOG:"))
    {
        Format::Cabrillo
    } else {
        Format::Adif
    }
}

// ── the ADIF tokenizer ─────────────────────────────────────────────────────────────────────────

/// One ADIF record as the tokenizer read it: its fields in file order, a repeated tag included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawRecord<'a> {
    /// Empty when the record is `over` the limit it was read with.
    pub(crate) fields: Vec<(String, String)>,
    /// A record past the limit it was read with ([`Fields`]): its fields, counted by name.
    pub(crate) over: Option<Over>,
    /// The record's own text: after the previous `<EOR>` (or the header) through its `<EOR>`.
    pub(crate) span: &'a str,
    /// The fields whose stated length ended inside a character or past the end of the text,
    /// read as empty.
    pub(crate) cut: Vec<String>,
}

/// An ADIF text, tokenized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdifRecords<'a> {
    pub(crate) records: Vec<RawRecord<'a>>,
    /// Fields after the last `<EOR>`: a record the file never ended. The import has never read
    /// one; the preview says so.
    pub(crate) tail: Fields,
    /// The header's own fields (`ADIF_VER`, `PROGRAMID`…): about the file, not a contact.
    pub(crate) header: Fields,
}

/// The records an ADIF text holds, as the import reads them: the body after `<EOH>` (the whole
/// text when there is none), each `<NAME:len[:type]>` read as `len` bytes of value, the name
/// upper-cased, and a record ended by `<EOR>`. A length that does not parse is `0`; a length past
/// the end clamps (it comes from the file, and must never wrap the scan backwards).
pub(crate) fn adif_records(text: &str) -> AdifRecords<'_> {
    let mut records = Vec::new();
    let (tail, header) = adif_each(text, usize::MAX, |r| records.push(r));
    AdifRecords {
        records,
        tail,
        header,
    }
}

/// [`adif_records`], handed over one record at a time, and each record (the header and the tail
/// too) read with at most `limit` fields ([`Fields`]). Answers the tail and the header.
pub(crate) fn adif_each<'a>(
    text: &'a str,
    limit: usize,
    mut each: impl FnMut(RawRecord<'a>),
) -> (Fields, Fields) {
    let (header, body) = match text.to_ascii_uppercase().find("<EOH>") {
        Some(i) => (&text[..i], &text[i + 5..]),
        None => ("", text),
    };
    let mut cur = Fields::new(limit);
    let mut cut: Vec<String> = Vec::new();
    // Where the current record's text begins: always just after an ASCII `>`, so a char boundary.
    let mut start = 0;
    scan(body, |name, val, at| match name {
        None => {
            let fields = std::mem::replace(&mut cur, Fields::new(limit));
            each(RawRecord {
                fields: fields.list,
                over: fields.over,
                span: &body[start..at],
                cut: std::mem::take(&mut cut),
            });
            start = at;
        }
        Some(name) => {
            let cut_name = val.is_none().then(|| name.clone());
            cur.push(name, val.unwrap_or_default());
            // A record over the limit is not imported, so a value cut in it is not noted.
            if cur.over.is_some() {
                cut.clear();
            } else if let Some(name) = cut_name {
                cut.push(name);
            }
        }
    });
    let mut head = Fields::new(limit);
    scan(header, |name, val, _| {
        if let Some(name) = name {
            head.push(name, val.unwrap_or_default());
        }
    });
    (cur, head)
}

/// A record's fields as a reader reads them, held to a limit. Once a record passes it, its fields
/// are no longer kept but counted, by name (`over`): such a record is not imported
/// ([`MAX_FIELDS`]), and a record built to be huge then costs its distinct names, not its fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fields {
    limit: usize,
    /// The fields in file order; empty once the record is `over`.
    pub(crate) list: Vec<(String, String)>,
    pub(crate) over: Option<Over>,
}

/// A record past its reader's limit: every field it had, counted by name, and its first few as
/// `NAME=value` text, to show which record it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Over {
    pub(crate) names: BTreeMap<String, usize>,
    pub(crate) head: String,
}

impl Fields {
    pub(crate) fn new(limit: usize) -> Self {
        Fields {
            limit,
            list: Vec::new(),
            over: None,
        }
    }

    pub(crate) fn push(&mut self, name: String, value: String) {
        if let Some(over) = &mut self.over {
            *over.names.entry(name).or_default() += 1;
        } else if self.list.len() < self.limit {
            self.list.push((name, value));
        } else {
            let head = field_list(&self.list);
            let mut names = BTreeMap::new();
            for (k, _) in std::mem::take(&mut self.list) {
                *names.entry(k).or_default() += 1;
            }
            *names.entry(name).or_default() += 1;
            self.over = Some(Over { names, head });
        }
    }

    /// How many fields were read, counted ones too.
    pub(crate) fn len(&self) -> usize {
        self.list.len() + self.over.as_ref().map_or(0, |o| o.names.values().sum())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.list.is_empty() && self.over.is_none()
    }

    /// The fields as a note's text.
    fn text(&self) -> String {
        self.over
            .as_ref()
            .map_or_else(|| field_list(&self.list), |o| o.head.clone())
    }
}

/// The scan both halves of a file share: `field(Some(name), value, _)` for each field — `value`
/// `None` when its length ends inside a character or past the end — and `field(None, _, at)` at
/// each `<EOR>`, `at` just after it.
fn scan(body: &str, mut field: impl FnMut(Option<String>, Option<String>, usize)) {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let end = match body[i..].find('>') {
            Some(e) => i + e,
            None => break,
        };
        let tag = &body[i + 1..end];
        i = end + 1;
        if tag.eq_ignore_ascii_case("EOR") {
            field(None, None, i);
            continue;
        }
        // NAME:len or NAME:len:type
        let mut parts = tag.splitn(3, ':');
        let name = parts.next().unwrap_or("").to_ascii_uppercase();
        let len: usize = parts
            .next()
            .and_then(|l| l.trim().parse().ok())
            .unwrap_or(0);
        let end = i.saturating_add(len);
        let val = body.get(i..end).map(str::to_string);
        i = end;
        field(Some(name), val, i);
    }
}

/// `fields` as ADIF text the import reads: one record each, every length in bytes (the
/// tokenizer's unit). What an ADX file and a Cabrillo log come to the log as.
pub(crate) fn adif_text(records: &[SourceRecord]) -> String {
    let mut out = String::from("<EOH>\n");
    for r in records {
        for (name, value) in &r.fields {
            out.push_str(&format!("<{name}:{}>{value}", value.len()));
        }
        out.push_str("<EOR>\n");
    }
    out
}

// ── a file read ────────────────────────────────────────────────────────────────────────────────

/// One record of the source, as ADIF fields, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    /// Where it is in the file, 1-based: the record's number (ADIF, ADX) or the line's (Cabrillo).
    pub at: usize,
    pub fields: Vec<(String, String)>,
}

/// What a reader noticed. Grouped by kind in the [`Preview`] and the [`Report`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NoteKind {
    /// A record with no `CALL`: not a contact, so not imported.
    NoCall,
    /// Fields after the last `<EOR>`: a record the file never ended, not imported.
    Unterminated,
    /// A tag twice in one record: the last value is the one read.
    Repeated,
    /// A value whose stated length ends inside a character or past the end: read as empty.
    CutValue,
    /// No readable `QSO_DATE`: the contact is dated 1970-01-01.
    BadDate,
    /// A `TIME_ON` that is not a time: the contact's time is unknown.
    BadTime,
    /// A modelled field whose value is not of its type (a `FREQ` that is not a number…): dropped.
    BadValue,
    /// A band no band plan names.
    UnknownBand,
    /// A mode ADIF does not define, or none.
    UnknownMode,
    /// A Cabrillo line that is not a readable `QSO:` line: not imported.
    UnreadableLine,
    /// A Cabrillo line whose columns do not fit its contest's template: read as an unknown
    /// contest's line.
    TemplateMismatch,
    /// A Cabrillo `CONTEST` with no template here: call, band, mode, time and an RST that looks
    /// like one are read, and the exchange is kept as text.
    UnknownContest,
    /// A Cabrillo `DG` line whose contest names no one mode: the mode is left blank.
    ModeUnresolved,
    /// A Cabrillo `X-QSO:` line (left out of the contest score): imported as a contact.
    ExcludedQso,
    /// ADX markup that is not a field (a field inside a field, an unclosed element).
    BadXml,
    /// A record with more than [`MAX_FIELDS`] fields: not imported, none of its fields.
    TooManyFields,
}

/// One thing noticed: where, and the source text it is about (`QSO_DATE=2023-13-45`). The text
/// is the file's own, in no language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub kind: NoteKind,
    pub at: usize,
    pub text: String,
}

/// Why a part of the source has no field of its own in the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UnmappedReason {
    /// About the file or the entry, not one contact (an ADIF/ADX header field, a Cabrillo header
    /// line).
    Header,
    /// A Cabrillo exchange column no ADIF field holds: kept in `STX_STRING`/`SRX_STRING` with
    /// the rest of its side's exchange.
    ExchangeText,
    /// Cabrillo's transmitter id: ADIF has no field for it.
    TransmitterId,
    /// An ADX field whose name ADIF's tags cannot carry.
    FieldName,
}

/// A part of the source the import does not store as a field of a contact, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unmapped {
    /// The header field or line, or the exchange column (`PREC`), as the file names it.
    pub what: String,
    /// For an exchange column: `"sent"` or `"rcvd"`.
    pub side: Option<&'static str>,
    pub count: usize,
    pub reason: UnmappedReason,
}

/// A file read, in any format, as the records the import will read.
#[derive(Debug, Clone)]
pub struct Source {
    pub format: Format,
    /// The records the import reads: not the ones over [`MAX_FIELDS`].
    pub records: Vec<SourceRecord>,
    /// Grouped as they are read, so a file of a million problems holds a few of each.
    pub notes: Vec<NoteGroup>,
    pub unmapped: Vec<Unmapped>,
    /// Cabrillo: the contest whose template read the lines (its ADIF `CONTEST_ID`).
    pub template: Option<String>,
    /// The fields after the last `<EOR>` of an ADIF file.
    tail: Fields,
    /// The records over [`MAX_FIELDS`]: how many, and all their fields, counted by name.
    over: Records,
    /// The ADIF the import reads, when it is not the file itself: what a format that is not ADIF
    /// makes, or an ADIF file without its records over the limit.
    adif: Option<String>,
}

impl Source {
    /// The ADIF text the import reads: the file itself for ADIF, else what its fields make.
    pub fn adif<'a>(&'a self, text: &'a str) -> &'a str {
        self.adif.as_deref().unwrap_or(text)
    }

    /// The ADIF its fields make, for a format that is not ADIF already (`None` for an ADIF file,
    /// whose text is its own), with the records let go.
    pub fn into_adif(self) -> Option<String> {
        self.adif
    }

    /// Whether the file names more than `max` distinct fields: in its records, those left out,
    /// its tail, and what it lists as not stored (a header's fields, a refused name, a Cabrillo
    /// header tag). Stops counting past `max`.
    fn names_more_than(&self, max: usize) -> bool {
        let mut seen: HashSet<&str> = HashSet::new();
        self.records
            .iter()
            .flat_map(|r| &r.fields)
            .chain(&self.tail.list)
            .map(|(k, _)| k)
            .chain(self.tail.over.iter().flat_map(|o| o.names.keys()))
            .chain(self.over.names.keys())
            .chain(self.unmapped.iter().map(|u| &u.what))
            .any(|k| seen.insert(k) && seen.len() > max)
    }
}

/// Read `text` as `format`.
pub fn read(text: &str, format: Format) -> Source {
    match format {
        Format::Adif => read_adif(text),
        Format::Adx => {
            let (mut records, notes, unmapped) = adx::read(text);
            let list = std::mem::take(&mut records.list);
            Source {
                format,
                adif: Some(adif_text(&list)),
                records: list,
                notes: notes.0,
                unmapped,
                template: None,
                tail: Fields::new(MAX_FIELDS),
                over: records,
            }
        }
        Format::Cabrillo => {
            // A line's fields are the template's columns, a bounded set: never near the limit.
            let c = cabrillo::read(text);
            Source {
                format,
                adif: Some(adif_text(&c.records)),
                records: c.records,
                notes: c.notes.0,
                unmapped: c.unmapped,
                template: c.template,
                tail: Fields::new(MAX_FIELDS),
                over: Records::default(),
            }
        }
    }
}

/// The most fields one record is read with: 2,000. ADIF defines a few hundred fields, and a
/// logger's own (`APP_…`) and user-defined ones add tens, so a real contact carries far fewer; a
/// record with more is damaged or built to be slow, and is not imported at all. Keeping its
/// first fields would store a contact the file itself overrides: a tag repeated later in the
/// record is the value the file means (the last one wins), and a `CALL` past the limit would
/// leave the record with another station's, or with none. The preview and the report say so
/// ([`NoteKind::TooManyFields`], [`DropReason::TooManyFields`]), and the report accounts for
/// every one of its fields. Past the limit a reader keeps only a count of each name
/// ([`Fields`]), so such a record does not cost memory by its size.
pub const MAX_FIELDS: usize = 2_000;

/// The largest file an import reads: 200 MB.
///
/// Measured on a 61 MB ADIF file of 143,906 contacts (about 420 bytes each): reading it and making
/// its preview peaks at about 280 MB above the text, and planning the import parses it again, so
/// an import holds several times the file at once. 200 MB is about 470,000 contacts, more than a
/// lifetime log of a busy station, and still well inside a desktop's memory at that multiple. A
/// larger file can be split and imported in parts; each part is its own run, undone as one.
pub const MAX_BYTES: usize = 200_000_000;

/// The most distinct field names a file is read with: 10,000. ADIF defines a few hundred, and a
/// logger's own and user-defined ones add tens, so a real file names far fewer. Each name costs
/// the preview and the report a row, so a file naming millions is built to exhaust memory, and is
/// refused before its preview, as one larger than [`MAX_BYTES`] is ([`TOO_MANY_NAMES`]).
pub const MAX_NAMES: usize = 10_000;

/// Why [`read_file`] refuses a file naming more than [`MAX_NAMES`] fields: a key the UI words in
/// the operator's language.
pub const TOO_MANY_NAMES: &str = "tooManyNames";

/// A file's text read in the format it is in ([`detect`], [`read`]), or why it is not read: it is
/// larger than [`MAX_BYTES`], or names more than [`MAX_NAMES`] fields.
pub fn read_file(text: &str) -> Result<Source, String> {
    if text.len() > MAX_BYTES {
        return Err(format!(
            "This file is {:.1} MB, and an import reads files of up to {} MB. Split it and import the parts.",
            text.len() as f64 / 1e6,
            MAX_BYTES / 1_000_000
        ));
    }
    let source = read(text, detect(text));
    if source.names_more_than(MAX_NAMES) {
        return Err(TOO_MANY_NAMES.to_string());
    }
    Ok(source)
}

fn read_adif(text: &str) -> Source {
    let mut notes = Notes::default();
    let mut records = Records::default();
    // The text of each record over the limit, for the import to read the file without it.
    let mut left_out: Vec<&str> = Vec::new();
    let (tail, header) = adif_each(text, MAX_FIELDS, |r| {
        let at = records.next_at();
        for name in r.cut {
            notes.push(Note {
                kind: NoteKind::CutValue,
                at,
                text: name,
            });
        }
        if !records.add(at, r.fields, r.over, &mut notes) {
            left_out.push(r.span);
        }
    });
    if !tail.is_empty() {
        notes.push(Note {
            kind: NoteKind::Unterminated,
            at: records.next_at(),
            text: tail.text(),
        });
    }
    Source {
        format: Format::Adif,
        adif: (!left_out.is_empty()).then(|| without(text, &left_out)),
        records: std::mem::take(&mut records.list),
        notes: notes.0,
        unmapped: header_unmapped(&header),
        template: None,
        tail,
        over: records,
    }
}

/// `text` without the `spans` (slices of it, in order): the file the import reads when records
/// are left out of it. Every other byte stays, so each record left in is read as before: a
/// record's span runs from just after the `<EOR>` before it through its own, and the tokenizer
/// carries nothing across an `<EOR>`.
fn without(text: &str, spans: &[&str]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    for span in spans {
        // A span is a slice of `text`, so its place in it is the distance between their starts.
        let at = span.as_ptr() as usize - text.as_ptr() as usize;
        out.push_str(&text[from..at]);
        from = at + span.len();
    }
    out.push_str(&text[from..]);
    out
}

/// The records a reader keeps, and those over [`MAX_FIELDS`], which it does not: only counted,
/// with all their fields counted by name.
#[derive(Debug, Clone, Default)]
pub(crate) struct Records {
    pub(crate) list: Vec<SourceRecord>,
    over_records: usize,
    names: BTreeMap<String, usize>,
}

impl Records {
    /// The number the next record read has.
    pub(crate) fn next_at(&self) -> usize {
        self.list.len() + self.over_records + 1
    }

    /// A record read: kept, or — over the limit — counted and noted. Whether it was kept.
    pub(crate) fn add(
        &mut self,
        at: usize,
        fields: Vec<(String, String)>,
        over: Option<Over>,
        notes: &mut Notes,
    ) -> bool {
        let Some(over) = over else {
            self.list.push(SourceRecord { at, fields });
            return true;
        };
        notes.push(Note {
            kind: NoteKind::TooManyFields,
            at,
            text: over.head,
        });
        for (name, n) in over.names {
            *self.names.entry(name).or_default() += n;
        }
        self.over_records += 1;
        false
    }
}

/// What a reader noticed, grouped as it is noticed ([`group`]).
#[derive(Debug, Default)]
pub(crate) struct Notes(pub(crate) Vec<NoteGroup>);

impl Notes {
    pub(crate) fn push(&mut self, n: Note) {
        group(&mut self.0, n);
    }
}

/// A header's fields, each listed once as about the file (past the limit, by name).
pub(crate) fn header_unmapped(header: &Fields) -> Vec<Unmapped> {
    let mut out = Listed::default();
    for (name, _) in &header.list {
        out.add(name, None, UnmappedReason::Header);
    }
    for (name, n) in header.over.iter().flat_map(|o| &o.names) {
        out.add_n(name, None, UnmappedReason::Header, *n);
    }
    out.list
}

/// The parts of a source listed as not stored, each once with its count, in the order first met.
/// Indexed, so a file naming many of them is not searched once per name.
#[derive(Default)]
pub(crate) struct Listed {
    pub(crate) list: Vec<Unmapped>,
    index: HashMap<(String, Option<&'static str>, UnmappedReason), usize>,
}

impl Listed {
    pub(crate) fn add(&mut self, what: &str, side: Option<&'static str>, reason: UnmappedReason) {
        self.add_n(what, side, reason, 1);
    }

    fn add_n(&mut self, what: &str, side: Option<&'static str>, reason: UnmappedReason, n: usize) {
        let next = self.list.len();
        let i = *self
            .index
            .entry((what.to_string(), side, reason))
            .or_insert(next);
        if i == next {
            self.list.push(Unmapped {
                what: what.to_string(),
                side,
                count: 0,
                reason,
            });
        }
        self.list[i].count += n;
    }
}

/// A few fields as `NAME=value` text, for a note.
fn field_list(fields: &[(String, String)]) -> String {
    fields
        .iter()
        .take(4)
        .map(|(k, v)| format!("{k}={}", clip(v)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A value shortened for a note: the first 40 characters.
fn clip(v: &str) -> String {
    let mut s: String = v.chars().take(40).collect();
    if s.len() < v.len() {
        s.push('…');
    }
    s
}

// ── a field's fate ─────────────────────────────────────────────────────────────────────────────

/// What became of one source field when its record is stored and written back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fate {
    /// Modelled, and written back byte for byte.
    Stored,
    /// Modelled, and written back in the log's spelling: the value it is written as.
    Normalized(String),
    /// Not modelled, kept verbatim, and written back byte for byte.
    Passthrough,
    /// Not written back, for the reason given.
    Dropped(DropReason),
}

/// Why a field is not written back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DropReason {
    /// The record has no `CALL`, so it is not a contact and is not imported.
    NoContact,
    /// After the last `<EOR>`: the file never ended the record, and it is not imported.
    Unterminated,
    /// The tag appears again later in the same record, and the later value is the one read.
    Repeated,
    /// The value is empty.
    Empty,
    /// The value is not of the field's type (a `FREQ` that is not a number, an `IOTA` that is not
    /// a reference), so the field is not stored.
    Invalid,
    /// The field states the log's default (`N` — not sent, not received), which the log keeps as
    /// the absence of a mark and does not write back.
    Default,
    /// The mode is stored once, as the mode itself, and written back as `MODE` (and `SUBMODE`
    /// where ADIF has one); this field's part of it is the mode the log holds.
    Rederived,
    /// Read by the import but not written back by this version's ADIF export.
    NotWritten,
    /// In a record of more than [`MAX_FIELDS`] fields, which is not imported.
    TooManyFields,
}

/// The modelled fields whose value is parsed to a type, so that a value of another shape is not
/// stored. A field the parser takes as text is never dropped for its value.
const TYPED: &[&str] = &[
    "FREQ",
    "FREQ_RX",
    "TX_PWR",
    "DXCC",
    "STX",
    "SRX",
    "IOTA",
    "QSO_DATE",
    "TIME_ON",
    "TIME_OFF",
    "QSO_DATE_OFF",
    "QSLSDATE",
    "QSL_SENT_VIA",
    "APP_NEXUS_ID",
    "APP_TEMPO_QSL_SENT_CLEARED",
];

/// The flags the log keeps only as a mark: anything but a yes is the absence of one.
const FLAGS: &[&str] = &[
    "QSL_SENT",
    "QSL_RCVD",
    "LOTW_QSL_RCVD",
    "EQSL_QSL_RCVD",
    "APP_QRZLOG_STATUS",
];

/// The fields the mode is read from.
const MODE_FIELDS: &[&str] = &["MODE", "SUBMODE", "APP_TEMPO_MODE"];

/// Each source field of `fields`, with its fate when the record is stored as `rec` and written
/// back as `written` (the export's fields). `rec` is `None` for a record the import does not
/// store as a contact.
fn fates(
    fields: &[(String, String)],
    rec: Option<&QsoRecord>,
    written: &[(String, String)],
) -> Vec<Fate> {
    let last: HashMap<&str, usize> = fields
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (k.as_str(), i))
        .collect();
    // Indexed once per record: a field is not found by scanning the others.
    let mut back_of: HashMap<&str, Vec<&str>> = HashMap::new();
    for (k, v) in written {
        back_of.entry(k.as_str()).or_default().push(v.as_str());
    }
    let kept: HashSet<(&str, &str)> = rec
        .map(|r| {
            r.extra
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect()
        })
        .unwrap_or_default();
    fields
        .iter()
        .enumerate()
        .map(|(i, (name, value))| {
            if rec.is_none() {
                return Fate::Dropped(DropReason::NoContact);
            }
            if last.get(name.as_str()) != Some(&i) {
                return Fate::Dropped(DropReason::Repeated);
            }
            let back = back_of.get(name.as_str()).map_or(&[][..], Vec::as_slice);
            if kept.contains(&(name.as_str(), value.as_str())) {
                return if back.contains(&value.as_str()) {
                    Fate::Passthrough
                } else {
                    Fate::Dropped(DropReason::NotWritten)
                };
            }
            if back.contains(&value.as_str()) {
                return Fate::Stored;
            }
            if let Some(v) = back.first() {
                return Fate::Normalized((*v).to_string());
            }
            let n = name.as_str();
            Fate::Dropped(if value.trim().is_empty() {
                DropReason::Empty
            } else if MODE_FIELDS.contains(&n) {
                DropReason::Rederived
            } else if FLAGS.contains(&n) && !is_yes(n, value) {
                DropReason::Default
            } else if TYPED.contains(&n) {
                DropReason::Invalid
            } else {
                DropReason::NotWritten
            })
        })
        .collect()
}

/// A flag's value that IS a mark: `Y`, a confirmation's `V`, QRZ's `C`.
fn is_yes(name: &str, v: &str) -> bool {
    let v = v.trim();
    v.eq_ignore_ascii_case("Y")
        || (name.ends_with("_RCVD") && v.eq_ignore_ascii_case("V"))
        || (name == "APP_QRZLOG_STATUS" && v.eq_ignore_ascii_case("C"))
}

/// One record read as the import reads it, and written back as the operator's own export writes
/// it, as fields.
fn stored_and_written(fields: &[(String, String)]) -> Option<Stored> {
    let rec = super::record_from(fields.iter().cloned().collect())?;
    let out = super::adif_record_own_log(&rec);
    let written = adif_records(&format!("<EOH>{out}<EOR>"))
        .records
        .into_iter()
        .next()
        .map(|r| r.fields)
        .unwrap_or_default();
    Some((rec, written))
}

// ── the preview ────────────────────────────────────────────────────────────────────────────────

/// The notes of one kind: how many, and the first few.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteGroup {
    pub kind: NoteKind,
    pub count: usize,
    pub examples: Vec<Note>,
}

/// How many records carry a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldCount {
    pub field: String,
    pub count: usize,
}

/// What an import of a file would do, before anything is written.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub format: Format,
    pub template: Option<String>,
    /// Records in the file.
    pub records: usize,
    /// Of them, the ones that are contacts (they have a `CALL`).
    pub contacts: usize,
    /// The earliest and latest contact, Unix seconds (records with no readable date left out).
    pub first_unix: Option<u64>,
    pub last_unix: Option<u64>,
    /// Filled by the caller from the log's own duplicate rule: contacts the log does not hold,
    /// contacts it holds already, and how many of those the import would upgrade.
    pub new: usize,
    pub duplicates: usize,
    pub updates: usize,
    pub notes: Vec<NoteGroup>,
    /// Which fields the file carries, most common first.
    pub coverage: Vec<FieldCount>,
    pub unmapped: Vec<Unmapped>,
}

/// How many examples a group of notes keeps.
const EXAMPLES: usize = 3;

/// A record as the import stores it, and the fields its export writes back.
type Stored = (QsoRecord, Vec<(String, String)>);

/// Each record of `source` read as the import reads it and written back as the export writes it
/// ([`stored_and_written`]), one at a time: `each` sees the record and what became of it, and
/// none of it is kept after, so a large file is not held in the log's form all at once. Answers
/// every note the source raises, grouped: the reader's own, and those its records' fields raise
/// as the import reads them.
fn each_stored(
    source: &Source,
    mut each: impl FnMut(&SourceRecord, Option<&Stored>),
) -> Vec<NoteGroup> {
    let mut groups = source.notes.clone();
    let mut notes = Vec::new();
    for r in &source.records {
        let s = stored_and_written(&r.fields);
        record_notes(
            r,
            s.as_ref().map(|(rec, w)| (rec, w.as_slice())),
            &mut notes,
        );
        notes.drain(..).for_each(|n| group(&mut groups, n));
        each(r, s.as_ref());
    }
    groups.sort_by_key(|g| g.kind);
    groups
}

fn record_notes(
    r: &SourceRecord,
    stored: Option<(&QsoRecord, &[(String, String)])>,
    notes: &mut Vec<Note>,
) {
    let mut note = |kind, text: String| {
        notes.push(Note {
            kind,
            at: r.at,
            text,
        })
    };
    let Some((rec, written)) = stored else {
        note(NoteKind::NoCall, field_list(&r.fields));
        return;
    };
    let get = |n: &str| {
        r.fields
            .iter()
            .rev()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.as_str())
    };
    let mut seen: HashSet<&str> = HashSet::new();
    for (k, _) in &r.fields {
        if !seen.insert(k) {
            note(NoteKind::Repeated, k.clone());
        }
    }
    match get("QSO_DATE") {
        Some(d) if valid_date(d) => {}
        d => note(
            NoteKind::BadDate,
            format!("QSO_DATE={}", clip(d.unwrap_or(""))),
        ),
    }
    if let Some(t) = get("TIME_ON") {
        if super::parse_adif_time(t).is_none() {
            note(NoteKind::BadTime, format!("TIME_ON={}", clip(t)));
        }
    }
    let fates = fates(&r.fields, Some(rec), written);
    for ((k, v), fate) in r.fields.iter().zip(&fates) {
        // The date and the time have notes of their own, above.
        if *fate == Fate::Dropped(DropReason::Invalid) && k != "QSO_DATE" && k != "TIME_ON" {
            note(NoteKind::BadValue, format!("{k}={}", clip(v)));
        }
    }
    if !rec.band.trim().is_empty() && super::query::band_key(&rec.band, 0.0).is_none() {
        note(NoteKind::UnknownBand, format!("BAND={}", clip(&rec.band)));
    }
    if !mode_known(&rec.mode) {
        note(NoteKind::UnknownMode, format!("MODE={}", clip(&rec.mode)));
    }
}

/// A `YYYYMMDD` date that names a day.
fn valid_date(d: &str) -> bool {
    let d = d.trim();
    let num = |a: usize, b: usize| d.get(a..b).and_then(|x| x.parse::<u32>().ok());
    d.len() == 8
        && d.bytes().all(|b| b.is_ascii_digit())
        && matches!((num(0, 4), num(4, 6), num(6, 8)), (Some(y), Some(m), Some(dd))
            if y >= 1930 && (1..=12).contains(&m) && (1..=days_in(y, m)).contains(&dd))
}

fn days_in(y: u32, m: u32) -> u32 {
    match m {
        2 if (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// ADIF 3.1.7's Mode enumeration, its import-only values included
/// (<https://adif.org/317/ADIF_317.htm>, the Mode table).
const ADIF_MODES: &[&str] = &[
    "AM",
    "ARDOP",
    "ATV",
    "CHIP",
    "CLO",
    "CONTESTI",
    "CW",
    "DIGITALVOICE",
    "DOMINO",
    "DYNAMIC",
    "FAX",
    "FM",
    "FSK441",
    "FSK",
    "FT8",
    "HELL",
    "ISCAT",
    "JT4",
    "JT6M",
    "JT9",
    "JT44",
    "JT65",
    "MFSK",
    "MSK144",
    "MTONE",
    "MT63",
    "OFDM",
    "OLIVIA",
    "OPERA",
    "PAC",
    "PAX",
    "PKT",
    "PSK",
    "PSK2K",
    "Q15",
    "QRA64",
    "ROS",
    "RTTY",
    "RTTYM",
    "SSB",
    "SSTV",
    "T10",
    "THOR",
    "THRB",
    "TOR",
    "V4",
    "VOI",
    "WINMOR",
    "WSPR",
    "AMTORFEC",
    "ASCI",
    "C4FM",
    "CHIP64",
    "CHIP128",
    "DOMINOF",
    "DSTAR",
    "FMHELL",
    "FSK31",
    "GTOR",
    "HELL80",
    "HFSK",
    "JT4A",
    "JT4B",
    "JT4C",
    "JT4D",
    "JT4E",
    "JT4F",
    "JT4G",
    "JT65A",
    "JT65B",
    "JT65C",
    "MFSK8",
    "MFSK16",
    "PAC2",
    "PAC3",
    "PAX2",
    "PCW",
    "PSK10",
    "PSK31",
    "PSK63",
    "PSK63F",
    "PSK125",
    "PSKAM10",
    "PSKAM31",
    "PSKAM50",
    "PSKFEC31",
    "PSKHELL",
    "QPSK31",
    "QPSK63",
    "QPSK125",
    "THRBX",
];

/// A mode the log can name: ADIF's, or a submode the log stores as the mode itself.
fn mode_known(mode: &str) -> bool {
    let m = mode.trim().to_ascii_uppercase();
    !m.is_empty()
        && (ADIF_MODES.contains(&m.as_str())
            || super::promoted_submode(&m).is_some()
            || super::adif_submode(&m).is_some()
            || super::psk_submode(&m).is_some())
}

/// A note counted in its kind's group, which keeps the first [`EXAMPLES`] by place in the file
/// (on a tie, the one noticed first). So the examples of notes grouped in turn are those of all of
/// them grouped at once.
fn group(groups: &mut Vec<NoteGroup>, n: Note) {
    match groups.iter_mut().find(|g| g.kind == n.kind) {
        Some(g) => {
            g.count += 1;
            let i = g.examples.partition_point(|e| e.at <= n.at);
            if i < EXAMPLES {
                g.examples.insert(i, n);
                g.examples.truncate(EXAMPLES);
            }
        }
        None => groups.push(NoteGroup {
            kind: n.kind,
            count: 1,
            examples: vec![n],
        }),
    }
}

/// The preview of `source`, with the log's counts left at zero for the caller to fill.
pub fn preview(source: &Source) -> Preview {
    let mut contacts = 0;
    let (mut first_unix, mut last_unix): (Option<u64>, Option<u64>) = (None, None);
    let notes = each_stored(source, |r, s| {
        let Some((rec, _)) = s else { return };
        contacts += 1;
        // A record with no readable date is left out of the range.
        if r.fields
            .iter()
            .rev()
            .find(|(k, _)| k == "QSO_DATE")
            .is_some_and(|(_, d)| valid_date(d))
        {
            let t = rec.when_unix;
            first_unix = Some(first_unix.map_or(t, |l| l.min(t)));
            last_unix = Some(last_unix.map_or(t, |h| h.max(t)));
        }
    });
    let mut coverage: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &source.records {
        let mut seen: HashSet<&str> = HashSet::new();
        for (k, _) in &r.fields {
            if seen.insert(k) {
                *coverage.entry(k).or_default() += 1;
            }
        }
    }
    let mut coverage: Vec<FieldCount> = coverage
        .into_iter()
        .map(|(field, count)| FieldCount {
            field: field.to_string(),
            count,
        })
        .collect();
    coverage.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.field.cmp(&b.field)));
    Preview {
        format: source.format,
        template: source.template.clone(),
        records: source.records.len() + source.over.over_records,
        contacts,
        first_unix,
        last_unix,
        new: 0,
        duplicates: 0,
        updates: 0,
        notes,
        coverage,
        unmapped: source.unmapped.clone(),
    }
}

// ── the report ─────────────────────────────────────────────────────────────────────────────────

/// How many of one dropped field went for one reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonCount {
    pub reason: DropReason,
    pub count: usize,
}

/// One source field, every occurrence accounted for: `occurrences` is the sum of the four.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldAccount {
    pub field: String,
    pub occurrences: usize,
    pub stored: usize,
    pub normalized: usize,
    pub passthrough: usize,
    pub dropped: usize,
    pub reasons: Vec<ReasonCount>,
    /// A few of the changes: `value → written as`, or a dropped value as it was.
    pub examples: Vec<String>,
}

/// What an import did with every part of the file.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub format: Format,
    pub template: Option<String>,
    pub records: usize,
    pub contacts: usize,
    pub added: usize,
    pub duplicates: usize,
    pub updated: usize,
    /// "Import new only": a contact the log holds was left as it is.
    pub new_only: bool,
    pub fields: Vec<FieldAccount>,
    pub unmapped: Vec<Unmapped>,
    pub notes: Vec<NoteGroup>,
    /// Every field of every record has a fate (the sum check, over the whole file).
    pub accounted: bool,
}

/// The import's counts: `(added, duplicates, updated)`.
pub type Counts = (usize, usize, usize);

/// The no-loss report for `source`, imported with `counts`.
pub fn report(source: &Source, counts: Counts, new_only: bool) -> Report {
    let mut contacts = 0;
    let mut fields: Vec<FieldAccount> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut total = 0usize;
    // `n` fields of one name and fate; `value` is theirs when they are one field read with its
    // value (a field over the limit is only counted).
    let mut account = |name: &str, value: Option<&str>, fate: Fate, n: usize| {
        total += n;
        let i = *index.entry(name.to_string()).or_insert_with(|| {
            fields.push(FieldAccount {
                field: name.to_string(),
                occurrences: 0,
                stored: 0,
                normalized: 0,
                passthrough: 0,
                dropped: 0,
                reasons: Vec::new(),
                examples: Vec::new(),
            });
            fields.len() - 1
        });
        let f = &mut fields[i];
        f.occurrences += n;
        let example = match fate {
            Fate::Stored => {
                f.stored += n;
                None
            }
            Fate::Passthrough => {
                f.passthrough += n;
                None
            }
            Fate::Normalized(to) => {
                f.normalized += n;
                value.map(|v| format!("{} → {}", clip(v), clip(&to)))
            }
            Fate::Dropped(reason) => {
                f.dropped += n;
                match f.reasons.iter_mut().find(|r| r.reason == reason) {
                    Some(r) => r.count += n,
                    None => f.reasons.push(ReasonCount { reason, count: n }),
                }
                value.map(clip)
            }
        };
        if let Some(e) = example {
            if f.examples.len() < EXAMPLES && !f.examples.contains(&e) {
                f.examples.push(e);
            }
        }
    };
    let notes = each_stored(source, |r, s| {
        let (rec, written) = match s {
            Some((rec, w)) => (Some(rec), w.as_slice()),
            None => (None, &[][..]),
        };
        contacts += usize::from(rec.is_some());
        for ((k, v), fate) in r.fields.iter().zip(fates(&r.fields, rec, written)) {
            account(k, Some(v), fate, 1);
        }
    });
    let unterminated = Fate::Dropped(DropReason::Unterminated);
    for (k, v) in &source.tail.list {
        account(k, Some(v), unterminated.clone(), 1);
    }
    for (k, n) in source.tail.over.iter().flat_map(|o| &o.names) {
        account(k, None, unterminated.clone(), *n);
    }
    for (k, n) in &source.over.names {
        account(k, None, Fate::Dropped(DropReason::TooManyFields), *n);
    }
    let occurrences: usize = source.records.iter().map(|r| r.fields.len()).sum::<usize>()
        + source.tail.len()
        + source.over.names.values().sum::<usize>();
    for f in &mut fields {
        f.reasons.sort_by_key(|r| r.reason);
    }
    fields.sort_by(|a, b| a.field.cmp(&b.field));
    let summed: usize = fields
        .iter()
        .map(|f| f.stored + f.normalized + f.passthrough + f.dropped)
        .sum();
    Report {
        format: source.format,
        template: source.template.clone(),
        records: source.records.len() + source.over.over_records,
        contacts,
        added: counts.0,
        duplicates: counts.1,
        updated: counts.2,
        new_only,
        fields,
        unmapped: source.unmapped.clone(),
        notes,
        accounted: summed == occurrences && total == occurrences,
    }
}

#[cfg(test)]
mod tests;
