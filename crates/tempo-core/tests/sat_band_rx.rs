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
