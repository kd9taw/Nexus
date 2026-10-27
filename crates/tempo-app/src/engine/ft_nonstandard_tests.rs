//! Operate's FT8/FT4 sequencer, outside a contest, working a station whose call is nonstandard
//! (`W9XYZ/M`), by value against what WSJT-X 3.0.2 sends in the same place
//! (`widgets/mainwindow.cpp` `genStdMsgs` 9564-9722 and `processMessage` 8918-9453).
//!
//! The partner's overs are fed in as the vendored unpacker prints them off the air, and every
//! over this station keys is read off the planned over, the text the modulator encodes, and
//! compared with WSJT-X's message three ways: the 77-bit type and the text a receiver decodes,
//! the 77 bits, and the FT4 tones.

use super::contest_ft_messages_tests::{air_form, bits77, keyed, tones_of};
use super::*;
use crate::dto::Tier;
use crate::test_util::StoredLog;

const ME: &str = "K1ABC";
const MY_GRID: &str = "FN42";
/// The SNR every one of the partner's overs is decoded at, so the report this station sends
/// it is `-12`.
const SNR: i32 = -12;

/// A decode of the partner's over, as the engine's decoder would deliver it.
fn heard(text: &str) -> modes::Decode {
    modes::Decode {
        message: text.into(),
        sync: 1.0,
        snr: SNR,
        dt: 0.1,
        freq: 1500.0,
        nap: 0,
        qual: 1.0,
        rv: None,
        raw: None,
        mode: None,
    }
}

/// `text` as a receiving station gets it off the air: through the FT8 encoder and the vendored
/// unpacker.
fn off_air(text: &str) -> String {
    air_form(text).1
}

/// What goes on the air for `text`: its 77-bit type and the text a receiver decodes, its 77
/// bits, and its FT4 tones.
fn air(text: &str) -> ((String, String), String, String) {
    let bits: String = bits77(&modes::make_mode(modes::ModeKind::Ft8).encode(text))
        .unwrap_or_default()
        .iter()
        .map(|b| char::from(b'0' + *b as u8))
        .collect();
    (air_form(text), bits, tones_of(modes::ModeKind::Ft4, text))
}

fn tier_name(t: Tier) -> &'static str {
    match t {
        Tier::Ft4 => "FT4",
        _ => "FT8",
    }
}

/// The first over keyed from `from` on, and its slot.
fn key_next(e: &mut Engine, from: u64) -> (u64, String) {
    (from..from + 4)
        .find_map(|s| keyed(e, s).map(|t| (s, t)))
        .unwrap_or_else(|| panic!("nothing keyed at slots {from}..{}", from + 4))
}

/// Operate's FT8 or FT4 with no contest: this station double-clicks W9XYZ/M's CQ.
fn answering_its_cq(tier: Tier) -> Engine {
    let mut e = Engine::new(ME, MY_GRID, 0);
    e.set_tier(tier);
    let cq = off_air("CQ W9XYZ/M");
    e.ingest_decodes_for_test(&[heard(&cq)], 2);
    e.work_station("W9XYZ/M", None, Some(&cq), Some(SNR), None)
        .expect("the double-click is taken");
    e
}

