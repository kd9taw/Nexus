//! The Cabrillo entry HEADER, as data — and the `CONTEST` token a mode-split
//! contest resolves to.
//!
//! ⭐ **This module exists because one of those header lines was a lie.**
//! `FieldDayLog::cabrillo` wrote
//!
//! ```text
//! CATEGORY-OPERATOR: MULTI-OP
//! ```
//!
//! as a string literal, on every entry Nexus has ever exported. `MULTI-OP` is a
//! CLAIM — that more than one operator was at the station — and a solo Field Day
//! entrant submitting that header submits a false one, in the file a sponsor scores.
//! It was not a value anybody could set; it was a `format!` argument. So the headers
//! become a record with a declared source, the operator category rides the SESSION
//! (the object that is the entry), and the default is [`OperatorCategory::SingleOp`]
//! — because a lone operator running a desktop logger is the case this build is
//! wrong about today, and a club that runs a multi-operator entry is already
//! configuring positions.
//!
//! ⚠️ **Only the headers with a SOURCE in this batch are modelled.** §6.1 lists
//! `CATEGORY-POWER`, `CLUB`, `OPERATORS` and `CLAIMED-SCORE` too; each of those is
//! fed by a picker or a score the batch that opens the Settings ▸ Contesting surface
//! supplies. A field nothing fills is a field that ships empty, and an emitted header
//! nobody set is the defect above written a second time.

/// Cabrillo's `CATEGORY-OPERATOR` — who operated the entry.
///
/// The three tokens Cabrillo 3.0 defines for the axis this build can declare.
/// `SINGLE-OP` is the default because it is the true statement about a lone
/// operator, which is the case the hardcoded `MULTI-OP` was wrong about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OperatorCategory {
    #[default]
    SingleOp,
    MultiOp,
    Checklog,
}

impl OperatorCategory {
    /// The Cabrillo token.
    pub const fn token(self) -> &'static str {
        match self {
            OperatorCategory::SingleOp => "SINGLE-OP",
            OperatorCategory::MultiOp => "MULTI-OP",
            OperatorCategory::Checklog => "CHECKLOG",
        }
    }

    /// The category a token names — `None` for anything else.
    ///
    /// `None` rather than a default, deliberately: a picker value this build does not
    /// understand must not silently become `SINGLE-OP`, which is a claim about the
    /// entry.
    pub fn from_token(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "SINGLE-OP" => Some(OperatorCategory::SingleOp),
            "MULTI-OP" => Some(OperatorCategory::MultiOp),
            "CHECKLOG" => Some(OperatorCategory::Checklog),
            _ => None,
        }
    }
}

/// The header block of one Cabrillo entry.
///
/// Every value is supplied by the caller; nothing here reads a setting or a clock.
/// [`render`](Self::render) emits the lines in Cabrillo's own order and omits nothing
/// — a header modelled here is a header this build can source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CabrilloHeaders {
    /// `CONTEST` — the sponsor's token, already resolved through
    /// [`resolve_contest_id`] for a mode-split contest and translated out of ADIF's
    /// namespace by [`cabrillo_contest_token`].
    pub contest: String,
    /// `CALLSIGN` — the station callsign the entry is submitted under.
    pub callsign: String,
    /// `CATEGORY-OPERATOR` — the entry declaration, no longer a literal.
    pub category_operator: OperatorCategory,
    /// `LOCATION` — the ENTRY's declared location, once. A mobile's per-contact
    /// truth is on the QSO lines and is not this value.
    pub location: String,
    /// `CREATED-BY` — the program that wrote the file.
    pub created_by: String,
    /// `X-` headers, emitted last in the order given. Cabrillo-legal and ignored by
    /// robots; `X-NEXUS-RULES-YEAR` says which rules data scored the entry.
    pub x_headers: Vec<(String, String)>,
}

