//! The internet road's account side, this computer's half: what Nexus on another computer says to
//! the Remote service before the pipe through the station's room opens (`remote/src/index.ts`, THE
//! NATIVE CLIENT), with nothing of HTTP in it, so a phone's shell can carry the same.
//!
//! - **Signed requests** ([`ComputerKey::sign_request`]): every request that acts as this computer's
//!   device for a station is signed by its device key for that station over a one-use challenge the
//!   service gave, the method, the path and the body exactly as sent (`signedBy`). The label starts
//!   what is signed, so a signature made for a request is never one for anything else the key signs.
//! - **The first sign-in** ([`Pkce`], [`authorize_url`], [`callback`]): through the system browser to
//!   the service's Native application, with PKCE (RFC 7636, S256 only) and a redirect to this
//!   computer's loopback address (RFC 8252 §7.3). The issuer, the application and the API are the
//!   service's to say (`GET /api/remote/config`), so none of them is built in here. The token it
//!   gives is kept in memory only, and only for as long as the service says.
//! - **Reopening** needs no sign-in: an approved device's own signature opens its station
//!   again (`stations/:id/client/challenge` and the signed verbs after it), so no account
//!   credential is ever kept on this computer.
use ring::digest::{digest, SHA256};
use ring::rand::{SecureRandom, SystemRandom};

use crate::{hex, ComputerKey};

/// What every native request's signature covers begins with: the service's `NATIVE_REQUEST`.
pub const NATIVE_REQUEST: &str = "nexus-native-request/1";
/// The path this computer's loopback listener takes the browser's redirect on.
pub const CALLBACK_PATH: &str = "/callback";

/// What a sign-in asks the issuer to say about whoever signs in: its `scope`. Never `profile`, so no
/// proof carries a name, a nickname or a picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The shack binding its station, or proving it again: who signed in, and nothing more. Its
    /// proof is listed to every computer of the account and of each person the station invites, so
    /// it carries no address.
    Station,
    /// A computer's or a phone's: who signed in, and the address the issuer verified, which is how
    /// a station knows a person it invited.
    Device,
}

impl Scope {
    /// The `scope` the authorize address carries.
    fn value(self) -> &'static str {
        match self {
            Self::Station => "openid",
            Self::Device => "openid email",
        }
    }
}

/// What a native request's signature covers: the label, the challenge, the method, the path, and
/// SHA-256 of the body exactly as sent, lowercase hex, one to a line.
pub fn signed_lines(challenge: &str, method: &str, path: &str, body: &[u8]) -> Vec<u8> {
    [
        NATIVE_REQUEST,
        challenge,
        method,
        path,
        &hex(digest(&SHA256, body).as_ref()),
    ]
    .join("\n")
    .into_bytes()
}

impl ComputerKey {
    /// This device's signature over a native request ([`signed_lines`]): ECDSA P-256 over SHA-256,
    /// r||s, lowercase hex, as the service checks it. `None` only when the system has no randomness.
    pub fn sign_request(
        &self,
        challenge: &str,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> Option<String> {
        let signed = signed_lines(challenge, method, path, body);
        let signature = self.pair.sign(&SystemRandom::new(), &signed).ok()?;
        Some(hex(signature.as_ref()))
    }
}

/// Bytes as base64url with no padding (RFC 4648 §5), PKCE's alphabet.
pub fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
        }
    }
    out
}

/// One sign-in's secrets (RFC 7636): the verifier this computer keeps until the code comes back, the
/// challenge the browser carries (S256), and the state the redirect must bring back unchanged. Made
/// for one sign-in, never stored, and no `Debug` or `Clone`.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

impl Pkce {
    /// A new sign-in's: 32 random bytes for the verifier and 16 for the state. `None` when the
    /// system has no randomness, when there is no safe sign-in to start.
    pub fn new() -> Option<Self> {
        let random = SystemRandom::new();
        let mut verifier = [0u8; 32];
        let mut state = [0u8; 16];
        random.fill(&mut verifier).ok()?;
        random.fill(&mut state).ok()?;
        Some(Self::of(&base64url(&verifier), base64url(&state)))
    }

    /// The challenge a verifier makes: base64url of its SHA-256.
    pub fn of(verifier: &str, state: String) -> Self {
        Self {
            verifier: verifier.to_string(),
            challenge: base64url(digest(&SHA256, verifier.as_bytes()).as_ref()),
            state,
        }
    }
}

