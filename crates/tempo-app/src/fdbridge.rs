//! The bridge between `tempo_net::fdsync`'s wire traits and the [`Engine`] —
//! one impl each of [`ClubBackend`] (host role) and [`PositionSync`]
//! (position role) over the shared engine mutex.
//!
//! Lives HERE rather than in the Tauri shell so the `fieldday_loopback`
//! integration test exercises the REAL bridge the shipped manager thread
//! uses, not a test double of it. Every method takes the engine lock briefly
//! and returns — the pump/accept threads own the sockets, the engine owns the
//! state, and neither ever blocks the other on I/O.
//!
//! Data-plane discipline: these impls can only call the engine's `fd_club_*`
//! / `fd_sync_*` / `fd_mirror_*` seam — rows and club state. Nothing here
//! (and nothing behind the traits) can key TX, touch CAT, or change settings.
//!
//! ⛔ **A JOIN's club key stops here.** The host role hashes it with SHA-256
//! ([`crate::fdevent::sha256_hex`]) and hands the engine only the hash, so the engine and the
//! club log hold no position's key but this station's own.

use crate::engine::{engine_lock, Engine};
use std::sync::{Arc, Mutex};
use tempo_net::fdsync::{
    ClockSample, ClubBackend, ClubListView, ClubRows, ClubState, JoinAccept, PosReport,
    PositionKey, PositionSync, Unreachable, WireQso,
};

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The host role: `fdsync::serve_until`'s backend over the shared engine.
pub struct EngineClubBackend(pub Arc<Mutex<Engine>>);

impl ClubBackend for EngineClubBackend {
    fn join(
        &self,
        v: u32,
        pos: &str,
        name: &str,
        call: &str,
        _max_seq: u64,
        contest: &str,
        role: &str,
        key: &PositionKey,
    ) -> Result<JoinAccept, String> {
        // Hashed before the engine lock is taken, and only a key a Nexus makes: anything else
        // is no key at all (`fdevent::NO_POSITION_KEY`).
        let key_hash = if key.is_club_key() {
            crate::fdevent::sha256_hex(key.secret())
        } else {
            String::new()
        };
        engine_lock(&self.0).fd_club_join(v, pos, name, call, contest, role, &key_hash)
    }

    fn merge(&self, hold: u64, row: &WireQso) -> Result<u64, String> {
        engine_lock(&self.0).fd_club_merge(hold, row)
    }

    fn held(&self, pos: &str, hold: u64) -> Result<(), String> {
        engine_lock(&self.0).fd_club_held(pos, hold)
    }

    fn position_status(&self, pos: &str, report: &PosReport) {
        engine_lock(&self.0).fd_club_pos_status(pos, report);
    }

    fn counts(&self) -> (usize, usize, u64) {
        // One lock, so the cursors and the generation they belong to are read together.
        let e = engine_lock(&self.0);
        let (dupes, sections) = e.fd_club_counts();
        (dupes, sections, e.fd_club_generation())
    }

    fn club_state(&self, dupes_from: usize, sections_from: usize, mark_seen: &str) -> ClubState {
        engine_lock(&self.0).fd_club_state(dupes_from, sections_from, mark_seen)
    }

    fn disconnect(&self, pos: &str, link: u64) {
        engine_lock(&self.0).fd_club_disconnect(pos, link);
    }

    fn retract(&self, hold: u64, pos: &str, seq: u64) -> Result<(), String> {
        engine_lock(&self.0).fd_club_retract(hold, pos, seq)
    }

    fn restore(&self, hold: u64, pos: &str, seq: u64) -> Result<(), String> {
        engine_lock(&self.0).fd_club_restore(hold, pos, seq)
    }

    fn retracted(&self, pos: &str) -> Vec<u64> {
        engine_lock(&self.0).fd_club_retracted(pos)
    }

    fn club_list(&self) -> Arc<ClubListView> {
        engine_lock(&self.0).fd_club_list()
    }
}

/// The position role: `fdsync::run_position_until`'s backend over the shared
/// engine (the host runs one too, pointed at its own loopback listener — a
/// host is just another position).
///
/// ⭐ **Each pump's bridge owns the club link until the next one is made** ([`Self::new`]).
/// The shell's supervisor stops a pump and starts the next for the same address when the
/// contest changes, and the stopped one reports its link down up to a quarter of a second
/// later, after the next may already be in. So every call that writes the mirror goes through
/// [`Self::mirror`], which drops it unless this bridge still owns the link.
pub struct EnginePositionSync {
    engine: Arc<Mutex<Engine>>,
    /// This pump's generation ([`Engine::fd_link_take_over`]).
    gen: u64,
    /// The call the JOIN this pump sent last named ([`PositionSync::identity`]), for the mirror
    /// when the welcome answers it (`ClubMirror::link_call`).
    joining: Mutex<String>,
}

