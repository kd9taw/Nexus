//! ⭐ **Sweepstakes' PRECEDENCE, derived from the entry's declared category.**
//!
//! SS is the one researched contest whose exchange carries a letter that is not a
//! value the operator holds — it is a restatement of the category they entered under.
//! Typing it into the entry strip would let the two disagree: an entry whose Cabrillo
//! header says `CATEGORY-OPERATOR: MULTI-OP` and whose 400 QSO lines all say `A` is
//! wrong in one of two places and the sponsor cannot tell which. So the letter is
//! DERIVED, from the same declaration the headers are written from, and there is one
//! source of truth for "what did I enter as".
//!
//! **The table is the sponsor's own** — `https://contests.arrl.org/ContestRules/SS-Rules.pdf`,
//! Version 2.1, §4.2, all 10 pages extracted and read 2026-09-09:
//!
//! > *"4.2 Precedence
//! > “Q” for Single Operator, QRP
//! > “A” for Single Operator, Low Power
//! > “B” for Single Operator, High Power
//! > “U” for Single Operator Unlimited, High Power, Single Operator Unlimited, Low
//! > Power and Single Operator Unlimited, QRP
//! > “M” for Multioperator, High Power or Multioperator, Low Power
//! > “S” for School Club"*
//!
//! and the power thresholds are the same document's Entry Categories table
//! (*"Version 1.6 – 20 Sep 2023"*): *"1 — 5 watts PEP output or less"* (QRP),
//! *"2 — 100 watts PEP output or less"* (Low Power), *"3 — 1500 watts PEP output or
//! the maximum allowable power level established by the national licensing authority
//! …"* (High Power).
//!
//! ⚠️ **Four axes, and this build had one of them.** The letter is a function of
//! `CATEGORY-OPERATOR` × `CATEGORY-POWER` × `CATEGORY-ASSISTED` × `CATEGORY-STATION` —
//! §6.1's own header table — and batch 8 shipped only the first. The other three are
//! settings beside it, in Cabrillo's own vocabulary, because that is the vocabulary the
//! headers are written in and a second spelling is a second thing to keep in step.
//!
//! ⚠️ **An undeclared axis REFUSES, and never guesses.** A default here is a claim
//! about the operator's own entry, made on their behalf, transmitted on every contact
//! of a 24-hour contest and then submitted — which is precisely the defect
//! [`OperatorCategory`](super::cabrillo::OperatorCategory) exists to have fixed (the
//! hardcoded `CATEGORY-OPERATOR: MULTI-OP`). The refusal is a sentence the operator
//! reads at session start, when they can still answer it.

use super::cabrillo::OperatorCategory;

/// Cabrillo `CATEGORY-POWER`, as the three tokens SS's power sub-categories use.
///
/// ⚠️ **Not Field Day's power TIER** (`Settings::fd_power_mult`), which is a scoring
/// multiplier picked from a legal set — a different axis with different thresholds, and
/// §6.1 rules that neither is derived from the other.
pub const POWER_HIGH: &str = "HIGH";
/// ≤ 100 W PEP — SS's Low Power sub-category.
pub const POWER_LOW: &str = "LOW";
/// ≤ 5 W PEP — SS's QRP sub-category.
pub const POWER_QRP: &str = "QRP";

/// Cabrillo `CATEGORY-ASSISTED`: the entrant used spotting assistance, which is what
/// ARRL calls **Single Operator Unlimited**.
pub const ASSISTED: &str = "ASSISTED";
/// …and its opposite, which is the plain Single Operator category.
pub const NON_ASSISTED: &str = "NON-ASSISTED";

/// Cabrillo `CATEGORY-STATION: SCHOOL` — ARRL's School Club category, whose precedence
/// is `S` whatever else is declared. (ARRL's own worked SS header on
/// <https://www.arrl.org/cabrillo-format-tutorial> carries this line, read 2026-09-09.)
pub const STATION_SCHOOL: &str = "SCHOOL";

/// The Sweepstakes precedence letter for an entry, or the sentence saying which
/// declaration is missing.
///
/// `power`, `assisted` and `station` are the operator's raw settings values, trimmed
/// and case-folded here rather than by each caller — a settings file is hand-editable
/// and `" low "` is the same declaration as `LOW`.
///
/// ⭐ **The order of the arms is the sponsor's own precedence between categories**, not
/// a convenience: School Club has no power sub-categories (*"School Club (S) — No power
/// sub-categories"*), and Multioperator has no assisted sub-category, so each is decided
/// before the axes it does not read. Reversing two of these arms is how a school club's
/// high-power multi-op entry would send `B`.
pub fn precedence(
    operator: OperatorCategory,
    power: &str,
    assisted: &str,
    station: &str,
) -> Result<&'static str, String> {
    let station = station.trim().to_ascii_uppercase();
    if station == STATION_SCHOOL {
        return Ok("S");
    }
    // ⚠️ A CHECKLOG is an entry TYPE, not an operating category: the station still
    // operated as one or more operators and still sent a real precedence on the air.
    // Cabrillo puts all three in one header, so a checklog entrant has said nothing
    // about the operator axis — and inventing one for them is the guess this module
    // refuses to make.
    if operator == OperatorCategory::Checklog {
        return Err(CHECKLOG_HAS_NO_PRECEDENCE.to_string());
    }
    if operator == OperatorCategory::MultiOp {
        return Ok("M");
    }
    let assisted = assisted.trim().to_ascii_uppercase();
    match assisted.as_str() {
        ASSISTED => return Ok("U"),
        NON_ASSISTED => {}
        _ => return Err(NO_ASSISTED.to_string()),
    }
    match power.trim().to_ascii_uppercase().as_str() {
        POWER_QRP => Ok("Q"),
        POWER_LOW => Ok("A"),
        POWER_HIGH => Ok("B"),
        _ => Err(NO_POWER.to_string()),
    }
}

