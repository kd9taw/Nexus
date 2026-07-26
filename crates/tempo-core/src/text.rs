//! Free-text chat over FT1: word-wrapped chunking + reassembly.
//!
//! FT1's free-text frame holds ~13 characters from the WSJT-X alphabet
//! (`0-9 A-Z space + - . / ?`, case-insensitive, uppercased on encode) and
//! carries **no callsign**. To send arbitrary-length messages, Tempo splits text
//! into chunks framed as `<id><seq><tot><payload>`:
//!   - `id`  : 'A'..'Z' — message id within a session
//!   - `seq` : 1..9 — chunk number
//!   - `tot` : 1..9 — total chunks
//!   - payload: up to [`PAYLOAD`] chars
//!
//! Chunks are **word-wrapped** so a chunk never begins or ends with a space —
//! this avoids the modem trimming boundary spaces. Reassembly rejoins chunks
//! with single spaces. (Multiple/awkward spacing is normalized — fine for
//! human messages; the trade for reliability on a 13-char substrate.)

use std::collections::{BTreeMap, HashMap};

/// Max characters in a free-text frame (conservative; alpha-heavy limit).
pub const FREETEXT_MAX: usize = 13;
/// Chunk header length (`id` + `seq` + `tot`).
pub const HEADER: usize = 3;
/// Max payload chars per chunk.
pub const PAYLOAD: usize = FREETEXT_MAX - HEADER; // 10
/// Max chunks per message (seq/tot are single digits).
pub const MAX_CHUNKS: usize = 9;

const ALLOWED: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ +-./?";

/// Uppercase and restrict to the FT1 free-text charset (unsupported → '?').
pub fn sanitize(s: &str) -> String {
    s.to_uppercase()
        .chars()
        .map(|c| if ALLOWED.contains(c) { c } else { '?' })
        .collect()
}

