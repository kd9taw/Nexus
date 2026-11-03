//! "Accept contacts other programs log": the reader for one datagram on Nexus's contact port.
//!
//! Nexus's contact port (`127.0.0.1:2239` by default, the FreeDV contact port grown up) takes the
//! contacts other programs log and puts them in the Nexus log. This module only READS: it says
//! what one datagram is, in one of the three forms programs push a logged contact in, and hands
//! each contact on as ADIF fields. The importer in tempo-app (`tempo_app::log_intake`) checks the
//! station call, the time and the call, and writes the contact through the ordinary import.
//!
//! The three forms, told apart by their first bytes (they cannot be confused):
//!
//! - **WSJT-X binary** (magic `0xADBCCBDA`): a **QSO Logged (type 5)** or a **Logged ADIF
//!   (type 12)** from any sender id: WSJT-X, JTDX, MSHV, JTAlert and GridTracker forwards. A
//!   program that sends both for one contact (WSJT-X does) is logged once: the second meets the
//!   importer's duplicate check. FreeDV's id is read by [`crate::freedv`] first, as it always was.
//! - **Raw ADIF text**: one or more records, with or without a header, as Ham Radio Deluxe's QSO
//!   Forwarding and Log4OM's ADIF inbound take them (the S.A.T. box's push, set to either). At
//!   most [`MAX_RECORDS`] records; a datagram holding more is refused whole.
//! - **N1MM `<contactinfo>` XML**, from N1MM's published list of broadcast fields
//!   (n1mmwp.hamdocs.com, "External UDP Broadcasts"). Its frequencies are in units of 10 Hz.
//!   `<contactreplace>` and `<contactdelete>` (an edit and a delete in N1MM's log) are NOT read in
//!   this version: a contact edited or deleted in N1MM stays as it was first logged here.
//!
//! ⚠️ **Everything here is untrusted input from a network port**, so every reader is total: any
//! bytes of any length answer an [`Intake`] and never panic. A datagram past [`MAX_DATAGRAM`] is
//! dropped whole. Only the fields in [`KEPT`] are kept, each value cleaned ([`clean`]): no
//! control characters, no `<` or `>` (so no value can open a field of its own), at most
//! [`MAX_VALUE`] characters. No field that says a contact is confirmed, uploaded or Nexus's own
//! (`QSL_RCVD`, `LOTW_QSL_SENT`, `APP_NEXUS_*` …) can come in this way: a program on the network
//! cannot mark a contact confirmed. Nothing here keys, tunes, replies or names a file.

use crate::wsjtx::{parse_inbound, Inbound, LoggedQso, MAGIC};

pub use crate::freedv::MAX_DATAGRAM;

/// The most contacts one datagram may carry. A logging program pushes one at a time; a datagram
/// with more is refused whole ([`Intake::TooBig`]).
pub const MAX_RECORDS: usize = 5;
/// The most fields one record may carry, kept or not. A contact has a few dozen.
pub const MAX_FIELDS: usize = 100;
/// The most characters one field's value keeps.
pub const MAX_VALUE: usize = 256;
/// The sender id Nexus puts on its own WSJT-X datagrams ([`crate::server::APP_ID`]).
pub const NEXUS_WSJTX_ID: &str = crate::server::APP_ID;
/// The `PROGRAMID` Nexus writes in its own ADIF (its HRD push, its type 12).
pub const NEXUS_PROGRAM_ID: &str = "Nexus";
/// The program name a `<contactinfo>` contact is logged as from.
pub const N1MM: &str = "N1MM";

/// The ADIF fields a contact from another program keeps: what the contact WAS (who, when, where,
/// how, the exchange, the satellite, the parks), never what has happened to it since (a QSL, an
/// upload, a confirmation) and never Nexus's own bookkeeping. Anything else is dropped.
pub const KEPT: &[&str] = &[
    "CALL",
    "QSO_DATE",
    "TIME_ON",
    "QSO_DATE_OFF",
    "TIME_OFF",
    "BAND",
    "BAND_RX",
    "FREQ",
    "FREQ_RX",
    "MODE",
    "SUBMODE",
    "RST_SENT",
    "RST_RCVD",
    "TX_PWR",
    "RX_PWR",
    "GRIDSQUARE",
    "VUCC_GRIDS",
    "NAME",
    "QTH",
    "COMMENT",
    "STATE",
    "CNTY",
    "DXCC",
    "CQZ",
    "ITUZ",
    "CONT",
    "IOTA",
    "PROP_MODE",
    "SAT_NAME",
    "SAT_MODE",
    "STATION_CALLSIGN",
    "OPERATOR",
    "MY_GRIDSQUARE",
    "MY_VUCC_GRIDS",
    "MY_RIG",
    "MY_ANTENNA",
    "MY_STATE",
    "MY_CNTY",
    "POTA_REF",
    "SOTA_REF",
    "WWFF_REF",
    "SIG",
    "SIG_INFO",
    "MY_POTA_REF",
    "MY_SOTA_REF",
    "MY_WWFF_REF",
    "MY_SIG",
    "MY_SIG_INFO",
    "CONTEST_ID",
    "SRX",
    "STX",
    "SRX_STRING",
    "STX_STRING",
    "ARRL_SECT",
    "CLASS",
];

