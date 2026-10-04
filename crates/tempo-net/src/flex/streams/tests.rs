//! The broker's rules, the DAX RX decoder and the DAX TX packet format. Nexus's own tests: upstream
//! tests the broker only through its consumers.

use super::*;

const RX: Holder = Holder(0);
const MONITOR: Holder = Holder(5);

fn created(actions: &[Action]) -> Vec<u8> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Create { channel } => Some(*channel),
            Action::Remove { .. } => None,
        })
        .collect()
}

#[test]
fn the_first_holder_asks_for_the_stream_and_later_holders_share_it() {
    let mut b = DaxBroker::new();
    assert_eq!(
        b.acquire(1, RX),
        (None, Some(Action::Create { channel: 1 }))
    );
    // A second holder, and the same holder again: no second create while one is in flight.
    assert_eq!(b.acquire(1, MONITOR), (None, None));
    assert_eq!(b.acquire(1, RX), (None, None));
    b.registered(0x0400_0001, 1, 0);
    assert_eq!(b.acquire(1, RX), (Some(0x0400_0001), None));
    assert_eq!(b.channel_of(0x0400_0001), Some(1));
    assert_eq!(b.stream_of(1), Some(0x0400_0001));
    assert!(b.held_by(1, RX) && b.held_by(1, MONITOR));
    // Held: nothing is ever due.
    assert!(b.poll(60_000).is_empty());
}

#[test]
fn the_last_release_removes_the_stream_only_after_the_grace_window() {
    let mut b = DaxBroker::new();
    b.acquire(2, RX);
    b.registered(0x0400_0002, 2, 0);
    b.release(2, RX, 10_000);
    assert!(
        b.poll(10_000 + REMOVAL_GRACE_MS - 1).is_empty(),
        "inside the grace window"
    );
    assert_eq!(
        b.poll(10_000 + REMOVAL_GRACE_MS),
        [Action::Remove {
            stream: 0x0400_0002,
            channel: 2
        }]
    );
    assert_eq!(
        b.channel_of(0x0400_0002),
        None,
        "our own removal forgets the stream first"
    );
    assert!(b.snapshot().is_empty());
    // The radio's removal status that follows changes nothing: no recreate for our own removal.
    b.unregistered(0x0400_0002, 12_000);
    assert!(b.poll(20_000).is_empty());
}

#[test]
fn a_reacquire_inside_the_grace_window_keeps_the_stream() {
    let mut b = DaxBroker::new();
    b.acquire(1, RX);
    b.registered(0x0400_0001, 1, 0);
    b.release(1, RX, 1_000);
    // The radio's transient rebind (or the consumer coming straight back) inside the window.
    assert_eq!(b.acquire(1, RX), (Some(0x0400_0001), None));
    assert!(b.poll(1_000 + REMOVAL_GRACE_MS).is_empty());
    assert!(b.poll(60_000).is_empty());
    assert_eq!(b.stream_of(1), Some(0x0400_0001));
}

#[test]
fn a_release_while_the_create_is_in_flight_makes_one_removal_and_no_churn() {
    let mut b = DaxBroker::new();
    b.acquire(3, RX);
    b.release(3, RX, 100);
    // The entry stays for the stream that is coming.
    assert!(b.snapshot()[0].create_pending);
    b.registered(0x0400_0003, 3, 200);
    assert!(b.poll(200 + REMOVAL_GRACE_MS - 1).is_empty());
    assert_eq!(
        b.poll(200 + REMOVAL_GRACE_MS),
        [Action::Remove {
            stream: 0x0400_0003,
            channel: 3
        }]
    );
    assert!(
        b.poll(60_000).is_empty(),
        "exactly one removal, no create after it"
    );
}

#[test]
fn a_stream_the_radio_removed_while_held_comes_back_after_the_recreate_delay() {
    let mut b = DaxBroker::new();
    b.acquire(1, RX);
    b.registered(0x0400_0001, 1, 0);
    b.unregistered(0x0400_0001, 5_000);
    assert_eq!(b.stream_of(1), None);
    assert!(b.poll(5_000 + RECREATE_DELAY_MS - 1).is_empty());
    assert_eq!(created(&b.poll(5_000 + RECREATE_DELAY_MS)), [1]);
    // Once: the create is now in flight.
    assert!(b.poll(60_000).is_empty());
    b.registered(0x0400_0011, 1, 6_000);
    assert_eq!(b.acquire(1, RX).0, Some(0x0400_0011));
}

