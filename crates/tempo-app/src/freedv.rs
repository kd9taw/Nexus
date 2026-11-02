//! FreeDV's own app beside Nexus — RECEIVE SIDE ONLY: its logged contacts land in the Nexus log,
//! and the calls it hears are listed for the Phone screen.
//!
//! Nexus does not encode or decode FreeDV, and nothing in this module transmits, keys, tunes or
//! answers. The operator runs FreeDV's free app beside Nexus (on the radio Nexus shares through
//! its CAT broker), and that app tells other programs two things over UDP
//! ([`tempo_net::freedv`] reads both):
//!
//! - **Its logged contacts**, WSJT-X protocol, QSO Logged (type 5) from id `FreeDV`. Nexus listens
//!   for them on `127.0.0.1:<freedv_log_port>` (2239 by default, ON by default): its own port,
//!   because 2237 is where Nexus SENDS its WSJT-X telemetry and where GridTracker and JTAlert
//!   listen. Bound to loopback, so nothing on the network can reach it. Each contact becomes one
//!   ADIF record ([`contact_adif`]) handed to the same import the companion mode's contacts use
//!   ([`crate::logwrite::import_logged_contact`]): the same duplicate check, worked-before marks
//!   and durable write. No new write path.
//! - **The calls it hears**, FreeDV 2.4's callsign broadcast (JSON to `224.0.0.1:7177` by
//!   default). OFF by default (`freedv_heard`); the Phone screen's FreeDV pane turns it on. Bound
//!   on every adapter because a group datagram arrives on the adapter it went out on, so the
//!   listener drops anything whose source is not this computer. The list is this session's only,
//!   capped at [`HEARD_CAP`], never stored.
//!
//! ⚠️ **A contact is logged under the station's own call or not at all.** FreeDV sends the call it
//! is set to as "my call"; one that is not Nexus's station call (case aside) is REFUSED with a
//! notice — on the screen and in the connection log — never dropped silently and never written
//! under the wrong call. FreeDV sends one instant as both the start and the end of a contact, so
//! the record carries that instant for both; nothing invents a start time.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tempo_net::freedv::{read_heard, read_log_datagram, HeardCall, LogIntake, MAX_DATAGRAM};
use tempo_net::wsjtx::LoggedQso;

use crate::engine::{engine_lock, Engine};

/// The port Nexus listens on for FreeDV's logged contacts, by default.
pub const DEFAULT_LOG_PORT: u16 = 2239;
/// The port of FreeDV's callsign broadcast, by default (FreeDV 2.4's own default).
pub const DEFAULT_HEARD_PORT: u16 = 7177;
/// How many heard calls the list keeps: the newest, one row per call.
pub const HEARD_CAP: usize = 50;

/// What the screens show of FreeDV: the two listeners, the last contact and the last refusal,
/// and the calls heard this session. In the snapshot as `freedv` while either listener is wanted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreedvDto {
    /// The port the contact listener is bound to on 127.0.0.1, while it is.
    pub log_port: Option<u16>,
    /// Why the contact listener is not listening, when its bind was refused.
    pub log_error: Option<String>,
    /// When FreeDV last sent anything on the contact port (Unix seconds): "FreeDV connected".
    pub alive_unix: Option<i64>,
    /// The last contact FreeDV logged, as the screen names it ("W5FDV · 14.236 MHz").
    pub last_contact: Option<String>,
    /// When it arrived (Unix seconds).
    pub last_contact_unix: Option<i64>,
    /// The last contact refused, and why, in a sentence. Cleared by the next contact imported.
    pub notice: Option<String>,
    /// The port the heard-call listener is bound to, while it is.
    pub heard_port: Option<u16>,
    /// Why the heard-call listener is not listening, when its bind was refused.
    pub heard_error: Option<String>,
    /// The calls FreeDV heard this session, newest first.
    pub heard: Vec<FreedvHeardDto>,
}

