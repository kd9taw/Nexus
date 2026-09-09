//! The transport-error message every live connector shows the operator.
//!
//! Nine connectors carried nine copies of the same `redact`, and every copy folded six different
//! failures into one sentence. `reqwest::Error::is_connect()` is true for a DNS failure, a refused
//! or unreachable TCP connect, a connect timeout, an unreachable proxy **and a rejected TLS
//! handshake** — all of which said "could not connect — check your network". D#181's reporter had
//! a working network and spent the evening in it.
//!
//! The TLS arm is the one that matters, because Nexus resolves its trust roots from the bundled
//! webpki set rather than the OS store: an HTTPS-inspecting antivirus (Kaspersky, ESET, Avast,
//! Bitdefender) or a corporate proxy re-signs with a local CA that Nexus does not carry, so
//! **every** connector fails at the handshake at once — a signature the old message hid.
//!
//! **Classification is by TYPE, never by stringifying the error.** These request bodies carry
//! logbook upload codes and passwords, and a `reqwest::Error`'s `Display`/`source` can echo the
//! request URL — so nothing here ever formats `e`, only fixed category messages.
//!
//! The split is measured (2026-08-28, reqwest 0.12 + rustls/webpki-roots), not assumed — the
//! deepest `std::io::Error` in the source chain:
//!
//! | failure | io kind |
//! |---|---|
//! | TCP connect refused | `ConnectionRefused` |
//! | DNS lookup failed | uncategorised |
//! | TLS: untrusted root / self-signed / expired peer cert | `Other` |
//! | TLS: corrupt record | `Other` |
//!
//! Note what that rules out: a rejected handshake *does* carry an `io::Error`, so "no io error in
//! the chain" is not the discriminator it looks like — it would have called every antivirus
//! interception a network fault. Only kinds positively identified as TLS take the new message;
//! everything else keeps the old network wording, so an unrecognised failure can never gain a
//! wrong diagnosis.

