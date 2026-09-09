//! ⭐ **`constant_sent` — the cross-row invariant, on the SENT side only.**
//!
//! Sweepstakes' CHECK is the only field in the researched set that is a property of the
//! LOG rather than of a contact. SS-Rules v2.1 §4.4 (read 2026-09-09):
//!
//! > *"4.4 Check — 4.4.1 The last two digits of the year the operator or the station was
//! > first licensed. 4.4.2 **The same Check must be sent throughout the contest.**"*
//!
//! ⚠️ **Both directions of that sentence matter, and read the wrong way round this flag
//! refuses legal contacts.** It constrains the check *I send*: rows 1–20 sending `74`
//! and row 21 sending `47` is one operator's typo, and worth saying so. It says nothing
//! whatever about the checks I *receive* — every station I work was licensed in a
//! different year, so a received check differing per row is the contest working
//! correctly. That is why the rule lives on
//! [`RoleSpec::constant_sent`](super::spec::RoleSpec::constant_sent), which is a list
//! over the SEND order, and not on the shared [`FieldSpec`](super::spec::FieldSpec).
//!
//! **It warns; it never blocks a contact.** The software cannot know whether the old
//! rows or the new one carry the typo, so refusing to log would throw away a real
//! contact on a guess. Export is different and is not a contradiction: a log whose rows
//! disagree is not a submittable entry under §4.4.2, which is the same class of refusal
//! `cabrillo()` already makes for a mode-split log holding both modes.

use super::spec::{FieldValue, RoleSpec};

/// One slot whose sent value changed part-way through a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstantSentMismatch {
    /// The slot id — `"CK"` for Sweepstakes.
    pub key: &'static str,
    /// What the log's FIRST row sent.
    pub first: String,
    /// …and the differing value.
    pub then: String,
    /// How many rows carried each, `(first, then)`. `(0, 0)` from the O(1) log-time
    /// comparison, which has one row of each in hand and no reason to walk the log.
    pub rows: (usize, usize),
}

impl ConstantSentMismatch {
    /// The sentence the operator reads. It names the slot, BOTH values and the row
    /// counts, because "your check is inconsistent" does not tell anybody which of the
    /// two to correct.
    pub fn message(&self) -> String {
        let ConstantSentMismatch {
            key,
            first,
            then,
            rows,
        } = self;
        match rows {
            (0, 0) => format!(
                "This contest sends the same {key} on every contact, and this one sends \
{then:?} where the log's earlier contacts sent {first:?}. One of the two is a typo — \
the contact is logged either way, so fix whichever is wrong."
            ),
            (a, b) => format!(
                "This contest sends the same {key} on every contact, and this log sends \
two: {first:?} on {a} contact(s) and {then:?} on {b}. One of the two is a typo. Fix the \
rows that are wrong and export again — a log that sends two different {key} values is \
not one entry."
            ),
        }
    }
}

/// The log-time check: does `tx` disagree with `first_tx` on any slot the role
/// declares constant? **O(1) in the log's length** — it compares the new row against
/// the first, which is the whole content of "every row carries the same value".
///
/// This is the moment the operator can still fix it, which is why the cheap check runs
/// here and the thorough one runs at export.
pub fn against_first(
    role: &'static RoleSpec,
    first_tx: &[FieldValue],
    tx: &[FieldValue],
) -> Option<ConstantSentMismatch> {
    for key in role.constant_sent {
        let first = raw(first_tx, key);
        let then = raw(tx, key);
        // An ABSENT value is not a differing one: a row that carries no value for the
        // slot (a legacy journal row, a merged row from a position that sent none)
        // says nothing about what was sent, and reading its emptiness as a second
        // value would fire on every log that has one.
        if first.is_empty() || then.is_empty() || first == then {
            continue;
        }
        return Some(ConstantSentMismatch {
            key,
            first: first.to_string(),
            then: then.to_string(),
            rows: (0, 0),
        });
    }
    None
}

/// The export-time scan: the whole log, with the row counts the message needs.
///
/// `rows` yields each row's SENT field vector, in log order. The first row carrying a
/// value for a slot is the reference — not the first row outright, because a leading
/// row with no value for the slot would make every later row a mismatch.
pub fn scan<'a>(
    role: &'static RoleSpec,
    rows: impl Iterator<Item = &'a [FieldValue]> + Clone,
) -> Option<ConstantSentMismatch> {
    for key in role.constant_sent {
        let mut first: Option<&str> = None;
        let mut n_first = 0usize;
        let mut odd: Option<(&str, usize)> = None;
        for tx in rows.clone() {
            let v = raw(tx, key);
            if v.is_empty() {
                continue;
            }
            match first {
                None => {
                    first = Some(v);
                    n_first = 1;
                }
                Some(f) if f == v => n_first += 1,
                Some(_) => match &mut odd {
                    // Only the FIRST differing value is named. A log with three
                    // different checks has the same defect and the same fix, and a
                    // message listing all of them is a message nobody finishes reading.
                    Some((o, n)) if *o == v => *n += 1,
                    Some(_) => {}
                    None => odd = Some((v, 1)),
                },
            }
        }
        if let (Some(f), Some((o, n))) = (first, odd) {
            return Some(ConstantSentMismatch {
                key,
                first: f.to_string(),
                then: o.to_string(),
                rows: (n_first, n),
            });
        }
    }
    None
}

