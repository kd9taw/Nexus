//! Dev harness: serve the REAL built TV page (ui/dist) through the REAL FullConnect
//! route, with canned RPC answers — so a headless browser can render the page without
//! a rig or an engine. Not shipped; `cargo run -p tempo-app --example tv_harness`.
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tempo_app::connect_web::{ConnectBoardData, FullConnect, RpcOutcome};
use tempo_app::fd_scoreboard::{serve_until, BoardSource};

fn main() {
    let dist = std::env::args()
        .nth(1)
        .expect("usage: tv_harness <ui/dist> [port]");
    let port: u16 = std::env::args()
        .nth(2)
        .and_then(|p| p.parse().ok())
        .unwrap_or(7374);
    let dist2 = dist.clone();
    let source: Arc<dyn BoardSource> = Arc::new(FullConnect::new(
        || Some(ConnectBoardData::default()),
        Arc::new(move |path: &str| {
            let p = std::path::Path::new(&dist2).join(path);
            let bytes = std::fs::read(&p).ok()?;
            let mime = match p.extension().and_then(|e| e.to_str()) {
                Some("html") => "text/html; charset=utf-8",
                Some("js") => "text/javascript",
                Some("css") => "text/css",
                Some("json" | "geojson") => "application/json",
                Some("png") => "image/png",
                Some("webp") => "image/webp",
                Some("svg") => "image/svg+xml",
                _ => "application/octet-stream",
            };
            Some((bytes, mime.to_string()))
        }),
        Arc::new(|cmd: &str, _args: &str| match cmd {
            "tv_station" => RpcOutcome::Ok(r#"{"call":"KD9TAW","grid":"EN52"}"#.into()),
            "get_propagation" => RpcOutcome::Ok(
                std::fs::read_to_string("/tmp/tv-prop.json").unwrap_or_else(|_| "null".into()),
            ),
            "get_kp_forecast" => RpcOutcome::Ok(r#"{"points":[]}"#.into()),
            "get_kc2g_muf" => RpcOutcome::Ok("[]".into()),
            _ => RpcOutcome::Err("offline in the harness".into()),
        }),
    ));
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).expect("bind");
    eprintln!("serving {dist} on http://127.0.0.1:{port}/");
    serve_until(listener, source, Arc::new(AtomicBool::new(false)));
}
