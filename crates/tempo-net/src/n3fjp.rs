//! N3FJP Field Day Contest Log — real-time QSO push over its TCP API.
//!
//! The club-network story: N3FJP runs the master Field Day log; Nexus is the
//! digital (or any) station and pushes each contact in as it's logged, exactly
//! like the classic WSJT-X→JTAlert→N3FJP bridge but native. Protocol per the
//! official API docs (n3fjp.com/help/api.html): XML-ish `<CMD>…</CMD>` lines,
//! **every command terminated `\r\n`** (a bare `\r\n` closes the connection),
//! N3FJP is the TCP server (default port 1100; enabled in N3FJP under
//! Settings ▸ Application Program Interface).
//!
//! Two push paths, by purpose:
//!  - **ADDDIRECT** ([`push_qso`]) — a direct DB insert (no UI-keystroke
//!    emulation, dupes excluded server-side) + CHECKLOG to refresh the screen.
//!    Fast and simple, but it **bypasses N3FJP's ENTER action** — the action
//!    that assigns a *contest* log's points/multipliers — so ADDDIRECT is the
//!    general/bulk path (a plain log or a backfill), NOT the scoring path.
//!  - **ENTER sequence** ([`push_qso_enter`]) — drives the entry form and fires
//!    ACTION ENTER, so N3FJP scores the contact. This is the **contest-correct
//!    Field Day path**, and it reads back the `<ENTERRESPONSE>` record count so
//!    a rejection surfaces instead of being silently lost.
//!
//! Plus two no-QSO helpers: [`report_band`] puts this position on the club's
//! port-1000 Network Status Display band board without CAT, and [`dupecheck`]
//! is a silent pre-log dupe hint against the combined club log. Connect-per-push
//! keeps the loop simple and robust across N3FJP restarts during a chaotic
//! Field Day.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(4);

/// Minimal XML escaping for field values (calls/sections are alnum, but a
/// comment or park name must never break the stream).
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn connect(host: &str, port: u16) -> std::io::Result<TcpStream> {
    let addr = format!("{host}:{port}");
    let stream = TcpStream::connect_timeout(
        &addr.parse().or_else(|_| {
            // Hostname: resolve via ToSocketAddrs.
            use std::net::ToSocketAddrs;
            addr.to_socket_addrs()
                .ok()
                .and_then(|mut a| a.next())
                .ok_or(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "unresolvable host",
                ))
        })?,
        TIMEOUT,
    )?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    Ok(stream)
}

/// One Field Day contact for the push.
#[derive(Debug, Clone)]
pub struct N3fjpQso {
    pub call: String,
    pub class: String,
    pub section: String,
    /// Band in METERS ("20", not "14") — the N3FJP convention.
    pub band_meters: String,
    /// N3FJP mode string: "FT8" / "FT4" / "CW" / "SSB" (it buckets to
    /// CW/PH/DIG for contest scoring itself).
    pub mode: String,
    /// Dial/RF frequency in MHz, e.g. 14.074.
    pub freq_mhz: f64,
    /// QSO time, unix seconds (formatted YYYY/MM/DD + HH:MM UTC).
    pub when_unix: u64,
    pub operator: String,
}

