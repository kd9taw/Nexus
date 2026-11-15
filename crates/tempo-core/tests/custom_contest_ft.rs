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
        ft_roundup: false,
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
    let data = StationData {
        mycall: call.into(),
        mygrid: grid.into(),
        fd_class: class.into(),
        fd_section: section.into(),
        contest_qth_state: "MA".into(),
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
                rx.answer_cq(&de, &grid);
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
    let cqs = [
        "CQ FD K1ABC FN42",
        "CQ TEST K1ABC FN42",
        "CQ WW K1ABC FN42",
        "CQ K1ABC FN42",
        "CQ PJ4/K1ABC",
    ];
    sp_answers_no_cq_by_itself(fd, &cqs);
    sp_answers_no_cq_by_itself(vhf, &cqs);
    sp_answers_no_cq_by_itself(ww, &cqs);
}
