//! The Logbook's field filters as SQL ([`LogFilter`] → a `WHERE` over the `qso` table, alias `q`)
//! and the read that carries it: the rows an order is built from, already filtered by the store.
//!
//! Each clause states its field EXACTLY as the Logbook means it — the rule the window used to
//! hold, ported — never a looser test for Rust to finish. The fields SQL cannot state are listed
//! in [`crate::logbook::query::filter`]'s header and are not here.
//!
//! Every value is a bound parameter; nothing the operator typed is written into the statement.
//! A text field is trimmed as JavaScript trims and folded to upper case as ASCII folds — every
//! stored column a clause compares is ASCII by its nature (calls, bands, modes, grids, states,
//! references, contest ids, QSL tokens), and the free text, which is not, is SQL's only when it is
//! ASCII.
//!
//! **The county clause names its index** (`qso_extra_cnty`, a partial index of the `CNTY` rows
//! only), so a county filter reads those rows and never walks the passthrough of the whole log.
//! The plan test below holds it to that.

use super::{
    contest_is_empty, extra_columns, extras_width, record_from_row, with_extras, with_submode,
    Error, LogDb, Result, QSO_COLUMNS, SUBMODE_JOIN, SUBMODE_VALUE,
};
use crate::logbook::query::filter::{field, LogFilter, QslState, SentState};
use crate::logbook::query::{ui_band_edges, UI_BAND_LABELS};
use crate::logbook::QsoRecord;
use rusqlite::params_from_iter;
use rusqlite::types::Value;

/// A filter's `WHERE`: the clauses (joined with AND; `1` for none) and their parameters in order.
#[derive(Debug, Default)]
pub(crate) struct Where {
    clauses: Vec<String>,
    params: Vec<Value>,
}

impl Where {
    fn push(&mut self, clause: impl Into<String>, params: impl IntoIterator<Item = Value>) {
        self.clauses.push(clause.into());
        self.params.extend(params);
    }

    pub(crate) fn sql(&self) -> String {
        if self.clauses.is_empty() {
            "1".to_string()
        } else {
            self.clauses.join(" AND ")
        }
    }
}

fn text(s: impl Into<String>) -> Value {
    Value::Text(s.into())
}

/// `s` as a GLOB pattern that matches itself: GLOB's own `[` is the one character to quote (a
/// `]` outside a set is literal), and `*` and `?` the caller keeps or has none of.
fn glob_quote(s: &str) -> String {
    s.replace('[', "[[]")
}

/// `s` quoted for a `LIKE … ESCAPE '\'`.
fn like_quote(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// The mode as the Logbook names it (`mode_key`, `query.rs`) — the sidebands are SSB, the two BPSK
/// spellings PSK — of the SQL text `x`.
fn mode_key_sql(x: &str) -> String {
    format!(
        "(CASE upper(trim({x})) WHEN 'USB' THEN 'SSB' WHEN 'LSB' THEN 'SSB' \
         WHEN 'BPSK31' THEN 'PSK31' WHEN 'BPSK63' THEN 'PSK63' ELSE upper(trim({x})) END)"
    )
}

/// Whether the confirmation token in column `c` is held (`record_from_row`'s rule).
fn confirmed(c: &str) -> String {
    if c == "qrz_status_raw" {
        format!("upper(q.{c}) IN ('C','Y')")
    } else {
        format!("upper(trim(q.{c})) IN ('Y','V')")
    }
}

/// Whether the upload stamp of `service` says it left the station: waiting for the service, taken,
/// or one the service already had. A rejected or refused upload was not sent.
fn uploaded(service: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM qso_upload u WHERE u.qso_id = q.id AND u.service = '{service}' \
         AND u.outcome IN ('pending','accepted','duplicate'))"
    )
}

