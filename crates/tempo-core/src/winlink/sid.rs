//! The FBB/B2F **SID** (System IDentification) line grammar.
//!
//! Each side opens by announcing itself in square brackets — `[WL2K-5.0-B2FWIHJM$]`,
//! `[RMS Express-1.5.45.0-B2FHM$]`, `[Paclink-unix-2.7-B2FIHM$]`, `[FBB-7.00i-B1FHM$]` — in the
//! form `[<product>-<version>-<flags>$]`, and the trailing flag letters say what the peer can do.
//!
//! **The contract, and the whole of it: require `F` and `B2`, tolerate the rest.** Nexus needs
//! FBB forwarding (`F`) and the B2 message format (`B2`), and it implements no fallback to
//! either's absence — so [`parse_sid`] refuses a peer that lacks one *at the handshake*, where
//! the reason is still legible, rather than proceeding into a transfer that cannot work and
//! failing later as a checksum or a framing error. Every OTHER letter is ignored, and kept
//! verbatim in [`Sid::flags`]. That leniency is the point: the flag set is open, real gateways
//! already advertise `W`, `I`, `J`, `H`, `M`, `C`, `G` and letters no published document lists,
//! and a gateway deployed after this code ships will advertise letters that do not exist today.
//! Refusing an unknown flag would refuse a *working* CMS for the sole offence of naming a
//! capability we do not model — the forward-compatibility failure this module exists to avoid.
//! Strict about what it needs, lenient about everything else.
//!
//! ⚠️ **Three grammar decisions here are readings, not citations.** There is no published SID
//! BNF, and each decision is confined to one small function so a live connect can settle it by
//! editing that function and nothing else:
//!
//! 1. **The three fields split from the RIGHT** (`split_body`) — flags after the last `-`,
//!    version between the last two, product everything before. `Paclink-unix` puts a hyphen in
//!    its own product name, so a left-to-right split mis-slices a real peer. The cost is the
//!    mirror case: a version carrying a hyphen (`1.0-beta`) would be mis-split. No observed SID
//!    does that, and one of them has to lose.
//! 2. **`B` takes the ASCII digits after it as ONE level token** (`scan_flags`) — `B1F` is
//!    level 1 (refused), `B2F` is level 2 (accepted), `B23` is level 23 and is NOT B2 even
//!    though `"B2"` is a substring of it. That last case is the only one where the tokenizer
//!    and a substring search disagree, and a test pins it (a substring mutant passes every
//!    other test in the module). A higher level is also *not* read as implying B2 — a peer
//!    offering only `B3` has not said it speaks B2, and guessing that it does is how a
//!    handshake fails after the negotiation instead of during it.
//! 3. **The `$` before `]` is required framing.** Historically `$` is itself a flag (BID
//!    support), so a peer omitting it is arguably well-formed rather than malformed. Every
//!    Winlink SID observed carries it, and treating it as framing is what this task specifies.
//!    If a real peer ever omits it, the fix is one condition in `frame` — nothing else reads it.
//!
//! Case is significant: flags are uppercase in every document and every observed SID, so a
//! lowercase letter is an *unknown flag*, not a synonym. Folding case would be tolerance with no
//! evidence behind it, and it would hide a peer that is genuinely speaking something else.
//!
//! Bytes, not `String`, throughout — the module-level rule in [`super`], and it bites here: a
//! peer's product name is whatever it put on the wire, and a lossy UTF-8 decode on the way in
//! silently rewrites the one field an operator would compare against another client's log.

