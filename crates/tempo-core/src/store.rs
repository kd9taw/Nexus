//! Store-and-forward for off-grid nets.
//!
//! A station (or relay) queues directed messages for callsigns that may not be
//! reachable right now. When the recipient becomes **present** (heard recently
//! in the [`Roster`]), the queued message is released for transmission as a
//! burst of frames: an identifying directed frame (carrying TO+FROM, so the
//! recipient can attribute it) followed by the word-wrapped free-text chunks.
//! Sends back off between attempts and stop once delivery is confirmed.

use crate::message::Msg;
use crate::roster::Roster;
use crate::text;

/// A queued outbound message.
#[derive(Debug, Clone)]
pub struct Pending {
    pub to: String,
    pub text: String,
    pub created_slot: u64,
    pub attempts: u32,
    pub last_attempt_slot: Option<u64>,
    pub delivered: bool,
    /// Chunk message-id char ('A'..'Z') stamped on the outbound bubble; pub so the
    /// engine can persist/restore the queue across restarts (the id must survive, or
    /// a late ACK for a restored message wouldn't match its bubble).
    pub id: char,
    /// Terminal "sent, never acknowledged": the message exhausted its transmit-cycle
    /// budget. Never released again, but kept briefly ACK-matchable so a LATE RR73 can
    /// still flip the bubble to Delivered (see [`Self::take_no_acks`] / `purge_stale`).
    /// NOT journaled — a restart re-derives it from `attempts >= cap`, self-healing.
    pub no_acked: bool,
    /// Implicitly confirmed: after this message went on the air, the peer transmitted a
    /// COMPLETE directed message back to us — they demonstrably hear us, so the resend
    /// schedule stops. Weaker than `delivered` (no id-bearing ACK; the bubble shows
    /// "confirmed", never "Delivered ✓") and still ACK-matchable for the real RR73.
    /// NOT journaled (re-resolves after restart via resend → their next reply).
    pub confirmed: bool,
}

/// One releasable message from [`StoreForward::due`]: the burst to transmit now plus
/// the bookkeeping the engine stamps onto the conversation bubble.
#[derive(Debug)]
pub struct Release {
    pub to: String,
    /// `Some` ONLY on the message's FIRST release (drives the one own-TX band-activity row).
    pub body: Option<String>,
    /// The on-air burst: `[identify, chunk…]`.
    pub frames: Vec<String>,
    /// Chunk message-id — the bubble's exact-match key.
    pub id: char,
    /// Cycle count AFTER this release ("sending k/N").
    pub attempts: u32,
}

/// A presence-gated store-and-forward queue.
#[derive(Debug)]
pub struct StoreForward {
    mycall: String,
    mygrid: String,
    queue: Vec<Pending>,
    next_id: u8, // cycles 'A'..'Z' for chunk message-ids
}

impl StoreForward {
    pub fn new(mycall: &str, mygrid: &str) -> Self {
        Self {
            mycall: mycall.to_string(),
            mygrid: mygrid.to_string(),
            queue: Vec::new(),
            next_id: 0,
        }
    }

    /// Rebind the operator identity used to stamp the `DE <call>` / grid prefix on
    /// released frames, WITHOUT dropping the pending queue (keyed by recipient). For
    /// an in-place callsign/grid change in Settings (see `AppState::set_identity`).
    pub fn set_identity(&mut self, mycall: &str, mygrid: &str) {
        self.mycall = mycall.to_string();
        self.mygrid = mygrid.to_string();
    }

    /// Queue a directed message for later delivery. Returns the chunk-id char assigned to
    /// it — the caller stamps the outbound conversation bubble with it so an id-bearing ACK
    /// confirms exactly this message.
    pub fn queue(&mut self, to: &str, text: &str, slot: u64) -> char {
        let id = (b'A' + self.next_id) as char;
        self.next_id = (self.next_id + 1) % 26;
        self.queue.push(Pending {
            to: to.to_string(),
            text: text.to_string(),
            created_slot: slot,
            attempts: 0,
            last_attempt_slot: None,
            delivered: false,
            id,
            no_acked: false,
            confirmed: false,
        });
        id
    }

    /// Number of messages still awaiting delivery.
    pub fn pending(&self) -> usize {
        self.queue.iter().filter(|p| !p.delivered).count()
    }