impl CabrilloHeaders {
    /// The header lines, `START-OF-LOG` first, each newline-terminated.
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("START-OF-LOG: 3.0\n");
        s.push_str(&format!("CONTEST: {}\n", self.contest));
        s.push_str(&format!("CALLSIGN: {}\n", self.callsign));
        s.push_str(&format!(
            "CATEGORY-OPERATOR: {}\n",
            self.category_operator.token()
        ));
        s.push_str(&format!("LOCATION: {}\n", self.location));
        s.push_str(&format!("CREATED-BY: {}\n", self.created_by));
        for (tag, val) in &self.x_headers {
            s.push_str(&format!("{tag}: {val}\n"));
        }
        s
    }
}

/// The `CONTEST` token for a log, resolved against the mode classes it holds.
///
/// A mode-split contest has a DISTINCT id per mode — `ARRL-SS-CW` and `ARRL-SS-SSB`
/// are two entries, scored separately — so a single file that holds both modes is not
/// a valid entry under either id. `by_mode` empty means the contest is not split and
/// `single` is the answer for every log.
///
/// **Refusing is the only honest third answer.** Picking the first id silently
/// submits the CW contacts as phone; picking neither writes a file with no `CONTEST`
/// header. The message names both ids because that is what tells the operator what to
/// do about it.
pub fn resolve_contest_id(
    single: &str,
    by_mode: &[(String, String)],
    classes: &[&str],
) -> Result<String, String> {
    if by_mode.is_empty() {
        return Ok(single.to_string());
    }
    let mut found: Vec<(&str, &str)> = Vec::new();
    for c in classes {
        let class = c.trim();
        if class.is_empty() {
            continue;
        }
        let id = by_mode
            .iter()
            .find(|(m, _)| m.eq_ignore_ascii_case(class))
            .map(|(_, id)| id.as_str())
            .ok_or_else(|| {
                format!(
                    "this contest declares a separate entry per mode and none is \
                     declared for {class} contacts — the log cannot be exported \
                     until it is"
                )
            })?;
        if !found.iter().any(|(_, i)| *i == id) {
            found.push((class, id));
        }
    }
    match found.as_slice() {
        // An empty log under a mode-split contest has no mode to resolve by; the
        // first declared id is the only non-arbitrary answer, and the file holds no
        // QSO lines to be wrong about.
        [] => Ok(by_mode[0].1.clone()),
        [(_, id)] => Ok((*id).to_string()),
        many => {
            let named = many
                .iter()
                .map(|(class, id)| format!("{class} contacts as {id}"))
                .collect::<Vec<_>>()
                .join(", and ");
            Err(format!(
                "this contest submits a separate entry per mode — {named} — so one \
                 file cannot be both. Export one mode at a time."
            ))
        }
    }
}

/// The Cabrillo `CONTEST:` token for a contest whose ADIF `CONTEST_ID` is `adif_id`.
///
/// ⭐ **These are two registries, not one, and for three shipped contests they
/// disagree.** A ruleset's `contest_id` is an ADIF value — ADIF 3.1.7's
/// `CONTEST_ID` row says "use enumeration values for interoperability", and every
/// id this build ships is verbatim from that enumeration (`ARRL-FIELD-DAY`,
/// `OH-QSO-PARTY`, `TX-QSO-PARTY`, `TN-QSO-PARTY`, `CA-QSO-PARTY`, `WFD`;
/// <https://adif.org/317/ADIF_317.htm> §III.B.5, read 2026-09-09). Cabrillo has no
/// such enumeration: the V3 header specification states that "Contest text values
/// are not an official part of the specification. Contest sponsors may define their
/// own contest values", and points at the WA7BNM Contest Calendar "Master List of
/// Cabrillo Names" (<https://www.contestcalendar.com/cabnames.php>, Revision Date
/// February 23, 2026, read 2026-09-09) as the extension of its own 31-name list.
/// That list names Field Day `ARRL-FD`, the Ohio QSO Party `MRRC-OHQP` (for the Mad
/// River Radio Club) and the Texas QSO Party `TXQP` — none of which is an ADIF
/// value, and none of which could be inferred: the list's state-party names are
/// irregular (`7QP`, `COQP`, `FCG-FQP`, `IAQP`, `KYQP`, `MRRC-OHQP`, `NEQP`,
/// `NJQP`, `SDQSOP`, `TXQP`, `WIQP`, `WVQP` sit alongside the `XX-QSO-PARTY`
/// forms).
///
/// So this maps ONE namespace to the other, at the last step before the header, and
/// the three divergences are its whole body. **It must never run the other way:**
/// `ARRL-FD`, `MRRC-OHQP` and `TXQP` are not ADIF enumeration values, and writing
/// one into `CONTEST_ID` ships an unresolvable token into other people's logbooks.
///
/// ⚠️ **`CQP` is the Collegiate QSO Party** (master list id 122), not California —
/// California is `CA-QSO-PARTY` in both registries. Nexus's internal event key for
/// the California QSO Party is `cqp`, so the shortening looks natural and files the
/// log under a different contest.
///
/// An id with no entry is its own Cabrillo token, which is the answer for the other
/// three shipped contests and for every contest the master list agrees with ADIF
/// about.
pub fn cabrillo_contest_token(adif_id: &str) -> &str {
    match adif_id {
        // master list id 57, "ARRL Field Day"
        "ARRL-FIELD-DAY" => "ARRL-FD",
        // master list id 100, "Ohio QSO Party" — the Mad River Radio Club
        "OH-QSO-PARTY" => "MRRC-OHQP",
        // master list id 133, "Texas QSO Party"
        "TX-QSO-PARTY" => "TXQP",
        other => other,
    }
}

