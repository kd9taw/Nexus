//! The contact port's reader, by value. Every WSJT-X datagram here is built by Nexus's own
//! encoder; the ADIF and the N1MM XML are written by hand from the formats' published shapes.
//! None is a capture of another program's.

use super::*;
use crate::wsjtx::{encode_heartbeat, encode_logged_adif, encode_qso_logged, QsoLogged};

/// 2026-10-14 01:02:03 UTC.
const WHEN: i64 = 1_791_939_723;

fn qso<'a>(dx_call: &'a str) -> QsoLogged<'a> {
    QsoLogged {
        time_off: WHEN + 60,
        time_on: WHEN,
        dx_call,
        dx_grid: "FN42",
        tx_freq: 14_075_500,
        mode: "FT8",
        report_sent: "-10",
        report_recvd: "-12",
        my_call: "K2DEF",
        my_grid: "FN31",
        ..Default::default()
    }
}

fn field<'a>(f: &'a Fields, k: &str) -> Option<&'a str> {
    f.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

fn one(i: Intake) -> (Format, Option<String>, Fields) {
    match i {
        Intake::Contacts {
            format,
            program,
            mut records,
        } if records.len() == 1 => (format, program, records.pop().unwrap()),
        other => panic!("expected one contact, got {other:?}"),
    }
}

#[test]
fn reads_type5_from_any_id() {
    for id in ["WSJT-X", "JTDX", "MSHV", "WSJT-X - TS-590"] {
        let (format, program, f) = one(read_datagram(&encode_qso_logged(id, &qso("K1ABC"))));
        assert_eq!(format, Format::Wsjtx);
        assert_eq!(program.as_deref(), Some(id));
        assert_eq!(field(&f, "CALL"), Some("K1ABC"));
        assert_eq!(field(&f, "MODE"), Some("FT8"));
        assert_eq!(field(&f, "FREQ"), Some("14.075500"));
        assert_eq!(field(&f, "QSO_DATE"), Some("20261014"));
        assert_eq!(field(&f, "TIME_ON"), Some("010203"));
        assert_eq!(field(&f, "TIME_OFF"), Some("010303"));
        assert_eq!(field(&f, "STATION_CALLSIGN"), Some("K2DEF"));
        assert_eq!(field(&f, "MY_GRIDSQUARE"), Some("FN31"));
        assert_eq!(field(&f, "RST_SENT"), Some("-10"));
    }
}

#[test]
fn reads_type12() {
    let adif = "<call:5>K1ABC<gridsquare:4>FN42<mode:4>MFSK<submode:3>FT4<freq:9>14.081000\
                <qso_date:8>20261014<time_on:6>010203<station_callsign:5>K2DEF<EOR>";
    let (format, program, f) = one(read_datagram(&encode_logged_adif("JTDX", adif)));
    assert_eq!(format, Format::WsjtxAdif);
    assert_eq!(
        program.as_deref(),
        Some("JTDX"),
        "the sender id, over the PROGRAMID"
    );
    assert_eq!(field(&f, "SUBMODE"), Some("FT4"));
    assert_eq!(field(&f, "FREQ"), Some("14.081000"));
}

#[test]
fn reads_one_raw_adif_record() {
    let d = b"<CALL:5>W1ABC<QSO_DATE:8>20261014<TIME_ON:4>0102<BAND:2>2m<MODE:2>FM\
              <PROP_MODE:3>SAT<SAT_NAME:5>AO-91<FREQ:7>145.960<FREQ_RX:7>435.250<EOR>";
    let (format, program, f) = one(read_datagram(d));
    assert_eq!(format, Format::Adif);
    assert_eq!(program, None, "no header, no name");
    assert_eq!(field(&f, "SAT_NAME"), Some("AO-91"));
    assert_eq!(field(&f, "PROP_MODE"), Some("SAT"));
    assert_eq!(field(&f, "FREQ_RX"), Some("435.250"));
    assert_eq!(field(&f, "TIME_ON"), Some("0102"));
}

