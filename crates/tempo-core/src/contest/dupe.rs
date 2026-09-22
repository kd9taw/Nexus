//! The dupe key, as data — an ordered `Vec<String>` a ruleset builds, not a tuple
//! three crates and a TypeScript file each rebuild by hand.
//!
//! ⭐ **Two field lists, because the mobile rule has two directions.** `by_fields`
//! covers working *someone else's* mobile: same call, same band, same mode, different
//! received county is a new contact. `by_sent_fields` covers *being* the mobile: when
//! I move from one county to the next and work the same station again, THEIR exchange
//! is unchanged, so a key built from the received side alone refuses my own legal
//! contact. For a QSO party both lists are the same slot and that one row of data
//! covers both sides of the sponsor's rule.
//!
//! ⚠️ **The component ORDER is specified, not implied by the struct.** Four sites in
//! two languages must build the identical key — this module, the club's merged rows,
//! the wire, and the while-typing verdict in the UI — so "obvious from the field
//! order" is not a specification. It is:
//!
//! ```text
//! [ CALL ]  [ BAND ]  [ MODE CLASS ]  [ by_fields… ]  [ by_sent_fields… ]
//! ```
//!
//! ⭐ **A SATELLITE CONTACT ADDS NO COMPONENT — it changes what two of them SAY.** ARRL
//! Field Day: *"Satellite QSOs also count for regular QSO credit. Show them listed
//! separately on the summary sheet as a separate 'band.'"* So with
//! [`DupeRule::satellite_is_a_band`] the BAND position carries `SAT/<bird>` instead of
//! the band, and with [`DupeRule::fm_satellite_once`] the MODE CLASS position of a
//! single-channel FM bird's row is empty — *"Stations are limited to one (1) completed
//! QSO on any single channel FM satellite."* The key's LENGTH and ORDER never move, which
//! is what lets the club's pre-joined keys and the UI's verdict keep reading it.
//!
//! with four rules that are each there because their absence is a bug:
//!
//! * a `bool` component is **omitted entirely** when its flag is false — never pushed
//!   as an empty string, or `["W1AW", "", "", "CT"]` and `["W1AW", "CT"]` become two
//!   keys for one rule;
//! * `by_fields` and `by_sent_fields` contribute in the order the rules file declares
//!   them, received before sent;
//! * every component is trimmed and uppercased, as the shipped `(call, band, mode)`
//!   tuple already did;
//! * the MODE CLASS component is the class itself unless the ruleset groups it
//!   ([`DupeRule::mode_class_groups`] — ILQP counts CW and digital as one mode), in
//!   which case it is the group's first member. The position never changes;
//! * a slot the row does not carry contributes the empty string **in position** — a
//!   missing value is not the same as an absent component, and it must not shift what
//!   follows it.
use super::spec::FieldValue;

/// The separator for a key that must become one string (the pre-joined club keys on
/// the DTO). `\u{1}` is what `logbook::worked_band_set` already folds with, for exactly
/// this reason — one convention in the codebase rather than two.
pub const KEY_SEP: char = '\u{1}';

