//! ⭐ **An operator's own contest on the air, FT8**: two stations running the same custom contest
//! work each other through the real modem ([`run_loopback_fieldday`]) in the WSJT-X contest
//! messages its exchange matches, and both log the contact with the exchange the other sent.
//!
//! ⚠️ The user layer is process-global and these tests run in parallel, so each one installs
//! under its OWN id and Cabrillo token.
use tempo_core::contest::{ContestSession, StationData};
use tempo_core::fd_rules::custom::{self, CustomDef, CustomField, CustomWindow};
use tempo_core::fd_rules::{install_custom, ruleset_by_id, FdRuleset, CURRENT_RULES_YEAR};
use tempo_core::fieldday::{run_loopback_fieldday, FieldDayStation, FtMessages};

/// 2026-11-14T00:00Z.
const NOW: u64 = 1_794_614_400;

fn install(id: &str, token: &str, fields: &[&str], ft_cq: Option<&str>) -> &'static FdRuleset {
    let d = CustomDef {
        format: custom::FORMAT.into(),
        version: custom::VERSION,
        id: id.into(),
        name: format!("Test {id}"),
        short: String::new(),
        cabrillo_contest: token.into(),
        modes: vec!["FT8".into(), "FT4".into()],
        window: CustomWindow::Weekend {
            month: 8,
            weekend: "nth_full".into(),
            n: 2,
            start_hour_utc: 0,
            duration_hours: 48,
        },
        exchange: fields
            .iter()
            .map(|f| CustomField {
                field: f.to_string(),
                label: String::new(),
            })
            .collect(),
        ft_cq: ft_cq.map(str::to_string),
        // The operator said Yes to the RTTY Roundup messages; no other shape reads the answer.
        ft_roundup: true,
        dupe: "band".into(),
        points: Default::default(),
    };
    let refused = install_custom(std::slice::from_ref(&d), NOW);
    assert!(refused.is_empty(), "{refused:?}");
    ruleset_by_id(id, CURRENT_RULES_YEAR).expect("installed")
}

/// One station in `rs` on FT8: `call` in `grid`, its Field Day class and section `class`/`section`.
fn station(
    rs: &'static FdRuleset,
    running: bool,
    call: &str,
    grid: &str,
    class: &str,
    section: &str,
) -> FieldDayStation {
    station_in(rs, running, call, grid, class, section, "MA")
}

/// [`station`] in the state or province `qth`.
fn station_in(
    rs: &'static FdRuleset,
    running: bool,
    call: &str,
    grid: &str,
    class: &str,
    section: &str,
    qth: &str,
) -> FieldDayStation {
    let data = StationData {
        mycall: call.into(),
        mygrid: grid.into(),
        fd_class: class.into(),
        fd_section: section.into(),
        contest_qth_state: qth.into(),
        contest_category_operator: "SINGLE-OP".into(),
        ..Default::default()
    };
    let session = ContestSession::for_ruleset(rs, &data).expect("a session starts");
    let mut st = if running {
        FieldDayStation::running(call, grid, session, "20m")
    } else {
        FieldDayStation::search_and_pounce(call, grid, session, "20m")
    };
    st.set_messages(FtMessages::for_contest(rs, "FT8"), running);
    st.set_wsjtx_field_day(FtMessages::wsjtx_field_day(rs, "FT8"));
    st
}

/// Each logged row as `call rx[..] tx[..]`.
fn rows(st: &FieldDayStation) -> Vec<String> {
    let kv = |v: &[tempo_core::contest::FieldValue]| {
        v.iter()
            .map(|f| format!("{}={}", f.key, f.raw))
            .collect::<Vec<_>>()
            .join(",")
    };
    st.log
        .qsos()
        .iter()
        .map(|q| format!("{} rx[{}] tx[{}]", q.call, kv(&q.rx), kv(&q.tx)))
        .collect()
}

