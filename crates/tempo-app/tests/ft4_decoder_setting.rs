//! Settings ▸ FT4 decoder: the JTDX-style decoder (JTDX 2.2.159's own FT4 decoder) or WSJT-X's
//! standard decoder runs FT4's full pass, and the JTDX-style one is the default, for a new
//! settings file and for one written before the setting existed.
//!
//! Which decoder ran is told from the records: JTDX's FT4 decoder reports no sync metric
//! (`ft4jtdx_cabi.f90` writes 0), the standard decoder always does.

use std::sync::Mutex;

use tempo_app::dto::Tier;
use tempo_app::engine::{run_decode_job, DecodePass, Engine};
use tempo_app::settings::{DecoderStyle, Settings};

/// Both decoders keep process-global state between periods: one test at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// WSJT-X's FT4 sample period as captured audio.
fn ft4_sample() -> Vec<f32> {
    ft4_fixture(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../ft4/tests/fixtures/ft4_sample.wav"
    ))
}

/// An FT4 fixture period as captured audio.
fn ft4_fixture(path: &str) -> Vec<f32> {
    let b = std::fs::read(path).expect("read the FT4 fixture");
    let mut i = 12usize;
    while i + 8 <= b.len() {
        let size = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        if &b[i..i + 4] == b"data" {
            let end = (i + 8 + size).min(b.len());
            let mut s: Vec<f32> = b[i + 8..end]
                .chunks_exact(2)
                .map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / 32767.0)
                .collect();
            s.resize(modes::make_mode(modes::ModeKind::Ft4).frame_samples(), 0.0);
            return s;
        }
        i += 8 + size + (size & 1);
    }
    panic!("no audio in {path}");
}

/// The boundary (full) pass over the sample with `style`, as the radio loop builds it.
fn full_pass(style: DecoderStyle) -> Vec<modes::Decode> {
    full_pass_over(style, ft4_sample())
}

/// The boundary (full) pass over `audio` with `style`, as the radio loop builds it.
fn full_pass_over(style: DecoderStyle, audio: Vec<f32>) -> Vec<modes::Decode> {
    let mut eng = Engine::with_settings(Settings {
        mycall: "KD9TAW".to_string(),
        band: "20m".to_string(),
        dial_mhz: 14.080,
        ft4_decoder: style,
        ..Settings::default()
    });
    eng.set_tier(Tier::Ft4);
    eng.set_frequency(14.080, "20m", "USB");
    eng.set_rx_offset(1500.0);
    let job = eng.build_decode_job(audio, 1, DecodePass::Boundary);
    run_decode_job(job).decodes().to_vec()
}

fn messages(d: &[modes::Decode]) -> Vec<String> {
    let mut m: Vec<String> = d.iter().map(|d| d.message.trim().to_string()).collect();
    m.sort();
    m
}

#[test]
fn the_setting_chooses_the_decoder_of_ft4_s_full_pass() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let jtdx = full_pass(DecoderStyle::Jtdx);
    let standard = full_pass(DecoderStyle::Standard);
    assert!(!jtdx.is_empty() && !standard.is_empty());
    assert!(
        jtdx.iter().all(|d| d.sync == 0.0),
        "JTDX-style: every record is JTDX's"
    );
    assert!(
        standard.iter().any(|d| d.sync > 0.0),
        "Standard: the records are the standard decoder's"
    );
    // On this real period both find the same messages (measured: 16 each).
    assert_eq!(messages(&jtdx), messages(&standard));
}

#[test]
fn jtdx_style_is_ft4_s_default_and_an_older_settings_file_gets_it() {
    assert_eq!(Settings::default().ft4_decoder, DecoderStyle::Jtdx);
    let old: Settings = serde_json::from_str("{}").expect("an empty settings file loads");
    assert_eq!(old.ft4_decoder, DecoderStyle::Jtdx);
    let s = Settings {
        ft4_decoder: DecoderStyle::Standard,
        ..Settings::default()
    };
    let json = serde_json::to_value(&s).expect("serialize");
    assert_eq!(json["ft4Decoder"], "standard");
    let back: Settings = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back.ft4_decoder, DecoderStyle::Standard);
}

/// A decode the JTDX-style FT4 decoder marks unchecked (an OSD decode it takes from noise) is
/// handed on by the engine still marked, so everything downstream that holds automation to
/// checked decodes sees it as such. The period is pure noise from which JTDX's FT4 decoder
/// decodes a free-text line that passes the CRC; the Standard decoder finds nothing in it.
#[test]
fn the_engine_hands_on_ft4_s_unchecked_mark() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let noise = || {
        ft4_fixture(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ft4/tests/fixtures/ft4_noise_osd_false_decode.wav"
        ))
    };
    let jtdx = full_pass_over(DecoderStyle::Jtdx, noise());
    assert!(
        !jtdx.is_empty(),
        "the JTDX-style decoder decodes the noise period"
    );
    assert!(
        jtdx.iter().all(|d| d.unchecked),
        "every decode from it arrives marked: {jtdx:?}"
    );
    assert!(full_pass_over(DecoderStyle::Standard, noise()).is_empty());
}
