//! Tempo network interoperability: WSJT-X-compatible UDP telemetry + PSK Reporter.
//!
//! This crate lets Tempo speak the wire protocols the amateur-radio ecosystem
//! already understands, so third-party apps interoperate with it unmodified:
//!
//! - [`wsjtx`] / [`server`] — the WSJT-X `NetworkMessage` UDP protocol. Loggers
//!   and helpers (JTAlert, GridTracker, N1MM+, log4om, …) listen for these
//!   datagrams (WSJT-X's default sink is `127.0.0.1:2237`). Tempo emits
//!   Heartbeat / Status / Decode / QSOLogged / Close, and parses the inbound
//!   Reply / HaltTx / FreeText control datagrams.
//! - [`pskreporter`] — the IPFIX-like UDP spot upload to
//!   `report.pskreporter.info:4739`, the same one WSJT-X uses to report heard
//!   stations.
//! - [`qds`] — the shared Qt `QDataStream` (big-endian) byte codec the WSJT-X
//!   protocol is framed with.
//! - [`cluster`] / [`aprsis`] — long-lived telnet sessions against public ham
//!   services (DX cluster / RBN, and APRS-IS). Same shape: a blocking thread
//!   with reconnect backoff around a pure `Read`/`Write` pump.
//!
//! Everything is pure Rust over `std` sockets and byte buffers; encoders take
//! plain field arguments so there is no dependency on the rest of the workspace
//! (`tempo-core` depends on `modes` which depends on THIS crate, so the arrow
//! cannot be reversed). [`aprsis`] therefore owns only wire framing; the APRS
//! protocol it carries — TNC2 splitting, the passcode, the iGate gating rules —
//! lives in `tempo_core::aprs::is`, and the caller joins the two. The
//! datagram layouts are exhaustively unit-tested (build-bytes / loopback only —
//! no test ever touches the real network).

pub mod aprsis;
pub mod cluster;
pub mod dxkeeper;
pub mod flexcat;
pub mod flexdisc;
pub mod flexvita;
pub mod mqtt;
pub mod n1mm;
pub mod n3fjp;
pub mod pskreporter;
pub mod qds;
pub mod server;
pub mod sntp;
pub mod wsjtx;

// Convenience re-exports for the common entry points.
pub use cluster::{parse_dx_spot, ClusterSpot};
pub use mqtt::subscribe as mqtt_subscribe;

/// Upper bound (secs) on how long a feed loop (cluster telnet / MQTT) can take to
/// observe its stop flag — both use 2 s socket read timeouts. Restart orchestration
/// sleeps `this + 1` so the coupling is explicit, not folklore.
pub const FEED_STOP_OBSERVE_SECS: u64 = 2;
pub use pskreporter::{PskReporter, Spot};
pub use server::{WsjtxServer, APP_ID};
pub use wsjtx::{Decode, Inbound, QsoLogged, Status};