/// ⭐ RR73 AND 73 TO A NONSTANDARD CALL ARE WSJT-X 3.0.2'S, BIT FOR BIT. Operate's FT8 and FT4
/// with no contest, a station signing W9XYZ/M that runs WSJT-X: double-clicking its CQ, and
/// called by it while calling CQ. Its overs are WSJT-X's: its report and roger with its call
/// hashed (`t0b`, mainwindow.cpp 9597, 9625-9626), its grid-less call (`msgtype(t0a, ui->tx1)`,
/// 9627) and its RR73 and 73 with ours hashed (9702, 9710). Each over this station keys packs to
/// the 77 bits of the over WSJT-X writes in its place, and on FT4 to the same tones: Tx1, Tx2 and
/// Tx3 with its call hashed (`t0a`, 9596, 9629-9631, 9655-9656), and the RR73 and 73 with its
/// call whole and ours hashed (`if(!bHisCall and bMyCall) t=hisCall + " <" + my_callsign + "> "
/// …`, 9701 and 9709), type 4 messages; so is the RRR sent in place of RR73 when the operator
/// prefers it, the same expression (`m_send_RR73 ? "RR73" : "RRR"`, 9701). Before, they went
/// `<W9XYZ/M> K1ABC RR73`, a type 1 message that is not WSJT-X's.
#[test]
fn rr73_and_73_to_a_nonstandard_call_are_wsjtxs_bit_for_bit() {
    for tier in [Tier::Ft8, Tier::Ft4] {
        let mut overs = Vec::new();
        // Double-clicking its CQ.
        let mut e = answering_its_cq(tier);
        let tx1 = keyed(&mut e, 4).expect("the answer");
        e.ingest_decodes_for_test(&[heard(&off_air("K1ABC <W9XYZ/M> -07"))], 6);
        let tx3 = keyed(&mut e, 6).expect("the roger");
        e.ingest_decodes_for_test(&[heard(&off_air("<K1ABC> W9XYZ/M RR73"))], 8);
        let tx5 = keyed(&mut e, 8).expect("the 73");
        overs.push(("Tx1", tx1, "<W9XYZ/M> K1ABC FN42", "i3=1"));
        overs.push(("Tx3", tx3, "<W9XYZ/M> K1ABC R-12", "i3=1"));
        overs.push(("Tx5", tx5, "W9XYZ/M <K1ABC> 73", "i3=4"));
        let log = e.stored_log();
        assert_eq!(
            log.iter().map(|r| r.call.as_str()).collect::<Vec<_>>(),
            ["W9XYZ/M"],
            "{}: answering its CQ, logged",
            tier_name(tier)
        );

        // Called by it while calling CQ.
        let mut e = Engine::new(ME, MY_GRID, 0);
        e.set_tier(tier);
        e.start_cq(None).expect("Call CQ");
        let (at, cq) = key_next(&mut e, 0);
        assert_eq!(cq, "CQ K1ABC FN42");
        e.ingest_decodes_for_test(&[heard(&off_air("<K1ABC> W9XYZ/M"))], at + 1);
        let (at, tx2) = key_next(&mut e, at + 2);
        e.ingest_decodes_for_test(&[heard(&off_air("K1ABC <W9XYZ/M> R-07"))], at + 1);
        let (at, tx4) = key_next(&mut e, at + 2);
        overs.push(("Tx2", tx2, "<W9XYZ/M> K1ABC -12", "i3=1"));
        overs.push(("Tx4", tx4, "W9XYZ/M <K1ABC> RR73", "i3=4"));
        e.ingest_decodes_for_test(&[heard(&off_air("<K1ABC> W9XYZ/M 73"))], at + 1);
        let log = e.stored_log();
        assert_eq!(
            log.iter().map(|r| r.call.as_str()).collect::<Vec<_>>(),
            ["W9XYZ/M"],
            "{}: called by it, logged",
            tier_name(tier)
        );

        // The same, with RRR in place of RR73 (Settings: prefer RRR); WSJT-X answers our RRR with
        // its 73 (9210-9215).
        let mut e = Engine::new(ME, MY_GRID, 0);
        let mut s = e.settings().clone();
        s.prefer_rrr = true;
        e.apply_settings(s);
        e.set_tier(tier);
        e.start_cq(None).expect("Call CQ");
        let (at, _) = key_next(&mut e, 0);
        e.ingest_decodes_for_test(&[heard(&off_air("<K1ABC> W9XYZ/M"))], at + 1);
        let (at, _) = key_next(&mut e, at + 2);
        e.ingest_decodes_for_test(&[heard(&off_air("K1ABC <W9XYZ/M> R-07"))], at + 1);
        let (at, rrr) = key_next(&mut e, at + 2);
        overs.push(("Tx4 RRR", rrr, "W9XYZ/M <K1ABC> RRR", "i3=4"));
        e.ingest_decodes_for_test(&[heard(&off_air("<K1ABC> W9XYZ/M 73"))], at + 1);
        let log = e.stored_log();
        assert_eq!(
            log.iter().map(|r| r.call.as_str()).collect::<Vec<_>>(),
            ["W9XYZ/M"],
            "{}: with RRR, logged",
            tier_name(tier)
        );

        for (over, ours, wsjtx, kind) in overs {
            let what = format!("{} {over} {ours:?}", tier_name(tier));
            assert_eq!(air_form(wsjtx).0, kind, "{what}: WSJT-X's packs");
            assert_eq!(air(&ours), air(wsjtx), "{what}: WSJT-X's on the air");
        }
    }
}