/// One contact's ADIF fields: upper-case names from [`KEPT`], cleaned values, in the order sent.
pub type Fields = Vec<(String, String)>;

/// The form a contact came in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// WSJT-X binary, type 5 or type 12.
    Wsjtx,
    /// Raw ADIF text.
    Adif,
    /// N1MM `<contactinfo>` XML.
    N1mm,
}

/// What one datagram on the contact port is.
#[derive(Debug, Clone, PartialEq)]
pub enum Intake {
    /// One or more contacts ([`MAX_RECORDS`] at most). `program` is the sender's own name for
    /// itself when it gave one: the WSJT-X sender id, the ADIF header's `PROGRAMID`, or [`N1MM`].
    /// Not yet checked against the station: that is the importer's job.
    Contacts {
        format: Format,
        program: Option<String>,
        records: Vec<Fields>,
    },
    /// Nexus's own datagram come back (WSJT-X id `Tempo`, `PROGRAMID` Nexus, N1MM `<app>NEXUS`):
    /// dropped, so nothing Nexus sends can log itself twice.
    Own,
    /// A datagram with more than [`MAX_RECORDS`] contacts, or a record with more than
    /// [`MAX_FIELDS`] fields: refused whole.
    TooBig,
    /// Anything else: not a logged contact (a heartbeat, a status, a decode, an N1MM edit),
    /// not one of the three forms, too long.
    Ignored,
}

/// Read one datagram. Total: any bytes answer.
pub fn read_datagram(bytes: &[u8]) -> Intake {
    if bytes.len() > MAX_DATAGRAM {
        return Intake::Ignored;
    }
    if bytes.len() >= 4 && u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == MAGIC {
        return read_wsjtx(bytes);
    }
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches(['\u{feff}', ' ', '\t', '\r', '\n']);
    if let Some(root) = xml_root(text) {
        return match root {
            "contactinfo" => read_contactinfo(text),
            _ => Intake::Ignored,
        };
    }
    if find_ci(text, "<eor>").is_some() {
        return read_adif_text(text, Format::Adif, None);
    }
    Intake::Ignored
}

/// The ADIF text of a WSJT-X Logged ADIF (type 12) already taken off its datagram (the companion
/// socket's, [`crate::wsjtx::Inbound::LoggedAdif`]): read as the contact port reads one, with
/// `program` (the sender id, when the caller still has it) over the envelope's `PROGRAMID`.
pub fn read_logged_adif(text: &str, program: Option<&str>) -> Intake {
    if text.len() > MAX_DATAGRAM {
        return Intake::Ignored;
    }
    read_adif_text(text, Format::Wsjtx, program.and_then(program_name))
}

fn read_wsjtx(bytes: &[u8]) -> Intake {
    match parse_inbound(bytes) {
        Some(Inbound::QsoLogged { id, .. } | Inbound::LoggedAdif { id, .. })
            if id == NEXUS_WSJTX_ID =>
        {
            Intake::Own
        }
        Some(Inbound::QsoLogged { id, qso }) => Intake::Contacts {
            format: Format::Wsjtx,
            program: program_name(&id),
            records: vec![qso_fields(&qso)],
        },
        Some(Inbound::LoggedAdif { id, adif }) => {
            read_adif_text(&adif, Format::Wsjtx, program_name(&id))
        }
        _ => Intake::Ignored,
    }
}