#[test]
fn a_stream_the_radio_removed_that_nobody_holds_is_forgotten() {
    let mut b = DaxBroker::new();
    b.acquire(1, RX);
    b.registered(0x0400_0001, 1, 0);
    b.release(1, RX, 100);
    b.unregistered(0x0400_0001, 200);
    assert!(b.snapshot().is_empty());
    assert!(
        b.poll(60_000).is_empty(),
        "no recreate, and the pending removal is void"
    );
}

#[test]
fn a_failed_create_is_retried_while_held_and_dropped_when_not() {
    let mut b = DaxBroker::new();
    b.acquire(4, RX);
    b.create_failed(4, 1_000);
    assert!(!b.snapshot()[0].create_pending, "the latch clears");
    assert!(b.poll(1_000 + CREATE_RETRY_MS - 1).is_empty());
    assert_eq!(created(&b.poll(1_000 + CREATE_RETRY_MS)), [4]);
    // A persistent refusal costs one create per retry period, no more.
    b.create_failed(4, 3_100);
    assert!(b.poll(3_100 + CREATE_RETRY_MS - 1).is_empty());
    assert_eq!(created(&b.poll(3_100 + CREATE_RETRY_MS)), [4]);
    // Released while the retry is pending: the retry is void, and a failure then drops the entry.
    b.create_failed(4, 6_000);
    b.release(4, RX, 6_100);
    assert!(b.poll(60_000).is_empty());
    assert!(b.snapshot().is_empty());
}

#[test]
fn a_create_failure_for_a_channel_with_a_stream_changes_nothing() {
    let mut b = DaxBroker::new();
    b.acquire(1, RX);
    b.registered(0x0400_0001, 1, 0);
    b.create_failed(1, 100);
    assert_eq!(b.stream_of(1), Some(0x0400_0001));
    assert!(b.poll(60_000).is_empty());
}

#[test]
fn one_stream_per_channel_the_newer_id_wins() {
    let mut b = DaxBroker::new();
    b.acquire(1, RX);
    b.registered(0x0400_0001, 1, 0);
    b.registered(0x0400_0009, 1, 10);
    assert_eq!(
        b.channel_of(0x0400_0001),
        None,
        "the stale id no longer routes audio"
    );
    assert_eq!(b.channel_of(0x0400_0009), Some(1));
    // Its status repeated: an echo moves nothing.
    b.registered(0x0400_0009, 1, 20);
    assert!(b.poll(60_000).is_empty());
}

#[test]
fn a_disconnect_forgets_everything_and_sends_nothing() {
    let mut b = DaxBroker::new();
    b.acquire(1, RX);
    b.registered(0x0400_0001, 1, 0);
    b.acquire(2, RX);
    b.create_failed(2, 0);
    b.release(1, RX, 0);
    b.reset_for_disconnect();
    assert!(b.snapshot().is_empty());
    assert_eq!(b.channel_of(0x0400_0001), None);
    assert!(b.poll(60_000).is_empty(), "every timer is void");
    assert_eq!(b.next_due(), None);
    // A new session starts from nothing.
    assert_eq!(
        b.acquire(1, RX),
        (None, Some(Action::Create { channel: 1 }))
    );
}

#[test]
fn channels_outside_one_to_eight_and_stream_zero_are_ignored() {
    let mut b = DaxBroker::new();
    assert_eq!(b.acquire(0, RX), (None, None));
    assert_eq!(b.acquire(9, RX), (None, None));
    b.registered(0x0400_0001, 9, 0);
    b.registered(0, 1, 0);
    b.unregistered(0, 0);
    assert!(b.snapshot().is_empty());
    assert_eq!(
        b.acquire(8, RX),
        (None, Some(Action::Create { channel: 8 }))
    );
}

#[test]
fn release_all_covers_every_channel_upstreams_stopped_at_four() {
    let mut b = DaxBroker::new();
    for ch in [2, 6, 8] {
        b.acquire(ch, RX);
        b.registered(0x0400_0000 | u32::from(ch), ch, 0);
    }
    b.acquire(6, MONITOR);
    b.release_all(RX, 100);
    let removed: Vec<_> = b.poll(100 + REMOVAL_GRACE_MS);
    assert_eq!(
        removed,
        [
            Action::Remove {
                stream: 0x0400_0002,
                channel: 2
            },
            Action::Remove {
                stream: 0x0400_0008,
                channel: 8
            },
        ],
        "channel 6 is still held by another consumer"
    );
    assert!(b.held_by(6, MONITOR));
}

#[test]
fn next_due_names_the_earliest_timer() {
    let mut b = DaxBroker::new();
    assert_eq!(b.next_due(), None);
    b.acquire(1, RX);
    b.create_failed(1, 1_000);
    b.acquire(2, RX);
    b.registered(0x0400_0002, 2, 0);
    b.release(2, RX, 500);
    assert_eq!(b.next_due(), Some(500 + REMOVAL_GRACE_MS));
}

