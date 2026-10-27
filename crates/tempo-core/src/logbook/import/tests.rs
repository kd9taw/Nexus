use super::*;
use crate::logbook::{parse_adif, Logbook};

fn tag(name: &str, value: &str) -> String {
    format!("<{name}:{}>{value}", value.len())
}

/// The notes a source keeps as examples: every note, while there are no more than
/// [`EXAMPLES`] of a kind.
fn noted(s: &Source) -> impl Iterator<Item = &Note> {
    s.notes.iter().flat_map(|g| &g.examples)
}

/// A record from `(name, value)` pairs.
fn rec(fields: &[(&str, &str)]) -> String {
    let mut s: String = fields.iter().map(|(k, v)| tag(k, v)).collect();
    s.push_str("<EOR>\n");
    s
}

/// The fields another logger writes, modelled and not: a contact with confirmations, a park, a
/// contest exchange, three programs' own fields and a user-defined one.
const RICH: &[(&str, &str)] = &[
    ("CALL", "K1ABC"),
    ("QSO_DATE", "20240315"),
    ("TIME_ON", "1432"),
    ("TIME_OFF", "143355"),
    ("BAND", "20M"),
    ("FREQ", "14.074"),
    ("MODE", "MFSK"),
    ("SUBMODE", "FT4"),
    ("RST_SENT", "-10"),
    ("RST_RCVD", "-12"),
    ("GRIDSQUARE", "FN42"),
    ("NAME", "Ann"),
    ("QTH", "Boston"),
    ("STATE", "MA"),
    ("CNTY", "MA,Suffolk"),
    ("COUNTRY", "United States"),
    ("DXCC", "291"),
    ("CQZ", "5"),
    ("ITUZ", "8"),
    ("CONT", "NA"),
    ("TX_PWR", "50"),
    ("QSL_RCVD", "Y"),
    ("LOTW_QSL_RCVD", "V"),
    ("LOTW_QSL_SENT", "Y"),
    ("EQSL_QSL_RCVD", "N"),
    ("QSL_SENT", "N"),
    ("CREDIT_GRANTED", "DXCC:LOTW,WAS"),
    ("POTA_REF", "US-0001"),
    ("STATION_CALLSIGN", "W9XYZ"),
    ("MY_GRIDSQUARE", "EN52"),
    ("OPERATOR", "W9XYZ"),
    ("COMMENT", "great signal"),
    ("NOTES", "first FT4 contact"),
    ("APP_LOG4X_DISTANCE", "1234.5"),
    ("APP_OTHER_COLOR", "red <b>"),
    ("SHOESIZE", "11"),
];

#[test]
fn the_format_is_read_off_the_text() {
    assert_eq!(detect("<?xml version=\"1.0\"?><ADX></ADX>"), Format::Adx);
    assert_eq!(detect("\u{feff}  <ADX><RECORDS/></ADX>"), Format::Adx);
    assert_eq!(
        detect("START-OF-LOG: 3.0\nCONTEST: CQ-WW-CW\n"),
        Format::Cabrillo
    );
    assert_eq!(
        detect("made by hand\n<EOH>\n<CALL:4>W1AW<EOR>"),
        Format::Adif
    );
    // A header that merely mentions Cabrillo is still ADIF.
    assert_eq!(detect("exported\nSTART-OF-LOG: no\n<EOH>"), Format::Adif);
}

#[test]
fn the_tokenizer_reads_what_the_import_reads() {
    let text = format!(
        "header {} <EOH>\n{}{}{}<CALL:4>W2XX",
        tag("ADIF_VER", "3.1.7"),
        rec(&[("CALL", "W1AW"), ("QSO_DATE", "20240101")]),
        rec(&[("NAME", "no call")]),
        rec(&[("call", "N0X"), ("CALL", "N0Y"), ("BAND", "40m")]),
    );
    let raw = adif_records(&text);
    assert_eq!(raw.records.len(), 3);
    assert_eq!(
        raw.header.list,
        vec![("ADIF_VER".to_string(), "3.1.7".to_string())]
    );
    assert_eq!(
        raw.tail.list,
        vec![("CALL".to_string(), "W2XX".to_string())]
    );
    // A repeated tag: the import reads the last.
    let calls: Vec<String> = parse_adif(&text).into_iter().map(|r| r.call).collect();
    assert_eq!(calls, ["W1AW", "N0Y"]);
}

#[test]
fn a_cut_value_is_noted_not_lost_silently() {
    // A length counted in characters over a two-byte character: the byte scan ends inside it.
    let text = "<EOH><CALL:4>W1AW<NAME:2>Jü<EOR>";
    let s = read(text, Format::Adif);
    assert!(noted(&s).any(|n| n.kind == NoteKind::CutValue && n.text == "NAME"));
    // Positive control: a byte-counted value is not cut.
    let ok = read("<EOH><CALL:4>W1AW<NAME:3>Jü<EOR>", Format::Adif);
    assert!(ok.notes.is_empty(), "{:?}", ok.notes);
}

#[test]
fn the_preview_counts_records_dates_coverage_and_problems() {
    let text = format!(
        "<EOH>\n{}{}{}{}{}",
        rec(&[
            ("CALL", "W1AW"),
            ("QSO_DATE", "20240102"),
            ("TIME_ON", "1200"),
            ("BAND", "20m"),
            ("MODE", "CW")
        ]),
        rec(&[
            ("CALL", "K2XX"),
            ("QSO_DATE", "20231231"),
            ("TIME_ON", "0930"),
            ("BAND", "40m"),
            ("MODE", "SSB"),
            ("FREQ", "seven")
        ]),
        rec(&[
            ("CALL", "N3YY"),
            ("QSO_DATE", "2024-13-01"),
            ("BAND", "99m"),
            ("MODE", "WARBLE")
        ]),
        rec(&[("NAME", "nobody")]),
        rec(&[
            ("CALL", "W4ZZ"),
            ("QSO_DATE", "20240301"),
            ("TIME_ON", "99xx"),
            ("BAND", "20m"),
            ("MODE", "FT8")
        ]),
    );
    let p = preview(&read(&text, Format::Adif));
    assert_eq!(p.records, 5);
    assert_eq!(p.contacts, 4);
    // The bad date is left out of the range.
    assert_eq!(p.first_unix, Some(1_704_015_000)); // 2023-12-31 09:30
    assert_eq!(p.last_unix, Some(1_709_251_200)); // 2024-03-01 (time unknown)
    let count = |k: NoteKind| p.notes.iter().find(|g| g.kind == k).map_or(0, |g| g.count);
    assert_eq!(count(NoteKind::NoCall), 1);
    assert_eq!(count(NoteKind::BadDate), 1);
    assert_eq!(count(NoteKind::BadTime), 1);
    assert_eq!(count(NoteKind::BadValue), 1);
    assert_eq!(count(NoteKind::UnknownBand), 1);
    assert_eq!(count(NoteKind::UnknownMode), 1);
    let bad = p
        .notes
        .iter()
        .find(|g| g.kind == NoteKind::BadValue)
        .unwrap();
    assert_eq!(bad.examples[0].at, 2);
    assert_eq!(bad.examples[0].text, "FREQ=seven");
    let cov = |f: &str| {
        p.coverage
            .iter()
            .find(|c| c.field == f)
            .map_or(0, |c| c.count)
    };
    assert_eq!(cov("CALL"), 4);
    assert_eq!(cov("MODE"), 4);
    assert_eq!(cov("FREQ"), 1);
    assert_eq!(cov("NAME"), 1);
    assert_eq!(p.coverage[0].field, "BAND"); // most common first, then by name
}

