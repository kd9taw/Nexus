//! The path a NEW contact took, as Nexus graded it when the contact was logged.
//!
//! **New fields only, never a changed one.** A contact gains these at log time, each through
//! [`super::fill_extra`], which never replaces a value the record already carries. A contact
//! already in the log is never given them — the Logbook's grading of the existing log is display
//! only and lives outside the log.
//!
//! **`APP_NEXUS_PATH`, `APP_NEXUS_PATH_CONF`, `APP_NEXUS_PATH_WHY`** — the graded route
//! ([`StampRoute`]), the confidence ([`StampConf`]) and the reasons, with what the grade was made
//! for (the far grid square and band, [`PathStamp::key`]) so a later edit of either can be told
//! from the grade. App-private and an OPINION, not the operator's antenna data: kept in the
//! operator's own log and files, and withheld from everything Nexus sends off the machine
//! ([`super::Audience::Outbound`], [`is_path_field`]). Written all three or none.
//!
//! ⛔ **Never `ANT_AZ` or `ANT_PATH`.** Those are the operator's antenna data by ADIF's
//! definition and travel to every service; Nexus does not know which antenna a rotator turns, so
//! it fills neither by itself (operator ruling). The operator sets `ANT_PATH` on the contact card,
//! and that value is theirs and travels like any field they type.
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
/// ADIF: the logging station's antenna azimuth, degrees — read (never written) for the expert's
/// heading agreement count.
pub const ANT_AZ_FIELD: &str = "ANT_AZ";

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
    Likely,
    Possible,
}

impl StampConf {
    pub fn tag(self) -> &'static str {
        match self {
            StampConf::Likely => "LIKELY",
            StampConf::Possible => "POSSIBLE",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
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

/// Give a contact being logged its grade, as new fields only: the three grade fields together,
/// and only when the record carries none of them. Returns whether they were written.
pub fn stamp_path(r: &mut QsoRecord, g: &PathStamp) -> bool {
    if r.extra.iter().any(|(k, _)| is_path_field(k)) {
        return false;
    }
    fill_extra(r, PATH_FIELD, g.route.tag().to_string());
    if let Some(c) = g.conf {
        fill_extra(r, PATH_CONF_FIELD, c.tag().to_string());
    }
    fill_extra(r, PATH_WHY_FIELD, g.why_value());
    true
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
            why: vec!["MARGIN_S".into(), "HEARD_S:2".into()],
            key: "PM95/20m".into(),
        }
    }

    #[test]
    fn a_new_contact_gains_the_grade_and_no_antenna_field() {
        let mut r = rec();
        let g = grade(StampRoute::Short, Some(StampConf::Likely));
        assert!(stamp_path(&mut r, &g));
        assert_eq!(extra_value(&r, PATH_FIELD), Some("S"));
        assert_eq!(extra_value(&r, PATH_CONF_FIELD), Some("LIKELY"));
        assert_eq!(
            extra_value(&r, PATH_WHY_FIELD),
            Some("MARGIN_S,HEARD_S:2;PM95/20m")
        );
        assert_eq!(read_path(&r), Some(g));
        // The operator's antenna data is never Nexus's to fill, whatever the grade.
        for (route, conf) in [
            (StampRoute::Short, Some(StampConf::Likely)),
            (StampRoute::Long, Some(StampConf::Possible)),
            (StampRoute::Either, None),
            (StampRoute::Unknown, None),
        ] {
            let mut r = rec();
            stamp_path(&mut r, &grade(route, conf));
            assert!(
                r.extra
                    .iter()
                    .all(|(k, _)| k != "ANT_AZ" && k != "ANT_PATH"),
                "{route:?}"
            );
            assert_eq!(
                r.extra.len(),
                2 + usize::from(conf.is_some()),
                "{route:?}: the grade fields and nothing else"
            );
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
        assert!(!stamp_path(
            &mut r,
            &grade(StampRoute::Short, Some(StampConf::Likely))
        ));
        // Nothing changed, and no half-grade was added beside the one it held.
        assert_eq!(r, before);
    }

    #[test]
    fn the_grade_stays_in_the_own_log_and_goes_to_no_service() {
        use crate::logbook::{adif_record, adif_record_own_log, adif_record_own_logger};
        let plain = rec();
        let mut r = rec();
        stamp_path(&mut r, &grade(StampRoute::Short, Some(StampConf::Likely)));
        // The operator's own file carries it, and reads back as the same contact.
        let own = adif_record_own_log(&r);
        for tag in [PATH_FIELD, PATH_CONF_FIELD, PATH_WHY_FIELD] {
            assert!(
                own.contains(&format!("<{tag}:")),
                "{tag} missing from {own}"
            );
        }
        let back = parse_adif(&format!("<EOH>{own}")).remove(0);
        assert_eq!(back.extra, r.extra);
        assert_eq!(read_path(&back), read_path(&r));
        assert_eq!(adif_record_own_logger(&r), own);
        // What goes off the machine is exactly the ungraded contact.
        assert_eq!(adif_record(&r), adif_record(&plain));
        // An antenna path the operator set travels as any field they type.
        let mut set = r.clone();
        crate::logbook::fill_extra(&mut set, "ANT_PATH", "L".into());
        assert!(adif_record(&set).contains("<ANT_PATH:1>L"));
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