/// A peer's parsed SID line: who it says it is, plus the two capabilities we require.
///
/// Only ever produced by [`parse_sid`], which means [`fbb`](Sid::fbb) and [`b2`](Sid::b2) are
/// both `true` in every value that exists — they are fields rather than an implied invariant so
/// a caller holding a `Sid` can read the capability directly instead of having to know that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sid {
    /// The product identifier — `WL2K`, `RMS Express`, `Paclink-unix`. May contain spaces and
    /// hyphens, and is not guaranteed UTF-8; kept as the bytes that arrived.
    pub product: Vec<u8>,
    /// The version field, verbatim and unparsed. `7.00i` and `3.4.062` are not semver, nothing
    /// in the handshake branches on a version, and turning it into numbers here would only
    /// invent a comparison nobody asked for.
    pub version: Vec<u8>,
    /// The flag letters exactly as received, with the trailing `$` excluded (it is framing —
    /// see the module header). Kept whole so a flag we do not model survives into a log line
    /// instead of being silently dropped by the parser that tolerated it.
    pub flags: Vec<u8>,
    /// `F` — FBB forwarding. Always `true`; see the type-level note.
    pub fbb: bool,
    /// `B2` — the B2 message format. Always `true`; see the type-level note.
    pub b2: bool,
}

/// Why a SID was refused. Three distinct causes because they need three different operator
/// answers: a capability gap is the far end's, malformed framing is usually ours or a desync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidError {
    /// Well-formed, forwards, but does not offer `B2` — a B1-only FBB BBS, say. There is no
    /// downgrade path in this implementation, so the session cannot continue.
    MissingB2,
    /// Well-formed but does not offer `F`. Checked **before** `B2`, so a SID offering neither
    /// reports this one: without FBB forwarding there is no B2 exchange to be missing.
    MissingFbb,
    /// The `[<product>-<version>-<flags>$]` framing is broken — no brackets, no `$`, a missing
    /// or empty field, or more than the SID on the line. Says nothing about capabilities;
    /// usually it means the line was not a SID at all.
    Malformed,
}

impl std::fmt::Display for SidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SidError::MissingB2 => {
                write!(f, "peer does not offer B2 — it cannot carry B2 messages")
            }
            SidError::MissingFbb => write!(f, "peer does not offer F — it does not forward mail"),
            SidError::Malformed => write!(f, "not a well-formed [product-version-flags$] SID"),
        }
    }
}

impl std::error::Error for SidError {}

/// Parse a SID line, requiring `F` and `B2` and tolerating every other flag.
///
/// `line` is the raw line as it arrived; a trailing CRLF (and any surrounding ASCII whitespace)
/// is tolerated, because the wire terminates lines and a caller should not have to strip that
/// to be understood. Everything else between the brackets must be the SID and nothing else — a
/// line carrying a SID *plus* another token is a desync, not a SID, and reading one out of the
/// middle of it would paper over that.
pub fn parse_sid(line: &[u8]) -> Result<Sid, SidError> {
    let body = frame(line).ok_or(SidError::Malformed)?;
    let (product, version, flags) = split_body(body).ok_or(SidError::Malformed)?;

    let (fbb, b2) = scan_flags(flags);
    // Order is load-bearing and pinned by a test: F first, so "offers neither" reads as
    // MissingFbb rather than depending on which check happens to run first after an edit.
    if !fbb {
        return Err(SidError::MissingFbb);
    }
    if !b2 {
        return Err(SidError::MissingB2);
    }

    Ok(Sid {
        product: product.to_vec(),
        version: version.to_vec(),
        flags: flags.to_vec(),
        fbb,
        b2,
    })
}

/// Strip the `[` … `$]` framing, returning the body between them.
///
/// The `$`-is-framing decision lives here and only here (module header, note 3): relaxing it to
/// "an optional BID flag" is one condition in this function.
fn frame(line: &[u8]) -> Option<&[u8]> {
    let line = line.trim_ascii();
    let body = line.strip_prefix(b"[")?.strip_suffix(b"$]")?;
    if body.is_empty() {
        return None;
    }
    Some(body)
}

