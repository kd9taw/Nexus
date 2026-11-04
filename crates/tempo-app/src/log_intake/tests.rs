//! The contact port's intake, by value, through the importer the listener runs: who may send,
//! what each form becomes in the log, the refusals and the bounds. Every WSJT-X datagram is built
//! by Nexus's own encoder; the ADIF and N1MM XML are written by hand. None is a capture of
//! another program's: the S.A.T.-shaped record below is a GUESS at that box's fields (its manual
//! documents the push, not the fields), to be replaced by a real capture.

use super::*;
use crate::freedv::{take_log_datagram, Note};
use crate::test_util::StoredLog;
use tempo_net::wsjtx::{encode_logged_adif, encode_qso_logged, QsoLogged};

/// 2026-10-14 01:02:03 UTC.
const WHEN: i64 = 1_791_939_723;

/// The lines a note was given, `(level, message)`, in order.
type Lines = Arc<Mutex<Vec<(String, String)>>>;

fn lines_note() -> (Note, Lines) {
    let lines: Lines = Arc::new(Mutex::new(Vec::new()));
    let l = Arc::clone(&lines);
    let note: Note = Arc::new(move |level, msg| {
        l.lock().unwrap().push((level.to_string(), msg));
    });
    (note, lines)
}

/// An engine whose station call is K2DEF, a gate, and the intake's note.
struct Station {
    e: Mutex<Engine>,
    gate: Gate,
    note: Note,
    lines: Lines,
    freedv_note: Note,
}

impl Station {
    fn new() -> Self {
        let (note, lines) = lines_note();
        let (freedv_note, _) = lines_note();
        Self {
            e: Mutex::new(Engine::new("K2DEF", "FN31", 0)),
            gate: Gate::new(),
            note,
            lines,
            freedv_note,
        }
    }

    fn send_from(&mut self, from: &str, bytes: &[u8]) {
        let from: SocketAddr = from.parse().unwrap();
        take_datagram(
            &self.e,
            bytes,
            from,
            &mut self.gate,
            &self.freedv_note,
            &self.note,
        );
    }

    fn send(&mut self, bytes: &[u8]) {
        self.send_from("127.0.0.1:50000", bytes);
    }

    /// Let every held type 5 go, as the listener does once its hold runs out.
    fn settle(&mut self) {
        flush_held(
            &self.e,
            &mut self.gate,
            &self.note,
            Instant::now() + HOLD * 2,
        );
    }

    fn calls(&self) -> Vec<String> {
        self.e.stored_log().iter().map(|r| r.call.clone()).collect()
    }

    fn lines(&self) -> Vec<(String, String)> {
        self.lines.lock().unwrap().clone()
    }

    /// Change the settings, and have the gate read them again at once.
    fn settings(&mut self, f: impl FnOnce(&mut Settings)) {
        {
            let mut e = engine_lock(&self.e);
            let mut s = e.settings().clone();
            f(&mut s);
            e.apply_settings(s);
        }
        self.gate.admission_read = None;
    }
}

fn qso<'a>(my_call: &'a str, dx_call: &'a str) -> QsoLogged<'a> {
    QsoLogged {
        time_off: WHEN + 60,
        time_on: WHEN,
        dx_call,
        dx_grid: "FN42",
        tx_freq: 14_075_500,
        mode: "FT8",
        report_sent: "-10",
        report_recvd: "-12",
        my_call,
        my_grid: "FN31",
        ..Default::default()
    }
}

fn via(r: &QsoRecord) -> Option<&str> {
    r.extra
        .iter()
        .find(|(k, _)| k == VIA_FIELD)
        .map(|(_, v)| v.as_str())
}

/// A raw ADIF record for `call` on 20 m FT8 at `unix`.
fn ft8_at(call: &str, unix: i64) -> String {
    let (d, t) = tempo_net::log_intake::adif_time(unix).unwrap();
    format!(
        "<CALL:{}>{call}<QSO_DATE:8>{d}<TIME_ON:6>{t}<BAND:3>20m<MODE:3>FT8<FREQ:9>14.075500<EOR>",
        call.len()
    )
}

/// Finding: before the contact port took other programs' contacts, an imported contact (FreeDV's)
/// was never queued for QRZ, Club Log, eQSL and the rest: only the log funnel queued uploads.
#[test]
fn a_freedv_contact_is_queued_for_the_service_uploads() {
    let e = Mutex::new(Engine::new("K2DEF", "FN31", 0));
    let note: Note = Arc::new(|_, _| {});
    let d = encode_qso_logged(
        "FreeDV",
        &QsoLogged {
            time_off: WHEN,
            time_on: WHEN,
            dx_call: "W5FDV",
            tx_freq: 14_236_000,
            mode: "DIGITALVOICE",
            my_call: "K2DEF",
            ..Default::default()
        },
    );
    take_log_datagram(&e, &d, &note);
    let queued = engine_lock(&e).take_pending_uploads();
    assert_eq!(queued.len(), 1, "the contact owes the services");
    assert_eq!(queued[0].rec.call, "W5FDV");
    assert_eq!(via(&queued[0].rec), Some("FreeDV"));
    let f = engine_lock(&e).freedv_mut().clone();
    assert_eq!(f.intake_program.as_deref(), Some("FreeDV"));
    assert_eq!(f.intake_tick, 1);
}