/// A query value, percent-encoded: every byte but RFC 3986's unreserved ones.
fn encoded(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A query value decoded, or `None` for one that is not percent-encoded UTF-8.
fn decoded(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let pair = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(pair, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// What the service says its sign-in is (`GET /api/remote/config`): the issuer, the Native
/// application's client id and the API its tokens are for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignIn {
    pub issuer: String,
    pub client_id: String,
    pub audience: String,
}

impl SignIn {
    /// The service's config, or `None` when it names no Native application (the service has none
    /// yet) or an issuer that is not an https address ending in `/`, as the service holds its own.
    pub fn from_config(config: &serde_json::Value) -> Option<Self> {
        let text = |name: &str| {
            config
                .get(name)
                .and_then(serde_json::Value::as_str)
                .filter(|v| !v.is_empty() && v.len() <= 512)
                .map(str::to_string)
        };
        let issuer = text("issuer")?;
        let plain = issuer.starts_with("https://")
            && issuer.ends_with('/')
            && !issuer.contains(['?', '#', ' ']);
        plain.then_some(Self {
            issuer,
            client_id: text("nativeClientId")?,
            audience: text("audience")?,
        })
    }
}

/// The address the system browser opens for the first sign-in: the issuer's authorize endpoint,
/// asking for `scope`, with the code returned to `redirect` (this computer's loopback listener)
/// under `pkce`. With a `nonce` (what the sign-in vouches for, `crate::account`), the ID token the
/// issuer gives back carries it; without one the address has no nonce.
pub fn authorize_url(
    sign_in: &SignIn,
    redirect: &str,
    pkce: &Pkce,
    scope: Scope,
    nonce: Option<&str>,
) -> String {
    let query = [
        ("response_type", "code"),
        ("client_id", &sign_in.client_id),
        ("redirect_uri", redirect),
        ("scope", scope.value()),
        ("audience", &sign_in.audience),
        ("code_challenge", &pkce.challenge),
        ("code_challenge_method", "S256"),
        ("state", &pkce.state),
    ]
    .into_iter()
    .chain(nonce.map(|nonce| ("nonce", nonce)))
    .map(|(name, value)| format!("{name}={}", encoded(value)))
    .collect::<Vec<_>>()
    .join("&");
    format!("{}authorize?{query}", sign_in.issuer)
}

/// The form this computer posts to the issuer's token endpoint for the code the redirect brought.
pub fn token_form(sign_in: &SignIn, redirect: &str, pkce: &Pkce, code: &str) -> String {
    [
        ("grant_type", "authorization_code"),
        ("client_id", sign_in.client_id.as_str()),
        ("code_verifier", pkce.verifier.as_str()),
        ("code", code),
        ("redirect_uri", redirect),
    ]
    .iter()
    .map(|(name, value)| format!("{name}={}", encoded(value)))
    .collect::<Vec<_>>()
    .join("&")
}

/// The redirect's request target as the loopback listener read it (`/callback?code=…&state=…`): the
/// code, when it came back with this sign-in's state. When the issuer said no instead
/// ([`issuer_said`]): `signInRefused` for the operator's own cancel at the consent screen,
/// `emailUnverified` for an address not confirmed yet (the tenant's login Action denies those, the
/// rule the web page's `client.ts` reads the same way), and `signInFailed` for any other error.
/// `signInFailed` too for a redirect that is not this sign-in's: another path, another state, no
/// code.
pub fn callback(target: &str, state: &str) -> Result<String, &'static str> {
    let Some(query) = ours(target, state) else {
        return Err("signInFailed");
    };
    if let Some((error, description)) = said_in(query) {
        return Err(refusal(&error, &description));
    }
    query
        .split('&')
        .filter_map(|pair| pair.strip_prefix("code="))
        .find_map(decoded)
        .filter(|code| !code.is_empty() && code.len() <= 2048)
        .ok_or("signInFailed")
}

/// The issuer's `error` and `error_description` (empty when it gave none) when the redirect is this
/// sign-in's and says no, for the diagnostic log: never the code, which a refusal does not carry.
/// `None` for a redirect that brought a code, or that this sign-in did not start.
pub fn issuer_said(target: &str, state: &str) -> Option<(String, String)> {
    said_in(ours(target, state)?)
}

/// The query of a redirect to [`CALLBACK_PATH`] that carries this sign-in's state, else `None`: a
/// redirect this sign-in did not start says nothing, refusal or code.
fn ours<'a>(target: &'a str, state: &str) -> Option<&'a str> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let came_back = query
        .split('&')
        .filter_map(|pair| pair.strip_prefix("state="))
        .find_map(decoded);
    (path == CALLBACK_PATH && came_back.as_deref() == Some(state)).then_some(query)
}

