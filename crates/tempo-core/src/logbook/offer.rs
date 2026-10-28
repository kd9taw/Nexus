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
//! exactly that removal), makes it the operator's. Modelled on the callbook's mark
//! ([`super::callbook::CALLBOOK_FILLED_FIELD`]): an app-private list in `extra`, never sent
//! itself. A callbook-sourced offer is also marked as a callbook fill, as the rest of the
//! callbook's answer is; that mark alone, on a contact logged before offers existed, holds
//! nothing back.
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
pub(crate) fn unmark_edited(old: &QsoRecord, rec: &mut QsoRecord) {
    let norm = |v: Option<&str>| {
        v.map(|v| v.trim().to_ascii_uppercase())
            .filter(|v| !v.is_empty())
    };
    if is_offer_held(old, "IOTA") && norm(rec.ota.iota.as_deref()) != norm(old.ota.iota.as_deref())
    {
        unmark_offer_held(rec, "IOTA");
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
    use crate::logbook::{adif_record, adif_record_own_log, edited, fields_set, parse_adif};

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
        // Confirmed on the card, it goes out.
        fields_set(&mut r, &[(OFFER_HELD_FIELD.to_string(), None)]);
        assert!(adif_record(&r).contains("<IOTA:6>EU-025"));
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
