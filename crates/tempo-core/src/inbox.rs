//! Directed messaging inbox: turns a stream of decodes into roster updates and
//! attributed chat messages.
//!
//! Because FT1 free-text frames carry no callsign, Tempo attributes free-text to
//! a sender by **temporal association**: a standard frame (CQ/beacon or a
//! directed `TO FROM …` frame) identifies the current talker, and subsequent
//! free-text chunks are attributed to that station until another identifies
//! itself. A station therefore precedes a free-text message with an identifying
//! frame (its beacon, or a directed frame naming the recipient). This is the
//! pragmatic session model for a 13-char, callsign-less free-text substrate.

use crate::message::Msg;
use crate::roster::Roster;
use crate::text::{self, Reassembler};
use modes::Decode;

/// Prefix that marks free text as an open broadcast (sender embedded).
pub const BROADCAST_PREFIX: &str = "DE";

/// If `text` is a `DE <CALL> <body>` open broadcast, return `(call, body)`.
///
/// The sender call is the first token after `DE`; the body is everything after
/// it (must be non-empty — `DE <CALL>` alone is just an identify, not a message).
pub fn parse_broadcast(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix(BROADCAST_PREFIX)?;
    // Require a separating space so "DESK" isn't mistaken for a broadcast.
    let rest = rest.strip_prefix(' ')?;
    let mut parts = rest.splitn(2, ' ');
    let call = parts.next()?.trim();
    let body = parts.next().unwrap_or("").trim();
    if call.is_empty() || body.is_empty() {
        return None;
    }
    Some((call.to_string(), body.to_string()))
}

/// Render an open-broadcast free-text string: `DE <MYCALL> <body>`.
pub fn broadcast_text(mycall: &str, body: &str) -> String {
    format!("{BROADCAST_PREFIX} {mycall} {body}")
}

/// A received chat message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    /// Attributed sender (most-recently-identified station), if known.
    pub from: Option<String>,
    /// Recipient, if the preceding directed frame named one (else broadcast).
    pub to: Option<String>,
    pub text: String,
    pub slot: u64,
    /// True if `to` matched our callsign.
    pub directed_to_me: bool,
}

/// Processes decodes into a roster + chat messages.
pub struct Inbox {
    pub mycall: String,
    pub roster: Roster,
    reasm: Reassembler,
    /// Most recently identified talker (sender of the last standard frame).
    current_from: Option<String>,
    /// Recipient named by the last directed frame, if any.
    current_to: Option<String>,
    pub messages: Vec<ChatMessage>,
}

impl Inbox {
    pub fn new(mycall: &str) -> Self {
        Self {
            mycall: mycall.to_string(),
            roster: Roster::new(),
            reasm: Reassembler::new(),
            current_from: None,
            current_to: None,
            messages: Vec::new(),
        }
    }

    /// Process all decodes heard in a slot.
    pub fn observe(&mut self, decodes: &[Decode], slot: u64) {
        for d in decodes {
            self.roster.observe(d, slot);
            let m = Msg::parse(&d.message);
            match &m {
                // Free text or a Tempo chunk: not a standard form.
                Msg::Other(s) => {
                    if let Some(full) = self.reasm.accept(s) {
                        self.push_text(full, slot);
                    } else if text::parse_chunk(s).is_none() {
                        // A plain (non-chunked) free-text frame.
                        self.push_text(s.clone(), slot);
                    }
                    // else: a partial chunk — buffered, nothing to emit yet.
                }
                // A standard frame identifies the current talker (and maybe a
                // directed recipient), establishing attribution context.
                _ => {
                    if let Some(sender) = m.sender() {
                        self.current_from = Some(sender.to_string());
                    }
                    self.current_to = m.addressee().map(|s| s.to_string());
                }
            }
        }
    }