#[test]
fn a_wsjtx_contact_from_any_program_lands_in_the_log_via_that_program() {
    let mut st = Station::new();
    st.send(&encode_qso_logged("JTDX", &qso("k2def", "K1ABC")));
    st.settle(); // a type 5 waits for its type 12, which this sender never sends
    let log = st.e.stored_log();
    assert_eq!(log.len(), 1, "{:?}", st.lines());
    let r = &log[0];
    assert_eq!(r.call, "K1ABC");
    assert_eq!(r.band, "20m", "the band from the frequency");
    assert_eq!(r.mode, "FT8");
    assert_eq!(r.grid.as_deref(), Some("FN42"));
    assert_eq!(r.station_callsign.as_deref(), Some("K2DEF"));
    assert_eq!(r.when_unix, WHEN as u64);
    assert_eq!(via(r), Some("JTDX"));
    // …and kept through the log's own file: the via mark round-trips.
    let adif = tempo_core::logbook::adif_record_own_log(r);
    assert!(adif.contains("<APP_NEXUS_VIA:4>JTDX"), "{adif}");
    let back = tempo_core::logbook::parse_adif(&adif);
    assert_eq!(via(&back[0]), Some("JTDX"));
    let f = engine_lock(&st.e).freedv_mut().clone();
    assert_eq!(
        (
            f.intake_program.as_deref(),
            f.intake_call.as_deref(),
            f.intake_tick
        ),
        (Some("JTDX"), Some("K1ABC"), 1)
    );
    assert!(f.intake_unix.is_some());
    assert_eq!(
        st.lines(),
        [(
            "info".to_string(),
            "JTDX logged K1ABC · 14.075 MHz: in the log".to_string()
        )]
    );
    // Queued for the services.
    let queued = engine_lock(&st.e).take_pending_uploads();
    assert_eq!(queued.len(), 1);
}

#[test]
fn type5_then_type12_from_one_sender_logs_once() {
    let mut st = Station::new();
    st.send(&encode_qso_logged("WSJT-X", &qso("K2DEF", "K1ABC")));
    st.send(&encode_logged_adif(
        "WSJT-X",
        "<call:5>K1ABC<gridsquare:4>FN42<mode:3>FT8<band:3>20m<freq:9>14.075500\
         <qso_date:8>20261014<time_on:6>010203<qso_date_off:8>20261014<time_off:6>010303\
         <station_callsign:5>K2DEF<EOR>",
    ));
    assert_eq!(st.calls(), ["K1ABC"]);
    assert_eq!(
        st.lines().len(),
        1,
        "one line for one contact: {:?}",
        st.lines()
    );
    assert_eq!(engine_lock(&st.e).freedv_mut().intake_tick, 1);
    assert_eq!(engine_lock(&st.e).take_pending_uploads().len(), 1);
}

/// Hand-written in the shape the S.A.T. box's "send to Log4OM / HRD" push is EXPECTED to take
/// (raw ADIF over UDP, satellite fields, uplink and downlink). UNVERIFIED: no capture yet.
#[test]
fn a_sat_shaped_adif_keeps_sat_name_prop_mode_freq_rx_and_gets_its_bands() {
    let mut st = Station::new();
    let d = "<CALL:5>W1ABC<QSO_DATE:8>20261014<TIME_ON:6>010203<MODE:2>FM<FREQ:7>145.850\
             <FREQ_RX:7>436.795<PROP_MODE:3>SAT<SAT_NAME:5>SO-50<GRIDSQUARE:4>FN42\
             <MY_GRIDSQUARE:4>FN31<RST_SENT:2>59<RST_RCVD:2>59<EOR>";
    st.send_from("127.0.0.1:40000", d.as_bytes());
    let log = st.e.stored_log();
    assert_eq!(log.len(), 1, "{:?}", st.lines());
    let r = &log[0];
    assert_eq!(r.sat_name.as_deref(), Some("SO-50"));
    assert_eq!(r.prop_mode.as_deref(), Some("SAT"));
    assert_eq!(r.band, "2m", "the uplink's band");
    let adif = tempo_core::logbook::adif_record_own_log(r);
    assert!(adif.contains("<FREQ_RX:10>436.795000"), "{adif}");
    assert!(adif.contains("<BAND_RX:4>70cm"), "{adif}");
    // No program name: this computer, never its address.
    assert_eq!(via(r), Some(THIS_COMPUTER_WORDS));
}

#[test]
fn a_contact_under_another_station_call_is_refused_with_a_sentence() {
    let mut st = Station::new();
    st.send(&encode_qso_logged("WSJT-X", &qso("W1AW", "K1ABC")));
    assert!(st.calls().is_empty(), "never written under the wrong call");
    let why = "WSJT-X logged K1ABC as W1AW, not imported: this station is K2DEF";
    assert_eq!(st.lines(), [("error".to_string(), why.to_string())]);
    assert_eq!(engine_lock(&st.e).freedv_mut().notice.as_deref(), Some(why));
    assert!(engine_lock(&st.e).take_pending_uploads().is_empty());
}

#[test]
fn one_naming_no_call_is_stamped_with_ours() {
    let mut st = Station::new();
    st.send(b"<CALL:5>W1ABC<QSO_DATE:8>20261014<TIME_ON:4>0102<MODE:2>CW<FREQ:6>7.0300<EOR>");
    let log = st.e.stored_log();
    assert_eq!(log.len(), 1, "{:?}", st.lines());
    assert_eq!(log[0].station_callsign.as_deref(), Some("K2DEF"));
    assert_eq!(log[0].band, "40m");
}

#[test]
fn a_contact_with_no_call_or_no_time_is_refused() {
    let mut st = Station::new();
    for d in [
        &b"<CALL:8>NOTACALL<QSO_DATE:8>20261014<TIME_ON:4>0102<EOR>"[..],
        b"<CALL:5>W1ABC<TIME_ON:4>0102<EOR>",
        b"<CALL:5>W1ABC<QSO_DATE:8>19991231<TIME_ON:4>0102<EOR>",
        b"<CALL:5>W1ABC<QSO_DATE:8>20261014<TIME_ON:4>2502<EOR>",
    ] {
        st.send(d);
    }
    assert!(st.calls().is_empty());
    assert_eq!(st.lines().len(), 4, "each refused with its reason");
    assert!(st.lines().iter().all(|(level, _)| level == "error"));
}

