//! FT8 + FT4 modem loopback for the ARRL Field Day exchange.
//!
//! The sibling `fieldday_loopback.rs` proves the Class+Section exchange end to
//! end, but only over FT1 (its harness builds FT1 frames via `tx::build`). FT8
//! and FT4 carry the same [`Msg::FieldDay`] 77-bit payload (the vendored WSJT-X
//! packer's ARRL Field Day message type), yet that support was only *inferred*
//! from reading the vendored sources — never exercised. This closes that
//! confidence gap: encode a real `Msg::FieldDay` exchange, push it through the
//! FT8 and FT4 modems (encode → waveform → clean-frame decode via the `modes`
//! `Mode`/`NativeSource` trait path), and assert the decoded text re-parses to
//! the *same* `Msg::FieldDay` — proving the payload survives both modems intact.

use modes::{make_mode, DecodeRequest, ModeKind, NativeSource, SignalSource};
use tempo_core::message::Msg;

const FS: f32 = 12_000.0;
const F0: f32 = 1500.0;

/// Scale a float waveform into an `iwave` frame of `frame_len` samples. FT8/FT4
/// `Mode::gen_wave` already includes the 0.5 s slot lead-in, so the tones are
/// self-positioned at offset 0 (matching the proven `modes` native harness).
fn frame(wave: &[f32], frame_len: usize) -> Vec<i16> {
    let mut iwave = vec![0i16; frame_len];
    for (i, &s) in wave.iter().enumerate() {
        if i < frame_len {
            iwave[i] = (s * 1000.0).clamp(-32768.0, 32767.0) as i16;
        }
    }
    iwave
}

/// Encode `msg`'s text through `kind`'s modem, decode the clean frame back, and
/// return whether the recovered text re-parses to the identical `Msg`.
fn round_trips(kind: ModeKind, msg: &Msg) -> bool {
    let text = msg.to_text();
    let mode = make_mode(kind);
    let tones = mode.encode(&text);
    assert!(
        !tones.is_empty(),
        "{} must pack the Field Day message '{text}'",
        kind.as_str()
    );
    let wave = mode.gen_wave(&tones, FS, F0);
    let iwave = frame(&wave, mode.frame_samples());

    let mut src = NativeSource::from_kind(kind);
    let decs = src.decode(&DecodeRequest::full_band(&iwave));
    let got = decs.iter().find(|d| d.message == text);
    assert!(
        got.is_some(),
        "{} must decode its own Field Day frame '{text}'; got {decs:?}",
        kind.as_str()
    );
    // The decoded text must re-parse to the SAME Msg::FieldDay — proving the
    // 77-bit Class+Section payload (not just some text) survived the modem.
    Msg::parse(&got.unwrap().message) == *msg
}

fn fd_exchange() -> Msg {
    Msg::FieldDay {
        to: "W9XYZ".into(),
        de: "K2DEF".into(),
        roger: false,
        class: "3A".into(),
        section: "WI".into(),
    }
}

fn fd_rogered_exchange() -> Msg {
    Msg::FieldDay {
        to: "W9XYZ".into(),
        de: "K2DEF".into(),
        roger: true,
        class: "2A".into(),
        section: "IL".into(),
    }
}

#[test]
fn field_day_exchange_round_trips_through_ft8() {
    assert!(round_trips(ModeKind::Ft8, &fd_exchange()));
    assert!(round_trips(ModeKind::Ft8, &fd_rogered_exchange()));
}

#[test]
fn field_day_exchange_round_trips_through_ft4() {
    assert!(round_trips(ModeKind::Ft4, &fd_exchange()));
    assert!(round_trips(ModeKind::Ft4, &fd_rogered_exchange()));
}
