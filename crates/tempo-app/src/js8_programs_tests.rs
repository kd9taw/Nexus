//! What JS8 tells the programs beside Nexus, as JS8Call 3.0.3 tells them, on the engine's real
//! paths and checked BY VALUE: JS8Call's API hears a JS8 contact logged (`LOG.QSO`) and counts a
//! tune carrier as the transmitter keyed, ALL.TXT names each JS8 frame's speed with JS8Call's
//! letter, and JS8NET hears this station's INFO with its macros replaced.
use super::*;
use crate::engine::js8_api::{answer, Request, StateWatch, Via};
use crate::engine::{DecodeApplied, DecodePass, DecodeResult};
use crate::test_util::StoredLog;
use ::js8::proto::alphabet::sixbit_from_str;
use ::js8::{Payload72, I3};
use serde_json::Value;

/// 2026-10-15 14:30:00 UTC, in ms: when the API's tests say things happen.
const NOW: u64 = 1_792_074_600_000;

/// A JS8 engine on 20 m, KD9TAW at EN52, TX offset 1500 Hz, the API's TCP and UDP sides on.
fn engine() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.js8_api_tcp = true;
    e.settings.js8_api_udp = true;
    e.js8_apply_station_config();
    e.js8_enter();
    e.set_frequency(14.078, "20m", "USB");
    e.set_tx_offset(1500.0);
    e
}

/// The lines of the events queued for the API's programs since the last look, with their ids.
fn events(e: &mut Engine) -> Vec<(String, i64)> {
    e.js8_api_poll()
        .expect("the API is on")
        .events
        .iter()
        .map(|ev| {
            let out = ev.to_outgoing();
            (out.line, out.id)
        })
        .collect()
}