/// The log funnel's 300 s duplicate check, called as it is: a contact the operator logged in
/// Nexus two minutes before the other program's arrives is not written twice.
#[test]
fn a_contact_the_operator_logged_two_minutes_earlier_is_a_duplicate() {
    let mut st = Station::new();
    let mine = tempo_core::logbook::parse_adif(&ft8_at("K1ABC", WHEN))
        .pop()
        .unwrap();
    engine_lock(&st.e).log_qso(mine);
    assert_eq!(st.calls(), ["K1ABC"], "premise: the operator's contact");
    engine_lock(&st.e).take_pending_uploads();
    st.send(ft8_at("K1ABC", WHEN + 120).as_bytes());
    assert_eq!(st.calls(), ["K1ABC"], "one contact");
    assert_eq!(
        st.lines()[0].1,
        "This computer (127.0.0.1) logged K1ABC · 14.075 MHz: already in the log"
    );
    assert!(engine_lock(&st.e).take_pending_uploads().is_empty());
    // Control: ten minutes on it is a contact of its own.
    st.send(ft8_at("K1ABC", WHEN + 600).as_bytes());
    assert_eq!(st.calls(), ["K1ABC", "K1ABC"]);
}

/// "Accept from devices on my network" off (the default): a datagram from a network address is
/// not read, and said once. On: a private address is read.
#[test]
fn a_lan_datagram_is_dropped_unless_lan_is_on() {
    let mut st = Station::new();
    let d = encode_qso_logged("WSJT-X", &qso("K2DEF", "K1ABC"));
    st.send_from("192.168.1.40:5000", &d);
    st.send_from("192.168.1.40:5000", &d);
    assert!(st.calls().is_empty());
    assert_eq!(st.lines().len(), 1, "said once: {:?}", st.lines());
    assert!(st.lines()[0].1.contains("not from this computer"));
    st.settings(|s| s.log_intake_lan = true);
    st.send_from("192.168.1.40:5000", &d);
    st.settle();
    assert_eq!(st.calls(), ["K1ABC"]);
}

#[test]
fn a_public_source_is_dropped_even_with_lan_on() {
    let mut st = Station::new();
    st.settings(|s| s.log_intake_lan = true);
    let d = encode_qso_logged("WSJT-X", &qso("K2DEF", "K1ABC"));
    for from in ["8.8.8.8:5000", "[2001:db8::1]:5000", "224.0.0.1:5000"] {
        st.send_from(from, &d);
    }
    assert!(st.calls().is_empty());
    st.send_from("10.0.0.9:5000", &d);
    st.settle();
    assert_eq!(st.calls(), ["K1ABC"], "control: a private source is read");
}

#[test]
fn a_pinned_peer_admits_only_itself() {
    let mut st = Station::new();
    st.settings(|s| {
        s.log_intake_lan = true;
        s.log_intake_peer = "192.168.1.40".into();
    });
    st.send_from(
        "192.168.1.41:5000",
        &encode_qso_logged("WSJT-X", &qso("K2DEF", "K1ABC")),
    );
    assert!(st.calls().is_empty());
    st.send_from(
        "192.168.1.40:5000",
        &encode_qso_logged("WSJT-X", &qso("K2DEF", "K1ABC")),
    );
    st.send_from(
        "127.0.0.1:5000",
        &encode_qso_logged("WSJT-X", &qso("K2DEF", "W1XYZ")),
    );
    st.settle();
    assert_eq!(st.calls(), ["K1ABC", "W1XYZ"], "the pin, and this computer");
    // A pin that is no address takes no network device at all.
    let a = Admission {
        lan: true,
        pin: None,
        pin_unreadable: true,
    };
    assert!(a.admits("192.168.1.40".parse().unwrap()).is_err());
    assert!(a.admits("127.0.0.1".parse().unwrap()).is_ok());
}

#[test]
fn the_rate_limit_drops_past_the_bucket_and_says_so_once() {
    let t0 = Instant::now();
    let mut limit = RateLimit::new(t0);
    let a: IpAddr = "127.0.0.1".parse().unwrap();
    let b: IpAddr = "127.0.0.2".parse().unwrap();
    let taken = (0..100).filter(|_| limit.take(a, t0)).count();
    assert_eq!(taken, SOURCE_BURST as usize, "one source's burst");
    // Another source has what is left of the total…
    let taken_b = (0..100).filter(|_| limit.take(b, t0)).count();
    assert_eq!(taken_b, (TOTAL_BURST - SOURCE_BURST) as usize);
    // …and a minute on, each has its minute's worth again.
    let t1 = t0 + Duration::from_secs(60);
    let again = (0..100).filter(|_| limit.take(a, t1)).count();
    assert_eq!(again, SOURCE_PER_MINUTE as usize);

    // Through the listener: past the burst, dropped, and said once.
    let mut st = Station::new();
    let burst = SOURCE_BURST as usize;
    for i in 0..(burst + 10) {
        let call = format!("W{}A{}", i % 10, (b'A' + (i / 10) as u8) as char);
        st.send(ft8_at(&call, WHEN + i as i64 * 1000).as_bytes());
    }
    assert_eq!(st.calls().len(), burst);
    let refusals: Vec<_> = st
        .lines()
        .into_iter()
        .filter(|(level, l)| level == "error" && l.contains("faster than"))
        .collect();
    assert_eq!(refusals.len(), 1, "said once: {refusals:?}");
}

#[test]
fn too_many_contacts_in_one_datagram_are_refused_and_said_once() {
    let mut st = Station::new();
    let d = ft8_at("W1ABC", WHEN).repeat(tempo_net::log_intake::MAX_RECORDS + 1);
    st.send(d.as_bytes());
    st.send(d.as_bytes());
    assert!(st.calls().is_empty());
    assert_eq!(st.lines().len(), 1);
    assert!(st.lines()[0].1.contains("more than 5 contacts"));
}

