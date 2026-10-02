//! NOAA SWPC daily solar indices — thirty days of solar flux and sunspot number.
//!
//! The Space Wx box shows the solar flux (SFI) from the live F10.7 feed, but one number cannot
//! say whether the Sun is waking up or going quiet, and it carries no sunspot number at all.
//! SWPC's "Daily Solar Data" text product answers both: one row per day for the last thirty
//! days, each with the 10.7 cm radio flux and the SESC sunspot number.
//!
//! ⚠️ THIS SSN IS NOT THE MODEL'S SSN. [`crate::model::SpaceWx::ssn`] is the 12-month SMOOTHED
//! sunspot number (R12) the P.533 engine wants, from the predicted-solar-cycle feed; these are
//! daily counts, which swing by a factor of ten in a fortnight. Nothing here reaches the
//! propagation model: the daily values are for the operator to read.
//!
//! The wire, `services.swpc.noaa.gov/text/daily-solar-indices.txt`: `:`-prefixed product lines
//! and `#`-prefixed comment lines, then one whitespace-separated row per day, oldest first:
//!
//! ```text
//! 2026 09 28   95     46      150      0    -999      *   2  0  0  0  0  0  0
//! ```
//!
//! year, month, day, the 10.7 cm flux, the SESC sunspot number, then columns this parser does
//! not read. SWPC writes a value it does not have as `-999` or `*`; either is `None` here, never
//! zero. A sunspot number of 0 IS a reading — the Sun was spotless on 18 Sep 2026, and the
//! fixture carries that day — so only the flux, which is never physically zero, treats 0 as
//! missing.
//!
//! Pure parsing only (no `live` feature needed); the HTTP fetch lives in [`crate::live::swpc`].

use serde::Serialize;

/// One day of the file. `None` = SWPC published no value for that day.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailySolarIndex {
    /// 00:00 UTC of the day, unix seconds.
    pub day_unix: i64,
    /// Observed 10.7 cm solar radio flux (SFI), solar flux units.
    pub sfi: Option<f32>,
    /// SESC daily sunspot number.
    pub ssn: Option<f32>,
}

/// Every dated row of the file, oldest first, one per day. `Default` (no days) is what the
/// command serves before the file has ever arrived, which the box reports as unavailable.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailySolarIndices {
    pub days: Vec<DailySolarIndex>,
}

/// Parse `daily-solar-indices.txt`. `None` when the text holds no dated row at all.
///
/// ⚠️ `None`, NOT an empty result, and the difference is the stale-honesty contract: the file
/// always carries thirty days, so a body with none is not this file (an error page served with
/// a 200, a changed format). The fetcher turns `None` into an error, and the command then keeps
/// serving the last good copy, whose own dates say how old it is — instead of replacing thirty
/// real days with nothing.
pub fn parse_daily_solar_indices(text: &str) -> Option<DailySolarIndices> {
    // Keyed by day: the output is oldest first and a repeated day keeps its LATER row.
    let mut days = std::collections::BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') || line.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = line.split_whitespace().collect();
        if cells.len() < 5 {
            continue;
        }
        let Some(day_unix) = day_of(cells[0], cells[1], cells[2]) else {
            continue; // not a data row
        };
        days.insert(
            day_unix,
            DailySolarIndex {
                day_unix,
                sfi: reading(cells[3]).filter(|v| *v > 0.0),
                ssn: reading(cells[4]),
            },
        );
    }
    (!days.is_empty()).then(|| DailySolarIndices {
        days: days.into_values().collect(),
    })
}

