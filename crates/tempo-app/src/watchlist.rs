//! The operator's watch list, as the station holds it — for the Needed board.
//!
//! The list is the DESKTOP's: Settings ▸ Spots & Alerts edits it and keeps it in ui-state.json
//! (`nexus.watchlist`, a blob this side deliberately never parses). The Needed board puts every
//! heard station it names at the top, even one nothing else needs (operator 2026-09-24: "watched
//! counts as needed"), and a Remote browser is served the board the station computes — so the
//! station needs the list too. The desktop's main window sends it whole on launch and after every
//! edit ([`crate::engine::Engine::set_watch_list`]); it is held in memory, never written.
//!
//! [`watched`] is the desktop's `watchedEntry` (`ui/src/watchlist.ts`), the matcher behind the
//! WATCH tile on the Call Roster, the Stations list and Spots. An entry that names a station there
//! must name it on the board, so both are held to one table of cases,
//! `tests/fixtures/watch-matches.json`, which the desktop's `watchlist.agreement.test.ts` reads too.

use serde::{Deserialize, Serialize};

/// What an entry names: a call or `*` pattern, a DXCC entity, or a grid square — the desktop's
/// `WatchKind`, whose spelling the wire carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WatchKind {
    Call,
    Dxcc,
    Grid,
}

/// One entry, as the desktop sends it: only what NAMES a station. The list's alert gates (CQ only,
/// a minimum SNR) are the alert's and stay on the desktop; the board, like the WATCH tile, matches
/// identity alone — a station does not leave the watch list when it stops calling CQ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchEntry {
    pub kind: WatchKind,
    pub value: String,
}

/// `text` against an entry's `pattern`, exactly as the desktop's `matchCallPattern` does:
/// case-insensitive, the pattern trimmed, `*` any run of characters (none included) and every
/// other character literal, anchored at both ends. An empty pattern names nothing; a bare `*`
/// names everything, as it does on the desktop.
fn glob(text: &str, pattern: &str) -> bool {
    let p = pattern.trim().to_uppercase();
    if p.is_empty() {
        return false;
    }
    let t = text.to_uppercase();
    let parts: Vec<&str> = p.split('*').collect();
    let &[first, .., last] = parts.as_slice() else {
        return t == p; // no star: the call itself
    };
    // The fixed head and tail may not share characters: `AB*BA` does not name `ABA`.
    if t.len() < first.len() + last.len() || !t.starts_with(first) || !t.ends_with(last) {
        return false;
    }
    let mut rest = &t[first.len()..t.len() - last.len()];
    for &middle in &parts[1..parts.len() - 1] {
        match rest.find(middle) {
            Some(at) => rest = &rest[at + middle.len()..],
            None => return false,
        }
    }
    true
}

/// The first entry of `list` that names this station — by call or `*` pattern, by DXCC entity (the
/// cty.dat name, compared whole and case-insensitively), or by grid (only when the evidence carried
/// one: a grid-less station is unknown, never a hit) — or `None`.
pub fn watched<'a>(
    list: &'a [WatchEntry],
    call: &str,
    entity: Option<&str>,
    grid: Option<&str>,
) -> Option<&'a WatchEntry> {
    let call = call.to_uppercase();
    if call.is_empty() {
        return None;
    }
    list.iter().find(|e| match e.kind {
        WatchKind::Call => glob(&call, &e.value),
        WatchKind::Dxcc => {
            let entity = entity.unwrap_or("").trim().to_uppercase();
            !entity.is_empty() && entity == e.value.trim().to_uppercase()
        }
        WatchKind::Grid => {
            let grid = grid.unwrap_or("").trim().to_uppercase();
            !grid.is_empty() && glob(&grid, &e.value)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ONE MATCHER, TWO LANGUAGES: the station and the desktop answer every case in the shared
    /// table alike. The desktop's half is `ui/src/watchlist.agreement.test.ts`, over the same file.
    #[test]
    fn the_watch_matcher_agrees_with_the_desktop_on_every_case_in_the_shared_table() {
        #[derive(Deserialize)]
        struct Subject {
            call: String,
            entity: Option<String>,
            grid: Option<String>,
        }
        #[derive(Deserialize)]
        struct Case {
            entry: WatchEntry,
            subject: Subject,
            names: bool,
        }
        #[derive(Deserialize)]
        struct Table {
            cases: Vec<Case>,
        }
        let table: Table =
            serde_json::from_str(include_str!("../tests/fixtures/watch-matches.json")).unwrap();
        assert!(table.cases.len() >= 20, "the table is there");
        let wrong: Vec<String> = table
            .cases
            .iter()
            .filter(|c| {
                let list = std::slice::from_ref(&c.entry);
                let s = &c.subject;
                watched(list, &s.call, s.entity.as_deref(), s.grid.as_deref()).is_some() != c.names
            })
            .map(|c| {
                format!(
                    "{:?} {:?} on {:?}",
                    c.entry.kind, c.entry.value, c.subject.call
                )
            })
            .collect();
        assert!(
            wrong.is_empty(),
            "the station disagrees with the table: {wrong:?}"
        );
    }

    /// The FIRST entry that names the station answers — the desktop's order.
    #[test]
    fn the_first_entry_that_names_the_station_answers() {
        let list = [
            WatchEntry {
                kind: WatchKind::Dxcc,
                value: "Bouvet".into(),
            },
            WatchEntry {
                kind: WatchKind::Call,
                value: "3Y0*".into(),
            },
        ];
        let hit = watched(&list, "3Y0J", Some("Bouvet"), None).unwrap();
        assert_eq!(hit.kind, WatchKind::Dxcc);
        let hit = watched(&list, "3Y0J", None, None).unwrap();
        assert_eq!(hit.kind, WatchKind::Call);
        assert!(watched(&list, "K1ABC", Some("United States"), None).is_none());
        assert!(watched(&[], "3Y0J", Some("Bouvet"), None).is_none());
    }

    /// The desktop's spelling, both ways: `{"kind":"call","value":"VP8*"}`.
    #[test]
    fn an_entry_reads_and_writes_the_desktops_spelling() {
        let e: WatchEntry = serde_json::from_str(r#"{"kind":"grid","value":"EM7*"}"#).unwrap();
        assert_eq!(
            e,
            WatchEntry {
                kind: WatchKind::Grid,
                value: "EM7*".into()
            }
        );
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"kind":"grid","value":"EM7*"}"#
        );
        assert!(serde_json::from_str::<WatchEntry>(r#"{"kind":"band","value":"20m"}"#).is_err());
    }
}