/// The Tx panel's RR73 (or RRR) and 73 rows, clicked, go out as WSJT-X's: the row as the panel
/// now shows it (`W9XYZ/M <K1ABC> RR73`) and as it showed it before (`<W9XYZ/M> K1ABC RR73`) both
/// key `W9XYZ/M <K1ABC> RR73`; never both calls hashed, which names neither station.
#[test]
fn the_tx_panels_rr73_and_73_rows_go_out_as_wsjtxs() {
    for (row, sent) in [
        ("W9XYZ/M <K1ABC> RR73", "W9XYZ/M <K1ABC> RR73"),
        ("<W9XYZ/M> K1ABC RR73", "W9XYZ/M <K1ABC> RR73"),
        ("W9XYZ/M <K1ABC> RRR", "W9XYZ/M <K1ABC> RRR"),
        ("<W9XYZ/M> K1ABC RRR", "W9XYZ/M <K1ABC> RRR"),
        ("W9XYZ/M <K1ABC> 73", "W9XYZ/M <K1ABC> 73"),
        ("<W9XYZ/M> K1ABC 73", "W9XYZ/M <K1ABC> 73"),
    ] {
        let mut e = answering_its_cq(Tier::Ft8);
        e.override_next_tx("W9XYZ/M", None, row);
        assert_eq!(keyed(&mut e, 4).as_deref(), Some(sent), "the row {row:?}");
    }
}

/// ⛔ The 73 still goes only to the station's own RR73. Mid-contact with W9XYZ/M, our roger on
/// the air, an RR73 to us from another station signing /M and W9XYZ/M's RR73 to another call
/// close nothing: no 73, no row, and the roger goes again. W9XYZ/M's own then closes as above.
#[test]
fn only_the_partners_own_rr73_draws_the_73() {
    for tier in [Tier::Ft8, Tier::Ft4] {
        let what = tier_name(tier);
        let mut e = answering_its_cq(tier);
        keyed(&mut e, 4).expect("the answer");
        e.ingest_decodes_for_test(&[heard(&off_air("K1ABC <W9XYZ/M> -07"))], 6);
        let roger = keyed(&mut e, 6).expect("the roger");
        e.ingest_decodes_for_test(
            &[
                heard(&off_air("<K1ABC> N0ABC/M RR73")),
                heard(&off_air("<K2DEF> W9XYZ/M RR73")),
            ],
            8,
        );
        assert_eq!(keyed(&mut e, 8), Some(roger), "{what}: the roger again");
        assert!(e.stored_log().is_empty(), "{what}: nothing logged");
        e.ingest_decodes_for_test(&[heard(&off_air("<K1ABC> W9XYZ/M RR73"))], 10);
        assert_eq!(
            keyed(&mut e, 10).as_deref(),
            Some("W9XYZ/M <K1ABC> 73"),
            "{what}: its own RR73"
        );
    }
}

/// What the station has queued to send next.
fn queued(e: &Engine) -> Option<String> {
    match &e.mode {
        Mode::Qso { station, .. } => station.outgoing().map(|m| m.to_text()),
        Mode::FieldDay { station, .. } => station.outgoing().map(|m| m.to_text()),
        _ => None,
    }
}