impl FreedvDto {
    /// Is there anything to show: a listener wanted, bound or refused.
    pub fn is_shown(&self) -> bool {
        self.log_port.is_some()
            || self.log_error.is_some()
            || self.heard_port.is_some()
            || self.heard_error.is_some()
    }

    /// One heard call, newest first, one row per call, capped.
    pub fn add_heard(&mut self, h: HeardCall, now_unix: i64) {
        self.heard.retain(|r| r.call != h.call);
        self.heard.insert(
            0,
            FreedvHeardDto {
                call: h.call,
                mode: h.mode,
                snr: h.snr,
                frequency_hz: h.frequency_hz,
                heard_unix: now_unix,
            },
        );
        self.heard.truncate(HEARD_CAP);
    }
}

/// One call FreeDV heard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreedvHeardDto {
    pub call: String,
    /// FreeDV's mode name (`RADEV1`); empty when FreeDV did not say.
    pub mode: String,
    pub snr: Option<i32>,
    pub frequency_hz: Option<u64>,
    /// When Nexus received it (Unix seconds).
    pub heard_unix: i64,
}

/// The ADIF record for a contact FreeDV logged, under `station_call` — or the sentence saying
/// why it is refused. Mode `FREEDV`, which the logbook writes `MODE=DIGITALVOICE SUBMODE=FREEDV`.
pub fn contact_adif(q: &LoggedQso, station_call: &str) -> Result<String, String> {
    let ours = station_call.trim().to_ascii_uppercase();
    let theirs = q.my_call.trim().to_ascii_uppercase();
    let call = q.dx_call.trim().to_ascii_uppercase();
    let shown = text(&call, 16);
    if ours.is_empty() {
        return Err(format!(
            "FreeDV logged {shown}, not imported: Nexus has no station call (Settings ▸ Station)"
        ));
    }
    if theirs.is_empty() {
        return Err(format!(
            "FreeDV logged {shown}, not imported: FreeDV sent no station call; set your call in FreeDV"
        ));
    }
    if theirs != ours {
        return Err(format!(
            "FreeDV logged {shown} as {}, not imported: this station is {ours}",
            text(&theirs, 16)
        ));
    }
    if !tempo_core::message::looks_like_call(&call) {
        return Err(format!(
            "FreeDV logged a contact with no valid call ({shown:?}), not imported"
        ));
    }
    let (Some(on), Some(off)) = (adif_time(q.time_on), adif_time(q.time_off)) else {
        return Err(format!(
            "FreeDV logged {call} with no usable time, not imported"
        ));
    };
    let mut out = String::new();
    let mut put = |k: &str, v: &str| {
        if !v.is_empty() {
            out.push_str(&format!("<{k}:{}>{v}", v.len()));
        }
    };
    put("CALL", &call);
    let grid = q.dx_grid.trim();
    if is_grid(grid) {
        put("GRIDSQUARE", grid);
    }
    put("MODE", "FREEDV");
    if q.tx_freq > 0 {
        let mhz = q.tx_freq as f64 / 1e6;
        if let Some(band) = crate::bandplan::band_for_dial(mhz) {
            put("BAND", band);
        }
        put("FREQ", &format!("{mhz:.6}"));
    }
    put("QSO_DATE", &on.0);
    put("TIME_ON", &on.1);
    put("QSO_DATE_OFF", &off.0);
    put("TIME_OFF", &off.1);
    put("RST_SENT", &text(&q.report_sent, 8));
    put("RST_RCVD", &text(&q.report_recvd, 8));
    put("NAME", &text(&q.name, 64));
    put("COMMENT", &text(&q.comments, 200));
    put("STATION_CALLSIGN", &ours);
    let my_grid = q.my_grid.trim();
    if is_grid(my_grid) {
        put("MY_GRIDSQUARE", my_grid);
    }
    out.push_str("<EOR>\n");
    Ok(out)
}

