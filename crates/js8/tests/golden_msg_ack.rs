//! Golden test of the MSG ACK against JS8Call's own ALL.TXT (sanitised, Task B4.1's fixture).
//!
//! The log holds two MSGs sent to the @SITREP group, each followed by the ACK of every station
//! that answered it: thirteen for KJ5MIW's, one for KD8NOA's. Each logged ACK is the frame a
//! JS8Call station keyed, as another station decoded it. For each one, the MSG is reassembled
//! from the log's own frames and handed to a station that is that ACKer (joined to @SITREP, at
//! the speed it answered at, autoreply on as JS8Call ships it), and the frame the station keys
//! must be the logged frame, bit for bit: JS8Call's reply text (`"%1 ACK"`, mainwindow.cpp:9139),
//! the frames the composer builds from it, and their i3 flags. The log's other ACKs answer MSGs
//! it never decoded, so they have nothing to be checked against.
mod common;

use common::golden::read_frames;
use js8::proto::reassembly::Message;
use js8::{
    Command, MessageEvent, Origin, Payload72, RawDecode, Reassembler, Speed, Station,
    StationConfig, TxFrame, Word87, I3,
};

/// `ACKER: TARGET ACK ` as JS8Call renders an ACK frame, as (acker, target).
fn logged_ack(text: &str) -> Option<(&str, &str)> {
    let (acker, rest) = text.trim_end().split_once(": ")?;
    let target = rest.strip_suffix(" ACK")?;
    (!target.contains(' ')).then_some((acker, target))
}

/// The first frame a station that is `acker`, at `speed`, keys for `msg`.
fn ack_keyed_by(acker: &str, speed: Speed, msg: &Message) -> Option<TxFrame> {
    let groups = if msg.to_text.starts_with('@') {
        vec![msg.to_text.clone()]
    } else {
        Vec::new()
    };
    let mut s = Station::new(StationConfig {
        mycall: acker.to_string(),
        speed,
        groups,
        // The logged replies keyed by themselves, the period after what they answer: those
        // stations did not ask first (JS8Call's AutoreplyConfirmation off).
        autoreply_confirmation: false,
        ..StationConfig::default()
    });
    s.on_event(&MessageEvent::Message(msg.clone()), 0);
    let mut rng = || 0u32;
    s.next_frame(60_000, &|_| false, &mut rng)
}

#[test]
fn every_logged_msg_ack_is_the_frame_the_station_keys() {
    let mut r = Reassembler::new();
    let mut msgs: Vec<Message> = Vec::new();
    let mut checked: Vec<String> = Vec::new();
    for g in &read_frames() {
        let vals = js8::proto::alphabet::sixbit_from_str(g.sixbit_str()).unwrap();
        let word = Word87::new(Payload72::from_chars12(vals), I3::from_u8(g.i3));
        if let Some((acker, target)) = logged_ack(&g.text) {
            // The MSG it answers: the last one the log decoded from its target.
            if let Some(msg) = msgs.iter().rev().find(|m| m.from == target) {
                let tx = ack_keyed_by(acker, g.speed, msg).unwrap_or_else(|| {
                    panic!("{acker} keys no ACK for {target}'s MSG to {}", msg.to_text)
                });
                assert_eq!(
                    tx.word, word,
                    "{acker}'s ACK to {target} is not the frame JS8Call keyed"
                );
                assert_eq!(
                    (tx.origin, tx.first, tx.last, tx.display.as_str()),
                    (
                        Origin::AutoReply,
                        true,
                        true,
                        format!("{acker}: {target} ACK").as_str()
                    ),
                    "{acker}'s ACK to {target}"
                );
                checked.push(format!("{acker}>{target}"));
            }
        }
        let rx = RawDecode {
            speed: g.speed,
            freq_hz: g.freq_hz,
            dt_s: g.dt_s,
            snr_db: g.snr_db,
            sync: 0.0,
            word,
            nharderrors: 0,
            quality: 1.0,
        };
        let mut events = r.age(g.at_ms);
        events.extend(r.feed(&rx, g.at_ms));
        for ev in events {
            if let MessageEvent::Message(m) = ev {
                if m.cmd == Some(Command::Msg) {
                    msgs.push(m);
                }
            }
        }
    }
    assert_eq!(
        checked.len(),
        14,
        "the log's fourteen answered ACKs, each checked: {checked:?}"
    );
    assert!(
        checked.iter().filter(|c| c.ends_with(">KJ5MIW")).count() == 13
            && checked.contains(&"KF4LXS>KD8NOA".to_string()),
        "the thirteen ACKs of KJ5MIW's MSG and KF4LXS's of KD8NOA's: {checked:?}"
    );
}