/// `cq` from `<K9QQQ/M>` as the FT8 encoder packs it, off the air, and the same CQ with the hash
/// of its call changed to one this station's table does not hold (the second call of a standard
/// message is bits 29-56; its last bit flipped is another hash).
fn hashed_cq(cq: &str) -> (String, String) {
    let sent =
        bits77(&modes::make_mode(modes::ModeKind::Ft8).encode(&format!("{cq} <K9QQQ/M> EN37")))
            .expect("the CQ packs");
    let mut unknown = sent.clone();
    unknown[56] ^= 1;
    let print = |b: &[i8]| tempo_fast::unpack77(b).unwrap_or_default();
    (print(&sent), print(&unknown))
}

/// ⛔ A DOUBLE-CLICK ON A CQ FROM A CALL THE DECODER COULD NOT RESOLVE ANSWERS NOTHING, outside a
/// contest as inside one. WSJT-X takes a CQ's call from the word after it (`deCallAndGrid`,
/// decodedtext.cpp 212-224), which `...` cannot be, and puts only a callsign into its DX call box
/// (`Radio::is_callsign`, mainwindow.cpp 9402), so it builds no message to `<...>`. Operate's FT8
/// and FT4 with no contest, a directed CQ whose call this station's hash table does not hold
/// decodes as `CQ TEST <...> EN37` (a plain CQ never carries a hashed call: the packer refuses
/// it): a double-click on it, a logger's Reply to it and the Remote page's work command are
/// refused, and change nothing: the mode, what is queued, TX (off), the RX and TX offsets, and
/// nothing keys. Its next CQ, heard with the call resolved, is worked as before. `CQ DX` is
/// checked as the decoder would print it.
#[test]
fn a_double_click_on_a_cq_from_an_unresolved_call_answers_nothing() {
    let state = |e: &Engine| {
        let mode = match &e.mode {
            Mode::Qso { station, running } => format!("Qso running={running} {:?}", station.state),
            Mode::FieldDay { .. } => "FieldDay".into(),
            Mode::Chat => "Chat".into(),
        };
        (
            mode,
            queued(e),
            e.tx_enabled,
            (e.rx_offset_hz, e.tx_offset_hz),
        )
    };
    for tier in [Tier::Ft8, Tier::Ft4] {
        for (cq, packed) in [("CQ TEST", true), ("CQ DX", false)] {
            let what = format!("{} {cq:?}", tier_name(tier));
            let mut e = Engine::new(ME, MY_GRID, 0);
            e.set_tier(tier);
            e.set_mode("qso-monitor").expect("QSO mode");
            let (resolved, unresolved) = if packed {
                hashed_cq(cq)
            } else {
                (format!("{cq} <K9QQQ/M> EN37"), format!("{cq} <...> EN37"))
            };
            assert_eq!(unresolved, format!("{cq} <...> EN37"), "{what}: unresolved");
            e.ingest_decodes_for_test(&[heard(&unresolved)], 2);
            let before = state(&e);
            let click = e.work_station("<...>", None, Some(&unresolved), Some(SNR), Some(1234.0));
            let reply = e.answer_udp_reply("<...>", &unresolved, SNR, Some(1234.0));
            let remote =
                e.call_station_ctx("<...>", None, Some(&unresolved), Some(SNR), Some(1234.0));
            assert_eq!(
                (
                    click.is_ok(),
                    reply.is_ok(),
                    remote.is_ok(),
                    state(&e),
                    keyed(&mut e, 4)
                ),
                (false, false, false, before.clone(), None),
                "{what}: no answer"
            );
            assert_eq!(before.1, None, "{what}: nothing queued");
            assert_eq!(resolved, format!("{cq} <K9QQQ/M> EN37"), "{what}: resolved");
            e.ingest_decodes_for_test(&[heard(&resolved)], 6);
            e.work_station("<K9QQQ/M>", None, Some(&resolved), Some(SNR), Some(1234.0))
                .expect("its next CQ is taken");
            assert!(e.tx_enabled, "{what}: TX on");
            assert_eq!(
                keyed(&mut e, 8).as_deref(),
                Some("<K9QQQ/M> K1ABC FN42"),
                "{what}: its next CQ, resolved, is answered"
            );
        }
    }
}

