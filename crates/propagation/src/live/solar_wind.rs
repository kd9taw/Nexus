//! Solar-wind fetcher (the `live` feature).
//!
//! Networked half: pulls NOAA SWPC's real-time solar-wind products and hands them to the pure
//! [`crate::solar_wind`] parsers. Each holds a day of one-minute records from more than one
//! spacecraft: 1.6 MB (magnetometer) and 2.4 MB (wind) of JSON on 2026-10-09, which the client's
//! gzip brings to 157 KB and 87 KB on the wire. Best-effort — a failed fetch just means the
//! leading-indicator insight is absent this poll (Kp/A from `swpc` still carry the load).

use std::time::Duration;

use serde_json::Value;

use crate::solar_wind::{assemble, SolarWind};

const MAG_URL: &str = "https://services.swpc.noaa.gov/json/rtsw/rtsw_mag_1m.json";
const PLASMA_URL: &str = "https://services.swpc.noaa.gov/json/rtsw/rtsw_wind_1m.json";
const UA: &str = "nexus-propagation/0.1 (+ham radio space weather)";

fn get_json(c: &reqwest::blocking::Client, url: &str) -> Result<Value, String> {
    c.get(url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .map_err(|e| e.to_string())
}

/// Fetch + parse the current solar-wind conditions (Bz, Bt, speed, density).
pub fn fetch_solar_wind() -> Result<SolarWind, String> {
    let c = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(UA)
        .build()
        .map_err(|e| e.to_string())?;
    let mag = get_json(&c, MAG_URL)?;
    // Plasma is best-effort; assemble() leaves speed/density NOT KNOWN (None) if it's absent.
    let plasma = get_json(&c, PLASMA_URL).unwrap_or(Value::Null);
    assemble(&mag, &plasma).ok_or_else(|| "no valid solar-wind sample".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Network test: `#[ignore]`d, run on demand with
    // `cargo test -p propagation --features live -- --ignored live::solar_wind`.

    /// The real files through the real client. The parser's offline tests pin the records; this
    /// pins the wire: SWPC still serves both products at these URLs, in a shape that parses, with a
    /// reading from the spacecraft it marks active that is recent and paired with a speed.
    #[test]
    #[ignore = "network: services.swpc.noaa.gov"]
    fn live_solar_wind_arrives_parses_and_is_recent() {
        let sw = fetch_solar_wind().expect("SWPC answered with products that parse");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!(
            sw.age_secs(now) < 2 * 3600,
            "the newest active reading is {} s old",
            sw.age_secs(now)
        );
        assert!(
            sw.speed_kms.is_some(),
            "the wind product gave no speed paired with the magnetometer's reading"
        );
        println!(
            "SWPC solar wind: Bz {} Bt {:?} speed {:?} density {:?}, {} s old",
            sw.bz_nt,
            sw.bt_nt,
            sw.speed_kms,
            sw.density,
            sw.age_secs(now)
        );
    }
}
