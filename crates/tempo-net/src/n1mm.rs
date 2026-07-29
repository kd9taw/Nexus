//! N1MM+ native contact broadcast — the `<contactinfo>` UDP XML datagram
//! (official: n1mmwp.hamdocs.com/appendices/external-udp-broadcasts/).
//!
//! N1MM-networked clubs run aggregation dashboards (n1mm_view-style) that
//! consume these datagrams; emitting them per Field Day QSO makes Nexus a
//! first-class station on that network. Emit-only: N1MM itself never accepts
//! inbound contactinfo (its UDP intake is spectrum/freq data only).
//!
//! The same datagram serves a SECOND audience, which is why the fields here are
//! not all contest fields: live map/dashboard consumers (OpenHamClock,
//! GridTracker) plot each broadcast contact as it is logged, contest or not. So
//! Nexus emits contactinfo on two paths — per Field Day contact from the radio
//! loop, and (when the operator enables it) per ORDINARY logged QSO from the
//! shell's log forwarder. Those two read disjoint logs, so a contact is never
//! broadcast twice; the invariant is pinned by
//! `a_field_day_contact_never_enters_the_general_upload_queue` in tempo-app.

use std::net::UdpSocket;

/// N1MM's own name for its general (non-contest) log. A plain QSO is not part of
/// any contest, and an empty `<contestname>` reads as a malformed contest entry
/// to consumers that bucket by it.
pub const GENERAL_LOG: &str = "DX";

/// One contact for the broadcast.
#[derive(Debug, Clone)]
pub struct N1mmContact {
    pub mycall: String,
    pub call: String,
    /// Band as the meter string the dashboards bucket by: "20" / "40" / "80"
    /// ("0.7" for 70 cm). NOT MHz.
    pub band: String,
    /// "CW" | "USB" | "FT8" | "FT4" …
    pub mode: String,
    /// "YYYY-MM-DD HH:MM:SS" UTC.
    pub timestamp: String,
    pub section: String,
    /// Maidenhead grid, when the record carries one — THE field a map consumer
    /// plots the contact from. Empty for a contest contact (class+section is the
    /// exchange, no grid is passed), and then omitted from the datagram entirely.
    pub gridsquare: String,
    pub points: u32,
    /// "ARRL-FIELD-DAY" | "WFD" | [`GENERAL_LOG`] for an ordinary QSO.
    pub contestname: String,
    /// RX/TX frequency in units of 10 Hz (N1MM convention).
    pub freq_10hz: u64,
    /// Our sent exchange, e.g. "3A WI".
    pub sent_exchange: String,
    pub operator: String,
    /// 32-hex unique id (consumers dedup on it).
    pub id: String,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Build the `<contactinfo>` datagram (the mandatory-for-consumers field set
/// plus the commonly displayed extras; absent fields are simply omitted —
/// consumers treat them as empty).
pub fn build_contactinfo(c: &N1mmContact) -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
            "<contactinfo>",
            "<app>NEXUS</app>",
            "<contestname>{contest}</contestname>",
            "<contestnr>1</contestnr>",
            "<timestamp>{ts}</timestamp>",
            "<mycall>{my}</mycall>",
            "<band>{band}</band>",
            "<rxfreq>{f}</rxfreq><txfreq>{f}</txfreq>",
            "<operator>{op}</operator>",
            "<mode>{mode}</mode>",
            "<call>{call}</call>",
            "<section>{sect}</section>",
            "{grid}",
            "<points>{pts}</points>",
            "<radionr>1</radionr>",
            "<IsRunQSO>0</IsRunQSO>",
            "<StationName>NEXUS</StationName>",
            "<ID>{id}</ID>",
            "<IsClaimedQso>1</IsClaimedQso>",
            "<SentExchange>{sent}</SentExchange>",
            "</contactinfo>"
        ),
        contest = esc(&c.contestname),
        ts = esc(&c.timestamp),
        my = esc(&c.mycall),
        band = esc(&c.band),
        f = c.freq_10hz,
        op = esc(&c.operator),
        mode = esc(&c.mode),
        call = esc(&c.call),
        sect = esc(&c.section),
        // Omitted when absent, per the convention above — an empty element would
        // change the long-proven Field Day datagram for no consumer's benefit.
        grid = if c.gridsquare.trim().is_empty() {
            String::new()
        } else {
            format!("<gridsquare>{}</gridsquare>", esc(c.gridsquare.trim()))
        },
        pts = c.points,
        id = esc(&c.id),
        sent = esc(&c.sent_exchange),
    )
}