fn raw<'a>(v: &'a [FieldValue], key: &str) -> &'a str {
    v.iter()
        .find(|f| f.key == key)
        .map(|f| f.raw.as_str())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contest::spec::{RoleSelector, RoleSpec};

    static SS_ROLE: RoleSpec = RoleSpec {
        id: "",
        selector: RoleSelector::Always,
        sends: &["NR", "PREC", "CALL", "CK", "SEC"],
        receives: &["NR", "PREC", "CALL", "CK", "SEC"],
        constant_sent: &["CK"],
    };
    static NO_CONSTANT: RoleSpec = RoleSpec {
        id: "",
        selector: RoleSelector::Always,
        sends: &["CLASS", "SECTION"],
        receives: &["CLASS", "SECTION"],
        constant_sent: &[],
    };

    fn tx(check: &str) -> Vec<FieldValue> {
        vec![
            FieldValue {
                key: "NR",
                raw: "1".into(),
                domain: None,
            },
            FieldValue {
                key: "CK",
                raw: check.into(),
                domain: None,
            },
        ]
    }

    /// ⭐ The invariant fires on a SENT value that changed part-way through.
    #[test]
    fn a_sent_check_that_changes_part_way_through_is_named() {
        let m = against_first(&SS_ROLE, &tx("74"), &tx("47")).expect("74 then 47");
        assert_eq!(m.key, "CK");
        assert_eq!((m.first.as_str(), m.then.as_str()), ("74", "47"));
        // Both values in the message, or the operator cannot tell which to correct.
        assert!(m.message().contains("\"74\""), "{}", m.message());
        assert!(m.message().contains("\"47\""), "{}", m.message());
    }

    /// POSITIVE CONTROL for the line above and the direction of the whole rule: the
    /// SAME value does not fire, and a role that declares no constant slot never fires
    /// however different its rows are.
    #[test]
    fn an_unchanged_check_and_an_unconstrained_role_never_fire() {
        assert_eq!(against_first(&SS_ROLE, &tx("74"), &tx("74")), None);
        assert_eq!(against_first(&NO_CONSTANT, &tx("74"), &tx("47")), None);
    }

    /// A row carrying NO value for the slot is not a second value — otherwise every
    /// log with one legacy row would fire.
    #[test]
    fn an_absent_value_is_not_a_differing_one() {
        assert_eq!(against_first(&SS_ROLE, &tx("74"), &tx("")), None);
        assert_eq!(against_first(&SS_ROLE, &tx(""), &tx("47")), None);
    }

    /// The export scan counts the rows on each side, which is what tells the operator
    /// how much of the log to fix.
    #[test]
    fn the_export_scan_counts_the_rows_on_each_side() {
        let rows = [tx("74"), tx("74"), tx("74"), tx("47"), tx("47")];
        let borrowed: Vec<&[FieldValue]> = rows.iter().map(|r| r.as_slice()).collect();
        let m = scan(&SS_ROLE, borrowed.iter().copied()).expect("three then two");
        assert_eq!(m.rows, (3, 2));
        assert!(m.message().contains(" 3 contact(s)"), "{}", m.message());
        assert!(m.message().contains(" 2"), "{}", m.message());

        // POSITIVE CONTROL: a consistent log scans clean, so the count above is the
        // mismatch and not a scan that always reports one.
        let ok = [tx("74"), tx("74")];
        let ok: Vec<&[FieldValue]> = ok.iter().map(|r| r.as_slice()).collect();
        assert_eq!(scan(&SS_ROLE, ok.iter().copied()), None);
    }

    /// A leading row with no value for the slot does not make every later row a
    /// mismatch — the reference is the first row that HAS one.
    #[test]
    fn the_reference_is_the_first_row_that_carries_a_value() {
        let rows = [tx(""), tx("74"), tx("74")];
        let borrowed: Vec<&[FieldValue]> = rows.iter().map(|r| r.as_slice()).collect();
        assert_eq!(scan(&SS_ROLE, borrowed.iter().copied()), None);
    }
}