/// A cell as a value, or `None` for SWPC's markers: `*`, and the negative fillers (`-999`,
/// `-1`) that no index can take.
fn reading(cell: &str) -> Option<f32> {
    cell.parse::<f32>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

/// 00:00 UTC of a `YYYY MM DD` date, unix seconds. `None` for anything that is not a real
/// calendar date: the round trip through the calendar is what refuses 30 February, which
/// the day arithmetic alone would quietly turn into 2 March.
fn day_of(y: &str, m: &str, d: &str) -> Option<i64> {
    let (y, m, d): (i64, u32, u32) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    if !(1900..=2100).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let days = crate::geo::days_from_civil(y, m, d);
    (crate::geo::civil_from_days(days) == (y, m, d)).then_some(days * 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real file as SWPC served it on 29 Sep 2026 (public domain, byte for byte).
    const REAL: &str = include_str!("../tests/fixtures/swpc-daily-solar-indices-2026-09-29.txt");

    fn day(y: i64, m: u32, d: u32) -> i64 {
        crate::geo::days_from_civil(y, m, d) * 86_400
    }

    #[test]
    fn parses_the_real_file() {
        let f = parse_daily_solar_indices(REAL).expect("the real file has dated rows");
        assert_eq!(f.days.len(), 30, "thirty days, one row each");
        let first = &f.days[0];
        assert_eq!(first.day_unix, day(2026, 8, 30));
        assert_eq!(first.sfi, Some(107.0));
        assert_eq!(first.ssn, Some(74.0));
        let last = f.days.last().unwrap();
        assert_eq!(last.day_unix, day(2026, 9, 28));
        assert_eq!(last.sfi, Some(95.0), "the flux column was misread");
        assert_eq!(last.ssn, Some(46.0), "the sunspot column was misread");
    }

    /// 18 Sep 2026 had no sunspots. That is a reading, and the trend line must dip to it.
    #[test]
    fn a_spotless_day_is_zero_not_missing() {
        let f = parse_daily_solar_indices(REAL).unwrap();
        let spotless = f
            .days
            .iter()
            .find(|x| x.day_unix == day(2026, 9, 18))
            .unwrap();
        assert_eq!(spotless.ssn, Some(0.0), "a spotless day read as missing");
        assert_eq!(spotless.sfi, Some(96.0));
    }

    /// SWPC's own "no value" spellings. A zero in their place would draw a dive in the trend
    /// line that the Sun never made.
    #[test]
    fn swpcs_missing_markers_are_none_never_zero() {
        let text = "\
2026 09 01  -999     55      130      0    -999      *   3  0  0  0  0  0  0
2026 09 02     *      *      170      1    -999      *   3  1  0  0  0  0  0
2026 09 03     0     -1      170      0    -999      *   7  0  0 10  0  0  0
";
        let f = parse_daily_solar_indices(text).unwrap();
        assert_eq!(f.days.len(), 3, "a row with missing values is still a day");
        assert_eq!((f.days[0].sfi, f.days[0].ssn), (None, Some(55.0)));
        assert_eq!((f.days[1].sfi, f.days[1].ssn), (None, None));
        // The flux is never physically zero; the sunspot number's -1 is a marker.
        assert_eq!((f.days[2].sfi, f.days[2].ssn), (None, None));
    }

    /// Anything that is not the file is `None`, so the command keeps the last good copy.
    #[test]
    fn a_body_that_is_not_the_file_is_none() {
        assert!(parse_daily_solar_indices("").is_none());
        assert!(
            parse_daily_solar_indices("<html><body>503 Service Unavailable</body></html>")
                .is_none()
        );
        assert!(
            parse_daily_solar_indices(":Product: Daily Solar Data  DSD.txt\n# no rows\n").is_none()
        );
    }

    /// Every later reader walks the days in order ("the newest value", "the trend") and counts
    /// them, so the output is oldest first with one row per day whatever order the file used,
    /// and a date that does not exist is not a day.
    #[test]
    fn days_come_out_oldest_first_once_and_real() {
        let text = "\
2026 09 03   108     37      170      0    -999      *   7  0  0 10  0  0  0
2026 02 30   100     10      100      0    -999      *   0  0  0  0  0  0  0
2026 09 01   101     55      130      0    -999      *   3  0  0  0  0  0  0
2026 09 03   109     38      170      0    -999      *   7  0  0 10  0  0  0
";
        let f = parse_daily_solar_indices(text).unwrap();
        let dates: Vec<i64> = f.days.iter().map(|x| x.day_unix).collect();
        assert_eq!(
            dates,
            [day(2026, 9, 1), day(2026, 9, 3)],
            "30 Feb became a day"
        );
        assert_eq!(
            f.days[1].sfi,
            Some(109.0),
            "a repeated day keeps the later row"
        );
    }
}