/// Band label → the meter-string the club-log protocols expect ("20m" → "20").
///
/// Shared by [`n1mm`] (`<band>`) and [`n3fjp`] (`fldBand` / `CHANGEBM`): both
/// bucket by METERS, never MHz. The centimeter bands need real values, not a
/// blind alpha-strip ("70cm" would have read as SEVENTY METERS in N3FJP).
///
/// ⚠️ CENTIMETRES ARE CONVERTED, NEVER STRIPPED. "70cm"/"33cm"/"23cm" were once
/// three hand-written arms with the strip below as the catch-all — and the strip
/// cannot tell "cm" from "m", so every OTHER centimetre label left here as a
/// bare number in a field that means METRES.
///
/// That was reachable, not hypothetical: `tempo_app::bandplan`'s Q65 plan ships
/// 13 cm, 9 cm, 5 cm, 3 cm and 1.2 cm channels (JT65 the first three), and
/// picking one stores that label as `settings.band`. FOUR CALL SITES CARRY IT
/// FROM THERE TO HERE, and only two of them are Field Day:
///
/// * `tempo_app::engine::n1mm_contact_for` — the STANDING N1MM broadcast, one
///   `<contactinfo>` per logged contact from `Engine::log_qso` → `push_to_n1mm`
///   whenever `n1mm_broadcast_target` resolves. Its argument is a `QsoRecord`,
///   so `QsoRecord.band` DOES reach this function and the GENERAL logbook DOES
///   have a club-network push — outside any contest, on every QSO.
/// * `tempo_audio`'s radio-loop Field-Day emitter, twice: the N3FJP `fldBand`
///   and the N1MM `<contactinfo>` `band`. Both read `fieldday::LoggedQso::band`,
///   which `log_submode_at` stamps from `FieldDayLog::band`, which
///   `sync_fd_band` keeps equal to `settings.band` on every QSY and at
///   `Engine::fd_log_manual`.
/// * the N3FJP band report (Network Status Display, opt-in
///   `n3fjp_report_band`) — `snap.radio.band`, read straight off the snapshot
///   with no log of any kind in the way. That field IS `settings.band`:
///   `Engine::set_frequency` mirrors it through `App::set_radio` in the same
///   statement that writes it, so a centimetre QSY is on this function's input
///   the moment the operator makes it, whether or not anything is logged.
///
/// So the value arrives either as a `QsoRecord.band` or as `settings.band`
/// itself, and on any of the four a 13 cm contact went out as `"13"` — thirteen
/// metres — when the value the wire wants is `"0.13"`. No claim is made that
/// this reached anyone's club log; what is on record is that Nexus could emit
/// it.
///
/// The three hand-written arms all encoded ONE rule (centimetres ÷ 100), so the
/// rule stands in for them. This adds no band to any vocabulary — the labels
/// Nexus emits are unchanged, and the three values that have always gone out are
/// byte-identical — it only guarantees that a centimetre label keeps the band it
/// was made on. Pinned here by `every_centimetre_band_converts_to_metres`,
/// against the channels actually shipped by `tempo_app::bandplan`'s
/// `no_shipped_channel_reaches_the_interop_wire_as_a_band_it_is_not_on`, and at
/// the two non-Field-Day call sites by `tempo_app::engine`'s
/// `an_ordinary_23cm_qso_reaches_the_club_wire_in_metres` and
/// `the_band_the_club_band_report_reads_is_the_dial_the_operator_is_on`.
pub fn band_for_interop(label: &str) -> String {
    if let Some(cm) = label.strip_suffix("cm") {
        if let Ok(n) = cm.parse::<f64>() {
            // Trim the trailing zeros a plain `{:.2}` leaves ("0.70", "0.30"):
            // the three values on the wire since this function existed are
            // "0.7"/"0.33"/"0.23", and consumers bucket on the exact string.
            return format!("{}", n / 100.0);
        }
    }
    label
        .trim_end_matches(|c: char| c.is_alphabetic())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::band_for_interop;

    #[test]
    fn band_labels_convert_to_meter_strings() {
        assert_eq!(band_for_interop("20m"), "20");
        assert_eq!(band_for_interop("160m"), "160");
        assert_eq!(band_for_interop("6m"), "6");
        // The three values that have gone out since this function existed —
        // unchanged by the conversion rule that replaced their hand-written arms.
        assert_eq!(band_for_interop("70cm"), "0.7");
        assert_eq!(band_for_interop("33cm"), "0.33");
        assert_eq!(band_for_interop("23cm"), "0.23");
    }

    #[test]
    fn every_centimetre_band_converts_to_metres() {
        // THE DEFECT. Three cm bands were spelled out by hand and everything
        // else fell to the alpha-strip, which cannot tell "cm" from "m" — so the
        // microwave channels the Q65 and JT65 plans ship left here as bare
        // numbers in a field that means METRES: 13 cm as "13", 3 cm as "3".
        //
        // Every label below is one `tempo_app::bandplan` ships today (that
        // census is asserted in bandplan.rs, which can see both crates). No
        // operator is claimed to have been affected — what is claimed, and
        // pinned here, is that Nexus could emit it.
        for (cm, wire) in [
            ("70cm", "0.7"),
            ("33cm", "0.33"),
            ("23cm", "0.23"),
            ("13cm", "0.13"),
            ("9cm", "0.09"),
            ("5cm", "0.05"),
            ("3cm", "0.03"),
        ] {
            assert_eq!(band_for_interop(cm), wire, "{cm} on the interop wire");
            let bare = cm.trim_end_matches("cm");
            assert_ne!(
                band_for_interop(cm),
                bare,
                "{cm} goes out as the bare number {bare}, which the wire reads as metres"
            );
        }
        // 1.2 cm is shipped too, and is the one label whose conversion is not
        // exact in binary — asserted on the value the wire actually carries.
        assert_eq!(band_for_interop("1.2cm"), format!("{}", 1.2f64 / 100.0));
        assert!(band_for_interop("1.2cm").starts_with("0.012"));
        // Metre bands are untouched, decimal ones included, and a label this
        // rule cannot parse still falls through to the strip.
        assert_eq!(band_for_interop("1.25m"), "1.25");
        assert_eq!(band_for_interop("2m"), "2");
        assert_eq!(band_for_interop("cm"), "");
    }
}
