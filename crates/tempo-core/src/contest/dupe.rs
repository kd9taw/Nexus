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
//! with four rules that are each there because their absence is a bug:
//!
//! * a `bool` component is **omitted entirely** when its flag is false — never pushed
//!   as an empty string, or `["W1AW", "", "", "CT"]` and `["W1AW", "CT"]` become two
//!   keys for one rule;
//! * `by_fields` and `by_sent_fields` contribute in the order the rules file declares
//!   them, received before sent;
//! * every component is trimmed and uppercased, as the shipped `(call, band, mode)`
//!   tuple already did;
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
}

impl DupeRule {
    /// The key for a logged row.
    pub fn key(&self, row: &crate::fieldday::LoggedQso) -> Vec<String> {
        self.key_of(&row.call, &row.band, &row.mode, &row.rx, &row.tx)
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
    ) -> Vec<String> {
        let mut k = Vec::with_capacity(3 + self.by_fields.len() + self.by_sent_fields.len());
        if self.by_call {
            k.push(norm(call));
        }
        if self.by_band {
            k.push(norm(band));
        }
        if self.by_mode_class {
            k.push(norm(mode_class));
        }
        for key in self.by_fields {
            k.push(norm(field(rx, key)));
        }
        for key in self.by_sent_fields {
            k.push(norm(field(tx, key)));
        }
        k
    }

    /// The key as one string, for a wire or a DTO that cannot carry a vector.
    pub fn joined(&self, row: &crate::fieldday::LoggedQso) -> String {
        join(&self.key(row))
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

#[cfg(test)]
mod tests {
    use super::*;

    const FD: DupeRule = DupeRule {
        by_call: true,
        by_band: true,
        by_mode_class: true,
        by_fields: &[],
        by_sent_fields: &[],
    };

    /// The QSO-party rule: a mobile in a new county is a new station, in BOTH
    /// directions.
    const QSO_PARTY: DupeRule = DupeRule {
        by_call: true,
        by_band: true,
        by_mode_class: true,
        by_fields: &["QTH"],
        by_sent_fields: &["QTH"],
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
            FD.key_of("w1aw", " 20m ", "cw", &[], &[]),
            vec!["W1AW", "20M", "CW"]
        );
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
        };
        assert_eq!(
            ss.key_of("W1AW", "20m", "CW", &[fv("QTH", "CT")], &[]),
            vec!["W1AW", "CT"]
        );
        assert_ne!(
            join(&ss.key_of("W1AW", "20m", "CW", &[fv("QTH", "CT")], &[])),
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
        };
        assert_eq!(
            r.key_of("W1AW", "", "", &[fv("B", "bee")], &[fv("C", "see")]),
            vec!["W1AW", "", "BEE", "SEE"]
        );
    }

    /// §4.1 direction 1 — I work somebody else's mobile.
    #[test]
    fn working_someone_elses_mobile_moves_the_key_and_working_them_twice_does_not() {
        let mine = [fv("QTH", "FRAN")];
        let k1 = QSO_PARTY.key_of("W8XYZ", "40m", "CW", &[fv("QTH", "FRAN")], &mine);
        let k2 = QSO_PARTY.key_of("W8XYZ", "40m", "CW", &[fv("QTH", "FAIR")], &mine);
        let k3 = QSO_PARTY.key_of("W8XYZ", "40m", "CW", &[fv("QTH", "FRAN")], &mine);
        assert_ne!(k1, k2, "the mobile moved — a new contact");
        assert_eq!(k1, k3, "same county, same station — a dupe");
    }

    /// §4.1 direction 2 — I AM the mobile. THEIR exchange is unchanged, so only the
    /// sent half can tell these apart.
    #[test]
    fn being_the_mobile_moves_the_key_from_the_sent_side_alone() {
        let theirs = [fv("QTH", "TX")];
        let davi = QSO_PARTY.key_of("K4ABC", "20m", "CW", &theirs, &[fv("QTH", "DAVI")]);
        let will = QSO_PARTY.key_of("K4ABC", "20m", "CW", &theirs, &[fv("QTH", "WILL")]);
        let again = QSO_PARTY.key_of("K4ABC", "20m", "CW", &theirs, &[fv("QTH", "WILL")]);
        assert_ne!(davi, will, "I crossed the county line — a new contact");
        assert_eq!(will, again, "same county, same station — a dupe");
        // POSITIVE CONTROL: with the sent half dropped from the rule the two are
        // indistinguishable, which is the defect this list exists to remove.
        let received_only = DupeRule {
            by_sent_fields: &[],
            ..QSO_PARTY
        };
        assert_eq!(
            received_only.key_of("K4ABC", "20m", "CW", &theirs, &[fv("QTH", "DAVI")]),
            received_only.key_of("K4ABC", "20m", "CW", &theirs, &[fv("QTH", "WILL")]),
        );
    }
}
