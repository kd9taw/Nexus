//! An IOTA reference FILLED IN at log time, from a DX-cluster spot's comment or a callbook answer,
//! and the mark such a reference carries until the operator touches it.
//!
//! **Logged automatically, shown first where there is a form** (operator rulings, 2026-11-10,
//! replacing the 2026-10-27 "offered, never stamped" rule and the 2026-11-06 no-popup hold). When a
//! spot or the callbook names the station's island group, the contact is logged with it on every
//! path. Where the operator logs through a form (the log strip, the Logbook's Add form, the
//! confirm-before-log popup) the reference is put in the form first ([`iota_offer`] works out
//! what), so it can be changed or cleared before the contact is saved; [`accept_iota_offer`] then
//! marks it by its source. Where nothing is on screen (an FT contact Auto-log or Log QSO writes
//! with no popup, a plain `log_qso` caller, Remote's synced log) [`fill_iota_at_log`] writes it.
//! Only ever into an empty IOTA field: a reference the operator typed is never replaced.
//!
//! **Nothing is held back.** A reference filled this way goes to every upload, logging program and
//! award like one the operator typed. Spots and callbooks can be wrong (a spotter's typo, a callbook
//! entry describing the licensee's home); the guards against that are which sources count, below,
//! and the operator's chance to correct it where a form shows it.
//!
//! **The mark ([`OFFER_HELD_FIELD`]) says only where the value came from.** Its ADIF name is kept
//! from the hold it once named, so a contact logged under that rule reads the same way. While it
//! names `IOTA` the reference is Nexus's fill, not the operator's word: a LoTW download replaces it
//! with the station's signed location (`reconcile`), the contact card can clear it, and an edit
//! that changes it takes the mark off ([`unmark_edited`]). A callbook-sourced one is also marked as
//! a callbook fill, as the rest of the callbook's answer is. The mark itself is app-private and
//! never sent to a service.
//!
//! **Which sources count.** A fresh spot on the contact's band wins over the callbook, which
//! describes the licensee and not where they are today; with none, the answer a lookup already gave
//! this session for exactly this call, never a stroked call's ([`iota_offer`]). Spots that disagree
//! with each other write nothing from either ([`SpotIota::Refused`]). Spots that agree but place the
//! station only on another band write nothing by themselves ([`SpotIota::OtherBand`]): with a
//! callbook answer already held naming that same group, the two vouch for each other and it is
//! written, marked as the callbook's. Judging the spots is the command layer's (it keeps them); the
//! logging path never makes a lookup.
//!
//! **The group's name in the COMMENT** ([`note_iota_in_comment`]): the log funnel appends
//! `IOTA <reference> <group name>` from the IOTA list, once, after anything already there.

use super::callbook::{CallbookAnswer, CALLBOOK_FILLED_FIELD};
use super::{extra_value, set_extra, valid_iota, QsoRecord};
use serde::{Deserialize, Serialize};

/// The fields Nexus filled on this contact at log time from a spot's comment or a callbook answer
/// and the operator has not touched since, comma-separated ADIF names (the module header). The name
/// is kept from the hold it once named. App-private and never sent; the reference it names is sent
/// like any other.
pub const OFFER_HELD_FIELD: &str = "APP_NEXUS_OFFER_HELD";

/// Where a filled-in reference came from — what the log form says, and which mark it gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OfferSource {
    /// The comment of the cluster spot the contact came from.
    Spot,
    /// A callbook answer for exactly this call.
    Callbook,
}

/// An IOTA reference a spot or the callbook names for the contact being logged: what a log form
/// is filled with, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IotaOffer {
    /// The reference, `XX-NNN`.
    pub reference: String,
    pub source: OfferSource,
}