fn of_type(lines: &[(String, i64)], kind: &str) -> Vec<(String, i64)> {
    let tag = format!(r#""type":"{kind}""#);
    lines
        .iter()
        .filter(|(l, _)| l.contains(&tag))
        .cloned()
        .collect()
}

/// A program's read of `kind`, as the API answers it over TCP.
fn read(e: &Engine, kind: &str) -> String {
    let ask = format!(r#"{{"params":{{"_ID":7}},"type":"{kind}","value":""}}"#);
    let req = Request::parse(ask.as_bytes()).expect("a request");
    answer(&req, Via::Tcp, &e.js8_api_snapshot(), NOW)
        .expect("answered")
        .line
}

/// The types of the lines the state watch announces.
fn kinds(out: &[crate::engine::js8_api::Outgoing]) -> Vec<String> {
    out.iter()
        .map(|o| {
            let v: Value = serde_json::from_str(&o.line).expect("JSON");
            v["type"].as_str().expect("a type").to_string()
        })
        .collect()
}

/// A JS8 contact as the JS8 log strip hands it to the log: W1AW in FN31pr on 20 m, 14.0795 MHz
/// (the dial and the TX offset), reports, a name and a comment, 14:30 to 14:35 UTC.
fn js8_contact() -> tempo_core::logbook::QsoRecord {
    let mut rec = tempo_core::logbook::parse_adif("<CALL:4>W1AW<EOR>").remove(0);
    rec.grid = Some("FN31pr".into());
    rec.band = "20m".into();
    rec.freq_mhz = 14.0795;
    rec.mode = "JS8".into();
    rec.rst_sent = Some("-07".into());
    rec.rst_rcvd = Some("-12".into());
    rec.name = Some("Hiram".into());
    rec.comment = Some("TNX QSO".into());
    rec.when_unix = 1_792_074_600;
    rec.time_off_unix = Some(1_792_074_900);
    rec
}

/// A JS8 contact logged is told to the API's programs as JS8Call 3.0.3's `acceptQSO` tells them
/// (mainwindow.cpp:3920-3940): `LOG.QSO`, whose value is the contact as JS8Call's log writes it
/// (`ADIF::QSOToADIF`, ADIF.cpp:385-437: lower-case tags one space apart, MFSK with the submode
/// JS8, the frequency in MHz to six places, then the comment, the name and the operator where
/// there is one) and whose params name its fields, the frequency in Hz and the times in ms. Its
/// id is -1, as every event's, so a TCP program hears it as it hears the others. Only a JS8
/// contact, and only while the API is on.
#[test]
fn a_logged_js8_contact_is_told_as_js8call_tells_it() {
    let mut e = engine();
    let _ = events(&mut e);
    e.log_qso(js8_contact());
    assert_eq!(
        of_type(&events(&mut e), "LOG.QSO"),
        vec![(
            concat!(
                r#"{"params":{"CALL":"W1AW","COMMENTS":"TNX QSO","EXTRA":{},"FREQ":14079500,"#,
                r#""GRID":"FN31PR","MODE":"MFSK","NAME":"Hiram","RPT.RECV":"-12","RPT.SENT":"-07","#,
                r#""STATION.CALL":"KD9TAW","STATION.GRID":"EN52","STATION.OP":"KD9TAW","#,
                r#""SUBMODE":"JS8","UTC.OFF":1792074900000,"UTC.ON":1792074600000,"_ID":-1},"#,
                r#""type":"LOG.QSO","value":"<call:4>W1AW <gridsquare:6>FN31PR <mode:4>MFSK "#,
                r#"<submode:3>JS8 <rst_sent:3>-07 <rst_rcvd:3>-12 <qso_date:8>20261015 "#,
                r#"<time_on:6>143000 <qso_date_off:8>20261015 <time_off:6>143500 <band:3>20m "#,
                r#"<freq:9>14.079500 <station_callsign:6>KD9TAW <my_gridsquare:4>EN52 "#,
                r#"<comment:7>TNX QSO <name:5>Hiram <operator:6>KD9TAW"}"#
            )
            .to_string(),
            -1
        )]
    );

    // With no grid, reports, name or comment: the grid and the reports go empty, as JS8Call
    // writes them, and the comment and the name are left out.
    let mut bare = tempo_core::logbook::parse_adif("<CALL:5>K1ABC<EOR>").remove(0);
    bare.band = "40m".into();
    bare.freq_mhz = 7.0795;
    bare.mode = "JS8".into();
    bare.when_unix = 1_792_074_600;
    e.log_qso(bare);
    let lines = of_type(&events(&mut e), "LOG.QSO");
    assert_eq!(lines.len(), 1, "{lines:?}");
    let v: Value = serde_json::from_str(&lines[0].0).expect("JSON");
    assert_eq!(
        v["value"],
        "<call:5>K1ABC <gridsquare:0> <mode:4>MFSK <submode:3>JS8 <rst_sent:0> <rst_rcvd:0> \
         <qso_date:8>20261015 <time_on:6>143000 <qso_date_off:8>20261015 <time_off:6>143000 \
         <band:3>40m <freq:8>7.079500 <station_callsign:6>KD9TAW <my_gridsquare:4>EN52 \
         <operator:6>KD9TAW"
    );
    assert_eq!(
        (&v["params"]["UTC.ON"], &v["params"]["UTC.OFF"]),
        (
            &Value::from(1_792_074_600_000u64),
            &Value::from(1_792_074_600_000u64)
        ),
        "a contact with no end time ends when it started"
    );

    // An FT8 contact is no JS8 contact: logged, and nothing told.
    let mut ft8 = js8_contact();
    ft8.call = "N0XYZ".into();
    ft8.mode = "FT8".into();
    let before = e.stored_records().len();
    e.log_qso(ft8);
    assert_eq!(
        e.stored_records().len(),
        before + 1,
        "control: it is logged"
    );
    assert!(of_type(&events(&mut e), "LOG.QSO").is_empty());

    // With the API off, a JS8 contact is logged and nothing waits to be told once it is on again.
    e.settings.js8_api_tcp = false;
    e.settings.js8_api_udp = false;
    let mut later = js8_contact();
    later.call = "VE3QQQ".into();
    let before = e.stored_records().len();
    e.log_qso(later);
    assert_eq!(
        e.stored_records().len(),
        before + 1,
        "control: it is logged"
    );
    e.settings.js8_api_tcp = true;
    assert!(of_type(&events(&mut e), "LOG.QSO").is_empty());

    // Nobody can ask for one: LOG.QSO is no request the API answers.
    assert!(
        Request::parse(br#"{"params":{"_ID":9},"type":"LOG.QSO","value":""}"#)
            .is_some_and(|r| answer(&r, Via::Tcp, &e.js8_api_snapshot(), NOW).is_none())
    );
}

/// A tune carrier is the transmitter keyed, as JS8Call 3.0.3 counts its own (`m_transmitting`
/// while `m_tune`, mainwindow.cpp:2560-2571, :2622-2625, :2675): `RIG.PTT` goes on as it comes up
/// and off as it ends, in the event's own form (`emitPTT`, :6685-6700: PTT, UTC and `_ID` -1, no
/// MESSAGE), and while it is up `RIG.GET_PTT` reads PTT true with the MESSAGE "TUNE"
/// (networkMessage.cpp:51-60) and `TX.GET_QUEUE_DEPTH` JS8Call's 1 for a key down with nothing
/// waiting (:759-767). A tune keys no frame, so it sends no tones (`emitTones` is for a frame,
/// mainwindow.cpp:2591-2618).
#[test]
fn a_tune_carrier_is_the_transmitter_keyed_as_js8call_counts_it() {
    let mut e = engine();
    let mut watch = StateWatch::default();
    let _ = watch.step(&e.js8_api_snapshot(), NOW);
    let _ = events(&mut e);
    e.set_tune(true);
    assert!(e.tuning(), "scene: the tune carrier is up");
    let (before, after) = watch.step(&e.js8_api_snapshot(), NOW);
    assert_eq!(
        (kinds(&before), kinds(&after)),
        (vec!["RIG.PTT".to_string(), "STATION.STATUS".into()], vec![])
    );
    assert_eq!(
        before[0].line,
        format!(
            r#"{{"params":{{"PTT":true,"UTC":{NOW},"_ID":-1}},"type":"RIG.PTT","value":"on"}}"#
        )
    );
    assert_eq!(
        read(&e, "RIG.GET_PTT"),
        r#"{"params":{"MESSAGE":"TUNE","PTT":true,"_ID":7},"type":"RIG.PTT_STATUS","value":""}"#
    );
    assert_eq!(
        read(&e, "TX.GET_QUEUE_DEPTH"),
        r#"{"params":{"DEPTH":1,"_ID":7},"type":"TX.QUEUE_DEPTH","value":""}"#
    );
    assert!(of_type(&events(&mut e), "TX.FRAME").is_empty(), "no tones");
    // Held, it is announced once.
    assert_eq!(
        watch.step(&e.js8_api_snapshot(), NOW + 1_000),
        (vec![], vec![])
    );

    e.set_tune(false);
    let (before, after) = watch.step(&e.js8_api_snapshot(), NOW + 5_000);
    assert_eq!(
        (kinds(&before), kinds(&after)),
        (vec![], vec!["RIG.PTT".to_string(), "STATION.STATUS".into()])
    );
    assert_eq!(
        after[0].line,
        format!(
            r#"{{"params":{{"PTT":false,"UTC":{},"_ID":-1}},"type":"RIG.PTT","value":"off"}}"#,
            NOW + 5_000
        )
    );
    assert_eq!(
        read(&e, "RIG.GET_PTT"),
        r#"{"params":{"MESSAGE":"","PTT":false,"_ID":7},"type":"RIG.PTT_STATUS","value":""}"#
    );
    assert_eq!(
        read(&e, "TX.GET_QUEUE_DEPTH"),
        r#"{"params":{"DEPTH":0,"_ID":7},"type":"TX.QUEUE_DEPTH","value":""}"#
    );
}

/// The word of a frame JS8Call printed in ALL.TXT, from its twelve characters and its bits.
fn word_of(chars: &str, i3: u8) -> Word87 {
    Word87::new(
        Payload72::from_chars12(sixbit_from_str(chars).expect("JS8Call's alphabet")),
        I3::from_u8(i3),
    )
}

/// ALL.TXT names each JS8 frame's speed with the letter JS8Call 3.0.3 writes for it
/// (`submodeChar`, DecodedText.cpp:33-48: Normal A, Fast B, JS8 40 C, Slow E, JS8 60 I), after the
/// audio offset, where JS8Call's own line has it. JS8Call's line for the first frame here, a
/// frame from the operator's JS8Call log (`crates/js8/tests/fixtures/js8call_frames.txt`), as
/// 3.0.3 writes it (`DecodedText::string`, DecodedText.cpp:319-343, after the date,
/// processDecodeEvent.cpp:240-245):
///
/// `2026-01-22 23:58:30-11  0.1  645 A  2Wu+WEWCuiZG         3    KD2UWR: @HB HEARTBEAT FN30 `
///
/// Nexus's line for it keeps the layout every other mode's line has, and gains the letter.
#[test]
fn all_txt_names_each_js8_frames_speed_with_js8calls_letter() {
    let mut e = engine();
    e.settings.write_all_txt = true;
    e.set_frequency(7.078, "40m", "USB");
    let heard = |speed: Js8Speed, freq: f32| modes::Decode {
        message: "KD2UWR: @HB HEARTBEAT FN30 ".into(),
        sync: 5.0,
        snr: -11,
        dt: 0.1,
        freq,
        nap: 0,
        qual: 0.9,
        rv: None,
        mode: Some(modes::ModeKind::Js8 { speed }),
        raw: Some(*word_of("2Wu+WEWCuiZG", 3).as_bytes()),
    };
    // The audio of the period that began 2026-01-22 23:58:30 UTC.
    const SLOT: u64 = 1_769_126_310 / 15 + 1;
    let decodes = vec![
        heard(Js8Speed::Normal, 645.0),
        heard(Js8Speed::Fast, 1045.0),
        heard(Js8Speed::Turbo, 1445.0),
        heard(Js8Speed::Slow, 1845.0),
        heard(Js8Speed::Ultra, 2245.0),
    ];
    let applied = e.apply_decode_result(DecodeResult {
        decodes,
        frame: vec![0.0; 1024],
        pass: DecodePass::Js8Multi {
            speed: Js8Speed::Normal,
            cycle_start_ms: 0,
        },
        slot: SLOT,
        epoch: e.decode_epoch,
        heard_on: None,
        failed: false,
        logged_adif: Vec::new(),
    });
    assert!(
        matches!(applied, DecodeApplied::Early { n: 5 }),
        "the five frames folded"
    );
    assert_eq!(
        e.take_all_txt_pending(),
        [
            "260122_235830     7.078 Rx JS8    -11  0.1  645 A KD2UWR: @HB HEARTBEAT FN30",
            "260122_235830     7.078 Rx JS8    -11  0.1 1045 B KD2UWR: @HB HEARTBEAT FN30",
            "260122_235830     7.078 Rx JS8    -11  0.1 1445 C KD2UWR: @HB HEARTBEAT FN30",
            "260122_235830     7.078 Rx JS8    -11  0.1 1845 E KD2UWR: @HB HEARTBEAT FN30",
            "260122_235830     7.078 Rx JS8    -11  0.1 2245 I KD2UWR: @HB HEARTBEAT FN30",
        ]
    );

    // Transmit: the frame keyed at Fast carries B.
    let mut e = engine();
    e.settings.write_all_txt = true;
    e.js8_set_speed(Js8Speed::Fast.index()).expect("Fast");
    e.set_tx_enabled(true);
    e.js8_send(None, "HI".into()).expect("queued");
    let period_ms = u64::from(Js8Speed::Fast.period_s()) * 1000;
    let slot = tempo_core::timing::now_unix_ms() as u64 / period_ms + 1;
    let plan = e.plan_tx(slot).expect("the message keys");
    assert!(matches!(
        plan.waveform,
        TxWaveform::Js8 {
            speed: Js8Speed::Fast,
            ..
        }
    ));
    let lines = e.take_all_txt_pending();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(
        &lines[0][13..],
        "    14.078 Tx JS8      0  0.0 1500 B KD9TAW: HI",
        "{}",
        lines[0]
    );
}

/// JS8NET hears this station's INFO as JS8Call 3.0.3's spot client sends it (`spotSetLocal`,
/// mainwindow.cpp:5543-5547): every macro replaced and every `<…>` left removed
/// (`replaceMacros(…, true)`, mainwindow.h:789-799), as the INFO? reply has it.
#[test]
fn js8net_hears_info_with_every_macro_replaced() {
    let mut e = engine();
    e.settings.js8_info = "QRP <MYCALL> <BOGUS>DIPOLE <MYGRID4> <MYVERSION>".into();
    e.js8_apply_station_config();
    let batch = e.take_js8net("Nexus 1.19.0");
    let local: Value =
        serde_json::from_str(batch.first().expect("RX.LOCAL goes first")).expect("JSON");
    assert_eq!(local["type"], "RX.LOCAL");
    assert_eq!(local["params"]["INFO"], "QRP KD9TAW DIPOLE EN52 NEXUS");
}
