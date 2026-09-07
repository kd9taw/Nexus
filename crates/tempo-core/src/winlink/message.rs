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
    /// containing CR or LF is therefore not representable, and neither is an empty one (nothing
    /// would separate it from the space in front of it); [`assemble_b2`] refuses both with
    /// [`AssembleError::AttachmentName`] rather than emit them. Spaces *are* fine — the
    /// length/name split is on the **first** space only, so `ICS 213 form.xml` round-trips.
    /// Filename policy belongs to the composer, not here.
    pub name: Vec<u8>,
    /// The payload, bytes. Winlink attachments are routinely binary — images, ICS-213 XML in
    /// whatever encoding the sender used, ZIPs — and nothing here inspects them.
    pub data: Vec<u8>,
}

/// A message's non-structural headers, stored as one block with spans into it.
///
/// **Not `Vec<(Vec<u8>, Vec<u8>)>`, and the reason is a measurement.** That shape costs a 48-byte
/// tuple slot in the vector plus two separate heap allocations per header, so `A: B\r\n` — six
/// plaintext bytes — cost about ten times its own size by requested capacity alone. A Winlink
/// message at the published 120,000-byte account maximum expands to roughly 5.7 MB of plaintext,
/// which is legitimate mail no CMS would refuse. One block plus a 16-byte span per header brings
/// the same message close to its own size, and — the part capacity cannot show — replaces two
/// allocations per header with two for the whole message.
///
/// The read API is unchanged in shape: [`Headers::iter`] yields `(&[u8], &[u8])`, which is what
/// [`assemble_b2`] and `mailbox::entry_from_blob` — the only two non-test consumers — already do.
///
/// ⚠️ `PartialEq` is derived over `(raw, spans)`, which is sequence equality **only because
/// [`Headers::push`] appends both halves in order and nothing else ever writes `raw`**. If a
/// later change writes into `raw` out of order, or reuses a span, the derive silently stops
/// meaning what the tests think it means.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers {
    /// Every header's name and value, concatenated with no separators. The spans say where each
    /// begins and ends, so no delimiter is needed and no byte is escaped.
    raw: Vec<u8>,
    /// One entry per header, in wire order.
    spans: Vec<Span>,
}

/// One header's extent inside [`Headers::raw`].
///
/// `u32` because a B2 header block that needed more than 4 GiB of offsets is not a message. Each
/// half is `(start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    /// The name's `(start, end)` in `raw`.
    name: (u32, u32),
    /// The value's `(start, end)` in `raw`.
    value: (u32, u32),
}

impl Headers {
    /// Appends one header, keeping wire order.
    ///
    /// Silently ignores a name or value that would push `raw` past `u32::MAX` — unreachable for
    /// any message that got this far (`b2f` bounds a transfer far below 4 GiB) and a truncating
    /// cast would be worse than a dropped header, because it would produce a span pointing at
    /// somebody else's bytes.
    pub fn push(&mut self, name: &[u8], value: &[u8]) {
        let start = self.raw.len();
        if u32::try_from(start + name.len() + value.len()).is_err() {
            return;
        }
        self.raw.extend_from_slice(name);
        let mid = self.raw.len();
        self.raw.extend_from_slice(value);
        let end = self.raw.len();
        self.spans.push(Span {
            name: (start as u32, mid as u32),
            value: (mid as u32, end as u32),
        });
    }

    /// How many headers there are.
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Every header as `(name, value)`, in wire order.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.spans.iter().map(|s| {
            (
                &self.raw[s.name.0 as usize..s.name.1 as usize],
                &self.raw[s.value.0 as usize..s.value.1 as usize],
            )
        })
    }

    /// The first header with this name, matched ASCII-case-insensitively.
    ///
    /// First match and case-insensitive because that is `mailbox::entry_from_blob`'s existing
    /// rule — a gateway that writes `SUBJECT:` is naming the field.
    pub fn get(&self, name: &[u8]) -> Option<&[u8]> {
        self.iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    }

    /// Bytes of heap these headers hold. Exhaustively destructured; see [`Message::heap_bytes`].
    pub fn heap_bytes(&self) -> usize {
        let Headers { raw, spans } = self;
        raw.capacity() + spans.capacity() * size_of::<Span>()
    }
}

