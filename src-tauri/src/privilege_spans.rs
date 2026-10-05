//! The licence-class band edges the scopes draw: where the operator's class may transmit a
//! section's emission, read from the transmit gate's own table
//! ([`tempo_app::privileges::allowed_spans`]).
//!
//! DISPLAY ONLY. Nothing here decides a transmission: `privileges::tx_allowed` stays the gate, and
//! its own test holds these spans to its answer at every segment edge and across every band. A
//! read, so it changes nothing; the class is the station's own setting, read under the engine lock
//! for the one call.
//!
//! Desktop only. The Remote page's commands are a closed, versioned set that this is not part of,
//! so `api.ts` answers there without asking, and the scopes there draw no edges.

use serde::Serialize;
use tauri::State;
use tempo_app::engine::{engine_lock, Engine};
use tempo_app::settings::{LicenseClass, OperatingMode};

use crate::SharedEngine;

/// What a scope needs to tint the frequencies the operator may not transmit on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivilegeSpans {
    /// The class the spans are for, so a reader can tell when the station's class has changed.
    pub class: LicenseClass,
    /// The section whose emission they judge.
    pub mode: OperatingMode,
    /// The class has no edges: `Open` (a non-US licence, or none declared), whose gate allows every
    /// frequency. `spans` is then empty, and a scope tints nothing.
    pub unrestricted: bool,
    /// `[lo, hi)` MHz, ascending, merged where they touch: inside one, the class may key the
    /// section's emission; outside every one, the gate refuses it.
    pub spans: Vec<[f64; 2]>,
}

/// The spans for `class` in `mode`, as the command answers them.
pub(crate) fn privilege_spans(class: LicenseClass, mode: OperatingMode) -> PrivilegeSpans {
    let spans = tempo_app::privileges::allowed_spans(class, mode);
    PrivilegeSpans {
        class,
        mode,
        unrestricted: spans.is_none(),
        spans: spans
            .unwrap_or_default()
            .into_iter()
            .map(|(lo, hi)| [lo, hi])
            .collect(),
    }
}

/// The spans for the station's own class.
fn spans_for(engine: &Engine, mode: OperatingMode) -> PrivilegeSpans {
    privilege_spans(engine.settings().license_class, mode)
}

/// `get_privilege_spans { mode }` → [`PrivilegeSpans`]: the licence-class band edges for the
/// section `mode` ('digital' | 'phone' | 'cw' | 'rtty' | 'keyboard', the snapshot's
/// `radio.operatingMode`). Read-only.
#[tauri::command(async)]
pub fn get_privilege_spans(state: State<'_, SharedEngine>, mode: OperatingMode) -> PrivilegeSpans {
    spans_for(&engine_lock(&state), mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spans_are_the_gates_own_for_the_stations_class() {
        let mut engine = Engine::new("KD9TAW", "EN52", 0);
        engine.set_license_class("general");
        let got = spans_for(&engine, OperatingMode::Phone);
        assert_eq!(got.class, LicenseClass::General);
        assert!(!got.unrestricted);
        let want: Vec<[f64; 2]> =
            tempo_app::privileges::allowed_spans(LicenseClass::General, OperatingMode::Phone)
                .unwrap()
                .into_iter()
                .map(|(lo, hi)| [lo, hi])
                .collect();
        assert_eq!(got.spans, want);
        assert!(
            got.spans.contains(&[14.225, 14.350]),
            "General's 20 m phone"
        );
        // A class change is read at the next call: nothing is cached here.
        engine.set_license_class("extra");
        assert!(spans_for(&engine, OperatingMode::Phone)
            .spans
            .contains(&[14.150, 14.350]));
    }

    #[test]
    fn open_is_unrestricted_and_has_no_spans() {
        let mut engine = Engine::new("KD9TAW", "EN52", 0);
        engine.set_license_class("open");
        let got = spans_for(&engine, OperatingMode::Digital);
        assert!(got.unrestricted);
        assert!(got.spans.is_empty());
    }

    /// The UI reads these names: `api.ts` `PrivilegeSpans`.
    #[test]
    fn the_wire_shape_is_what_the_ui_reads() {
        let v = serde_json::to_value(privilege_spans(LicenseClass::Technician, OperatingMode::Cw))
            .unwrap();
        assert_eq!(v["class"], "technician");
        assert_eq!(v["mode"], "cw");
        assert_eq!(v["unrestricted"], false);
        assert_eq!(v["spans"][0], serde_json::json!([3.525, 3.6]));
        // The section arrives as the snapshot names it.
        let mode: OperatingMode = serde_json::from_value(serde_json::json!("keyboard")).unwrap();
        assert_eq!(mode, OperatingMode::Keyboard);
    }

    /// A command that is not registered fails at run time, not at build time.
    #[test]
    fn the_command_is_registered() {
        let src = include_str!("lib.rs");
        let list = src
            .split_once("tauri::generate_handler![")
            .expect("the handler list")
            .1
            .split_once("])")
            .expect("the end of the handler list")
            .0;
        assert!(
            list.lines()
                .any(|l| l.trim() == "privilege_spans::get_privilege_spans,"),
            "get_privilege_spans is not registered: the scopes' band edges would never load"
        );
    }
}