/// Untrusted text for one ADIF field: control characters and the tag brackets dropped (so no
/// text can open a field of its own), trimmed, at most `max` characters.
fn text(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control() && *c != '<' && *c != '>')
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

/// A Maidenhead locator of 4, 6 or 8 characters.
fn is_grid(g: &str) -> bool {
    let b = g.as_bytes();
    let pair = |i: usize, lo: u8, hi: u8| {
        b[i].to_ascii_uppercase() >= lo
            && b[i].to_ascii_uppercase() <= hi
            && b[i + 1].to_ascii_uppercase() >= lo
            && b[i + 1].to_ascii_uppercase() <= hi
    };
    matches!(b.len(), 4 | 6 | 8)
        && pair(0, b'A', b'R')
        && pair(2, b'0', b'9')
        && (b.len() < 6 || pair(4, b'A', b'X'))
        && (b.len() < 8 || pair(6, b'0', b'9'))
}

/// `(YYYYMMDD, HHMMSS)` UTC for a Unix time from 2000 to 2099 — anything else is not a time a
/// contact was made at.
fn adif_time(unix: i64) -> Option<(String, String)> {
    if !(946_684_800..4_102_444_800).contains(&unix) {
        return None;
    }
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    // Days since 1970-01-01 to a civil date (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    Some((
        format!("{y:04}{m:02}{d:02}"),
        format!(
            "{:02}{:02}{:02}",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        ),
    ))
}

