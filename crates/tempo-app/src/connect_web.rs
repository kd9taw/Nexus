//! Connect on the shack TV — the propagation picture served as a plain web page to
//! any browser on the local network.
//!
//! Same transport as the Field Day spectator scoreboard ([`crate::fd_scoreboard`]):
//! its hand-rolled GET/HEAD-only server over `std::net::TcpListener`, reached through
//! the same [`BoardSource`] trait, on its own base path and its own port. Nothing new
//! is added to the dependency tree and nothing here can reach an engine.
//!
//! **Read-only by construction.** The transport accepts nothing but GET and HEAD, and
//! what those can reach is the summary snapshot, the app's own bundled UI files, and
//! the RPC gated by [`RPC_ALLOWLIST`] — a hand-written list of read-only
//! public-weather commands, enforced here (tested with a dispatcher that panics if a
//! non-allowlisted name reaches it) and re-checked in src-tauri's dispatcher, whose
//! arms are each hand-written: there is no generic invoke bridge for the list to
//! accidentally widen into. No path from an inbound request reaches a setter, CAT, or
//! the transmit path.
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

// ---------------------------------------------------------------------------
// The FULL Connect page — the app's own UI served to a browser
// ---------------------------------------------------------------------------

/// Commands a LAN browser may invoke. **THIS LIST IS THE SECURITY BOUNDARY** for the
/// full page, and it is enforced in [`FullConnect::extra`] — in this crate, where a
/// unit test can drive the route with a fake dispatcher and prove a name outside the
/// list never reaches it. The dispatcher in src-tauri re-checks as defence in depth.
///
/// ⚠️ RULES FOR ADDING A NAME, all three, no exceptions:
///   1. Read-only: the command must not mutate engine state, settings, or credentials,
///      must not touch CAT, and must have no path to the transmit gate.
///   2. No operating state: nothing that carries the log, the needs board, the dial
///      frequency, transmit state, or any credential — the module header's threat
///      model, and `no_sensitive_command_is_allowlisted` pins the known names.
///   3. Public-weather data only: propagation, space weather, satellites, parks,
///      contests — the picture of the IONOSPHERE, not of the STATION.
pub const RPC_ALLOWLIST: &[&str] = &[
    "get_propagation",     // the nowcast: advisory, openings, spots, space wx, insights
    "get_kc2g_muf",        // ionosonde MUF stations (map overlay)
    "get_space_wx_scales", // NOAA R/S/G scales + alerts
    "get_xray_now",        // the 60 s flare fast lane (map D-RAP layer)
    "get_aurora",          // OVATION oval (map layer)
    "get_pca",             // polar-cap absorption (map layer)
    "get_satellites",      // satellite positions (map layer)
    "get_ota_map_spots",   // POTA activators (map layer)
    "get_kp_forecast",     // the three-day outlook pane
    "get_band_outlook",    // per-band outlook pane
    "get_path_outlook",    // outlook for a clicked spot
    "get_getting_out",     // the getting-out pane
    "get_dxped_windows",   // DXpedition windows pane
    "get_openings_log",    // the openings history pane
    "get_declination",     // magnetic declination for map bearings
    "tv_station",          // callsign + grid ONLY (src-tauri builds it by hand)
];

/// What the src-tauri dispatcher returns for one RPC. `NotAllowed` exists so the
/// defence-in-depth check over there is distinguishable from a command that ran and
/// failed — the route turns both into errors, but differently (404 vs 500).
pub enum RpcOutcome {
    Ok(String),
    NotAllowed,
    Err(String),
}

/// Serves the app's own bundled UI plus the read-only RPC, wrapping a
/// [`CachedConnect`] so `/connect/data.json` keeps answering (the simple summary
/// page's feed, and the payload-shape test that guards it).
///
/// Both capabilities arrive as closures so this crate never depends on tauri: the
/// asset closure wraps the embedded-asset resolver over in src-tauri, and the RPC
/// closure wraps the hand-written command dispatcher there. **The allowlist check
/// happens HERE, before the dispatcher is ever called** — a browser asking for a
/// name outside [`RPC_ALLOWLIST`] gets a 404 from a route that provably (by test)
/// never invoked anything.
/// `path` (no leading slash) → (bytes, mime). `None` = no such asset.
pub type AssetFn = std::sync::Arc<dyn Fn(&str) -> Option<(Vec<u8>, String)> + Send + Sync>;
/// `(command, args-json)` → outcome. Only ever called with allowlisted names.
pub type RpcFn = std::sync::Arc<dyn Fn(&str, &str) -> RpcOutcome + Send + Sync>;