/// What to fill a contact being logged with `call` whose IOTA field holds `held` with:
///
/// * nothing when the field already holds a reference (the operator's, or the station's);
/// * else the reference the spot's comment named (`spot_iota`, from the cluster's comment
///   parser), because it describes this activation;
/// * else the callbook's, only when the answer is this exact call's record and the call is not
///   stroked: a lookup of `W1ABC/P` answers with the licensee's home, not the island they are on
///   (the callbook module's own rule for every location field).
///
/// Every candidate must be a reference by its shape; `known` is the island-group check when the
/// operator has the IOTA list (`None` without it), and a reference the list lacks is not filled.
pub fn iota_offer(
    call: &str,
    held: Option<&str>,
    spot_iota: Option<&str>,
    callbook: Option<&CallbookAnswer>,
    known: Option<&dyn Fn(&str) -> bool>,
) -> Option<IotaOffer> {
    if held.is_some_and(|h| !h.trim().is_empty()) {
        return None;
    }
    let listed = |r: &String| known.is_none_or(|k| k(r));
    if let Some(r) = spot_iota.and_then(valid_iota).filter(listed) {
        return Some(IotaOffer {
            reference: r,
            source: OfferSource::Spot,
        });
    }
    let call = call.trim();
    let own = callbook.filter(|a| !call.contains('/') && a.call.trim().eq_ignore_ascii_case(call));
    own.and_then(|a| a.iota.as_deref())
        .and_then(valid_iota)
        .filter(listed)
        .map(|r| IotaOffer {
            reference: r,
            source: OfferSource::Callbook,
        })
}

/// Write `offer` into `rec`, marked by where it came from: into an empty IOTA field, or, when the
/// field already holds exactly that reference (a log form filled with it and logged it as it was),
/// only the mark. `false` (and `rec` untouched) when the field holds another reference, which is
/// the operator's, or the offer is not a reference.
pub fn accept_iota_offer(rec: &mut QsoRecord, offer: &IotaOffer) -> bool {
    let Some(r) = valid_iota(&offer.reference) else {
        return false;
    };
    let held = rec
        .ota
        .iota
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty());
    if held.is_some_and(|h| !h.eq_ignore_ascii_case(&r)) {
        return false;
    }
    rec.ota.iota = Some(r);
    let fields: &[&str] = match offer.source {
        OfferSource::Spot => &[OFFER_HELD_FIELD],
        OfferSource::Callbook => &[OFFER_HELD_FIELD, CALLBOOK_FILLED_FIELD],
    };
    for field in fields {
        let mut tags: Vec<String> = marked(rec, field).into_iter().map(str::to_string).collect();
        tags.push("IOTA".to_string());
        set_mark(rec, field, tags);
    }
    true
}

/// What the spots say of a contact being logged where no form shows it first, for its call, band
/// and start time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpotIota {
    /// No fresh spot of this call names a group.
    Silent,
    /// Every fresh spot of this call names this group, one of them on the contact's band.
    Names(String),
    /// Every fresh spot of this call names this group, and none is on the contact's band. It places
    /// the station elsewhere, so it is written only when a callbook answer already held names the
    /// same group ([`fill_iota_at_log`]).
    OtherBand(String),
    /// Fresh spots disagree about the group. The callbook does not settle it, so nothing is
    /// written.
    Refused,
}

/// Write into a contact logged where no form showed it first (an FT contact with no popup, a
/// held popup contact before it is shown, a plain or synced log call) the reference a fresh spot on
/// its band names, else `callbook`'s under [`iota_offer`]'s rules, into an empty IOTA field only and
/// marked by its source ([`accept_iota_offer`]). A group the spots name only on another band goes
/// in when `callbook`'s names the same one, marked as the callbook's, since no spot places the
/// station on this band. `false` (and `rec` untouched) when there is nothing to write. `callbook`
/// is an answer already held: this never asks for one.
pub fn fill_iota_at_log(
    rec: &mut QsoRecord,
    spots: &SpotIota,
    callbook: Option<&CallbookAnswer>,
    known: &dyn Fn(&str) -> bool,
) -> bool {
    iota_at_log(&rec.call, rec.ota.iota.as_deref(), spots, callbook, known)
        .is_some_and(|offer| accept_iota_offer(rec, &offer))
}