/// Said to the operator when no power sub-category has been declared. It names the
/// sponsor's own thresholds, because "pick a power category" without them is a question
/// the operator has to leave the app to answer.
pub const NO_POWER: &str = "Sweepstakes sends your entry category as the PRECEDENCE, and \
you have not picked a power category. Choose High (up to 1500 W), Low (up to 100 W) or \
QRP (up to 5 W) on the Contesting tab in Settings.";

/// …and when the assisted axis is undeclared. Single Operator and Single Operator
/// Unlimited are different categories with different letters, and spotting assistance
/// is the only thing that separates them.
pub const NO_ASSISTED: &str = "Sweepstakes sends your entry category as the PRECEDENCE, \
and you have not said whether you are using spotting assistance. Single Operator sends \
Q, A or B; Single Operator Unlimited — any station using spots, skimmers or a cluster — \
sends U. Choose one on the Contesting tab in Settings.";

/// …and for a checklog, which declares nothing about how the station was operated.
pub const CHECKLOG_HAS_NO_PRECEDENCE: &str = "A checklog says nothing about how the \
station was operated, and Sweepstakes sends that category as the PRECEDENCE. Set your \
entry category to Single-op or Multi-op on the Contesting tab in Settings — a checklog \
is how the log is SUBMITTED, not how it was worked.";

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ **SS-Rules v2.1 §4.2, letter for letter**, with the power thresholds from the
    /// same document's Entry Categories table. Read 2026-09-09; a sponsor change is a
    /// red test with the quoted rule attached, not a silent wrong letter on the air.
    #[test]
    fn every_precedence_letter_is_the_sponsors_own_category() {
        for (operator, power, assisted, want) in [
            // "“Q” for Single Operator, QRP"
            (OperatorCategory::SingleOp, POWER_QRP, NON_ASSISTED, "Q"),
            // "“A” for Single Operator, Low Power"
            (OperatorCategory::SingleOp, POWER_LOW, NON_ASSISTED, "A"),
            // "“B” for Single Operator, High Power"
            (OperatorCategory::SingleOp, POWER_HIGH, NON_ASSISTED, "B"),
            // "“U” for Single Operator Unlimited, High Power, Single Operator
            // Unlimited, Low Power and Single Operator Unlimited, QRP" — every power
            // sub-category, one letter.
            (OperatorCategory::SingleOp, POWER_HIGH, ASSISTED, "U"),
            (OperatorCategory::SingleOp, POWER_LOW, ASSISTED, "U"),
            (OperatorCategory::SingleOp, POWER_QRP, ASSISTED, "U"),
            // "“M” for Multioperator, High Power or Multioperator, Low Power"
            (OperatorCategory::MultiOp, POWER_HIGH, NON_ASSISTED, "M"),
            (OperatorCategory::MultiOp, POWER_LOW, ASSISTED, "M"),
        ] {
            assert_eq!(
                precedence(operator, power, assisted, ""),
                Ok(want),
                "{operator:?} / {power} / {assisted}"
            );
        }
        // "“S” for School Club" — and the sponsor's "School Club (S): No power
        // sub-categories" is why it is decided before the power axis is read.
        assert_eq!(
            precedence(OperatorCategory::SingleOp, "", "", STATION_SCHOOL),
            Ok("S")
        );
        assert_eq!(
            precedence(
                OperatorCategory::MultiOp,
                POWER_HIGH,
                ASSISTED,
                STATION_SCHOOL
            ),
            Ok("S")
        );
    }

    /// ⭐ **An undeclared axis refuses, naming what to do — it never defaults.** A
    /// default here is a claim about the operator's entry, sent on every contact of a
    /// 24-hour contest.
    #[test]
    fn an_undeclared_axis_refuses_instead_of_guessing() {
        // POSITIVE CONTROL: the fully declared entry above resolves, so each refusal
        // below is the missing axis and not a function that always refuses.
        assert!(precedence(OperatorCategory::SingleOp, POWER_LOW, NON_ASSISTED, "").is_ok());

        assert_eq!(
            precedence(OperatorCategory::SingleOp, "", NON_ASSISTED, ""),
            Err(NO_POWER.to_string())
        );
        assert_eq!(
            precedence(OperatorCategory::SingleOp, POWER_LOW, "", ""),
            Err(NO_ASSISTED.to_string())
        );
        assert_eq!(
            precedence(OperatorCategory::Checklog, POWER_LOW, NON_ASSISTED, ""),
            Err(CHECKLOG_HAS_NO_PRECEDENCE.to_string())
        );
        // A value this build does not understand is NOT read as the nearest one it
        // does: `LOW POWER` is not `LOW`, and guessing is what a hand-edited settings
        // file would exploit.
        assert_eq!(
            precedence(OperatorCategory::SingleOp, "LOW POWER", NON_ASSISTED, ""),
            Err(NO_POWER.to_string())
        );
        // Multi-op reads neither axis, so neither can block it.
        assert_eq!(precedence(OperatorCategory::MultiOp, "", "", ""), Ok("M"));
    }

    /// A hand-edited settings file is trimmed and case-folded, exactly as every other
    /// operator-typed value in this crate is.
    #[test]
    fn a_declaration_is_trimmed_and_case_folded() {
        assert_eq!(
            precedence(OperatorCategory::SingleOp, " qrp ", " non-assisted ", " "),
            Ok("Q")
        );
        assert_eq!(
            precedence(OperatorCategory::SingleOp, "", "", " school "),
            Ok("S")
        );
    }
}