#[test]
fn a_clean_file_raises_no_notes() {
    let text = format!("<EOH>\n{}", rec(RICH));
    let p = preview(&read(&text, Format::Adif));
    assert!(p.notes.is_empty(), "{:?}", p.notes);
}

#[test]
fn the_report_names_every_field_and_drops_nothing_from_a_clean_record() {
    let text = format!("{}<EOH>\n{}", tag("PROGRAMID", "X"), rec(RICH));
    let r = report(&read(&text, Format::Adif), (1, 0, 0), false);
    assert!(r.accounted);
    for (name, _) in RICH {
        assert!(
            r.fields.iter().any(|f| f.field == *name),
            "{name} missing from the report"
        );
    }
    assert_eq!(r.fields.len(), RICH.len());
    let dropped: Vec<_> = r
        .fields
        .iter()
        .filter(|f| f.dropped > 0)
        .map(|f| (f.field.as_str(), f.reasons.clone()))
        .collect();
    // The two `N` flags are the log's default; nothing else is dropped.
    assert_eq!(
        dropped,
        [
            (
                "EQSL_QSL_RCVD",
                vec![ReasonCount {
                    reason: DropReason::Default,
                    count: 1
                }]
            ),
            (
                "QSL_SENT",
                vec![ReasonCount {
                    reason: DropReason::Default,
                    count: 1
                }]
            ),
        ]
    );
    let f = |n: &str| r.fields.iter().find(|f| f.field == n).unwrap();
    assert_eq!(f("BAND").normalized, 1);
    assert_eq!(f("BAND").examples, ["20M → 20m"]);
    assert_eq!(f("CALL").stored, 1);
    for p in [
        "APP_LOG4X_DISTANCE",
        "APP_OTHER_COLOR",
        "SHOESIZE",
        "CNTY",
        "CQZ",
    ] {
        assert_eq!(f(p).passthrough, 1, "{p}");
    }
    assert_eq!(r.unmapped[0].what, "PROGRAMID");
    assert_eq!(r.unmapped[0].reason, UnmappedReason::Header);
}

#[test]
fn the_report_accounts_for_a_damaged_file_field_by_field() {
    let text = format!(
        "<EOH>\n{}{}<CALL:4>W2XX<BAND:3>40m",
        rec(&[
            ("CALL", "W1AW"),
            ("QSO_DATE", "20240101"),
            ("FREQ", "abc"),
            ("COMMENT", ""),
            ("NAME", "A"),
            ("NAME", "B")
        ]),
        rec(&[("NAME", "nocall")]),
    );
    let r = report(&read(&text, Format::Adif), (1, 0, 0), false);
    assert!(r.accounted);
    let reasons = |n: &str| {
        r.fields
            .iter()
            .find(|f| f.field == n)
            .unwrap()
            .reasons
            .iter()
            .map(|r| r.reason)
            .collect::<Vec<_>>()
    };
    assert_eq!(reasons("FREQ"), [DropReason::Invalid]);
    assert_eq!(reasons("COMMENT"), [DropReason::Empty]);
    assert_eq!(
        reasons("NAME"),
        [DropReason::NoContact, DropReason::Repeated]
    );
    assert_eq!(reasons("CALL"), [DropReason::Unterminated]);
    assert_eq!(reasons("BAND"), [DropReason::Unterminated]);
    let name = r.fields.iter().find(|f| f.field == "NAME").unwrap();
    assert_eq!((name.occurrences, name.stored, name.dropped), (3, 1, 2));
}

/// Every passthrough field written back byte for byte by the operator's own export — the
/// round trip, end to end through a log.
#[test]
fn passthrough_fields_round_trip_byte_for_byte() {
    let odd = [
        ("APP_X_MULTI", "line one\r\nline two"),
        ("APP_X_SPACES", "  padded  "),
        ("APP_X_UNICODE", "Jürgen — Zürich"),
        ("APP_X_ANGLE", "<not a tag>"),
        ("MY_CITY", "Springfield"),
    ];
    let mut fields: Vec<(&str, &str)> = vec![
        ("CALL", "W1AW"),
        ("QSO_DATE", "20240101"),
        ("TIME_ON", "120000"),
    ];
    fields.extend(odd);
    let text = format!("<EOH>\n{}", rec(&fields));
    let mut log = Logbook::new();
    let (added, _, _) = log.import_adif(&text);
    assert_eq!(added.len(), 1);
    let out = crate::logbook::adif_record_own_log(&log.records()[0]);
    for (k, v) in odd {
        assert!(
            out.contains(&tag(k, v)),
            "{k} not written back byte for byte: {out}"
        );
    }
    let r = report(&read(&text, Format::Adif), (1, 0, 0), false);
    for (k, _) in odd {
        assert_eq!(
            r.fields.iter().find(|f| f.field == k).unwrap().passthrough,
            1,
            "{k}"
        );
    }
}

#[test]
fn the_report_would_name_a_passthrough_the_export_lost() {
    // Positive control for the measurement: a written-back record missing a field the record
    // keeps verbatim is a drop, not a passthrough.
    let fields = vec![
        ("CALL".to_string(), "W1AW".to_string()),
        ("APP_X_Y".to_string(), "1".to_string()),
    ];
    let (rec, mut written) = stored_and_written(&fields).unwrap();
    assert_eq!(fates(&fields, Some(&rec), &written)[1], Fate::Passthrough);
    written.retain(|(k, _)| k != "APP_X_Y");
    assert_eq!(
        fates(&fields, Some(&rec), &written)[1],
        Fate::Dropped(DropReason::NotWritten)
    );
}

