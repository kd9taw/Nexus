//! Which human DX-cluster nodes to keep connected when the operator lets Nexus choose.
//!
//! Pure: no sockets and no clock of its own — every call takes `now` in Unix seconds — so each
//! rule below is tested with a fake clock. The feeds say what every attempt came to
//! ([`Outcome`]); [`NodePool::record`] turns that into a standing per node, and
//! [`NodePool::pick`] names the nodes to keep connected.
//!
//! The rules are the operator's (2026-09-17):
//! - keep [`CONNECTED_AT_ONCE`] nodes connected: today's working count, and the network copies
//!   every spot to every node, so a third node adds load and no spots;
//! - skip a node that failed EVERY attempt for [`SKIP_AFTER_SECS`], at least
//!   [`SKIP_AFTER_TRIES`] tries, while another feed was up — for [`SKIP_FOR_SECS`]. A working
//!   session clears the count. Long enough to ride out a node's reboot, short enough to look
//!   again once a day;
//! - prefer a node that worked in the last [`RECENT_OK_SECS`], then one never tried, then one
//!   whose skip ran out; ties go by a hash of the operator's callsign, so every install does not
//!   pile onto the same two nodes, and a dead node's users scatter instead of moving as one;
//! - at most one DXSpider node at a time, unless nothing else is eligible — the DXSpider
//!   exposure a default install already had;
//! - never fewer than two while two exist, however many are skipped.
//!
//! **Silence is never evidence.** A logged-in session on a dead band reports nothing, so it can
//! neither fail nor be skipped; see [`Outcome`].
//!
//! This decides nothing about a list the operator wrote: that list is connected as written, and
//! the pool only supplies each node's standing for Settings to show.

use super::Outcome;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The node software a node names — or, where its greeting names none, the software its public
/// listing gives and its prompt matches. It changes one decision only: the DXSpider rule in
/// [`NodePool::pick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Software {
    DxSpider,
    CcCluster,
}

/// A human-run DX-cluster node built into this release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    /// `host:port`, exactly as it is dialled.
    pub host: &'static str,
    /// The node's own callsign — what an operator knows it by.
    pub callsign: &'static str,
    pub software: Software,
}

/// The nodes Nexus picks from, built into each release (operator decision: no hosted list, so no
/// new server and no new way to fail).
///
/// Every entry is a human DX-cluster port — no RBN and no skimmer port, since RBN is wired
/// separately — and every entry was checked the one way worth believing: **its greeting read
/// back**. One connection per node on 2026-09-17 between 16:04 and 16:07 UTC, banner read for up
/// to 10 s, **nothing sent** (no callsign, no login, no telnet reply), and kept only if the
/// greeting ended in a login prompt that Nexus's own prompt reader recognises. A TCP connect
/// alone proves nothing — ve7cc.net:23 accepts one and never says a word.
///
/// | Node | Software (evidence) | Greeting |
/// |---|---|---|
/// | `dxspots.com:7300` AE5E | CC Cluster (banner: version 3.397) | 931 bytes, first at 0.13 s, `login:` |
/// | `dxc.ai9t.com:7300` AI9T | CC Cluster (banner: 3.397; its public listing says AR-Cluster, the banner wins) | 126 bytes, 0.11 s, `login:` |
/// | `cluster.n2wq.com:7373` N2WQ | CC Cluster (banner: 3.397) | 99 bytes, 0.09 s, `login:` |
/// | `nd4x.com:7373` ND4X | CC Cluster (banner: 3.397) | 184 bytes, 0.14 s, `login:` |
/// | `dx.w1nr.net:23` W1NR | DXSpider (banner: "DXSpider Node W1NR") | 167 bytes, 0.05 s, `login:` |
/// | `dxc.w4mya.us:7373` W4MYA | CC Cluster (banner: 3.397; skimmer spots only on request) | 232 bytes, 0.17 s, `login:` |
/// | `dx.svs.com:7300` W9AEK | DXSpider (banner names none; DXSpider's stock notice and bare `login:`, listed as DX-Spider) | 85 bytes, 0.04 s, `login:` |
/// | `dxc.wb3ffv.us:7300` WB3FFV | DXSpider (banner names none; bare `login:`, listed as DX-Spider) | 385 bytes, 0.10 s, `login:` |
///
/// Checked the same way and left out:
/// - `ve7cc.net:23` (VE7CC-1) — accepted the connection, 0 bytes in 10 s: mute, as it was twice
///   earlier the same day.
/// - `dxc.wa9pie.net:8000` (WA9PIE-2) — refused on IPv4, no IPv6 route from the checking host.
/// - `w6cua.no-ip.org:7300` — refused. `k0wl.ddns.net:7373` — timed out.
/// - `k1ttt.net:7373` (AR-Cluster) — greets, but its `Please enter your call:` line ENDS IN A
///   NEWLINE and nothing follows, and Nexus answers only a prompt left waiting at the end of
///   the input: it would never log in.
/// - `dxc.n4zkf.com:7373` (CC Cluster) — greets, but its banner says RBN CW spots are on until
///   `SET/NOCW`: a skimmer feed Nexus already takes from RBN.
/// - `k4zr.no-ip.org:7300`, `n7od.pentux.net:7300` (DXSpider) — greet; left out to keep DXSpider
///   nodes a minority. The one-DXSpider rule sends every pair to a non-DXSpider node, so a list
///   heavy in DXSpider loads the others past twice their share
///   (`the_shipped_list_spreads_load_within_twice_a_fair_share`).
///
/// Candidates came from the public telnet directory at ng3k.com (updated 2026-07-31).
/// Reachability is a fact about that day, not a promise — which is why the pool checks every node
/// it uses, every time.
pub const NODES: [Node; 8] = [
    Node {
        host: "dxspots.com:7300",
        callsign: "AE5E",
        software: Software::CcCluster,
    },
    Node {
        host: "dxc.ai9t.com:7300",
        callsign: "AI9T",
        software: Software::CcCluster,
    },
    Node {
        host: "cluster.n2wq.com:7373",
        callsign: "N2WQ",
        software: Software::CcCluster,
    },
    Node {
        host: "nd4x.com:7373",
        callsign: "ND4X",
        software: Software::CcCluster,
    },
    Node {
        host: "dx.w1nr.net:23",
        callsign: "W1NR",
        software: Software::DxSpider,
    },
    Node {
        host: "dxc.w4mya.us:7373",
        callsign: "W4MYA",
        software: Software::CcCluster,
    },
    Node {
        host: "dx.svs.com:7300",
        callsign: "W9AEK",
        software: Software::DxSpider,
    },
    Node {
        host: "dxc.wb3ffv.us:7300",
        callsign: "WB3FFV",
        software: Software::DxSpider,
    },
];

