//! NOAA SWPC space-weather adapter (the `live` feature).
//!
//! Pulls the 10.7 cm solar flux, the 1-minute estimated planetary K-index, the GOES long-band
//! X-ray flux and NOAA's daily planetary A from the public SWPC services (no auth; NOAA SWPC
//! products are US Government works, public domain) and normalizes them into a [`SpaceWx`]
//! plus a [`SpaceWxStamps`] saying when each number was measured.
//!
//! THE HONESTY CONTRACT. Each quantity is fetched and parsed on its own: one product failing
//! leaves that number a stand-in (its stamp `None`) and keeps the others, and a value is a
//! reading only with its own sample time from the product — never the time Nexus fetched it.
//! A missing X-ray is unknown, not a quiet B1.0. The "A" is NOAA's daily planetary A for a
//! dated day, never an estimate; the instantaneous ap from Kp rides beside it as `ap_now`.

use std::time::Duration;

use serde_json::Value;

use crate::model::{SpaceWx, SpaceWxStamps};

const UA: &str = "nexus-propagation/0.1 (+ham radio propagation nowcast)";

/// Penticton 10.7 cm flux, three readings a day, newest first.
const SFI_URL: &str = "https://services.swpc.noaa.gov/json/f107_cm_flux.json";
/// The 1-minute estimated planetary K-index, newest last.
const KP_URL: &str = "https://services.swpc.noaa.gov/json/planetary_k_index_1m.json";
/// GOES primary X-ray flux, 1-minute samples over six hours, newest last.
const XRAY_URL: &str = "https://services.swpc.noaa.gov/json/goes/primary/xrays-6-hour.json";
/// Daily Geomagnetic Data (DGD.txt): thirty days of daily A and K, issued 00:30 UT.
const DGD_URL: &str = "https://services.swpc.noaa.gov/text/daily-geomagnetic-indices.txt";

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(UA)
        .build()
        .map_err(|e| e.to_string())
}

fn get_json(c: &reqwest::blocking::Client, url: &str) -> Result<Value, String> {
    c.get(url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .map_err(|e| e.to_string())
}

fn get_text(c: &reqwest::blocking::Client, url: &str) -> Result<String, String> {
    c.get(url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())
}

/// One measured value and the product's own time for it (Unix seconds, UTC).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub value: f32,
    pub at: i64,
}

/// What one SWPC poll measured: the numbers the model runs on (a stand-in from
/// [`SpaceWx::default`] wherever a product did not answer) and when each was measured.
#[derive(Debug, Clone, Copy)]
pub struct SwpcReading {
    pub wx: SpaceWx,
    pub stamps: SpaceWxStamps,
    /// The instantaneous ap estimated from the measured Kp (`None` without one).
    pub ap_now: Option<f32>,
}

/// Fetch the current space weather from NOAA SWPC. `Err` only when NO product answered with
/// a value it dates; any one that did becomes a reading and the rest stay stand-ins.
pub fn fetch_space_wx() -> Result<SwpcReading, String> {
    let c = client()?;
    let sfi = get_json(&c, SFI_URL).ok().and_then(|v| parse_sfi(&v));
    let kp = get_json(&c, KP_URL).ok().and_then(|v| parse_kp(&v));
    let xray = get_json(&c, XRAY_URL).ok().and_then(|v| parse_xray(&v));
    let a = get_text(&c, DGD_URL)
        .ok()
        .and_then(|b| parse_daily_planetary_a(&b));
    reading_from(sfi, kp, xray, a).ok_or_else(|| "space weather: no SWPC product answered".into())
}

/// Assemble a reading from whichever products answered. Pure. `None` when none did.
pub fn reading_from(
    sfi: Option<Sample>,
    kp: Option<Sample>,
    xray: Option<Sample>,
    a: Option<Sample>,
) -> Option<SwpcReading> {
    let stamps = SpaceWxStamps {
        sfi_at: sfi.map(|s| s.at),
        kp_at: kp.map(|s| s.at),
        xray_at: xray.map(|s| s.at),
        a_day: a.map(|s| s.at),
    };
    if !stamps.any() {
        return None;
    }
    let stand_in = SpaceWx::default();
    Some(SwpcReading {
        wx: SpaceWx {
            sfi: sfi.map_or(stand_in.sfi, |s| s.value),
            ssn: None, // R12 rides the separate solar-cycle feed (LAST_SSN)
            kp: kp.map_or(stand_in.kp, |s| s.value),
            a_index: a.map_or(stand_in.a_index, |s| s.value),
            xray_long: xray.map_or(stand_in.xray_long, |s| s.value),
        },
        stamps,
        ap_now: kp.map(|s| kp_to_ap(s.value)),
    })
}

