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

/// ⭐ **What only the host's own screen acts on stays out of everything Remote reads**: the Give
/// button's handle on a turned-away entry (`FdClubRefusedDto::handle`), and the club codes, this
/// laptop's own (`FdClubDto::club_code`) and each turned-away laptop's. The button and its command
/// exist on the host's desktop alone, and a code is shown only there and on that laptop's own
/// screen: one read off any other page is one a peer could make a key to match. The page's Field
/// Day check refuses a key it does not know, so leaving them out also keeps every published page
/// able to read such a host. The Field Day capture and Remote's snapshot both go through here.
pub(in crate::remote_service) fn leave_out_the_hosts_own(
    field_day: &mut Option<tempo_app::dto::FieldDayStatus>,
) {
    if let Some(club) = field_day.as_mut().and_then(|f| f.club.as_mut()) {
        club.club_code.clear();
        for r in &mut club.refused {
            r.handle = None;
            r.club_code = None;
        }
    }
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
        let mut field_day = e.bounded_field_day_status()?;
        leave_out_the_hosts_own(&mut field_day);
        Capture {
            active: s.fd_active,
            field_day,
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
pub(in crate::remote_service) mod tests {
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
    /// ⭐ **The host's Give button reaches Remote neither as a button nor as data.** A host that
    /// turned a laptop away because another laptop holds its position shows the entry with its
    /// handle on its own screen; the capture Remote reads carries the entry, its name, call and
    /// sentence, and not the handle, so the page's Field Day check (which refuses a key it does
    /// not know) reads it as before. CONTROL: the station's own snapshot carries the handle.
    #[test]
    fn the_capture_carries_a_turned_away_laptop_but_not_the_hosts_give_handle() {
        let dir = std::env::temp_dir().join(format!(
            "nexus-remote-fd-give-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let s = tempo_app::settings::Settings {
            fd_active: true,
            fd_event: "arrlfd".into(),
            fd_class: "2A".into(),
            fd_section: "WI".into(),
            mycall: "W9XYZ".into(),
            fd_host_enable: true,
            fd_position_id: "aaaa0001".into(),
            ..Default::default()
        };
        let mut e = tempo_app::engine::Engine::with_settings(s);
        e.restore_field_day_if_enabled();
        e.fd_host_start(dir.join("fd_event_give.jsonl")).unwrap();
        let v = tempo_net::fdsync::PROTO_VERSION;
        let join = |e: &mut tempo_app::engine::Engine, hash: &str| {
            e.fd_club_join(v, "bbbb0002", "CW tent", "W9XYZ", "arrlfd", "", hash)
                .map(|_| ())
        };
        assert_eq!(join(&mut e, &"a".repeat(64)), Ok(()), "the first laptop");
        assert_eq!(
            join(&mut e, &"b".repeat(64)),
            Err(tempo_app::fdevent::POSITION_HELD.to_string()),
            "another laptop naming its position"
        );
        let native = e.snapshot().field_day.unwrap().club.unwrap().refused;
        assert_eq!(native.len(), 1, "{native:?}");
        assert!(
            native[0].handle.is_some(),
            "CONTROL: the host's own snapshot carries the handle"
        );
        let e = Arc::new(Mutex::new(e));
        let value = read_engine(&e).unwrap();
        let entry = &value["fieldDay"]["club"]["refused"][0];
        let mut keys: Vec<&str> = entry
            .as_object()
            .expect("the turned-away entry is in the capture")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["call", "posName", "reason"], "{entry}");
        assert_eq!(entry["posName"], "CW tent");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A host on ARRL Field Day with a club key of its own, which served the CW tent and turned
    /// another laptop naming the tent's position away: its own snapshot shows its own club code
    /// and the turned-away laptop's, beside the Give button's handle.
    pub(in crate::remote_service) fn host_showing_codes(
        dir: &std::path::Path,
    ) -> (tempo_app::engine::Engine, [String; 2]) {
        let s = tempo_app::settings::Settings {
            fd_active: true,
            fd_event: "arrlfd".into(),
            fd_class: "2A".into(),
            fd_section: "WI".into(),
            mycall: "W9XYZ".into(),
            fd_host_enable: true,
            fd_position_id: "aaaa0001".into(),
            ..Default::default()
        };
        let mut e = tempo_app::engine::Engine::with_settings(s);
        e.restore_field_day_if_enabled();
        let own_key = tempo_net::fdsync::PositionKey::new("d".repeat(64));
        let own = tempo_app::fdevent::sha256_hex(own_key.secret());
        e.set_fd_position_key(own_key);
        e.fd_host_start(dir.join("fd_event_codes.jsonl")).unwrap();
        let v = tempo_net::fdsync::PROTO_VERSION;
        let join = |e: &mut tempo_app::engine::Engine, hash: &str| {
            e.fd_club_join(v, "bbbb0002", "CW tent", "W9XYZ", "arrlfd", "", hash)
                .map(|_| ())
        };
        assert_eq!(join(&mut e, &"a".repeat(64)), Ok(()), "the tent");
        assert_eq!(
            join(&mut e, &"b".repeat(64)),
            Err(tempo_app::fdevent::POSITION_HELD.to_string()),
            "another laptop naming its position"
        );
        let codes = [
            tempo_app::fdevent::club_code(&own),
            tempo_app::fdevent::club_code(&"b".repeat(64)),
        ];
        let native = e.snapshot().field_day.unwrap().club.unwrap();
        assert_eq!(native.club_code, codes[0], "CONTROL: the host's own code");
        assert_eq!(
            native.refused[0].club_code.as_deref(),
            Some(codes[1].as_str()),
            "CONTROL: the turned-away laptop's code"
        );
        assert!(native.refused[0].handle.is_some(), "CONTROL: the handle");
        (e, codes)
    }

    /// ⭐ **No club code reaches Remote through the Field Day capture**: the host's own code and
    /// the turned-away laptop's are on its own screen, and the capture carries the entry, its name,
    /// call and sentence, and neither code, so the page's Field Day check reads it as before.
    /// CONTROL: the station's own snapshot carries both codes (`host_showing_codes`).
    #[test]
    fn the_capture_carries_no_club_code() {
        let dir = std::env::temp_dir().join(format!(
            "nexus-remote-fd-codes-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let (e, codes) = host_showing_codes(&dir);
        let e = Arc::new(Mutex::new(e));
        let value = read_engine(&e).unwrap();
        let club = value["fieldDay"]["club"]
            .as_object()
            .expect("the club block is in the capture");
        assert!(!club.contains_key("clubCode"), "{club:?}");
        let mut keys: Vec<&str> = value["fieldDay"]["club"]["refused"][0]
            .as_object()
            .expect("the turned-away entry is in the capture")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["call", "posName", "reason"]);
        let text = serde_json::to_string(&value).unwrap();
        for code in &codes {
            assert!(!text.contains(code.as_str()), "the capture holds {code}");
        }
        let _ = std::fs::remove_dir_all(&dir);
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
