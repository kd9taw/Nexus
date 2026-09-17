//! The human DX-cluster nodes — the SSB/phone half of the spot sources: which node feeds are
//! running, which should be, and how each node has done. RBN's CW and digital feeds are wired
//! separately in `lib.rs` and never enter here.
//!
//! **What should run is the operator's choice** (`Settings::cluster_nodes_auto`):
//! - **automatic** — [`tempo_net::cluster::pool`] picks from the nodes built into the release,
//!   keeps two connected, and moves off one that has stopped working;
//! - **their own list** — every node in `Settings::cluster_hosts` runs, as written, and nothing
//!   ever switches it. The pool still records how each node does, for Settings to show.
//!
//! **One registry, keyed by host** ([`FEEDS`]): every running feed with its own stop flag and its
//! own logged-in flag. [`reconcile`] starts what should run and stops what should not, so a node
//! removed on Save disconnects on Save and a node the pool skips disconnects at once. It replaced
//! a start-only latch, which could not stop a node short of a restart, and a list of logged-in
//! flags that did not say whose they were — which is how the Phone pill could name a dead node.
//!
//! **Lock order: [`FEEDS`], then [`CONFIG`], then [`HEALTH`]** — take any of them in that order,
//! never the reverse. Nothing here takes the engine lock: the feed threads read [`CONFIG`], a
//! mirror of the settings `lib.rs` keeps in step at launch, on every Save and after a callsign
//! change — the reason `UNASSISTED` and `OPERATOR_QTH` are mirrors too.

use crate::{conn_log, is_real_call, now_unix, SharedHealth, SharedSpots};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use tempo_app::settings::Settings;
use tempo_net::cluster::pool::{self as node_pool, Failure, NodePool, Skip, Software};
use tempo_net::cluster::Outcome;

/// The settings the node feeds run under, mirrored so a feed thread never needs the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NodeConfig {
    /// `Settings::cluster_active()`: the cluster switch, with Unassisted mode folded in.
    pub active: bool,
    /// `Settings::cluster_nodes_auto`.
    pub auto: bool,
    /// `Settings::cluster_hosts`, the operator's own list.
    pub hosts: Vec<String>,
    /// The bare callsign — the login adds the SSID, the pool's ordering does not.
    pub mycall: String,
    /// `Settings::cluster_ssid`.
    pub ssid: String,
}

impl NodeConfig {
    pub(crate) fn of(s: &Settings) -> Self {
        Self {
            active: s.cluster_active(),
            auto: s.cluster_nodes_auto,
            hosts: s.cluster_hosts.clone(),
            mycall: s.mycall.clone(),
            ssid: s.cluster_ssid.clone(),
        }
    }
}

/// The mirror. Starts inactive: nothing runs before `lib.rs` has published the real settings.
static CONFIG: Mutex<NodeConfig> = Mutex::new(NodeConfig {
    active: false,
    auto: true,
    hosts: Vec::new(),
    mycall: String::new(),
    ssid: String::new(),
});

/// Publish the settings the node feeds run under. Starts and stops nothing by itself.
pub(crate) fn set_config(config: NodeConfig) {
    *lock(&CONFIG) = config;
}

/// A poisoned lock still holds a usable registry; a panic elsewhere must not strand the feeds.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One running node feed.
pub(crate) struct Feed {
    host: String,
    /// The feed thread's own stop flag.
    stop: Arc<AtomicBool>,
    /// True while its session is logged in — the flag `tempo_net::cluster::run` keeps, with the
    /// meaning it has always had there: a login prompt was answered.
    connected: Arc<AtomicBool>,
}

