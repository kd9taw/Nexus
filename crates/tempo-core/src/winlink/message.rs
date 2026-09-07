//! B2 message assemble/parse — the message body carried inside an FBB transfer.
//!
//! A B2 message is a header block (MID, date, sender, recipients, subject, body and attachment
//! lengths) followed by the body and each attachment laid end to end, with the header's declared
//! lengths — not any delimiter — deciding where each part stops. That makes the lengths
//! load-bearing: a body that disagrees with its declared length is a malformed message, not a
//! message to guess at.
//!
//! Bodies and attachment names are bytes. Winlink carries binary attachments and non-UTF-8 text
//! from the field, and this layer neither validates nor transcodes them.
//!
//! This module sees the message **after** decompression: `b2f` unwraps the FBB framing and runs
//! [`super::lzhuf`], and hands the plain bytes here. Nothing below knows about LZHUF, SOH/STX/EOT,
//! or a socket.
//!
//! # The format
//!
//! ```text
//! Mid: ABCDE1234567\r\n
//! Date: 2026/09/06 12:00\r\n
//! From: N0CALL\r\n
//! Subject: hello\r\n
//! Body: 11\r\n
//! File: 2 a.txt\r\n
//! \r\n
//! hello world\r\n
//! xy\r\n
//! ```
//!
//! An ASCII header block of `Name: value` lines terminated by a blank line, then the body of
//! exactly `Body:` bytes, then one payload per `File: <bytelen> <name>` header **in header
//! order**, each part followed by CRLF. Grammar from the ARSFI B2F document (winlink.org/B2F) and
//! cross-read against wl2k-go's `fbb/message.go` (LA5NTA, MIT), which is the open implementation
//! that interoperates with RMS Express.
//!
//! Three of those header names are **structural** — `Mid:` names the message, `Body:` and `File:`
//! declare where the parts stop — so they are lifted out into [`Message::mid`],
//! [`Message::body`] and [`Message::attachments`] rather than left in
//! [`Message::headers`]. Everything else (`Date:`, `From:`, `To:`, `Cc:`, `Subject:`, `Mbo:`, and
//! anything a gateway invents) is carried through verbatim, in wire order, name and value
//! untouched. This layer has no opinion about what a `Date:` means.
//!
//! # The body is always plain text, and that is a product decision, not an accident
//!
//! Nexus forms send the readable body **generated independently from the template's `Msg:`
//! block**, with the structured field data in a separate small XML attachment (spec §3). A
//! station with no copy of the template still reads the message, because the body is the message
//! and the XML is an enhancement layer over it. That is what makes shipping a curated template
//! subset safe, and it is why nothing here treats the body as a container to be decoded: to this
//! module the body is bytes, and to the operator on the far end it is text they can read.
//!
//! # Why every part is followed by CRLF, and why that CRLF is checked
//!
//! The separator after the body and after each attachment is not decoration — **it is the only
//! thing that turns a wrong declared length into an error instead of a silent truncation.** With
//! the lengths trusted blindly, a `File: 4 a` in front of a five-byte payload yields a
//! four-byte attachment and a parser that never noticed; the fifth byte is simply gone, and the
//! operator is handed a corrupt file that looks fine. Requiring the CRLF where the length says
//! the part ends makes that case fail loudly as [`MessageError::BadFileLength`].
//!
//! Two leniencies were considered and **rejected for that reason**:
//!
//! * *Accepting a bare LF as the separator.* It would let an off-by-one through undetected on
//!   every part that really ends in CRLF: reading one byte short leaves the `\n` sitting exactly
//!   where the parser looks for a separator.
//! * *Treating a missing separator as end-of-part.* That is the silent truncation itself.
//!
//! The residual limit is honest and unavoidable: a declared length that is short by exactly the
//! amount that leaves a CRLF at the cut is undetectable from inside one message. The FBB
//! proposal's `<u-size>` is the outer check for that (see [`super::fbb`]), and it lives in the
//! session, not here.
//!
//! Trailing bytes after the last part are refused for the same reason. A B2 message ends exactly
//! where its lengths say it ends; leftovers mean a length is wrong or the framing is, and a
//! parser that ignored them would accept a message it had already mis-sliced. ⚠️ If a real
//! transcript ever shows a gateway padding the tail, this is the one line to relax — and it
//! should be relaxed with the fixture in hand, not pre-emptively.