/// Field Day's exchange: `CQ FD`, the class and section each way, and both logs agree.
#[test]
fn a_field_day_shaped_contest_works_a_contact_in_field_days_messages() {
    let rs = install(
        "custom_air_fd",
        "AIR-FD",
        &["fd_class", "arrl_section"],
        None,
    );
    let mut run = station(rs, true, "K1ABC", "FN42", "2B", "EMA");
    let mut sp = station(rs, false, "W9XYZ", "EN52", "1D", "WI");
    assert_eq!(run.outgoing().unwrap().to_text(), "CQ FD K1ABC FN42");
    run_loopback_fieldday(&mut run, &mut sp, 0.0, 12);
    assert_eq!(
        rows(&run),
        ["W9XYZ rx[CLASS=1D,SECTION=WI] tx[CLASS=2B,SECTION=EMA]"]
    );
    assert_eq!(
        rows(&sp),
        ["K1ABC rx[CLASS=2B,SECTION=EMA] tx[CLASS=1D,SECTION=WI]"]
    );
}

/// NA VHF / ARRL Digi: `CQ TEST`, the four-character grid each way, and both logs agree.
#[test]
fn a_grid_contest_calling_cq_test_works_a_contact_in_the_na_vhf_messages() {
    let rs = install("custom_air_navhf", "AIR-NAVHF", &["grid4"], Some("TEST"));
    let mut run = station(rs, true, "K1ABC", "FN42", "", "");
    let mut sp = station(rs, false, "W9XYZ", "EN52", "", "");
    assert_eq!(run.outgoing().unwrap().to_text(), "CQ TEST K1ABC FN42");
    run_loopback_fieldday(&mut run, &mut sp, 0.0, 12);
    assert_eq!(rows(&run), ["W9XYZ rx[GRID=EN52] tx[GRID=FN42]"]);
    assert_eq!(rows(&sp), ["K1ABC rx[GRID=FN42] tx[GRID=EN52]"]);
}

/// Every over of one contact between a running station and an S&P station, as text: the run's
/// over is decoded by S&P at `snr`, whose operator double-clicks the first CQ it hears, and back.
/// `R:` the run's overs, `S:` S&P's, until both are done or `max` overs.
fn overs(run: &mut FieldDayStation, sp: &mut FieldDayStation, snr: i32, max: u64) -> Vec<String> {
    let mut out = Vec::new();
    for slot in 0..max {
        let (tx, rx, who) = if slot % 2 == 0 {
            (&mut *run, &mut *sp, "R")
        } else {
            (&mut *sp, &mut *run, "S")
        };
        let Some(m) = tx.outgoing() else {
            continue;
        };
        let text = m.to_text();
        out.push(format!("{who}: {text}"));
        let mut d = heard(&text);
        d.snr = snr;
        rx.observe(std::slice::from_ref(&d), slot);
        if who == "R" && rx.state == tempo_core::fieldday::FdState::Listening {
            if let tempo_core::message::Msg::Cq { de, grid, .. } =
                tempo_core::message::Msg::parse(&text)
            {
                rx.answer_cq(&de, &grid, snr);
            }
        }
        tx.after_tx();
        if run.done() && sp.done() {
            break;
        }
    }
    out
}

/// WW Digi: the NA VHF messages with `CQ WW`, over for over as WSJT-X's genStdMsgs writes them,
/// and a real contact through the modem.
#[test]
fn a_grid_contest_calling_cq_ww_works_a_contact_in_the_ww_digi_messages() {
    let rs = install("custom_air_ww", "AIR-WW", &["grid4"], Some("WW"));
    assert_eq!(FtMessages::for_contest(rs, "FT8"), FtMessages::WwDigi);
    let mut run = station(rs, true, "K1ABC", "FN42", "", "");
    let mut sp = station(rs, false, "W9XYZ", "EN52", "", "");
    assert_eq!(
        overs(&mut run, &mut sp, -7, 12),
        [
            "R: CQ WW K1ABC FN42",
            "S: K1ABC W9XYZ EN52",
            "R: W9XYZ K1ABC R FN42",
            "S: K1ABC W9XYZ RR73",
            "R: W9XYZ K1ABC 73",
        ]
    );
    assert_eq!(rows(&run), ["W9XYZ rx[GRID=EN52] tx[GRID=FN42]"]);
    assert_eq!(rows(&sp), ["K1ABC rx[GRID=FN42] tx[GRID=EN52]"]);
    let mut run = station(rs, true, "K1ABC", "FN42", "", "");
    let mut sp = station(rs, false, "W9XYZ", "EN52", "", "");
    run_loopback_fieldday(&mut run, &mut sp, 0.0, 12);
    assert_eq!(rows(&run), ["W9XYZ rx[GRID=EN52] tx[GRID=FN42]"]);
    assert_eq!(rows(&sp), ["K1ABC rx[GRID=FN42] tx[GRID=EN52]"]);
    // Every over packs as WSJT-X's standard message (i3=1).
    for text in ["CQ WW K1ABC FN42", "K1ABC W9XYZ EN52", "W9XYZ K1ABC R FN42"] {
        assert_eq!(i3n3(text).map(|(i3, _)| i3), Some(1), "{text}");
    }
}

