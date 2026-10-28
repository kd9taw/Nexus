//! Award status: what has been submitted to an award desk and what it has granted, and the
//! contacts ready to submit.
//!
//! **Where the status lives.** On each contact, in the ADIF fields made for it:
//! `CREDIT_SUBMITTED` (applied for, not yet granted) and `CREDIT_GRANTED` (the award desk
//! credited it), each a list of ADIF Credit codes (`DXCC`, `WAS`, `CQWAZ_MIXED`, `IOTA`,
//! `USACA` …). The operator sets them from the contact card and Bulk edit; LoTW's download fills
//! `CREDIT_GRANTED` for DXCC. Nothing here writes a contact: this module only reads.
//!
//! **Per award, not per contact.** An award credits each entity (a DXCC entity, a state, a zone,
//! an island group, a county) once. An entity is *granted* when any of its contacts holds a
//! granted code of the award's family (`DXCC` or `DXCC_BAND`, as LoTW writes both), *submitted*
//! when one holds a submitted code and none a granted one. **Ready to submit** is every entity
//! confirmed by the award's own rule ([`AwardKind::confirms`]) and neither submitted nor granted,
//! each with ONE contact to submit for it — its oldest confirmed contact, or its newest.
//!
//! **Which contact counts for which entity** is the award fold's own rule ([`crate::awards`]):
//! [`crate::awards::counted_entity`] for DXCC (no satellite contact, no 60 m), WAS by `STATE` on
//! a US-family entity, [`crate::awards::counted_zone`] for WAZ, the `IOTA` reference, and
//! [`crate::us_counties::county_for`] for the counties. The tests hold the counts here to the
//! fold's.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::awards::{counted_entity, counted_zone, earns_dxcc_credit, valid_state};
use crate::awards::{QslChannels, RecordFields};
use crate::dxcc;
use crate::model::Band;

/// An award whose status the log keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AwardKind {
    /// ARRL DXCC (Mixed).
    Dxcc,
    /// ARRL Worked All States.
    Was,
    /// CQ Worked All Zones (Mixed).
    Waz,
    /// RSGB Islands On The Air.
    Iota,
    /// CQ United States of America Counties Award.
    Usaca,
}

impl AwardKind {
    pub const ALL: [AwardKind; 5] = [
        AwardKind::Dxcc,
        AwardKind::Was,
        AwardKind::Waz,
        AwardKind::Iota,
        AwardKind::Usaca,
    ];

    /// The ADIF Credit code the operator's Submitted / Granted writes for this award.
    pub fn credit_code(self) -> &'static str {
        match self {
            AwardKind::Dxcc => "DXCC",
            AwardKind::Was => "WAS",
            AwardKind::Waz => "CQWAZ_MIXED",
            AwardKind::Iota => "IOTA",
            AwardKind::Usaca => "USACA",
        }
    }

    /// The award's family of ADIF Credit codes: `DXCC` takes `DXCC`, `DXCC_BAND`, `DXCC_MODE` …
    fn family(self) -> &'static str {
        match self {
            AwardKind::Waz => "CQWAZ",
            k => k.credit_code(),
        }
    }

    /// Whether `codes` (a contact's `CREDIT_SUBMITTED` or `CREDIT_GRANTED`) hold a code of this
    /// award's family.
    pub fn held_in(self, codes: &[String]) -> bool {
        let family = self.family();
        codes.iter().any(|c| {
            c.eq_ignore_ascii_case(family)
                || (c.len() > family.len()
                    && c[..family.len()].eq_ignore_ascii_case(family)
                    && c.as_bytes()[family.len()] == b'_')
        })
    }

    /// Whether a contact is confirmed for this award, by the award's own rule. `award_confirmed`
    /// is the log's LoTW-or-card flag (an older record may hold it with no channel beside it).
    /// - DXCC, WAS: LoTW or a card (the ARRL takes no eQSL).
    /// - WAZ: those, or an eQSL from an Authenticity-Guaranteed member.
    /// - IOTA: a card or LoTW (IOTA takes LoTW-matched contacts since 2020).
    /// - US Counties: a card, or an eQSL from an Authenticity-Guaranteed member (CQ's rules
    ///   name no LoTW credit for this award).
    pub fn confirms(self, award_confirmed: bool, qsl: QslChannels) -> bool {
        let arrl = award_confirmed || qsl.card || qsl.lotw;
        match self {
            AwardKind::Dxcc | AwardKind::Was | AwardKind::Iota => arrl,
            AwardKind::Waz => arrl || qsl.eqsl_ag_confirmed(),
            AwardKind::Usaca => qsl.card || qsl.eqsl_ag_confirmed(),
        }
    }

    /// The channels that confirm a contact for this award, by name — what an application lists.
    fn channels(self, award_confirmed: bool, qsl: QslChannels) -> Vec<String> {
        let mut out = Vec::new();
        if qsl.card {
            out.push("card");
        }
        let lotw = qsl.lotw || (award_confirmed && !qsl.card);
        if lotw && self != AwardKind::Usaca {
            out.push("lotw");
        }
        if qsl.eqsl_ag_confirmed() && matches!(self, AwardKind::Waz | AwardKind::Usaca) {
            out.push("eqsl");
        }
        out.into_iter().map(str::to_string).collect()
    }
}