impl Feed {
    /// A feed's flags, for `lib.rs` to start a thread on.
    pub(crate) fn new(host: &str) -> Self {
        Self {
            host: host.to_string(),
            stop: Arc::new(AtomicBool::new(false)),
            connected: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn host(&self) -> &str {
        &self.host
    }

    pub(crate) fn flags(&self) -> (Arc<AtomicBool>, Arc<AtomicBool>) {
        (self.stop.clone(), self.connected.clone())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

/// The running node feeds, at most one per host (hosts compare without case).
pub(crate) struct Feeds(Vec<Feed>);

impl Feeds {
    const fn new() -> Self {
        Self(Vec::new())
    }

    /// Stop every feed whose host is not in `desired`, then start each host in `desired` that is
    /// not running, through `start`. Returns the hosts stopped and the hosts started.
    ///
    /// A stopped feed leaves the registry at once, so nothing that reads it — `post_spot`, the
    /// Phone pill — counts it again, even in the second or two its thread takes to notice.
    fn reconcile(
        &mut self,
        desired: &[String],
        mut start: impl FnMut(&str) -> Feed,
    ) -> (Vec<String>, Vec<String>) {
        let mut stopped = Vec::new();
        self.0.retain(|f| {
            let keep = desired.iter().any(|d| d.eq_ignore_ascii_case(&f.host));
            if !keep {
                f.stop.store(true, Ordering::SeqCst);
                stopped.push(f.host.clone());
            }
            keep
        });
        let mut started = Vec::new();
        for host in desired {
            if !self.0.iter().any(|f| f.host.eq_ignore_ascii_case(host)) {
                self.0.push(start(host));
                started.push(host.clone());
            }
        }
        (stopped, started)
    }

    /// Stop every feed: a changed callsign or SSID has to log in again everywhere.
    fn stop_all(&mut self) {
        for f in self.0.drain(..) {
            f.stop.store(true, Ordering::SeqCst);
        }
    }

    fn is_running(&self, host: &str) -> bool {
        self.0.iter().any(|f| f.host.eq_ignore_ascii_case(host))
    }

    fn is_connected(&self, host: &str) -> bool {
        self.0
            .iter()
            .any(|f| f.host.eq_ignore_ascii_case(host) && f.is_connected())
    }

    /// Whether any node is logged in: what `post_spot` needs before it queues a spot.
    pub(crate) fn any_connected(&self) -> bool {
        self.0.iter().any(Feed::is_connected)
    }

    /// Whether a node OTHER than `host` is logged in.
    fn any_other_connected(&self, host: &str) -> bool {
        self.0
            .iter()
            .any(|f| !f.host.eq_ignore_ascii_case(host) && f.is_connected())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The Phone source label: the nodes logged in right now — or, while none is, the nodes being
    /// tried. It never names a node that is not connected while one is: the label used to come
    /// from the list of nodes STARTED, so the pill could read "live" beside a dead node's name.
    pub(crate) fn label(&self) -> Option<String> {
        let connected: Vec<String> = self
            .0
            .iter()
            .filter(|f| f.is_connected())
            .map(|f| f.host.clone())
            .collect();
        if connected.is_empty() {
            let tried: Vec<String> = self.0.iter().map(|f| f.host.clone()).collect();
            crate::summarize_hosts(&tried)
        } else {
            crate::summarize_hosts(&connected)
        }
    }
}

/// Every running node feed.
pub(crate) static FEEDS: Mutex<Feeds> = Mutex::new(Feeds::new());

/// Read the registry.
pub(crate) fn feeds() -> MutexGuard<'static, Feeds> {
    lock(&FEEDS)
}

/// The pool, and the text last written to `cluster-nodes.json`, so the file is written only when
/// it would change.
struct Health {
    pool: NodePool,
    written: String,
}

impl Health {
    /// The health in `path`. A missing or unreadable file is no health at all — every node
    /// untried — and never an error: it is a status file.
    fn load(path: &Path) -> Self {
        let pool = NodePool::from_json(&std::fs::read_to_string(path).unwrap_or_default());
        Self {
            written: pool.to_json(),
            pool,
        }
    }

    /// Write the pool to `path` if its text changed since the last write, keeping only nodes
    /// that are built in or in the operator's list — so the file does not grow with every node
    /// ever typed. Returns whether it wrote.
    fn save_if_changed(&mut self, path: &Path, config: &NodeConfig) -> bool {
        self.pool.retain(|host| {
            node_pool::NODES
                .iter()
                .any(|n| n.host.eq_ignore_ascii_case(host))
                || config
                    .hosts
                    .iter()
                    .any(|h| h.trim().eq_ignore_ascii_case(host))
        });
        let text = self.pool.to_json();
        if text == self.written || !crate::write_json_atomic(path, &text) {
            return false;
        }
        self.written = text;
        true
    }
}

/// Node health, read from `cluster-nodes.json` on first use.
static HEALTH: LazyLock<Mutex<Health>> = LazyLock::new(|| Mutex::new(Health::load(&health_path())));

/// Where node health is kept: `cluster-nodes.json` beside settings.json. Codes and times only —
/// see `tempo_net::cluster::pool::Failure`.
#[cfg(not(test))]
fn health_path() -> PathBuf {
    crate::config_dir().join("cluster-nodes.json")
}

/// The test twin of [`health_path`]: never the operator's file (`conn_health_path` records what
/// a test writing the real one costs). The tests below build their own [`Health`] on their own
/// paths; this only catches a test that reaches the global by accident.
#[cfg(test)]
fn health_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "nexus-test-cluster-nodes-{}.json",
        std::process::id()
    ))
}

/// How recently a spot must have arrived, from any feed, for another feed to count as up.
const OTHER_FEED_FRESH_SECS: i64 = 120;

/// The skip rule's offline guard, as far as this app can see: another feed logged in — RBN, or
/// another node — AND a spot from any feed inside [`OTHER_FEED_FRESH_SECS`].
///
/// **"Logged in" alone is not enough, and the common way to go offline is why.** Every feed only
/// reads — it never writes to RBN, and writes to a node only to post a spot — and a socket that
/// only reads, with no keepalive, can go on looking connected long after its path has gone. So a
/// laptop whose Wi-Fi drops would still show RBN "up" while each node it tried failed, and those
/// nodes would be skipped for a day. Spots arriving is what shows the path works. A quiet spell
/// can only delay a skip, never cause one.
fn other_feed_live_at(
    now: i64,
    last_spot_unix: i64,
    rbn_connected: bool,
    other_node_connected: bool,
) -> bool {
    (rbn_connected || other_node_connected)
        && last_spot_unix > 0
        && now.saturating_sub(last_spot_unix) <= OTHER_FEED_FRESH_SECS
}

/// The Unix time as the pool counts it.
fn now_secs() -> u64 {
    u64::try_from(now_unix()).unwrap_or(0)
}

/// What should be running: the pool's pick in automatic mode, the operator's list otherwise —
/// less blanks, RBN endpoints (wired separately) and repeats, and otherwise as written.
fn desired(config: &NodeConfig, pool: &NodePool, feeds: &Feeds, now: u64) -> Vec<String> {
    if config.auto {
        return pool
            .pick(
                &node_pool::NODES,
                &config.mycall,
                |h| feeds.is_connected(h),
                now,
            )
            .iter()
            .map(|n| n.host.to_string())
            .collect();
    }
    let mut out: Vec<String> = Vec::new();
    for host in &config.hosts {
        let host = host.trim();
        if !host.is_empty()
            && !host.contains("reversebeacon.net")
            && !out.iter().any(|o| o.eq_ignore_ascii_case(host))
        {
            out.push(host.to_string());
        }
    }
    out
}

/// Bring the running node feeds in line with the settings mirror and the pool, starting feeds
/// through `lib.rs` and stopping them by their flags. Returns the hosts stopped and started.
///
/// Does nothing while the cluster is off (or Unassisted) or there is no real callsign: turning the
/// cluster off has always left running feeds alone until a restart, and this does not change that.
///
/// `restarting` is the callsign-change drain's own call. Everything else stands aside while a
/// drain is in flight — checked under the registry lock, after the drain's `stop_all` took the
/// same lock, so a feed can never be started under the OLD callsign and survive the drain.
pub(crate) fn reconcile(
    spots: &SharedSpots,
    health: &SharedHealth,
    restarting: bool,
) -> (Vec<String>, Vec<String>) {
    let mut feeds = lock(&FEEDS);
    if !restarting && crate::FEED_RESTART_IN_FLIGHT.load(Ordering::SeqCst) {
        return Default::default();
    }
    let config = lock(&CONFIG).clone();
    if !config.active || !is_real_call(&config.mycall) {
        return Default::default();
    }
    let want = desired(&config, &lock(&HEALTH).pool, &feeds, now_secs());
    let call = tempo_net::cluster::login_call(&config.mycall, &config.ssid);
    feeds.reconcile(&want, |host| {
        let feed = Feed::new(host);
        crate::start_human_cluster_feed(spots, health, &feed, &call);
        feed
    })
}

/// Stop every node feed — see [`Feeds::stop_all`].
pub(crate) fn stop_all() {
    lock(&FEEDS).stop_all();
}

/// A node feed says what an attempt came to: record it, write the change, and move off the node
/// if that skipped it — with one line in the connection log for the switch.
pub(crate) fn on_outcome(
    spots: &SharedSpots,
    health: &SharedHealth,
    host: &str,
    outcome: &Outcome,
) {
    let now = now_unix();
    let other_live = other_feed_live_at(
        now,
        health.cluster_last.load(Ordering::Relaxed),
        health.cluster_connected.load(Ordering::Relaxed),
        lock(&FEEDS).any_other_connected(host),
    );
    let config = lock(&CONFIG).clone();
    let skip = {
        let mut h = lock(&HEALTH);
        let skip = h.pool.record(host, outcome, other_live, now_secs());
        h.save_if_changed(&health_path(), &config);
        skip
    };
    let (stopped, started) = reconcile(spots, health, false);
    if let Some(skip) = skip {
        if stopped.iter().any(|s| s.eq_ignore_ascii_case(host)) {
            conn_log(
                "DX Cluster",
                "info",
                switch_line(host, skip, started.first().map(String::as_str)),
            );
        }
    }
}

/// The connection log's one line for a switch: the node left, why, and what took its place.
fn switch_line(host: &str, skip: Skip, next: Option<&str>) -> String {
    let what = match skip.failure {
        Failure::Unreachable => "could not be reached",
        Failure::NoGreeting => "sent nothing",
        Failure::NoPrompt => "sent no login prompt",
        Failure::DroppedAfterLogin => "dropped every login",
    };
    let minutes = skip.failing_secs / 60;
    match next {
        Some(next) => format!("{host} {what} for {minutes} min — using {next}"),
        None => format!("{host} {what} for {minutes} min — no other node to use"),
    }
}

/// One node as Settings › Spot Sources shows it.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NodeView {
    host: String,
    /// The node's callsign, for a node built into this release.
    #[serde(skip_serializing_if = "Option::is_none")]
    callsign: Option<&'static str>,
    /// Its software, for a node built into this release.
    #[serde(skip_serializing_if = "Option::is_none")]
    software: Option<Software>,
    /// A feed is running for it now.
    running: bool,
    /// Logged in now.
    connected: bool,
    /// Its latest failure, if it has failed since it last worked.
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<Failure>,
    /// While automatic mode skips it: when the skip ends (Unix seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped_until_unix: Option<u64>,
}