// ── ADX ─────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn adx_reads_as_the_same_contacts_as_adi() {
    let adi = format!(
        "<EOH>\n{}{}",
        rec(RICH),
        rec(&[
            ("CALL", "W1AW"),
            ("QSO_DATE", "20240101"),
            ("BAND", "40m"),
            ("MODE", "CW")
        ])
    );
    let mut adx = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ADX>\n  <HEADER>\n    <ADIF_VER>3.1.7</ADIF_VER>\n    \
         <USERDEF FIELDID=\"1\" TYPE=\"N\">SHOESIZE</USERDEF>\n  </HEADER>\n  <RECORDS>\n    <RECORD>\n",
    );
    for (k, v) in RICH {
        let v = v.replace('&', "&amp;").replace('<', "&lt;");
        if let Some(rest) = k.strip_prefix("APP_") {
            let (prog, field) = rest.split_once('_').unwrap();
            adx.push_str(&format!(
                "      <APP PROGRAMID=\"{prog}\" FIELDNAME=\"{field}\" TYPE=\"S\">{v}</APP>\n"
            ));
        } else if *k == "SHOESIZE" {
            adx.push_str(&format!(
                "      <USERDEF FIELDNAME=\"SHOESIZE\">{v}</USERDEF>\n"
            ));
        } else {
            adx.push_str(&format!("      <{k}>{v}</{k}>\n"));
        }
    }
    adx.push_str(
        "    </RECORD>\n    <RECORD><CALL>W1AW</CALL><QSO_DATE>20240101</QSO_DATE><!-- a note -->\
         <BAND>40m</BAND><MODE>CW</MODE></RECORD>\n  </RECORDS>\n</ADX>\n",
    );
    assert_eq!(detect(&adx), Format::Adx);
    let s = read(&adx, Format::Adx);
    assert!(s.notes.is_empty(), "{:?}", s.notes);
    assert_eq!(parse_adif(s.adif(&adx)), parse_adif(&adi));
    // The same fields, so the same report.
    let from_adx = report(&s, (2, 0, 0), false);
    let from_adi = report(&read(&adi, Format::Adif), (2, 0, 0), false);
    assert_eq!(from_adx.fields, from_adi.fields);
    assert_eq!(
        s.unmapped
            .iter()
            .map(|u| u.what.as_str())
            .collect::<Vec<_>>(),
        ["ADIF_VER", "USERDEF"]
    );
}

#[test]
fn adx_entities_and_markup_problems() {
    let adx = "<ADX><RECORDS><RECORD><CALL>W1AW</CALL><NAME>A &amp; B &#233;&#x41;</NAME>\
               <COMMENT>x<B>bold</B></COMMENT></RECORD></RECORDS></ADX>";
    let s = read(adx, Format::Adx);
    assert_eq!(
        s.records[0].fields[1],
        ("NAME".to_string(), "A & B éA".to_string())
    );
    assert!(noted(&s).any(|n| n.kind == NoteKind::BadXml));
}

// ── Cabrillo ────────────────────────────────────────────────────────────────────────────────────

fn field<'a>(r: &'a SourceRecord, k: &str) -> Option<&'a str> {
    r.fields
        .iter()
        .find(|(f, _)| f == k)
        .map(|(_, v)| v.as_str())
}

#[test]
fn sweepstakes_lines_read_with_arrls_template() {
    let log =
        "START-OF-LOG: 3.0\nCONTEST: ARRL-SS-CW\nCALLSIGN: W1AW\nCATEGORY-OPERATOR: SINGLE-OP\n\
               NAME: Somebody\nQSO: 14025 CW 2024-11-02 2100 W1AW 1 A 38 CT K8MM 12 Q 92 MI\n\
               QSO:  3550 CW 2024-11-02 2105 W1AW 2 A 38 CT N0XYZ 7 B 99 MN\nEND-OF-LOG:\n";
    assert_eq!(detect(log), Format::Cabrillo);
    let s = read(log, Format::Cabrillo);
    assert_eq!(s.template.as_deref(), Some("ARRL-SS-CW"));
    assert_eq!(s.records.len(), 2);
    let r = &s.records[0];
    assert_eq!(r.at, 6);
    for (k, v) in [
        ("CALL", "K8MM"),
        ("QSO_DATE", "20241102"),
        ("TIME_ON", "210000"),
        ("BAND", "20m"),
        ("FREQ", "14.025"),
        ("MODE", "CW"),
        ("STATION_CALLSIGN", "W1AW"),
        ("STX", "1"),
        ("MY_ARRL_SECT", "CT"),
        ("SRX", "12"),
        ("ARRL_SECT", "MI"),
        ("STX_STRING", "1 A 38 CT"),
        ("SRX_STRING", "12 Q 92 MI"),
        ("CONTEST_ID", "ARRL-SS-CW"),
    ] {
        assert_eq!(field(r, k), Some(v), "{k}");
    }
    // Precedence and check have no ADIF field: listed, both sides, once per line.
    let listed = |w: &str, side| {
        s.unmapped
            .iter()
            .find(|u| u.what == w && u.side == Some(side))
            .map(|u| (u.count, u.reason))
    };
    assert_eq!(
        listed("PREC", "rcvd"),
        Some((2, UnmappedReason::ExchangeText))
    );
    assert_eq!(
        listed("CK", "sent"),
        Some((2, UnmappedReason::ExchangeText))
    );
    let headers: Vec<&str> = s
        .unmapped
        .iter()
        .filter(|u| u.reason == UnmappedReason::Header)
        .map(|u| u.what.as_str())
        .collect();
    assert_eq!(headers, ["CALLSIGN", "CATEGORY-OPERATOR", "NAME"]);
    assert!(s.notes.is_empty(), "{:?}", s.notes);
    // And they come to the log: the section is the contacted station's, the serial is a number.
    let recs = parse_adif(s.adif(log));
    assert_eq!(recs[1].call, "N0XYZ");
    assert_eq!(recs[1].band, "80m");
    assert_eq!(recs[1].contest.as_ref().unwrap().srx, Some(7));
    assert!(recs[1]
        .extra
        .contains(&("ARRL_SECT".to_string(), "MN".to_string())));
}

#[test]
fn cq_ww_lines_read_with_cqs_template_and_a_transmitter_id() {
    let log = "START-OF-LOG: 3.0\nCONTEST: CQ-WW-SSB\nGRID-LOCATOR: EN52\n\
               QSO:  3799 PH 2024-10-26 0711 N6TW          59  03     JT1Z          59  23     0\n\
               QSO: 144 PH 2024-10-26 0715 N6TW 59 03 W6ABC 59 03 1\nEND-OF-LOG:\n";
    let s = read(log, Format::Cabrillo);
    assert_eq!(s.template.as_deref(), Some("CQ-WW-SSB"));
    let r = &s.records[0];
    for (k, v) in [
        ("CALL", "JT1Z"),
        ("MODE", "SSB"),
        ("BAND", "80m"),
        ("RST_SENT", "59"),
        ("RST_RCVD", "59"),
        ("MY_CQ_ZONE", "03"),
        ("CQZ", "23"),
        ("MY_GRIDSQUARE", "EN52"),
        ("CONTEST_ID", "CQ-WW-SSB"),
    ] {
        assert_eq!(field(r, k), Some(v), "{k}");
    }
    assert_eq!(field(&s.records[1], "BAND"), Some("2m"));
    assert_eq!(field(&s.records[1], "FREQ"), None); // a band token is not a frequency
    let t = s
        .unmapped
        .iter()
        .find(|u| u.reason == UnmappedReason::TransmitterId)
        .unwrap();
    assert_eq!(t.count, 2);
    assert!(s.notes.is_empty(), "{:?}", s.notes);
}