/// Nodes kept connected at once (operator decision).
pub const CONNECTED_AT_ONCE: usize = 2;
/// How long a node must fail every attempt before it is skipped (operator decision).
pub const SKIP_AFTER_SECS: u64 = 5 * 60;
/// …and in at least this many attempts: a node backing off to ten minutes between tries reaches
/// five minutes on its second failure, and two failures are not a verdict.
pub const SKIP_AFTER_TRIES: u32 = 3;
/// How long a skip lasts (operator decision).
pub const SKIP_FOR_SECS: u64 = 24 * 60 * 60;
/// How long a working session keeps a node ahead of nodes never tried.
pub const RECENT_OK_SECS: u64 = 7 * 24 * 60 * 60;

/// Why a node is not working — a CODE, never the node's or the OS's own words, because it is
/// written to disk (`cluster-nodes.json`) and shown in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Failure {
    Unreachable,
    NoGreeting,
    NoPrompt,
    DroppedAfterLogin,
}

impl Failure {
    /// The failure `outcome` is, or `None` for a working session.
    pub fn of(outcome: &Outcome) -> Option<Self> {
        match outcome {
            Outcome::Unreachable(_) => Some(Failure::Unreachable),
            Outcome::NoGreeting => Some(Failure::NoGreeting),
            Outcome::NoPrompt => Some(Failure::NoPrompt),
            Outcome::DroppedAfterLogin => Some(Failure::DroppedAfterLogin),
            Outcome::LoggedIn => None,
        }
    }
}

/// What the pool knows about one node. Only codes and times — see [`Failure`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NodeHealth {
    /// When the node last proved a working session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_ok_unix: Option<u64>,
    /// Its latest failure, if it has failed since it last worked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    /// When it was last skipped. Kept after the skip runs out, until the node works again: that
    /// is what ranks it behind nodes never tried.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_unix: Option<u64>,
    /// The run of failures the skip rule measures. **Not written down**: failures on either side
    /// of a closed app are not "five minutes of failures", and resuming the count after a restart
    /// could skip a node on its first try of the day.
    #[serde(skip)]
    streak: Option<Streak>,
}

/// A run of consecutive failed attempts, each made while another feed was up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Streak {
    since_unix: u64,
    tries: u32,
}