/// What [`fill_iota_at_log`] would write into a contact with `call` whose IOTA field holds `held`,
/// without a record to write it into: the contest log's rows carry it as an offer and the merge
/// writes it ([`accept_iota_offer`]).
pub fn iota_at_log(
    call: &str,
    held: Option<&str>,
    spots: &SpotIota,
    callbook: Option<&CallbookAnswer>,
    known: &dyn Fn(&str) -> bool,
) -> Option<IotaOffer> {
    let spot = match spots {
        SpotIota::Refused => return None,
        SpotIota::Silent | SpotIota::OtherBand(_) => None,
        SpotIota::Names(group) => Some(group.as_str()),
    };
    iota_offer(call, held, spot, callbook, Some(known)).filter(|offer| match spots {
        SpotIota::OtherBand(group) => valid_iota(group).as_ref() == Some(&offer.reference),
        _ => true,
    })
}

/// The fields `field` (a mark) names on this record.
fn marked<'a>(r: &'a QsoRecord, field: &str) -> Vec<&'a str> {
    extra_value(r, field)
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Rewrite a mark: sorted, deduplicated, gone when empty.
fn set_mark(r: &mut QsoRecord, field: &str, mut tags: Vec<String>) {
    tags.sort();
    tags.dedup();
    set_extra(r, field, (!tags.is_empty()).then(|| tags.join(",")));
}

/// The fields Nexus filled at log time on this record ([`OFFER_HELD_FIELD`]).
pub fn offer_held(r: &QsoRecord) -> Vec<&str> {
    marked(r, OFFER_HELD_FIELD)
}

/// Whether `tag` on this record is one Nexus filled at log time from a spot or the callbook, and the
/// operator has not touched it since.
pub fn is_offer_held(r: &QsoRecord, tag: &str) -> bool {
    offer_held(r).contains(&tag)
}

/// Take `tag` off the mark: the value is the operator's (or LoTW's) now.
pub fn unmark_offer_held(r: &mut QsoRecord, tag: &str) {
    if !is_offer_held(r, tag) {
        return;
    }
    let tags = offer_held(r)
        .into_iter()
        .filter(|t| *t != tag)
        .map(str::to_string)
        .collect();
    set_mark(r, OFFER_HELD_FIELD, tags);
}

/// Take a reference Nexus filled at log time back out of `rec`, with its marks (this one and the
/// callbook's): a call corrected before the contact was logged makes it a stranger's. `false` (and
/// `rec` untouched) when the IOTA field holds no such fill.
pub fn drop_filled_iota(rec: &mut QsoRecord) -> bool {
    if !is_offer_held(rec, "IOTA") {
        return false;
    }
    rec.ota.iota = None;
    unmark_offer_held(rec, "IOTA");
    super::callbook::unmark_callbook_filled(rec, "IOTA");
    true
}

/// Set the mark to exactly `tags` (a merge carrying the marks of the copies it keeps).
pub(crate) fn set_offer_held(r: &mut QsoRecord, tags: Vec<String>) {
    set_mark(r, OFFER_HELD_FIELD, tags);
}

/// An edit that changed a field Nexus filled at log time makes it the operator's: it comes off the
/// mark, as a callbook-filled one does ([`super::callbook::unmark_edited`]). `old` before the edit,
/// `rec` after. Once it is off the mark the reference is no longer the callbook's either, so a LoTW
/// download no longer replaces it.
pub fn unmark_edited(old: &QsoRecord, rec: &mut QsoRecord) {
    let norm = |v: Option<&str>| {
        v.map(|v| v.trim().to_ascii_uppercase())
            .filter(|v| !v.is_empty())
    };
    if !is_offer_held(old, "IOTA") {
        return;
    }
    if norm(rec.ota.iota.as_deref()) != norm(old.ota.iota.as_deref()) {
        unmark_offer_held(rec, "IOTA");
    }
    if !is_offer_held(rec, "IOTA") {
        super::callbook::unmark_callbook_filled(rec, "IOTA");
    }
}

