//! ⭐ A HASHED CALL THIS STATION SENDS STILL RESOLVES WHEN ITS HASH COMES BACK, with the lock-free
//! encoder on the transmit path.
//!
//! The vendored encoder stores every call it packs in the table the decoder reads to print a
//! hashed `<call>`: that is how a reply to a nonstandard call decodes as `<PJ4/K1ABC> …` rather
//! than `<...> …`, and how the sequencer then sees it is addressed to this station. The Rust
//! encoder writes no table; the vendored one runs on the message afterwards
//! (`ft8::encode_later`), at once when the modem is free and otherwise first thing in the next
//! modem call. These scenes check, by the decoder's own output, that the call is in the table
//! when the reply is decoded, including when the over was encoded while a decode held the modem.
//!
//! Its own test binary: the table is the whole process's, and another test's encodes could
//! overwrite a 22-bit entry these scenes read.

use std::sync::mpsc;
use std::time::Duration;

use modes::wsjt77::ft8_tones;
use modes::{make_mode, Ft8A7ResetGuard, ModeKind};

/// The decoded lines of a reply `text`, built with the pure Rust encoder (which writes no table)
/// and decoded by the vendored decoder, which prints a hash from the table.
fn decode_reply(text: &str) -> Vec<String> {
    let mode = make_mode(ModeKind::Ft8);
    let wave = mode.gen_wave(&ft8_tones(text).unwrap(), 12_000.0, 1_500.0);
    let mut frame = vec![0i16; ft8::NMAX];
    for (s, &x) in frame.iter_mut().zip(&wave) {
        *s = (x * 1_000.0) as i16;
    }
    ft8::decode_frame(&frame, 200, 3_000, 3, "", "", 0, 0, false, false, false)
        .into_iter()
        .map(|d| d.message.trim().to_string())
        .collect()
}

/// A decode in flight: holds the modem until told to let go.
fn a_decode_holding_the_modem() -> (mpsc::Sender<()>, std::thread::JoinHandle<()>) {
    let (holding, held) = mpsc::channel();
    let (go, wait) = mpsc::channel::<()>();
    let decode = std::thread::spawn(move || {
        let modem = loop {
            match Ft8A7ResetGuard::try_acquire() {
                Some(modem) => break modem,
                None => std::thread::sleep(Duration::from_millis(1)),
            }
        };
        holding.send(()).unwrap();
        let _ = wait.recv_timeout(Duration::from_secs(30));
        drop(modem);
    });
    held.recv().unwrap();
    (go, decode)
}

/// Encode `text` as the transmit path does, on another thread; its tones, or `None` when it has
/// not finished within `limit` (it waited for the modem).
fn encode_within(text: &str, limit: Duration) -> Option<Vec<i32>> {
    let (done, finished) = mpsc::channel();
    let text = text.to_string();
    std::thread::spawn(move || {
        let _ = done.send(make_mode(ModeKind::Ft8).encode(&text));
    });
    finished.recv_timeout(limit).ok()
}

/// ⭐ Sent while a decode holds the modem: made at once, and the call is in the table for the next
/// decode. The control call, never sent, comes back as `<...>`.
#[test]
fn a_hashed_call_sent_during_a_decode_resolves_when_its_reply_is_decoded() {
    let (go, decode) = a_decode_holding_the_modem();
    let tones = encode_within("CQ ZK3/W9QZT", Duration::from_secs(5));
    let _ = go.send(());
    decode.join().unwrap();
    let tones = tones.expect("the over waited for the decode holding the modem");
    // Not compared with `ft8::encode` here: that would run the vendored encoder, which writes the
    // table itself and would make the scene pass with no hand-off at all (the golden test compares
    // the tones).
    assert!(
        tones.len() == 79 && tones.iter().any(|&t| t != 0),
        "the over packs"
    );
    assert_eq!(
        decode_reply("<ZK3/W9QZT> K1ABC -10"),
        ["<ZK3/W9QZT> K1ABC -10"],
        "a reply to the hash of the call this station sent"
    );
    assert_eq!(
        decode_reply("<ZK3/W9QZX> K1ABC -10"),
        ["<...> K1ABC -10"],
        "control: a reply to the hash of a call never sent"
    );
}

/// Sent with no decode running: the call is in the table for the next reader of it, here the
/// vendored unpacker.
#[test]
fn a_hashed_call_sent_with_no_decode_running_is_in_the_table_for_the_next_reader() {
    let tones = make_mode(ModeKind::Ft8).encode("<W9XYZ> VP2V/K1QQB RR73");
    assert!(tones.iter().any(|&t| t != 0), "the over packs");
    // Read straight back through the vendored unpacker, which consults the table.
    let bits = |text: &str| -> Vec<i8> {
        let tones = ft8_tones(text).unwrap();
        // genft8's Gray map, undone: data symbols 8..36 and 44..72 carry the first 87 bits.
        const GRAY: [i32; 8] = [0, 1, 3, 2, 5, 6, 4, 7];
        let data = tones[7..36].iter().chain(&tones[43..72]);
        data.flat_map(|&t| {
            let v = GRAY.iter().position(|&g| g == t).unwrap() as i8;
            [v >> 2 & 1, v >> 1 & 1, v & 1]
        })
        .take(77)
        .collect()
    };
    assert_eq!(
        tempo_fast::unpack77(&bits("<VP2V/K1QQB> K1ABC R-07")).as_deref(),
        Some("<VP2V/K1QQB> K1ABC R-07"),
        "the call this station sent resolves"
    );
    assert_eq!(
        tempo_fast::unpack77(&bits("<VP2V/K1QQX> K1ABC R-07")).as_deref(),
        Some("<...> K1ABC R-07"),
        "control: a call never sent does not"
    );
}
