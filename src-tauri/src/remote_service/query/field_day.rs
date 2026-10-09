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
    /// ⭐ The ticked and planned objectives, ONLY for a contest that scores by them (Winter
    /// Field Day). The hosted page refuses a key it does not know, so every other contest's
    /// capture keeps the four keys every published page accepts; the page that knows these
    /// two must be deployed before a station release that sends them.
    #[serde(skip_serializing_if = "Option::is_none")]
    fd_objectives: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fd_objectives_planned: Option<Vec<String>>,
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
        let e = tempo_app::engine::engine_try_lock(engine).map_err(|_| "applicationBusy")?;
        let s = e.settings();
        if s.fd_operator.len() > 1024
            || s.fd_event.len() > 1024
            || s.fd_bonuses.len() > 64
            || s.fd_bonuses_planned.len() > 64
            || s.fd_objectives.len() > 64
            || s.fd_objectives_planned.len() > 64
            || s.fd_bonuses
                .iter()
                .chain(&s.fd_bonuses_planned)
                .chain(&s.fd_objectives)
                .chain(&s.fd_objectives_planned)
                .any(|v| v.len() > 1024)
        {
            return Err("applicationTooLarge");
        }
        let objectives = tempo_core::fd_rules::ruleset_by_id(
            s.fd_event.trim(),
            tempo_core::fd_rules::CURRENT_RULES_YEAR,
        )
        .is_some_and(|rs| !rs.objective_menu.is_empty());
        Capture {
            active: s.fd_active,
            field_day: e.bounded_field_day_status()?,
            settings: DisplaySettings {
                fd_operator: s.fd_operator.clone(),
                fd_power_mult: s.fd_power_mult,
                fd_bonuses: s.fd_bonuses.clone(),
                fd_bonuses_planned: s.fd_bonuses_planned.clone(),
                fd_objectives: objectives.then(|| s.fd_objectives.clone()),
                fd_objectives_planned: objectives.then(|| s.fd_objectives_planned.clone()),
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
        let s = tempo_app::settings::Settings {
            fd_active: true,
            fd_event: event.into(),
            fd_class: "1D".into(),
            fd_section: "EMA".into(),
            fd_operator: "W1AW".into(),
            fd_power_mult: 2,
            fd_bonuses: vec!["emergency-power".into()],
            fd_bonuses_planned: vec!["natural-power".into()],
            fd_objectives: vec!["wfd-qrp".into()],
            fd_objectives_planned: vec!["wfd-six-hours".into()],
            ..Default::default()
        };
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
            // The objective lists ride only Winter Field Day's capture: ARRL Field Day's keeps
            // the four keys every published page accepts, whatever the settings hold.
            let mut want = serde_json::json!({"fdOperator":"W1AW","fdPowerMult":2,"fdBonuses":["emergency-power"],"fdBonusesPlanned":["natural-power"]});
            if event == "wfd" {
                want["fdObjectives"] = serde_json::json!(["wfd-qrp"]);
                want["fdObjectivesPlanned"] = serde_json::json!(["wfd-six-hours"]);
            }
            assert_eq!(value["settings"], want, "{event}");
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