/// Did the TLS layer reject the peer, rather than the network fail to reach it?
///
/// Walks the source chain looking for the deepest cause by TYPE. `Other`/`InvalidData` are how
/// rustls surfaces a handshake rejection through `io::Error`; a chain with no `io::Error` at all
/// means nothing reachability-shaped was reported either, which is the same reading.
fn tls_rejected(e: &reqwest::Error) -> bool {
    let mut src: Option<&(dyn std::error::Error + 'static)> = Some(e);
    let mut saw_io = false;
    while let Some(err) = src {
        if let Some(io) = err.downcast_ref::<std::io::Error>() {
            saw_io = true;
            if matches!(
                io.kind(),
                std::io::ErrorKind::Other | std::io::ErrorKind::InvalidData
            ) {
                return true;
            }
        }
        src = err.source();
    }
    !saw_io
}

/// The operator-facing message for a transport failure, prefixed with the connector's `label`.
///
/// A connector with its own wording for one category handles that arm itself and delegates the
/// rest here (LoTW and eQSL both say "can be slow" on a timeout).
pub(crate) fn redact(label: &str, e: &reqwest::Error) -> String {
    if e.is_timeout() {
        format!("{label}: request timed out — try again shortly")
    } else if e.is_redirect() {
        format!("{label}: blocked an unexpected redirect")
    } else if e.is_connect() {
        if tls_rejected(e) {
            format!(
                "{label}: the secure connection was rejected — antivirus or a proxy inspecting \
                 HTTPS traffic is the likely cause"
            )
        } else {
            format!("{label}: could not connect — check your network")
        }
    } else {
        format!("{label}: request failed")
    }
}

/// As [`redact`], for a connector whose client **follows** redirects rather than refusing them.
///
/// `is_redirect()` does not mean the same thing on both kinds of client. Where the policy is
/// `Policy::none()` it means one redirect arrived and was refused — "blocked an unexpected
/// redirect" is literally what happened. Where redirects are followed it means the *limit* was
/// reached: the server kept sending the client somewhere else and never settled. Telling that
/// operator something was blocked points them at the wrong thing, which is the whole defect this
/// module exists to fix.
pub(crate) fn redact_following_redirects(label: &str, e: &reqwest::Error) -> String {
    if e.is_redirect() {
        format!("{label}: too many redirects — the server never settled on a final address")
    } else {
        redact(label, e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    /// Both cases go through a real `reqwest` client against a real loopback socket, because the
    /// thing under test is what reqwest's error chain actually looks like — a hand-built error
    /// would only test our idea of it. No internet is involved.
    fn client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .https_only(true)
            .build()
            .expect("client")
    }

    fn err_for(port: u16) -> reqwest::Error {
        client()
            .get(format!("https://127.0.0.1:{port}/upload?code=SECRETCODE"))
            .send()
            .expect_err("loopback request must fail")
    }

    #[test]
    fn a_network_that_is_genuinely_unreachable_still_says_check_your_network() {
        // The negative control for the TLS split below: bind a port, drop it, connect to nothing.
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        drop(l);
        let e = err_for(port);
        assert!(e.is_connect(), "a refused connect is is_connect()");
        assert_eq!(
            redact("Test", &e),
            "Test: could not connect — check your network"
        );
    }

    /// A loopback server that answers every request with a redirect back to itself, so a client
    /// that follows redirects hits its limit. Plain HTTP: the error class is what is under test,
    /// and reaching it over TLS would need a certificate this suite has no way to make.
    fn redirect_loop_server() -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for stream in l.incoming().take(8) {
                let Ok(mut s) = stream else { continue };
                let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
                let mut buf = [0u8; 1024];
                while matches!(s.read(&mut buf), Ok(n) if n > 0) {
                    let sent = s.write_all(
                        b"HTTP/1.1 302 Found\r\nLocation: /\r\nContent-Length: 0\r\n\r\n",
                    );
                    if sent.is_err() {
                        break;
                    }
                    let _ = s.flush();
                }
            }
        });
        port
    }

    #[test]
    fn a_client_that_follows_redirects_is_told_the_chain_never_settled() {
        // hearham and geocode follow redirects, so `is_redirect()` there means the LIMIT was hit,
        // not that anything was blocked. Both wordings are checked against the same error, which
        // is the point: one of them sends the operator looking for something that never happened.
        let port = redirect_loop_server();
        let e = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::limited(2))
            .build()
            .expect("client")
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .expect_err("a redirect loop must fail");
        assert!(e.is_redirect(), "too many redirects is is_redirect()");
        assert_eq!(
            redact_following_redirects("Test", &e),
            "Test: too many redirects — the server never settled on a final address"
        );
        assert_eq!(redact("Test", &e), "Test: blocked an unexpected redirect");
    }

    #[test]
    fn the_following_variant_differs_on_redirects_and_nowhere_else() {
        // The control for the split above: everything that is not a redirect must reach the same
        // message either way, so the special case cannot quietly change a second category.
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        drop(l);
        let e = err_for(port);
        assert!(!e.is_redirect());
        assert_eq!(redact_following_redirects("Test", &e), redact("Test", &e));
    }

    #[test]
    fn a_rejected_tls_handshake_names_antivirus_or_a_proxy_instead_of_the_network() {
        // The reported case, in the only form that is reproducible offline: something answers the
        // TCP connect but is not a TLS peer we accept. On the operator's machine that something is
        // the antivirus's own certificate; the error class reqwest reports is the same.
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
                let _ = s.flush();
                std::thread::sleep(Duration::from_millis(300));
            }
        });
        let e = err_for(port);
        assert!(e.is_connect(), "a rejected handshake is also is_connect()");
        let msg = redact("Test", &e);
        assert!(
            msg.contains("secure connection") && msg.contains("antivirus"),
            "a handshake rejection must not be blamed on the network: {msg}"
        );
        // The classification is by type, so the message can never carry what the error text does.
        assert!(!msg.contains("SECRETCODE"), "upload code leaked: {msg}");
        assert!(!msg.contains("127.0.0.1"), "host leaked: {msg}");
        assert!(!msg.contains(&port.to_string()), "port leaked: {msg}");
    }
}
