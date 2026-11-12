//! PSK Reporter query-API adapter (the `live` feature).
//!
//! Pulls the operator's recent reception reports (both "who hears me" and "who
//! I hear", via `callsign=`) from `retrieve.pskreporter.info/query` and
//! normalizes the XML `<receptionReport>` rows into [`PathSpot`]s. These feed
//! both the advisor (band/region scoring) and the opening detector (per-band).
//!
//! RATE LIMIT: PSK Reporter asks for no more than one query per dataset per
//! 5 minutes — the caller must cache and not poll faster.
//!
//! **Incremental polling** ([`ReportFeed`]). Every answer carries
//! `<lastSequenceNumber value="N"/>`, the last sequence number in PSK Reporter's database when it
//! answered; a query sending `lastseqno=N` gets only the records numbered N or later. So after
//! the first answer of a run each query asks only for what is new, and the feed keeps the
//! nowcast's half-hour view itself, from the answers it has had: the nowcast sees the same
//! reports a fresh half-hour query would have returned. The first query of a run is a catch-up
//! instead: as far back as the newest report the operator has kept ([`look_back_secs`]), up to
//! the 24 hours PSK Reporter allows, never less than the view.
//!
//! **Row limit.** Without `rptlimit` PSK Reporter returns only its last 100 records, so a busy
//! station lost reports silently. Every query now sends [`RPTLIMIT`]. An answer that reaches it
//! may still be short of what was there; the feed says so once a run ([`ReportFeed::accept`]) and
//! the next five-minute query carries on from that answer's sequence number. It never asks
//! sooner.

use std::time::Duration;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::model::{Band, PathSpot};

// PSK Reporter's `appcontact` is how they reach the APP's maintainer about query
// behavior — a project contact, never an end-user's personal address (this ships
// in every binary). Use the public project repo.
const APPCONTACT: &str = "https://github.com/kd9taw/nexus";
/// The most a reception-report answer is read to: XML rows of about 300 bytes each, so this is
/// roughly a hundred thousand reports, well past a busy station's window.
const REPORTS_REPLY_BYTES: u64 = 32 * 1024 * 1024;
const UA: &str = "nexus-propagation/0.1 (+ham radio propagation nowcast)";

/// The most reports one query asks for (`rptlimit`). PSK Reporter answers only its last 100
/// without it. A thousand is two hundred reports a minute over a five-minute poll, far past a
/// busy station's rate, and a few hundred kilobytes of XML; only a launch catch-up over many
/// hours can reach it, and then the next poll carries on (see the module header).
pub const RPTLIMIT: usize = 1000;

/// The furthest back PSK Reporter lets a query reach (`flowStartSeconds`), seconds.
pub const MAX_LOOK_BACK_SECS: i64 = 24 * 3600;

/// One reception-report query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// How far back to ask, seconds (`flowStartSeconds=-N`).
    pub look_back_secs: i64,
    /// Only records numbered this or later (`lastseqno`), when known.
    pub last_seqno: Option<i64>,
}

/// One answer: its reports, the database's last sequence number, and how many rows it held.
#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub spots: Vec<PathSpot>,
    /// `<lastSequenceNumber value="N"/>`, when the answer carried one.
    pub last_seqno: Option<i64>,
    /// `<receptionReport>` rows in the answer, kept or not (a row on no known band is not a
    /// spot, but it counts toward [`RPTLIMIT`]).
    pub rows: usize,
}

impl Reply {
    /// Whether the answer reached [`RPTLIMIT`], so it may be short of what was there.
    pub fn at_limit(&self) -> bool {
        self.rows >= RPTLIMIT
    }
}

/// How far back a query reaches when the last report already in hand is from `since`: back to
/// it, never less than `floor` (the view the nowcast shows), never more than
/// [`MAX_LOOK_BACK_SECS`]. With nothing in hand, the whole 24 hours.
pub fn look_back_secs(now: i64, since: Option<i64>, floor: i64) -> i64 {
    let floor = floor.clamp(1, MAX_LOOK_BACK_SECS);
    since.map_or(MAX_LOOK_BACK_SECS, |t| {
        (now - t).clamp(floor, MAX_LOOK_BACK_SECS)
    })
}