#[test]
fn field_day_lines_read_class_and_section() {
    let log = "START-OF-LOG: 3.0\nCONTEST: ARRL-FD\nQSO: 7000 DG 2024-06-22 1805 W9XYZ 3A WI K1ABC 2A CT\nEND-OF-LOG:\n";
    let s = read(log, Format::Cabrillo);
    assert_eq!(s.template.as_deref(), Some("ARRL-FIELD-DAY"));
    let r = &s.records[0];
    assert_eq!(field(r, "CLASS"), Some("2A"));
    assert_eq!(field(r, "ARRL_SECT"), Some("CT"));
    assert_eq!(field(r, "MY_ARRL_SECT"), Some("WI"));
    // DG names no mode, and Field Day is not a one-mode contest: left blank, and said.
    assert_eq!(field(r, "MODE"), None);
    assert!(noted(&s).any(|n| n.kind == NoteKind::ModeUnresolved && n.at == 3));
    // The sent class has no ADIF field.
    assert!(s
        .unmapped
        .iter()
        .any(|u| u.what == "CLASS" && u.side == Some("sent")));
}

#[test]
fn an_unknown_contest_keeps_call_band_mode_time_rst_and_the_exchange_text() {
    let log = "START-OF-LOG: 3.0\nCONTEST: MY-CLUB-SPRINT\nX-SPECIAL: anything\n\
               QSO: 21025 CW 2024-05-04 1500 W1AW 599 MA ALEX K2XYZ 579 NY BOB\n\
               X-QSO: 21030 CW 2024-05-04 1501 W1AW 599 MA ALEX N3ABC 599 PA SUE 1\n\
               QSO: 21030 CW 2024-05-04 1502 W1AW\nEND-OF-LOG:\n";
    let s = read(log, Format::Cabrillo);
    assert_eq!(s.template, None);
    assert_eq!(s.records.len(), 2);
    let r = &s.records[0];
    for (k, v) in [
        ("CALL", "K2XYZ"),
        ("BAND", "15m"),
        ("MODE", "CW"),
        ("TIME_ON", "150000"),
        ("RST_SENT", "599"),
        ("RST_RCVD", "579"),
        ("STX_STRING", "599 MA ALEX"),
        ("SRX_STRING", "579 NY BOB"),
        ("CONTEST_ID", "MY-CLUB-SPRINT"),
    ] {
        assert_eq!(field(r, k), Some(v), "{k}");
    }
    assert_eq!(field(&s.records[1], "CALL"), Some("N3ABC"));
    let kinds: Vec<NoteKind> = noted(&s).map(|n| n.kind).collect();
    assert!(kinds.contains(&NoteKind::UnknownContest));
    assert!(kinds.contains(&NoteKind::ExcludedQso));
    // The line with no other call is not imported, and is listed.
    assert!(noted(&s).any(|n| n.kind == NoteKind::UnreadableLine && n.at == 6));
    assert!(s
        .unmapped
        .iter()
        .any(|u| u.what == "X-SPECIAL" && u.reason == UnmappedReason::Header));
    // Every generated field survives the log.
    let rep = report(&s, (2, 0, 0), false);
    assert!(rep.accounted);
    assert!(
        rep.fields.iter().all(|f| f.dropped == 0),
        "{:?}",
        rep.fields
    );
}

#[test]
fn the_respelled_contest_tokens_find_their_rulesets() {
    for (token, id) in [
        ("ARRL-FD", "ARRL-FIELD-DAY"),
        ("TXQP", "TX-QSO-PARTY"),
        ("ILLINOIS QSO PARTY", "IL QSO Party"),
        ("IL-QSO-PARTY", "IL QSO Party"),
        ("cq-wpx-cw", "CQ-WPX-CW"),
    ] {
        assert_eq!(
            cabrillo::ruleset_for(token, 2026).map(|r| r.contest_id),
            Some(id),
            "{token}"
        );
    }
    assert!(cabrillo::ruleset_for("NOT-A-CONTEST", 2026).is_none());
}

#[test]
fn new_only_leaves_a_held_contact_as_it_is() {
    let held = rec(&[
        ("CALL", "W1AW"),
        ("QSO_DATE", "20240101"),
        ("TIME_ON", "120000"),
        ("BAND", "20m"),
        ("MODE", "CW"),
    ]);
    let again = rec(&[
        ("CALL", "W1AW"),
        ("QSO_DATE", "20240101"),
        ("TIME_ON", "120000"),
        ("BAND", "20m"),
        ("MODE", "CW"),
        ("QSL_RCVD", "Y"),
    ]);
    let new = rec(&[
        ("CALL", "K2XX"),
        ("QSO_DATE", "20240101"),
        ("TIME_ON", "130000"),
        ("BAND", "20m"),
        ("MODE", "CW"),
    ]);
    let text = format!("<EOH>{again}{new}");
    let mut a = Logbook::new();
    a.import_adif(&held);
    let mut b = a.clone();
    assert_eq!(
        a.import_adif_new_only(&text).1,
        1,
        "the held contact is counted as a duplicate"
    );
    assert_eq!(a.records().len(), 2);
    assert!(
        !a.records()[0].qsl_rcvd.card,
        "new only took nothing from the duplicate"
    );
    // Positive control: the ordinary import upgrades it.
    let (added, skipped, merged) = b.import_adif(&text);
    assert_eq!((added.len(), skipped, merged), (1, 1, 1));
    assert!(b.records()[0].qsl_rcvd.card);
}

/// The preview and the report of a 100,000-record file, timed. Run by hand, in release:
/// `cargo test -p tempo-core --release --lib import::tests::timing -- --ignored --nocapture`.
#[test]
#[ignore]
fn timing_a_hundred_thousand_records() {
    let mut text = String::from("<EOH>\n");
    for i in 0..100_000u32 {
        let call = format!("K{}AB{}", i % 10, i % 997);
        let mut fields: Vec<(&str, String)> =
            RICH.iter().map(|(k, v)| (*k, v.to_string())).collect();
        fields[0].1 = call;
        fields[2].1 = format!("{:02}{:02}", i / 60 % 24, i % 60);
        text.push_str(&fields.iter().map(|(k, v)| tag(k, v)).collect::<String>());
        text.push_str("<EOR>\n");
    }
    let t = std::time::Instant::now();
    let source = read(&text, Format::Adif);
    let read_ms = t.elapsed().as_millis();
    let p = preview(&source);
    let preview_ms = t.elapsed().as_millis();
    let r = report(&source, (100_000, 0, 0), false);
    let report_ms = t.elapsed().as_millis() - preview_ms;
    assert_eq!(p.records, 100_000);
    assert!(r.accounted);
    println!(
        "{} MB, 100000 records: read {read_ms} ms, read + preview {preview_ms} ms, report {report_ms} ms",
        text.len() / 1_000_000
    );
}

// ── what a damaged or unusual file must never do ─────────────────────────────────────────────────

