//! Signing in to the Remote service's account through the system browser, for both ends of Remote:
//! the shack binding itself to its owner (`remote_service::account`), and a computer adding a
//! station with the account (`lan_client::account`). One sign-in, so the two cannot drift: PKCE
//! (RFC 7636, S256 only) and a redirect to this computer's loopback address (RFC 8252 §7.3), on the
//! one port the Native application allows.
//!
//! A sign-in may carry a `nonce`, the hash of the keys it vouches for (`tempo_client::account`).
//! The issuer's answer then carries an ID token with that nonce, which is kept beside the access
//! token for the caller to check and forward. Neither is ever written to a log or kept past the
//! exchange that needs it. What the token says about whoever signed in is the caller's [`Scope`]:
//! the shack's asks only who, a computer's also the address the issuer verified.
use std::time::Duration;

use serde_json::Value;
use tempo_client::native::{self, Pkce, Scope, SignIn};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The loopback port the sign-in's redirect comes back to: the Native application's allowed
/// callback is exactly `http://127.0.0.1:42078/callback`.
pub(crate) const SIGN_IN_PORT: u16 = 42078;
/// How long the browser has to come back with the code.
pub(crate) const SIGN_IN_WAIT: Duration = Duration::from_secs(5 * 60);
/// The most of the issuer's token answer read.
const ANSWER_LIMIT: usize = 16384;

/// What a sign-in gave: the access token for the service's API, and the ID token when the sign-in
/// carried a nonce. No `Debug` and no `Clone`: nothing may print them.
pub(crate) struct Signed {
    pub(crate) access: String,
    pub(crate) id_token: Option<String>,
}

/// One sign-in through the system browser to `sign`'s issuer and application, asking for `scope`,
/// on `listener`, this computer's loopback port, already bound for the redirect (each caller binds
/// [`SIGN_IN_PORT`] first, so a port that is taken is `signInPortBusy` before any browser opens):
/// the browser sent to the issuer by `open`, and the code it brings back within `wait` exchanged
/// for tokens over `http`. With a `nonce`, an answer with no ID token is `signInFailed`: whoever
/// asked for one cannot go on without it. `signInTimedOut` after `wait`; when the issuer said no,
/// `signInRefused` (a cancel), `emailUnverified` or `signInFailed` ([`redirected`]).
pub(crate) async fn signed_in(
    http: &reqwest::Client,
    sign: &SignIn,
    scope: Scope,
    nonce: Option<&str>,
    listener: TcpListener,
    wait: Duration,
    open: impl FnOnce(&str) -> Result<(), String>,
) -> Result<Signed, &'static str> {
    let port = listener.local_addr().map_err(|_| "signInFailed")?.port();
    let redirect = format!("http://127.0.0.1:{port}{}", native::CALLBACK_PATH);
    let pkce = Pkce::new().ok_or("signInFailed")?;
    open(&native::authorize_url(sign, &redirect, &pkce, scope, nonce))
        .map_err(|_| "signInFailed")?;
    let code = tokio::time::timeout(wait, redirected(&listener, &pkce.state))
        .await
        .map_err(|_| "signInTimedOut")??;
    drop(listener);
    let form = native::token_form(sign, &redirect, &pkce, &code);
    let response = http
        .post(format!("{}oauth/token", sign.issuer))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .map_err(|_| "signInFailed")?;
    let given = answer(response).await.ok_or("signInFailed")?;
    let (access, _) = native::token(&given).ok_or("signInFailed")?;
    let id_token = native::id_token(&given);
    if nonce.is_some() && id_token.is_none() {
        return Err("signInFailed");
    }
    Ok(Signed { access, id_token })
}

