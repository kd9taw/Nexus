//! FT4's JTDX-style decoder (JTDX 2.2.159's own FT4 decoder) as the engine asks for it: only on
//! FT4 with Settings ▸ FT4 decoder on JTDX-style; at depth 3, JTDX's default, under every preset
//! and at the operator's Decode depth under Custom; with the preset's SWL; and never feeding FT8's
//! CPU guard, which measures FT8's full pass against FT8's budget.

use super::*;
use crate::settings::{DecoderStyle, JtdxCustom, JtdxPreset};

/// An FT4 station on 20 m, with `f` applied to its settings.
fn station(f: impl FnOnce(&mut crate::settings::Settings)) -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_license_class("extra");
    let mut st = e.settings().clone();
    f(&mut st);
    e.apply_settings(st);
    e.set_tier(Tier::Ft4);
    e.set_frequency(14.080, "20m", "USB");
    e
}

fn job(e: &Engine) -> DecodeJob {
    e.build_decode_job(vec![0.0; 72_576], 1, DecodePass::Boundary)
}

#[test]
fn ft4_runs_the_jtdx_style_decoder_by_default_and_the_standard_one_when_chosen() {
    assert!(job(&station(|_| {})).jtdx.is_some());
    let standard = station(|s| s.ft4_decoder = DecoderStyle::Standard);
    assert!(job(&standard).jtdx.is_none());
    // FT4's setting is its own: FT8's Standard leaves FT4 JTDX-style.
    let ft8_standard = station(|s| s.ft8_decoder = DecoderStyle::Standard);
    assert!(job(&ft8_standard).jtdx.is_some());
}

#[test]
fn ft4_depth_is_jtdx_s_default_under_every_preset_and_decode_depth_under_custom() {
    for preset in [JtdxPreset::Easy, JtdxPreset::Keen, JtdxPreset::Ludicrous] {
        let e = station(|s| {
            s.jtdx_preset = preset;
            s.decode_depth = 1;
        });
        let j = job(&e);
        assert_eq!(j.jtdx.as_ref().map(|r| r.ft4_depth), Some(3), "{preset:?}");
        // The standard early pass keeps the operator's Decode depth.
        assert_eq!(j.ndepth, 1);
    }
    let custom = station(|s| {
        s.jtdx_preset = JtdxPreset::Custom;
        s.decode_depth = 2;
    });
    assert_eq!(job(&custom).jtdx.map(|r| r.ft4_depth), Some(2));
}

#[test]
fn ft4_takes_swl_from_the_preset() {
    let swl = |preset: JtdxPreset, custom_swl: bool| {
        let e = station(|s| {
            s.jtdx_preset = preset;
            s.jtdx_custom = JtdxCustom {
                swl: custom_swl,
                ..JtdxCustom::default()
            };
        });
        job(&e).jtdx.expect("JTDX-style").knobs.swl
    };
    assert!(!swl(JtdxPreset::Easy, true));
    assert!(swl(JtdxPreset::Ludicrous, false));
    assert!(swl(JtdxPreset::Custom, true));
    assert!(!swl(JtdxPreset::Custom, false));
}

#[test]
fn ft4_never_feeds_ft8_s_cpu_guard() {
    assert!(job(&station(|_| {})).jtdx_preset.is_none());
}