/// `over`, a message whose second call is hashed, as the FT8 encoder packs it, off the air with
/// that call's hash changed to one this station's table does not hold (the second call of a
/// standard message is bits 29-56; its last bit flipped is another hash).
fn unresolved(over: &str) -> String {
    let mut bits =
        bits77(&modes::make_mode(modes::ModeKind::Ft8).encode(over)).expect("the over packs");
    bits[56] ^= 1;
    tempo_fast::unpack77(&bits).unwrap_or_default()
}

/// ⛔ A CQ RUN NEVER ANSWERS A CALLER THE DECODER COULD NOT RESOLVE. `<...>` names nobody, and
/// WSJT-X builds no message to it: its messages go to the call in its DX call box (`genStdMsgs`,
/// mainwindow.cpp 9567-9577), which takes only a callsign (`Radio::is_callsign`, 9402). Operate's
/// FT8 and FT4 Call CQ, first-heard and with a best-caller choice: a caller's grid or report from
/// `<...>` keys the CQ again, and the next caller in the same period, its call resolved, is
/// answered, also where the `<...>` caller is the stronger.
#[test]
fn a_cq_run_never_answers_an_unresolved_caller() {
    for tier in [Tier::Ft8, Tier::Ft4] {
        for strategy in ["first", "strongest"] {
            for (over, answer) in [
                ("K1ABC <K9QQQ/M> EN37", "<K9QQQ/M> K1ABC -12"),
                ("K1ABC <K9QQQ/M> -07", "<K9QQQ/M> K1ABC R-12"),
            ] {
                let what = format!("{} {strategy} {over:?}", tier_name(tier));
                let caller = unresolved(over);
                assert_eq!(
                    caller,
                    over.replace("<K9QQQ/M>", "<...>"),
                    "{what}: unresolved"
                );
                let mut e = Engine::new(ME, MY_GRID, 0);
                let mut s = e.settings().clone();
                s.best_caller = strategy.into();
                e.apply_settings(s);
                e.set_tier(tier);
                e.start_cq(None).expect("Call CQ");
                let (at, cq) = key_next(&mut e, 0);
                e.ingest_decodes_for_test(&[heard(&caller)], at + 1);
                let (at, next) = key_next(&mut e, at + 2);
                assert_eq!(next, cq, "{what}: the CQ again");
                let louder = modes::Decode {
                    snr: 0,
                    ..heard(&caller)
                };
                e.ingest_decodes_for_test(&[louder, heard(over)], at + 1);
                assert_eq!(
                    key_next(&mut e, at + 2).1,
                    answer,
                    "{what}: the next caller, resolved, is answered"
                );
            }
        }
    }
}

/// Where the station is, what it has queued, TX, and the RX and TX offsets.
fn where_it_is(e: &Engine) -> (String, Option<String>, bool, (f32, f32)) {
    let mode = match &e.mode {
        Mode::Qso { station, running } => format!("Qso running={running} {:?}", station.state),
        Mode::FieldDay { .. } => "FieldDay".into(),
        Mode::Chat => "Chat".into(),
    };
    (
        mode,
        queued(e),
        e.tx_enabled,
        (e.rx_offset_hz, e.tx_offset_hz),
    )
}