/// When a station counts again, as data.
///
/// Field Day's rule — a station once per band per mode class — is the shipped
/// behaviour and stays exactly that: both field lists empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DupeRule {
    pub by_call: bool,
    pub by_band: bool,
    pub by_mode_class: bool,
    /// RECEIVED slot ids — working someone else's mobile.
    pub by_fields: &'static [&'static str],
    /// SENT slot ids — being the mobile.
    pub by_sent_fields: &'static [&'static str],
    /// ⭐ **Mode classes that count as ONE mode.** Empty — every contest before the
    /// Illinois QSO Party — is three separate classes, the shipped behaviour.
    ///
    /// ILQP's own 2025 rules (w9awe.org, read 2026-09-17): *"Stations may be worked
    /// once per band and mode (phone and CW/digital)"*. The parenthesis names two
    /// modes where this build counts three, so `[["CW", "DIG"]]` says it: a class in a
    /// group contributes the group's FIRST member to the key, and a class in no group
    /// contributes itself.
    ///
    /// It lives on the rule rather than in a caller because four sites in two
    /// languages build this key (this module's header), and a fold applied by one of
    /// them is exactly how they come to disagree.
    pub mode_class_groups: &'static [&'static [&'static str]],
    /// ⭐ **What a duplicate IS to this sponsor: a row that scores zero, or a contact
    /// that was never made.** `false` — the shipped behaviour, and still Field Day's —
    /// REFUSES the contact and writes nothing. `true` LOGS it, marks it
    /// ([`LoggedQso::dupe`](crate::fieldday::LoggedQso::dupe)) and scores it zero.
    ///
    /// **Both sponsors that cross-check say the same thing.** ARRL LGCK.1 and CQ
    /// XII.E.1: *"Duplicate contacts are removed with no additional penalty."* CQ
    /// instructs entrants in as many words — *"Yes! Please log all contacts that you
    /// make even if they are duplicates"*, *"No, please do not remove any QSOs from
    /// your log! This will cause the other station that worked you to lose credit for
    /// the contact."* Dropping the row is what costs points, and it costs them to the
    /// OTHER operator: their contact becomes Not-In-Log, 1× extra at ARRL, 2× extra at
    /// CQ. The checker is what zeroes a dupe; the entrant's job is to report it.
    ///
    /// ⚠️ **It is `false` for ARRL and Winter Field Day, and that is not an oversight.**
    /// Field Day has no log checking at all — no cross-check, no NIL, and *"Complete
    /// station logs are NOT required for submission, and ARRL does not use the logs."*
    /// The whole justification above is inapplicable there, and Field Day's deliverable
    /// is a dupe sheet plus **raw non-dupe** QSO counts. So FD keeps refusing.
    ///
    /// ⚠️ **Not part of the key**, deliberately: it says what to DO with a duplicate,
    /// never what makes two contacts the same one. The four key-building sites in two
    /// languages (this module's header) are untouched by it, and the while-typing DUPE
    /// verdict reads the same as it always did — what changed is only what happens when
    /// the operator commits anyway.
    pub log_dupes: bool,
    /// ⭐ **A BIRD IS ITS OWN BAND.** ARRL Field Day, rule 7.3.8: *"Satellite QSOs also
    /// count for regular QSO credit. Show them listed separately on the summary sheet as
    /// a separate 'band.'"*
    ///
    /// `true` puts `SAT/<bird>` in the key's BAND position for a contact worked through
    /// a satellite, so a station worked on 70 cm terrestrially and again through a 70 cm
    /// bird is two contacts and both count. `false` — every ruleset that has not asked
    /// for it, which is all of them but ARRL Field Day — is the shipped behaviour
    /// exactly: the band travels, and a satellite row is indistinguishable from a
    /// terrestrial one on the same band.
    ///
    /// ⚠️ It needs [`by_band`](Self::by_band): with no band position there is nowhere to
    /// put it, and the rules loader refuses the combination rather than reading a flag
    /// that silently does nothing.
    pub satellite_is_a_band: bool,
    /// ⭐ **ONE QSO PER SINGLE-CHANNEL FM SATELLITE.** ARRL Field Day: *"Stations are
    /// limited to one (1) completed QSO on any single channel FM satellite."*
    ///
    /// `true` drops the MODE CLASS out of the key for a row whose bird is a single
    /// channel — on one channel there is no second mode to work the same station in, so
    /// keeping it would let one bird yield two contacts. [`satellite_is_a_band`] already
    /// refuses a repeat in the SAME mode; this is the part it does not reach.
    ///
    /// ⛔ **It applies to a single-channel bird and to nothing else.** The same sponsor
    /// sentence says the additional QSOs through a LINEAR transponder *"may be counted
    /// for QSO credit"*, and a limit applied to one of those refuses a contact the rules
    /// allow — on the only copy of it there is. Which birds are which is a fact about the
    /// bird ([`SatLeg::single_channel_fm`](crate::fieldday::SatLeg::single_channel_fm)),
    /// stated by the satellite catalogue, never inferred here.
    ///
    /// ⚠️ It needs [`satellite_is_a_band`](Self::satellite_is_a_band). Without it the
    /// band position still holds the real band, and an emptied mode class would make an
    /// FM satellite contact collide with a TERRESTRIAL one on the same band — a legal
    /// contact refused, which is the direction this module refuses to fail in. The rules
    /// loader refuses the combination, and [`build`](Self::build) checks it again rather
    /// than trusting a hand-built rule.
    pub fm_satellite_once: bool,
}