#[test]
fn an_n1mm_contactinfo_lands_in_the_log_via_n1mm() {
    let mut st = Station::new();
    let x = r#"<?xml version="1.0" encoding="utf-8"?>
<contactinfo><app>N1MM</app><contestname>DX</contestname><timestamp>2026-10-14 01:02:03</timestamp>
<mycall>K2DEF</mycall><band>14</band><rxfreq>1402500</rxfreq><txfreq>1402500</txfreq>
<mode>CW</mode><call>DL1ABC</call><snt>599</snt><rcv>579</rcv><IsClaimedQso>1</IsClaimedQso>
</contactinfo>"#;
    st.send(x.as_bytes());
    let log = st.e.stored_log();
    assert_eq!(log.len(), 1, "{:?}", st.lines());
    let r = &log[0];
    assert_eq!(r.call, "DL1ABC");
    assert!((r.freq_mhz - 14.025).abs() < 1e-9, "{}", r.freq_mhz);
    assert_eq!(r.band, "20m");
    assert_eq!(r.rst_rcvd.as_deref(), Some("579"));
    assert_eq!(via(r), Some("N1MM"));
}

/// Nexus's own datagrams sent back to the port (a WSJT-X target pointed at it) log nothing.
#[test]
fn nexus_own_datagrams_log_nothing() {
    let mut st = Station::new();
    st.send(&encode_qso_logged("Tempo", &qso("K2DEF", "K1ABC")));
    assert!(st.calls().is_empty());
    assert!(st.lines().is_empty());
}

#[test]
fn an_intake_port_equal_to_an_outbound_target_is_refused() {
    let mut s = Settings::default();
    assert_eq!(wanted(&s), Ok(Some((2239, false))), "the default listens");
    s.log_intake_lan = true;
    assert_eq!(wanted(&s), Ok(Some((2239, true))));
    s.wsjtx_udp = true;
    s.wsjtx_udp_addr = "127.0.0.1:2237, 127.0.0.1:2239".into();
    let why = wanted(&s).unwrap_err();
    assert!(
        why.contains("port 2239") && why.contains("WSJT-X UDP"),
        "{why}"
    );
    // Off, that target sends nothing, and the port is free to listen on.
    s.wsjtx_udp = false;
    assert!(wanted(&s).is_ok());
    s.hrd_logging = true;
    s.hrd_udp_addr = "127.0.0.1:2239".into();
    assert!(wanted(&s).unwrap_err().contains("Ham Radio Deluxe"));
    s.hrd_logging = false;
    s.n1mm_addr = "192.168.1.255:2239".into();
    assert!(wanted(&s).unwrap_err().contains("N1MM"));
    s.n1mm_addr = "192.168.1.255".into();
    assert!(wanted(&s).is_ok(), "N1MM's own port is 12060");
    s.freedv_log = false;
    assert_eq!(wanted(&s), Ok(None));
}

/// The whole intake, not only the reader, on random bytes and damaged datagrams of every form,
/// from this computer: nothing panics, and nothing but whole contacts reach the log.
#[test]
fn damaged_datagrams_never_panic_the_intake() {
    let mut st = Station::new();
    let mut seed: u64 = 0xdead_beef_0bad_f00d;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let goods: Vec<Vec<u8>> = vec![
        encode_qso_logged("WSJT-X", &qso("K2DEF", "K1ABC")),
        encode_logged_adif(
            "JTDX",
            "<call:5>W1ABC<qso_date:8>20261014<time_on:6>010203<mode:3>FT8<EOR>",
        ),
        b"<CALL:5>W2ABC<QSO_DATE:8>20261014<TIME_ON:4>0102<MODE:2>CW<EOR>".to_vec(),
        br#"<?xml version="1.0"?><contactinfo><timestamp>2026-10-14 01:02:03</timestamp><txfreq>1402500</txfreq><mode>CW</mode><call>W3ABC</call></contactinfo>"#.to_vec(),
    ];
    for i in 0..2_000 {
        let good = &goods[i % goods.len()];
        let mut bytes = match (i / goods.len()) % 3 {
            0 => (0..(next() % 500) as usize).map(|_| next() as u8).collect(),
            1 => good[..(next() as usize % good.len())].to_vec(),
            _ => good.clone(),
        };
        if !bytes.is_empty() && (i / goods.len()) % 3 == 2 {
            let at = next() as usize % bytes.len();
            bytes[at] ^= 1 << (next() % 8);
        }
        // The rate limit is not what this is about.
        st.gate.limit = RateLimit::new(Instant::now());
        st.send(&bytes);
    }
    let log = st.e.stored_log();
    assert!(!log.is_empty(), "control: whole contacts did get through");
    for r in log.iter() {
        assert!(tempo_core::message::looks_like_call(&r.call), "{}", r.call);
        assert_eq!(r.station_callsign.as_deref(), Some("K2DEF"));
    }
}

#[test]
fn private_means_private() {
    for (ip, private) in [
        ("10.1.2.3", true),
        ("172.16.0.1", true),
        ("172.32.0.1", false),
        ("192.168.0.1", true),
        ("169.254.1.1", true),
        ("100.64.0.1", false),
        ("8.8.8.8", false),
        ("255.255.255.255", false),
        ("0.0.0.0", false),
        ("fd12::1", true),
        ("fe80::1", true),
        ("2001:db8::1", false),
        ("::ffff:192.168.1.5", true),
        ("::ffff:8.8.8.8", false),
    ] {
        assert_eq!(is_private(ip.parse().unwrap()), private, "{ip}");
    }
}