#[test]
fn a_file_that_starts_with_non_ascii_text_is_read() {
    for text in [
        "Журнал связей\n<EOH><CALL:4>W1AW<EOR>",
        "\u{feff}Журнал\n<EOH><CALL:4>W1AW<EOR>",
        "<NAME:6>日本<CALL:4>W1AW<EOR>",
        "日本語のログ\n<EOH><CALL:4>W1AW<EOR>",
        "\u{feff}日",
        "Ж",
    ] {
        assert_eq!(detect(text), Format::Adif, "{text}");
        let p = preview(&read(text, detect(text)));
        assert_eq!(p.contacts, usize::from(text.contains("W1AW")), "{text}");
    }
    // The signatures still read after a byte-order mark, with non-ASCII text behind them.
    assert_eq!(
        detect("\u{feff}START-OF-LOG: 3.0\nNAME: Ӂ\n"),
        Format::Cabrillo
    );
    assert_eq!(
        detect("\u{feff}<ADX><HEADER>日本</HEADER></ADX>"),
        Format::Adx
    );
}

#[test]
fn a_dx_multi_transmitter_rtty_line_reads_the_other_call_not_its_rst() {
    // A DX entrant sends RST and zone, receives RST, zone and QTH, and ends the line with its
    // transmitter id: eight columns, the width of a W/VE line with none.
    let log = "START-OF-LOG: 3.0\nCONTEST: CQ-WW-RTTY\nCALLSIGN: DL1ABC\n\
               QSO: 14080 RY 2024-09-28 0001 DL1ABC 599 14 K1ABC 599 05 MA 0\n\
               QSO: 14081 RY 2024-09-28 0002 DL1ABC 599 14 DX JA1XYZ 599 25 DX\n\
               QSO: 14082 RY 2024-09-28 0003 DL1ABC 599 14 VE3XX 599 04 ON\nEND-OF-LOG:\n";
    let s = read(log, Format::Cabrillo);
    assert_eq!(s.template.as_deref(), Some("CQ-WW-RTTY"));
    let calls: Vec<_> = s.records.iter().map(|r| field(r, "CALL")).collect();
    assert_eq!(calls, [Some("K1ABC"), Some("JA1XYZ"), Some("VE3XX")]);
    let r = &s.records[0];
    for (k, v) in [
        ("RST_SENT", "599"),
        ("MY_CQ_ZONE", "14"),
        ("RST_RCVD", "599"),
        ("CQZ", "05"),
        ("STX_STRING", "599 14"),
        ("SRX_STRING", "599 05 MA"),
    ] {
        assert_eq!(field(r, k), Some(v), "{k}");
    }
    assert!(s.notes.is_empty(), "{:?}", s.notes);
    // Both readings find a call, in different columns: listed, not guessed at.
    let s = read(
        "START-OF-LOG: 3.0\nCONTEST: CQ-WW-RTTY\n\
         QSO: 14080 RY 2024-09-28 0001 DL1ABC 599 14 K1ABC JA1XYZ 05 MA 0\nEND-OF-LOG:\n",
        Format::Cabrillo,
    );
    assert!(s.records.is_empty(), "{:?}", s.records);
    assert!(noted(&s).any(|n| n.kind == NoteKind::UnreadableLine && n.at == 3));
}

