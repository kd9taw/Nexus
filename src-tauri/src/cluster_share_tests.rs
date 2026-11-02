//! Two windows on one computer, as two `Share`s over one config dir, each with its own spot
//! intake and outbox, logging into fake cluster nodes on loopback through the real
//! `tempo_net::cluster::run`. A window's process ending is `close`: the lease is let go, the
//! relay stops, its feeds stop.
//!
//! These dial localhost: on the WSL2 box run them inside `unshare -rn` (see the agent rules).

use super::*;
use std::collections::VecDeque;
use std::sync::atomic::AtomicUsize;
use std::sync::OnceLock;

/// A cluster node: prompts for a call, counts logins and how many are up at once, records every
/// line a logged-in client sends, and sends spot lines to every session.
struct FakeNode {
    addr: SocketAddr,
    logins: AtomicUsize,
    live: AtomicUsize,
    max_live: AtomicUsize,
    heard: Mutex<Vec<String>>,
    sessions: Mutex<Vec<TcpStream>>,
    refuse: AtomicBool,
}

impl FakeNode {
    fn start() -> Arc<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let node = Arc::new(Self {
            addr: listener.local_addr().unwrap(),
            logins: AtomicUsize::new(0),
            live: AtomicUsize::new(0),
            max_live: AtomicUsize::new(0),
            heard: Mutex::default(),
            sessions: Mutex::default(),
            refuse: AtomicBool::new(false),
        });
        let n = node.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if n.refuse.load(Ordering::SeqCst) {
                    continue;
                }
                let n = n.clone();
                std::thread::spawn(move || n.serve(stream));
            }
        });
        node
    }

    fn serve(&self, mut stream: TcpStream) {
        if stream
            .write_all(b"Welcome to the test node\r\nlogin: ")
            .is_err()
        {
            return;
        }
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut call = String::new();
        if reader.read_line(&mut call).unwrap_or(0) == 0 {
            return;
        }
        self.logins.fetch_add(1, Ordering::SeqCst);
        let now = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_live.fetch_max(now, Ordering::SeqCst);
        lock(&self.sessions).push(stream.try_clone().unwrap());
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            let l = line.trim();
            if !l.is_empty() {
                lock(&self.heard).push(l.to_string());
            }
            line.clear();
        }
        self.live.fetch_sub(1, Ordering::SeqCst);
    }

    /// Send a spot of `dx` to every logged-in session.
    fn spot(&self, dx: &str) {
        let line = format!("DX de W9TST:     14025.0  {dx}  test spot     1200Z\r\n");
        for s in lock(&self.sessions).iter_mut() {
            let _ = s.write_all(line.as_bytes());
        }
    }

    /// Drop every session and refuse new ones: the node goes away.
    fn kick(&self) {
        self.refuse.store(true, Ordering::SeqCst);
        for s in lock(&self.sessions).drain(..) {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }

    fn heard(&self, line: &str) -> usize {
        lock(&self.heard).iter().filter(|l| *l == line).count()
    }
}

/// One window: its own share, spot intake, outbox and feeds.
struct TestHooks {
    call: String,
    nodes: Vec<SocketAddr>,
    active: AtomicBool,
    me: OnceLock<Share>,
    feeds_stop: Arc<AtomicBool>,
    feeds_conn: Mutex<Vec<Arc<AtomicBool>>>,
    outbox: Arc<Mutex<VecDeque<String>>>,
    own: Arc<Mutex<Vec<String>>>,
    relayed: Mutex<Vec<(Source, String)>>,
    feeds_started: AtomicUsize,
}

impl TestHooks {
    fn start_feeds(&self) {
        self.feeds_started.fetch_add(1, Ordering::SeqCst);
        for &addr in &self.nodes {
            let conn = Arc::new(AtomicBool::new(false));
            lock(&self.feeds_conn).push(conn.clone());
            let (stop, outbox, own) = (
                self.feeds_stop.clone(),
                self.outbox.clone(),
                self.own.clone(),
            );
            let share = self.me.get().unwrap().clone();
            let call = self.call.clone();
            std::thread::spawn(move || {
                tempo_net::cluster::run(
                    &addr.to_string(),
                    &call,
                    |sp| {
                        share.relay(Source::Node, sp);
                        lock(&own).push(sp.dx_call.clone());
                    },
                    &stop,
                    &conn,
                    &outbox,
                    |_| {},
                );
            });
        }
    }
}