/// Where a line for the operator goes besides the screen: the connection log, in the app.
pub type Note = Arc<dyn Fn(&'static str, String) + Send + Sync>;

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// One datagram from the contact port, acted on: a contact imported or refused, FreeDV seen.
/// What the listener does with each datagram, split out so a test drives it without a socket.
pub fn take_log_datagram(engine: &Mutex<Engine>, bytes: &[u8], note: &Note) {
    match read_log_datagram(bytes) {
        LogIntake::Ignored => {}
        LogIntake::Alive => engine_lock(engine).freedv_mut().alive_unix = Some(now_unix()),
        LogIntake::Contact(q) => {
            let station = engine_lock(engine).settings().mycall.clone();
            let now = now_unix();
            match contact_adif(&q, &station) {
                Err(why) => {
                    {
                        let mut e = engine_lock(engine);
                        let f = e.freedv_mut();
                        f.alive_unix = Some(now);
                        f.notice = Some(why.clone());
                    }
                    tempo_core::applog::warn("freedv", &why);
                    note("error", why);
                }
                Ok(adif) => {
                    let (added, _durable) = crate::logwrite::import_logged_contact(engine, &adif);
                    let mhz = q.tx_freq as f64 / 1e6;
                    let call = q.dx_call.trim().to_ascii_uppercase();
                    let named = if q.tx_freq > 0 {
                        format!("{call} · {mhz:.3} MHz")
                    } else {
                        call.clone()
                    };
                    let line = match &added {
                        Ok(0) => format!("FreeDV logged {named}: already in the log"),
                        Ok(_) => format!("FreeDV logged {named}: in the log"),
                        Err(e) => format!("FreeDV logged {named}, not imported: {e}"),
                    };
                    {
                        let mut e = engine_lock(engine);
                        let f = e.freedv_mut();
                        f.alive_unix = Some(now);
                        f.last_contact = Some(named);
                        f.last_contact_unix = Some(now);
                        f.notice = added.as_ref().err().map(|_| line.clone());
                    }
                    let level = if added.is_ok() { "info" } else { "error" };
                    if added.is_err() {
                        tempo_core::applog::error("freedv", &line);
                    }
                    note(level, line);
                }
            }
        }
    }
}

/// Serve the contact port until `shutdown`: each datagram from this computer, at most
/// [`MAX_DATAGRAM`] bytes, goes to [`take_log_datagram`]. A longer one is dropped whole.
pub fn serve_log_until(
    sock: UdpSocket,
    engine: Arc<Mutex<Engine>>,
    shutdown: Arc<AtomicBool>,
    note: Note,
) {
    serve_log_within(sock, engine, shutdown, note, MAX_DATAGRAM);
}

/// [`serve_log_until`] with its byte bound as a parameter. The buffer is one byte longer than
/// `bound`, so a datagram past the bound is told apart from one at it and dropped whole. A test
/// passes a bound under the loopback MTU: some hosts' loopback drops a UDP datagram of
/// [`MAX_DATAGRAM`] bytes before it arrives, which would prove nothing about this loop.
pub(crate) fn serve_log_within(
    sock: UdpSocket,
    engine: Arc<Mutex<Engine>>,
    shutdown: Arc<AtomicBool>,
    note: Note,
    bound: usize,
) {
    let _ = sock.set_read_timeout(Some(Duration::from_millis(250)));
    let mut buf = vec![0u8; bound + 1];
    while !shutdown.load(Ordering::Relaxed) {
        let Ok((n, from)) = sock.recv_from(&mut buf) else {
            continue;
        };
        // Bound to 127.0.0.1, so only this computer can reach it; said again here so a change to
        // the bind cannot quietly open it.
        if n > bound || !from.ip().is_loopback() {
            continue;
        }
        take_log_datagram(&engine, &buf[..n], &note);
    }
}

/// Serve the heard-call port until `shutdown`: each datagram from this computer read as one
/// heard call. The computer's own addresses are re-read at most every 10 s, when a datagram
/// comes from an address not yet known to be its own.
pub fn serve_heard_until(sock: UdpSocket, engine: Arc<Mutex<Engine>>, shutdown: Arc<AtomicBool>) {
    let _ = sock.set_read_timeout(Some(Duration::from_millis(250)));
    let mut buf = vec![0u8; MAX_DATAGRAM + 1];
    let own_now = || -> Vec<Ipv4Addr> {
        tempo_net::fdsync::lan_addrs()
            .into_iter()
            .map(|a| a.ip)
            .collect()
    };
    let mut own = own_now();
    let mut own_read = Instant::now();
    while !shutdown.load(Ordering::Relaxed) {
        let Ok((n, from)) = sock.recv_from(&mut buf) else {
            continue;
        };
        if n > MAX_DATAGRAM {
            continue;
        }
        if !tempo_net::freedv::from_this_computer(from.ip(), &own) {
            if own_read.elapsed() < Duration::from_secs(10) {
                continue;
            }
            own = own_now();
            own_read = Instant::now();
            if !tempo_net::freedv::from_this_computer(from.ip(), &own) {
                continue;
            }
        }
        if let Some(h) = read_heard(&buf[..n]) {
            engine_lock(&engine).freedv_mut().add_heard(h, now_unix());
        }
    }
}

/// Open the contact listener: UDP on 127.0.0.1 alone, never every adapter, so nothing on the
/// network can send it a contact.
pub fn bind_log(port: u16) -> std::io::Result<UdpSocket> {
    UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
}

/// A listener the manager runs: its port and the flag that stops it.
struct Running {
    port: u16,
    shutdown: Arc<AtomicBool>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

/// A bind refused on `port` at `at`: the same port is tried again after [`RETRY`], another port
/// at once (changing the port is the operator's fix for a conflict).
const RETRY: Duration = Duration::from_secs(30);

/// One listener's state in the manager: what runs, and the last refusal.
#[derive(Default)]
struct Slot {
    running: Option<Running>,
    failed: Option<(u16, Instant)>,
}

impl Slot {
    /// Bring the listener to `want` (a port, or none): stop it, start it, or leave it. `bind`
    /// opens the socket; `serve` runs it on a thread of its own. Answers the bound port or the
    /// refusal, when either changed.
    fn reconcile(
        &mut self,
        want: Option<u16>,
        bind: impl Fn(u16) -> std::io::Result<UdpSocket>,
        serve: impl FnOnce(UdpSocket, Arc<AtomicBool>),
    ) -> Option<Result<u16, (u16, std::io::Error)>> {
        let have = self.running.as_ref().map(|r| r.port);
        if want == have {
            return None;
        }
        self.running = None;
        let port = want?;
        if matches!(self.failed, Some((p, at)) if p == port && at.elapsed() < RETRY) {
            return None;
        }
        match bind(port) {
            Ok(sock) => {
                let shutdown = Arc::new(AtomicBool::new(false));
                serve(sock, Arc::clone(&shutdown));
                self.running = Some(Running { port, shutdown });
                self.failed = None;
                Some(Ok(port))
            }
            Err(e) => {
                let first = !matches!(self.failed, Some((p, _)) if p == port);
                self.failed = Some((port, Instant::now()));
                first.then_some(Err((port, e)))
            }
        }
    }
}

/// The sentence for a port that would not bind.
fn bind_message(what: &str, addr: &str, e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::AddrInUse {
        format!(
            "{what} cannot listen on {addr}: another program holds that port. Pick another port \
             in Settings ▸ Radio ▸ Transmit limits & sharing, and the same one in FreeDV"
        )
    } else {
        format!("{what} cannot listen on {addr}: {e}")
    }
}

/// Run both FreeDV listeners for as long as the app runs, following the settings once a
/// second: `freedv_log` / `freedv_log_port` for contacts, `freedv_heard` / `freedv_heard_port`
/// for heard calls. Port 0 is "not listening".
pub fn run(engine: Arc<Mutex<Engine>>, note: Note) {
    let mut log = Slot::default();
    let mut heard = Slot::default();
    loop {
        let (want_log, want_heard) = {
            let e = engine_lock(&engine);
            let s = e.settings();
            (
                s.freedv_log
                    .then_some(s.freedv_log_port)
                    .filter(|p| *p != 0),
                s.freedv_heard
                    .then_some(s.freedv_heard_port)
                    .filter(|p| *p != 0),
            )
        };
        let eng = Arc::clone(&engine);
        let n = Arc::clone(&note);
        let changed = log.reconcile(want_log, bind_log, move |sock, sd| {
            spawn("nexus-freedv-log", move || {
                serve_log_until(sock, eng, sd, n)
            })
        });
        {
            let mut e = engine_lock(&engine);
            let f = e.freedv_mut();
            f.log_port = log.running.as_ref().map(|r| r.port);
            if want_log.is_none() || f.log_port.is_some() {
                f.log_error = None;
            }
            if let Some(Err((port, err))) = &changed {
                let msg = bind_message("FreeDV contacts", &format!("127.0.0.1:{port}"), err);
                f.log_error = Some(msg.clone());
                tempo_core::applog::error("freedv", &msg);
                note("error", msg);
            }
        }
        if let Some(Ok(port)) = changed {
            note(
                "info",
                format!("listening for FreeDV's logged contacts on 127.0.0.1:{port}"),
            );
        }
        let eng = Arc::clone(&engine);
        let changed = heard.reconcile(
            want_heard,
            tempo_net::freedv::bind_heard,
            move |sock, sd| {
                spawn("nexus-freedv-heard", move || {
                    serve_heard_until(sock, eng, sd)
                })
            },
        );
        {
            let mut e = engine_lock(&engine);
            let f = e.freedv_mut();
            f.heard_port = heard.running.as_ref().map(|r| r.port);
            if want_heard.is_none() || f.heard_port.is_some() {
                f.heard_error = None;
            }
            if want_heard.is_none() {
                f.heard.clear();
            }
            if let Some(Err((port, err))) = &changed {
                let msg = bind_message("FreeDV heard calls", &format!("port {port}"), err);
                f.heard_error = Some(msg.clone());
                tempo_core::applog::error("freedv", &msg);
                note("error", msg);
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(f) {
        tempo_core::applog::error("freedv", &format!("{name} could not start: {e}"));
    }
}

#[cfg(test)]
mod tests;
