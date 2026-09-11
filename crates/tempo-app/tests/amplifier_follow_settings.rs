use std::path::PathBuf;
use tempo_app::{
    engine::Engine,
    settings::{RadioProfile, RadioProfilePatch, Settings},
};

struct Store(PathBuf);
impl Store {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "nexus-amp-follow-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("settings.json")
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn settings() -> Settings {
    let mut s = Settings {
        amp_model: "spe".into(),
        amp_port: "follow-radio-zero".into(),
        amp_follow_band: false,
        ..Default::default()
    };
    s.ensure_radio_profiles();
    s.radios.push(RadioProfile {
        id: 1,
        name: "Other radio".into(),
        amp_model: "kpa".into(),
        amp_port: "follow-radio-one".into(),
        amp_follow_band: true,
        rigctld_port: 4534,
        rotctld_port: 4535,
        ..Default::default()
    });
    // Match the normal loader's daemon-port allocation before taking an
    // unchanged-profile baseline; that allocation is independent of follow.
    s.ensure_distinct_radio_ports();
    s
}

fn patch(e: &Engine, id: u32) -> RadioProfilePatch {
    serde_json::from_value(
        serde_json::to_value(e.settings().radios.iter().find(|p| p.id == id).unwrap()).unwrap(),
    )
    .unwrap()
}

fn profile(s: &Settings, id: u32) -> &RadioProfile {
    s.radios.iter().find(|p| p.id == id).unwrap()
}

#[test]
fn active_settings_form_follow_choice_survives_save_reload_in_both_directions() {
    let store = Store::new();
    let mut e = Engine::with_settings(settings());
    let other = serde_json::to_value(profile(e.settings(), 1)).unwrap();
    for desired in [true, false] {
        let mut form = e.settings().clone();
        form.amp_follow_band = desired;
        // This is the actual active-radio SettingsPanel -> set_settings path.
        e.apply_settings(form);
        assert_eq!(e.settings().amp_follow_band, desired);
        assert_eq!(
            profile(e.settings(), 0).amp_follow_band,
            desired,
            "the poller consumes this profile"
        );
        assert_eq!(
            serde_json::to_value(profile(e.settings(), 1)).unwrap(),
            other
        );
        e.settings().save(&store.file()).unwrap();
        let restored = Settings::load(&store.file());
        assert_eq!(restored.amp_follow_band, desired);
        assert_eq!(profile(&restored, 0).amp_follow_band, desired);
        assert_eq!(serde_json::to_value(profile(&restored, 1)).unwrap(), other);
        e = Engine::with_settings(restored);
        assert_eq!(e.settings().amp_follow_band, desired);
    }
}

#[test]
fn radio_switch_reflects_each_profiles_saved_follow_choice() {
    let mut e = Engine::with_settings(settings());
    for id in [1, 0, 1, 0] {
        e.set_active_radio(id);
        assert_eq!(e.settings().active_radio, id);
        assert_eq!(e.settings().amp_follow_band, id == 1);
        assert!(!profile(e.settings(), 0).amp_follow_band);
        assert!(profile(e.settings(), 1).amp_follow_band);
        assert_eq!(profile(e.settings(), 0).amp_port, "follow-radio-zero");
        assert_eq!(profile(e.settings(), 1).amp_port, "follow-radio-one");
    }
}

#[test]
fn per_radio_edit_updates_the_active_mirror_without_reverting_another_profile() {
    let store = Store::new();
    let mut e = Engine::with_settings(settings());
    let mut edit = patch(&e, 0);
    edit.amp_follow_band = true;
    e.update_radio_profile(0, edit);
    assert!(e.settings().amp_follow_band);
    let mut edit = patch(&e, 1);
    edit.amp_follow_band = false;
    e.update_radio_profile(1, edit);
    assert_eq!(e.settings().active_radio, 0);
    assert!(e.settings().amp_follow_band);
    e.settings().save(&store.file()).unwrap();
    let restored = Settings::load(&store.file());
    assert!(restored.amp_follow_band);
    assert!(profile(&restored, 0).amp_follow_band);
    assert!(!profile(&restored, 1).amp_follow_band);
}

#[test]
fn a_stale_native_form_edits_its_named_profile_without_switching_the_live_radio() {
    let mut e = Engine::with_settings(settings());
    let mut form = e.settings().clone();
    form.amp_follow_band = true;
    e.set_active_radio(1);
    e.apply_settings(form);
    assert_eq!(e.settings().active_radio, 1);
    assert!(profile(e.settings(), 0).amp_follow_band);
    assert!(profile(e.settings(), 1).amp_follow_band);
    assert!(e.settings().amp_follow_band);
}

#[test]
fn legacy_flat_settings_seed_follow_but_existing_profiles_remain_authoritative() {
    let store = Store::new();
    for desired in [false, true] {
        let mut legacy = settings();
        legacy.radios.clear();
        legacy.amp_follow_band = desired;
        std::fs::write(store.file(), serde_json::to_vec(&legacy).unwrap()).unwrap();
        let loaded = Settings::load(&store.file());
        assert_eq!(profile(&loaded, 0).amp_follow_band, desired);
        assert_eq!(loaded.amp_follow_band, desired);
    }
    // Old mirror omissions can leave these representations different. Keep
    // the profile already used by the worker; do not silently enable/disable it.
    for desired in [true, false] {
        let mut previous = settings();
        previous.radios[0].amp_follow_band = desired;
        previous.amp_follow_band = !desired;
        std::fs::write(store.file(), serde_json::to_vec(&previous).unwrap()).unwrap();
        let loaded = Settings::load(&store.file());
        assert_eq!(profile(&loaded, 0).amp_follow_band, desired);
        assert_eq!(loaded.amp_follow_band, desired);
    }
}