impl Hooks for TestHooks {
    fn active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    fn status(&self) -> HolderStatus {
        let conns = lock(&self.feeds_conn);
        HolderStatus {
            rbn_connected: false,
            nodes_running: !conns.is_empty(),
            nodes_connected: conns.iter().any(|c| c.load(Ordering::SeqCst)),
            nodes_label: (!conns.is_empty()).then(|| "test node".to_string()),
        }
    }

    fn post(&self, line: String) {
        lock(&self.outbox).push_back(line);
    }

    fn take(&self, source: Source, spot: &ClusterSpot) {
        lock(&self.relayed).push((source, spot.dx_call.clone()));
    }

    fn next_post(&self) -> Option<String> {
        lock(&self.outbox).pop_front()
    }

    fn promote(&self) {
        self.start_feeds();
    }
}

struct Window {
    share: Share,
    hooks: Arc<TestHooks>,
    claim: Claim,
}

impl Window {
    fn open(dir: &Path, call: &str, nodes: &[&Arc<FakeNode>]) -> Self {
        let hooks = Arc::new(TestHooks {
            call: call.to_string(),
            nodes: nodes.iter().map(|n| n.addr).collect(),
            active: AtomicBool::new(true),
            me: OnceLock::new(),
            feeds_stop: Arc::default(),
            feeds_conn: Mutex::default(),
            outbox: Arc::default(),
            own: Arc::default(),
            relayed: Mutex::default(),
            feeds_started: AtomicUsize::new(0),
        });
        let share = Share::new(dir.to_path_buf(), hooks.clone());
        let _ = hooks.me.set(share.clone());
        // What `start_cluster_feeds` does: the claim decides whether this window's feeds start.
        let claim = share.claim(call);
        if claim == Claim::Hold {
            hooks.start_feeds();
        }
        Self {
            share,
            hooks,
            claim,
        }
    }

    /// The window's process ends.
    fn close(&self) {
        self.share.close();
        self.hooks.feeds_stop.store(true, Ordering::SeqCst);
    }

    /// Queue a spot the operator posted here, as `queue_spot` does.
    fn post(&self, line: &str) {
        lock(&self.hooks.outbox).push_back(line.to_string());
    }

    fn relayed(&self, dx: &str) -> usize {
        lock(&self.hooks.relayed)
            .iter()
            .filter(|(_, d)| d == dx)
            .count()
    }
}

