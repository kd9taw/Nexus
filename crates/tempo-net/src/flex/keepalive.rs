//! Keepalive: one `ping` a second after `keepalive enable`, and the count of missed replies that
//! ends the session.
//!
//! The radio drops a client that stops pinging for 15 s (D `TCPIP-keepalive`). This side counts
//! missed ping replies: a ping whose reply has not arrived when the next one is due is missed,
//! and any reply to the latest ping clears the count. [`MISS_LIMIT`] misses end the session
//! ([`MISS_LIMIT_POOR_LINK`] on a link the caller has marked poor). One miss is survivable, and
//! the session reports it ([`Tick::Missed`]) so a transmission in progress can be unkeyed at once:
//! a missed ping during an over unkeys locally before anything else (spec §6.3).
//!
//! **Grace while another GUI client connects.** The radio replays its status to a connecting GUI
//! client, and ping replies can stall meanwhile. When the limit is reached within
//! `(limit + 1)` ping intervals of another client's connect, the session is given
//! [`CLIENT_CONNECT_GRACE_MS`] more before it ends.
//!
//! Time is milliseconds on a monotonic clock, passed in. Pure: the session sends the ping this
//! asks for and reports its sequence number back.
//!
//! PORTED from AetherSDR (https://github.com/aethersdr/AetherSDR, GPL-3.0; the upstream file
//! carries no per-file header, the licence is the repository's), the ping bookkeeping in
//! `src/core/backends/flex/RadioConnection.cpp` (the latest ping's sequence number, and its reply
//! recognised by that number) at commit `32fa50e4896a846a6970fa3f443bd49d667c139d` (2026-10-03),
//! translated from C++/Qt to Rust. The miss limits, the poor-link limit and the client-connect
//! grace follow the ping timer in `src/models/RadioModel.cpp` (8258–8311), read as protocol facts;
//! no code is taken from it. Deliberate differences: a miss is a ping whose reply had not arrived
//! when the next was due, counted per ping rather than per timer tick (so five missed replies end
//! the session, as the plan states it); the session is told of each miss; RTT measurement through
//! the kernel's TCP statistics is not ported. Recorded in the repo-root NOTICE (AetherSDR entry).

/// One ping a second (D `TCPIP-keepalive`).
pub const PING_INTERVAL_MS: u64 = 1000;

/// Missed replies that end the session.
pub const MISS_LIMIT: u32 = 5;

/// Missed replies that end the session on a link the caller has marked poor.
pub const MISS_LIMIT_POOR_LINK: u32 = 15;

/// Extra time given when the limit is reached right after another GUI client connected.
pub const CLIENT_CONNECT_GRACE_MS: u64 = 5000;

/// What a tick asks of the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    /// Not due yet, or keepalive not started.
    Idle,
    /// Send a ping. The previous one was answered.
    Ping,
    /// Send a ping. The previous one was not answered: `misses` in a row now.
    Missed { misses: u32 },
    /// End the session: `misses` in a row, past the limit and any grace.
    Lost { misses: u32 },
}

/// The keepalive for one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keepalive {
    interval_ms: u64,
    started: bool,
    next_due_ms: u64,
    /// The latest ping's sequence number and when it went out, until its reply arrives.
    outstanding: Option<(u32, u64)>,
    misses: u32,
    poor_link: bool,
    last_foreign_connect_ms: Option<u64>,
    grace_until_ms: Option<u64>,
    lost: bool,
    last_rtt_ms: Option<u64>,
}

impl Default for Keepalive {
    fn default() -> Self {
        Keepalive::new(PING_INTERVAL_MS)
    }
}

impl Keepalive {
    pub fn new(interval_ms: u64) -> Keepalive {
        Keepalive {
            interval_ms: interval_ms.max(1),
            started: false,
            next_due_ms: 0,
            outstanding: None,
            misses: 0,
            poor_link: false,
            last_foreign_connect_ms: None,
            grace_until_ms: None,
            lost: false,
            last_rtt_ms: None,
        }
    }

    /// `keepalive enable` was sent: the first ping is due now.
    pub fn start(&mut self, now_ms: u64) {
        self.started = true;
        self.next_due_ms = now_ms;
    }

    pub fn misses(&self) -> u32 {
        self.misses
    }

    /// The round trip of the last answered ping.
    pub fn last_rtt_ms(&self) -> Option<u64> {
        self.last_rtt_ms
    }

    /// Mark the link poor (the higher limit) or normal.
    pub fn set_poor_link(&mut self, poor: bool) {
        self.poor_link = poor;
    }

    /// Another GUI client connected: the radio may stall replies while it replays status.
    pub fn note_foreign_client_connected(&mut self, now_ms: u64) {
        self.last_foreign_connect_ms = Some(now_ms);
    }

    fn limit(&self) -> u32 {
        if self.poor_link {
            MISS_LIMIT_POOR_LINK
        } else {
            MISS_LIMIT
        }
    }