/// ⛔ NO CLICK WORKS A CALL THE DECODER COULD NOT RESOLVE, whatever its message: a double-click,
/// a logger's Reply and the Remote page's work command on a call to this station from `<...>`
/// (its grid, its report, its roger) or on its call to somebody else, and a roster click on
/// `<...>`, are refused and change nothing, on FT8 and FT4, as no message to `<...>` is built in
/// WSJT-X (its DX call box takes only a callsign, `Radio::is_callsign`, mainwindow.cpp 9402).
/// The same message heard with the call resolved is worked as before.
#[test]
fn no_click_works_a_call_the_decoder_could_not_resolve() {
    for tier in [Tier::Ft8, Tier::Ft4] {
        for (over, answer) in [
            ("K1ABC <K9QQQ/M> EN37", "<K9QQQ/M> K1ABC -12"),
            ("K1ABC <K9QQQ/M> -07", "<K9QQQ/M> K1ABC R-12"),
            ("K1ABC <K9QQQ/M> R-07", "K9QQQ/M <K1ABC> RR73"),
            ("W9XYZ <K9QQQ/M> EN37", "<K9QQQ/M> K1ABC FN42"),
        ] {
            let what = format!("{} {over:?}", tier_name(tier));
            let heard_as = unresolved(over);
            assert_eq!(
                heard_as,
                over.replace("<K9QQQ/M>", "<...>"),
                "{what}: unresolved"
            );
            let mut e = Engine::new(ME, MY_GRID, 0);
            e.set_tier(tier);
            e.set_mode("qso-monitor").expect("QSO mode");
            e.ingest_decodes_for_test(&[heard(&heard_as)], 2);
            let before = where_it_is(&e);
            let click = e.work_station("<...>", None, Some(&heard_as), Some(SNR), Some(1234.0));
            let reply = e.answer_udp_reply("<...>", &heard_as, SNR, Some(1234.0));
            let remote =
                e.call_station_ctx("<...>", None, Some(&heard_as), Some(SNR), Some(1234.0));
            let roster = e.work_station("<...>", None, None, None, Some(1234.0));
            assert_eq!(
                (
                    [click.is_ok(), reply.is_ok(), remote.is_ok(), roster.is_ok()],
                    where_it_is(&e),
                    keyed(&mut e, 4)
                ),
                ([false; 4], before.clone(), None),
                "{what}: no answer"
            );
            assert_eq!(before.1, None, "{what}: nothing queued");
            e.ingest_decodes_for_test(&[heard(over)], 6);
            e.work_station("<K9QQQ/M>", None, Some(over), Some(SNR), Some(1234.0))
                .expect("its next is taken");
            assert_eq!(
                keyed(&mut e, 8).as_deref(),
                Some(answer),
                "{what}: its next, resolved, is answered"
            );
        }
    }
}

/// ⭐ A CLICKED TX3 ROW GOES OUT AS THE ROW SHOWS IT, as WSJT-X keys the text of the Tx3 box
/// clicked (`on_txb3_clicked`, mainwindow.cpp 8771-8777; `ui->tx3->text()`, 7901). A /P station
/// working a nonstandard call can put no number in its roger, so its Tx3 row is `<W9XYZ/M>
/// K1ABC/P RRR`; clicked, it went out as `W9XYZ/M <K1ABC/P> RRR`, the Tx4 row. The Tx4 row and
/// the sequence's own Tx3 keep their forms.
#[test]
fn a_clicked_tx3_row_goes_out_as_the_row_shows_it() {
    for tier in [Tier::Ft8, Tier::Ft4] {
        for (dx, cq) in [("W9XYZ/M", "CQ W9XYZ/M"), ("PJ4/W9XYZ", "CQ PJ4/W9XYZ")] {
            let what = format!("{} {dx}", tier_name(tier));
            let working = || {
                let mut e = Engine::new("K1ABC/P", MY_GRID, 0);
                e.set_tier(tier);
                let cq = off_air(cq);
                e.ingest_decodes_for_test(&[heard(&cq)], 2);
                e.work_station(dx, None, Some(&cq), Some(SNR), None)
                    .expect("the double-click is taken");
                e
            };
            for row in [format!("<{dx}> K1ABC/P RRR"), format!("{dx} <K1ABC/P> RRR")] {
                let mut e = working();
                e.override_next_tx(dx, None, &row);
                assert_eq!(
                    keyed(&mut e, 4).as_deref(),
                    Some(row.as_str()),
                    "{what}: the row {row:?}"
                );
            }
            let mut e = working();
            keyed(&mut e, 4).expect("the answer");
            e.ingest_decodes_for_test(&[heard(&format!("K1ABC/P <{dx}> -07"))], 6);
            assert_eq!(
                keyed(&mut e, 6),
                Some(format!("<{dx}> K1ABC/P RRR")),
                "{what}: the sequence's Tx3"
            );
        }
    }
}

