//! The contest strip's two aids, from the shell's side: the Super Check Partial download and the
//! operator's call-history file. Every rule is `tempo_app::scp`'s or `tempo_app::call_history`'s;
//! this file is the HTTP and the commands.
//!
//! Nothing here holds the Engine lock across I/O. The switches are read through `with_engine`,
//! and the lock is gone before a byte is fetched or read: the radio loop is the only unkey path,
//! and a download takes as long as the network does. Neither aid reaches Remote: these commands
//! are the desktop's alone, and the hosted page never asks for them.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use tauri::State;
use tempo_app::call_history::{self, HistoryMeta};
use tempo_app::scp::{self, Fetcher, Gate, Reply, ScpMeta};

use crate::SharedEngine;

/// Where both aids live: the shared data folder beside the logbook, so every radio window on this
/// PC (and a club's shared folder) reads one list.
fn dir() -> PathBuf {
    crate::shared_data_dir().join("contest-lists")
}

/// The site's rule: "Include a User-Agent header identifying your software and version".
fn user_agent() -> String {
    format!("Nexus/{}", env!("CARGO_PKG_VERSION"))
}

/// One check at a time: a second window's strip mounting mid-download must not start another.
static CHECKING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The real GET, for `scp::ensure`. It reads at most one byte past the list's size cap, so a
/// runaway reply costs no more memory than a list can.
struct Http(reqwest::blocking::Client);

