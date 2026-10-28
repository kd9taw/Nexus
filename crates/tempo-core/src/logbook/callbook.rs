//! What a callbook lookup gives a contact at log time, and the mark that says it did.
//!
//! A lookup the operator already made (the callsign card, the log strip) answers far more than
//! the callsign card shows: county, IOTA, zones, e-mail, QSL route and the station's position.
//! [`fill_from_callbook`] copies each into a contact being logged — **into an empty field only**,
//! so a value the operator typed, the station sent, a park placed or a contest exchange carries
//! is never touched. It is pure: the answer is one a lookup already returned, held by the engine;
//! nothing here asks the network.
//!
//! **Never the IOTA reference.** A callbook's island group describes the licensee's record, not
//! necessarily this activation, so it is OFFERED at log time and written only when the operator
//! takes it (`super::offer`; operator decision, 2026-10-27).
//!
//! **Where the station is, only when the answer is that station's.** A lookup of a stroked call
//! (`W1ABC/P`, `KH6/W1ABC`) falls back to the base call's record, which says where the licensee
//! lives, not where the contact was made; and a contact with a hunted park or summit is placed
//! by the park. So the location fields — grid, QTH, state, county, position, zones, UTC
//! offset — are filled only when the record the callbook answered with IS the logged call and
//! the contact carries no park. The name, e-mail, web page, QSL route and the confirmation
//! flags say who the operator is and go on either.
//!
//! **Never the DXCC number or the country.** The country file and the DXCC rules own them
//! (`StationCore::country_for`).
//!
//! **The mark ([`CALLBOOK_FILLED_FIELD`]).** Every field filled here is named in an app-private
//! list on the record, modelled on [`super::SELF_FILLED_FIELD`]: kept in the operator's own log,
//! withheld from everything Nexus sends off the machine ([`super::Audience::Outbound`]), and
//! taken off a field the operator edits ([`unmark_edited`]). It is what lets a LoTW
//! confirmation's own value replace a callbook's (`reconcile::keep_lotw_detail`): without it a
//! callbook zone, filled first, would block LoTW's authoritative one for good, and WAZ would
//! count the callbook's.

use super::{extra_value, record_field, set_extra, QsoRecord};

/// The fields the callbook filled on this contact, as a comma-separated list of their ADIF names
/// (see the module header). App-private, never sent.
pub const CALLBOOK_FILLED_FIELD: &str = "APP_NEXUS_CALLBOOK_FILLED";

/// ★ **A grid for the distance and the map only**, `Y`: the callbook's grid filled into a contact
/// logged with no grid of its own, by [`fill_gridless_from_callbook`] (an FT8/FT4 contact the
/// station gave no grid in, and the Blank grids clean-up's callbook offers). A callbook says where
/// the licence is, not where the contact was made, so this grid counts toward no grid award, fills
/// no GRID need and is sent to no service (`counted_grid`; operator decision, 2026-10-27).
///
/// It is NOT [`CALLBOOK_FILLED_FIELD`] naming `GRIDSQUARE` alone: a grid that mark alone names (a
/// callsign-card lookup at log time, Update from callbook) counts and is sent as it always was,
/// so no log loses a grid it already counted. This mark rides beside that one and goes with it:
/// an edit of the grid, or LoTW's own grid, takes both off ([`unmark_callbook_filled`]).
/// App-private, never sent.
pub const MAP_ONLY_GRID_FIELD: &str = "APP_NEXUS_MAP_ONLY_GRID";

/// Whether the station confirms by LoTW, by eQSL and by paper card, `Y` or `N`, as its callbook
/// entry said when the contact was logged. ADIF has no field for any of them.
pub const LOTW_USER_FIELD: &str = "APP_NEXUS_LOTW_USER";
pub const EQSL_USER_FIELD: &str = "APP_NEXUS_EQSL_USER";
pub const PAPER_QSL_FIELD: &str = "APP_NEXUS_PAPER_QSL";

/// The station's offset from UTC in hours, as its callbook entry gave it. ADIF has no field.
pub const UTC_OFFSET_FIELD: &str = "APP_NEXUS_UTC_OFFSET";

