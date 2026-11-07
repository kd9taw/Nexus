//! An IOTA reference OFFERED to a contact at log time, and the mark a reference taken from an offer
//! carries.
//!
//! **Offered, never stamped.** A DX-cluster spot's comment and a callbook answer both name island
//! groups ("IOTA EU-131"), and both can be wrong: a spotter's typo, a stale comment re-spotted
//! from another trip, a callbook entry describing the licensee's home and not this activation.
//! So neither is written into a contact by itself. [`iota_offer`] works out what to offer for the
//! contact being logged; the operator sees it and confirms; only then does
//! [`accept_iota_offer`] write it — and only into an empty IOTA field, so a reference the
//! operator typed is never replaced.
//!
//! **A reference taken from an offer is held back from every upload ([`OFFER_HELD_FIELD`]) until
//! the operator touches it**, whichever source it came from (operator decision, 2026-10-27).
//! Confirming an offer at log time is a glance at a chip during a pileup; the reference stays in
//! the operator's own log and its exports, but nothing Nexus sends off the machine
//! ([`super::Audience::Outbound`]) carries it while the mark names it. Editing the field, or
//! confirming it on the contact card (which takes the mark off, [`super::FieldEdit`] allows
//! exactly that removal), makes it the operator's: a callbook-sourced one is then no longer
//! marked as the callbook's, so a later LoTW download does not replace it. Modelled on the callbook's mark
//! ([`super::callbook::CALLBOOK_FILLED_FIELD`]): an app-private list in `extra`, never sent
//! itself. A callbook-sourced offer is also marked as a callbook fill, as the rest of the
//! callbook's answer is; that mark alone, on a contact logged before offers existed, holds
//! nothing back.
//!
//! **One exception to "offered": an FT contact logged with no popup** (operator decisions,
//! 2026-11-06, replacing the 2026-10-27 one for this path alone). There is nothing on screen to
//! take an offer from, so what would have been offered is written by itself
//! ([`hold_iota_at_log`]) and held exactly as a taken offer is, marked by where it came from: the
//! log shows it and the New IOTA need clears, nothing is sent until the operator confirms it on the
//! contact card, and it counts toward no confirmed total. A fresh spot on the contact's band wins
//! over the callbook, which describes the licensee and not where they are today; with none, the
//! answer a lookup already gave this session for exactly this call. Spots that disagree with each
//! other write nothing from either ([`SpotIota::Refused`]). Spots that agree but place the station
//! only on another band write nothing by themselves ([`SpotIota::OtherBand`]): with a callbook
//! answer already held naming that same group, the two vouch for each other and it is written,
//! marked as the callbook's. Judging the spots is the command layer's (it keeps them); the logging path never makes a lookup.
//!
//! **A held reference is worked, not confirmed.** It counts as a group worked, and no card or LoTW
//! match on the contact makes it a confirmed group until the operator has confirmed the reference:
//! such a match confirms that the contact happened, not which island it was made from.
//!
//! **The engine's seam.** The log-time half is the engine's to call (it holds the spot the
//! contact came from and the session's callbook answers): `iota_offer` while the log form is
//! open, `accept_iota_offer` on the record it is about to write when the operator said yes.

use super::callbook::{CallbookAnswer, CALLBOOK_FILLED_FIELD};
use super::{extra_value, set_extra, valid_iota, QsoRecord};
use serde::{Deserialize, Serialize};

/// The fields filled on this contact from an offer the operator took at log time (a spot's
/// comment's, or a callbook's), comma-separated ADIF names. App-private, never sent; while it names
/// `IOTA` the reference itself is withheld from uploads.
pub const OFFER_HELD_FIELD: &str = "APP_NEXUS_OFFER_HELD";

/// Where an offered reference came from — what the chip says, and which mark it gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OfferSource {
    /// The comment of the cluster spot the contact came from.
    Spot,
    /// A callbook answer for exactly this call.
    Callbook,
}

/// An IOTA reference offered to the contact being logged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IotaOffer {
    /// The reference, `XX-NNN`.
    pub reference: String,
    pub source: OfferSource,
}

