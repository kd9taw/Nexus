//! NOAA's numbered active regions on the sun (`json/solar_regions.json`): where on the sun a flare
//! can come from, for the sun disc on the map.
//!
//! THE PRODUCT. A flat array of rows, one per region per day, about thirty days of them (about
//! 100 KB). Each row carries its `observed_date` (`YYYY-MM-DD`), its NOAA `region` number, and its
//! heliographic `latitude` and `longitude` in degrees as seen from Earth (positive north, positive
//! EAST of the central meridian; |longitude| > 90 is behind the limb), plus NOAA's `location`
//! string ("N05E68"), magnetic and spot classes, area, spot count and flare probabilities.
//!
//! THE HONESTY CONTRACT (the same rules as every other space-weather reading):
//! - Only the newest observed day is kept, and only the regions on the visible disc.
//! - A row with no date, an unreadable date, no region number, or a position that is not a
//!   finite number in range is dropped, never guessed.
//! - A row dated AHEAD of the station's clock (its day starts more than [`REGIONS_AHEAD_SECS`]
//!   after now) is not used: a list from tomorrow is a clock or feed fault.
//! - An answer that is not a JSON array, is empty, has no usable dated row, or is larger than
//!   [`REGIONS_MAX_BYTES`] / [`REGIONS_MAX_ROWS`] is not a list: the caller keeps what it held.
//! - A list is drawn only while [`regions_state`] says Held: from its observed day until
//!   [`REGIONS_STALE_SECS`] after that day began. Positions are NOT rotated forward to now (that
//!   would be a modelled picture); they are drawn as observed, with the day they are from.

use serde::Serialize;
use serde_json::Value;

use crate::geo::days_from_civil;

/// The most of an answer that is parsed (the live file was 100 KB on 30 Oct 2026).
pub const REGIONS_MAX_BYTES: usize = 1024 * 1024;
/// The most rows that are read (the live file had 174).
pub const REGIONS_MAX_ROWS: usize = 5000;
/// A day may start this far after the station's clock and still be today's (clock skew).
pub const REGIONS_AHEAD_SECS: i64 = 120;
/// How long after its observed day begins a list is still drawn. NOAA publishes the list once a
/// day, a little after the day it describes, so a list is normally 24–51 h past its day's start;
/// three days allows one missed update and no more.
pub const REGIONS_STALE_SECS: i64 = 3 * 86_400;

/// One numbered region on the visible disc, as NOAA observed it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SolarRegion {
    /// NOAA's region number.
    pub region: u32,
    /// Heliographic latitude, degrees (positive north).
    pub lat_deg: f64,
    /// Heliographic longitude from the central meridian, degrees (positive EAST, as NOAA gives it).
    pub lon_deg: f64,
    /// NOAA's own location string ("N05E68"), when it sent one.
    pub location: Option<String>,
    /// Magnetic class (A, B, BG, BGD …), when NOAA gives one.
    pub mag_class: Option<String>,
    /// McIntosh spot class, when NOAA gives one.
    pub spot_class: Option<String>,
    /// Area, millionths of the hemisphere.
    pub area: Option<u32>,
    pub number_spots: Option<u32>,
    /// NOAA's flare probabilities for the next day, percent.
    pub m_flare_probability: Option<u32>,
    pub x_flare_probability: Option<u32>,
}

/// The newest observed day's regions on the visible disc.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SolarRegions {
    /// The observed day, Unix seconds of its 00:00 UTC.
    pub observed_day: i64,
    /// The regions on the visible disc that day (may be empty: none numbered on the disc).
    pub regions: Vec<SolarRegion>,
}

/// Whether a held list may be drawn now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RegionsState {
    Held,
    /// Its day starts ahead of the station's clock.
    Ahead,
    /// Older than [`REGIONS_STALE_SECS`] from its day's start.
    Stale,
}

