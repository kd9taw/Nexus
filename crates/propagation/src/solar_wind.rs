//! Real-time solar-wind adapter (pure parser half) — the LEADING geomagnetic indicator.
//!
//! Kp and the A-index (from `swpc`) lag real conditions by hours; the DSCOVR/ACE
//! solar-wind stream is the early warning. A southward interplanetary magnetic field
//! (Bz negative) couples energy into the magnetosphere → polar/high-latitude HF paths
//! degrade and aurora can light up VHF, typically 1–2 h before Kp catches up. A fast
//! wind stream (high speed) does the same on a slower fuse.
//!
//! NOAA SWPC serves these as the well-known "array of arrays" products: row 0 is the
//! column headers, every later row is all-STRING values, newest last:
//!   - `services.swpc.noaa.gov/products/solar-wind/mag-1-day.json`
//!     headers include `bz_gsm` (nT) and `bt` (total field, nT)
//!   - `services.swpc.noaa.gov/products/solar-wind/plasma-1-day.json`
//!     headers include `speed` (km/s) and `density` (p/cm³)
//!
//! Pure (`&Value` in, `Option<...>` out) so it is unit-testable offline; the networked
//! fetcher lives in `live::solar_wind`. Columns are located BY NAME (not fixed index) so
//! a reordered/extended product doesn't silently misread.
//!
//! ⚠️ A SAMPLE SAYS WHEN IT WAS MADE AND WHAT IT DOES NOT KNOW. The station keeps its last good
//! sample while the feed is unreachable, so every sample carries the time its magnetometer reading
//! was made (`time_unix`, the row's own time tag), and a reading with no time is no sample: its age
//! could be anything and a reader shown it could only take it as current. Past
//! [`SOLAR_WIND_STALE_SECS`] a sample is a record of the past — the insight feed stops speaking from
//! it and the Space Wx gauges say how old it is. Speed and density come from the separate plasma
//! product and are `None` when it did not answer, or when its newest reading is not from the same
//! moment as the magnetometer's — never 0, which is a solar wind that stopped blowing. Bt is `None`
//! the same way when the magnetometer row carries Bz without it.

use serde::Serialize;
use serde_json::Value;

/// How long a sample speaks for "now", in seconds. DSCOVR's products are one-minute data a few
/// minutes behind; older than this, a sample is history. The pairing of the plasma reading with the
/// magnetometer's uses it, the insight feed stops speaking from an older sample, and the UI's gauges
/// say the sample's age from the same moment (`propViz.ts` reads this number out of this file).
pub const SOLAR_WIND_STALE_SECS: i64 = 30 * 60;

/// Current solar-wind conditions (most recent valid sample in the product window).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SolarWind {
    /// Bz (GSM), nT. Negative = southward = the geoeffective case.
    pub bz_nt: f32,
    /// Total field magnitude Bt, nT. `None` = NOT KNOWN: the magnetometer row carried Bz without it.
    pub bt_nt: Option<f32>,
    /// Bulk speed, km/s. `None` = NOT KNOWN: the plasma product did not answer, or its newest
    /// reading is not from this sample's moment.
    pub speed_kms: Option<f32>,
    /// Proton density, p/cm³, from the same plasma reading as the speed. `None` = not known.
    pub density: Option<f32>,
    /// When the magnetometer reading was made (its row's `time_tag`), Unix seconds UTC.
    pub time_unix: i64,
}

impl SolarWind {
    /// How old the sample is at `now` (Unix seconds UTC), never negative.
    pub fn age_secs(&self, now: i64) -> i64 {
        (now - self.time_unix).max(0)
    }

    /// True once the sample is a record of the past rather than a reading of the field now.
    pub fn is_stale(&self, now: i64) -> bool {
        self.age_secs(now) >= SOLAR_WIND_STALE_SECS
    }
}

/// Index of a named column in a SWPC product's header row (row 0).
fn col(header: &Value, name: &str) -> Option<usize> {
    header
        .as_array()?
        .iter()
        .position(|c| c.as_str() == Some(name))
}

/// The newest data row (scanning from the end) whose `idx` column parses as a float and whose
/// `time_i` column parses as a time — so a trailing `null`/blank sample doesn't blank the readout,
/// and no reading is taken without knowing when it was made. Returns that row and its time.
fn newest_dated_row_with(rows: &[Value], idx: usize, time_i: usize) -> Option<(&[Value], i64)> {
    rows.iter().rev().find_map(|r| {
        let a = r.as_array()?;
        cell(a, idx)?; // require the anchor column to parse
        let t = crate::kc2g::parse_naive_utc_unix(a.get(time_i)?.as_str()?)?;
        Some((a.as_slice(), t))
    })
}

/// Parse cell `idx` of a data row as f32 (values arrive as strings; `null`/blank → None).
fn cell(row: &[Value], idx: usize) -> Option<f32> {
    row.get(idx)?.as_str()?.trim().parse::<f32>().ok()
}