/// ⭐ **What the dupe key needs to know about the bird a contact was worked through** —
/// a borrowed view, so the key builder never has to own a satellite type.
///
/// [`Default`] is the TERRESTRIAL contact (`bird: ""`), which is what nearly every caller
/// passes and what every ruleset but ARRL Field Day ignores entirely.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SatKey<'a> {
    /// The satellite's own name — never the transponder's, because ARRL's limit is per
    /// SATELLITE and a bird with a voice repeater and a packet digipeater is one bird.
    /// Empty = a terrestrial contact, and then neither satellite flag can change the key.
    pub bird: &'a str,
    /// A single-channel FM satellite rather than a linear transponder — see
    /// [`DupeRule::fm_satellite_once`].
    pub single_channel_fm: bool,
}

impl DupeRule {
    /// The key for a logged row.
    pub fn key(&self, row: &crate::fieldday::LoggedQso) -> Vec<String> {
        self.key_of(
            &row.call,
            &row.band,
            &row.mode,
            &row.rx,
            &row.tx,
            row.sat_key(),
        )
    }

    /// The key for a contact that is not a row yet — the while-typing verdict and the
    /// pre-log dupe check. Same builder as [`key`](Self::key), because two builders is
    /// how the two halves come to disagree.
    pub fn key_of(
        &self,
        call: &str,
        band: &str,
        mode_class: &str,
        rx: &[FieldValue],
        tx: &[FieldValue],
        sat: SatKey<'_>,
    ) -> Vec<String> {
        self.build(
            call,
            band,
            mode_class,
            sat,
            &|k| Some(field(rx, k).to_string()),
            &|k| Some(field(tx, k).to_string()),
        )
        .expect("a contest-log lookup never declines")
    }

    /// The key for a GENERAL-LOG row, whose exchange is `(slot, raw)` pairs rather
    /// than the contest log's triples — `None` when the row cannot supply every slot
    /// the rule names.
    ///
    /// ⭐ **The `None` is the §3.1 advisory rule, and its direction is the whole
    /// point.** A record that cannot supply a named component does not enter the
    /// exact-key set at all; the caller puts it in a worked-this-session set keyed on
    /// the call alone, which the UI shows as an advisory and never as a DUPE refusal.
    /// **Under-reporting a dupe costs one duplicate contact that scores zero;
    /// over-reporting refuses a legal contact.**
    pub fn key_of_pairs(
        &self,
        call: &str,
        band: &str,
        mode_class: &str,
        rcvd: &[(String, String)],
        sent: &[(String, String)],
        sat: SatKey<'_>,
    ) -> Option<Vec<String>> {
        self.build(call, band, mode_class, sat, &|k| pair(rcvd, k), &|k| {
            pair(sent, k)
        })
    }

    /// The ONE key builder. Both public builders funnel through it with different
    /// missing-slot policies, because two builders is how the contest half and the
    /// general half come to disagree about the shape of a key they must union.
    fn build(
        &self,
        call: &str,
        band: &str,
        mode_class: &str,
        sat: SatKey<'_>,
        rx: &dyn Fn(&str) -> Option<String>,
        tx: &dyn Fn(&str) -> Option<String>,
    ) -> Option<Vec<String>> {
        let mut k = Vec::with_capacity(3 + self.by_fields.len() + self.by_sent_fields.len());
        // A contact worked through a bird, under a ruleset that asks for it. Resolved
        // once: both components below read it, and they must agree about whether this
        // row is a satellite row at all.
        let bird = sat.bird.trim();
        let via_sat = self.satellite_is_a_band && self.by_band && !bird.is_empty();
        if self.by_call {
            k.push(norm(call));
        }
        if self.by_band {
            // ⭐ A BIRD IS ITS OWN BAND (ARRL 7.3.8). The band POSITION is unchanged —
            // what it says is the bird. `SAT/` prefixed so it can never be read as, or
            // collide with, a band label.
            k.push(if via_sat {
                norm(&format!("SAT/{bird}"))
            } else {
                norm(band)
            });
        }
        if self.by_mode_class {
            // ⭐ ONE CHANNEL, ONE QSO. Emptied IN POSITION rather than omitted — the key
            // must keep its length, and this cannot collide with anything: the band
            // component beside it already says `SAT/<bird>`, so the only row an empty
            // mode class here can match is another contact through the same bird.
            //
            // ⛔ `via_sat` is load-bearing and not belt-and-braces: with the real band
            // still in the position above, an emptied mode class would make this row
            // collide with a TERRESTRIAL contact on the same band and refuse a legal
            // QSO. The rules loader refuses that combination too; a hand-built rule
            // cannot reach it either.
            k.push(
                if via_sat && self.fm_satellite_once && sat.single_channel_fm {
                    String::new()
                } else {
                    self.mode_key(&norm(mode_class))
                },
            );
        }
        for key in self.by_fields {
            k.push(norm(&rx(key)?));
        }
        for key in self.by_sent_fields {
            k.push(norm(&tx(key)?));
        }
        Some(k)
    }

