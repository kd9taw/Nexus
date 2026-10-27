//! Custom awards: an award the operator defines — a name and a list of what to work (calls,
//! prefixes, grids, references, or the values of one ADIF field), with optional band and mode
//! filters — counted worked and confirmed from the log like any other award.
//!
//! Each listed item is one slot. A contact fills every slot it matches:
//! * **Call** — the contact's call, or its home call without a portable designator
//!   (`W1AW/4` fills `W1AW`).
//! * **Prefix** — every listed prefix the home call starts with (`VK`, `VK2`).
//! * **Grid** — the listed field, square or subsquare the contact's grid starts with (`FN`, `FN31`,
//!   `FN31PR`).
//! * **Reference** — a park, summit, island or flora-and-fauna reference the contact carries.
//! * **Field** — the value the contact holds in the ADIF field the award names (`CNTY`,
//!   `MY_SIG_INFO`, `APP_…`), compared without regard to case.
//!
//! Confirmed means LoTW or a paper card unless the definition says any channel counts (eQSL and
//! the QRZ logbook too) — the operator's award, the operator's rule.
//!
//! The definitions are kept in Settings and move between stations as a file
//! ([`export_definitions`] / [`import_definitions`]): JSON, `{"format":
//! "nexus-custom-awards","version":1,"awards":[…]}`.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::model::ModeClass;

/// The `format` tag a definitions file carries.
pub const FILE_FORMAT: &str = "nexus-custom-awards";
/// The definitions file version this build writes and reads.
pub const FILE_VERSION: u32 = 1;
/// The most awards a station keeps, and the most items one award lists — bounds on a file
/// someone else wrote, so an import cannot make the award fold unbounded.
pub const MAX_AWARDS: usize = 64;
pub const MAX_TARGETS: usize = 20_000;

/// What a custom award's items are.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CustomKind {
    #[default]
    Call,
    Prefix,
    Grid,
    Reference,
    Field,
}

/// Which confirmations a custom award counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CustomConfirm {
    /// LoTW or a paper card — what the ARRL's awards count.
    #[default]
    LotwCard,
    /// Any channel the log holds: LoTW, card, eQSL or the QRZ logbook.
    Any,
}

/// One custom award, as the operator defined it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomAward {
    /// Stable within the station, so the view can name an award across edits.
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: CustomKind,
    /// The ADIF field a [`CustomKind::Field`] award reads, upper case. Empty for the other kinds.
    #[serde(default)]
    pub field: String,
    /// What to work: one slot each.
    #[serde(default)]
    pub targets: Vec<String>,
    /// Band labels ("20m") the award counts; empty = every band.
    #[serde(default)]
    pub bands: Vec<String>,
    /// Modes the award counts: an ADIF mode ("FT8") or a class ("CW", "PHONE", "DIGITAL");
    /// empty = every mode.
    #[serde(default)]
    pub modes: Vec<String>,
    #[serde(default)]
    pub confirmation: CustomConfirm,
}

impl CustomAward {
    /// The definition in its canonical form: trimmed, items and modes upper case, bands lower
    /// case, duplicates gone (first kept), a Field award's field name upper case. An item list
    /// that ends up empty, a blank name or a Field award with no field is refused.
    pub fn normalized(&self) -> Result<CustomAward, String> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err("a custom award needs a name".into());
        }
        let field = self.field.trim().to_ascii_uppercase();
        if self.kind == CustomKind::Field && field.is_empty() {
            return Err(format!(
                "{name}: a field award needs the ADIF field it reads"
            ));
        }
        let dedup = |items: &[String], upper: bool| -> Vec<String> {
            let mut seen = HashSet::new();
            items
                .iter()
                .map(|t| {
                    let t = t.trim();
                    if upper {
                        t.to_ascii_uppercase()
                    } else {
                        t.to_ascii_lowercase()
                    }
                })
                .filter(|t| !t.is_empty() && seen.insert(t.clone()))
                .collect()
        };
        let targets = dedup(&self.targets, true);
        if targets.is_empty() {
            return Err(format!("{name}: list at least one item to work"));
        }
        if targets.len() > MAX_TARGETS {
            return Err(format!("{name}: more than {MAX_TARGETS} items"));
        }
        Ok(CustomAward {
            id: self.id.trim().to_string(),
            name,
            kind: self.kind,
            field: if self.kind == CustomKind::Field {
                field
            } else {
                String::new()
            },
            targets,
            bands: dedup(&self.bands, false),
            modes: dedup(&self.modes, true),
            confirmation: self.confirmation,
        })
    }
}

