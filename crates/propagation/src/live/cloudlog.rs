//! Cloudlog / Wavelog QSO upload (HTTP JSON). Cloudlog (and its Wavelog fork) are self-hosted
//! web logbooks with an identical QSO API: `POST {base}/index.php/api/qso` with the instance
//! API key + station-profile id + one ADIF record. The URL + JSON builders are pure (unit-
//! tested); [`upload`] does the blocking POST, to where [`Destination`] decided it may go —
//! `https://`, or plain `http://` to the operator's own network only (#378).
//!
//! # ⛔ Where the API key is kept out of, and by what
//!
//! The key rides in the REQUEST body, and an instance can echo a request back — a debug-mode
//! PHP notice, a WAF page, a proxy error. So every string this module hands upwards is
//! assumed to be able to contain the key, and the boundary that matters is **what gets
//! written down**:
//!
//! - **Persisted** (`conn-health.json`, mode 0644, survives the session): guarded by an
//!   ALLOW-LIST at the sink — `note_conn_health` in `src-tauri/src/lib.rs` takes a
//!   `ConnDetail`, which can only be built from a string literal, so nothing derived from a
//!   response can reach it at all. That is the guarantee, and it is the only one. What this
//!   module gives that sink is [`CloudlogFailure`] — a closed set of CLASSES, so the row can
//!   still name which failure it was without persisting a character the instance wrote.
//! - **Ephemeral** (the operator's toast for this upload, and the in-memory connection log):
//!   carries the instance's own words, because #226 is precisely that Nexus threw them away.
//!   [`echoes_key`] reduces the chance the key is among them, and **it is best-effort, not a
//!   guarantee** — see its own note. Do not build anything on it, and do not add a round to
//!   it: three were spent, and the server picks the encoding.
//! - **`nexus-diag.log`** (rotated, on disk, made to be emailed to whoever is helping): the
//!   class and the HTTP status only, never `.message` — `diag_upload_failure` in
//!   `src-tauri/src/lib.rs` takes nothing else.
//! - **stderr is NOT ephemeral, and that is why [`CloudlogError`] has no `Display`.** On
//!   Linux the desktop session redirects a GUI process's stderr into `~/.xsession-errors`,
//!   mode 0644, which outlives the session exactly as `conn-health.json` does. A type that
//!   formats itself is one `eprintln!` away from that file, so this one does not; read
//!   `.message` where the instance's words are actually wanted.

use super::neterr;

/// Why one Cloudlog/Wavelog upload did not go through — the **class**, which is a closed set
/// Nexus decides, told apart from the instance's own words, which are not.
///
/// ⚠️ This exists because of the persisted row. `conn-health.json` may hold only sentences
/// Nexus wrote (see `note_conn_health` in `src-tauri/src/lib.rs`), and when that rule landed
/// this module had nothing but `Ok`/`Err(String)` to offer it — so every Cloudlog failure
/// collapsed into one sentence, and after a restart the panel could not tell a station
/// profile id that is not linked to the key from a URL that is not a Cloudlog instance from
/// an HTTP 500. HRDLog and WRL kept per-class sentences only because their services answer
/// with a closed result set; Cloudlog's classes are just as closed, they were only never
/// named. Naming them is what lets the row be as specific as its siblings without persisting
/// a single character the instance wrote.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloudlogFailure {
    /// Nothing was sent: the instance URL, the API key or the station profile id is missing,
    /// or the URL is not `https://` and not plain `http://` to the operator's own network (see
    /// [`Destination`]).
    NotConfigured,
    /// Nexus never got an answer out of the instance — DNS, a refused connect, a rejected
    /// TLS handshake, a redirect.
    Unreachable,
    /// The instance answered and rejected the credentials (HTTP 401/403).
    Credentials,
    /// The URL answered, but not as a Cloudlog/Wavelog QSO API (HTTP 404).
    NotAnApi,
    /// The instance took the request and refused to FILE the record — a station profile id
    /// not linked to this key, a malformed ADIF field. Carried in the body of an HTTP 2xx,
    /// which is why a 2xx alone never means the QSO landed.
    RecordRefused,
    /// The instance is in trouble (HTTP 5xx) — not the credentials, and worth retrying.
    ServerError,
    /// Answered, refused, and with no class of its own.
    Refused,
}

impl CloudlogFailure {
    /// Every class. The caller keeps one persisted sentence per class, so it needs to be able
    /// to enumerate them — see `cloudlog_stamp` in `src-tauri/src/lib.rs`.
    pub const ALL: [CloudlogFailure; 7] = [
        CloudlogFailure::NotConfigured,
        CloudlogFailure::Unreachable,
        CloudlogFailure::Credentials,
        CloudlogFailure::NotAnApi,
        CloudlogFailure::RecordRefused,
        CloudlogFailure::ServerError,
        CloudlogFailure::Refused,
    ];
}

/// One failed Cloudlog/Wavelog upload: what class of failure it was, and what the instance
/// said about it.
///
/// ⛔ The two halves go to different places and that is the whole point of the split.
/// `class` is Nexus's own, and is what the Connections row persists. `message` may quote the
/// instance verbatim — it goes to the operator's toast and to this session's in-memory
/// connection log, is read once, and is dropped. Deliberately no `Display`: a type that
/// formats itself is a type that can be handed to `eprintln!` or a log macro by accident, and
/// stderr is a file on Linux. Read `.message` where the words are wanted.
#[derive(Clone, Debug)]
pub struct CloudlogError {
    pub class: CloudlogFailure,
    pub message: String,
    /// The HTTP status the instance answered with, or `None` when nothing was sent or nothing
    /// came back. A number off the status line, never the body — so, like `class`, it may go
    /// where the instance's words may not (`nexus-diag.log`).
    pub status: Option<u16>,
}

impl CloudlogError {
    fn new(class: CloudlogFailure, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
            status: None,
        }
    }

    /// The same failure, recorded as answered with `status`.
    fn answered(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }
}

/// How much of the server's own words to show, in characters.
///
/// A Cloudlog/Wavelog `reason` is a short sentence; what else can arrive on this socket is a
/// reverse proxy's HTML error page or a PHP notice. The bound is not about screen space —
/// the panel row already truncates with `text-overflow: ellipsis` (`ui/src/styles.css`
/// `.conn-when`) — it is about where the string GOES: `src-tauri/src/lib.rs` puts it in the
/// 200-entry in-memory connection log and hands it to the operator's toast, and a 50 000-
/// character proxy page is 200 log entries' worth of memory for one failure. 160 leaves room
/// for a real sentence and cuts a web page off at its title, which is the part that
/// identifies it. (It no longer reaches `conn-health.json` at all — only a [`CloudlogFailure`]
/// does.)
///
/// Measured against a 50 000-character HTML error page: the whole operator-facing message
/// comes out at **201 characters** (this bound, the ellipsis, and the longest prefix
/// `classify` builds). A genuine Cloudlog reason lands well inside it — the reported
/// station-profile rejection measures 96.
///
/// ⚠️ It is a SIZE bound and nothing else. It was once described as a backstop for the API-key
/// scrub; it never was one, and the arithmetic says so plainly — a Cloudlog key is 33
/// characters and this is 160, so a key echoed near-verbatim fits with 120 to spare. What
/// keeps the key out of the persisted file is the allow-list at the sink (module header);
/// [`echoes_key`] only thins the ephemeral surfaces.
const REASON_MAX_CHARS: usize = 160;

/// Build the QSO API endpoint from a user-entered base URL. Tolerant of a trailing slash, an
/// already-present `/index.php`, or the full `/index.php/api/qso` path.
pub fn api_url(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    if b.ends_with("/api/qso") {
        b.to_string()
    } else if b.contains("/index.php") {
        format!("{b}/api/qso")
    } else {
        format!("{b}/index.php/api/qso")
    }
}

/// Where one Cloudlog/Wavelog request may go, decided once, before anything is sent (#378).
///
/// ⛔ CREDENTIAL. Every request to the instance carries the API key: in the body of an upload,
/// in the PATH of a station_info lookup. Over `https://` that is what it always was, and that
/// path is untouched: the same https-only client, the same proxies, no redirects. Plain
/// `http://` sends the key unencrypted, so it is accepted only where the key cannot leave the
/// operator's own network, and every part of that is decided here rather than left to the
/// socket:
///
/// - **The host is on the network** ([`on_own_network`]): a literal address in one of its
///   ranges, or a name EVERY address of which is in one. A name with a single address outside
///   is refused, because nothing here chooses which of its addresses a connection would use.
/// - **The request goes where the check looked, and only there.** A name's checked addresses
///   are pinned into the client (`resolve_to_addrs`), so nothing resolves the name again between
///   the check and the send: a name re-pointed in that window (DNS rebinding) cannot take the
///   key with it. The client takes no proxy (an `HTTP_PROXY` in the environment would carry the
///   key to the proxy, and the proxy would resolve the name itself) and follows no redirect.
///
/// Anything else that says `http://` is refused before a socket is opened, as it always was.
pub struct Destination {
    /// The base URL as Settings holds it, trimmed. [`upload`] and [`fetch_station_info`] build
    /// their endpoints from it, so both go to the host checked here.
    base: String,
    /// `Some` only for plain http to the operator's own network.
    lan: Option<LanHost>,
}

/// A plain-http host on the operator's own network, as checked.
struct LanHost {
    /// The URL's host as reqwest looks it up: the parser lowercases a name, and an IPv6
    /// literal keeps its brackets.
    host: String,
    /// The addresses a NAME was checked at, pinned into the client. Empty for a literal
    /// address, which nothing resolves.
    pinned: Vec<std::net::SocketAddr>,
}

/// A host-name lookup: the system resolver in use, a stand-in in the tests.
type Lookup<'a> = &'a dyn Fn(&str, u16) -> std::io::Result<Vec<std::net::SocketAddr>>;

/// How long a host name's lookup may take. reqwest bounds its own lookups by the request's
/// timeout; this one happens before the request, so it has a bound of its own.
const LOOKUP_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

impl Destination {
    /// Decide where a request to `base_url` may go. The system resolver answers for a name.
    pub fn check(base_url: &str) -> Result<Destination, CloudlogError> {
        Self::check_with(base_url, &system_lookup)
    }

    fn check_with(base_url: &str, lookup: Lookup<'_>) -> Result<Destination, CloudlogError> {
        let base = base_url.trim().to_string();
        // Anything that is not plain http (https://, another scheme, a URL that does not parse)
        // is left to the https-only client exactly as before, which sends only https.
        let url = match reqwest::Url::parse(&api_url(&base)) {
            Ok(u) if u.scheme() == "http" => u,
            _ => return Ok(Destination { base, lan: None }),
        };
        let host = url.host_str().unwrap_or_default().to_string();
        // A literal address arrives here as the URL parser normalised it (`0x7f.1` is
        // `127.0.0.1` by now), which is also the address reqwest connects to.
        let literal = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .ok();
        let pinned = match literal {
            Some(ip) if on_own_network(ip) => Vec::new(),
            Some(ip) => return Err(plain_http_refused(&format!("{ip} is not one"))),
            None => {
                let port = url.port_or_known_default().unwrap_or(80);
                let found = lookup(&host, port).map_err(|_| {
                    plain_http_refused(&format!("{host} could not be looked up to check"))
                })?;
                if let Some(off) = found.iter().find(|a| !on_own_network(a.ip())) {
                    return Err(plain_http_refused(&format!(
                        "{host} resolves to {}, which is not one",
                        off.ip()
                    )));
                }
                if found.is_empty() {
                    return Err(plain_http_refused(&format!(
                        "{host} resolves to no address"
                    )));
                }
                found
            }
        };
        Ok(Destination {
            base,
            lan: Some(LanHost { host, pinned }),
        })
    }