    /// Frames to transmit *now*: for each undelivered message whose recipient is
    /// active (heard within `window` slots), out of `backoff`, and still inside its
    /// `max_attempts` transmit-cycle budget, build the on-air burst `[identify, chunk…]`
    /// and record the attempt. Returns `(recipient, body, frames)` per releasable
    /// message — `body` is `Some` ONLY on the message's FIRST release, so the engine can
    /// record one own-TX band-activity row when the message actually goes on the air
    /// (not at compose time, and not once per resend).
    ///
    /// Cadence (2026-07 rework): `max_attempts` is the bounded-ARQ cycle cap — the fix
    /// for "it keeps sending and sending". The attempt is stamped at the slot the burst
    /// ENDS (release slot + 2·(frames−1); the engine drains one frame per own-parity
    /// slot, i.e. every second slot), so `backoff` is a real LISTENING gap after the
    /// last frame — measured from the release slot, a long burst outlived the backoff
    /// and one message transmitted continuously every other slot for minutes.
    pub fn due(
        &mut self,
        roster: &Roster,
        slot: u64,
        window: u64,
        backoff: u64,
        max_attempts: u32,
    ) -> Vec<Release> {
        let mut out = Vec::new();
        for p in self
            .queue
            .iter_mut()
            .filter(|p| !p.delivered && !p.no_acked && !p.confirmed && p.attempts < max_attempts)
        {
            if !roster.is_active(&p.to, slot, window) {
                continue;
            }
            if let Some(last) = p.last_attempt_slot {
                if slot.saturating_sub(last) < backoff {
                    continue;
                }
            }
            let mut frames = vec![Msg::Grid {
                to: p.to.clone(),
                de: self.mycall.clone(),
                grid: self.mygrid.clone(),
            }
            .to_text()];
            frames.extend(text::chunk(&p.text, p.id));
            p.attempts += 1;
            p.last_attempt_slot = Some(slot + (frames.len() as u64 - 1) * 2);
            let body = if p.attempts == 1 {
                Some(p.text.clone())
            } else {
                None
            };
            out.push(Release {
                to: p.to.clone(),
                body,
                frames,
                id: p.id,
                attempts: p.attempts,
            });
        }
        out
    }

    /// Collect messages that have just EXHAUSTED their cycle budget (sent `max_attempts`
    /// times, never acknowledged) — flags each once and returns `(recipient, id)` so the
    /// engine can stamp the conversation bubble "no-ack". The entry itself stays in the
    /// queue, still ACK-matchable, so a LATE RR73 can flip it to delivered (the honest
    /// FIFO answer — it is that peer's oldest undelivered message); `purge_stale` bounds
    /// how long that grace lasts.
    pub fn take_no_acks(&mut self, max_attempts: u32) -> Vec<(String, char)> {
        let mut out = Vec::new();
        for p in self
            .queue
            .iter_mut()
            .filter(|p| !p.delivered && !p.no_acked && !p.confirmed && p.attempts >= max_attempts)
        {
            p.no_acked = true;
            out.push((p.to.clone(), p.id));
        }
        out
    }

    /// Implicit ACK: the peer just transmitted a COMPLETE directed message to us, so they
    /// demonstrably hear us — stop resending every in-flight (released, unresolved) message
    /// to them. Returns the chunk-ids confirmed so the engine can stamp the bubbles
    /// "confirmed" (NOT Delivered — only the id-bearing RR73 earns that). Held (never
    /// released) messages are untouched: nothing was heard, nothing to confirm.
    pub fn confirm_in_flight(&mut self, to: &str) -> Vec<char> {
        let mut out = Vec::new();
        for p in self.queue.iter_mut().filter(|p| {
            !p.delivered && !p.no_acked && !p.confirmed && p.attempts > 0 && p.to == to
        }) {
            p.confirmed = true;
            out.push(p.id);
        }
        out
    }

    /// Drop no-acked entries whose last transmission is older than `horizon` slots —
    /// bounds the late-ACK grace so terminal messages can't accumulate as FIFO zombies
    /// that would eat an ACK meant for a newer message. Returns how many were dropped.
    pub fn purge_stale(&mut self, slot: u64, horizon: u64) -> usize {
        let before = self.queue.len();
        self.queue.retain(|p| {
            !((p.no_acked || p.confirmed)
                && p.last_attempt_slot
                    .is_some_and(|last| slot.saturating_sub(last) > horizon))
        });
        before - self.queue.len()
    }

    /// Mark all messages for `to` delivered (e.g. on receiving an ack/roger).
    pub fn mark_delivered(&mut self, to: &str) {
        for p in self.queue.iter_mut().filter(|p| p.to == to) {
            p.delivered = true;
        }
    }

    /// Mark the OLDEST still-undelivered message for `to` delivered, returning whether one
    /// was marked. An RR73 ACK carries no message id, so each received ACK clears exactly
    /// ONE message FIFO — never the whole peer queue (which would silently drop a
    /// still-in-flight later message and falsely show it "delivered").
    pub fn mark_one_delivered(&mut self, to: &str) -> bool {
        if let Some(p) = self.queue.iter_mut().find(|p| !p.delivered && p.to == to) {
            p.delivered = true;
            true
        } else {
            false
        }
    }

