//! "Accept contacts other programs log": the contact port, grown from FreeDV's into the one
//! place another program can log a contact into the Nexus log — RECEIVE ONLY.
//!
//! An operator who keeps the Nexus log while another program holds the radio (the S.A.T.
//! satellite tracker owns the IC-9700's CI-V port; WSJT-X, JTDX or N1MM run a session of their
//! own) points that program's "send logged contacts" at Nexus's contact port: UDP
//! `127.0.0.1:<freedv_log_port>`, 2239 by default and ON by default, the port FreeDV's contacts
//! have always come to. FreeDV's contacts are read exactly as before ([`crate::freedv`]); every
//! other datagram is read by [`tempo_net::log_intake`] (WSJT-X type 5 and type 12 from any id,
//! raw ADIF text, N1MM `<contactinfo>`), checked here ([`contact_adif`]) and written by the
//! import ([`crate::logwrite::intake_contact`]): Nexus's own FT exchange is never touched.
//!
//! **Who may send** ([`Admission`]). This computer only, by default: bound to 127.0.0.1. With
//! "Accept from devices on my network" on (`log_intake_lan`, OFF by default) the port is bound on
//! every adapter, and EVERY datagram is still judged by its source before anything reads it:
//! this computer, or a private address (RFC 1918, link-local, IPv6 unique-local) and, when one is
//! set (`log_intake_peer`), that address alone. A public source is never read.
//!
//! **What it can do.** Add a contact to the log, no more: nothing here keys, tunes, replies,
//! changes a setting or names a file. A contact must be under the station's own call (one naming
//! another is refused with a sentence, never written under the wrong call), carry a call that
//! looks like one and a time from 2000 to 2099. It carries the program it came from as
//! [`VIA_FIELD`], kept in the record and shown in its detail. Added, it is queued for the service
//! uploads like the operator's own (held while contest mode is on) and never sent to the local
//! loggers, so nothing echoes back.
//!
//! **Bounds**, because this is outside data on a network port: a datagram past
//! [`tempo_net::log_intake::MAX_DATAGRAM`] is dropped whole; the reader's own caps on records,
//! fields and values; [`RateLimit`] per source and in total. A refusal of a source, a rate or a
//! size is said once in the connection log when it starts, not once per datagram.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempo_core::logbook::QsoRecord;
use tempo_net::log_intake::{read_datagram, Fields, Intake, MAX_RECORDS};

use crate::engine::{engine_lock, Engine};
use crate::freedv::Note;
use crate::logwrite::IntakeOutcome;
use crate::settings::Settings;

/// The app-defined ADIF field naming the program a contact came in from ("WSJT-X", "N1MM", the
/// S.A.T.'s address). In the record's passthrough fields, so it is written to `log.adi` and read
/// back with it; shown in the contact's detail as "via <program>".
pub const VIA_FIELD: &str = "APP_NEXUS_VIA";

/// Whether `rec` came in from another program ([`VIA_FIELD`]): the upload worker sends such a
/// contact to the services and not to DXKeeper, which may be the program that sent it.
pub fn from_another_program(rec: &QsoRecord) -> bool {
    rec.extra.iter().any(|(k, _)| k == VIA_FIELD)
}

fn field<'a>(f: &'a Fields, k: &str) -> Option<&'a str> {
    f.iter()
        .find(|(n, _)| n == k)
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

/// What one contact from another program comes to: the ADIF record for the import, the call, and
/// its frequency in MHz when it carried one.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub adif: String,
    pub call: String,
    pub mhz: Option<f64>,
}