    fn push_text(&mut self, text: String, slot: u64) {
        // An open broadcast embeds its sender as a `DE <CALL> ` prefix (FT8-style
        // "to everyone"). When the free-text is *not* part of a directed exchange
        // and carries that prefix, attribute it to the embedded call and route it
        // as a broadcast (to = None) into the band-activity bucket.
        if self.current_to.is_none() {
            if let Some((de, body)) = parse_broadcast(&text) {
                self.messages.push(ChatMessage {
                    from: Some(de),
                    to: None,
                    text: body,
                    slot,
                    directed_to_me: false,
                });
                return;
            }
        }

        let to = self.current_to.clone();
        let directed_to_me = to.as_deref() == Some(self.mycall.as_str());
        self.messages.push(ChatMessage {
            from: self.current_from.clone(),
            to,
            text,
            slot,
            directed_to_me,
        });
    }

    /// Messages directed specifically to me.
    pub fn for_me(&self) -> Vec<&ChatMessage> {
        self.messages.iter().filter(|m| m.directed_to_me).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(msg: &str) -> Decode {
        Decode {
            message: msg.to_string(),
            sync: 1.0,
            snr: -8,
            dt: 0.0,
            freq: 1500.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            mode: None,
        }
    }

    #[test]
    fn attributes_freetext_to_identified_sender() {
        let mut inbox = Inbox::new("K2DEF");
        // W9XYZ identifies via a directed frame to me, then sends a 2-chunk msg.
        inbox.observe(&[dec("K2DEF W9XYZ EN37")], 0); // directed grid to me (identify)
        let frames = text::chunk("MEET AT THE REPEATER AT NOON", 'A');
        for (i, f) in frames.iter().enumerate() {
            inbox.observe(&[dec(f)], (i as u64) + 1);
        }
        let mine = inbox.for_me();
        assert_eq!(mine.len(), 1, "messages: {:?}", inbox.messages);
        assert_eq!(mine[0].from.as_deref(), Some("W9XYZ"));
        assert!(mine[0].directed_to_me);
        assert_eq!(
            mine[0].text,
            text::normalize("MEET AT THE REPEATER AT NOON")
        );
        assert!(inbox.roster.get("W9XYZ").is_some());
    }

    #[test]
    fn parse_broadcast_splits_sender_and_body() {
        assert_eq!(
            parse_broadcast("DE W9XYZ HELLO ALL"),
            Some(("W9XYZ".to_string(), "HELLO ALL".to_string()))
        );
        // `DE <CALL>` with no body is an identify, not a broadcast.
        assert_eq!(parse_broadcast("DE W9XYZ"), None);
        // Must have the `DE ` prefix (with separating space).
        assert_eq!(parse_broadcast("DESK W9XYZ HI"), None);
        assert_eq!(parse_broadcast("HELLO ALL"), None);
    }

    #[test]
    fn broadcast_freetext_routes_as_broadcast_to_embedded_sender() {
        let mut inbox = Inbox::new("K2DEF");
        // A bare `DE W9XYZ HELLO ALL` free-text frame, with no prior directed
        // context, is an open broadcast attributed to W9XYZ (to = None).
        inbox.observe(&[dec("DE W9XYZ HELLO ALL")], 0);
        assert_eq!(inbox.messages.len(), 1, "messages: {:?}", inbox.messages);
        let m = &inbox.messages[0];
        assert_eq!(m.from.as_deref(), Some("W9XYZ"));
        assert_eq!(m.to, None, "broadcast has no recipient");
        assert_eq!(m.text, "HELLO ALL");
        assert!(!m.directed_to_me);
    }

    #[test]
    fn chunked_broadcast_reassembles_and_routes() {
        let mut inbox = Inbox::new("K2DEF");
        // A longer broadcast is chunked; reassembled text keeps the DE prefix.
        let frames = text::chunk(&broadcast_text("W9XYZ", "NET ON 7130 AT 0200Z"), 'A');
        for (i, f) in frames.iter().enumerate() {
            inbox.observe(&[dec(f)], i as u64);
        }
        assert_eq!(inbox.messages.len(), 1, "messages: {:?}", inbox.messages);
        let m = &inbox.messages[0];
        assert_eq!(m.from.as_deref(), Some("W9XYZ"));
        assert_eq!(m.to, None);
        assert_eq!(m.text, text::normalize("NET ON 7130 AT 0200Z"));
    }
}
