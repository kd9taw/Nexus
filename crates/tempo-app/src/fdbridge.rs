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

use crate::engine::{engine_lock, Engine};
use std::sync::{Arc, Mutex};
use tempo_net::fdsync::{ClubBackend, ClubState, JoinAccept, PositionSync, WireQso};

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The host role: `fdsync::serve_until`'s backend over the shared engine.
pub struct EngineClubBackend(pub Arc<Mutex<Engine>>);

impl ClubBackend for EngineClubBackend {
    fn join(&self, pos: &str, name: &str, call: &str, _max_seq: u64) -> Result<JoinAccept, String> {
        engine_lock(&self.0).fd_club_join(pos, name, call)
    }

    fn merge(&self, row: &WireQso) -> u64 {
        engine_lock(&self.0).fd_club_merge(row)
    }

    fn position_status(&self, pos: &str, band: &str, mode: &str, op: &str, freq: u64) {
        engine_lock(&self.0).fd_club_pos_status(pos, band, mode, op, freq);
    }

    fn counts(&self) -> (usize, usize) {
        engine_lock(&self.0).fd_club_counts()
    }

    fn club_state(&self, dupes_from: usize, sections_from: usize, mark_seen: &str) -> ClubState {
        engine_lock(&self.0).fd_club_state(dupes_from, sections_from, mark_seen)
    }

    fn disconnect(&self, pos: &str) {
        engine_lock(&self.0).fd_club_disconnect(pos);
    }
}

/// The position role: `fdsync::run_position_until`'s backend over the shared
/// engine (the host runs one too, pointed at its own loopback listener — a
/// host is just another position).
pub struct EnginePositionSync(pub Arc<Mutex<Engine>>);

impl PositionSync for EnginePositionSync {
    fn identity(&self) -> (String, String, String, u64) {
        engine_lock(&self.0).fd_sync_identity()
    }

    fn outbox_after(&self, after: u64) -> Vec<WireQso> {
        engine_lock(&self.0).fd_sync_outbox(after)
    }

    fn on_welcome(&self, acked: u64, event: &str, host_call: &str, host_now_unix: u64) {
        // Skew = local − host, computed at the one moment both clocks are in
        // hand. WARNED about above ±30 s, never adjusted (ids are clock-free
        // and FD has no time-window dupe rule).
        let skew = now_unix() as i64 - host_now_unix as i64;
        engine_lock(&self.0)
            .fd_mirror_mut()
            .on_welcome(acked, event, host_call, skew);
    }

    fn on_ack(&self, seq: u64) {
        let mut eng = engine_lock(&self.0);
        let m = eng.fd_mirror_mut();
        m.acked = m.acked.max(seq);
    }

    fn on_club(&self, _snap: bool, st: &ClubState) {
        engine_lock(&self.0).fd_mirror_mut().apply(st);
    }

    fn on_error(&self, msg: &str) {
        engine_lock(&self.0).fd_mirror_mut().last_error = Some(msg.to_string());
    }

    fn position_report(&self) -> Option<(String, String, String, u64)> {
        Some(engine_lock(&self.0).fd_position_report())
    }

    fn on_link(&self, connected: bool) {
        engine_lock(&self.0)
            .fd_mirror_mut()
            .on_link(connected, now_unix());
    }
}