/// Every definition normalised, each given an id where it has none or repeats an earlier one's.
pub fn normalize_all(defs: &[CustomAward]) -> Result<Vec<CustomAward>, String> {
    if defs.len() > MAX_AWARDS {
        return Err(format!("more than {MAX_AWARDS} custom awards"));
    }
    let mut ids = HashSet::new();
    let mut out = Vec::with_capacity(defs.len());
    for (i, d) in defs.iter().enumerate() {
        let mut n = d.normalized()?;
        if n.id.is_empty() || ids.contains(&n.id) {
            let mut k = i + 1;
            while ids.contains(&format!("custom-{k}")) {
                k += 1;
            }
            n.id = format!("custom-{k}");
        }
        ids.insert(n.id.clone());
        out.push(n);
    }
    Ok(out)
}

#[derive(Serialize, Deserialize)]
struct DefinitionsFile {
    format: String,
    version: u32,
    awards: Vec<CustomAward>,
}

/// The definitions as a file another station imports.
pub fn export_definitions(defs: &[CustomAward]) -> String {
    serde_json::to_string_pretty(&DefinitionsFile {
        format: FILE_FORMAT.into(),
        version: FILE_VERSION,
        awards: defs.to_vec(),
    })
    .unwrap_or_default()
}

/// The definitions a file holds, normalised ([`normalize_all`]); refused, by reason, when it is
/// not a definitions file or one of its awards is not one.
pub fn import_definitions(text: &str) -> Result<Vec<CustomAward>, String> {
    let file: DefinitionsFile =
        serde_json::from_str(text).map_err(|e| format!("not a custom awards file: {e}"))?;
    if file.format != FILE_FORMAT {
        return Err(format!(
            "not a custom awards file (format {:?})",
            file.format
        ));
    }
    if file.version > FILE_VERSION {
        return Err(format!(
            "made by a newer Nexus (version {}); this one reads version {FILE_VERSION}",
            file.version
        ));
    }
    normalize_all(&file.awards)
}

/// What a contact carries for the custom awards.
pub struct CustomContact<'a> {
    pub call: &'a str,
    pub band: &'a str,
    pub mode: &'a str,
    pub grid: Option<&'a str>,
    /// Every reference the contact carries (park, summit, island, flora and fauna), any case.
    pub refs: &'a [&'a str],
    /// The contact's value of an ADIF field, by name.
    pub field: &'a dyn Fn(&str) -> Option<String>,
    /// Confirmed by LoTW or a card.
    pub lotw_card: bool,
    /// Confirmed by any channel.
    pub any: bool,
}

/// One award's worked and confirmed slots, by item index.
#[derive(Debug, Clone, Default)]
struct Tally {
    worked: HashSet<u32>,
    confirmed: HashSet<u32>,
}

/// One custom award's standing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CustomAwardProgress {
    pub id: String,
    pub name: String,
    pub kind: CustomKind,
    pub total: usize,
    pub worked: usize,
    pub confirmed: usize,
    /// Items never worked, in the definition's order.
    pub unworked: Vec<String>,
    /// Items worked but not confirmed, in the definition's order.
    pub unconfirmed: Vec<String>,
}

/// The custom awards' fold: each definition's items, indexed, and what the log has filled.
#[derive(Debug, Clone, Default)]
pub struct CustomAwards {
    defs: Vec<CustomAward>,
    index: Vec<HashMap<String, u32>>,
    tallies: Vec<Tally>,
}

