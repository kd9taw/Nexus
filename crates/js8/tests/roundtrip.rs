//! Property identities for the JS8 message layer (proptest, 256 cases each).
//!
//! WHY PROPERTIES ON TOP OF THE GOLDEN: the ALL.TXT oracle covers what was on the air this
//! winter (one group, two /P calls, no numeric non-SNR arguments). These generate the rest of
//! each grammar — every command id, every num, arbitrary base calls and grids, texts over the
//! coder alphabets — and assert the two things a codec must never violate: encode→decode is the
//! identity, and a corrupted word is refused (CRC-12 catches every single-bit flip).
mod common;

use js8::proto::callsign::{is_base_call, pack28, unpack28, CallRef};
use js8::proto::command::Command;
use js8::proto::compose::frames;
use js8::proto::frame::{decode_word, encode_frame, pack_data_prefix, Frame, FrameError};
use js8::proto::jsc;
use js8::proto::reassembly::{MessageEvent, Reassembler};
use js8::RawDecode;
use js8::{Speed, Word87, I3};
use proptest::prelude::*;

/// A CANONICAL base call: base-packable AND stable under `pack28`→`unpack28`. The stability
/// filter drops the aliases the 3DA0/3X prefix workaround folds together (real "3X…"/"3DA0…"
/// calls pack as "Q…"/"3D0…", so the non-canonical spellings "Q…"/"3D0…" unpack back to the
/// canonical form and do NOT round-trip to themselves — a callsign-codec fact, B1 finding (a),
/// not a frame-codec bug). The frame round-trip identity is stated over canonical calls.
fn base_call() -> impl Strategy<Value = String> {
    "[0-9A-Z]?[0-9A-Z][0-9][A-Z]{0,3}".prop_filter("JS8 canonical base-packable", |s| {
        s.len() >= 3
            && is_base_call(s)
            && pack28(&CallRef::Base(s.clone()))
                .is_some_and(|v| unpack28(v) == Some(CallRef::Base(s.clone())))
    })
}

fn grid4() -> impl Strategy<Value = String> {
    "[A-R]{2}[0-9]{2}"
}

fn speed() -> impl Strategy<Value = Speed> {
    prop::sample::select(Speed::ALL.to_vec())
}

fn command() -> impl Strategy<Value = Command> {
    prop::sample::select(Command::ALL.to_vec())
}

fn i3() -> impl Strategy<Value = I3> {
    (any::<bool>(), any::<bool>()).prop_map(|(first, last)| I3 {
        first,
        last,
        data: false,
    })
}