/// The `(i3, n3)` WSJT-X's packer gives `text` (`packjt77.f90` `pack77`), through the port the
/// transmitter uses; `None` where it has no frame for it.
fn i3n3(text: &str) -> Option<(u8, u8)> {
    let mut msg = [b' '; 37];
    for (m, b) in msg.iter_mut().zip(text.bytes()) {
        *m = b;
    }
    let c77 = modes::wsjt77::pack::pack(&msg).ok()??;
    let n = |a: usize, b: usize| c77[a..b].iter().fold(0u8, |v, &x| (v << 1) | x);
    Some((n(74, 77), n(71, 74)))
}

/// ⭐ Every section a Field Day-shaped contest accepts packs into WSJT-X's Field Day message
/// (i3=0, n3=3): Nexus's `fd_sections` list, less `MX`, is the packer's `csec` (packjt77.f90
/// `pack77_03`), so no section the builder offers goes out as anything else.
///
/// `MX` (Mexico, Winter Field Day's rules) is in neither `csec` nor WSJT-X's own Field Day
/// exchange (`field_day_exchange_re`, Configuration.cpp 262-283): the packer has no frame for it,
/// so the keying point refuses that over and nothing goes out, exactly as ARRL Field Day's does.
#[test]
fn every_field_day_section_packs_as_the_field_day_message() {
    let sections = tempo_core::fd_rules::fd_sections_domain();
    assert!(sections.values.len() > 80, "the shipped section list");
    for (code, _) in sections.values {
        let want = (*code != "MX").then_some((0, 3));
        for text in [
            format!("W9XYZ K1ABC 2B {code}"),
            format!("W9XYZ K1ABC R 2B {code}"),
        ] {
            assert_eq!(i3n3(&text), want, "{text}");
        }
    }
    assert!(modes::wsjt77::ft8_refusal("W9XYZ K1ABC 2B MX").is_some());
}

/// A decode of `text` at -7 dB.
fn heard(text: &str) -> modes::Decode {
    modes::Decode {
        message: text.into(),
        sync: 1.0,
        snr: -7,
        dt: 0.0,
        freq: 1500.0,
        nap: 0,
        qual: 1.0,
        rv: None,
        raw: None,
        mode: None,
    }
}

/// ⛔ S&P never starts a contact by itself in a custom contest's messages: every CQ it hears is
/// passed by with nothing queued, as WSJT-X calls nobody the operator did not pick.
fn sp_answers_no_cq_by_itself(rs: &'static FdRuleset, cqs: &[&str]) {
    let mut sp = station(rs, false, "W9XYZ", "EN52", "1D", "WI");
    for (k, cq) in cqs.iter().enumerate() {
        sp.observe(&[heard(cq)], 2 * k as u64);
        assert!(
            sp.outgoing().is_none(),
            "{} answered {cq:?} by itself",
            rs.event
        );
        assert!(sp.dxcall.is_none(), "{cq:?}");
    }
}

#[test]
fn sp_in_each_custom_contest_arm_answers_no_cq_by_itself() {
    let fd = install(
        "custom_gate_fd",
        "GATE-FD",
        &["fd_class", "arrl_section"],
        None,
    );
    let vhf = install("custom_gate_navhf", "GATE-NAVHF", &["grid4"], Some("TEST"));
    let ww = install("custom_gate_ww", "GATE-WW", &["grid4"], Some("WW"));
    let ru = install("custom_gate_ru", "GATE-RU", &["rst", "serial"], None);
    let cqs = [
        "CQ FD K1ABC FN42",
        "CQ TEST K1ABC FN42",
        "CQ WW K1ABC FN42",
        "CQ RU K1ABC FN42",
        "CQ K1ABC FN42",
        "CQ PJ4/K1ABC",
    ];
    sp_answers_no_cq_by_itself(fd, &cqs);
    sp_answers_no_cq_by_itself(vhf, &cqs);
    sp_answers_no_cq_by_itself(ww, &cqs);
    sp_answers_no_cq_by_itself(ru, &cqs);
}