pub struct FullConnect<F: Fn() -> Option<ConnectBoardData> + Send + Sync> {
    inner: CachedConnect<F>,
    assets: AssetFn,
    rpc: RpcFn,
}

impl<F: Fn() -> Option<ConnectBoardData> + Send + Sync> FullConnect<F> {
    pub fn new(provider: F, assets: AssetFn, rpc: RpcFn) -> Self {
        Self {
            inner: CachedConnect::new(provider),
            assets,
            rpc,
        }
    }
}

/// Minimal percent-decoding for the `args` query value. Only what an encoded JSON
/// object needs; a malformed escape decays to the raw text, which then fails JSON
/// parsing in the dispatcher loudly rather than half-decoding silently.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

use crate::fd_scoreboard::Response;

fn json_response(status: u16, reason: &'static str, body: String) -> Response {
    Response {
        status,
        reason,
        content_type: "application/json".into(),
        body: body.into_bytes(),
        allow: None,
    }
}

impl<F: Fn() -> Option<ConnectBoardData> + Send + Sync> BoardSource for FullConnect<F> {
    fn data(&self) -> String {
        self.inner.data()
    }
    fn meta(&self) -> String {
        self.inner.meta()
    }
    fn page(&self) -> &'static str {
        // Fallback only: when the asset closure cannot produce the TV entry (a unit
        // test, or a build with no embedded frontend) the simple summary still serves,
        // so the URL never goes dark.
        CONNECT_PAGE
    }
    fn base(&self) -> &'static str {
        "connect"
    }

    /// The full page's routes. Reached only for GET/HEAD — the transport 405s
    /// everything else before consulting this — with the RAW path (query intact).
    fn extra(&self, raw_path: &str) -> Option<Response> {
        let (path, query) = match raw_path.split_once('?') {
            Some((p, q)) => (p, q),
            None => (raw_path, ""),
        };

        // --- the read-only RPC ---
        if let Some(cmd) = path.strip_prefix("/connect/rpc/") {
            // ⚠️ THE GATE. A name outside the allowlist 404s here, and the test drives
            // this route with a dispatcher that panics if called — so "never invoked"
            // is proven, not asserted.
            if !RPC_ALLOWLIST.contains(&cmd) {
                return Some(json_response(
                    404,
                    "Not Found",
                    format!("{{\"error\":\"'{}' is not served to the network\"}}", cmd),
                ));
            }
            let args = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("args="))
                .map(percent_decode)
                .unwrap_or_else(|| "null".to_string());
            return Some(match (self.rpc)(cmd, &args) {
                RpcOutcome::Ok(body) => json_response(200, "OK", body),
                RpcOutcome::NotAllowed => json_response(
                    404,
                    "Not Found",
                    "{\"error\":\"refused by the dispatcher\"}".into(),
                ),
                RpcOutcome::Err(e) => json_response(
                    500,
                    "Internal Server Error",
                    serde_json::json!({ "error": e }).to_string(),
                ),
            });
        }

        // --- the TV entry and the bundled assets ---
        let trimmed = path.trim_end_matches('/');
        if trimmed.is_empty() || trimmed == "/connect" {
            // The full page when the build carries it; None falls through to the
            // simple summary via the standard route.
            return (self.assets)("connect-tv.html").map(|(bytes, mime)| Response {
                status: 200,
                reason: "OK",
                content_type: mime,
                body: bytes,
                allow: None,
            });
        }
        // Keep the summary feed's paths on the standard route.
        if trimmed == "/connect/data.json" || trimmed == "/connect/meta.json" {
            return None;
        }
        // A browser that loaded the page at `/connect/` (trailing slash) resolves the
        // page's relative asset URLs under `/connect/…` — same files, one directory
        // deeper. Serve both spellings rather than telling that browser 404.
        let rel = trimmed.trim_start_matches('/');
        let rel = rel.strip_prefix("connect/").unwrap_or(rel);
        (self.assets)(rel).map(|(bytes, mime)| Response {
            status: 200,
            reason: "OK",
            content_type: mime,
            body: bytes,
            allow: None,
        })
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

    // ---- the full page ----

    use std::sync::Arc;

    fn full(
        assets: impl Fn(&str) -> Option<(Vec<u8>, String)> + Send + Sync + 'static,
        rpc: impl Fn(&str, &str) -> RpcOutcome + Send + Sync + 'static,
    ) -> FullConnect<impl Fn() -> Option<ConnectBoardData> + Send + Sync> {
        FullConnect::new(|| Some(data()), Arc::new(assets), Arc::new(rpc))
    }

    /// ⚠️ THE ALLOWLIST IS THE SECURITY BOUNDARY, so this drives the route with a
    /// dispatcher that PANICS if invoked: "a non-allowlisted name never reaches the
    /// dispatcher" is proven by execution, not asserted by reading the code.
    #[test]
    fn a_command_outside_the_allowlist_never_reaches_the_dispatcher() {
        let s = full(
            |_| None,
            |cmd, _| panic!("dispatcher invoked for non-allowlisted '{cmd}'"),
        );
        for probe in [
            "/connect/rpc/get_snapshot",
            "/connect/rpc/get_settings",
            "/connect/rpc/set_frequency",
            "/connect/rpc/get_log",
            "/connect/rpc/halt_tx",
            "/connect/rpc/",
        ] {
            let r = s.extra(probe).expect("the rpc route must answer");
            assert_eq!(r.status, 404, "{probe} did not 404");
        }
    }

    /// The positive control for the test above: an allowlisted name DOES reach the
    /// dispatcher, with its query args decoded.
    #[test]
    fn an_allowlisted_command_reaches_the_dispatcher_with_its_args() {
        let s = full(
            |_| None,
            |cmd, args| {
                assert_eq!(cmd, "get_path_outlook");
                assert_eq!(args, r#"{"call":"KD9TAW"}"#);
                RpcOutcome::Ok(r#"{"ok":true}"#.into())
            },
        );
        let r = s
            .extra("/connect/rpc/get_path_outlook?args=%7B%22call%22%3A%22KD9TAW%22%7D")
            .expect("route must answer");
        assert_eq!(r.status, 200);
        assert_eq!(String::from_utf8_lossy(&r.body), r#"{"ok":true}"#);
    }

    /// ⚠️ THE NAMES THAT MUST NEVER APPEAR. Every command that carries the log, the
    /// needs board, settings, credentials, the dial, or any write. If one of these is
    /// ever added to RPC_ALLOWLIST this fails before the change ships.
    #[test]
    fn no_sensitive_command_is_allowlisted() {
        const FORBIDDEN: &[&str] = &[
            "get_snapshot",    // dial frequency, transmit state, the roster
            "get_settings",    // ports, hosts, every knob
            "get_log",         // the log
            "get_log_stats",   // the log, aggregated
            "get_need_alerts", // the needs board
            "get_credentials_status",
            "set_frequency",
            "set_tx_enabled",
            "halt_tx",
        ];
        for f in FORBIDDEN {
            assert!(
                !RPC_ALLOWLIST.contains(f),
                "'{f}' is on the LAN allowlist — that is operating state or a write"
            );
        }
        // Controls: the list is real and carries what the page needs.
        assert!(RPC_ALLOWLIST.contains(&"get_propagation"));
        assert!(RPC_ALLOWLIST.len() >= 10);
        // And every name is read-shaped: no set_/clear_/start_/stop_ verbs.
        for name in RPC_ALLOWLIST {
            assert!(
                !name.starts_with("set_")
                    && !name.starts_with("clear_")
                    && !name.starts_with("start_")
                    && !name.starts_with("stop_"),
                "'{name}' is verb-shaped like a write"
            );
        }
    }

    /// The TV entry serves at `/` when the build carries it, and the simple summary
    /// keeps serving when it does not — the URL never goes dark.
    #[test]
    fn the_tv_entry_serves_at_root_with_a_summary_fallback() {
        let with = full(
            |p| {
                (p == "connect-tv.html")
                    .then(|| (b"<title>tv</title>".to_vec(), "text/html".to_string()))
            },
            |_, _| RpcOutcome::Err("unused".into()),
        );
        let r = with.extra("/").expect("root must serve the TV entry");
        assert_eq!(r.status, 200);
        assert_eq!(String::from_utf8_lossy(&r.body), "<title>tv</title>");

        let without = full(|_| None, |_, _| RpcOutcome::Err("unused".into()));
        assert!(
            without.extra("/").is_none(),
            "with no asset the hook must fall through to the summary page"
        );
        // …and the standard route serves the summary there (proven in the socket test).
    }

    /// The summary feed's own paths stay on the standard route even when an asset
    /// closure exists — data.json is pinned by the payload-shape test and must not be
    /// shadowed by a bundled file.
    #[test]
    fn the_summary_feed_is_never_shadowed() {
        let s = full(
            |_| Some((b"shadow".to_vec(), "text/plain".to_string())),
            |_, _| RpcOutcome::Err("unused".into()),
        );
        assert!(s.extra("/connect/data.json").is_none());
        assert!(s.extra("/connect/meta.json").is_none());
    }

    /// It answers on its own base path, so its URLs do not claim to be a scoreboard.
    #[test]
    fn answers_on_the_connect_base() {
        let s = CachedConnect::new(|| Some(data()));
        assert_eq!(s.base(), "connect");
    }

    /// THE FULL PAGE, END TO END OVER A REAL SOCKET: the TV entry at `/`, its assets
    /// at both path spellings, the RPC answering an allowlisted read, refusing an
    /// operating-state command, and still refusing every write by method.
    #[test]
    fn the_full_page_serves_and_stays_read_only_over_a_real_socket() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let source: Arc<dyn BoardSource> = Arc::new(FullConnect::new(
            || Some(data()),
            Arc::new(|p: &str| match p {
                "connect-tv.html" => {
                    Some((b"<title>Nexus Connect</title>".to_vec(), "text/html".into()))
                }
                "assets/tv-abc.js" => Some((b"console.log(1)".to_vec(), "text/javascript".into())),
                _ => None,
            }),
            Arc::new(|cmd: &str, _args: &str| match cmd {
                "get_kp_forecast" => RpcOutcome::Ok(r#"{"points":[]}"#.into()),
                other => panic!("dispatcher reached with '{other}'"),
            }),
        ));
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

        // The TV entry at the bare host:port — what someone types into a browser.
        let root = talk("GET / HTTP/1.1\r\nHost: tv\r\n\r\n");
        assert!(root.starts_with("HTTP/1.1 200"), "root: {root:.60}");
        assert!(root.contains("Nexus Connect"));

        // Assets, at both spellings a browser can produce.
        assert!(talk("GET /assets/tv-abc.js HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 200"));
        assert!(talk("GET /connect/assets/tv-abc.js HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 200"));

        // The RPC: an allowlisted read answers; operating state 404s; a write 405s.
        let rpc = talk("GET /connect/rpc/get_kp_forecast HTTP/1.1\r\n\r\n");
        assert!(rpc.starts_with("HTTP/1.1 200"), "rpc: {rpc:.60}");
        assert!(rpc.contains(r#"{"points":[]}"#));
        assert!(talk("GET /connect/rpc/get_snapshot HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 404"));
        assert!(
            talk("POST /connect/rpc/get_kp_forecast HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 405")
        );

        // The summary feed still answers beside the full page.
        assert!(talk("GET /connect/data.json HTTP/1.1\r\n\r\n").contains("KD9TAW"));

        shutdown.store(true, Ordering::Relaxed);
        let _ = server.join();
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