/// Every contest the rules table holds, and one it does not: lines of every width with the other
/// call in every column and digits everywhere else. Whatever reading a line gets, the CALL stored is
/// that call, never a report, a serial or a zone, and never the log's own call.
#[test]
fn an_imported_call_is_never_a_number_or_the_logs_own_call() {
    let seed: serde_json::Value =
        serde_json::from_str(include_str!("../../fd_rules.seed.json")).unwrap();
    let mut contests: Vec<String> = seed["rulesets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["contest_id"].as_str().unwrap().to_string())
        .collect();
    contests.push("CLUB-X".to_string());
    assert!(contests.len() > 10, "{contests:?}");
    let filler = ["599", "14", "MA", "123", "DX", "05", "59", "7"];
    for contest in &contests {
        for n in 2..=11usize {
            for p in 1..n {
                for other in ["K1ABC", "W1AW"] {
                    let mut a: Vec<&str> = (0..n).map(|i| filler[i % filler.len()]).collect();
                    a[0] = "W1AW";
                    a[p] = other;
                    let log = format!(
                        "START-OF-LOG: 3.0\nCONTEST: {contest}\nCALLSIGN: W1AW\n\
                         QSO: 14025 CW 2024-09-28 0001 {}\nEND-OF-LOG:\n",
                        a.join(" ")
                    );
                    for r in &read(&log, Format::Cabrillo).records {
                        let call = field(r, "CALL").unwrap_or("");
                        assert_eq!(call, "K1ABC", "{contest}: {}", a.join(" "));
                    }
                }
            }
        }
    }
}

#[test]
fn the_logs_own_call_is_never_stored_as_the_other_station() {
    let s = read(
        "START-OF-LOG: 3.0\nCONTEST: CLUB-X\n\
         QSO: 14025 CW 2024-11-02 2100 W1AW 1 A W1AW 38 CT K8MM 12 Q 92 MI\nEND-OF-LOG:\n",
        Format::Cabrillo,
    );
    assert_eq!(s.records.len(), 1);
    assert_eq!(field(&s.records[0], "CALL"), Some("K8MM"));
    assert_eq!(field(&s.records[0], "STATION_CALLSIGN"), Some("W1AW"));
    // The only call besides the sender's is the header's own: not a contact, and listed.
    let s = read(
        "START-OF-LOG: 3.0\nCONTEST: CQ-WW-SSB\nCALLSIGN: N6TW\n\
         QSO: 3799 PH 2024-10-26 0711 N6TW 59 03 N6TW 59 03\n\
         QSO: 3799 PH 2024-10-26 0712 N6TW/7 59 03 N6TW 59 03\nEND-OF-LOG:\n",
        Format::Cabrillo,
    );
    assert!(s.records.is_empty(), "{:?}", s.records);
    let unreadable: Vec<usize> = noted(&s)
        .filter(|n| n.kind == NoteKind::UnreadableLine)
        .map(|n| n.at)
        .collect();
    assert_eq!(unreadable, [4, 5]);
    // Two other calls and no template to say which one was worked: listed, not guessed.
    let s = read(
        "START-OF-LOG: 3.0\nCONTEST: CLUB-X\n\
         QSO: 14025 CW 2024-11-02 2100 W1AW 1 K8MM 38 CT N0XYZ 12\nEND-OF-LOG:\n",
        Format::Cabrillo,
    );
    assert!(s.records.is_empty(), "{:?}", s.records);
    assert!(noted(&s).any(|n| n.kind == NoteKind::UnreadableLine && n.at == 3));
}

#[test]
fn an_unknown_contests_serials_are_not_stored_as_reports() {
    let s = read(
        "START-OF-LOG: 3.0\nCONTEST: CLUB-SPRINT\n\
         QSO: 14025 CW 2024-05-04 1500 W1AW 123 BOB CT K2XYZ 45 ANN NY\n\
         QSO: 14025 CW 2024-05-04 1501 W1AW 123 BOB CT K2XYZ 145 ANN NY\n\
         QSO: 14250 PH 2024-05-04 1502 W1AW 59 BOB CT K2XYZ 57 ANN NY\n\
         QSO: 14250 PH 2024-05-04 1503 W1AW 599 BOB CT K2XYZ 579 ANN NY\nEND-OF-LOG:\n",
        Format::Cabrillo,
    );
    assert_eq!(s.records.len(), 4);
    let rst = |i: usize| {
        (
            field(&s.records[i], "RST_SENT"),
            field(&s.records[i], "RST_RCVD"),
        )
    };
    assert_eq!(rst(0), (None, None));
    assert_eq!(rst(1), (None, None));
    assert_eq!(rst(2), (Some("59"), Some("57")));
    assert_eq!(rst(3), (None, None), "a CW report on a phone line");
    // The exchange is kept whole as text either way.
    assert_eq!(field(&s.records[0], "STX_STRING"), Some("123 BOB CT"));
    assert_eq!(field(&s.records[0], "SRX_STRING"), Some("45 ANN NY"));
}

#[test]
fn an_adx_field_left_open_ends_at_its_record() {
    let adx = "<ADX><RECORDS><RECORD><CALL>W1AW</RECORD>\
               <RECORD><CALL>K1ABC</CALL><BAND>20m</BAND></RECORD>\
               <RECORD><CALL>N0XYZ</CALL>\
               <RECORD><CALL>K2XX</CALL></RECORD></RECORDS></ADX>";
    let s = read(adx, Format::Adx);
    let calls: Vec<Option<&str>> = s.records.iter().map(|r| field(r, "CALL")).collect();
    assert_eq!(calls, [None, Some("K1ABC"), Some("N0XYZ"), Some("K2XX")]);
    assert!(
        s.records
            .iter()
            .all(|r| r.fields.iter().all(|(k, _)| k != "RECORD")),
        "{:?}",
        s.records
    );
    assert_eq!(field(&s.records[1], "BAND"), Some("20m"));
    let bad = noted(&s).filter(|n| n.kind == NoteKind::BadXml).count();
    assert_eq!(bad, 2, "{:?}", s.notes);
}

/// How the time `run` takes grows from size `n` to `8 * n`. The small size is run eight times in a
/// row, so that both sides are the same work when it is linear and take as long: a test thread
/// sharing a core with others is then slowed alike on both sides, where a short run would often
/// fit between two of theirs and a long one never does. Each side is timed three times, in turn,
/// and the fastest kept. Linear work grows about 8 times (less, with a fixed cost) and quadratic
/// work 64 times, so a growth under [`LINEAR`] is linear on any machine, however fast or loaded.
fn growth<T>(n: usize, make: impl Fn(usize) -> T, run: impl Fn(&T)) -> f64 {
    let (small, large) = (make(n), make(8 * n));
    let time = |input: &T, times: usize| {
        let t = std::time::Instant::now();
        for _ in 0..times {
            run(input);
        }
        t.elapsed()
    };
    let (mut fast_small, mut fast_large) = (std::time::Duration::MAX, std::time::Duration::MAX);
    for _ in 0..3 {
        fast_small = fast_small.min(time(&small, 8));
        fast_large = fast_large.min(time(&large, 1));
    }
    8.0 * fast_large.as_secs_f64() / fast_small.as_secs_f64()
}

/// The most a size eight times larger may take, as a multiple: three times linear, and a third of
/// quadratic.
const LINEAR: f64 = 24.0;

#[test]
fn adx_ampersands_read_in_linear_time() {
    let adx = |n: usize| {
        format!(
            "<ADX><RECORDS><RECORD><CALL>W1AW</CALL><COMMENT>{}</COMMENT></RECORD></RECORDS></ADX>",
            "&".repeat(n)
        )
    };
    let s = read(&adx(1_000), Format::Adx);
    assert_eq!(
        field(&s.records[0], "COMMENT"),
        Some("&".repeat(1_000).as_str())
    );
    let g = growth(20_000, adx, |text| {
        read(text, Format::Adx);
    });
    assert!(g < LINEAR, "{g:.1}x");
}

#[test]
fn a_file_over_the_limit_is_refused_with_its_size() {
    let big = " ".repeat(MAX_BYTES + 1);
    let err = read_file(&big).unwrap_err();
    assert!(err.contains("MB"), "{err}");
    let ok = read_file("<EOH><CALL:4>W1AW<EOR>").unwrap();
    assert_eq!(ok.records.len(), 1);
}

#[test]
fn a_preview_shows_the_first_examples_by_place_in_the_file() {
    // The reader finds the stray lines of the header before the QSO lines it cannot read, which
    // come earlier in the file.
    let log = "START-OF-LOG: 3.0\nQSO: 14025\nQSO: 7025\nCONTEST: CQ-WW-CW\n\
               QSO: 14025 CW 2024-11-23 1200 W1AW 599 05 K8MM 599 04\nstray\nstray\n";
    let p = preview(&read(log, Format::Cabrillo));
    let g = p
        .notes
        .iter()
        .find(|g| g.kind == NoteKind::UnreadableLine)
        .unwrap();
    assert_eq!(g.count, 4);
    let at: Vec<usize> = g.examples.iter().map(|n| n.at).collect();
    assert_eq!(at, [2, 3, 6]);
}

/// A file with a record, a header or a Cabrillo log of many fields, many field names or many
/// records: each read, previewed and reported in time linear in its size, so no step looks a field
/// up by scanning the others.
#[test]
fn many_fields_read_in_linear_time() {
    let many = |prefix: &str, n: usize| -> String {
        (0..n).map(|i| tag(&format!("{prefix}{i}"), "v")).collect()
    };
    let adx = |header: String, fields: String| {
        format!(
            "<ADX><HEADER>{header}</HEADER><RECORDS><RECORD><CALL>W1AW</CALL>{fields}</RECORD>\
             <RECORD><CALL>K1ABC</CALL></RECORD></RECORDS></ADX>"
        )
    };
    type Make = Box<dyn Fn(usize) -> String>;
    let cases: Vec<(&str, Make)> = vec![
        (
            "an ADIF record over the limit",
            Box::new(move |n| {
                format!(
                    "<EOH>{}{}<EOR>{}",
                    tag("CALL", "W1AW"),
                    many("APP_X_F", n),
                    rec(&[("CALL", "K1ABC")])
                )
            }),
        ),
        (
            "ADIF records",
            Box::new(|n| {
                let mut text = String::from("<EOH>");
                for i in 0..n / 4 {
                    text.push_str(&rec(&[
                        ("CALL", &format!("K{}AB", i % 10)),
                        ("APP_X_A", "v"),
                        ("APP_X_B", "v"),
                    ]));
                }
                text
            }),
        ),
        (
            "an ADIF header",
            Box::new(move |n| format!("{}<EOH>{}<EOR>", many("H", n), tag("CALL", "W1AW"))),
        ),
        (
            "an ADX header",
            Box::new(move |n| {
                adx(
                    (0..n).map(|i| format!("<H{i}>v</H{i}>")).collect(),
                    String::new(),
                )
            }),
        ),
        (
            "an ADX record over the limit",
            Box::new(move |n| {
                adx(
                    String::new(),
                    (0..n).map(|i| format!("<F{i}>v</F{i}>")).collect(),
                )
            }),
        ),
        (
            "ADX field names ADIF cannot carry",
            Box::new(move |n| {
                adx(
                    String::new(),
                    (0..n)
                        .map(|i| format!("<USERDEF FIELDNAME=\"A B{i}\">v</USERDEF>"))
                        .collect(),
                )
            }),
        ),
        (
            "Cabrillo headers",
            Box::new(|n| {
                format!(
                    "START-OF-LOG: 3.0\nCONTEST: CQ-WW-CW\n{}QSO: 14025 CW 2024-11-23 1200 W1AW 599 05 K8MM 599 04\n",
                    (0..n).map(|i| format!("X-H{i}: v\n")).collect::<String>()
                )
            }),
        ),
    ];
    for (what, make) in cases {
        let g = growth(5_000, make, |text| {
            let source = read(text, detect(text));
            let p = preview(&source);
            let r = report(&source, (1, 0, 0), false);
            assert!(p.contacts >= 1, "{what}");
            assert!(r.accounted, "{what}");
        });
        assert!(g < LINEAR, "{what}: {g:.1}x");
    }
}

/// The limit bounds a record, but no step leans on it: a record of 40,000 fields that skipped it
/// is still previewed and reported in time linear in its fields.
#[test]
fn a_record_past_the_limit_is_still_handled_in_linear_time() {
    let source = |n: usize| {
        let mut fields = vec![("CALL".to_string(), "W1AW".to_string())];
        fields.extend((0..n).map(|i| (format!("APP_X_F{i}"), "v".to_string())));
        let mut source = read("", Format::Adif);
        source.records.push(SourceRecord { at: 1, fields });
        source
    };
    let p = preview(&source(20_000));
    assert_eq!((p.contacts, p.coverage.len()), (1, 20_001));
    let g = growth(5_000, source, |source| {
        let p = preview(source);
        let r = report(source, (1, 0, 0), false);
        assert_eq!(p.contacts, 1);
        assert!(r.accounted);
    });
    assert!(g < LINEAR, "{g:.1}x");
}

/// A record over the limit is not imported at all: keeping its first fields would store a contact
/// the file overrides later in the record. Checked by value, with a repeated CALL and BAND whose
/// last copies (the ones the file means) sit past the limit.
#[test]
fn a_record_over_the_field_limit_is_not_imported_at_all() {
    // CALL and BAND are fields 1 and 2, and again 2,000 and 2,001.
    let over = |filler: usize| {
        let mut fields = vec![("CALL", "K1AAA".to_string()), ("BAND", "20M".to_string())];
        fields.extend((0..filler).map(|i| ("APP_X_F", i.to_string())));
        fields.push(("CALL", "W9BBB".to_string()));
        fields.push(("BAND", "40M".to_string()));
        let mut s: String = fields.iter().map(|(k, v)| tag(k, v)).collect();
        s.push_str("<EOR>\n");
        s
    };
    let before = rec(&[("CALL", "N0BEF"), ("BAND", "15M"), ("QSO_DATE", "20240101")]);
    let after = rec(&[("CALL", "N0AFT"), ("BAND", "10M"), ("QSO_DATE", "20240102")]);
    let text = format!("<EOH>\n{before}{}{after}", over(MAX_FIELDS - 3));
    let source = read(&text, Format::Adif);

    // The import reads neither of the record's calls, and its neighbours as a file without it.
    let row = |r: &QsoRecord| (r.call.clone(), r.band.clone(), r.when_unix);
    let stored: Vec<_> = parse_adif(source.adif(&text)).iter().map(row).collect();
    let alone: Vec<_> = parse_adif(&format!("<EOH>\n{before}{after}"))
        .iter()
        .map(row)
        .collect();
    assert_eq!(stored, alone);
    let calls: Vec<&str> = stored.iter().map(|(c, _, _)| c.as_str()).collect();
    assert_eq!(calls, ["N0BEF", "N0AFT"]);
    let at: Vec<usize> = source.records.iter().map(|r| r.at).collect();
    assert_eq!(at, [1, 3]);

    // The preview says so, and counts none of its fields.
    let p = preview(&source);
    assert_eq!((p.records, p.contacts), (3, 2));
    let g = p
        .notes
        .iter()
        .find(|g| g.kind == NoteKind::TooManyFields)
        .expect("noted");
    assert_eq!((g.count, g.examples[0].at), (1, 2));
    assert!(
        g.examples[0].text.starts_with("CALL=K1AAA BAND=20M"),
        "{:?}",
        g.examples
    );
    assert!(
        p.coverage.iter().all(|c| c.field != "APP_X_F"),
        "{:?}",
        p.coverage
    );
    assert_eq!(
        p.coverage
            .iter()
            .find(|c| c.field == "CALL")
            .map(|c| c.count),
        Some(2)
    );

    // The report accounts for every one of its fields, as not imported.
    let r = report(&source, (2, 0, 0), false);
    assert!(r.accounted);
    let account = |name: &str| r.fields.iter().find(|f| f.field == name).unwrap();
    let over_count = |name: &str| {
        account(name)
            .reasons
            .iter()
            .find(|c| c.reason == DropReason::TooManyFields)
            .map_or(0, |c| c.count)
    };
    assert_eq!(
        (account("CALL").occurrences, account("CALL").stored),
        (4, 2)
    );
    assert_eq!((over_count("CALL"), over_count("BAND")), (2, 2));
    assert_eq!(account("APP_X_F").occurrences, MAX_FIELDS - 3);
    assert_eq!(over_count("APP_X_F"), MAX_FIELDS - 3);

    // Positive control: the same record one field shorter is at the limit, and is imported with
    // the values the file means, the last of each.
    let text = format!("<EOH>\n{before}{}{after}", over(MAX_FIELDS - 4));
    let source = read(&text, Format::Adif);
    assert_eq!(source.adif(&text), text);
    let stored: Vec<_> = parse_adif(source.adif(&text)).iter().map(row).collect();
    assert_eq!(stored.len(), 3);
    assert_eq!(
        (stored[1].0.as_str(), stored[1].1.as_str()),
        ("W9BBB", "40m")
    );
    assert!(preview(&source)
        .notes
        .iter()
        .all(|g| g.kind != NoteKind::TooManyFields));

    // An ADX record over the limit is left out the same way.
    let fields: String = (0..MAX_FIELDS).map(|i| format!("<F{i}>v</F{i}>")).collect();
    let adx = format!(
        "<ADX><RECORDS><RECORD><CALL>K1AAA</CALL>{fields}</RECORD>\
         <RECORD><CALL>N0AFT</CALL></RECORD></RECORDS></ADX>"
    );
    let source = read(&adx, Format::Adx);
    let calls: Vec<String> = parse_adif(source.adif(&adx))
        .into_iter()
        .map(|r| r.call)
        .collect();
    assert_eq!(calls, ["N0AFT"]);
    assert_eq!(source.records[0].at, 2);
    let r = report(&source, (1, 0, 0), false);
    assert!(r.accounted);
    assert_eq!(r.records, 2);
}

/// A record, a header or a tail built to be huge is held as a count of each name, not as its
/// fields: what bounds the memory a hostile file within the size limit can take.
#[test]
fn a_huge_record_is_counted_not_held() {
    let n = 100_000;
    let fields = "<A:0>".repeat(n);
    let text = format!(
        "{fields}<EOH><CALL:4>W1AW{fields}<EOR>{}<CALL:4>W2XX{fields}",
        rec(&[("CALL", "K1ABC")])
    );
    let source = read(&text, Format::Adif);
    let held =
        source.records.iter().map(|r| r.fields.len()).sum::<usize>() + source.tail.list.len();
    assert_eq!(held, 1);
    assert_eq!(source.over.names.get("A"), Some(&n));
    assert_eq!(source.tail.len(), n + 1);
    assert_eq!(
        source
            .unmapped
            .iter()
            .map(|u| (u.what.as_str(), u.count))
            .collect::<Vec<_>>(),
        [("A", n)]
    );
    let r = report(&source, (1, 0, 0), false);
    assert!(r.accounted);
    let a = r.fields.iter().find(|f| f.field == "A").unwrap();
    assert_eq!((a.occurrences, a.dropped), (2 * n, 2 * n));
}

/// An ADIF file naming `n` distinct fields: `CALL` and `X0`…, a thousand of them in the header, 2,500
/// in a record over the field limit, the last thousand in the tail, and the rest in kept records.
fn adif_naming(n: usize) -> String {
    let x = |r: std::ops::Range<usize>| -> String { r.map(|i| format!("<X{i}:0>")).collect() };
    let m = n - 1;
    let mut s = format!("{}<EOH>\n<CALL:4>W1AW{}<EOR>\n", x(0..1000), x(1000..3500));
    let mut i = 3500;
    while i < m - 1000 {
        let j = (i + 1000).min(m - 1000);
        s.push_str(&format!("<CALL:5>K1ABC{}<EOR>\n", x(i..j)));
        i = j;
    }
    s.push_str(&format!("<CALL:4>N0XX{}", x(m - 1000..m)));
    s
}

#[test]
fn a_file_naming_too_many_fields_is_refused_before_its_preview() {
    let ok = read_file(&adif_naming(MAX_NAMES)).unwrap();
    assert_eq!(ok.records.len(), 6);
    assert_eq!(
        read_file(&adif_naming(MAX_NAMES + 1)).unwrap_err(),
        TOO_MANY_NAMES
    );

    // ADX field names, and Cabrillo header tags, count the same way.
    let fields: String = (0..=MAX_NAMES).map(|i| format!("<F{i}/>")).collect();
    let adx = format!("<ADX><RECORDS><RECORD>{fields}</RECORD></RECORDS></ADX>");
    assert_eq!(read_file(&adx).unwrap_err(), TOO_MANY_NAMES);
    let tags: String = (0..=MAX_NAMES).map(|i| format!("X{i}: v\n")).collect();
    let cabrillo = format!("START-OF-LOG: 3.0\n{tags}END-OF-LOG:\n");
    assert_eq!(detect(&cabrillo), Format::Cabrillo);
    assert_eq!(read_file(&cabrillo).unwrap_err(), TOO_MANY_NAMES);
}

/// A record of a few fields, or one just over the field limit, as ADIF text after the text
/// between records: `(its text, over the limit)`.
fn a_record() -> impl proptest::strategy::Strategy<Value = (String, bool)> {
    use proptest::prelude::*;
    let names = ["CALL", "BAND", "APP_X_F", "QSO_DATE"];
    let between = prop::sample::select(vec!["", "\n", "\r\n", " junk > ", "é ñ 漢 comment\n"]);
    let eor = prop::sample::select(vec!["<EOR>", "<eor>", "<EoR>"]);
    let value = ("[a-g0-9 <>:é漢\r\n]{0,6}", any::<bool>()).prop_map(|(v, eor)| {
        if eor {
            format!("{v}<EOR>")
        } else {
            v
        }
    });
    let few = prop::collection::vec((0..names.len(), value), 0..5).prop_map(move |fields| {
        let s: String = fields.iter().map(|(n, v)| tag(names[*n], v)).collect();
        (s, false)
    });
    let over = (1usize..5, 1usize..4).prop_map(move |(extra, step)| {
        let s: String = (0..MAX_FIELDS + extra)
            .map(|i| tag(names[(i * step) % names.len()], "v"))
            .collect();
        (s, true)
    });
    (between, prop_oneof![3 => few, 1 => over], eor)
        .prop_map(|(between, (fields, over), eor)| (format!("{between}{fields}{eor}"), over))
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 64,
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// The text an import reads is the file without its records over the field limit, byte for
    /// byte, header and tail as they were; and the report accounts for every field the file holds.
    /// (The text after `<EOH>` and between records is the next record's, cut with it.)
    #[test]
    fn records_over_the_limit_are_cut_and_every_field_is_accounted(
        header in proptest::sample::select(vec!["", "<eoh>", "<ADIF_VER:5>3.1.4<EOH>"]),
        records in proptest::collection::vec(a_record(), 0..6),
        tail in proptest::sample::select(vec!["", "\n", "<CALL:4>W1AW", "<CALL:99>W1", "junk"]),
    ) {
        let text: String = std::iter::once(header.to_string())
            .chain(records.iter().map(|(r, _)| r.clone()))
            .chain(std::iter::once(tail.to_string()))
            .collect();
        let kept: String = std::iter::once(header.to_string())
            .chain(records.iter().filter(|(_, over)| !over).map(|(r, _)| r.clone()))
            .chain(std::iter::once(tail.to_string()))
            .collect();
        let source = read(&text, Format::Adif);
        let cut = source.adif(&text);
        proptest::prop_assert_eq!(cut, kept.as_str());
        let (whole, read_cut) = (adif_records(&text), adif_records(cut));
        proptest::prop_assert_eq!(&read_cut, &adif_records(&kept));
        proptest::prop_assert_eq!(&read_cut.header, &whole.header);
        proptest::prop_assert_eq!(&read_cut.tail, &whole.tail);
        let fields: Vec<_> = source.records.iter().map(|r| r.fields.clone()).collect();
        let kept_fields: Vec<_> = read_cut.records.iter().map(|r| r.fields.clone()).collect();
        proptest::prop_assert_eq!(fields, kept_fields);

        let r = report(&source, (0, 0, 0), false);
        proptest::prop_assert!(r.accounted);
        let total: usize = whole.records.iter().map(|r| r.fields.len()).sum::<usize>() + whole.tail.len();
        proptest::prop_assert_eq!(r.fields.iter().map(|f| f.occurrences).sum::<usize>(), total);
        proptest::prop_assert_eq!(r.records, records.len());
    }
}