// ── DAX RX decode ──

fn be_f32(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

#[test]
fn float32_stereo_decodes_as_sent() {
    let samples = [0.5f32, -0.25, 0.125, 1.0];
    assert_eq!(
        decode_dax_audio(AUDIO_CLASS, &be_f32(&samples)),
        Some(samples.to_vec())
    );
}

#[test]
fn int16_mono_goes_to_both_sides() {
    let payload: Vec<u8> = [16_384i16, -32_768]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();
    assert_eq!(
        decode_dax_audio(AUDIO_REDUCED_CLASS, &payload),
        Some(vec![0.5, 0.5, -1.0, -1.0])
    );
}

#[test]
fn a_partial_or_poisoned_audio_packet_is_refused_whole() {
    // Not a whole stereo frame, too short, odd int16, a NaN, an infinity, another class.
    assert_eq!(decode_dax_audio(AUDIO_CLASS, &[0u8; 12]), None);
    assert_eq!(decode_dax_audio(AUDIO_CLASS, &[0u8; 4]), None);
    assert_eq!(decode_dax_audio(AUDIO_CLASS, &[]), None);
    assert_eq!(decode_dax_audio(AUDIO_REDUCED_CLASS, &[0u8; 3]), None);
    assert_eq!(decode_dax_audio(AUDIO_REDUCED_CLASS, &[]), None);
    assert_eq!(
        decode_dax_audio(AUDIO_CLASS, &be_f32(&[0.1, f32::NAN])),
        None
    );
    assert_eq!(
        decode_dax_audio(AUDIO_CLASS, &be_f32(&[f32::INFINITY, 0.1])),
        None
    );
    assert_eq!(decode_dax_audio(0x8003, &be_f32(&[0.1, 0.1])), None);
    // Control: the same frame, finite, decodes.
    assert!(decode_dax_audio(AUDIO_CLASS, &be_f32(&[0.1, 0.2])).is_some());
}

#[test]
fn the_trailer_comes_off_only_when_flagged() {
    let p = [1u8, 2, 3, 4, 5, 6, 7, 8];
    assert_eq!(without_trailer(&p, true), Some(&p[..4]));
    assert_eq!(without_trailer(&p, false), Some(&p[..]));
    assert_eq!(without_trailer(&p[..3], true), None);
}

// ── DAX TX packets ──

#[test]
fn a_dax_tx_packet_is_the_tested_format_byte_for_byte() {
    let samples: Vec<f32> = (0..TX_FRAMES_PER_PACKET * 2)
        .map(|i| i as f32 / 1000.0)
        .collect();
    let p = dax_tx_packet(0x8400_0000, 0x1F, &samples).unwrap();
    // 7 header words and 256 payload words.
    assert_eq!(p.len(), (7 + 256) * 4);
    // Type 1, class id present, no trailer, TSI 3, TSF 1, count 0xF (masked to four bits), size
    // 263 words.
    assert_eq!(&p[0..4], &[0x18, 0xDF, 0x01, 0x07]);
    assert_eq!(&p[4..8], &0x8400_0000u32.to_be_bytes());
    // The FlexRadio OUI, right-justified in the class id's first word.
    assert_eq!(&p[8..12], &[0x00, 0x00, 0x1C, 0x2D]);
    // Information class "SL", packet class 0x03E3: float32 stereo, not the untested int16 mono.
    assert_eq!(&p[12..16], &[0x53, 0x4C, 0x03, 0xE3]);
    assert_eq!(&p[16..28], &[0u8; 12], "timestamps are zero");
    assert_eq!(&p[28..32], &samples[0].to_be_bytes());
    assert_eq!(&p[p.len() - 4..], &samples[samples.len() - 1].to_be_bytes());
}

#[test]
fn a_dax_tx_packet_reads_back_through_the_receive_side() {
    let samples: Vec<f32> = (0..TX_FRAMES_PER_PACKET * 2)
        .map(|i| ((i as f32) * 0.37).sin() * 0.5)
        .collect();
    let p = dax_tx_packet(0x8400_0000, 6, &samples).unwrap();
    let v = crate::flexvita::parse_vita(&p).expect("a well-formed VITA-49 packet");
    assert_eq!(v.packet_type, 1);
    assert_eq!(v.stream_id, Some(0x8400_0000));
    assert_eq!(v.class_oui, Some(0x001C2D));
    assert_eq!(v.packet_class, Some(AUDIO_CLASS));
    assert_eq!(v.packet_count, 6);
    assert!(!v.has_trailer);
    assert_eq!(decode_dax_audio(AUDIO_CLASS, v.payload), Some(samples));
}

#[test]
fn the_count_wraps_at_sixteen_and_odd_or_oversized_audio_is_refused() {
    let frame = [0.0f32; 2];
    let count = |c: u8| (dax_tx_packet(1, c, &frame).unwrap()[1] & 0x0F) as u8;
    assert_eq!(count(15), 15);
    assert_eq!(count(16), 0);
    assert_eq!(dax_tx_packet(1, 0, &[0.0; 3]), None);
    assert_eq!(dax_tx_packet(1, 0, &vec![0.0; 70_000]), None);
}

#[test]
fn dax_tx_goes_to_the_vita_port_not_4993() {
    // The radio's VITA-49 receive port (FlexRadio's API documentation; AetherSDR sends there).
    assert_eq!(VITA_PORT, 4991);
    assert_eq!(UDP_REGISTRATION_PORT, 4992);
    assert_eq!(DAX_RATE_HZ, 24_000);
}

/// What a source line, its comment cut off, says about a DAX sender: `Some(why)` for one of the
/// two marks the retired sender left.
fn retired_sender_mark(line: &str) -> Option<&'static str> {
    let code = line.split("//").next().unwrap_or("");
    let port = code.match_indices("4993").any(|(i, m)| {
        let digit = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        !digit(code[..i].chars().next_back()) && !digit(code[i + m.len()..].chars().next())
    });
    if port {
        return Some("names UDP 4993");
    }
    let int16_class = code.contains("REDUCED_CLASS") || code.contains("0x0123");
    (int16_class && code.contains("<< 16"))
        .then_some("builds a VITA class word from the int16 class")
}