impl EnginePositionSync {
    /// The bridge for a new pump, which takes the club link over from every older one.
    pub fn new(engine: Arc<Mutex<Engine>>) -> Self {
        let gen = engine_lock(&engine).fd_link_take_over();
        Self {
            engine,
            gen,
            joining: Mutex::default(),
        }
    }

    /// Apply `f` to the mirror while this pump owns the club link: a stopped pump's calls
    /// land on nothing.
    fn mirror(&self, f: impl FnOnce(&mut crate::fdevent::ClubMirror)) {
        let mut eng = engine_lock(&self.engine);
        if eng.fd_link_owned_by(self.gen) {
            f(eng.fd_mirror_mut());
        }
    }
}

impl PositionSync for EnginePositionSync {
    fn identity(&self) -> (String, String, String, u64) {
        let identity = engine_lock(&self.engine).fd_sync_identity();
        *self
            .joining
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = identity.2.clone();
        identity
    }

    fn key(&self) -> PositionKey {
        engine_lock(&self.engine).fd_position_key()
    }

    fn contest(&self) -> String {
        engine_lock(&self.engine).fd_position_contest()
    }

    fn role(&self) -> String {
        engine_lock(&self.engine).fd_position_role()
    }

    fn host_contest(&self, contest: &str) -> Result<(), String> {
        engine_lock(&self.engine).fd_accept_host_contest(contest)
    }

    fn outbox_after(&self, after: u64) -> tempo_net::fdsync::Outbox {
        // One lock: a contact taken back or put back between the two reads would otherwise
        // leave one of them wrong for a tick.
        let e = engine_lock(&self.engine);
        tempo_net::fdsync::Outbox {
            marks: e.fd_sync_marks(),
            rows: e.fd_sync_outbox(after),
        }
    }

    fn on_welcome(&self, acked: u64, event: &str, host_call: &str, host_now_unix: u64) {
        // Skew = local − host in whole seconds, computed at the one moment both
        // clocks are in hand: the club line's number until the first timed round
        // trip closes (`on_clock`). Shown, never adjusted (ids are clock-free and
        // FD has no time-window dupe rule).
        let now = now_unix();
        let skew = now as i64 - host_now_unix as i64;
        // The call this link joined under is the one its JOIN named, whatever Settings say now.
        let joined = self
            .joining
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        self.mirror(|m| {
            m.on_welcome(acked, event, host_call, skew);
            m.link_call = joined;
            m.heard(now);
        });
    }

    // Every line from the host in the event is this laptop hearing it (`ClubMirror::heard`): the
    // host sends a club line every heartbeat, so a live link is heard at least that often.
    fn on_ack(&self, seq: u64) {
        let now = now_unix();
        self.mirror(|m| {
            m.acked = m.acked.max(seq);
            m.heard(now);
        });
    }

    fn on_retracted(&self, seq: u64, gone: bool) {
        let now = now_unix();
        self.mirror(|m| {
            m.on_retracted(seq, gone);
            m.heard(now);
        });
    }

    fn on_club(&self, _snap: bool, st: &ClubState) {
        let now = now_unix();
        self.mirror(|m| {
            m.apply(st);
            m.heard(now);
        });
    }

    fn on_rows(&self, rows: &ClubRows) {
        let now = now_unix();
        self.mirror(|m| {
            m.on_rows(rows);
            m.heard(now);
        });
    }

    fn on_error(&self, msg: &str) {
        self.mirror(|m| m.last_error = Some(msg.to_string()));
    }

    fn position_report(&self) -> Option<PosReport> {
        Some(engine_lock(&self.engine).fd_position_report())
    }

    fn on_link(&self, connected: bool) {
        let now = now_unix();
        self.mirror(|m| m.on_link(connected, now));
    }

    fn on_unreachable(&self, why: Unreachable) {
        self.mirror(|m| m.on_unreachable(why));
    }