// ---- F3: WSJT-X's ARRL RTTY Roundup messages ------------------------------------------------------

/// WSJT-X's report from an SNR (genStdMsgs, mainwindow.cpp 9619-9622): `nn=(n+36)/6`, clamped to
/// 2..=9, C++ integer division truncating toward zero.
#[test]
fn the_contest_report_is_wsjtxs_5n9() {
    use tempo_core::fieldday::contest_rst;
    for (snr, want) in [
        (-30, "529"),
        (-37, "529"),
        (-50, "529"),
        (-24, "529"),
        (-18, "539"),
        (-7, "549"),
        (-6, "559"),
        (0, "569"),
        (6, "579"),
        (12, "589"),
        (18, "599"),
        (20, "599"),
        (49, "599"),
    ] {
        assert_eq!(contest_rst(snr), want, "{snr} dB");
    }
}

/// RST + serial, asked for the Roundup: `CQ RU`, then WSJT-X's Tx2 / Tx3 / Tx4 / Tx5 over for
/// over, the serial as four digits, and both logs agree (the serial logged as its number).
#[test]
fn an_rst_and_serial_contest_works_a_contact_in_the_roundup_messages() {
    let rs = install("custom_air_ru", "AIR-RU", &["rst", "serial"], None);
    assert_eq!(FtMessages::for_contest(rs, "FT8"), FtMessages::RttyRoundup);
    assert_eq!(FtMessages::for_contest(rs, "FT4"), FtMessages::RttyRoundup);
    let mut run = station(rs, true, "K1ABC", "FN42", "", "");
    let mut sp = station(rs, false, "W9XYZ", "EN52", "", "");
    assert_eq!(
        overs(&mut run, &mut sp, 6, 12),
        [
            "R: CQ RU K1ABC FN42",
            "S: K1ABC W9XYZ 579 0001",
            "R: W9XYZ K1ABC R 579 0001",
            "S: K1ABC W9XYZ RR73",
            "R: W9XYZ K1ABC 73",
        ]
    );
    assert_eq!(rows(&run), ["W9XYZ rx[RST=579,NR=1] tx[RST=579,NR=1]"]);
    assert_eq!(rows(&sp), ["K1ABC rx[RST=579,NR=1] tx[RST=579,NR=1]"]);
    // The next contact gets the next serial, and the report follows the SNR it was heard at.
    run.rearm(true);
    sp.rearm(false);
    let mut sp2 = station(rs, false, "W1AW", "FN31", "", "");
    let second = overs(&mut run, &mut sp2, -30, 12);
    assert_eq!(second[1], "S: K1ABC W1AW 529 0001");
    assert_eq!(second[2], "R: W1AW K1ABC R 529 0002");
    assert_eq!(rows(&run)[1], "W1AW rx[RST=529,NR=1] tx[RST=529,NR=2]");
    // Through the modem: every over packs as the RTTY contest message and decodes back.
    let mut run = station(rs, true, "K1ABC", "FN42", "", "");
    let mut sp = station(rs, false, "W9XYZ", "EN52", "", "");
    run_loopback_fieldday(&mut run, &mut sp, 0.0, 12);
    assert_eq!(rows(&run).len(), 1, "{:?}", run.transcript);
    assert_eq!(rows(&sp).len(), 1, "{:?}", sp.transcript);
    assert!(
        rows(&run)[0].starts_with("W9XYZ rx[RST=5"),
        "{:?}",
        rows(&run)
    );
    assert!(
        rows(&run)[0].contains(",NR=1] tx[RST=5"),
        "{:?}",
        rows(&run)
    );
}