/// The `Mid:` header — the message identifier, lifted into [`Message::mid`].
const HDR_MID: &[u8] = b"Mid";
/// The `Body:` header — the body's byte length, lifted into [`Message::body`].
const HDR_BODY: &[u8] = b"Body";
/// The `File:` header — `<bytelen> <name>`, lifted into [`Message::attachments`].
const HDR_FILE: &[u8] = b"File";
/// The line terminator, everywhere in the format. See the module header for why it is exact.
const CRLF: &[u8] = b"\r\n";

/// One attachment: the name from its `File:` header and the payload bytes that follow the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// The filename, verbatim bytes from the `File:` header — everything after the first space.
    ///
    /// ⚠️ [`assemble_b2`] writes it into a header line unescaped, because B2 has no escape and
    /// inventing one would produce a message no other implementation could read. A name
    /// containing CR or LF therefore produces an unparseable message, and an empty name is not
    /// representable at all (nothing would separate it from the space in front of it); spaces
    /// *are* fine — the length/name split is on the **first** space only, so
    /// `ICS 213 form.xml` round-trips. Filename policy belongs to the composer, not here.
    pub name: Vec<u8>,
    /// The payload, bytes. Winlink attachments are routinely binary — images, ICS-213 XML in
    /// whatever encoding the sender used, ZIPs — and nothing here inspects them.
    pub data: Vec<u8>,
}

/// A parsed or composed B2 message.
///
/// The three structural headers are fields; every other header is carried in
/// [`headers`](Message::headers) in wire order. See the module header for the split.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Message {
    /// The `Mid:` value — the message identifier the FBB proposal names it by.
    pub mid: Vec<u8>,
    /// Every non-structural header, `(name, value)`, in the order they appeared on the wire.
    /// Names are kept in the case they were sent in; values have one leading space stripped.
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
    /// The readable body. Always plain text on the wire (module header, spec §3); bytes here.
    pub body: Vec<u8>,
    /// The attachments, in `File:` header order — which is the order their payloads appear in.
    pub attachments: Vec<Attachment>,
}

/// Why a B2 message could not be parsed.
///
/// Two variants and not one, because they say different things to the session: a
/// [`BadFileLength`](MessageError::BadFileLength) message arrived intact enough to read its
/// headers and is wrong about an attachment, which is the failure this module exists to make
/// impossible to miss (module header). Everything else is [`Malformed`](MessageError::Malformed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageError {
    /// The message is not a B2 message: no blank line ending the header block, a header line with
    /// no `:`, a missing or duplicated `Mid:` or `Body:`, a length that is not ASCII decimal, a
    /// `File:` value with no name, a body that runs past the end of the buffer or does not end
    /// where `Body:` says it does, or bytes left over after the last attachment.
    Malformed,
    /// A `File:` header's declared byte length disagrees with the payload that followed it —
    /// either it runs past the end of the buffer, or the part does not end in CRLF where the
    /// length says it does. **Never a truncation:** see the module header.
    BadFileLength,
}

/// Serialises a [`Message`] into B2 wire bytes. The inverse of [`parse_b2`].
///
/// Header order is canonical rather than preserved: `Mid:`, then
/// [`headers`](Message::headers) in their own order, then `Body:`, then one `File:` per
/// attachment. That is the order wl2k-go emits and RMS Express expects; [`parse_b2`] itself does
/// not depend on header order, so a round-trip through this function is value-preserving but not
/// necessarily byte-identical to the message that came in.
///
/// Infallible by signature, and that is a deliberate narrowing: the lengths it writes are
/// *computed* from the data, so the one class of corruption this format has cannot originate
/// here — the length can never disagree with the payload it counts.
///
/// What that signature does **not** cover, and what the caller therefore owns: no field written
/// into a header line — [`Message::mid`], a [`headers`](Message::headers) name or value, an
/// [`Attachment::name`] — may contain CR or LF, and an attachment name may not be empty. B2 has
/// no escape, so there is nothing this function could do about one except invent an encoding no
/// other implementation reads. [`parse_b2`] refuses the results, which is how such a message is
/// caught: by failing its own round-trip, loudly.
pub fn assemble_b2(msg: &Message) -> Vec<u8> {
    let mut out = Vec::new();
    push_header(&mut out, HDR_MID, &msg.mid);
    for (name, value) in &msg.headers {
        push_header(&mut out, name, value);
    }
    push_header(&mut out, HDR_BODY, msg.body.len().to_string().as_bytes());
    for att in &msg.attachments {
        let mut value = att.data.len().to_string().into_bytes();
        value.push(b' ');
        value.extend_from_slice(&att.name);
        push_header(&mut out, HDR_FILE, &value);
    }
    out.extend_from_slice(CRLF);
    out.extend_from_slice(&msg.body);
    out.extend_from_slice(CRLF);
    for att in &msg.attachments {
        out.extend_from_slice(&att.data);
        out.extend_from_slice(CRLF);
    }
    out
}