/// What to offer a contact being logged with `call` whose IOTA field holds `held`:
///
/// * nothing when the field already holds a reference (the operator's, or the station's);
/// * else the reference the spot's comment named (`spot_iota`, from the cluster's comment
///   parser), because it describes this activation;
/// * else the callbook's, only when the answer is this exact call's record and the call is not
///   stroked: a lookup of `W1ABC/P` answers with the licensee's home, not the island they are on
///   (the callbook module's own rule for every location field).
///
/// Every candidate must be a reference by its shape; `known` is the island-group check when the
/// operator has the IOTA list (`None` without it), and a reference the list lacks is not offered.
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

/// Write an offer the operator confirmed into `rec`: only into an empty IOTA field, marked by
/// where it came from. `false` (and `rec` untouched) when the field already holds one or the
/// offer is not a reference.
pub fn accept_iota_offer(rec: &mut QsoRecord, offer: &IotaOffer) -> bool {
    if rec
        .ota
        .iota
        .as_deref()
        .is_some_and(|h| !h.trim().is_empty())
    {
        return false;
    }
    let Some(r) = valid_iota(&offer.reference) else {
        return false;
    };
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

/// What the spots say of a contact being logged with no popup, for its call, band and start time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpotIota {
    /// No fresh spot of this call names a group.
    Silent,
    /// Every fresh spot of this call names this group, one of them on the contact's band.
    Names(String),
    /// Every fresh spot of this call names this group, and none is on the contact's band. It places
    /// the station elsewhere, so it is written only when a callbook answer already held names the
    /// same group ([`hold_iota_at_log`]).
    OtherBand(String),
    /// Fresh spots disagree about the group. The callbook does not settle it, so nothing is
    /// written.
    Refused,
}