/// RST + state/province, asked for the Roundup: the state goes where the serial would.
#[test]
fn an_rst_and_state_contest_works_a_contact_in_the_roundup_messages() {
    let rs = install(
        "custom_air_ru_qth",
        "AIR-RU-QTH",
        &["rst", "state_province"],
        None,
    );
    assert_eq!(FtMessages::for_contest(rs, "FT8"), FtMessages::RttyRoundup);
    let mut run = station_in(rs, true, "K1ABC", "FN42", "", "", "MA");
    let mut sp = station_in(rs, false, "W9XYZ", "EN52", "", "", "IL");
    assert_eq!(
        overs(&mut run, &mut sp, 6, 12),
        [
            "R: CQ RU K1ABC FN42",
            "S: K1ABC W9XYZ 579 IL",
            "R: W9XYZ K1ABC R 579 MA",
            "S: K1ABC W9XYZ RR73",
            "R: W9XYZ K1ABC 73",
        ]
    );
    assert_eq!(rows(&run), ["W9XYZ rx[RST=579,QTH=IL] tx[RST=579,QTH=MA]"]);
    assert_eq!(rows(&sp), ["K1ABC rx[RST=579,QTH=MA] tx[RST=579,QTH=IL]"]);
    let mut run = station_in(rs, true, "K1ABC", "FN42", "", "", "MA");
    let mut sp = station_in(rs, false, "W9XYZ", "EN52", "", "", "IL");
    run_loopback_fieldday(&mut run, &mut sp, 0.0, 12);
    assert_eq!(rows(&run).len(), 1, "{:?}", run.transcript);
    assert!(
        rows(&run)[0].contains("QTH=IL] tx[RST="),
        "{:?}",
        rows(&run)
    );
    assert!(rows(&sp)[0].contains("QTH=MA] tx[RST="), "{:?}", rows(&sp));
}

/// The Roundup's messages pack as WSJT-X's RTTY contest message (i3=3), AK and HI too (in the
/// packer's `cmult` though WSJT-X's settings box leaves them out), and 0105 (a serial its settings
/// box turns away, which the packer takes).
#[test]
fn the_roundup_messages_pack_as_the_rtty_contest_message() {
    for text in [
        "K1ABC W9XYZ 579 0013",
        "K1ABC W9XYZ R 579 0013",
        "K1ABC W9XYZ 579 WI",
        "K1ABC W9XYZ R 579 WI",
        "K1ABC W9XYZ 579 AK",
        "K1ABC W9XYZ 579 HI",
        "K1ABC W9XYZ 579 0105",
        "K1ABC W9XYZ 529 7999",
        "TU; K1ABC W9XYZ R 579 MA",
        "K1ABC W9XYZ/QRP 579 0001",
        "K1ABC PJ4/W9XYZ 579 0001",
    ] {
        assert_eq!(i3n3(text).map(|(i3, _)| i3), Some(3), "{text}");
    }
    // A nonstandard call goes in whole: bit for bit WSJT-X's bracketed `t0b` for a suffix…
    assert_eq!(
        modes::wsjt77::ft8_tones("K1ABC W9XYZ/QRP 579 0001"),
        modes::wsjt77::ft8_tones("K1ABC <W9XYZ/QRP> 579 0001")
    );
    // …and the RTTY message for a prefix, where WSJT-X's own bracketed form is not one.
    assert_ne!(
        i3n3("K1ABC <PJ4/W9XYZ> 579 0001").map(|(i3, _)| i3),
        Some(3)
    );
    // DX is no multiplier the message carries.
    assert_ne!(i3n3("K1ABC W9XYZ 579 DX").map(|(i3, _)| i3), Some(3));
}

/// A serial past 7999 and a QTH the message has no word for (DX) are refused: nothing is sent,
/// nothing is logged, and the reason is said.
#[test]
fn the_roundup_refuses_an_exchange_its_message_cannot_carry() {
    let rs = install("custom_air_ru_lim", "AIR-RU-LIM", &["rst", "serial"], None);
    // Running at serial 8000: a caller's exchange gets no answer, the run keeps calling CQ.
    let mut run = station(rs, true, "K1ABC", "FN42", "", "");
    run.log.session.next_serial = 8000;
    let why = run.exchange_refusal().expect("refused up front");
    assert!(why.contains("serial 8000"), "{why}");
    assert!(why.contains("1 to 7999"), "{why}");
    run.observe(&[heard("K1ABC W9XYZ 579 0013")], 1);
    assert_eq!(run.outgoing().unwrap().to_text(), "CQ RU K1ABC FN42");
    assert_eq!(run.take_refusal().as_deref(), Some(why.as_str()));
    assert!(run.log.qsos().is_empty());
    run.observe(&[heard("K1ABC W9XYZ R 579 0013")], 3);
    assert!(
        run.log.qsos().is_empty(),
        "a rogered caller is not logged either"
    );
    // 7999 still goes.
    run.log.session.next_serial = 7999;
    assert_eq!(run.exchange_refusal(), None);
    run.observe(&[heard("K1ABC W9XYZ 579 0013")], 5);
    assert_eq!(run.outgoing().unwrap().to_text(), "W9XYZ K1ABC R 549 7999");
    // S&P at serial 8000: the double-click is refused, S&P keeps listening.
    let mut sp = station(rs, false, "W9XYZ", "EN52", "", "");
    sp.log.session.next_serial = 8000;
    sp.answer_cq("K1ABC", "FN42", 6);
    assert!(sp.outgoing().is_none());
    assert_eq!(sp.state, tempo_core::fieldday::FdState::Listening);
    assert!(sp.take_refusal().unwrap().contains("serial 8000"));
    // A state the message has no word for.
    let rs = install(
        "custom_air_ru_dx",
        "AIR-RU-DX",
        &["rst", "state_province"],
        None,
    );
    let mut sp = station_in(rs, false, "W9XYZ", "EN52", "", "", "DX");
    let why = sp.exchange_refusal().expect("DX refused up front");
    assert!(why.contains("Your QTH \"DX\""), "{why}");
    sp.answer_cq("K1ABC", "FN42", 6);
    assert!(sp.outgoing().is_none());
    assert_eq!(sp.take_refusal().as_deref(), Some(why.as_str()));
    assert!(sp.log.qsos().is_empty());
}