/// The `error` a query carries and its `error_description`, when it carries an error.
fn said_in(query: &str) -> Option<(String, String)> {
    let value = |name: &str| {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('=').filter(|(key, _)| *key == name))
            .find_map(|(_, value)| decoded(value))
    };
    let error = query
        .split('&')
        .any(|pair| pair == "error" || pair.starts_with("error="))
        .then(|| value("error").unwrap_or_default())?;
    Some((error, value("error_description").unwrap_or_default()))
}

/// What the issuer's no means for the operator. Its consent screen's Decline is `access_denied`
/// with "User did not authorize the request" (or no description): a cancel. The tenant's login
/// Action denying an unconfirmed address is `access_denied` with its own sentence about verifying
/// the email: confirm it, then sign in again. Anything else is a sign-in that did not finish.
fn refusal(error: &str, description: &str) -> &'static str {
    let text = description.to_ascii_lowercase();
    let unverified = text == "email_unverified"
        || (text.contains("verif") && (text.contains("email") || text.contains("e-mail")));
    match error {
        "access_denied" if unverified => "emailUnverified",
        "access_denied" if text.is_empty() || text == "user did not authorize the request" => {
            "signInRefused"
        }
        _ => "signInFailed",
    }
}

/// The token the issuer's answer carries, and how long it lasts (seconds, at most an hour, as the
/// service holds a token to), or `None` for an answer that is not one.
pub fn token(answer: &serde_json::Value) -> Option<(String, u64)> {
    let token = answer.get("access_token")?.as_str()?;
    let bearer = answer
        .get("token_type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("bearer"));
    let shaped = bearer
        && !token.is_empty()
        && token.len() <= 8192
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.~+/=".contains(&b));
    let lasts = answer.get("expires_in")?.as_u64()?.min(3600);
    (shaped && lasts > 0).then(|| (token.to_string(), lasts))
}

/// The ID token the issuer's answer carries (the sign-in asks for `openid`), or `None` when it has
/// none or one that is not three base64url parts of a token's size. Taken as it came: whoever keeps
/// it checks it (`crate::account::verify_proof`).
pub fn id_token(answer: &serde_json::Value) -> Option<String> {
    let token = answer.get("id_token")?.as_str()?;
    let shaped = token.len() <= 4096
        && token.split('.').count() == 3
        && token.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        });
    shaped.then(|| token.to_string())
}

