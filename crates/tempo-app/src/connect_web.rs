//! Connect on the shack TV — the propagation picture served as a plain web page to
//! any browser on the local network.
//!
//! Same transport as the Field Day spectator scoreboard ([`crate::fd_scoreboard`]):
//! its hand-rolled GET/HEAD-only server over `std::net::TcpListener`, reached through
//! the same [`BoardSource`] trait, on its own base path and its own port. Nothing new
//! is added to the dependency tree and nothing here can reach an engine.
//!
//! **Read-only by construction.** The serve thread is handed a snapshot provider and
//! nothing else — there is no path from an inbound request to a setter, to CAT, or to
//! the transmit path. The page has no form, no script that posts, and no endpoint that
//! accepts anything but GET and HEAD.
//!
//! ⚠️ **THIS EXPOSES THE STATION ON THE LAN AND THE SETTING MUST SAY SO.** The Field
//! Day scoreboard is justified partly because a contest log is already broadcast in
//! clear on the air; that argument does NOT carry over here. This page names the
//! operator's callsign and grid square. It deliberately does NOT carry the current
//! dial frequency, the log, or the needs board: what the station is doing right this
//! second is a different thing from what the ionosphere is doing, and only the second
//! is what a wall display is for.
//!
//! No script loads from anywhere, no font, no image — a shack TV or a stick browser is
//! often on a network with no route to the internet at all, and a page that needs a CDN
//! would render bare exactly where it is meant to be used.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::fd_scoreboard::BoardSource;

/// The page. Self-contained: no external stylesheet, script, font or image.
const CONNECT_PAGE: &str = include_str!("../assets/connect_page.html");

/// How long a built payload serves before it is rebuilt. The page polls faster than
/// this; the TTL, not the poll, decides how often the snapshot lock is taken.
const CACHE_TTL: Duration = Duration::from_secs(5);

/// One band's line on the board.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectBand {
    pub band: String,
    /// Observed activity tier ("Hot" / "Active" / "Moderate" / "Quiet").
    pub tier: String,
    /// Modelled openness from physics, independent of what was heard.
    pub modeled: String,
    pub stations: u32,
    /// The one-clause plain reason the advisor gives.
    pub reason: String,
}

/// One live opening.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectOpening {
    pub band: String,
    pub mode: String,
    pub octant: String,
    pub stations: u32,
    pub confidence: String,
    pub is_new: bool,
}

/// Everything the page draws. Built from the propagation snapshot; carries no log,
/// no needs board and no dial frequency (see the module warning).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectBoardData {
    pub call: String,
    pub grid: String,
    /// The advisor's single prescriptive sentence.
    pub headline: String,
    pub banners: Vec<String>,
    pub bands: Vec<ConnectBand>,
    pub openings: Vec<ConnectOpening>,
    pub sfi: f32,
    pub kp: f32,
    pub a_index: f32,
    pub xray_class: String,
    /// Plain-language insight lines, already ranked.
    pub insights: Vec<String>,
    /// Worst planetary K still forecast, and when — `None` when NOAA has published
    /// nothing forward. Never defaulted to zero: a quiet sky and no data look
    /// identical on a display and only one of them is a forecast.
    pub kp_peak_ahead: Option<f32>,
    pub kp_peak_unix: Option<i64>,
    /// Snapshot provenance: "live" | "cached" | "partial" | "offline". Shown, always
    /// — a wall display that cannot say its data is stale is worse than a blank one.
    pub source: String,
    pub as_of_unix: i64,
}

/// Served when the page is asked for and there is no snapshot yet.
const INACTIVE_BODY: &str =
    "{\"active\":false,\"note\":\"No propagation snapshot yet — Nexus is still starting up.\"}";

struct Cache {
    built: Option<Instant>,
    body: String,
}

/// The standard source: caches a built payload for [`CACHE_TTL`] and stamps a
/// revision so the page can tell a real change from a repeat.
pub struct CachedConnect<F: Fn() -> Option<ConnectBoardData> + Send + Sync> {
    provider: F,
    cache: Mutex<Cache>,
    rev: AtomicU64,
    ttl: Duration,
}

impl<F: Fn() -> Option<ConnectBoardData> + Send + Sync> CachedConnect<F> {
    pub fn new(provider: F) -> Self {
        Self::with_ttl(provider, CACHE_TTL)
    }

    pub fn with_ttl(provider: F, ttl: Duration) -> Self {
        Self {
            provider,
            cache: Mutex::new(Cache {
                built: None,
                body: String::new(),
            }),
            rev: AtomicU64::new(0),
            ttl,
        }
    }

    fn fresh(&self) -> Option<String> {
        let g = self.cache.lock().ok()?;
        let built = g.built?;
        (built.elapsed() < self.ttl && !g.body.is_empty()).then(|| g.body.clone())
    }
}

impl<F: Fn() -> Option<ConnectBoardData> + Send + Sync> BoardSource for CachedConnect<F> {
    fn data(&self) -> String {
        if let Some(hit) = self.fresh() {
            return hit;
        }
        // Build OFF the cache lock: the provider takes the engine's snapshot lock,
        // and holding two locks across a network thread is how a serve loop wedges.
        let built = (self.provider)();
        let body = match built {
            Some(d) => {
                let rev = self.rev.fetch_add(1, Ordering::Relaxed) + 1;
                serde_json::json!({
                    "active": true,
                    "rev": rev,
                    "board": d,
                })
                .to_string()
            }
            None => INACTIVE_BODY.to_string(),
        };
        if let Ok(mut g) = self.cache.lock() {
            g.built = Some(Instant::now());
            g.body = body.clone();
        }
        body
    }

    fn meta(&self) -> String {
        serde_json::json!({ "kind": "connect", "version": 1 }).to_string()
    }

