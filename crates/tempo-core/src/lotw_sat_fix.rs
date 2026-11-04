//! #163: the past satellite contacts that went to LoTW with the wrong band, and the corrected
//! form they go again in.
//!
//! Before the satellite stamp wrote both legs, a satellite contact was logged — and uploaded —
//! with its DOWNLINK alone: band and FREQ the downlink, no FREQ_RX. LoTW matches a satellite
//! contact on its UPLINK band, so those uploads sit at LoTW as a record nobody's matches. A
//! corrected copy goes as a new record (TQSL's duplicate check includes BAND, so it is not
//! refused); the old one stays at LoTW as a harmless non-match.
//!
//! The old row holds nothing about the uplink, so the corrected one takes it from
//! [`NOMINAL_UPLINKS`]: the transponders of the birds people log, each named by its LoTW
//! designator and its DOWNLINK band (AO-7's two modes have two downlink bands). A bird this table
//! does not know is never offered: nothing here guesses an uplink.
//!
//! Pure: what to offer ([`sat_band_fix`]) and what the row becomes ([`sat_band_fixed`]). The
//! offer, the rewrite and the upload are the station's, and only ever on the operator's click.

use crate::logbook::{query, QsoRecord};

/// (LoTW designator, downlink band, nominal uplink MHz) — the passband or channel centre the
/// satellite stamp itself would have written as FREQ, from AMSAT's published frequencies. Only
/// CROSS-BAND birds: a same-band bird (the ISS's 2 m voice and APRS) went to LoTW on the right band.
const NOMINAL_UPLINKS: &[(&str, &str, f64)] = &[
    ("SO-50", "70cm", 145.850),
    ("AO-91", "2m", 435.250),
    ("AO-92", "2m", 435.350),
    ("PO-101", "2m", 437.500),
    ("RS-44", "70cm", 145.965),
    ("FO-29", "70cm", 145.950),
    // AO-7 mode A (2 m up, 10 m down) and mode B (70 cm up, 2 m down).
    ("AO-7", "10m", 145.900),
    ("AO-7", "2m", 432.150),
    // The ISS's V/U cross-band FM repeater.
    ("ARISS", "70cm", 145.990),
    // QO-100's narrowband transponder (13 cm up, 3 cm down).
    ("QO-100", "3cm", 2400.250),
];

/// The nominal uplink (MHz) of `sat` heard on `downlink_band`, or `None` when the table does not
/// know that bird on that band.
pub fn nominal_uplink_mhz(sat: &str, downlink_band: &str) -> Option<f64> {
    NOMINAL_UPLINKS
        .iter()
        .find(|(name, band, _)| {
            name.eq_ignore_ascii_case(sat.trim()) && band.eq_ignore_ascii_case(downlink_band.trim())
        })
        .map(|&(_, _, mhz)| mhz)
}

/// Whether `r` is a satellite contact already on LoTW in the OLD, downlink-only shape, and the
/// uplink its corrected copy takes — `Some(uplink MHz)` to offer it, `None` to leave it.
///
/// Offered only when every one of these holds: PROP_MODE=SAT with a SAT_NAME; marked sent to LoTW
/// (pending, accepted or duplicate); NOT confirmed — by LoTW or for an award — since a confirmed
/// contact already matched as it is; its time known (LoTW cannot match one without); the old
/// shape — a FREQ and no FREQ_RX, the FREQ on the record's band; and a bird the table knows on
/// that band.
pub fn sat_band_fix(r: &QsoRecord) -> Option<f64> {
    let sat = r
        .prop_mode
        .as_deref()
        .is_some_and(|p| p.trim().eq_ignore_ascii_case("SAT"));
    let sent = r.upload.lotw.as_ref().is_some_and(|s| s.outcome.is_sent());
    if !sat
        || !sent
        || r.qsl_rcvd.lotw
        || r.award_confirmed
        || !r.time_known
        || r.freq_rx_mhz.is_some()
        || !(r.freq_mhz.is_finite() && r.freq_mhz > 0.0)
        || !query::ui_band_for_mhz(r.freq_mhz).eq_ignore_ascii_case(r.band.trim())
    {
        return None;
    }
    nominal_uplink_mhz(r.sat_name.as_deref()?, &r.band)
}