impl FromIterator<(Vec<u8>, Vec<u8>)> for Headers {
    /// Lets an existing `vec![(name, value), …]` become a [`Headers`] with `.into_iter().collect()`.
    fn from_iter<T: IntoIterator<Item = (Vec<u8>, Vec<u8>)>>(iter: T) -> Self {
        let mut h = Headers::default();
        for (name, value) in iter {
            h.push(&name, &value);
        }
        h
    }
}

impl<'a> IntoIterator for &'a Headers {
    type Item = (&'a [u8], &'a [u8]);
    type IntoIter = Box<dyn Iterator<Item = (&'a [u8], &'a [u8])> + 'a>;

    /// So `for (name, value) in &msg.headers` keeps working unchanged.
    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
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
    ///
    /// A [`Headers`] rather than a `Vec` of pairs; see that type for the measurement that forced
    /// it. Build one from pairs with `.into_iter().collect()`.
    pub headers: Headers,
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

/// Why a [`Message`] could not be serialised — a field with no representation in B2.
///
/// B2 has no escape, so a byte that would end a header line early, or split it in a different
/// place, cannot be written at all. Each variant names the field, because the caller that has to
/// answer for it is a composer with a form on screen, not a parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssembleError {
    /// [`Message::mid`] contains CR or LF, which would end the `Mid:` line inside the MID.
    Mid,
    /// A [`Message::headers`] name is empty, or contains `:`, CR or LF. The colon is the silent
    /// one — see [`assemble_b2`] for the measured table of what each does.
    HeaderName,
    /// A [`Message::headers`] value contains CR or LF, which would end its line early and leave
    /// the remainder to be read as a header line of its own.
    HeaderValue,
    /// An [`Attachment::name`] is empty or contains CR or LF. See [`Attachment::name`].
    AttachmentName,
}

