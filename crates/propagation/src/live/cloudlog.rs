//! Cloudlog / Wavelog QSO upload (HTTP JSON). Cloudlog (and its Wavelog fork) are self-hosted
//! web logbooks with an identical QSO API: `POST {base}/index.php/api/qso` with the instance
//! API key + station-profile id + one ADIF record. The URL + JSON builders are pure (unit-
//! tested); [`upload`] does the blocking POST.

/// How much of the server's own words to show, in characters.
///
/// A Cloudlog/Wavelog `reason` is a short sentence; what else can arrive on this socket is a
/// reverse proxy's HTML error page or a PHP notice. The bound is not about screen space —
/// the panel row already truncates with `text-overflow: ellipsis` (`ui/src/styles.css`
/// `.conn-when`) — it is about where the string GOES: `src-tauri/src/lib.rs` puts it in the
/// 200-entry in-memory connection log and writes it into `conn-health.json` on every change,
/// so an unbounded body is an unbounded repeated disk write. 160 leaves room for a real
/// sentence and cuts a web page off at its title, which is the part that identifies it.
///
/// Measured against a 50 000-character HTML error page: the whole operator-facing message
/// comes out at **201 characters** (this bound, the ellipsis, and the longest prefix
/// `classify` builds). A genuine Cloudlog reason lands well inside it — the reported
/// station-profile rejection measures 96.
///
/// ⚠️ It is a SIZE bound and nothing else. It was once described as a backstop for the API-key
/// scrub; it never was one, and the arithmetic says so plainly — a Cloudlog key is 33
/// characters and this is 160, so a key echoed near-verbatim fits with 120 to spare. The
/// scrub's backstop is [`echoes_key`], which fails closed.
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
fn classify_body(text: &str, reason: Option<&str>) -> Result<String, String> {
    let t = text.to_ascii_lowercase().replace(' ', "");
    if t.contains("\"status\":\"failed\"") || t.contains("\"status\":\"error\"") {
        return Err(match reason {
            Some(r) => format!("Cloudlog rejected the QSO: {r}"),
            None => "Cloudlog rejected the QSO — check the instance log".to_string(),
        });
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

/// ⛔ CREDENTIAL. Could a reader recover the API key from `text`?
///
/// The literal `str::replace` in [`server_reason`] catches a verbatim echo and nothing else:
/// **one re-encoded character defeats it**. A server that echoes the request body does not
/// have to echo it byte for byte — a PHP notice HTML-escapes what it prints, a WAF page
/// percent-encodes it, a JSON error writes it as `\uXXXX`, and a zero-width character
/// anywhere inside it leaves something that still reads as the key on screen. And truncation
/// is not the backstop it was once described as: see [`REASON_MAX_CHARS`].
///
/// So this asks a much weaker question than "is the key present". Ignoring every
/// non-alphanumeric, and again with escape-shaped runs deleted, does **any
/// [`KEY_WINDOW`]-character stretch** of the key appear? A window rather than the whole key
/// because encoding a single *letter* (`&#99;l0udl0g…`) drops one character out of the middle
/// of the projection and whole-key containment then sees nothing — which is the same
/// one-character defeat as `str::replace`, only better disguised.
///
/// Weaker is the point: a false positive costs a sentence, a false negative writes a
/// credential into a world-readable file that nothing ever cleans up.
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
/// 1. **The API key is scrubbed, and the scrub fails closed.** The key rides in the REQUEST
///    body, and a debug-mode PHP notice or a WAF page can echo a request straight back. This
///    string does not stop at the panel — `src-tauri/src/lib.rs` keeps it in the connection
///    log and writes it into `conn-health.json`, which is world-readable and persisted — so an
///    echoed key would land on disk in cleartext. The literal replacement below catches a
///    verbatim echo; [`echoes_key`] catches the rest, and when it fires **none of the body is
///    shown**. There is no safe way to excise a key from a string that has been re-encoded
///    around it, and a lost sentence is recoverable where a leaked credential is not.
/// 2. **It is flattened to one line and cut to [`REASON_MAX_CHARS`].** A size bound, not a
///    security one — see that constant.
///
/// A JSON answer's explanation is read from its named field, because the object as a whole is
/// machine shape rather than words for an operator. A body that is not JSON at all — a
/// reverse proxy's HTML page, a PHP notice — IS the message, and a bounded slice of it is
/// worth showing: knowing a proxy answered instead of Cloudlog is the actionable half.
fn server_reason(text: &str, key: &str) -> Option<String> {
    let k = key.trim();
    let scrubbed = match k {
        "" => text.to_string(),
        k => text.replace(k, "[api key]"),
    };
    let words = match serde_json::from_str::<serde_json::Value>(&scrubbed) {
        Ok(v) => ["reason", "message", "error"]
            .iter()
            .find_map(|f| v.get(f).and_then(serde_json::Value::as_str))
            .or_else(|| v.as_str())
            .map(str::to_string)?,
        Err(_) => scrubbed.clone(),
    };
    // Fail closed. Both views are checked: `scrubbed` is the body as it arrived, and `words`
    // is what serde produced from it — by which point any `\uXXXX` the server escaped the key
    // with has already been decoded back into the key itself.
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

/// POST one ADIF record to a Cloudlog/Wavelog instance. `Ok(body)` when the record is actually
/// filed; a redacted error otherwise (the API key is in the REQUEST body — never echoed into an
/// error string). Enforces HTTPS + no redirects so a credential-bearing request can't be
/// downgraded onto cleartext, matching every sibling connector.
pub fn upload(base_url: &str, key: &str, station_id: &str, adif: &str) -> Result<String, String> {
    if key.trim().is_empty() {
        return Err("Cloudlog API key is empty — set it in Settings".to_string());
    }
    if station_id.trim().is_empty() {
        return Err("Cloudlog station profile id is empty — set it in Settings".to_string());
    }
    let url = api_url(base_url);
    let body = build_body(key, station_id, adif);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "couldn't build HTTP client".to_string())?;
    let resp = client
        .post(&url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .map_err(|_| {
            "Cloudlog/Wavelog unreachable — check the URL (must be https://)".to_string()
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
fn classify(status: u16, text: &str, key: &str) -> Result<String, String> {
    let reason = server_reason(text, key);
    if (200..300).contains(&status) {
        return classify_body(text, reason.as_deref());
    }
    let what = if status == 401 || status == 403 {
        "rejected the credentials"
    } else {
        "refused the upload"
    };
    Err(match reason {
        Some(r) => format!("Cloudlog HTTP {status} — {what}: {r}"),
        None if status == 401 || status == 403 => format!(
            "Cloudlog HTTP {status} — {what}, and said no more. Check the API key and the \
             station profile id (a key is scoped to one profile)."
        ),
        None => format!("Cloudlog HTTP {status} — {what}, and said no more."),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        .unwrap_err();
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
        .unwrap_err();
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
        let err = classify(403, &body, KEY).unwrap_err();
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
            let err = classify(403, &body, KEY).unwrap_err();
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
        .unwrap_err();
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
        let err = classify(502, &hostile, KEY).unwrap_err();
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
        let err = classify(403, &body, KEY).unwrap_err();
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

    #[test]
    fn a_server_that_says_nothing_still_gets_the_old_actionable_guess() {
        // The control for the three above: when there is no reason to surface, the message
        // must not become emptier than it was.
        let err = classify(401, "", KEY).unwrap_err();
        assert!(
            err.contains("API key"),
            "no-reason fallback lost its hint: {err}"
        );
        let err = classify(500, "", KEY).unwrap_err();
        assert!(err.contains("500"), "the status must still be named: {err}");
    }
}
