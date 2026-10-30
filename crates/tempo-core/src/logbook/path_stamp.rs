//! The path a NEW contact took, as Nexus graded it when the contact was logged, and the beam
//! heading it was made on.
//!
//! **New fields only, never a changed one.** A contact gains these at log time, each through
//! [`super::fill_extra`], which never replaces a value the record already carries: a heading or
//! an antenna path the operator typed, or another program logged, stays exactly as it was. A
//! contact already in the log is never given them — the Logbook's grading of the existing log is
//! display only and lives outside the log.
//!
//! - **`APP_NEXUS_PATH`, `APP_NEXUS_PATH_CONF`, `APP_NEXUS_PATH_WHY`** — the graded route
//!   ([`StampRoute`]), the confidence ([`StampConf`]) and the reasons, with what the grade was
//!   made for (the far grid square and band, [`PathStamp::key`]) so a later edit of either can be
//!   told from the grade. App-private and an OPINION, not the operator's antenna data: kept in the
//!   operator's own log and files, and withheld from everything Nexus sends off the machine
//!   ([`super::Audience::Outbound`], [`is_path_field`]). Written all three or none.
//! - **`ANT_AZ`** (ADIF: the logging station's antenna azimuth, degrees) — from a connected
//!   rotator's reading at the time of the contact. Standard ADIF and the operator's own antenna
//!   data, so it travels like any other field.
//! - **`ANT_PATH`** (ADIF: `S` short path, `L` long path …) — by ADIF's definition the operator's
//!   antenna path, so a model's guess never goes there. Written `S` only when the grade is
//!   *confirmed* by the station's own heading ([`StampConf::Confirmed`]); a long path is never
//!   confirmed while the long path is unvalidated, so `L` is written only by the operator.
//!
//! The grading itself lives with the propagation model (src-tauri hands the engine a grader);
//! this module is the record's side of it, so every writer and reader agrees on the tags.

use super::{extra_value, fill_extra, QsoRecord};

/// The graded route.
pub const PATH_FIELD: &str = "APP_NEXUS_PATH";
/// The grade's confidence; absent when the route is neither short nor long.
pub const PATH_CONF_FIELD: &str = "APP_NEXUS_PATH_CONF";
/// The reasons behind the grade and what it was made for: `REASON,REASON;GRID4/BAND`.
pub const PATH_WHY_FIELD: &str = "APP_NEXUS_PATH_WHY";
/// ADIF: the logging station's antenna azimuth, degrees.
pub const ANT_AZ_FIELD: &str = "ANT_AZ";
/// ADIF: the logging station's antenna path.
pub const ANT_PATH_FIELD: &str = "ANT_PATH";

/// Whether `tag` is one of the app-private grade fields, withheld from every service.
pub fn is_path_field(tag: &str) -> bool {
    matches!(tag, PATH_FIELD | PATH_CONF_FIELD | PATH_WHY_FIELD)
}

/// The route a grade names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StampRoute {
    Short,
    Long,
    /// Both routes modelled within the margin — a real answer.
    Either,
    /// Both routes modelled closed: outside the model.
    Unknown,
}

impl StampRoute {
    pub fn tag(self) -> &'static str {
        match self {
            StampRoute::Short => "S",
            StampRoute::Long => "L",
            StampRoute::Either => "EITHER",
            StampRoute::Unknown => "UNKNOWN",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "S" => Some(StampRoute::Short),
            "L" => Some(StampRoute::Long),
            "EITHER" => Some(StampRoute::Either),
            "UNKNOWN" => Some(StampRoute::Unknown),
            _ => None,
        }
    }
}

/// How sure a grade is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StampConf {
    /// The station's own beam heading agreed, with the model's margin.
    Confirmed,
    Likely,
    Possible,
}

impl StampConf {
    pub fn tag(self) -> &'static str {
        match self {
            StampConf::Confirmed => "CONFIRMED",
            StampConf::Likely => "LIKELY",
            StampConf::Possible => "POSSIBLE",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "CONFIRMED" => Some(StampConf::Confirmed),
            "LIKELY" => Some(StampConf::Likely),
            "POSSIBLE" => Some(StampConf::Possible),
            _ => None,
        }
    }
}

