//! A logged contact carries the worked station's continent (ADIF `CONT`) — the country
//! file's reading of the call, the one a contest row records — and never a guess for a call the
//! file cannot place.
//!
//! Its own test binary because the call resolver is installed once per process
//! (`tempo_core::contest::install_call_resolver`), and the library's tests run without one.

use tempo_app::engine::Engine;
use tempo_core::contest::{install_call_resolver, CallLocation};
use tempo_core::logbook::{adif_record, adif_record_own_log, extra_field, parse_adif, QsoRecord};

fn place(call: &str) -> Option<CallLocation> {
    let (entity, continent) = match call.get(..2)? {
        "JA" => ("Japan", "AS"),
        "DL" => ("Fed. Rep. of Germany", "EU"),
        _ => return None,
    };
    Some(CallLocation {
        entity,
        continent,
        cq_zone: None,
    })
}

fn qso(call: &str, when: u64) -> QsoRecord {
    QsoRecord {
        id: None,
        call: call.into(),
        grid: None,
        country: None,
        state: None,
        band: "20m".into(),
        freq_mhz: 14.074,
        freq_rx_mhz: None,
        mode: "FT8".into(),
        rst_sent: Some("-10".into()),
        rst_rcvd: Some("-12".into()),
        name: None,
        qth: None,
        comment: None,
        notes: None,
        tx_power: None,
        when_unix: when,
        time_off_unix: None,
        confirmed: false,
        award_confirmed: false,
        qsl_rcvd: Default::default(),
        qsl_sent: Default::default(),
        credit_granted: Vec::new(),
        credit_submitted: Vec::new(),
        upload: Default::default(),
        ota: Default::default(),
        time_known: true,
        dxcc: None,
        prop_mode: None,
        sat_name: None,
        operator: None,
        my_grid: None,
        my_rig: None,
        station_callsign: None,
        extra: Vec::new(),
        contest: None,
    }
}

#[test]
fn a_contact_carries_the_continent_its_call_resolves_to() {
    install_call_resolver(place).expect("the first install in this test binary");
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.log_qso(qso("JA1XYZ", 1_790_000_000));
    e.log_qso(qso("ZZ9ZZ", 1_790_000_100));
    let mut own = qso("DL1ABC", 1_790_000_200);
    own.extra = vec![("CONT".into(), "EU".into())];
    e.log_qso(own);
    let logged = e.take_pending_udp_qsos();
    let of = |call: &str| logged.iter().find(|r| r.call == call).expect("logged");
    assert_eq!(extra_field(of("JA1XYZ"), "CONT"), Some("AS"));
    assert_eq!(
        extra_field(of("ZZ9ZZ"), "CONT"),
        None,
        "a call nothing places"
    );
    let conts: Vec<_> = of("DL1ABC")
        .extra
        .iter()
        .filter(|(k, _)| k == "CONT")
        .collect();
    assert_eq!(conts.len(), 1, "the record's own is kept, written once");

    // Out to the services, and through the operator's own log and back.
    assert!(adif_record(of("JA1XYZ")).contains("<CONT:2>AS"));
    let back = &parse_adif(&adif_record_own_log(of("JA1XYZ")))[0];
    assert_eq!(extra_field(back, "CONT"), Some("AS"));
}
