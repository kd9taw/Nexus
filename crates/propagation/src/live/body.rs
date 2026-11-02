//! A reply's body, read only as far as a limit: a server that sends far more than the answer it
//! was asked for is refused, never read to the end.

use std::io::Read;

/// The most a callbook answer is read to (callook.info, QRZ, HamQTH). One is a few kilobytes.
pub(crate) const CALLBOOK_REPLY_BYTES: u64 = 256 * 1024;

/// `body` read as text, or `Err` with a sentence prefixed `who` when it is longer than `limit`
/// bytes or cannot be read. Read as UTF-8 with any invalid byte replaced, as `Response::text`
/// reads it without the `charset` feature.
pub(crate) fn read_capped(body: impl Read, limit: u64, who: &str) -> Result<String, String> {
    let mut bytes = Vec::new();
    body.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| format!("{who}: could not read the response body"))?;
    if bytes.len() as u64 > limit {
        let size = if limit >= 1024 * 1024 {
            format!("{} MB", limit / (1024 * 1024))
        } else {
            format!("{} KB", limit / 1024)
        };
        return Err(format!(
            "{who}: the server's answer was larger than {size}, so it was not read"
        ));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// A successful reply's body through [`read_capped`]. Any other status — a redirect the client
/// did not follow among them — is `Err` with a sentence prefixed `who`, and its body is not read.
pub(crate) fn read_ok(
    resp: reqwest::blocking::Response,
    limit: u64,
    who: &str,
) -> Result<String, String> {
    if !resp.status().is_success() {
        return Err(format!("{who}: answered HTTP {}", resp.status().as_u16()));
    }
    read_capped(resp, limit, who)
}

/// A loopback HTTP server that answers each connection, in turn, with the next of `replies`
/// (whole responses, head and body) and closes it. Its port.
#[cfg(test)]
pub(crate) fn serve(replies: Vec<Vec<u8>>) -> u16 {
    use std::io::Write;
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = l.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for reply in replies {
            let Ok((mut s, _)) = l.accept() else { return };
            let mut head = Vec::new();
            let mut b = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && matches!(s.read(&mut b), Ok(1)) {
                head.push(b[0]);
            }
            let _ = s.write_all(&reply);
            let _ = s.flush();
        }
    });
    port
}

/// A `200 OK` response carrying `body`.
#[cfg(test)]
pub(crate) fn ok_reply(body: &[u8]) -> Vec<u8> {
    let mut r = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    r.extend_from_slice(body);
    r
}

/// The limits a capped fetcher keeps, checked on a loopback server through the fetcher's OWN
/// client functions, so loosening one in the module turns its test red:
/// - `fetch` (a GET with a given client) reads a body of exactly `limit` bytes whole and refuses
///   one byte more with a sentence prefixed `who`;
/// - a redirect is an error, not followed (the server's second answer is what following it
///   would have fetched);
/// - `client`, the one the fetcher ships with, refuses plain HTTP outright, while a client from
///   `builder` (the same timeout and redirect policy, without the HTTPS rule) reads that server.
#[cfg(test)]
pub(crate) fn assert_wire_limits(
    builder: fn() -> reqwest::blocking::ClientBuilder,
    client: fn() -> Result<reqwest::blocking::Client, String>,
    fetch: impl Fn(&reqwest::blocking::Client, &str) -> Result<String, String>,
    limit: u64,
    who: &str,
) {
    let plain = builder().build().expect("client");
    let url = |port: u16| format!("http://127.0.0.1:{port}/product");

    let edge = vec![b' '; limit as usize];
    let port = serve(vec![ok_reply(&edge)]);
    assert_eq!(
        fetch(&plain, &url(port)).map(|s| s.len()),
        Ok(edge.len()),
        "a body at the limit is read whole"
    );
    let huge = vec![b' '; limit as usize + 1];
    let err = fetch(&plain, &url(serve(vec![ok_reply(&huge)]))).unwrap_err();
    assert!(
        err.starts_with(&format!("{who}: ")) && err.contains("larger than"),
        "a body past the limit must be refused: {err}"
    );

    let port = serve(vec![redirect_reply(), ok_reply(b"followed")]);
    let err = fetch(&plain, &url(port)).unwrap_err();
    assert!(
        err.contains("302"),
        "a redirect must not be followed: {err}"
    );

    let port = serve(vec![ok_reply(b"plain")]);
    assert!(
        fetch(&client().expect("client"), &url(port)).is_err(),
        "the shipped client spoke plain HTTP"
    );
    let port = serve(vec![ok_reply(b"plain")]);
    assert_eq!(
        fetch(&plain, &url(port)).as_deref(),
        Ok("plain"),
        "CONTROL: the server answers plain HTTP"
    );
}

/// A `302 Found` response pointing at `/elsewhere`: a client that follows it asks again.
#[cfg(test)]
pub(crate) fn redirect_reply() -> Vec<u8> {
    b"HTTP/1.1 302 Found\r\nLocation: /elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn a_body_over_the_limit_is_refused_with_a_sentence() {
        let err = read_capped(Cursor::new(vec![b'a'; 2049]), 2048, "Test").unwrap_err();
        assert_eq!(
            err,
            "Test: the server's answer was larger than 2 KB, so it was not read"
        );
        let err = read_capped(
            Cursor::new(vec![b'a'; 2 * 1024 * 1024 + 1]),
            2 * 1024 * 1024,
            "Test",
        )
        .unwrap_err();
        assert_eq!(
            err,
            "Test: the server's answer was larger than 2 MB, so it was not read"
        );
    }

    #[test]
    fn a_body_at_or_under_the_limit_is_read_whole() {
        assert_eq!(
            read_capped(Cursor::new(vec![b'a'; 2048]), 2048, "Test").map(|s| s.len()),
            Ok(2048)
        );
        assert_eq!(
            read_capped(Cursor::new("W1AW é".as_bytes()), 2048, "Test").as_deref(),
            Ok("W1AW é")
        );
    }
}