    /// Drop delivered or over-attempted messages; returns how many were purged.
    pub fn purge(&mut self, max_attempts: u32) -> usize {
        let before = self.queue.len();
        self.queue
            .retain(|p| !p.delivered && p.attempts < max_attempts);
        before - self.queue.len()
    }

    /// Drop every queued message for `to`, delivered or not; returns how many were dropped.
    /// The operator deleted the conversation, so nothing further for that peer may go on the
    /// air. Without this, deleting a thread leaves its messages transmitting for up to
    /// `MAX_SEND_ATTEMPTS` releases — and a message to a never-heard peer stays at
    /// `attempts == 0`, which `purge` never collects, so it would queue indefinitely.
    pub fn drop_for(&mut self, to: &str) -> usize {
        let before = self.queue.len();
        self.queue.retain(|p| p.to != to);
        before - self.queue.len()
    }

    /// The undelivered queue, cloned — the engine journals this to disk so queued
    /// messages survive a restart (they used to die with the process and the bubbles
    /// were marked "abandoned"). Delivered entries are history, not work — excluded.
    pub fn export(&self) -> Vec<Pending> {
        self.queue
            .iter()
            .filter(|p| !p.delivered)
            .cloned()
            .collect()
    }

    /// Restore a journaled queue at startup. Replaces the (empty) queue; `next_id`
    /// advances past the highest restored id so new messages can't collide with a
    /// restored message's chunk-id while its ACK may still arrive.
    pub fn restore(&mut self, items: Vec<Pending>) {
        if items.is_empty() {
            return;
        }
        let max_id = items.iter().map(|p| p.id as u8).max().unwrap_or(b'A');
        self.next_id = (max_id.saturating_sub(b'A') + 1) % 26;
        self.queue = items;
    }

    /// Is this exact message still queued (undelivered) for `to`? Drives the restart
    /// restore: a conversation bubble stays "held" only when its message really is
    /// still in the queue; anything else is marked abandoned.
    pub fn has_pending(&self, to: &str, text: &str) -> bool {
        self.queue
            .iter()
            .any(|p| !p.delivered && p.to == to && p.text == text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use modes::Decode;

    fn dec(msg: &str) -> Decode {
        Decode {
            message: msg.to_string(),
            sync: 1.0,
            snr: -7,
            dt: 0.0,
            freq: 1500.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            mode: None,
        }
    }

    #[test]
    fn holds_until_recipient_present_then_releases() {
        let mut sf = StoreForward::new("W9XYZ", "EN37");
        sf.queue("N0XYZ", "QSY TO 40M AT 0200Z PSE", 0);
        assert_eq!(sf.pending(), 1);

        let mut roster = Roster::new();
        // Recipient not heard yet → nothing to send.
        assert!(sf.due(&roster, 1, 10, 3, 8).is_empty());

        // Recipient appears on the air.
        roster.observe(&dec("CQ N0XYZ EN52"), 5);
        let due = sf.due(&roster, 6, 10, 3, 8);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].to, "N0XYZ");
        assert_eq!(
            due[0].body.as_deref(),
            Some("QSY TO 40M AT 0200Z PSE"),
            "body on first release"
        );
        // identify frame + at least one chunk.
        let burst_len = due[0].frames.len() as u64;
        assert!(burst_len >= 2);
        assert!(due[0].frames[0].contains("N0XYZ") && due[0].frames[0].contains("W9XYZ"));

        // Backoff runs from the burst's END (release slot + 2·(frames−1); one frame per
        // own-parity slot) so there's a real LISTENING gap after the last frame — from the
        // release slot, a long burst outlived the backoff and re-released immediately.
        let burst_end = 6 + (burst_len - 1) * 2;
        assert!(
            sf.due(&roster, burst_end + 2, 60, 3, 8).is_empty(),
            "within backoff (measured from burst end) → not resent"
        );
        roster.observe(&dec("CQ N0XYZ EN52"), burst_end + 2); // keep the peer present
        let resend = sf.due(&roster, burst_end + 3, 60, 3, 8);
        assert_eq!(resend.len(), 1, "past backoff → resent");
        assert_eq!(resend[0].body, None, "no body on a resend");

        // Delivered → no longer due, pending drops.
        sf.mark_delivered("N0XYZ");
        assert_eq!(sf.pending(), 0);
        assert!(sf.due(&roster, burst_end + 20, 60, 3, 8).is_empty());
    }