/// The day's 10.7 cm solar flux from `f107_cm_flux.json` (newest first): Penticton measures
/// three times a day and the canonical daily value is the local-noon (20:00 UT) one, so that is
/// the reading taken — the same number NOAA's daily file and the 30-day trend carry. Falls back
/// to the newest dated reading only if no noon reading is in the file.
pub fn parse_sfi(v: &Value) -> Option<Sample> {
    let rows = v.as_array()?;
    let dated = |r: &Value| -> Option<Sample> {
        Some(Sample {
            value: r.get("flux")?.as_f64()? as f32,
            at: crate::kc2g::parse_naive_utc_unix(r.get("time_tag")?.as_str()?)?,
        })
    };
    rows.iter()
        .filter(|r| r.get("reporting_schedule").and_then(Value::as_str) == Some("Noon"))
        .find_map(dated)
        .or_else(|| rows.iter().find_map(dated))
}

/// The newest 1-minute estimated planetary Kp (`planetary_k_index_1m.json`, newest last).
pub fn parse_kp(v: &Value) -> Option<Sample> {
    v.as_array()?.iter().rev().find_map(|r| {
        let kp = r
            .get("estimated_kp")
            .and_then(Value::as_f64)
            .or_else(|| r.get("kp_index").and_then(Value::as_f64))?;
        Some(Sample {
            value: kp as f32,
            at: crate::kc2g::parse_naive_utc_unix(r.get("time_tag")?.as_str()?)?,
        })
    })
}

/// The newest GOES long-band (0.1–0.8 nm) X-ray sample that carries a flux (newest last). The
/// `flux` field is the electron-corrected value. A gap in the feed leaves the last sample with
/// its own, older time — the caller sees how old it is instead of reading it as now.
pub fn parse_xray(v: &Value) -> Option<Sample> {
    v.as_array()?.iter().rev().find_map(|r| {
        if r.get("energy").and_then(Value::as_str) != Some("0.1-0.8nm") {
            return None;
        }
        Some(Sample {
            value: r.get("flux")?.as_f64()? as f32,
            at: crate::kc2g::parse_naive_utc_unix(r.get("time_tag")?.as_str()?)?,
        })
    })
}

/// NOAA's daily planetary A from `DGD.txt`: the newest day with a value (the current UT day
/// reads `-1` until it is over). `at` is 00:00 UT of that day.
///
/// Each data row is `YYYY MM DD`, then Fredericksburg A and eight K, College A and eight K,
/// then the planetary A and eight decimal Kp. The planetary A is the token just before the
/// first decimal one; a fixed token count would break on the unspaced `-1-1-1` of an
/// incomplete day.
pub fn parse_daily_planetary_a(body: &str) -> Option<Sample> {
    body.lines()
        .filter_map(|line| {
            let toks: Vec<&str> = line.split_whitespace().collect();
            let (y, m, d) = (toks.first()?, toks.get(1)?, toks.get(2)?);
            if y.len() != 4 || !y.bytes().all(|b| b.is_ascii_digit()) {
                return None; // a header or comment line
            }
            let first_kp = toks.iter().position(|t| t.contains('.'))?;
            let a: f32 = toks.get(first_kp.checked_sub(1)?)?.parse().ok()?;
            let day = crate::kc2g::parse_naive_utc_unix(&format!("{y}-{m}-{d}T00:00:00"))?;
            (a >= 0.0).then_some(Sample { value: a, at: day })
        })
        .max_by_key(|s| s.at)
}

/// Standalone GOES X-ray fetch — the 60 s "fast lane" behind `get_xray_now`, so a flare's
/// onset reaches the UI in ~1 min instead of the 5-min snapshot TTL. Carries the sample time.
pub fn fetch_xray_now() -> Result<Sample, String> {
    let v = get_json(&client()?, XRAY_URL)?;
    parse_xray(&v).ok_or_else(|| "xray: no dated long-band sample".to_string())
}

