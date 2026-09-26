//! ⭐ **The export model has a DIRECTION** (§2.1.1) — the standard ADIF columns one
//! row's exchange writes, resolved where the exchange spec is in hand.
//!
//! **The defect this exists to prevent.** `Domain.adif` was one tag applied to a slot
//! in both directions. `QTH` is that slot on every row of a QSO-party log, sent *and*
//! received. Apply one tag both ways and a Tennessee mobile's rows export
//! `<CNTY:3>WIL` on a QSO **whose contacted station is in Connecticut** — in ADIF,
//! `CNTY` is the *contacted* station's county, so a reimport moves W1AW to Tennessee.
//! Don't apply it and the county I was actually in has no ADIF column at all.
//!
//! ⚠️ **Every `MY_*` tag here was checked against adif.org's own field list before
//! this module shipped** — the same discipline §6.2 applies to a sponsor's Cabrillo
//! template, and it applies here because a wrong tag is a field this build invents,
//! and an invented field ships into other people's logbooks and cannot be recalled.
//! The read: ADIF Specification **V 3.1.7**, `https://adif.org/317/ADIF_317.htm`,
//! stamped *"Released ADIF Version 3.1.7, updated 2026-03-22"*, fetched and searched
//! in full on **2026-09-09**.
//!
//! | Tag | In the 3.1.7 QSO field list? | The spec's own words |
//! |---|---|---|
//! | `MY_GRIDSQUARE` | **yes** (the positive control — it is also in this tree) | "the logging station's … Maidenhead Grid Square" |
//! | `MY_CNTY` | **yes** | "the logging station's Secondary Administrative Subdivision (e.g. US county, JA Gun)" |
//! | `MY_STATE` | **yes** | "the code for the logging station's Primary Administrative Subdivision (e.g. US State, JA Island, VE Province)" |
//! | `MY_ARRL_SECT` | **yes** | "the logging station's ARRL section" |
//! | `MY_COUNTY` | **NO** — not a field | (the plausible-looking name that does not exist) |
//!
//! Negative controls for that search: `MY_COUNTY`, `MY_SECTION` and two invented names
//! returned zero anchors, so a "yes" above is the document answering rather than the
//! method saying yes to everything.
//!
//! **A tag that had not checked out would ship as `sent: None`**, and its value would
//! ride the private carrier ([`carrier`](super::carrier)) instead of an ADIF column
//! that means something else. That path is not hypothetical — `CLASS`, Sweepstakes'
//! precedence and its check all take it, and [`directed_columns`] is tested on it.
use super::render::role_for;
use super::spec::{Domain, ExchangeSpec, FieldKind, FieldValue};
use crate::fieldday::LoggedQso;

/// The standard ADIF columns this row's exchange exports under, `(tag, value)`, sent
/// side first then received, each side in its role's declared order.
///
/// A slot with no tag that way round contributes nothing: there is no standard column
/// for it, and its value rides `APP_NEXUS_MYEX`/`APP_NEXUS_EX` instead. An empty value
/// contributes nothing either — `<MY_CNTY:0>` is the malformed field importers reject.
///
/// **Resolved here, not at write time, because this is the only place the spec is in
/// hand.** A general-log record carries `(slot, raw)` pairs and no exchange, so the
/// ADIF writer downstream cannot know that `SECTION` means `MY_ARRL_SECT` one way and
/// `ARRL_SECT` the other.
pub fn directed_columns(row: &LoggedQso, spec: &ExchangeSpec) -> Vec<(String, String)> {
    let role = role_for(row, spec);
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push = |tag: Option<&'static str>, raw: &str| {
        let (Some(tag), false) = (tag, raw.trim().is_empty()) else {
            return;
        };
        // A second slot claiming a tag another slot already wrote is a ruleset bug,
        // not a value to merge: two `<STATE>` fields in one record is exactly the
        // "undefined territory" a duplicate hands TQSL. First declared wins, and the
        // §2.5 validator is where such a ruleset gets refused.
        if !out.iter().any(|(t, _)| t == tag) {
            out.push((tag.to_string(), raw.trim().to_string()));
        }
    };
    for key in role.sends {
        let Some(v) = row.tx.iter().find(|v| v.key == *key) else {
            continue;
        };
        push(tags_for(spec, v).sent, &v.raw);
    }
    for key in role.receives {
        let Some(v) = row.rx.iter().find(|v| v.key == *key) else {
            continue;
        };
        push(tags_for(spec, v).rcvd, &v.raw);
    }
    out
}

/// The tags one copied value exports under.
///
/// ⭐ **The MATCHED domain wins over the slot's own default**, which is what a
/// [`FieldKind::OneOf`] slot needs: one slot with two disjoint meanings has two export
/// tags per direction, and without this an in-state OhQP operator's received
/// state-or-province exports under the county tag. For a plain `Enum` slot the two
/// agree by construction (a ruleset sets the slot's tags *from* its domain's), so this
/// is the general rule rather than a special case.
fn tags_for(spec: &ExchangeSpec, v: &FieldValue) -> super::spec::AdifTags {
    v.domain
        .and_then(|id| domain_by_id(spec, id))
        .map(|d| d.adif)
        .or_else(|| spec.field(v.key).map(|f| f.adif))
        .unwrap_or(super::spec::AdifTags {
            rcvd: None,
            sent: None,
        })
}