impl CustomAwards {
    pub fn new(defs: &[CustomAward]) -> Self {
        let defs: Vec<CustomAward> = defs.iter().filter_map(|d| d.normalized().ok()).collect();
        let index = defs
            .iter()
            .map(|d| {
                d.targets
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (t.clone(), i as u32))
                    .collect()
            })
            .collect();
        let tallies = vec![Tally::default(); defs.len()];
        Self {
            defs,
            index,
            tallies,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// The definitions, as normalised.
    pub fn defs(&self) -> &[CustomAward] {
        &self.defs
    }

    /// Fold one contact in; returns the `(award, item)` slots it filled — every one, confirmed or
    /// not, so a caller can tell which the contact would complete if it were confirmed.
    pub fn add(&mut self, c: &CustomContact<'_>) -> Vec<(u16, u32)> {
        let mut hits = Vec::new();
        let band = c.band.trim().to_ascii_lowercase();
        let mode = c.mode.trim().to_ascii_uppercase();
        let class = ModeClass::from_adif(&mode).label().to_ascii_uppercase();
        let call = c.call.trim().to_ascii_uppercase();
        let home = home_call(&call);
        for (a, d) in self.defs.iter().enumerate() {
            if !d.bands.is_empty() && !d.bands.contains(&band) {
                continue;
            }
            if !d.modes.is_empty() && !d.modes.iter().any(|m| *m == mode || *m == class) {
                continue;
            }
            let index = &self.index[a];
            let mut found: Vec<u32> = Vec::new();
            let mut look = |key: &str| {
                if let Some(&i) = index.get(key) {
                    if !found.contains(&i) {
                        found.push(i);
                    }
                }
            };
            match d.kind {
                CustomKind::Call => {
                    look(&call);
                    look(home);
                }
                CustomKind::Prefix => {
                    for end in 1..=home.len() {
                        if home.is_char_boundary(end) {
                            look(&home[..end]);
                        }
                    }
                }
                CustomKind::Grid => {
                    if let Some(g) = c.grid.map(|g| g.trim().to_ascii_uppercase()) {
                        for end in [2, 4, 6, 8] {
                            if g.len() >= end && g.is_char_boundary(end) {
                                look(&g[..end]);
                            }
                        }
                    }
                }
                CustomKind::Reference => {
                    for r in c.refs {
                        for one in r.split(',') {
                            let one = one.trim().to_ascii_uppercase();
                            if !one.is_empty() {
                                look(&one);
                            }
                        }
                    }
                }
                CustomKind::Field => {
                    if let Some(v) = (c.field)(&d.field) {
                        look(v.trim().to_ascii_uppercase().as_str());
                    }
                }
            }
            let confirmed = match d.confirmation {
                CustomConfirm::LotwCard => c.lotw_card,
                CustomConfirm::Any => c.any,
            };
            let t = &mut self.tallies[a];
            for i in found {
                t.worked.insert(i);
                if confirmed {
                    t.confirmed.insert(i);
                }
                hits.push((a as u16, i));
            }
        }
        hits
    }

    /// Whether item `item` of award `award` is confirmed.
    pub fn is_confirmed(&self, award: u16, item: u32) -> bool {
        self.tallies
            .get(award as usize)
            .is_some_and(|t| t.confirmed.contains(&item))
    }

    /// The award's name and the item's text.
    pub fn describe(&self, award: u16, item: u32) -> Option<(&str, &str)> {
        let d = self.defs.get(award as usize)?;
        Some((d.name.as_str(), d.targets.get(item as usize)?.as_str()))
    }

    /// Every award's standing, in the definitions' order.
    pub fn progress(&self) -> Vec<CustomAwardProgress> {
        self.defs
            .iter()
            .zip(&self.tallies)
            .map(|(d, t)| {
                let (mut unworked, mut unconfirmed) = (Vec::new(), Vec::new());
                for (i, item) in d.targets.iter().enumerate() {
                    let i = i as u32;
                    if !t.worked.contains(&i) {
                        unworked.push(item.clone());
                    } else if !t.confirmed.contains(&i) {
                        unconfirmed.push(item.clone());
                    }
                }
                CustomAwardProgress {
                    id: d.id.clone(),
                    name: d.name.clone(),
                    kind: d.kind,
                    total: d.targets.len(),
                    worked: t.worked.len(),
                    confirmed: t.confirmed.len(),
                    unworked,
                    unconfirmed,
                }
            })
            .collect()
    }
}

/// A call without its portable designators: the longest part between strokes.
fn home_call(call: &str) -> &str {
    call.split('/').max_by_key(|p| p.len()).unwrap_or(call)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str, kind: CustomKind, targets: &[&str]) -> CustomAward {
        CustomAward {
            name: name.into(),
            kind,
            targets: targets.iter().map(|t| t.to_string()).collect(),
            ..CustomAward::default()
        }
    }