    /// Where the API key travels unencrypted, for the Connections log: the host, and for a
    /// name the addresses it was checked at. `None` over https.
    pub fn cleartext_to(&self) -> Option<String> {
        let lan = self.lan.as_ref()?;
        if lan.pinned.is_empty() {
            return Some(lan.host.clone());
        }
        let mut ips: Vec<String> = Vec::new();
        for a in &lan.pinned {
            let ip = a.ip().to_string();
            if !ips.contains(&ip) {
                ips.push(ip);
            }
        }
        Some(format!("{} ({})", lan.host, ips.join(", ")))
    }

    /// The client for this destination. https: exactly the client every request used before
    /// #378. Plain http on the operator's network: no proxy, no redirect, and a name pinned to
    /// the addresses it was checked at.
    fn client(&self) -> Result<reqwest::blocking::Client, CloudlogError> {
        let builder = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none());
        let builder = match &self.lan {
            None => builder.https_only(true),
            Some(lan) if lan.pinned.is_empty() => builder.no_proxy(),
            Some(lan) => builder.no_proxy().resolve_to_addrs(&lan.host, &lan.pinned),
        };
        builder.build().map_err(|_| {
            CloudlogError::new(CloudlogFailure::Unreachable, "couldn't build HTTP client")
        })
    }
}

/// Is `ip` on the operator's own network, so that a key sent to it in the clear stays there?
///
/// Loopback (127/8, `::1`), the private IPv4 ranges (10/8, 172.16/12, 192.168/16), link-local
/// (169.254/16, fe80::/10, which are never routed off the link) and IPv6 unique-local
/// (fc00::/7). An IPv4-mapped IPv6 address counts as the IPv4 address it carries.
///
/// Deliberately not on it: 100.64/10, the carrier-grade NAT range, which a provider's own
/// network uses as readily as a home VPN does; the unspecified, broadcast and multicast
/// addresses; and every global address, a home's own IPv6 prefix included, because nothing in
/// an address says which global addresses are the operator's.
fn on_own_network(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        std::net::IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => on_own_network(std::net::IpAddr::V4(v4)),
            None => v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local(),
        },
    }
}

/// The refusal for plain http anywhere but the operator's own network. `why` says what about
/// this host failed the check.
fn plain_http_refused(why: &str) -> CloudlogError {
    CloudlogError::new(
        CloudlogFailure::NotConfigured,
        format!(
            "Cloudlog/Wavelog: the instance URL must be https://. Plain http:// is accepted only \
             for an address on your own network (192.168.x.x, 10.x.x.x, 172.16–31.x.x, \
             127.x.x.x and the like, or a name that resolves only to such addresses), because \
             the API key travels unencrypted, and {why}. Nothing was sent"
        ),
    )
}

/// The system resolver, bounded by [`LOOKUP_DEADLINE`].
fn system_lookup(host: &str, port: u16) -> std::io::Result<Vec<std::net::SocketAddr>> {
    use std::net::ToSocketAddrs;
    let target = (host.to_string(), port);
    within(LOOKUP_DEADLINE, move || {
        target.to_socket_addrs().map(Iterator::collect)
    })
}

/// `f`'s answer, or a timeout if it has none by `deadline`. A lookup cannot be cancelled, so
/// its thread is left to finish on its own; nothing waits for it.
fn within<F>(deadline: std::time::Duration, f: F) -> std::io::Result<Vec<std::net::SocketAddr>>
where
    F: FnOnce() -> std::io::Result<Vec<std::net::SocketAddr>> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(deadline).unwrap_or_else(|_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "the lookup took too long",
        ))
    })
}

/// Escape a string for embedding in a JSON string literal.
fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

/// Build the Cloudlog/Wavelog JSON request body for one ADIF record.
pub fn build_body(key: &str, station_id: &str, adif: &str) -> String {
    format!(
        "{{\"key\":\"{}\",\"station_profile_id\":\"{}\",\"type\":\"adif\",\"string\":\"{}\"}}",
        json_escape(key),
        json_escape(station_id),
        json_escape(adif)
    )
}

/// Classify a Cloudlog/Wavelog 2xx response. The per-record import result is carried IN the
/// body (`{"status":"created"}` on success; `{"status":"failed"|"error",...}` on a rejected or
/// misfiled record), so an HTTP 2xx alone does not mean the QSO was filed. Lenient on unknown
/// body shapes so a Wavelog variant with a different success payload isn't reported as failed.
fn classify_body(text: &str, reason: Option<&str>) -> Result<String, CloudlogError> {
    let t = text.to_ascii_lowercase().replace(' ', "");
    if t.contains("\"status\":\"failed\"") || t.contains("\"status\":\"error\"") {
        return Err(CloudlogError::new(
            CloudlogFailure::RecordRefused,
            match reason {
                Some(r) => format!("Cloudlog rejected the QSO: {r}"),
                None => "Cloudlog rejected the QSO — check the instance log".to_string(),
            },
        ));
    }
    Ok(text.to_string())
}

/// Unicode **Cf** (format) characters, which [`char::is_control`] does not cover.
///
/// ⚠️ U+202E RIGHT-TO-LEFT OVERRIDE visually reverses everything after it, so a server-supplied
/// string carrying one rewrites the rest of the failure detail on the panel row and in
/// `conn-health.json`. U+200B, U+FEFF and U+00AD are simply invisible — they pad a message
/// with characters nobody can see or delete. C0/C1, DEL, ESC and BEL were already stripped by
/// the `is_control` filter below; these are the rest of the class.
///
/// Listed rather than derived: `std` carries no general-category table, and the alternative
/// (keep only what looks safe) throws away every non-Latin script a service might answer in.
/// The same filter guards the other end of this string — `note_conn_health` in
/// `src-tauri/src/lib.rs`, which is where every connector's detail is persisted.
fn is_invisible_format(c: char) -> bool {
    matches!(
        c as u32,
        0x00AD                  // SOFT HYPHEN
        | 0x0600..=0x0605       // Arabic number signs
        | 0x061C                // ARABIC LETTER MARK
        | 0x06DD | 0x070F | 0x0890..=0x0891 | 0x08E2
        | 0x180E                // MONGOLIAN VOWEL SEPARATOR
        | 0x200B..=0x200F       // ZWSP, ZWNJ, ZWJ, LRM, RLM
        | 0x202A..=0x202E       // bidi embedding/override — U+202E is the dangerous one
        | 0x2060..=0x2064       // WORD JOINER, invisible operators
        | 0x2066..=0x206F       // bidi isolates, deprecated format characters
        | 0xFEFF                // ZWNBSP / byte-order mark
        | 0xFFF9..=0xFFFB       // interlinear annotation
        | 0xE0000..=0xE007F     // TAGS — invisible by construction
    )
}

/// What the operator is told instead of the body when the server echoed our own API key back.
///
/// Fixed text: nothing from the body survives. It is still the actionable half — an instance
/// answering with the request it just received is a debug-mode notice or a proxy page, not
/// Cloudlog, and that is a thing to go and look at.
const KEY_ECHOED: &str = "the reply echoed the API key back, so its wording is withheld \
                          (something is answering with the request it received)";

/// How much of the key has to show through for [`echoes_key`] to suppress the body.
///
/// Twelve alphanumeric characters of a random key will not appear in a Cloudlog sentence or a
/// proxy's error page by chance, and twelve characters of a credential is already more than
/// belongs in a persisted file. Short enough that encoding one character in the middle cannot
/// hide the rest, which is the failure this window exists for.
const KEY_WINDOW: usize = 12;

/// `s` reduced to its alphanumeric characters, lowercased.
fn alnum_lower(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// `text` with every escape-shaped run deleted — HTML entities (`&#45;`, `&#x2D;`, `&amp;`),
/// percent escapes (`%2D`) and backslash escapes (`\u002d`, `\x2d`).
///
/// Deliberately NOT a decoder: it removes the escape rather than producing the character it
/// stood for. That is what [`echoes_key`] needs — the comparison drops non-alphanumerics
/// anyway, so an escape standing for one of the key's separators has to VANISH rather than
/// turn into the digits of its own code point (`&#45;` decoded is `-`, which then drops out;
/// `&#45;` half-decoded is `45`, which wedges two digits into the middle of the key and hides
/// it). Every form is bounded so a bare `&` or `%` in prose is left alone.
fn strip_escapes(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let hex = |from: usize, n: usize| {
        c.len() >= from + n && c[from..from + n].iter().all(char::is_ascii_hexdigit)
    };
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            // An entity is at most `&#x10FFFF;`; anything longer is not one.
            '&' => match c[i + 1..].iter().take(10).position(|ch| *ch == ';') {
                Some(n) => i += n + 2,
                None => {
                    out.push(c[i]);
                    i += 1;
                }
            },
            '%' if hex(i + 1, 2) => i += 3,
            '\\' if c.len() > i + 1 && c[i + 1] == 'u' && hex(i + 2, 4) => i += 6,
            '\\' if c.len() > i + 1 && c[i + 1] == 'x' && hex(i + 2, 2) => i += 4,
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }
    out
}

/// ⚠️ BEST EFFORT, AND KNOWN TO BE DEFEATABLE. Does the API key show through `text`?
///
/// **This is not what keeps the key off disk** — the allow-list at `note_conn_health` is (see
/// the module header). It runs on the surfaces that are shown once and dropped: the toast for
/// this upload and the in-memory connection log. There it is worth having and worth no more
/// rounds than it has had.
///
/// What it does: ignoring every non-alphanumeric, and again with escape-shaped runs deleted,
/// does any [`KEY_WINDOW`]-character stretch of the key appear? That catches the echoes a
/// server produces without trying — a PHP notice HTML-escaping what it prints, a WAF page
/// percent-encoding it, a JSON error writing `\uXXXX`, a zero-width character wedged inside.
///
/// **What it does not catch, measured rather than assumed:**
/// - an echo with EVERY character escaped (`%63%6C…`, `&#99;…`, `&#x63;…`). [`strip_escapes`]
///   deletes escape runs instead of decoding them, so both views go blind and all 33
///   characters stay recoverable from the text.
/// - fragmentation: [`KEY_WINDOW`] is a fixed 12, so two escaped characters at roughly
///   one-third and two-thirds leave no 12-character stretch in either view.
///
/// Those are recorded, not scheduled. Round 1 was `str::replace`, defeated by one re-encoded
/// character; round 2 was this, defeated two ways; the server chooses the encoding, so a
/// blocklist here has no last move. Round 3 was to stop playing and allow-list the sink
/// instead. If a persisted surface ever needs the instance's words, the answer is a bounded
/// classification of the response, not a better detector here.
fn echoes_key(text: &str, key: &str) -> bool {
    let needle: Vec<char> = alnum_lower(key).chars().collect();
    let win = needle.len().min(KEY_WINDOW);
    // Below this the shape is not distinctive and ordinary prose would match it. A key this
    // short is not a working Cloudlog key; the literal scrub still applies to it.
    if win < 8 {
        return false;
    }
    let views = [alnum_lower(text), alnum_lower(&strip_escapes(text))];
    needle.windows(win).any(|w| {
        let stretch: String = w.iter().collect();
        views.iter().any(|v| v.contains(&stretch))
    })
}