/// The ADIF record for one contact `program` logged, under `station_call` — or the sentence
/// saying why it is refused. A record naming a station call that is not ours is refused; one
/// naming none takes ours. The band comes from the frequency when the record names none. The
/// program goes in as [`VIA_FIELD`].
pub fn contact_adif(f: &Fields, program: &str, station_call: &str) -> Result<Checked, String> {
    let ours = station_call.trim().to_ascii_uppercase();
    let call = field(f, "CALL").unwrap_or("").to_ascii_uppercase();
    let shown: String = call.chars().take(16).collect();
    if ours.is_empty() {
        return Err(format!(
            "{program} logged {shown}, not imported: Nexus has no station call (Settings ▸ Station)"
        ));
    }
    if !tempo_core::message::looks_like_call(&call) {
        return Err(format!(
            "{program} logged a contact with no valid call ({shown:?}), not imported"
        ));
    }
    if let Some(theirs) = field(f, "STATION_CALLSIGN") {
        let theirs = theirs.to_ascii_uppercase();
        if theirs != ours {
            let theirs: String = theirs.chars().take(16).collect();
            return Err(format!(
                "{program} logged {call} as {theirs}, not imported: this station is {ours}"
            ));
        }
    }
    let usable_time = field(f, "QSO_DATE")
        .zip(field(f, "TIME_ON"))
        .is_some_and(|(d, t)| is_date(d) && is_time(t));
    if !usable_time {
        return Err(format!(
            "{program} logged {call} with no usable time, not imported"
        ));
    }
    let mhz = |k: &str| {
        field(f, k)
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|m| m.is_finite() && *m > 0.0 && *m < 300_000.0)
    };
    let (freq, freq_rx) = (mhz("FREQ"), mhz("FREQ_RX"));
    let mut out = String::new();
    let mut put = |k: &str, v: &str| {
        if !v.is_empty() {
            out.push_str(&format!("<{k}:{}>{v}", v.len()));
        }
    };
    for (k, v) in f {
        match k.as_str() {
            "CALL" => put(k, &call),
            "STATION_CALLSIGN" => {}
            "FREQ" if freq.is_none() => {}
            "FREQ_RX" if freq_rx.is_none() => {}
            "GRIDSQUARE" | "MY_GRIDSQUARE" if !is_grid(v) => {}
            _ => put(k, v),
        }
    }
    if field(f, "BAND").is_none() {
        if let Some(band) = freq.and_then(crate::bandplan::band_for_dial) {
            put("BAND", band);
        }
    }
    if field(f, "BAND_RX").is_none() {
        if let Some(band) = freq_rx.and_then(crate::bandplan::band_for_dial) {
            put("BAND_RX", band);
        }
    }
    put("STATION_CALLSIGN", &ours);
    put(VIA_FIELD, program);
    out.push_str("<EOR>\n");
    Ok(Checked {
        adif: out,
        call,
        mhz: freq,
    })
}

/// `YYYYMMDD`, a day that exists, from 2000 to 2099: never one the calendar would roll over
/// (`20260231` is no date, not 3 March).
fn is_date(d: &str) -> bool {
    let n = |r: std::ops::Range<usize>| d.get(r).and_then(|s| s.parse::<u32>().ok());
    if d.len() != 8 || !d.bytes().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let (Some(y), Some(m), Some(day)) = (n(0..4), n(4..6), n(6..8)) else {
        return false;
    };
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (2000..2100).contains(&y) && (1..=days).contains(&day)
}

/// `HHMM` or `HHMMSS` on a 24-hour clock.
fn is_time(t: &str) -> bool {
    let n = |r: std::ops::Range<usize>| t.get(r).and_then(|s| s.parse::<u32>().ok());
    matches!(t.len(), 4 | 6)
        && t.bytes().all(|c| c.is_ascii_digit())
        && n(0..2).is_some_and(|h| h < 24)
        && n(2..4).is_some_and(|m| m < 60)
        && (t.len() == 4 || n(4..6).is_some_and(|s| s < 60))
}

/// A Maidenhead locator of 4, 6 or 8 characters.
fn is_grid(g: &str) -> bool {
    let b = g.as_bytes();
    let pair = |i: usize, lo: u8, hi: u8| {
        (lo..=hi).contains(&b[i].to_ascii_uppercase())
            && (lo..=hi).contains(&b[i + 1].to_ascii_uppercase())
    };
    matches!(b.len(), 4 | 6 | 8)
        && pair(0, b'A', b'R')
        && pair(2, b'0', b'9')
        && (b.len() < 6 || pair(4, b'A', b'X'))
        && (b.len() < 8 || pair(6, b'0', b'9'))
}

