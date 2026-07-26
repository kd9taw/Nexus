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

/// One message being reassembled: how many chunks it needs, the chunks so far, the slot its most
/// recent chunk arrived in (for ageing out a set that never completes), and who we believe sent it.
#[derive(Debug)]
struct Partial {
    tot: usize,
    parts: BTreeMap<usize, String>,
    last_slot: u64,
    /// Bound from the talker context at the FIRST chunk of this id and never revised — see the
    /// warning on [`Reassembler`].
    from: String,
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
/// ⚠️ KEYED BY `id` ALONE, DELIBERATELY. Sender attribution is recorded at the FIRST chunk of a
/// message and never revised — it is for display, NOT for routing.
///
/// This was briefly keyed on `(sender, id)` to stop two stations on the same id from merging.
/// That is WRONG here and was reverted before it ever shipped: a chunk carries no callsign, so
/// the sender can only come from the talker context, and that context is updated by ANY standard
/// frame from ANY station. Chunks arrive one per own-parity slot (~8 s apart at TempoFast), so on
/// a live band another station's frame lands between them and moves the context — chunk 1 buckets
/// under one call, chunk 2 under another, and the message NEVER completes. That breaks the common
/// case on any active band to fix a rare one.
///
/// KNOWN LIMIT, accepted: two stations mid-message on the SAME id merge into one garbled message,
/// and a stale partial can merge with a new message reusing that id 26 messages later. Ids cycle
/// `A..Z`, so this needs several simultaneous Tempo stations to bite. The real fix is a sender
/// hash IN THE FRAME — see the Tempo header-packing item, which is already reworking the 3-byte
/// header and is where the bits for it would come from.
#[derive(Debug, Default)]
pub struct Reassembler {
    buffers: HashMap<char, Partial>,
}

impl Reassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a frame heard from `from` in `slot`. Returns `Some(message)` when a chunk set is
    /// complete, `None` if the frame is not a chunk or the message is still partial.
    ///
    /// `from` is recorded on the FIRST chunk of an id and ignored afterwards: the talker context
    /// drifts between a message's chunks on a live band, so routing on it would split the message
    /// (see the warning on [`Reassembler`]).
    pub fn accept(&mut self, from: &str, frame: &str, slot: u64) -> Option<String> {
        let (id, seq, tot, payload) = parse_chunk(frame)?;
        let entry = self.buffers.entry(id).or_insert_with(|| Partial {
            tot,
            parts: BTreeMap::new(),
            last_slot: slot,
            from: from.to_string(),
        });
        entry.tot = tot;
        entry.last_slot = slot;
        // Re-inserting the same seq overwrites, so a retransmission cannot inflate the count —
        // only DISTINCT chunks complete a set. Order does not matter (BTreeMap by seq), so a
        // set can arrive 2-then-1 and still assemble.
        entry.parts.insert(seq, payload);
        if entry.parts.len() == tot {
            let done = self.buffers.remove(&id).unwrap();
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
        let stale: Vec<char> = self
            .buffers
            .iter()
            .filter(|(_, p)| now_slot.saturating_sub(p.last_slot) > max_age)
            .map(|(k, _)| *k)
            .collect();
        stale
            .into_iter()
            .filter_map(|id| {
                let p = self.buffers.remove(&id)?;
                Some(Incomplete {
                    from: p.from,
                    id,
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
            .map(|(id, p)| {
                (
                    p.last_slot,
                    Incomplete {
                        from: p.from.clone(),
                        id: *id,
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

    // ⚠️ THE REGRESSION THIS TEST EXISTS TO PREVENT — caught before shipping, 2026-07-26.
    //
    // Reassembly was briefly keyed on (sender, id) to stop two stations on the same id merging.
    // A chunk carries NO callsign, so the sender can only come from the talker context — and
    // that context moves on ANY standard frame from ANY station. Chunks arrive ~8 s apart at
    // TempoFast, so on a live band another station transmits in between, the context changes,
    // and the message splits across two buckets and NEVER completes.
    //
    // This test walks that exact sequence: the attributed sender CHANGES mid-message. The
    // message must still assemble. Routing on a drifting context breaks the common case on any
    // active band in order to fix a rare one.
    #[test]
    fn a_message_still_assembles_when_the_talker_context_drifts() {
        let frames = chunk("YOU ARE THE MAN SETH", 'B');
        assert_eq!(frames.len(), 3, "need a multi-chunk message");
        let mut r = Reassembler::new();

        // Chunk 1 while N9UM is the identified talker.
        assert_eq!(r.accept("N9UM", &frames[0], 10), None);
        // A third station transmits in between and becomes the "current" talker.
        assert_eq!(r.accept("W1ABC", &frames[1], 12), None);
        // And another. The message is still N9UM's and must still complete.
        let done = r.accept("K2DEF", &frames[2], 14);
        assert_eq!(done.as_deref(), Some("YOU ARE THE MAN SETH"));
        assert!(r.pending().is_empty());
    }

    // Attribution is bound at the FIRST chunk and never revised, so an incomplete set still
    // names the station that actually started it rather than whoever last transmitted.
    #[test]
    fn attribution_binds_to_the_first_chunk_not_the_latest_frame() {
        let frames = chunk("YOU ARE THE MAN SETH", 'B');
        let mut r = Reassembler::new();
        r.accept("N9UM", &frames[0], 10);
        r.accept("W1ABC", &frames[1], 12); // context drifted; message is still N9UM's
        let aged = r.age_out(100, 30);
        assert_eq!(aged.len(), 1);
        assert_eq!(
            aged[0].from, "N9UM",
            "credited to whoever started the message"
        );
        assert_eq!((aged[0].have, aged[0].tot), (2, 3));
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