#[test]
fn reads_adif_with_header_and_a_small_batch() {
    let d = "S.A.T. export\n<ADIF_VER:5>3.1.4<PROGRAMID:6>Tracky<EOH>\n\
             <call:5>W1ABC<qso_date:8>20261014<time_on:6>010203<mode:2>FM<eor>\n\
             <call:5>W2ABC<qso_date:8>20261014<time_on:6>010503<mode:2>FM<eor>\n";
    match read_datagram(d.as_bytes()) {
        Intake::Contacts {
            format,
            program,
            records,
        } => {
            assert_eq!(format, Format::Adif);
            assert_eq!(program.as_deref(), Some("Tracky"));
            let calls: Vec<_> = records.iter().map(|r| field(r, "CALL").unwrap()).collect();
            assert_eq!(calls, ["W1ABC", "W2ABC"]);
            assert!(
                records.iter().all(|r| field(r, "PROGRAMID").is_none()),
                "the header is not a field of a contact"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn ignores_nexus_own_id_tempo() {
    assert_eq!(
        read_datagram(&encode_qso_logged(NEXUS_WSJTX_ID, &qso("K1ABC"))),
        Intake::Own
    );
    assert_eq!(
        read_datagram(&encode_logged_adif(
            NEXUS_WSJTX_ID,
            "<CALL:5>K1ABC<QSO_DATE:8>20261014<TIME_ON:4>0102<EOR>"
        )),
        Intake::Own
    );
    // Nexus's own HRD push carries its PROGRAMID; its own N1MM broadcast its <app>.
    let hrd = "<ADIF_VER:5>3.1.8<PROGRAMID:5>Nexus<EOH><CALL:5>K1ABC<QSO_DATE:8>20261014\
               <TIME_ON:4>0102<EOR>";
    assert_eq!(read_datagram(hrd.as_bytes()), Intake::Own);
    let own = crate::n1mm::build_contactinfo(&crate::n1mm::N1mmContact {
        mycall: "K2DEF".into(),
        call: "K1ABC".into(),
        band: "20".into(),
        mode: "CW".into(),
        timestamp: "2026-10-14 01:02:03".into(),
        section: String::new(),
        gridsquare: String::new(),
        points: 1,
        contestname: "DX".into(),
        radionr: 1,
        freq_10hz: 1_402_500,
        sent_exchange: String::new(),
        rst_sent: "599".into(),
        rst_rcvd: "599".into(),
        sent_nr: None,
        operator: String::new(),
        id: "0".into(),
    });
    assert_eq!(read_datagram(own.as_bytes()), Intake::Own);
}

#[test]
fn a_heartbeat_or_a_status_is_not_a_contact() {
    assert_eq!(
        read_datagram(&encode_heartbeat("WSJT-X", 3, "2.7.0", "")),
        Intake::Ignored
    );
    assert_eq!(read_datagram(b"hello"), Intake::Ignored);
    assert_eq!(read_datagram(b""), Intake::Ignored);
    // A record never closed by <EOR> is no record.
    assert_eq!(
        read_datagram(b"<CALL:5>W1ABC<QSO_DATE:8>20261014<eor"),
        Intake::Ignored
    );
}

#[test]
fn drops_oversize_whole() {
    let mut d = b"<CALL:5>W1ABC<QSO_DATE:8>20261014<TIME_ON:4>0102<EOR>".to_vec();
    assert!(matches!(read_datagram(&d), Intake::Contacts { .. }));
    d.resize(MAX_DATAGRAM + 1, b' ');
    assert_eq!(read_datagram(&d), Intake::Ignored);
}

#[test]
fn too_many_records_or_fields_are_refused_whole() {
    let rec = "<CALL:5>W1ABC<QSO_DATE:8>20261014<TIME_ON:4>0102<EOR>";
    let five = rec.repeat(MAX_RECORDS);
    assert!(
        matches!(read_datagram(five.as_bytes()), Intake::Contacts { records, .. } if records.len() == MAX_RECORDS)
    );
    let six = rec.repeat(MAX_RECORDS + 1);
    assert_eq!(read_datagram(six.as_bytes()), Intake::TooBig);
    let wide = format!(
        "<CALL:5>W1ABC{}<EOR>",
        "<APP_X_Y:1>z".repeat(MAX_FIELDS + 1)
    );
    assert!(wide.len() < MAX_DATAGRAM);
    assert_eq!(read_datagram(wide.as_bytes()), Intake::TooBig);
}

/// A program on the network cannot mark a contact confirmed or uploaded, plant one of Nexus's own
/// fields, or open a field of its own from inside a value.
#[test]
fn only_the_contacts_own_fields_come_in_and_no_value_opens_a_field() {
    let d = "<CALL:5>W1ABC<QSO_DATE:8>20261014<TIME_ON:4>0102<QSL_RCVD:1>Y<LOTW_QSL_RCVD:1>Y\
             <LOTW_QSL_SENT:1>Y<APP_NEXUS_ID:3>123<APP_NEXUS_VIA:4>fake<NOTES:4>priv\
             <COMMENT:19>hi<EOR><CALL:4>W1XX<EOR>";
    let (_, _, f) = one(read_datagram(d.as_bytes()));
    let names: Vec<_> = f.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, ["CALL", "QSO_DATE", "TIME_ON", "COMMENT"]);
    assert_eq!(field(&f, "COMMENT"), Some("hiEORCALL:4W1XX"));
}

/// N1MM's published `<contactinfo>` example, nearly as the page prints it: the frequency in
/// units of 10 Hz.
const CONTACTINFO: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<contactinfo>
	<app>N1MM</app>
	<contestname>CWOPS</contestname>
	<contestnr>73</contestnr>
	<timestamp>2020-01-17 16:43:38</timestamp>
	<mycall>W2XYZ</mycall>
	<band>3.5</band>
	<rxfreq>352519</rxfreq>
	<txfreq>352519</txfreq>
	<operator></operator>
	<mode>CW</mode>
	<call>W1AW</call>
	<countryprefix>K</countryprefix>
	<snt>599</snt>
	<sntnr>5</sntnr>
	<rcv>599</rcv>
	<rcvnr>0</rcvnr>
	<gridsquare></gridsquare>
	<name>Hiram &amp; co</name>
	<power></power>
	<IsOriginal>False</IsOriginal>
	<ID>f9ffac4fcd3e479ca86e137df1338531</ID>
	<IsClaimedQso>1</IsClaimedQso>
	<oldtimestamp>2020-01-17 16:43:38</oldtimestamp>
	<oldcall>W1AW</oldcall>
	<SentExchange>XYZ NY</SentExchange>
</contactinfo>"#;

#[test]
fn reads_n1mm_contactinfo_with_its_frequency_in_10_hz_units() {
    let (format, program, f) = one(read_datagram(CONTACTINFO.as_bytes()));
    assert_eq!(format, Format::N1mm);
    assert_eq!(program.as_deref(), Some(N1MM));
    assert_eq!(field(&f, "CALL"), Some("W1AW"));
    assert_eq!(field(&f, "FREQ"), Some("3.525190"));
    assert_eq!(field(&f, "FREQ_RX"), None, "the same frequency both ways");
    assert_eq!(field(&f, "QSO_DATE"), Some("20200117"));
    assert_eq!(field(&f, "TIME_ON"), Some("164338"));
    assert_eq!(field(&f, "MODE"), Some("CW"));
    assert_eq!(field(&f, "STX"), Some("5"));
    assert_eq!(field(&f, "SRX"), None, "a 0 is no number");
    assert_eq!(field(&f, "STATION_CALLSIGN"), Some("W2XYZ"));
    assert_eq!(field(&f, "CONTEST_ID"), Some("CWOPS"));
    assert_eq!(field(&f, "NAME"), Some("Hiram & co"));
    assert_eq!(field(&f, "OPERATOR"), None, "empty is absent");
    // Split: the receive frequency is kept as FREQ_RX.
    let split = CONTACTINFO.replace("<rxfreq>352519", "<rxfreq>352719");
    let (_, _, f) = one(read_datagram(split.as_bytes()));
    assert_eq!(field(&f, "FREQ_RX"), Some("3.527190"));
}

#[test]
fn n1mm_edits_deletes_and_x_qsos_are_not_read_in_this_version() {
    for root in ["contactreplace", "contactdelete", "lookupinfo"] {
        let d = CONTACTINFO.replace("contactinfo>", &format!("{root}>"));
        assert_eq!(read_datagram(d.as_bytes()), Intake::Ignored, "{root}");
    }
    let x = CONTACTINFO.replace("<IsClaimedQso>1", "<IsClaimedQso>0");
    assert_eq!(read_datagram(x.as_bytes()), Intake::Ignored);
}

/// Random bytes, truncations and bit-flips of a real datagram of each form: every one answers,
/// none panics. A fixed-seed generator, so a failure repeats.
#[test]
fn arbitrary_bytes_never_panic() {
    let mut seed: u64 = 0x0010_ad1f_ca7e_5eed;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let wsjtx5 = encode_qso_logged("WSJT-X", &qso("K1ABC"));
    let wsjtx12 = encode_logged_adif(
        "WSJT-X",
        "<call:5>K1ABC<qso_date:8>20261014<time_on:6>010203<mode:3>FT8<EOR>",
    );
    let adif = "<ADIF_VER:5>3.1.4<EOH><CALL:5>W1ABC<QSO_DATE:8>20261014<TIME_ON:4>0102\
                <NAME:6>Ünïcø<EOR>"
        .as_bytes()
        .to_vec();
    let xml = CONTACTINFO.as_bytes().to_vec();
    let goods = [wsjtx5, wsjtx12, adif, xml];
    let mut contacts = 0;
    for i in 0..40_000 {
        let good = &goods[i % goods.len()];
        let mut bytes: Vec<u8> = match (i / goods.len()) % 3 {
            0 => (0..(next() % 700) as usize).map(|_| next() as u8).collect(),
            1 => good[..(next() as usize % good.len())].to_vec(),
            _ => good.clone(),
        };
        if (i / goods.len()) % 3 == 2 && !bytes.is_empty() {
            for _ in 0..=(next() % 3) {
                let at = next() as usize % bytes.len();
                bytes[at] ^= 1 << (next() % 8);
            }
        }
        if let Intake::Contacts { records, .. } = read_datagram(&bytes) {
            contacts += 1;
            assert!(records.len() <= MAX_RECORDS);
            for r in &records {
                assert!(r.len() <= MAX_FIELDS);
                for (k, v) in r {
                    assert!(KEPT.contains(&k.as_str()), "{k}");
                    assert!(!v.contains(['<', '>']) && v.chars().count() <= MAX_VALUE);
                }
            }
        }
    }
    assert!(
        contacts > 0,
        "no frame ever parsed: the loop never reached a reader"
    );
}

#[test]
fn adif_time_is_utc_from_2000_to_2099() {
    assert_eq!(
        adif_time(WHEN),
        Some(("20261014".to_string(), "010203".to_string()))
    );
    assert_eq!(
        adif_time(951_782_400),
        Some(("20000229".to_string(), "000000".to_string()))
    );
    assert_eq!(adif_time(0), None);
    assert_eq!(adif_time(i64::MAX), None);
}

/// A type 5 whose times carry Qt timespec `spec` (2 with an offset in seconds from UTC).
fn type5_with_timespec(spec: u8, offset: i32) -> Vec<u8> {
    use crate::qds::QdsWriter;
    let jd = |unix: i64| unix.div_euclid(86_400) + 2_440_588;
    let ms = |unix: i64| (unix.rem_euclid(86_400) * 1000) as u32;
    let time = |w: &mut QdsWriter, unix: i64| {
        // The local wall time the offset makes of the UTC instant.
        let local = unix + i64::from(offset);
        w.put_i64(jd(local)).put_u32(ms(local)).put_u8(spec);
        if spec == 2 {
            w.put_i32(offset);
        }
    };
    let mut w = QdsWriter::new();
    w.put_u32(crate::wsjtx::MAGIC)
        .put_u32(3)
        .put_u32(5)
        .put_utf8(Some("SomeLogger"));
    time(&mut w, WHEN + 30);
    w.put_utf8(Some("K1ABC"))
        .put_utf8(Some("FN42"))
        .put_u64(14_074_000)
        .put_utf8(Some("FT8"))
        .put_utf8(Some("-10"))
        .put_utf8(Some("-12"))
        .put_utf8(Some(""))
        .put_utf8(Some(""))
        .put_utf8(Some(""));
    time(&mut w, WHEN);
    w.put_utf8(Some(""))
        .put_utf8(Some("K2DEF"))
        .put_utf8(Some("FN31"))
        .put_utf8(Some(""))
        .put_utf8(Some(""))
        .put_utf8(Some(""));
    w.into_bytes()
}

/// Qt's timespec 2 (an offset from UTC) carries the offset after the time: it is read, and the
/// time taken back to UTC. CONTROL: timespec 1 (UTC), as WSJT-X sends it.
#[test]
fn a_type5_with_an_offset_from_utc_is_read_in_utc() {
    for (spec, offset) in [(2u8, 3600), (2, -18_000), (1, 0)] {
        let (_, program, f) = one(read_datagram(&type5_with_timespec(spec, offset)));
        assert_eq!(program.as_deref(), Some("SomeLogger"));
        assert_eq!(
            field(&f, "CALL"),
            Some("K1ABC"),
            "spec {spec} offset {offset}"
        );
        assert_eq!(field(&f, "QSO_DATE"), Some("20261014"));
        assert_eq!(
            field(&f, "TIME_ON"),
            Some("010203"),
            "spec {spec} offset {offset}"
        );
    }
}

/// A type 5 whose header reads but whose body does not (local time, a time zone, a damaged
/// datagram) is said to be unreadable, under the sender's name, never read wrong.
#[test]
fn a_type5_in_local_time_or_a_time_zone_is_unreadable_not_read_wrong() {
    for spec in [0u8, 3] {
        assert_eq!(
            read_datagram(&type5_with_timespec(spec, 0)),
            Intake::Unreadable {
                program: Some("SomeLogger".into())
            },
            "spec {spec}"
        );
    }
    let mut cut = encode_qso_logged("WSJT-X", &qso("K1ABC"));
    cut.truncate(40);
    assert_eq!(
        read_datagram(&cut),
        Intake::Unreadable {
            program: Some("WSJT-X".into())
        }
    );
    // Not a contact's header: nothing to say.
    assert_eq!(
        read_datagram(&encode_heartbeat("WSJT-X", 3, "2.7.0", "")),
        Intake::Ignored
    );
}