/// One `Name: value\r\n` line.
fn push_header(out: &mut Vec<u8>, name: &[u8], value: &[u8]) {
    out.extend_from_slice(name);
    out.extend_from_slice(b": ");
    out.extend_from_slice(value);
    out.extend_from_slice(CRLF);
}

/// Parses B2 wire bytes into a [`Message`]. The inverse of [`assemble_b2`].
///
/// Every refusal is listed on [`MessageError`]. The one that matters is the last: a `File:`
/// length that disagrees with its payload is [`MessageError::BadFileLength`], never a shortened
/// attachment.
pub fn parse_b2(bytes: &[u8]) -> Result<Message, MessageError> {
    let mut rest = bytes;
    let mut mid: Option<Vec<u8>> = None;
    let mut body_len: Option<usize> = None;
    // The `File:` headers in wire order, which is also their payloads' order.
    let mut files: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut headers: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();

    // The header block: `Name: value` lines up to the first blank one. Running out of lines
    // before that blank one is malformed rather than "the headers were all of it" — without the
    // terminator there is no way to know the body has not been cut off mid-header.
    loop {
        let (line, tail) = split_line(rest).ok_or(MessageError::Malformed)?;
        rest = tail;
        if line.is_empty() {
            break;
        }
        let (name, value) = split_header(line)?;
        // Structural names are matched case-insensitively: a gateway that writes `MID:` is
        // naming the field, and treating it as an unknown header would drop the MID and then
        // fail the message for not having one.
        if name.eq_ignore_ascii_case(HDR_MID) {
            // `replace` returning `Some` means a second one — ambiguous, and the message would
            // be filed under whichever this parser happened to keep.
            if mid.replace(value.to_vec()).is_some() {
                return Err(MessageError::Malformed);
            }
        } else if name.eq_ignore_ascii_case(HDR_BODY) {
            if body_len.replace(parse_len(value)?).is_some() {
                return Err(MessageError::Malformed);
            }
        } else if name.eq_ignore_ascii_case(HDR_FILE) {
            files.push(split_file(value)?);
        } else {
            headers.push((name.to_vec(), value.to_vec()));
        }
    }

    // Both are required, and neither has a defensible default. An absent `Mid:` would leave the
    // mailbox keying on an empty identifier; an absent `Body:` leaves nothing to say where the
    // body stops, because the format has no delimiter to fall back on — the attachments start
    // immediately after it.
    let mid = mid.ok_or(MessageError::Malformed)?;
    let body_len = body_len.ok_or(MessageError::Malformed)?;

    let (body, tail) = take_part(rest, body_len).ok_or(MessageError::Malformed)?;
    rest = tail;

    let mut attachments = Vec::with_capacity(files.len());
    for (len, name) in files {
        // The whole point of the module: a `File:` length that does not land on a CRLF is an
        // error, never a shorter attachment. See the module header.
        let (data, tail) = take_part(rest, len).ok_or(MessageError::BadFileLength)?;
        rest = tail;
        attachments.push(Attachment {
            name,
            data: data.to_vec(),
        });
    }

    if !rest.is_empty() {
        return Err(MessageError::Malformed);
    }

    Ok(Message {
        mid,
        headers,
        body: body.to_vec(),
        attachments,
    })
}

/// Splits one CRLF-terminated line off the front, returning it without its terminator.
///
/// `None` when no CRLF remains, which inside the header block means the block never ended.
/// Deliberately not tolerant of a bare LF — see the module header: that tolerance is what would
/// let an off-by-one length through [`take_part`] undetected.
fn split_line(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let at = bytes.windows(CRLF.len()).position(|w| w == CRLF)?;
    Some((&bytes[..at], &bytes[at + CRLF.len()..]))
}