/// Text over what the JSC dictionary certainly covers (uppercase words, digits, punctuation).
fn text() -> impl Strategy<Value = String> {
    "[A-Z0-9 .,?!/+-]{1,40}".prop_filter("not all spaces", |s| !s.trim().is_empty())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn directed_frames_round_trip(from in base_call(), to in base_call(), cmd in command(), num in prop::option::of(-30i8..=31), pf in any::<bool>(), pt in any::<bool>(), i3 in i3(), speed in speed()) {
        let f = Frame::Directed { from: CallRef::Base(from), to: CallRef::Base(to), cmd, num, portable_from: pf, portable_to: pt };
        let w = encode_frame(&f, i3, speed).unwrap();
        prop_assert!(w.verify());
        let (back, i3b) = decode_word(&w, speed).unwrap();
        prop_assert_eq!(back, f);
        prop_assert_eq!(i3b, i3);
    }

    #[test]
    fn heartbeat_and_compound_frames_round_trip(call in base_call(), grid in prop::option::of(grid4()), is_cq in any::<bool>(), idx in 0u8..8, speed in speed()) {
        let hb = Frame::Heartbeat { call: call.clone(), grid: grid.clone(), is_cq, idx };
        let w = encode_frame(&hb, I3 { first: true, last: true, data: false }, speed).unwrap();
        prop_assert_eq!(decode_word(&w, speed).unwrap().0, hb);
        let c = Frame::Compound { call: format!("{call}/P"), grid };
        let w = encode_frame(&c, I3::default(), speed).unwrap();
        prop_assert_eq!(decode_word(&w, speed).unwrap().0, c);
    }

    #[test]
    fn compound_directed_frames_round_trip(call in base_call(), cmd in command(), num in prop::option::of(-30i8..=31), speed in speed()) {
        // Only SNR-type commands carry a number in the compound-directed form (packCmd).
        let num = if cmd.carries_snr() { Some(num.unwrap_or(0)) } else { None };
        let f = Frame::CompoundDirected { call: format!("{call}/QRP"), cmd, num };
        let w = encode_frame(&f, I3::default(), speed).unwrap();
        prop_assert_eq!(decode_word(&w, speed).unwrap().0, f);
    }

    #[test]
    fn data_prefix_round_trips_at_every_speed(t in text(), speed in speed()) {
        let (payload, i3, n) = pack_data_prefix(&t, speed).unwrap();
        prop_assert!(n >= 1 && n <= t.len());
        let back = Frame::unpack(&payload, i3, speed).unwrap();
        let Frame::Data { text: got, dense } = back else { panic!("not data") };
        prop_assert_eq!(dense, speed.uses_fast_data());
        prop_assert_eq!(got.as_str(), &t[..n]);
    }

    #[test]
    fn jsc_compress_decompress_is_the_identity(t in text()) {
        let (bits, n) = jsc::compress(&t, 100_000);
        prop_assert_eq!(n, t.len());
        prop_assert_eq!(jsc::decompress(&bits), t);
    }

    #[test]
    fn crc12_refuses_every_single_bit_flip(from in base_call(), to in base_call(), cmd in command(), bit in 0usize..87, speed in speed()) {
        let f = Frame::Directed { from: CallRef::Base(from), to: CallRef::Base(to), cmd, num: None, portable_from: false, portable_to: false };
        let w = encode_frame(&f, I3 { first: true, last: false, data: false }, speed).unwrap();
        let mut bytes = *w.as_bytes();
        bytes[bit / 8] ^= 0x80 >> (bit % 8);
        let flipped = Word87::from_bytes(bytes);
        prop_assert!(!flipped.verify());
        prop_assert_eq!(decode_word(&flipped, speed).err(), Some(FrameError::Crc));
    }

    /// The reassembly safety property (Task B4.6): compose a multi-frame checksummed message,
    /// then feed its frames with one dropped. A closed message either carries the EXACT body or
    /// is reported incomplete/checksum-bad — a lossy stream never yields confident WRONG text.
    #[test]
    fn reassembly_is_exact_or_incomplete_never_wrong(to in base_call(), body in "[A-Z0-9 ]{3,30}", speed in speed()) {
        let body = body.trim().to_string();
        prop_assume!(!body.is_empty());
        let toref = CallRef::Base(to);
        let Ok(seq) = frames("KD9TAW", Some(&toref), &format!("MSG {body}"), speed) else { return Ok(()); };
        prop_assume!(seq.len() >= 2);
        let period = speed.period_s() as u64 * 1000;
        let feed = |drop_idx: Option<usize>| -> Vec<js8::proto::reassembly::Message> {
            let mut r = Reassembler::new();
            let mut out = Vec::new();
            for (n, (f, i3)) in seq.iter().enumerate() {
                if Some(n) == drop_idx { continue; }
                let word = encode_frame(f, *i3, speed).unwrap();
                let rx = RawDecode { speed, freq_hz: 1500.0, dt_s: 0.0, snr_db: -10, sync: 0.0, word, nharderrors: 0, quality: 1.0 };
                for e in r.feed(&rx, n as u64 * period) { if let MessageEvent::Message(m) = e { out.push(m); } }
            }
            for e in r.age(u64::MAX / 2) { if let MessageEvent::Message(m) = e { out.push(m); } }
            out
        };
        // Whole stream: exactly one complete message with the exact body.
        let whole = feed(None);
        prop_assert_eq!(whole.len(), 1);
        prop_assert!(whole[0].complete && whole[0].text == body, "whole: {:?}", whole[0]);
        // Dropping any one frame: no closed message may claim the wrong body as complete.
        for d in 0..seq.len() {
            for m in feed(Some(d)) {
                if m.complete {
                    prop_assert_eq!(&m.text, &body, "lossy drop {} produced wrong complete text", d);
                }
            }
        }
    }
}