/// Serialises a [`Message`] into B2 wire bytes. The inverse of [`parse_b2`].
///
/// Header order is canonical rather than preserved: `Mid:`, then
/// [`headers`](Message::headers) in their own order, then `Body:`, then one `File:` per
/// attachment. That is the order wl2k-go emits and RMS Express expects; [`parse_b2`] itself does
/// not depend on header order, so a round-trip through this function is value-preserving but not
/// necessarily byte-identical to the message that came in.
///
/// The declared lengths cannot be wrong: they are *computed* from the data, so the one class of
/// corruption this format has cannot originate here — the length can never disagree with the
/// payload it counts.
///
/// The error is about **representability**, which is the other half. No field written into a
/// header line — [`Message::mid`], a [`headers`](Message::headers) name or value, an
/// [`Attachment::name`] — may contain CR or LF; a header name may not be empty or contain a
/// colon; an attachment name may not be empty. B2 has no escape, so there is nothing this
/// function could do about one except invent an encoding no other implementation reads, and it
/// refuses instead.
///
/// **Most of these were never caught loudly.** Two earlier wordings here said that CR, LF and an
/// empty attachment name all produce bytes [`parse_b2`] rejects, and that a colon in a header
/// name was the only silent case. Measured — each guard deleted in turn, then a round trip — that
/// is wrong in both directions. What every unrepresentable field actually does with its guard
/// gone:
///
/// | field | CRLF pair | lone CR or lone LF | empty |
/// |---|---|---|---|
/// | [`Message::mid`] | **silent** | round-trips unchanged | n/a |
/// | header name | `Malformed` | round-trips unchanged | `Malformed` |
/// | header value | **silent** | round-trips unchanged | no guard; an empty value is legal |
/// | [`Attachment::name`] | **silent** | round-trips unchanged | `Malformed` |
///
/// * **Silent** means `Ok` and a **different message than went in**, with nothing anywhere to
///   signal it. The pair ends the line early, the field comes back truncated to what preceded it,
///   and the remainder is read as a header line of its own: `mid = "M\r\nX: y"` round-trips to
///   `mid = "M"` plus a header `X: y`. A colon in a header name is the same class — `a:b: c`
///   parses back as the header `a` with the value `b: c`. Four fields, one failure mode.
/// * A **lone CR or lone LF** is not caught anywhere, and does not corrupt here either:
///   `split_line` cuts on the pair, so the byte rides through this parser and the field comes back
///   byte-identical. It is refused on interoperability grounds rather than round-trip grounds —
///   half a terminator is a byte a lenient implementation at the far end may well split on, and
///   this format has no escape with which to promise otherwise.
/// * Three cells are **loud**, and for unrelated reasons rather than as a class: a CRLF pair in a
///   header *name* truncates the line to something with no colon left in it, which is not a header
///   at all; an empty header name and an empty attachment name each fail a different one of
///   [`parse_b2`]'s own field checks. Nothing generalises from them to the silent four.
///
/// This is email over radio; a loud refusal beats a body the far end cannot know is wrong, so
/// every unrepresentable field is refused here and none is left to the round trip. Do not read
/// any of these guards as belt-and-braces over something [`parse_b2`] already rejects: most are
/// not, and all seven checks below are pinned — deleting any one of them turns exactly one test
/// in this module red.
///
/// One residual is deliberately not an error, because it is already loud: a
/// [`headers`](Message::headers) entry named `Mid`, `Body` or `File` is a malformed [`Message`]
/// rather than an unrepresentable field — those three names are structural and live in their own
/// fields — and [`parse_b2`] answers it with [`MessageError::Malformed`] or
/// [`MessageError::BadFileLength`], never with a different message. A phantom `File:` header
/// steals bytes from the front of the attachment region, and since the real lengths are computed
/// from the real payloads, the last part always comes up short.
pub fn assemble_b2(msg: &Message) -> Result<Vec<u8>, AssembleError> {
    if has_line_break(&msg.mid) {
        return Err(AssembleError::Mid);
    }
    for (name, value) in &msg.headers {
        // The colon check is the one whose absence is silent: this function would emit `a:b: c`,
        // which parses back as a header named `a`. The other two are refused for reasons the
        // round trip does not supply either — an empty or CRLF-bearing name is `Malformed` on the
        // way back, not a different message. See this function's doc for the measured table.
        if name.is_empty() || name.contains(&b':') || has_line_break(name) {
            return Err(AssembleError::HeaderName);
        }
        if has_line_break(value) {
            return Err(AssembleError::HeaderValue);
        }
    }
    for att in &msg.attachments {
        if att.name.is_empty() || has_line_break(&att.name) {
            return Err(AssembleError::AttachmentName);
        }
    }

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
    Ok(out)
}