// --- Who may send --------------------------------------------------------------------------------

/// Who the contact port takes a datagram from, as the settings say ([`Admission::of`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admission {
    /// "Accept from devices on my network" (`log_intake_lan`).
    pub lan: bool,
    /// The one network address allowed (`log_intake_peer`), when one is set.
    pub pin: Option<IpAddr>,
    /// A pinned address was set but is not an address: no network device is taken.
    pub pin_unreadable: bool,
}

impl Admission {
    pub fn of(s: &Settings) -> Self {
        let peer = s.log_intake_peer.trim();
        let pin = peer.parse::<IpAddr>().ok();
        Self {
            lan: s.log_intake_lan,
            pin,
            pin_unreadable: !peer.is_empty() && pin.is_none(),
        }
    }

    /// `Ok` when a datagram from `ip` may be read; the reason, in a sentence's words, when not.
    pub fn admits(&self, ip: IpAddr) -> Result<(), &'static str> {
        let ip = canonical(ip);
        if ip.is_loopback() {
            return Ok(());
        }
        if !self.lan {
            return Err(
                "it is not from this computer, and \"Accept from devices on my network\" is off",
            );
        }
        if !is_private(ip) {
            return Err("it is not from a private network address");
        }
        if self.pin_unreadable {
            return Err("the network address set to send contacts is not an address");
        }
        match self.pin {
            Some(pin) if canonical(pin) != ip => {
                Err("it is not from the network address set to send contacts")
            }
            _ => Ok(()),
        }
    }
}

/// An IPv4 address carried in IPv6 (`::ffff:a.b.c.d`) as the IPv4 address it is.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    }
}