    #[test]
    fn cycle_cap_goes_terminal_no_ack_with_late_ack_grace() {
        // The bounded-ARQ core: a message stops after `max_attempts` cycles, is flagged
        // no-ack exactly once, stays ACK-matchable for a late RR73, and purge_stale
        // bounds that grace.
        let mut sf = StoreForward::new("W9XYZ", "EN37");
        sf.queue("N0XYZ", "HELLO", 0);
        let mut roster = Roster::new();
        let mut slot = 0u64;
        let mut releases = 0;
        // Drive far more slots than the cap allows — only 3 releases may happen.
        for _ in 0..40 {
            roster.observe(&dec("CQ N0XYZ EN52"), slot); // peer stays present
            releases += sf.due(&roster, slot, 60, 2, 3).len();
            slot += 1;
        }
        assert_eq!(releases, 3, "the cycle cap bounds transmissions");
        let no_acks = sf.take_no_acks(3);
        assert_eq!(no_acks.len(), 1, "flagged terminal once");
        assert_eq!(no_acks[0].0, "N0XYZ");
        assert!(sf.take_no_acks(3).is_empty(), "flagging is one-shot");
        // Late ACK inside the grace window still lands (the honest FIFO answer).
        assert!(sf.mark_one_delivered("N0XYZ"), "late RR73 still matches");
        // A second capped-out message that never gets acked is dropped once stale.
        sf.queue("N0XYZ", "AGAIN", slot);
        for _ in 0..30 {
            roster.observe(&dec("CQ N0XYZ EN52"), slot);
            sf.due(&roster, slot, 60, 2, 3);
            slot += 1;
        }
        sf.take_no_acks(3);
        // Drops BOTH stale no-acked entries: the unacknowledged second message AND the
        // late-delivered first one (delivered entries are history — regular purge drops
        // them too; staying flagged no_acked doesn't shield them).
        assert_eq!(sf.purge_stale(slot + 1000, 150), 2, "stale no-acks dropped");
        assert!(!sf.mark_one_delivered("N0XYZ"), "nothing left to match");
    }

    #[test]
    fn presence_window_expires() {
        let mut sf = StoreForward::new("W9XYZ", "EN37");
        sf.queue("N0XYZ", "TEST", 0);
        let mut roster = Roster::new();
        roster.observe(&dec("CQ N0XYZ EN52"), 5);
        // Heard at slot 5; at slot 100 with window 10 it is stale → not due.
        assert!(sf.due(&roster, 100, 10, 3, 8).is_empty());
        // Within window → due.
        assert_eq!(sf.due(&roster, 12, 10, 3, 8).len(), 1);
    }

    #[test]
    fn one_ack_clears_only_the_oldest_message() {
        // Regression: an RR73 ACK has no message id, so it must clear exactly ONE queued
        // message FIFO — never the whole peer queue (which silently dropped a later
        // still-in-flight message and falsely marked it delivered).
        let mut sf = StoreForward::new("W9XYZ", "EN37");
        sf.queue("N0XYZ", "FIRST", 0);
        sf.queue("N0XYZ", "SECOND", 1);
        assert_eq!(sf.pending(), 2);
        assert!(sf.mark_one_delivered("N0XYZ"));
        assert_eq!(sf.pending(), 1, "one ACK clears exactly one message");
        assert!(sf.mark_one_delivered("N0XYZ"));
        assert_eq!(sf.pending(), 0);
        assert!(!sf.mark_one_delivered("N0XYZ"), "nothing left to clear");
    }

    #[test]
    fn purge_drops_delivered_and_exhausted() {
        let mut sf = StoreForward::new("W9XYZ", "EN37");
        sf.queue("N0XYZ", "ONE", 0);
        sf.queue("K2DEF", "TWO", 0);
        sf.mark_delivered("N0XYZ");
        assert_eq!(sf.purge(5), 1);
        assert_eq!(sf.pending(), 1);
    }

    #[test]
    fn drop_for_cancels_one_peers_queue_including_what_purge_would_never_collect() {
        let mut sf = StoreForward::new("W9XYZ", "EN37");
        sf.queue("N0XYZ", "ONE", 0);
        sf.queue("N0XYZ", "TWO", 0);
        sf.queue("K2DEF", "THREE", 0);

        // Never released (peer unheard) → attempts stays 0, which `purge` NEVER collects:
        // without drop_for these would sit queued forever.
        assert_eq!(
            sf.purge(8),
            0,
            "purge cannot reach a never-released message"
        );

        assert_eq!(
            sf.drop_for("N0XYZ"),
            2,
            "both of that peer's messages dropped"
        );
        assert_eq!(sf.pending(), 1, "the other peer is untouched");
        assert_eq!(sf.drop_for("N0XYZ"), 0, "idempotent");
    }
}