/// The [`Domain`] an id names, searching every slot's kind — including the arms of a
/// [`FieldKind::OneOf`], which is the only place the losing arms of a matched
/// either-or slot are reachable from.
fn domain_by_id(spec: &ExchangeSpec, id: &str) -> Option<&'static Domain> {
    fn find(kind: &FieldKind, id: &str) -> Option<&'static Domain> {
        match kind {
            FieldKind::Enum { domain } => (domain.id == id).then_some(*domain),
            FieldKind::OneOf(arms) => arms.iter().find_map(|k| find(k, id)),
            _ => None,
        }
    }
    spec.fields.iter().find_map(|f| find(&f.kind, id))
}

/// ADIF 3.1.7's `STATE` codes (`Primary_Administrative_Subdivision`) for the four DXCC
/// entities whose stations send a state or province in a contest exchange, keyed by the
/// country file's entity name. Read from `https://adif.org/317/ADIF_317.htm` on 2026-09-26:
/// the United States (291) lists the 48 contiguous states and DC, Alaska (6) and Hawaii
/// (110) are entities of their own with one code each, and Canada (1) lists 13.
const ADIF_STATES: [(&str, &[&str]); 4] = [
    (
        "United States",
        &[
            "AL", "AZ", "AR", "CA", "CO", "CT", "DE", "DC", "FL", "GA", "ID", "IL", "IN", "IA",
            "KS", "KY", "LA", "ME", "MD", "MA", "MI", "MN", "MS", "MO", "MT", "NE", "NV", "NH",
            "NJ", "NM", "NY", "NC", "ND", "OH", "OK", "OR", "PA", "RI", "SC", "SD", "TN", "TX",
            "UT", "VT", "VA", "WA", "WV", "WI", "WY",
        ],
    ),
    ("Alaska", &["AK"]),
    ("Hawaii", &["HI"]),
    (
        "Canada",
        &[
            "AB", "BC", "MB", "NB", "NL", "NS", "NT", "NU", "ON", "PE", "QC", "SK", "YT",
        ],
    ),
];

/// The contest spellings of a Canadian call area that ADIF spells differently. CQ WW RTTY's
/// QTH list (IV.C.3) splits Newfoundland into NF (VO1) and LB (VO2) and writes NWT and PEI;
/// ADIF has one province, NL, and NT and PE.
const CALL_AREA_TO_ADIF: [(&str, &str); 4] =
    [("NF", "NL"), ("LB", "NL"), ("NWT", "NT"), ("PEI", "PE")];

/// A contest code as ADIF spells it: [`CALL_AREA_TO_ADIF`]'s spelling, else the code.
fn adif_spelling(code: &str) -> &str {
    CALL_AREA_TO_ADIF
        .iter()
        .find(|(contest, _)| *contest == code)
        .map_or(code, |(_, adif)| adif)
}