/// A sender's name for itself, cleaned and at most 32 characters; `None` when it is blank.
fn program_name(s: &str) -> Option<String> {
    let s: String = clean(s).chars().take(32).collect();
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// A QSO Logged (type 5) as ADIF fields.
fn qso_fields(q: &LoggedQso) -> Fields {
    let mut f = Fields::new();
    let mut put = |k: &str, v: &str| {
        let v = clean(v);
        if !v.is_empty() {
            f.push((k.to_string(), v));
        }
    };
    put("CALL", &q.dx_call);
    put("GRIDSQUARE", &q.dx_grid);
    put("MODE", &q.mode);
    if q.tx_freq > 0 {
        put("FREQ", &format!("{:.6}", q.tx_freq as f64 / 1e6));
    }
    if let Some((d, t)) = adif_time(q.time_on) {
        put("QSO_DATE", &d);
        put("TIME_ON", &t);
    }
    if let Some((d, t)) = adif_time(q.time_off) {
        put("QSO_DATE_OFF", &d);
        put("TIME_OFF", &t);
    }
    put("RST_SENT", &q.report_sent);
    put("RST_RCVD", &q.report_recvd);
    put("TX_PWR", &q.tx_power);
    put("COMMENT", &q.comments);
    put("NAME", &q.name);
    put("OPERATOR", &q.op_call);
    put("STATION_CALLSIGN", &q.my_call);
    put("MY_GRIDSQUARE", &q.my_grid);
    put("STX_STRING", &q.exchange_sent);
    put("SRX_STRING", &q.exchange_recvd);
    put("PROP_MODE", &q.adif_propmode);
    put("SAT_NAME", &q.satellite);
    put("SAT_MODE", &q.sat_mode);
    put("FREQ_RX", &q.freq_rx);
    f
}

/// Untrusted text for one field: control characters and the tag brackets dropped, trimmed, at
/// most [`MAX_VALUE`] characters.
pub fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() && *c != '<' && *c != '>')
        .take(MAX_VALUE)
        .collect::<String>()
        .trim()
        .to_string()
}

/// `(YYYYMMDD, HHMMSS)` UTC for a Unix time from 2000 to 2099; anything else is no time a
/// contact was made at.
pub fn adif_time(unix: i64) -> Option<(String, String)> {
    if !(946_684_800..4_102_444_800).contains(&unix) {
        return None;
    }
    // "YYYY-MM-DD HH:MM:SS", the same civil-date arithmetic the N1MM writer uses.
    let ts = crate::n1mm::utc_timestamp(unix as u64);
    let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
    let (date, time) = ts.split_once(' ')?;
    Some((digits(date), digits(time)))
}

/// The byte offset of `needle` in `hay`, ASCII case aside.
fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// Read ADIF text: its header (when it has one) for `PROGRAMID`, then each record ended by
/// `<EOR>`. A record left open at the end is not a record. `program` (the WSJT-X sender id) wins
/// over the header's `PROGRAMID`.
fn read_adif_text(text: &str, format: Format, program: Option<String>) -> Intake {
    let mut header_program: Option<String> = None;
    let mut records: Vec<Fields> = Vec::new();
    let mut current = Fields::new();
    let mut count = 0usize;
    let b = text.as_bytes();
    let mut i = 0usize;
    while let Some(off) = b[i..].iter().position(|&c| c == b'<') {
        let open = i + off;
        let Some(close_off) = b[open..].iter().position(|&c| c == b'>') else {
            break;
        };
        let close = open + close_off;
        // Inside the brackets: NAME, or NAME:LEN, or NAME:LEN:TYPE. ASCII only, so slicing at
        // these offsets is on character boundaries.
        let spec = &b[open + 1..close];
        i = close + 1;
        let mut parts = spec.splitn(3, |&c| c == b':');
        let name: String = parts
            .next()
            .unwrap_or_default()
            .iter()
            .map(|c| c.to_ascii_uppercase() as char)
            .collect();
        let Some(len) = parts.next() else {
            match name.trim() {
                "EOH" => {
                    // What came before was the header: keep its PROGRAMID, never a contact.
                    header_program = current
                        .iter()
                        .find(|(k, _)| k == "PROGRAMID")
                        .and_then(|(_, v)| program_name(v));
                    current.clear();
                    count = 0;
                }
                "EOR" => {
                    if count > MAX_FIELDS {
                        return Intake::TooBig;
                    }
                    let fields: Fields = std::mem::take(&mut current)
                        .into_iter()
                        .filter(|(k, _)| KEPT.contains(&k.as_str()))
                        .collect();
                    count = 0;
                    if !fields.is_empty() {
                        records.push(fields);
                        if records.len() > MAX_RECORDS {
                            return Intake::TooBig;
                        }
                    }
                }
                _ => {}
            }
            continue;
        };
        // A length of digits that fits in what is left; anything else is not a field, and the
        // reading carries on after its tag.
        let len = std::str::from_utf8(len)
            .ok()
            .filter(|l| !l.is_empty() && l.len() <= 5 && l.bytes().all(|c| c.is_ascii_digit()))
            .and_then(|l| l.parse::<usize>().ok());
        let Some(len) = len else { continue };
        let end = i.saturating_add(len);
        if end > b.len() || !text.is_char_boundary(end) {
            break;
        }
        let value = &text[i..end];
        i = end;
        count += 1;
        if name == "PROGRAMID" || KEPT.contains(&name.as_str()) {
            let value = clean(value);
            if !value.is_empty() && current.len() <= MAX_FIELDS {
                current.retain(|(k, _)| *k != name);
                current.push((name, value));
            }
        }
    }
    let program = program.or(header_program);
    if program
        .as_deref()
        .is_some_and(|p| p.eq_ignore_ascii_case(NEXUS_PROGRAM_ID))
    {
        return Intake::Own;
    }
    if records.is_empty() {
        return Intake::Ignored;
    }
    Intake::Contacts {
        format,
        program,
        records,
    }
}