/// One contact as the status fold reads it. Borrowed: the caller hands each record in turn.
#[derive(Debug, Clone, Copy, Default)]
pub struct StatusContact<'a> {
    /// The contact's id in the log (what a change to it names).
    pub id: &'a str,
    pub call: &'a str,
    pub when_unix: u64,
    pub band: &'a str,
    pub mode: &'a str,
    /// The log's LoTW-or-card confirmation.
    pub award_confirmed: bool,
    /// A satellite contact (`PROP_MODE=SAT`): no DXCC credit but Satellite DXCC.
    pub sat: bool,
    /// ADIF `STATE`, `CNTY` and `IOTA`.
    pub state: Option<&'a str>,
    pub cnty: Option<&'a str>,
    pub iota: Option<&'a str>,
    /// The DXCC and zone the record carries, and its confirmation channels.
    pub rec: RecordFields,
    pub granted: &'a [String],
    pub submitted: &'a [String],
}

/// The entity of `kind` contact `c` counts for, by the award fold's rule: `(key, label)`.
pub fn entity_of(kind: AwardKind, c: &StatusContact<'_>) -> Option<String> {
    match kind {
        AwardKind::Dxcc => {
            if c.sat || Band::from_label(c.band).is_some_and(|b| !earns_dxcc_credit(b)) {
                return None;
            }
            let counted = counted_entity(dxcc::resolve(c.call).as_ref(), &c.rec)?;
            (!counted.deleted).then(|| counted.name.to_string())
        }
        AwardKind::Was => {
            let counted = counted_entity(dxcc::resolve(c.call).as_ref(), &c.rec)?;
            if !counted.is_us_state() {
                return None;
            }
            c.state.and_then(valid_state).map(str::to_string)
        }
        AwardKind::Waz => {
            counted_zone(dxcc::resolve(c.call).as_ref(), &c.rec).map(|z| z.to_string())
        }
        AwardKind::Iota => c
            .iota
            .map(|s| s.trim().to_ascii_uppercase())
            .filter(|s| !s.is_empty()),
        AwardKind::Usaca => c
            .cnty
            .and_then(|cnty| crate::us_counties::county_for(cnty, c.state))
            .and_then(crate::us_counties::county)
            .map(|county| county.label()),
    }
}

/// A contact to submit for an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyRow {
    /// The entity it is submitted for: a DXCC entity's name, a state's code, a zone's number, an
    /// island group's reference, a county as `STATE,Name`.
    pub entity: String,
    pub id: String,
    pub call: String,
    pub when_unix: u64,
    pub band: String,
    pub mode: String,
    /// The channels that confirm it for this award (`card`, `lotw`, `eqsl`).
    pub confirmed_by: Vec<String>,
}