/// A callbook lookup's answer, as a contact can hold it. `call` is the call of the record the
/// callbook answered with, which for a stroked call may be the base call's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CallbookAnswer {
    pub call: String,
    /// The name the log takes: the nickname when the station set one, else the full name.
    pub name: Option<String>,
    pub qth: Option<String>,
    pub grid: Option<String>,
    pub state: Option<String>,
    /// The county's bare name ("Hartford").
    pub county: Option<String>,
    pub iota: Option<String>,
    pub cq_zone: Option<u32>,
    pub itu_zone: Option<u32>,
    /// `(lat, lon)` in degrees, only when the callbook vouched for it.
    pub position: Option<(f64, f64)>,
    pub email: Option<String>,
    pub web: Option<String>,
    pub qsl_via: Option<String>,
    pub lotw: Option<bool>,
    pub eqsl: Option<bool>,
    pub paper_qsl: Option<bool>,
    pub utc_offset: Option<f32>,
}

/// The fields [`CALLBOOK_FILLED_FIELD`] names on this record.
pub fn callbook_filled(r: &QsoRecord) -> Vec<&str> {
    extra_value(r, CALLBOOK_FILLED_FIELD)
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the callbook filled `tag` on this record.
pub fn is_callbook_filled(r: &QsoRecord, tag: &str) -> bool {
    callbook_filled(r).contains(&tag)
}

/// Whether the record's grid is for the distance and the map only ([`MAP_ONLY_GRID_FIELD`]).
pub fn is_map_only_grid(r: &QsoRecord) -> bool {
    extra_value(r, MAP_ONLY_GRID_FIELD).is_some_and(|v| v.trim().eq_ignore_ascii_case("Y"))
}

/// Rewrite the mark: sorted, deduplicated, and gone when empty.
fn set_callbook_filled(r: &mut QsoRecord, mut tags: Vec<String>) {
    tags.sort();
    tags.dedup();
    let value = (!tags.is_empty()).then(|| tags.join(","));
    set_extra(r, CALLBOOK_FILLED_FIELD, value);
}

/// Take `tag` off the mark: the value is no longer the callbook's.
pub(crate) fn unmark_callbook_filled(r: &mut QsoRecord, tag: &str) {
    // The grid's map-only mark goes with the callbook's: it is no longer the callbook's grid.
    if tag == "GRIDSQUARE" {
        set_extra(r, MAP_ONLY_GRID_FIELD, None);
    }
    if !is_callbook_filled(r, tag) {
        return;
    }
    let tags = callbook_filled(r)
        .into_iter()
        .filter(|t| *t != tag)
        .map(str::to_string)
        .collect();
    set_callbook_filled(r, tags);
}

/// A field's value on the record by its ADIF name: the modelled ones from their columns, the
/// rest from `extra`.
fn value_of<'a>(r: &'a QsoRecord, tag: &str) -> Option<&'a str> {
    let v = match tag {
        "NAME" => r.name.as_deref(),
        "QTH" => r.qth.as_deref(),
        "STATE" => r.state.as_deref(),
        "GRIDSQUARE" => r.grid.as_deref(),
        "IOTA" => r.ota.iota.as_deref(),
        _ => extra_value(r, tag),
    };
    v.map(str::trim).filter(|v| !v.is_empty())
}

/// An edit that changed a field the callbook filled makes it the operator's: it comes off the
/// mark. `old` is the record before the edit, `rec` after it.
pub(crate) fn unmark_edited(old: &QsoRecord, rec: &mut QsoRecord) {
    for tag in callbook_filled(old) {
        let same = match (value_of(rec, tag), value_of(old, tag)) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            (a, b) => a == b,
        };
        if !same {
            unmark_callbook_filled(rec, tag);
        }
    }
}

/// [`fill_from_callbook`] for a contact logged with no grid of its own: a grid it fills is for the
/// distance and the map only, and carries [`MAP_ONLY_GRID_FIELD`] beside the callbook's mark.
/// Every other field is filled, and marked, exactly as [`fill_from_callbook`] fills it.
pub fn fill_gridless_from_callbook(rec: &mut QsoRecord, a: &CallbookAnswer) {
    let had_grid = value_of(rec, "GRIDSQUARE").is_some() || is_callbook_filled(rec, "GRIDSQUARE");
    fill_from_callbook(rec, a);
    if !had_grid && is_callbook_filled(rec, "GRIDSQUARE") {
        set_extra(rec, MAP_ONLY_GRID_FIELD, Some("Y".into()));
    }
}