/// `Name: value` → `(name, value)`, splitting on the **first** colon and stripping **one** leading
/// space from the value.
///
/// One space, not all leading whitespace: writing `Name: ` + value and stripping exactly what was
/// written round-trips a value byte for byte, including one that legitimately begins with a
/// space. An empty name is refused — that is a line that is not a header, not a header with no
/// name.
fn split_header(line: &[u8]) -> Result<(&[u8], &[u8]), MessageError> {
    let at = line
        .iter()
        .position(|&b| b == b':')
        .ok_or(MessageError::Malformed)?;
    let (name, rest) = line.split_at(at);
    if name.is_empty() {
        return Err(MessageError::Malformed);
    }
    let value = &rest[1..]; // past the colon
    Ok((name, value.strip_prefix(b" ").unwrap_or(value)))
}

/// ASCII decimal, and nothing else: no sign, no space, no `0x`, no empty field.
///
/// `usize::from_str` would take a leading `+` and reject the rest, so the digit check comes first
/// and the parse only has overflow left to fail on — an absurd declared length is refused here
/// rather than wrapping into a plausible one.
fn parse_len(value: &[u8]) -> Result<usize, MessageError> {
    if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
        return Err(MessageError::Malformed);
    }
    std::str::from_utf8(value)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or(MessageError::Malformed)
}

/// A `File:` value, `<bytelen> <name>` → `(len, name)`.
///
/// The split is on the **first** space only, so an attachment name may contain spaces —
/// `ICS 213 form.xml` is a name a field station really sends. An empty name is refused: it is not
/// representable on the wire (nothing would separate it from the space in front of it), so
/// accepting one here would produce a [`Message`] that [`assemble_b2`] could not serialise back.
fn split_file(value: &[u8]) -> Result<(usize, Vec<u8>), MessageError> {
    let at = value
        .iter()
        .position(|&b| b == b' ')
        .ok_or(MessageError::Malformed)?;
    let name = &value[at + 1..];
    if name.is_empty() {
        return Err(MessageError::Malformed);
    }
    Ok((parse_len(&value[..at])?, name.to_vec()))
}