    /// Whether a ping is due, and what the last one's fate means. After [`Tick::Ping`] or
    /// [`Tick::Missed`] the session sends a ping and calls [`sent`](Self::sent).
    pub fn tick(&mut self, now_ms: u64) -> Tick {
        if !self.started || self.lost || now_ms < self.next_due_ms {
            return Tick::Idle;
        }
        self.next_due_ms = now_ms + self.interval_ms;
        if self.outstanding.take().is_none() {
            return Tick::Ping;
        }
        self.misses += 1;
        let limit = self.limit();
        if self.misses < limit {
            return Tick::Missed {
                misses: self.misses,
            };
        }
        if self.grace_until_ms.is_some_and(|until| now_ms < until) {
            return Tick::Missed {
                misses: self.misses,
            };
        }
        let window = u64::from(limit + 1) * self.interval_ms;
        let recent_connect = self
            .last_foreign_connect_ms
            .is_some_and(|at| now_ms >= at && now_ms - at <= window);
        if recent_connect {
            self.grace_until_ms = Some(now_ms + CLIENT_CONNECT_GRACE_MS);
            return Tick::Missed {
                misses: self.misses,
            };
        }
        self.lost = true;
        Tick::Lost {
            misses: self.misses,
        }
    }

    /// The ping the tick asked for went out with this sequence number.
    pub fn sent(&mut self, sequence: u32, now_ms: u64) {
        self.outstanding = Some((sequence, now_ms));
    }

    /// A reply arrived. Returns whether it answered the latest ping, which clears the misses.
    pub fn reply(&mut self, sequence: u32, now_ms: u64) -> bool {
        match self.outstanding {
            Some((seq, sent)) if seq == sequence => {
                self.outstanding = None;
                self.misses = 0;
                self.last_rtt_ms = Some(now_ms.saturating_sub(sent));
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `ticks` ping intervals; `answered(n)` says whether ping n (from 1) gets its reply
    /// before the next is due. Returns every tick's verdict.
    fn run(k: &mut Keepalive, ticks: usize, answered: impl Fn(usize) -> bool) -> Vec<Tick> {
        let mut out = Vec::new();
        let mut seq = 100;
        for n in 1..=ticks {
            let now = n as u64 * 1000;
            let t = k.tick(now);
            out.push(t);
            if matches!(t, Tick::Ping | Tick::Missed { .. }) {
                seq += 1;
                k.sent(seq, now);
                if answered(n) {
                    assert!(k.reply(seq, now + 20));
                }
            }
        }
        out
    }

    #[test]
    fn nothing_before_keepalive_starts() {
        let mut k = Keepalive::default();
        assert_eq!(k.tick(10_000), Tick::Idle);
        k.start(10_000);
        assert_eq!(k.tick(10_000), Tick::Ping);
        assert_eq!(k.tick(10_500), Tick::Idle, "one a second");
    }

    #[test]
    fn four_missed_replies_survive_and_the_count_clears() {
        let mut k = Keepalive::default();
        k.start(1000);
        // Pings 2–5 lost: misses reach 4, ping 6 is answered.
        let ticks = run(&mut k, 8, |n| !(2..=5).contains(&n));
        assert_eq!(
            ticks,
            [
                Tick::Ping,
                Tick::Ping,
                Tick::Missed { misses: 1 },
                Tick::Missed { misses: 2 },
                Tick::Missed { misses: 3 },
                Tick::Missed { misses: 4 },
                Tick::Ping,
                Tick::Ping,
            ]
        );
        assert_eq!(k.misses(), 0);
        assert_eq!(k.last_rtt_ms(), Some(20));
    }

    #[test]
    fn five_missed_replies_end_the_session() {
        let mut k = Keepalive::default();
        k.start(1000);
        let ticks = run(&mut k, 8, |n| n == 1);
        assert_eq!(ticks[5], Tick::Missed { misses: 4 });
        assert_eq!(ticks[6], Tick::Lost { misses: 5 });
        assert_eq!(ticks[7], Tick::Idle, "nothing after the end");
    }

    #[test]
    fn a_poor_link_allows_fifteen() {
        let mut k = Keepalive::default();
        k.set_poor_link(true);
        k.start(1000);
        let ticks = run(&mut k, 20, |n| n == 1);
        assert_eq!(ticks[15], Tick::Missed { misses: 14 });
        assert_eq!(ticks[16], Tick::Lost { misses: 15 });
    }

    #[test]
    fn grace_while_another_client_connects() {
        let mut k = Keepalive::default();
        k.start(1000);
        k.note_foreign_client_connected(3000);
        // At the fifth miss (t = 7000) the connect is within (5 + 1) intervals: grace to 12000.
        let ticks = run(&mut k, 14, |n| n == 1);
        assert_eq!(ticks[6], Tick::Missed { misses: 5 }, "grace granted");
        assert_eq!(
            ticks[10],
            Tick::Missed { misses: 9 },
            "still inside the grace"
        );
        assert_eq!(ticks[11], Tick::Lost { misses: 10 }, "the grace ran out");
        // Without a recent connect there is no grace.
        let mut k = Keepalive::default();
        k.start(1000);
        k.note_foreign_client_connected(0);
        let ticks = run(&mut k, 8, |n| n == 1);
        assert_eq!(ticks[6], Tick::Lost { misses: 5 });
    }

    #[test]
    fn only_the_latest_ping_counts() {
        let mut k = Keepalive::default();
        k.start(0);
        assert_eq!(k.tick(0), Tick::Ping);
        k.sent(1, 0);
        assert!(
            !k.reply(7, 5),
            "another command's reply is not a ping reply"
        );
        assert_eq!(k.tick(1000), Tick::Missed { misses: 1 });
        k.sent(2, 1000);
        assert!(
            !k.reply(1, 1100),
            "a late reply to an older ping does not clear"
        );
        assert!(k.reply(2, 1100));
        assert_eq!(k.tick(2000), Tick::Ping);
    }
}
