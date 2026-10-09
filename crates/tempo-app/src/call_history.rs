//! Call history — the operator's own file of what stations sent last time, in N1MM Logger+'s
//! call-history format. The contest strip fills its exchange boxes from it as a call is typed.
//!
//! The operator imports the file; Nexus never downloads or bundles one. N1MM's library of
//! user-contributed files publishes no licence, and the files hold other people's names and
//! locations, so a file stays where the operator put it: on this PC, never on the club wire and
//! never on Remote. The format is written from N1MM's own description of it, which makes it a
//! protocol reference rather than a port:
//!
//! - comma- or semicolon-delimited;
//! - `#` at the start of a line makes it a comment;
//! - a `!!Order!!` line names the columns (`!!Order!!,Call,Name,CK,Sect`), and without one the
//!   order is N1MM's fixed default, [`DEFAULT_ORDER`];
//! - any other `!!…!!` line is one of N1MM's own switches, and is read past here.
//!
//! A file is bound to ONE contest when it is imported. N1MM's own warning is that "it is easy to
//! forget that you have the wrong call history table in the database", and a file for last
//! spring's QSO party filling this weekend's Field Day boxes is exactly that mistake.
//!
//! This module only READS. What may go into a box — a value the contest's own list accepts, never
//! a guess, never over what the operator typed — is the strip's decision
//! (`ui/src/features/callHistoryFill.ts`), because the contests' value lists live in the UI.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// N1MM's column order for a file with no `!!Order!!` line.
pub const DEFAULT_ORDER: [&str; 15] = [
    "Call",
    "Name",
    "Loc1",
    "Loc2",
    "Sect",
    "State",
    "CK",
    "BirthDate",
    "Exch1",
    "Misc",
    "Power",
    "CqZone",
    "ITUZone",
    "UserText",
    "LastUpdateNote",
];
/// The operator's file as imported, byte for byte, in the directory the shell passes in.
pub const FILE_NAME: &str = "call_history.txt";
/// What was imported, when, and for which contest.
pub const META_NAME: &str = "call_history.meta.json";
/// A call-history file is a few hundred kilobytes; one past this is not one.
pub const MAX_BYTES: usize = 32 * 1024 * 1024;

/// A parsed file: the columns and, per call, the values it holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallHistory {
    /// The columns as the file orders them, in N1MM's spelling for the ones N1MM defines.
    pub fields: Vec<String>,
    /// Call → column → value. Only non-empty values; the call itself is the key.
    pub entries: BTreeMap<String, BTreeMap<String, String>>,
}

/// What was imported. Kept beside the file in app data, never in settings, so no profile or
/// backup carries another station's call history around.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoryMeta {
    /// The name of the file the operator picked, for display.
    pub file_name: String,
    /// The contest the file is for: a `FieldDayStatus::event` id (`ilqp`, `arrlfd`, …).
    pub contest: String,
    /// When it was imported (Unix seconds).
    pub imported_at: i64,
    /// Calls in it.
    pub count: usize,
    /// Its columns.
    pub fields: Vec<String>,
}

/// N1MM's spelling of a column name it defines (`CALL` → `Call`); any other name as written.
fn canonical(name: &str) -> String {
    DEFAULT_ORDER
        .iter()
        .find(|k| k.eq_ignore_ascii_case(name))
        .map_or_else(|| name.to_string(), |k| (*k).to_string())
}

/// Read a call-history file. `Err` says why it is not one, in words for the Settings line.
pub fn parse(text: &str) -> Result<CallHistory, String> {
    if text.len() > MAX_BYTES {
        return Err("the file is too large to be a call history".into());
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    // The file's delimiter, read off its first line that is not a comment: a semicolon file is
    // one whose first line has more semicolons than commas.
    let first = lines.clone().next().unwrap_or("");
    let delim = if first.matches(';').count() > first.matches(',').count() {
        ';'
    } else {
        ','
    };
    let mut fields: Vec<String> = DEFAULT_ORDER.iter().map(|k| (*k).to_string()).collect();
    let mut entries = BTreeMap::new();
    for line in lines {
        if line
            .get(..9)
            .is_some_and(|p| p.eq_ignore_ascii_case("!!Order!!"))
        {
            fields = line[9..]
                .split(delim)
                .map(str::trim)
                .filter(|f| !f.is_empty())
                .map(canonical)
                .collect();
            continue;
        }
        if line.starts_with("!!") {
            continue; // one of N1MM's own switches
        }
        let Some(at) = fields.iter().position(|f| f == "Call") else {
            return Err("its !!Order!! line names no Call column".into());
        };
        let cols: Vec<&str> = line.split(delim).map(str::trim).collect();
        let call = cols
            .get(at)
            .map_or_else(String::new, |c| c.to_ascii_uppercase());
        if !crate::scp::is_call_shaped(&call) {
            continue;
        }
        let row = fields
            .iter()
            .zip(&cols)
            .filter(|(f, v)| f.as_str() != "Call" && !v.is_empty())
            .map(|(f, v)| (f.clone(), (*v).to_string()))
            .collect();
        entries.insert(call, row);
    }
    if entries.is_empty() {
        return Err("no calls were found in it".into());
    }
    Ok(CallHistory { fields, entries })
}

/// Import `text` for `contest`. The file is parsed FIRST, and only a file that reads replaces the
/// one held: a bad pick keeps the history the operator already had.
pub fn import(
    dir: &Path,
    text: &str,
    file_name: &str,
    contest: &str,
    now: i64,
) -> Result<HistoryMeta, String> {
    let parsed = parse(text)?;
    let meta = HistoryMeta {
        file_name: file_name.to_string(),
        contest: contest.to_string(),
        imported_at: now,
        count: parsed.entries.len(),
        fields: parsed.fields,
    };
    let json = serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir)
        .and_then(|()| crate::scp::write_atomic(&dir.join(FILE_NAME), text.as_bytes()))
        .and_then(|()| crate::scp::write_atomic(&dir.join(META_NAME), &json))
        .map_err(|e| format!("the file could not be saved: {e}"))?;
    Ok(meta)
}

