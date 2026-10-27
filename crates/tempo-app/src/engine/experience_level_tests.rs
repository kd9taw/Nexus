//! The experience answer's one writer: what the first-run wizard and Settings ▸ Station write,
//! and why a settings form save cannot move it.

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
