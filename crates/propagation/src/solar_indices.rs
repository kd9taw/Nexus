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
//! TODAY'S COUNT SO FAR. The text file carries finished UTC days only, so its newest sunspot
//! number is yesterday's. SWPC's observed-SSN JSON (`json/solar-cycle/swpc_observed_ssn.json`)
//! carries the same daily counts plus a row dated today, which SWPC revises as the day goes on:
//!
//! ```text
//! [{"Obsdate":"1996-03-12T00:00:00","swpc_ssn":0}, ... ,{"Obsdate":"2026-11-16T00:00:00","swpc_ssn":95}]
//! ```
//!
//! oldest first, one record per UTC day back to 1996. [`ssn_today`] takes only the row dated the
//! current UTC day, never "the newest row": just after 00:00 UTC the newest is yesterday's, and
//! yesterday is the text file's job. The value is a running count, so it travels with the time
//! Nexus fetched it ([`SsnToday::fetched_at`]); like the daily counts, it never reaches the model.
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
    /// Today's sunspot number so far, from the observed-SSN JSON; `None` when that file has no
    /// row for the current UTC day or has not arrived. Never folded into `days`, which stay the
    /// finished days.
    pub today: Option<SsnToday>,
}

/// The current UTC day's running sunspot count, and when Nexus read it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SsnToday {
    /// 00:00 UTC of the day the count is for, unix seconds.
    pub day_unix: i64,
    /// SWPC's count so far today. SWPC revises it during the day.
    pub ssn: f32,
    /// When Nexus fetched the file this count came from, unix seconds.
    pub fetched_at: i64,
}

/// One record of the observed-SSN JSON.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObservedSsn {
    /// 00:00 UTC of the record's `Obsdate`, unix seconds.
    pub day_unix: i64,
    pub ssn: f32,
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
        today: None,
    })
}

/// Parse `swpc_observed_ssn.json`. `None` when the body holds no dated record at all (an error
/// page, a changed format), the same stale-honesty contract as [`parse_daily_solar_indices`]:
/// the fetcher turns it into an error and the command keeps what it held. A record whose date or
/// count does not read is skipped, never zero.
pub fn parse_observed_ssn(text: &str) -> Option<Vec<ObservedSsn>> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let records: Vec<ObservedSsn> = v
        .as_array()?
        .iter()
        .filter_map(|r| {
            // "YYYY-MM-DDT00:00:00": the date is all that is read.
            let date = r.get("Obsdate")?.as_str()?.get(..10)?;
            let mut ymd = date.split('-');
            let day_unix = day_of(ymd.next()?, ymd.next()?, ymd.next()?)?;
            let ssn = r.get("swpc_ssn")?.as_f64()?;
            (ssn.is_finite() && ssn >= 0.0).then_some(ObservedSsn {
                day_unix,
                ssn: ssn as f32,
            })
        })
        .collect();
    (!records.is_empty()).then_some(records)
}

/// The record dated the current UTC day of `now_unix`, stamped as fetched at `now_unix`. `None`
/// when there is no such record: SWPC has not started today's row, or the file is old.
pub fn ssn_today(records: &[ObservedSsn], now_unix: i64) -> Option<SsnToday> {
    let today = now_unix.div_euclid(86_400) * 86_400;
    // The last record for the day: a repeated day keeps its later row, as the text file's does.
    records
        .iter()
        .rev()
        .find(|r| r.day_unix == today)
        .map(|r| SsnToday {
            day_unix: today,
            ssn: r.ssn,
            fetched_at: now_unix,
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

    /// `swpc_observed_ssn.json` as SWPC served it on 16 Nov 2026 at 21:04 UTC (public domain),
    /// trimmed to its first two and last 45 records; each record is byte for byte. The whole
    /// file is 9,911 records back to 1996.
    const OBSERVED: &str =
        include_str!("../tests/fixtures/swpc-observed-ssn-2026-11-16-trimmed.json");

    #[test]
    fn parses_the_real_observed_ssn_file() {
        let r = parse_observed_ssn(OBSERVED).expect("the real file has dated records");
        assert_eq!(r.len(), 47);
        assert_eq!(
            r[0],
            ObservedSsn {
                day_unix: day(1996, 3, 12),
                ssn: 0.0
            },
            "a spotless day is a zero reading, not a skipped record"
        );
        let last2: Vec<_> = r[r.len() - 2..].to_vec();
        assert_eq!(
            last2,
            [
                ObservedSsn {
                    day_unix: day(2026, 11, 15),
                    ssn: 83.0
                },
                ObservedSsn {
                    day_unix: day(2026, 11, 16),
                    ssn: 95.0
                },
            ]
        );
    }

    #[test]
    fn a_body_that_is_not_the_observed_file_is_none() {
        assert!(parse_observed_ssn("").is_none());
        assert!(parse_observed_ssn("<html>503</html>").is_none());
        assert!(parse_observed_ssn("[]").is_none());
        assert!(parse_observed_ssn(r#"{"Obsdate":"2026-11-16T00:00:00","swpc_ssn":95}"#).is_none());
    }

    #[test]
    fn a_record_that_does_not_read_is_skipped_never_zero() {
        let text = r#"[{"Obsdate":"2026-11-14T00:00:00","swpc_ssn":null},
            {"Obsdate":"2026-02-30T00:00:00","swpc_ssn":10},
            {"Obsdate":"2026-11-15T00:00:00","swpc_ssn":-1},
            {"Obsdate":"garbage","swpc_ssn":5},
            {"Obsdate":"2026-11-16T00:00:00","swpc_ssn":95}]"#;
        let r = parse_observed_ssn(text).unwrap();
        assert_eq!(
            r,
            [ObservedSsn {
                day_unix: day(2026, 11, 16),
                ssn: 95.0
            }]
        );
    }

    /// Today is the current UTC date's row, read at that moment: late on the 16th it is the
    /// 16th's partial count; one second before midnight it is still the 16th's.
    #[test]
    fn today_is_the_current_utc_dates_row() {
        let r = parse_observed_ssn(OBSERVED).unwrap();
        let at = day(2026, 11, 16) + 21 * 3600 + 4 * 60;
        assert_eq!(
            ssn_today(&r, at),
            Some(SsnToday {
                day_unix: day(2026, 11, 16),
                ssn: 95.0,
                fetched_at: at
            })
        );
        let last_second = day(2026, 11, 17) - 1;
        assert_eq!(ssn_today(&r, last_second).map(|t| t.ssn), Some(95.0));
        // Earlier in the file's history, the 15th's row was "today" on the 15th.
        let on_15th = day(2026, 11, 15) + 12 * 3600;
        assert_eq!(ssn_today(&r, on_15th).map(|t| t.ssn), Some(83.0));
    }

    /// Just after 00:00 UTC SWPC has not started the new day's row: the newest record is
    /// yesterday's, and it is NOT today's count.
    #[test]
    fn just_after_midnight_with_no_new_row_there_is_no_today() {
        let r = parse_observed_ssn(OBSERVED).unwrap();
        assert_eq!(ssn_today(&r, day(2026, 11, 17)), None);
        assert_eq!(ssn_today(&r, day(2026, 11, 17) + 5), None);
        assert_eq!(ssn_today(&[], day(2026, 11, 16) + 60), None);
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
