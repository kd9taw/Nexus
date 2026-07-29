//! APRS-IS — the internet side of APRS: protocol, filters, and the RX-iGate uplink rules.
//!
//! Pure logic, no sockets. The socket loop that drives this lives in `tempo_net::aprsis`, which
//! owns wire framing only (that crate cannot depend on this one — `tempo-core` reaches it through
//! `modes`). Keeping the protocol here means every rule below is a unit test rather than a live
//! connection, and it sits beside the [`parser`](super::parser) that decodes what the feed
//! delivers.
//!
//! Two directions, with very different stakes:
//!
//! * **Down (feed-in).** Log in, subscribe with a server-side filter, receive TNC2 text lines.
//!   Read-only needs no passcode at all (`pass -1`), so the feed works for any operator. A mistake
//!   here shows the wrong stations.
//!
//! * **Up (RX iGate).** Contribute packets our own antenna heard. A mistake here is published to a
//!   global network under the operator's callsign, and the rules that prevent it —
//!   [`gate_check`] and the q-construct in [`gated_line`] — are the reason this module exists.
//!   Every one of them is a documented APRS-IS obligation, not a preference.
//!
//! Nexus never gates internet→RF. That direction transmits unattended, which the alerts doctrine
//! forbids; nothing in this module can key a radio.
//!
//! References: <http://www.aprs-is.net/> — Connecting.aspx (login), javAPRSFilter.aspx (filters),
//! q.aspx (q-constructs), IGateDetails.aspx (the gating prohibitions), ServerDesign.aspx (the
//! server's own 30 s duplicate window). Cross-checked against Dire Wolf `src/igate.c`, aprx
//! `aprsis.c`, and aprsc `src/passcode.c` — all GPL-compatible reference implementations, read for
//! their documented behaviour only; no code was copied.

use super::packet::Tnc2;

/// Compute the APRS-IS passcode for a callsign.
///
/// The algorithm Steve Dimse released to the APRS community in April 2000 (from the aprsd
/// sources); still the one every server verifies against. The SSID is stripped and case folded,
/// then callsign bytes are XORed into a 0x73E2 seed alternating high/low byte, masked to 15 bits.
///
/// This is a *verification* code, not a secret: it is derivable from a public callsign by anyone,
/// which is why it belongs in ordinary settings and not the keychain. Nexus computes it from the
/// operator's own configured callsign and never for anyone else's.
pub fn passcode(call: &str) -> u16 {
    let base = call.split('-').next().unwrap_or("").trim().to_ascii_uppercase();
    let mut hash: u16 = 0x73E2;
    // The reference C loop reads two bytes per iteration, which for an odd-length callsign reads
    // the NUL terminator as the second byte — XOR by zero. The index-parity form is equivalent and
    // has no terminator to reason about.
    for (i, b) in base.bytes().take(9).enumerate() {
        hash ^= if i % 2 == 0 {
            u16::from(b) << 8
        } else {
            u16::from(b)
        };
    }
    hash & 0x7FFF
}

/// The passcode literal for a receive-only login. `-1` is not a number the hash can produce; it is
/// the documented sentinel telling the server "I will never send". An unverified client receives
/// the full stream normally and is refused only on upload.
pub const READ_ONLY_PASS: &str = "-1";

/// Build the APRS-IS login line, CRLF-terminated.
///
/// `user CALL pass CODE vers APP VERSION [filter FILTER]`. `pass` is [`READ_ONLY_PASS`] unless a
/// passcode is supplied. The app name and version must each be one whitespace-free word; `filter`
/// is a *server command* riding the login's trailing field, which is the preferred way to set it.
pub fn login_line(call: &str, code: Option<u16>, app: &str, version: &str, filter: &str) -> String {
    let pass = match code {
        Some(c) => c.to_string(),
        None => READ_ONLY_PASS.to_string(),
    };
    let app = sanitize_token(app, "Nexus");
    let version = sanitize_token(version, "0");
    let mut s = format!(
        "user {} pass {} vers {} {}",
        call.trim().to_ascii_uppercase(),
        pass,
        app,
        version
    );
    let filter = filter.trim();
    if !filter.is_empty() {
        s.push_str(" filter ");
        s.push_str(filter);
    }
    s.push_str("\r\n");
    s
}

