//! Real-time solar-wind adapter (pure parser half) — the LEADING geomagnetic indicator.
//!
//! Kp and the A-index (from `swpc`) lag real conditions by hours; the real-time solar-wind
//! stream from the spacecraft upstream at L1 is the early warning. A southward interplanetary magnetic field
//! (Bz negative) couples energy into the magnetosphere → polar/high-latitude HF paths
//! degrade and aurora can light up VHF, typically 1–2 h before Kp catches up. A fast
//! wind stream (high speed) does the same on a slower fuse.
//!
//! NOAA SWPC serves these as real-time JSON products, each a list of named records, one a minute
//! per spacecraft over the last day, with a `time_tag` in UTC (no zone), newest first:
//!   - `services.swpc.noaa.gov/json/rtsw/rtsw_mag_1m.json`
//!     fields include `bz_gsm` (nT) and `bt` (total field, nT)
//!   - `services.swpc.noaa.gov/json/rtsw/rtsw_wind_1m.json`
//!     fields include `proton_speed` (km/s) and `proton_density` (p/cm³)
//!
//! Each list mixes spacecraft: a `source` field names one (SOLAR1, ACE and IMAP on 2026-10-09),
//! and `active` marks the one NOAA's real-time solar wind is using. Only active records are
//! read: the others are a second opinion NOAA is not giving, and IMAP's arrive seconds after
//! the active one's, so "the newest record" alone would read the wrong spacecraft. A bad sample
//! arrives as `null` values (`overall_quality` was 0 on every record in the 2026-10-09
//! capture). These replaced the `products/solar-wind/{mag,plasma}-1-day.json` products (rows of
//! strings under a header row), which answered 404 by 2026-10-08.
//!
//! Pure (`&Value` in, `Option<...>` out) so it is unit-testable offline; the networked
//! fetcher lives in `live::solar_wind`. Fields are read BY NAME and the newest reading is found
//! BY ITS OWN TIME, not its place in the list, so a reordered/extended product doesn't silently
//! misread.
//!
//! ⚠️ A SAMPLE SAYS WHEN IT WAS MADE AND WHAT IT DOES NOT KNOW. The station keeps its last good
//! sample while the feed is unreachable, so every sample carries the time its magnetometer reading
//! was made (`time_unix`, the record's own time tag), and a reading with no time is no sample: its age
//! could be anything and a reader shown it could only take it as current. Past
//! [`SOLAR_WIND_STALE_SECS`] a sample is a record of the past — the insight feed stops speaking from
//! it and the Space Wx gauges say how old it is. Speed and density come from the separate plasma
//! product and are `None` when it did not answer, or when its newest reading is not from the same
//! moment as the magnetometer's — never 0, which is a solar wind that stopped blowing. Bt is `None`
//! the same way when the magnetometer reading carries Bz without it.

use serde::Serialize;
use serde_json::{Map, Value};

/// How long a sample speaks for "now", in seconds. The real-time products are one-minute data a
/// few minutes behind; older than this, a sample is history. The pairing of the plasma reading with the
/// magnetometer's uses it, the insight feed stops speaking from an older sample, and the UI's gauges
/// say the sample's age from the same moment (`propViz.ts` reads this number out of this file).
pub const SOLAR_WIND_STALE_SECS: i64 = 30 * 60;

/// Current solar-wind conditions (most recent valid sample in the product window).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SolarWind {
    /// Bz (GSM), nT. Negative = southward = the geoeffective case.
    pub bz_nt: f32,
    /// Total field magnitude Bt, nT. `None` = NOT KNOWN: the magnetometer reading carried Bz without
    /// it.
    pub bt_nt: Option<f32>,
    /// Bulk speed, km/s. `None` = NOT KNOWN: the plasma product did not answer, or its newest
    /// reading is not from this sample's moment.
    pub speed_kms: Option<f32>,
    /// Proton density, p/cm³, from the same plasma reading as the speed. `None` = not known.
    pub density: Option<f32>,
    /// When the magnetometer reading was made (its record's `time_tag`), Unix seconds UTC.
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

/// A record's `field` as f32. NOAA sends numbers; a bad or missing sample is `null` → None.
fn num(record: &Map<String, Value>, field: &str) -> Option<f32> {
    record.get(field)?.as_f64().map(|x| x as f32)
}