/// ⭐ FREE TEXT IN A CONTACT WITH A STATION SIGNING W9XYZ/M GOES OUT AS WSJT-X'S MESSAGE FOR THE
/// TWO CALLS. Typed `73` went out as `W9XYZ/M K1ABC 73`, which the packer sends as `W9XYZ K1ABC 73`:
/// another station. Where exactly one call is nonstandard, WSJT-X writes that call whole and the
/// other hashed (`genStdMsgs`, mainwindow.cpp: RR73 or RRR and 73, 9701-9702 and 9709-9710; a
/// report, `t0a` 9596 and 9629-9631 or `t0b` 9597 and 9625-9626, then 9655-9656); each typed text
/// packs to the 77 bits and FT4 tones of WSJT-X's message. With both calls standard, or both
/// nonstandard, WSJT-X's message is `t0` (9591, 9595), which the text as typed already packs to.
/// Text that is no standard message goes as typed where the packer has a frame for it; `TNX`
/// beside W9XYZ/M has none and is never queued (`ft_refused_text_tests`).
#[test]
fn free_text_goes_out_as_wsjtxs_message_for_the_two_calls() {
    for tier in [Tier::Ft8, Tier::Ft4] {
        for (me, cq, typed, wsjtx) in [
            ("K1ABC", "CQ W9XYZ/M", "73", "W9XYZ/M <K1ABC> 73"),
            ("K1ABC", "CQ W9XYZ/M", "RR73", "W9XYZ/M <K1ABC> RR73"),
            ("K1ABC", "CQ W9XYZ/M", "RRR", "W9XYZ/M <K1ABC> RRR"),
            ("K1ABC", "CQ W9XYZ/M", "-12", "<W9XYZ/M> K1ABC -12"),
            ("K1ABC", "CQ W9XYZ/M", "R-12", "<W9XYZ/M> K1ABC R-12"),
            ("K1ABC", "CQ PJ4/W9XYZ", "73", "PJ4/W9XYZ <K1ABC> 73"),
            ("K1ABC", "CQ YW18FIFA", "73", "YW18FIFA <K1ABC> 73"),
            ("K1ABC/P", "CQ W9XYZ/M", "73", "W9XYZ/M <K1ABC/P> 73"),
            ("K1ABC/QRP", "CQ W9XYZ EN37", "73", "<W9XYZ> K1ABC/QRP 73"),
            ("K1ABC/QRP", "CQ W9XYZ EN37", "-12", "W9XYZ <K1ABC/QRP> -12"),
            // As typed: WSJT-X's already.
            ("K1ABC", "CQ W9XYZ EN37", "73", "W9XYZ K1ABC 73"),
            ("K1ABC", "CQ W9XYZ/P EN37", "73", "W9XYZ/P K1ABC 73"),
            ("K1ABC/P", "CQ F4CYH/R JN18", "73", "F4CYH/R K1ABC/P 73"),
            ("PJ4/K1ABC", "CQ W9XYZ/M", "73", "W9XYZ K1ABC 73"),
        ] {
            let what = format!("{} {me} {cq:?} {typed:?}", tier_name(tier));
            let mut e = Engine::new(me, MY_GRID, 0);
            e.set_tier(tier);
            let cq = off_air(cq);
            e.ingest_decodes_for_test(&[heard(&cq)], 2);
            let Msg::Cq { de, .. } = Msg::parse(&cq) else {
                panic!("{what}: a CQ")
            };
            e.work_station(&de, None, Some(&cq), Some(SNR), None)
                .expect("the double-click is taken");
            e.qso_freetext(typed);
            let sent = keyed(&mut e, 4).expect("the free text");
            assert_eq!(air(&sent), air(wsjtx), "{what}: WSJT-X's on the air");
        }
        // Not a standard message, and `W9XYZ/M K1ABC TNX` has no frame (`pack77_4` builds it
        // from a hash and a call it never set; it went on the air as `<...>`): never queued, and
        // the reply keys.
        let mut e = answering_its_cq(tier);
        e.qso_freetext("TNX");
        assert_eq!(
            keyed(&mut e, 4).as_deref(),
            Some("<W9XYZ/M> K1ABC FN42"),
            "{}: free text with no frame",
            tier_name(tier)
        );
    }
}
