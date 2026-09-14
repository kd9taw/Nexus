//! Operating preferences a Remote browser may change. The allow-list lives beside the Settings
//! projection it is a subset of (`query::configuration::WRITABLE_*_KEYS`, where each key's reason is
//! written), and every other setting, including one added later, is denied. A change is checked
//! against the Settings document revision the browser showed; the engine then saves it atomically
//! and publishes exactly those fields. The broad form-apply path never runs, so nothing here tunes,
//! keys or touches the TX-enable latch.
use super::super::query::{settings_revision, WRITABLE_CONTROL_KEYS, WRITABLE_LOGGING_KEYS};
use super::logging::{ChangeReason, ChangeWork};
use serde_json::{Map, Value};
use tempo_app::engine::Engine;
use tempo_app::remote_control::Reason;

/// The local grants these changes need, as (logging grant, station control). None when any key is
/// off the allow-list.
pub(super) fn grants(values: &Map<String, Value>) -> Option<(bool, bool)> {
    let mut needs = (false, false);
    for key in values.keys() {
        if WRITABLE_LOGGING_KEYS.contains(&key.as_str()) {
            needs.0 = true;
        } else if WRITABLE_CONTROL_KEYS.contains(&key.as_str()) {
            needs.1 = true;
        } else {
            return None;
        }
    }
    Some(needs)
}

/// Under the Engine lock. The save is synchronous and atomic, as every other remote preference save.
pub(super) fn prepare(
    engine: &mut Engine,
    revision: &str,
    values: &Map<String, Value>,
) -> Result<ChangeWork, ChangeReason> {
    // What the operator changed must be what the station still holds.
    if settings_revision(engine.settings()).ok().as_deref() != Some(revision) {
        return Err(ChangeReason::ContextChanged);
    }
    let allowed: Vec<&str> = WRITABLE_CONTROL_KEYS
        .iter()
        .chain(WRITABLE_LOGGING_KEYS)
        .copied()
        .collect();
    match engine.save_remote_preferences(values, &allowed) {
        Ok(()) => Ok(ChangeWork::SettingsSaved),
        // Nothing was published, but a save that failed part way cannot say what the file holds.
        Err(Reason::PersistenceFailed) => Ok(ChangeWork::SettingsUnconfirmed),
        Err(_) => Err(ChangeReason::InvalidChange),
    }
}
