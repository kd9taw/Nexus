//! Golden test of the heartbeat acknowledgement (`HEARTBEAT SNR`) against JS8Call's own ALL.TXT
//! (sanitised, Task B4.1's fixture).
//!
//! The log holds the HB-ACKs JS8Call stations sent to the heartbeats they heard. JS8Call answers
//! the station as it was heard (`sendHeartbeatAck(d.from, d.snr, extra)`, mainwindow.cpp:9098), so
//! a `/P` call keeps its `/P`, which is a flag in the directed frame. For every logged HB-ACK that
//! is one frame (`ACKER: TARGET HEARTBEAT SNR ±NN`, first and last), a station that is that
//! ACKer, with HB, autoreply and HB-ACK on, hearing TARGET's heartbeat at that SNR, keys that frame
//! bit for bit. A compound call on either side takes two frames, the ACKer's announcement and then
//! the directed part. The log's directed parts name no sender, so for those a station with a
//! compound call of its own answers, and its second frame is the one compared.
mod common;

use common::golden::read_frames;
use js8::proto::callsign::CallRef;
use js8::proto::reassembly::{Checksum, Message};
use js8::{MessageEvent, Origin, Payload72, Speed, Station, StationConfig, TxFrame, Word87, I3};

/// `TARGET`'s heartbeat as it reaches the station, heard at `snr`.
fn heartbeat(target: &str, snr: i32, speed: Speed) -> MessageEvent {
    MessageEvent::Message(Message {
        from: target.to_string(),
        to: CallRef::parse("@HB"),
        to_text: "@HB".into(),
        cmd: None,
        num: None,
        text: String::new(),
        checksum: Checksum::NotRequired,
        path: Vec::new(),
        freq_hz: 1500.0,
        snr_db: snr,
        speed,
        first_ms: 0,
        last_ms: 0,
        frames: 1,
        complete: true,
        compound_from: None,
        grid: None,
        cq: None,
    })
}

/// Every frame a station that is `acker`, at `speed`, keys in answer to `target`'s heartbeat.
/// Its own heartbeat is 30 minutes away, so nothing else goes out meanwhile.
fn hb_ack_keyed_by(acker: &str, target: &str, snr: i32, speed: Speed) -> Vec<TxFrame> {
    let mut s = Station::new(StationConfig {
        mycall: acker.to_string(),
        grid: "EN52".into(),
        speed,
        hb_ack: true,
        hb_interval_min: 30,
        // The logged replies keyed by themselves, the period after what they answer: those
        // stations did not ask first (JS8Call's AutoreplyConfirmation off).
        autoreply_confirmation: false,
        ..StationConfig::default()
    });
    s.set_hb(true, 0);
    s.on_event(&heartbeat(target, snr, speed), 0);
    s.process_tx_queue(); // JS8Call's processTxQueue, within the second
    let mut rng = || 0u32;
    (0..2u64)
        .filter_map(|k| s.next_frame(60_000 + k * 15_000, &|_| false, &mut rng))
        .collect()
}

#[test]
fn every_logged_hb_ack_is_the_frame_the_station_keys() {
    let (mut single, mut portable, mut two_frame) = (0, 0, 0);
    for g in &read_frames() {
        let Some((head, snr_text)) = g.text.trim_end().rsplit_once(" HEARTBEAT SNR ") else {
            continue;
        };
        let Ok(snr) = snr_text.parse::<i32>() else {
            continue;
        };
        let vals = js8::proto::alphabet::sixbit_from_str(g.sixbit_str()).unwrap();
        let word = Word87::new(Payload72::from_chars12(vals), I3::from_u8(g.i3));
        match (g.i3, head.split_once(": ")) {
            (3, Some((acker, target))) => {
                let tx = hb_ack_keyed_by(acker, target, snr, g.speed);
                assert_eq!(tx.len(), 1, "{acker}'s HB-ACK to {target} is one frame");
                assert_eq!(
                    tx[0].word, word,
                    "{acker}'s HB-ACK to {target} is not the frame JS8Call keyed"
                );
                assert_eq!(
                    (tx[0].origin, tx[0].display.as_str()),
                    (
                        Origin::HbAck,
                        format!("{acker}: {target} HEARTBEAT SNR {snr_text}").as_str()
                    )
                );
                single += 1;
                portable += usize::from(target.ends_with("/P"));
            }
            // A directed part, the second of two frames: any compound-call ACKer sends this one.
            (2, None) if !head.starts_with('@') => {
                let tx = hb_ack_keyed_by("K1ABC/QRP", head, snr, g.speed);
                assert_eq!(tx.len(), 2, "an HB-ACK from a compound call is two frames");
                assert_eq!(
                    tx[1].word, word,
                    "the HB-ACK to {head} is not the frame JS8Call keyed"
                );
                two_frame += 1;
            }
            _ => {}
        }
    }
    assert_eq!(
        (single, portable, two_frame),
        (211, 2, 17),
        "the log's single-frame HB-ACKs (two to a /P call) and two-frame directed parts"
    );
}