/// Parse the `mag-1-day` product → (Bz, Bt, the reading's time) from the newest dated row
/// with a valid Bz. Bt is `None` when that row carries none.
pub fn parse_mag(v: &Value) -> Option<(f32, Option<f32>, i64)> {
    let arr = v.as_array()?;
    let header = arr.first()?;
    let bz_i = col(header, "bz_gsm")?;
    let bt_i = col(header, "bt");
    let time_i = col(header, "time_tag")?;
    let (row, time) = newest_dated_row_with(&arr[1..], bz_i, time_i)?;
    Some((cell(row, bz_i)?, bt_i.and_then(|i| cell(row, i)), time))
}

/// Parse the `plasma-1-day` product → (speed, density, the reading's time) from the newest dated
/// row with a speed. The density is `None` when that row carries none.
pub fn parse_plasma(v: &Value) -> Option<(f32, Option<f32>, i64)> {
    let arr = v.as_array()?;
    let header = arr.first()?;
    let speed_i = col(header, "speed")?;
    let dens_i = col(header, "density");
    let time_i = col(header, "time_tag")?;
    let (row, time) = newest_dated_row_with(&arr[1..], speed_i, time_i)?;
    Some((cell(row, speed_i)?, dens_i.and_then(|i| cell(row, i)), time))
}