/// Unix secs → the `<timestamp>` N1MM emits: "YYYY-MM-DD HH:MM:SS" UTC.
pub fn utc_timestamp(unix: u64) -> String {
    let secs_of_day = unix % 86_400;
    let days = (unix / 86_400) as i64;
    let (h, m, sec) = (
        (secs_of_day / 3600) as u32,
        ((secs_of_day % 3600) / 60) as u32,
        (secs_of_day % 60) as u32,
    );
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{sec:02}")
}

/// The 32-hex `<ID>` consumers dedup on: derived from the contact itself (log
/// time + call), so re-broadcasting the SAME contact is idempotent rather than a
/// second pin on the map. `seq` separates contacts that share a log second (a
/// batch pushed from one slot boundary).
pub fn dedup_id(when_unix: u64, call: &str, seq: u64) -> String {
    format!(
        "{:016x}{:016x}",
        when_unix.wrapping_mul(31).wrapping_add(seq),
        call.bytes()
            .fold(0u64, |a, b| a.wrapping_mul(131).wrapping_add(b as u64))
    )
}

/// N1MM's documented broadcast port, used when the operator gave a bare host.
pub const DEFAULT_PORT: u16 = 12060;

/// The socket address a configured `addr` resolves to. An operator who names a
/// port MUST get that port: consumers are routinely stacked on one host (HRD on
/// 12060, N1MM on 12061), so silently substituting the default would deliver every
/// contact to the wrong listener.
pub fn resolve_target(addr: &str) -> String {
    let addr = addr.trim();
    if addr.contains(':') {
        addr.to_string()
    } else {
        format!("{addr}:{DEFAULT_PORT}")
    }
}