/// A contact's grade as the log keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathStamp {
    pub route: StampRoute,
    /// `Short`/`Long` only.
    pub conf: Option<StampConf>,
    /// The reasons, as tokens the grader wrote (`MARGIN_S`, `HEADING_S` …).
    pub why: Vec<String>,
    /// What the grade was made for: the far station's 4-character grid square and the band,
    /// `FN31/20m`.
    pub key: String,
}

impl PathStamp {
    /// The `APP_NEXUS_PATH_WHY` value.
    fn why_value(&self) -> String {
        format!("{};{}", self.why.join(","), self.key)
    }
}

/// What the log-time grader found for a contact: the grade, and the beam heading (degrees true)
/// when a rotator's reading covers the contact. Either may be missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathAtLog {
    pub grade: Option<PathStamp>,
    pub ant_az: Option<u16>,
}

/// Give a contact being logged its grade and heading, as new fields only. The three grade fields
/// are written together, and only when the record carries none of them; `ANT_AZ` and `ANT_PATH`
/// each only when absent. `ANT_PATH` is `S` for a confirmed short path and nothing otherwise.
/// Returns whether anything was written.
pub fn stamp_path(r: &mut QsoRecord, found: &PathAtLog) -> bool {
    let mut wrote = false;
    if let Some(az) = found.ant_az.filter(|a| *a <= 360) {
        wrote |= fill_extra(r, ANT_AZ_FIELD, az.to_string());
    }
    let Some(g) = &found.grade else {
        return wrote;
    };
    let held = r.extra.iter().any(|(k, _)| is_path_field(k));
    if !held {
        fill_extra(r, PATH_FIELD, g.route.tag().to_string());
        if let Some(c) = g.conf {
            fill_extra(r, PATH_CONF_FIELD, c.tag().to_string());
        }
        fill_extra(r, PATH_WHY_FIELD, g.why_value());
        wrote = true;
    }
    if g.route == StampRoute::Short && g.conf == Some(StampConf::Confirmed) {
        wrote |= fill_extra(r, ANT_PATH_FIELD, "S".to_string());
    }
    wrote
}

/// The grade a record carries, if it carries a readable one.
pub fn read_path(r: &QsoRecord) -> Option<PathStamp> {
    let route = StampRoute::from_tag(extra_value(r, PATH_FIELD)?)?;
    let conf = extra_value(r, PATH_CONF_FIELD).and_then(StampConf::from_tag);
    let (why, key) = extra_value(r, PATH_WHY_FIELD)
        .and_then(|v| v.rsplit_once(';'))
        .map(|(why, key)| {
            (
                why.split(',')
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .collect(),
                key.trim().to_string(),
            )
        })
        .unwrap_or_default();
    Some(PathStamp {
        route,
        conf,
        why,
        key,
    })
}

