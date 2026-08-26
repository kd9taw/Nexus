//! Amplifier link — the wire formats for SPE Expert and Elecraft KPA, and nothing else.
//!
//! Pure codec: builds request bytes, parses reply bytes, and owns no port, no thread and no
//! state. The I/O half lives with the poller, exactly as the rotator splits its framing from its
//! transport, so every rule below is testable without an amplifier on the bench.
//!
//! # Why this is written rather than delegated to Hamlib
//!
//! Hamlib has THREE amplifier backends in total (`amplifiers/{elecraft,expert,gemini}`). Gemini
//! is Ethernet-only, so from a serial-port picker that is ONE usable amplifier — for the price
//! of a third daemon and a third port to manage. And its SPE backend says "Initial prototype" in
//! its own README and carries this, verbatim:
//!
//! ```text
//! case RIG_POWER_OFF:     cmd[0] = 0x0a;
//! case RIG_POWER_STANDBY: cmd[0] = 0x0a;   // the identical byte
//! ```
//!
//! …with `expert_reset()` calling `set_powerstat(STANDBY)`. OFF and STANDBY cannot both be
//! `0x0a` and both be right. That is not a driver to put in front of a 2 kW amplifier.
//!
//! # Provenance
//!
//! ⚠️ The frame layouts below are FUNCTIONAL FACTS of the wire — byte order, a checksum rule, a
//! terminator — read from Hamlib's implementation as a protocol reference. No code is ported and
//! no expression is copied; this is the "documented protocol / clean-room reference" arm of the
//! OSS-integration checklist, not a port. **SPE's own protocol document should confirm the
//! keystroke map before anything here is allowed to WRITE**, which in v1 it never is.
//!
//! # v1 is READ-ONLY, and that is a safety decision
//!
//! SPE's whole command set is front-panel KEYSTROKES — relative steps and cycles, no absolute
//! setters. Every write's meaning depends on a state we learn a poll late. And to say it once:
//! putting an amplifier in standby is NOT a way to stop a transmission — the exciter keeps
//! keying and the drive passes straight through — so nothing here may ever appear in a cockpit's
//! stop-line census.

/// Which amplifier family a link speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmpModel {
    /// SPE Expert 1.3K-FA / 1.5K-FA / 2K-FA — one binary protocol, three amplifiers.
    SpeExpert,
    /// Elecraft KPA500 / KPA1500 — line-oriented ASCII. Writing this natively also gets the
    /// KPA500, which Hamlib does not register at all.
    ElecraftKpa,
}

// ─────────────────────────── SPE Expert ───────────────────────────

/// The SPE preamble: three `0x55` bytes open every frame in both directions.
const SPE_PREAMBLE: [u8; 3] = [0x55, 0x55, 0x55];

/// Build an SPE request frame: `55 55 55 <len> <cmd…> <checksum>`.
///
/// The checksum is the low byte of the sum of the COMMAND bytes only — not the preamble and not
/// the length. Returns `None` for an empty command or one longer than a single frame can carry,
/// rather than emitting a frame the amplifier would have to reject.
pub fn spe_request(cmd: &[u8]) -> Option<Vec<u8>> {
    if cmd.is_empty() || cmd.len() > 250 {
        return None;
    }
    let mut f = Vec::with_capacity(cmd.len() + 5);
    f.extend_from_slice(&SPE_PREAMBLE);
    f.push(cmd.len() as u8);
    f.extend_from_slice(cmd);
    // Wrapping sum: the checksum is a byte, and a long command legitimately overflows it.
    f.push(cmd.iter().fold(0u8, |a, b| a.wrapping_add(*b)));
    Some(f)
}

/// How many further bytes a reply carries, given its 4-byte header `55 55 55 <n>`.
///
/// ⚠️ `n` counts the WHOLE frame including the header, so the body still to read is `n - 4`.
/// Hamlib reads `n - 3`, which would leave one byte of this frame in the buffer to be misread as
/// the first byte of the next one. That is exactly the desync class Nexus's rigctld client drops
/// a connection to avoid, so this returns the body length and the caller reads precisely that.
/// **Flagged as the one number the bench must confirm** — if the amplifier turns out to answer
/// with `n - 3`, this constant moves and its test moves with it.
///
/// `None` when the header is short, mispreambled, or claims a length that cannot contain itself.
pub fn spe_reply_body_len(header: &[u8]) -> Option<usize> {
    if header.len() < 4 || header[..3] != SPE_PREAMBLE {
        return None;
    }
    let total = header[3] as usize;
    total.checked_sub(4)
}

// ────────────────────────── Elecraft KPA ──────────────────────────