/// `TU; ` before the runner's rogered exchange (FT4's WSJT-X sends it) is read past: logged and
/// closed with RR73. A first call with a grid is answered with the exchange, as WSJT-X's Tx2.
#[test]
fn the_roundup_reads_tu_and_answers_a_grid_caller_with_the_exchange() {
    let rs = install("custom_air_ru_tu", "AIR-RU-TU", &["rst", "serial"], None);
    let mut sp = station(rs, false, "W9XYZ", "EN52", "", "");
    sp.answer_cq("K1ABC", "FN42", 6);
    assert_eq!(sp.outgoing().unwrap().to_text(), "K1ABC W9XYZ 579 0001");
    sp.observe(&[heard("TU; W9XYZ K1ABC R 559 0042")], 1);
    assert_eq!(sp.outgoing().unwrap().to_text(), "K1ABC W9XYZ RR73");
    assert_eq!(rows(&sp), ["K1ABC rx[RST=559,NR=42] tx[RST=579,NR=1]"]);
    let mut run = station(rs, true, "K1ABC", "FN42", "", "");
    run.observe(&[heard("K1ABC W9XYZ EN52")], 1);
    assert_eq!(run.outgoing().unwrap().to_text(), "W9XYZ K1ABC 549 0001");
    run.after_tx();
    run.observe(&[heard("K1ABC W9XYZ R 579 0007")], 3);
    assert_eq!(run.outgoing().unwrap().to_text(), "W9XYZ K1ABC RR73");
    assert_eq!(rows(&run), ["W9XYZ rx[RST=579,NR=7] tx[RST=549,NR=1]"]);
    // A message from `<...>` is passed by (mainwindow.cpp 9098-9099).
    let mut run = station(rs, true, "K1ABC", "FN42", "", "");
    run.observe(&[heard("K1ABC <...> 579 0013")], 1);
    assert_eq!(run.outgoing().unwrap().to_text(), "CQ RU K1ABC FN42");
}

/// Nonstandard calls in the Roundup: each goes in whole, and the over packs as the RTTY message.
#[test]
fn the_roundup_carries_a_nonstandard_call_whole() {
    let rs = install("custom_air_ru_ns", "AIR-RU-NS", &["rst", "serial"], None);
    for me in ["K1ABC/QRP", "PJ4/K1ABC"] {
        let mut sp = station(rs, false, me, "FN42", "", "");
        sp.answer_cq("W9XYZ", "EN52", 6);
        let text = sp.outgoing().unwrap().to_text();
        assert_eq!(text, format!("W9XYZ {me} 579 0001"));
        assert_eq!(i3n3(&text).map(|(i3, _)| i3), Some(3), "{text}");
        // The runner's rogered exchange to me, my call hashed as the decoder prints it.
        sp.observe(&[heard(&format!("<{me}> W9XYZ R 579 0003"))], 1);
        assert_eq!(
            rows(&sp),
            ["W9XYZ rx[RST=579,NR=3] tx[RST=579,NR=1]"],
            "{me}"
        );
    }
}