/// What was imported, if anything.
pub fn status(dir: &Path) -> Option<HistoryMeta> {
    let bytes = std::fs::read(dir.join(META_NAME)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The imported file, read again, with what was recorded about it.
pub fn load(dir: &Path) -> Option<(HistoryMeta, CallHistory)> {
    let meta = status(dir)?;
    let text = std::fs::read(dir.join(FILE_NAME)).ok()?;
    let parsed = parse(&String::from_utf8_lossy(&text)).ok()?;
    Some((meta, parsed))
}

/// Forget the imported file: the operator's Clear.
pub fn clear(dir: &Path) -> std::io::Result<()> {
    for name in [META_NAME, FILE_NAME] {
        match std::fs::remove_file(dir.join(name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-callhist-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_order_line_names_the_columns() {
        let h = parse("!!Order!!,Call,Name,Loc1\nK9AAA,BOB,COOK\n").unwrap();
        assert_eq!(h.fields, vec!["Call", "Name", "Loc1"]);
        assert_eq!(h.entries.len(), 1);
        assert_eq!(
            h.entries["K9AAA"],
            row(&[("Name", "BOB"), ("Loc1", "COOK")])
        );
    }

    #[test]
    fn a_semicolon_file_and_a_comment_line_read_the_same() {
        let comma = parse("!!Order!!,Call,Name,Loc1\nK9AAA,BOB,COOK\n").unwrap();
        let semi =
            parse("# Illinois, from last year\r\n!!Order!!;Call;Name;Loc1\r\nK9AAA;BOB;COOK\r\n")
                .unwrap();
        assert_eq!(semi, comma);
    }

    #[test]
    fn without_an_order_line_the_columns_are_n1mms_default() {
        let h = parse("K9AAA,BOB,COOK,,EMA\n").unwrap();
        assert_eq!(h.fields.len(), DEFAULT_ORDER.len());
        // Loc2 is empty, so it is simply absent: a blank is never a value.
        assert_eq!(
            h.entries["K9AAA"],
            row(&[("Name", "BOB"), ("Loc1", "COOK"), ("Sect", "EMA")])
        );
    }

    #[test]
    fn column_names_take_n1mms_spelling_and_calls_are_upper_case() {
        let h = parse("!!ORDER!!,CALL,sect,cqzone\nk9aaa, il ,4\n").unwrap();
        assert_eq!(h.fields, vec!["Call", "Sect", "CqZone"]);
        assert_eq!(h.entries["K9AAA"], row(&[("Sect", "il"), ("CqZone", "4")]));
    }

    #[test]
    fn a_line_without_a_call_is_skipped_and_a_file_without_calls_is_refused() {
        let h = parse("!!Order!!,Call,Name\n,NOBODY\nK9AAA,BOB\n!!MapStateToSect!!\n").unwrap();
        assert_eq!(h.entries.keys().collect::<Vec<_>>(), vec!["K9AAA"]);
        assert!(parse("# nothing here\n").is_err());
        assert!(parse("").is_err());
        assert!(
            parse("!!Order!!,Name,Sect\nBOB,IL\n").is_err(),
            "a file whose columns name no Call is not a call history"
        );
    }

    #[test]
    fn an_import_is_bound_to_its_contest_and_a_bad_pick_keeps_the_last_one() {
        let d = dir("import");
        let meta = import(
            &d,
            "!!Order!!,Call,Loc1\nK9AAA,COOK\nW9XYZ,LAKE\n",
            "il.txt",
            "ilqp",
            7,
        )
        .unwrap();
        assert_eq!(
            (meta.contest.as_str(), meta.count, meta.file_name.as_str()),
            ("ilqp", 2, "il.txt")
        );
        let (held, history) = load(&d).unwrap();
        assert_eq!(held, meta);
        assert_eq!(history.entries["W9XYZ"], row(&[("Loc1", "LAKE")]));
        assert!(import(&d, "not a call history\n", "junk.txt", "wfd", 8).is_err());
        assert_eq!(
            status(&d).unwrap().contest,
            "ilqp",
            "the bad pick replaced nothing"
        );
        clear(&d).unwrap();
        assert!(load(&d).is_none());
        assert!(clear(&d).is_ok(), "clearing nothing is not an error");
    }
}