/// Build an Elecraft query: `^<verb>;`.
///
/// Verbs are two upper-case letters (`FR` frequency, `AE` antenna, `OS` operate/standby). A verb
/// that is not exactly that is refused rather than sent, because the KPA answers an unknown verb
/// with silence and a silent amplifier is indistinguishable from an unplugged one.
pub fn kpa_query(verb: &str) -> Option<String> {
    let ok = verb.len() == 2 && verb.bytes().all(|b| b.is_ascii_uppercase());
    ok.then(|| format!("^{verb};"))
}

/// Extract the payload of a KPA reply for `verb` — `^FR14074;` with verb `FR` yields `14074`.
///
/// Returns `None` when the reply is for a DIFFERENT verb, which matters more than it looks: the
/// KPA streams unsolicited status, so a reply arriving after a query is not necessarily the
/// answer TO it. Matching the verb is what stops an antenna number being read as a frequency.
pub fn kpa_payload<'a>(reply: &'a str, verb: &str) -> Option<&'a str> {
    let body = reply.trim().strip_prefix('^')?.strip_suffix(';')?;
    let rest = body.strip_prefix(verb)?;
    Some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spe_frames_a_request_with_the_documented_checksum() {
        // Single-byte command 0x0b: 55 55 55 01 0b 0b
        let f = spe_request(&[0x0b]).expect("a frame");
        assert_eq!(f, vec![0x55, 0x55, 0x55, 0x01, 0x0b, 0x0b]);

        // Multi-byte: the checksum sums the COMMAND bytes only — not the preamble, not the len.
        let f = spe_request(&[0x10, 0x20, 0x30]).expect("a frame");
        assert_eq!(f, vec![0x55, 0x55, 0x55, 0x03, 0x10, 0x20, 0x30, 0x60]);
        assert_eq!(f[7], 0x10u8 + 0x20 + 0x30, "checksum is the command sum");

        // It WRAPS rather than saturating or panicking — a long command legitimately exceeds 255.
        let f = spe_request(&[0xff, 0x02]).expect("a frame");
        assert_eq!(*f.last().unwrap(), 0x01, "0xff + 0x02 wraps to 0x01");

        // Refused rather than malformed.
        assert!(
            spe_request(&[]).is_none(),
            "an empty command is not a frame"
        );
        assert!(spe_request(&[0u8; 251]).is_none(), "too long to frame");
    }

    #[test]
    fn spe_reply_length_excludes_the_header_and_rejects_nonsense() {
        // n counts the whole frame, so a 12-byte frame has 8 bytes of body left to read.
        assert_eq!(spe_reply_body_len(&[0x55, 0x55, 0x55, 12]), Some(8));
        // A frame that is only its header has no body — legal, and not an error.
        assert_eq!(spe_reply_body_len(&[0x55, 0x55, 0x55, 4]), Some(0));

        // A length that cannot contain its own header is refused, NOT treated as zero: a wrong
        // count desyncs every frame after it, which is the failure this rejects early.
        assert_eq!(spe_reply_body_len(&[0x55, 0x55, 0x55, 3]), None);
        // Wrong preamble, or a truncated header, is not a frame.
        assert_eq!(spe_reply_body_len(&[0x55, 0x55, 0xAA, 12]), None);
        assert_eq!(spe_reply_body_len(&[0x55, 0x55, 0x55]), None);
        assert_eq!(spe_reply_body_len(&[]), None);
    }

    #[test]
    fn kpa_queries_are_built_and_junk_verbs_refused() {
        assert_eq!(kpa_query("FR").as_deref(), Some("^FR;"));
        assert_eq!(kpa_query("OS").as_deref(), Some("^OS;"));
        // Refused rather than sent — the KPA answers an unknown verb with silence, and silence
        // is indistinguishable from a dead link.
        assert!(kpa_query("F").is_none());
        assert!(kpa_query("FRQ").is_none());
        assert!(kpa_query("fr").is_none(), "lower case is not a KPA verb");
        assert!(kpa_query("").is_none());
    }

    #[test]
    fn kpa_payload_matches_the_verb_it_was_asked_for() {
        assert_eq!(kpa_payload("^FR14074;", "FR"), Some("14074"));
        assert_eq!(kpa_payload("^AE2;", "AE"), Some("2"));
        // Whitespace around a framed reply is tolerated.
        assert_eq!(kpa_payload("  ^AE2;\r\n", "AE"), Some("2"));

        // ⭐ THE ONE THAT MATTERS. The KPA streams unsolicited status, so the next thing to
        // arrive after a query is not necessarily its answer. An antenna reply must NEVER be
        // read as a frequency.
        assert_eq!(
            kpa_payload("^AE2;", "FR"),
            None,
            "wrong verb is not an answer"
        );

        // Unframed junk is not a reply.
        assert_eq!(kpa_payload("FR14074", "FR"), None, "no ^ and no ;");
        assert_eq!(kpa_payload("^FR14074", "FR"), None, "unterminated");
        assert_eq!(kpa_payload("", "FR"), None);
    }
}