/// Unix secs → ("YYYY/MM/DD", "HH:MM") UTC (same civil math as the Cabrillo
/// exporter — two tiny fields don't justify a date crate).
fn n3fjp_datetime(unix: u64) -> (String, String) {
    let secs_of_day = unix % 86_400;
    let days = (unix / 86_400) as i64;
    let (h, m) = (
        (secs_of_day / 3600) as u32,
        ((secs_of_day % 3600) / 60) as u32,
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
    (format!("{y:04}/{mo:02}/{d:02}"), format!("{h:02}:{m:02}"))
}

/// Build the ADDDIRECT command line for one QSO (without the trailing CRLF).
pub fn build_adddirect(q: &N3fjpQso) -> String {
    let (date, time) = n3fjp_datetime(q.when_unix);
    // EXCLUDEDUPES is a documented ADDDIRECT field; STAYOPEN is a separate
    // top-level command and we connect-per-push anyway — never send it here.
    let mut s = String::from("<CMD><ADDDIRECT><EXCLUDEDUPES>TRUE</EXCLUDEDUPES>");
    s.push_str(&format!("<fldCall>{}</fldCall>", esc(&q.call)));
    s.push_str(&format!("<fldDateStr>{date}</fldDateStr>"));
    s.push_str(&format!("<fldTimeOnStr>{time}</fldTimeOnStr>"));
    s.push_str(&format!("<fldBand>{}</fldBand>", esc(&q.band_meters)));
    s.push_str(&format!("<fldMode>{}</fldMode>", esc(&q.mode)));
    s.push_str(&format!("<fldFrequency>{:.4}</fldFrequency>", q.freq_mhz));
    s.push_str(&format!("<fldClass>{}</fldClass>", esc(&q.class)));
    s.push_str(&format!("<fldSection>{}</fldSection>", esc(&q.section)));
    if !q.operator.is_empty() {
        s.push_str(&format!("<fldOperator>{}</fldOperator>", esc(&q.operator)));
    }
    s.push_str("<fldComments>via Nexus</fldComments></CMD>");
    s
}

/// Push one QSO into N3FJP (connect → ADDDIRECT → CHECKLOG → close).
pub fn push_qso(host: &str, port: u16, q: &N3fjpQso) -> Result<(), String> {
    let mut stream = connect(host, port).map_err(|e| format!("N3FJP connect: {e}"))?;
    let line = build_adddirect(q);
    stream
        .write_all(format!("{line}\r\n<CMD><CHECKLOG></CMD>\r\n").as_bytes())
        .map_err(|e| format!("N3FJP send: {e}"))?;
    Ok(())
}

/// Build the full ENTER-sequence command block for one QSO — the
/// contest-correct path (N3FJP scores a contest log on ENTER; ADDDIRECT
/// bypasses it). Each command is CRLF-terminated; the block is ready to write
/// as-is. Order per the N3FJP API docs: freeze rig polls, set band+mode, fill
/// the entry form, fire ENTER, release rig polls.
pub fn build_enter_sequence(q: &N3fjpQso) -> String {
    let mut s = String::new();
    // Freeze N3FJP's own rig polling so a poll landing mid-sequence can't stomp
    // the band/mode/freq we're about to set on the entry form.
    s.push_str("<CMD><IGNORERIGPOLLS><VALUE>TRUE</VALUE></CMD>\r\n");
    // Band + mode without CAT (band in METERS — the N3FJP convention).
    s.push_str(&format!(
        "<CMD><CHANGEBM><BAND>{}</BAND><MODE>{}</MODE></CMD>\r\n",
        esc(&q.band_meters),
        esc(&q.mode)
    ));
    // Fill the FD-log entry controls. UPDATE targets FORM controls (txtEntry*),
    // NOT the ADDDIRECT DB field names (fld*).
    s.push_str(&format!(
        "<CMD><UPDATE><CONTROL>TXTENTRYFREQUENCY</CONTROL><VALUE>{:.4}</VALUE></CMD>\r\n",
        q.freq_mhz
    ));
    s.push_str(&format!(
        "<CMD><UPDATE><CONTROL>TXTENTRYCALL</CONTROL><VALUE>{}</VALUE></CMD>\r\n",
        esc(&q.call)
    ));
    s.push_str(&format!(
        "<CMD><UPDATE><CONTROL>TXTENTRYCLASS</CONTROL><VALUE>{}</VALUE></CMD>\r\n",
        esc(&q.class)
    ));
    s.push_str(&format!(
        "<CMD><UPDATE><CONTROL>TXTENTRYSECTION</CONTROL><VALUE>{}</VALUE></CMD>\r\n",
        esc(&q.section)
    ));
    // Operator initials/call (Field Day rotates ops, so this differs from the
    // station call). `txtEntryOperator` per the N3FJP API docs; only when set,
    // mirroring the ADDDIRECT `fldOperator` field.
    if !q.operator.is_empty() {
        s.push_str(&format!(
            "<CMD><UPDATE><CONTROL>TXTENTRYOPERATOR</CONTROL><VALUE>{}</VALUE></CMD>\r\n",
            esc(&q.operator)
        ));
    }
    // Fire the log action — this is what SCORES the contact (ADDDIRECT doesn't),
    // and the one command that answers (`<ENTERRESPONSE>` = records added).
    s.push_str("<CMD><ACTION><VALUE>ENTER</VALUE></CMD>\r\n");
    // Release rig polling.
    s.push_str("<CMD><IGNORERIGPOLLS><VALUE>FALSE</VALUE></CMD>\r\n");
    s
}

/// Extract the ENTER record count from N3FJP's reply
/// (`<CMD><ENTERRESPONSE><VALUE>N</VALUE></CMD>`; N = records added — 1 on
/// success, 0 when the QSO wasn't added). Tolerant of a bare `<VALUE>` reply.
fn parse_enter_response(resp: &str) -> Option<u32> {
    resp.split("<ENTERRESPONSE>")
        .nth(1)
        .or(Some(resp))
        .and_then(|r| r.split("<VALUE>").nth(1))
        .and_then(|r| r.split("</VALUE>").next())
        .and_then(|v| v.trim().parse::<u32>().ok())
}

/// Push one QSO via the ENTER sequence (connect → sequence → read
/// `<ENTERRESPONSE>` → close) — the contest-correct path that actually scores
/// the contact. Returns a short status on success; an `Err` when N3FJP added 0
/// records (dupe/rejected) or the reply is missing/unparseable, so the caller
/// can surface a scoring failure instead of losing it.
pub fn push_qso_enter(host: &str, port: u16, q: &N3fjpQso) -> Result<String, String> {
    let mut stream = connect(host, port).map_err(|e| format!("N3FJP connect: {e}"))?;
    stream
        .write_all(build_enter_sequence(q).as_bytes())
        .map_err(|e| format!("N3FJP send: {e}"))?;
    // ACTION ENTER is the one command that answers — mirror test_connection's read.
    let mut buf = [0u8; 1024];
    let n = stream
        .read(&mut buf)
        .map_err(|e| format!("N3FJP no ENTER response: {e}"))?;
    let resp = String::from_utf8_lossy(&buf[..n]);
    match parse_enter_response(&resp) {
        Some(0) => Err("N3FJP ENTER added 0 records (dupe or rejected)".to_string()),
        Some(added) => Ok(format!("N3FJP logged {added} record(s)")),
        None => Err(format!("N3FJP ENTER: unparseable reply: {}", resp.trim())),
    }
}

/// Build the no-CAT band-report command: `CHANGEBM` when N3FJP's rig interface
/// is OFF (set the entry band+mode directly), else `SENDRIGPOLL` (simulate a
/// rig reply, so N3FJP's own poll loop doesn't immediately overwrite a
/// CHANGEBM). Without the trailing CRLF.
fn build_band_report(band_meters: &str, mode: &str, freq_mhz: f64, rig_iface_on: bool) -> String {
    if rig_iface_on {
        format!(
            "<CMD><SENDRIGPOLL><FREQ>{freq_mhz:.4}</FREQ><MODE>{}</MODE></CMD>",
            esc(mode)
        )
    } else {
        format!(
            "<CMD><CHANGEBM><BAND>{}</BAND><MODE>{}</MODE></CMD>",
            esc(band_meters),
            esc(mode)
        )
    }
}

/// Report THIS position's band/mode to N3FJP *without CAT*, so the club's
/// Network Status Display band board shows where this operator is even though
/// N3FJP isn't polling the rig. Fire-and-forget (no reply to read). `band_meters`
/// is in METERS ("20"); `rig_iface_on` selects the verb (see [`build_band_report`]).
pub fn report_band(
    host: &str,
    port: u16,
    band_meters: &str,
    mode: &str,
    freq_mhz: f64,
    rig_iface_on: bool,
) -> Result<(), String> {
    let mut stream = connect(host, port).map_err(|e| format!("N3FJP connect: {e}"))?;
    let line = build_band_report(band_meters, mode, freq_mhz, rig_iface_on);
    stream
        .write_all(format!("{line}\r\n").as_bytes())
        .map_err(|e| format!("N3FJP band report: {e}"))?;
    Ok(())
}

/// Build the DUPECHECK command (silent pre-log dupe hint vs the combined club
/// log). Without the trailing CRLF.
fn build_dupecheck(call: &str, band_meters: &str, mode: &str) -> String {
    format!(
        "<CMD><DUPECHECK><CALL>{}</CALL><BAND>{}</BAND><MODE>{}</MODE></CMD>",
        esc(call),
        esc(band_meters),
        esc(mode)
    )
}

/// A DUPECHECK reply signals a worked-before contact. The exact reply tag is
/// version-dependent (a live dry-run should confirm it), so accept the truthy
/// tokens conservatively — a parse miss defaults to "not a dupe" so it never
/// blocks a real QSO.
fn parse_dupe_response(resp: &str) -> bool {
    let up = resp.to_ascii_uppercase();
    up.contains("<VALUE>TRUE") || up.contains("<VALUE>1<") || up.contains("<DUPE>TRUE")
}

/// Silent pre-log dupe hint: ask N3FJP whether `call` was already worked on this
/// band+mode in the *combined* club log. Warning-only — the combined-log check
/// has a race window (~1s TCP), so a `false` is never a guarantee. Returns
/// `true` = already worked.
pub fn dupecheck(
    host: &str,
    port: u16,
    call: &str,
    band_meters: &str,
    mode: &str,
) -> Result<bool, String> {
    let mut stream = connect(host, port).map_err(|e| format!("N3FJP connect: {e}"))?;
    stream
        .write_all(format!("{}\r\n", build_dupecheck(call, band_meters, mode)).as_bytes())
        .map_err(|e| format!("N3FJP dupecheck send: {e}"))?;
    let mut buf = [0u8; 1024];
    let n = stream
        .read(&mut buf)
        .map_err(|e| format!("N3FJP dupecheck no response: {e}"))?;
    Ok(parse_dupe_response(&String::from_utf8_lossy(&buf[..n])))
}

/// Test the connection: handshake `<CMD><PROGRAM></CMD>` and report what's on
/// the other end ("N3FJP's Field Day Contest Log v6.6").
pub fn test_connection(host: &str, port: u16) -> Result<String, String> {
    let mut stream = connect(host, port).map_err(|e| format!("connect failed: {e}"))?;
    stream
        .write_all(b"<CMD><PROGRAM></CMD>\r\n")
        .map_err(|e| format!("send failed: {e}"))?;
    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).map_err(|e| {
        format!("no response: {e} (is the TCP API enabled in N3FJP ▸ Settings ▸ API?)")
    })?;
    let resp = String::from_utf8_lossy(&buf[..n]);
    let pgm = resp
        .split("<PGM>")
        .nth(1)
        .and_then(|r| r.split("</PGM>").next())
        .unwrap_or("unknown program");
    let ver = resp
        .split("<VER>")
        .nth(1)
        .and_then(|r| r.split("</VER>").next())
        .unwrap_or("?");
    Ok(format!("{pgm} v{ver}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adddirect_line_matches_the_documented_grammar() {
        let q = N3fjpQso {
            call: "W1AW".into(),
            class: "2A".into(),
            section: "CT".into(),
            band_meters: "20".into(),
            mode: "FT8".into(),
            freq_mhz: 14.074,
            when_unix: 1_782_583_500, // 2026-06-27 18:05 UTC (FD Saturday)
            operator: "KD9TAW".into(),
        };
        let line = build_adddirect(&q);
        assert!(line.starts_with("<CMD><ADDDIRECT><EXCLUDEDUPES>TRUE</EXCLUDEDUPES>"));
        assert!(line.contains("<fldCall>W1AW</fldCall>"));
        assert!(line.contains("<fldDateStr>2026/06/27</fldDateStr>"));
        assert!(line.contains("<fldTimeOnStr>18:05</fldTimeOnStr>"));
        assert!(line.contains("<fldBand>20</fldBand>"), "band in METERS");
        assert!(line.contains("<fldMode>FT8</fldMode>"));
        assert!(line.contains("<fldFrequency>14.0740</fldFrequency>"));
        assert!(line.contains("<fldClass>2A</fldClass>"));
        assert!(line.contains("<fldSection>CT</fldSection>"));
        assert!(line.ends_with("</CMD>"));
        assert!(!line.contains('\n'), "single line; CRLF added at send");
    }

    #[test]
    fn values_are_xml_escaped() {
        let q = N3fjpQso {
            call: "A&B<C>".into(),
            class: "1D".into(),
            section: "WI".into(),
            band_meters: "40".into(),
            mode: "CW".into(),
            freq_mhz: 7.0,
            when_unix: 0,
            operator: String::new(),
        };
        let line = build_adddirect(&q);
        assert!(line.contains("<fldCall>A&amp;B&lt;C&gt;</fldCall>"));
    }

    #[test]
    fn enter_sequence_has_the_scoring_grammar() {
        let q = N3fjpQso {
            call: "W1AW".into(),
            class: "2A".into(),
            section: "CT".into(),
            band_meters: "20".into(),
            mode: "FT8".into(),
            freq_mhz: 14.074,
            when_unix: 1_782_583_500,
            operator: "KD9TAW".into(),
        };
        let block = build_enter_sequence(&q);
        // Rig polls frozen for the whole sequence, then released.
        assert!(block.contains("<CMD><IGNORERIGPOLLS><VALUE>TRUE</VALUE></CMD>"));
        assert!(block.contains("<CMD><IGNORERIGPOLLS><VALUE>FALSE</VALUE></CMD>"));
        // Band + mode set without CAT.
        assert!(block.contains("<CMD><CHANGEBM><BAND>20</BAND><MODE>FT8</MODE></CMD>"));
        // Entry-form controls carry the exchange (FORM controls, not fld* DB names).
        assert!(block.contains("<CONTROL>TXTENTRYFREQUENCY</CONTROL><VALUE>14.0740</VALUE>"));
        assert!(block.contains("<CONTROL>TXTENTRYCALL</CONTROL><VALUE>W1AW</VALUE>"));
        assert!(block.contains("<CONTROL>TXTENTRYCLASS</CONTROL><VALUE>2A</VALUE>"));
        assert!(block.contains("<CONTROL>TXTENTRYSECTION</CONTROL><VALUE>CT</VALUE>"));
        // Operator initials/call ride the FD Operator control (rotating ops).
        assert!(block.contains("<CONTROL>TXTENTRYOPERATOR</CONTROL><VALUE>KD9TAW</VALUE>"));
        // The ENTER action is what SCORES the contact.
        assert!(block.contains("<CMD><ACTION><VALUE>ENTER</VALUE></CMD>"));
        // Ordering: freeze → fill the form → ENTER → release.
        let freeze_at = block.find("<VALUE>TRUE").unwrap();
        let call_at = block.find("TXTENTRYCALL").unwrap();
        let enter_at = block.find("<ACTION>").unwrap();
        let release_at = block.find("<VALUE>FALSE").unwrap();
        assert!(
            freeze_at < call_at,
            "freeze rig polls before filling the form"
        );
        assert!(call_at < enter_at, "fill the form before ENTER");
        assert!(enter_at < release_at, "release rig polls after ENTER");
        assert!(block.ends_with("\r\n"), "every command CRLF-terminated");
    }

    #[test]
    fn enter_sequence_omits_operator_when_unset() {
        let q = N3fjpQso {
            call: "W1AW".into(),
            class: "2A".into(),
            section: "CT".into(),
            band_meters: "20".into(),
            mode: "FT8".into(),
            freq_mhz: 14.074,
            when_unix: 1_782_583_500,
            operator: String::new(),
        };
        let block = build_enter_sequence(&q);
        assert!(
            !block.contains("TXTENTRYOPERATOR"),
            "no empty operator control"
        );
    }

    #[test]
    fn enter_response_parses_the_record_count() {
        assert_eq!(
            parse_enter_response("<CMD><ENTERRESPONSE><VALUE>1</VALUE></CMD>"),
            Some(1)
        );
        // 0 records added = dupe/rejection → the caller reports a scoring failure.
        assert_eq!(
            parse_enter_response("<CMD><ENTERRESPONSE><VALUE>0</VALUE></CMD>"),
            Some(0)
        );
        // Tolerates a bare VALUE reply.
        assert_eq!(parse_enter_response("<VALUE>2</VALUE>"), Some(2));
        // Missing/garbage → None (surfaced as an error, never a false success).
        assert_eq!(parse_enter_response("<CMD><PONG></CMD>"), None);
    }

    #[test]
    fn band_report_picks_changebm_or_sendrigpoll() {
        // Rig interface OFF → CHANGEBM sets the entry band+mode directly.
        let off = build_band_report("40", "CW", 7.030, false);
        assert!(off.contains("<CMD><CHANGEBM><BAND>40</BAND><MODE>CW</MODE></CMD>"));
        assert!(!off.contains("SENDRIGPOLL"));
        // Rig interface ON → SENDRIGPOLL simulates a rig reply (a CHANGEBM would
        // be overwritten by N3FJP's own poll).
        let on = build_band_report("20", "DIG", 14.071, true);
        assert!(on.contains("<CMD><SENDRIGPOLL><FREQ>14.0710</FREQ><MODE>DIG</MODE></CMD>"));
        assert!(!on.contains("CHANGEBM"));
    }

    #[test]
    fn dupecheck_grammar_and_parse() {
        assert_eq!(
            build_dupecheck("W1AW", "10", "CW"),
            "<CMD><DUPECHECK><CALL>W1AW</CALL><BAND>10</BAND><MODE>CW</MODE></CMD>"
        );
        assert!(parse_dupe_response(
            "<CMD><DUPERESPONSE><VALUE>TRUE</VALUE></CMD>"
        ));
        assert!(!parse_dupe_response(
            "<CMD><DUPERESPONSE><VALUE>FALSE</VALUE></CMD>"
        ));
    }
}