    /// The key as one string, for a wire or a DTO that cannot carry a vector.
    pub fn joined(&self, row: &crate::fieldday::LoggedQso) -> String {
        join(&self.key(row))
    }

    /// ⭐ **What one mode class contributes to the key**, which is the class itself
    /// unless this ruleset groups it ([`mode_class_groups`](Self::mode_class_groups)).
    ///
    /// The group's FIRST member is the representative rather than a synthesised label
    /// (`"CW+DIG"`): a key component that is not a class this build recognises would be
    /// a new token for every reader of a key — the club wire and the UI included — to
    /// learn, and the representative needs only to be stable, which a declared list's
    /// first entry is.
    ///
    /// `class` is already normalised by the caller.
    pub fn mode_key(&self, class: &str) -> String {
        self.mode_class_groups
            .iter()
            .find(|g| g.iter().any(|m| m.eq_ignore_ascii_case(class)))
            .and_then(|g| g.first())
            .map_or_else(|| class.to_string(), |m| norm(m))
    }
}

/// A key vector as one string. Public because the club wire and the UI both need the
/// same fold, and a second implementation of it is a second chance to disagree.
pub fn join(key: &[String]) -> String {
    key.join(&KEY_SEP.to_string())
}

fn norm(s: &str) -> String {
    s.trim().to_ascii_uppercase()
}

fn field<'a>(vals: &'a [FieldValue], key: &str) -> &'a str {
    vals.iter()
        .find(|v| v.key == key)
        .map(|v| v.raw.as_str())
        .unwrap_or("")
}

