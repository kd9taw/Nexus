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
/// three hand-written arms with the strip below as the catch-all — which cannot
/// tell "cm" from "m", so any OTHER centimetre label came out as a bare metre
/// count: `band_for_interop("6cm")` returned `"6"`, byte-identical to 6 METRES,
/// and the club log filed the contact on the wrong band with nothing to show it
/// had happened. The three arms all encoded one rule (centimetres ÷ 100), so the
/// rule stands in for them and the collision is unrepresentable rather than
/// merely absent. This adds no band to any vocabulary — the labels Nexus emits
/// are unchanged — it only guarantees that whatever reaches this function keeps
/// the band it was made on.
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
        // The trap: an alpha-strip alone turns the cm bands into absurd meter
        // counts, and the club log silently files the contact on the wrong band.
        assert_eq!(band_for_interop("70cm"), "0.7");
        assert_eq!(band_for_interop("33cm"), "0.33");
        assert_eq!(band_for_interop("23cm"), "0.23");
    }

    #[test]
    fn a_centimetre_band_can_never_collide_with_a_metre_band() {
        // ⚠️ THE COLLISION. Three cm bands were spelled out by hand and
        // everything else fell to the alpha-strip, which cannot tell "cm" from
        // "m": a 6 cm contact went out as "6" — BYTE-IDENTICAL to a 6 m
        // contact — and N1MM / N3FJP file it on 6 metres. Silent, permanent in
        // the club log, and unfixable from our end once broadcast.
        //
        // The three hand-written arms all encoded the SAME rule (N/100), so the
        // rule replaces them. No token is invented and no band vocabulary is
        // added anywhere: this only guarantees that whatever centimetre label
        // reaches this function is DISTINGUISHABLE from a metre band.
        for cm in ["3cm", "6cm", "9cm", "13cm", "23cm", "33cm", "70cm"] {
            let meters = cm.trim_end_matches("cm");
            assert_ne!(
                band_for_interop(cm),
                band_for_interop(&format!("{meters}m")),
                "{cm} goes out on the interop wire as {meters} METRES"
            );
        }
        assert_eq!(band_for_interop("6cm"), "0.06");
        assert_eq!(band_for_interop("13cm"), "0.13");
        // Metre bands are untouched, decimal ones included.
        assert_eq!(band_for_interop("1.25m"), "1.25");
        assert_eq!(band_for_interop("2m"), "2");
    }
}