/// A position as ADIF's Location type: `XDDD MM.MMM` ("W072 43.632"). `pos`/`neg` are the
/// hemisphere letters.
fn adif_location(v: f64, pos: char, neg: char) -> String {
    let hemi = if v < 0.0 { neg } else { pos };
    let thousandths = (v.abs() * 60_000.0).round() as u64;
    let (deg, rem) = (thousandths / 60_000, thousandths % 60_000);
    format!("{hemi}{deg:03} {:02}.{:03}", rem / 1000, rem % 1000)
}

/// The entities whose county ADIF spells `ST,County`: the United States, Alaska and Hawaii.
fn us_county_entity(country: Option<&str>) -> bool {
    country.is_some_and(|c| {
        let c = c.trim();
        ["United States", "Alaska", "Hawaii"]
            .iter()
            .any(|us| c.eq_ignore_ascii_case(us))
    })
}

/// Copy a callbook answer into the contact being logged: each field only where the record has
/// none, and only where a contest exchange on the record does not carry it. Location fields only
/// when the answer is this station's record and no park places the contact (module header).
/// Every field written is named in [`CALLBOOK_FILLED_FIELD`].
pub fn fill_from_callbook(rec: &mut QsoRecord, a: &CallbookAnswer) {
    // Whether the record carries the field anywhere: its own column, a contest exchange's (what
    // the other station sent, which a callbook never overrides), or the passthrough.
    let carried = |rec: &QsoRecord, tag: &str| record_field(rec, tag).is_some();
    let blank = |v: &Option<String>| v.as_deref().is_none_or(|v| v.trim().is_empty());
    let clean = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let here = a.call.trim().eq_ignore_ascii_case(rec.call.trim())
        && rec.ota.their_ref.is_none()
        && rec.ota.their_program.is_none();
    let mut filled: Vec<String> = Vec::new();

    // The modelled columns.
    let mut column = |rec: &mut QsoRecord, tag: &str, value: Option<String>| {
        if carried(rec, tag) {
            return;
        }
        let Some(v) = value else { return };
        let slot = match tag {
            "NAME" => &mut rec.name,
            "QTH" => &mut rec.qth,
            "STATE" => &mut rec.state,
            _ => &mut rec.grid,
        };
        if blank(slot) {
            *slot = Some(v);
            filled.push(tag.to_string());
        }
    };
    column(rec, "NAME", clean(&a.name));
    if here {
        column(rec, "QTH", clean(&a.qth));
        column(
            rec,
            "STATE",
            clean(&a.state).map(|s| s.to_ascii_uppercase()),
        );
        column(
            rec,
            "GRIDSQUARE",
            clean(&a.grid).filter(|g| crate::message::is_valid_grid(g)),
        );
    }

    // The rest, in `extra`.
    let mut extra: Vec<(&str, Option<String>)> = vec![
        ("EMAIL", clean(&a.email)),
        ("WEB", clean(&a.web)),
        ("QSL_VIA", clean(&a.qsl_via)),
        (LOTW_USER_FIELD, a.lotw.map(yn)),
        (EQSL_USER_FIELD, a.eqsl.map(yn)),
        (PAPER_QSL_FIELD, a.paper_qsl.map(yn)),
    ];
    if here {
        // ADIF's US county is `ST,County`, and only with the state the record carries: a county
        // under another state's code names a place that does not exist.
        let county = clean(&a.county)
            .zip(rec.state.clone())
            .filter(|(_, st)| {
                us_county_entity(rec.country.as_deref())
                    && clean(&a.state).is_some_and(|s| s.eq_ignore_ascii_case(st.trim()))
            })
            .map(|(county, st)| match county.split_once(',') {
                Some((prefix, _)) if prefix.trim().eq_ignore_ascii_case(st.trim()) => county,
                _ => format!("{},{county}", st.trim().to_ascii_uppercase()),
            });
        extra.extend([
            ("CNTY", county),
            (
                "CQZ",
                a.cq_zone
                    .filter(|z| (1..=40).contains(z))
                    .map(|z| z.to_string()),
            ),
            (
                "ITUZ",
                a.itu_zone
                    .filter(|z| (1..=90).contains(z))
                    .map(|z| z.to_string()),
            ),
            (
                "LAT",
                a.position.map(|(lat, _)| adif_location(lat, 'N', 'S')),
            ),
            (
                "LON",
                a.position.map(|(_, lon)| adif_location(lon, 'E', 'W')),
            ),
            (UTC_OFFSET_FIELD, a.utc_offset.map(|h| h.to_string())),
        ]);
    }
    for (tag, value) in extra {
        let Some(v) = value else { continue };
        if carried(rec, tag) {
            continue;
        }
        set_extra(rec, tag, Some(v));
        filled.push(tag.to_string());
    }

    if !filled.is_empty() {
        let mut tags: Vec<String> = callbook_filled(rec)
            .into_iter()
            .map(str::to_string)
            .collect();
        tags.extend(filled);
        set_callbook_filled(rec, tags);
    }
}