/// True when the field carries a byte that would end its header line somewhere the format does
/// not expect one. Either half of a CRLF on its own is enough: [`split_line`] cuts on the pair,
/// so a lone CR in a value leaves a line this parser reads whole and another one would not.
fn has_line_break(field: &[u8]) -> bool {
    field.iter().any(|&b| b == b'\r' || b == b'\n')
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
    let mut headers = Headers::default();

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
            headers.push(name, value);
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

/// The heap this message costs, in bytes.
///
/// **Exhaustively destructured, `capacity()` never `len()`** — `b2f::Session::held_bytes`'s
/// discipline, for its reason: adding a field is `error[E0027]: pattern does not mention field`,
/// so a buffer cannot be added silently. ⚠️ What the compiler enforces is a *decision*, not a
/// correct answer: rustc's own suggested fix is `<field>: _`, which silences it while charging
/// nothing. That hole is a human one.
///
/// `capacity` rather than `len` because the question this answers is how much memory is held, not
/// how many bytes arrived. Charging wire bytes instead is the accounting error the B2F session
/// shipped with and had to have corrected.
impl Message {
    /// Bytes of heap this message holds. See the note above the impl.
    pub fn heap_bytes(&self) -> usize {
        let Message {
            mid,
            headers,
            body,
            attachments,
        } = self;
        let attachment_bytes: usize = attachments
            .iter()
            .map(|a| a.name.capacity() + a.data.capacity())
            .sum();
        mid.capacity()
            + headers.heap_bytes()
            + body.capacity()
            + attachments.capacity() * size_of::<Attachment>()
            + attachment_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A message with one header, a body and one attachment — the shape every test below varies.
    fn sample() -> Message {
        Message {
            mid: b"ABCDE1234567".to_vec(),
            headers: vec![(b"Subject".to_vec(), b"test".to_vec())]
                .into_iter()
                .collect(),
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
        let bytes = assemble_b2(&msg).unwrap();
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
            headers: Headers::default(),
            body: b"body".to_vec(),
            attachments: vec![Attachment {
                name: b"a".to_vec(),
                data: b"12345".to_vec(),
            }],
        })
        .unwrap();
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
            headers: Headers::default(),
            body: b"body".to_vec(),
            attachments: vec![Attachment {
                name: b"a".to_vec(),
                data: b"12345".to_vec(),
            }],
        })
        .unwrap();
        // Positive control: uncorrupted, the same bytes parse and keep all five payload bytes.
        assert_eq!(parse_b2(&good).unwrap().attachments[0].data, b"12345");

        let short = set_first_file_length(&good, 4);
        assert_eq!(parse_b2(&short), Err(MessageError::BadFileLength));
    }

    /// The body's length is checked the same way, but it is not an attachment fault, so it is
    /// `Malformed` — the module header's split.
    #[test]
    fn a_wrong_body_length_is_malformed() {
        let good = assemble_b2(&sample()).unwrap();
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
        let wire = assemble_b2(&sample()).unwrap();
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
        let back = parse_b2(&assemble_b2(&sample()).unwrap()).unwrap();
        assert_eq!(
            back.headers.iter().collect::<Vec<_>>(),
            vec![(&b"Subject"[..], &b"test"[..])]
        );
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
        assert_eq!(
            m.attachments,
            vec![Attachment {
                name: b"f".to_vec(),
                data: b"z".to_vec()
            }]
        );
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
            headers: Headers::default(),
            body: vec![],
            attachments: vec![],
        };
        let wire = assemble_b2(&msg).unwrap();
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
            headers: Headers::default(),
            body: b"line\r\nline\x00\xff".to_vec(),
            attachments: vec![Attachment {
                name: b"bin\xc3(".to_vec(),
                data: vec![0x00, 0x0d, 0x0a, 0xff, 0xfe, 0x1a],
            }],
        };
        assert_eq!(parse_b2(&assemble_b2(&msg).unwrap()).unwrap(), msg);
    }

    /// An attachment name with a space is fine — the `File:` value splits on the *first* space
    /// only, so everything after it is the name. Pinned because "split on space" is the obvious
    /// wrong implementation.
    #[test]
    fn an_attachment_name_may_contain_spaces() {
        let msg = Message {
            mid: b"M".to_vec(),
            headers: Headers::default(),
            body: vec![],
            attachments: vec![Attachment {
                name: b"ICS 213 form.xml".to_vec(),
                data: b"d".to_vec(),
            }],
        };
        assert_eq!(parse_b2(&assemble_b2(&msg).unwrap()).unwrap(), msg);
    }

    #[test]
    fn bytes_after_the_last_attachment_are_refused() {
        let mut wire = assemble_b2(&sample()).unwrap();
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
            (
                "no colon",
                &b"Mid: M\r\nnot a header\r\nBody: 0\r\n\r\n\r\n"[..],
            ),
            ("no Mid", &b"Body: 0\r\n\r\n\r\n"[..]),
            ("no Body", &b"Mid: M\r\n\r\n\r\n"[..]),
            ("two Mids", &b"Mid: A\r\nMid: B\r\nBody: 0\r\n\r\n\r\n"[..]),
            (
                "two Bodies",
                &b"Mid: A\r\nBody: 0\r\nBody: 0\r\n\r\n\r\n"[..],
            ),
            ("non-decimal Body", &b"Mid: A\r\nBody: 1x\r\n\r\n\r\n"[..]),
            ("negative Body", &b"Mid: A\r\nBody: -1\r\n\r\n\r\n"[..]),
            ("empty Body", &b"Mid: A\r\nBody: \r\n\r\n\r\n"[..]),
            // The case `parse_len`'s digit guard was written for, and the only one that needs
            // it: `usize::from_str` accepts a leading `+`, so `1x` and `-1` above are refused by
            // the parse whether the guard is there or not, and these two are not.
            (
                "plus-signed Body",
                &b"Mid: A\r\nBody: +4\r\n\r\nabcd\r\n"[..],
            ),
            (
                "plus-signed File length",
                &b"Mid: A\r\nBody: 0\r\nFile: +1 n\r\n\r\n\r\nx\r\n"[..],
            ),
            (
                "File with no name",
                &b"Mid: A\r\nBody: 0\r\nFile: 1\r\n\r\n\r\nx\r\n"[..],
            ),
            // The row above has no space at all, so it is refused one branch earlier — by the
            // missing `<len> <name>` separator. This one has the space and nothing after it,
            // which is the only input that reaches `split_file`'s empty-name guard. Without that
            // guard it parses as an attachment named "", which `assemble_b2` cannot write back.
            (
                "File with an empty name",
                &b"Mid: A\r\nBody: 0\r\nFile: 1 \r\n\r\n\r\nx\r\n"[..],
            ),
            (
                "File with no length",
                &b"Mid: A\r\nBody: 0\r\nFile:  x\r\n\r\n\r\nx\r\n"[..],
            ),
            (
                "non-decimal File",
                &b"Mid: A\r\nBody: 0\r\nFile: 1x n\r\n\r\n\r\nx\r\n"[..],
            ),
            (
                "empty header name",
                &b"Mid: A\r\n: v\r\nBody: 0\r\n\r\n\r\n"[..],
            ),
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
    ///
    /// The first fixture does not pin [`split_line`] on its own, and that is the whole reason
    /// the second exists: it is refused by [`take_part`] (there is no CRLF after the body) no
    /// matter what `split_line` does, so a `split_line` that has learned to accept `\n` passes
    /// it. The second fixture has an LF-terminated **header block** and a correct CRLF after the
    /// body, so `split_line` is the only thing left that can refuse it — a lenient one parses it
    /// as `mid = "A"`, `body = "hi"`. The third moves the bare LF to the body separator, where
    /// `take_part` is the guard.
    #[test]
    fn a_bare_lf_separator_is_not_accepted() {
        assert_eq!(
            parse_b2(b"Mid: A\nBody: 0\n\n\n"),
            Err(MessageError::Malformed)
        );
        assert_eq!(
            parse_b2(b"Mid: A\nBody: 2\n\nhi\r\n"),
            Err(MessageError::Malformed)
        );
        assert_eq!(
            parse_b2(b"Mid: A\r\nBody: 2\r\n\r\nhi\n"),
            Err(MessageError::Malformed)
        );
        // Positive control: the same message, CRLF throughout, parses — so the three refusals
        // above are about the separator and nothing else.
        assert_eq!(
            parse_b2(b"Mid: A\r\nBody: 2\r\n\r\nhi\r\n").unwrap().body,
            b"hi"
        );
    }

    #[test]
    fn a_header_value_keeps_its_bytes_but_loses_one_leading_space() {
        let m = parse_b2(b"Mid: A\r\nX:v\r\nY:  v \r\nBody: 0\r\n\r\n\r\n").unwrap();
        assert_eq!(
            m.headers.iter().collect::<Vec<_>>(),
            vec![(&b"X"[..], &b"v"[..]), (&b"Y"[..], &b" v "[..])]
        );
    }

    /// The finding this file was reopened for: a colon in a header **name** is the one field
    /// this module could once serialise into a *different message*. `a:b: c` parses back as the
    /// header `a` with the value `b: c` — `Ok`, no error, corrupt. Now refused before a byte is
    /// written.
    #[test]
    fn an_unrepresentable_header_is_refused_by_assemble() {
        let with = |name: &[u8], value: &[u8]| Message {
            mid: b"M".to_vec(),
            headers: vec![(name.to_vec(), value.to_vec())].into_iter().collect(),
            body: vec![],
            attachments: vec![],
        };
        assert_eq!(
            assemble_b2(&with(b"a:b", b"c")),
            Err(AssembleError::HeaderName),
            "a colon in a header name has no representation"
        );
        assert_eq!(
            assemble_b2(&with(b"", b"c")),
            Err(AssembleError::HeaderName)
        );
        assert_eq!(
            assemble_b2(&with(b"a\r\nb", b"c")),
            Err(AssembleError::HeaderName)
        );
        assert_eq!(
            assemble_b2(&with(b"a", b"c\r\nd")),
            Err(AssembleError::HeaderValue)
        );
        // Positive control: the same header with the colon taken out of the name serialises and
        // survives the round trip, so the refusals above are the colon, not the shape.
        let ok = with(b"ab", b"c");
        assert_eq!(parse_b2(&assemble_b2(&ok).unwrap()), Ok(ok));
    }

    /// The other two fields written into a header line, refused up front for the same reason the
    /// colon is: the composer owns the field, so the composer gets the error. Neither CRLF case
    /// was ever caught loudly by the round trip — a pair in the MID or in an attachment name
    /// parses back `Ok` as a different message, and a lone CR parses back unchanged. The empty
    /// attachment name is the one case here [`parse_b2`] refuses on its own.
    #[test]
    fn an_unrepresentable_mid_or_attachment_name_is_refused_by_assemble() {
        let msg = |mid: &[u8], name: &[u8]| Message {
            mid: mid.to_vec(),
            headers: Headers::default(),
            body: vec![],
            attachments: vec![Attachment {
                name: name.to_vec(),
                data: b"d".to_vec(),
            }],
        };
        assert_eq!(assemble_b2(&msg(b"M\rX", b"a")), Err(AssembleError::Mid));
        assert_eq!(assemble_b2(&msg(b"M\nX", b"a")), Err(AssembleError::Mid));
        assert_eq!(
            assemble_b2(&msg(b"M", b"")),
            Err(AssembleError::AttachmentName),
            "an empty name has nothing to separate it from the space in front of it"
        );
        assert_eq!(
            assemble_b2(&msg(b"M", b"a\nb")),
            Err(AssembleError::AttachmentName)
        );
        // Positive control.
        let ok = msg(b"M", b"a");
        assert_eq!(parse_b2(&assemble_b2(&ok).unwrap()), Ok(ok));
    }

    /// The residual `assemble_b2` deliberately leaves to the round trip: a `headers` entry
    /// carrying a structural name is a malformed [`Message`], not an unrepresentable field, and
    /// the round trip catches it the way the module header says such things are caught —
    /// loudly, never as a different message.
    #[test]
    fn a_structural_name_in_headers_fails_its_round_trip_loudly() {
        for (name, value, expected) in [
            (&b"Mid"[..], &b"OTHER"[..], MessageError::Malformed),
            (&b"Body"[..], &b"0"[..], MessageError::Malformed),
            // A phantom `File:` eats the front of the attachment region; the real lengths are
            // computed from the real payloads, so the last part is always short.
            (&b"File"[..], &b"2 p"[..], MessageError::BadFileLength),
        ] {
            let wire = assemble_b2(&Message {
                mid: b"M".to_vec(),
                headers: vec![(name.to_vec(), value.to_vec())].into_iter().collect(),
                body: b"ab".to_vec(),
                attachments: vec![],
            })
            .unwrap();
            assert_eq!(
                parse_b2(&wire),
                Err(expected),
                "{}",
                String::from_utf8_lossy(name)
            );
        }
    }

    proptest! {
        /// Round-trip over arbitrary binary bodies, headers and attachments. The generators
        /// exclude exactly what [`assemble_b2`] refuses and nothing more, so embedded CRLFs in a
        /// body, NULs and invalid UTF-8 everywhere are all in range. The one extra exclusion is
        /// a structural header name, which is not an unrepresentable field but a malformed
        /// [`Message`] — pinned by `a_structural_name_in_headers_fails_its_round_trip_loudly`
        /// instead, because its round trip is an error rather than an equality.
        #[test]
        fn arbitrary_messages_round_trip(
            mid in proptest::collection::vec(
                any::<u8>().prop_filter("no CR/LF in a MID", |b| *b != b'\r' && *b != b'\n'),
                1..12,
            ),
            headers in proptest::collection::vec(
                (
                    proptest::collection::vec(
                        any::<u8>().prop_filter("no colon/CR/LF in a header name", |b| {
                            *b != b':' && *b != b'\r' && *b != b'\n'
                        }),
                        1..12,
                    )
                    .prop_filter("a structural name belongs in its own field", |n: &Vec<u8>| {
                        ![HDR_MID, HDR_BODY, HDR_FILE]
                            .iter()
                            .any(|h| n.eq_ignore_ascii_case(h))
                    }),
                    proptest::collection::vec(
                        any::<u8>()
                            .prop_filter("no CR/LF in a header value", |b| {
                                *b != b'\r' && *b != b'\n'
                            }),
                        0..24,
                    ),
                ),
                0..4,
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
                headers: headers.into_iter().collect(),
                body,
                attachments: atts
                    .into_iter()
                    .map(|(name, data)| Attachment { name, data })
                    .collect(),
            };
            prop_assert_eq!(parse_b2(&assemble_b2(&msg).unwrap()), Ok(msg));
        }
    }
}