/// A skip that has just begun — what the connection log reports when the pick moves on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Skip {
    /// How the node failed on its last try.
    pub failure: Failure,
    /// How long it had been failing.
    pub failing_secs: u64,
}

/// Every node's standing, keyed by host. Nodes this pool never heard of have none, which is the
/// standing of a node never tried.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodePool {
    nodes: BTreeMap<String, NodeHealth>,
}

/// The file shape: an object, not a bare map, so a later build can add beside `nodes` without a
/// file this build wrote reading as corrupt.
#[derive(Serialize, Deserialize)]
struct Persisted {
    nodes: BTreeMap<String, NodeHealth>,
}

/// A host as the pool files it: trimmed and lowercased, so `DX.W1NR.NET:23` in an operator's list
/// and `dx.w1nr.net:23` in [`NODES`] are one node.
fn key(host: &str) -> String {
    host.trim().to_ascii_lowercase()
}

/// A stable per-operator ordering of nodes: 64-bit FNV-1a over the callsign, then the host.
/// Written out rather than taken from std, whose hasher promises stability across neither releases
/// nor platforms — and an operator's nodes must not move because Nexus was rebuilt.
///
/// Measured rather than assumed (`a_callsign_always_gets_the_same_pair_and_the_load_spreads`,
/// 1000 synthetic calls that differ only in their last characters): the busiest of eight nodes gets
/// 1.17× its fair share, and 1.16× on the shipped list. The host bytes coming after the callsign
/// are what spread neighbouring calls.
fn rank(callsign: &str, host: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in callsign.bytes().chain([b'\n']).chain(host.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl NodePool {
    /// Record what an attempt at `host` came to, and return the skip it started, if any.
    ///
    /// `other_feed_live` is the offline guard: whether another feed (RBN, or another node) was
    /// logged in AND delivering when this failure arrived. A failure without that says nothing
    /// about the node — it may be our network, a sleeping laptop or a captive portal — so it is
    /// not counted, and it breaks the run: the rule is failures on EVERY attempt.
    pub fn record(
        &mut self,
        host: &str,
        outcome: &Outcome,
        other_feed_live: bool,
        now: u64,
    ) -> Option<Skip> {
        let h = self.nodes.entry(key(host)).or_default();
        let Some(failure) = Failure::of(outcome) else {
            // A working session: the node's slate is clean.
            *h = NodeHealth {
                last_ok_unix: Some(now),
                ..NodeHealth::default()
            };
            return None;
        };
        h.failure = Some(failure);
        if !other_feed_live {
            h.streak = None;
            return None;
        }
        if h.skipped_unix.is_some_and(|at| now < at + SKIP_FOR_SECS) {
            // Already skipped. Only a node the operator listed is still being tried; its skip
            // runs its course rather than being extended by every failure.
            return None;
        }
        let streak = h.streak.get_or_insert(Streak {
            since_unix: now,
            tries: 0,
        });
        streak.tries += 1;
        let failing_secs = now.saturating_sub(streak.since_unix);
        if streak.tries < SKIP_AFTER_TRIES || failing_secs < SKIP_AFTER_SECS {
            return None;
        }
        h.skipped_unix = Some(now);
        h.streak = None;
        Some(Skip {
            failure,
            failing_secs,
        })
    }

    /// The nodes to keep connected, best first: at most [`CONNECTED_AT_ONCE`] of `nodes`.
    ///
    /// `callsign` is the operator's own — the bare call, without the cluster SSID, so choosing an
    /// SSID does not move them to other nodes. `connected` says whether a node is logged in right
    /// now, which counts as working however long ago its last proof was written down.
    ///
    /// A pure function of the pool, the callsign and the clock: the same inputs always give the
    /// same pick, in any order of `nodes`.
    pub fn pick<'n>(
        &self,
        nodes: &'n [Node],
        callsign: &str,
        connected: impl Fn(&str) -> bool,
        now: u64,
    ) -> Vec<&'n Node> {
        let callsign = callsign.trim().to_ascii_uppercase();
        let mut ranked: Vec<(Option<u8>, u64, &Node)> = nodes
            .iter()
            .map(|n| {
                (
                    self.tier(n.host, connected(n.host), now),
                    rank(&callsign, &key(n.host)),
                    n,
                )
            })
            .collect();
        // Eligible nodes by tier, then skipped ones; the operator's hash inside each.
        ranked.sort_by_key(|(tier, order, _)| (tier.unwrap_or(u8::MAX), *order));
        let mut picked: Vec<&Node> = Vec::with_capacity(CONNECTED_AT_ONCE);
        // Eligible nodes first, one DXSpider at most; then a second DXSpider rather than fewer
        // nodes; then the same two passes over skipped nodes — never fewer than two while two exist.
        for (skipped, one_spider) in [(false, true), (false, false), (true, true), (true, false)] {
            for (tier, _, node) in &ranked {
                if picked.len() == CONNECTED_AT_ONCE {
                    return picked;
                }
                let spider_taken = node.software == Software::DxSpider
                    && picked.iter().any(|p| p.software == Software::DxSpider);
                if tier.is_none() == skipped
                    && !(one_spider && spider_taken)
                    && !picked.iter().any(|p| key(p.host) == key(node.host))
                {
                    picked.push(node);
                }
            }
        }
        picked
    }

    /// Where `host` stands: `None` while it is skipped, else its tier, lower first — worked
    /// recently (or is connected right now), then never tried or no recent verdict, then skipped
    /// before and not working since.
    fn tier(&self, host: &str, connected: bool, now: u64) -> Option<u8> {
        let h = self.nodes.get(&key(host));
        if let Some(at) = h.and_then(|h| h.skipped_unix) {
            return (now >= at + SKIP_FOR_SECS).then_some(2);
        }
        let recent_ok = h
            .and_then(|h| h.last_ok_unix)
            .is_some_and(|at| now.saturating_sub(at) <= RECENT_OK_SECS);
        Some(if connected || recent_ok { 0 } else { 1 })
    }

    /// What the pool knows about `host`.
    pub fn health(&self, host: &str) -> Option<&NodeHealth> {
        self.nodes.get(&key(host))
    }

    /// When `host`'s skip ends, while it is skipped.
    pub fn skipped_until(&self, host: &str, now: u64) -> Option<u64> {
        let until = self.health(host)?.skipped_unix? + SKIP_FOR_SECS;
        (now < until).then_some(until)
    }

    /// Forget every node `keep` does not want — so the file does not grow with every node an
    /// operator ever typed.
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.nodes.retain(|host, _| keep(host));
    }

    /// The pool as `cluster-nodes.json` holds it. Deterministic (sorted by host), so a caller can
    /// write only when the text changed.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&Persisted {
            nodes: self.nodes.clone(),
        })
        .unwrap_or_default()
    }

    /// A pool read back from [`to_json`](Self::to_json)'s text. Anything unreadable is no health at
    /// all — never a startup failure over a status file; the worst case is every node treated as
    /// untried, which is where a fresh install starts.
    pub fn from_json(text: &str) -> Self {
        Self {
            nodes: serde_json::from_str::<Persisted>(text)
                .map(|p| p.nodes)
                .unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60;
    const HOUR: u64 = 60 * MIN;
    /// An arbitrary Unix time to start the fake clock from.
    const T0: u64 = 1_758_000_000;

    fn cc(host: &'static str) -> Node {
        Node {
            host,
            callsign: "W9XYZ",
            software: Software::CcCluster,
        }
    }
    fn spider(host: &'static str) -> Node {
        Node {
            host,
            callsign: "W9XYZ",
            software: Software::DxSpider,
        }
    }

    const EIGHT: [&str; 8] = [
        "a.example.net:7300",
        "b.example.net:7300",
        "c.example.net:7373",
        "d.example.net:23",
        "e.example.net:8000",
        "f.example.net:7300",
        "g.example.net:7373",
        "h.example.net:23",
    ];

    fn hosts(picked: &[&Node]) -> Vec<&'static str> {
        picked.iter().map(|n| n.host).collect()
    }

    /// Nobody connected — the state at launch, and the one every ordering rule has to decide on
    /// its own.
    fn none(_: &str) -> bool {
        false
    }

    /// Reports `outcome` for `host` every `every` seconds from `from` up to and including `to`.
    fn fail_between(
        pool: &mut NodePool,
        host: &str,
        outcome: Outcome,
        other_feed_live: bool,
        (from, to, every): (u64, u64, u64),
    ) -> Option<Skip> {
        let mut skip = None;
        let mut t = from;
        while t <= to {
            skip = skip.or(pool.record(host, &outcome, other_feed_live, t));
            t += every;
        }
        skip
    }

    /// T3.
    #[test]
    fn a_node_that_fails_for_five_minutes_while_another_feed_is_up_is_skipped_and_replaced() {
        let nodes = EIGHT.map(cc);
        let mut pool = NodePool::default();
        let before = pool.pick(&nodes, "W1AW", none, T0);
        let dead = before[0].host;
        // A mute node: one failure a minute, the other feeds logged in and delivering.
        let skip = fail_between(
            &mut pool,
            dead,
            Outcome::NoGreeting,
            true,
            (T0, T0 + 5 * MIN, MIN),
        );
        assert_eq!(
            skip.map(|s| s.failure),
            Some(Failure::NoGreeting),
            "five minutes and six failed tries, with another feed up, is a skip"
        );
        let after = pool.pick(&nodes, "W1AW", none, T0 + 5 * MIN);
        assert!(!hosts(&after).contains(&dead), "{after:?}");
        assert_eq!(after.len(), 2, "…and another node takes its place");
        assert_eq!(
            after[0].host, before[1].host,
            "the working node stays; only the dead one is replaced"
        );
        assert_eq!(
            pool.skipped_until(dead, T0 + 5 * MIN),
            Some(T0 + 5 * MIN + SKIP_FOR_SECS)
        );
    }

    #[test]
    fn three_tries_are_needed_however_long_the_failures_last() {
        // A node backing off to ten minutes between tries reaches five minutes on its second
        // failure. Two failures are not a verdict.
        let mut pool = NodePool::default();
        let host = "a.example.net:7300";
        assert_eq!(pool.record(host, &Outcome::NoGreeting, true, T0), None);
        assert_eq!(
            pool.record(host, &Outcome::NoGreeting, true, T0 + 10 * MIN),
            None,
            "two tries over ten minutes"
        );
        assert!(
            pool.record(host, &Outcome::NoGreeting, true, T0 + 20 * MIN)
                .is_some(),
            "the third is"
        );
    }

    /// T4 — the load-bearing one. A dead band must never cost a node its place.
    #[test]
    fn a_node_that_logged_in_and_then_went_quiet_for_six_hours_is_never_skipped() {
        let nodes = EIGHT.map(cc);
        let mut pool = NodePool::default();
        let picked = pool.pick(&nodes, "W1AW", none, T0);
        for n in &picked {
            pool.record(n.host, &Outcome::LoggedIn, true, T0);
        }
        // Six hours of nothing: a quiet session produces no outcome at all, so nothing is recorded.
        let later = T0 + 6 * HOUR;
        assert_eq!(
            hosts(&pool.pick(&nodes, "W1AW", none, later)),
            hosts(&picked),
            "still picked after six silent hours"
        );
        for n in &picked {
            assert_eq!(pool.skipped_until(n.host, later), None, "{}", n.host);
        }
    }

    /// T5, both ways: the offline guard.
    #[test]
    fn failures_with_no_other_feed_up_are_never_counted_toward_a_skip() {
        let host = "a.example.net:7300";
        let window = (T0, T0 + 30 * MIN, MIN);

        // Nothing else was up: this is our network, a sleeping laptop or a captive portal, and
        // it says nothing about the node.
        let mut offline = NodePool::default();
        assert_eq!(
            fail_between(
                &mut offline,
                host,
                Outcome::Unreachable("x".into()),
                false,
                window
            ),
            None
        );
        assert_eq!(offline.skipped_until(host, T0 + 30 * MIN), None);

        // POSITIVE CONTROL: the very same failures with another feed up ARE a skip.
        let mut online = NodePool::default();
        assert!(fail_between(
            &mut online,
            host,
            Outcome::Unreachable("x".into()),
            true,
            window
        )
        .is_some());
    }

    #[test]
    fn one_failure_with_nothing_else_up_starts_the_count_again() {
        let host = "a.example.net:7300";
        let mut pool = NodePool::default();
        fail_between(
            &mut pool,
            host,
            Outcome::NoPrompt,
            true,
            (T0, T0 + 4 * MIN, MIN),
        );
        // The network drops for one try…
        pool.record(host, &Outcome::NoPrompt, false, T0 + 5 * MIN);
        // …so four more minutes are not "failed every attempt for five minutes".
        assert_eq!(
            fail_between(
                &mut pool,
                host,
                Outcome::NoPrompt,
                true,
                (T0 + 6 * MIN, T0 + 10 * MIN, MIN)
            ),
            None
        );
        // Control: a full five minutes after the gap is.
        assert!(pool
            .record(host, &Outcome::NoPrompt, true, T0 + 11 * MIN)
            .is_some());
    }

    /// T6.
    #[test]
    fn a_login_clears_the_failures_before_it() {
        let host = "a.example.net:7300";
        let mut pool = NodePool::default();
        fail_between(
            &mut pool,
            host,
            Outcome::DroppedAfterLogin,
            true,
            (T0, T0 + 2 * MIN, 30),
        );
        pool.record(host, &Outcome::LoggedIn, true, T0 + 3 * MIN);
        // Four more minutes of failures: with the first two counted it would be six.
        assert_eq!(
            fail_between(
                &mut pool,
                host,
                Outcome::DroppedAfterLogin,
                true,
                (T0 + 4 * MIN, T0 + 8 * MIN, 30)
            ),
            None,
            "the streak began again at the first failure after the login"
        );
        // Control: five minutes after the login's first failure it is a skip.
        assert!(pool
            .record(host, &Outcome::DroppedAfterLogin, true, T0 + 9 * MIN)
            .is_some());
    }

    /// T7.
    #[test]
    fn with_every_node_skipped_two_are_still_picked() {
        let nodes = EIGHT.map(cc);
        let mut pool = NodePool::default();
        for n in &nodes {
            fail_between(
                &mut pool,
                n.host,
                Outcome::NoGreeting,
                true,
                (T0, T0 + 5 * MIN, MIN),
            );
            assert!(
                pool.skipped_until(n.host, T0 + 5 * MIN).is_some(),
                "{}",
                n.host
            );
        }
        let picked = pool.pick(&nodes, "W1AW", none, T0 + 5 * MIN);
        assert_eq!(picked.len(), 2, "never fewer than two while two exist");
        // The same two a call would get from a healthy list: skipping everything must not
        // reshuffle operators onto new nodes.
        assert_eq!(
            hosts(&picked),
            hosts(&NodePool::default().pick(&nodes, "W1AW", none, T0))
        );
    }

    #[test]
    fn a_list_of_one_picks_one_and_an_empty_list_picks_none() {
        let pool = NodePool::default();
        assert_eq!(pool.pick(&[cc(EIGHT[0])], "W1AW", none, T0).len(), 1);
        assert!(pool.pick(&[], "W1AW", none, T0).is_empty());
    }

    /// T8.
    #[test]
    fn at_most_one_dxspider_node_unless_nothing_else_is_eligible() {
        let mixed = [
            spider(EIGHT[0]),
            spider(EIGHT[1]),
            spider(EIGHT[2]),
            cc(EIGHT[3]),
        ];
        // Whatever the callsign's ordering, the CC node is taken rather than a second DXSpider.
        for call in ["W1AW", "K1ABC", "W9XYZ", "W1AW/P", "K1ABC/M"] {
            let picked = NodePool::default().pick(&mixed, call, none, T0);
            let spiders = picked
                .iter()
                .filter(|n| n.software == Software::DxSpider)
                .count();
            assert_eq!((picked.len(), spiders), (2, 1), "{call}: {picked:?}");
        }
        // Nothing else eligible: two DXSpider nodes rather than one node.
        let all_spider = [spider(EIGHT[0]), spider(EIGHT[1]), spider(EIGHT[2])];
        assert_eq!(
            NodePool::default()
                .pick(&all_spider, "W1AW", none, T0)
                .len(),
            2
        );
        let mut pool = NodePool::default();
        fail_between(
            &mut pool,
            EIGHT[3],
            Outcome::NoGreeting,
            true,
            (T0, T0 + 5 * MIN, MIN),
        );
        let picked = pool.pick(&mixed, "W1AW", none, T0 + 5 * MIN);
        assert_eq!(
            picked.len(),
            2,
            "the only other node is skipped, so a second DXSpider node is better than one node"
        );
        assert!(picked.iter().all(|n| n.software == Software::DxSpider));
    }

    fn synthetic_calls() -> impl Iterator<Item = String> {
        // The example callsigns with an index glued on. Deliberately not assignable callsigns, and
        // the worst case for the hash: long shared prefixes that differ only at the end.
        (0..1000).map(|i| format!("{}{i}", ["W1AW", "K1ABC", "W9XYZ"][i % 3]))
    }

    /// Picks per node across 1000 callsigns, as a multiple of each node's fair share.
    fn worst_share(nodes: &[Node]) -> f64 {
        let pool = NodePool::default();
        let mut counts: BTreeMap<&str, usize> = nodes.iter().map(|n| (n.host, 0)).collect();
        let mut calls = 0;
        for call in synthetic_calls() {
            calls += 1;
            for n in pool.pick(nodes, &call, none, T0) {
                *counts.get_mut(n.host).unwrap() += 1;
            }
        }
        let fair = (calls * CONNECTED_AT_ONCE) as f64 / nodes.len() as f64;
        counts
            .values()
            .map(|&c| c as f64 / fair)
            .fold(0.0, f64::max)
    }

    /// T9.
    #[test]
    fn a_callsign_always_gets_the_same_pair_and_the_load_spreads() {
        let nodes = EIGHT.map(cc);
        let pool = NodePool::default();
        let first = hosts(&pool.pick(&nodes, "W1AW", none, T0));
        assert_eq!(hosts(&pool.pick(&nodes, "W1AW", none, T0 + HOUR)), first);
        assert_eq!(
            hosts(&pool.pick(&nodes, " w1aw ", none, T0)),
            first,
            "the callsign as typed"
        );
        let mut reversed = nodes;
        reversed.reverse();
        assert_eq!(
            hosts(&pool.pick(&reversed, "W1AW", none, T0)),
            first,
            "the order of the list is not the order of the pick"
        );

        let spread = worst_share(&nodes);
        assert!(spread <= 2.0, "one node got {spread:.2}× its fair share");
        // POSITIVE CONTROL: the measure can fail. Without the callsign in the ordering every
        // install would sit on the same two nodes — four times their share with eight nodes.
        let no_hash: f64 = {
            let mut counts = BTreeMap::new();
            for _ in synthetic_calls() {
                for n in nodes.iter().take(CONNECTED_AT_ONCE) {
                    *counts.entry(n.host).or_insert(0usize) += 1;
                }
            }
            let fair = 1000.0 * CONNECTED_AT_ONCE as f64 / nodes.len() as f64;
            counts
                .values()
                .map(|&c| c as f64 / fair)
                .fold(0.0, f64::max)
        };
        assert!(no_hash > 2.0, "control measured {no_hash:.2}");

        // A mixed list: the DXSpider rule pushes load onto the other nodes, and still within bound.
        let mixed = [
            spider(EIGHT[0]),
            spider(EIGHT[1]),
            spider(EIGHT[2]),
            spider(EIGHT[3]),
            cc(EIGHT[4]),
            cc(EIGHT[5]),
            cc(EIGHT[6]),
            cc(EIGHT[7]),
        ];
        let spread = worst_share(&mixed);
        assert!(spread <= 2.0, "one node got {spread:.2}× its fair share");
    }

    #[test]
    fn the_shipped_list_spreads_load_within_twice_a_fair_share() {
        let spread = worst_share(&NODES);
        assert!(spread <= 2.0, "one node got {spread:.2}× its fair share");
        // POSITIVE CONTROL, and the reason the shipped list is mostly CC Cluster: the DXSpider rule
        // sends every pair to a non-DXSpider node, so a list heavy in DXSpider overloads the few
        // others. Seven DXSpider nodes and one CC Cluster node put that one node in every pair.
        let mut heavy = EIGHT.map(spider);
        heavy[7] = cc(EIGHT[7]);
        let control = worst_share(&heavy);
        assert!(control > 2.0, "control measured {control:.2}");
    }

    #[test]
    fn an_expired_skip_waits_behind_untried_nodes() {
        let nodes = EIGHT.map(cc);
        let mut pool = NodePool::default();
        let first = pool.pick(&nodes, "W1AW", none, T0)[0].host;
        fail_between(
            &mut pool,
            first,
            Outcome::NoGreeting,
            true,
            (T0, T0 + 5 * MIN, MIN),
        );
        let day_later = T0 + 5 * MIN + SKIP_FOR_SECS;
        assert_eq!(
            pool.skipped_until(first, day_later),
            None,
            "the skip is over"
        );
        assert!(
            !hosts(&pool.pick(&nodes, "W1AW", none, day_later)).contains(&first),
            "…but a node that failed its last check ranks behind nodes never tried"
        );
    }

    #[test]
    fn a_node_that_worked_this_week_ranks_ahead_of_one_never_tried() {
        let nodes = EIGHT.map(cc);
        let mut pool = NodePool::default();
        let pick = pool.pick(&nodes, "W1AW", none, T0);
        // The operator's hash puts `pick` first; a node lower down that worked recently jumps it.
        let last = nodes
            .iter()
            .find(|n| !hosts(&pick).contains(&n.host))
            .unwrap()
            .host;
        pool.record(last, &Outcome::LoggedIn, true, T0);
        assert!(hosts(&pool.pick(&nodes, "W1AW", none, T0 + HOUR)).contains(&last));
        // A week later that is no longer news, and the hash decides again.
        assert_eq!(
            hosts(&pool.pick(&nodes, "W1AW", none, T0 + RECENT_OK_SECS + 1)),
            hosts(&pick)
        );
    }

    #[test]
    fn a_connected_node_keeps_its_place() {
        // A node connected right now is working right now, however long ago its last proof was
        // written down — a session that has been up for eight days must not be swapped out.
        let nodes = EIGHT.map(cc);
        let mut pool = NodePool::default();
        let pick = pool.pick(&nodes, "W1AW", none, T0);
        let other = nodes
            .iter()
            .find(|n| !hosts(&pick).contains(&n.host))
            .unwrap()
            .host;
        for n in &pick {
            pool.record(n.host, &Outcome::LoggedIn, true, T0);
        }
        pool.record(other, &Outcome::LoggedIn, true, T0 + 7 * 24 * HOUR);
        let eight_days = T0 + 8 * 24 * HOUR;
        let up = |h: &str| hosts(&pick).contains(&h);
        assert_eq!(
            hosts(&pool.pick(&nodes, "W1AW", up, eight_days)),
            hosts(&pick)
        );
    }

    #[test]
    fn a_working_session_ends_a_skip() {
        let host = "a.example.net:7300";
        let mut pool = NodePool::default();
        fail_between(
            &mut pool,
            host,
            Outcome::NoGreeting,
            true,
            (T0, T0 + 5 * MIN, MIN),
        );
        assert!(pool.skipped_until(host, T0 + 6 * MIN).is_some());
        pool.record(host, &Outcome::LoggedIn, true, T0 + 6 * MIN);
        assert_eq!(pool.skipped_until(host, T0 + 6 * MIN), None);
        assert_eq!(pool.health(host).unwrap().failure, None);
    }

    /// T13, the pure half: `src-tauri` owns the file.
    #[test]
    fn health_round_trips_and_an_unreadable_file_is_no_health() {
        let mut pool = NodePool::default();
        pool.record("a.example.net:7300", &Outcome::LoggedIn, true, T0);
        fail_between(
            &mut pool,
            "b.example.net:7300",
            Outcome::NoPrompt,
            true,
            (T0, T0 + 5 * MIN, MIN),
        );
        let text = pool.to_json();
        assert_eq!(NodePool::from_json(&text).to_json(), text);
        assert_eq!(
            NodePool::from_json(&text).skipped_until("b.example.net:7300", T0 + 6 * MIN),
            pool.skipped_until("b.example.net:7300", T0 + 6 * MIN)
        );
        for broken in [
            "",
            "{",
            "[]",
            "{\"nodes\":{\"a\":{\"failure\":\"onFire\"}}}",
        ] {
            assert_eq!(
                NodePool::from_json(broken),
                NodePool::default(),
                "{broken:?}"
            );
        }
    }

    #[test]
    fn a_node_s_own_words_never_reach_the_file() {
        let mut pool = NodePool::default();
        pool.record(
            "a.example.net:7300",
            &Outcome::Unreachable("cannot reach a.example.net:7300: SECRET-ish server text".into()),
            true,
            T0,
        );
        let text = pool.to_json();
        assert!(!text.contains("SECRET"), "{text}");
        assert!(
            text.contains("unreachable"),
            "the reason code is kept: {text}"
        );
    }

    #[test]
    fn a_failure_streak_does_not_survive_a_restart() {
        // Failures either side of a closed app are not five minutes of failures.
        let host = "a.example.net:7300";
        let mut pool = NodePool::default();
        fail_between(
            &mut pool,
            host,
            Outcome::NoGreeting,
            true,
            (T0, T0 + 4 * MIN, MIN),
        );
        let mut reloaded = NodePool::from_json(&pool.to_json());
        assert_eq!(
            reloaded.record(host, &Outcome::NoGreeting, true, T0 + 12 * HOUR),
            None
        );
        // Control: without the reload the same report completes the streak.
        assert!(pool
            .record(host, &Outcome::NoGreeting, true, T0 + 5 * MIN)
            .is_some());
    }

    #[test]
    fn hosts_match_whatever_their_case_or_spacing() {
        let mut pool = NodePool::default();
        pool.record(" A.Example.NET:7300 ", &Outcome::LoggedIn, true, T0);
        assert!(pool.health("a.example.net:7300").is_some());
    }

    #[test]
    fn the_shipped_nodes_are_distinct_human_nodes() {
        let mut seen = std::collections::BTreeSet::new();
        for n in &NODES {
            assert!(seen.insert(key(n.host)), "{} listed twice", n.host);
            let (host, port) = n.host.rsplit_once(':').expect("host:port");
            assert!(
                !host.is_empty() && port.parse::<u16>().is_ok(),
                "{}",
                n.host
            );
            // RBN is wired separately, and a skimmer port duplicates it.
            assert!(!host.contains("reversebeacon.net"), "{}", n.host);
            assert_ne!(n.host, "dxc.nc7j.com:7373", "NC7J's skimmer port");
        }
    }
}
