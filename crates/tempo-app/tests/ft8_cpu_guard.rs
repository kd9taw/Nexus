//! The JTDX-style FT8 decoder's CPU guard: a full pass that takes longer than a reply can wait
//! (`JTDX_FULL_PASS_BUDGET`) steps the preset down one — Ludicrous to Keen, Keen and Custom to
//! Easy — and says so; Easy is the floor, said once; the standard decoder is never chosen for the
//! operator; picking a preset in Settings starts over. The slow decode is injected
//! (`DecodeResult::with_jtdx_took`): a real one on this machine is never that slow.

use std::time::Duration;

use tempo_app::dto::Tier;
use tempo_app::engine::{run_decode_job, DecodePass, Engine, JTDX_FULL_PASS_BUDGET};
use tempo_app::settings::{DecoderStyle, JtdxPreset, Settings};

fn station(preset: JtdxPreset) -> Engine {
    let mut e = Engine::with_settings(Settings {
        mycall: "KD9TAW".to_string(),
        ft8_decoder: DecoderStyle::Jtdx,
        jtdx_preset: preset,
        ..Settings::default()
    });
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e
}

/// One full pass (a silent period: the decoder returns at once) whose time is then `took`.
fn full_pass(e: &mut Engine, slot: u64, took: Duration) {
    let frame = vec![0.0f32; e.active_capture_samples()];
    let job = e.build_decode_job(frame, slot, DecodePass::Boundary);
    let result = run_decode_job(job).with_jtdx_took(took);
    let _ = e.apply_decode_result(result);
}

const LATE: Duration = Duration::from_millis(3_000);
const ON_TIME: Duration = Duration::from_millis(400);

#[test]
fn an_on_time_pass_changes_nothing() {
    let mut e = station(JtdxPreset::Ludicrous);
    full_pass(&mut e, 1, ON_TIME);
    full_pass(&mut e, 2, JTDX_FULL_PASS_BUDGET);
    assert_eq!(e.jtdx_preset_running(), JtdxPreset::Ludicrous);
    assert!(e.snapshot().ft8_step_down.is_none());
}

#[test]
fn late_passes_step_down_one_preset_at_a_time_to_easy_and_stop_there() {
    let mut e = station(JtdxPreset::Ludicrous);

    full_pass(&mut e, 1, LATE);
    assert_eq!(e.jtdx_preset_running(), JtdxPreset::Keen);
    let sd = e.snapshot().ft8_step_down.expect("the step is told");
    assert_eq!(
        (sd.from, sd.to),
        (JtdxPreset::Ludicrous, Some(JtdxPreset::Keen))
    );
    assert_eq!(sd.decode_ms, 3_000);
    assert_eq!(sd.budget_ms, JTDX_FULL_PASS_BUDGET.as_millis() as u32);

    full_pass(&mut e, 2, LATE);
    assert_eq!(e.jtdx_preset_running(), JtdxPreset::Easy);
    let sd = e.snapshot().ft8_step_down.expect("told");
    assert_eq!((sd.from, sd.to), (JtdxPreset::Keen, Some(JtdxPreset::Easy)));

    // Easy is the floor: said once, and the decoder stays JTDX-style on Easy.
    full_pass(&mut e, 3, LATE);
    let floor = e.snapshot().ft8_step_down.expect("told");
    assert_eq!((floor.from, floor.to), (JtdxPreset::Easy, None));
    full_pass(&mut e, 4, LATE);
    assert_eq!(
        e.snapshot().ft8_step_down.expect("told").seq,
        floor.seq,
        "a late Easy pass is said once"
    );
    assert_eq!(e.jtdx_preset_running(), JtdxPreset::Easy);
    assert_eq!(
        e.settings().ft8_decoder,
        DecoderStyle::Jtdx,
        "never the standard decoder by itself"
    );
    assert_eq!(
        e.settings().jtdx_preset,
        JtdxPreset::Ludicrous,
        "the operator's choice is kept; the step is for this session"
    );
}

#[test]
fn custom_steps_down_to_easy() {
    let mut e = station(JtdxPreset::Custom);
    full_pass(&mut e, 1, LATE);
    assert_eq!(e.jtdx_preset_running(), JtdxPreset::Easy);
}

#[test]
fn a_late_result_of_a_preset_no_longer_running_steps_nothing() {
    let mut e = station(JtdxPreset::Ludicrous);
    // Two passes in flight on Ludicrous; the first steps down, the second (also Ludicrous's)
    // must not take it down a second step.
    let frame = vec![0.0f32; e.active_capture_samples()];
    let a = e.build_decode_job(frame.clone(), 1, DecodePass::Boundary);
    let b = e.build_decode_job(frame, 2, DecodePass::Boundary);
    let (ra, rb) = (run_decode_job(a), run_decode_job(b));
    let _ = e.apply_decode_result(ra.with_jtdx_took(LATE));
    let _ = e.apply_decode_result(rb.with_jtdx_took(LATE));
    assert_eq!(e.jtdx_preset_running(), JtdxPreset::Keen);
}

#[test]
fn choosing_a_preset_in_settings_starts_over() {
    let mut e = station(JtdxPreset::Ludicrous);
    full_pass(&mut e, 1, LATE);
    assert_eq!(e.jtdx_preset_running(), JtdxPreset::Keen);
    let mut st = e.settings().clone();
    st.jtdx_preset = JtdxPreset::Keen;
    e.apply_settings(st);
    assert_eq!(e.jtdx_preset_running(), JtdxPreset::Keen);
    let mut st = e.settings().clone();
    st.jtdx_preset = JtdxPreset::Ludicrous;
    e.apply_settings(st);
    assert_eq!(
        e.jtdx_preset_running(),
        JtdxPreset::Ludicrous,
        "the operator's own pick runs again"
    );
}

#[test]
fn the_standard_decoder_has_no_guard() {
    let mut e = station(JtdxPreset::Ludicrous);
    let mut st = e.settings().clone();
    st.ft8_decoder = DecoderStyle::Standard;
    e.apply_settings(st);
    full_pass(&mut e, 1, LATE);
    assert!(e.snapshot().ft8_step_down.is_none());
}
