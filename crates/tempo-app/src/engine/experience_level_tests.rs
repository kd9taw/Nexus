//! The experience answer's one writer: what the first-run wizard and Settings ▸ Station write,
//! and why a settings form save cannot move it. Also the setup wizard's "seen" record, which only
//! ever goes from not seen to seen.

use super::*;
use crate::settings::ExperienceLevel;

#[test]
fn the_experience_answer_is_written_by_its_verb() {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    assert_eq!(e.settings().experience_level, None, "not asked yet");
    e.set_experience_level(ExperienceLevel::NewToDigital);
    assert_eq!(
        e.settings().experience_level,
        Some(ExperienceLevel::NewToDigital)
    );
}

/// A surface holding an old settings copy (a form loaded before the wizard answered, a cockpit
/// that snapshots settings once) must not post an old answer, or "not asked", over the live one.
#[test]
fn a_form_save_cannot_move_the_experience_answer() {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    let stale_unasked = e.settings().clone();
    e.set_experience_level(ExperienceLevel::NewToHobby);
    e.apply_settings(stale_unasked);
    assert_eq!(
        e.settings().experience_level,
        Some(ExperienceLevel::NewToHobby),
        "a stale form cleared it"
    );

    let mut stale = e.settings().clone();
    stale.experience_level = Some(ExperienceLevel::Experienced);
    e.apply_settings(stale);
    assert_eq!(
        e.settings().experience_level,
        Some(ExperienceLevel::NewToHobby),
        "a stale form moved it"
    );
}

/// Once the setup wizard is done it stays done: a stale form (loaded before it was), a backup
/// restore or a factory reset must not make a station a new install again. CONTROL: the verb is
/// what marks it, and a form cannot mark it alone either way it reads.
#[test]
fn the_setup_wizard_once_seen_stays_seen() {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    let mut fresh = e.settings().clone();
    fresh.setup_wizard_seen = false;
    e.apply_restored_settings(fresh.clone());
    assert!(!e.settings().setup_wizard_seen, "a new install is due");
    e.mark_setup_wizard_seen();
    assert!(e.settings().setup_wizard_seen, "the verb marks it");
    e.apply_settings(fresh.clone());
    assert!(e.settings().setup_wizard_seen, "a stale form unmarked it");
    e.apply_restored_settings(fresh);
    assert!(e.settings().setup_wizard_seen, "a restore or reset unmarked it");
}