/// A config dir of the test's own.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nexus-cluster-share-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Wait up to `within` for `ok`.
fn until(within: Duration, mut ok: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + within;
    while Instant::now() < end {
        if ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    ok()
}

/// The follower's relay is up and says a node is logged in.
fn following_connected(w: &Window) -> bool {
    w.share
        .following()
        .is_some_and(|(up, s)| up && s.nodes_connected)
}

#[test]
fn two_windows_make_one_login_per_cluster() {
    let dir = scratch("one-login");
    let (n1, n2) = (FakeNode::start(), FakeNode::start());
    let a = Window::open(&dir, "W1AW", &[&n1, &n2]);
    let b = Window::open(&dir, "W1AW", &[&n1, &n2]);
    assert_eq!((a.claim, b.claim), (Claim::Hold, Claim::Follow));
    assert!(until(Duration::from_secs(10), || following_connected(&b)));
    // Long enough for a second window's feeds to have logged in, had they started.
    std::thread::sleep(Duration::from_secs(2));
    for n in [&n1, &n2] {
        assert_eq!(n.logins.load(Ordering::SeqCst), 1, "one login per node");
        assert_eq!(n.max_live.load(Ordering::SeqCst), 1, "never two at once");
    }
    assert_eq!(b.hooks.feeds_started.load(Ordering::SeqCst), 0);
    a.close();
    b.close();
}

#[test]
fn the_follower_receives_the_holders_spots() {
    let dir = scratch("spots");
    let n = FakeNode::start();
    let a = Window::open(&dir, "W1AW", &[&n]);
    let b = Window::open(&dir, "W1AW", &[&n]);
    assert!(until(Duration::from_secs(10), || following_connected(&b)));
    n.spot("K1ABC");
    assert!(
        until(Duration::from_secs(5), || b.relayed("K1ABC") == 1),
        "the follower takes the holder's spot"
    );
    assert_eq!(lock(&b.hooks.relayed)[0].0, Source::Node, "as a node spot");
    assert_eq!(
        lock(&a.hooks.own).as_slice(),
        ["K1ABC"],
        "the holder keeps its own"
    );
    assert_eq!(
        a.relayed("K1ABC"),
        0,
        "the holder takes nothing from a relay"
    );
    a.close();
    b.close();
}

#[test]
fn a_spot_posted_in_the_follower_goes_out_once() {
    let dir = scratch("post");
    let n = FakeNode::start();
    let a = Window::open(&dir, "W1AW", &[&n]);
    let b = Window::open(&dir, "W1AW", &[&n]);
    assert!(until(Duration::from_secs(10), || following_connected(&b)));
    b.post("DX 14025 K1ABC from the second window\r\n");
    let line = "DX 14025 K1ABC from the second window";
    assert!(until(Duration::from_secs(5), || n.heard(line) == 1));
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(n.heard(line), 1, "sent once, through the holder's login");
    assert!(lock(&a.hooks.outbox).is_empty() && lock(&b.hooks.outbox).is_empty());
    a.close();
    b.close();
}

#[test]
fn when_the_holder_closes_the_follower_takes_over_once() {
    let dir = scratch("handover");
    let n = FakeNode::start();
    let a = Window::open(&dir, "W1AW", &[&n]);
    let b = Window::open(&dir, "W1AW", &[&n]);
    assert!(until(Duration::from_secs(10), || following_connected(&b)));
    let closed = Instant::now();
    a.close();
    assert!(
        until(RETRY * 2 + Duration::from_secs(5), || n
            .logins
            .load(Ordering::SeqCst)
            == 2),
        "the follower logs in once the holder is gone"
    );
    let took = closed.elapsed();
    assert!(
        took <= RETRY * 2 + Duration::from_secs(5),
        "bounded: {took:?}"
    );
    assert!(b.share.following().is_none(), "it holds the logins now");
    assert_eq!(b.hooks.feeds_started.load(Ordering::SeqCst), 1);
    // No storm, and never two sessions at once.
    std::thread::sleep(RETRY + Duration::from_secs(1));
    assert_eq!(n.logins.load(Ordering::SeqCst), 2);
    assert_eq!(n.max_live.load(Ordering::SeqCst), 1);
    // A third window follows the new holder.
    let c = Window::open(&dir, "W1AW", &[&n]);
    assert_eq!(c.claim, Claim::Follow);
    assert!(until(Duration::from_secs(10), || following_connected(&c)));
    b.close();
    c.close();
}

#[test]
fn the_follower_says_what_reaches_it() {
    let dir = scratch("status");
    let n = FakeNode::start();
    let a = Window::open(&dir, "W1AW", &[&n]);
    let b = Window::open(&dir, "W1AW", &[&n]);
    assert!(until(Duration::from_secs(10), || following_connected(&b)));
    let (_, s) = b.share.following().unwrap();
    assert_eq!(s.nodes_label.as_deref(), Some("test node"));
    // The node goes away: the holder's status says so, and the follower with it.
    n.kick();
    assert!(
        until(Duration::from_secs(8), || b
            .share
            .following()
            .is_some_and(|(up, s)| up && !s.nodes_connected)),
        "relay up, no node logged in"
    );
    // The holder goes away: the follower no longer claims a relay.
    a.close();
    assert!(until(Duration::from_secs(4), || b
        .share
        .following()
        .is_some_and(|(up, _)| !up)
        || b.share.following().is_none()));
    b.close();
}

#[test]
fn a_single_window_is_unchanged() {
    // CONTROL: alone, a window holds, logs in once, takes its own spots and posts once.
    let dir = scratch("single");
    let n = FakeNode::start();
    let a = Window::open(&dir, "W1AW", &[&n]);
    assert_eq!(a.claim, Claim::Hold);
    assert!(a.share.following().is_none());
    assert!(until(Duration::from_secs(10), || n
        .logins
        .load(Ordering::SeqCst)
        == 1
        && !lock(&n.sessions).is_empty()));
    n.spot("K1ABC");
    assert!(until(Duration::from_secs(5), || lock(&a.hooks.own).len() == 1));
    a.post("DX 7025 K1ABC alone\r\n");
    assert!(until(Duration::from_secs(5), || n
        .heard("DX 7025 K1ABC alone")
        == 1));
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(n.heard("DX 7025 K1ABC alone"), 1);
    assert_eq!(n.logins.load(Ordering::SeqCst), 1);
    a.close();
}

#[test]
fn windows_on_different_calls_each_hold_their_own() {
    let dir = scratch("calls");
    let n = FakeNode::start();
    let a = Window::open(&dir, "W1AW", &[&n]);
    let b = Window::open(&dir, "K1ABC", &[&n]);
    assert_eq!((a.claim, b.claim), (Claim::Hold, Claim::Hold));
    assert!(until(Duration::from_secs(10), || n
        .logins
        .load(Ordering::SeqCst)
        == 2));
    a.close();
    b.close();
}

#[test]
fn a_stranger_without_the_secret_gets_nothing_and_posts_nothing() {
    let dir = scratch("stranger");
    let n = FakeNode::start();
    let a = Window::open(&dir, "W1AW", &[&n]);
    assert!(until(Duration::from_secs(10), || n
        .logins
        .load(Ordering::SeqCst)
        == 1));
    let (port, secret) = read_relay(&dir, "W1AW").expect("the relay file");
    assert_eq!(secret.len(), 64);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(relay_path(&dir, "W1AW"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "owner-only");
    }
    let wrong = "0".repeat(64);
    for hello in [
        format!("HELLO {wrong}\n"),
        "POST DX 14025 K1XX evil\n".to_string(),
    ] {
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(6))).unwrap();
        s.write_all(hello.as_bytes()).unwrap();
        let _ = s.write_all(b"POST DX 14025 K1XX evil\n");
        let mut got = Vec::new();
        let _ = s.read_to_end(&mut got);
        assert!(
            got.is_empty(),
            "no status, no spot: {:?}",
            String::from_utf8_lossy(&got)
        );
    }
    // CONTROL: the right secret is served.
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(format!("HELLO {secret}\n").as_bytes()).unwrap();
    let mut first = String::new();
    BufReader::new(s).read_line(&mut first).unwrap();
    assert!(first.starts_with("S {"), "{first}");
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(n.heard("DX 14025 K1XX evil"), 0);
    a.close();
}