#[cfg(test)]
mod heap_tests {
    use super::*;

    #[test]
    fn a_message_of_many_headers_does_not_cost_ten_times_its_plaintext() {
        // 200_000 header lines of "A: B\r\n" — 1_200_024 plaintext bytes. This is not a hostile
        // input: it is legal B2, inside every ceiling b2f enforces, and answered `FS +`.
        //
        // MEASURED, on this machine, by running the test before and after the representation
        // change: 12_982_916 bytes (10.8x) as `Vec<(Vec<u8>, Vec<u8>)>`, 4_718_596 bytes (3.9x)
        // as one block with spans.
        //
        // ⚠️ **3x is unreachable for THIS shape and the bound says 4 for that reason, not to be
        // generous.** A `Span` is 16 bytes and a header line here is 6 plaintext bytes, so the
        // span table alone is a 2.67x floor. That is an artefact of one-byte names and values;
        // `a_message_at_winlinks_account_maximum_costs_about_its_own_size` below measures the
        // shape real mail actually has.
        //
        // ⚠️ **And this counts requested `capacity()` only — it is a LOWER bound on real memory.**
        // The old shape made two allocations per header (400_000 of them here), each carrying an
        // allocator header and rounded up to a minimum chunk; that per-allocation overhead is
        // what the ~22.6x figure in the programme ledger reflects and it is not countable from
        // inside this process. The new shape makes TWO allocations for the whole message, so the
        // uncounted half of the win is larger than the counted half.
        let mut blob = Vec::from(&b"Mid: AAA1\r\n"[..]);
        for _ in 0..200_000 {
            blob.extend_from_slice(b"A: B\r\n");
        }
        blob.extend_from_slice(b"Body: 0\r\n\r\n\r\n");
        let plain = blob.len();
        let msg = parse_b2(&blob).expect("legal B2");
        let heap = msg.heap_bytes();
        // Print it, because the number is the finding.
        eprintln!(
            "plaintext {plain} bytes -> Message {heap} bytes ({:.1}x)",
            heap as f64 / plain as f64
        );
        assert!(
            heap <= plain * 4,
            "a Message costs {:.1}x its plaintext ({heap} bytes for {plain}); at Winlink's own \
             120,000-byte account maximum that is a message a CMS can legitimately send us",
            heap as f64 / plain as f64
        );
    }

