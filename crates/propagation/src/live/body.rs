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
        return Err(format!(
            "{who}: the server's answer was larger than {} KB, so it was not read",
            limit / 1024
        ));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
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