/// Collapse whitespace out of a login token — the protocol splits the login line on spaces, so an
/// app name or version containing one would silently shift every field after it.
fn sanitize_token(s: &str, fallback: &str) -> String {
    let t: String = s
        .trim()
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .collect();
    if t.is_empty() {
        fallback.to_string()
    } else {
        t
    }
}

/// What the operator wants to see, translated into an APRS-IS server-side filter string.
///
/// Filter terms are space-separated and OR together — each is a *subscription* that adds traffic,
/// so ordering does not matter and an empty spec subscribes to nothing beyond the port default.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FilterSpec {
    /// Range filter anchored on the operator's own position, in km. `None` = no range term.
    ///
    /// Emitted as `r/lat/lon/km` when a position is known, NOT as the `m/km` "my range" filter.
    /// `m/` centres on the last position the *server* saw from this login, which for a
    /// receive-only station that never beacons is nothing at all — the filter is accepted and
    /// then silently matches zero packets. `r/` needs no server-side state.
    pub range_km: Option<u32>,
    /// Operator position (degrees) for the range filter. Without it, `range_km` falls back to
    /// `m/km`, which is the only remaining option and is honest about its limits.
    pub center: Option<(f64, f64)>,
    /// Watched callsigns — a budlist that passes their traffic from anywhere on earth,
    /// irrespective of range.
    pub buddies: Vec<String>,
    /// Include weather stations and positionless weather reports.
    pub weather: bool,
    /// Include objects and items (repeaters, NWS alerts, event markers).
    pub objects: bool,
    /// Include APRS text messages.
    pub messages: bool,
}

impl FilterSpec {
    /// Render the filter string for the login line. Empty when nothing is subscribed.
    pub fn build(&self) -> String {
        let mut terms: Vec<String> = Vec::new();
        if let Some(km) = self.range_km.filter(|k| *k > 0) {
            terms.push(match self.center {
                // Coordinates are signed decimal degrees; 2 dp is ~1 km, far finer than any
                // sensible range and short enough to keep the login line well under 512 bytes.
                Some((lat, lon)) => format!("r/{lat:.2}/{lon:.2}/{km}"),
                None => format!("m/{km}"),
            });
        }
        let buds: Vec<String> = self
            .buddies
            .iter()
            .map(|b| b.trim().to_ascii_uppercase())
            .filter(|b| !b.is_empty())
            .collect();
        if !buds.is_empty() {
            terms.push(format!("b/{}", buds.join("/")));
        }
        // One `t/` term carrying every wanted letter, which is how the filter is specified:
        // p=position o=object i=item m=message q=query s=status t=telemetry u=user-defined
        // n=NWS w=weather. Positions always ride along — a station with no position cannot map.
        let mut types = String::from("p");
        if self.objects {
            types.push_str("oi");
        }
        if self.messages {
            types.push('m');
        }
        if self.weather {
            types.push('w');
        }
        if types.len() > 1 {
            terms.push(format!("t/{types}"));
        }
        terms.join(" ")
    }
}

// ---------------------------------------------------------------------------------------------
// RX iGate uplink — the correctness core
// ---------------------------------------------------------------------------------------------

/// The q-construct a **receive-only** iGate appends. Per q.aspx: *"Receive-only IGates will use
/// this exclusively for all packets gated to APRS-IS."*
///
/// Not `qAR`. `qAR` advertises an iGate that can also deliver messages back to the station over
/// RF; emitting it from a gate that never transmits asks the network to route traffic at us that
/// we can never deliver. Nexus does not gate internet→RF at all, so `qAO` is the only correct
/// construct — see [`Q_GATED_BIDIRECTIONAL`] for the one it would use if that ever changed.
pub const Q_GATED_RX_ONLY: &str = "qAO";

/// The q-construct a **bidirectional** iGate appends. Unused: Nexus has no internet→RF path.
/// Present so the distinction is stated in the code rather than lost in a commit message.
pub const Q_GATED_BIDIRECTIONAL: &str = "qAR";

/// Path tokens that forbid gating a packet from RF to APRS-IS. Absolute: no setting relaxes them.
///
/// * `TCPIP` / `TCPXX` — the packet already came *from* the internet. Gating it back is a loop.
/// * `NOGATE` / `RFONLY` — the originating station asked to stay off the internet. That request is
///   the operator's to honour, not ours to weigh.
///
/// Matched case-insensitively and ignoring a trailing `*`, because a digipeated path carries the
/// has-been-repeated marker on used tokens.
pub const NO_GATE_TOKENS: [&str; 4] = ["TCPIP", "TCPXX", "NOGATE", "RFONLY"];

