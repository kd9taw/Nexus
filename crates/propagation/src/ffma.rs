//! The ARRL Fred Fish Memorial Award (FFMA): 6 m contacts confirmed with all 488 Maidenhead
//! grid squares in the 48 contiguous United States.
//!
//! THE GRID LIST IS ARRL'S, TRANSCRIBED — NEVER DERIVED. "The 488 Required Grid Squares" on
//! <https://www.arrl.org/ffma> (read 2026-10-24) prints the list field by field as ranges, and
//! [`RANGES`] is that text range for range, in the page's order. A square is not in the award
//! because it touches the 48 states on a map: the ARRL decides, and a coastline-derived list
//! would disagree with theirs at the edges. The tests hold the page's own per-field counts and
//! its total of 488.
//!
//! The rules on that page, and where each lives:
//! * 6 m (50 MHz) only, any mode — the award fold keys on the 6 m band label.
//! * "No contacts through active repeaters or satellites are permitted." A satellite contact
//!   (`PROP_MODE=SAT`) never reaches the per-band grid sets the award reads, exactly as for
//!   VUCC. The fold does not see repeater contacts as such, so they are not excluded here.
//! * EME is not mentioned, so a 6 m moonbounce contact counts like any other.
//! * Confirmations go to an ARRL VHF awards manager; Nexus counts what VUCC counts (LoTW or a
//!   paper card, never eQSL).
//! * Contacts with aeronautical mobiles, contacts before 1983-01-01 and the 200 km operating
//!   circle are rules the log cannot judge; they are the operator's to apply.

use std::collections::HashSet;
use std::sync::LazyLock;

/// The total the ARRL names for the award.
pub const FFMA_GRID_COUNT: usize = 488;

/// "The 488 Required Grid Squares", as printed: `(field, first, last)` names the run of squares
/// `field` + `first`..=`last` (two digits each). One row per printed line, in the page's order.
const RANGES: &[(&str, u8, u8)] = &[
    // CM FIELD: 12 grid squares
    ("CM", 79, 79),
    ("CM", 86, 89),
    ("CM", 93, 99),
    // CN FIELD: 27 grid squares
    ("CN", 70, 78),
    ("CN", 80, 88),
    ("CN", 90, 98),
    // DL FIELD 5 grid squares
    ("DL", 79, 79),
    ("DL", 88, 89),
    ("DL", 98, 99),
    // DM FIELD: 90 grid squares
    ("DM", 2, 9),
    ("DM", 12, 19),
    ("DM", 22, 29),
    ("DM", 31, 39),
    ("DM", 41, 49),
    ("DM", 51, 59),
    ("DM", 61, 99),
    // DN FIELD: 90 grid squares
    ("DN", 0, 8),
    ("DN", 10, 18),
    ("DN", 20, 28),
    ("DN", 30, 38),
    ("DN", 40, 48),
    ("DN", 50, 58),
    ("DN", 60, 68),
    ("DN", 70, 78),
    ("DN", 80, 88),
    ("DN", 90, 98),
    // EL FIELD: 27 grid squares
    ("EL", 6, 9),
    ("EL", 15, 19),
    ("EL", 28, 29),
    ("EL", 39, 39),
    ("EL", 49, 49),
    ("EL", 58, 59),
    ("EL", 79, 79),
    ("EL", 84, 84),
    ("EL", 86, 89),
    ("EL", 94, 99),
    // EM FIELD: 100 grid squares
    ("EM", 0, 99),
    // EN FIELD: 80 grid squares
    ("EN", 0, 8),
    ("EN", 10, 18),
    ("EN", 20, 29),
    ("EN", 30, 38),
    ("EN", 40, 48),
    ("EN", 50, 58),
    ("EN", 60, 67),
    ("EN", 70, 76),
    ("EN", 80, 86),
    ("EN", 90, 92),
    // FM FIELD: 20 grid squares
    ("FM", 2, 9),
    ("FM", 13, 19),
    ("FM", 25, 29),
    // FN FIELD: 37 grid squares
    ("FN", 0, 3),
    ("FN", 10, 14),
    ("FN", 20, 25),
    ("FN", 30, 35),
    ("FN", 41, 46),
    ("FN", 51, 51),
    ("FN", 53, 57),
    ("FN", 64, 67),
];

static GRIDS: LazyLock<Vec<String>> = LazyLock::new(|| {
    RANGES
        .iter()
        .flat_map(|&(field, first, last)| (first..=last).map(move |n| format!("{field}{n:02}")))
        .collect()
});

static GRID_SET: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| GRIDS.iter().map(String::as_str).collect());

/// The 488 FFMA grid squares, upper case, in the ARRL list's order.
pub fn ffma_grids() -> &'static [String] {
    &GRIDS
}

/// Is `grid4` (a 4-character square, upper case) one of the 488?
pub fn is_ffma_grid(grid4: &str) -> bool {
    GRID_SET.contains(grid4)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list is exactly the 488 the award names, with no square twice.
    #[test]
    fn the_list_is_exactly_488_distinct_squares() {
        assert_eq!(ffma_grids().len(), FFMA_GRID_COUNT);
        assert_eq!(GRID_SET.len(), FFMA_GRID_COUNT, "a square appears twice");
    }

    /// Each field holds the count the ARRL page prints in its heading — the check that a
    /// range was transcribed as printed.
    #[test]
    fn each_field_holds_the_count_the_page_prints() {
        let printed = [
            ("CM", 12),
            ("CN", 27),
            ("DL", 5),
            ("DM", 90),
            ("DN", 90),
            ("EL", 27),
            ("EM", 100),
            ("EN", 80),
            ("FM", 20),
            ("FN", 37),
        ];
        for (field, count) in printed {
            let got = ffma_grids().iter().filter(|g| g.starts_with(field)).count();
            assert_eq!(got, count, "{field} field");
        }
        assert_eq!(printed.iter().map(|p| p.1).sum::<usize>(), FFMA_GRID_COUNT);
    }

    /// Every entry is a well-formed square, and squares outside the 48 states are not in it:
    /// Hawaii, Alaska, Puerto Rico, and the open Atlantic just past the list's last EN row.
    #[test]
    fn every_square_is_well_formed_and_the_outside_is_out() {
        for g in ffma_grids() {
            let b = g.as_bytes();
            assert!(
                b.len() == 4
                    && (b'A'..=b'R').contains(&b[0])
                    && (b'A'..=b'R').contains(&b[1])
                    && b[2].is_ascii_digit()
                    && b[3].is_ascii_digit(),
                "{g}"
            );
        }
        for inside in ["CM79", "DM61", "EM00", "EN92", "FN31", "FN67", "EL58"] {
            assert!(is_ffma_grid(inside), "{inside} is on the ARRL list");
        }
        for outside in ["BL11", "BP51", "FK68", "EN93", "FN52", "DM01", "CN99"] {
            assert!(!is_ffma_grid(outside), "{outside} is not on the ARRL list");
        }
    }
}
