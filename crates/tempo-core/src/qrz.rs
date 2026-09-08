//! Pure QRZ.com XML callsign-lookup helpers — the offline, unit-testable core of
//! the QRZ enrichment connector (mirrors [`crate::lotw`]/[`crate::eqsl`]). No I/O.
//!
//! QRZ's XML API is a **two-step session-key** flow (unlike LoTW/eQSL's
//! per-request credentials): log in once with the account username+password to
//! get an opaque session `<Key>`, then look up callsigns with `?s=<key>`. This
//! module builds both URLs, parses the `<Session>` block (key / quota / errors)
//! and the `<Callsign>` data block, and detects session expiry. The thin HTTPS
//! transport lives behind the `live` feature elsewhere.
//!
//! ⚠️ Both the login URL (password) and the lookup URL (session key) are
//! secret-bearing; the transport must redact errors. The session `<Key>` is a
//! bearer secret, so [`QrzSession`]'s `Debug` redacts it (and [`QrzLogin`]'s
//! redacts the password).
//!
//! ⚠️ Subscription reality: a FREE QRZ account's XML returns only name/address/
//! country — **grid and state are subscriber-only**. So `grid`/`state` are
//! `Option` and routinely `None` for non-subscribers (the expected case).

/// The QRZ XML endpoint. Host + scheme are a hard-coded https constant so the
/// secret-bearing query strings can only ever go to QRZ over TLS.
pub const QRZ_XML_URL: &str = "https://xmldata.qrz.com/xml/current/";

/// Login inputs (exchanged once for a session key). `Debug` redacts the password.
#[derive(Clone)]
pub struct QrzLogin {
    pub username: String,
    pub password: String,
    /// Product identifier sent as `agent=` (e.g. "nexus/0.1"); aids QRZ support.
    pub agent: String,
}

impl std::fmt::Debug for QrzLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QrzLogin")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("agent", &self.agent)
            .finish()
    }
}

/// The parsed `<Session>` block. `key == None` ⇒ no valid session ⇒ the caller
/// must (re)login. `Debug` redacts the key (a bearer secret).
#[derive(Clone, Default, PartialEq)]
pub struct QrzSession {
    pub key: Option<String>,
    /// Subscription expiry, or the literal `non-subscriber`.
    pub sub_exp: Option<String>,
    /// Lookups used in the current 24 h period.
    pub count: Option<u32>,
    pub message: Option<String>,
    pub error: Option<String>,
}

impl std::fmt::Debug for QrzSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QrzSession")
            .field("key", &self.key.as_ref().map(|_| "<redacted>"))
            .field("sub_exp", &self.sub_exp)
            .field("count", &self.count)
            .field("message", &self.message)
            .field("error", &self.error)
            .finish()
    }
}

impl QrzSession {
    /// True iff QRZ reported the session expired/invalid (no key, or an explicit
    /// timeout error) — the caller should re-login and retry once.
    pub fn needs_login(&self) -> bool {
        // Any session-level error (Timeout / "Invalid session key" / "Session does
        // not exist") means re-login; a lookup-level "Not found" does NOT (no
        // "session"), so it won't trigger a needless re-login.
        self.key.is_none()
            || self
                .error
                .as_deref()
                .is_some_and(|e| e.to_ascii_lowercase().contains("session"))
    }

    // ⛔ THERE IS DELIBERATELY NO `holds_no_record` HERE, AND THERE CANNOT BE ONE.
    //
    // QRZ delivers an authoritative miss and a REFUSAL in the same shape: an `<Error>`
    // beside a live `<Key>`, with no `<Callsign>`. Nothing structural separates them, so
    // four rounds of readers each tried to separate them by WORDING — `Ok(None)`,
    // `NotFound`, `contains("not found")`, then a prefix anchored at `not found` followed
    // by end-of-string or a colon. The fifth costume broke the anchored one too: QRZ's own
    // *"Not found: your subscription does not cover this record"* begins exactly like a miss
    // and is a refusal, and reading it as a miss stamps the Connections row GREEN — which
    // does not merely fail to warn, it CLEARS an existing red (#245).
    //
    // A predicate that has been wrong in five different disguises is not a predicate that
    // needs a sixth wording; it is a question the data cannot answer. So the honest answer
    // is that this module does not answer it, and the caller fails closed: an answer with no
    // record is not a verified lookup, whatever it says. See `QrzOutcome::NoRecord` in
    // `src-tauri/src/lib.rs`.
}

/// A parsed QRZ callsign record. **Pure** (no serde — the serde DTO lives in
/// tempo-app, mirroring `ReconcileSummary`→`LotwSyncResult`). `grid`/`state` are
/// subscriber-only and routinely `None`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QrzLookup {
    pub call: String,
    pub name: Option<String>,
    /// QRZ `<nickname>` — the operator's preferred short/first name when they set one.
    /// Preferred over `name` for display when present (operators want to be greeted by it).
    pub nickname: Option<String>,
    /// City (QRZ `addr2`).
    pub qth: Option<String>,
    pub grid: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
    pub dxcc: Option<u32>,
    pub cq_zone: Option<u32>,
    pub itu_zone: Option<u32>,
    /// Profile photo URL (QRZ `<image>`). Subscriber-only + operator-supplied, so routinely `None`.
    pub image: Option<String>,
    /// The station's EXACT position, when QRZ reports a real one — the input QRZ's own
    /// distance/bearing figures are computed from. `None` unless `<geoloc>` says the
    /// coordinates were surveyed (`user`) or geocoded from the address (`geocode`);
    /// see [`parse_callsign`] for why the other provenances are refused.
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

/// Percent-encode a query value (RFC 3986 unreserved set). Same encoder as the
/// sibling connectors; kept local. Encodes `;`/`&`/`=`/space etc. so a password
/// or callsign can't break the query string.
fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Build the login URL (carries the password — secret). QRZ accepts `&` or `;`
/// separators; we use `&` (standard) with percent-encoded values.
pub fn build_login_url(l: &QrzLogin) -> String {
    format!(
        "{QRZ_XML_URL}?username={}&password={}&agent={}",
        pct(&l.username),
        pct(&l.password),
        pct(&l.agent),
    )
}

/// Build a lookup URL (carries the session key — secret).
pub fn build_lookup_url(session_key: &str, callsign: &str) -> String {
    format!(
        "{QRZ_XML_URL}?s={}&callsign={}",
        pct(session_key.trim()),
        pct(callsign.trim()),
    )
}

/// True iff `body` is a QRZ XML response (not an HTML/error page). The
/// `<QRZDatabase` root is QRZ-specific, so a `contains` check is safe here (an
/// HTML page never carries it).
pub fn is_qrz_xml(body: &str) -> bool {
    body.to_ascii_lowercase().contains("<qrzdatabase")
}

/// Parse the `<Session>` block. Missing fields ⇒ `None`.
pub fn parse_session(xml: &str) -> QrzSession {
    QrzSession {
        key: tag(xml, "Key"),
        sub_exp: tag(xml, "SubExp"),
        count: tag(xml, "Count").and_then(|c| c.parse().ok()),
        message: tag(xml, "Message"),
        error: tag(xml, "Error"),
    }
}

/// Parse the `<Callsign>` data block. `None` if there is no callsign record
/// (e.g. a login-only or error response).
///
/// `lat`/`lon` are gated on QRZ's `<geoloc>` provenance tag, because QRZ returns
/// coordinates for every record regardless of how little it actually knows:
///
/// | `geoloc` | meaning | kept? |
/// |---|---|---|
/// | `user` | the operator placed their own pin | yes — exact |
/// | `geocode` | geocoded from the postal address | yes — street-accurate |
/// | `grid` | back-derived from the locator | no — identical to our own grid center |
/// | `dxcc` | the DXCC entity's centroid | **no — hundreds of km worse than the grid** |
/// | absent | unknown provenance | no |
///
/// Taking the coordinates ungated would make a bearing WORSE than the grid square
/// it replaced whenever QRZ fell back to `dxcc`, so absence is reported as absence
/// and the caller falls back to the locator.
pub fn parse_callsign(xml: &str) -> Option<QrzLookup> {
    // ⛔ Scope EVERY field to the returned `<Callsign>` record. `tag` is an unscoped substring
    // scan, so a refusal whose `<Error>`/`<Session>` prose quotes `<call>`/`<grid>` — with no
    // `<Callsign>` element at all — used to fabricate a record QRZ never sent, shown to the operator
    // and stamped green (#245, round 7 F2). No record element ⇒ no record.
    let xml = callsign_scope(xml)?;
    let call = tag(xml, "call")?;
    let name = tag(xml, "name_fmt").or_else(|| match (tag(xml, "fname"), tag(xml, "name")) {
        (Some(f), Some(l)) => Some(format!("{f} {l}")),
        (Some(f), None) => Some(f),
        (None, Some(l)) => Some(l),
        (None, None) => None,
    });
    // Both coordinates or neither — a half-parsed position is not a position.
    let precise = tag(xml, "geoloc")
        .is_some_and(|g| matches!(g.trim().to_ascii_lowercase().as_str(), "user" | "geocode"));
    let (lat, lon) = match (
        precise.then(|| tag(xml, "lat")).flatten(),
        precise.then(|| tag(xml, "lon")).flatten(),
    ) {
        (Some(la), Some(lo)) => match (la.trim().parse::<f64>(), lo.trim().parse::<f64>()) {
            (Ok(la), Ok(lo)) if (-90.0..=90.0).contains(&la) && (-180.0..=180.0).contains(&lo) => {
                (Some(la), Some(lo))
            }
            _ => (None, None),
        },
        _ => (None, None),
    };
    Some(QrzLookup {
        lat,
        lon,
        call,
        name,
        nickname: tag(xml, "nickname"),
        qth: tag(xml, "addr2"),
        grid: tag(xml, "grid"),
        state: tag(xml, "state"),
        country: tag(xml, "country"),
        dxcc: tag(xml, "dxcc").and_then(|d| d.parse().ok()),
        cq_zone: tag(xml, "cqzone").and_then(|d| d.parse().ok()),
        itu_zone: tag(xml, "ituzone").and_then(|d| d.parse().ok()),
        image: tag(xml, "image"),
    })
}