/// `body` read as NOAA's region list at `now`, or `Err` with why it is not one.
pub fn parse_solar_regions(body: &str, now: i64) -> Result<SolarRegions, String> {
    if body.len() > REGIONS_MAX_BYTES {
        return Err("the region list is larger than 1 MB".into());
    }
    let v: Value = serde_json::from_str(body).map_err(|_| "not a region list".to_string())?;
    let rows = v.as_array().ok_or("not a region list")?;
    if rows.is_empty() {
        return Err("the region list is empty".into());
    }
    if rows.len() > REGIONS_MAX_ROWS {
        return Err(format!(
            "the region list has more than {REGIONS_MAX_ROWS} rows"
        ));
    }
    // Every usable row, with its day; tomorrow's rows (or later) are a clock or feed fault.
    let usable: Vec<(i64, Option<SolarRegion>)> = rows
        .iter()
        .filter_map(|r| {
            let day = r
                .get("observed_date")
                .and_then(Value::as_str)
                .and_then(parse_day)?;
            if day > now.saturating_add(REGIONS_AHEAD_SECS) {
                return None;
            }
            let region = region_of(r)?;
            Some((day, (region.lon_deg.abs() <= 90.0).then_some(region)))
        })
        .collect();
    let observed_day = usable
        .iter()
        .map(|(d, _)| *d)
        .max()
        .ok_or("the region list has no usable dated row")?;
    let mut regions: Vec<SolarRegion> = usable
        .into_iter()
        .filter(|(d, _)| *d == observed_day)
        .filter_map(|(_, r)| r)
        .collect();
    regions.sort_by_key(|r| r.region);
    regions.dedup_by_key(|r| r.region);
    Ok(SolarRegions {
        observed_day,
        regions,
    })
}

/// `YYYY-MM-DD` → Unix seconds of that day's 00:00 UTC; anything else is no date.
fn parse_day(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let t = &s[r];
        t.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| t.parse().ok())?
    };
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let days = days_from_civil(y as i64, m, d);
    // A day that does not exist (31 Nov) rolls into the next month: refuse it.
    let (yy, mm, dd) = crate::geo::civil_from_days(days);
    ((yy, mm, dd) == (y as i64, m, d)).then_some(days * 86_400)
}

/// One row's region, or `None` when its number or position is not a real one.
fn region_of(r: &Value) -> Option<SolarRegion> {
    let region = u32::try_from(r.get("region")?.as_u64()?).ok()?;
    let lat = r
        .get("latitude")?
        .as_f64()
        .filter(|x| x.is_finite() && x.abs() <= 90.0)?;
    let lon = r
        .get("longitude")?
        .as_f64()
        .filter(|x| x.is_finite() && x.abs() <= 180.0)?;
    let text = |k: &str| {
        r.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() <= 16)
            .map(String::from)
    };
    let count = |k: &str| {
        r.get(k)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    };
    Some(SolarRegion {
        region,
        lat_deg: lat,
        lon_deg: lon,
        location: text("location"),
        mag_class: text("mag_class"),
        spot_class: text("spot_class"),
        area: count("area"),
        number_spots: count("number_spots"),
        m_flare_probability: count("m_flare_probability").filter(|p| *p <= 100),
        x_flare_probability: count("x_flare_probability").filter(|p| *p <= 100),
    })
}

/// Whether a list observed on `observed_day` may be drawn at `now`.
pub fn regions_state(observed_day: i64, now: i64) -> RegionsState {
    let age = now.saturating_sub(observed_day);
    if age < -REGIONS_AHEAD_SECS {
        RegionsState::Ahead
    } else if age >= REGIONS_STALE_SECS {
        RegionsState::Stale
    } else {
        RegionsState::Held
    }
}