/// Split `<product>-<version>-<flags>` from the RIGHT (module header, note 1), so a hyphen in
/// the product name is kept rather than mistaken for a field separator.
///
/// Returns `None` unless all three fields are present and non-empty — an empty version or an
/// empty flag string is a broken line, not a peer with no capabilities.
fn split_body(body: &[u8]) -> Option<(&[u8], &[u8], &[u8])> {
    let last = body.iter().rposition(|&b| b == b'-')?;
    let prev = body[..last].iter().rposition(|&b| b == b'-')?;
    let product = &body[..prev];
    let version = &body[prev + 1..last];
    let flags = &body[last + 1..];
    if product.is_empty() || version.is_empty() || flags.is_empty() {
        return None;
    }
    Some((product, version, flags))
}

/// Scan the flag string for the only two capabilities we require, returning `(fbb, b2)`.
///
/// Unknown letters advance the cursor and are otherwise ignored — that is the tolerance the
/// module exists for. `B` is the one flag that carries an argument: the ASCII digits following
/// it are its protocol level, consumed as one token (module header, note 2), so `B1` and the
/// hypothetical `B12` are levels 1 and 12 and neither is `B2`.
fn scan_flags(flags: &[u8]) -> (bool, bool) {
    let (mut fbb, mut b2) = (false, false);
    let mut i = 0;
    while i < flags.len() {
        match flags[i] {
            b'F' => {
                fbb = true;
                i += 1;
            }
            b'B' => {
                let digits_from = i + 1;
                let mut end = digits_from;
                while end < flags.len() && flags[end].is_ascii_digit() {
                    end += 1;
                }
                b2 |= &flags[digits_from..end] == b"2";
                // `end >= i + 1` always, so the cursor advances even for a bare `B`.
                i = end;
            }
            _ => i += 1,
        }
    }
    (fbb, b2)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- the two negative controls the spec names (§5) ---------------------------------

    /// A real FBB BBS SID: it forwards (`F`) but speaks only the **B1** compressed protocol.
    /// We cannot carry B2 messages to it, so the session must refuse rather than negotiate down.
    #[test]
    fn sid_without_b2_is_refused() {
        assert_eq!(parse_sid(b"[FBB-7.00i-B1FHM$]"), Err(SidError::MissingB2));
    }

    /// Undocumented capability letters must be tolerated, not rejected, as long as `F` and `B2`
    /// hold — the flag set is open, and refusing an unknown letter refuses a working gateway.
    #[test]
    fn sid_with_invented_flags_still_proceeds() {
        let sid = parse_sid(b"[TestGW-1.0-B2FHMQ9Z$]").expect("F and B2 present -> must parse");
        assert!(sid.fbb && sid.b2);
        assert_eq!(sid.flags, b"B2FHMQ9Z", "unknown flags must be kept verbatim");
    }

    // ---- the shapes real peers actually send --------------------------------------------

    #[test]
    fn real_world_sids_parse() {
        for line in [
            &b"[WL2K-5.0-B2FWIHJM$]"[..],
            &b"[RMS Express-1.5.45.0-B2FHM$]"[..],
            &b"[Airmail-3.4.062-B2FHIM$]"[..],
        ] {
            let sid = parse_sid(line).unwrap_or_else(|e| panic!("{line:?} must parse, got {e:?}"));
            assert!(sid.fbb && sid.b2, "{line:?}");
        }
    }

    /// `Paclink-unix` puts a hyphen in the product name, so the three fields are split from the
    /// RIGHT: flags after the last `-`, version between the last two, product is everything left.
    #[test]
    fn product_may_contain_hyphens() {
        let sid = parse_sid(b"[Paclink-unix-2.7-B2FIHM$]").expect("must parse");
        assert_eq!(sid.product, b"Paclink-unix");
        assert_eq!(sid.version, b"2.7");
        assert_eq!(sid.flags, b"B2FIHM");
    }

    // ---- what is required, and which error says so ---------------------------------------

    #[test]
    fn sid_without_fbb_is_refused() {
        assert_eq!(parse_sid(b"[TestGW-1.0-B2HM$]"), Err(SidError::MissingFbb));
    }

    /// Precedence, pinned so it cannot drift: with neither capability the answer is `MissingFbb`
    /// — without FBB forwarding there is no B2 exchange to be missing.
    #[test]
    fn missing_both_reports_missing_fbb() {
        assert_eq!(parse_sid(b"[TestGW-1.0-HM$]"), Err(SidError::MissingFbb));
    }

    /// Positive control on the flag tokenizer: `B` takes the digits after it as ONE level token,
    /// so `B12` is level 12 and `B23` is level 23 — neither is B2.
    ///
    /// `B23` is the case that earns this test. It is the ONLY shape where a `flags.contains("B2")`
    /// substring search and the tokenizer disagree (the substring is there; the token is not), and
    /// swapping `scan_flags` for that search was run as a mutant: it passes every other test in
    /// this module and fails on this line alone. `B12` is kept because it is the shape a reader
    /// expects to see here, but both implementations get it right — it discriminates nothing.
    #[test]
    fn b_level_is_a_token_not_a_substring() {
        assert_eq!(parse_sid(b"[TestGW-1.0-B23FHM$]"), Err(SidError::MissingB2));
        assert_eq!(parse_sid(b"[TestGW-1.0-B12FHM$]"), Err(SidError::MissingB2));
        assert!(parse_sid(b"[TestGW-1.0-FHMB2$]").is_ok(), "B2 last is still B2");
    }

    /// Flags are uppercase everywhere they are documented; lowercase is an unknown flag, not a
    /// synonym. Folding case would be tolerance we have no evidence for.
    #[test]
    fn flags_are_case_sensitive() {
        assert_eq!(parse_sid(b"[TestGW-1.0-b2f$]"), Err(SidError::MissingFbb));
    }

    // ---- framing --------------------------------------------------------------------------

    #[test]
    fn broken_framing_is_malformed() {
        for line in [
            &b""[..],
            &b"   "[..],
            &b"WL2K-5.0-B2FHM"[..],        // no brackets at all
            &b"[WL2K-5.0-B2FHM]"[..],      // no `$` before the `]`
            &b"[WL2K-5.0-B2FHM$"[..],      // no closing bracket
            &b"WL2K-5.0-B2FHM$]"[..],      // no opening bracket
            &b"[$]"[..],                   // empty body
            &b"[WL2K-B2FHM$]"[..],         // only one `-`: no version field
            &b"[-1.0-B2FHM$]"[..],         // empty product
            &b"[WL2K--B2FHM$]"[..],        // empty version
            &b"[WL2K-5.0-$]"[..],          // empty flags
            &b"[WL2K-5.0-B2FHM$] ;PQ:12345"[..], // more than the SID on the line
        ] {
            assert_eq!(parse_sid(line), Err(SidError::Malformed), "{line:?}");
        }
    }

    /// The line arrives off a wire that terminates lines; a trailing CRLF must not be framing.
    #[test]
    fn line_terminator_and_surrounding_space_are_tolerated() {
        let bare = parse_sid(b"[WL2K-5.0-B2FWIHJM$]").expect("must parse");
        assert_eq!(parse_sid(b"[WL2K-5.0-B2FWIHJM$]\r\n"), Ok(bare.clone()));
        assert_eq!(parse_sid(b"  [WL2K-5.0-B2FWIHJM$]\n"), Ok(bare));
    }

    /// The bytes-not-`String` rule is only real if the API accepts bytes a `String` could not
    /// hold: a peer's product name is whatever it put on the wire, and a lossy decode would
    /// silently rewrite it.
    #[test]
    fn parse_is_byte_oriented_not_utf8() {
        let sid = parse_sid(b"[G\xffW-1.0-B2FHM$]").expect("non-UTF-8 product must still parse");
        assert_eq!(sid.product, b"G\xffW");
    }
}