/// ⛔ **THE ONE POSITIVE SIGNAL — the qrz-xml health row goes green on this and nothing else.**
///
/// True iff `body` is a QRZ XML response carrying a field QRZ serves **only to an entitled
/// subscription**: a `<Callsign>` record with a `<grid>` or a `<state>` (this module's header,
/// and [`parse_callsign`]'s own free-tier test — a free or lapsed account is given
/// name/address/country and nothing more). Every other body is `false`.
///
/// ⚠️ **It is an ALLOW-LIST, and that is the whole fix.** #245 returned six times because the
/// check was a DENY-list: a lookup counted as proof unless its body matched a known failure, so
/// every unrecognised body defaulted to GREEN and each round appended one more wording. Five of
/// those rounds were beaten by the next wording. The sixth costume was not a wording at all —
/// QRZ's non-subscriber reply (`fixtures::LOOKUP_FREE`) is a *success* body with a real
/// `<Callsign>` in it, so a reader hunting for failure text found none and stamped the row green
/// over a subscription that had lapsed. Inverted, a body nobody has ever seen is not-confirmed
/// **by construction** — there is no list left to be behind.
///
/// ⚠️ **The price, stated here rather than discovered later:** an entitled lookup whose record
/// carries neither a grid nor a state reads as not-confirmed, and the row stays red until a
/// lookup that returns one. That is the correct direction to be wrong in — a red row costs a
/// glance and the next real lookup clears it, while a green row over a lapsed subscription is
/// the bug this was reported as.
///
/// ⚠️ **The signal is SCOPED to the returned `<Callsign>` record (round 7).** Round 6's allow-list
/// read `<grid>`/`<state>` as a bare substring anywhere in the body, so QRZ's own
/// `<Message>`/`<Session>`/`<Error>` prose — or a tag smuggled into a field VALUE — flipped the
/// row green. It now asks [`callsign_has_field`], which finds the field only as a DIRECT CHILD of
/// the record.
///
/// ⛔ **NEEDS-BENCH — the vendor claim under the `state` half.** The green condition assumes QRZ
/// serves BOTH `<grid>` and `<state>` only to an entitled subscription. Nothing in the repo
/// verifies that, and `<state>` for a US call is derivable from the FCC ULS (which QRZ holds for
/// every US ham regardless of subscription). If QRZ returns `<state>` to a free/lapsed US account,
/// every US non-subscriber goes green and #245 returns. Settling it needs a live lookup from a
/// lapsed/free QRZ XML subscription — **do not send credentials out to check it.** If `state` IS
/// returned free, the narrowing is one line: drop the `|| callsign_has_field(body, "state")` term
/// below; `grid` alone stays subscriber-only.
pub fn proves_entitled_lookup(body: &str) -> bool {
    proves_entitled_lookup_at(body, now_unix())
}

/// As [`proves_entitled_lookup`], with the clock injected so the `<SubExp>` expiry check is
/// deterministic under test. `now_unix` is UTC seconds.
///
/// ⛔ **QRZ's own `<SubExp>` is consulted, and an explicit negative disqualifies REGARDLESS of the
/// two positive tokens (round 7 F1).** Round 6 read only `grid`/`state` and never looked at the
/// field QRZ provides *specifically* to state entitlement, so a body that literally says
/// `non-subscriber`, and one whose subscription expired years ago, both stamped the row green. A
/// definitive negative in hand is now believed over any grid/state — see [`subscription_denied`].
/// This also blunts the NEEDS-BENCH risk above: a free account's reply carries
/// `<SubExp>non-subscriber</SubExp>`, which is refused here even if it also carried a `<state>`.
pub fn proves_entitled_lookup_at(body: &str, now_unix: i64) -> bool {
    if !is_qrz_xml(body) {
        return false;
    }
    let session = parse_session(body);
    if session.needs_login() || subscription_denied(&session, now_unix) {
        return false;
    }
    parse_callsign(body).is_some()
        && (callsign_has_field(body, "grid") || callsign_has_field(body, "state"))
}

/// True iff `<SubExp>` explicitly says this is NOT an entitled subscription: the literal
/// `non-subscriber`, or an expiry whose year is already past. QRZ writes SubExp as that literal or
/// a human date (e.g. `Wed Jan 1 2031`); the year is the first 4-digit run. Deliberately coarse —
/// it never disqualifies a current/future-year subscriber, and an ABSENT SubExp (a real subscriber
/// record omits it, e.g. `fixtures::LOOKUP_FULL`) is not a disqualifier. An unparseable date is not
/// a disqualifier either; the grid/state check remains the gate. A same-year mid-year lapse is not
/// caught by the year test — the `non-subscriber` literal and the grid/state gate are the backstops.
fn subscription_denied(session: &QrzSession, now_unix: i64) -> bool {
    let Some(s) = session
        .sub_exp
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return false;
    };
    if s.eq_ignore_ascii_case("non-subscriber") {
        return true;
    }
    match (subexp_year(s), current_year(now_unix)) {
        (Some(exp), Some(this)) => exp < this,
        _ => false,
    }
}

/// The first 4-digit run of `s` read as a year in 1900..=9999, or `None`.
fn subexp_year(s: &str) -> Option<i32> {
    s.split(|c: char| !c.is_ascii_digit())
        .find(|g| g.len() == 4)
        .and_then(|g| g.parse::<i32>().ok())
        .filter(|y| (1900..=9999).contains(y))
}