    /// The shape that actually matters: a message at Winlink's published 120,000-byte account
    /// maximum, which expands to roughly 5.7 MB of plaintext. That is legitimate mail no CMS
    /// would refuse, and it is what the programme ledger escalated.
    ///
    /// Real mail is a handful of headers and a large body, so the span table is noise and the
    /// message should cost about its own size. The bound is 1.25x rather than 1.0x because a
    /// `Vec` that grew by doubling can hold up to twice its length, and `capacity()` — correctly
    /// — charges what is held, not what was asked for.
    #[test]
    fn a_message_at_winlinks_account_maximum_costs_about_its_own_size() {
        let body_len = 5_700_000usize;
        let mut blob = Vec::from(&b"Mid: ABCDEFGHIJKL\r\n"[..]);
        for (name, value) in [
            ("Date", "2026/09/07 12:00"),
            ("Type", "Private"),
            ("From", "SMTP:netcontrol@example.com"),
            ("To", "N0CALL"),
            ("Cc", "W1AW"),
            ("Subject", "Traffic for the section net"),
            ("Mbo", "WL2K"),
        ] {
            blob.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
        }
        blob.extend_from_slice(format!("Body: {body_len}\r\n\r\n").as_bytes());
        blob.extend(std::iter::repeat_n(b'x', body_len));
        blob.extend_from_slice(b"\r\n");

        let plain = blob.len();
        let msg = parse_b2(&blob).expect("legal B2");
        let heap = msg.heap_bytes();
        eprintln!(
            "account-maximum message: plaintext {plain} bytes -> Message {heap} bytes ({:.2}x)",
            heap as f64 / plain as f64
        );
        assert!(
            heap * 4 <= plain * 5,
            "a message at the account maximum costs {:.2}x its plaintext ({heap} bytes for \
             {plain}) — real mail should cost about its own size",
            heap as f64 / plain as f64
        );
    }

    /// `heap_bytes` must actually respond to the headers, or both bounds above are vacuous.
    ///
    /// THE POSITIVE CONTROL: an empty-header message and a many-header message with the same
    /// body must not cost the same.
    #[test]
    fn heap_bytes_charges_the_header_block() {
        let bare = parse_b2(b"Mid: A\r\nBody: 0\r\n\r\n\r\n").expect("legal B2");
        let mut with_headers = Vec::from(&b"Mid: A\r\n"[..]);
        for i in 0..1000 {
            with_headers.extend_from_slice(format!("H{i}: value-{i}\r\n").as_bytes());
        }
        with_headers.extend_from_slice(b"Body: 0\r\n\r\n\r\n");
        let loaded = parse_b2(&with_headers).expect("legal B2");
        assert!(
            loaded.heap_bytes() > bare.heap_bytes() + 10_000,
            "heap_bytes does not charge the header block: {} vs {}",
            loaded.heap_bytes(),
            bare.heap_bytes()
        );
        assert_eq!(loaded.headers.len(), 1000);
    }
}