/// Source callsigns that are never a real station — an unconfigured radio's factory default.
const BOGUS_SOURCES: [&str; 3] = ["NOCALL", "N0CALL", "SERVER"];

/// Source PREFIXES that are never a real station: a path alias that leaked into the source field.
/// Matched on the prefix because the aliases are numbered (`WIDE1-1`, `TRACE2-2`, `TCPIP`).
const BOGUS_SOURCE_PREFIXES: [&str; 4] = ["WIDE", "TRACE", "RELAY", "TCP"];

/// Why a packet must not be gated to APRS-IS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateReject {
    /// Not a well-formed TNC2 line.
    Malformed,
    /// The path carries a token that forbids gating (the token is named).
    ForbiddenPath(String),
    /// A generic query (`?`). Answering these is a station's own business; relaying them is noise.
    GenericQuery,
    /// The source callsign is a path alias or an unconfigured-radio default.
    BogusSource(String),
    /// A third-party (`}`) packet whose inner header shows it came from the internet.
    ThirdPartyFromInternet,
    /// Empty information field — nothing to contribute.
    Empty,
}

impl GateReject {
    /// A short operator-facing reason, for the diagnostic counter in the APRS view.
    pub fn reason(&self) -> String {
        match self {
            GateReject::Malformed => "not a valid packet".into(),
            GateReject::ForbiddenPath(t) => format!("{t} in path"),
            GateReject::GenericQuery => "generic query".into(),
            GateReject::BogusSource(s) => format!("bogus source {s}"),
            GateReject::ThirdPartyFromInternet => "third-party from internet".into(),
            GateReject::Empty => "empty packet".into(),
        }
    }
}

/// Strip a trailing has-been-repeated marker and compare a path token case-insensitively.
fn token_is(tok: &str, want: &str) -> bool {
    tok.trim().trim_end_matches('*').eq_ignore_ascii_case(want)
}

/// May this RF-heard packet be gated to APRS-IS?
///
/// The caller must already have established that the packet was heard **on the air**. That is the
/// first and most important rule — an iGate exists to contribute what its own antenna received —
/// and it is not checkable from the bytes, so it is enforced structurally upstream: only the RF
/// decode path ever reaches this function.
///
/// Everything checkable from the bytes is checked here, per IGateDetails.aspx.
pub fn gate_check(line: &[u8]) -> Result<(), GateReject> {
    let t = Tnc2::split(line).ok_or(GateReject::Malformed)?;
    gate_check_parts(&t)
}