    fn page(&self) -> &'static str {
        CONNECT_PAGE
    }

    fn base(&self) -> &'static str {
        "connect"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> ConnectBoardData {
        ConnectBoardData {
            call: "KD9TAW".into(),
            grid: "EN52".into(),
            headline: "20 m is your best bet".into(),
            bands: vec![ConnectBand {
                band: "20m".into(),
                tier: "Active".into(),
                modeled: "Open".into(),
                stations: 12,
                reason: "12 stations".into(),
            }],
            source: "live".into(),
            as_of_unix: 1_780_000_000,
            ..Default::default()
        }
    }

    #[test]
    fn serves_the_board_and_marks_it_active() {
        let s = CachedConnect::new(|| Some(data()));
        let body = s.data();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["active"], true);
        assert_eq!(v["board"]["call"], "KD9TAW");
        assert_eq!(v["board"]["bands"][0]["band"], "20m");
        assert_eq!(v["rev"], 1);
    }

    /// ⚠️ THE PAGE MUST NOT CARRY WHAT THE STATION IS DOING. Propagation is public
    /// weather; the dial, the log and the needs board are not, and this page is served
    /// to anyone on the network. Pinned as a payload SHAPE test so a later field
    /// cannot be added without this failing.
    #[test]
    fn the_payload_carries_no_operating_state() {
        let s = CachedConnect::new(|| Some(data()));
        let v: serde_json::Value = serde_json::from_str(&s.data()).unwrap();
        let board = v["board"].as_object().expect("board is an object");
        for leaked in [
            "dialMhz",
            "freqMhz",
            "frequency",
            "log",
            "qsos",
            "needs",
            "needAlerts",
            "transmitting",
            "mode",
            "rigMode",
        ] {
            assert!(
                !board.contains_key(leaked),
                "the TV page is carrying `{leaked}` — that is what the station is doing, \
                 not what the ionosphere is doing"
            );
        }
    }

    /// No snapshot yet must read as "not ready", never as a quiet band plan.
    #[test]
    fn says_so_when_there_is_no_snapshot() {
        let s = CachedConnect::new(|| None);
        let v: serde_json::Value = serde_json::from_str(&s.data()).unwrap();
        assert_eq!(v["active"], false);
        assert!(v["note"].as_str().unwrap().contains("starting up"));
    }

    #[test]
    fn caches_within_the_ttl_and_rebuilds_after_it() {
        use std::sync::atomic::AtomicUsize;
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        CALLS.store(0, Ordering::Relaxed);
        let s = CachedConnect::with_ttl(
            || {
                CALLS.fetch_add(1, Ordering::Relaxed);
                Some(data())
            },
            Duration::from_millis(80),
        );
        s.data();
        s.data();
        s.data();
        assert_eq!(CALLS.load(Ordering::Relaxed), 1, "the TTL did not hold");
        std::thread::sleep(Duration::from_millis(120));
        s.data();
        assert_eq!(CALLS.load(Ordering::Relaxed), 2, "the TTL never expired");
    }

    /// The page is served where there may be no route to the internet at all, so it
    /// must not reference an external host. A CDN link would render this bare in
    /// exactly the place it is meant to be used.
    #[test]
    fn the_page_loads_nothing_from_the_network() {
        for probe in [
            "http://",
            "https://",
            "//cdn",
            "fonts.googleapis",
            "integrity=",
            "src=\"//",
        ] {
            assert!(
                !CONNECT_PAGE.contains(probe),
                "the TV page references `{probe}` — it must be wholly self-contained"
            );
        }
    }

    /// It answers on its own base path, so its URLs do not claim to be a scoreboard.
    #[test]
    fn answers_on_the_connect_base() {
        let s = CachedConnect::new(|| Some(data()));
        assert_eq!(s.base(), "connect");
    }

    /// END TO END OVER A REAL SOCKET. The unit tests above prove the payload; this
    /// proves the DELIVERY — that a browser typing the host and port actually gets the
    /// page, that the JSON the page fetches is really at the path the page asks for,
    /// and that the thing refuses to be written to. A feature reached by URL is not
    /// shipped until the endpoint answers.
    #[test]
    fn a_browser_on_the_lan_gets_the_page_and_the_data() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let source: Arc<dyn BoardSource> = Arc::new(CachedConnect::new(|| Some(data())));
        let sd = shutdown.clone();
        let server =
            std::thread::spawn(move || crate::fd_scoreboard::serve_until(listener, source, sd));

        let talk = |req: &str| -> String {
            let mut s = TcpStream::connect(addr).unwrap();
            s.write_all(req.as_bytes()).unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            out
        };

        // The bare host:port — what someone types into a TV.
        let root = talk("GET / HTTP/1.1\r\nHost: tv\r\n\r\n");
        assert!(
            root.starts_with("HTTP/1.1 200"),
            "GET / did not answer: {root:.60}"
        );
        assert!(
            root.contains("Nexus Connect"),
            "GET / served something else"
        );

        // The named path, and the JSON the page actually fetches.
        assert!(talk("GET /connect HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 200"));
        let json = talk("GET /connect/data.json HTTP/1.1\r\n\r\n");
        assert!(json.starts_with("HTTP/1.1 200"));
        assert!(json.contains("KD9TAW"), "the data endpoint served no board");

        // It is not a scoreboard, and it is not writable.
        assert!(talk("GET /scoreboard/data.json HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 404"));
        let post = talk("POST /connect/data.json HTTP/1.1\r\n\r\n");
        assert!(
            post.starts_with("HTTP/1.1 405"),
            "the LAN page accepted a POST — it must be read-only by construction"
        );

        shutdown.store(true, Ordering::Relaxed);
        let _ = server.join();
    }
}