/// The issuer's answer when it is a success with JSON in it, read up to [`ANSWER_LIMIT`].
async fn answer(mut response: reqwest::Response) -> Option<Value> {
    if !response.status().is_success() {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > ANSWER_LIMIT {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).ok()
}

/// The browser's redirect to this computer's loopback port: the code it brought for this sign-in's
/// `state`, or the issuer's no, told by what it said (`native::callback`) and written to the
/// diagnostic log as the issuer's error and its description, never a code or a token. A request for
/// anything else (a favicon, a redirect this sign-in did not start) is answered and the wait goes on.
pub(crate) async fn redirected(
    listener: &TcpListener,
    state: &str,
) -> Result<String, &'static str> {
    loop {
        let Ok((mut stream, peer)) = listener.accept().await else {
            continue;
        };
        if !peer.ip().is_loopback() {
            continue;
        }
        let target = read_target(&mut stream).await;
        let result = match target.as_deref() {
            Some(target) => native::callback(target, state),
            None => Err("signInFailed"),
        };
        let said = target
            .as_deref()
            .and_then(|target| native::issuer_said(target, state));
        if let Some((error, description)) = &said {
            tempo_core::applog::warn(
                "remote",
                &format!(
                    "sign-in: the issuer said {} ({})",
                    loggable(error),
                    loggable(description)
                ),
            );
        }
        let page = match (result.as_ref().err().copied(), &said) {
            (None, _) => "Nexus is signed in. You can close this tab and go back to Nexus.",
            (Some("signInRefused"), _) => "The sign-in was cancelled. You can close this tab.",
            (Some("emailUnverified"), _) => {
                "Confirm your email address with the link Nexus Remote sent you, then sign in again \
                 from Nexus. You can close this tab."
            }
            (Some(_), Some(_)) => {
                "The sign-in did not finish. Go back to Nexus and try again. You can close this tab."
            }
            (Some(_), None) => "This is not the sign-in Nexus is waiting for.",
        };
        let body =
            format!("<!doctype html><meta charset=\"utf-8\"><title>Nexus</title><p>{page}</p>");
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\n\
             cache-control: no-store\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(reply.as_bytes()).await;
        let _ = stream.shutdown().await;
        match result {
            Ok(code) => return Ok(code),
            Err(reason) if said.is_some() => return Err(reason),
            Err(_) => continue,
        }
    }
}

/// What the issuer said, as the diagnostic log takes it: one line, printable, and short.
fn loggable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(200).collect()
}

/// A GET's request target from the first line of a request read within a second, or `None`.
async fn read_target(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut chunk = [0u8; 1024];
    let read = async {
        while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 8192 {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            head.extend_from_slice(&chunk[..n]);
        }
        Some(())
    };
    tokio::time::timeout(Duration::from_secs(1), read)
        .await
        .ok()??;
    let line = std::str::from_utf8(&head).ok()?.lines().next()?.to_string();
    let mut parts = line.split(' ');
    match (parts.next(), parts.next()) {
        (Some("GET"), Some(target)) => Some(target.to_string()),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A GET of `target` on `at`, as the browser makes it: the page it was answered with.
    pub(crate) async fn ask(at: std::net::SocketAddr, target: &str) -> String {
        let mut stream = TcpStream::connect(at).await.unwrap();
        stream
            .write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut page = String::new();
        let _ = stream.read_to_string(&mut page).await;
        page
    }

    /// ★ The sign-in's loopback listener takes only the redirect it is waiting for: a favicon and a
    /// redirect with another state are answered and the wait goes on; the code with this state is
    /// taken. CONTROL: the issuer's refusal with this state ends the wait (`signInRefused`).
    #[tokio::test]
    async fn the_loopback_listener_takes_only_this_sign_ins_redirect() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = listener.local_addr().unwrap();
        let waiting = tokio::spawn(async move {
            let code = redirected(&listener, "state-1").await;
            (code, listener)
        });
        assert!(ask(at, "/favicon.ico").await.contains("not the sign-in"));
        assert!(ask(at, "/callback?code=stolen&state=other")
            .await
            .contains("not the sign-in"));
        assert!(ask(at, "/callback?code=the-code&state=state-1")
            .await
            .contains("signed in"));
        let (code, listener) = waiting.await.unwrap();
        assert_eq!(code, Ok("the-code".to_string()));

        let refused = tokio::spawn(async move { redirected(&listener, "state-2").await });
        let _ = ask(at, "/callback?error=access_denied&state=state-2").await;
        assert_eq!(refused.await.unwrap(), Err("signInRefused"));
    }

    /// ★ Every no the issuer says under this sign-in's state ends the wait, told by what it said:
    /// an address not confirmed yet is never a cancel, and no other error leaves the operator
    /// waiting out the five minutes. CONTROL: the same no under another state is answered and the
    /// wait goes on.
    #[tokio::test]
    async fn the_issuers_no_ends_the_wait_as_what_it_said() {
        let unverified = "/callback?error=access_denied&error_description=Please%20verify%20your%20email%20address%2C%20then%20sign%20in%20again.";
        for (target, page, reason) in [
            (unverified, "Confirm your email", "emailUnverified"),
            (
                "/callback?error=unauthorized_client&error_description=Grant%20type",
                "did not finish",
                "signInFailed",
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let at = listener.local_addr().unwrap();
            let waiting = tokio::spawn(async move { redirected(&listener, "s").await });
            assert!(ask(at, &format!("{target}&state=other"))
                .await
                .contains("not the sign-in"));
            assert!(ask(at, &format!("{target}&state=s")).await.contains(page));
            let ended = tokio::time::timeout(Duration::from_secs(5), waiting)
                .await
                .expect("the wait ended")
                .unwrap();
            assert_eq!(ended, Err(reason));
        }
    }
}