/// A WSJT-X Logged ADIF (type 12) as WSJT-X frames it (`MessageClient::logged_ADIF`): a header
/// naming the program, `<EOH>`, the record, ` <EOR>`.
fn wsjtx_envelope(record: &str) -> String {
    format!("\n<adif_ver:5>3.1.0\n<programid:6>WSJT-X\n<EOH>\n{record} <EOR>")
}

const COMPANION_RECORD: &str = "<call:5>W1ABC <gridsquare:4>FN42 <mode:3>FT8 <rst_sent:3>-10 \
    <rst_rcvd:3>-12 <qso_date:8>20261014 <time_on:6>010203 <band:3>20m <freq:9>14.075500 \
    <station_callsign:5>K2DEF";

/// A contact WSJT-X logs in companion mode (its type 12 off the companion socket) goes the
/// contact port's road: queued for the uploads exactly as one that arrives on 2239, via WSJT-X,
/// and sent to no local logger.
#[test]
fn a_companion_type12_is_queued_for_the_uploads_like_a_contact_port_one() {
    let companion = Station::new();
    {
        let mut e = engine_lock(&companion.e);
        let mut s = e.settings().clone();
        s.wsjtx_udp = true;
        s.hrd_logging = true;
        e.apply_settings(s);
    }
    let made = take_companion_adif(&companion.e, &wsjtx_envelope(COMPANION_RECORD));
    assert_eq!(made, [Ok(IntakeOutcome::Added)]);
    let mut port = Station::new();
    port.send(&encode_logged_adif("WSJT-X", COMPANION_RECORD));
    let queued = |st: &Station| {
        let mut e = engine_lock(&st.e);
        let q: Vec<_> = e
            .take_pending_uploads()
            .into_iter()
            .map(|p| {
                (
                    p.rec.call.clone(),
                    p.legs,
                    p.origin,
                    via(&p.rec).map(String::from),
                )
            })
            .collect();
        q
    };
    let (by_companion, by_port) = (queued(&companion), queued(&port));
    assert_eq!(by_companion.len(), 1);
    assert_eq!(by_companion, by_port, "queued exactly alike");
    assert_eq!(by_companion[0].3.as_deref(), Some("WSJT-X"));
    let mut e = engine_lock(&companion.e);
    assert!(
        e.take_pending_udp_qsos().is_empty(),
        "no WSJT-X UDP forward"
    );
    assert!(e.take_hrd_pending().is_empty(), "no HRD forward");
    assert_eq!(e.freedv_mut().intake_tick, 1, "the toast");
}

/// The same contact arriving on both roads, either one first, is one contact.
#[test]
fn a_contact_on_the_companion_socket_and_the_contact_port_is_imported_once() {
    let mut st = Station::new();
    take_companion_adif(&st.e, &wsjtx_envelope(COMPANION_RECORD));
    st.send(&encode_logged_adif("WSJT-X", COMPANION_RECORD));
    assert_eq!(st.calls(), ["W1ABC"]);
    assert_eq!(engine_lock(&st.e).take_pending_uploads().len(), 1);

    let mut st = Station::new();
    st.send(&encode_logged_adif("WSJT-X", COMPANION_RECORD));
    let made = take_companion_adif(&st.e, &wsjtx_envelope(COMPANION_RECORD));
    assert_eq!(made, [Ok(IntakeOutcome::InLog)]);
    assert_eq!(st.calls(), ["W1ABC"]);
    assert_eq!(engine_lock(&st.e).take_pending_uploads().len(), 1);
}

/// The companion road keeps the contact port's station-call refusal: another station call is not
/// written. (Nexus's own record is kept out upstream, by its sender id.)
#[test]
fn the_companion_road_refuses_another_station_call() {
    let st = Station::new();
    let theirs = COMPANION_RECORD.replace("<station_callsign:5>K2DEF", "<station_callsign:4>W1AW");
    let made = take_companion_adif(&st.e, &wsjtx_envelope(&theirs));
    assert!(
        matches!(&made[..], [Err(why)] if why.contains("this station is K2DEF")),
        "{made:?}"
    );
    assert!(st.calls().is_empty());
    // Control: under ours it is written.
    assert_eq!(
        take_companion_adif(&st.e, &wsjtx_envelope(COMPANION_RECORD)),
        [Ok(IntakeOutcome::Added)]
    );
}

/// A QSO Logged (type 5) as stock WSJT-X frames it: it ends at the propagation mode
/// (`MessageClient::qso_logged`), with no satellite name, satellite mode or receive frequency
/// after it. A satellite contact therefore says PROP_MODE SAT and names no bird.
fn stock_type5_sat(call: &str) -> Vec<u8> {
    use tempo_net::qds::QdsWriter;
    let mut w = QdsWriter::new();
    w.put_u32(tempo_net::wsjtx::MAGIC)
        .put_u32(3)
        .put_u32(5)
        .put_utf8(Some("WSJT-X"));
    w.put_qdatetime(WHEN + 30)
        .put_utf8(Some(call))
        .put_utf8(Some("FN42"))
        .put_u64(145_900_000)
        .put_utf8(Some("FT4"))
        .put_utf8(Some("-10"))
        .put_utf8(Some("-12"))
        .put_utf8(Some(""))
        .put_utf8(Some(""))
        .put_utf8(Some(""))
        .put_qdatetime(WHEN)
        .put_utf8(Some(""))
        .put_utf8(Some("K2DEF"))
        .put_utf8(Some("FN31"))
        .put_utf8(Some(""))
        .put_utf8(Some(""))
        .put_utf8(Some("SAT"));
    w.into_bytes()
}

/// The type 12 WSJT-X sends right after it, which names the bird.
fn type12_sat(call: &str) -> Vec<u8> {
    encode_logged_adif(
        "WSJT-X",
        &format!(
            "<call:{}>{call}<mode:4>MFSK<submode:3>FT4<band:2>2m<freq:10>145.900000\
             <qso_date:8>20261014<time_on:6>010203<station_callsign:5>K2DEF\
             <prop_mode:3>SAT<sat_name:5>RS-44<EOR>",
            call.len()
        ),
    )
}

