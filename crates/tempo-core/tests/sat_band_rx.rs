//! #163 — satellite contacts in ADIF: BAND/FREQ the uplink, BAND_RX/FREQ_RX the downlink, the
//! record counted on its downlink band. What the import and restore paths must keep true for the
//! rows an OLDER build already holds: a satellite contact imported from another logger before the
//! reader mapped BAND_RX was stored on its uplink band with BAND_RX parked in the passthrough, and
//! re-importing that file, or restoring a backup of that row, must find it — never add a second
//! copy. Adopted from the data-integrity review of the change.
use tempo_core::logbook::{
    adif_header, adif_record, adif_record_own_log, parse_adif, Logbook, QsoRecord,
};

const T: u64 = 1_704_070_800; // 2024-01-01 01:00:00

fn base(call: &str, band: &str, mode: &str) -> QsoRecord {
    let t = format!(
        "<EOH>\n<CALL:{}>{call}<QSO_DATE:8>20240101<TIME_ON:6>010000<BAND:{}>{band}<MODE:{}>{mode}<EOR>\n",
        call.len(),
        band.len(),
        mode.len()
    );
    let mut r = parse_adif(&t).remove(0);
    r.when_unix = T;
    r
}

fn tags<'a>(out: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}:");
    out.match_indices(&open)
        .map(|(i, _)| {
            let rest = &out[i + open.len()..];
            let (len, tail) = rest.split_once('>').unwrap();
            let len: usize = len.split(':').next().unwrap().parse().unwrap();
            &tail[..len]
        })
        .collect()
}

fn reimport_adds(rows: Vec<QsoRecord>, text: &str) -> (usize, usize) {
    let mut lb = Logbook::new();
    for r in rows {
        lb.add(r);
    }
    let (added, skipped, _merged) = lb.import_adif(text);
    (added.len(), skipped)
}

/// A foreign satellite row an OLDER build imported: band = BAND (the uplink, 2m), BAND_RX parked.
fn legacy_foreign() -> QsoRecord {
    let mut r = base("W1AW", "2m", "FM");
    r.freq_mhz = 145.85;
    r.freq_rx_mhz = Some(436.795);
    r.prop_mode = Some("SAT".into());
    r.sat_name = Some("SO-50".into());
    r.extra = vec![("BAND_RX".into(), "70cm".into())];
    r
}

const FOREIGN: &str = "<EOH>\n<CALL:4>W1AW<QSO_DATE:8>20240101<TIME_ON:6>010000<MODE:2>FM\
<BAND:2>2m<BAND_RX:4>70cm<FREQ:7>145.850<FREQ_RX:7>436.795<PROP_MODE:3>SAT<SAT_NAME:5>SO-50<EOR>\n";

#[test]
fn reimporting_a_foreign_satellite_file_over_its_older_import_adds_nothing() {
    let (added, skipped) = reimport_adds(vec![legacy_foreign()], FOREIGN);
    assert_eq!(
        (added, skipped),
        (0, 1),
        "the same file again is the same contact"
    );
}

#[test]
fn restoring_a_backup_of_an_older_satellite_import_adds_nothing() {
    let out = adif_header() + &adif_record_own_log(&legacy_foreign());
    let (added, skipped) = reimport_adds(vec![legacy_foreign()], &out);
    assert_eq!(
        (added, skipped),
        (0, 1),
        "a restore over the live log is the same contact"
    );
}

#[test]
fn a_band_rx_whose_frequencies_disagree_is_not_mapped() {
    // BAND 2m with FREQ in 70cm (a logger's error): the reader must leave it as it came.
    let t = "<EOH>\n<CALL:4>W1AW<QSO_DATE:8>20240101<TIME_ON:6>010000<MODE:2>FM\
<BAND:2>2m<BAND_RX:4>70cm<FREQ:7>436.795<FREQ_RX:7>436.795<EOR>\n";
    let r = parse_adif(t).remove(0);
    assert_eq!(r.band, "2m");
    let out = adif_record(&r);
    assert_eq!(tags(&out, "BAND"), ["2m"], "{out}");
    assert_eq!(tags(&out, "BAND_RX"), ["70cm"], "{out}");
}

#[test]
fn a_parked_band_rx_is_never_written_twice() {
    let mut r = base("W1AW", "70cm", "FM");
    r.freq_mhz = 145.85;
    r.freq_rx_mhz = Some(436.795);
    r.extra = vec![("BAND_RX".into(), "70cm".into())];
    let out = adif_record(&r);
    assert_eq!(tags(&out, "BAND_RX"), ["70cm"], "{out}");
}