/// The newest record NOAA marks `active` whose `field` is a number and whose `time_tag` parses —
/// so a `null` sample doesn't blank the readout, a spacecraft NOAA is not using is never read, and
/// no reading is taken without knowing when it was made. Found by its own time, not its place in
/// the list. Returns that record and its time.
fn newest_active_with<'a>(v: &'a Value, field: &str) -> Option<(&'a Map<String, Value>, i64)> {
    v.as_array()?
        .iter()
        .filter_map(|r| {
            let r = r.as_object()?;
            if r.get("active").and_then(Value::as_bool) != Some(true) {
                return None;
            }
            num(r, field)?; // require the anchor field
            let t = crate::kc2g::parse_naive_utc_unix(r.get("time_tag")?.as_str()?)?;
            Some((r, t))
        })
        .max_by_key(|&(_, t)| t)
}

/// Parse the `rtsw_mag_1m` product → (Bz, Bt, the reading's time) from the newest active reading
/// with a Bz. Bt is `None` when that reading carries none.
pub fn parse_mag(v: &Value) -> Option<(f32, Option<f32>, i64)> {
    let (r, time) = newest_active_with(v, "bz_gsm")?;
    Some((num(r, "bz_gsm")?, num(r, "bt"), time))
}

/// Parse the `rtsw_wind_1m` product → (speed, density, the reading's time) from the newest active
/// reading with a speed. The density is `None` when that reading carries none.
pub fn parse_plasma(v: &Value) -> Option<(f32, Option<f32>, i64)> {
    let (r, time) = newest_active_with(v, "proton_speed")?;
    Some((num(r, "proton_speed")?, num(r, "proton_density"), time))
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

    /// NOAA's own records, captured at 03:4x UTC on 9 Oct 2026: the newest twelve of each product,
    /// verbatim and in NOAA's order (newest first). Three spacecraft are in them: SOLAR1, the one
    /// marked `active`, and ACE and IMAP, which are not. IMAP's newest magnetometer record
    /// (03:41:02, Bz -4.67) is newer than the active one's (03:40:00, Bz -4.4).
    const MAG_CAPTURE: &str = include_str!("../tests/fixtures/swpc-rtsw-mag-1m-2026-10-09.json");
    const WIND_CAPTURE: &str = include_str!("../tests/fixtures/swpc-rtsw-wind-1m-2026-10-09.json");

    /// 03:40:00 UTC on 9 Oct 2026, the newest active reading in both captures.
    const CAPTURED: i64 = 1_791_517_200;

    /// One `rtsw_mag_1m` record: the fields this reads, and a few it does not.
    fn mag(tag: &str, source: &str, active: bool, bz: Value, bt: Value) -> Value {
        json!({"time_tag": tag, "active": active, "source": source, "bx_gsm": 1.12,
               "by_gsm": -0.68, "bz_gsm": bz, "bt": bt, "overall_quality": 0})
    }

    /// One `rtsw_wind_1m` record, the same way.
    fn wind(tag: &str, source: &str, active: bool, speed: Value, density: Value) -> Value {
        json!({"time_tag": tag, "active": active, "source": source, "proton_speed": speed,
               "proton_density": density, "proton_temperature": 98011, "overall_quality": 0})
    }

    /// The magnetometer product with one active reading, `bz` at `tag`.
    fn mag_at(tag: &str, bz: f64) -> Value {
        json!([mag(tag, "SOLAR1", true, json!(bz), json!(9.3))])
    }

    /// The wind product with one active reading, `speed` at `tag`.
    fn wind_at(tag: &str, speed: f64) -> Value {
        json!([wind(tag, "SOLAR1", true, json!(speed), json!(5.2))])
    }

    /// 12:00:00 UTC on 29 Sep 2026, the magnetometer reading's time in these fixtures.
    const NOON: i64 = 1_790_683_200;

    #[test]
    fn reads_noaas_own_records_from_the_spacecraft_it_marks_active() {
        let mag: Value = serde_json::from_str(MAG_CAPTURE).unwrap();
        let wind: Value = serde_json::from_str(WIND_CAPTURE).unwrap();
        let sw = assemble(&mag, &wind).expect("NOAA's own records made no sample");
        assert_eq!(
            sw.time_unix, CAPTURED,
            "not the newest active reading's time (IMAP's newer 03:41:02 is not active)"
        );
        assert!(
            (sw.bz_nt - -4.4).abs() < 1e-3,
            "Bz {} where the active spacecraft's is -4.4",
            sw.bz_nt
        );
        assert!((sw.bt_nt.unwrap() - 4.82).abs() < 1e-3);
        assert!((sw.speed_kms.unwrap() - 360.5).abs() < 1e-3);
        assert!((sw.density.unwrap() - 6.46).abs() < 1e-3);
    }

    #[test]
    fn a_newer_reading_from_a_spacecraft_not_marked_active_is_not_read() {
        let v = json!([
            mag("2026-09-29T12:00:02", "IMAP", false, json!(7.0), json!(8.0)),
            mag(
                "2026-09-29T12:00:00",
                "SOLAR1",
                true,
                json!(-6.0),
                json!(9.3)
            ),
            mag("2026-09-29T12:00:00", "ACE", false, json!(2.0), json!(3.0)),
        ]);
        let (bz, _, time) = parse_mag(&v).unwrap();
        assert!(
            (bz - -6.0).abs() < 1e-3,
            "read Bz {bz}, not the active spacecraft's -6.0"
        );
        assert_eq!(time, NOON);
        // Control: the same newer record, marked active, is the one read.
        let v = json!([
            mag("2026-09-29T12:00:02", "IMAP", true, json!(7.0), json!(8.0)),
            mag(
                "2026-09-29T12:00:00",
                "SOLAR1",
                true,
                json!(-6.0),
                json!(9.3)
            ),
        ]);
        let (bz, _, _) = parse_mag(&v).unwrap();
        assert!(
            (bz - 7.0).abs() < 1e-3,
            "control: an active newer reading was not read ({bz})"
        );
    }

    #[test]
    fn no_reading_marked_active_is_no_sample() {
        let inactive = json!([mag(
            "2026-09-29T12:00:00",
            "ACE",
            false,
            json!(-6.0),
            json!(9.3)
        )]);
        assert!(
            parse_mag(&inactive).is_none(),
            "a reading NOAA does not mark active became a sample"
        );
        let unmarked = json!([{"time_tag": "2026-09-29T12:00:00", "bz_gsm": -6.0, "bt": 9.3}]);
        assert!(
            parse_mag(&unmarked).is_none(),
            "a reading with no active mark became a sample"
        );
        assert!(
            parse_mag(&mag_at("2026-09-29T12:00:00", -6.0)).is_some(),
            "control: the same reading, marked active, is one"
        );
    }

    #[test]
    fn parses_bz_and_bt_from_the_newest_valid_reading() {
        // Newest first, as NOAA lists them; the newest carries no Bz (a bad sample arrives as null).
        let v = json!([
            mag(
                "2024-01-01T00:02:00",
                "SOLAR1",
                true,
                Value::Null,
                Value::Null
            ),
            mag(
                "2024-01-01T00:01:00",
                "SOLAR1",
                true,
                json!(-8.2),
                json!(9.3)
            ),
            mag(
                "2024-01-01T00:00:00",
                "SOLAR1",
                true,
                json!(-3.5),
                json!(5.1)
            ),
        ]);
        let (bz, bt, time) = parse_mag(&v).unwrap();
        assert!((bz - -8.2).abs() < 1e-3); // skipped the null reading
        assert!((bt.unwrap() - 9.3).abs() < 1e-3);
        assert_eq!(time, 1_704_067_260, "the 00:01 reading's own time");
    }

    #[test]
    fn the_newest_reading_is_found_by_its_time_not_its_place_in_the_list() {
        let v = json!([
            mag(
                "2024-01-01T00:00:00",
                "SOLAR1",
                true,
                json!(-3.5),
                json!(5.1)
            ),
            mag(
                "2024-01-01T00:01:00",
                "SOLAR1",
                true,
                json!(-8.2),
                json!(9.3)
            ),
        ]);
        let (bz, _, time) = parse_mag(&v).unwrap();
        assert_eq!(
            time, 1_704_067_260,
            "oldest first, the newest reading was not the one read"
        );
        assert!((bz - -8.2).abs() < 1e-3);
    }

    #[test]
    fn parses_speed_and_density_by_field_name() {
        let (speed, density, _) = parse_plasma(&wind_at("2024-01-01T00:00:00", 420.0)).unwrap();
        assert!((speed - 420.0).abs() < 1e-3);
        assert!((density.unwrap() - 5.2).abs() < 1e-3);
    }

    #[test]
    fn assemble_survives_missing_plasma() {
        let sw = assemble(&mag_at("2024-01-01T00:00:00", -6.0), &json!(null)).unwrap();
        assert!((sw.bz_nt - -6.0).abs() < 1e-3);
        // Not known — this assertion pinned the 0 until the plasma feed's absence reached the wire.
        assert_eq!(sw.speed_kms, None);
        assert_eq!(sw.density, None);
    }

    #[test]
    fn empty_retired_or_unrecognised_products_are_no_sample() {
        assert!(parse_mag(&json!([])).is_none());
        assert!(parse_mag(&json!(null)).is_none());
        // The retired `products/solar-wind` layout (a header row, then rows of strings) is no
        // sample, never a misread one.
        let retired = json!([
            ["time_tag", "bz_gsm", "bt"],
            ["2026-09-29 12:00:00.000", "-6.0", "7.0"]
        ]);
        assert!(parse_mag(&retired).is_none(), "the retired layout was read");
        // A wind reading without a speed.
        let no_speed =
            json!([{"time_tag": "2026-09-29T12:00:00", "active": true, "proton_density": 5.2}]);
        assert!(parse_plasma(&no_speed).is_none());
    }

    /// ⚠️ THE WIRE, NOT THE STRUCT: a page reads the serialized sample. "The plasma product did not
    /// answer" must reach it as NOT KNOWN — a 0 there is a solar wind that stopped blowing, which the
    /// Sun's never does, and it put "wind 0 km/s" into the insight feed.
    #[test]
    fn a_missing_plasma_feed_reaches_the_wire_as_not_known_never_zero() {
        let sw = assemble(&mag_at("2026-09-29T12:00:00", -6.0), &Value::Null).unwrap();
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

    /// The sample says WHEN it was measured — the magnetometer reading's own time tag, not when
    /// Nexus fetched it — so every reader can tell a reading from a record of the past.
    #[test]
    fn the_sample_carries_the_time_its_magnetometer_reading_was_made() {
        let sw = assemble(
            &mag_at("2026-09-29T12:00:00", -6.0),
            &wind_at("2026-09-29T11:59:00", 420.0),
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

    /// A plasma product whose newest valid reading is from hours before the magnetometer's is down
    /// in all but name: its speed is not THIS sample's speed.
    #[test]
    fn a_plasma_reading_from_another_hour_is_not_this_samples_speed() {
        let sw = assemble(
            &mag_at("2026-09-29T12:00:00", -6.0),
            &wind_at("2026-09-29T10:00:00", 420.0),
        )
        .unwrap();
        let wire = serde_json::to_value(sw).unwrap();
        assert_eq!(
            wire["speedKms"],
            Value::Null,
            "a two-hour-old speed was paired with a fresh Bz"
        );
    }

    /// Bt the same way as the speed: a magnetometer reading that carries Bz but no total field (a
    /// null, or a record without the field) sends Bt as not known — a 0 there is a field that
    /// vanished, and it read as "Bt 0.0 nT" in the insight feed.
    #[test]
    fn a_missing_total_field_reaches_the_wire_as_not_known_never_zero() {
        let null_bt = json!([mag(
            "2026-09-29T12:00:00",
            "SOLAR1",
            true,
            json!(-6.0),
            Value::Null
        )]);
        let wire = serde_json::to_value(assemble(&null_bt, &Value::Null).unwrap()).unwrap();
        assert_eq!(
            wire["btNt"],
            Value::Null,
            "a missing Bt went out as {}",
            wire["btNt"]
        );
        let no_field = json!([
            {"time_tag": "2026-09-29T12:00:00", "active": true, "source": "SOLAR1", "bz_gsm": -6.0}
        ]);
        let wire = serde_json::to_value(assemble(&no_field, &Value::Null).unwrap()).unwrap();
        assert_eq!(
            wire["btNt"],
            Value::Null,
            "a reading without Bt sent {}",
            wire["btNt"]
        );
        // Control: a reading that has Bt sends it.
        let wire = serde_json::to_value(
            assemble(&mag_at("2026-09-29T12:00:00", -6.0), &Value::Null).unwrap(),
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
            assemble(&mag_at("not a time", -6.0), &Value::Null).is_none(),
            "an undated reading became a sample"
        );
        assert!(
            assemble(&mag_at("2026-09-29T12:00:00", -6.0), &Value::Null).is_some(),
            "control: the same reading, dated, is one"
        );
    }
}