/// The key a grade for `r` is made under: the far grid square (4 characters, upper case) and the
/// band, or `None` without either.
pub fn grade_key(r: &QsoRecord) -> Option<String> {
    let grid = r.grid.as_deref()?.trim();
    if grid.len() < 4 || r.band.trim().is_empty() {
        return None;
    }
    let g4: String = grid
        .chars()
        .take(4)
        .collect::<String>()
        .to_ascii_uppercase();
    Some(format!("{g4}/{}", r.band.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::logbook::parse_adif;

    fn rec() -> QsoRecord {
        parse_adif(
            "<EOH><CALL:6>JA1ABC<QSO_DATE:8>20261030<TIME_ON:6>120000<BAND:3>20m<MODE:3>FT8\
             <GRIDSQUARE:6>PM95vq<EOR>",
        )
        .remove(0)
    }

    fn grade(route: StampRoute, conf: Option<StampConf>) -> PathStamp {
        PathStamp {
            route,
            conf,
            why: vec!["MARGIN_S".into(), "HEADING_S".into()],
            key: "PM95/20m".into(),
        }
    }

    #[test]
    fn a_new_contact_gains_the_grade_and_the_heading() {
        let mut r = rec();
        let found = PathAtLog {
            grade: Some(grade(StampRoute::Short, Some(StampConf::Likely))),
            ant_az: Some(312),
        };
        assert!(stamp_path(&mut r, &found));
        assert_eq!(extra_value(&r, PATH_FIELD), Some("S"));
        assert_eq!(extra_value(&r, PATH_CONF_FIELD), Some("LIKELY"));
        assert_eq!(
            extra_value(&r, PATH_WHY_FIELD),
            Some("MARGIN_S,HEADING_S;PM95/20m")
        );
        assert_eq!(extra_value(&r, ANT_AZ_FIELD), Some("312"));
        // Likely is not confirmed: no antenna path.
        assert_eq!(extra_value(&r, ANT_PATH_FIELD), None);
        assert_eq!(read_path(&r), found.grade);
    }

    #[test]
    fn only_a_confirmed_short_path_writes_ant_path() {
        let mut r = rec();
        stamp_path(
            &mut r,
            &PathAtLog {
                grade: Some(grade(StampRoute::Short, Some(StampConf::Confirmed))),
                ant_az: None,
            },
        );
        assert_eq!(extra_value(&r, ANT_PATH_FIELD), Some("S"));
        for (route, conf) in [
            (StampRoute::Long, Some(StampConf::Possible)),
            (StampRoute::Long, Some(StampConf::Confirmed)),
            (StampRoute::Either, None),
            (StampRoute::Unknown, None),
            (StampRoute::Short, Some(StampConf::Likely)),
        ] {
            let mut r = rec();
            stamp_path(
                &mut r,
                &PathAtLog {
                    grade: Some(grade(route, conf)),
                    ant_az: None,
                },
            );
            assert_eq!(extra_value(&r, ANT_PATH_FIELD), None, "{route:?} {conf:?}");
        }
    }

    #[test]
    fn nothing_the_record_already_carries_is_replaced() {
        let mut r = rec();
        r.extra = vec![
            ("ANT_AZ".into(), "45".into()),
            ("ANT_PATH".into(), "L".into()),
            (PATH_FIELD.into(), "L".into()),
        ];
        let before = r.clone();
        stamp_path(
            &mut r,
            &PathAtLog {
                grade: Some(grade(StampRoute::Short, Some(StampConf::Confirmed))),
                ant_az: Some(312),
            },
        );
        // Nothing changed, and no half-grade was added beside the one it held.
        assert_eq!(r, before);
        // No grade and no heading: nothing at all.
        let mut r = rec();
        assert!(!stamp_path(&mut r, &PathAtLog::default()));
        assert_eq!(r, rec());
    }

    #[test]
    fn the_grade_stays_in_the_own_log_and_goes_to_no_service() {
        use crate::logbook::{adif_record, adif_record_own_log, adif_record_own_logger};
        let plain = rec();
        let mut r = rec();
        stamp_path(
            &mut r,
            &PathAtLog {
                grade: Some(grade(StampRoute::Short, Some(StampConf::Confirmed))),
                ant_az: Some(312),
            },
        );
        // The operator's own file carries all of it, and reads back as the same contact.
        let own = adif_record_own_log(&r);
        for tag in [
            PATH_FIELD,
            PATH_CONF_FIELD,
            PATH_WHY_FIELD,
            "ANT_AZ",
            "ANT_PATH",
        ] {
            assert!(
                own.contains(&format!("<{tag}:")),
                "{tag} missing from {own}"
            );
        }
        let back = parse_adif(&format!("<EOH>{own}")).remove(0);
        assert_eq!(back.extra, r.extra);
        assert_eq!(read_path(&back), read_path(&r));
        // A logger on the operator's own machine gets the operator's own record.
        assert_eq!(adif_record_own_logger(&r), own);
        // What goes off the machine: the antenna fields, never the grade — and nothing else moves.
        let out = adif_record(&r);
        assert!(!out.contains("APP_NEXUS_PATH"), "{out}");
        let expect = adif_record(&plain).replace("<EOR>", "<ANT_AZ:3>312<ANT_PATH:1>S<EOR>");
        assert_eq!(out, expect);
    }

    #[test]
    fn the_grade_key_is_the_far_square_and_band() {
        assert_eq!(grade_key(&rec()).as_deref(), Some("PM95/20m"));
        let mut r = rec();
        r.grid = Some("pm9".into());
        assert_eq!(grade_key(&r), None);
        r.grid = None;
        assert_eq!(grade_key(&r), None);
        // A grade read back with no key or an unknown route.
        let mut r = rec();
        r.extra = vec![(PATH_FIELD.into(), "SIDEWAYS".into())];
        assert_eq!(read_path(&r), None);
        r.extra = vec![(PATH_FIELD.into(), "EITHER".into())];
        assert_eq!(read_path(&r).unwrap().key, "");
    }
}