/// Write into an FT contact logged with no popup what its log form would have offered, held back as
/// a taken offer is (the module header): the spot's reference, else `callbook`'s under
/// [`iota_offer`]'s rules, into an empty IOTA field only and marked by its source. A group the spots
/// name only on another band goes in when `callbook`'s names the same one, marked as the callbook's,
/// since no spot places the station on this band. `false` (and `rec` untouched) when there is nothing
/// to write. `callbook` is an answer already held: this never asks for one.
pub fn hold_iota_at_log(
    rec: &mut QsoRecord,
    spots: &SpotIota,
    callbook: Option<&CallbookAnswer>,
    known: &dyn Fn(&str) -> bool,
) -> bool {
    let spot = match spots {
        SpotIota::Refused => return false,
        SpotIota::Silent | SpotIota::OtherBand(_) => None,
        SpotIota::Names(group) => Some(group.as_str()),
    };
    let offer = iota_offer(
        &rec.call,
        rec.ota.iota.as_deref(),
        spot,
        callbook,
        Some(known),
    )
    .filter(|offer| match spots {
        SpotIota::OtherBand(group) => valid_iota(group).as_ref() == Some(&offer.reference),
        _ => true,
    });
    offer.is_some_and(|offer| accept_iota_offer(rec, &offer))
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

/// The fields taken from an offer on this record ([`OFFER_HELD_FIELD`]).
pub fn offer_held(r: &QsoRecord) -> Vec<&str> {
    marked(r, OFFER_HELD_FIELD)
}

/// Whether `tag` on this record was taken from an offer and the operator has not touched it since.
pub fn is_offer_held(r: &QsoRecord, tag: &str) -> bool {
    offer_held(r).contains(&tag)
}

/// Take `tag` off the held mark: the value is the operator's (or LoTW's) now.
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

/// Set the held mark to exactly `tags` (a merge carrying the marks of the copies it keeps).
pub(crate) fn set_offer_held(r: &mut QsoRecord, tags: Vec<String>) {
    set_mark(r, OFFER_HELD_FIELD, tags);
}

/// An edit that changed a field taken from an offer makes it the operator's: it comes off the mark, as
/// a callbook-filled one does ([`super::callbook::unmark_edited`]). `old` before the edit, `rec` after.
/// Once it is off the mark, by that edit or by a confirm on the contact card, the reference is no
/// longer the callbook's either, so a LoTW download no longer replaces it.
pub(crate) fn unmark_edited(old: &QsoRecord, rec: &mut QsoRecord) {
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

/// Whether an outbound copy of `r` must leave its IOTA reference out: it was taken from an offer
/// and the operator has not touched it ([`OFFER_HELD_FIELD`]).
pub fn withhold_iota(r: &QsoRecord) -> bool {
    is_offer_held(r, "IOTA")
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
        assert!(is_offer_held(&r, "IOTA"));
        // A second offer never replaces it.
        assert!(!accept_iota_offer(&mut r, &spot("EU-031")));
        assert_eq!(r.ota.iota.as_deref(), Some("EU-028"));

        let mut c = rec("IA5X");
        let from_cb = IotaOffer {
            reference: "EU-025".into(),
            source: OfferSource::Callbook,
        };
        assert!(accept_iota_offer(&mut c, &from_cb));
        // Held back as a spot's is, and marked as the callbook's.
        assert!(is_offer_held(&c, "IOTA"));
        assert!(crate::logbook::callbook::is_callbook_filled(&c, "IOTA"));
    }

    /// THE UPLOAD GATE: a spot's reference stays in the operator's own log and leaves the machine
    /// in no upload, and neither does the mark, until the operator touches it.
    #[test]
    fn a_spot_filled_reference_is_withheld_from_every_upload() {
        let mut r = rec("IA5X");
        accept_iota_offer(&mut r, &spot("EU-028"));
        let own = adif_record_own_log(&r);
        assert!(own.contains("<IOTA:6>EU-028"), "{own}");
        assert!(own.contains(OFFER_HELD_FIELD), "{own}");
        let out = adif_record(&r);
        assert!(!out.contains("IOTA:"), "{out}");
        assert!(!out.contains(OFFER_HELD_FIELD), "{out}");
        // Typed by the operator, it goes out like any field.
        let mut typed = rec("IA5X");
        typed.ota.iota = Some("EU-028".into());
        assert!(adif_record(&typed).contains("<IOTA:6>EU-028"));
    }

    #[test]
    fn an_edit_of_the_reference_makes_it_the_operators() {
        let mut old = rec("IA5X");
        accept_iota_offer(&mut old, &spot("EU-028"));
        // An edit that leaves it as it was keeps it held back…
        let same = edited(&old, old.clone());
        assert!(is_offer_held(&same, "IOTA"));
        // …one that corrects it sends the correction.
        let mut fix = old.clone();
        fix.ota.iota = Some("EU-031".into());
        let fixed = edited(&old, fix);
        assert!(!is_offer_held(&fixed, "IOTA"));
        assert!(adif_record(&fixed).contains("<IOTA:6>EU-031"));
    }

    /// A reference taken from the CALLBOOK's offer is held back exactly as a spot's is (operator
    /// decision, 2026-10-27), and still says it is the callbook's.
    #[test]
    fn a_callbook_reference_taken_from_the_offer_is_withheld_from_every_upload() {
        let mut r = rec("IA5X");
        let from_cb = IotaOffer {
            reference: "EU-025".into(),
            source: OfferSource::Callbook,
        };
        assert!(accept_iota_offer(&mut r, &from_cb));
        assert!(crate::logbook::callbook::is_callbook_filled(&r, "IOTA"));
        let own = adif_record_own_log(&r);
        assert!(own.contains("<IOTA:6>EU-025"), "{own}");
        let out = adif_record(&r);
        assert!(!out.contains("IOTA:"), "{out}");
        assert!(!out.contains(OFFER_HELD_FIELD), "{out}");
        // Confirmed on the card, it goes out, and it is the operator's now: no longer the
        // callbook's either, so a later LoTW download does not replace what they confirmed.
        fields_set(&mut r, &[(OFFER_HELD_FIELD.to_string(), None)]);
        assert!(adif_record(&r).contains("<IOTA:6>EU-025"));
        assert!(!crate::logbook::callbook::is_callbook_filled(&r, "IOTA"));
    }

    /// A contact logged before offers existed, whose IOTA a callbook lookup filled at log time,
    /// is unchanged: its reference goes out as it always did.
    #[test]
    fn an_older_callbook_filled_reference_still_goes_out() {
        let mut r = rec("IA5X");
        r.ota.iota = Some("EU-025".into());
        set_extra(&mut r, CALLBOOK_FILLED_FIELD, Some("IOTA,NAME".into()));
        assert!(!withhold_iota(&r));
        assert!(adif_record(&r).contains("<IOTA:6>EU-025"));
    }

    /// A logging program on the operator's own machine (DXKeeper, HRD Logbook) can upload the
    /// contact itself, so a held reference never reaches it either. One the operator typed, and
    /// an older callbook-filled one, still do, as before.
    #[test]
    fn a_held_reference_never_reaches_a_local_logger() {
        let mut r = rec("IA5X");
        let from_cb = IotaOffer {
            reference: "EU-025".into(),
            source: OfferSource::Callbook,
        };
        assert!(accept_iota_offer(&mut r, &from_cb));
        let sent = adif_record_own_logger(&r);
        assert!(!sent.contains("<IOTA:"), "{sent}");
        assert!(!sent.contains(OFFER_HELD_FIELD), "{sent}");
        assert!(!sent.contains("IOTA"), "no mark names it: {sent}");
        assert!(sent.contains("<CALL:4>IA5X"), "{sent}");

        let mut typed = rec("IA5X");
        typed.ota.iota = Some("EU-028".into());
        assert!(adif_record_own_logger(&typed).contains("<IOTA:6>EU-028"));
        let mut older = rec("IA5X");
        older.ota.iota = Some("EU-025".into());
        set_extra(&mut older, CALLBOOK_FILLED_FIELD, Some("IOTA,NAME".into()));
        assert!(adif_record_own_logger(&older).contains("<IOTA:6>EU-025"));
    }

    const ANY: &dyn Fn(&str) -> bool = &|_| true;

    fn names(r: &str) -> SpotIota {
        SpotIota::Names(r.into())
    }

    /// THE NO-POPUP PATH: the spot's reference goes in held, into an empty field only, with no
    /// callbook mark, and leaves the machine in no upload.
    #[test]
    fn a_spots_reference_on_a_no_popup_contact_is_written_held() {
        let mut r = rec("IA5X");
        assert!(hold_iota_at_log(&mut r, &names("eu-028"), None, ANY));
        assert_eq!(r.ota.iota.as_deref(), Some("EU-028"));
        assert!(withhold_iota(&r));
        assert!(!crate::logbook::callbook::is_callbook_filled(&r, "IOTA"));
        assert!(adif_record_own_log(&r).contains("<IOTA:6>EU-028"));
        assert!(!adif_record(&r).contains("IOTA"));
        assert!(!adif_record_own_logger(&r).contains("IOTA"));
    }

    /// With no spot, the answer already held for exactly this call: held, withheld alike, and
    /// marked as the callbook's, which is what the contact card's note reads.
    #[test]
    fn with_no_spot_a_no_popup_contact_takes_the_callbooks_reference_held() {
        let cb = answer("IA5X", "eu-025");
        let mut r = rec("IA5X");
        assert!(hold_iota_at_log(&mut r, &SpotIota::Silent, Some(&cb), ANY));
        assert_eq!(r.ota.iota.as_deref(), Some("EU-025"));
        assert!(withhold_iota(&r));
        assert!(crate::logbook::callbook::is_callbook_filled(&r, "IOTA"));
        assert!(adif_record_own_log(&r).contains("<IOTA:6>EU-025"));
        assert!(!adif_record(&r).contains("IOTA"));
        assert!(!adif_record_own_logger(&r).contains("IOTA"));
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
            assert!(hold_iota_at_log(&mut r, &names(spot), Some(&cb), ANY));
            assert_eq!(r.ota.iota.as_deref(), Some(want));
            assert!(withhold_iota(&r));
            assert!(!crate::logbook::callbook::is_callbook_filled(&r, "IOTA"));
        }
    }

    /// A spot only on another band and a callbook answer that name the same group vouch for each
    /// other: it goes in, held, marked as the callbook's, since no spot places the station here.
    /// Either alone says less: the spot names no other group, and a callbook that disagrees loses.
    #[test]
    fn a_spot_on_another_band_writes_only_with_a_callbook_answer_naming_its_group() {
        let other = SpotIota::OtherBand("EU-025".into());
        let mut r = rec("IA5X");
        assert!(hold_iota_at_log(
            &mut r,
            &other,
            Some(&answer("IA5X", "EU-025")),
            ANY
        ));
        assert_eq!(r.ota.iota.as_deref(), Some("EU-025"));
        assert!(withhold_iota(&r));
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
            assert!(!hold_iota_at_log(&mut r, &other, cb.as_ref(), ANY), "{why}");
            assert_eq!(r.ota.iota, None, "{why}");
        }

        // The island list still judges it.
        let mut r = rec("IA5X");
        let cb = answer("IA5X", "EU-025");
        assert!(!hold_iota_at_log(&mut r, &other, Some(&cb), &|_| false));
        assert_eq!(r.ota.iota, None);
    }

    /// Spots that disagree stop the callbook too, even when it agrees with one of them.
    #[test]
    fn refused_spots_write_nothing_from_either() {
        let cb = answer("IA5X", "EU-025");
        let mut r = rec("IA5X");
        assert!(!hold_iota_at_log(
            &mut r,
            &SpotIota::Refused,
            Some(&cb),
            ANY
        ));
        assert_eq!(r.ota.iota, None);
        assert!(!withhold_iota(&r));
    }

    /// The offer's call rules: a stroked call and another call's answer give nothing. No answer
    /// gives nothing. The island list judges the callbook's too, and a spot's it lacks is a
    /// misreading that leaves the callbook's standing, as the offer has it.
    #[test]
    fn a_no_popup_contact_keeps_the_offers_callbook_and_list_rules() {
        let write = |call: &str,
                     spots: &SpotIota,
                     cb: Option<&CallbookAnswer>,
                     known: &dyn Fn(&str) -> bool| {
            let mut r = rec(call);
            hold_iota_at_log(&mut r, spots, cb, known);
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

    #[test]
    fn a_no_popup_contact_takes_nothing_over_a_reference_or_from_junk() {
        let cb = answer("IA5X", "EU-025");
        let mut typed = rec("IA5X");
        typed.ota.iota = Some("EU-001".into());
        assert!(!hold_iota_at_log(
            &mut typed,
            &names("EU-028"),
            Some(&cb),
            ANY
        ));
        assert!(!hold_iota_at_log(
            &mut typed,
            &SpotIota::Silent,
            Some(&cb),
            ANY
        ));
        assert_eq!(typed.ota.iota.as_deref(), Some("EU-001"));
        assert!(!withhold_iota(&typed), "what it held is not marked");

        let junk = [
            names("EU-5"),
            names("heard on EU-028"),
            names(""),
            SpotIota::Silent,
        ];
        for spots in &junk {
            let mut r = rec("IA5X");
            assert!(!hold_iota_at_log(&mut r, spots, None, ANY), "{spots:?}");
            assert_eq!(r.ota.iota, None, "{spots:?}");
            assert!(!withhold_iota(&r), "{spots:?}");
        }
    }

    /// Clear on the contact card takes a held reference back out with its marks; it can touch no
    /// other reference, and goes stale like any card change.
    #[test]
    fn clearing_on_the_card_removes_a_held_reference_and_no_other() {
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
        assert!(!withhold_iota(&r));
        assert!(!crate::logbook::callbook::is_callbook_filled(&r, "IOTA"));
        assert!(!adif_record_own_log(&r).contains("IOTA"));

        let mut typed = rec("IA5X");
        typed.ota.iota = Some("EU-001".into());
        assert!(clear("EU-001").check_on(&typed).is_err(), "not held");
        fields_set(&mut typed, &[("IOTA".to_string(), None)]);
        assert_eq!(typed.ota.iota.as_deref(), Some("EU-001"));
    }

    /// Confirming on the contact card takes the mark off (the one APP_NEXUS_ field a card may
    /// change, and only to remove it), and the unchanged reference then goes out.
    #[test]
    fn confirming_on_the_card_releases_the_reference() {
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
}