/// One slot of a general-log pair vector. An absent slot AND a present-but-empty one
/// are both `None`: a blank county is not a county, and treating it as one would put
/// every blank row on the same key and refuse the second of them.
fn pair(vals: &[(String, String)], key: &str) -> Option<String> {
    vals.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, raw)| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FD: DupeRule = DupeRule {
        by_call: true,
        by_band: true,
        by_mode_class: true,
        by_fields: &[],
        by_sent_fields: &[],
        mode_class_groups: &[],
        log_dupes: false,
        satellite_is_a_band: false,
        fm_satellite_once: false,
    };

    /// The QSO-party rule: a mobile in a new county is a new station, in BOTH
    /// directions.
    const QSO_PARTY: DupeRule = DupeRule {
        by_call: true,
        by_band: true,
        by_mode_class: true,
        by_fields: &["QTH"],
        by_sent_fields: &["QTH"],
        mode_class_groups: &[],
        log_dupes: false,
        satellite_is_a_band: false,
        fm_satellite_once: false,
    };

    fn fv(key: &'static str, raw: &str) -> FieldValue {
        FieldValue {
            key,
            raw: raw.into(),
            domain: None,
        }
    }

    #[test]
    fn field_days_key_is_exactly_the_shipped_tuple() {
        assert_eq!(
            FD.key_of(
                "w1aw",
                " 20m ",
                "cw",
                &[],
                &[],
                crate::contest::SatKey::default()
            ),
            vec!["W1AW", "20M", "CW"]
        );
    }

    /// ⭐ **Two mode classes that count as ONE mode** — the Illinois QSO Party's own
    /// 2025 rules (w9awe.org, read 2026-09-17): *"Stations may be worked once per band
    /// and mode (phone and CW/digital)"*. The parenthesis is the whole rule: there are
    /// two modes there, not three, so a station worked on CW and then on RTTY on the
    /// same band is a DUPE — and every contest before it counts them separately.
    ///
    /// The grouping is on the RULE rather than in the caller because four sites in two
    /// languages build this key (this module's header), and a fold applied by one of
    /// them is how they come to disagree.
    #[test]
    fn a_grouped_mode_class_keys_as_its_group_and_an_ungrouped_one_as_itself() {
        const ILQP: DupeRule = DupeRule {
            by_call: true,
            by_band: true,
            by_mode_class: true,
            by_fields: &["QTH"],
            by_sent_fields: &["QTH"],
            mode_class_groups: &[&["CW", "DIG"]],
            log_dupes: false,
            satellite_is_a_band: false,
            fm_satellite_once: false,
        };
        let rx = [fv("QTH", "COOK")];
        let tx = [fv("QTH", "COOK")];
        let key = |r: &DupeRule, m: &str| {
            join(&r.key_of(
                "W9AWE",
                "40m",
                m,
                &rx,
                &tx,
                crate::contest::SatKey::default(),
            ))
        };
        assert_eq!(
            key(&ILQP, "CW"),
            key(&ILQP, "DIG"),
            "CW then digital, same band, same station — one contact"
        );
        assert_ne!(
            key(&ILQP, "PH"),
            key(&ILQP, "CW"),
            "…and phone is still its own mode"
        );
        // POSITIVE CONTROL: the same three contacts under a rule with no grouping —
        // every shipped ruleset — are three keys, not two.
        const UNGROUPED: DupeRule = DupeRule {
            mode_class_groups: &[],
            ..ILQP
        };
        let plain: Vec<String> = ["PH", "CW", "DIG"]
            .iter()
            .map(|m| key(&UNGROUPED, m))
            .collect();
        assert_eq!(
            plain.iter().collect::<std::collections::HashSet<_>>().len(),
            3,
            "without the grouping the three classes are three keys"
        );
        // A grouped class keys as the group's FIRST member, whichever member the row
        // carries — the mode component, in its own position, and nothing else moves.
        assert_eq!(
            ILQP.key_of(
                "W9AWE",
                "40m",
                "DIG",
                &rx,
                &tx,
                crate::contest::SatKey::default()
            ),
            vec!["W9AWE", "40M", "CW", "COOK", "COOK"]
        );
        // And the fold is case- and whitespace-insensitive like every other component.
        assert_eq!(key(&ILQP, " dig "), key(&ILQP, "CW"));
    }

    /// A false flag omits its component; it never contributes an empty string, or two
    /// different rules collapse onto one key.
    #[test]
    fn a_false_flag_omits_its_component_rather_than_blanking_it() {
        let ss = DupeRule {
            by_call: true,
            by_band: false,
            by_mode_class: false,
            by_fields: &["QTH"],
            by_sent_fields: &[],
            mode_class_groups: &[],
            log_dupes: false,
            satellite_is_a_band: false,
            fm_satellite_once: false,
        };
        assert_eq!(
            ss.key_of(
                "W1AW",
                "20m",
                "CW",
                &[fv("QTH", "CT")],
                &[],
                crate::contest::SatKey::default()
            ),
            vec!["W1AW", "CT"]
        );
        assert_ne!(
            join(&ss.key_of(
                "W1AW",
                "20m",
                "CW",
                &[fv("QTH", "CT")],
                &[],
                crate::contest::SatKey::default()
            )),
            join(&[
                "W1AW".to_string(),
                String::new(),
                String::new(),
                "CT".to_string()
            ]),
            "a blanked component and an omitted one must not be the same key"
        );
    }

    /// Received components come before sent ones, and a slot the row does not carry
    /// holds its POSITION with an empty string rather than shifting the rest along.
    #[test]
    fn a_missing_slot_holds_its_position() {
        let r = DupeRule {
            by_call: true,
            by_band: false,
            by_mode_class: false,
            by_fields: &["A", "B"],
            by_sent_fields: &["C"],
            mode_class_groups: &[],
            log_dupes: false,
            satellite_is_a_band: false,
            fm_satellite_once: false,
        };
        assert_eq!(
            r.key_of(
                "W1AW",
                "",
                "",
                &[fv("B", "bee")],
                &[fv("C", "see")],
                crate::contest::SatKey::default()
            ),
            vec!["W1AW", "", "BEE", "SEE"]
        );
    }

    /// ⭐ §3.1 — the two halves of the session B4 index are built by the SAME builder,
    /// so a general-log row and a contest-log row with the same exchange produce the
    /// same key and the union means something.
    #[test]
    fn a_general_log_row_builds_the_identical_key_as_a_contest_row() {
        let rx = [fv("QTH", "FRAN")];
        let tx = [fv("QTH", "DAVI")];
        let pairs_rx = [("QTH".to_string(), "fran".to_string())];
        let pairs_tx = [("QTH".to_string(), "davi".to_string())];
        assert_eq!(
            QSO_PARTY.key_of_pairs(
                "w8xyz",
                "40m",
                "CW",
                &pairs_rx,
                &pairs_tx,
                crate::contest::SatKey::default()
            ),
            Some(QSO_PARTY.key_of(
                "W8XYZ",
                "40M",
                "cw",
                &rx,
                &tx,
                crate::contest::SatKey::default()
            )),
        );
    }

    /// …and a row that cannot supply a named slot DECLINES rather than keying on a
    /// blank, which is what keeps an ordinary contact out of the exact set instead of
    /// colliding every blank row onto one key.
    #[test]
    fn a_row_that_cannot_supply_a_named_slot_declines() {
        let tx = [("QTH".to_string(), "DAVI".to_string())];
        assert_eq!(
            QSO_PARTY.key_of_pairs(
                "W8XYZ",
                "40m",
                "CW",
                &[],
                &tx,
                crate::contest::SatKey::default()
            ),
            None,
            "no received county — no exact key"
        );
        assert_eq!(
            QSO_PARTY.key_of_pairs(
                "W8XYZ",
                "40m",
                "CW",
                &[("QTH".to_string(), "   ".to_string())],
                &tx,
                crate::contest::SatKey::default(),
            ),
            None,
            "a blank county is not a county"
        );
        // POSITIVE CONTROL: a rule naming no exchange slot — Field Day's — always
        // keys, so the decline above is about the missing slot and not about a
        // builder that never answers.
        assert!(FD
            .key_of_pairs(
                "W8XYZ",
                "40m",
                "CW",
                &[],
                &[],
                crate::contest::SatKey::default()
            )
            .is_some());
    }

    /// §4.1 direction 1 — I work somebody else's mobile.
    #[test]
    fn working_someone_elses_mobile_moves_the_key_and_working_them_twice_does_not() {
        let mine = [fv("QTH", "FRAN")];
        let k1 = QSO_PARTY.key_of(
            "W8XYZ",
            "40m",
            "CW",
            &[fv("QTH", "FRAN")],
            &mine,
            crate::contest::SatKey::default(),
        );
        let k2 = QSO_PARTY.key_of(
            "W8XYZ",
            "40m",
            "CW",
            &[fv("QTH", "FAIR")],
            &mine,
            crate::contest::SatKey::default(),
        );
        let k3 = QSO_PARTY.key_of(
            "W8XYZ",
            "40m",
            "CW",
            &[fv("QTH", "FRAN")],
            &mine,
            crate::contest::SatKey::default(),
        );
        assert_ne!(k1, k2, "the mobile moved — a new contact");
        assert_eq!(k1, k3, "same county, same station — a dupe");
    }

    /// §4.1 direction 2 — I AM the mobile. THEIR exchange is unchanged, so only the
    /// sent half can tell these apart.
    #[test]
    fn being_the_mobile_moves_the_key_from_the_sent_side_alone() {
        let theirs = [fv("QTH", "TX")];
        let davi = QSO_PARTY.key_of(
            "K4ABC",
            "20m",
            "CW",
            &theirs,
            &[fv("QTH", "DAVI")],
            crate::contest::SatKey::default(),
        );
        let will = QSO_PARTY.key_of(
            "K4ABC",
            "20m",
            "CW",
            &theirs,
            &[fv("QTH", "WILL")],
            crate::contest::SatKey::default(),
        );
        let again = QSO_PARTY.key_of(
            "K4ABC",
            "20m",
            "CW",
            &theirs,
            &[fv("QTH", "WILL")],
            crate::contest::SatKey::default(),
        );
        assert_ne!(davi, will, "I crossed the county line — a new contact");
        assert_eq!(will, again, "same county, same station — a dupe");
        // POSITIVE CONTROL: with the sent half dropped from the rule the two are
        // indistinguishable, which is the defect this list exists to remove.
        let received_only = DupeRule {
            by_sent_fields: &[],
            mode_class_groups: &[],
            ..QSO_PARTY
        };
        assert_eq!(
            received_only.key_of(
                "K4ABC",
                "20m",
                "CW",
                &theirs,
                &[fv("QTH", "DAVI")],
                crate::contest::SatKey::default()
            ),
            received_only.key_of(
                "K4ABC",
                "20m",
                "CW",
                &theirs,
                &[fv("QTH", "WILL")],
                crate::contest::SatKey::default()
            ),
        );
    }
}
