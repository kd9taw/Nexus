//! Golden test of the JS8 frame codec against JS8Call's own ALL.TXT (sanitised, Task B4.1).
//! Task B4.1 lands only the fixture-reader gate; Task B4.4 adds the codec assertions.
mod common;

use common::golden::{read_directed, read_frames};
use js8::Speed;

#[test]
fn frames_fixture_reads_with_the_expected_shape() {
    let frames = read_frames();
    // The oracle cannot be silently truncated: floors per speed as measured 2026-09-05
    // (965 A / 341 B / 27 E). The pin in SHA256SUMS is the exact truth.
    let n = |s: Speed| frames.iter().filter(|f| f.speed == s).count();
    assert!(frames.len() >= 1333, "only {} frames", frames.len());
    assert!(
        n(Speed::Normal) >= 965 && n(Speed::Fast) >= 341 && n(Speed::Slow) >= 27,
        "A={} B={} E={}",
        n(Speed::Normal),
        n(Speed::Fast),
        n(Speed::Slow)
    );
    assert_eq!(
        n(Speed::Turbo),
        0,
        "no Turbo line existed in the source log; a Turbo row means a different log was sanitised"
    );
    assert_eq!(frames[0].at_ms, 0);
    assert!(
        frames.last().unwrap().at_ms > 24 * 3600 * 1000,
        "the clock lines did not accumulate"
    );
    let anchor = frames
        .iter()
        .find(|f| &f.sixbit == b"2Wu+WEWCuiZG")
        .expect("the KD2UWR heartbeat anchor");
    assert_eq!(anchor.i3, 3);
    assert_eq!(anchor.text, "KD2UWR: @HB HEARTBEAT FN30 ");
    let trailing = frames.iter().filter(|f| f.text.ends_with(' ')).count();
    assert!(
        trailing > 500,
        "trailing spaces were stripped somewhere: {trailing}"
    );
}

#[test]
fn directed_fixture_reads_with_the_expected_shape() {
    let rows = read_directed();
    assert!(rows.len() >= 728, "only {} directed lines", rows.len());
    assert!(rows
        .iter()
        .all(|r| !r.text.contains('\u{2662}') && !r.text.ends_with(' ')));
    assert!(rows.iter().any(|r| r.text == "KD2UWR: @HB HEARTBEAT"));
    assert!(rows.iter().any(|r| r.text.contains(" HEARTBEAT SNR ")));
}

#[test]
fn jsc_blob_matches_its_sha256_pin() {
    let pin = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/proto/jsc/dict.sha256"
    ))
    .unwrap();
    let canonical = js8::proto::jsc::canonical_bytes();
    assert_eq!(common::hex(&common::sha256(&canonical)), pin.trim(), "dict.bin does not inflate to the pinned canonical table — regenerate with scripts/gen-js8-jsc-dict.mjs");
    assert_eq!(
        pin.trim(),
        "146c354f9804e48c68d9ae45d6836fbb6228178fd080fe8176ba9bdba7f201a4",
        "the pin itself moved: upstream jsc_list.cpp changed under the pinned commit?"
    );
}

use js8::proto::alphabet::{sixbit_from_str, sixbit_to_string};
use js8::proto::frame::{decode_word, encode_frame, Frame};
use js8::{Payload72, Word87, I3};
use std::collections::BTreeMap;

fn kind(f: &Frame) -> &'static str {
    match f {
        Frame::Heartbeat { .. } => "hb",
        Frame::Compound { .. } => "compound",
        Frame::CompoundDirected { .. } => "compdir",
        Frame::Directed { .. } => "directed",
        Frame::Data { dense: true, .. } => "data72",
        Frame::Data { dense: false, .. } => "data70",
    }
}