#[test]
fn a_follower_without_a_reachable_relay_knocks_once_per_retry() {
    // Another window holds the lease, but its relay is not answering this window: no storm.
    let dir = scratch("storm");
    let held = try_lease(&dir, "W1AW").expect("the other window's lease");
    let knock = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = knock.local_addr().unwrap().port();
    write_owner_only(
        &relay_path(&dir, "W1AW"),
        &format!("{port} {}\n", "1".repeat(64)),
    )
    .unwrap();
    let knocks = Arc::new(AtomicUsize::new(0));
    let k = knocks.clone();
    std::thread::spawn(move || {
        for s in knock.incoming().flatten() {
            k.fetch_add(1, Ordering::SeqCst);
            drop(s); // refuses at once, as a relay does a wrong secret
        }
    });
    let n = FakeNode::start();
    let b = Window::open(&dir, "W1AW", &[&n]);
    assert_eq!(b.claim, Claim::Follow);
    std::thread::sleep(RETRY * 2 + Duration::from_secs(1));
    let count = knocks.load(Ordering::SeqCst);
    assert!(
        (2..=3).contains(&count),
        "{count} knocks in two retry intervals"
    );
    assert!(
        b.share.following().is_some_and(|(up, _)| !up),
        "and says it is not connected"
    );
    assert_eq!(n.logins.load(Ordering::SeqCst), 0, "and logs in nowhere");
    b.close();
    drop(held);
}