/// The `WHERE` that keeps exactly the rows `f` lets through, for every field SQL answers.
pub(crate) fn where_of(f: &LogFilter) -> Where {
    let mut w = Where::default();
    let call = field(&f.call).to_ascii_uppercase();
    if !call.is_empty() {
        // A pattern names the whole call; plain text is a prefix. `call_norm` is the call trimmed
        // and ASCII-uppercased, exactly as this text now is.
        let pattern = if call.contains(['*', '?']) {
            glob_quote(&call)
        } else {
            format!("{}*", glob_quote(&call))
        };
        w.push("q.call_norm GLOB ?", [text(pattern)]);
    }
    let band = field(&f.band).to_ascii_lowercase();
    if !band.is_empty() {
        // `band_key`: the stored label where it names a band of the UI's table, else the band its
        // frequency falls in.
        match ui_band_edges(&band) {
            Some((lo, hi)) => {
                let known = UI_BAND_LABELS
                    .iter()
                    .map(|l| format!("'{l}'"))
                    .collect::<Vec<_>>()
                    .join(",");
                w.push(
                    format!(
                        "(lower(trim(q.band)) = ? OR (lower(trim(q.band)) NOT IN ({known}) \
                         AND q.freq_mhz >= ? AND q.freq_mhz <= ?))"
                    ),
                    [text(band.clone()), Value::Real(lo), Value::Real(hi)],
                );
            }
            None => w.push("lower(trim(q.band)) = ?", [text(band)]),
        }
    }
    let mode = field(&f.mode);
    if !mode.is_empty() {
        let key = crate::logbook::query::mode_key(mode);
        w.push(
            format!(
                "({} = ? OR EXISTS (SELECT 1 FROM qso_extra s WHERE s.qso_id = q.id \
                 AND s.name = 'SUBMODE' AND {} = ?))",
                mode_key_sql("q.mode"),
                mode_key_sql("s.value")
            ),
            [text(key.clone()), text(key)],
        );
    }
    if let Some(from) = f.from_unix {
        w.push(
            "q.when_unix >= ?",
            [Value::Integer(i64::try_from(from).unwrap_or(i64::MAX))],
        );
    }
    if let Some(to) = f.to_unix {
        w.push(
            "q.when_unix < ?",
            [Value::Integer(i64::try_from(to).unwrap_or(i64::MAX))],
        );
    }
    if let Some(z) = f.itu_zone {
        w.push(
            "EXISTS (SELECT 1 FROM qso_extra i WHERE i.qso_id = q.id AND i.name = 'ITUZ' \
             AND trim(i.value) = ?)",
            [text(z.to_string())],
        );
    }
    let grid = field(&f.grid).to_ascii_uppercase();
    if !grid.is_empty() {
        w.push(
            "upper(trim(q.grid)) GLOB ?",
            [text(format!("{}*", glob_quote(&grid)))],
        );
    }
    let state = field(&f.state).to_ascii_uppercase();
    if !state.is_empty() {
        // The state every reader counts (`logbook::record_field`): a merged contest contact's
        // exchange STATE — the first by `ord`, unless blank — else the column.
        w.push(
            "upper(trim(coalesce(nullif(trim((SELECT c.value FROM qso_contest_adif c \
             WHERE c.qso_id = q.id AND c.tag = 'STATE' ORDER BY c.ord LIMIT 1)), ''), \
             q.state))) = ?",
            [text(state)],
        );
    }
    let county = field(&f.county);
    if !county.is_empty() {
        w.push(
            "q.id IN (SELECT c.qso_id FROM qso_extra c INDEXED BY qso_extra_cnty \
             WHERE c.name = 'CNTY' AND c.value LIKE ? ESCAPE '\\')",
            [text(format!("{}%", like_quote(county)))],
        );
    }
    let iota = field(&f.iota).to_ascii_uppercase();
    if !iota.is_empty() {
        w.push("upper(trim(q.ota_iota)) = ?", [text(iota)]);
    }
    let reference = field(&f.reference).to_ascii_uppercase();
    if !reference.is_empty() {
        w.push(
            "(instr(upper(q.ota_their_ref), ?) > 0 OR instr(upper(q.ota_my_ref), ?) > 0)",
            [text(reference.clone()), text(reference)],
        );
    }
    let my_ref = field(&f.my_ref).to_ascii_uppercase();
    if !my_ref.is_empty() {
        // One whole element of the list, as the export splits it (`,` or `;`, spaces aside).
        w.push(
            "instr(',' || replace(replace(upper(q.ota_my_ref), ' ', ''), ';', ',') || ',', ?) > 0",
            [text(format!(",{},", my_ref.replace(' ', "")))],
        );
    }
    let contest = field(&f.contest).to_ascii_uppercase();
    if !contest.is_empty() {
        w.push("upper(trim(q.contest_id)) = ?", [text(contest)]);
    }
    for (state, column, service) in [
        (f.qsl.lotw, "lotw_rcvd_raw", Some("lotw")),
        (f.qsl.eqsl, "eqsl_rcvd_raw", Some("eqsl")),
        (f.qsl.qrz, "qrz_status_raw", Some("qrz")),
        (f.qsl.card, "qsl_card_rcvd_raw", None),
    ] {
        let sent = || match service {
            Some(s) => uploaded(s),
            None => "q.qsl_sent = 1".to_string(),
        };
        match state {
            None => {}
            Some(QslState::Confirmed) => w.push(confirmed(column), []),
            Some(QslState::Sent) => w.push(sent(), []),
            Some(QslState::NotSent) => w.push(format!("NOT {}", sent()), []),
        }
    }
    match f.qsl.clublog {
        None => {}
        Some(SentState::Sent) => w.push(uploaded("clublog"), []),
        Some(SentState::NotSent) => w.push(format!("NOT {}", uploaded("clublog")), []),
    }
    let words = field(&f.text);
    if !words.is_empty() && words.is_ascii() {
        let pattern = format!("%{}%", like_quote(words));
        w.push(
            "(q.comment LIKE ? ESCAPE '\\' OR q.name LIKE ? ESCAPE '\\' \
             OR q.qth LIKE ? ESCAPE '\\')",
            [text(pattern.clone()), text(pattern.clone()), text(pattern)],
        );
    }
    w
}

