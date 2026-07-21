//! DXKeeper (DXLab Suite, AA6YQ) — real-time QSO push over its TCP Network Service.
//!
//! Protocol per DXLab's *DXKeeper TCP/IP Directives* (v7, 2026-07-01,
//! `dxlabsuite.com/dxkeeper/DXKeeper TCPIP Messages.pdf`). Nexus is the TCP **client**;
//! DXKeeper runs the listener (enable it under Configuration ▸ Defaults ▸ Network Service).
//!
//! ## The port is base + 1, not the base
//!
//! DXLab reserves a *block* of adjacent ports; the operator configures only the **Base
//! Port** (default 52000). DXKeeper answers on the **second** port in the block — **52001** —
//! and Commander on the third (52002). Nothing listens on the base itself. Operators
//! routinely report "52000" because that is the number DXKeeper's own config panel shows
//! them, so [`port_for_base`] does the +1 and the UI asks for the Base Port by name.
//!
//! ## Four things that silently break this integration
//!
//! 1. **One directive per connection.** "After accepting and executing a directive via
//!    TCP/IP, DXKeeper closes and then re-opens the TCP/IP port." A kept-alive socket logs
//!    QSO #1 and silently discards every one after it. Hence connect-per-push, same as
//!    [`crate::n3fjp`].
//! 2. **`<parameters:N>` is a BYTE count of everything after the tag, including the trailing
//!    `<EOR>`.** Rust's `String::len()` is already bytes so this is free here — but JTDX uses
//!    Qt's `QString::length()`, which counts UTF-16 code units, so any accented call, name or
//!    QTH truncates their record. [`build_externallog`] is tested against non-ASCII for
//!    exactly this reason.
//! 3. **DXKeeper never acknowledges.** None of its directives reply. A successful write
//!    proves the bytes left this process, *not* that the QSO landed. Real errors appear only
//!    in DXKeeper's own Server Log (Network Service ▸ Display Log).
//! 4. **`externallog`, not `log`.** Only `externallog` carries the per-QSO upload flags.
//!    Nexus already owns the LoTW / eQSL / ClubLog / QRZ connectors, so those flags go out as
//!    `N` — otherwise every QSO uploads twice to four services.
//!
//! Caveat we cannot fix from this side, per the spec: if DXKeeper's own QSL Configuration has
//! *Auto upload* ticked for Club Log or QRZ, it uploads regardless of our `N`. The operator
//! must untick those in DXKeeper. Surfaced in the settings hint rather than hidden here.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// DXLab's default Base Port. DXKeeper itself is on [`port_for_base`] of this.
pub const DEFAULT_BASE_PORT: u16 = 52000;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// DXKeeper answers on the SECOND port of the DXLab block — base + 1.
///
/// Saturating so a base of 65535 cannot wrap to 0 and produce a nonsense connect.
pub fn port_for_base(base: u16) -> u16 {
    base.saturating_add(1)
}

/// One ADIF field in DXLab's `<name:bytelen>value` syntax.
///
/// The length is the value's length in BYTES, which is what ADIF specifies and what DXKeeper
/// counts when it slices the buffer.
fn adif_field(name: &str, value: &str) -> String {
    format!("<{name}:{}>{value}", value.len())
}

/// Build the `externallog` directive for one already-ADIF-encoded QSO.
///
/// `adif_record` may or may not already end with `<EOR>` — this normalizes to exactly one.
/// `tempo_core::logbook::adif_record` DOES emit one (`logbook.rs:824`), so blindly appending
/// would ship a doubled terminator on every real QSO. The `<EOR>` must be inside the
/// `<parameters:N>` byte count, so it is added here, in the one place that also computes
/// that count.
///
/// `upload_flags` is false when Nexus owns the upload connectors (the normal case), which
/// sends `N` for eQSL / LoTW / ClubLog / QRZ so the QSO is not uploaded twice.
pub fn build_externallog(adif_record: &str, upload_flags: bool) -> String {
    let yn = if upload_flags { "Y" } else { "N" };
    let trimmed = adif_record.trim_end();
    let body = trimmed
        .strip_suffix("<EOR>")
        .or_else(|| trimmed.strip_suffix("<eor>"))
        .unwrap_or(trimmed)
        .trim_end();
    let record = format!("{body}<EOR>");

    // Everything the <parameters:N> length must cover.
    let mut params = adif_field("ExternalLogADIF", &record);
    for f in ["UploadeQSL", "UploadLoTW", "UploadClubLog", "UploadQRZ"] {
        params.push_str(&adif_field(f, yn));
    }
    // Let DXKeeper enrich what it can — it owns the callbook subscription and the override
    // database, and this is exactly the value a DXKeeper user is there for.
    for f in ["DeduceMissing", "QueryCallbook", "CheckOverrides"] {
        params.push_str(&adif_field(f, "Y"));
    }

    format!(
        "{}{}",
        adif_field("command", "externallog"),
        adif_field("parameters", &params)
    )
}