/// One award's status over the log: entities worked, confirmed, submitted and granted, and the
/// contacts ready to submit, by entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwardStatus {
    pub award: AwardKind,
    /// The ADIF Credit code a Submitted / Granted writes.
    pub credit_code: String,
    pub worked: usize,
    pub confirmed: usize,
    /// Submitted and not yet granted.
    pub submitted: usize,
    pub granted: usize,
    /// One contact per entity confirmed and neither submitted nor granted, by entity.
    pub ready: Vec<ReadyRow>,
}

#[derive(Default)]
struct Entity {
    pick: Option<ReadyRow>,
    confirmed: bool,
    submitted: bool,
    granted: bool,
}

/// The status of one award, folded a contact at a time ([`Self::add`], then [`Self::finish`]).
pub struct StatusFold {
    kind: AwardKind,
    newest: bool,
    entities: BTreeMap<String, Entity>,
}

impl StatusFold {
    /// `newest`: submit each entity's newest confirmed contact; else its oldest.
    pub fn new(kind: AwardKind, newest: bool) -> Self {
        StatusFold {
            kind,
            newest,
            entities: BTreeMap::new(),
        }
    }

    pub fn add(&mut self, c: &StatusContact<'_>) {
        let Some(key) = entity_of(self.kind, c) else {
            return;
        };
        let e = self.entities.entry(key.clone()).or_default();
        e.granted |= self.kind.held_in(c.granted);
        e.submitted |= self.kind.held_in(c.submitted);
        if !self.kind.confirms(c.award_confirmed, c.rec.qsl) {
            return;
        }
        e.confirmed = true;
        let better = match &e.pick {
            None => true,
            Some(p) if self.newest => c.when_unix > p.when_unix,
            Some(p) => c.when_unix < p.when_unix,
        };
        if better {
            e.pick = Some(ReadyRow {
                entity: key,
                id: c.id.to_string(),
                call: c.call.to_string(),
                when_unix: c.when_unix,
                band: c.band.to_string(),
                mode: c.mode.to_string(),
                confirmed_by: self.kind.channels(c.award_confirmed, c.rec.qsl),
            });
        }
    }