/// The UTC calendar year of `now_unix` (negative times → `None`).
fn current_year(now_unix: i64) -> Option<i32> {
    (now_unix >= 0).then(|| crate::logbook::datetime_utc(now_unix as u64).0)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// True iff the QRZ `<Callsign>` record a lookup returned carries `field` as a **non-empty direct
/// child element** — the record's OWN `<grid>`/`<state>`, not a tag of that name sitting elsewhere
/// in the document. This is the whole positive signal for [`proves_entitled_lookup`], so it is
/// deliberately narrow.
///
/// It is NOT a substring test — that was the round-6 hole (#245, round 7). It looks inside the
/// FIRST `<Callsign>…</Callsign>` element only (so a tag in QRZ's sibling `<Session>`/`<Message>`/
/// `<Error>` blocks does not count), and within that record it steps over the entire content of
/// every child, so a `<state>` nested inside an `<addr2>` value is passed over rather than read as
/// the record's own state.
///
/// # Honest limits
/// tempo-core has no XML parser and adding one for one predicate is not worth a new dependency;
/// this is the same hand-rolled shape as [`tag`], scoped. It assumes QRZ records are FLAT (every
/// real field is a direct child — the captured corpus and the vendor format bear this out); it
/// matches only the attribute-free `<field>` form, refusing an attributed `<grid …>` exactly as
/// [`tag`] does (no `>`-in-attribute hazard); and a child opened but never closed ends the walk,
/// which is the fail-closed direction.
/// The content of the first `<Callsign>…</Callsign>` element, or `None` when there is no record
/// element at all. Every field a lookup reports must be read from INSIDE this — QRZ's
/// `<Error>`/`<Session>`/`<Message>` prose can quote `<call>`/`<grid>`, and an unscoped scan
/// fabricates a record from a refusal that has no `<Callsign>` (#245, round 7). Requires the
/// opening `<Callsign` tag; a missing close is tolerated (scoped to end) since a well-formed body
/// always carries one and a truncated record should still parse what it has.
fn callsign_scope(xml: &str) -> Option<&str> {
    let lower = xml.to_ascii_lowercase();
    let open = lower.find("<callsign")?;
    let gt = lower[open..].find('>')? + open;
    let start = gt + 1;
    let end = lower[start..]
        .find("</callsign>")
        .map_or(xml.len(), |r| start + r);
    Some(&xml[start..end])
}

fn callsign_has_field(body: &str, field: &str) -> bool {
    // The record the lookup returned; nothing outside it can be entitlement.
    let Some(scope) = callsign_scope(body) else {
        return false;
    };
    let lower = scope.to_ascii_lowercase();
    let mut pos = 0usize;
    let end = lower.len();

    // Walk the record's DIRECT children only.
    while pos < end {
        let Some(rel) = lower[pos..end].find('<') else {
            return false;
        };
        let tag_open = pos + rel;
        let Some(gt_rel) = lower[tag_open..end].find('>') else {
            return false; // an unterminated tag — stop rather than guess
        };
        let inner = &lower[tag_open + 1..tag_open + gt_rel]; // between '<' and '>'
        let content_start = tag_open + gt_rel + 1;
        // Element name (for skipping a child's content): up to the first whitespace or '/'.
        let name_end = inner
            .find(|c: char| c.is_ascii_whitespace() || c == '/')
            .unwrap_or(inner.len());
        let name = &inner[..name_end];
        // A closing (`</x>`), self-closing (`<x/>`), or nameless tag has no child content to skip.
        if name.is_empty() || inner.starts_with('/') || inner.ends_with('/') {
            pos = content_start;
            continue;
        }
        let close = format!("</{name}>");
        let Some(crel) = lower[content_start..end].find(&close) else {
            return false; // a child opened but never closed inside the record — fail closed
        };
        let child_end = content_start + crel;
        // Match only the EXACT attribute-free `<field>` (QRZ data tags carry none); the raw slice
        // must hold something once trimmed.
        if inner == field && !scope[content_start..child_end].trim().is_empty() {
            return true;
        }
        pos = child_end + close.len();
    }
    false
}

/// Extract the text content of the first `<name>…</name>` element (case-insensitive
/// tag name), XML-unescaped. `None` if absent/empty. Matches only the attribute-free
/// `<name>` form — QRZ data elements never carry attributes, so an attributed tag
/// safely yields `None` rather than a mis-bounded value (no `>`-in-attribute hazard).
fn tag(xml: &str, name: &str) -> Option<String> {
    // Lowercased copy for case-insensitive search; `to_ascii_lowercase` preserves
    // byte length + boundaries, so indices map 1:1 onto `xml`.
    let lower = xml.to_ascii_lowercase();
    let n = name.to_ascii_lowercase();
    let open = format!("<{n}>");
    let start = lower.find(&open)?;
    let after_open = start + open.len();
    let close = format!("</{n}>");
    let close_rel = lower[after_open..].find(&close)?;
    let raw = xml[after_open..after_open + close_rel].trim();
    let v = unescape_xml(raw);
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// Decode the common XML entities (`&amp;` LAST so it doesn't double-decode).
fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

// ----- QRZ Logbook PUSH (a separate per-logbook API key; see `qrz-push.md`) -----

/// The QRZ Logbook API endpoint (POST, `name=value` form). Hard-coded https.
pub const QRZ_LOGBOOK_URL: &str = "https://logbook.qrz.com/api";

/// Outcome of a QRZ Logbook INSERT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrzPushResult {
    /// Inserted (new `logid`).
    Ok,
    /// Overwrote a duplicate (only when `OPTION=REPLACE` was sent).
    Replace,
    /// A duplicate that was rejected — benign ("already in your QRZ logbook").
    Duplicate,
    /// The API key was missing/invalid/insufficient.
    AuthFail,
    /// Any other failure (see `reason`).
    Fail,
}

impl QrzPushResult {
    /// Map a QRZ push outcome to the generic per-QSO [`UploadOutcome`] for the
    /// logbook's `upload.qrz` cursor. `Ok`/`Replace` → on file (Accepted); a
    /// rejected duplicate is benign (Duplicate); `AuthFail`/`Fail` are bounces the
    /// diagnostics surface as R9. Always definitive (no transient/None case).
    pub fn to_upload_outcome(self) -> crate::logbook::UploadOutcome {
        use crate::logbook::UploadOutcome as U;
        match self {
            QrzPushResult::Ok | QrzPushResult::Replace => U::Accepted,
            QrzPushResult::Duplicate => U::Duplicate,
            QrzPushResult::AuthFail => U::AuthFail,
            QrzPushResult::Fail => U::Rejected,
        }
    }

    /// WHY, for the per-QSO stamp — the CLASS, off the `RESULT` token.
    ///
    /// ⛔ Deliberately not [`QrzPush::reason`]. That is QRZ's own prose, QRZ echoes the
    /// failing request back, and the request carries the API key — and this value is written
    /// into `log.adi`, which is signed by TQSL and uploaded to ARRL. See
    /// [`crate::logbook::UploadDetail`]. The reason still reaches the operator; it goes to
    /// the connection log and the toast, and dies with the session.
    pub fn to_upload_detail(self) -> Option<crate::logbook::UploadDetail> {
        use crate::logbook::UploadDetail as D;
        match self {
            QrzPushResult::Ok | QrzPushResult::Replace | QrzPushResult::Duplicate => None,
            QrzPushResult::AuthFail => Some(D::Credentials),
            QrzPushResult::Fail => Some(D::RecordRefused),
        }
    }
}

/// Parsed QRZ Logbook INSERT response.
#[derive(Debug, Clone, PartialEq)]
pub struct QrzPush {
    pub result: QrzPushResult,
    pub logid: Option<String>,
    pub count: u32,
    pub reason: Option<String>,
}

/// Build the `name=value` POST body for an INSERT. The ADIF tags (`<…>`) and the
/// key are percent-encoded so they survive form-encoding. The body carries the API
/// key — never log it.
pub fn build_insert_body(api_key: &str, adif_record: &str, replace: bool) -> String {
    let mut body = format!(
        "KEY={}&ACTION=INSERT&ADIF={}",
        pct(api_key.trim()),
        pct(adif_record),
    );
    if replace {
        body.push_str("&OPTION=REPLACE");
    }
    body
}

/// Build the body of a QRZ Logbook **STATUS** request — validates the API key
/// with a real round-trip WITHOUT inserting anything (the Test button).
pub fn build_status_body(api_key: &str) -> String {
    format!("KEY={}&ACTION=STATUS", pct(api_key.trim()))
}

/// What a STATUS round-trip proved about the logbook the key unlocks.
#[derive(Debug, Clone, PartialEq)]
pub struct QrzStatus {
    pub ok: bool,
    /// Logbook owner callsign (QRZ `OWNER`), when reported.
    pub owner: Option<String>,
    /// Logbook name (QRZ `BOOK_NAME`/`BOOKID`), when reported.
    pub book: Option<String>,
    /// QSO count in the logbook (QRZ `COUNT`).
    pub count: u32,
    /// Failure reason (auth errors etc.).
    pub reason: Option<String>,
}

/// Parse a QRZ Logbook STATUS `name=value` response.
pub fn parse_status_response(body: &str) -> QrzStatus {
    let mut ok = false;
    let mut owner = None;
    let mut book = None;
    let mut count = 0u32;
    let mut reason = None;
    for pair in body.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        let val = urldecode(v.trim());
        match k.trim().to_ascii_uppercase().as_str() {
            "RESULT" => ok = val.eq_ignore_ascii_case("OK"),
            "OWNER" if !val.is_empty() => owner = Some(val),
            "BOOK_NAME" if !val.is_empty() => book = Some(val),
            "BOOKID" if book.is_none() && !val.is_empty() => book = Some(format!("book {val}")),
            "COUNT" => count = val.parse().unwrap_or(0),
            "REASON" if !val.is_empty() => reason = Some(val),
            _ => {}
        }
    }
    QrzStatus {
        ok,
        owner,
        book,
        count,
        reason,
    }
}

/// Result of a QRZ Logbook **FETCH** — the raw ADIF payload plus the header counts. The caller
/// feeds `adif` through the ordinary ADIF importer + reconcile merge; this only unwraps the
/// QRZ envelope. Two-way sync: pull the operator's own book back down (new QSOs + confirmations).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QrzFetch {
    /// `RESULT=OK` (or `STATUS=OK`).
    pub ok: bool,
    /// Records returned (QRZ `COUNT`).
    pub count: u32,
    /// The ADIF text QRZ returned (may be empty), taken verbatim.
    pub adif: String,
    /// Failure reason (auth errors etc.), when not OK.
    pub reason: Option<String>,
}

/// Build the body of a QRZ Logbook **FETCH** request — pulls the operator's own logbook back
/// down. The bare form fetches the whole book; QRZ streams it as one ADIF blob in the response
/// `ADIF` field. Carries the API key — never log it.
pub fn build_fetch_body(api_key: &str) -> String {
    format!("KEY={}&ACTION=FETCH", pct(api_key.trim()))
}