/// Every node Settings can show: the built-in nodes, in their shipped order, then the operator's
/// own that are not among them.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NodesView {
    /// The SAVED choice — the form may hold an unsaved one.
    auto: bool,
    nodes: Vec<NodeView>,
}

/// The node standings, for `get_cluster_nodes`.
pub(crate) fn view() -> NodesView {
    let feeds = lock(&FEEDS);
    let config = lock(&CONFIG).clone();
    let health = lock(&HEALTH);
    view_of(&config, &health.pool, &feeds, now_secs())
}

fn view_of(config: &NodeConfig, pool: &NodePool, feeds: &Feeds, now: u64) -> NodesView {
    let mut hosts: Vec<String> = node_pool::NODES
        .iter()
        .map(|n| n.host.to_string())
        .collect();
    for host in config.hosts.iter().map(|h| h.trim()) {
        if !host.is_empty() && !hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
            hosts.push(host.to_string());
        }
    }
    let nodes = hosts
        .into_iter()
        .map(|host| {
            let built_in = node_pool::NODES
                .iter()
                .find(|n| n.host.eq_ignore_ascii_case(&host));
            NodeView {
                callsign: built_in.map(|n| n.callsign),
                software: built_in.map(|n| n.software),
                running: feeds.is_running(&host),
                connected: feeds.is_connected(&host),
                failure: pool.health(&host).and_then(|h| h.failure),
                skipped_until_unix: pool.skipped_until(&host, now),
                host,
            }
        })
        .collect();
    NodesView {
        auto: config.auto,
        nodes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An arbitrary Unix time to start the fake clock from.
    const T0: u64 = 1_758_000_000;

    fn auto_config() -> NodeConfig {
        NodeConfig {
            active: true,
            auto: true,
            hosts: Vec::new(),
            mycall: "W1AW".into(),
            ssid: String::new(),
        }
    }

    fn flags_of<'f>(feeds: &'f Feeds, host: &str) -> &'f Feed {
        feeds.0.iter().find(|f| f.host == host).unwrap()
    }

    /// T11.
    #[test]
    fn a_skipped_node_is_stopped_and_no_longer_counts_as_connected() {
        let config = auto_config();
        let mut pool = NodePool::default();
        let mut feeds = Feeds::new();
        let want = desired(&config, &pool, &feeds, T0);
        assert_eq!(want.len(), 2, "{want:?}");
        feeds.reconcile(&want, Feed::new);
        let (victim, other) = (want[0].clone(), want[1].clone());
        flags_of(&feeds, &other)
            .connected
            .store(true, Ordering::Relaxed);
        let victim_stop = flags_of(&feeds, &victim).stop.clone();

        // The victim fails every try for five minutes while the other node is up.
        for minute in 0..=5 {
            pool.record(&victim, &Outcome::NoGreeting, true, T0 + minute * 60);
        }
        let want = desired(&config, &pool, &feeds, T0 + 300);
        let (stopped, started) = feeds.reconcile(&want, Feed::new);
        assert_eq!(stopped, vec![victim.clone()]);
        assert!(
            victim_stop.load(Ordering::SeqCst),
            "its thread was told to stop"
        );
        assert_eq!(started.len(), 1, "and a replacement was started");
        assert!(!feeds.is_running(&victim));
        assert!(feeds.is_running(&other), "the working node was left alone");
    }

    /// T11, the `post_spot` half: a node leaving the registry stops counting at once, even while
    /// its thread's flag still reads logged in.
    #[test]
    fn a_stopped_node_stops_counting_as_connected_before_its_thread_ends() {
        let mut feeds = Feeds::new();
        feeds.reconcile(&["a.example.net:7300".to_string()], Feed::new);
        let flag = flags_of(&feeds, "a.example.net:7300").connected.clone();
        flag.store(true, Ordering::Relaxed);
        assert!(feeds.any_connected(), "control: it counts while it runs");
        feeds.reconcile(&[], Feed::new);
        assert!(
            flag.load(Ordering::Relaxed),
            "the thread has not noticed yet"
        );
        assert!(
            !feeds.any_connected(),
            "but post_spot must not queue a spot for it"
        );
    }

    #[test]
    fn a_list_the_operator_wrote_is_never_switched() {
        // D5: skipped or not, the operator's nodes all run as written.
        let config = NodeConfig {
            auto: false,
            hosts: vec!["a.example.net:7300".into(), "b.example.net:7373".into()],
            ..auto_config()
        };
        let mut pool = NodePool::default();
        for minute in 0..=5 {
            pool.record(
                "a.example.net:7300",
                &Outcome::NoGreeting,
                true,
                T0 + minute * 60,
            );
        }
        assert!(pool.skipped_until("a.example.net:7300", T0 + 300).is_some());
        assert_eq!(
            desired(&config, &pool, &Feeds::new(), T0 + 300),
            config.hosts
        );
    }

    #[test]
    fn my_list_runs_as_written_less_blanks_rbn_and_repeats() {
        let config = NodeConfig {
            auto: false,
            hosts: vec![
                " dx.example.net:23 ".into(),
                String::new(),
                "telnet.reversebeacon.net:7000".into(),
                "DX.EXAMPLE.NET:23".into(),
                "b.example.net:7300".into(),
            ],
            ..auto_config()
        };
        assert_eq!(
            desired(&config, &NodePool::default(), &Feeds::new(), T0),
            vec![
                "dx.example.net:23".to_string(),
                "b.example.net:7300".to_string()
            ]
        );
    }

    #[test]
    fn automatic_mode_runs_two_built_in_nodes() {
        let want = desired(&auto_config(), &NodePool::default(), &Feeds::new(), T0);
        assert_eq!(want.len(), 2);
        for host in &want {
            assert!(node_pool::NODES.iter().any(|n| n.host == host), "{host}");
        }
    }

    #[test]
    fn a_running_node_is_not_restarted_and_a_repeat_is_not_doubled() {
        let mut feeds = Feeds::new();
        let a = "a.example.net:7300".to_string();
        feeds.reconcile(std::slice::from_ref(&a), Feed::new);
        let first_stop = flags_of(&feeds, &a).stop.clone();
        let (stopped, started) = feeds.reconcile(&[a.to_uppercase()], Feed::new);
        assert_eq!((stopped.len(), started.len()), (0, 0));
        assert!(!first_stop.load(Ordering::SeqCst));
        assert_eq!(feeds.0.len(), 1);
    }

    /// T12.
    #[test]
    fn the_phone_label_never_names_a_node_that_is_not_connected_while_one_is() {
        let mut feeds = Feeds::new();
        feeds.reconcile(
            &[
                "dead.example.net:23".to_string(),
                "live.example.net:7300".to_string(),
            ],
            Feed::new,
        );
        assert_eq!(
            feeds.label().as_deref(),
            Some("dead.example.net:23 +1"),
            "while none is connected, the label names the nodes being tried"
        );
        flags_of(&feeds, "live.example.net:7300")
            .connected
            .store(true, Ordering::Relaxed);
        assert_eq!(feeds.label().as_deref(), Some("live.example.net:7300"));
        flags_of(&feeds, "dead.example.net:23")
            .connected
            .store(true, Ordering::Relaxed);
        assert_eq!(feeds.label().as_deref(), Some("dead.example.net:23 +1"));
        assert_eq!(Feeds::new().label(), None);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexus-test-cluster-nodes-{}-{:?}-{name}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("cluster-nodes.json")
    }

    /// T13.
    #[test]
    fn node_health_saves_and_reloads_unchanged_and_a_missing_or_corrupt_file_is_no_health() {
        let path = scratch("roundtrip");
        let config = auto_config();

        let mut missing = Health::load(&path);
        assert_eq!(
            missing.pool,
            NodePool::default(),
            "a missing file is no health"
        );
        assert!(
            !missing.save_if_changed(&path, &config),
            "and nothing to write"
        );
        assert!(!path.exists());

        let host = node_pool::NODES[0].host;
        missing.pool.record(host, &Outcome::LoggedIn, true, T0);
        for minute in 0..=5 {
            missing.pool.record(
                node_pool::NODES[1].host,
                &Outcome::NoPrompt,
                true,
                T0 + minute * 60,
            );
        }
        assert!(missing.save_if_changed(&path, &config));
        assert!(
            !missing.save_if_changed(&path, &config),
            "written only when it would change"
        );
        let reloaded = Health::load(&path);
        assert_eq!(reloaded.pool.to_json(), missing.pool.to_json());
        assert_eq!(
            reloaded
                .pool
                .skipped_until(node_pool::NODES[1].host, T0 + 400),
            missing
                .pool
                .skipped_until(node_pool::NODES[1].host, T0 + 400)
        );

        std::fs::write(&path, "{ this is not json").unwrap();
        assert_eq!(
            Health::load(&path).pool,
            NodePool::default(),
            "corrupt is no health"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_health_file_keeps_only_nodes_still_in_use() {
        let path = scratch("prune");
        let mut health = Health::load(&path);
        health
            .pool
            .record("typed.once.example.net:23", &Outcome::LoggedIn, true, T0);
        health
            .pool
            .record(node_pool::NODES[0].host, &Outcome::LoggedIn, true, T0);
        let listed = NodeConfig {
            hosts: vec!["listed.example.net:7300".into()],
            ..auto_config()
        };
        health
            .pool
            .record("listed.example.net:7300", &Outcome::LoggedIn, true, T0);
        assert!(health.save_if_changed(&path, &listed));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("typed.once"), "{text}");
        assert!(text.contains(node_pool::NODES[0].host) && text.contains("listed.example.net"));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn another_feed_counts_as_up_only_while_spots_are_arriving() {
        let now = 1_758_000_000_i64;
        assert!(
            other_feed_live_at(now, now - 5, true, false),
            "RBN up and delivering"
        );
        assert!(
            other_feed_live_at(now, now - 5, false, true),
            "another node up and delivering"
        );
        assert!(
            !other_feed_live_at(now, now - 5, false, false),
            "nothing else logged in"
        );
        // THE CASE THIS EXISTS FOR: a dropped network leaves the sessions looking logged in.
        assert!(
            !other_feed_live_at(now, now - OTHER_FEED_FRESH_SECS - 1, true, true),
            "logged in, but nothing has arrived for minutes"
        );
        assert!(
            !other_feed_live_at(now, 0, true, true),
            "nothing has ever arrived"
        );
    }

    #[test]
    fn a_switch_is_one_line_naming_both_nodes_and_why() {
        let skip = Skip {
            failure: Failure::NoPrompt,
            failing_secs: 5 * 60 + 17,
        };
        assert_eq!(
            switch_line("ve7cc.net:23", skip, Some("dxspots.com:7300")),
            "ve7cc.net:23 sent no login prompt for 5 min — using dxspots.com:7300"
        );
        assert!(switch_line("ve7cc.net:23", skip, None).ends_with("no other node to use"));
    }

    #[test]
    fn settings_see_every_built_in_node_then_the_operators_own() {
        let config = NodeConfig {
            auto: false,
            hosts: vec![
                "mine.example.net:23".into(),
                node_pool::NODES[2].host.to_uppercase(),
            ],
            ..auto_config()
        };
        let mut feeds = Feeds::new();
        feeds.reconcile(&["mine.example.net:23".to_string()], Feed::new);
        flags_of(&feeds, "mine.example.net:23")
            .connected
            .store(true, Ordering::Relaxed);
        let mut pool = NodePool::default();
        pool.record("mine.example.net:23", &Outcome::NoGreeting, false, T0);
        let view = view_of(&config, &pool, &feeds, T0);
        assert!(!view.auto);
        assert_eq!(
            view.nodes.len(),
            node_pool::NODES.len() + 1,
            "no duplicate for a built-in node typed in capitals"
        );
        assert_eq!(view.nodes[0].host, node_pool::NODES[0].host);
        assert_eq!(view.nodes[0].callsign, Some(node_pool::NODES[0].callsign));
        let mine = view.nodes.last().unwrap();
        assert_eq!(
            (
                mine.host.as_str(),
                mine.callsign,
                mine.running,
                mine.connected,
                mine.failure
            ),
            (
                "mine.example.net:23",
                None,
                true,
                true,
                Some(Failure::NoGreeting)
            )
        );
    }
}
