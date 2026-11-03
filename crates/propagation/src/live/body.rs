//! A reply's body, read only as far as a limit: a server that sends far more than the answer it
//! was asked for is refused, never read to the end. Every fetcher in this crate reads its answer
//! here, and so do the app's own blocking downloads (`src-tauri`); a test in this module scans
//! `live/` for a response read with no limit.

use std::io::Read;

/// The most a callbook answer is read to (callook.info, QRZ, HamQTH). One is a few kilobytes.
pub(crate) const CALLBOOK_REPLY_BYTES: u64 = 256 * 1024;

/// `body` read as text, or `Err` with a sentence prefixed `who` when it is longer than `limit`
/// bytes or cannot be read. Read as UTF-8 with any invalid byte replaced, as `Response::text`
/// reads it without the `charset` feature.
pub fn read_capped(body: impl Read, limit: u64, who: &str) -> Result<String, String> {
    read_capped_bytes(body, limit, who).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// [`read_capped`] for a binary answer: the bytes as they came.
pub fn read_capped_bytes(body: impl Read, limit: u64, who: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    body.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| format!("{who}: could not read the response body"))?;
    if bytes.len() as u64 > limit {
        return Err(too_large(limit, who));
    }
    Ok(bytes)
}

/// The sentence for an answer longer than `limit` bytes, prefixed `who` — for a fetcher that
/// reads its body in its own loop (LoTW's, which also bounds time).
pub fn too_large(limit: u64, who: &str) -> String {
    let size = if limit >= 1024 * 1024 {
        format!("{} MB", limit / (1024 * 1024))
    } else {
        format!("{} KB", limit / 1024)
    };
    format!("{who}: the server's answer was larger than {size}, so it was not read")
}

/// A successful reply's body through [`read_capped`]. Any other status — a redirect the client
/// did not follow among them — is `Err` with a sentence prefixed `who`, and its body is not read.
pub fn read_ok(resp: reqwest::blocking::Response, limit: u64, who: &str) -> Result<String, String> {
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

/// A fetcher's body read keeps its limit: `read`, given a real loopback response, reads a body
/// of exactly `limit` bytes whole and refuses one byte more with a sentence prefixed `who`.
/// For the fetchers whose client and URL are fixed inside the module, so their read is the part
/// a test can reach.
#[cfg(test)]
pub(crate) fn assert_body_capped(
    read: impl Fn(reqwest::blocking::Response) -> Result<String, String>,
    limit: u64,
    who: &str,
) {
    let get = |body: &[u8]| {
        let port = serve(vec![ok_reply(body)]);
        reqwest::blocking::get(format!("http://127.0.0.1:{port}/product")).expect("loopback reply")
    };
    let edge = vec![b' '; limit as usize];
    assert_eq!(
        read(get(&edge)).map(|s| s.len()),
        Ok(edge.len()),
        "a body at the limit is read whole"
    );
    let huge = vec![b' '; limit as usize + 1];
    let err = read(get(&huge)).unwrap_err();
    assert!(
        err.starts_with(&format!("{who}: ")) && err.contains("larger than"),
        "a body past the limit must be refused: {err}"
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

    /// Ways a reqwest response is read to its end with no limit. Matched with whitespace
    /// removed, so a chain split over lines is still caught.
    const UNCAPPED_READS: [&str; 7] = [
        ".text()",
        ".json()",
        ".json::<",
        ".bytes()?",
        ".bytes().map_err",
        ".bytes().await",
        ".copy_to(",
    ];

    /// Reads through `std::io::Read`, which a blocking reqwest response implements. Uncapped unless
    /// the statement bounds them with `.take(`; one whose call is `.await`ed is tokio's AsyncRead
    /// (a socket in a test), which a reqwest response never is.
    const IO_READS: [&str; 3] = [".read_to_end(", ".read_to_string(", "io::copy("];

    /// Whether `code` (whitespace removed) has an `IO_READS` call `p` with no `.take(` in its
    /// statement and no `.await` on its result.
    fn unbounded_io_read(code: &str, p: &str) -> bool {
        code.match_indices(p).any(|(at, _)| {
            let start = code[..at].rfind([';', '{', '}']).map_or(0, |i| i + 1);
            let end = code[at..].find(';').map_or(code.len(), |i| at + i);
            // The call's closing paren, then whether `.await` follows it.
            let mut depth = 0usize;
            let mut close = code.len();
            for (i, c) in code[at + p.len() - 1..].char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            close = at + p.len() - 1 + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            !code[start..end].contains(".take(") && !code[close..].starts_with(".await")
        })
    }

    /// `source` with `//` comments dropped (whole-line, or trailing after ` // `, so a URL's
    /// `https://` stays) and all whitespace removed.
    fn code_of(source: &str) -> String {
        source
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .map(|l| l.split(" // ").next().unwrap_or_default())
            .collect::<String>()
            .split_whitespace()
            .collect()
    }

    /// The uncapped reads in `source`, by pattern.
    fn uncapped_reads(source: &str) -> Vec<&'static str> {
        let code = code_of(source);
        UNCAPPED_READS
            .into_iter()
            .filter(|p| code.contains(p))
            .chain(IO_READS.into_iter().filter(|p| unbounded_io_read(&code, p)))
            .collect()
    }

    /// Every network fetcher in this crate reads its answer through this module, so none can
    /// be made to read without end: no module reads a response whole, and every module that
    /// sends a request outside its tests reads the answer here (`read_capped`, `read_ok`,
    /// `read_capped_bytes`, or `too_large` for LoTW's own time-bounded loop).
    #[test]
    fn every_fetcher_reads_through_a_limit() {
        // CONTROLS: the scan sees an uncapped read, even split over lines, and passes a capped one.
        assert_eq!(
            uncapped_reads("let t = resp\n    .text()\n    .map_err(f)?;"),
            [".text()"]
        );
        assert_eq!(
            uncapped_reads("let v = r.json::<Value>()?; // .text()"),
            [".json::<"]
        );
        assert!(uncapped_reads("super::body::read_capped(resp, L, \"X\")").is_empty());
        // CONTROLS for the `std::io::Read` route: an unbounded read, `io::copy` included, is seen;
        // one bounded by `.take(` and a tokio socket's `.await`ed read are not.
        assert_eq!(
            uncapped_reads("let mut v = Vec::new();\n resp.read_to_end(&mut v)?;"),
            [".read_to_end("]
        );
        assert_eq!(
            uncapped_reads("std::io::copy(&mut resp, &mut file)?;"),
            ["io::copy("]
        );
        assert!(uncapped_reads(
            "resp.take(LIMIT + 1)\n    .read_to_end(&mut body)\n    .map_err(f)?;"
        )
        .is_empty());
        assert!(uncapped_reads("let _ = stream.read_to_string(&mut page).await;").is_empty());

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/live");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("src/live") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name == "body.rs" {
                continue; // the limit itself, and this scan's own patterns
            }
            let source = std::fs::read_to_string(&path).expect("source");
            let found = uncapped_reads(&source);
            assert!(
                found.is_empty(),
                "{name} reads a response with no limit: {found:?}"
            );
            let shipped = code_of(source.split("#[cfg(test)]\nmod tests").next().unwrap());
            if shipped.contains(".send()") || shipped.contains(".execute(") {
                assert!(
                    shipped.contains("body::read_") || shipped.contains("body::too_large("),
                    "{name} sends a request but does not read the answer through live::body"
                );
                checked += 1;
            }
        }
        assert!(
            checked >= 25,
            "only {checked} fetchers were found; is the scan looking?"
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