/// A private address: RFC 1918 and link-local IPv4, unique-local (`fc00::/7`) and link-local
/// (`fe80::/10`) IPv6. Never a public, multicast, broadcast or unspecified one.
pub fn is_private(ip: IpAddr) -> bool {
    match canonical(ip) {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

// --- How much ------------------------------------------------------------------------------------

/// A token bucket: `burst` contacts at once, refilled at `per_minute`.
#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    at: Instant,
}

impl Bucket {
    fn full(burst: f64, now: Instant) -> Self {
        Self {
            tokens: burst,
            at: now,
        }
    }

    fn refill(&mut self, burst: f64, per_minute: f64, now: Instant) {
        let secs = now.saturating_duration_since(self.at).as_secs_f64();
        self.tokens = (self.tokens + secs * per_minute / 60.0).min(burst);
        self.at = now;
    }
}

/// How many contacts one source may send at once, and then a minute. A logging program sends one
/// per contact, so even a contest's best hour (a few hundred) stays well inside.
pub const SOURCE_BURST: f64 = 30.0;
pub const SOURCE_PER_MINUTE: f64 = 10.0;
/// …and every source together.
pub const TOTAL_BURST: f64 = 60.0;
pub const TOTAL_PER_MINUTE: f64 = 30.0;
/// The most sources the limiter remembers; past it, a new source counts against the total only.
const SOURCES_KEPT: usize = 64;

/// The contact port's rate limit: per source and in total ([`SOURCE_BURST`], [`TOTAL_BURST`]).
#[derive(Debug)]
pub struct RateLimit {
    sources: HashMap<IpAddr, Bucket>,
    total: Bucket,
}

impl RateLimit {
    pub fn new(now: Instant) -> Self {
        Self {
            sources: HashMap::new(),
            total: Bucket::full(TOTAL_BURST, now),
        }
    }

    /// Spend one contact from `ip` at `now`: `true` when it may be taken.
    pub fn take(&mut self, ip: IpAddr, now: Instant) -> bool {
        self.total.refill(TOTAL_BURST, TOTAL_PER_MINUTE, now);
        if self.sources.len() >= SOURCES_KEPT && !self.sources.contains_key(&ip) {
            // Forget the sources that are quiet again (their bucket back to full).
            self.sources.retain(|_, b| {
                b.refill(SOURCE_BURST, SOURCE_PER_MINUTE, now);
                b.tokens < SOURCE_BURST
            });
        }
        let source = if self.sources.len() < SOURCES_KEPT || self.sources.contains_key(&ip) {
            let b = self
                .sources
                .entry(ip)
                .or_insert_with(|| Bucket::full(SOURCE_BURST, now));
            b.refill(SOURCE_BURST, SOURCE_PER_MINUTE, now);
            Some(b)
        } else {
            None
        };
        let source_ok = source.as_ref().is_none_or(|b| b.tokens >= 1.0);
        if !source_ok || self.total.tokens < 1.0 {
            return false;
        }
        if let Some(b) = source {
            b.tokens -= 1.0;
        }
        self.total.tokens -= 1.0;
        true
    }
}

// --- The port Nexus sends to ---------------------------------------------------------------------

/// The sentence refusing `port` for the contact port when Nexus itself sends to that port: its
/// WSJT-X UDP telemetry, its HRD logging, its JS8Call API, its N1MM broadcast. A contact Nexus sent there would come
/// straight back in. `None` when the port is none of them.
pub fn outbound_clash(s: &Settings, port: u16) -> Option<String> {
    let mut targets: Vec<(&str, String)> = Vec::new();
    if s.wsjtx_udp {
        for a in s.wsjtx_udp_addr.split(',') {
            targets.push(("WSJT-X UDP messages", a.trim().to_string()));
        }
    }
    if s.hrd_logging {
        targets.push((
            "Ham Radio Deluxe logging",
            s.hrd_udp_addr.trim().to_string(),
        ));
    }
    if s.js8_api_udp {
        targets.push((
            "JS8Call API messages",
            format!("127.0.0.1:{}", s.js8_api_udp_port),
        ));
    }
    if !s.n1mm_addr.trim().is_empty() {
        targets.push((
            "N1MM contact broadcast",
            tempo_net::n1mm::resolve_target(&s.n1mm_addr),
        ));
    }
    targets.into_iter().find_map(|(what, addr)| {
        let theirs = addr.rsplit_once(':')?.1.trim().parse::<u16>().ok()?;
        (theirs == port).then(|| {
            format!(
                "Contacts from other programs cannot use port {port}: Nexus sends its own {what} \
                 to {addr}, and they would come straight back in. Pick another port here, and the \
                 same one in the other program"
            )
        })
    })
}

/// The contact port as the settings want it: `Ok(None)` while it is off (or on port 0), the port
/// and whether it is bound on every adapter, or the sentence refusing a port Nexus sends to
/// ([`outbound_clash`]).
pub fn wanted(s: &Settings) -> Result<Option<(u16, bool)>, String> {
    let Some(port) = s
        .freedv_log
        .then_some(s.freedv_log_port)
        .filter(|p| *p != 0)
    else {
        return Ok(None);
    };
    match outbound_clash(s, port) {
        Some(why) => Err(why),
        None => Ok(Some((port, s.log_intake_lan))),
    }
}

// --- One datagram --------------------------------------------------------------------------------

/// What the listener keeps between datagrams: the rate limit, the refusals already said, the
/// admission as the settings last said, and the contact just added (for the pair below).
pub struct Gate {
    limit: RateLimit,
    /// FreeDV's "I am here" datagrams (Heartbeat, Status, Decode), each of which takes the Engine
    /// lock: limited as contacts are, in a bucket of their own so presence never starves one.
    alive: RateLimit,
    /// A QSO Logged (type 5) waiting [`HOLD`] for its Logged ADIF (type 12).
    held: Option<Held>,
    said: HashSet<String>,
    admission: Admission,
    admission_read: Option<Instant>,
    last_added: Option<(String, String, Instant)>,
}

/// The most refusals remembered as said; past it, a new one is not said (the connection log keeps
/// what it has, and a flood of addresses cannot flood it).
const SAID_KEPT: usize = 64;
/// How long a QSO Logged (type 5) waits for its Logged ADIF (type 12). WSJT-X sends the two back to
/// back for one contact, and the type 12 is the fuller (stock WSJT-X's type 5 names no satellite
/// and no submode): when it comes inside the hold it is logged and the type 5 let go; when it does
/// not, the type 5 is logged as it is.
pub const HOLD: Duration = Duration::from_secs(2);

/// A checked type 5 waiting for its type 12: the program, the contact's identity
/// ([`contact_key`]), the record, and when it came.
struct Held {
    program: String,
    key: Option<ContactKey>,
    checked: Checked,
    at: Instant,
}

/// A contact's identity for the hold: call, band, mode and start time, in the log's one form.
type ContactKey = (String, String, String, u64);

fn contact_key(adif: &str) -> Option<ContactKey> {
    let mut rec = tempo_core::logbook::parse_adif(adif).pop()?;
    tempo_core::logbook::canonical::contact(&mut rec);
    Some((
        rec.call.to_ascii_uppercase(),
        rec.band.to_ascii_lowercase(),
        rec.mode.to_ascii_uppercase(),
        rec.when_unix,
    ))
}

/// How long after a contact the same program's second datagram for it (WSJT-X sends a type 5 and
/// then a type 12) is let go without a line.
const PAIR_WINDOW: Duration = Duration::from_secs(10);

impl Gate {
    pub fn new() -> Self {
        Self {
            limit: RateLimit::new(Instant::now()),
            alive: RateLimit::new(Instant::now()),
            held: None,
            said: HashSet::new(),
            admission: Admission {
                lan: false,
                pin: None,
                pin_unreadable: false,
            },
            admission_read: None,
            last_added: None,
        }
    }

    /// Say `line` once while `key` stays refused ([`Self::clear`] lets it be said again).
    fn once(&mut self, key: String, note: &Note, line: String) {
        if self.said.len() < SAID_KEPT && self.said.insert(key) {
            tempo_core::applog::warn("log-intake", &line);
            note("error", line);
        }
    }

    fn clear(&mut self, key: &str) {
        self.said.remove(key);
    }
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// One datagram from `from` on the contact port, acted on: dropped by its source or the rate,
/// FreeDV's taken as it always was ([`crate::freedv::take_log_datagram`], its lines to
/// `freedv_note`), anything else read and each contact imported or refused (its lines to
/// `note`). Split out so a test drives it without a socket.
pub fn take_datagram(
    engine: &Mutex<Engine>,
    bytes: &[u8],
    from: SocketAddr,
    gate: &mut Gate,
    freedv_note: &Note,
    note: &Note,
) {
    let ip = from.ip();
    let now = Instant::now();
    flush_held(engine, gate, note, now);
    if gate
        .admission_read
        .is_none_or(|at| now.saturating_duration_since(at) >= Duration::from_secs(1))
    {
        gate.admission = Admission::of(engine_lock(engine).settings());
        gate.admission_read = Some(now);
    }
    if let Err(why) = gate.admission.admits(ip) {
        gate.once(
            format!("source {ip}"),
            note,
            format!("A datagram from {ip} on the contact port was not read: {why}"),
        );
        return;
    }
    let rate_key = format!("rate {ip}");
    match tempo_net::freedv::read_log_datagram(bytes) {
        tempo_net::freedv::LogIntake::Contact(_) => {
            if !gate.limit.take(ip, now) {
                gate.once(rate_key, note, rate_line(ip));
                return;
            }
            gate.clear(&rate_key);
            crate::freedv::take_log_datagram(engine, bytes, freedv_note);
            return;
        }
        tempo_net::freedv::LogIntake::Alive => {
            if gate.alive.take(ip, now) {
                crate::freedv::take_log_datagram(engine, bytes, freedv_note);
            }
            return;
        }
        tempo_net::freedv::LogIntake::Ignored => {}
    }
    let size_key = format!("size {ip}");
    let (format, program, records) = match read_datagram(bytes) {
        Intake::Contacts {
            format,
            program,
            records,
        } => (format, program, records),
        Intake::TooBig => {
            gate.once(
                size_key,
                note,
                format!(
                    "{ip} sent a datagram with more than {MAX_RECORDS} contacts or more fields \
                     than a contact has, so it was not read"
                ),
            );
            return;
        }
        Intake::Own | Intake::Ignored => return,
    };
    gate.clear(&size_key);
    // The program's own name for itself, or where it is when it gave none.
    let program = program.unwrap_or_else(|| canonical(ip).to_string());
    for fields in records {
        if !gate.limit.take(ip, now) {
            gate.once(rate_key.clone(), note, rate_line(ip));
            return;
        }
        gate.clear(&rate_key);
        take_contact(engine, &fields, &program, format, gate, note, now);
    }
}

/// Log the held type 5 once its [`HOLD`] has run out at `now` (no type 12 came for it). The
/// listener asks on every datagram and every quiet quarter second; a test asks with a `now` of its
/// own.
pub fn flush_held(engine: &Mutex<Engine>, gate: &mut Gate, note: &Note, now: Instant) {
    if gate
        .held
        .as_ref()
        .is_some_and(|h| now.saturating_duration_since(h.at) >= HOLD)
    {
        let h = gate.held.take().expect("checked above");
        log_checked(engine, h.checked, &h.program, gate, note, now);
    }
}

fn rate_line(ip: IpAddr) -> String {
    format!(
        "{ip} is sending contacts faster than Nexus takes them ({SOURCE_PER_MINUTE:.0} a minute), \
         so the rest are dropped until it slows down"
    )
}

/// One contact `program` logged: checked, imported, and said.
fn take_contact(
    engine: &Mutex<Engine>,
    fields: &Fields,
    program: &str,
    format: tempo_net::log_intake::Format,
    gate: &mut Gate,
    note: &Note,
    now: Instant,
) {
    use tempo_net::log_intake::Format;
    let station = engine_lock(engine).settings().mycall.clone();
    let checked = match contact_adif(fields, program, &station) {
        Ok(c) => c,
        Err(why) => {
            engine_lock(engine).freedv_mut().notice = Some(why.clone());
            tempo_core::applog::warn("log-intake", &why);
            note("error", why);
            return;
        }
    };
    match format {
        // A type 5 waits for its type 12 (a held one before it goes now, not lost).
        Format::Wsjtx => {
            if let Some(h) = gate.held.take() {
                log_checked(engine, h.checked, &h.program, gate, note, now);
            }
            gate.held = Some(Held {
                program: program.to_string(),
                key: contact_key(&checked.adif),
                checked,
                at: now,
            });
            return;
        }
        // The type 12 for the held type 5 is logged in its place.
        Format::WsjtxAdif => {
            let key = contact_key(&checked.adif);
            if key.is_some()
                && gate
                    .held
                    .as_ref()
                    .is_some_and(|h| h.program == program && h.key == key)
            {
                gate.held = None;
            }
        }
        Format::Adif | Format::N1mm => {}
    }
    log_checked(engine, checked, program, gate, note, now);
}

/// One checked contact `program` logged: imported, and said.
fn log_checked(
    engine: &Mutex<Engine>,
    checked: Checked,
    program: &str,
    gate: &mut Gate,
    note: &Note,
    now: Instant,
) {
    let (made, _durable) = crate::logwrite::intake_contact(engine, &checked.adif);
    let named = match checked.mhz {
        Some(mhz) => format!("{} · {mhz:.3} MHz", checked.call),
        None => checked.call.clone(),
    };
    // WSJT-X sends a type 5 and then a type 12 for one contact: the second meets the duplicate
    // check, and is let go without a line of its own.
    let second_of_pair = matches!(made, Ok(IntakeOutcome::InLog))
        && gate.last_added.as_ref().is_some_and(|(p, c, at)| {
            p == program && *c == checked.call && now.saturating_duration_since(*at) < PAIR_WINDOW
        });
    if second_of_pair {
        return;
    }
    let line = match &made {
        Ok(IntakeOutcome::Added) => format!("{program} logged {named}: in the log"),
        Ok(IntakeOutcome::InLog) => format!("{program} logged {named}: already in the log"),
        Err(e) => format!("{program} logged {named}, not imported: {e}"),
    };
    {
        let mut e = engine_lock(engine);
        let f = e.freedv_mut();
        match &made {
            Ok(IntakeOutcome::Added) => {
                f.note_intake(program, &checked.call, now_unix());
                f.notice = None;
            }
            Ok(IntakeOutcome::InLog) => {}
            Err(_) => f.notice = Some(line.clone()),
        }
    }
    if matches!(made, Ok(IntakeOutcome::Added)) {
        gate.last_added = Some((program.to_string(), checked.call.clone(), now));
    }
    if made.is_err() {
        tempo_core::applog::error("log-intake", &line);
        note("error", line);
    } else {
        note("info", line);
    }
}

/// ★ A contact WSJT-X logged in companion mode (its Logged ADIF, type 12, off the companion or
/// the sink socket, whose own trust rules the caller applied: this computer only, never Nexus's
/// own id, which decides what is Nexus's own here) into the log by the contact port's road ([`contact_adif`],
/// [`crate::logwrite::intake_contact`]): the same station-call check, the funnel's 300 s
/// duplicate check, "import new only", the service uploads, no local forward, and the via mark.
/// The program is the envelope's `PROGRAMID` (WSJT-X, JTDX and MSHV each write their own name
/// there), or "WSJT-X" when it names none. What became of each contact; an `Err` is a refusal or
/// an import that failed, in a sentence.
///
/// The same contact arriving on the contact port as well is one contact: the second meets the
/// duplicate check.
pub fn take_companion_adif(
    engine: &Mutex<Engine>,
    adif: &str,
) -> Vec<Result<IntakeOutcome, String>> {
    // Nexus's own record is kept out upstream, by its sender id (the companion socket and the
    // sink keep no Logged ADIF under it). A record whose envelope names Nexus under another
    // program's id is that program's, as the companion import always took it.
    let read = match tempo_net::log_intake::read_logged_adif(adif, None) {
        Intake::Own => tempo_net::log_intake::read_logged_adif(adif, Some("WSJT-X")),
        read => read,
    };
    let (program, records) = match read {
        Intake::Contacts {
            program, records, ..
        } => (program.unwrap_or_else(|| "WSJT-X".into()), records),
        _ => return Vec::new(),
    };
    let station = engine_lock(engine).settings().mycall.clone();
    records
        .iter()
        .map(|fields| {
            let checked = contact_adif(fields, &program, &station)?;
            let (made, _durable) = crate::logwrite::intake_contact(engine, &checked.adif);
            if matches!(made, Ok(IntakeOutcome::Added)) {
                engine_lock(engine)
                    .freedv_mut()
                    .note_intake(&program, &checked.call, now_unix());
            }
            made
        })
        .collect()
}

/// Serve the contact port until `shutdown`: each datagram at most `bound` bytes (a longer one is
/// dropped whole: the buffer is one byte longer than the bound so the two can be told apart)
/// goes to [`take_datagram`].
pub fn serve_within(
    sock: UdpSocket,
    engine: Arc<Mutex<Engine>>,
    shutdown: Arc<AtomicBool>,
    freedv_note: Note,
    note: Note,
    bound: usize,
) {
    let _ = sock.set_read_timeout(Some(Duration::from_millis(250)));
    let mut buf = vec![0u8; bound + 1];
    let mut gate = Gate::new();
    while !shutdown.load(Ordering::Relaxed) {
        let Ok((n, from)) = sock.recv_from(&mut buf) else {
            flush_held(&engine, &mut gate, &note, Instant::now());
            continue;
        };
        if n > bound {
            continue;
        }
        take_datagram(&engine, &buf[..n], from, &mut gate, &freedv_note, &note);
    }
    // Stopped (a port change, the switch off, the app closing): a held contact is logged, not
    // lost.
    flush_held(&engine, &mut gate, &note, Instant::now() + HOLD);
}

/// Open the contact port on every adapter, for "Accept from devices on my network". The listener,
/// not the bind, keeps out every source that is not this computer or a private address
/// ([`Admission`]).
pub fn bind_lan(port: u16) -> std::io::Result<UdpSocket> {
    UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))
}

#[cfg(test)]
mod tests;