/// The server's own explanation of a failure, made safe to show — or `None` when it said
/// nothing an operator can use.
///
/// ⚠️ #226, and this is the whole point of the issue. Cloudlog and Wavelog answer a rejected
/// upload with a body naming what they rejected: a missing or read-only API key, a station
/// profile id not linked to that key, a malformed record. Nexus read that body and threw it
/// away, so all of them arrived as "check the API key" — the one thing the reporter had
/// already checked, on two independent instances, with a key four other clients accept.
///
/// Two things are done to the body before any of it is shown:
///
/// 1. **The API key is scrubbed as far as a scrub can go.** The key rides in the REQUEST
///    body, and a debug-mode PHP notice or a WAF page can echo a request straight back. The
///    literal replacement below catches a verbatim echo; [`echoes_key`] catches the ordinary
///    re-encodings, and when it fires **none of the body is shown**. ⚠️ Neither is a
///    guarantee, and this string must not be treated as one: it is for the surfaces that are
///    read once and dropped. Nothing derived from it may be persisted — `conn-health.json`
///    takes only Nexus's own sentences, enforced by the type of `note_conn_health` in
///    `src-tauri/src/lib.rs`. See the module header.
/// 2. **It is flattened to one line and cut to [`REASON_MAX_CHARS`].** A size bound, not a
///    security one — see that constant.
///
/// A JSON answer's explanation is read from its named field, because the object as a whole is
/// machine shape rather than words for an operator. A body that is not JSON at all — a
/// reverse proxy's HTML page, a PHP notice — IS the message, and a bounded slice of it is
/// worth showing: knowing a proxy answered instead of Cloudlog is the actionable half.
///
/// The fields, in order: `reason`, the one Cloudlog and Wavelog write for a client; then
/// Wavelog's `messages` (see [`joined_messages`]); then `message` and `error`.
fn server_reason(text: &str, key: &str) -> Option<String> {
    let k = key.trim();
    let scrubbed = match k {
        "" => text.to_string(),
        k => text.replace(k, "[api key]"),
    };
    let words = match serde_json::from_str::<serde_json::Value>(&scrubbed) {
        Ok(v) => v
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| joined_messages(&v))
            .or_else(|| {
                ["message", "error"]
                    .iter()
                    .find_map(|f| v.get(f).and_then(serde_json::Value::as_str))
                    .or_else(|| v.as_str())
                    .map(str::to_string)
            })?,
        Err(_) => scrubbed.clone(),
    };
    // Fail closed. Both views are checked: `scrubbed` is the body as it arrived, and `words`
    // is what serde produced from it — by which point any `\uXXXX` the server escaped the key
    // with has already been decoded back into the key itself, and any markup splitting it has
    // become a space.
    if !k.is_empty() && (echoes_key(&scrubbed, k) || echoes_key(&words, k)) {
        return Some(KEY_ECHOED.to_string());
    }
    // Control characters (newlines included) and invisible format characters become spaces,
    // then runs of whitespace collapse: an HTML page is otherwise 40 blank lines in a tooltip,
    // and a U+202E reverses the rest of the row on screen.
    let flat = words
        .chars()
        .map(|c| {
            if c.is_control() || is_invisible_format(c) {
                ' '
            } else {
                c
            }
        })
        .collect::<String>();
    let mut out: String = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.is_empty() {
        return None;
    }
    if out.chars().count() > REASON_MAX_CHARS {
        out = out.chars().take(REASON_MAX_CHARS).collect::<String>() + "…";
    }
    Some(out)
}

/// Wavelog's explanation of a QSO its import refused: the non-empty strings of `messages`,
/// joined, with their markup turned into spaces — or `None` when there are none.
///
/// ⚠️ This is where a refused QSO's reason actually is. Wavelog's `Api.php` `qso()` (read
/// 2026-10-04) answers it with HTTP 400 and
/// `{"status":"abort",…,"messages":["","<reason html>"]}`: no `reason` field at all, so Nexus
/// read nothing and told the operator the instance "said no more" — while Wavelog had named the
/// QSO, the callsign and the station location it refused. The reason is
/// `Logbook_model::import_bulk`'s, written for a web page (`<b>` around values, `<br>` after
/// each line), which is why the tags go.
fn joined_messages(v: &serde_json::Value) -> Option<String> {
    let said: Vec<String> = v
        .get("messages")?
        .as_array()?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(without_tags)
        .filter(|m| !m.trim().is_empty())
        .collect();
    (!said.is_empty()).then(|| said.join(" "))
}

/// `text` with every HTML tag (`<br>`, `<b>`, `</b>`, `<br />`) turned into a space, so the
/// words either side of a `<br>` stay apart. A `<` that does not open a tag — nothing but a
/// letter, `/` or `!` after it, or no `>` to close it — is left as it is.
fn without_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let opens = after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        match after.find('>') {
            Some(end) if opens => {
                out.push(' ');
                rest = &after[end + 1..];
            }
            _ => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// POST one ADIF record to a Cloudlog/Wavelog instance. `Ok(body)` when the record is actually
/// filed; a redacted error otherwise (the API key is in the REQUEST body — never echoed into an
/// error string). Sent where `dest` decided it may go — `https://`, or plain `http://` to the
/// operator's own network only — and never after a redirect, so a credential-bearing request
/// cannot be moved onto cleartext or off that network.
pub fn upload(
    dest: &Destination,
    key: &str,
    station_id: &str,
    adif: &str,
) -> Result<String, CloudlogError> {
    if key.trim().is_empty() {
        return Err(CloudlogError::new(
            CloudlogFailure::NotConfigured,
            "Cloudlog API key is empty — set it in Settings",
        ));
    }
    if station_id.trim().is_empty() {
        return Err(CloudlogError::new(
            CloudlogFailure::NotConfigured,
            "Cloudlog station profile id is empty — set it in Settings",
        ));
    }
    // #226: `station_profile_id` is the NUMERIC id of a station location owned by the key's user
    // (Wavelog `Api.php` qso() → `Stations::check_station_against_user`). A callsign here is
    // answered with HTTP 401 on every contact, so it is refused before anything is sent, and the
    // trimmed id is what goes out.
    let station_id = station_id.trim();
    if !station_id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CloudlogError::new(
            CloudlogFailure::NotConfigured,
            "Cloudlog/Wavelog station profile id must be the station location number (Station \
             Locations — the number at the end of that location's Edit link), not a callsign. \
             Nothing was sent — fix it in Settings",
        ));
    }
    let url = api_url(&dest.base);
    let body = build_body(key, station_id, adif);
    let client = dest.client()?;
    let resp = client
        .post(&url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        // #226's defect on the transport arm: this flattened every way of failing to reach
        // the instance into one sentence blaming the URL. `is_connect()` is true for a DNS
        // failure, a refused connect, an unreachable proxy AND a rejected TLS handshake, and
        // it is the handshake that matters — an HTTPS-inspecting antivirus re-signs with a CA
        // Nexus does not carry (D#181), so "check the URL" sends that operator after the one
        // thing that is right. `neterr` splits them and never stringifies the error, which
        // this request needs anyway: the API key is in its body.
        .map_err(|e| {
            if e.is_builder() {
                // The one case the old sentence WAS right about: `https_only` rejects a URL
                // that is not https:// here, before any I/O (reqwest async_impl/client.rs:2582).
                // A plain http:// one never gets this far — `Destination` decided it — so what
                // lands here is another scheme or a URL that does not parse. Nothing was sent,
                // and the fix is in Settings — the same class as an empty key, not a network
                // failure.
                CloudlogError::new(
                    CloudlogFailure::NotConfigured,
                    "Cloudlog/Wavelog: the instance URL must be https:// — an upload carrying \
                     the API key is never sent in the clear",
                )
            } else {
                // Policy::none, so `redact`'s redirect wording is the right one.
                CloudlogError::new(CloudlogFailure::Unreachable, neterr::redact("Cloudlog", &e))
            }
        })?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    classify(status.as_u16(), &text, key)
}

/// Turn one answered response into the operator's result. Pure, so the whole
/// classification is unit-testable without a server (`upload` above is only the socket).
///
/// Where the server explained itself, its words lead and Nexus's guess is dropped: a guess
/// printed beside an answer is noise. Where it did not, the guess is all there is, so it
/// stays exactly as it was — and it names the station profile id as well as the key, because
/// #226's actual failure was the profile id and the old wording never mentioned it.
fn classify(status: u16, text: &str, key: &str) -> Result<String, CloudlogError> {
    let reason = server_reason(text, key);
    if (200..300).contains(&status) {
        return classify_body(text, reason.as_deref()).map_err(|e| e.answered(status));
    }
    // #378: neither client follows a redirect with the key in the request, so a 3xx arrives
    // here as an answer. Say which answer it was: "refused the upload" would read as the
    // instance saying no, when it only said "not here".
    if (300..400).contains(&status) {
        return Err(
            CloudlogError::new(CloudlogFailure::Refused, redirected(status)).answered(status),
        );
    }
    // The status line is a bounded value Nexus reads off the wire, not text out of the body,
    // so classifying by it carries nothing the instance chose. 404 is the wrong-URL case
    // #226's reporter could not tell from the others, and 5xx is the instance's own trouble
    // rather than anything the operator can fix.
    let class = match status {
        401 | 403 => CloudlogFailure::Credentials,
        404 => CloudlogFailure::NotAnApi,
        500..=599 => CloudlogFailure::ServerError,
        _ => CloudlogFailure::Refused,
    };
    let what = if class == CloudlogFailure::Credentials {
        "rejected the credentials"
    } else {
        "refused the upload"
    };
    Err(CloudlogError::new(
        class,
        match reason {
            Some(r) => format!("Cloudlog HTTP {status} — {what}: {r}"),
            None if class == CloudlogFailure::Credentials => format!(
                "Cloudlog HTTP {status} — {what}, and said no more. Check the API key and the \
                 station profile id (a key is scoped to one profile)."
            ),
            None => format!("Cloudlog HTTP {status} — {what}, and said no more."),
        },
    )
    .answered(status))
}

/// What the operator is told about a redirect, which is never followed with the API key.
fn redirected(status: u16) -> String {
    format!(
        "Cloudlog HTTP {status} — the instance answered with a redirect, and Nexus never \
         follows one with the API key. Put the address it redirects to in Settings"
    )
}

/// One station location as Wavelog/Cloudlog reports it (#226). Their `station_info` answer is
/// an array of these; the operator picks one and its `station_id` is what the QSO upload sends
/// as `station_profile_id`.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudlogStation {
    pub station_id: String,
    pub profile_name: String,
    pub callsign: String,
    pub gridsquare: String,
    pub active: bool,
}