impl LogDb {
    /// The statement [`Self::each_filtered`] runs: `columns` read (the id always), each row's
    /// SUBMODE when `submode`, the passthrough fields `extras` names, and only the rows `filter`
    /// lets through, in log order. Its own function so a test can ask SQLite how it plans it.
    pub(crate) fn filtered_sql(
        &self,
        columns: &[&str],
        extras: &[&str],
        submode: bool,
        filter: &Where,
    ) -> Result<String> {
        let (value, join) = if submode {
            (SUBMODE_VALUE, SUBMODE_JOIN)
        } else {
            ("", "")
        };
        let (extra_values, extra_joins) = extra_columns(extras)?;
        Ok(format!(
            "SELECT {}{value}{extra_values}, q.rowid FROM qso q{join}{extra_joins} WHERE {} \
             ORDER BY q.rowid",
            self.narrow_projection(columns)?,
            filter.sql()
        ))
    }

    /// Hand `each` every row `filter` lets through — of the fields SQL answers — in log order,
    /// with only `columns` filled, its SUBMODE when `submode`, and the passthrough fields `extras`
    /// names: the rows a Logbook order is built from. One row at a time, never the log's rows held
    /// together; the caller runs it inside one read.
    pub fn each_filtered(
        &self,
        columns: &[&str],
        extras: &[&str],
        submode: bool,
        filter: &LogFilter,
        each: &mut dyn FnMut(u32, &QsoRecord),
    ) -> Result<()> {
        let w = where_of(filter);
        let mut stmt = self
            .conn
            .prepare(&self.filtered_sql(columns, extras, submode, &w)?)?;
        let mut rows = stmt.query(params_from_iter(w.params.iter()))?;
        let submode_at = QSO_COLUMNS.len();
        let extras_at = submode_at + usize::from(submode);
        let rowid_at = extras_at + extras_width(extras);
        while let Some(row) = rows.next()? {
            let rowid = u32::try_from(row.get::<_, i64>(rowid_at)?)
                .map_err(|_| Error::Sql(rusqlite::Error::IntegralValueOutOfRange(rowid_at, 0)))?;
            let mut rec = record_from_row(row)?;
            if rec.contest.as_deref().is_some_and(contest_is_empty) {
                rec.contest = None;
            }
            if submode {
                with_submode(&mut rec, row.get(submode_at)?);
            }
            with_extras(&mut rec, row, extras_at, extras)?;
            crate::logbook::canonical::contact(&mut rec);
            each(rowid, &rec);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::query::filter::{AwardNeed, QslFilter};
    use crate::logbook::query::{LogQuery, OrderBuilder, SortKey, SortTerm};
    use crate::logbook::sqlite::{Resolved, ORDER_COLUMNS};
    use crate::logbook::{parse_adif, ContestFields, RecordId, UploadOutcome, UploadStatus};

    /// One contact, made by hand: `call` on `band`/`mode` at `when` (seconds past 2026-01-01).
    fn qso(call: &str, band: &str, mode: &str, when: u64) -> QsoRecord {
        let freq = match band {
            "20m" => "14.074",
            "40m" => "7.074",
            "6m" => "50.313",
            _ => "0",
        };
        let mut r = parse_adif(&format!(
            "<CALL:{}>{call}<BAND:{}>{band}<MODE:{}>{mode}<FREQ:{}>{freq}<QSO_DATE:8>20260101\
             <TIME_ON:6>000000<EOR>",
            call.len(),
            band.len(),
            mode.len(),
            freq.len()
        ))
        .remove(0);
        r.when_unix += when;
        r
    }

    /// A small log in which each filter field picks out a known set of calls.
    fn log() -> Vec<QsoRecord> {
        let mut out = Vec::new();
        let mut a = qso("K1ABC", "20m", "FT8", 0);
        a.grid = Some("FN31pr".into());
        a.state = Some("CT".into());
        a.extra.push(("CNTY".into(), "CT,HARTFORD".into()));
        a.extra.push(("ITUZ".into(), "8".into()));
        a.qsl_rcvd.lotw = true;
        a.upload.lotw = Some(UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1,
            detail: None,
        });
        a.comment = Some("Nice signal from Hartford".into());
        out.push(a);
        let mut b = qso("K1ABD/P", "40m", "SSB", 3_600);
        b.extra.push(("SUBMODE".into(), "USB".into()));
        b.state = Some("ma".into());
        b.extra.push(("CNTY".into(), "MA,MIDDLESEX".into()));
        b.ota.their_ref = Some("US-1234,US-5678".into());
        b.upload.lotw = Some(UploadStatus {
            outcome: UploadOutcome::Rejected,
            when_unix: 1,
            detail: None,
        });
        b.qsl_sent.sent = true;
        out.push(b);
        let mut c = qso("DL1XYZ", "", "MFSK", 86_400);
        // No band label: its frequency's band (20 m) is its band.
        c.freq_mhz = 14.080;
        c.extra.push(("SUBMODE".into(), "FT4".into()));
        c.grid = Some("JO62".into());
        c.ota.iota = Some("EU-005".into());
        c.name = Some("Jürgen".into());
        c.qth = Some("MÜNCHEN".into());
        c.contest = Some(Box::new(ContestFields {
            contest_id: "CQ-WW-DX".into(),
            ..Default::default()
        }));
        c.upload.clublog = Some(UploadStatus {
            outcome: UploadOutcome::Pending,
            when_unix: 1,
            detail: None,
        });
        out.push(c);
        let mut d = qso("W6[X]", "6m", "USB", 2 * 86_400);
        d.qsl_rcvd.card = true;
        d.extra.push(("ITUZ".into(), " 6 ".into()));
        d.comment = Some("100% copy_ok".into());
        out.push(d);
        for (i, r) in out.iter_mut().enumerate() {
            r.id = Some(RecordId::Provisional {
                hash: 500 + i as u64,
                ordinal: 0,
            });
            r.confirmed = r.qsl_rcvd.any();
            r.award_confirmed = r.qsl_rcvd.award();
        }
        out
    }

    fn store() -> LogDb {
        let mut db = LogDb::open_in_memory().unwrap();
        db.insert_all(log().iter().map(|r| (r, Resolved::default())))
            .unwrap();
        db
    }

    /// The calls `f` lets through, in log order.
    fn through(db: &LogDb, f: &LogFilter) -> Vec<String> {
        let mut out = Vec::new();
        db.each_filtered(ORDER_COLUMNS, &[], false, f, &mut |_, r| {
            out.push(r.call.clone())
        })
        .unwrap();
        out
    }

    /// An activation's contacts: the station's own park, whole, in a one-park or two-fer list
    /// however it is spelled, across 0000Z — never a longer reference that starts with it, and
    /// never a hunter's contact with that park.
    #[test]
    fn my_ref_keeps_the_station_s_own_park_whole() {
        let day = 1_767_225_600 + 3 * 86_400; // a 0000Z
        let row = |call: &str, mine: Option<&str>, theirs: Option<&str>, when: u64| {
            let mut r = qso(call, "20m", "SSB", 0);
            r.when_unix = when;
            r.ota.my_ref = mine.map(Into::into);
            r.ota.their_ref = theirs.map(Into::into);
            r
        };
        let rows = [
            row("W1BEFORE", Some("US-1000"), None, day - 86_400 + 600),
            row("W1LATE", Some("US-1000,US-2000"), None, day - 1_200),
            row("W1AFTER", Some("US-1000, US-2000"), None, day + 600),
            row("W1SEMI", Some("us-2000;us-1000"), None, day + 900),
            row("W1LONGER", Some("US-10000"), None, day + 1_000),
            row("W1HUNTED", None, Some("US-1000"), day + 1_100),
            row("W1OTHER", Some("US-2000"), None, day + 1_200),
        ];
        let mut rows = rows;
        for (i, r) in rows.iter_mut().enumerate() {
            r.id = Some(RecordId::Provisional {
                hash: 900 + i as u64,
                ordinal: 0,
            });
        }
        let mut db = LogDb::open_in_memory().unwrap();
        db.insert_all(rows.iter().map(|r| (r, Resolved::default())))
            .unwrap();
        let f = |my_ref: &str, from_unix: Option<u64>| LogFilter {
            my_ref: my_ref.into(),
            from_unix,
            ..Default::default()
        };
        assert_eq!(
            through(&db, &f(" us-1000 ", Some(day - 86_400))),
            ["W1BEFORE", "W1LATE", "W1AFTER", "W1SEMI"]
        );
        assert_eq!(
            through(&db, &f("US-1000", Some(day - 3_600))),
            ["W1LATE", "W1AFTER", "W1SEMI"]
        );
        assert_eq!(
            through(&db, &f("US-2000", None)),
            ["W1LATE", "W1AFTER", "W1SEMI", "W1OTHER"]
        );
        assert_eq!(through(&db, &f("US-10000", None)), ["W1LONGER"]);
        assert!(!f("US-1000", None).is_empty());
        assert!(f("  ", None).is_empty());
    }

    #[test]
    fn each_field_filter_keeps_exactly_its_rows() {
        let db = store();
        let all = ["K1ABC", "K1ABD/P", "DL1XYZ", "W6[X]"];
        let cases: Vec<(LogFilter, Vec<&str>)> = vec![
            (LogFilter::default(), all.to_vec()),
            // Call: a prefix, ASCII case aside; a pattern names the whole call; GLOB's `[` is
            // literal.
            (
                LogFilter {
                    call: " k1ab ".into(),
                    ..Default::default()
                },
                vec!["K1ABC", "K1ABD/P"],
            ),
            (
                LogFilter {
                    call: "*/P".into(),
                    ..Default::default()
                },
                vec!["K1ABD/P"],
            ),
            (
                LogFilter {
                    call: "K?ABC".into(),
                    ..Default::default()
                },
                vec!["K1ABC"],
            ),
            (
                LogFilter {
                    call: "W6[".into(),
                    ..Default::default()
                },
                vec!["W6[X]"],
            ),
            // Band: the label, or the frequency's band where the label names none.
            (
                LogFilter {
                    band: "20M".into(),
                    ..Default::default()
                },
                vec!["K1ABC", "DL1XYZ"],
            ),
            // Mode: the sidebands are SSB, a SUBMODE counts.
            (
                LogFilter {
                    mode: "ssb".into(),
                    ..Default::default()
                },
                vec!["K1ABD/P", "W6[X]"],
            ),
            (
                LogFilter {
                    mode: "FT4".into(),
                    ..Default::default()
                },
                vec!["DL1XYZ"],
            ),
            (
                LogFilter {
                    mode: "USB".into(),
                    ..Default::default()
                },
                vec!["K1ABD/P", "W6[X]"],
            ),
            // Dates: from inclusive, to exclusive.
            (
                LogFilter {
                    from_unix: Some(log()[1].when_unix),
                    to_unix: Some(log()[3].when_unix),
                    ..Default::default()
                },
                vec!["K1ABD/P", "DL1XYZ"],
            ),
            (
                LogFilter {
                    itu_zone: Some(6),
                    ..Default::default()
                },
                vec!["W6[X]"],
            ),
            (
                LogFilter {
                    grid: "fn3".into(),
                    ..Default::default()
                },
                vec!["K1ABC"],
            ),
            (
                LogFilter {
                    state: "MA".into(),
                    ..Default::default()
                },
                vec!["K1ABD/P"],
            ),
            (
                LogFilter {
                    county: "ma,".into(),
                    ..Default::default()
                },
                vec!["K1ABD/P"],
            ),
            (
                LogFilter {
                    county: "CT,HART".into(),
                    ..Default::default()
                },
                vec!["K1ABC"],
            ),
            (
                LogFilter {
                    iota: "eu-005".into(),
                    ..Default::default()
                },
                vec!["DL1XYZ"],
            ),
            (
                LogFilter {
                    reference: "us-5678".into(),
                    ..Default::default()
                },
                vec!["K1ABD/P"],
            ),
            (
                LogFilter {
                    contest: "cq-ww-dx".into(),
                    ..Default::default()
                },
                vec!["DL1XYZ"],
            ),
            // Free text in the comment, the name or the QTH; `%` and `_` are only themselves.
            (
                LogFilter {
                    text: "hartford".into(),
                    ..Default::default()
                },
                vec!["K1ABC"],
            ),
            (
                LogFilter {
                    text: "100%".into(),
                    ..Default::default()
                },
                vec!["W6[X]"],
            ),
            // Unquoted, `_` would match the space and `%` anything between.
            (
                LogFilter {
                    text: "0%_c".into(),
                    ..Default::default()
                },
                vec![],
            ),
            // Several fields: AND.
            (
                LogFilter {
                    call: "K1".into(),
                    band: "40m".into(),
                    ..Default::default()
                },
                vec!["K1ABD/P"],
            ),
        ];
        for (f, want) in cases {
            assert_eq!(through(&db, &f), want, "{f:?}");
        }
    }

    #[test]
    fn qsl_state_per_service() {
        let db = store();
        let qsl = |q: QslFilter| LogFilter {
            qsl: q,
            ..Default::default()
        };
        let cases: Vec<(QslFilter, Vec<&str>)> = vec![
            (
                QslFilter {
                    lotw: Some(QslState::Confirmed),
                    ..Default::default()
                },
                vec!["K1ABC"],
            ),
            // A rejected upload was not sent.
            (
                QslFilter {
                    lotw: Some(QslState::Sent),
                    ..Default::default()
                },
                vec!["K1ABC"],
            ),
            (
                QslFilter {
                    lotw: Some(QslState::NotSent),
                    ..Default::default()
                },
                vec!["K1ABD/P", "DL1XYZ", "W6[X]"],
            ),
            (
                QslFilter {
                    card: Some(QslState::Confirmed),
                    ..Default::default()
                },
                vec!["W6[X]"],
            ),
            (
                QslFilter {
                    card: Some(QslState::Sent),
                    ..Default::default()
                },
                vec!["K1ABD/P"],
            ),
            // A pending upload has left the station.
            (
                QslFilter {
                    clublog: Some(SentState::Sent),
                    ..Default::default()
                },
                vec!["DL1XYZ"],
            ),
            (
                QslFilter {
                    eqsl: Some(QslState::Confirmed),
                    ..Default::default()
                },
                vec![],
            ),
            (
                QslFilter {
                    lotw: Some(QslState::NotSent),
                    card: Some(QslState::NotSent),
                    ..Default::default()
                },
                vec!["DL1XYZ", "W6[X]"],
            ),
        ];
        for (q, want) in cases {
            assert_eq!(through(&db, &qsl(q.clone())), want, "{q:?}");
        }
    }

    /// Free text with a letter outside ASCII is the order's to decide, by JavaScript's case rule:
    /// SQL lets every row through for it, and the order keeps the one that matches.
    #[test]
    fn free_text_outside_ascii_folds_all_of_unicode() {
        let db = store();
        let f = LogFilter {
            text: "münchen".into(),
            ..Default::default()
        };
        assert_eq!(through(&db, &f).len(), 4, "SQL leaves it to the order");
        let q = LogQuery {
            sort: SortKey::Call,
            asc: true,
            search: String::new(),
            needs_confirm_only: false,
            then: Vec::new(),
            filter: f,
        };
        let mut b = OrderBuilder::new(&q, &|_| None);
        let (columns, extras) = b.reads(ORDER_COLUMNS);
        let mut calls = HashMapCalls::default();
        db.each_filtered(&columns, &extras, false, &q.filter, &mut |rowid, r| {
            calls.0.push((rowid, r.call.clone()));
            b.push(rowid, r)
        })
        .unwrap();
        assert_eq!(calls.named(&b.finish()), ["DL1XYZ"]);
    }

    /// ★ The State filter and the State sort take a merged contest contact's state as every
    /// reader does (`logbook::record_field`): the state its exchange carried, which its upload
    /// carries and WAS counts — not the one looked up from its call beside it; a blank exchange
    /// state is none, and the column answers.
    #[test]
    fn the_state_filter_and_sort_read_the_exchanges_state() {
        let with = |call: &str, when: u64, own: &str, sent: Option<&str>| {
            let mut r = qso(call, "20m", "CW", when);
            r.state = Some(own.into());
            r.contest = sent.map(|s| {
                Box::new(ContestFields {
                    contest_id: "NAQP-CW".into(),
                    adif: vec![("STATE".into(), s.into())],
                    ..Default::default()
                })
            });
            r
        };
        let mut rows = [
            with("K1SENT", 0, "TX", Some("NY")),
            with("K2BLANK", 60, "NY", Some(" ")),
            with("K3PLAIN", 120, "NY", None),
            with("K4AZ", 180, "AZ", None),
        ];
        for (i, r) in rows.iter_mut().enumerate() {
            r.id = Some(RecordId::Provisional {
                hash: 900 + i as u64,
                ordinal: 0,
            });
        }
        let mut db = LogDb::open_in_memory().unwrap();
        db.insert_all(rows.iter().map(|r| (r, Resolved::default())))
            .unwrap();
        let state = |s: &str| LogFilter {
            state: s.into(),
            ..Default::default()
        };
        assert_eq!(through(&db, &state("ny")), ["K1SENT", "K2BLANK", "K3PLAIN"]);
        assert!(
            through(&db, &state("TX")).is_empty(),
            "the looked-up state is shadowed"
        );
        let q = LogQuery {
            sort: SortKey::State,
            asc: false,
            search: String::new(),
            needs_confirm_only: false,
            then: Vec::new(),
            filter: LogFilter::default(),
        };
        let mut b = OrderBuilder::new(&q, &|_| None);
        let (columns, extras) = b.reads(ORDER_COLUMNS);
        let mut calls = HashMapCalls::default();
        db.each_filtered(&columns, &extras, false, &q.filter, &mut |rowid, r| {
            calls.0.push((rowid, r.call.clone()));
            b.push(rowid, r)
        })
        .unwrap();
        // Descending: the three NY (newest first, the time breaking the tie), then AZ. K1SENT
        // sorted by its looked-up TX would come first.
        assert_eq!(
            calls.named(&b.finish()),
            ["K3PLAIN", "K2BLANK", "K1SENT", "K4AZ"]
        );
    }

    #[derive(Default)]
    struct HashMapCalls(Vec<(u32, String)>);
    impl HashMapCalls {
        fn named(&self, order: &[u32]) -> Vec<String> {
            order
                .iter()
                .map(|h| self.0.iter().find(|(r, _)| r == h).unwrap().1.clone())
                .collect()
        }
    }

    /// Up to three keys, each in its own direction; the time breaks a full tie in the first
    /// key's direction — and the county, a passthrough tag, sorts like any column.
    #[test]
    fn a_second_and_third_key_order_ties_in_their_own_direction() {
        let db = store();
        let order_of = |sort: SortKey, asc: bool, then: Vec<SortTerm>| {
            let q = LogQuery {
                sort,
                asc,
                search: String::new(),
                needs_confirm_only: false,
                then,
                filter: LogFilter::default(),
            };
            let mut b = OrderBuilder::new(&q, &|_| None);
            let (columns, extras) = b.reads(ORDER_COLUMNS);
            let mut calls = HashMapCalls::default();
            db.each_filtered(&columns, &extras, false, &q.filter, &mut |rowid, r| {
                calls.0.push((rowid, r.call.clone()));
                b.push(rowid, r)
            })
            .unwrap();
            calls.named(&b.finish())
        };
        // Mode ascending, then band (frequency) descending: the two SSB rows are 40 m and 6 m.
        assert_eq!(
            order_of(
                SortKey::Mode,
                true,
                vec![SortTerm {
                    sort: SortKey::Band,
                    asc: false
                }]
            ),
            ["K1ABC", "DL1XYZ", "W6[X]", "K1ABD/P"]
        );
        // The positive control: the same first key with the second ascending flips the SSB pair.
        assert_eq!(
            order_of(
                SortKey::Mode,
                true,
                vec![SortTerm {
                    sort: SortKey::Band,
                    asc: true
                }]
            ),
            ["K1ABC", "DL1XYZ", "K1ABD/P", "W6[X]"]
        );
        // County ascending: no county sorts first, the time breaking that tie ascending.
        assert_eq!(
            order_of(SortKey::County, true, Vec::new()),
            ["DL1XYZ", "W6[X]", "K1ABC", "K1ABD/P"]
        );
        // A key past the third is not read.
        let many = vec![
            SortTerm {
                sort: SortKey::Mode,
                asc: true,
            },
            SortTerm {
                sort: SortKey::State,
                asc: true,
            },
            SortTerm {
                sort: SortKey::Call,
                asc: false,
            },
        ];
        assert_eq!(
            order_of(SortKey::Grid, true, many.clone()),
            order_of(SortKey::Grid, true, many[..2].to_vec())
        );
    }

    /// The plans: the county filter reads its partial index, the date range the time index, and
    /// a call prefix may take the call index — never a walk of the passthrough.
    #[test]
    fn the_filters_read_their_indexes() {
        let db = store();
        let plan = |f: LogFilter| -> Vec<String> {
            let w = where_of(&f);
            let sql = db.filtered_sql(ORDER_COLUMNS, &[], false, &w).unwrap();
            let mut stmt = db
                .conn
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap();
            stmt.query_map(params_from_iter(w.params.iter()), |r| r.get::<_, String>(3))
                .unwrap()
                .map(|d| d.unwrap())
                .collect()
        };
        let county = plan(LogFilter {
            county: "MA,".into(),
            ..Default::default()
        });
        assert!(
            county
                .iter()
                .any(|p| p.contains("USING INDEX qso_extra_cnty (value>? AND value<?)")),
            "{county:?}"
        );
        assert!(
            !county.iter().any(|p| p.starts_with("SCAN c")),
            "{county:?}"
        );
        let dates = plan(LogFilter {
            from_unix: Some(1),
            to_unix: Some(2),
            ..Default::default()
        });
        assert!(dates.iter().any(|p| p.contains("qso_recent")), "{dates:?}");
        // Every passthrough field a filter reads is looked up by `qso_extra`'s own key.
        let itu = plan(LogFilter {
            itu_zone: Some(8),
            mode: "SSB".into(),
            ..Default::default()
        });
        assert!(
            itu.iter()
                .filter(|p| p.contains("qso_extra"))
                .all(|p| p.contains("sqlite_autoindex_qso_extra_1 (qso_id=? AND name=?)")),
            "{itu:?}"
        );
    }

    /// A filter round-trips as the window sends it, and an unknown field is refused rather than
    /// ignored — a filter the engine did not understand must not answer as if it had.
    #[test]
    fn a_filter_reads_as_the_window_sends_it() {
        let f: LogFilter = serde_json::from_value(serde_json::json!({
            "call": "K1", "band": "20m", "fromUnix": 5, "cqZone": 5, "ituZone": 8,
            "qsl": {"lotw": "notSent", "clublog": "sent"}, "need": "vucc", "text": "x"
        }))
        .unwrap();
        assert_eq!(f.need, Some(AwardNeed::Vucc));
        assert_eq!(f.qsl.clublog, Some(SentState::Sent));
        assert!(f.reads_sent() && f.decided_by_caller());
        let back = serde_json::to_value(&f).unwrap();
        assert_eq!(serde_json::from_value::<LogFilter>(back).unwrap(), f);
        assert!(serde_json::from_value::<LogFilter>(serde_json::json!({"bandd": "20m"})).is_err());
        assert!(serde_json::from_value::<LogFilter>(
            serde_json::json!({"qsl": {"clublog": "confirmed"}})
        )
        .is_err());
        assert!(LogFilter {
            call: "   ".into(),
            ..Default::default()
        }
        .is_empty());
    }

    /// What the county index costs a lifetime log, and what it buys: its size (pages with and
    /// without it, each store vacuumed), its build time — what an existing store pays once, the
    /// first time this build opens it — and a county filter's read with and without it.
    /// `cargo test -p tempo-core --release --lib -- --ignored --nocapture county_index_cost`
    /// (`COUNTY_INDEX_ROWS`, 150,000 by default; a third of the contacts carry a county).
    #[test]
    #[ignore = "a release-build bench, run by hand"]
    fn county_index_cost() {
        use std::time::Instant;
        let n: usize = std::env::var("COUNTY_INDEX_ROWS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(150_000);
        let dir = std::env::temp_dir().join(format!("nexus-county-index-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.sqlite3");
        let mut db = LogDb::open(&path).unwrap();
        let states = ["CT", "MA", "NY", "IL", "CA", "TX", "OH"];
        let mut records = Vec::with_capacity(n);
        for i in 0..n {
            let mut r = qso(
                &format!("K{}B{:03}", i % 10, i % 997),
                "20m",
                "FT8",
                i as u64 * 60,
            );
            r.id = Some(RecordId::Provisional {
                hash: i as u64,
                ordinal: 0,
            });
            let state = states[i % 7];
            r.state = Some(state.into());
            if i % 3 == 0 {
                r.extra
                    .push(("CNTY".into(), format!("{state},COUNTY{:02}", i % 40)));
            }
            r.extra.push(("SUBMODE".into(), "X".into()));
            records.push(r);
        }
        for chunk in records.chunks(4096) {
            db.insert_all(chunk.iter().map(|r| (r, Resolved::default())))
                .unwrap();
        }
        let filter = LogFilter {
            county: "MA,".into(),
            ..Default::default()
        };
        let read = |db: &LogDb| {
            let t = Instant::now();
            let mut found = 0;
            db.each_filtered(ORDER_COLUMNS, &[], false, &filter, &mut |_, _| found += 1)
                .unwrap();
            (found, t.elapsed().as_secs_f64() * 1000.0)
        };
        let (found, with_index) = read(&db);
        let pages = |db: &LogDb| -> i64 {
            db.conn
                .query_row("PRAGMA page_count", [], |r| r.get(0))
                .unwrap()
        };
        let page_size: i64 = db
            .conn
            .query_row("PRAGMA page_size", [], |r| r.get(0))
            .unwrap();
        db.conn.execute_batch("VACUUM").unwrap();
        let with = pages(&db);
        db.conn
            .execute_batch("DROP INDEX qso_extra_cnty; VACUUM")
            .unwrap();
        let without = pages(&db);
        // Without the index the county clause cannot run as written (it names its index), so the
        // same test is read through the passthrough's own key, once per contact.
        let t = Instant::now();
        let walked: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM qso q WHERE EXISTS (SELECT 1 FROM qso_extra c \
                 WHERE c.qso_id = q.id AND c.name = 'CNTY' AND c.value LIKE 'MA,%')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let without_index = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        db.conn
            .execute_batch(
                "CREATE INDEX qso_extra_cnty ON qso_extra(value COLLATE NOCASE) WHERE name = 'CNTY'",
            )
            .unwrap();
        let build = t.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(found as i64, walked, "both reads find the same contacts");
        println!(
            "COUNTY_INDEX rows={n} counties={} index={:.1} KiB ({} pages of {page_size} B) \
             store={:.1} MiB build={build:.0}ms county_filter_read={with_index:.1}ms \
             (found {found}) without_index_walk={without_index:.1}ms",
            n.div_ceil(3),
            ((with - without) * page_size) as f64 / 1024.0,
            with - without,
            (with * page_size) as f64 / (1024.0 * 1024.0),
        );
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