/// ⭐ **The worked station's `STATE`, from what it SENT** — the state or province in its
/// received exchange, spelled as ADIF spells it, or `None`.
///
/// **Why the merge asks this rather than [`directed_columns`].** A directed column is one
/// tag applied to every value of a slot, and CQ WW RTTY's QTH cannot have one: `NWT`, `NF`,
/// `LB` and `PEI` are not ADIF `STATE` values, so its rules file exports the slot under no
/// tag (see the seed's `_provenance`), and the contest log and its own export keep the
/// sponsor's codes. Four of the codes need MAPPING, which a tag cannot say. The general-log
/// record is the one every upload service reads, so the merge maps them here and writes
/// `QsoRecord::state`.
///
/// A value counts only when both hold:
/// * the domain it matched IS a list of states and provinces: every code in it spells, in
///   ADIF, a `STATE` of one of the four entities. An ARRL section (`CT`) or a county is not
///   a state even where its code looks like one, and it is refused here rather than read by
///   its spelling;
/// * its ADIF spelling is a `STATE` of the station's OWN entity. A VE3 that sent `MA` claims
///   nothing, and what it sent stays in `SRX_STRING`.
pub(crate) fn received_state(row: &LoggedQso, spec: &ExchangeSpec) -> Option<&'static str> {
    let is_adif_state = |code: &str| ADIF_STATES.iter().any(|(_, codes)| codes.contains(&code));
    let (_, own) = ADIF_STATES
        .iter()
        .find(|(entity, _)| row.entity.as_deref() == Some(*entity))?;
    row.rx.iter().find_map(|v| {
        let raw = v.raw.trim().to_ascii_uppercase();
        let code = own.iter().copied().find(|c| *c == adif_spelling(&raw))?;
        let domain = v.domain.and_then(|id| domain_by_id(spec, id))?;
        domain
            .values
            .iter()
            .all(|(c, _)| is_adif_state(adif_spelling(c)))
            .then_some(code)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contest::exchanges::qso_party_shaped;
    use crate::contest::{field_day, ContestSession};
    use crate::fieldday::{FdEvent, FieldDayLog};

    /// Field Day, both directions. `CLASS` is a real ADIF field (3.1.7: *"contest
    /// class (e.g. for ARRL Field Day)"*) and is the CONTACTED station's, so it
    /// exports received-side only — **`MY_CLASS` does not exist in the field list**
    /// (checked 2026-09-09; zero anchors, alongside `MY_CHECK` and `MY_PRECEDENCE`).
    /// So the class I sent takes the fallback: no column, and the private carrier.
    /// That is the fallback path running on a SHIPPED exchange, not a fixture.
    #[test]
    fn field_day_exports_its_section_both_ways_and_its_own_class_not_at_all() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 100));
        let cols = directed_columns(&log.qsos()[0], field_day(FdEvent::ArrlFd));
        assert_eq!(
            cols,
            vec![
                // Sent side first, in send order: my section. My class has no tag.
                ("MY_ARRL_SECT".to_string(), "WI".to_string()),
                // Then theirs, in receive order: their class and their section.
                ("CLASS".to_string(), "2A".to_string()),
                ("ARRL_SECT".to_string(), "EMA".to_string()),
            ],
            "the section I sent is MINE and the one they sent is THEIRS"
        );
        // …and the class I sent is on the row for the private carrier to take.
        assert_eq!(log.qsos()[0].sent("CLASS"), "3A");
        assert!(
            !cols.iter().any(|(_, v)| v == "3A"),
            "MY_CLASS is not an ADIF field and must not be invented: {cols:?}"
        );
    }

    /// ⭐ §10 — the TNQP mobile. I am in Williamson County TN and I work a Connecticut
    /// station: my county must export as `MY_CNTY`, their state as `STATE`, and
    /// `CNTY` — the CONTACTED station's county — must not appear at all.
    #[test]
    fn a_mobiles_own_county_never_exports_as_the_other_stations() {
        let spec = qso_party_shaped();
        let row = party_row(spec, ("QTH", "WIL", "tn_counties"), ("QTH", "CT", "us_ca"));
        let cols = directed_columns(&row, spec);
        assert!(
            cols.contains(&("MY_CNTY".to_string(), "WIL".to_string())),
            "{cols:?}"
        );
        assert!(
            cols.contains(&("STATE".to_string(), "CT".to_string())),
            "{cols:?}"
        );
        assert!(
            !cols.iter().any(|(t, _)| t == "CNTY"),
            "the contacted station is in Connecticut and has no county: {cols:?}"
        );

        // POSITIVE CONTROL: a station that really did send me a county DOES export
        // `CNTY`. Without it the assertion above would also pass on a writer that had
        // simply lost the received side.
        let in_state = party_row(
            spec,
            ("QTH", "WIL", "tn_counties"),
            ("QTH", "DAV", "tn_counties"),
        );
        let cols = directed_columns(&in_state, spec);
        assert!(
            cols.contains(&("CNTY".to_string(), "DAV".to_string())),
            "{cols:?}"
        );
        assert!(
            cols.contains(&("MY_CNTY".to_string(), "WIL".to_string())),
            "{cols:?}"
        );
        assert!(
            !cols.iter().any(|(t, _)| t == "STATE"),
            "nobody sent a state on this row: {cols:?}"
        );
    }

    /// ⭐ **The fallback, which is what a tag that did not check out gets.** The
    /// party shape's `RST` slot declares no ADIF tag either way, so it writes no
    /// column — and its value is still on the row for the private carrier to encode.
    #[test]
    fn a_slot_with_no_declared_tag_writes_no_column() {
        let spec = qso_party_shaped();
        let row = party_row(spec, ("QTH", "WIL", "tn_counties"), ("QTH", "CT", "us_ca"));
        let cols = directed_columns(&row, spec);
        assert!(
            !cols.iter().any(|(t, v)| t.contains("RST") || v == "599"),
            "a slot with no corroborated tag must not invent one: {cols:?}"
        );
        assert_eq!(row.sent("RST"), "599", "…and the value is still on the row");
        assert_eq!(row.rcvd("RST"), "599");
    }

    /// One row of the party shape: `(slot, raw, domain)` for the sent QTH and the
    /// received QTH, with a constant RST both ways.
    fn party_row(
        spec: &'static crate::contest::ExchangeSpec,
        sent: (&'static str, &str, &'static str),
        rcvd: (&'static str, &str, &'static str),
    ) -> LoggedQso {
        let val = |key: &'static str, raw: &str, domain: Option<&'static str>| FieldValue {
            key: spec.field(key).expect("declared slot").key,
            raw: raw.to_string(),
            domain,
        };
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("W1AW", "1D", "CT", "CW", 0, 100));
        let row = &mut log.qsos_mut()[0];
        row.role = String::new();
        row.tx = vec![val("RST", "599", None), val(sent.0, sent.1, Some(sent.2))];
        row.rx = vec![val("RST", "599", None), val(rcvd.0, rcvd.1, Some(rcvd.2))];
        log.qsos()[0].clone()
    }
}