/// How a station location disagrees with the QSO filed under it — the two checks Wavelog's
/// import makes before it files one (`Logbook_model::import`, read 2026-10-04), and refuses it
/// on with HTTP 400: a STATION_CALLSIGN that is not the location's ("Differing station callsign
/// … SKIPPED"), or a MY_GRIDSQUARE that does not fit the location's grid ("Differing locator …
/// SKIPPED"). Nexus stamps STATION_CALLSIGN on every contact it logs, so a location carrying any
/// other callsign refuses all of them.
///
/// The rule is Settings' warning at the pick too (`ui/src/settings/cloudlogLocation.ts`); keep
/// the two alike. The callsign is compared whole, the grid on its first four characters, a
/// location listing several grids (comma-separated) matches on any of them, and an empty side
/// gives no verdict — Wavelog fills a missing STATION_CALLSIGN or MY_GRIDSQUARE from the
/// location, so a QSO without one cannot disagree on it. ⚠️ Wavelog compares grids more strictly
/// than this (one must begin with the other, so DM41AB against DM41CD is refused too); that case
/// reaches the operator in Wavelog's own words, through [`joined_messages`], rather than here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocationMismatch {
    /// The location's callsign is not the QSO's STATION_CALLSIGN.
    pub callsign: bool,
    /// The location's grid is not the QSO's MY_GRIDSQUARE, in its first four characters.
    pub grid: bool,
}

/// The first four characters of a grid, uppercased — or `None` when there are fewer.
fn square(grid: &str) -> Option<String> {
    let sq: String = grid.trim().to_ascii_uppercase().chars().take(4).collect();
    (sq.chars().count() == 4).then_some(sq)
}

/// Compare a station location with the STATION_CALLSIGN and MY_GRIDSQUARE a QSO carries (empty
/// when it carries none). See [`LocationMismatch`].
pub fn location_mismatch(
    location: &CloudlogStation,
    station_call: &str,
    my_grid: &str,
) -> LocationMismatch {
    let call = station_call.trim().to_ascii_uppercase();
    let theirs = location.callsign.trim().to_ascii_uppercase();
    let squares: Vec<String> = location.gridsquare.split(',').filter_map(square).collect();
    LocationMismatch {
        callsign: !call.is_empty() && !theirs.is_empty() && call != theirs,
        grid: square(my_grid).is_some_and(|mine| !squares.is_empty() && !squares.contains(&mine)),
    }
}

/// A value the instance sent about a location, fit to repeat to the operator: shaped like a
/// callsign, a station number or a grid list — letters, digits, `/`, `-` and `,`, at most 20.
/// These are the instance's words, so anything else (a direction override, a sentence, a
/// 33-character key) is not repeated.
fn plain(value: &str) -> Option<&str> {
    let v = value.trim();
    let fits = !v.is_empty()
        && v.chars().count() <= 20
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | ','));
    fits.then_some(v)
}

/// What the operator is told beside Wavelog's own refusal when the QSO it refused does not
/// match the station location it was sent to — in Nexus's words, naming both sides. `None` when
/// they match, or when a location value it would have to repeat is not [`plain`]: then the
/// instance's own refusal stands alone.
///
/// Ephemeral, like the rest of [`CloudlogError::message`]: the toast and this session's
/// connection log, never a file.
pub fn location_mismatch_note(
    location: &CloudlogStation,
    station_call: &str,
    my_grid: &str,
) -> Option<String> {
    let m = location_mismatch(location, station_call, my_grid);
    let mut said = Vec::new();
    if m.callsign {
        said.push(format!(
            "its callsign is {}, but this QSO was logged as {}",
            plain(&location.callsign)?,
            station_call.trim().to_ascii_uppercase()
        ));
    }
    if m.grid {
        said.push(format!(
            "its grid is {}, but this QSO carries {}",
            plain(&location.gridsquare)?,
            my_grid.trim().to_ascii_uppercase()
        ));
    }
    if said.is_empty() {
        return None;
    }
    Some(format!(
        "Station location {} is not this QSO's station: {}. Pick a location that matches in \
         Settings, or change this one in Wavelog",
        plain(&location.station_id)?,
        said.join("; ")
    ))
}

/// The station_info endpoint for a base URL — Wavelog's own shape, with the API key in the
/// PATH: `GET {base}/index.php/api/station_info/{key}`. Base handling is [`api_url`]'s.
///
/// ⛔ THE KEY IS IN THE URL, which is why this module never logs a station_info URL, why the
/// errors below are built from the STATUS LINE and [`neterr::redact`] only — never from the
/// URL or the response body, either of which can carry it back — and why a key that is not
/// URL-path-safe is refused HERE rather than escaping into the path (a `/` would silently
/// change which endpoint is called).
pub fn station_info_url(base: &str, key: &str) -> Result<String, CloudlogError> {
    let k = key.trim();
    let safe = |b: &u8| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
    if k.is_empty() || !k.bytes().all(|b| safe(&b)) {
        // The key is NOT quoted back — the whole point of this guard.
        return Err(CloudlogError::new(
            CloudlogFailure::NotConfigured,
            "the Cloudlog/Wavelog API key is empty or has characters that cannot go in a URL \
             — check it in Settings",
        ));
    }
    let qso = api_url(base);
    let root = qso.strip_suffix("/qso").unwrap_or(qso.as_str());
    Ok(format!("{root}/station_info/{k}"))
}