/// Kp → instantaneous ap (the standard Kp-to-ap table, interpolated): the "ap now"
/// estimate. Not the daily A, which is the mean of a whole UT day's eight ap values.
pub fn kp_to_ap(kp: f32) -> f32 {
    const AP: [f32; 10] = [0.0, 4.0, 7.0, 15.0, 27.0, 48.0, 80.0, 140.0, 240.0, 400.0];
    let k = kp.clamp(0.0, 9.0);
    let i = k.floor() as usize;
    if i >= 9 {
        return AP[9];
    }
    AP[i] + (AP[i + 1] - AP[i]) * (k - i as f32)
}

/// The NOAA planetary-K outlook (3-hourly, about a week back and three days
/// forward). See [`crate::kpforecast`] for what the samples mean; this is only the
/// wire.
pub fn fetch_kp_forecast() -> Result<crate::kpforecast::KpForecast, String> {
    let c = client()?;
    let body = c
        .get("https://services.swpc.noaa.gov/products/noaa-planetary-k-index-forecast.json")
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())?;
    Ok(crate::kpforecast::parse_kp_forecast(&body))
}

/// NOAA's daily solar indices: the last thirty days of 10.7 cm flux and sunspot number. See
/// [`crate::solar_indices`] for what the rows mean; this is only the wire.
///
/// A body with no dated row is an ERROR, never an empty success: the command caches only what
/// this returns `Ok`, so an error page served with a 200 cannot replace thirty real days with
/// nothing, and the last good copy keeps being served with its own dates on it.
pub fn fetch_daily_solar_indices() -> Result<crate::solar_indices::DailySolarIndices, String> {
    let c = client()?;
    let body = c
        .get("https://services.swpc.noaa.gov/text/daily-solar-indices.txt")
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())?;
    crate::solar_indices::parse_daily_solar_indices(&body)
        .ok_or_else(|| "daily solar indices: no dated row in the response".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Live SWPC products captured 2026-10-30 ~02:46 UT, trimmed (public domain, see the URLs above).
    const F107: &str = include_str!("../../tests/fixtures/swpc-f107-cm-flux-2026-10-30.json");
    const KP_1M: &str =
        include_str!("../../tests/fixtures/swpc-planetary-k-index-1m-2026-10-30.json");
    const XRAYS: &str = include_str!("../../tests/fixtures/swpc-xrays-6-hour-2026-10-30.json");
    const DGD: &str =
        include_str!("../../tests/fixtures/swpc-daily-geomagnetic-indices-2026-10-30.txt");

    fn json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }
    fn utc(s: &str) -> i64 {
        crate::kc2g::parse_naive_utc_unix(s).unwrap()
    }

    /// Penticton reads three times a day; the day's SFI is the noon (20:00 UT) reading, which
    /// is what NOAA's daily file and the 30-day trend beside the gauge carry. The newest row
    /// (the 22:00 afternoon reading, 90) is not it.
    #[test]
    fn sfi_is_the_noon_reading_with_its_own_time() {
        let s = parse_sfi(&json(F107)).unwrap();
        assert_eq!(s.value, 89.0);
        assert_eq!(s.at, utc("2026-10-29T20:00:00"));
        // No noon row at all: the newest dated reading, not nothing.
        let only = serde_json::json!([
            {"time_tag": "2026-10-29T22:00:00", "flux": 90.0, "reporting_schedule": "Afternoon"},
        ]);
        assert_eq!(parse_sfi(&only).map(|s| s.value), Some(90.0));
    }

    #[test]
    fn kp_and_xray_carry_the_products_own_sample_time() {
        let kp = parse_kp(&json(KP_1M)).unwrap();
        assert_eq!((kp.value, kp.at), (0.67, utc("2026-10-30T02:40:00")));
        let x = parse_xray(&json(XRAYS)).unwrap();
        assert_eq!(x.at, utc("2026-10-30T02:42:00Z"));
        assert!((x.value - 2.13e-7).abs() < 1e-9, "{}", x.value);
    }

    /// A GOES gap: the newest long-band rows carry no flux. The reading is the last real sample
    /// with ITS time, so the UI can say how old it is, never the fetch time.
    #[test]
    fn an_xray_gap_keeps_the_last_samples_own_time() {
        let v = serde_json::json!([
            {"time_tag": "2026-10-30T01:00:00Z", "flux": 3.0e-6, "energy": "0.1-0.8nm"},
            {"time_tag": "2026-10-30T02:40:00Z", "flux": null, "energy": "0.1-0.8nm"},
            {"time_tag": "2026-10-30T02:41:00Z", "flux": 1.0e-9, "energy": "0.05-0.4nm"},
        ]);
        let x = parse_xray(&v).unwrap();
        assert_eq!((x.value, x.at), (3.0e-6, utc("2026-10-30T01:00:00Z")));
        // An undated value is not a reading.
        let undated = serde_json::json!([{"flux": 3.0e-6, "energy": "0.1-0.8nm"}]);
        assert_eq!(parse_xray(&undated), None);
    }

    /// NOAA's daily planetary A from DGD.txt: the newest COMPLETE day (29 Oct, A 4). Today's row
    /// is all `-1` until the day ends and is skipped.
    #[test]
    fn the_a_is_noaas_daily_planetary_a_for_the_newest_complete_day() {
        let a = parse_daily_planetary_a(DGD).unwrap();
        assert_eq!((a.value, a.at), (4.0, utc("2026-10-29T00:00:00")));
        // A storm day reads its own planetary A, not Fredericksburg's or College's.
        let storm = "2026 10 04    21  3 3 3 4 4 3 4 4    46  2 2 5 6 6 6 5 3    36   3.00  3.33  4.33  5.00  4.67  4.33  5.67  4.67\n";
        assert_eq!(parse_daily_planetary_a(storm).map(|s| s.value), Some(36.0));
        assert_eq!(parse_daily_planetary_a("<html>error</html>"), None);
    }

    /// One product failing keeps the others as readings and leaves only its own number a
    /// stand-in, with no stamp. The A is the daily A; the Kp-derived ap rides apart.
    #[test]
    fn each_quantity_is_a_reading_or_a_stand_in_on_its_own() {
        let kp = parse_kp(&json(KP_1M));
        let a = parse_daily_planetary_a(DGD);
        // SFI and X-ray failed: Kp and A are still readings.
        let r = reading_from(None, kp, None, a).expect("Kp and A answered");
        assert_eq!(r.stamps.kp_at, Some(utc("2026-10-30T02:40:00")));
        assert_eq!(r.stamps.a_day, Some(utc("2026-10-29T00:00:00")));
        assert_eq!(r.stamps.sfi_at, None, "SFI is a stand-in");
        assert_eq!(
            r.stamps.xray_at, None,
            "a missing X-ray is unknown, not quiet"
        );
        assert_eq!(r.wx.kp, 0.67);
        assert_eq!(r.wx.a_index, 4.0, "the daily planetary A, not kp_to_ap");
        let ap = r.ap_now.unwrap();
        assert!(
            (ap - kp_to_ap(0.67)).abs() < 1e-6 && (ap - 2.68).abs() < 0.01,
            "{ap}"
        );
        // Nothing answered: no reading at all.
        assert!(reading_from(None, None, None, None).is_none());
        // No Kp: no ap estimate either.
        assert_eq!(
            reading_from(parse_sfi(&json(F107)), None, None, None)
                .unwrap()
                .ap_now,
            None
        );
    }

    #[test]
    fn kp_to_ap_follows_the_standard_table() {
        assert_eq!(kp_to_ap(0.0), 0.0);
        assert_eq!(kp_to_ap(5.0), 48.0);
        assert_eq!(kp_to_ap(9.0), 400.0);
        assert!((kp_to_ap(2.5) - 11.0).abs() < 1e-6);
    }

    // ---- Network test: `#[ignore]`d, run on demand with
    // `cargo test -p propagation --features live -- --ignored live::swpc`.

    /// The real file through the real client. The parser's offline tests pin the rows; this pins
    /// the wire: SWPC still serves the text product at this URL, in a shape that parses.
    #[test]
    #[ignore = "network: services.swpc.noaa.gov"]
    fn live_daily_solar_indices_arrive_and_parse() {
        let f = fetch_daily_solar_indices().expect("SWPC answered with a file that parses");
        assert!(
            f.days.len() >= 25,
            "about thirty days, got {}",
            f.days.len()
        );
        assert!(
            f.days.iter().any(|d| d.sfi.is_some() && d.ssn.is_some()),
            "no day carries both a flux and a sunspot number"
        );
        let newest = f.days.last().unwrap();
        println!(
            "SWPC daily indices: {} days, newest day_unix {} sfi {:?} ssn {:?}",
            f.days.len(),
            newest.day_unix,
            newest.sfi,
            newest.ssn
        );
    }
}
