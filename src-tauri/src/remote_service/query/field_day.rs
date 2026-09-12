//! Coherent, bounded Field Day display. This reader never enters an event,
//! changes an operator/bonus, exports a file, or writes a contact.
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DisplaySettings {
    fd_operator: String,
    fd_power_mult: u32,
    fd_bonuses: Vec<String>,
    fd_bonuses_planned: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Capture {
    active: bool,
    field_day: Option<tempo_app::dto::FieldDayStatus>,
    settings: DisplaySettings,
    ruleset: crate::FdRulesetDto,
}

pub(super) fn read_engine(engine: &crate::SharedEngine) -> Result<Value, &'static str> {
    let capture = {
        let e = engine.try_lock().map_err(|_| "applicationBusy")?;
        let s = e.settings();
        if s.fd_operator.len() > 1024
            || s.fd_event.len() > 1024
            || s.fd_bonuses.len() > 64
            || s.fd_bonuses_planned.len() > 64
            || s.fd_bonuses
                .iter()
                .chain(&s.fd_bonuses_planned)
                .any(|v| v.len() > 1024)
        {
            return Err("applicationTooLarge");
        }
        Capture {
            active: s.fd_active,
            field_day: e.bounded_field_day_status()?,
            settings: DisplaySettings {
                fd_operator: s.fd_operator.clone(),
                fd_power_mult: s.fd_power_mult,
                fd_bonuses: s.fd_bonuses.clone(),
                fd_bonuses_planned: s.fd_bonuses_planned.clone(),
            },
            ruleset: crate::fd_ruleset_dto(&s.fd_event),
        }
    };
    let value = serde_json::to_value(capture).map_err(|_| "applicationUnavailable")?;
    if serde_json::to_vec(&value)
        .map_err(|_| "applicationUnavailable")?
        .len()
        > 224 * 1024
    {
        return Err("applicationTooLarge");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    fn engine(event: &str) -> crate::SharedEngine {
        let mut s = tempo_app::settings::Settings::default();
        s.fd_active = true;
        s.fd_event = event.into();
        s.fd_class = "1D".into();
        s.fd_section = "EMA".into();
        s.fd_operator = "W1AW".into();
        s.fd_power_mult = 2;
        s.fd_bonuses = vec!["emergency-power".into()];
        s.fd_bonuses_planned = vec!["natural-power".into()];
        let mut e = tempo_app::engine::Engine::with_settings(s);
        e.restore_field_day_if_enabled();
        assert!(e.fd_log_manual("K1ABC", "2A", "WI", "CW").unwrap());
        assert!(e.fd_log_manual("K1ABC", "2A", "WI", "PH").unwrap());
        Arc::new(Mutex::new(e))
    }
    #[test]
    fn coherent_native_context_uses_exact_scoring_settings_and_rules() {
        for event in ["arrlfd", "wfd"] {
            let e = engine(event);
            let original = e.lock().unwrap().field_day_log_adif();
            let value = read_engine(&e).unwrap();
            let native = e.lock().unwrap().snapshot().field_day.unwrap();
            assert_eq!(value["fieldDay"], serde_json::to_value(native).unwrap());
            assert_eq!(value["fieldDay"]["qsoCount"], 2);
            assert_eq!(value["fieldDay"]["points"], 3);
            assert_eq!(
                value["ruleset"],
                serde_json::to_value(crate::fd_ruleset_dto(event)).unwrap()
            );
            assert_eq!(
                value["settings"],
                serde_json::json!({"fdOperator":"W1AW","fdPowerMult":2,"fdBonuses":["emergency-power"],"fdBonusesPlanned":["natural-power"]})
            );
            assert_eq!(e.lock().unwrap().field_day_log_adif(), original);
        }
    }
    #[test]
    fn disabled_and_busy_sources_are_distinct_and_recover_without_activation() {
        let e = Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
            Default::default(),
        )));
        let off = read_engine(&e).unwrap();
        assert_eq!(off["active"], false);
        assert!(off["fieldDay"].is_null());
        let held = e.lock().unwrap();
        assert_eq!(read_engine(&e).unwrap_err(), "applicationBusy");
        drop(held);
        assert_eq!(read_engine(&e).unwrap(), off);
    }
}