/// A held row with a parked BAND_RX is still the same contact as a restatement that carries BAND
/// alone — another logger's export of the satellite contact without BAND_RX, or a terrestrial row
/// whose stray BAND_RX disagrees with its band. Dedup finds the held row by its band as well as by
/// the band it is counted on.
#[test]
fn a_parked_band_rx_row_still_dedupes_against_a_band_only_restatement() {
    let sat = "<EOH>\n<CALL:4>W1AW<QSO_DATE:8>20240101<TIME_ON:6>010000<MODE:2>FM\
<BAND:2>2m<FREQ:7>145.850<PROP_MODE:3>SAT<SAT_NAME:5>SO-50<EOR>\n";
    assert_eq!(
        reimport_adds(vec![legacy_foreign()], sat),
        (0, 1),
        "satellite"
    );

    let mut stray = base("W1AW", "20m", "SSB");
    stray.extra = vec![("BAND_RX".into(), "40m".into())];
    let plain =
        "<EOH>\n<CALL:4>W1AW<QSO_DATE:8>20240101<TIME_ON:6>010000<BAND:3>20m<MODE:3>SSB<EOR>\n";
    assert_eq!(
        reimport_adds(vec![stray], plain),
        (0, 1),
        "terrestrial stray BAND_RX"
    );

    // CONTROL: a different band is a different contact.
    let other =
        "<EOH>\n<CALL:4>W1AW<QSO_DATE:8>20240101<TIME_ON:6>010000<BAND:3>40m<MODE:2>FM<EOR>\n";
    assert_eq!(
        reimport_adds(vec![base("W1AW", "20m", "FM")], other),
        (1, 0)
    );
}

// ---- Adopted from the third data-integrity review: the reverse direction, one file in both
// orders, a confirmation by either band, and which held row an upgrade lands on.

const HDR: &str = "<CALL:4>W1AW<QSO_DATE:8>20240101<TIME_ON:6>010000";
const SAT_BANDONLY: &str = "<MODE:2>FM<BAND:2>2m<FREQ:7>145.850<PROP_MODE:3>SAT<SAT_NAME:5>SO-50";
const SAT_PARKED: &str =
    "<MODE:2>FM<BAND:2>2m<BAND_RX:4>70cm<FREQ:7>145.850<PROP_MODE:3>SAT<SAT_NAME:5>SO-50";
const SAT_MAPPED: &str = "<MODE:2>FM<BAND:2>2m<BAND_RX:4>70cm<FREQ:7>145.850<FREQ_RX:7>436.795\
<PROP_MODE:3>SAT<SAT_NAME:5>SO-50";

fn held(body: &str) -> QsoRecord {
    let mut r = parse_adif(&format!("<EOH>\n{HDR}{body}<EOR>\n")).remove(0);
    r.when_unix = T;
    r
}
fn one(body: &str) -> String {
    format!("<EOH>\n{HDR}{body}<EOR>\n")
}

#[test]
fn an_incoming_row_with_a_parked_band_rx_dedupes_against_a_held_band_only_row() {
    assert_eq!(
        reimport_adds(vec![held(SAT_BANDONLY)], &one(SAT_PARKED)),
        (0, 1),
        "satellite"
    );
    assert_eq!(
        reimport_adds(
            vec![held("<BAND:3>20m<MODE:3>SSB<FREQ:6>14.250")],
            &one("<BAND:3>20m<BAND_RX:3>40m<MODE:3>SSB<FREQ:6>14.250")
        ),
        (0, 1),
        "terrestrial stray BAND_RX"
    );
    // A TQSL-shaped restatement the reader maps onto its downlink is still the held row another
    // logger wrote with its uplink as BAND.
    assert_eq!(
        reimport_adds(vec![held(SAT_BANDONLY)], &one(SAT_MAPPED)),
        (0, 1),
        "mapped"
    );
    // CONTROL: a different band is a different contact.
    assert_eq!(
        reimport_adds(
            vec![held("<BAND:3>20m<MODE:3>SSB")],
            &one("<BAND:3>15m<MODE:3>SSB")
        ),
        (1, 0)
    );
}

#[test]
fn one_file_holding_both_shapes_of_a_contact_adds_it_once_in_either_order() {
    for (first, second) in [(SAT_PARKED, SAT_BANDONLY), (SAT_BANDONLY, SAT_PARKED)] {
        let t = format!("<EOH>\n{HDR}{first}<EOR>\n{HDR}{second}<EOR>\n");
        let mut lb = Logbook::new();
        let (added, skipped, _) = lb.import_adif(&t);
        assert_eq!(
            (added.len(), skipped, lb.records().len()),
            (1, 1, 1),
            "{first} then {second}"
        );
    }
}

#[test]
fn an_upgrade_lands_on_the_held_row_that_is_on_that_band_not_on_a_twin() {
    // Two held rows, same call, second and mode: a satellite row with a parked BAND_RX (counted on
    // 70cm, known on 2m too) and a real 2m simplex contact. A 2m confirmation is the 2m contact's.
    let mut lb = Logbook::new();
    lb.add(held(SAT_PARKED));
    lb.add(held("<MODE:2>FM<BAND:2>2m<FREQ:7>146.520"));
    let (added, skipped, merged) =
        lb.import_adif(&one("<MODE:2>FM<BAND:2>2m<FREQ:7>146.520<QSL_RCVD:1>Y"));
    assert_eq!((added.len(), skipped, merged), (0, 1, 1));
    let real = lb
        .records()
        .iter()
        .find(|r| r.extra.is_empty())
        .unwrap()
        .clone();
    let twin = lb
        .records()
        .iter()
        .find(|r| !r.extra.is_empty())
        .unwrap()
        .clone();
    assert!(real.qsl_rcvd.card, "the 2m contact took it");
    assert!(!twin.qsl_rcvd.card, "the satellite row did not");
}

