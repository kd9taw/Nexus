//! APRS keeps its 300-packet journal and age-based 2,000-station map distinct.
//! Page their complete native projections; stream health independently of maps.
use serde_json::{json, Value};

pub(super) fn live(e: &tempo_app::engine::Engine) -> Result<Value, &'static str> {
    let s = e.settings();
    if s.mygrid.len() > 12
        || s.aprs_symbol_table.len() > 4
        || s.aprs_symbol_code.len() > 4
        || s.aprs_comment.len() > 256
        || s.aprs_path.len() > 16
        || s.aprs_path.iter().any(|p| p.len() > 80)
        || s.aprs_is_watch_calls.len() > 100
        || s.aprs_is_watch_calls.iter().any(|p| p.len() > 80)
    {
        return Err("applicationTooLarge");
    }
    let health = e.aprs_health();
    let inet = e.aprs_is_status();
    if health.radio_name.len() > 256 || inet.last_reject.as_ref().is_some_and(|s| s.len() > 1024) {
        return Err("applicationTooLarge");
    }
    Ok(
        json!({"health": health, "isStatus": inet, "capturedAtMs": super::now_ms(),
        "settings": {"mygrid":s.mygrid,"aprsChannelMhz":s.aprs_channel_mhz,
            "aprsComment":s.aprs_comment,"aprsPath":s.aprs_path,
            "aprsSymbolTable":s.aprs_symbol_table,"aprsSymbolCode":s.aprs_symbol_code,
            "aprsIsEnabled":s.aprs_is_enabled,"aprsIsRadiusKm":s.aprs_is_radius_km,
            "aprsIsWatchCalls":s.aprs_is_watch_calls}}),
    )
}

pub(super) fn capture(
    engine: &crate::SharedEngine,
) -> Result<(Vec<Value>, usize, Value), &'static str> {
    let e = engine.try_lock().map_err(|_| "applicationBusy")?;
    if !e.aprs_display_within_budget(2 * 1024 * 1024) {
        return Err("applicationTooLarge");
    }
    let captured_at_ms = super::now_ms();
    let heard = e.aprs_heard();
    let roster = e.aprs_stations((captured_at_ms / 1000) as i64);
    drop(e);
    let meta = json!({"capturedAtMs": captured_at_ms, "ttlMin": roster.ttl_min,
        "fadeAfterMin": roster.fade_after_min, "packets": heard.len(), "stations": roster.stations.len()});
    let rows: Vec<_> = heard
        .into_iter()
        .map(|v| json!({"kind":"packet","value":v}))
        .chain(
            roster
                .stations
                .into_iter()
                .map(|v| json!({"kind":"station","value":v})),
        )
        .collect();
    let total = rows.len();
    Ok((rows, total, meta))
}

#[cfg(test)]
pub(super) fn seed(e: &mut tempo_app::engine::Engine) {
    use tempo_app::engine::{AprsHeard, AprsSource};
    let at = (super::now_ms() / 1000) as i64;
    for i in 0..2000 {
        e.push_aprs_heard(AprsHeard {
            source: if i == 0 {
                "W1AW".into()
            } else {
                format!("K{i}TEST")
            },
            dest: "APNEXU".into(),
            path: vec!["WIDE1-1".into()],
            lat: Some(41.714),
            lon: Some(-72.728),
            symbol_table: '/',
            symbol_code: '>',
            kind: "position",
            text: format!("Station {i}"),
            speed_knots: None,
            course_deg: None,
            addressee: None,
            msg_id: None,
            at_unix: at - 2000 + i,
            source_kind: if i % 2 == 0 {
                AprsSource::Rf
            } else {
                AprsSource::Inet
            },
            wx: None,
            raw: format!("K{i}TEST>APNEXU:test"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tempo_app::engine::Engine;
    #[test]
    fn full_packet_and_station_stores_remain_distinct_and_native() {
        let mut e = Engine::with_settings(Default::default());
        seed(&mut e);
        let expected_heard = serde_json::to_value(e.aprs_heard()).unwrap();
        let expected_roster =
            serde_json::to_value(e.aprs_stations((super::super::now_ms() / 1000) as i64)).unwrap();
        let expected_settings = serde_json::to_value(e.settings()).unwrap();
        let shared = Arc::new(Mutex::new(e));
        let (rows, total, meta) = capture(&shared).unwrap();
        assert_eq!(total, 2300);
        assert_eq!(rows.len(), total);
        assert_eq!(meta["packets"], 300);
        assert_eq!(meta["stations"], 2000);
        assert_eq!(
            json!(rows[..300].iter().map(|r| &r["value"]).collect::<Vec<_>>()),
            expected_heard
        );
        assert_eq!(
            json!(rows[300..].iter().map(|r| &r["value"]).collect::<Vec<_>>()),
            expected_roster["stations"]
        );
        assert!(rows[300..].iter().any(|r| r["value"]["call"] == "W1AW"));
        assert!(!rows[..300].iter().any(|r| r["value"]["source"] == "W1AW"));
        let e = shared.lock().unwrap();
        let health = live(&e).unwrap();
        assert_eq!(
            health["health"],
            serde_json::to_value(e.aprs_health()).unwrap()
        );
        assert_eq!(
            health["isStatus"],
            serde_json::to_value(e.aprs_is_status()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(e.settings()).unwrap(),
            expected_settings
        );
        assert!(!e.snapshot().radio.tx_enabled);
        assert_eq!(capture(&shared).unwrap_err(), "applicationBusy");
    }
    #[test]
    fn display_budget_and_setting_projection_refuse_unbounded_or_private_data() {
        let mut e = Engine::with_settings(Default::default());
        seed(&mut e);
        assert!(e.aprs_display_within_budget(2 * 1024 * 1024));
        assert!(!e.aprs_display_within_budget(100));
        let value = live(&e).unwrap();
        let settings = value["settings"].as_object().unwrap();
        assert_eq!(settings.len(), 9);
        for key in [
            "aprsIsPasscode",
            "aprsIsServer",
            "qrzPassword",
            "qrzUsername",
            "radios",
        ] {
            assert!(!settings.contains_key(key));
        }
        let mut s = e.settings().clone();
        s.aprs_comment = "X".repeat(257);
        e.apply_settings(s);
        assert_eq!(live(&e).unwrap_err(), "applicationTooLarge");
    }
}