/// The query URL for `mycall`. Only `lastseqno` (when known) and `rptlimit` are added to what
/// Nexus has always sent.
pub fn query_url(mycall: &str, q: &Query) -> String {
    let mut url = format!(
        "https://retrieve.pskreporter.info/query?callsign={}&flowStartSeconds=-{}&rronly=1",
        mycall.trim(),
        q.look_back_secs.max(1),
    );
    if let Some(n) = q.last_seqno {
        url.push_str(&format!("&lastseqno={n}"));
    }
    url.push_str(&format!("&rptlimit={RPTLIMIT}&appcontact={APPCONTACT}"));
    url
}

/// Fetch the operator's reception reports from the last `window_secs` seconds (a one-off query,
/// with no cursor: the example's).
pub fn fetch_paths(mycall: &str, window_secs: i64) -> Result<Vec<PathSpot>, String> {
    let q = Query {
        look_back_secs: window_secs,
        last_seqno: None,
    };
    fetch(mycall, &q).map(|r| r.spots)
}

/// Run one query.
pub fn fetch(mycall: &str, q: &Query) -> Result<Reply, String> {
    let url = query_url(mycall, q);
    // Same transport discipline as every other connector (qrz/clublog/eqsl/
    // lotw/…): HTTPS only, no redirects, and errors REDACTED to fixed strings —
    // reqwest's Display can echo the request URL, and while this endpoint
    // carries no credential today, the next person adding auth to a PSKR-style
    // fetch copies whichever template exists.
    let c = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "PSK Reporter: HTTP client initialization failed".to_string())?;
    let resp = c
        .get(&url)
        .send()
        .map_err(|_| "PSK Reporter: request failed (network)".to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "PSK Reporter: server returned HTTP {}",
            resp.status().as_u16()
        ));
    }
    let xml = super::body::read_capped(resp, REPORTS_REPLY_BYTES, "PSK Reporter")?;
    Ok(parse_reply(&xml))
}

/// Parse the `<receptionReport .../>` rows of a PSK Reporter XML response.
pub fn parse_reports(xml: &str) -> Vec<PathSpot> {
    parse_reply(xml).spots
}