/// A DELTA fetch: only records QRZ has touched since `since_date` (`YYYY-MM-DD`, UTC).
///
/// This is what makes an automatic hourly sync reasonable. The bare
/// [`build_fetch_body`] pulls the WHOLE logbook every run, which is fine for a button
/// the operator presses and rude on a timer against someone else's server.
///
/// From QRZ's Logbook API guide: `"MODSINCE:2023-01-01" only return records modified
/// since this date`.
///
/// ⚠️ **ONE option, deliberately.** QRZ's guide contradicts itself on how to combine
/// them — the prose says "separated by the ampersand or semicolon (& or ;)" while the
/// example reads `BAND:80m,MODE:SSB,MAX:400`. Sending a single option sidesteps a
/// disagreement we cannot test from here without an account.
///
/// ⚠️ **NOT `STATUS:CONFIRMED`, even though it exists.** This sync deliberately merges
/// QSOs the operator logged elsewhere (a phone in the field) as well as confirmations;
/// filtering to confirmed-only would silently drop those.
///
/// ⚠️ **MODSINCE is DATE granularity, not a timestamp** — the guide shows only
/// `YYYY-MM-DD`. Callers should therefore reach back a day rather than pass the exact
/// last-sync date, so nothing falls through a UTC day boundary or QRZ's own clock. Re-
/// fetching a day of overlap is free: `reconcile` is idempotent (a second run yields
/// all-zero counts).
pub fn build_fetch_since_body(api_key: &str, since_date: &str) -> String {
    let d = since_date.trim();
    if d.is_empty() {
        return build_fetch_body(api_key);
    }
    format!(
        "KEY={}&ACTION=FETCH&OPTION={}",
        pct(api_key.trim()),
        pct(&format!("MODSINCE:{d}")),
    )
}

/// The `YYYY-MM-DD` to hand [`build_fetch_since_body`] for a sync whose last success was
/// `last_ok_unix`, reaching `overlap_days` back for the reasons in that function's note.
/// `None` (never synced) yields `None` — the caller does a full fetch to seed the log.
pub fn fetch_since_date(last_ok_unix: Option<u64>, overlap_days: u64) -> Option<String> {
    let t = last_ok_unix?.saturating_sub(overlap_days.saturating_mul(86_400));
    let days = (t / 86_400) as i64;
    // Civil date from a Unix day count (Howard Hinnant's algorithm) — no chrono here.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Split a FETCH response at the `ADIF=` field boundary. The ADIF payload itself contains `&`,
/// `=`, and newlines, so it CANNOT be split as an ordinary `name=value` pair — QRZ always emits
/// it last. Everything before the boundary is the small metadata header; everything after is the
/// ADIF blob — which QRZ HTML-entity-ENCODES (`&lt;CALL:5&gt;…&lt;eor&gt;`), so it must be decoded
/// back to literal `<…>` (see [`html_unescape`]) or an ADIF importer finds zero records.
fn split_adif(body: &str) -> (&str, String) {
    let lower = body.to_ascii_lowercase();
    let mut search = 0;
    while let Some(rel) = lower[search..].find("adif=") {
        let idx = search + rel;
        let at_boundary = idx == 0 || body.as_bytes()[idx - 1] == b'&';
        if at_boundary {
            let head_end = idx.saturating_sub(1); // drop the trailing '&' (0 stays 0)
            return (&body[..head_end], html_unescape(&body[idx + 5..]));
        }
        search = idx + 5;
    }
    (body, String::new())
}

/// Decode the HTML entities QRZ uses in the FETCH ADIF payload. QRZ returns the ADIF
/// with its angle brackets escaped — `&lt;CALL:5&gt;W1AW&lt;eor&gt;` — NOT literal, so
/// without this an ADIF importer sees no `<tag>` markers and pulls back 0 QSOs even
/// though RESULT=OK (the reported "sync from QRZ shows 0 QSOs" bug). `&amp;` is decoded
/// LAST so an escaped literal `&amp;lt;` becomes `&lt;`, not `<`. Matches what working
/// QRZ clients (e.g. k0swe/qrz-logbook) do.
fn html_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Parse a QRZ Logbook FETCH response into its header + ADIF payload.
pub fn parse_fetch(body: &str) -> QrzFetch {
    let (head, adif) = split_adif(body);
    let mut ok = false;
    let mut count = 0u32;
    let mut reason = None;
    for pair in head.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        let val = urldecode(v.trim());
        match k.trim().to_ascii_uppercase().as_str() {
            "RESULT" | "STATUS" => ok = val.eq_ignore_ascii_case("OK"),
            "COUNT" => count = val.parse().unwrap_or(0),
            "REASON" if !val.is_empty() => reason = Some(val),
            _ => {}
        }
    }
    QrzFetch {
        ok,
        count,
        adif,
        reason,
    }
}

/// Parse a QRZ Logbook `name=value` response. A `RESULT=FAIL` whose `REASON`
/// mentions "duplicate" maps to [`QrzPushResult::Duplicate`] (benign).
pub fn parse_push_response(body: &str) -> QrzPush {
    let mut result_raw: Option<String> = None;
    let mut logid = None;
    let mut count = 0u32;
    let mut reason = None;
    for pair in body.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        let val = urldecode(v.trim());
        match k.trim().to_ascii_uppercase().as_str() {
            "RESULT" => result_raw = Some(val.to_ascii_uppercase()),
            "LOGID" if !val.is_empty() => logid = Some(val),
            "COUNT" => count = val.parse().unwrap_or(0),
            "REASON" if !val.is_empty() => reason = Some(val),
            _ => {}
        }
    }
    let is_dup = reason
        .as_deref()
        .is_some_and(|r| r.to_ascii_lowercase().contains("duplicate"));
    let result = match result_raw.as_deref() {
        Some("OK") => QrzPushResult::Ok,
        Some("REPLACE") => QrzPushResult::Replace,
        Some("AUTH") => QrzPushResult::AuthFail,
        Some("FAIL") if is_dup => QrzPushResult::Duplicate,
        _ => QrzPushResult::Fail,
    };
    QrzPush {
        result,
        logid,
        count,
        reason,
    }
}

/// Minimal `application/x-www-form-urlencoded` value decoder (`+`→space, `%XX`).
fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                // Parse the two raw bytes as ASCII hex — NEVER slice `s` (a `%`
                // followed by a multibyte UTF-8 byte would split a char boundary
                // and panic). `from_utf8` rejects non-ASCII hex bytes safely.
                let hex = [bytes[i + 1], bytes[i + 2]];
                match std::str::from_utf8(&hex)
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// **Every complete QRZ XML response this repo has a sample of — one copy, read by two crates.**
///
/// ⚠️ The parser here and the connector-health stamp in `src-tauri` both decide things about
/// these exact bytes, and #245 is six rounds of those two readers disagreeing about them. A
/// second copy of a body is a second chance to disagree, so the bodies live here and both
/// crates read them. `const`, so a build that never mentions them carries nothing.
///
/// **In scope: whole responses** — anything with a `<QRZDatabase>` root, which is what comes
/// off the wire and what `qrz_outcome_from_body` is handed. The bare `<Callsign>…` fragments
/// elsewhere in this file's tests are [`parse_callsign`] probes, not responses; QRZ never sends
/// one, and each is exercised where it is written.
///
/// [`QrzXmlSample::verifies_subscription`] is the answer the whole chain must give for that
/// body: `true` only where it carries positive proof of an entitled lookup. It is `true` for
/// exactly one sample.
pub mod fixtures {
    /// One captured QRZ XML response, and what the qrz-xml Connections row may do with it.
    pub struct QrzXmlSample {
        /// What this fixture is called, for an assertion message that names the failure.
        pub name: &'static str,
        /// The response body.
        pub body: &'static str,
        /// May the qrz-xml row go **green** on this body? Green means "the XML subscription is
        /// live and entitled", and it does not merely fail to warn when it is wrong — it
        /// CLEARS an existing red.
        pub verifies_subscription: bool,
    }

    /// A login round trip: a session key, a quota count, a live subscription expiry — and no
    /// record, because a login is not a lookup.
    pub const LOGIN_OK: &str = "<?xml version=\"1.0\" ?>\n\
<QRZDatabase version=\"1.34\" xmlns=\"http://xmldata.qrz.com\">\n\
<Session><Key>3b1fc0de</Key><Count>12</Count><SubExp>Wed Jan 1 2031</SubExp></Session>\n\
</QRZDatabase>";

    /// The session died: an `<Error>` and no `<Key>` at all.
    pub const EXPIRED: &str = "<?xml version=\"1.0\" ?><QRZDatabase version=\"1.34\">\
<Session><Error>Session Timeout</Error></Session></QRZDatabase>";

    /// QRZ's genuine miss — a live key, an `<Error>` naming the callsign, no record.
    pub const NOT_FOUND: &str = "<QRZDatabase version=\"1.34\"><Session><Key>abc</Key>\
<Error>Not found: g1srdd</Error></Session></QRZDatabase>";

