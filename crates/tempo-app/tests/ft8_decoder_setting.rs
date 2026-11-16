//! Settings ▸ FT8 decoder: the JTDX-style decoder (WSJT-X 3.0.2's multithreaded one, its port of
//! JTDX's) or WSJT-X's standard decoder runs FT8's full pass, and the JTDX-style one is the
//! default, for a new settings file and for one written before the setting existed.
//!
//! Which decoder ran is told from the records: ft8var reports no sync metric (its callback has
//! none, `ft8var_cabi.f90` writes 0), the standard decoder always does.

use std::sync::Mutex;

use tempo_app::dto::Tier;
use tempo_app::engine::{run_decode_job, DecodePass, Engine};
use tempo_app::settings::{DecoderStyle, JtdxPreset, Settings};

/// Both decoders keep process-global state between periods: one test at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn ft8_sample() -> Vec<f32> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../ft8/tests/fixtures/ft8_sample.wav"
    );
    let b = std::fs::read(path).expect("read the FT8 sample");
    let mut i = 12usize;
    while i + 8 <= b.len() {
        let size = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        if &b[i..i + 4] == b"data" {
            let end = (i + 8 + size).min(b.len());
            let mut s: Vec<f32> = b[i + 8..end]
                .chunks_exact(2)
                .map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / 32767.0)
                .collect();
            s.resize(modes::make_mode(modes::ModeKind::Ft8).frame_samples(), 0.0);
            return s;
        }
        i += 8 + size + (size & 1);
    }
    panic!("no audio in {path}");
}

/// The boundary (full) pass over the sample with `style`, as the radio loop builds it.
fn full_pass(style: DecoderStyle) -> Vec<modes::Decode> {
    let mut eng = Engine::with_settings(Settings {
        mycall: "KD9TAW".to_string(),
        band: "20m".to_string(),
        dial_mhz: 14.074,
        ft8_decoder: style,
        jtdx_preset: JtdxPreset::Easy,
        ..Settings::default()
    });
    eng.set_tier(Tier::Ft8);
    eng.set_frequency(14.074, "20m", "USB");
    eng.set_rx_offset(1500.0);
    let job = eng.build_decode_job(ft8_sample(), 1, DecodePass::Boundary);
    run_decode_job(job).decodes().to_vec()
}

#[test]
fn the_setting_chooses_the_decoder_of_the_full_pass() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let jtdx = full_pass(DecoderStyle::Jtdx);
    let standard = full_pass(DecoderStyle::Standard);
    assert!(!jtdx.is_empty() && !standard.is_empty());
    assert!(
        jtdx.iter().all(|d| d.sync == 0.0),
        "JTDX-style: every record is ft8var's"
    );
    assert!(
        standard.iter().any(|d| d.sync > 0.0),
        "Standard: the records are the standard decoder's"
    );
}

#[test]
fn the_jtdx_style_full_pass_finds_every_message_the_standard_one_finds_on_the_sample() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let jtdx: std::collections::HashSet<String> = full_pass(DecoderStyle::Jtdx)
        .iter()
        .map(|d| d.message.trim().to_string())
        .collect();
    let missed: Vec<String> = full_pass(DecoderStyle::Standard)
        .iter()
        .map(|d| d.message.trim().to_string())
        .filter(|m| !jtdx.contains(m))
        .collect();
    assert!(
        missed.is_empty(),
        "only the standard decoder found {missed:?}"
    );
}

#[test]
fn jtdx_style_easy_is_the_default_and_an_older_settings_file_gets_it() {
    let fresh = Settings::default();
    assert_eq!(fresh.ft8_decoder, DecoderStyle::Jtdx);
    assert_eq!(fresh.jtdx_preset, JtdxPreset::Easy);
    // A settings file from before the setting existed: none of its keys.
    let old: Settings = serde_json::from_str("{}").expect("an empty settings file loads");
    assert_eq!(old.ft8_decoder, DecoderStyle::Jtdx);
    assert_eq!(old.jtdx_preset, JtdxPreset::Easy);
    // And the setting round-trips under its own keys.
    let s = Settings {
        ft8_decoder: DecoderStyle::Standard,
        jtdx_preset: JtdxPreset::Ludicrous,
        ..Settings::default()
    };
    let json = serde_json::to_value(&s).expect("serialize");
    assert_eq!(json["ft8Decoder"], "standard");
    assert_eq!(json["jtdxPreset"], "ludicrous");
    let back: Settings = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back.ft8_decoder, DecoderStyle::Standard);
    assert_eq!(back.jtdx_preset, JtdxPreset::Ludicrous);
}
