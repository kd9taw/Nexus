//! The FSK callsign ID, all the way round: Nexus sends a picture with the burst
//! on the end, and Nexus reads the callsign back off its own transmission.
//!
//! `fsk.rs`'s unit tests already round-trip the burst on its own. This file is
//! the other half — the burst sitting where it really sits, after a whole
//! picture, decoded by the real `SstvDecoder` off the real `encode_image_with_id`
//! output. That is a different claim: it says the burst is in the part of the
//! transmission the receiver looks in, at the level the receiver expects, after
//! audio long enough to have moved the decoder through three of its states.
//!
//! It is also the test that found the receive-side bug this work had to fix
//! first — see `decoder.rs`'s `FSK_CAPTURE_SECONDS`. The post-image capture used
//! to be a flat three seconds INCLUDING the carried-back image tail, so for any
//! mode whose four carry-back lines exceed three seconds (PD-240, PD-290,
//! Scottie DX, Pasokon P7) the window was already full of picture before the
//! burst arrived, and the callsign was never read. That was true of received
//! pictures from other stations too, and had been since the decoder shipped.

#![allow(clippy::expect_used, clippy::panic)]

use tempo_sstv::{encode_image_with_id, for_mode, SourceImage, SstvDecoder, SstvEvent, SstvMode};

const RATE: u32 = 12_000;
const CALL: &str = "KD9TAW";

fn flat_image(mode: SstvMode) -> SourceImage {
    let spec = for_mode(mode);
    SourceImage {
        width: spec.line_pixels,
        height: spec.image_lines,
        rgb: vec![[96, 128, 160]; (spec.line_pixels * spec.image_lines) as usize],
    }
}

/// Send `mode` with (or without) the ID and return what the decoder made of it.
fn transmit_and_receive(mode: SstvMode, id: Option<&str>) -> Vec<SstvEvent> {
    let img = flat_image(mode);
    let mut audio = encode_image_with_id(mode, &img, RATE, id).expect("encode");
    // Runway, as every other loopback test uses: the decoder needs the buffer
    // to fill and the post-image capture to reach its cap.
    audio.extend(std::iter::repeat_n(0.0_f32, RATE as usize * 6));
    let mut dec = SstvDecoder::new(RATE).expect("decoder");
    dec.process(&audio)
}

fn decoded_id(events: &[SstvEvent]) -> Option<String> {
    events.iter().find_map(|e| match e {
        SstvEvent::FskId { text } => Some(text.clone()),
        _ => None,
    })
}

/// ⭐ The whole point. Four modes, chosen for their line times rather than their
/// families: Martin 2 (0.23 s) sits well inside the old capture window, Scottie 1
/// (0.43 s) near it, and PD-240 (1.00 s) and Scottie DX (1.05 s) well outside it
/// — those two are the ones that never read an ID before this batch.
#[test]
fn a_transmitted_picture_carries_a_callsign_that_reads_back() {
    for mode in [
        SstvMode::Martin2,
        SstvMode::Scottie1,
        SstvMode::Pd240,
        SstvMode::ScottieDx,
    ] {
        let events = transmit_and_receive(mode, Some(CALL));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SstvEvent::ImageComplete { .. })),
            "{mode:?}: the picture itself did not decode",
        );
        assert_eq!(
            decoded_id(&events).as_deref(),
            Some(CALL),
            "{mode:?}: the callsign burst did not read back",
        );
    }
}

/// ⭐ THE CONTROL, and it is the default. With the setting off — which is what
/// `encode_image` does and what every station gets until they switch it on — the
/// transmission carries no burst and the receiver reports no ID. Without this,
/// the test above would also pass against a decoder that hallucinated a callsign
/// out of the trailing silence.
#[test]
fn the_control_a_picture_sent_with_the_setting_off_carries_no_id() {
    for mode in [SstvMode::Martin2, SstvMode::Pd240] {
        let events = transmit_and_receive(mode, None);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SstvEvent::ImageComplete { .. })),
            "{mode:?}: the picture itself did not decode",
        );
        assert_eq!(
            decoded_id(&events),
            None,
            "{mode:?}: a picture sent WITHOUT an ID reported one anyway",
        );
    }
}

/// A callsign the burst cannot carry is sent as no burst at all, never as a
/// mangled one — the operator would rather have no ident on the end than a
/// wrong one.
#[test]
fn an_unsendable_callsign_sends_nothing_rather_than_something_wrong() {
    let events = transmit_and_receive(SstvMode::Martin2, Some("K9"));
    assert_eq!(decoded_id(&events), None);
}