/// Assemble a [`SolarWind`] from the two products. The magnetometer's dated Bz is required (the
/// leading signal). The plasma reading is best-effort and counts only when it is from the same
/// moment, within [`SOLAR_WIND_STALE_SECS`] of the magnetometer's: a plasma product that has
/// stalled for hours leaves speed and density not known instead of lending this sample an old speed.
pub fn assemble(mag: &Value, plasma: &Value) -> Option<SolarWind> {
    let (bz_nt, bt_nt, time_unix) = parse_mag(mag)?;
    let plasma = parse_plasma(plasma)
        .filter(|&(_, _, time)| (time - time_unix).abs() <= SOLAR_WIND_STALE_SECS);
    Some(SolarWind {
        bz_nt,
        bt_nt,
        speed_kms: plasma.map(|(speed, _, _)| speed),
        density: plasma.and_then(|(_, density, _)| density),
        time_unix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_bz_and_bt_from_newest_valid_row() {
        let v = json!([
            ["time_tag", "bx_gsm", "by_gsm", "bz_gsm", "lon_gsm", "lat_gsm", "bt"],
            [
                "2024-01-01 00:00:00.000",
                "1.0",
                "2.0",
                "-3.5",
                "180",
                "10",
                "5.1"
            ],
            [
                "2024-01-01 00:01:00.000",
                "1.1",
                "2.1",
                "-8.2",
                "181",
                "11",
                "9.3"
            ],
            [
                "2024-01-01 00:02:00.000",
                null,
                null,
                null,
                null,
                null,
                null
            ]
        ]);
        let (bz, bt, time) = parse_mag(&v).unwrap();
        assert!((bz - -8.2).abs() < 1e-3); // skipped the trailing null row
        assert!((bt.unwrap() - 9.3).abs() < 1e-3);
        assert_eq!(time, 1_704_067_260, "the 00:01 row's own time");
    }

    #[test]
    fn parses_speed_and_density_by_column_name() {
        let v = json!([
            ["time_tag", "density", "speed", "temperature"],
            ["2024-01-01 00:00:00.000", "5.2", "420", "100000"]
        ]);
        let (speed, density, _) = parse_plasma(&v).unwrap();
        assert!((speed - 420.0).abs() < 1e-3);
        assert!((density.unwrap() - 5.2).abs() < 1e-3);
    }

    #[test]
    fn assemble_survives_missing_plasma() {
        let mag = json!([
            ["time_tag", "bz_gsm", "bt"],
            ["2024-01-01 00:00:00.000", "-6.0", "7.0"]
        ]);
        let plasma = json!(null);
        let sw = assemble(&mag, &plasma).unwrap();
        assert!((sw.bz_nt - -6.0).abs() < 1e-3);
        // Not known — this assertion pinned the 0 until the plasma feed's absence reached the wire.
        assert_eq!(sw.speed_kms, None);
        assert_eq!(sw.density, None);
    }

    #[test]
    fn empty_or_headerless_is_none() {
        assert!(parse_mag(&json!([])).is_none());
        assert!(parse_mag(&json!(null)).is_none());
        assert!(parse_plasma(&json!([["time_tag", "density"]])).is_none()); // no speed col
    }

    /// The `mag-1-day` shape with one reading, `bz` at `tag`.
    fn mag_at(tag: &str, bz: &str) -> Value {
        json!([
            ["time_tag", "bx_gsm", "by_gsm", "bz_gsm", "lon_gsm", "lat_gsm", "bt"],
            [tag, "1.0", "2.0", bz, "180", "10", "9.3"]
        ])
    }

    /// The `plasma-1-day` shape with one reading, `speed` at `tag`.
    fn plasma_at(tag: &str, speed: &str) -> Value {
        json!([
            ["time_tag", "density", "speed", "temperature"],
            [tag, "5.2", speed, "100000"]
        ])
    }

    /// 12:00:00 UTC on 29 Sep 2026, the magnetometer reading's time in these fixtures.
    const NOON: i64 = 1_790_683_200;

    /// ⚠️ THE WIRE, NOT THE STRUCT: a page reads the serialized sample. "The plasma product did not
    /// answer" must reach it as NOT KNOWN — a 0 there is a solar wind that stopped blowing, which the
    /// Sun's never does, and it put "wind 0 km/s" into the insight feed.
    #[test]
    fn a_missing_plasma_feed_reaches_the_wire_as_not_known_never_zero() {
        let sw = assemble(&mag_at("2026-09-29 12:00:00.000", "-6.0"), &Value::Null).unwrap();
        let wire = serde_json::to_value(sw).unwrap();
        assert_eq!(
            wire["speedKms"],
            Value::Null,
            "a missing speed went out as {}",
            wire["speedKms"]
        );
        assert_eq!(
            wire["density"],
            Value::Null,
            "a missing density went out as {}",
            wire["density"]
        );
        assert_eq!(
            wire["bzNt"],
            json!(-6.0),
            "control: the magnetometer's Bz is on the wire"
        );
    }

    /// The sample says WHEN it was measured — the magnetometer row's own time tag, not when Nexus
    /// fetched it — so every reader can tell a reading from a record of the past.
    #[test]
    fn the_sample_carries_the_time_its_magnetometer_reading_was_made() {
        let sw = assemble(
            &mag_at("2026-09-29 12:00:00.000", "-6.0"),
            &plasma_at("2026-09-29 11:59:00.000", "420"),
        )
        .unwrap();
        let wire = serde_json::to_value(sw).unwrap();
        assert_eq!(
            wire["timeUnix"],
            json!(NOON),
            "the sample carries no time of its own"
        );
        assert_eq!(
            wire["speedKms"],
            json!(420.0),
            "control: a same-minute plasma reading is paired"
        );
    }

    /// A plasma product whose newest valid row is from hours before the magnetometer's is down in all
    /// but name: its speed is not THIS sample's speed.
    #[test]
    fn a_plasma_reading_from_another_hour_is_not_this_samples_speed() {
        let sw = assemble(
            &mag_at("2026-09-29 12:00:00.000", "-6.0"),
            &plasma_at("2026-09-29 10:00:00.000", "420"),
        )
        .unwrap();
        let wire = serde_json::to_value(sw).unwrap();
        assert_eq!(
            wire["speedKms"],
            Value::Null,
            "a two-hour-old speed was paired with a fresh Bz"
        );
    }

    /// Bt the same way as the speed: a magnetometer row that carries Bz but no total field (a null
    /// cell, or a product without the column) sends Bt as not known — a 0 there is a field that
    /// vanished, and it read as "Bt 0.0 nT" in the insight feed.
    #[test]
    fn a_missing_total_field_reaches_the_wire_as_not_known_never_zero() {
        let null_cell = json!([
            ["time_tag", "bz_gsm", "bt"],
            ["2026-09-29 12:00:00.000", "-6.0", null]
        ]);
        let wire = serde_json::to_value(assemble(&null_cell, &Value::Null).unwrap()).unwrap();
        assert_eq!(
            wire["btNt"],
            Value::Null,
            "a missing Bt went out as {}",
            wire["btNt"]
        );
        let no_column = json!([["time_tag", "bz_gsm"], ["2026-09-29 12:00:00.000", "-6.0"]]);
        let wire = serde_json::to_value(assemble(&no_column, &Value::Null).unwrap()).unwrap();
        assert_eq!(
            wire["btNt"],
            Value::Null,
            "a product without Bt sent {}",
            wire["btNt"]
        );
        // Control: a row that has Bt sends it.
        let wire = serde_json::to_value(
            assemble(&mag_at("2026-09-29 12:00:00.000", "-6.0"), &Value::Null).unwrap(),
        )
        .unwrap();
        // An f32 9.3 goes out as 9.300000190734863, so compare as a number, not as JSON.
        let bt = wire["btNt"]
            .as_f64()
            .expect("control: a present Bt is on the wire");
        assert!((bt - 9.3).abs() < 1e-3, "control: Bt went out as {bt}");
    }

    /// No time, no sample: an undated reading could be of any age, and a reader shown it could only
    /// take it as current.
    #[test]
    fn an_undated_magnetometer_reading_is_no_sample() {
        assert!(
            assemble(&mag_at("not a time", "-6.0"), &Value::Null).is_none(),
            "an undated reading became a sample"
        );
        assert!(
            assemble(&mag_at("2026-09-29 12:00:00.000", "-6.0"), &Value::Null).is_some(),
            "control: the same reading, dated, is one"
        );
    }
}