fn gate_check_parts(t: &Tnc2) -> Result<(), GateReject> {
    if t.info.is_empty() {
        return Err(GateReject::Empty);
    }
    for tok in &t.path {
        if let Some(bad) = NO_GATE_TOKENS.iter().find(|w| token_is(tok, w)) {
            return Err(GateReject::ForbiddenPath((*bad).to_string()));
        }
    }
    let src = t.source.trim().trim_end_matches('*');
    let src_base = src.split('-').next().unwrap_or(src);
    let bogus = BOGUS_SOURCES.iter().any(|b| src_base.eq_ignore_ascii_case(b))
        || BOGUS_SOURCE_PREFIXES.iter().any(|p| {
            src_base.len() >= p.len() && src_base[..p.len()].eq_ignore_ascii_case(p)
        });
    if bogus {
        return Err(GateReject::BogusSource(src.to_string()));
    }
    match t.info.first() {
        Some(b'?') => return Err(GateReject::GenericQuery),
        // A third-party wrapper is gated on its INNER header: if that carries TCPIP/TCPXX the
        // packet originated on the internet and came back to us over RF, and returning it would
        // close a loop. Anything else falls through and is gated as the ordinary packet it is.
        Some(b'}') => {
            if let Some(inner) = Tnc2::split(&t.info[1..]) {
                for tok in &inner.path {
                    if token_is(tok, "TCPIP") || token_is(tok, "TCPXX") {
                        return Err(GateReject::ThirdPartyFromInternet);
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Append the receive-only q-construct to a TNC2 line, producing the exact bytes to upload.
///
/// `SRC>DEST,path` + `,qAO,IGATECALL` + `:info`. IGateDetails.aspx: *"IGates must not modify paths
/// of packets gated to APRS-IS except to append `,qAR,IGATECALL`."* The RF path is preserved
/// byte-for-byte, `*` markers and all — it is the evidence of how the packet actually travelled,
/// and the network uses it for coverage analysis.
///
/// `None` if the line is not well-formed TNC2. Does **not** re-check [`gate_check`]; call that
/// first. Returns raw bytes so a non-UTF-8 information field survives intact.
pub fn gated_line(line: &[u8], igate_call: &str) -> Option<Vec<u8>> {
    let t = Tnc2::split(line)?;
    let call = igate_call.trim().to_ascii_uppercase();
    if call.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(line.len() + call.len() + 8);
    out.extend_from_slice(t.source.as_bytes());
    out.push(b'>');
    out.extend_from_slice(t.dest.as_bytes());
    for p in &t.path {
        out.push(b',');
        out.extend_from_slice(p.as_bytes());
    }
    out.push(b',');
    out.extend_from_slice(Q_GATED_RX_ONLY.as_bytes());
    out.push(b',');
    out.extend_from_slice(call.as_bytes());
    out.push(b':');
    out.extend_from_slice(t.info);
    Some(out)
}

/// The APRS-IS duplicate key: origin callsign (SSID significant), destination callsign (SSID
/// **ignored**), and the information field verbatim.
///
/// **The path is deliberately excluded** — ServerDesign.aspx: *"Note that the path is ignored in
/// duplicate checking."* One transmission reaching us through three digipeaters is one packet, and
/// keying on the path would call all three distinct and upload every copy.
fn dupe_key(t: &Tnc2) -> DupeKey {
    let src = t.source.trim().trim_end_matches('*').to_ascii_uppercase();
    let dest = t.dest.trim().trim_end_matches('*');
    let dest_base = dest.split('-').next().unwrap_or(dest).to_ascii_uppercase();
    (src, dest_base, t.info.to_vec())
}

/// The duplicate key: origin callsign, destination base callsign, information field.
type DupeKey = (String, String, Vec<u8>);

/// Default duplicate-suppression window, matching the server's own sliding window.
pub const DUPE_WINDOW_SECS: i64 = 30;

/// A sliding duplicate window over packets already uploaded.
///
/// APRS-IS servers apply a 30-second window themselves, so this is belt-and-braces rather than a
/// protocol obligation — Dire Wolf deliberately disabled its RF→IS duplicate check so the network
/// could see every copy for coverage analysis. Nexus keeps one because a stuck digipeater or a
/// beacon looping through two paths otherwise spends the operator's callsign on traffic the server
/// will discard anyway.
#[derive(Debug, Clone)]
pub struct DupeWindow {
    seen: std::collections::VecDeque<(i64, DupeKey)>,
    window: i64,
}

impl DupeWindow {
    /// A window of `secs` seconds.
    pub fn new(secs: i64) -> Self {
        DupeWindow {
            seen: std::collections::VecDeque::new(),
            window: secs.max(0),
        }
    }

    /// Is this packet new at time `now` (unix seconds)? Records it when so. A malformed line is
    /// never a duplicate — [`gate_check`] rejects it first.
    pub fn accept(&mut self, line: &[u8], now: i64) -> bool {
        while let Some((t, _)) = self.seen.front() {
            if now - *t > self.window {
                self.seen.pop_front();
            } else {
                break;
            }
        }
        let Some(t) = Tnc2::split(line) else {
            return true;
        };
        let key = dupe_key(&t);
        if self.seen.iter().any(|(_, k)| *k == key) {
            return false;
        }
        self.seen.push_back((now, key));
        true
    }
}

impl Default for DupeWindow {
    fn default() -> Self {
        DupeWindow::new(DUPE_WINDOW_SECS)
    }
}

/// Default uploads permitted per minute. Generous next to real traffic — a busy digipeater site
/// hears well under this — but a hard ceiling on what a stuck transmitter or a decoder fault can
/// spend the operator's callsign on.
pub const RATE_CAP_PER_MIN: u32 = 60;

/// A sliding one-minute upload cap.
#[derive(Debug, Clone)]
pub struct RateCap {
    sent: std::collections::VecDeque<i64>,
    per_min: u32,
}

impl RateCap {
    pub fn new(per_min: u32) -> Self {
        RateCap {
            sent: std::collections::VecDeque::new(),
            per_min,
        }
    }

    /// Is there budget to upload at `now` (unix seconds)? Consumes it when so.
    pub fn accept(&mut self, now: i64) -> bool {
        while let Some(t) = self.sent.front() {
            if now - *t >= 60 {
                self.sent.pop_front();
            } else {
                break;
            }
        }
        if self.sent.len() as u32 >= self.per_min {
            return false;
        }
        self.sent.push_back(now);
        true
    }
}

impl Default for RateCap {
    fn default() -> Self {
        RateCap::new(RATE_CAP_PER_MIN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The aprslib test vectors (tests/test_passcode.py) — the one published call→code table, and
    // reproducible from aprsc's C implementation.
    #[test]
    fn passcode_matches_the_published_vectors() {
        for (call, want) in [
            ("TESTCALL", 31742u16),
            ("N0CALL", 13023),
            ("SUCHCALL", 27890),
            ("MUCHSIGN", 27128),
            ("WOW", 29613),
        ] {
            assert_eq!(passcode(call), want, "passcode({call})");
        }
    }

    #[test]
    fn passcode_ignores_case_and_ssid() {
        // All four are the same station, so all four must verify with the same code.
        for call in ["TESTCALL", "testcall", "tEsTcAlL", "TESTCALL-12", "TESTCALL-0"] {
            assert_eq!(passcode(call), 31742, "passcode({call})");
        }
    }

    #[test]
    fn login_line_read_only_needs_no_passcode() {
        let s = login_line("kd9taw", None, "Nexus", "0.21.1", "r/41.90/-87.60/150");
        assert_eq!(
            s,
            "user KD9TAW pass -1 vers Nexus 0.21.1 filter r/41.90/-87.60/150\r\n"
        );
    }

    #[test]
    fn login_line_with_a_passcode_and_no_filter() {
        let s = login_line("KD9TAW", Some(passcode("KD9TAW")), "Nexus", "0.21.1", "  ");
        assert!(s.starts_with("user KD9TAW pass "));
        assert!(!s.contains("filter"), "no filter term when none is set");
        assert!(s.ends_with("\r\n"));
    }

    #[test]
    fn login_line_never_lets_a_token_shift_the_fields() {
        // A version string with a space would push `filter` into the version slot.
        let s = login_line("KD9TAW", None, "My App", "0.1 beta", "t/p");
        assert_eq!(s, "user KD9TAW pass -1 vers My-App 0.1-beta filter t/p\r\n");
    }

    #[test]
    fn filter_prefers_an_anchored_range_over_my_range() {
        let f = FilterSpec {
            range_km: Some(150),
            center: Some((41.9, -87.6)),
            weather: true,
            objects: true,
            messages: true,
            ..Default::default()
        };
        assert_eq!(f.build(), "r/41.90/-87.60/150 t/poimw");
    }

    #[test]
    fn filter_falls_back_to_my_range_without_a_position() {
        // `m/` matches nothing until the login has beaconed — which a receive-only station never
        // does — so it is the fallback, never the default. Grid-less operators get the honest
        // no-op rather than a silently wrong anchor.
        let f = FilterSpec {
            range_km: Some(50),
            center: None,
            ..Default::default()
        };
        assert_eq!(f.build(), "m/50");
    }

    #[test]
    fn filter_builds_a_budlist_and_omits_empty_terms() {
        let f = FilterSpec {
            range_km: None,
            center: None,
            buddies: vec!["kd9taw".into(), "  ".into(), "W9XYZ-9".into()],
            messages: true,
            ..Default::default()
        };
        assert_eq!(f.build(), "b/KD9TAW/W9XYZ-9 t/pm");
        assert_eq!(FilterSpec::default().build(), "", "nothing on = no filter");
    }

    // ---- the uplink rules. Every fixture is a real captured or reference-suite line.

    #[test]
    fn gate_refuses_every_forbidden_path_token() {
        // aprsc's own drop-rule regression fixtures (tests/t/11misc-drops.t).
        for (line, want) in [
            (&b"SRC-1>APRS,RFONLY,WIDE2-1:>should drop, RFONLY"[..], "RFONLY"),
            (b"SRC-1>APRS,NOGATE,WIDE2-1:>should drop, NOGATE", "NOGATE"),
            (b"SRC2>DST,DIGI,TCPXX,WIDE1-1:>unverified login", "TCPXX"),
            (b"SRC2>DST,DIGI,TCPXX*,WIDE1-1:>used marker too", "TCPXX"),
            (
                b"K9LGE-5>APDR16,TCPIP*,qAC,T2EDM:=4153.96N/08857.08W$098/065",
                "TCPIP",
            ),
        ] {
            match gate_check(line) {
                Err(GateReject::ForbiddenPath(t)) => assert_eq!(t, want),
                other => panic!("{} must be refused, got {other:?}", String::from_utf8_lossy(line)),
            }
        }
    }

    #[test]
    fn gate_matches_forbidden_tokens_regardless_of_case_or_used_marker() {
        for line in [
            &b"SRC>APRS,nogate:>lower case"[..],
            b"SRC>APRS,NoGate*:>mixed case with marker",
            b"SRC>APRS,rfonly*:>both",
        ] {
            assert!(
                matches!(gate_check(line), Err(GateReject::ForbiddenPath(_))),
                "must be refused: {}",
                String::from_utf8_lossy(line)
            );
        }
    }

    #[test]
    fn gate_accepts_an_ordinary_rf_heard_packet() {
        // Real RF paths, no internet tokens: exactly what an iGate exists to contribute.
        for line in [
            &b"KI4KK-9>SS5X8U,WIDE1-1,WIDE2-1:`j&$ {!v/]\"4%}="[..],
            b"KP4DMR-5>APOTU0,KP4DMR-8*,WIDE2-2:/191803z1804.03N/06553.03W>322/031",
            b"N3FLR-9>TP2Q1Q,K3MJW-1*,WIDE1*,NJ3T-9*,N3KTX-4*,WIDE2*:'kPl\"0k/]Voice Alert",
            b"SR9WXL>AKLPRZ,WIDE2-1:!4947.70N/01926.80E_310/000g002t056",
        ] {
            assert_eq!(
                gate_check(line),
                Ok(()),
                "must be gated: {}",
                String::from_utf8_lossy(line)
            );
        }
    }

    #[test]
    fn gate_refuses_generic_queries_and_bogus_sources() {
        assert_eq!(gate_check(b"SRC>APRS,WIDE1-1:?APRS?"), Err(GateReject::GenericQuery));
        assert_eq!(gate_check(b"SRC>APRS,WIDE1-1:"), Err(GateReject::Empty));
        for src in ["N0CALL", "NOCALL", "WIDE1-1", "TRACE", "RELAY"] {
            let line = format!("{src}>APRS,WIDE1-1:!4903.50N/07201.75W-");
            assert!(
                matches!(gate_check(line.as_bytes()), Err(GateReject::BogusSource(_))),
                "must be refused: {line}"
            );
        }
        assert!(matches!(gate_check(b"not a packet"), Err(GateReject::Malformed)));
    }

    #[test]
    fn gate_refuses_a_third_party_packet_that_came_from_the_internet() {
        // aprsc 11misc-drops.t: the inner header's TCPIP/TCPXX is what condemns it — gating this
        // back would close the loop the tokens exist to prevent.
        assert_eq!(
            gate_check(b"SRC>DST,DIGI:}SRC2>DST,DIGI,TCPIP*:>should drop, 3rd party"),
            Err(GateReject::ThirdPartyFromInternet)
        );
        assert_eq!(
            gate_check(b"SRC>DST,DIGI:}SRC3>DST,DIGI,TCPXX*:>should drop, 3rd party TCPXX"),
            Err(GateReject::ThirdPartyFromInternet)
        );
        // ...but a third-party frame that did NOT come from the internet is ordinary traffic.
        assert_eq!(
            gate_check(b"UU1AA>TEST,WIDE1-1:}OF7LZB>DST,NET,GATE:!6013.69NR02450.97E&"),
            Ok(())
        );
    }

    #[test]
    fn gated_line_appends_qao_and_leaves_the_rf_path_untouched() {
        // The worked example from IGateDetails.aspx, with the receive-only construct.
        let out = gated_line(
            b"SP3VN>URRT29,WIDE1-1:`,Rl\"R[/`\"4k}radio.sp3vn@gmail.com_0",
            "kd9taw-10",
        )
        .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out),
            "SP3VN>URRT29,WIDE1-1,qAO,KD9TAW-10:`,Rl\"R[/`\"4k}radio.sp3vn@gmail.com_0"
        );
    }

    #[test]
    fn gated_line_is_qao_not_qar_because_nexus_never_gates_to_rf() {
        let out = gated_line(b"SRC>DEST,WIDE1-1:!4903.50N/07201.75W-", "KD9TAW").unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains(",qAO,KD9TAW:"), "{s}");
        assert!(
            !s.contains("qAR"),
            "qAR advertises a message path back to RF that Nexus does not have: {s}"
        );
    }

    #[test]
    fn gated_line_preserves_used_digi_markers_and_a_pathless_packet() {
        let out = gated_line(b"W4BTA-8>SXQV2U,K3NAL-1,WIDE1,N3KTX-7,WIDE2*:'h5io_,j/]=", "KD9TAW")
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out),
            "W4BTA-8>SXQV2U,K3NAL-1,WIDE1,N3KTX-7,WIDE2*,qAO,KD9TAW:'h5io_,j/]="
        );
        // A direct packet with no digipeaters gets the construct as its only path element.
        let direct = gated_line(b"XE2N-10>APDW16:!2537.90NR10014.20W&", "KD9TAW").unwrap();
        assert_eq!(
            String::from_utf8_lossy(&direct),
            "XE2N-10>APDW16,qAO,KD9TAW:!2537.90NR10014.20W&"
        );
    }

    #[test]
    fn gated_line_keeps_a_non_utf8_information_field_byte_for_byte() {
        let mut line = b"OH7LZB-13>SX15S6,WIDE1-1:'I',l ".to_vec();
        line.push(0x1C);
        line.extend_from_slice(b">/]");
        let out = gated_line(&line, "KD9TAW").unwrap();
        assert!(out.contains(&0x1C), "the raw Mic-E byte must survive the uplink");
        assert!(out.ends_with(b">/]"));
    }

    #[test]
    fn gated_line_refuses_an_unconfigured_callsign() {
        assert!(gated_line(b"SRC>DEST,WIDE1-1:!hi", "   ").is_none());
        assert!(gated_line(b"not a packet", "KD9TAW").is_none());
    }

    #[test]
    fn dupe_window_ignores_the_path_like_the_server_does() {
        let mut w = DupeWindow::new(DUPE_WINDOW_SECS);
        // One transmission, heard through two different digipeaters: one packet.
        assert!(w.accept(b"KD9TAW-9>APRS,WIDE1-1:!4903.50N/07201.75W-", 1000));
        assert!(
            !w.accept(b"KD9TAW-9>APRS,W9XYZ-1*,WIDE2-1:!4903.50N/07201.75W-", 1002),
            "the path is excluded from the duplicate key"
        );
        // The destination SSID is ignored too, but the origin SSID is significant.
        assert!(!w.accept(b"KD9TAW-9>APRS-1,WIDE1-1:!4903.50N/07201.75W-", 1003));
        assert!(w.accept(b"KD9TAW-7>APRS,WIDE1-1:!4903.50N/07201.75W-", 1004));
    }

    #[test]
    fn dupe_window_lets_a_repeat_through_once_it_slides_past() {
        let mut w = DupeWindow::new(DUPE_WINDOW_SECS);
        let line = b"KD9TAW-9>APRS,WIDE1-1:!4903.50N/07201.75W-";
        assert!(w.accept(line, 1000));
        assert!(!w.accept(line, 1029), "still inside the 30 s window");
        assert!(w.accept(line, 1031), "a genuine later beacon must not be swallowed");
    }

    #[test]
    fn dupe_window_separates_different_payloads_from_the_same_station() {
        let mut w = DupeWindow::new(DUPE_WINDOW_SECS);
        assert!(w.accept(b"KD9TAW-9>APRS,WIDE1-1:!4903.50N/07201.75W-", 1000));
        assert!(w.accept(b"KD9TAW-9>APRS,WIDE1-1:!4903.51N/07201.75W-moved", 1001));
    }

    #[test]
    fn rate_cap_bounds_a_packet_storm_and_recovers() {
        let mut c = RateCap::new(3);
        assert!(c.accept(1000));
        assert!(c.accept(1000));
        assert!(c.accept(1000));
        assert!(!c.accept(1000), "the cap must bite");
        assert!(!c.accept(1059), "still inside the minute");
        assert!(c.accept(1060), "budget returns as the window slides");
    }
}