/// Every `.rs` file under `dir`, test files and directories left out.
fn rust_sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "target" && name != "tests" && name != "node_modules" {
                rust_sources(&path, out);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" {
            out.push(path);
        }
    }
}

/// ⭐ NO DAX SENDER TO 4993, OR IN INT16, COMES BACK (operator ruling, 2026-10-04, "Retire it").
/// The older native audio worker sent DAX TX as int16 mono (`0x0123`, a format no tested client
/// sends) to UDP 4993, where an over keyed with no audio. It is retired: the only DAX TX Nexus
/// sends is [`dax_tx_packet`]'s float32 stereo, to [`VITA_PORT`]. Every Rust source of the
/// workspace and the Tauri shell is scanned, test code left out (a file's code after its first
/// `#[cfg(test)]`, test files, `tests/`), for the two marks that sender left. A merge that brings
/// it back, or a new sender of its shape, fails here.
#[test]
fn no_dax_sender_to_4993_or_in_int16_remains() {
    // The checker, both ways: the retired sender's own lines trip it, the live sender's do not,
    // and neither does prose about the old port.
    assert!(retired_sender_mark("const FLEX_VITA_PORT: u16 = 4993;").is_some());
    assert!(retired_sender_mark(
        "    let class_word = (u32::from(FLEX_INFO_CLASS) << 16) | u32::from(DAX_AUDIO_REDUCED_CLASS);"
    )
    .is_some());
    assert!(retired_sender_mark("pub const VITA_PORT: u16 = 4991;").is_none());
    assert!(retired_sender_mark(
        "    out.extend_from_slice(&((FLEX_INFO_CLASS << 16) | u32::from(AUDIO_CLASS)).to_be_bytes());"
    )
    .is_none());
    assert!(
        retired_sender_mark("//! the older sender used 4993, where an over keyed silent").is_none()
    );
    assert!(retired_sender_mark("let port = 49930;").is_none());

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in ["crates", "src-tauri/src"] {
        rust_sources(&root.join(dir), &mut files);
    }
    let mut marks = Vec::new();
    let mut live_port = 0;
    for file in &files {
        let text = std::fs::read_to_string(file).expect("a source file reads");
        for (n, line) in text.lines().enumerate() {
            if line.trim() == "#[cfg(test)]" {
                break;
            }
            if line.split("//").next().unwrap_or("").contains("4991") {
                live_port += 1;
            }
            if let Some(why) = retired_sender_mark(line) {
                let at = file.strip_prefix(&root).unwrap_or(file).display();
                marks.push(format!("{at}:{}: {why}: {}", n + 1, line.trim()));
            }
        }
    }
    // The scan read the real tree: hundreds of files, and the live sender's port among them.
    assert!(files.len() > 300, "only {} source files found", files.len());
    assert!(
        live_port > 0,
        "the scan never saw the radio's VITA port, 4991"
    );
    assert!(
        marks.is_empty(),
        "a retired DAX sender is back:\n{}",
        marks.join("\n")
    );
}