/// `r` in the corrected shape — FREQ the nominal uplink, FREQ_RX the downlink it was logged on,
/// its band (the downlink, which it is counted on) unchanged — the shape the satellite stamp now
/// logs, which goes to LoTW as BAND = the uplink's band, BAND_RX = the downlink's. `None` when it
/// is not one [`sat_band_fix`] offers.
pub fn sat_band_fixed(r: &QsoRecord) -> Option<QsoRecord> {
    let up = sat_band_fix(r)?;
    let mut fixed = r.clone();
    fixed.freq_rx_mhz = Some(r.freq_mhz);
    fixed.freq_mhz = up;
    Some(fixed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::{adif_record, parse_adif, UploadOutcome, UploadStatus};

    /// A satellite contact as an older build logged and uploaded it.
    fn old_upload(sat: &str, band: &str, down: f64) -> QsoRecord {
        let mut r = parse_adif(&format!(
            "<EOH>\n<CALL:4>W1AW<QSO_DATE:8>20240101<TIME_ON:6>010000<BAND:{}>{band}<MODE:2>FM<EOR>\n",
            band.len()
        ))
        .remove(0);
        r.freq_mhz = down;
        r.prop_mode = Some("SAT".into());
        r.sat_name = Some(sat.into());
        r.upload.lotw = Some(UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1_704_000_000,
            detail: None,
        });
        r
    }

    #[test]
    fn an_old_satellite_upload_is_offered_and_goes_again_with_the_uplink_band() {
        let r = old_upload("SO-50", "70cm", 436.795);
        assert_eq!(sat_band_fix(&r), Some(145.850));
        let fixed = sat_band_fixed(&r).unwrap();
        assert_eq!(fixed.band, "70cm", "still counted on the downlink");
        assert_eq!(fixed.freq_rx_mhz, Some(436.795));
        let out = adif_record(&fixed);
        assert!(out.contains("<BAND:2>2m"), "{out}");
        assert!(out.contains("<BAND_RX:4>70cm"), "{out}");
        assert!(out.contains("<FREQ:10>145.850000"), "{out}");
        // Fixed, it is no longer offered.
        assert_eq!(sat_band_fix(&fixed), None);
        // AO-7's two modes, by the band it was heard on.
        assert_eq!(
            sat_band_fix(&old_upload("AO-7", "10m", 29.45)),
            Some(145.900)
        );
        assert_eq!(
            sat_band_fix(&old_upload("AO-7", "2m", 145.95)),
            Some(432.150)
        );
    }

    #[test]
    fn confirmed_unsent_unknown_and_already_right_contacts_are_not_offered() {
        let mut lotw = old_upload("SO-50", "70cm", 436.795);
        lotw.qsl_rcvd.lotw = true;
        assert_eq!(
            sat_band_fix(&lotw),
            None,
            "LoTW-confirmed: it matched as it is"
        );
        let mut card = old_upload("SO-50", "70cm", 436.795);
        card.award_confirmed = true;
        assert_eq!(sat_band_fix(&card), None, "confirmed for an award");
        let mut unsent = old_upload("SO-50", "70cm", 436.795);
        unsent.upload.lotw = None;
        assert_eq!(
            sat_band_fix(&unsent),
            None,
            "never sent: the next upload is right anyway"
        );
        let mut rejected = old_upload("SO-50", "70cm", 436.795);
        rejected.upload.lotw.as_mut().unwrap().outcome = UploadOutcome::Rejected;
        assert_eq!(sat_band_fix(&rejected), None, "not on LoTW");
        assert_eq!(
            sat_band_fix(&old_upload("XW-2A", "2m", 145.67)),
            None,
            "unknown bird"
        );
        assert_eq!(
            sat_band_fix(&old_upload("ARISS", "2m", 145.80)),
            None,
            "a same-band bird went on the right band"
        );
        let mut terrestrial = old_upload("SO-50", "70cm", 436.795);
        terrestrial.prop_mode = None;
        terrestrial.sat_name = None;
        assert_eq!(sat_band_fix(&terrestrial), None, "not a satellite contact");
        let mut dateonly = old_upload("SO-50", "70cm", 436.795);
        dateonly.time_known = false;
        assert_eq!(sat_band_fix(&dateonly), None, "LoTW cannot match it anyway");
        let mut nofreq = old_upload("SO-50", "70cm", 436.795);
        nofreq.freq_mhz = 0.0;
        assert_eq!(sat_band_fix(&nofreq), None, "no downlink to keep");
    }
}