/// The name of the first element of an XML text, after an optional `<?xml …?>` declaration;
/// `None` when the text does not open like XML.
fn xml_root(text: &str) -> Option<&str> {
    let mut t = text;
    if t.starts_with("<?") {
        let end = t.find("?>")?;
        t = t[end + 2..].trim_start();
    }
    let rest = t.strip_prefix('<')?;
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    let name = &rest[..end];
    // ADIF's `<CALL:5>` and `<EOH>` open with a name too: an XML root closes its tag right after
    // its name, and the text closes the root.
    (!name.is_empty() && rest[end..].starts_with(['>', ' ']) && t.contains(&format!("</{name}>")))
        .then_some(name)
}

/// The text of `<tag>…</tag>` in `xml`, entities read, cleaned; `""` when absent.
fn xml_field(xml: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let Some(start) = xml.find(&open).map(|s| s + open.len()) else {
        return String::new();
    };
    let Some(len) = xml[start..].find(&close) else {
        return String::new();
    };
    let raw = &xml[start..start + len];
    clean(
        &raw.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&"),
    )
}

/// An N1MM `<contactinfo>` as one contact. Nexus's own broadcast (`<app>NEXUS</app>`) is
/// [`Intake::Own`]; a contact N1MM marks as not counted (`<IsClaimedQso>0`, an X-QSO) is not
/// logged.
fn read_contactinfo(xml: &str) -> Intake {
    let Some(end) = xml.find("</contactinfo>") else {
        return Intake::Ignored;
    };
    let xml = &xml[..end];
    if xml_field(xml, "app").eq_ignore_ascii_case("NEXUS") {
        return Intake::Own;
    }
    if xml_field(xml, "IsClaimedQso") == "0" {
        return Intake::Ignored;
    }
    let mut f = Fields::new();
    let mut put = |k: &str, v: String| {
        if !v.is_empty() {
            f.push((k.to_string(), v));
        }
    };
    put("CALL", xml_field(xml, "call"));
    // "2020-01-17 16:43:38" UTC.
    let ts = xml_field(xml, "timestamp");
    if let Some((date, time)) = ts.split_once(' ') {
        let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
        put("QSO_DATE", digits(date));
        put("TIME_ON", digits(time));
    }
    // Units of 10 Hz: 352519 is 3.52519 MHz.
    let mhz = |tag: &str| {
        xml_field(xml, tag)
            .parse::<u64>()
            .ok()
            .filter(|f| *f > 0)
            .map(|f| f as f64 / 1e5)
    };
    let (tx, rx) = (mhz("txfreq"), mhz("rxfreq"));
    if let Some(tx) = tx.or(rx) {
        put("FREQ", format!("{tx:.6}"));
    }
    if let (Some(tx), Some(rx)) = (tx, rx) {
        if (tx - rx).abs() > 1e-7 {
            put("FREQ_RX", format!("{rx:.6}"));
        }
    }
    put("MODE", xml_field(xml, "mode"));
    put("RST_SENT", xml_field(xml, "snt"));
    put("RST_RCVD", xml_field(xml, "rcv"));
    let number = |tag: &str| {
        let n = xml_field(xml, tag);
        if n.parse::<u64>().is_ok_and(|n| n > 0) {
            n
        } else {
            String::new()
        }
    };
    put("STX", number("sntnr"));
    put("SRX", number("rcvnr"));
    put("GRIDSQUARE", xml_field(xml, "gridsquare"));
    put("NAME", xml_field(xml, "name"));
    put("QTH", xml_field(xml, "qth"));
    put("COMMENT", xml_field(xml, "comment"));
    // The worked station's power, in N1MM's own words.
    put("RX_PWR", xml_field(xml, "power"));
    put("STATION_CALLSIGN", xml_field(xml, "mycall"));
    put("OPERATOR", xml_field(xml, "operator"));
    let contest = xml_field(xml, "contestname");
    if !contest.eq_ignore_ascii_case(crate::n1mm::GENERAL_LOG) {
        put("CONTEST_ID", contest);
    }
    Intake::Contacts {
        format: Format::N1mm,
        program: Some(N1MM.to_string()),
        records: vec![f],
    }
}

#[cfg(test)]
mod tests;