#[test]
fn a_window_that_does_not_want_the_cluster_neither_holds_nor_follows() {
    let dir = scratch("inactive");
    let n = FakeNode::start();
    let a = Window::open(&dir, "W1AW", &[&n]);
    let b = Window::open(&dir, "W1AW", &[&n]);
    b.hooks.active.store(false, Ordering::SeqCst);
    assert!(until(Duration::from_secs(10), || n
        .logins
        .load(Ordering::SeqCst)
        == 1));
    a.close();
    std::thread::sleep(RETRY + Duration::from_secs(2));
    assert_eq!(
        n.logins.load(Ordering::SeqCst),
        1,
        "only the first window ever logged in"
    );
    assert_eq!(b.hooks.feeds_started.load(Ordering::SeqCst), 0);
    b.close();
}

#[test]
fn only_a_single_dx_line_passes_the_relay() {
    assert_eq!(
        checked_post("POST DX 14025 K1ABC tnx\r\n").as_deref(),
        Some("DX 14025 K1ABC tnx\r\n")
    );
    for bad in [
        "POST SET/NAME evil\n",
        "POST DX 14025\n",
        "DX 14025 K1ABC\n",
        "POST DX 14025 K1ABC a\u{7}b\n",
        &format!("POST DX 14025 K1ABC {}\n", "x".repeat(300)),
    ] {
        assert_eq!(checked_post(bad), None, "{bad:?}");
    }
}

#[test]
fn the_lease_key_names_a_file_for_any_call() {
    assert_eq!(lease_key(" kd9taw/p "), "KD9TAW_P");
    assert_eq!(lease_key("W1AW"), "W1AW");
}

#[test]
fn the_follower_pills_claim_only_what_the_relay_proves() {
    let up = HolderStatus {
        rbn_connected: true,
        nodes_running: true,
        nodes_connected: true,
        nodes_label: Some("dx.example.net".into()),
    };
    let now = 1_800_000_000;
    // Relay down: nothing is known, so nothing is connected, whatever was last said.
    let down = crate::followed_feed_health(true, false, &up, 0, 0, 0, now);
    assert!(down.cluster_via_other_window);
    assert_eq!(down.cluster.state, "connecting");
    assert!(down.phone_cluster.enabled, "the Phone pill stays up");
    assert_eq!(down.phone_cluster.state, "connecting");
    assert_eq!(down.phone_cluster_host, None);
    // Relay up, the other window logged in: connected, under its label.
    let live = crate::followed_feed_health(true, true, &up, 0, 0, 0, now);
    assert_eq!(live.cluster.state, "connected");
    assert_eq!(live.phone_cluster.state, "connected");
    assert_eq!(live.phone_cluster_host.as_deref(), Some("dx.example.net"));
    // Relay up, the other window logged into nothing: not connected.
    let none = HolderStatus {
        nodes_running: true,
        ..HolderStatus::default()
    };
    let idle = crate::followed_feed_health(true, true, &none, 0, 0, 0, now);
    assert_eq!(idle.cluster.state, "connecting");
    assert_eq!(idle.phone_cluster.state, "connecting");
    // This window's own cluster switched off (or Unassisted): nothing reaches it, both pills off.
    let off = crate::followed_feed_health(false, true, &up, 0, 0, 0, now);
    assert!(!off.cluster.enabled && !off.phone_cluster.enabled);
    // CONTROL: a window that follows no one reads its own feeds, never "via the other window".
    assert!(
        crate::cluster_following().is_none(),
        "no test starts the desktop's share"
    );
    assert!(!crate::read_feed_health(&std::sync::Arc::default()).cluster_via_other_window);
}