/// One field of a station row, as a trimmed string. Wavelog sends every field as a string;
/// a number is accepted too rather than read as missing.
fn station_field(v: &serde_json::Value, key: &str) -> String {
    match v.get(key) {
        Some(serde_json::Value::String(s)) => s.trim().to_string(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Is this row's `station_active` flag set? `"1"`, `1` and `true` all mean active.
fn station_active(v: &serde_json::Value) -> bool {
    match v.get("station_active") {
        Some(serde_json::Value::String(s)) => {
            let s = s.trim();
            s == "1" || s.eq_ignore_ascii_case("true")
        }
        Some(serde_json::Value::Number(n)) => n.as_i64() == Some(1),
        Some(serde_json::Value::Bool(b)) => *b,
        _ => false,
    }
}

/// One answered station_info response → the station list, or a class with Nexus's OWN sentence.
///
/// ⛔ The body is PARSED, never quoted: an error page from this endpoint can echo the request
/// URL, and that URL carries the API key. `upload`'s `server_reason` path is deliberately not
/// reused here for that reason.
pub fn classify_station_info(
    status: u16,
    body: &str,
) -> Result<Vec<CloudlogStation>, CloudlogError> {
    let not_a_list = || {
        CloudlogError::new(
            CloudlogFailure::NotAnApi,
            "that URL answered, but not with a Cloudlog/Wavelog station list — check the \
             instance URL in Settings",
        )
    };
    match status {
        200..=299 => {}
        401 | 403 => {
            return Err(CloudlogError::new(
                CloudlogFailure::Credentials,
                "Cloudlog/Wavelog refused the API key for station locations — check the key, \
                 and that it is not a read-only one",
            ))
        }
        300..=399 => {
            return Err(CloudlogError::new(
                CloudlogFailure::Refused,
                redirected(status),
            ))
        }
        404 => {
            return Err(CloudlogError::new(
                CloudlogFailure::NotAnApi,
                "this Cloudlog/Wavelog has no station_info endpoint (an older version) — \
                 enter the station location number by hand",
            ))
        }
        500..=599 => {
            return Err(CloudlogError::new(
                CloudlogFailure::ServerError,
                format!("Cloudlog HTTP {status} — the instance is in trouble; try again shortly"),
            ))
        }
        _ => {
            return Err(CloudlogError::new(
                CloudlogFailure::Refused,
                format!("Cloudlog HTTP {status} — the station list was refused"),
            ))
        }
    }
    let parsed: serde_json::Value = serde_json::from_str(body).map_err(|_| not_a_list())?;
    let rows = parsed.as_array().ok_or_else(not_a_list)?;
    Ok(rows
        .iter()
        .map(|v| CloudlogStation {
            station_id: station_field(v, "station_id"),
            profile_name: station_field(v, "station_profile_name"),
            callsign: station_field(v, "station_callsign"),
            gridsquare: station_field(v, "station_gridsquare"),
            active: station_active(v),
        })
        // A row with no id cannot fill the field this picker exists to fill.
        .filter(|st| !st.station_id.is_empty())
        .collect())
}

/// GET the operator's station locations (#226). Sent where `dest` decided it may go, as
/// [`upload`] is, with no redirects and the same 20 s timeout. Runs ONLY on an explicit button
/// press — never on load and never on a timer — because it spends the API key on the wire.
pub fn fetch_station_info(
    dest: &Destination,
    key: &str,
) -> Result<Vec<CloudlogStation>, CloudlogError> {
    let url = station_info_url(&dest.base, key)?;
    let client = dest.client()?;
    let resp = client.get(&url).send().map_err(|e| {
        if e.is_builder() {
            // `https_only` refused a URL that is not https:// before any I/O (plain http:// was
            // decided by `Destination` and never gets here) — a Settings problem, and the one
            // that matters most here: this request's URL carries the key.
            CloudlogError::new(
                CloudlogFailure::NotConfigured,
                "Cloudlog/Wavelog: the instance URL must be https:// — a request carrying the \
                 API key is never sent in the clear",
            )
        } else {
            // `redact` classifies by error TYPE and never stringifies it, so the URL — and the
            // key inside it — cannot ride out in the message.
            CloudlogError::new(CloudlogFailure::Unreachable, neterr::redact("Cloudlog", &e))
        }
    })?;
    let status = resp.status().as_u16();
    let text = resp.text().unwrap_or_default();
    classify_station_info(status, &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::{SocketAddr, TcpListener};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// The destination for `base`, through the system resolver, as Settings would produce it.
    fn dest(base: &str) -> Destination {
        match Destination::check(base) {
            Ok(d) => d,
            Err(e) => panic!("{base} was refused: {}", e.message),
        }
    }

    /// #226's defect on the transport arm: every way of failing to reach the instance was
    /// flattened into one sentence blaming the URL.
    ///
    /// `neterr` exists because `is_connect()` is true for a DNS failure, a refused connect, an
    /// unreachable proxy AND a rejected TLS handshake — and the handshake case is the one that
    /// matters, since an HTTPS-inspecting antivirus re-signs with a CA Nexus does not carry
    /// (D#181). Telling that operator to check their URL sends them after the one thing that
    /// is right.
    #[test]
    fn a_transport_failure_says_what_actually_failed() {
        // A refused connect: bind a port, drop it, connect to nothing.
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        drop(l);
        let e = upload(
            &dest(&format!("https://127.0.0.1:{port}")),
            KEY,
            "3",
            "<eor>",
        )
        .unwrap_err();
        let err = e.message;
        assert!(
            err.contains("could not connect"),
            "an unreachable instance was blamed on the URL: {err}"
        );
        assert_eq!(
            e.class,
            CloudlogFailure::Unreachable,
            "a refused connect is not a credential or a record problem"
        );

        // A rejected TLS handshake: something answers the TCP connect but is not a peer we
        // accept. On the operator's machine that something is the antivirus's certificate.
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut sock, _)) = l.accept() {
                let _ = sock.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
                let _ = sock.flush();
                std::thread::sleep(Duration::from_millis(300));
            }
        });
        let e = upload(
            &dest(&format!("https://127.0.0.1:{port}")),
            KEY,
            "3",
            "<eor>",
        )
        .unwrap_err();
        let err = e.message;
        assert!(
            err.contains("antivirus"),
            "a rejected handshake was blamed on the URL: {err}"
        );
        assert_eq!(e.class, CloudlogFailure::Unreachable);
        // The classification is by type, so the message can never carry the request.
        assert!(!err.contains(KEY), "API key leaked into the message: {err}");

        // The control: the one case the old sentence was right about must keep its answer.
        // An http:// URL off the operator's own network is refused before any I/O (#378 takes
        // plain http only on it). Nothing was sent and the fix is in Settings, so its CLASS is
        // the not-configured one, not the network one.
        let e = Destination::check("http://203.0.113.7")
            .and_then(|d| upload(&d, KEY, "3", "<eor>"))
            .unwrap_err();
        let err = e.message;
        assert!(
            err.contains("https://"),
            "control: an http:// URL must still be told to use https: {err}"
        );
        assert_eq!(e.class, CloudlogFailure::NotConfigured);

        // …and the two Settings-side refusals, which never open a socket at all.
        assert_eq!(
            upload(&dest("https://log.example.invalid"), "  ", "3", "<eor>")
                .unwrap_err()
                .class,
            CloudlogFailure::NotConfigured,
            "an empty API key is a configuration problem, not a network one"
        );
        assert_eq!(
            upload(&dest("https://log.example.invalid"), KEY, " ", "<eor>")
                .unwrap_err()
                .class,
            CloudlogFailure::NotConfigured,
            "so is an empty station profile id"
        );
    }

    /// #226 (DG3ET): Wavelog's `station_profile_id` is the NUMBER of a station location owned
    /// by the key's user (Wavelog `Api.php` qso() → `Stations::check_station_against_user`). The
    /// reporter typed his callsign, Wavelog answered 401 "station id does not belong to the API
    /// key owner", and Nexus sent it — and kept sending it. A value that cannot be a location
    /// number is refused here, before anything leaves the machine, and says what is wanted.
    #[test]
    fn a_station_profile_id_that_is_not_a_number_is_refused_before_sending() {
        for id in ["DG3ET", "3a", "home", "-3", "3.0"] {
            let e = upload(&dest("https://log.example.invalid"), KEY, id, "<eor>").unwrap_err();
            assert_eq!(
                e.class,
                CloudlogFailure::NotConfigured,
                "{id:?} must be refused as a Settings problem, not sent: {}",
                e.message
            );
            assert!(
                e.message.contains("location number"),
                "{id:?}: say what is wanted — {}",
                e.message
            );
            assert!(
                !e.message.contains(KEY),
                "API key in the message: {}",
                e.message
            );
        }
        // Control: a numeric id (spaces trimmed, as Settings stores it) is NOT refused for its
        // shape — it goes on to the network, which for this unresolvable host is Unreachable.
        let e = upload(&dest("https://log.example.invalid"), KEY, " 3 ", "<eor>").unwrap_err();
        assert_eq!(e.class, CloudlogFailure::Unreachable, "{}", e.message);
    }

    #[test]
    fn api_url_tolerates_url_variants() {
        let want = "https://log.example.com/index.php/api/qso";
        assert_eq!(api_url("https://log.example.com"), want);
        assert_eq!(api_url("https://log.example.com/"), want);
        assert_eq!(api_url("https://log.example.com/index.php"), want);
        assert_eq!(api_url("https://log.example.com/index.php/api/qso"), want);
    }

    #[test]
    fn body_has_the_documented_shape_and_escapes() {
        let b = build_body("K3Y", "3", "<CALL:5>W1ABC \"x\" <EOR>");
        assert!(b.starts_with("{\"key\":\"K3Y\",\"station_profile_id\":\"3\",\"type\":\"adif\""));
        assert!(b.contains("W1ABC"));
        assert!(b.contains("\\\"x\\\""), "embedded quotes escaped: {b}");
    }

    const KEY: &str = "cl0udl0g-4pi-k3y-abcdef0123456789";

    /// #226: Wavelog's own answer shape — an array of station locations, every field a string.
    /// The operator picks one and Nexus fills the station profile id with its `station_id`.
    #[test]
    fn station_info_parses_a_wavelog_shaped_answer() {
        let body = r#"[
          {"station_id":"3","station_profile_name":"Home","station_callsign":"DG3ET",
           "station_gridsquare":"JO31NF","station_active":"1"},
          {"station_id":"7","station_profile_name":"Portable","station_callsign":"DG3ET/P",
           "station_gridsquare":"JN48","station_active":"0"}
        ]"#;
        let got = classify_station_info(200, body).expect("a station list");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].station_id, "3");
        assert_eq!(got[0].profile_name, "Home");
        assert_eq!(got[0].callsign, "DG3ET");
        assert_eq!(got[0].gridsquare, "JO31NF");
        assert!(got[0].active, "station_active 1 is active");
        assert_eq!(got[1].station_id, "7");
        assert!(!got[1].active, "station_active 0 is not");
        // An instance with no locations answers with an empty array — not an error.
        assert_eq!(
            classify_station_info(200, "[]").expect("empty list").len(),
            0
        );
    }

    /// #226: the three answers an operator has to tell apart — a refused or read-only key, a
    /// Cloudlog too old to have the endpoint (enter the number by hand), and the instance in
    /// trouble.
    #[test]
    fn station_info_tells_an_old_cloudlog_from_a_refused_key() {
        let e = classify_station_info(401, "").unwrap_err();
        assert_eq!(e.class, CloudlogFailure::Credentials);
        assert!(e.message.to_lowercase().contains("key"), "{}", e.message);
        let e = classify_station_info(404, "<html><title>Not Found</title></html>").unwrap_err();
        assert_eq!(e.class, CloudlogFailure::NotAnApi);
        assert!(e.message.contains("by hand"), "{}", e.message);
        assert_eq!(
            classify_station_info(500, "").unwrap_err().class,
            CloudlogFailure::ServerError
        );
    }

    /// ⛔ CREDENTIAL. station_info carries the API key IN THE URL PATH, so every string this
    /// path can produce is checked for it — and the check is proven able to trip.
    #[test]
    fn the_api_key_never_reaches_a_station_info_error_or_url_error() {
        // A port nothing listens on: a transport failure, the error most likely to quote a URL.
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        drop(l);
        let e = fetch_station_info(&dest(&format!("https://127.0.0.1:{port}")), KEY).unwrap_err();
        let runs = key_runs(KEY);
        assert!(
            !runs.is_empty(),
            "control: the key has readable runs to look for"
        );
        for r in &runs {
            assert!(
                !e.message.contains(r.as_str()),
                "API key ran into the error message ({r}): {}",
                e.message
            );
        }
        // POSITIVE CONTROL: the same check trips on a planted raw key, so a clean message above
        // is evidence rather than a detector that can never fire.
        let planted = format!("GET https://log.example.org/index.php/api/station_info/{KEY}");
        assert!(
            runs.iter().any(|r| planted.contains(r.as_str())),
            "the leak check cannot see a raw key — it proves nothing"
        );
    }

    /// #226: the key goes in a URL PATH, so anything that is not URL-path-safe is refused
    /// before a request is built — a key with a slash would otherwise change the endpoint.
    #[test]
    fn a_key_that_cannot_go_in_a_url_is_refused_before_sending() {
        for bad in [
            "key/with/slash",
            "key with space",
            "key?q=1",
            "key#frag",
            "",
        ] {
            let e = station_info_url("https://log.example.org", bad).unwrap_err();
            assert_eq!(e.class, CloudlogFailure::NotConfigured, "{bad:?}");
            assert!(!e.message.contains(bad) || bad.is_empty(), "{}", e.message);
        }
        // A real key builds the documented path, and the base handling is api_url's.
        let want = format!("https://log.example.org/index.php/api/station_info/{KEY}");
        for base in [
            "https://log.example.org",
            "https://log.example.org/",
            "https://log.example.org/index.php",
        ] {
            assert_eq!(station_info_url(base, KEY).expect(base), want, "{base}");
        }
    }

    #[test]
    fn an_auth_rejection_carries_the_servers_own_reason() {
        // #226: the same key works from GridTracker2 and WSJT-X-improved and fails against
        // two independent instances, because what the server actually rejected was NOT the
        // key. Collapsing every 401 into "check the API key" sends the operator back to the
        // one thing they have already checked, and Nexus has the answer in hand.
        let err = classify(
            401,
            r#"{"status":"failed","reason":"station_profile_id 7 is not linked to this API key"}"#,
            KEY,
        )
        .unwrap_err()
        .message;
        assert!(
            err.contains("station_profile_id 7 is not linked to this API key"),
            "the server's own reason must reach the operator: {err}"
        );
    }

    #[test]
    fn a_2xx_that_rejects_the_record_carries_the_reason_too() {
        // Cloudlog files the per-record verdict IN the body, so this is the other half of
        // the same defect: HTTP 200 and the QSO still did not land.
        let err = classify(
            200,
            r#"{"status":"failed","reason":"ADIF field BAND is missing"}"#,
            KEY,
        )
        .unwrap_err()
        .message;
        assert!(
            err.contains("ADIF field BAND is missing"),
            "a rejected record must say why: {err}"
        );
    }

    #[test]
    fn a_reason_that_echoes_the_api_key_never_reaches_the_message() {
        // The key rides in the REQUEST body, so a debug-mode PHP notice or a WAF page can
        // echo it straight back — and this string is persisted to conn-health.json and kept
        // in the connection log, so an echo would put the key on disk in cleartext.
        let body = format!(r#"{{"status":"failed","reason":"denied for key={KEY} (profile 7)"}}"#);
        let err = classify(403, &body, KEY).unwrap_err().message;
        // Positive control, and it has to be a word the OLD flattened message never used —
        // "rejected" would have passed against "auth rejected — check the API key" and this
        // test would have proved nothing.
        assert!(
            err.contains("(profile 7)"),
            "the reason must be surfaced: {err}"
        );
        assert!(!err.contains(KEY), "API key leaked into the message: {err}");
    }

    /// The key's own alphanumeric runs of 8 characters or more — here, `cl0udl0g` and
    /// `abcdef0123456789`.
    ///
    /// This is the leak detector, and it is deliberately NOT the implementation's notion of a
    /// match: re-encoding is something done to a key's *separators* (`-` becomes `&#45;`,
    /// `%2D`, `\u002d`), so its alphanumeric runs come through every encoder untouched. A
    /// message carrying one of these is a message a person can read the key out of, whatever
    /// escaping sits between the runs. Asking the question this way keeps the test from
    /// re-deriving the answer from the code under test.
    fn key_runs(key: &str) -> Vec<String> {
        key.split(|c: char| !c.is_alphanumeric())
            .filter(|r| r.chars().count() >= 8)
            .map(str::to_string)
            .collect()
    }

    /// ⛔ CREDENTIAL. One re-encoded character must not defeat the scrub.
    ///
    /// The key rides in the REQUEST body, so a debug-mode PHP notice or a WAF page can echo it
    /// straight back — and a server that echoes a request does not echo it byte for byte: it
    /// HTML-escapes what it prints, or percent-encodes it, or escapes it as `\uXXXX` in JSON.
    /// A literal `str::replace` catches none of those, and truncation is not the backstop it
    /// was claimed to be: [`REASON_MAX_CHARS`] is 160 and a Cloudlog key is 33, so a
    /// near-verbatim key fits with 120 characters to spare and lands in `conn-health.json`,
    /// which is world-readable and persisted.
    #[test]
    fn a_re_encoded_api_key_never_reaches_the_message() {
        let cases = [
            // Fully HTML-entity encoded separators — a PHP notice printing what it received.
            ("html entities", KEY.replace('-', "&#45;")),
            // Hex entities, the other spelling of the same thing.
            ("hex entities", KEY.replace('-', "&#x2D;")),
            // Percent-encoded — a WAF page echoing a URL-encoded body.
            ("percent escapes", KEY.replace('-', "%2D")),
            // JSON's own escape, which the body is already made of.
            ("json unicode escapes", KEY.replace('-', r"\u002d")),
            // PARTIALLY encoded: ONE character. The case the review named, and the one that
            // shows the defect is not about any particular encoder.
            ("one separator", KEY.replacen('-', "&#45;", 1)),
            // …and one encoded LETTER, which breaks a run as well as a separator.
            ("one letter", KEY.replacen('c', "&#99;", 1)),
            // A zero-width character wedged in: on screen this still reads as the key.
            ("a zero-width split", KEY.replacen('-', "-\u{200b}", 1)),
        ];
        for (what, encoded) in cases {
            let body =
                format!(r#"{{"status":"failed","reason":"denied for key={encoded} (profile 7)"}}"#);
            let runs = key_runs(&encoded);
            // The positive control, per case: there IS still readable key material in this
            // body. Without it an encoding that happened to destroy the key would read as a
            // pass, and the case would be proving nothing.
            assert!(
                !runs.is_empty() && runs.iter().all(|r| body.contains(r)),
                "control ({what}): no readable key material left in the body to leak"
            );
            let err = classify(403, &body, KEY).unwrap_err().message;
            for r in &runs {
                assert!(
                    !err.contains(r.as_str()),
                    "API key survived {what} into the message ({r}): {err}"
                );
            }
        }
    }

    #[test]
    fn a_body_that_never_carried_the_key_still_says_what_the_server_said() {
        // The control for the test above: failing closed must not mean failing silent. Same
        // shape of body, no key in it — the server's words must still reach the operator, or
        // "the key never leaks" would be satisfied by never showing anything.
        let err = classify(
            403,
            r#"{"status":"failed","reason":"station_profile_id 7 is not linked to this API key"}"#,
            KEY,
        )
        .unwrap_err()
        .message;
        assert!(
            err.contains("station_profile_id 7 is not linked"),
            "the reason was suppressed although the key was never in it: {err}"
        );
    }

    #[test]
    fn an_enormous_or_hostile_body_is_bounded_and_flattened_to_one_line() {
        // A reverse proxy answers with an HTML page, not a Cloudlog reason. Showing a slice
        // of it is useful (it tells the operator the URL reached a proxy), showing all of it
        // is not: the message is written to conn-health.json on every change.
        let hostile = format!(
            "<!DOCTYPE html>\n<html><head><title>502 Bad Gateway</title></head>\n{}",
            "A".repeat(50_000)
        );
        let err = classify(502, &hostile, KEY).unwrap_err().message;
        assert!(
            err.contains("502 Bad Gateway"),
            "the useful head of the page must survive: {err}"
        );
        // Measured at 201 characters for this 50 000-character page. Asserted tightly, so a
        // longer prefix or a raised REASON_MAX_CHARS has to be a deliberate edit here rather
        // than silent drift.
        assert!(
            err.chars().count() <= 201,
            "unbounded body reached the message ({} chars)",
            err.chars().count()
        );
        assert!(
            !err.contains('\n') && !err.contains('\r'),
            "the message must stay one line: {err}"
        );
    }

    #[test]
    fn an_invisible_character_cannot_rewrite_what_the_operator_reads() {
        // `char::is_control` covers C0/C1 and DEL and stops there — it does not cover Unicode
        // Cf. U+202E RIGHT-TO-LEFT OVERRIDE visually reverses everything after it, so a
        // server-supplied string can rewrite the rest of the failure detail on the panel row;
        // U+200B and U+FEFF are simply invisible. All three reached the row and the persisted
        // conn-health.json.
        let hostile = "profile 7 \u{202e}denied\u{200b} for \u{feff}this key\u{00ad}";
        let body = format!(r#"{{"status":"failed","reason":"{hostile}"}}"#);
        let err = classify(403, &body, KEY).unwrap_err().message;
        for (name, c) in [
            ("U+202E right-to-left override", '\u{202e}'),
            ("U+200B zero-width space", '\u{200b}'),
            ("U+FEFF byte-order mark", '\u{feff}'),
            ("U+00AD soft hyphen", '\u{00ad}'),
        ] {
            // The control: the character really is in the body, so a filter that did nothing
            // could not pass by accident.
            assert!(body.contains(c), "control: {name} is not in the body");
            assert!(!err.contains(c), "{name} reached the operator: {err:?}");
        }
        // …and the words the operator needs are still there. Stripping everything would
        // satisfy the assertions above and tell them nothing.
        assert!(
            err.contains("profile 7") && err.contains("denied"),
            "the reason was destroyed rather than cleaned: {err}"
        );
    }

    /// ⛔ THE PERSISTED CLOUDLOG ROW HAS TO SAY *WHICH* FAILURE.
    ///
    /// `conn-health.json` may hold only sentences Nexus wrote (`note_conn_health`), and this
    /// module's answer to that rule was one sentence for every failure — so after a restart
    /// the panel could not tell a station-profile id that is not linked to the key from a URL
    /// that is not a Cloudlog instance from an HTTP 500. HRDLog and WRL keep per-class
    /// sentences because their services answer with a closed result set; Cloudlog's classes
    /// are just as closed, they were only never named.
    ///
    /// So `classify` returns the CLASS beside the instance's words. The class is the half the
    /// row persists; the words are the half that is read once and dropped.
    #[test]
    fn each_way_a_cloudlog_upload_can_fail_is_its_own_class() {
        // The three the reporter could not tell apart after a restart, plus the two the
        // transport arm already distinguished.
        let cases = [
            (
                "a station profile not linked to the key",
                classify(
                    200,
                    r#"{"status":"failed","reason":"station_profile_id 7 is not linked"}"#,
                    KEY,
                ),
                CloudlogFailure::RecordRefused,
            ),
            (
                "a URL that is not a Cloudlog API",
                classify(404, "<html><title>Not Found</title></html>", KEY),
                CloudlogFailure::NotAnApi,
            ),
            (
                "the instance itself in trouble",
                classify(500, "", KEY),
                CloudlogFailure::ServerError,
            ),
            (
                "a rejected key",
                classify(403, "", KEY),
                CloudlogFailure::Credentials,
            ),
            (
                "a status with no class of its own",
                classify(418, "", KEY),
                CloudlogFailure::Refused,
            ),
        ];
        let mut seen: Vec<CloudlogFailure> = Vec::new();
        for (what, got, want) in cases {
            let e = got.expect_err(what);
            assert_eq!(e.class, want, "{what} was classified as {:?}", e.class);
            // The class is not a replacement for the instance's words — the toast still
            // carries them, and #226 is precisely that Nexus threw them away.
            assert!(
                !e.message.is_empty(),
                "{what}: the operator gets no message"
            );
            seen.push(e.class);
        }
        // The point of the test, and the thing one collapsed sentence failed: the classes
        // are DISTINCT. Without this a `classify` that returned `Refused` for everything
        // would satisfy every assertion above.
        for (i, a) in seen.iter().enumerate() {
            for b in &seen[i + 1..] {
                assert_ne!(a, b, "two different failures share one class: {a:?}");
            }
        }
        // Every class is reachable, so the caller's persisted-sentence table cannot carry a
        // dead arm — and a class added here without a case above trips this.
        assert_eq!(
            CloudlogFailure::ALL.len(),
            seen.len() + 2,
            "CloudlogFailure::ALL and this test's coverage drifted (NotConfigured and \
             Unreachable are covered by a_transport_failure_says_what_actually_failed)"
        );
    }

    #[test]
    fn a_server_that_says_nothing_still_gets_the_old_actionable_guess() {
        // The control for the three above: when there is no reason to surface, the message
        // must not become emptier than it was.
        let err = classify(401, "", KEY).unwrap_err().message;
        assert!(
            err.contains("API key"),
            "no-reason fallback lost its hint: {err}"
        );
        let err = classify(500, "", KEY).unwrap_err().message;
        assert!(err.contains("500"), "the status must still be named: {err}");
    }

    /// Wavelog's refusal of a QSO its import would not file, as `Api.php` `qso()` writes it
    /// (read 2026-10-04): HTTP 400, `"status":"abort"`, and the reason in a `messages` ARRAY —
    /// an empty first entry, then `Logbook_model::import_bulk`'s HTML. PHP's `json_encode`
    /// escapes every `/`, so `</b>` arrives as `<\/b>`. Nexus read only `reason`, `message`
    /// and `error`, so this arrived as "refused the upload, and said no more".
    fn wavelog_abort(reason_html: &str) -> String {
        format!(
            r#"{{"status":"abort","type":"adif","string":"","adif_count":1,"adif_errors":1,"messages":["","{reason_html}"]}}"#
        )
    }

    #[test]
    fn a_wavelog_import_refusal_says_what_it_refused() {
        // A station location whose callsign is not the one on the QSO: Wavelog's import skips
        // it as a critical error.
        let body = wavelog_abort(
            r"Differing station callsign <b>N0CALL<\/b> while importing QSO with DL1ABC for <b>PRACTICE<\/b>: SKIPPED<br>",
        );
        let e = classify(400, &body, KEY).unwrap_err();
        assert_eq!(e.class, CloudlogFailure::Refused, "{}", e.message);
        assert!(
            e.message.contains(
                "Differing station callsign N0CALL while importing QSO with DL1ABC for PRACTICE"
            ),
            "Wavelog's reason must reach the operator, without its markup: {}",
            e.message
        );
        assert!(
            !e.message.contains('<') && !e.message.contains('>'),
            "HTML tags reached the operator: {}",
            e.message
        );
        assert!(
            !e.message.contains("said no more"),
            "the instance did say more: {}",
            e.message
        );
    }

    #[test]
    fn a_wavelog_duplicate_says_it_is_a_duplicate() {
        // The other refusal an operator meets every day: the QSO is already in the log.
        let body = wavelog_abort(
            r"Date\/Time: 2026-10-04 12:34:00 Callsign: DL1ABC Band: 20m Duplicate for N0CALL<br>",
        );
        let e = classify(400, &body, KEY).unwrap_err();
        assert!(
            e.message.contains(
                "Date/Time: 2026-10-04 12:34:00 Callsign: DL1ABC Band: 20m Duplicate for N0CALL"
            ),
            "{}",
            e.message
        );
        assert!(!e.message.contains("<br>"), "{}", e.message);
    }

    /// ⛔ CREDENTIAL. Reading `messages` must not open a way round the echo check: a key the
    /// instance echoes inside it is withheld exactly as one inside `reason` is.
    #[test]
    fn an_api_key_echoed_inside_messages_still_fails_closed() {
        let cases = [
            ("verbatim", KEY.to_string()),
            ("json unicode escapes", KEY.replace('-', r"\u002d")),
            ("html entities", KEY.replace('-', "&#45;")),
            // Split by markup, which `messages` is read with turned into spaces.
            ("split by tags", KEY.replace('-', r"<b>-<\/b>")),
        ];
        for (what, echoed) in cases {
            let body = wavelog_abort(&format!(
                r"Differing station callsign <b>N0CALL<\/b> for key {echoed}<br>"
            ));
            // The positive control: readable key material really is in the body.
            let runs = key_runs(KEY);
            assert!(
                !runs.is_empty() && runs.iter().all(|r| body.contains(r.as_str())),
                "control ({what}): no key material in the body to leak"
            );
            let e = classify(400, &body, KEY).unwrap_err();
            for r in &runs {
                assert!(
                    !e.message.contains(r.as_str()),
                    "API key survived {what} into the message ({r}): {}",
                    e.message
                );
            }
            if what != "verbatim" {
                // A verbatim echo is replaced in place; an encoded one withholds everything.
                assert!(
                    e.message.contains(KEY_ECHOED),
                    "{what}: the body was not withheld: {}",
                    e.message
                );
            }
        }
    }

    #[test]
    fn reason_is_preferred_over_messages() {
        // Wavelog's earlier refusals (a bad key, a station id that is not the key owner's) carry
        // `reason`; when both are present it is the one written for the client.
        let body = r#"{"status":"failed","reason":"station id does not belong to the API key owner.","messages":["","something else"]}"#;
        let err = classify(401, body, KEY).unwrap_err().message;
        assert!(err.contains("station id does not belong"), "{err}");
        assert!(!err.contains("something else"), "{err}");
    }

    #[test]
    fn a_failure_carries_the_http_status_it_was_answered_with() {
        let body = wavelog_abort(r"Duplicate for N0CALL<br>");
        assert_eq!(classify(400, &body, KEY).unwrap_err().status, Some(400));
        let refused_record = r#"{"status":"failed","reason":"ADIF field BAND is missing"}"#;
        assert_eq!(
            classify(200, refused_record, KEY).unwrap_err().status,
            Some(200)
        );
        assert_eq!(classify(301, "", KEY).unwrap_err().status, Some(301));
        assert_eq!(classify(503, "", KEY).unwrap_err().status, Some(503));
        // Nothing was sent, so nothing answered.
        let unsent = upload(&dest("https://log.example.org"), "", "7", "<EOR>").unwrap_err();
        assert_eq!(unsent.class, CloudlogFailure::NotConfigured);
        assert_eq!(unsent.status, None);
    }

    fn location(callsign: &str, gridsquare: &str) -> CloudlogStation {
        CloudlogStation {
            station_id: "11".to_string(),
            profile_name: "26PRACTICE".to_string(),
            callsign: callsign.to_string(),
            gridsquare: gridsquare.to_string(),
            active: true,
        }
    }

    /// The backend half of `ui/src/settings/cloudlogLocation.ts` — the same cases, the same
    /// answers.
    #[test]
    fn a_station_location_that_is_not_the_qso_is_a_mismatch() {
        let both = LocationMismatch {
            callsign: true,
            grid: true,
        };
        let none = LocationMismatch::default();
        assert_eq!(
            location_mismatch(&location("PRACTICE", "DM42"), "N0CALL", "DM41"),
            both
        );
        // The control: the comparison can also say no, whatever the case or spacing.
        assert_eq!(
            location_mismatch(&location(" n0call ", "dm41ab"), "N0CALL", "DM41"),
            none
        );
        assert!(!location_mismatch(&location("N0CALL", "DM41XY"), "N0CALL", "DM41AB").grid);
        assert!(location_mismatch(&location("N0CALL", "DM42AB"), "N0CALL", "DM41AB").grid);
        assert!(location_mismatch(&location("N0CALL/P", "DM41"), "N0CALL", "DM41").callsign);
        assert!(!location_mismatch(&location("N0CALL", "DM41,DM42"), "N0CALL", "DM42").grid);
        assert!(location_mismatch(&location("N0CALL", "DM41,DM42"), "N0CALL", "DM43").grid);
        // A QSO that carries no STATION_CALLSIGN or MY_GRIDSQUARE is filled from the location
        // by Wavelog, so there is nothing to disagree with.
        assert_eq!(
            location_mismatch(&location("PRACTICE", "DM42"), "", ""),
            none
        );
        assert_eq!(
            location_mismatch(&location("", "DM"), "N0CALL", "DM41"),
            none
        );
    }

    #[test]
    fn a_refusal_names_the_location_mismatch_it_found() {
        let note = location_mismatch_note(&location("PRACTICE", "DM42"), "N0CALL", "DM41")
            .expect("a location that is not the QSO is named");
        for want in ["PRACTICE", "N0CALL", "DM42", "DM41"] {
            assert!(note.contains(want), "{want} is not named: {note}");
        }
        let call_only = location_mismatch_note(&location("PRACTICE", "DM41"), "N0CALL", "")
            .expect("a callsign mismatch alone is named");
        assert!(
            call_only.contains("PRACTICE") && !call_only.contains("grid"),
            "{call_only}"
        );
        // The control: a location that is the QSO says nothing.
        assert_eq!(
            location_mismatch_note(&location("N0CALL", "DM41AB"), "N0CALL", "DM41"),
            None
        );
        // The location's values are the instance's own: one that is not shaped like a callsign
        // or a grid is not repeated, and the instance's own refusal stands alone.
        for odd in [
            "PRACTICE\u{202e}X",
            "A-VERY-LONG-NAME-THAT-IS-NO-CALLSIGN",
            "PRAC TICE",
        ] {
            assert_eq!(
                location_mismatch_note(&location(odd, "DM42"), "N0CALL", "DM41"),
                None,
                "{odd:?} was repeated to the operator"
            );
        }
    }

    // ---- #378: plain http, and only on the operator's own network ----------------------------

    /// A stand-in resolver that answers `addrs` to every question and counts the questions.
    fn lookup_answering(
        addrs: Vec<SocketAddr>,
    ) -> (
        impl Fn(&str, u16) -> std::io::Result<Vec<SocketAddr>>,
        Arc<AtomicUsize>,
    ) {
        let asked = Arc::new(AtomicUsize::new(0));
        let n = asked.clone();
        let lookup = move |_host: &str, _port: u16| {
            n.fetch_add(1, Ordering::SeqCst);
            Ok(addrs.clone())
        };
        (lookup, asked)
    }

    /// One HTTP response, `Content-Length` and `Connection: close` filled in.
    fn answer(status: &str, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    const CREATED: &str = r#"{"status":"created"}"#;

    /// One HTTP request off a socket: its head, and as much body as its Content-Length says.
    fn read_request(sock: &mut std::net::TcpStream) -> String {
        use std::io::Read;
        let _ = sock.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match sock.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
                let len = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if buf.len() >= end + 4 + len {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// A plain-http instance on loopback that answers ONE request with `response`, verbatim,
    /// and hands back what it received.
    fn http_once(response: String) -> (u16, std::sync::mpsc::Receiver<String>) {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Ok((mut sock, _)) = l.accept() {
                let got = read_request(&mut sock);
                let _ = sock.write_all(response.as_bytes());
                let _ = sock.flush();
                let _ = tx.send(got);
            }
        });
        (port, rx)
    }

    /// A loopback listener that only counts the connections it gets, until `stop` is set.
    fn counting_listener() -> (u16, Arc<AtomicUsize>, Arc<AtomicBool>) {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        l.set_nonblocking(true).expect("nonblocking");
        let port = l.local_addr().expect("addr").port();
        let hits = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (h, s) = (hits.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::SeqCst) {
                match l.accept() {
                    Ok(_) => {
                        h.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        });
        (port, hits, stop)
    }

    /// #378 (IZ5FSA): a Wavelog on the LAN, with no certificate, reached over plain http. The
    /// upload and the station-location lookup both go through, key and all, to the instance
    /// there.
    #[test]
    fn a_plain_http_request_to_a_lan_address_is_sent() {
        let (port, got) = http_once(answer(
            "200 OK",
            "Content-Type: application/json\r\n",
            CREATED,
        ));
        let r = upload(
            &dest(&format!("http://127.0.0.1:{port}")),
            KEY,
            "3",
            "<eor>",
        );
        let req = got.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
        assert!(
            req.starts_with("POST /index.php/api/qso "),
            "the upload never reached the LAN instance ({:?}): {req:?}",
            r.as_ref().err().map(|e| e.message.clone())
        );
        assert!(
            req.contains(KEY),
            "the key rides in the body, as over https"
        );
        if let Err(e) = r {
            panic!(
                "the instance filed the QSO, and the upload says: {}",
                e.message
            );
        }

        let (port, got) = http_once(answer("200 OK", "Content-Type: application/json\r\n", "[]"));
        let list = fetch_station_info(&dest(&format!("http://127.0.0.1:{port}")), KEY);
        let req = got.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
        assert!(
            req.starts_with(&format!("GET /index.php/api/station_info/{KEY} ")),
            "the station lookup never reached the LAN instance: {req:?}"
        );
        match list {
            Ok(l) => assert!(l.is_empty(), "{} stations from an empty list", l.len()),
            Err(e) => panic!("an empty station list is not an error: {}", e.message),
        }
    }

    /// #378: the literal addresses that are the operator's own network. The parser's own
    /// normalisation decides what a literal is (`0x7f.1` IS 127.0.0.1, to reqwest as to this
    /// check), and a literal is never looked up.
    #[test]
    fn plain_http_is_accepted_for_an_address_on_the_operators_own_network() {
        let (lookup, asked) = lookup_answering(Vec::new());
        for host in [
            "127.0.0.1",
            "127.255.255.254",
            "0x7f.1",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.20",
            "169.254.10.20",
            "[::1]",
            "[fd00::20]",
            "[fe80::1]",
            "[::ffff:192.168.1.20]",
        ] {
            match Destination::check_with(&format!("http://{host}:8086/wavelog"), &lookup) {
                Ok(d) => assert!(
                    d.cleartext_to().is_some(),
                    "{host}: accepted, so the log must be able to say where the key goes"
                ),
                Err(e) => panic!("{host} is on the operator's own network: {}", e.message),
            }
        }
        assert_eq!(
            Destination::check_with("http://192.168.1.20/", &lookup)
                .ok()
                .and_then(|d| d.cleartext_to())
                .as_deref(),
            Some("192.168.1.20"),
            "a literal address is named as it is"
        );
        assert_eq!(
            asked.load(Ordering::SeqCst),
            0,
            "a literal address was looked up"
        );
    }

    /// #378, and the half that was true before it: plain http to anything off the operator's
    /// own network is refused before a socket is opened, and says how to fix it. Through the
    /// whole send, so the assertion holds for the code before #378 (which refused at the client)
    /// as for the code after it (which refuses at the check).
    #[test]
    fn plain_http_to_an_address_off_the_network_is_refused_as_before() {
        let (lookup, asked) = lookup_answering(Vec::new());
        let send = |url: &str| {
            Destination::check_with(url, &lookup).and_then(|d| upload(&d, KEY, "3", "<eor>"))
        };
        for host in [
            "203.0.113.7",
            "8.8.8.8",
            "100.64.0.1",
            "0.0.0.0",
            "172.32.0.1",
            "192.169.1.1",
            "224.0.0.1",
            "255.255.255.255",
            "[2001:db8::20]",
            "[::ffff:8.8.8.8]",
            "[::]",
        ] {
            let e = send(&format!("http://{host}/"))
                .err()
                .unwrap_or_else(|| panic!("{host} is not on the operator's own network"));
            assert_eq!(
                e.class,
                CloudlogFailure::NotConfigured,
                "{host}: {}",
                e.message
            );
            assert!(
                e.message.contains("https://"),
                "{host}: say what is wanted — {}",
                e.message
            );
        }
        assert_eq!(
            asked.load(Ordering::SeqCst),
            0,
            "a literal address was looked up"
        );
    }

    /// #378: a host NAME is accepted only if EVERY address it resolves to is on the network.
    /// Nothing here chooses which address a connection uses, so one address outside is enough
    /// to refuse — and a name that cannot be looked up cannot be checked.
    #[test]
    fn a_name_is_accepted_only_if_every_address_it_resolves_to_is_on_the_network() {
        let sa = |s: &str| s.parse::<SocketAddr>().expect(s);
        let (lookup, asked) = lookup_answering(vec![sa("192.168.1.20:80"), sa("[fd00::20]:80")]);
        let d = Destination::check_with("http://wavelog.lan", &lookup)
            .expect("a name whose addresses are all on the network");
        assert_eq!(
            d.cleartext_to().as_deref(),
            Some("wavelog.lan (192.168.1.20, fd00::20)"),
            "the log names the host and the addresses it was checked at"
        );
        assert_eq!(
            asked.load(Ordering::SeqCst),
            1,
            "looked up once, for the check"
        );

        for (what, addrs, named) in [
            (
                "one global address among private ones",
                vec![sa("192.168.1.20:80"), sa("[2001:db8::20]:80")],
                "2001:db8::20",
            ),
            (
                "a public address",
                vec![sa("93.184.216.34:80")],
                "93.184.216.34",
            ),
            ("no address at all", Vec::new(), "no address"),
        ] {
            let (lookup, _) = lookup_answering(addrs);
            let e = Destination::check_with("http://wavelog.lan", &lookup)
                .err()
                .unwrap_or_else(|| panic!("{what}: accepted"));
            assert_eq!(e.class, CloudlogFailure::NotConfigured, "{what}");
            assert!(e.message.contains(named), "{what}: name it — {}", e.message);
        }

        let failing = |_: &str, _: u16| -> std::io::Result<Vec<SocketAddr>> {
            Err(std::io::Error::other("x"))
        };
        let e = Destination::check_with("http://wavelog.lan", &failing)
            .err()
            .expect("a name that cannot be looked up cannot be checked");
        assert!(
            e.message.contains("could not be looked up"),
            "{}",
            e.message
        );

        // The real resolver, on a name that cannot exist (RFC 6761 `.invalid`): refused, by
        // whichever of the two ways this box's DNS answers it.
        let e = Destination::check("http://nexus-n35.invalid")
            .err()
            .expect("a name that resolves nowhere on the network is refused");
        assert_eq!(e.class, CloudlogFailure::NotConfigured, "{}", e.message);
    }

    /// #378, DNS rebinding. A name is checked, then sent to; if anything resolved it AGAIN in
    /// between, a name re-pointed in that window would take the key with it. The request must go
    /// to the address the check saw, still asking for the instance by name.
    #[test]
    fn a_lan_name_is_pinned_to_the_address_the_check_saw() {
        use std::net::ToSocketAddrs;
        let name = "nexus-n35-lan.test";
        // Positive control: this box cannot resolve the name (RFC 6761 `.test`), so a request
        // that reaches the instance can only have gone to the pinned address. If it could, a
        // lookup at send time would look exactly like the pin, and this test would prove
        // nothing.
        assert!(
            (name, 80)
                .to_socket_addrs()
                .map(|mut a| a.next().is_none())
                .unwrap_or(true),
            "control: {name} resolves on this box, so a pin cannot be told from a lookup"
        );
        let (port, got) = http_once(answer(
            "200 OK",
            "Content-Type: application/json\r\n",
            CREATED,
        ));
        // A rebinding name: loopback for the check, a public address for anything after it.
        let asked = Arc::new(AtomicUsize::new(0));
        let n = asked.clone();
        let rebinding = move |_: &str, p: u16| -> std::io::Result<Vec<SocketAddr>> {
            let first = n.fetch_add(1, Ordering::SeqCst) == 0;
            Ok(vec![if first {
                SocketAddr::from(([127, 0, 0, 1], p))
            } else {
                SocketAddr::from(([93, 184, 216, 34], p))
            }])
        };
        let d = Destination::check_with(&format!("http://{name}:{port}"), &rebinding)
            .expect("checked at loopback");
        let r = upload(&d, KEY, "3", "<eor>");
        let req = got.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
        assert!(
            req.contains(KEY),
            "the upload never reached the checked address ({:?})",
            r.as_ref().err().map(|e| e.message.clone())
        );
        assert!(
            req.to_ascii_lowercase()
                .contains(&format!("host: {name}:{port}")),
            "the instance must still be asked for by name (a virtual host needs it): {req:?}"
        );
        assert!(r.is_ok(), "{:?}", r.err().map(|e| e.message));
        assert_eq!(
            asked.load(Ordering::SeqCst),
            1,
            "the name was looked up again after the check"
        );
    }

    /// #378: a redirect is never followed with the key. Here it points off the checked host,
    /// with a 307, which keeps the method AND the body, so a client that followed it would hand
    /// the key to another host.
    #[test]
    fn a_redirect_off_the_lan_host_is_not_followed() {
        let (elsewhere, hits, stop) = counting_listener();
        let (port, got) = http_once(answer(
            "307 Temporary Redirect",
            &format!("Location: http://localhost:{elsewhere}/index.php/api/qso\r\n"),
            "",
        ));
        let r = upload(
            &dest(&format!("http://127.0.0.1:{port}")),
            KEY,
            "3",
            "<eor>",
        );
        let req = got.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
        std::thread::sleep(Duration::from_millis(300));
        stop.store(true, Ordering::SeqCst);
        // Positive control: the upload went to the LAN instance, so what this tests is the
        // redirect, not a refusal of plain http.
        assert!(
            req.contains(KEY),
            "the upload never reached the LAN instance: {req:?}"
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "the redirect was followed: the key went on to a second host"
        );
        let e = r.expect_err("a redirect is not a filed QSO");
        assert_eq!(e.class, CloudlogFailure::Refused, "{}", e.message);
        assert!(
            e.message.contains("redirect"),
            "say it was a redirect: {}",
            e.message
        );
    }

    /// #378: an `HTTP_PROXY` in the environment never sees a LAN upload. A proxy would carry
    /// the key off the network, and would resolve a pinned name for itself.
    ///
    /// The environment belongs to the whole process, so the upload runs in a child: this test
    /// binary, filtered to this one test, with `HTTP_PROXY` pointing at a listener here. The
    /// child also makes one request through a default client, which DOES honour the proxy: the
    /// positive control, without which a proxy nobody ever used would pass this test.
    #[test]
    fn a_lan_upload_never_goes_through_a_proxy() {
        const CHILD: &str = "NEXUS_TEST_CLOUDLOG_PROXY_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let (port, got) = http_once(answer(
                "200 OK",
                "Content-Type: application/json\r\n",
                CREATED,
            ));
            let r = upload(
                &dest(&format!("http://127.0.0.1:{port}")),
                KEY,
                "3",
                "<eor>",
            );
            let req = got.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
            assert!(
                req.contains(KEY),
                "the upload did not go to the instance directly ({:?})",
                r.err().map(|e| e.message)
            );
            // The control request: the proxy listener must see exactly this one.
            let _ = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("client")
                .get("http://127.0.0.1:9/")
                .send();
            return;
        }
        let (proxy, hits, stop) = counting_listener();
        let me = format!(
            "{}::a_lan_upload_never_goes_through_a_proxy",
            module_path!().split_once("::").map_or("", |(_, rest)| rest)
        );
        let via = format!("http://127.0.0.1:{proxy}");
        let out = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([me.as_str(), "--exact", "--test-threads=1"])
            .env(CHILD, "1")
            .env("HTTP_PROXY", &via)
            .env("http_proxy", &via)
            .env_remove("NO_PROXY")
            .env_remove("no_proxy")
            .output()
            .expect("run the child");
        std::thread::sleep(Duration::from_millis(200));
        stop.store(true, Ordering::SeqCst);
        let shown = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            shown.contains("running 1 test"),
            "control: the child ran no test, so it proved nothing:\n{shown}"
        );
        assert!(out.status.success(), "the child failed:\n{shown}");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "the proxy must see the control request and nothing else (2 means the LAN upload \
             went through it; 0 means the control never reached it)"
        );
    }

    /// #378 leaves https alone: no lookup ahead of the request, no pin, nothing said about
    /// cleartext. Anything that is neither https:// nor plain http:// still gets the https-only
    /// refusal it always did.
    #[test]
    fn https_is_decided_exactly_as_before() {
        let (lookup, asked) = lookup_answering(Vec::new());
        for url in [
            "https://log.example.org",
            "HTTPS://Log.Example.org/index.php",
            "https://192.168.1.20",
        ] {
            let d = Destination::check_with(url, &lookup)
                .ok()
                .unwrap_or_else(|| panic!("{url} is refused"));
            assert!(d.cleartext_to().is_none(), "{url} is not plain http");
        }
        assert_eq!(
            asked.load(Ordering::SeqCst),
            0,
            "an https destination was looked up ahead of the request"
        );
        let e =
            upload(&dest("ftp://127.0.0.1:21"), KEY, "3", "<eor>").expect_err("ftp:// is not sent");
        assert_eq!(e.class, CloudlogFailure::NotConfigured, "{}", e.message);
        assert!(e.message.contains("https://"), "{}", e.message);
    }

    /// #378: a lookup that never answers is given up on, so a hung resolver cannot hold the
    /// upload worker the way reqwest's own timeout would not let it.
    #[test]
    fn a_lookup_that_never_answers_is_given_up_on() {
        let t0 = std::time::Instant::now();
        let r = within(Duration::from_millis(100), || {
            std::thread::sleep(Duration::from_secs(3));
            Ok(Vec::new())
        });
        assert_eq!(
            r.err().map(|e| e.kind()),
            Some(std::io::ErrorKind::TimedOut)
        );
        assert!(
            t0.elapsed() < Duration::from_secs(2),
            "waited {:?}",
            t0.elapsed()
        );
        // Control: an answer inside the deadline is the answer.
        let ten = SocketAddr::from(([10, 0, 0, 1], 80));
        let r = within(Duration::from_secs(2), move || Ok(vec![ten]));
        assert_eq!(r.ok(), Some(vec![ten]));
    }
}