/// Finding (review): a WSJT-X satellite contact lost its SAT_NAME. The type 5 logged first with
/// PROP_MODE SAT alone (which TQSL refuses, with the batch), and the type 12 naming the bird met
/// the duplicate check. The type 5 is now held a moment for its type 12, which is logged instead.
#[test]
fn a_wsjtx_satellite_contact_keeps_its_sat_name_from_the_type12() {
    let mut st = Station::new();
    st.send(&stock_type5_sat("K1ABC"));
    st.send(&type12_sat("K1ABC"));
    st.settle();
    let log = st.e.stored_log();
    assert_eq!(log.len(), 1, "{:?}", st.lines());
    assert_eq!(log[0].prop_mode.as_deref(), Some("SAT"));
    assert_eq!(log[0].sat_name.as_deref(), Some("RS-44"), "SAT_NAME lost");
    assert_eq!(st.lines().len(), 1, "one line: {:?}", st.lines());
}

/// A type 5 whose type 12 never comes (FreeDV-like senders, a program that sends only type 5) is
/// logged once the hold runs out, as it was before.
#[test]
fn a_type5_whose_type12_never_comes_is_logged_after_the_hold() {
    let mut st = Station::new();
    st.send(&encode_qso_logged("JTDX", &qso("K2DEF", "K1ABC")));
    assert!(st.calls().is_empty(), "held for its type 12");
    flush_held(&st.e, &mut st.gate, &st.note, Instant::now() + HOLD / 2);
    assert!(st.calls().is_empty(), "still inside the hold");
    st.settle();
    assert_eq!(st.calls(), ["K1ABC"]);
    // A second type 5 lets the first go at once rather than replace it.
    let mut st = Station::new();
    st.send(&encode_qso_logged("JTDX", &qso("K2DEF", "K1ABC")));
    st.send(&encode_qso_logged("JTDX", &qso("K2DEF", "W1XYZ")));
    assert_eq!(st.calls(), ["K1ABC"]);
    st.settle();
    assert_eq!(st.calls(), ["K1ABC", "W1XYZ"]);
}

/// The type 12 first: it is logged at once, and the type 5 after it is the same contact, let go
/// without a line once its hold runs out.
#[test]
fn a_type12_first_then_its_type5_logs_once() {
    let mut st = Station::new();
    st.send(&type12_sat("K1ABC"));
    assert_eq!(st.calls(), ["K1ABC"]);
    st.send(&stock_type5_sat("K1ABC"));
    st.settle();
    let log = st.e.stored_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].sat_name.as_deref(), Some("RS-44"));
    assert_eq!(st.lines().len(), 1, "{:?}", st.lines());
}

/// A date that does not exist is refused, never rolled over into the next month.
#[test]
fn an_impossible_date_is_refused() {
    let mut st = Station::new();
    for d in ["20260231", "20260431", "20250229", "21000101"] {
        st.send(format!("<CALL:5>W1ABC<QSO_DATE:8>{d}<TIME_ON:4>0102<EOR>").as_bytes());
    }
    assert!(st.calls().is_empty(), "{:?}", st.lines());
    st.send(b"<CALL:5>W1ABC<QSO_DATE:8>20240229<TIME_ON:4>0102<EOR>");
    assert_eq!(st.calls(), ["W1ABC"], "control: a leap day");
}

/// Nexus's own JS8 API UDP target is one of the ports it sends to, refused for the contact port.
#[test]
fn the_js8_api_udp_port_is_refused_for_the_contact_port() {
    let mut s = Settings::default();
    s.freedv_log_port = s.js8_api_udp_port;
    assert!(wanted(&s).is_ok(), "off, it sends nothing");
    s.js8_api_udp = true;
    assert!(wanted(&s).unwrap_err().contains("JS8"), "{:?}", wanted(&s));
}

/// FreeDV's "I am here" datagrams are rate-limited as contacts are, in a bucket of their own so
/// presence can never starve a contact.
#[test]
fn freedvs_presence_is_rate_limited_and_cannot_starve_a_contact() {
    let mut st = Station::new();
    let hb = tempo_net::wsjtx::encode_heartbeat("FreeDV", 3, "2.4.0", "");
    for _ in 0..200 {
        st.send(&hb);
    }
    assert!(
        !st.gate
            .alive
            .take("127.0.0.1".parse().unwrap(), Instant::now()),
        "the presence bucket is spent"
    );
    // A contact still goes in.
    take_datagram(
        &st.e,
        &tempo_net::wsjtx::encode_qso_logged(
            "FreeDV",
            &QsoLogged {
                time_off: WHEN,
                time_on: WHEN,
                dx_call: "W5FDV",
                tx_freq: 14_236_000,
                mode: "DIGITALVOICE",
                my_call: "K2DEF",
                ..Default::default()
            },
        ),
        "127.0.0.1:5000".parse().unwrap(),
        &mut st.gate,
        &st.freedv_note,
        &st.note,
    );
    assert_eq!(st.calls(), ["W5FDV"]);
}

/// What the system says the contact port is bound to, as `/proc/net/udp` writes it
/// (`ADDR:PORT` in hex): `0100007F` is 127.0.0.1, `00000000` every adapter.
#[cfg(target_os = "linux")]
fn bound_addrs(port: u16) -> Vec<String> {
    std::fs::read_to_string("/proc/net/udp")
        .unwrap_or_default()
        .lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
        .filter(|a| a.ends_with(&format!(":{port:04X}")))
        .collect()
}