    /// ⛔ **The fifth costume**, kept because it is the one that was still green in round 4: a
    /// REFUSAL whose wording opens exactly like a miss. Nothing here separates it from
    /// [`NOT_FOUND`] except the words, which is why nothing tries to.
    pub const REFUSAL_DRESSED_AS_A_MISS: &str = "<QRZDatabase><Session><Key>live</Key>\
<Error>Not found: your subscription does not cover this record</Error></Session></QRZDatabase>";

    /// A key QRZ itself will not accept — present, and dead.
    pub const INVALID_SESSION_KEY: &str = "<QRZDatabase><Session><Key>stale</Key>\
<Error>Invalid session key</Error></Session></QRZDatabase>";

    /// An answer with neither a record nor a reason.
    pub const NO_RECORD_NO_REASON: &str =
        "<QRZDatabase><Session><Key>live</Key></Session></QRZDatabase>";

    /// **A real subscriber record — the only sample in this corpus that proves anything.** The
    /// `<grid>` and `<state>` are what a free account is not given, so their presence is the
    /// positive signal (see [`super::proves_entitled_lookup`]); `<lat>`/`<lon>`/`<geoloc>` were
    /// missing from this fixture until 2026-08-01, which is why nothing caught the caller card
    /// re-deriving a position QRZ had already given us exactly.
    pub const LOOKUP_FULL: &str = "<?xml version=\"1.0\" ?>\n\
<QRZDatabase version=\"1.34\" xmlns=\"http://xmldata.qrz.com\">\n\
<Callsign><call>AA7BQ</call><fname>Fred</fname><name>Lloyd</name><addr2>Scottsdale</addr2>\
<state>AZ</state><country>United States</country><grid>DM43bp</grid><dxcc>291</dxcc>\
<lat>33.634000</lat><lon>-111.887000</lon><geoloc>user</geoloc>\
<cqzone>3</cqzone><ituzone>6</ituzone><image>https://cdn-xml.qrz.com/q/aa7bq/aa7bq.jpg</image></Callsign>\n\
<Session><Key>abc</Key><Count>13</Count></Session>\n</QRZDatabase>";

    /// ⛔ **THE SIXTH COSTUME, AND THE REASON THIS CORPUS EXISTS (#245, round 6).**
    ///
    /// A free — or lapsed — account: name and country, **no grid, no state**. It is a
    /// *success* body with a real `<Callsign>` in it, so five rounds of hunting for failure
    /// wording had nothing to find here and stamped the row GREEN over a subscription that was
    /// not paying for anything. `verifies_subscription` is `false` and a build that says
    /// otherwise is the defect, not a new fixture.
    pub const LOOKUP_FREE: &str = "<QRZDatabase version=\"1.34\"><Callsign><call>AA7BQ</call>\
<name_fmt>Fred Lloyd</name_fmt><country>United States</country></Callsign>\
<Session><Key>abc</Key><SubExp>non-subscriber</SubExp>\
<Message>A subscription is required to obtain the complete data</Message></Session></QRZDatabase>";

    /// ⛔ Round 5's own "control: a record is a working subscription" — a `<call>` and nothing
    /// else beside a live key. It was asserted GREEN, and it is the shape the free tier
    /// degrades to. Kept, red, as the fixture that names what changed.
    pub const BARE_CALL: &str = "<QRZDatabase><Callsign><call>AA7BQ</call></Callsign>\
<Session><Key>live</Key></Session></QRZDatabase>";

    /// A record whose text needs XML unescaping (and, incidentally, no `<Session>` at all).
    pub const ENTITY_ESCAPED_NAME: &str = "<QRZDatabase><Callsign><call>X</call>\
<name>Smith &amp; Jones</name></Callsign></QRZDatabase>";

    /// An attributed `<grid id="a>b">`, which [`super::parse_callsign`] refuses rather than
    /// mis-bounding at the `>` inside the attribute. So the grid is unreadable — and an
    /// unreadable subscriber field proves nothing, which is the fail-closed direction.
    pub const ATTRIBUTED_GRID: &str = "<QRZDatabase><Callsign><call>X</call>\
<grid id=\"a>b\">DM43</grid></Callsign></QRZDatabase>";