/// Normalize whitespace the way reassembly will (single spaces, trimmed).
pub fn normalize(s: &str) -> String {
    sanitize(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split a message into Tempo free-text chunk frames. `id` should be 'A'..'Z'.
pub fn chunk(msg: &str, id: char) -> Vec<String> {
    let s = sanitize(msg);
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();

    for word in s.split_whitespace() {
        // Hard-split words longer than the payload budget.
        let mut chars: Vec<char> = word.chars().collect();
        while chars.len() > PAYLOAD {
            if !cur.is_empty() {
                chunks.push(std::mem::take(&mut cur));
            }
            chunks.push(chars[..PAYLOAD].iter().collect());
            chars.drain(..PAYLOAD);
        }
        let w: String = chars.iter().collect();
        if w.is_empty() {
            continue;
        }
        if cur.is_empty() {
            cur = w;
        } else if cur.chars().count() + 1 + w.chars().count() <= PAYLOAD {
            cur.push(' ');
            cur.push_str(&w);
        } else {
            chunks.push(std::mem::take(&mut cur));
            cur = w;
        }
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    if chunks.len() > MAX_CHUNKS {
        chunks.truncate(MAX_CHUNKS);
    }

    let tot = chunks.len();
    chunks
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{}{}{}{}", id, i + 1, tot, p))
        .collect()
}

/// If `frame` is a Tempo text chunk, return `(id, seq, total, payload)`.
pub fn parse_chunk(frame: &str) -> Option<(char, usize, usize, String)> {
    let cs: Vec<char> = frame.chars().collect();
    if cs.len() < HEADER {
        return None;
    }
    let id = cs[0];
    if !id.is_ascii_uppercase() {
        return None;
    }
    let seq = cs[1].to_digit(10)? as usize;
    let tot = cs[2].to_digit(10)? as usize;
    if seq < 1 || tot < 1 || seq > tot || tot > MAX_CHUNKS {
        return None;
    }
    Some((id, seq, tot, cs[HEADER..].iter().collect()))
}

/// One message being reassembled: how many chunks it needs, the chunks so far, and the slot its
/// most recent chunk arrived in (for ageing out a set that never completes).
#[derive(Debug)]
struct Partial {
    tot: usize,
    parts: BTreeMap<usize, String>,
    last_slot: u64,
}

/// A chunk set that arrived but never completed — surfaced so the operator sees
/// "incomplete (2 of 3) from N9UM" instead of a message silently never appearing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incomplete {
    pub from: String,
    pub id: char,
    pub have: usize,
    pub tot: usize,
    /// What did arrive, joined — partial text is better than nothing.
    pub text: String,
}

/// Accumulates chunk frames and yields complete messages.
///
/// ⚠️ KEYED BY (SENDER, id), NOT id ALONE. Chunk ids only cycle `A..Z`, so a bare-`id` map
/// MERGES two stations' chunks when both happen to be sending message `B` — one garbled message
/// out of two real ones — and also merges a stale partial `B` with a NEW `B` 26 messages later.
/// Invisible with a single peer on a quiet band; a real corruption path on a busy opening with
/// several Tempo stations. Pass `""` when the sender is unknown; that is its own bucket rather
/// than a wildcard that would collide with everyone.
#[derive(Debug, Default)]
pub struct Reassembler {
    buffers: HashMap<(String, char), Partial>,
}

impl Reassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a frame heard from `from` in `slot`. Returns `Some(message)` when a chunk set is
    /// complete, `None` if the frame is not a chunk or the message is still partial.
    pub fn accept(&mut self, from: &str, frame: &str, slot: u64) -> Option<String> {
        let (id, seq, tot, payload) = parse_chunk(frame)?;
        let key = (from.to_string(), id);
        let entry = self.buffers.entry(key.clone()).or_insert(Partial {
            tot,
            parts: BTreeMap::new(),
            last_slot: slot,
        });
        entry.tot = tot;
        entry.last_slot = slot;
        // Re-inserting the same seq overwrites, so a retransmission cannot inflate the count —
        // only DISTINCT chunks complete a set. Order does not matter (BTreeMap by seq), so a
        // set can arrive 2-then-1 and still assemble.
        entry.parts.insert(seq, payload);
        if entry.parts.len() == tot {
            let done = self.buffers.remove(&key).unwrap();
            Some(done.parts.into_values().collect::<Vec<_>>().join(" "))
        } else {
            None
        }
    }

    /// Drop chunk sets whose newest chunk is older than `max_age` slots, returning what they had.
    ///
    /// Without this a partial set is buffered FOREVER: the operator sees the fragments in band
    /// activity, the chat window stays empty, and nothing ever explains why (exactly what
    /// happened on the first two-station QSO, 2026-07-26). The caller surfaces the returned
    /// [`Incomplete`] so the message is visibly undelivered rather than silently absent.
    ///
    /// ⚠️ `max_age` must comfortably exceed the sender's whole retry budget — a chunk can
    /// legitimately arrive many cycles later when an earlier burst was lost. Ageing out early
    /// would discard a set that was about to complete.
    pub fn age_out(&mut self, now_slot: u64, max_age: u64) -> Vec<Incomplete> {
        let stale: Vec<(String, char)> = self
            .buffers
            .iter()
            .filter(|(_, p)| now_slot.saturating_sub(p.last_slot) > max_age)
            .map(|(k, _)| k.clone())
            .collect();
        stale
            .into_iter()
            .filter_map(|k| {
                let p = self.buffers.remove(&k)?;
                Some(Incomplete {
                    from: k.0,
                    id: k.1,
                    have: p.parts.len(),
                    tot: p.tot,
                    text: p.parts.into_values().collect::<Vec<_>>().join(" "),
                })
            })
            .collect()
    }

    /// Chunk sets still waiting, newest-chunk-first — for a live "incomplete (2 of 3)" indicator
    /// while the sender is still retrying.
    pub fn pending(&self) -> Vec<Incomplete> {
        let mut v: Vec<(u64, Incomplete)> = self
            .buffers
            .iter()
            .map(|(k, p)| {
                (
                    p.last_slot,
                    Incomplete {
                        from: k.0.clone(),
                        id: k.1,
                        have: p.parts.len(),
                        tot: p.tot,
                        text: p.parts.values().cloned().collect::<Vec<_>>().join(" "),
                    },
                )
            })
            .collect();
        v.sort_by(|a, b| b.0.cmp(&a.0));
        v.into_iter().map(|(_, i)| i).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_uppercases_and_filters() {
        assert_eq!(sanitize("hello, world!"), "HELLO? WORLD?");
        assert_eq!(sanitize("MSG/01 ok-go."), "MSG/01 OK-GO.");
    }

    #[test]
    fn chunks_fit_frame_budget_and_have_no_boundary_spaces() {
        let frames = chunk("HELLO TEMPO THIS IS A LONGER TEST MESSAGE 73", 'A');
        assert!(frames.len() > 1);
        for f in &frames {
            assert!(f.chars().count() <= FREETEXT_MAX, "frame too long: {f}");
            let (_, _, _, payload) = parse_chunk(f).expect("valid chunk");
            assert!(
                !payload.starts_with(' ') && !payload.ends_with(' '),
                "boundary space in {f}"
            );
        }
    }

    #[test]
    fn chunk_then_reassemble_roundtrips() {
        let msg = "HELLO TEMPO THIS IS A LONGER TEST MESSAGE 73";
        let frames = chunk(msg, 'A');
        let mut r = Reassembler::new();
        let mut out = None;
        for f in &frames {
            if let Some(full) = r.accept("N9UM", f, 0) {
                out = Some(full);
            }
        }
        assert_eq!(out.as_deref(), Some(normalize(msg).as_str()));
    }

    #[test]
    fn reassembles_out_of_order() {
        let frames = chunk("ONE TWO THREE FOUR FIVE SIX SEVEN", 'B');
        let mut r = Reassembler::new();
        let mut out = None;
        for f in frames.iter().rev() {
            if let Some(full) = r.accept("N9UM", f, 0) {
                out = Some(full);
            }
        }
        assert_eq!(out.as_deref(), Some("ONE TWO THREE FOUR FIVE SIX SEVEN"));
    }

    // ⚠️ THE CORRUPTION THIS KEY EXISTS TO PREVENT. Chunk ids only cycle 'A'..'Z', so on a busy
    // band two stations are eventually mid-message on the SAME id. Keyed on id alone their
    // chunks merged into one garbled message — and a stale partial also merged with a NEW
    // message reusing that id 26 messages later.
    #[test]
    fn two_stations_on_the_same_id_never_merge() {
        let a = chunk("ALPHA BRAVO CHARLIE DELTA", 'B');
        let b = chunk("ZULU YANKEE XRAY WHISKEY", 'B');
        assert!(a.len() > 1 && b.len() > 1, "need multi-chunk messages");
        let mut r = Reassembler::new();

        // Interleave the two stations, as a shared band actually delivers them.
        let mut from_a = None;
        let mut from_b = None;
        for i in 0..a.len().max(b.len()) {
            if let Some(f) = a.get(i) {
                if let Some(full) = r.accept("N9UM", f, i as u64) {
                    from_a = Some(full);
                }
            }
            if let Some(f) = b.get(i) {
                if let Some(full) = r.accept("W1ABC", f, i as u64) {
                    from_b = Some(full);
                }
            }
        }
        assert_eq!(from_a.as_deref(), Some("ALPHA BRAVO CHARLIE DELTA"));
        assert_eq!(from_b.as_deref(), Some("ZULU YANKEE XRAY WHISKEY"));
    }

    // A message that never completes must not sit in the buffer forever with nothing said. The
    // operator saw exactly this on the first two-station QSO: fragments visible in band
    // activity, chat window empty, no explanation.
    #[test]
    fn an_incomplete_set_ages_out_and_reports_what_it_had() {
        let frames = chunk("YOU ARE THE MAN SETH", 'B');
        assert_eq!(frames.len(), 3);
        let mut r = Reassembler::new();
        // Chunks 1 and 2 arrive; 3 never does.
        assert_eq!(r.accept("N9UM", &frames[0], 10), None);
        assert_eq!(r.accept("N9UM", &frames[1], 11), None);

        // Still inside the retry window: it must NOT be discarded — a late chunk is normal.
        assert!(r.age_out(20, 30).is_empty(), "must not age out mid-retry");
        let pending = r.pending();
        assert_eq!(pending.len(), 1);
        assert_eq!((pending[0].have, pending[0].tot), (2, 3));
        assert_eq!(pending[0].from, "N9UM");

        // Past the window: reported once, with the partial text, then gone.
        let aged = r.age_out(100, 30);
        assert_eq!(aged.len(), 1);
        assert_eq!(aged[0].from, "N9UM");
        assert_eq!((aged[0].have, aged[0].tot), (2, 3));
        assert!(aged[0].text.starts_with("YOU ARE"), "keeps what did arrive");
        assert!(
            r.age_out(200, 30).is_empty(),
            "reported once, not every sweep"
        );
        assert!(r.pending().is_empty());
    }

    // A completed set must leave nothing behind, or `pending()` would report a phantom
    // "incomplete" for a message the operator already read.
    #[test]
    fn a_completed_message_leaves_no_pending_state() {
        let frames = chunk("HELLO THERE FRIEND", 'C');
        let mut r = Reassembler::new();
        for (i, f) in frames.iter().enumerate() {
            r.accept("N9UM", f, i as u64);
        }
        assert!(r.pending().is_empty());
        assert!(r.age_out(999, 1).is_empty());
    }

    // Retransmissions are how this protocol survives a lossy band — a repeat must overwrite,
    // never inflate the count toward a false "complete".
    #[test]
    fn retransmitted_chunks_do_not_fake_completion() {
        let frames = chunk("ONE TWO THREE FOUR FIVE", 'D');
        assert!(frames.len() >= 2);
        let mut r = Reassembler::new();
        for _ in 0..5 {
            assert_eq!(
                r.accept("N9UM", &frames[0], 0),
                None,
                "the same chunk five times is still one chunk"
            );
        }
        assert_eq!(r.pending()[0].have, 1);
    }

    #[test]
    fn non_chunk_frames_rejected() {
        assert!(parse_chunk("HELLO WORLD").is_none());
        assert!(parse_chunk("CQ W9XYZ EN37").is_none());
        assert!(parse_chunk("A12HELLO").is_some());
    }
}