/// ⛔ THE BIND, read back from the system while the real listener manager runs: with "Accept from
/// devices on my network" off the port is on 127.0.0.1 alone, never every adapter; on, every
/// adapter; off again, 127.0.0.1; and a port Nexus sends to is let go, with the reason. (The
/// admission check inside the listener would still keep a network source out if the bind were
/// wrong; this holds the bind itself.)
#[cfg(target_os = "linux")]
#[test]
fn the_manager_binds_loopback_while_the_network_is_off_and_every_adapter_while_on() {
    let port = {
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        s.local_addr().unwrap().port()
    };
    let e = Arc::new(Mutex::new(Engine::new("K2DEF", "FN31", 0)));
    let set = |f: &dyn Fn(&mut Settings)| {
        let mut g = engine_lock(&e);
        let mut s = g.settings().clone();
        f(&mut s);
        g.apply_settings(s);
    };
    set(&|s| {
        s.freedv_log = true;
        s.freedv_log_port = port;
        s.log_intake_lan = false;
    });
    let note: Note = Arc::new(|_, _| {});
    let (e2, n1, n2) = (Arc::clone(&e), Arc::clone(&note), Arc::clone(&note));
    std::thread::spawn(move || crate::freedv::run(e2, n1, n2));
    let until = |want: &[String]| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while bound_addrs(port) != want && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        bound_addrs(port)
    };
    let lo = [format!("0100007F:{port:04X}")];
    let any = [format!("00000000:{port:04X}")];
    assert_eq!(until(&lo), lo, "off: 127.0.0.1 only");
    set(&|s| s.log_intake_lan = true);
    assert_eq!(until(&any), any, "on: every adapter");
    set(&|s| s.log_intake_lan = false);
    assert_eq!(until(&lo), lo, "off again: 127.0.0.1 only");
    set(&|s| {
        s.wsjtx_udp = true;
        s.wsjtx_udp_addr = format!("127.0.0.1:{port}");
    });
    assert_eq!(
        until(&[]),
        Vec::<String>::new(),
        "a port Nexus sends to is let go"
    );
    assert!(engine_lock(&e)
        .freedv_mut()
        .log_error
        .clone()
        .is_some_and(|m| m.contains("cannot use port")));
    set(&|s| s.freedv_log = false);
}

/// ⛔ A device that gives no program name of its own (the S.A.T. tracker's raw ADIF) is never
/// stored, shown or sent under its network address: the contact is logged via "A device on your
/// network" (or "This computer"), the screen names it in its own words, and only the connection
/// log line, on this computer, says the address.
#[test]
fn a_device_with_no_program_name_is_stored_under_no_address() {
    let mut st = Station::new();
    st.settings(|s| s.log_intake_lan = true);
    st.send_from(
        "192.168.1.40:5000",
        b"<CALL:5>W1ABC<QSO_DATE:8>20261014<TIME_ON:4>0102<MODE:2>FM<FREQ:7>145.850<EOR>",
    );
    let log = st.e.stored_log();
    assert_eq!(log.len(), 1, "{:?}", st.lines());
    assert_eq!(via(&log[0]), Some(NETWORK_DEVICE_WORDS));
    let own = tempo_core::logbook::adif_record_own_log(&log[0]);
    assert!(!own.contains("192.168"), "the log's own file: {own}");
    let f = engine_lock(&st.e).freedv_mut().clone();
    assert_eq!(f.intake_program.as_deref(), Some(FROM_A_NETWORK_DEVICE));
    let queued = engine_lock(&st.e).take_pending_uploads();
    let sent = tempo_core::logbook::adif_record(&queued[0].rec);
    assert!(
        !sent.contains("192.168") && !sent.contains(VIA_FIELD),
        "{sent}"
    );
    assert_eq!(
        st.lines(),
        [(
            "info".to_string(),
            "A device on your network (192.168.1.40) logged W1ABC · 145.850 MHz: in the log"
                .to_string()
        )],
        "the address, in the connection log alone"
    );
    // A refusal shown beside the switch names no address either; its log line does.
    st.send_from(
        "192.168.1.40:5000",
        b"<CALL:5>W1ABC<QSO_DATE:8>20261014<TIME_ON:4>0102<STATION_CALLSIGN:4>W1AW<EOR>",
    );
    let notice = engine_lock(&st.e).freedv_mut().notice.clone().unwrap();
    assert!(!notice.contains("192.168"), "{notice}");
    assert!(
        st.lines()[1].1.contains("(192.168.1.40)"),
        "{:?}",
        st.lines()
    );
}

/// A type 5 the listener could not read (its time not in UTC, a damaged datagram) is said once
/// in the connection log, never dropped in silence.
#[test]
fn an_unreadable_contact_is_said_once() {
    let mut st = Station::new();
    let mut cut = encode_qso_logged("WSJT-X", &qso("K2DEF", "K1ABC"));
    cut.truncate(40);
    st.send(&cut);
    st.send(&cut);
    assert!(st.calls().is_empty());
    assert_eq!(st.lines().len(), 1, "{:?}", st.lines());
    assert!(
        st.lines()[0].1.contains("could not read"),
        "{:?}",
        st.lines()
    );
}

/// A lone QSO Logged as JTDX sends it, for the hold.
fn lone(call: &str) -> QsoLogged<'_> {
    let mut q = qso("K2DEF", call);
    q.time_on = WHEN;
    q
}