fn report(body: &str) -> Vec<QsoRecord> {
    parse_adif(&format!("<EOH>\n{HDR}{body}<QSL_RCVD:1>Y<EOR>\n"))
}

#[test]
fn a_confirmation_finds_its_contact_by_either_band_it_is_known_by() {
    use tempo_core::reconcile::reconcile;
    let mut log = vec![held("<BAND:3>20m<BAND_RX:3>40m<MODE:3>SSB<FREQ:6>14.250")];
    assert_eq!(
        reconcile(&mut log, &report("<BAND:3>20m<MODE:3>SSB")).matched,
        1,
        "stray BAND_RX"
    );
    for body in [
        "<MODE:2>FM<BAND:2>2m<BAND_RX:4>70cm<PROP_MODE:3>SAT<SAT_NAME:5>SO-50",
        SAT_MAPPED,
    ] {
        let mut log = vec![held(SAT_BANDONLY)];
        assert_eq!(reconcile(&mut log, &report(body)).matched, 1, "{body}");
    }
    // CONTROLS: a plain row still matches its plain report, and another band does not.
    let mut log = vec![held("<BAND:3>20m<MODE:3>SSB<FREQ:6>14.250")];
    assert_eq!(
        reconcile(&mut log, &report("<BAND:3>20M<MODE:3>USB")).matched,
        1
    );
    let mut log = vec![held("<BAND:3>20m<MODE:3>SSB")];
    assert_eq!(
        reconcile(&mut log, &report("<BAND:3>40m<MODE:3>SSB")).matched,
        0
    );
}

// ---- Adopted from the fourth review: a satellite contact's uplink band is a twin, and a twin
// never pairs across kinds. A 2m simplex confirmation is not the SO-50 contact's, and a 2m
// simplex QSO in a QRZ book is not swallowed by it (nor the reverse).

/// This build's satellite stamp shape: band = downlink, FREQ = uplink, FREQ_RX = downlink.
const SAT_STAMP: &str =
    "<MODE:2>FM<BAND:4>70cm<FREQ:7>145.850<FREQ_RX:7>436.795<PROP_MODE:3>SAT<SAT_NAME:5>SO-50";
const TERR_2M: &str = "<MODE:2>FM<BAND:2>2m<FREQ:7>146.520";
const LOTW_SAT: &str = "<MODE:2>FM<BAND:2>2M<BAND_RX:4>70CM<FREQ:9>145.85000<FREQ_RX:9>436.79500\
<PROP_MODE:3>SAT<SAT_NAME:5>SO-50";

fn at(body: &str, secs: u64) -> QsoRecord {
    let mut r = held(body);
    r.when_unix = T + secs;
    r.time_known = true;
    r
}
fn rows_at(rows: &[(&str, u64)]) -> Vec<QsoRecord> {
    let mut s = String::from("<EOH>\n");
    for (b, secs) in rows {
        let t = T + secs - 1_704_067_200;
        s.push_str(&format!(
            "<CALL:4>W1AW<QSO_DATE:8>20240101<TIME_ON:6>{:02}{:02}{:02}{b}<QSL_RCVD:1>Y<EOR>\n",
            t / 3600,
            t / 60 % 60,
            t % 60
        ));
    }
    parse_adif(&s)
}
fn confirmed(r: &QsoRecord) -> bool {
    let q = &r.qsl_rcvd;
    q.card || q.lotw || q.eqsl || q.qrz
}

#[test]
fn a_terrestrial_2m_confirmation_never_confirms_a_satellite_contact() {
    let mut log = vec![at(SAT_STAMP, 0)];
    let s = tempo_core::reconcile::reconcile(&mut log, &rows_at(&[(TERR_2M, 600)]));
    assert_eq!(s.matched, 0);
}

#[test]
fn a_qrz_book_never_merges_a_satellite_and_a_terrestrial_contact() {
    let mut log = vec![at(SAT_STAMP, 0)];
    let (added, _) = tempo_core::reconcile::merge_and_add(&mut log, rows_at(&[(TERR_2M, 600)]));
    assert_eq!(added.len(), 1, "the 2m simplex QSO is its own contact");
    let mut log = vec![at(TERR_2M, 0)];
    let (added, _) = tempo_core::reconcile::merge_and_add(&mut log, rows_at(&[(LOTW_SAT, 600)]));
    assert_eq!(added.len(), 1, "the SO-50 QSO is its own contact");
}

#[test]
fn with_both_logged_each_confirmation_lands_on_its_own() {
    let mut log = vec![at(SAT_STAMP, 0), at(TERR_2M, 600)];
    let s = tempo_core::reconcile::reconcile(&mut log, &rows_at(&[(TERR_2M, 600), (LOTW_SAT, 0)]));
    assert_eq!(s.matched, 2);
    assert!(confirmed(&log[0]) && confirmed(&log[1]));
}