/// THE oracle for the payload layout: every decode line JS8Call ever logged on this box must
/// unpack to the line JS8Call rendered, byte for byte, and pack back to the same 12 chars.
/// Round-tripping proves the ENCODER for every frame type actually seen on the air — HB, the
/// CQ variants, HEARTBEAT SNR acks, STATUS/QSL/RR/73/ACK/HW CPY?, relay '>', compound calls,
/// compound-directed SNR?, Huffman and JSC 70-bit data, and 72-bit dense data.
#[test]
fn every_logged_frame_unpacks_renders_and_repacks_byte_exactly() {
    let frames = read_frames();
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    for (n, g) in frames.iter().enumerate() {
        let vals = sixbit_from_str(g.sixbit_str())
            .unwrap_or_else(|| panic!("fixture row {n}: {} is not sixbit", g.sixbit_str()));
        let payload = Payload72::from_chars12(vals);
        let i3 = I3::from_u8(g.i3);
        match Frame::unpack(&payload, i3, g.speed) {
            Ok(frame) => {
                let rendered = frame.render();
                if rendered != g.text {
                    failures.push(format!(
                        "row {n} {} i3={}: rendered {rendered:?}, JS8Call logged {:?}",
                        g.sixbit_str(),
                        g.i3,
                        g.text
                    ));
                    continue;
                }
                match frame.pack(g.speed) {
                    Ok((repacked, i3b)) => {
                        if repacked.chars12() != vals {
                            failures.push(format!(
                                "row {n}: {:?} repacked to {} not {}",
                                frame,
                                sixbit_to_string(&repacked.chars12()),
                                g.sixbit_str()
                            ));
                        }
                        if i3b.data != i3.data {
                            failures.push(format!(
                                "row {n}: pack chose data={} but the log says {}",
                                i3b.data, i3.data
                            ));
                        }
                    }
                    Err(e) => failures.push(format!("row {n}: {:?} does not pack: {e:?}", frame)),
                }
                *kinds.entry(kind(&frame)).or_default() += 1;
            }
            Err(e) => {
                // JS8Call prints the raw 12 chars when nothing unpacks; we must agree on that too.
                if g.text != sixbit_to_string(&vals) {
                    failures.push(format!(
                        "row {n} {} i3={}: we fail with {e:?} but JS8Call rendered {:?}",
                        g.sixbit_str(),
                        g.i3,
                        g.text
                    ));
                } else {
                    *kinds.entry("undecodable").or_default() += 1;
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} golden frames failed:\n{}",
        failures.len(),
        frames.len(),
        failures.join("\n")
    );
    // Coverage floors, measured 2026-09-05: data70 362 (34 Huffman + 328 JSC), data72 179,
    // hb 158, directed 549, compound 41, compdir 44. A regenerated fixture may only grow them.
    let at = |k: &str| kinds.get(k).copied().unwrap_or(0);
    assert!(
        at("data70") >= 362
            && at("data72") >= 179
            && at("hb") >= 158
            && at("directed") >= 549
            && at("compound") >= 41
            && at("compdir") >= 44,
        "coverage collapsed: {kinds:?}"
    );
    eprintln!("golden frame kinds: {kinds:?}");
}

/// The seam: pack → Word87 → verify → unpack is the identity, and a CRC is stamped.
#[test]
fn encode_frame_then_decode_word_is_the_identity_on_every_golden_frame() {
    for g in read_frames() {
        let vals = sixbit_from_str(g.sixbit_str()).unwrap();
        let i3 = I3::from_u8(g.i3);
        let Ok(frame) = Frame::unpack(&Payload72::from_chars12(vals), i3, g.speed) else {
            continue;
        };
        let word = encode_frame(&frame, i3, g.speed).unwrap();
        assert!(word.verify(), "{frame:?}");
        assert_eq!(word.payload72().chars12(), vals);
        assert_eq!(
            word.i3(),
            I3 {
                data: i3.data,
                ..i3
            }
        );
        let (back, i3_back) = decode_word(&word, g.speed).unwrap();
        assert_eq!(back, frame);
        assert_eq!(i3_back, i3);
        // A flipped CRC bit is refused at the ONLY raw → Frame entry.
        let mut bytes = *word.as_bytes();
        bytes[10] ^= 0x10; // bit 83, inside the CRC field
        assert!(matches!(
            decode_word(&Word87::from_bytes(bytes), g.speed),
            Err(js8::proto::frame::FrameError::Crc)
        ));
    }
}