    /// The corpus, in one list, so a walk over it cannot miss a fixture someone added.
    pub const ALL: &[QrzXmlSample] = &[
        QrzXmlSample {
            name: "LOGIN_OK",
            body: LOGIN_OK,
            verifies_subscription: false,
        },
        QrzXmlSample {
            name: "EXPIRED",
            body: EXPIRED,
            verifies_subscription: false,
        },
        QrzXmlSample {
            name: "NOT_FOUND",
            body: NOT_FOUND,
            verifies_subscription: false,
        },
        QrzXmlSample {
            name: "REFUSAL_DRESSED_AS_A_MISS",
            body: REFUSAL_DRESSED_AS_A_MISS,
            verifies_subscription: false,
        },
        QrzXmlSample {
            name: "INVALID_SESSION_KEY",
            body: INVALID_SESSION_KEY,
            verifies_subscription: false,
        },
        QrzXmlSample {
            name: "NO_RECORD_NO_REASON",
            body: NO_RECORD_NO_REASON,
            verifies_subscription: false,
        },
        QrzXmlSample {
            name: "LOOKUP_FULL",
            body: LOOKUP_FULL,
            verifies_subscription: true,
        },
        QrzXmlSample {
            name: "LOOKUP_FREE",
            body: LOOKUP_FREE,
            verifies_subscription: false,
        },
        QrzXmlSample {
            name: "BARE_CALL",
            body: BARE_CALL,
            verifies_subscription: false,
        },
        QrzXmlSample {
            name: "ENTITY_ESCAPED_NAME",
            body: ENTITY_ESCAPED_NAME,
            verifies_subscription: false,
        },
        QrzXmlSample {
            name: "ATTRIBUTED_GRID",
            body: ATTRIBUTED_GRID,
            verifies_subscription: false,
        },
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_url_encodes_password() {
        let url = build_login_url(&QrzLogin {
            username: "AA7BQ".into(),
            password: "p@ss;w&rd".into(),
            agent: "nexus/0.1".into(),
        });
        assert!(url.starts_with("https://xmldata.qrz.com/xml/current/?username=AA7BQ&password="));
        assert!(url.contains("password=p%40ss%3Bw%26rd")); // ; and & encoded
        assert!(!url.contains("p@ss;w&rd"));
        assert!(url.contains("agent=nexus%2F0.1"));
    }

    #[test]
    fn lookup_url_encodes_key_and_call() {
        let url = build_lookup_url("KEY 123", "dl1abc");
        assert_eq!(
            url,
            "https://xmldata.qrz.com/xml/current/?s=KEY%20123&callsign=dl1abc"
        );
    }

    #[test]
    fn login_debug_redacts_password() {
        let dbg = format!(
            "{:?}",
            QrzLogin {
                username: "AA7BQ".into(),
                password: "sekret".into(),
                agent: "x".into()
            }
        );
        assert!(dbg.contains("<redacted>") && !dbg.contains("sekret"));
    }

    // The response bodies live in `super::fixtures` — ONE copy, shared with src-tauri's
    // health stamp, because #245 is six rounds of two readers disagreeing about these bytes.
    use super::fixtures::{
        ATTRIBUTED_GRID, ENTITY_ESCAPED_NAME, EXPIRED, INVALID_SESSION_KEY, LOGIN_OK, LOOKUP_FREE,
        LOOKUP_FULL, NOT_FOUND,
    };

    #[test]
    fn session_login_ok_has_key_no_relogin() {
        let s = parse_session(LOGIN_OK);
        assert_eq!(s.key.as_deref(), Some("3b1fc0de"));
        assert_eq!(s.count, Some(12));
        assert!(!s.needs_login());
    }

    #[test]
    fn session_expired_needs_login() {
        let s = parse_session(EXPIRED);
        assert!(s.key.is_none());
        assert!(s.needs_login());
    }

    #[test]
    fn session_not_found_keeps_key_no_relogin() {
        // Not-found is a valid session with an error — must NOT trigger re-login.
        let s = parse_session(NOT_FOUND);
        assert_eq!(s.key.as_deref(), Some("abc"));
        assert!(!s.needs_login());
        assert!(s.error.as_deref().unwrap().contains("Not found"));
    }

    #[test]
    fn session_debug_redacts_key() {
        let dbg = format!("{:?}", parse_session(LOGIN_OK));
        assert!(dbg.contains("<redacted>") && !dbg.contains("3b1fc0de"));
    }

    #[test]
    fn parses_full_subscriber_record() {
        let r = parse_callsign(LOOKUP_FULL).unwrap();
        assert_eq!(r.call, "AA7BQ");
        assert_eq!(r.name.as_deref(), Some("Fred Lloyd"));
        assert_eq!(r.qth.as_deref(), Some("Scottsdale"));
        assert_eq!(r.grid.as_deref(), Some("DM43bp"));
        assert_eq!(r.state.as_deref(), Some("AZ"));
        assert_eq!(r.dxcc, Some(291));
        assert_eq!(r.cq_zone, Some(3));
        assert_eq!(r.itu_zone, Some(6));
        assert_eq!(
            r.image.as_deref(),
            Some("https://cdn-xml.qrz.com/q/aa7bq/aa7bq.jpg")
        );
        // The exact position QRZ computes ITS OWN distance/bearing from.
        assert_eq!(r.lat, Some(33.634));
        assert_eq!(r.lon, Some(-111.887));
    }

    /// The `<geoloc>` gate: QRZ hands back coordinates for every record, but only
    /// `user`/`geocode` are a real position. `dxcc` is the entity centroid — using it
    /// would put a bearing HUNDREDS of km further off than the grid square it replaced,
    /// so a weak provenance must report absence and let the locator win.
    #[test]
    fn coordinates_are_kept_only_when_geoloc_vouches_for_them() {
        let rec = |geoloc: &str| {
            format!(
                "<Callsign><call>W1ABC</call><grid>FN31pr</grid>\
<lat>41.714700</lat><lon>-72.727200</lon>{geoloc}</Callsign>"
            )
        };
        for good in ["<geoloc>user</geoloc>", "<geoloc>geocode</geoloc>"] {
            let r = parse_callsign(&rec(good)).unwrap();
            assert_eq!(r.lat, Some(41.7147), "{good} is a real position");
            assert_eq!(r.lon, Some(-72.7272), "{good} is a real position");
        }
        for weak in [
            "<geoloc>grid</geoloc>", // back-derived — no better than our own math
            "<geoloc>dxcc</geoloc>", // entity centroid — far WORSE than the grid
            "",                      // no provenance at all
        ] {
            let r = parse_callsign(&rec(weak)).unwrap();
            assert!(r.lat.is_none() && r.lon.is_none(), "refused: {weak:?}");
            assert_eq!(
                r.grid.as_deref(),
                Some("FN31pr"),
                "the locator still stands"
            );
        }
    }

    #[test]
    fn a_half_or_out_of_range_position_is_no_position() {
        // One coordinate only, and an out-of-range pair — both must collapse to None
        // rather than reaching the UI as a plausible-looking point.
        for xml in [
            "<Callsign><call>W1ABC</call><lat>41.7</lat><geoloc>user</geoloc></Callsign>",
            "<Callsign><call>W1ABC</call><lon>-72.7</lon><geoloc>user</geoloc></Callsign>",
            "<Callsign><call>W1ABC</call><lat>91.0</lat><lon>-72.7</lon><geoloc>user</geoloc></Callsign>",
            "<Callsign><call>W1ABC</call><lat>41.7</lat><lon>x</lon><geoloc>user</geoloc></Callsign>",
        ] {
            let r = parse_callsign(xml).unwrap();
            assert!(r.lat.is_none() && r.lon.is_none(), "refused: {xml}");
        }
    }

    #[test]
    fn parses_free_record_without_grid_state() {
        let r = parse_callsign(LOOKUP_FREE).unwrap();
        assert_eq!(r.call, "AA7BQ");
        assert_eq!(r.name.as_deref(), Some("Fred Lloyd")); // from name_fmt
        assert!(r.grid.is_none(), "free tier has no grid");
        assert!(r.state.is_none(), "free tier has no state");
        assert_eq!(r.country.as_deref(), Some("United States"));
    }

    /// ⛔ **THE ALLOW-LIST, WALKED OVER EVERY CAPTURED RESPONSE (#245, round 6).**
    ///
    /// The pure half of the gate: [`proves_entitled_lookup`] is the only thing that can put the
    /// qrz-xml row green, so every fixture in [`fixtures::ALL`] is driven through it and checked
    /// against the answer the corpus records. `LOOKUP_FREE` — QRZ's non-subscriber reply, which
    /// is a *success* body carrying a real `<Callsign>` — must be `false`; it is the body that
    /// survived five rounds of hunting for failure wording.
    ///
    /// The end-to-end half (the same corpus through `qrz_outcome_from_body` into the health
    /// stamp) is `every_qrz_fixture_is_classified_and_only_a_subscriber_record_is_green` in
    /// `src-tauri`.
    #[test]
    fn only_a_subscriber_scoped_field_proves_an_entitled_lookup() {
        // Both controls, because one direction is half a test: the corpus must contain a body
        // that MUST come out true and bodies that MUST come out false. A predicate hard-wired
        // to either answer passes half of this and fails the other half.
        assert!(
            fixtures::ALL.iter().any(|s| s.verifies_subscription),
            "control: a corpus with no positive sample cannot catch a predicate stuck at false"
        );
        assert!(
            fixtures::ALL.iter().any(|s| !s.verifies_subscription),
            "control: a corpus with no negative sample cannot catch a predicate stuck at true"
        );

        for s in fixtures::ALL {
            assert_eq!(
                proves_entitled_lookup(s.body),
                s.verifies_subscription,
                "{} was classified wrong — green means the XML subscription is live and \
                 entitled, and a wrong green CLEARS an existing red (#245)",
                s.name
            );
        }

        // The two subscriber-scoped fields, one at a time, so the disjunction is real and not
        // just `<grid>` twice: a DX record has no `<state>`, a US record with no locator on
        // file has no `<grid>`, and either alone is a field a free account is not given.
        for field in ["<grid>DM43bp</grid>", "<state>AZ</state>"] {
            let body = format!(
                "<QRZDatabase><Callsign><call>AA7BQ</call>{field}</Callsign>\
<Session><Key>live</Key></Session></QRZDatabase>"
            );
            assert!(
                proves_entitled_lookup(&body),
                "{field} is subscriber-scoped and proves the lookup was entitled"
            );
            // …and the same record WITHOUT it does not, which is what makes the line above a
            // statement about the field rather than about the envelope.
            let without = body.replace(field, "");
            assert!(
                !proves_entitled_lookup(&without),
                "a record with no subscriber-scoped field proves nothing: {without}"
            );
        }

        // Unknown falls on the not-green side BY CONSTRUCTION, not by enumeration — the whole
        // point of the inversion. Nothing below is a shape QRZ has ever sent.
        for junk in [
            "",
            "<!DOCTYPE html><html><body>QRZ is down</body></html>",
            "<QRZDatabase><Callsign><call>AA7BQ</call><grid>DM43bp</grid></Callsign></QRZDatabase>",
            "<QRZDatabase><Session><Key>live</Key><Grid>DM43bp</Grid></Session></QRZDatabase>",
        ] {
            assert!(
                !proves_entitled_lookup(junk),
                "an unrecognised body must read not-confirmed: {junk}"
            );
        }
    }

    /// ⛔ **THE POSITIVE SIGNAL IS SCOPED TO THE RETURNED `<Callsign>` RECORD (#245, round 7).**
    ///
    /// Round 6 inverted the check to an allow-list, which was the right move, but the allow-list
    /// read `<grid>`/`<state>` as a bare substring ANYWHERE in the body. An adversarial pass showed
    /// four bodies that a non-subscriber (or QRZ itself) can produce that flip the row green with no
    /// entitlement: the tag inside QRZ's own `<Message>`/`<Session>`/`<Error>` prose, or smuggled
    /// into a field VALUE. A green does not merely fail to warn — it CLEARS an existing red. This
    /// pins that the signal must be a DIRECT CHILD of the record the lookup returned.
    ///
    /// Both controls, because one direction is half a test: a real subscriber record must stay
    /// green (or a predicate stuck at `false` passes every attack row), and the free reply must
    /// stay red.
    #[test]
    fn a_subscriber_field_outside_the_callsign_record_does_not_prove_entitlement() {
        // Positive controls — these must NOT change with the scoping.
        assert!(
            proves_entitled_lookup(LOOKUP_FULL),
            "control: a real subscriber record (grid+state, direct children) is still green"
        );
        assert!(
            !proves_entitled_lookup(LOOKUP_FREE),
            "control: the free/lapsed reply is still red"
        );
        // A US record with only <state> and a DX record with only <grid>, each a direct child:
        // either alone still proves entitlement, so the fix did not over-narrow the honest case.
        assert!(
            proves_entitled_lookup(
                "<QRZDatabase><Callsign><call>W1AW</call><state>CT</state></Callsign>\
<Session><Key>live</Key></Session></QRZDatabase>"
            ),
            "control: a real <state> child still proves entitlement"
        );
        assert!(
            proves_entitled_lookup(
                "<QRZDatabase><Callsign><call>DL1ABC</call><grid>JO31</grid></Callsign>\
<Session><Key>live</Key></Session></QRZDatabase>"
            ),
            "control: a real <grid> child still proves entitlement"
        );

        // The four attacks. Each carries a live session and a real <call> in the record, so it is
        // NOT vetoed by `needs_login` or by "a record came back" — the ONLY thing standing between
        // it and a false green is the scoping. Every one must be red.
        let attacks: [(&str, &str); 4] = [
            (
                "grid inside <Message>",
                "<QRZDatabase><Callsign><call>AA7BQ</call><name_fmt>Fred</name_fmt>\
<country>United States</country></Callsign><Session><Key>live</Key>\
<SubExp>non-subscriber</SubExp><Message>upgrade to see <grid>DM43bp</grid></Message>\
</Session></QRZDatabase>",
            ),
            (
                "state inside <Session>",
                "<QRZDatabase><Callsign><call>AA7BQ</call><country>United States</country>\
</Callsign><Session><Key>live</Key><state>AZ</state></Session></QRZDatabase>",
            ),
            (
                "state smuggled inside an <addr2> value",
                "<QRZDatabase><Callsign><call>AA7BQ</call>\
<addr2>Scottsdale<state>AZ</state></addr2></Callsign>\
<Session><Key>live</Key></Session></QRZDatabase>",
            ),
            (
                "non-session <Error> carrying call+grid",
                "<QRZDatabase><Error>Not found: <grid>DM43bp</grid></Error>\
<Callsign><call>AA7BQ</call></Callsign><Session><Key>live</Key></Session></QRZDatabase>",
            ),
        ];
        for (name, body) in attacks {
            assert!(
                !proves_entitled_lookup(body),
                "a subscriber field OUTSIDE the returned record flipped the row green ({name}): \
                 {body}"
            );
        }
    }

    /// ⛔ **A refusal with no `<Callsign>` must not become a fabricated record (#245, round 7 F2).**
    ///
    /// `tag()` is an unscoped substring scan, so a refusal whose `<Error>` prose quotes
    /// `<call>`/`<grid>` used to produce a `QrzOutcome::Found` — a record QRZ never sent, shown to
    /// the operator, AND a green stamp. `parse_callsign` now requires a real `<Callsign>` element,
    /// so a body with none returns `None` (no record) rather than inventing one from loose tags.
    #[test]
    fn a_refusal_with_no_callsign_element_is_not_a_record() {
        // Control: a genuine record still parses.
        assert!(
            parse_callsign(LOOKUP_FREE).is_some(),
            "control: a real <Callsign> record still parses"
        );

        // An <Error> body quoting the record tokens, with NO <Callsign> element at all.
        let error_body = "<QRZDatabase><Session><Key>live</Key></Session>\
<Error>Not found: <call>W1AW</call> <grid>DM43bp</grid> is not covered</Error></QRZDatabase>";
        assert!(
            parse_callsign(error_body).is_none(),
            "a refusal with no <Callsign> was turned into a record: {:?}",
            parse_callsign(error_body)
        );
        assert!(
            !proves_entitled_lookup(error_body),
            "a refusal with no <Callsign> stamped the row green"
        );

        // Two records: the call from one, the grid from another — the scope must not stitch a
        // field from outside the first record onto it.
        let split = "<QRZDatabase><Callsign><call>W1AW</call></Callsign>\
<Callsign><grid>DM43bp</grid></Callsign><Session><Key>live</Key></Session></QRZDatabase>";
        let rec = parse_callsign(split).expect("the first record parses");
        assert_eq!(rec.call, "W1AW");
        assert!(
            rec.grid.is_none(),
            "a grid from a SECOND record was stitched onto the first: {:?}",
            rec.grid
        );
        assert!(
            !proves_entitled_lookup(split),
            "a grid outside the first record stamped it green"
        );
    }

    /// ⛔ **QRZ's `<SubExp>` disqualifies REGARDLESS of grid/state (#245, round 7 F1).**
    ///
    /// Round 6 read only the two positive tokens and ignored the field QRZ provides specifically to
    /// state entitlement, so a body that says `non-subscriber`, and one expired years ago, both
    /// stamped the row green when they carried a grid. QRZ's own definitive negative is now believed.
    #[test]
    fn an_explicit_non_subscriber_or_expired_subexp_is_never_green() {
        // 2026-01-01 UTC — a fixed "now" so the expiry test is deterministic.
        const NOW: i64 = 1_767_225_600;

        let with_grid = |session: &str| {
            format!(
                "<QRZDatabase><Callsign><call>AA7BQ</call><grid>DM43bp</grid></Callsign>\
<Session><Key>live</Key>{session}</Session></QRZDatabase>"
            )
        };

        // Controls, both directions: a real subscriber record with a grid and NO SubExp is green,
        // and a subscriber whose SubExp is a FUTURE year stays green (the check must not red-flag a
        // paid-up account).
        assert!(
            proves_entitled_lookup_at(LOOKUP_FULL, NOW),
            "control: a real subscriber record (no SubExp) is green"
        );
        assert!(
            proves_entitled_lookup_at(&with_grid(""), NOW),
            "control: a grid with no SubExp is green"
        );
        assert!(
            proves_entitled_lookup_at(&with_grid("<SubExp>Wed Jan 1 2031</SubExp>"), NOW),
            "control: a future SubExp with a grid stays green"
        );

        // The finding: an explicit non-subscriber, and an expired subscription, each WITH a grid —
        // both must be red because SubExp overrides the positive token.
        assert!(
            !proves_entitled_lookup_at(&with_grid("<SubExp>non-subscriber</SubExp>"), NOW),
            "a body that says non-subscriber stamped green over a grid"
        );
        assert!(
            !proves_entitled_lookup_at(&with_grid("<SubExp>Wed Jan 1 2020</SubExp>"), NOW),
            "a subscription expired in 2020 stamped green over a grid"
        );
        assert!(
            !proves_entitled_lookup_at(&with_grid("<SubExp>2019-12-31</SubExp>"), NOW),
            "an expired ISO-dated SubExp stamped green over a grid"
        );
    }

    /// ⛔ **The SubExp disqualifier makes the `<state>` NEEDS-BENCH question moot.**
    ///
    /// The green rests on the vendor assumption that QRZ withholds `<state>` from non-subscribers.
    /// `<state>` is FCC-ULS-derivable, so QRZ *might* return it to a free US account — the exact
    /// hypothetical the bench question is about. This is the body that hypothetical produces: a
    /// non-subscriber whose `<Callsign>` carries a real `<state>` DIRECT child. Without the
    /// disqualifier it goes green (the scoping passes it — the field is genuinely in the record);
    /// with it, the explicit `non-subscriber` marker refuses it regardless. So even if the vendor
    /// assumption is false, #245 cannot return through this door.
    #[test]
    fn a_non_subscriber_body_carrying_a_real_state_is_disqualified_by_subexp() {
        const NOW: i64 = 1_767_225_600;

        // The body the bench question is about: a real <state> child on the record, and the session
        // declaring the account a non-subscriber.
        let free_with_state =
            "<QRZDatabase><Callsign><call>W1AW</call><state>CT</state></Callsign>\
<Session><Key>live</Key><SubExp>non-subscriber</SubExp></Session></QRZDatabase>";

        // The CONTROL that proves the disqualifier is what does the work: strip the SubExp and the
        // very same record IS green — so "red" below cannot be an accident of the scoping.
        let same_without_marker = free_with_state.replace("<SubExp>non-subscriber</SubExp>", "");
        assert!(
            proves_entitled_lookup_at(&same_without_marker, NOW),
            "control: a <state> direct child with no SubExp marker is green (the bench scenario)"
        );

        // With QRZ's own non-subscriber marker present, it is red no matter the field.
        assert!(
            !proves_entitled_lookup_at(free_with_state, NOW),
            "a non-subscriber body carrying a real <state> was stamped green"
        );
    }

    #[test]
    fn parses_the_nickname_when_present() {
        let with = "<Callsign><call>W1XYZ</call><name_fmt>John Public</name_fmt>\
                    <nickname>Johnny</nickname></Callsign>";
        let r = parse_callsign(with).unwrap();
        assert_eq!(r.name.as_deref(), Some("John Public"));
        assert_eq!(r.nickname.as_deref(), Some("Johnny"), "nickname is parsed");
        // Absent → None, so the UI falls back to the full name.
        let without = parse_callsign(
            "<Callsign><call>W1XYZ</call><name_fmt>John Public</name_fmt></Callsign>",
        )
        .unwrap();
        assert!(without.nickname.is_none());
    }

    #[test]
    fn no_callsign_block_is_none() {
        assert!(parse_callsign(LOGIN_OK).is_none());
        assert!(parse_callsign(EXPIRED).is_none());
    }

    #[test]
    fn is_qrz_xml_accepts_qrz_rejects_html() {
        assert!(is_qrz_xml(LOGIN_OK));
        assert!(!is_qrz_xml(
            "<!DOCTYPE html><html><title>QRZ.com</title></html>"
        ));
    }

    #[test]
    fn tag_unescapes_entities() {
        assert_eq!(
            parse_callsign(ENTITY_ESCAPED_NAME).unwrap().name.as_deref(),
            Some("Smith & Jones")
        );
    }

    #[test]
    fn key_present_but_invalid_session_error_needs_login() {
        // QRZ can return a present-but-dead key with an "Invalid session key" error;
        // must re-login, not keep reusing the dead key.
        assert!(parse_session(INVALID_SESSION_KEY).needs_login());
    }

    #[test]
    fn attributed_tag_yields_none_not_misbounded_value() {
        // Defends the no-open_attr fix: a (non-QRZ) attributed tag must not return a
        // value mis-bounded at a '>' inside the attribute.
        assert!(parse_callsign(ATTRIBUTED_GRID).unwrap().grid.is_none());
    }

    #[test]
    fn insert_body_encodes_adif_and_key() {
        let body = build_insert_body("AB-12-CD", "<call:4>W1AW<eor>", false);
        assert!(body.starts_with("KEY=AB-12-CD&ACTION=INSERT&ADIF="));
        assert_eq!(
            build_status_body(" AB-12-CD "),
            "KEY=AB-12-CD&ACTION=STATUS"
        );
        // STATUS parse: the success shape QRZ actually returns.
        let st = parse_status_response(
            "RESULT=OK&OWNER=KD9TAW&BOOK_NAME=My+Logbook&COUNT=1234&ACTION=STATUS",
        );
        assert!(st.ok);
        assert_eq!(st.owner.as_deref(), Some("KD9TAW"));
        assert_eq!(st.book.as_deref(), Some("My Logbook"));
        assert_eq!(st.count, 1234);
        // Auth failure carries the reason through.
        let st = parse_status_response("RESULT=AUTH&REASON=invalid+api+key");
        assert!(!st.ok);
        assert_eq!(st.reason.as_deref(), Some("invalid api key"));
        // ADIF tag delimiters must be percent-encoded into the form value.
        assert!(body.contains("ADIF=%3Ccall%3A4%3EW1AW%3Ceor%3E"));
        assert!(!body.contains("&OPTION="));
        assert!(build_insert_body("k", "<eor>", true).ends_with("&OPTION=REPLACE"));
    }

    #[test]
    fn push_response_ok_with_logid() {
        let p = parse_push_response("RESULT=OK&COUNT=1&LOGID=123456");
        assert_eq!(p.result, QrzPushResult::Ok);
        assert_eq!(p.logid.as_deref(), Some("123456"));
        assert_eq!(p.count, 1);
    }

    #[test]
    fn push_response_duplicate_is_benign() {
        // A duplicate is RESULT=FAIL + a "duplicate" reason + COUNT=0 → Duplicate.
        let p = parse_push_response("RESULT=FAIL&COUNT=0&REASON=Unable+to+add+QSO%3A+duplicate");
        assert_eq!(p.result, QrzPushResult::Duplicate);
        assert_eq!(p.count, 0);
        assert!(p.reason.as_deref().unwrap().contains("duplicate"));
    }

    #[test]
    fn push_response_auth_and_plain_fail() {
        assert_eq!(
            parse_push_response("RESULT=AUTH").result,
            QrzPushResult::AuthFail
        );
        assert_eq!(
            parse_push_response("RESULT=FAIL&REASON=bad+ADIF").result,
            QrzPushResult::Fail
        );
    }

    #[test]
    fn push_response_urldecode_does_not_panic_on_multibyte_after_percent() {
        // A network REASON with '%' before a multibyte UTF-8 byte must not panic
        // (the old str-slice decoder split a char boundary).
        let p = parse_push_response("RESULT=FAIL&REASON=oops %€ at end");
        assert_eq!(p.result, QrzPushResult::Fail);
        assert!(p.reason.is_some());
        // A trailing bare '%' is also safe.
        assert_eq!(
            parse_push_response("RESULT=OK&REASON=100%").result,
            QrzPushResult::Ok
        );
    }

    #[test]
    fn push_result_maps_to_upload_outcome() {
        use crate::logbook::UploadOutcome as U;
        assert_eq!(QrzPushResult::Ok.to_upload_outcome(), U::Accepted);
        assert_eq!(QrzPushResult::Replace.to_upload_outcome(), U::Accepted);
        assert_eq!(QrzPushResult::Duplicate.to_upload_outcome(), U::Duplicate);
        assert_eq!(QrzPushResult::AuthFail.to_upload_outcome(), U::AuthFail);
        assert_eq!(QrzPushResult::Fail.to_upload_outcome(), U::Rejected);
    }

    #[test]
    fn delta_fetch_sends_exactly_one_modsince_option() {
        // QRZ's guide contradicts itself on how to COMBINE options (prose says & or ;,
        // the example uses commas), so we send one and sidestep it.
        let b = build_fetch_since_body("abc123", "2026-07-28");
        assert_eq!(b, "KEY=abc123&ACTION=FETCH&OPTION=MODSINCE%3A2026-07-28");
        assert_eq!(b.matches("OPTION=").count(), 1, "exactly one OPTION");
        assert!(!b.contains("STATUS"), "must not filter to confirmed-only");
    }

    #[test]
    fn an_empty_since_falls_back_to_the_full_fetch() {
        assert_eq!(
            build_fetch_since_body("abc123", "   "),
            build_fetch_body("abc123")
        );
    }

    #[test]
    fn never_synced_means_no_since_date_so_the_caller_seeds_with_a_full_fetch() {
        assert_eq!(fetch_since_date(None, 1), None);
    }

    #[test]
    fn the_since_date_reaches_back_past_the_day_boundary() {
        // MODSINCE is DATE granularity, so a sync at 00:30 UTC that passed its own date
        // would miss anything QRZ stamped late on the previous day. 2026-07-28T00:30Z
        // with one day of overlap must ask for the 27th.
        let t = 1_785_197_400u64; // 2026-07-28T00:30:00Z
        assert_eq!(fetch_since_date(Some(t), 1).as_deref(), Some("2026-07-27"));
        assert_eq!(fetch_since_date(Some(t), 0).as_deref(), Some("2026-07-28"));
    }

    #[test]
    fn the_since_date_is_a_real_civil_date_across_a_leap_year() {
        // 2024-03-01T12:00:00Z back one day must be 2024-02-29, not 2024-02-28.
        let t = 1_709_294_400u64;
        assert_eq!(fetch_since_date(Some(t), 1).as_deref(), Some("2024-02-29"));
    }

    #[test]
    fn fetch_body_is_key_and_action() {
        assert_eq!(build_fetch_body("  abc123  "), "KEY=abc123&ACTION=FETCH");
    }

    #[test]
    fn fetch_decodes_html_encoded_adif_payload() {
        // QRZ sends the ADIF with its angle brackets HTML-ENCODED (this is what real
        // FETCH responses look like — the earlier "verbatim/literal" assumption is why
        // sync silently returned 0 QSOs). parse_fetch must decode it back to literal
        // `<...>` so the ADIF importer downstream finds records. `ADIF=` is last and the
        // payload carries `&` (from the entities) + `=` + newlines, so it's cleaved at
        // the field boundary, not split as pairs.
        let encoded =
            "&lt;CALL:5&gt;W1AW&lt;QSO_DATE:8&gt;20240101&lt;APP_QRZLOG_STATUS:1&gt;C&lt;eor&gt;\n";
        let body = format!("RESULT=OK&COUNT=1&ADIF={encoded}");
        let f = parse_fetch(&body);
        assert!(f.ok);
        assert_eq!(f.count, 1);
        assert_eq!(
            f.adif,
            "<CALL:5>W1AW<QSO_DATE:8>20240101<APP_QRZLOG_STATUS:1>C<eor>\n"
        );
        assert!(
            f.adif.contains("<eor>"),
            "records must have real ADIF markers"
        );
        assert!(f.reason.is_none());
    }

    #[test]
    fn fetch_leaves_literal_adif_unchanged() {
        // Decoding is idempotent on already-literal brackets (defensive if QRZ ever
        // stops encoding, or for hand-fed .adi content).
        let adif = "<CALL:5>W1AW<QSO_DATE:8>20240101<eor>\n";
        let f = parse_fetch(&format!("RESULT=OK&COUNT=1&ADIF={adif}"));
        assert_eq!(f.adif, adif);
    }

    #[test]
    fn fetch_reports_failure_reason() {
        let f = parse_fetch("RESULT=FAIL&REASON=invalid+api+key");
        assert!(!f.ok);
        assert_eq!(f.adif, "");
        assert_eq!(f.reason.as_deref(), Some("invalid api key"));
    }

    #[test]
    fn fetch_tolerates_status_alias_and_empty_book() {
        // Some responses use STATUS= and an empty ADIF (nothing to pull).
        let f = parse_fetch("STATUS=OK&COUNT=0&ADIF=");
        assert!(f.ok);
        assert_eq!(f.count, 0);
        assert_eq!(f.adif, "");
    }
}