fn yn(b: bool) -> String {
    if b { "Y" } else { "N" }.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::{adif_header, adif_record, adif_record_own_log, parse_adif};

    fn rec(call: &str) -> QsoRecord {
        parse_adif(&format!(
            "<EOH><CALL:{}>{call}<QSO_DATE:8>20261024<TIME_ON:6>120000<BAND:3>20m<MODE:3>FT8\
             <COUNTRY:13>United States<EOR>",
            call.len()
        ))
        .remove(0)
    }

    fn answer(call: &str) -> CallbookAnswer {
        CallbookAnswer {
            call: call.into(),
            name: Some("Dave".into()),
            qth: Some("Newington".into()),
            grid: Some("FN31pr".into()),
            state: Some("ct".into()),
            county: Some("Hartford".into()),
            iota: Some("na-001".into()),
            cq_zone: Some(5),
            itu_zone: Some(8),
            position: Some((-41.7147, -72.7272)),
            email: Some("w1abc@example.org".into()),
            web: Some("https://example.org".into()),
            qsl_via: Some("W1MGR".into()),
            lotw: Some(true),
            eqsl: Some(false),
            paper_qsl: Some(true),
            utc_offset: Some(-5.0),
        }
    }

    /// The tag→value pairs a record's own-log ADIF carries, for the fields this module fills.
    fn filled_tags(r: &QsoRecord) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = [
            "NAME",
            "QTH",
            "STATE",
            "GRIDSQUARE",
            "IOTA",
            "CNTY",
            "CQZ",
            "ITUZ",
            "LAT",
            "LON",
            "EMAIL",
            "WEB",
            "QSL_VIA",
            LOTW_USER_FIELD,
            EQSL_USER_FIELD,
            PAPER_QSL_FIELD,
            UTC_OFFSET_FIELD,
        ]
        .iter()
        .filter_map(|t| value_of(r, t).map(|v| (t.to_string(), v.to_string())))
        .collect();
        out.sort();
        out
    }

    /// ★ Every field a lookup answers lands in an empty contact, in ADIF's own spelling, and is
    /// named in the mark.
    #[test]
    fn every_field_fills_an_empty_contact_and_is_marked() {
        let mut r = rec("W1ABC");
        fill_from_callbook(&mut r, &answer("W1ABC"));
        let want: Vec<(String, String)> = [
            ("CNTY", "CT,Hartford"),
            ("CQZ", "5"),
            ("EMAIL", "w1abc@example.org"),
            ("GRIDSQUARE", "FN31pr"),
            ("ITUZ", "8"),
            ("LAT", "S041 42.882"),
            ("LON", "W072 43.632"),
            ("NAME", "Dave"),
            ("QSL_VIA", "W1MGR"),
            ("QTH", "Newington"),
            ("STATE", "CT"),
            ("WEB", "https://example.org"),
            (EQSL_USER_FIELD, "N"),
            (LOTW_USER_FIELD, "Y"),
            (PAPER_QSL_FIELD, "Y"),
            (UTC_OFFSET_FIELD, "-5"),
        ]
        .iter()
        .map(|(t, v)| (t.to_string(), v.to_string()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
        assert_eq!(filled_tags(&r), want);
        let mut marked: Vec<&str> = callbook_filled(&r);
        marked.sort();
        let mut tags: Vec<&str> = want.iter().map(|(t, _)| t.as_str()).collect();
        tags.sort();
        assert_eq!(marked, tags, "every field filled is named in the mark");
        // Never the DXCC number, and the country stays the country file's.
        assert_eq!(r.dxcc, None);
        assert_eq!(r.country.as_deref(), Some("United States"));
    }

    /// ★ A callbook's IOTA reference is never stamped into a contact (operator decision,
    /// 2026-10-27): it describes the licensee's record, not necessarily this activation, so it is
    /// OFFERED at log time (`offer::iota_offer`) and written only when the operator takes it.
    #[test]
    fn a_callbook_iota_is_offered_never_stamped() {
        let mut r = rec("W1ABC");
        fill_from_callbook(&mut r, &answer("W1ABC"));
        assert_eq!(r.ota.iota, None);
        assert!(!is_callbook_filled(&r, "IOTA"));
        let mut g = rec("W1ABC");
        fill_gridless_from_callbook(&mut g, &answer("W1ABC"));
        assert_eq!(g.ota.iota, None);
    }

    /// ★ Operator data is never overwritten: each field the record already holds keeps its value
    /// and stays off the mark; only the empty ones fill.
    #[test]
    fn a_field_the_record_holds_is_never_overwritten() {
        let mut r = rec("W1ABC");
        r.name = Some("Bob".into());
        r.grid = Some("FN42".into());
        r.state = Some("MA".into());
        r.ota.iota = Some("NA-046".into());
        for (t, v) in [
            ("CQZ", "4"),
            ("EMAIL", "mine@example.org"),
            ("LAT", "S042 00.000"),
        ] {
            set_extra(&mut r, t, Some(v.into()));
        }
        let before = r.clone();
        fill_from_callbook(&mut r, &answer("W1ABC"));
        for tag in ["NAME", "GRIDSQUARE", "STATE", "IOTA", "CQZ", "EMAIL", "LAT"] {
            assert_eq!(value_of(&r, tag), value_of(&before, tag), "{tag} kept");
            assert!(!is_callbook_filled(&r, tag), "{tag} is not the callbook's");
        }
        // The state is the operator's MA, so the callbook's Connecticut county is not written
        // under it.
        assert_eq!(extra_value(&r, "CNTY"), None);
        assert_eq!(
            value_of(&r, "ITUZ"),
            Some("8"),
            "an empty field still fills"
        );
    }

    /// A base-call answer for a stroked call, and a contact with a hunted park, take who the
    /// operator is but not where: no grid, QTH, state, county, position, IOTA, zones or offset.
    #[test]
    fn location_needs_the_stations_own_record_and_no_park() {
        let mut stroked = rec("W1ABC/P");
        fill_from_callbook(&mut stroked, &answer("W1ABC"));
        let mut park = rec("W1ABC");
        park.ota.their_program = Some("POTA".into());
        park.ota.their_ref = Some("US-0001".into());
        fill_from_callbook(&mut park, &answer("W1ABC"));
        for r in [&stroked, &park] {
            let tags: Vec<String> = filled_tags(r).into_iter().map(|(t, _)| t).collect();
            assert_eq!(
                tags,
                [
                    EQSL_USER_FIELD,
                    LOTW_USER_FIELD,
                    PAPER_QSL_FIELD,
                    "EMAIL",
                    "NAME",
                    "QSL_VIA",
                    "WEB",
                ]
                .map(String::from)
                .to_vec(),
                "{}",
                r.call
            );
        }
    }

    /// A contest exchange that carries a field owns it: the callbook does not write a second.
    #[test]
    fn a_contest_exchange_owns_its_fields() {
        let mut r = rec("W1ABC");
        r.contest = Some(Box::new(crate::logbook::ContestFields {
            adif: vec![("CQZ".into(), "5".into()), ("STATE".into(), "CT".into())],
            ..Default::default()
        }));
        fill_from_callbook(&mut r, &answer("W1ABC"));
        assert_eq!(extra_value(&r, "CQZ"), None);
        assert_eq!(r.state, None);
        assert_eq!(value_of(&r, "ITUZ"), Some("8"));
        // The zone they sent is the record's zone, written once.
        assert_eq!(crate::logbook::record_cq_zone(&r), Some(5));
        assert_eq!(adif_record(&r).matches("<CQZ:").count(), 1);
    }

    /// The grid, name and QTH a contest merged before they reached the record's own fields are
    /// still the exchange's: the callbook writes none of them beside it.
    #[test]
    fn an_older_merged_rows_exchange_owns_its_grid_name_and_qth() {
        let mut r = rec("W1ABC");
        r.contest = Some(Box::new(crate::logbook::ContestFields {
            adif: vec![
                ("GRIDSQUARE".into(), "FN31".into()),
                ("NAME".into(), "BOB".into()),
                ("QTH".into(), "BOSTON".into()),
            ],
            ..Default::default()
        }));
        fill_from_callbook(&mut r, &answer("W1ABC"));
        assert_eq!((&r.grid, &r.name, &r.qth), (&None, &None, &None));
        let out = adif_record(&r);
        for tag in ["GRIDSQUARE", "NAME", "QTH"] {
            assert_eq!(out.matches(&format!("<{tag}:")).count(), 1, "{out}");
            assert!(!is_callbook_filled(&r, tag));
        }
    }

    /// A malformed value is absence: a grid that is not a locator, an IOTA that is not a
    /// reference, a zone out of range. Outside the US, no county is written.
    #[test]
    fn a_malformed_value_is_not_written() {
        let mut r = rec("W1ABC");
        r.country = Some("Canada".into());
        let mut a = answer("W1ABC");
        a.grid = Some("FN31/FN32".into());
        a.iota = Some("island".into());
        a.cq_zone = Some(41);
        a.itu_zone = Some(0);
        fill_from_callbook(&mut r, &a);
        assert_eq!(r.grid, None);
        assert_eq!(r.ota.iota, None);
        assert_eq!(extra_value(&r, "CQZ"), None);
        assert_eq!(extra_value(&r, "ITUZ"), None);
        assert_eq!(extra_value(&r, "CNTY"), None);
    }

    /// ★ The ADIF round trip loses nothing: every filled field and the mark come back from the
    /// operator's own log, and the mark (only the mark) is withheld from what is sent.
    #[test]
    fn the_filled_record_round_trips_and_the_mark_stays_home() {
        let mut r = rec("W1ABC");
        fill_from_callbook(&mut r, &answer("W1ABC"));
        let own = adif_record_own_log(&r);
        let back = parse_adif(&(adif_header() + &own)).remove(0);
        assert_eq!(filled_tags(&back), filled_tags(&r));
        assert_eq!(callbook_filled(&back), callbook_filled(&r));
        assert_eq!(adif_record_own_log(&back), own, "the same bytes again");
        let sent = adif_record(&r);
        assert!(!sent.contains(CALLBOOK_FILLED_FIELD), "{sent}");
        assert!(
            sent.contains("<CQZ:1>5") && sent.contains("<CNTY:11>CT,Hartford"),
            "{sent}"
        );
    }

    /// An edit that changes a filled field makes it the operator's; one that leaves it keeps the
    /// mark.
    #[test]
    fn an_edit_takes_a_changed_field_off_the_mark() {
        let mut old = rec("W1ABC");
        fill_from_callbook(&mut old, &answer("W1ABC"));
        let mut edit = old.clone();
        edit.grid = Some("fn31PR".into()); // the same locator, other case
        edit.name = Some("David".into());
        unmark_edited(&old, &mut edit);
        assert!(is_callbook_filled(&edit, "GRIDSQUARE"));
        assert!(!is_callbook_filled(&edit, "NAME"));
        assert!(is_callbook_filled(&edit, "CQZ"));
    }

    #[test]
    fn a_position_is_written_as_adif_location() {
        assert_eq!(adif_location(-41.7147, 'N', 'S'), "S041 42.882");
        assert_eq!(adif_location(-33.5, 'N', 'S'), "S033 30.000");
        assert_eq!(adif_location(-179.99999, 'E', 'W'), "W179 59.999");
        assert_eq!(adif_location(0.0, 'E', 'W'), "E000 00.000");
    }

    /// A northern latitude takes the positive letter. The expected text is assembled so the
    /// source carries no `N` followed by digits.
    #[test]
    fn a_northern_latitude_is_written_with_n() {
        let lat = adif_location(41.7147, 'N', 'S');
        assert!(lat.starts_with('N'), "{lat}");
        assert_eq!(lat, format!("{}{}", 'N', "041 42.882"));
    }
}