/// Does this side's slot list declare a [`Call`](super::spec::FieldKind::Call) slot —
/// i.e. is this side's callsign ALREADY the QSO line's own callsign column?
///
/// ⭐ **§6.2's derived exception, with the direction the sponsor's own template gives
/// it.** A Cabrillo QSO line is
///
/// ```text
/// QSO: <freq> <mo> <date> <time> <mycall> <my fields…> <theircall> <their fields…> [t]
/// ```
///
/// — the two callsign columns sit OUTSIDE the exchange and are structural: a writer
/// that emits only exchange slots loses both callsigns on every contest, and an
/// unsubmittable line is the failure mode. Sweepstakes is the one contest whose ON-AIR
/// exchange also contains a callsign (SS-Rules v2.1 §4.3: *"Your call sign (the call
/// sign must be included during the exchange)"*), so without an exception its line
/// would carry each callsign TWICE — four callsigns on a line that admits two.
///
/// ⚠️ **The exception suppresses the SLOT, not the structural column, and that is a
/// correction to the design sketch made against ARRL's own published template.** §6.2
/// assumed the reverse — that the structural column is dropped and the `Call` slot
/// supplies the callsign "at its own declared position", third of five — and reserved
/// the question for the batch that read the sponsor. It is read.
/// <https://www.arrl.org/cabrillo-format-tutorial> (ARRL's own Cabrillo Format &
/// Tutorial page, read 2026-09-09) publishes a **QSO DATA TEMPLATE** for Sweepstakes
/// with a lettered legend:
///
/// ```text
/// Guide:A        B     C          D     E     F G H  I  J     K L M  N
/// QSO: 14000 CW 2009-11-07 2100 W1AW   1 M 38 CT K8MM  1 Q 92 MI
/// ```
///
/// > *"E= Your call. F= Your QSO #. G= Your precedence. H= Your check … I= Your ARRL
/// > Section. J= The call of the station you worked. K= Their QSO number to you.
/// > L= Their precedence. M= Their check. N = Their ARRL Section."*
///
/// The callsign is column **E**, first of the side — the structural position every
/// other contest uses — and the exchange columns that follow are serial, precedence,
/// check, section, with **no callsign among them**. So the `Call` slot's Cabrillo home
/// IS the structural column, and what the exception removes is the slot's second
/// appearance. Written the other way round, a Nexus SS entry would read
/// `… 1 M W1AW 38 CT …` against a template that says `… W1AW 1 M 38 CT …` — the same
/// two callsigns, in columns the sponsor's parser reads as something else.
///
/// The exception is still read off the slot list rather than declared by a flag: there
/// is one source of truth and no ruleset can say `Call` in `sends` and `structural
/// callsign` in the same breath.
pub fn side_declares_call(spec: &super::spec::ExchangeSpec, slots: &[&'static str]) -> bool {
    slots.iter().any(|k| is_call_slot(spec, k))
}

/// Is this ONE slot the exchange's callsign — the slot the QSO line's structural
/// column already carries, and which must therefore not be written a second time?
///
/// The per-slot half of [`side_declares_call`], and the predicate the QSO-line writer
/// filters on. Both read the same `FieldKind`, so "this side declares a call" and
/// "this is the column that carries it" cannot come to mean different things.
pub fn is_call_slot(spec: &super::spec::ExchangeSpec, key: &str) -> bool {
    matches!(
        spec.field(key).map(|f| f.kind),
        Some(super::spec::FieldKind::Call)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contest::exchanges::sweepstakes_shaped;
    use crate::contest::field_day;
    use crate::fieldday::FdEvent;

    /// ⭐ **Every shipped contest's Cabrillo `CONTEST:` token, pinned to the value
    /// verified against the master list on 2026-09-09 (Sweepstakes' two against ARRL's
    /// own published headers as well, and CQ WW's and CQ WPX's four against the
    /// sponsors' own `cabrillo.htm` pages)** — so a later edit that
    /// "tidies" one back to a regular-looking `XX-QSO-PARTY` form goes red instead
    /// of shipping.
    ///
    /// Each id comes through `ruleset_by_id`, the fallible lookup the app itself
    /// uses, so this is the token a real export would carry and not a literal
    /// nobody reads. The ADIF half is pinned in the same breath: those six values
    /// are the ADIF 3.1.7 `CONTEST_ID` enumeration, and the Cabrillo correction
    /// must never be applied to them.
    #[test]
    fn every_shipped_contest_emits_its_verified_cabrillo_token() {
        // POSITIVE CONTROL: an id the two registries agree about passes through
        // untouched, so the six answers below are a MAP and not a rewriter that
        // returns whatever it is handed.
        assert_eq!(cabrillo_contest_token("CQ-WW-CW"), "CQ-WW-CW");
        // ⚠️ California must never shorten to CQP — on the master list that is the
        // Collegiate QSO Party (id 122), a different contest, and Nexus's own
        // internal event key for California is `cqp`.
        assert_ne!(cabrillo_contest_token("CA-QSO-PARTY"), "CQP");
        let mut wrong: Vec<String> = Vec::new();
        for (event, adif_id, cabrillo) in [
            ("arrlfd", "ARRL-FIELD-DAY", "ARRL-FD"),
            ("wfd", "WFD", "WFD"),
            ("tnqp", "TN-QSO-PARTY", "TN-QSO-PARTY"),
            ("ohqp", "OH-QSO-PARTY", "MRRC-OHQP"),
            ("cqp", "CA-QSO-PARTY", "CA-QSO-PARTY"),
            ("txqp", "TX-QSO-PARTY", "TXQP"),
            // ⭐ Sweepstakes is the contest where the two registries AGREE, and it is
            // pinned for that reason: ADIF 3.1.7's CONTEST_ID enumeration lists
            // ARRL-SS-CW = "ARRL November Sweepstakes (CW)" and ARRL-SS-SSB = "ARRL
            // November Sweepstakes (Phone)" (adif.org/317/ADIF_317.htm, "updated
            // 2026-03-22", read 2026-09-09); the WA7BNM master list carries the same
            // two strings at ids 177 and 178; and ARRL's own cabrillo-format-tutorial
            // page prints the literal lines "CONTEST: ARRL-SS-CW" and "CONTEST:
            // ARRL-SS-SSB". So the map must leave both ALONE — a future "tidy" that
            // added an ARRL-SS-* row here would break the one contest whose two
            // registries already match.
            ("arrlss_cw", "ARRL-SS-CW", "ARRL-SS-CW"),
            ("arrlss_ssb", "ARRL-SS-SSB", "ARRL-SS-SSB"),
            // ⭐ CQ WW and CQ WPX — four more rows where the two registries AGREE, each
            // verified against its OWN source on 2026-09-09 because a token confirmed in
            // one registry says nothing about the other. ADIF 3.1.7's CONTEST_ID
            // enumeration (https://adif.org/317/ADIF_317.htm, "updated 2026-03-22") lists
            // CQ-WW-CW = "CQ WW DX Contest (CW)", CQ-WW-SSB = "CQ WW DX Contest (SSB)",
            // CQ-WPX-CW = "CQ WW WPX Contest (CW)" and CQ-WPX-SSB = "CQ WW WPX Contest
            // (SSB)". The WA7BNM master list (contestcalendar.com/cabnames.php, Revision
            // Date February 23, 2026) carries the same four strings at ids 192, 172, 29
            // and 291. And the SPONSORS publish them directly: cqww.com/cabrillo.htm and
            // cqwpx.com/cabrillo.htm each say "The contest-name must be one of the
            // following" over exactly two names, with "Be sure to use hyphens as shown. DO
            // NOT PUT THE YEAR OR USE PH INSTEAD OF SSB."
            //
            // ⚠️ So `cabrillo_contest_token` gets NO arm for any of them, and these four
            // rows are what stops one being added. ⚠️ Neither sponsor publishes an RTTY
            // token: ADIF's own labels for CQ-WW-RTTY and CQ-WPX-RTTY are "CQ/RJ WW RTTY
            // DX Contest" and "CQ/RJ WW RTTY WPX Contest" — a different sponsor's contests
            // with their own rules — so no RTTY ruleset ships here and neither string may
            // be attached to these rulesets.
            ("cqww_cw", "CQ-WW-CW", "CQ-WW-CW"),
            ("cqww_ssb", "CQ-WW-SSB", "CQ-WW-SSB"),
            ("cqwpx_cw", "CQ-WPX-CW", "CQ-WPX-CW"),
            ("cqwpx_ssb", "CQ-WPX-SSB", "CQ-WPX-SSB"),
        ] {
            let rs = crate::fd_rules::ruleset_by_id(event, crate::fd_rules::CURRENT_RULES_YEAR)
                .unwrap_or_else(|| panic!("the seed must carry {event}"));
            if rs.contest_id != adif_id {
                wrong.push(format!(
                    "{event} ADIF CONTEST_ID is {:?}, want {adif_id:?}",
                    rs.contest_id
                ));
            }
            let got = cabrillo_contest_token(rs.contest_id);
            if got != cabrillo {
                wrong.push(format!(
                    "{event} Cabrillo CONTEST: token is {got:?}, want {cabrillo:?}"
                ));
            }
        }
        // Every mismatch at once: three of these six were wrong together, and a
        // run that names only the first hides the other two.
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// ⭐ §6.2 — the exception is READ OFF the slot list. Field Day's sides declare no
    /// `Call` slot, so both callsign columns are structural; the Sweepstakes shape's
    /// do, so neither is.
    #[test]
    fn only_a_side_with_a_call_slot_supplies_its_own_callsign() {
        let fd = field_day(FdEvent::ArrlFd);
        let fd_role = fd.roles[0];
        assert!(!side_declares_call(fd, fd_role.sends));
        assert!(!side_declares_call(fd, fd_role.receives));
        // POSITIVE CONTROL: an exchange that really does carry one trips it, so the
        // two `false`s above are the absence of a `Call` slot and not a matcher that
        // never fires.
        let ss = sweepstakes_shaped();
        let ss_role = ss.roles[0];
        assert!(side_declares_call(ss, ss_role.sends));
        assert!(side_declares_call(ss, ss_role.receives));
    }

    #[test]
    fn the_operator_category_round_trips_through_its_token() {
        for c in [
            OperatorCategory::SingleOp,
            OperatorCategory::MultiOp,
            OperatorCategory::Checklog,
        ] {
            assert_eq!(OperatorCategory::from_token(c.token()), Some(c));
        }
        assert_eq!(
            OperatorCategory::from_token(" multi-op "),
            Some(OperatorCategory::MultiOp),
            "a picker value is trimmed and case-folded"
        );
        // A token this build does not understand is NOT quietly a single-op entry.
        assert_eq!(OperatorCategory::from_token("SINGLE-OP-ASSISTED"), None);
        assert_eq!(OperatorCategory::from_token(""), None);
    }

    /// The default is the statement that is true of a lone operator. The hardcoded
    /// `MULTI-OP` this replaces was the opposite claim, submitted for every entry.
    #[test]
    fn the_default_entry_is_single_op() {
        assert_eq!(OperatorCategory::default().token(), "SINGLE-OP");
    }

    #[test]
    fn the_header_block_is_the_seven_cabrillo_lines_in_order() {
        let h = CabrilloHeaders {
            contest: "ARRL-FD".into(),
            callsign: "W9XYZ".into(),
            category_operator: OperatorCategory::MultiOp,
            location: "WI".into(),
            created_by: "Nexus".into(),
            x_headers: vec![("X-NEXUS-RULES-YEAR".into(), "2026".into())],
        };
        assert_eq!(
            h.render(),
            "START-OF-LOG: 3.0\n\
             CONTEST: ARRL-FD\n\
             CALLSIGN: W9XYZ\n\
             CATEGORY-OPERATOR: MULTI-OP\n\
             LOCATION: WI\n\
             CREATED-BY: Nexus\n\
             X-NEXUS-RULES-YEAR: 2026\n"
        );
    }

    /// A contest with one id answers with it for every log, including one that spans
    /// every mode — which is what keeps both Field Day events out of the refusal.
    ///
    /// The id here is the contest's DECLARED id, which is what the caller passes:
    /// this resolver only picks between mode arms, and the translation to Cabrillo's
    /// namespace happens after it ([`cabrillo_contest_token`]). So `ARRL-FIELD-DAY`
    /// is the right literal below and must not be "corrected" to `ARRL-FD`.
    #[test]
    fn an_unsplit_contest_resolves_to_its_one_id() {
        assert_eq!(
            resolve_contest_id("ARRL-FIELD-DAY", &[], &["CW", "PH", "DIG"]),
            Ok("ARRL-FIELD-DAY".to_string())
        );
        assert_eq!(
            resolve_contest_id("WFD", &[], &[]),
            Ok("WFD".to_string()),
            "an empty log still has a CONTEST header"
        );
    }

    /// ⭐ §6.2 — a mode-split contest whose log holds both modes is REFUSED, and the
    /// message names both ids.
    #[test]
    fn a_mode_split_log_holding_both_modes_is_refused_by_name() {
        let split = vec![
            ("CW".to_string(), "ARRL-SS-CW".to_string()),
            ("PH".to_string(), "ARRL-SS-SSB".to_string()),
        ];
        // POSITIVE CONTROL first: each mode ALONE resolves, so the refusal below is
        // the two-mode condition and not a resolver that always refuses.
        assert_eq!(
            resolve_contest_id("ARRL-SS", &split, &["CW"]),
            Ok("ARRL-SS-CW".to_string())
        );
        assert_eq!(
            resolve_contest_id("ARRL-SS", &split, &["PH", "PH"]),
            Ok("ARRL-SS-SSB".to_string())
        );
        let err = resolve_contest_id("ARRL-SS", &split, &["CW", "PH"])
            .expect_err("both modes in one file is not an entry");
        assert!(err.contains("ARRL-SS-CW"), "{err}");
        assert!(err.contains("ARRL-SS-SSB"), "{err}");
    }

    /// A class the split declares nothing for is named, not mapped to whichever id
    /// happens to be first.
    #[test]
    fn a_mode_the_split_does_not_declare_is_refused_by_name() {
        let split = vec![("CW".to_string(), "ARRL-SS-CW".to_string())];
        let err = resolve_contest_id("ARRL-SS", &split, &["DIG"])
            .expect_err("an undeclared mode class has no id");
        assert!(err.contains("DIG"), "{err}");
    }
}