fn connect(host: &str, port: u16) -> Result<TcpStream, String> {
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or_else(|| format!("could not resolve {host}:{port}"))?;
    let s = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(CONNECT_TIMEOUT)).ok();
    Ok(s)
}

/// Push one QSO into DXKeeper: connect → one directive → close.
///
/// `base_port` is the operator-facing **Base Port** (what DXKeeper's config panel shows);
/// the +1 happens here so no caller has to remember it.
///
/// Returns `Ok` when the bytes were written. That is genuinely all we can know — DXKeeper
/// sends no acknowledgement of any kind.
pub fn push_qso(
    host: &str,
    base_port: u16,
    adif_record: &str,
    upload_flags: bool,
) -> Result<(), String> {
    let port = port_for_base(base_port);
    let mut stream = connect(host, port).map_err(|e| {
        format!(
            "DXKeeper connect {host}:{port}: {e} \
             (is Network Service enabled in DXKeeper ▸ Configuration ▸ Defaults?)"
        )
    })?;
    stream
        .write_all(build_externallog(adif_record, upload_flags).as_bytes())
        .map_err(|e| format!("DXKeeper send: {e}"))?;
    stream.flush().map_err(|e| format!("DXKeeper flush: {e}"))?;
    Ok(())
}

/// Can we reach DXKeeper's Network Service? Opens and closes a connection without logging
/// anything — the Settings "Test" button.
///
/// A successful connect is the strongest signal available: since DXKeeper never replies,
/// there is nothing further to wait for, and inventing a probe QSO to check the path would
/// put a junk record in the operator's log.
pub fn test_connection(host: &str, base_port: u16) -> Result<String, String> {
    let port = port_for_base(base_port);
    connect(host, port).map_err(|e| {
        format!(
            "No DXKeeper on {host}:{port} — {e}. Enable Configuration ▸ Defaults ▸ \
             Network Service in DXKeeper, and check its Base Port (Nexus adds 1)."
        )
    })?;
    Ok(format!(
        "Connected to DXKeeper on {host}:{port} (Base Port {base_port} + 1). \
         DXKeeper sends no reply, so this confirms the port is open, not that a QSO logs."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dxkeeper_listens_on_base_plus_one() {
        // The whole point: operators report the Base Port because that is what their config
        // panel labels. Connecting to 52000 itself reaches nothing.
        assert_eq!(port_for_base(52000), 52001);
        assert_eq!(port_for_base(DEFAULT_BASE_PORT), 52001);
        assert_eq!(port_for_base(65535), 65535, "must not wrap to 0");
    }

    #[test]
    fn parameters_length_counts_bytes_including_the_eor() {
        let msg = build_externallog("<call:5>W1ABC<mode:3>FT8", false);
        let tag = "<parameters:";
        let i = msg.find(tag).expect("parameters field") + tag.len();
        let j = msg[i..].find('>').unwrap() + i;
        let declared: usize = msg[i..j].parse().unwrap();
        assert_eq!(
            declared,
            msg[j + 1..].len(),
            "declared length must equal the actual remaining bytes"
        );
        assert!(msg.contains("<EOR>"), "EOR must be inside the counted span");
    }

    #[test]
    fn length_is_bytes_not_chars_for_non_ascii() {
        // This is the JTDX bug: Qt's QString::length() returns UTF-16 units, so an accented
        // name declares a short length and DXKeeper truncates the record.
        let msg = build_externallog("<call:5>W1ABC<name:6>José", false);
        let tag = "<parameters:";
        let i = msg.find(tag).expect("parameters field") + tag.len();
        let j = msg[i..].find('>').unwrap() + i;
        let declared: usize = msg[i..j].parse().unwrap();
        assert_eq!(declared, msg[j + 1..].len());
        assert!(
            declared > msg[j + 1..].chars().count(),
            "bytes exceed chars here"
        );
    }

    #[test]
    fn upload_flags_default_to_n_so_nexus_does_not_double_upload() {
        let off = build_externallog("<call:5>W1ABC", false);
        for f in ["UploadeQSL", "UploadLoTW", "UploadClubLog", "UploadQRZ"] {
            assert!(off.contains(&format!("<{f}:1>N")), "{f} must be N: {off}");
        }
        let on = build_externallog("<call:5>W1ABC", true);
        assert!(on.contains("<UploadLoTW:1>Y"));
    }

    #[test]
    fn uses_externallog_and_asks_dxkeeper_to_enrich() {
        let msg = build_externallog("<call:5>W1ABC", false);
        assert!(msg.starts_with("<command:11>externallog"), "{msg}");
        // DeduceMissing/QueryCallbook are the value a DXKeeper user is there for.
        assert!(msg.contains("<QueryCallbook:1>Y"));
        assert!(msg.contains("<CheckOverrides:1>Y"));
    }

    #[test]
    fn a_trailing_eor_in_the_input_is_not_doubled() {
        let msg = build_externallog("<call:5>W1ABC<EOR>", false);
        assert_eq!(msg.matches("<EOR>").count(), 1, "{msg}");
    }
}