/// Fire one datagram at `addr` ("host:port", or a bare host for
/// [`DEFAULT_PORT`]). Best-effort: a down dashboard must never block a QSO.
pub fn send_contact(addr: &str, c: &N1mmContact) -> Result<(), String> {
    let target = resolve_target(addr);
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    sock.send_to(build_contactinfo(c).as_bytes(), &target)
        .map_err(|e| format!("N1MM send to {target}: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contactinfo_carries_the_consumer_mandatory_fields() {
        let xml = build_contactinfo(&fd_contact());
        for needle in [
            "<mycall>W9XYZ</mycall>",
            "<call>W1AW</call>",
            "<band>20</band>",
            "<mode>FT8</mode>",
            "<timestamp>2026-06-27 18:05:00</timestamp>",
            "<section>CT</section>",
            "<points>2</points>",
            "<StationName>NEXUS</StationName>",
            "<ID>0123456789abcdef0123456789abcdef</ID>",
            "<contestname>ARRL-FIELD-DAY</contestname>",
            "<contestnr>1</contestnr>",
            "<SentExchange>3A WI</SentExchange>",
            "<rxfreq>1407400</rxfreq>",
        ] {
            assert!(xml.contains(needle), "missing {needle} in {xml}");
        }
        assert!(xml.starts_with("<?xml"));
        assert!(xml.ends_with("</contactinfo>"));
    }

    /// A contest contact carries no grid (the exchange is class+section), and the
    /// FD datagram that has been on the air since 0.8.0 must stay byte-identical —
    /// so an empty grid emits NO element at all, per this module's omit convention.
    #[test]
    fn an_empty_grid_emits_no_gridsquare_element() {
        let xml = build_contactinfo(&fd_contact());
        assert!(!xml.contains("gridsquare"), "{xml}");
    }

    /// The general-logging path DOES have a grid, and it is the field a map
    /// consumer (OpenHamClock, GridTracker) plots the contact from.
    #[test]
    fn a_grid_is_broadcast_when_the_record_carries_one() {
        let c = N1mmContact {
            gridsquare: "FN31pr".into(),
            ..fd_contact()
        };
        let xml = build_contactinfo(&c);
        assert!(xml.contains("<gridsquare>FN31pr</gridsquare>"), "{xml}");
    }

    #[test]
    fn the_timestamp_is_n1mms_utc_shape() {
        assert_eq!(utc_timestamp(1_782_583_500), "2026-06-27 18:05:00");
        // Epoch, and a leap day — the civil-from-days math is hand-rolled.
        assert_eq!(utc_timestamp(0), "1970-01-01 00:00:00");
        assert_eq!(utc_timestamp(1_709_164_799), "2024-02-28 23:59:59");
        assert_eq!(utc_timestamp(1_709_164_800), "2024-02-29 00:00:00");
    }

    /// Consumers dedup on the 32-hex ID: the SAME contact must always hash to the
    /// same id (a re-push is idempotent), two contacts in one batch must not
    /// collide, and the width is fixed at 32.
    #[test]
    fn the_dedup_id_is_stable_and_32_hex() {
        let a = dedup_id(1_782_583_500, "W1AW", 0);
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            a,
            dedup_id(1_782_583_500, "W1AW", 0),
            "same contact, same id"
        );
        assert_ne!(a, dedup_id(1_782_583_500, "K1ABC", 0), "different call");
        assert_ne!(
            a,
            dedup_id(1_782_583_500, "W1AW", 1),
            "same second, batch #2"
        );
    }

    /// A tester ran HRD on 12060 and N1MM on 12061 on the same host and watched
    /// Wireshark. Stacked consumers are the normal case, so a named port is not a
    /// hint — substituting the default would hand every contact to the neighbour.
    #[test]
    fn a_named_port_is_honoured_and_a_bare_host_falls_back() {
        assert_eq!(resolve_target("127.0.0.1:12061"), "127.0.0.1:12061");
        assert_eq!(resolve_target("127.0.0.1"), "127.0.0.1:12060");
        assert_eq!(resolve_target(" 127.0.0.1:12061 "), "127.0.0.1:12061");
        assert_eq!(resolve_target("dashboard.local"), "dashboard.local:12060");
    }

    /// End-to-end over loopback (no network egress): the datagram must arrive at
    /// the port the operator named, and NOT at the default port a co-hosted
    /// consumer is sitting on.
    #[test]
    fn the_datagram_arrives_at_the_named_port_and_nowhere_else() {
        let n1mm = UdpSocket::bind("127.0.0.1:0").unwrap();
        n1mm.set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        // The "other consumer" — a second listener that must stay silent.
        let other = UdpSocket::bind("127.0.0.1:0").unwrap();
        other
            .set_read_timeout(Some(std::time::Duration::from_millis(250)))
            .unwrap();

        let addr = n1mm.local_addr().unwrap().to_string();
        send_contact(&addr, &fd_contact()).expect("send to a named port succeeds");

        let mut buf = [0u8; 4096];
        let (n, _) = n1mm
            .recv_from(&mut buf)
            .expect("the named port receives it");
        let xml = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(xml.starts_with("<?xml"), "{xml}");
        assert!(xml.contains("<call>W1AW</call>"), "{xml}");

        // Exactly ONE datagram per contact — a second pin on the map is a bug.
        assert!(
            n1mm.recv_from(&mut buf).is_err(),
            "one contact must produce one datagram"
        );
        assert!(
            other.recv_from(&mut buf).is_err(),
            "nothing may leak to another listener on the same host"
        );
    }

    fn fd_contact() -> N1mmContact {
        N1mmContact {
            mycall: "W9XYZ".into(),
            call: "W1AW".into(),
            band: "20".into(),
            mode: "FT8".into(),
            timestamp: "2026-06-27 18:05:00".into(),
            section: "CT".into(),
            gridsquare: String::new(),
            points: 2,
            contestname: "ARRL-FIELD-DAY".into(),
            freq_10hz: 1_407_400,
            sent_exchange: "3A WI".into(),
            operator: "KD9TAW".into(),
            id: "0123456789abcdef0123456789abcdef".into(),
        }
    }
}