/// Until when (Unix seconds) a list observed on `observed_day` is drawn.
pub fn regions_until(observed_day: i64) -> i64 {
    observed_day.saturating_add(REGIONS_STALE_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIVE: &str = include_str!("../tests/fixtures/swpc-solar-regions-2026-10-30.json");
    /// 2026-10-30 02:46 UTC, when the fixture was downloaded.
    const NOW: i64 = 1_793_328_360;
    const DAY: i64 = 86_400;

    fn day(y: i64, m: u32, d: u32) -> i64 {
        days_from_civil(y, m, d) * DAY
    }

    fn row(date: &str, region: u32, lat: f64, lon: f64) -> String {
        format!(
            r#"{{"observed_date":"{date}","region":{region},"latitude":{lat},"longitude":{lon},"location":"X","area":10,"spot_class":"Axx","mag_class":"A","number_spots":1,"m_flare_probability":1,"x_flare_probability":1}}"#
        )
    }

    #[test]
    fn the_live_list_gives_the_newest_days_regions_on_the_visible_disc() {
        let r = parse_solar_regions(LIVE, NOW).expect("the live file is a list");
        assert_eq!(r.observed_day, day(2026, 10, 29));
        let nums: Vec<u32> = r.regions.iter().map(|x| x.region).collect();
        assert_eq!(nums, vec![4562, 4563, 4564]);
        let w = &r.regions[1];
        assert_eq!(
            (w.lat_deg, w.lon_deg),
            (-18.0, -81.0),
            "S18W81: west is negative"
        );
        assert_eq!(w.location.as_deref(), Some("S18W81"));
        assert_eq!(w.mag_class.as_deref(), Some("A"));
        // A region with no spots that day (plage) is still listed, with nothing made up.
        let p = &r.regions[0];
        assert_eq!(
            (p.area, p.number_spots, p.mag_class.clone()),
            (None, None, None)
        );
    }

    #[test]
    fn a_region_behind_the_limb_is_not_on_the_disc() {
        let body = format!(
            "[{},{},{}]",
            row("2026-10-29", 1, 10.0, 90.0),
            row("2026-10-29", 2, 10.0, -91.0),
            row("2026-10-29", 3, 10.0, 105.0)
        );
        let r = parse_solar_regions(&body, NOW).unwrap();
        assert_eq!(
            r.regions.iter().map(|x| x.region).collect::<Vec<_>>(),
            vec![1]
        );
        // A day whose regions are all behind the limb is still that day's list: an empty disc.
        let body = format!("[{}]", row("2026-10-29", 2, 10.0, -91.0));
        let r = parse_solar_regions(&body, NOW).unwrap();
        assert_eq!((r.observed_day, r.regions.len()), (day(2026, 10, 29), 0));
    }

    #[test]
    fn bad_answers_are_not_a_list() {
        for bad in [
            "",
            "<html>Service Unavailable</html>",
            "{\"observed_date\":\"2026-10-29\"}",
            "[1, 2, 3]",
            "null",
            "[",
        ] {
            assert!(parse_solar_regions(bad, NOW).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn an_empty_list_is_not_a_list() {
        assert!(parse_solar_regions("[]", NOW).is_err());
        assert!(parse_solar_regions("  [ ]  ", NOW).is_err());
    }

    #[test]
    fn undated_and_unreadable_rows_are_dropped_never_guessed() {
        let body = format!(
            "[{},{},{},{},{},{},{}]",
            r#"{"region":7,"latitude":1,"longitude":1}"#,
            r#"{"observed_date":null,"region":8,"latitude":1,"longitude":1}"#,
            r#"{"observed_date":"29 Oct 2026","region":9,"latitude":1,"longitude":1}"#,
            r#"{"observed_date":"2026-13-40","region":10,"latitude":1,"longitude":1}"#,
            r#"{"observed_date":"2026-10-28","region":null,"latitude":1,"longitude":1}"#,
            r#"{"observed_date":"2026-10-28","region":12,"latitude":"N10","longitude":1}"#,
            row("2026-10-27", 13, 95.0, 0.0),
        );
        // No row is usable: not a list.
        assert!(parse_solar_regions(&body, NOW).is_err());
        // CONTROL: one good row beside them is the list, alone.
        let body = format!(
            "{},{}]",
            body.trim_end_matches(']'),
            row("2026-10-26", 14, 5.0, 5.0)
        );
        let r = parse_solar_regions(&body, NOW).unwrap();
        assert_eq!(r.observed_day, day(2026, 10, 26));
        assert_eq!(
            r.regions.iter().map(|x| x.region).collect::<Vec<_>>(),
            vec![14]
        );
    }

    #[test]
    fn a_day_ahead_of_the_clock_is_not_used() {
        let body = format!(
            "[{},{}]",
            row("2026-10-31", 20, 5.0, 5.0),
            row("2026-10-29", 21, 5.0, 5.0)
        );
        let r = parse_solar_regions(&body, NOW).unwrap();
        assert_eq!(
            r.observed_day,
            day(2026, 10, 29),
            "tomorrow's rows were used"
        );
        // Today's own date is not ahead; within the skew allowance of midnight it is not either.
        let today = format!("[{}]", row("2026-10-30", 22, 5.0, 5.0));
        assert_eq!(
            parse_solar_regions(&today, NOW).unwrap().observed_day,
            day(2026, 10, 30)
        );
        let at = day(2026, 10, 31) - REGIONS_AHEAD_SECS;
        let next = format!("[{}]", row("2026-10-31", 23, 5.0, 5.0));
        assert!(
            parse_solar_regions(&next, at).is_ok(),
            "within the skew allowance"
        );
        assert!(
            parse_solar_regions(&next, at - 1).is_err(),
            "one second beyond it"
        );
    }

    #[test]
    fn an_oversized_answer_is_refused() {
        let big = format!("[{}]", " ".repeat(REGIONS_MAX_BYTES));
        assert!(parse_solar_regions(&big, NOW).is_err());
        let rows: Vec<String> = (0..=REGIONS_MAX_ROWS as u32)
            .map(|i| row("2026-10-29", i, 1.0, 1.0))
            .collect();
        let many = format!("[{}]", rows.join(","));
        assert!(
            many.len() < REGIONS_MAX_BYTES,
            "CONTROL: this one is refused for its rows"
        );
        assert!(parse_solar_regions(&many, NOW).is_err());
        // CONTROL: at the row limit it is read.
        let rows: Vec<String> = (0..REGIONS_MAX_ROWS as u32)
            .map(|i| row("2026-10-29", i, 1.0, 1.0))
            .collect();
        assert!(parse_solar_regions(&format!("[{}]", rows.join(",")), NOW).is_ok());
    }

    #[test]
    fn non_finite_or_out_of_range_positions_are_dropped() {
        let body = format!(
            "[{},{},{}]",
            row("2026-10-29", 1, 91.0, 1.0),
            row("2026-10-29", 2, 1.0, -181.0),
            row("2026-10-29", 3, -90.0, 0.0)
        );
        let r = parse_solar_regions(&body, NOW).unwrap();
        assert_eq!(
            r.regions.iter().map(|x| x.region).collect::<Vec<_>>(),
            vec![3]
        );
        // 1e999 parses as infinity in some readers: not a position.
        let inf = r#"[{"observed_date":"2026-10-29","region":5,"latitude":1e999,"longitude":1}]"#;
        assert!(parse_solar_regions(inf, NOW)
            .map(|r| r.regions.is_empty())
            .unwrap_or(true));
    }

    #[test]
    fn a_list_is_held_from_its_day_until_three_days_after_it_began() {
        let d = day(2026, 10, 29);
        assert_eq!(regions_state(d, d), RegionsState::Held);
        assert_eq!(
            regions_state(d, d + REGIONS_STALE_SECS - 1),
            RegionsState::Held
        );
        assert_eq!(
            regions_state(d, d + 3 * DAY),
            RegionsState::Stale,
            "exactly three days"
        );
        assert_eq!(regions_state(d, d + 30 * DAY), RegionsState::Stale);
        assert_eq!(
            regions_state(d, d - 120),
            RegionsState::Held,
            "inside the skew allowance"
        );
        assert_eq!(regions_state(d, d - 121), RegionsState::Ahead);
        assert_eq!(regions_until(d), d + 3 * DAY);
        // No overflow at the ends of time.
        assert_eq!(regions_state(i64::MIN, i64::MAX), RegionsState::Stale);
        assert_eq!(regions_state(i64::MAX, i64::MIN), RegionsState::Ahead);
        assert_eq!(regions_until(i64::MAX), i64::MAX);
    }
}