/// Append `IOTA <reference> <group name>` to `rec`'s COMMENT, after anything already there
/// (separated by `; `), when it holds a valid IOTA reference and `name` gives its group's name (the
/// IOTA list's; `None` without the list, or for a group it lacks, writes nothing). Never twice: a
/// comment already naming the group (any case) is left as it is. `true` when it appended.
pub fn note_iota_in_comment(rec: &mut QsoRecord, name: &dyn Fn(&str) -> Option<String>) -> bool {
    let Some(r) = rec.ota.iota.as_deref().and_then(valid_iota) else {
        return false;
    };
    let Some(group) = name(&r)
        .map(|n| n.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|n| !n.is_empty())
    else {
        return false;
    };
    let held = rec.comment.as_deref().unwrap_or_default();
    if held.to_lowercase().contains(&group.to_lowercase()) {
        return false;
    }
    let note = format!("IOTA {r} {group}");
    rec.comment = Some(if held.trim().is_empty() {
        note
    } else {
        format!("{}; {note}", held.trim_end())
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::FieldEdit;
    use crate::logbook::{
        adif_record, adif_record_own_log, adif_record_own_logger, edited, fields_set, parse_adif,
    };

    fn rec(call: &str) -> QsoRecord {
        parse_adif(&format!(
            "<EOH><CALL:{}>{call}<QSO_DATE:8>20261027<TIME_ON:6>120000<BAND:3>20m<MODE:2>CW\
             <COUNTRY:5>Italy<EOR>",
            call.len()
        ))
        .remove(0)
    }

    fn answer(call: &str, iota: &str) -> CallbookAnswer {
        CallbookAnswer {
            call: call.into(),
            iota: Some(iota.into()),
            ..CallbookAnswer::default()
        }
    }

    fn spot(r: &str) -> IotaOffer {
        IotaOffer {
            reference: r.into(),
            source: OfferSource::Spot,
        }
    }

    #[test]
    fn the_spot_is_offered_before_the_callbook_and_nothing_over_a_held_reference() {
        let cb = answer("IA5X", "EU-025");
        let o = iota_offer("IA5X", None, Some("eu-028"), Some(&cb), None).unwrap();
        assert_eq!(o, spot("EU-028"));
        let o = iota_offer("IA5X", None, None, Some(&cb), None).unwrap();
        assert_eq!(o.source, OfferSource::Callbook);
        assert_eq!(o.reference, "EU-025");
        assert_eq!(
            iota_offer("IA5X", Some("EU-001"), Some("EU-028"), Some(&cb), None),
            None
        );
        assert_eq!(
            iota_offer("IA5X", None, Some("EU-5"), None, None),
            None,
            "junk"
        );
    }

    /// The callbook's reference describes the licensee's record: never for a stroked call, never
    /// for an answer that is another call's (a base-call fallback).
    #[test]
    fn the_callbook_is_offered_only_for_its_own_plain_call() {
        let cb = answer("IA5X", "EU-025");
        assert_eq!(iota_offer("IA5X/P", None, None, Some(&cb), None), None);
        assert_eq!(iota_offer("IA5Y", None, None, Some(&cb), None), None);
    }

    #[test]
    fn with_the_island_list_an_unknown_group_is_not_offered() {
        let known = |r: &str| r == "EU-028";
        assert!(iota_offer("IA5X", None, Some("EU-028"), None, Some(&known)).is_some());
        assert_eq!(
            iota_offer("IA5X", None, Some("EU-999"), None, Some(&known)),
            None
        );
    }

    #[test]
    fn accepting_writes_an_empty_field_only_and_marks_where_it_came_from() {
        let mut r = rec("IA5X");
        assert!(accept_iota_offer(&mut r, &spot("eu-028")));
        assert_eq!(r.ota.iota.as_deref(), Some("EU-028"));
        assert!(is_offer_held(&r, "IOTA"), "marked as Nexus's fill");
        // Another offer never replaces it.
        assert!(!accept_iota_offer(&mut r, &spot("EU-031")));
        assert_eq!(r.ota.iota.as_deref(), Some("EU-028"));

        let mut c = rec("IA5X");
        let from_cb = IotaOffer {
            reference: "EU-025".into(),
            source: OfferSource::Callbook,
        };
        assert!(accept_iota_offer(&mut c, &from_cb));
        assert!(is_offer_held(&c, "IOTA"));
        assert!(crate::logbook::callbook::is_callbook_filled(&c, "IOTA"));
    }

    /// A log form filled with the reference and logged as it was: the field already holds it, and
    /// accepting marks it by its source. A form the operator changed holds another one, which is
    /// theirs: no mark.
    #[test]
    fn a_form_logged_with_the_filled_reference_takes_the_mark_and_a_changed_one_does_not() {
        let mut kept = rec("IA5X");
        kept.ota.iota = Some("EU-028".into());
        assert!(accept_iota_offer(&mut kept, &spot("EU-028")));
        assert!(is_offer_held(&kept, "IOTA"));

        let mut changed = rec("IA5X");
        changed.ota.iota = Some("EU-031".into());
        assert!(!accept_iota_offer(&mut changed, &spot("EU-028")));
        assert_eq!(changed.ota.iota.as_deref(), Some("EU-031"));
        assert!(!is_offer_held(&changed, "IOTA"));
    }

    /// NOTHING IS HELD BACK (operator ruling, 2026-11-10): a reference filled from a spot goes to
    /// every upload and to the logging programs on the machine, as one typed does. Its mark never
    /// leaves Nexus.
    #[test]
    fn a_filled_reference_goes_to_every_upload_and_its_mark_to_none() {
        let mut r = rec("IA5X");
        accept_iota_offer(&mut r, &spot("EU-028"));
        let own = adif_record_own_log(&r);
        assert!(own.contains("<IOTA:6>EU-028"), "{own}");
        assert!(own.contains(OFFER_HELD_FIELD), "{own}");
        let out = adif_record(&r);
        assert!(out.contains("<IOTA:6>EU-028"), "{out}");
        assert!(!out.contains(OFFER_HELD_FIELD), "{out}");
        let local = adif_record_own_logger(&r);
        assert!(local.contains("<IOTA:6>EU-028"), "{local}");
    }

    #[test]
    fn an_edit_of_the_reference_makes_it_the_operators() {
        let mut old = rec("IA5X");
        accept_iota_offer(&mut old, &spot("EU-028"));
        // An edit that leaves it as it was keeps it Nexus's…
        let same = edited(&old, old.clone());
        assert!(is_offer_held(&same, "IOTA"));
        // …one that corrects it makes it the operator's, and the correction goes out.
        let mut fix = old.clone();
        fix.ota.iota = Some("EU-031".into());
        let fixed = edited(&old, fix);
        assert!(!is_offer_held(&fixed, "IOTA"));
        assert!(adif_record(&fixed).contains("<IOTA:6>EU-031"));
    }

    /// A callbook's reference goes out as a spot's does, and still says it is the callbook's, so a
    /// LoTW download can replace it.
    #[test]
    fn a_callbook_filled_reference_goes_to_every_upload() {
        let mut r = rec("IA5X");
        let from_cb = IotaOffer {
            reference: "EU-025".into(),
            source: OfferSource::Callbook,
        };
        assert!(accept_iota_offer(&mut r, &from_cb));
        assert!(crate::logbook::callbook::is_callbook_filled(&r, "IOTA"));
        let out = adif_record(&r);
        assert!(out.contains("<IOTA:6>EU-025"), "{out}");
        assert!(!out.contains(OFFER_HELD_FIELD), "{out}");
        assert!(adif_record_own_logger(&r).contains("<IOTA:6>EU-025"));
    }

    /// A contact logged under the old rule, whose reference was held back, is sent like any other
    /// now: the mark it carries says only where the value came from.
    #[test]
    fn a_reference_held_under_the_old_rule_now_goes_out() {
        let mut r = rec("IA5X");
        r.ota.iota = Some("EU-025".into());
        set_extra(&mut r, OFFER_HELD_FIELD, Some("IOTA".into()));
        set_extra(&mut r, CALLBOOK_FILLED_FIELD, Some("IOTA,NAME".into()));
        let out = adif_record(&r);
        assert!(out.contains("<IOTA:6>EU-025"), "{out}");
        assert!(!out.contains(OFFER_HELD_FIELD), "{out}");
        assert!(adif_record_own_logger(&r).contains("<IOTA:6>EU-025"));
    }

    const ANY: &dyn Fn(&str) -> bool = &|_| true;

    fn names(r: &str) -> SpotIota {
        SpotIota::Names(r.into())
    }

    /// THE PATH WITH NO FORM: the spot's reference goes in, into an empty field only, with no
    /// callbook mark, and goes out in every upload.
    #[test]
    fn a_spots_reference_is_filled_where_no_form_shows_it() {
        let mut r = rec("IA5X");
        assert!(fill_iota_at_log(&mut r, &names("eu-028"), None, ANY));
        assert_eq!(r.ota.iota.as_deref(), Some("EU-028"));
        assert!(is_offer_held(&r, "IOTA"));
        assert!(!crate::logbook::callbook::is_callbook_filled(&r, "IOTA"));
        assert!(adif_record(&r).contains("<IOTA:6>EU-028"));
        assert!(adif_record_own_logger(&r).contains("<IOTA:6>EU-028"));
    }

    /// With no spot, the answer already held for exactly this call, marked as the callbook's,
    /// which is what the contact card's note reads.
    #[test]
    fn with_no_spot_the_callbooks_reference_is_filled() {
        let cb = answer("IA5X", "eu-025");
        let mut r = rec("IA5X");
        assert!(fill_iota_at_log(&mut r, &SpotIota::Silent, Some(&cb), ANY));
        assert_eq!(r.ota.iota.as_deref(), Some("EU-025"));
        assert!(crate::logbook::callbook::is_callbook_filled(&r, "IOTA"));
        assert!(adif_record(&r).contains("<IOTA:6>EU-025"));
    }

    /// Agreeing or not, a fresh spot is the source: the contact says it came from the spot.
    #[test]
    fn a_spot_wins_over_the_callbook_whether_they_agree_or_not() {
        for (spot, cb, want) in [
            ("EU-025", "EU-025", "EU-025"),
            ("EU-028", "EU-025", "EU-028"),
        ] {
            let cb = answer("IA5X", cb);
            let mut r = rec("IA5X");
            assert!(fill_iota_at_log(&mut r, &names(spot), Some(&cb), ANY));
            assert_eq!(r.ota.iota.as_deref(), Some(want));
            assert!(!crate::logbook::callbook::is_callbook_filled(&r, "IOTA"));
        }
    }

    /// A spot only on another band and a callbook answer that name the same group vouch for each
    /// other: it goes in, marked as the callbook's, since no spot places the station here. Either
    /// alone says less: the spot names no other group, and a callbook that disagrees loses.
    #[test]
    fn a_spot_on_another_band_writes_only_with_a_callbook_answer_naming_its_group() {
        let other = SpotIota::OtherBand("EU-025".into());
        let mut r = rec("IA5X");
        assert!(fill_iota_at_log(
            &mut r,
            &other,
            Some(&answer("IA5X", "EU-025")),
            ANY
        ));
        assert_eq!(r.ota.iota.as_deref(), Some("EU-025"));
        assert!(crate::logbook::callbook::is_callbook_filled(&r, "IOTA"));

        for (cb, why) in [
            (None, "no answer in hand"),
            (
                Some(answer("IA5X", "EU-028")),
                "the answer names another group",
            ),
            (Some(answer("IA5Y", "EU-025")), "another call's answer"),
        ] {
            let mut r = rec("IA5X");
            assert!(!fill_iota_at_log(&mut r, &other, cb.as_ref(), ANY), "{why}");
            assert_eq!(r.ota.iota, None, "{why}");
        }

        // The island list still judges it.
        let mut r = rec("IA5X");
        let cb = answer("IA5X", "EU-025");
        assert!(!fill_iota_at_log(&mut r, &other, Some(&cb), &|_| false));
        assert_eq!(r.ota.iota, None);
    }

    /// Spots that disagree stop the callbook too, even when it agrees with one of them.
    #[test]
    fn refused_spots_write_nothing_from_either() {
        let cb = answer("IA5X", "EU-025");
        let mut r = rec("IA5X");
        assert!(!fill_iota_at_log(
            &mut r,
            &SpotIota::Refused,
            Some(&cb),
            ANY
        ));
        assert_eq!(r.ota.iota, None);
        assert!(!is_offer_held(&r, "IOTA"));
    }

    /// The offer's call rules: a stroked call and another call's answer give nothing. No answer
    /// gives nothing. The island list judges the callbook's too, and a spot's it lacks is a
    /// misreading that leaves the callbook's standing.
    #[test]
    fn filling_keeps_the_offers_callbook_and_list_rules() {
        let write = |call: &str,
                     spots: &SpotIota,
                     cb: Option<&CallbookAnswer>,
                     known: &dyn Fn(&str) -> bool| {
            let mut r = rec(call);
            fill_iota_at_log(&mut r, spots, cb, known);
            r.ota.iota
        };
        let cb = answer("IA5X", "EU-025");
        let silent = SpotIota::Silent;
        assert_eq!(write("IA5X/P", &silent, Some(&cb), ANY), None, "stroked");
        let stroked = answer("IA5X/P", "EU-025");
        assert_eq!(
            write("IA5X/P", &silent, Some(&stroked), ANY),
            None,
            "stroked"
        );
        assert_eq!(write("IA5Y", &silent, Some(&cb), ANY), None, "another call");
        assert_eq!(write("IA5X", &silent, None, ANY), None, "no answer yet");
        let listed = |g: &str| g == "EU-028";
        assert_eq!(
            write("IA5X", &silent, Some(&cb), &listed),
            None,
            "off the list"
        );
        let listed = |g: &str| g == "EU-025";
        assert_eq!(
            write("IA5X", &names("EU-999"), Some(&cb), &listed).as_deref(),
            Some("EU-025")
        );
    }

    /// NOTHING INVENTED: no source, junk, or a reference already there writes nothing.
    #[test]
    fn filling_takes_nothing_over_a_reference_or_from_junk() {
        let cb = answer("IA5X", "EU-025");
        let mut typed = rec("IA5X");
        typed.ota.iota = Some("EU-001".into());
        assert!(!fill_iota_at_log(
            &mut typed,
            &names("EU-028"),
            Some(&cb),
            ANY
        ));
        assert!(!fill_iota_at_log(
            &mut typed,
            &SpotIota::Silent,
            Some(&cb),
            ANY
        ));
        assert_eq!(typed.ota.iota.as_deref(), Some("EU-001"));
        assert!(!is_offer_held(&typed, "IOTA"), "what it held is not marked");

        let junk = [
            names("EU-5"),
            names("heard on EU-028"),
            names(""),
            SpotIota::Silent,
        ];
        for spots in &junk {
            let mut r = rec("IA5X");
            assert!(!fill_iota_at_log(&mut r, spots, None, ANY), "{spots:?}");
            assert_eq!(r.ota.iota, None, "{spots:?}");
            assert!(!is_offer_held(&r, "IOTA"), "{spots:?}");
        }
    }

    /// Clear on the contact card takes a filled reference back out with its marks; it can touch no
    /// other reference, and goes stale like any card change.
    #[test]
    fn clearing_on_the_card_removes_a_filled_reference_and_no_other() {
        let clear = |old: &str| FieldEdit {
            tag: "IOTA".into(),
            old: Some(old.into()),
            new: None,
        };
        let mut r = rec("IA5X");
        let from_cb = IotaOffer {
            reference: "EU-025".into(),
            source: OfferSource::Callbook,
        };
        assert!(accept_iota_offer(&mut r, &from_cb));
        assert!(clear("EU-025").check().is_ok());
        assert!(clear("EU-025").check_on(&r).is_ok());
        assert!(clear("EU-025").is_fresh(&r));
        assert!(!clear("EU-028").is_fresh(&r), "made against another value");
        let write = FieldEdit {
            tag: "IOTA".into(),
            old: None,
            new: Some("EU-028".into()),
        };
        assert!(write.check().is_err(), "a card clears it, never writes it");

        fields_set(&mut r, &[("IOTA".to_string(), None)]);
        assert_eq!(r.ota.iota, None);
        assert!(!is_offer_held(&r, "IOTA"));
        assert!(!crate::logbook::callbook::is_callbook_filled(&r, "IOTA"));
        assert!(!adif_record_own_log(&r).contains("IOTA"));

        let mut typed = rec("IA5X");
        typed.ota.iota = Some("EU-001".into());
        assert!(clear("EU-001").check_on(&typed).is_err(), "not Nexus's");
        fields_set(&mut typed, &[("IOTA".to_string(), None)]);
        assert_eq!(typed.ota.iota.as_deref(), Some("EU-001"));
    }

    /// Taking the mark off on the contact card (the one APP_NEXUS_ field a card may change, and
    /// only to remove it) makes the reference the operator's.
    #[test]
    fn taking_the_mark_off_on_the_card_makes_the_reference_the_operators() {
        let mut r = rec("IA5X");
        accept_iota_offer(&mut r, &spot("EU-028"));
        let take_off = FieldEdit {
            tag: OFFER_HELD_FIELD.into(),
            old: Some("IOTA".into()),
            new: None,
        };
        assert!(take_off.check().is_ok());
        let set_it = FieldEdit {
            tag: OFFER_HELD_FIELD.into(),
            old: None,
            new: Some("IOTA".into()),
        };
        assert!(set_it.check().is_err(), "a card never writes the mark");
        fields_set(&mut r, &[(OFFER_HELD_FIELD.to_string(), None)]);
        assert!(!is_offer_held(&r, "IOTA"));
        assert!(adif_record(&r).contains("<IOTA:6>EU-028"));
    }

    /// Dropping a fill takes the reference and both marks out; a typed reference is never touched.
    #[test]
    fn dropping_a_fill_takes_it_out_with_its_marks_and_never_a_typed_one() {
        let mut r = rec("IA5X");
        let from_cb = IotaOffer {
            reference: "EU-025".into(),
            source: OfferSource::Callbook,
        };
        assert!(accept_iota_offer(&mut r, &from_cb));
        assert!(drop_filled_iota(&mut r));
        assert_eq!(r.ota.iota, None);
        assert!(!is_offer_held(&r, "IOTA"));
        assert!(!crate::logbook::callbook::is_callbook_filled(&r, "IOTA"));

        let mut typed = rec("IA5X");
        typed.ota.iota = Some("EU-001".into());
        assert!(!drop_filled_iota(&mut typed));
        assert_eq!(typed.ota.iota.as_deref(), Some("EU-001"));
    }

    fn group_names(r: &str) -> Option<String> {
        match r {
            "EU-005" => Some("Great Britain".into()),
            "OC-146" => Some("Example  Reef ".into()),
            _ => None,
        }
    }

    /// THE GROUP'S NAME IN THE COMMENT: `IOTA <ref> <name>` alone, or after what is there with `; `,
    /// never twice, never without a name from the list, never over what the comment says.
    #[test]
    fn the_group_name_is_appended_to_the_comment_once() {
        let mut r = rec("IA5X");
        r.ota.iota = Some("EU-005".into());
        assert!(note_iota_in_comment(&mut r, &group_names));
        assert_eq!(r.comment.as_deref(), Some("IOTA EU-005 Great Britain"));
        assert!(!note_iota_in_comment(&mut r, &group_names), "once");
        assert_eq!(r.comment.as_deref(), Some("IOTA EU-005 Great Britain"));

        let mut kept = rec("IA5X");
        kept.ota.iota = Some("EU-005".into());
        kept.comment = Some("FT8  Sent: -07  Rcvd: -10".into());
        assert!(note_iota_in_comment(&mut kept, &group_names));
        assert_eq!(
            kept.comment.as_deref(),
            Some("FT8  Sent: -07  Rcvd: -10; IOTA EU-005 Great Britain"),
            "the comment is kept, the name after it"
        );

        let mut named = rec("IA5X");
        named.ota.iota = Some("EU-005".into());
        named.comment = Some("tnx from great britain".into());
        assert!(!note_iota_in_comment(&mut named, &group_names));
        assert_eq!(named.comment.as_deref(), Some("tnx from great britain"));

        let mut spaced = rec("IA5X");
        spaced.ota.iota = Some("oc-146".into());
        assert!(note_iota_in_comment(&mut spaced, &group_names));
        assert_eq!(spaced.comment.as_deref(), Some("IOTA OC-146 Example Reef"));

        for (iota, why) in [
            (None, "no reference"),
            (Some("EU-999"), "not on the list"),
            (Some("EU-5"), "junk"),
        ] {
            let mut r = rec("IA5X");
            r.ota.iota = iota.map(str::to_string);
            r.comment = Some("hi".into());
            assert!(!note_iota_in_comment(&mut r, &group_names), "{why}");
            assert_eq!(r.comment.as_deref(), Some("hi"), "{why}");
        }
    }
}