/// (Second review.) WSJT-X's real pair, for each upstream sender id: the type 12 takes the held
/// type 5's place AT ONCE, the row is the type 12's, with one upload and one line, and a later
/// flush adds nothing.
#[test]
fn the_pair_supersedes_at_once_and_counts_once() {
    for id in ["WSJT-X", "JTDX", "MSHV", "WSJT-X - IC9700"] {
        let mut st = Station::new();
        let five = stock_type5_sat("K1ABC");
        // The same datagram under this sender id.
        let five = [
            &five[..12],
            &(id.len() as u32).to_be_bytes()[..],
            id.as_bytes(),
            &five[12 + 4 + "WSJT-X".len()..],
        ]
        .concat();
        st.send(&five);
        assert!(st.gate.held.is_some(), "{id}: held");
        assert!(st.calls().is_empty());
        st.send(&encode_logged_adif(
            id,
            "<call:5>K1ABC<mode:4>MFSK<submode:3>FT4<band:2>2m<freq:10>145.900000\
             <qso_date:8>20261014<time_on:6>010203<station_callsign:5>K2DEF\
             <prop_mode:3>SAT<sat_name:5>RS-44<EOR>",
        ));
        assert!(st.gate.held.is_none(), "{id}: superseded at once");
        assert_eq!(st.e.stored_log()[0].sat_name.as_deref(), Some("RS-44"));
        st.settle();
        assert_eq!(st.calls(), ["K1ABC"]);
        assert_eq!(engine_lock(&st.e).take_pending_uploads().len(), 1, "{id}");
        assert_eq!(st.lines().len(), 1, "{id}: {:?}", st.lines());
    }
}

/// (Second review.) The real listener on a socket: a lone type 5 is logged by the quiet tick with
/// no other traffic; one held when the listener stops is logged by the stop; one held at a quit
/// is logged before the quit lets go of the listener ([`Listeners::stop`]); and (third review)
/// one held at the Windows self-update is logged by its flush ([`Listeners::flush`]) while the
/// listener KEEPS listening, as a failed install leaves a working station.
#[test]
fn the_listener_flushes_on_its_quiet_tick_on_stop_at_a_quit_and_at_an_update() {
    #[derive(PartialEq)]
    enum End {
        Quiet,
        Stop,
        Quit,
        Update,
    }
    let run = |end: End| {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = sock.local_addr().unwrap();
        let e = Arc::new(Mutex::new(Engine::new("K2DEF", "FN31", 0)));
        let sd = Arc::new(AtomicBool::new(false));
        let listeners = Arc::new(Listeners::new());
        let n: Note = Arc::new(|_, _| {});
        let t = {
            let (e, sd, l, n1, n2) = (
                Arc::clone(&e),
                Arc::clone(&sd),
                Arc::clone(&listeners),
                Arc::clone(&n),
                Arc::clone(&n),
            );
            std::thread::spawn(move || serve_with(sock, e, sd, n1, n2, 4096, &l))
        };
        let until = Instant::now() + Duration::from_secs(5);
        while listeners.serving.load(Ordering::SeqCst) == 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        let tx = UdpSocket::bind("127.0.0.1:0").unwrap();
        tx.send_to(&encode_qso_logged("JTDX", &lone("K1ABC")), addr)
            .unwrap();
        std::thread::sleep(Duration::from_millis(400));
        match end {
            End::Quiet => std::thread::sleep(HOLD + Duration::from_millis(900)),
            End::Stop => {
                assert!(e.stored_log().is_empty(), "still held");
                sd.store(true, Ordering::Relaxed);
                t.join().unwrap();
            }
            End::Quit => {
                assert!(e.stored_log().is_empty(), "still held");
                assert!(
                    listeners.stop(Duration::from_secs(5)),
                    "the listener let go in time"
                );
                assert_eq!(e.stored_log().len(), 1, "logged before the quit goes on");
            }
            End::Update => {
                assert!(e.stored_log().is_empty(), "still held");
                assert!(listeners.flush(Duration::from_secs(5)), "it answered");
                assert_eq!(e.stored_log().len(), 1, "logged before the update goes on");
                assert_eq!(
                    listeners.serving.load(Ordering::SeqCst),
                    1,
                    "still listening"
                );
                // CONTROL: it still takes contacts.
                tx.send_to(&encode_qso_logged("JTDX", &lone("W1XYZ")), addr)
                    .unwrap();
                std::thread::sleep(HOLD + Duration::from_millis(900));
                assert_eq!(
                    e.stored_log().len(),
                    2,
                    "a failed install leaves it working"
                );
            }
        }
        let logged = e.stored_log().len();
        sd.store(true, Ordering::Relaxed);
        logged
    };
    assert_eq!(run(End::Quiet), 1, "the quiet tick flushes");
    assert_eq!(run(End::Stop), 1, "the stop flushes");
    assert_eq!(run(End::Quit), 1, "the quit flushes");
    assert_eq!(run(End::Update), 2, "the update flushes and listens on");
}

/// (Third review.) A sender that alternates an unreadable contact with a good one gets ONE line,
/// not one per pair: a refusal is said again only after it has been quiet for [`REARM`]. CONTROL:
/// once it has been, the next is said.
#[test]
fn an_alternating_sender_is_said_once_until_it_has_been_quiet() {
    let mut st = Station::new();
    let mut cut = encode_qso_logged("WSJT-X", &qso("K2DEF", "K1ABC"));
    cut.truncate(40);
    for i in 0..10 {
        st.send(&cut);
        st.send(&ft8_at(&format!("W{i}ABC"), WHEN + i64::from(i) * 1000).into_bytes());
    }
    let said = |st: &Station| {
        st.lines()
            .iter()
            .filter(|(_, l)| l.contains("could not read"))
            .count()
    };
    assert_eq!(st.calls().len(), 10, "the good ones go in");
    assert_eq!(said(&st), 1, "{:?}", st.lines());
    // Quiet for longer than the re-arm: the next is said.
    for at in st.gate.said.values_mut() {
        *at = Instant::now().checked_sub(REARM * 2).unwrap();
    }
    st.send(&ft8_at("W9XYZ", WHEN + 99_000).into_bytes());
    st.send(&cut);
    assert_eq!(said(&st), 2, "{:?}", st.lines());
}