/// Parse a PSK Reporter XML response: its rows, and its `<lastSequenceNumber value="N"/>`.
pub fn parse_reply(xml: &str) -> Reply {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut out = Reply::default();
    loop {
        match reader.read_event_into(&mut buf) {
            // Rows are self-closing (`Empty`), but tolerate `Start` too.
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => match e.name().as_ref() {
                b"receptionReport" => {
                    out.rows += 1;
                    if let Some(s) = report_from_attrs(&e) {
                        out.spots.push(s);
                    }
                }
                b"lastSequenceNumber" => {
                    out.last_seqno = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"value")
                        .and_then(|a| String::from_utf8_lossy(&a.value).parse().ok());
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// The run's PSK Reporter polling state for one call: whether the launch catch-up has been
/// answered, the half-hour view the nowcast reads, and whether the row limit has been reported.
/// The sequence-number cursor itself is the caller's to keep (the heard-reports store), passed
/// in to [`Self::next_query`]. No clock, no network: the caller runs the query.
#[derive(Debug, Clone, Default)]
pub struct ReportFeed {
    /// When the last answer was had (Unix seconds); `None` until the catch-up is answered.
    answered_at: Option<i64>,
    /// Every report in the view, newest first, each once.
    view: Vec<PathSpot>,
    limit_reported: bool,
}

impl ReportFeed {
    pub fn new() -> Self {
        Self::default()
    }

    /// The next query. Until an answer is had this run, the launch catch-up: back to
    /// `newest_kept` (the newest report the operator has kept, see [`look_back_secs`]) and no
    /// cursor, so the view fills whatever the cursor says. After that, `cursor` (the last
    /// sequence number seen) as `lastseqno`, and back to the last answer, never less than
    /// `window_secs`, so a gap in polling (a sleep, a failing network) is still covered.
    pub fn next_query(
        &self,
        now: i64,
        window_secs: i64,
        newest_kept: Option<i64>,
        cursor: Option<i64>,
    ) -> Query {
        match self.answered_at {
            None => Query {
                look_back_secs: look_back_secs(now, newest_kept, window_secs),
                last_seqno: None,
            },
            Some(at) => Query {
                look_back_secs: look_back_secs(now, Some(at), window_secs),
                last_seqno: cursor,
            },
        }
    }

    /// Take an answer had at `now` into the view, which keeps the last `window_secs`. Returns
    /// true the first time this run an answer reaches [`RPTLIMIT`], for the caller to say so once.
    pub fn accept(&mut self, reply: &Reply, now: i64, window_secs: i64) -> bool {
        self.answered_at = Some(now);
        let cutoff = now - window_secs;
        let mut view: Vec<PathSpot> = reply
            .spots
            .iter()
            .chain(self.view.iter())
            .filter(|s| s.time >= cutoff)
            .cloned()
            .collect();
        // Each report once (`lastseqno` returns the one AT the number again), the newest first
        // as PSK Reporter orders an answer; the first copy is the newer answer's.
        let mut seen = std::collections::HashSet::new();
        view.retain(|s| seen.insert(report_key(s)));
        view.sort_by(|a, b| b.time.cmp(&a.time));
        self.view = view;
        let first = reply.at_limit() && !self.limit_reported;
        self.limit_reported |= reply.at_limit();
        first
    }

    /// The reports of the last `window_secs` as of the last answer, newest first.
    pub fn view(&self) -> &[PathSpot] {
        &self.view
    }
}

/// What makes two rows the same report: sender, receiver, time, band, mode (as the store's key).
fn report_key(s: &PathSpot) -> (String, String, i64, &'static str, String) {
    (
        s.tx_call.to_ascii_uppercase(),
        s.rx_call.to_ascii_uppercase(),
        s.time,
        s.band.label(),
        s.mode.clone().unwrap_or_default(),
    )
}

fn report_from_attrs(e: &BytesStart) -> Option<PathSpot> {
    let (mut rx_call, mut rx_grid, mut tx_call, mut tx_grid) = (None, None, None, None);
    let (mut freq, mut time, mut mode, mut snr) = (None, None, None, None);
    for a in e.attributes().flatten() {
        let val = String::from_utf8_lossy(&a.value).to_string();
        match a.key.as_ref() {
            b"receiverCallsign" => rx_call = Some(val),
            b"receiverLocator" => rx_grid = Some(val),
            b"senderCallsign" => tx_call = Some(val),
            b"senderLocator" => tx_grid = Some(val),
            b"frequency" => freq = val.parse::<f64>().ok(),
            b"flowStartSeconds" => time = val.parse::<i64>().ok(),
            b"mode" => mode = Some(val),
            b"sNR" => snr = val.parse::<f32>().ok(),
            _ => {}
        }
    }
    let freq_mhz = freq? / 1_000_000.0;
    let band = Band::from_mhz(freq_mhz)?;
    Some(PathSpot {
        time: time?,
        tx_call: tx_call?,
        tx_grid: tx_grid.filter(|s| !s.is_empty()),
        rx_call: rx_call?,
        rx_grid: rx_grid.filter(|s| !s.is_empty()),
        band,
        mode,
        snr,
        // The HTTP reception reports DO carry the exact frequency (unlike the
        // band-level MQTT topics) — keep it so map click-to-work lands on the spot.
        freq_mhz: Some(freq_mhz),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parser handles the real PSK Reporter XML row shape.
    #[test]
    fn parses_reception_reports() {
        let xml = r#"<?xml version="1.0"?>
<receptionReports>
  <lastSequenceNumber value="1"/>
  <receptionReport receiverCallsign="VE2FVV" receiverLocator="FN59ue" senderCallsign="W8MSC" senderLocator="EN82el" frequency="14074698" flowStartSeconds="1780683960" mode="FT8" senderDXCC="United States" sNR="-18" />
  <receptionReport receiverCallsign="K4DOL" receiverLocator="FM05pd" senderCallsign="EA1AEC" senderLocator="IN52pn" frequency="50313000" flowStartSeconds="1780683959" mode="FT8" sNR="-21" />
</receptionReports>"#;
        let spots = parse_reports(xml);
        assert_eq!(spots.len(), 2);
        assert_eq!(spots[0].tx_call, "W8MSC");
        assert_eq!(spots[0].rx_call, "VE2FVV");
        assert_eq!(spots[0].band, Band::B20);
        assert_eq!(spots[0].snr, Some(-18.0));
        assert_eq!(spots[1].band, Band::B6);
    }

    /// A recorded answer (`callsign=W1AW`, 2026-11-12), and the next one, five minutes on,
    /// asked with `lastseqno` set to the first answer's last sequence number.
    const FIRST: &str = include_str!("../../tests/fixtures/pskreporter-w1aw-2026-11-12.xml");
    const NEXT: &str =
        include_str!("../../tests/fixtures/pskreporter-w1aw-lastseqno-2026-11-12.xml");
    /// When each was answered (its `maxFlowStartSeconds`, about the server's clock).
    const FIRST_AT: i64 = 1_794_515_117;
    const NEXT_AT: i64 = 1_794_515_473;
    const FIRST_SEQNO: i64 = 74_776_286_880;
    const VIEW: i64 = 1800;

    /// The last sequence number is the `value` of the answer's `<lastSequenceNumber>`; every
    /// row counts toward the limit.
    #[test]
    fn the_last_sequence_number_is_read_from_a_recorded_answer() {
        let r = parse_reply(FIRST);
        assert_eq!(r.last_seqno, Some(FIRST_SEQNO));
        assert_eq!(r.rows, FIRST.matches("<receptionReport ").count());
        assert!(!r.spots.is_empty());
        let n = parse_reply(NEXT);
        assert!(n.last_seqno.unwrap() > FIRST_SEQNO);
        // An answer with no `<lastSequenceNumber>` has none.
        assert_eq!(parse_reply("<receptionReports/>").last_seqno, None);
    }

    /// The first query of a run is the catch-up, with no cursor; once it is answered the next
    /// one carries `lastseqno`, and everything else Nexus sends is as before.
    #[test]
    fn the_query_carries_lastseqno_after_the_first_answer() {
        let mut feed = ReportFeed::new();
        let first = feed.next_query(FIRST_AT, VIEW, None, Some(FIRST_SEQNO - 5));
        assert_eq!(first.last_seqno, None, "the catch-up ignores an old cursor");
        let url = query_url("W1AW", &first);
        assert!(!url.contains("lastseqno"), "{url}");
        assert_eq!(
            url,
            format!(
                "https://retrieve.pskreporter.info/query?callsign=W1AW&flowStartSeconds=-86400&rronly=1&rptlimit=1000&appcontact={APPCONTACT}"
            )
        );

        let reply = parse_reply(FIRST);
        feed.accept(&reply, FIRST_AT, VIEW);
        let next = feed.next_query(NEXT_AT, VIEW, None, reply.last_seqno);
        assert_eq!(
            query_url("W1AW", &next),
            format!(
                "https://retrieve.pskreporter.info/query?callsign=W1AW&flowStartSeconds=-1800&rronly=1&lastseqno={FIRST_SEQNO}&rptlimit=1000&appcontact={APPCONTACT}"
            )
        );
    }

    /// The catch-up reaches back to the newest kept report: the whole day with none kept, three
    /// hours for a three-hour gap, capped at 24 hours for two days, never less than the view.
    #[test]
    fn the_launch_catch_up_window() {
        let now = FIRST_AT;
        let catch_up = |newest| ReportFeed::new().next_query(now, VIEW, newest, None);
        assert_eq!(catch_up(None).look_back_secs, 86_400);
        assert_eq!(catch_up(Some(now - 3 * 3600)).look_back_secs, 3 * 3600);
        assert_eq!(catch_up(Some(now - 2 * 86_400)).look_back_secs, 86_400);
        assert_eq!(catch_up(Some(now - 600)).look_back_secs, VIEW);
        assert_eq!(
            catch_up(Some(now + 600)).look_back_secs,
            VIEW,
            "a clock ahead"
        );
        // After the catch-up, a gap in polling is covered the same way, cursor and all.
        let mut feed = ReportFeed::new();
        feed.accept(&parse_reply(FIRST), now, VIEW);
        let q = feed.next_query(now + 3 * 3600, VIEW, None, Some(7));
        assert_eq!((q.look_back_secs, q.last_seqno), (3 * 3600, Some(7)));
    }

    /// Every query asks for the row limit, cursor or not.
    #[test]
    fn every_query_sends_rptlimit() {
        for last_seqno in [None, Some(FIRST_SEQNO)] {
            let url = query_url(
                "W1AW",
                &Query {
                    look_back_secs: VIEW,
                    last_seqno,
                },
            );
            assert_eq!(url.matches("&rptlimit=1000&").count(), 1, "{url}");
        }
    }

    /// An answer that reaches the limit is reported once a run, and the next query carries on
    /// from its sequence number over the usual window: no wider ask, no extra one.
    #[test]
    fn an_answer_at_the_limit_is_reported_once_and_the_next_poll_carries_on() {
        // RPTLIMIT rows: the recorded answer's first row, one second apart.
        let row = FIRST
            .lines()
            .find(|l| l.contains("<receptionReport "))
            .unwrap();
        let t = row.split("flowStartSeconds=\"").nth(1).unwrap();
        let t: i64 = t[..t.find('"').unwrap()].parse().unwrap();
        let mut xml = String::from("<receptionReports>\n<lastSequenceNumber value=\"900\"/>\n");
        for i in 0..RPTLIMIT as i64 {
            xml.push_str(&row.replace(
                &format!("flowStartSeconds=\"{t}\""),
                &format!("flowStartSeconds=\"{}\"", t - i),
            ));
            xml.push('\n');
        }
        xml.push_str("</receptionReports>");
        let full = parse_reply(&xml);
        assert_eq!(full.rows, RPTLIMIT);
        assert!(full.at_limit());
        assert!(!parse_reply(FIRST).at_limit());

        let mut feed = ReportFeed::new();
        assert!(feed.accept(&full, t, VIEW), "the first time is reported");
        let q = feed.next_query(t + 300, VIEW, None, full.last_seqno);
        assert_eq!((q.look_back_secs, q.last_seqno), (VIEW, Some(900)));
        assert!(!feed.accept(&full, t + 300, VIEW), "and only the first");
    }

    /// The nowcast reads the same: the view built from the catch-up and an incremental answer
    /// gives the snapshot a fresh half-hour query would have, over the same reports.
    #[test]
    fn the_incremental_view_gives_the_same_snapshot_as_a_fresh_window() {
        let (first, next) = (parse_reply(FIRST), parse_reply(NEXT));
        let mut feed = ReportFeed::new();
        feed.accept(&first, FIRST_AT, VIEW);
        feed.accept(&next, NEXT_AT, VIEW);

        // What one half-hour query at NEXT_AT returns: every report of the two answers made in
        // the last half hour, each once, in the server's order (not the feed's sort).
        let mut fresh: Vec<PathSpot> = next
            .spots
            .iter()
            .chain(&first.spots)
            .filter(|s| s.time >= NEXT_AT - VIEW)
            .cloned()
            .collect();
        let mut seen = std::collections::HashSet::new();
        fresh.retain(|s| seen.insert(report_key(s)));
        assert!(!fresh.is_empty());
        assert!(
            fresh.len() < first.spots.len() + next.spots.len(),
            "the fixtures exercise the window or the overlap"
        );

        let snap = |spots: &[PathSpot]| {
            let s = crate::engine::PropagationEngine::new("W1AW", "FN31pr").snapshot_measured(
                NEXT_AT,
                spots,
                &crate::model::SpaceWx::default(),
                true,
                &[],
                &crate::LogNeeds::new(),
            );
            serde_json::to_value(&s).unwrap()
        };
        assert_eq!(snap(feed.view()), snap(&fresh));
        let keys = |v: &[PathSpot]| {
            let mut k = v.iter().map(report_key).collect::<Vec<_>>();
            k.sort();
            k
        };
        assert_eq!(keys(feed.view()), keys(&fresh));
    }
}
