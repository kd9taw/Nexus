//! ⭐ A NONSTANDARD CALLER'S `<...>` RESOLVES ONCE THE DECODER HAS HEARD HIS CALL IN FULL, from the
//! same messages WSJT-X 3.0.2 learns it from, and from no others.
//!
//! A station with a nonstandard call that answers a CQ sends `K1ABC <PJ4/W8AA> -08`: his own call
//! goes as a 22-bit hash, and a receiver prints it only if its table already holds that call.
//! `packjt77.f90` stores a call (`save_hash_call`) when it unpacks one in full as the SENDER: the
//! second call of a standard message, and the full call of an i3=4 message whose hash is the other
//! one (`iflip=0`: `CQ PJ4/W8AA`, `<K1ABC> PJ4/W8AA`). A full call in the first place of an i3=4
//! message (`iflip=1`) is not stored, in WSJT-X or here. The table is the process's, and lives
//! until the process ends, as the decoder process's does in WSJT-X.
//!
//! Each scene decodes one over from a caller no other scene uses, so the order the scenes run in
//! does not matter. Its own test binary: the table is the whole process's.

use modes::make_mode;
use modes::wsjt77::ft8_tones;
use modes::ModeKind;

/// This station: a standard call, calling CQ.
const MYCALL: &str = "K1ABC";

/// What the decoder prints for one over of `text` (built with the pure Rust encoder, which writes
/// no table), decoded as this station decodes it.
fn hear(text: &str) -> Vec<String> {
    let mode = make_mode(ModeKind::Ft8);
    let wave = mode.gen_wave(&ft8_tones(text).unwrap(), 12_000.0, 1_413.0);
    let mut frame = vec![0i16; ft8::NMAX];
    for (s, &x) in frame.iter_mut().zip(&wave) {
        *s = (x * 1_000.0) as i16;
    }
    ft8::decode_frame(
        &frame, 200, 3_000, 3, MYCALL, "", 0, 1_766, true, false, false,
    )
    .into_iter()
    .map(|d| d.message.trim().to_string())
    .collect()
}

#[test]
fn a_caller_never_heard_in_full_shows_as_unresolved() {
    assert_eq!(hear("K1ABC <PJ4/W8AAA> -08"), ["K1ABC <...> -08"]);
}

#[test]
fn a_caller_whose_cq_was_decoded_resolves() {
    assert_eq!(hear("CQ PJ4/W8BBB"), ["CQ PJ4/W8BBB"]);
    assert_eq!(hear("K1ABC <PJ4/W8BBB> -08"), ["K1ABC <PJ4/W8BBB> -08"]);
}

#[test]
fn a_caller_whose_first_call_to_this_station_was_decoded_resolves() {
    // WSJT-X's Tx1 for a nonstandard call: this station's call hashed, his in full.
    assert_eq!(hear("<K1ABC> PJ4/W8CCC"), ["<K1ABC> PJ4/W8CCC"]);
    assert_eq!(hear("K1ABC <PJ4/W8CCC> -08"), ["K1ABC <PJ4/W8CCC> -08"]);
}

#[test]
fn a_caller_heard_working_another_station_resolves() {
    // W1AW itself was never heard in full here, so its own hash prints `<...>`.
    assert_eq!(hear("<W1AW> PJ4/W8DDD RR73"), ["<...> PJ4/W8DDD RR73"]);
    assert_eq!(hear("K1ABC <PJ4/W8DDD> -08"), ["K1ABC <PJ4/W8DDD> -08"]);
}

/// The control the scenes above need: his call in full in the FIRST place of an i3=4 message,
/// where `unpack77` only adds it to the recent calls. WSJT-X prints `<...>` for his later hash
/// too, so this station does as well.
#[test]
fn a_caller_heard_only_as_the_called_station_stays_unresolved_as_in_wsjtx() {
    assert_eq!(hear("PJ4/W8EEE <W1AW> 73"), ["PJ4/W8EEE <...> 73"]);
    assert_eq!(hear("K1ABC <PJ4/W8EEE> -08"), ["K1ABC <...> -08"]);
}