    fn contact<'a>(
        call: &'a str,
        band: &'a str,
        mode: &'a str,
        grid: Option<&'a str>,
        refs: &'a [&'a str],
        field: &'a dyn Fn(&str) -> Option<String>,
        confirmed: bool,
    ) -> CustomContact<'a> {
        CustomContact {
            call,
            band,
            mode,
            grid,
            refs,
            field,
            lotw_card: confirmed,
            any: confirmed,
        }
    }

    const NO_FIELD: &dyn Fn(&str) -> Option<String> = &|_| None;

    /// A definitions file written by one station reads back the same on another — normalised,
    /// and with every field kept.
    #[test]
    fn definitions_round_trip_through_a_file() {
        let defs = normalize_all(&[
            CustomAward {
                id: "club".into(),
                name: " Club members ".into(),
                kind: CustomKind::Call,
                targets: vec!["w1aw".into(), "K1ABC".into(), "W1AW".into()],
                bands: vec!["20M".into()],
                modes: vec!["cw".into()],
                confirmation: CustomConfirm::Any,
                ..CustomAward::default()
            },
            CustomAward {
                name: "Counties".into(),
                kind: CustomKind::Field,
                field: "cnty".into(),
                targets: vec!["CT,Hartford".into()],
                ..CustomAward::default()
            },
        ])
        .unwrap();
        assert_eq!(defs[0].name, "Club members");
        assert_eq!(
            defs[0].targets,
            vec!["W1AW", "K1ABC"],
            "upper case, duplicate gone"
        );
        assert_eq!(defs[0].bands, vec!["20m"]);
        assert_eq!(defs[0].modes, vec!["CW"]);
        assert_eq!(defs[1].field, "CNTY");
        assert_eq!(defs[1].id, "custom-2", "an award with no id is given one");
        let file = export_definitions(&defs);
        assert!(file.contains("\"format\": \"nexus-custom-awards\""));
        assert_eq!(import_definitions(&file).unwrap(), defs);
    }

    /// A file that is not a definitions file, or holds an award that is not one, is refused with
    /// a reason and imports nothing.
    #[test]
    fn a_bad_definitions_file_is_refused() {
        assert!(import_definitions("{}").is_err());
        assert!(import_definitions("not json").is_err());
        assert!(import_definitions(r#"{"format":"other","version":1,"awards":[]}"#).is_err());
        assert!(
            import_definitions(r#"{"format":"nexus-custom-awards","version":9,"awards":[]}"#)
                .is_err()
        );
        let blank = r#"{"format":"nexus-custom-awards","version":1,
            "awards":[{"name":"","targets":["W1AW"]}]}"#;
        assert!(import_definitions(blank).is_err());
        let no_field = r#"{"format":"nexus-custom-awards","version":1,
            "awards":[{"name":"x","kind":"field","targets":["A"]}]}"#;
        assert!(import_definitions(no_field).is_err());
        let ok = r#"{"format":"nexus-custom-awards","version":1,
            "awards":[{"name":"x","kind":"grid","targets":["fn31"]}]}"#;
        assert_eq!(import_definitions(ok).unwrap()[0].targets, vec!["FN31"]);
    }

    /// Each kind fills the slots it should, worked and confirmed, with the band and mode filters
    /// applied.
    #[test]
    fn each_kind_counts_worked_and_confirmed() {
        let defs = [
            def("Calls", CustomKind::Call, &["W1AW", "K1ABC", "N0CALL"]),
            def("Prefixes", CustomKind::Prefix, &["VK", "VK2", "ZL"]),
            def("Grids", CustomKind::Grid, &["FN31", "EM", "JO65AB"]),
            def("Parks", CustomKind::Reference, &["US-1234", "KFF-0001"]),
            CustomAward {
                field: "CNTY".into(),
                ..def("Counties", CustomKind::Field, &["CT,HARTFORD", "MA,ESSEX"])
            },
            CustomAward {
                bands: vec!["20m".into()],
                modes: vec!["CW".into()],
                ..def("20 m CW calls", CustomKind::Call, &["W1AW"])
            },
        ];
        let mut c = CustomAwards::new(&defs);
        let cnty: &dyn Fn(&str) -> Option<String> =
            &|f| (f == "CNTY").then(|| "ct,hartford".to_string());
        c.add(&contact(
            "W1AW/4",
            "40m",
            "SSB",
            Some("fn31pr"),
            &[],
            cnty,
            true,
        ));
        c.add(&contact(
            "K1ABC",
            "20m",
            "FT8",
            Some("EM12"),
            &["us-1234,us-5678"],
            NO_FIELD,
            false,
        ));
        c.add(&contact(
            "VK2XYZ",
            "20m",
            "CW",
            None,
            &["KFF-0001"],
            NO_FIELD,
            true,
        ));
        c.add(&contact("W1AW", "20m", "SSB", None, &[], NO_FIELD, true));
        let p = c.progress();
        let get = |n: &str| p.iter().find(|x| x.name == n).unwrap().clone();
        let calls = get("Calls");
        assert_eq!((calls.worked, calls.confirmed, calls.total), (2, 1, 3));
        assert_eq!(calls.unconfirmed, vec!["K1ABC"]);
        assert_eq!(calls.unworked, vec!["N0CALL"]);
        let pfx = get("Prefixes");
        assert_eq!(
            (pfx.worked, pfx.confirmed),
            (2, 2),
            "VK2XYZ fills VK and VK2"
        );
        let grids = get("Grids");
        assert_eq!(
            (grids.worked, grids.confirmed),
            (2, 1),
            "FN31 confirmed, EM worked"
        );
        assert_eq!(grids.unworked, vec!["JO65AB"]);
        let parks = get("Parks");
        assert_eq!((parks.worked, parks.confirmed), (2, 1));
        assert_eq!(parks.unconfirmed, vec!["US-1234"]);
        let cn = get("Counties");
        assert_eq!((cn.worked, cn.confirmed), (1, 1));
        let filtered = get("20 m CW calls");
        assert_eq!(
            (filtered.worked, filtered.confirmed),
            (0, 0),
            "W1AW was worked on 40 m SSB and 20 m SSB, never 20 m CW"
        );
    }

    /// "Any channel" counts an eQSL; the default counts only LoTW or a card.
    #[test]
    fn the_confirmation_rule_is_the_definitions() {
        let strict = def("Strict", CustomKind::Call, &["W1AW"]);
        let any = CustomAward {
            confirmation: CustomConfirm::Any,
            ..def("Any", CustomKind::Call, &["W1AW"])
        };
        let mut c = CustomAwards::new(&[strict, any]);
        c.add(&CustomContact {
            lotw_card: false,
            any: true,
            ..contact("W1AW", "20m", "CW", None, &[], NO_FIELD, false)
        });
        let p = c.progress();
        assert_eq!(p[0].confirmed, 0);
        assert_eq!(p[1].confirmed, 1);
    }
}