/// Takes exactly `n` bytes **followed by CRLF**, returning the bytes and the remainder.
///
/// `None` when either half fails: the buffer is shorter than the declared length, or the declared
/// length does not land where a separator is. The second is the case that matters — it is the
/// only way a wrong-but-fitting length is visible from inside one message, and the caller turns
/// it into [`MessageError::BadFileLength`] rather than a shortened attachment.
fn take_part(rest: &[u8], n: usize) -> Option<(&[u8], &[u8])> {
    let end = n.checked_add(CRLF.len())?;
    if rest.len() < end || &rest[n..end] != CRLF {
        return None;
    }
    Some((&rest[..n], &rest[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A message with one header, a body and one attachment — the shape every test below varies.
    fn sample() -> Message {
        Message {
            mid: b"ABCDE1234567".to_vec(),
            headers: vec![(b"Subject".to_vec(), b"test".to_vec())],
            body: b"hello world".to_vec(),
            attachments: vec![Attachment {
                name: b"a.txt".to_vec(),
                data: b"xy".to_vec(),
            }],
        }
    }

    /// Rewrites the first `File:` header's declared byte length to `n`, leaving the payload
    /// untouched — the corruption the length check exists to catch.
    ///
    /// Asserts it found the header: a helper that silently did nothing would turn every test
    /// built on it into a test of the uncorrupted message, which is the positive control this
    /// whole file's negative results rest on.
    fn set_first_file_length(bytes: &[u8], n: usize) -> Vec<u8> {
        let at = bytes
            .windows(HDR_FILE.len() + 2)
            .position(|w| w == b"File: ")
            .expect("no `File: ` header to corrupt");
        let start = at + HDR_FILE.len() + 2;
        let end = start
            + bytes[start..]
                .iter()
                .position(|&b| b == b' ')
                .expect("`File:` value must be `<len> <name>`");
        let mut out = bytes[..start].to_vec();
        out.extend_from_slice(n.to_string().as_bytes());
        out.extend_from_slice(&bytes[end..]);
        out
    }

    /// The brief's helper: make the first `File:` length disagree with its payload.
    fn corrupt_first_file_length(bytes: &mut Vec<u8>) {
        *bytes = set_first_file_length(bytes, 9);
    }

    #[test]
    fn assemble_parse_roundtrip() {
        let msg = sample();
        let bytes = assemble_b2(&msg);
        let back = parse_b2(&bytes).unwrap();
        assert_eq!(back.body, msg.body);
        assert_eq!(back.attachments.len(), 1);
        assert_eq!(back.attachments[0].data, b"xy");
        assert_eq!(back, msg, "the whole message must survive the round trip");
    }

    #[test]
    fn a_wrong_file_length_is_rejected() {
        // "File: <bytelen> <name>" whose declared length disagrees with the payload must fail,
        // not silently truncate.
        let mut bytes = assemble_b2(&Message {
            mid: b"ABCDE1234567".to_vec(),
            headers: vec![],
            body: b"body".to_vec(),
            attachments: vec![Attachment {
                name: b"a".to_vec(),
                data: b"12345".to_vec(),
            }],
        });
        corrupt_first_file_length(&mut bytes); // helper: change the declared byte length
        assert!(matches!(parse_b2(&bytes), Err(MessageError::BadFileLength)));
    }

    /// The other direction, and the one a length-trusting parser gets wrong *silently*: declared
    /// short, so there are bytes to spare and nothing runs off the end. Without the CRLF check
    /// this parses as a four-byte attachment and loses the `5`.
    #[test]
    fn a_short_file_length_truncates_nothing_and_is_rejected() {
        let good = assemble_b2(&Message {
            mid: b"ABCDE1234567".to_vec(),
            headers: vec![],
            body: b"body".to_vec(),
            attachments: vec![Attachment {
                name: b"a".to_vec(),
                data: b"12345".to_vec(),
            }],
        });
        // Positive control: uncorrupted, the same bytes parse and keep all five payload bytes.
        assert_eq!(parse_b2(&good).unwrap().attachments[0].data, b"12345");

        let short = set_first_file_length(&good, 4);
        assert_eq!(parse_b2(&short), Err(MessageError::BadFileLength));
    }

    /// The body's length is checked the same way, but it is not an attachment fault, so it is
    /// `Malformed` — the module header's split.
    #[test]
    fn a_wrong_body_length_is_malformed() {
        let good = assemble_b2(&sample());
        let bad = replace_once(&good, b"Body: 11\r\n", b"Body: 10\r\n");
        assert_eq!(parse_b2(&bad), Err(MessageError::Malformed));
        let over = replace_once(&good, b"Body: 11\r\n", b"Body: 99\r\n");
        assert_eq!(parse_b2(&over), Err(MessageError::Malformed));
    }

    /// Replaces the first occurrence of `from` with `to`, asserting it was there — the same
    /// positive control as [`set_first_file_length`].
    fn replace_once(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
        let at = bytes
            .windows(from.len())
            .position(|w| w == from)
            .expect("pattern not present — the test is not testing what it says");
        let mut out = bytes[..at].to_vec();
        out.extend_from_slice(to);
        out.extend_from_slice(&bytes[at + from.len()..]);
        out
    }

    /// Pins the exact wire bytes. Written out by hand from the format in the module header, so a
    /// change to the serialiser that both halves of the round-trip agreed on still fails here.
    #[test]
    fn pinned_wire_bytes() {
        let wire = assemble_b2(&sample());
        assert_eq!(
            wire,
            b"Mid: ABCDE1234567\r\n\
              Subject: test\r\n\
              Body: 11\r\n\
              File: 2 a.txt\r\n\
              \r\n\
              hello world\r\n\
              xy\r\n"
                .to_vec()
        );
    }

    /// The structural headers are fields, not entries in `headers` — a parser that left them in
    /// both places would have `assemble_b2` emit each of them twice on the way back out.
    #[test]
    fn structural_headers_do_not_leak_into_headers() {
        let back = parse_b2(&assemble_b2(&sample())).unwrap();
        assert_eq!(back.headers, vec![(b"Subject".to_vec(), b"test".to_vec())]);
        assert_eq!(back.mid, b"ABCDE1234567");
    }

    /// Structural header names are matched case-insensitively. RMS Express writes `Mid:`, but a
    /// gateway that writes `MID:` is naming the same field, and treating it as an unknown header
    /// would drop the MID and then fail on a message that is perfectly well formed.
    #[test]
    fn structural_header_names_are_case_insensitive() {
        let wire = b"MID: X\r\nBODY: 2\r\nFILE: 1 f\r\n\r\nhi\r\nz\r\n";
        let m = parse_b2(wire).unwrap();
        assert_eq!(m.mid, b"X");
        assert_eq!(m.body, b"hi");
        assert_eq!(m.attachments, vec![Attachment { name: b"f".to_vec(), data: b"z".to_vec() }]);
        assert!(m.headers.is_empty());
    }

    /// Parsing does not depend on header order — only the payloads do, and they follow the
    /// `File:` headers' order.
    #[test]
    fn header_order_does_not_matter_and_file_order_does() {
        let wire = b"File: 1 second\r\nBody: 1\r\nFile: 2 first\r\nMid: M\r\n\r\nB\r\nX\r\nYZ\r\n";
        let m = parse_b2(wire).unwrap();
        assert_eq!(m.mid, b"M");
        assert_eq!(m.body, b"B");
        // "second" was proposed first, so it takes the first payload.
        assert_eq!(m.attachments[0].name, b"second");
        assert_eq!(m.attachments[0].data, b"X");
        assert_eq!(m.attachments[1].name, b"first");
        assert_eq!(m.attachments[1].data, b"YZ");
    }

    #[test]
    fn an_empty_body_and_no_attachments_round_trip() {
        let msg = Message {
            mid: b"M".to_vec(),
            headers: vec![],
            body: vec![],
            attachments: vec![],
        };
        let wire = assemble_b2(&msg);
        assert_eq!(wire, b"Mid: M\r\nBody: 0\r\n\r\n\r\n".to_vec());
        assert_eq!(parse_b2(&wire).unwrap(), msg);
    }

    /// Bytes, never `String`: a payload full of NULs, CRLFs and invalid UTF-8 must come back
    /// exactly. A parser that split on CRLF instead of counting would cut this attachment in
    /// three; one that went through `String` would not compile the fixture, let alone carry it.
    #[test]
    fn binary_payloads_survive_including_embedded_crlf() {
        let msg = Message {
            mid: b"M".to_vec(),
            headers: vec![],
            body: b"line\r\nline\x00\xff".to_vec(),
            attachments: vec![Attachment {
                name: b"bin\xc3(".to_vec(),
                data: vec![0x00, 0x0d, 0x0a, 0xff, 0xfe, 0x1a],
            }],
        };
        assert_eq!(parse_b2(&assemble_b2(&msg)).unwrap(), msg);
    }

    /// An attachment name with a space is fine — the `File:` value splits on the *first* space
    /// only, so everything after it is the name. Pinned because "split on space" is the obvious
    /// wrong implementation.
    #[test]
    fn an_attachment_name_may_contain_spaces() {
        let msg = Message {
            mid: b"M".to_vec(),
            headers: vec![],
            body: vec![],
            attachments: vec![Attachment {
                name: b"ICS 213 form.xml".to_vec(),
                data: b"d".to_vec(),
            }],
        };
        assert_eq!(parse_b2(&assemble_b2(&msg)).unwrap(), msg);
    }

    #[test]
    fn bytes_after_the_last_attachment_are_refused() {
        let mut wire = assemble_b2(&sample());
        wire.extend_from_slice(b"junk");
        assert_eq!(parse_b2(&wire), Err(MessageError::Malformed));
    }

    #[test]
    fn a_header_block_with_no_blank_line_is_malformed() {
        assert_eq!(
            parse_b2(b"Mid: M\r\nBody: 0\r\n"),
            Err(MessageError::Malformed)
        );
    }

    #[test]
    fn every_structural_defect_in_the_header_block_is_malformed() {
        for (why, wire) in [
            ("no colon", &b"Mid: M\r\nnot a header\r\nBody: 0\r\n\r\n\r\n"[..]),
            ("no Mid", &b"Body: 0\r\n\r\n\r\n"[..]),
            ("no Body", &b"Mid: M\r\n\r\n\r\n"[..]),
            ("two Mids", &b"Mid: A\r\nMid: B\r\nBody: 0\r\n\r\n\r\n"[..]),
            ("two Bodies", &b"Mid: A\r\nBody: 0\r\nBody: 0\r\n\r\n\r\n"[..]),
            ("non-decimal Body", &b"Mid: A\r\nBody: 1x\r\n\r\n\r\n"[..]),
            ("negative Body", &b"Mid: A\r\nBody: -1\r\n\r\n\r\n"[..]),
            ("empty Body", &b"Mid: A\r\nBody: \r\n\r\n\r\n"[..]),
            ("File with no name", &b"Mid: A\r\nBody: 0\r\nFile: 1\r\n\r\n\r\nx\r\n"[..]),
            ("File with no length", &b"Mid: A\r\nBody: 0\r\nFile:  x\r\n\r\n\r\nx\r\n"[..]),
            ("non-decimal File", &b"Mid: A\r\nBody: 0\r\nFile: 1x n\r\n\r\n\r\nx\r\n"[..]),
            ("empty header name", &b"Mid: A\r\n: v\r\nBody: 0\r\n\r\n\r\n"[..]),
            ("truncated after headers", &b"Mid: A\r\nBody: 0\r\n\r\n"[..]),
        ] {
            assert_eq!(parse_b2(wire), Err(MessageError::Malformed), "{why}");
        }
        // Positive control: the same shape, defect removed, parses.
        assert!(parse_b2(b"Mid: A\r\nBody: 0\r\nFile: 1 n\r\n\r\n\r\nx\r\n").is_ok());
    }

    /// A body ending in something other than CRLF where `Body:` says it ends. Separate from the
    /// wrong-length test because here the *count* is right and only the separator is wrong, which
    /// is what a stream of concatenated parts with no separator at all looks like.
    #[test]
    fn a_body_not_followed_by_crlf_is_malformed() {
        assert_eq!(
            parse_b2(b"Mid: A\r\nBody: 2\r\n\r\nhiXX"),
            Err(MessageError::Malformed)
        );
        // Positive control: same bytes, correct separator.
        assert!(parse_b2(b"Mid: A\r\nBody: 2\r\n\r\nhi\r\n").is_ok());
    }

    /// A bare LF where the format calls for CRLF is refused, not accepted as a separator — the
    /// leniency the module header rejects, pinned so it cannot be added back by accident.
    #[test]
    fn a_bare_lf_separator_is_not_accepted() {
        assert_eq!(
            parse_b2(b"Mid: A\nBody: 0\n\n\n"),
            Err(MessageError::Malformed)
        );
    }

    #[test]
    fn a_header_value_keeps_its_bytes_but_loses_one_leading_space() {
        let m = parse_b2(b"Mid: A\r\nX:v\r\nY:  v \r\nBody: 0\r\n\r\n\r\n").unwrap();
        assert_eq!(
            m.headers,
            vec![
                (b"X".to_vec(), b"v".to_vec()),
                (b"Y".to_vec(), b" v ".to_vec()),
            ]
        );
    }

    proptest! {
        /// Round-trip over arbitrary binary bodies and attachments. Names exclude CR and LF only
        /// — the documented precondition on [`Attachment::name`] — and nothing else is
        /// constrained, so embedded CRLFs, NULs and invalid UTF-8 are all in range.
        #[test]
        fn arbitrary_messages_round_trip(
            mid in proptest::collection::vec(
                any::<u8>().prop_filter("no CR/LF in a MID", |b| *b != b'\r' && *b != b'\n'),
                1..12,
            ),
            body in proptest::collection::vec(any::<u8>(), 0..300),
            atts in proptest::collection::vec(
                (
                    proptest::collection::vec(
                        any::<u8>().prop_filter("no CR/LF in a name", |b| *b != b'\r' && *b != b'\n'),
                        1..12,
                    ),
                    proptest::collection::vec(any::<u8>(), 0..300),
                ),
                0..4,
            ),
        ) {
            let msg = Message {
                mid,
                headers: vec![],
                body,
                attachments: atts
                    .into_iter()
                    .map(|(name, data)| Attachment { name, data })
                    .collect(),
            };
            prop_assert_eq!(parse_b2(&assemble_b2(&msg)), Ok(msg));
        }
    }
}