/// One TURN server the service's `turn` route named: the host and port of a plain `turn:` URL over
/// UDP, which is what this computer's TURN client speaks, and its credential. No `Debug`: the
/// credential goes nowhere but that server.
#[derive(Clone, PartialEq, Eq)]
pub struct TurnServer {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

/// The TURN servers in the `turn` route's answer (`{"iceServers":[{urls,username,credential}]}`),
/// UDP only, in the order given: a `turns:` URL or a `?transport=tcp` one is left out, since this
/// computer's TURN client sends datagrams. At most four, as the service names at most four.
pub fn turn_servers(answer: &serde_json::Value) -> Vec<TurnServer> {
    let Some(servers) = answer.get("iceServers").and_then(|s| s.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for server in servers {
        let text = |name: &str| {
            server
                .get(name)
                .and_then(serde_json::Value::as_str)
                .filter(|v| !v.is_empty() && v.len() <= 1024)
        };
        let (Some(username), Some(password)) = (text("username"), text("credential")) else {
            continue;
        };
        let urls: Vec<&str> = match server.get("urls") {
            Some(serde_json::Value::String(url)) => vec![url.as_str()],
            Some(serde_json::Value::Array(urls)) => {
                urls.iter().filter_map(|u| u.as_str()).collect()
            }
            _ => continue,
        };
        for url in urls {
            let Some(rest) = url.strip_prefix("turn:") else {
                continue;
            };
            let (at, transport) = rest.split_once('?').unwrap_or((rest, "transport=udp"));
            if transport != "transport=udp" {
                continue;
            }
            let Some((host, port)) = at.rsplit_once(':') else {
                continue;
            };
            let named = !host.is_empty()
                && host.len() <= 253
                && host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
            let Some(port) = port.parse::<u16>().ok().filter(|p| *p != 0) else {
                continue;
            };
            if named && out.len() < 4 {
                out.push(TurnServer {
                    host: host.to_ascii_lowercase(),
                    port,
                    username: username.to_string(),
                    password: password.to_string(),
                });
            }
        }
    }
    out
}

/// Whether the account's entitlement takes in the internet road now (N4: the internet road is the
/// paid service's, the LAN is free). The service's own answer when it names one (`access`: only
/// `"internet"` takes it in); a service that names none has only the trial, which takes it in while
/// it is on and has not ended at `now_ms` (ms since the epoch). The service refuses the pipe by the
/// same rule whatever this says: this is what the screen tells the operator before asking.
pub fn internet_included(entitlement: &serde_json::Value, now_ms: u64) -> bool {
    if let Some(access) = entitlement.get("access") {
        return access.as_str() == Some("internet");
    }
    let enabled = entitlement
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let until = entitlement
        .get("expiresAt")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    enabled && until > now_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED};

    /// ★ RFC 7636's own example (Appendix B): the verifier makes the challenge the RFC gives, so the
    /// issuer computes the same one. CONTROL: one character of the verifier changed makes another.
    #[test]
    fn the_challenge_is_rfc_7636s_s256() {
        let pkce = Pkce::of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk", "s".into());
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let other = Pkce::of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXl", "s".into());
        assert_ne!(other.challenge, pkce.challenge);
        let fresh = Pkce::new().unwrap();
        assert_eq!(fresh.verifier.len(), 43);
        assert_eq!(fresh.state.len(), 22);
        assert_ne!(fresh.verifier, Pkce::new().unwrap().verifier);
    }

    /// RFC 4648's test vectors, base64url with no padding.
    #[test]
    fn base64url_is_rfc_4648s() {
        for (bytes, text) in [
            ("", ""),
            ("f", "Zg"),
            ("fo", "Zm8"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64url(bytes.as_bytes()), text);
        }
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    /// ★ A signed request is the service's five lines, signed by this device's key: its public key
    /// verifies it, over exactly those lines. CONTROL: the same signature over a body one byte
    /// different, another path, or another challenge does not verify.
    #[test]
    fn a_request_is_signed_over_the_services_five_lines() {
        let (key, _) = ComputerKey::generate().unwrap();
        let challenge = "ab".repeat(32);
        let body = br#"{"name":"Desk","kind":"computer","platform":"windows"}"#;
        let path = "/api/remote/stations/60000000-0000-4000-8000-000000000001/client/device";
        let signature = key.sign_request(&challenge, "POST", path, body).unwrap();
        assert_eq!(signature.len(), 128);
        let lines = String::from_utf8(signed_lines(&challenge, "POST", path, body)).unwrap();
        let sha = hex(digest(&SHA256, body).as_ref());
        assert_eq!(
            lines,
            format!("nexus-native-request/1\n{challenge}\nPOST\n{path}\n{sha}")
        );
        let spki = tempo_stream::protocol::hex_bytes(key.public_key()).unwrap();
        let point = &spki[spki.len() - 65..];
        let verifier = UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, point);
        let sig = tempo_stream::protocol::hex_bytes(&signature).unwrap();
        assert!(verifier.verify(lines.as_bytes(), &sig).is_ok());
        for (c, p, b) in [
            (challenge.as_str(), path, &b"{}"[..]),
            (challenge.as_str(), "/api/remote/client/session", &body[..]),
            (&"cd".repeat(32), path, &body[..]),
        ] {
            assert!(
                verifier
                    .verify(&signed_lines(c, "POST", p, b), &sig)
                    .is_err(),
                "a signature passed for another request"
            );
        }
    }

    /// ★ The issuer's no is told by what it said. The tenant's login Action refusing an unconfirmed
    /// address comes back as `access_denied` with its own sentence; it is never a cancel. Nor is any
    /// other issuer error. CONTROL: the operator's own Decline at the consent screen stays a cancel.
    #[test]
    fn an_unverified_address_is_not_told_as_a_cancel() {
        let denied = "/callback?error=access_denied&error_description=Please%20verify%20your%20email%20address%20using%20the%20link%20we%20sent%2C%20then%20sign%20in%20again.&state=xyz";
        assert_eq!(callback(denied, "xyz"), Err("emailUnverified"));
        let coded = "/callback?error=access_denied&error_description=email_unverified&state=xyz";
        assert_eq!(callback(coded, "xyz"), Err("emailUnverified"));
        // Nor any other issuer error that is not the operator's own cancel.
        let audience = "/callback?error=access_denied&error_description=Service%20not%20found%3A%20https%3A%2F%2Fexample&state=xyz";
        assert_eq!(callback(audience, "xyz"), Err("signInFailed"));
        let client = "/callback?error=unauthorized_client&error_description=Grant%20type%20not%20allowed&state=xyz";
        assert_eq!(callback(client, "xyz"), Err("signInFailed"));
        // CONTROL: the operator's own cancel at the consent screen stays a cancel.
        let declined = "/callback?error=access_denied&error_description=User%20did%20not%20authorize%20the%20request&state=xyz";
        assert_eq!(callback(declined, "xyz"), Err("signInRefused"));
        // An unverified address under ANOTHER state says nothing.
        assert_eq!(
            callback(&denied.replace("xyz", "other"), "xyz"),
            Err("signInFailed")
        );
    }

    /// What the issuer said, for the log: only under this sign-in's state, and only for a no.
    #[test]
    fn the_issuers_no_is_read_for_the_log_only_under_this_state() {
        assert_eq!(
            issuer_said(
                "/callback?error=unauthorized_client&error_description=Grant%20type&state=xyz",
                "xyz"
            ),
            Some(("unauthorized_client".into(), "Grant type".into()))
        );
        assert_eq!(
            issuer_said("/callback?error=access_denied&state=xyz", "xyz"),
            Some(("access_denied".into(), String::new()))
        );
        assert_eq!(
            issuer_said("/callback?error=access_denied&state=other", "xyz"),
            None
        );
        assert_eq!(issuer_said("/callback?code=c&state=xyz", "xyz"), None);
    }

    /// ★ The redirect's code is taken only with this sign-in's state; a refusal is told as one,
    /// and only under the same state. CONTROL: the same redirect with another state is refused.
    #[test]
    fn the_code_comes_back_only_with_this_sign_ins_state() {
        assert_eq!(
            callback("/callback?code=a%2Fb&state=xyz", "xyz"),
            Ok("a/b".into())
        );
        assert_eq!(
            callback("/callback?state=xyz&code=c", "xyz"),
            Ok("c".into())
        );
        assert_eq!(
            callback("/callback?code=c&state=xyy", "xyz"),
            Err("signInFailed")
        );
        assert_eq!(callback("/callback?code=c", "xyz"), Err("signInFailed"));
        assert_eq!(
            callback("/callback?error=access_denied&state=xyz", "xyz"),
            Err("signInRefused")
        );
        assert_eq!(
            callback("/callback?error=access_denied&state=other", "xyz"),
            Err("signInFailed")
        );
        assert_eq!(
            callback("/other?code=c&state=xyz", "xyz"),
            Err("signInFailed")
        );
        assert_eq!(
            callback("/callback?code=&state=xyz", "xyz"),
            Err("signInFailed")
        );
    }

    /// The authorize address carries the service's Native application, its API, the loopback
    /// redirect and S256, every value encoded; the token form carries the verifier.
    #[test]
    fn the_authorize_address_and_the_token_form() {
        let sign_in = SignIn::from_config(&serde_json::json!({
            "issuer": "https://login.example/", "nativeClientId": "native-id",
            "audience": "https://api.example/remote", "clientId": "spa-id"
        }))
        .unwrap();
        let pkce = Pkce::of("verifier-verifier-verifier-verifier-verifi", "st".into());
        let url = authorize_url(
            &sign_in,
            "http://127.0.0.1:5123/callback",
            &pkce,
            Scope::Device,
            None,
        );
        assert!(url.starts_with(
            "https://login.example/authorize?response_type=code&client_id=native-id&"
        ));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5123%2Fcallback"));
        assert!(url.contains("audience=https%3A%2F%2Fapi.example%2Fremote"));
        assert!(url.contains("&scope=openid%20email&"));
        assert!(url.contains(&format!("code_challenge={}", pkce.challenge)));
        assert!(url.contains("code_challenge_method=S256&state=st"));
        assert!(!url.contains("spa-id"));
        let form = token_form(
            &sign_in,
            "http://127.0.0.1:5123/callback",
            &pkce,
            "the/code",
        );
        assert_eq!(
            form,
            "grant_type=authorization_code&client_id=native-id&code_verifier=verifier-verifier-verifier-verifier-verifi&code=the%2Fcode&redirect_uri=http%3A%2F%2F127.0.0.1%3A5123%2Fcallback"
        );
    }

    /// The service's config is taken only with a Native application and a plain https issuer.
    #[test]
    fn a_service_with_no_native_application_has_no_sign_in() {
        let config = |issuer: &str, native: Option<&str>| {
            SignIn::from_config(&serde_json::json!({
                "issuer": issuer, "nativeClientId": native, "audience": "https://api.example/"
            }))
        };
        assert!(config("https://login.example/", Some("n")).is_some());
        assert!(config("https://login.example/", None).is_none());
        assert!(config("https://login.example/", Some("")).is_none());
        assert!(config("http://login.example/", Some("n")).is_none());
        assert!(config("https://login.example", Some("n")).is_none());
        assert!(config("https://login.example/?x=1/", Some("n")).is_none());
    }

    /// The issuer's answer: a bearer token, its life held to an hour.
    #[test]
    fn the_token_lasts_at_most_an_hour() {
        let answer = |token: &str, kind: &str, lasts: u64| {
            token_of(
                serde_json::json!({"access_token": token, "token_type": kind, "expires_in": lasts}),
            )
        };
        fn token_of(v: serde_json::Value) -> Option<(String, u64)> {
            token(&v)
        }
        assert_eq!(
            answer("a.b.c", "Bearer", 86400),
            Some(("a.b.c".into(), 3600))
        );
        assert_eq!(answer("a.b.c", "bearer", 60), Some(("a.b.c".into(), 60)));
        assert_eq!(answer("a.b.c", "mac", 60), None);
        assert_eq!(answer("", "Bearer", 60), None);
        assert_eq!(answer("a b", "Bearer", 60), None);
        assert_eq!(answer("a.b.c", "Bearer", 0), None);
    }

    /// The authorize address carries a nonce only when the sign-in has one, and is otherwise the same.
    #[test]
    fn the_authorize_address_carries_the_nonce_it_is_given() {
        let sign = SignIn {
            issuer: "https://login.example/".into(),
            client_id: "native".into(),
            audience: "https://api.example/".into(),
        };
        let pkce = Pkce::of("verifier", "state-1".into());
        let redirect = "http://127.0.0.1:42078/callback";
        let plain = authorize_url(&sign, redirect, &pkce, Scope::Device, None);
        assert_eq!(
            plain,
            format!(
                "https://login.example/authorize?response_type=code&client_id=native&redirect_uri=\
                 http%3A%2F%2F127.0.0.1%3A42078%2Fcallback&scope=openid%20email&audience=\
                 https%3A%2F%2Fapi.example%2F&code_challenge={}&code_challenge_method=S256&\
                 state=state-1",
                pkce.challenge
            )
        );
        let with = authorize_url(&sign, redirect, &pkce, Scope::Device, Some("n-1_x"));
        assert_eq!(with, format!("{plain}&nonce=n-1_x"));
    }

    /// ★ Rulings 547 and 548: no sign-in asks for the profile (a name, a nickname, a picture), and the
    /// station's asks for no address either, since its proof is listed to every computer of the
    /// account and of each person the station invites. A device's asks for the address the issuer
    /// verified: how a station knows a person it invited. Each read off the address by value.
    #[test]
    fn a_station_asks_only_who_signed_in_and_a_device_its_address_too() {
        let sign = SignIn {
            issuer: "https://login.example/".into(),
            client_id: "native".into(),
            audience: "https://api.example/".into(),
        };
        let pkce = Pkce::of("verifier", "state-1".into());
        let scope_of = |scope: Scope| {
            let url = authorize_url(&sign, "http://127.0.0.1:42078/callback", &pkce, scope, None);
            url.split(['?', '&'])
                .filter_map(|pair| pair.strip_prefix("scope="))
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(scope_of(Scope::Station), ["openid"]);
        assert_eq!(scope_of(Scope::Device), ["openid%20email"]);
    }

    /// The ID token is taken only in a token's shape; nothing about it is trusted here.
    #[test]
    fn the_id_token_is_kept_in_a_tokens_shape() {
        let of = |v: serde_json::Value| id_token(&v);
        assert_eq!(
            of(serde_json::json!({"id_token": "aA0-_.bB.c"})),
            Some("aA0-_.bB.c".into())
        );
        assert_eq!(of(serde_json::json!({"access_token": "a.b.c"})), None);
        assert_eq!(of(serde_json::json!({"id_token": "a.b"})), None);
        assert_eq!(of(serde_json::json!({"id_token": "a..c"})), None);
        assert_eq!(of(serde_json::json!({"id_token": "a.b=.c"})), None);
        let long = format!("a.{}.c", "b".repeat(4096));
        assert_eq!(of(serde_json::json!({ "id_token": long })), None);
    }

    /// ★ The `turn` route's answer read as the TURN servers this computer's client can use: plain
    /// `turn:` over UDP, a single URL or a list, in order; `turns:`, TCP, a URL with no port or a
    /// bad host, and an entry with no credential are left out, and never more than four.
    #[test]
    fn the_turn_routes_answer_gives_the_udp_servers() {
        let answer = serde_json::json!({"iceServers":[
            {"urls":["turn:turn.example.net:3478?transport=udp","turns:turn.example.net:5349?transport=tcp",
                "turn:turn.example.net:3478?transport=tcp","turn:Other.Example.net:80"],
             "username":"u1","credential":"p1"},
            {"urls":"turn:third.example.net:3478","username":"u2","credential":"p2"},
            {"urls":["turn:nocred.example.net:3478"],"username":"u3"},
            {"urls":["turn:bad host:3478","turn:noport.example.net","turn:zero.example.net:0"],
             "username":"u4","credential":"p4"}
        ]});
        let servers = turn_servers(&answer);
        let named: Vec<(String, u16, String, String)> = servers
            .into_iter()
            .map(|s| (s.host, s.port, s.username, s.password))
            .collect();
        assert_eq!(
            named,
            vec![
                ("turn.example.net".into(), 3478, "u1".into(), "p1".into()),
                ("other.example.net".into(), 80, "u1".into(), "p1".into()),
                ("third.example.net".into(), 3478, "u2".into(), "p2".into()),
            ]
        );
        let many = serde_json::json!({"iceServers":[{"urls":["turn:a.net:1","turn:b.net:2",
            "turn:c.net:3","turn:d.net:4","turn:e.net:5"],"username":"u","credential":"p"}]});
        assert_eq!(turn_servers(&many).len(), 4);
        assert!(turn_servers(&serde_json::json!({})).is_empty());
    }

    /// ★ N4's seam: the service's `access` decides when it names one, only `internet` taking in the
    /// internet road, whatever the trial says; with none, the trial does, while on and unexpired.
    #[test]
    fn the_internet_road_is_the_entitlements_to_give() {
        let now = 1_000_000;
        let trial = |enabled: bool, until: u64| serde_json::json!({"enabled":enabled,"expiresAt":until,"state":"active"});
        assert!(internet_included(&trial(true, now + 1), now));
        assert!(!internet_included(&trial(true, now), now), "ended");
        assert!(!internet_included(&trial(false, now + 1), now), "disabled");
        assert!(!internet_included(&serde_json::json!({}), now));
        let paid = |access: &str| {
            let mut e = trial(false, 0);
            e["access"] = serde_json::json!(access);
            e
        };
        assert!(
            internet_included(&paid("internet"), now),
            "paid, trial over"
        );
        let mut lan = trial(true, now + 1);
        lan["access"] = serde_json::json!("lan");
        assert!(!internet_included(&lan, now), "LAN only, trial or not");
        assert!(!internet_included(&paid("none"), now));
    }
}