    pub fn finish(self) -> AwardStatus {
        let mut out = AwardStatus {
            award: self.kind,
            credit_code: self.kind.credit_code().to_string(),
            worked: self.entities.len(),
            confirmed: 0,
            submitted: 0,
            granted: 0,
            ready: Vec::new(),
        };
        for e in self.entities.into_values() {
            out.confirmed += usize::from(e.confirmed);
            if e.granted {
                out.granted += 1;
            } else if e.submitted {
                out.submitted += 1;
            } else if let Some(row) = e.pick {
                out.ready.push(row);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::awards::{Awards, ContactExtras};

    struct C {
        id: &'static str,
        call: &'static str,
        when: u64,
        band: &'static str,
        conf: bool,
        qsl: QslChannels,
        state: Option<&'static str>,
        cnty: Option<&'static str>,
        iota: Option<&'static str>,
        dxcc: Option<u32>,
        granted: Vec<String>,
        submitted: Vec<String>,
    }

    fn c(id: &'static str, call: &'static str, when: u64) -> C {
        C {
            id,
            call,
            when,
            band: "20m",
            conf: false,
            qsl: QslChannels::default(),
            state: None,
            cnty: None,
            iota: None,
            dxcc: None,
            granted: Vec::new(),
            submitted: Vec::new(),
        }
    }

    impl C {
        fn lotw(mut self) -> Self {
            self.conf = true;
            self.qsl.lotw = true;
            self
        }
        fn card(mut self) -> Self {
            self.conf = true;
            self.qsl.card = true;
            self
        }
        fn eqsl_ag(mut self) -> Self {
            self.qsl.eqsl = true;
            self.qsl.eqsl_ag = true;
            self
        }
        fn contact(&self) -> StatusContact<'_> {
            StatusContact {
                id: self.id,
                call: self.call,
                when_unix: self.when,
                band: self.band,
                mode: "CW",
                award_confirmed: self.conf,
                sat: false,
                state: self.state,
                cnty: self.cnty,
                iota: self.iota,
                rec: RecordFields {
                    dxcc: self.dxcc,
                    qsl: self.qsl,
                    when_unix: self.when,
                    ..RecordFields::default()
                },
                granted: &self.granted,
                submitted: &self.submitted,
            }
        }
    }

    fn status(kind: AwardKind, newest: bool, log: &[C]) -> AwardStatus {
        let mut f = StatusFold::new(kind, newest);
        for q in log {
            f.add(&q.contact());
        }
        f.finish()
    }

    fn ids(s: &AwardStatus) -> Vec<&str> {
        s.ready.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn dxcc_ready_is_confirmed_by_lotw_or_card_and_not_yet_submitted_or_granted() {
        let mut granted = c("g", "JA1ABC", 10).lotw();
        granted.granted = vec!["DXCC".into(), "DXCC_BAND".into()];
        let mut submitted = c("s", "G4ABC", 10).card();
        submitted.submitted = vec!["DXCC".into()];
        let log = [
            // Germany: two confirmed contacts, none credited — ready, oldest by default.
            c("dl-new", "DL1ABC", 300).lotw(),
            c("dl-old", "DL2ABC", 100).card(),
            c("dl-unconf", "DL3ABC", 50),
            // Japan granted on one contact: another confirmed one is not ready.
            granted,
            c("ja2", "JA2ABC", 20).card(),
            // England submitted.
            submitted,
            // France: an eQSL from an AG member is no DXCC confirmation.
            c("f", "F5ABC", 10).eqsl_ag(),
            // Italy: worked only.
            c("i", "I2ABC", 10),
        ];
        let s = status(AwardKind::Dxcc, false, &log);
        assert_eq!(ids(&s), ["dl-old"]);
        assert_eq!(
            (s.worked, s.confirmed, s.submitted, s.granted),
            (5, 3, 1, 1)
        );
        assert_eq!(s.ready[0].confirmed_by, ["card"]);
        assert_eq!(ids(&status(AwardKind::Dxcc, true, &log)), ["dl-new"]);
    }

    #[test]
    fn a_granted_band_credit_alone_marks_the_entity_granted_and_another_award_does_not() {
        let mut band = c("a", "DL1ABC", 10).lotw();
        band.granted = vec!["DXCC_BAND".into()];
        assert!(status(AwardKind::Dxcc, false, &[band]).ready.is_empty());
        // WAS credit is not DXCC credit, and a code merely starting with the letters is not one.
        let mut was = c("b", "DL1ABC", 10).lotw();
        was.granted = vec!["WAS".into(), "DXCCX".into()];
        assert_eq!(ids(&status(AwardKind::Dxcc, false, &[was])), ["b"]);
    }

    #[test]
    fn dxcc_counts_as_the_award_fold_counts() {
        // 60 m earns no DXCC credit; the record's DXCC number wins over the call.
        let mut sixty = c("60", "DL1ABC", 10).lotw();
        sixty.band = "60m";
        let mut navassa = c("kp1", "K1N", 10).card();
        navassa.dxcc = Some(182);
        let s = status(AwardKind::Dxcc, false, &[sixty, navassa]);
        assert_eq!(ids(&s), ["kp1"]);
        assert_eq!(s.ready[0].entity, dxcc::entity_by_dxcc(182).unwrap().name);
    }

    #[test]
    fn waz_takes_an_ag_eqsl_and_was_does_not() {
        let mut w = c("w", "W1AW", 10).eqsl_ag();
        w.state = Some("CT");
        let log = [w];
        assert_eq!(ids(&status(AwardKind::Waz, false, &log)), ["w"]);
        assert!(status(AwardKind::Was, false, &log).ready.is_empty());
        assert_eq!(status(AwardKind::Was, false, &log).worked, 1);
        assert_eq!(
            status(AwardKind::Waz, false, &log).ready[0].confirmed_by,
            ["eqsl"]
        );
    }

    #[test]
    fn was_needs_a_us_entity_and_iota_takes_lotw() {
        let mut vk = c("vk", "VK6ABC", 10).lotw();
        vk.state = Some("WA");
        let mut w7 = c("w7", "W7ABC", 10).lotw();
        w7.state = Some("WA");
        let s = status(AwardKind::Was, false, &[vk, w7]);
        assert_eq!(ids(&s), ["w7"]);
        let mut isl = c("isl", "EI1ABC", 10).lotw();
        isl.iota = Some(" eu-115 ");
        let s = status(AwardKind::Iota, false, &[isl]);
        assert_eq!(s.ready[0].entity, "EU-115");
    }

    #[test]
    fn counties_count_from_cnty_and_confirm_by_card_or_ag_eqsl_not_lotw() {
        let mut card = c("card", "W9ABC", 10).card();
        card.cnty = Some("Marion, IN");
        let mut lotw = c("lotw", "W9XYZ", 20).lotw();
        lotw.cnty = Some("IN,Lake");
        let mut eq = c("eq", "W9QQQ", 30).eqsl_ag();
        eq.cnty = Some("Allen");
        eq.state = Some("IN");
        let mut odd = c("odd", "W9ZZZ", 40).card();
        odd.cnty = Some("VA,Fairfax City");
        let s = status(AwardKind::Usaca, false, &[card, lotw, eq, odd]);
        assert_eq!(
            ids(&s),
            ["eq", "card"],
            "by county: IN,Allen then IN,Marion"
        );
        assert_eq!(s.ready[0].entity, "IN,Allen");
        assert_eq!((s.worked, s.confirmed), (3, 2));
        assert_eq!(s.ready[1].confirmed_by, ["card"]);
    }

    /// The status fold and the award fold read the same contacts the same way: the confirmed
    /// counts agree on a log that exercises every award's rule.
    #[test]
    fn confirmed_counts_agree_with_the_award_fold() {
        let mut log = vec![
            c("1", "DL1ABC", 1).lotw(),
            c("2", "JA1ABC", 2).card(),
            c("3", "F5ABC", 3).eqsl_ag(),
            c("4", "G4ABC", 4),
        ];
        let mut w = c("5", "W9ABC", 5).card();
        w.state = Some("IN");
        w.cnty = Some("IN,Marion");
        w.iota = Some("NA-001");
        log.push(w);
        let mut e = c("6", "W1ABC", 6).eqsl_ag();
        e.state = Some("MA");
        e.cnty = Some("MA,Middlesex");
        log.push(e);
        let mut sixty = c("7", "SM5ABC", 7).lotw();
        sixty.band = "60m";
        log.push(sixty);

        let mut awards = Awards::new();
        for q in &log {
            let k = q.contact();
            awards.add_contact(
                k.call,
                k.band,
                k.mode,
                k.award_confirmed,
                false,
                q.qsl.card,
                k.state,
                None,
                k.iota,
                false,
                k.rec,
                ContactExtras {
                    cnty: k.cnty,
                    ..ContactExtras::default()
                },
            );
        }
        let sum = awards.summary();
        let confirmed = |kind| status(kind, false, &log).confirmed;
        assert_eq!(confirmed(AwardKind::Dxcc), sum.dxcc_confirmed);
        assert_eq!(confirmed(AwardKind::Was), sum.was.confirmed);
        assert_eq!(confirmed(AwardKind::Waz), sum.waz_confirmed);
        assert_eq!(confirmed(AwardKind::Iota), sum.iota.confirmed);
        assert_eq!(confirmed(AwardKind::Usaca), sum.usaca.confirmed);
        assert_eq!((sum.usaca.worked, sum.usaca.confirmed), (2, 2));
    }

    #[test]
    fn the_award_fold_counts_counties_and_the_ones_it_cannot_place() {
        let mut awards = Awards::new();
        let rec = RecordFields::default();
        for cnty in [
            "IN,Marion",
            "in, marion county",
            "VA,Fairfax City",
            "IN,Lake",
        ] {
            awards.add_contact(
                "W9ABC",
                "20m",
                "CW",
                false,
                false,
                false,
                Some("IN"),
                None,
                None,
                false,
                rec,
                ContactExtras {
                    cnty: Some(cnty),
                    ..ContactExtras::default()
                },
            );
        }
        let u = awards.summary().usaca;
        assert_eq!(
            (u.worked, u.confirmed, u.unplaced, u.total),
            (2, 0, 1, 3077)
        );
        assert_eq!((u.level, u.next_level), (None, Some(500)));
    }
}