    fn on_clock(&self, sample: ClockSample) {
        // Into the mirror and nowhere else: what the club measures is shown, never applied
        // (FT's slot clock steers by the SNTP measurement alone).
        let now = now_unix();
        self.mirror(|m| {
            m.on_clock(sample);
            m.heard(now);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fdevent::LinkState;

    /// The link as the club line and the chip read it.
    fn link(engine: &Arc<Mutex<Engine>>) -> (LinkState, bool) {
        let mut eng = engine_lock(engine);
        let m = eng.fd_mirror_mut();
        (m.link_state(), m.connected)
    }

    /// ⭐ **A link is on the call its JOIN named** (`ClubMirror::link_call`), whatever Settings
    /// say by the time the host's welcome comes: it is what a contact that carries no call of its
    /// own was logged on.
    #[test]
    fn a_link_is_on_the_call_its_join_named() {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN61", 0)));
        let pump = EnginePositionSync::new(engine.clone());
        assert_eq!(
            pump.identity().2,
            "W9XYZ",
            "CONTROL: the JOIN names the Settings call"
        );
        {
            let mut eng = engine_lock(&engine);
            let mut s = eng.settings().clone();
            s.mycall = "K9ABC".into();
            eng.apply_settings(s);
        }
        pump.on_link(true);
        pump.on_welcome(0, "ILQP", "W9XYZ", now_unix());
        assert_eq!(engine_lock(&engine).fd_mirror_mut().link_call, "W9XYZ");
    }

    /// ⭐ **A pump the supervisor replaced cannot take the new link down.** When the contest
    /// changes, the shell's supervisor reports the old link down itself and starts the next
    /// pump for the same address; the old pump notices its shutdown up to a quarter of a second
    /// later and reports its own link down. On a LAN the next pump is in by then. Delivered here
    /// in that losing order: the new pump's link and welcome, then the old pump's last calls.
    #[test]
    fn a_replaced_pumps_last_calls_do_not_reach_the_new_link() {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN61", 0)));
        let old = EnginePositionSync::new(engine.clone());
        old.on_link(true);
        old.on_welcome(0, "ILQP rehearsal", "W9AAA", now_unix());
        assert_eq!(link(&engine), (LinkState::Joined, true));
        // The supervisor, as it stops the old pump (`lib.rs`, the position client reconcile).
        {
            let mut eng = engine_lock(&engine);
            eng.fd_link_take_over();
            eng.fd_mirror_mut().on_link(false, now_unix());
        }
        let new = EnginePositionSync::new(engine.clone());
        new.on_link(true);
        new.on_welcome(0, "ILQP rehearsal", "W9AAA", now_unix());
        assert_eq!(link(&engine), (LinkState::Joined, true));
        // Only now does the old pump's session end.
        old.on_link(false);
        assert_eq!(
            link(&engine),
            (LinkState::Joined, true),
            "the old pump's link going down is not the new link's"
        );
        // Nothing else it says lands either.
        old.on_unreachable(Unreachable::NoAnswer);
        old.on_error("the old session's refusal");
        old.on_ack(99);
        old.on_club(
            true,
            &ClubState {
                score: 999,
                ..Default::default()
            },
        );
        old.on_clock(ClockSample {
            t0: 1_000,
            t1: 61_000,
            t2: 61_001,
            t3: 1_002,
        });
        old.on_welcome(7, "Another event", "K1OLD", now_unix());
        old.on_retracted(5, true);
        assert_eq!(link(&engine), (LinkState::Joined, true));
        {
            let mut eng = engine_lock(&engine);
            let m = eng.fd_mirror_mut();
            assert_eq!(m.last_error, None);
            assert_eq!(m.unreachable, None);
            assert_eq!((m.acked, m.score), (0, 0));
            assert!(
                m.retracted.is_empty(),
                "the old host's take-backs are not the new host's: {:?}",
                m.retracted
            );
            assert_eq!(
                m.clock.skew_ms(),
                None,
                "no round trip of the new link's yet"
            );
            assert_eq!(
                (m.event.as_str(), m.host_call.as_str()),
                ("ILQP rehearsal", "W9AAA")
            );
        }
        // CONTROL: the pump that owns the link is heard, its link going down included.
        new.on_ack(3);
        new.on_retracted(3, true);
        new.on_link(false);
        assert_eq!(link(&engine), (LinkState::Lost(None), false));
        assert_eq!(engine_lock(&engine).fd_mirror_mut().acked, 3);
        assert_eq!(
            engine_lock(&engine)
                .fd_mirror_mut()
                .retracted
                .iter()
                .collect::<Vec<_>>(),
            [&3]
        );
    }
}