impl Fetcher for Http {
    fn get(&self, url: &str, headers: &[(&'static str, String)]) -> Result<Reply, String> {
        let mut req = self.0.get(url);
        for (name, value) in headers {
            req = req.header(*name, value);
        }
        let resp = req.send().map_err(|e| e.to_string())?;
        let header = |name: reqwest::header::HeaderName| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let etag = header(reqwest::header::ETAG);
        let last_modified = header(reqwest::header::LAST_MODIFIED);
        let cache_control = header(reqwest::header::CACHE_CONTROL);
        let status = resp.status().as_u16();
        let mut body = Vec::new();
        resp.take(scp::MAX_BYTES as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|e| e.to_string())?;
        Ok(Reply {
            status,
            etag,
            last_modified,
            cache_control,
            body,
        })
    }
}

/// What is held, as the Settings line shows it: the record, with the count read off the file.
fn status_in(dir: &Path) -> ScpMeta {
    let mut meta = scp::read_meta(dir);
    meta.count = scp::load(dir).len();
    meta
}

/// Download the Super Check Partial list, or check for a newer one, when the site's rules allow
/// it now; `manual` is the operator's "Update now". The strip calls this when it mounts with SCP
/// on, which is what downloads the list the first time a contest starts.
#[tauri::command]
pub async fn scp_ensure(state: State<'_, SharedEngine>, manual: bool) -> Result<ScpMeta, String> {
    let (active, auto_update) = crate::with_engine(&state, |eng| {
        let s = eng.settings();
        (s.scp_active(), s.scp_auto_update)
    })
    .await?;
    let gate = Gate {
        active,
        auto_update,
        manual,
    };
    tauri::async_runtime::spawn_blocking(move || {
        let dir = dir();
        let _one = match CHECKING.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            // Another window is already asking: say what is held rather than ask twice.
            Err(std::sync::TryLockError::WouldBlock) => return Ok(status_in(&dir)),
        };
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(scp::ensure(
            &Http(client),
            &dir,
            gate,
            crate::now_unix(),
            &user_agent(),
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What is held of the list, asking the site nothing.
#[tauri::command(async)]
pub fn get_scp_status() -> ScpMeta {
    status_in(&dir())
}

/// The calls, once per strip mount. None while SCP is effectively off: the switch, or Unassisted.
#[tauri::command]
pub async fn get_scp_calls(state: State<'_, SharedEngine>) -> Result<Vec<String>, String> {
    if !crate::with_engine(&state, |eng| eng.settings().scp_active()).await? {
        return Ok(Vec::new());
    }
    tauri::async_runtime::spawn_blocking(|| scp::load(&dir()))
        .await
        .map_err(|e| e.to_string())
}

/// Import a call-history file the operator picked, for one contest (a `FieldDayStatus::event` id).
#[tauri::command(async)]
pub fn import_call_history(
    text: String,
    file_name: String,
    contest: String,
) -> Result<HistoryMeta, String> {
    call_history::import(&dir(), &text, &file_name, &contest, crate::now_unix())
}

/// What call history is imported, if any.
#[tauri::command(async)]
pub fn get_call_history_status() -> Option<HistoryMeta> {
    call_history::status(&dir())
}

/// The strip's view of the imported file: which contest it is for, its name, and per call the
/// values it holds. The strip decides what may go into a box; this only reads.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StripHistory {
    contest: String,
    file_name: String,
    entries: BTreeMap<String, BTreeMap<String, String>>,
}

/// The imported call history for the strip; none while it is effectively off.
#[tauri::command]
pub async fn get_call_history(
    state: State<'_, SharedEngine>,
) -> Result<Option<StripHistory>, String> {
    if !crate::with_engine(&state, |eng| eng.settings().call_history_active()).await? {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(|| {
        call_history::load(&dir()).map(|(meta, h)| StripHistory {
            contest: meta.contest,
            file_name: meta.file_name,
            entries: h.entries,
        })
    })
    .await
    .map_err(|e| e.to_string())
}

/// Forget the imported call history: the operator's Clear.
#[tauri::command(async)]
pub fn clear_call_history() -> Result<(), String> {
    call_history::clear(&dir()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const COMMANDS: [&str; 7] = [
        "scp_ensure",
        "get_scp_status",
        "get_scp_calls",
        "import_call_history",
        "get_call_history_status",
        "get_call_history",
        "clear_call_history",
    ];

    /// Each command the strip and Settings invoke is DEFINED here and REGISTERED in the handler
    /// list. A name missing from `generate_handler!` fails only at runtime, with nothing at
    /// compile time to catch it, so the list is read from the source.
    #[test]
    fn every_command_the_ui_invokes_is_defined_and_registered() {
        let lib = include_str!("lib.rs");
        let list = lib
            .split_once("tauri::generate_handler![")
            .expect("the handler list")
            .1
            .split_once("])")
            .expect("the end of the handler list")
            .0;
        let api = include_str!("../../ui/src/api.ts");
        let me = include_str!("contest_lists.rs");
        let registered = |name: &str| {
            list.lines()
                .any(|l| l.trim() == format!("contest_lists::{name},"))
        };
        for name in COMMANDS {
            assert!(
                api.contains(&format!("invoke('{name}'")),
                "api.ts never calls {name}"
            );
            assert!(
                me.contains(&format!("fn {name}(")),
                "{name} is not defined here"
            );
            assert!(
                registered(name),
                "{name} is not registered: the UI's call would fail"
            );
        }
        assert!(
            !registered("no_such_command"),
            "control: the reader is reading a real list"
        );
    }

    /// The real fetcher, against a one-shot server on this machine (the site is never contacted):
    /// it sends the headers it is given and hands back the validators and the body.
    #[test]
    fn the_shell_fetcher_sends_its_headers_and_reads_the_validators_back() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut req = Vec::new();
            let mut buf = [0u8; 4096];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = s.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
            }
            s.write_all(
                b"HTTP/1.1 200 OK\r\nETag: \"v9\"\r\nLast-Modified: Fri, 09 Oct 2026 00:03:42 GMT\r\n\
                  Cache-Control: public, max-age=172800\r\nContent-Length: 7\r\nConnection: close\r\n\r\n\
                  K9AAA\r\n",
            )
            .unwrap();
            String::from_utf8_lossy(&req).to_ascii_lowercase()
        });
        let http = Http(
            reqwest::blocking::Client::builder()
                .no_proxy()
                .build()
                .unwrap(),
        );
        let reply = http
            .get(
                &format!("http://{addr}/downloads/MASTER.SCP"),
                &[
                    ("User-Agent", "Nexus/9.9.9".into()),
                    ("If-None-Match", "\"v8\"".into()),
                ],
            )
            .unwrap();
        let req = server.join().unwrap();
        assert!(req.contains("user-agent: nexus/9.9.9"), "{req}");
        assert!(req.contains("if-none-match: \"v8\""), "{req}");
        assert_eq!(reply.status, 200);
        assert_eq!(reply.etag.as_deref(), Some("\"v9\""));
        assert_eq!(
            reply.last_modified.as_deref(),
            Some("Fri, 09 Oct 2026 00:03:42 GMT")
        );
        assert_eq!(
            reply.cache_control.as_deref(),
            Some("public, max-age=172800")
        );
        assert_eq!(reply.body, b"K9AAA\r\n");
    }
}
