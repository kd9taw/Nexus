//! The rotator link watch: what the diagnostic log says about a rotctld that is slow or stops
//! answering.
//!
//! ⭐ **The diagnostic log said nothing about a rotator at all** (2026-11-03, a tester's GS-232A
//! and GS-232B rotators "losing connection" over the internet). A missed poll, a deadline, a
//! refused connection and a failed move were logged nowhere; an External rotctld left no line in
//! any file. So nothing a tester could send could tell a slow link from a dead controller, or
//! Nexus's own deadline from Hamlib's.
//!
//! Every exchange with a rotctld ([`super::exchange`], which every command goes through) is noted
//! here, and three kinds of line reach the log:
//! - **The edges**: "stopped answering" (why, after how many ms) and "answering again" (after how
//!   many missed commands, over how long). Edge-triggered, never once per poll, and at most one
//!   "stopped answering" per [`EDGE_EVERY`]: a link that drops out every few seconds is counted
//!   in the summary instead of filling the file.
//! - **A summary every [`WINDOW`] while the link is slow or missing**: reads and moves, misses,
//!   the latency spread. Nothing at all while it is healthy. The spread is what tells the causes
//!   apart: failures piled at the deadline are Nexus giving up, `RPRT -5` answers are Hamlib's
//!   serial timeout, refused connections are a daemon that is not running.
//!
//! The rules are a pure struct ([`LinkWatch`]) with the clock passed in, so they are tested; the
//! process-wide table and the log calls are the thin part.
//!
//! The log is built to be sent to a stranger, so an address that is not on this computer is
//! written as "a remote rotctld" and its port: where the operator's rotator lives is theirs.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a summary window runs.
pub(crate) const WINDOW: Duration = Duration::from_secs(60);
/// At most one "stopped answering" line per link this often.
pub(crate) const EDGE_EVERY: Duration = Duration::from_secs(60);
/// An answer slower than this makes a window worth summarising even with no miss: a link this
/// slow is one round trip from Nexus's own deadlines.
const SLOW_MS: u32 = 1_000;

/// What one exchange came to, for the link — not for the caller, which has its own answer.
/// `Err` carries why, in the log's words.
pub(crate) type Heard = Result<(), String>;

/// Classify one exchange's result. A refusal is an ANSWER (the link works; the rotator said
/// no), except Hamlib's timeout and I/O codes, which are rotctld saying the controller behind
/// it did not answer.
pub(crate) fn heard(result: &std::io::Result<String>, connected: bool, deadline_ms: u64) -> Heard {
    use std::io::ErrorKind;
    match result {
        Ok(text) => {
            let code = text
                .lines()
                .map(str::trim)
                .find(|l| l.starts_with("RPRT"))
                .and_then(super::rprt_code);
            match code {
                Some(-5) => {
                    Err("rotctld answered RPRT -5: the controller did not answer it".into())
                }
                Some(-6) => Err("rotctld answered RPRT -6: an I/O error on its port".into()),
                _ => Ok(()),
            }
        }
        Err(e) if !connected => Err(match e.kind() {
            ErrorKind::ConnectionRefused => "connection refused (is rotctld running?)".into(),
            ErrorKind::TimedOut => "the connection timed out".into(),
            _ => format!("could not connect ({})", e.kind()),
        }),
        Err(e) => Err(match e.kind() {
            ErrorKind::TimedOut | ErrorKind::WouldBlock => {
                format!("no answer within {deadline_ms} ms")
            }
            ErrorKind::UnexpectedEof => "rotctld closed the connection unanswered".into(),
            k => format!("{k}"),
        }),
    }
}

/// One link's record.
#[derive(Debug, Default)]
pub(crate) struct LinkWatch {
    /// Since when it has not answered, and how many commands it has missed: `None` while it
    /// answers.
    down: Option<(Instant, u32)>,
    /// Whether this episode's "stopped answering" was written (so "answering again" is too).
    down_said: bool,
    /// When a "stopped answering" was last written.
    last_edge: Option<Instant>,
    window: Option<Instant>,
    reads: u32,
    read_misses: u32,
    moves: u32,
    move_misses: u32,
    /// Episodes this window, said or not.
    drops: u32,
    latency_ms: Vec<u32>,
}

impl LinkWatch {
    /// Note one exchange. `mover` is a command that moves something (`P`, `S`); everything else
    /// is a read. Returns the lines to log, in order.
    pub(crate) fn observe(
        &mut self,
        now: Instant,
        heard: &Heard,
        mover: bool,
        elapsed: Duration,
        label: &str,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        // A finished window is summarised before this exchange joins the next one.
        if let Some(start) = self.window {
            if now.duration_since(start) >= WINDOW {
                if let Some(s) = self.summary(label) {
                    lines.push(s);
                }
                self.reset_window(now);
            }
        } else {
            self.reset_window(now);
        }
        let ms = u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX);
        self.latency_ms.push(ms);
        if mover {
            self.moves += 1;
        } else {
            self.reads += 1;
        }
        match (heard, self.down) {
            (Ok(()), None) => {}
            (Ok(()), Some((since, missed))) => {
                if self.down_said {
                    lines.push(format!(
                        "{label} is answering again, after {missed} missed command{} over {} s",
                        if missed == 1 { "" } else { "s" },
                        now.duration_since(since).as_secs()
                    ));
                }
                self.down = None;
                self.down_said = false;
            }
            (Err(why), down) => {
                if mover {
                    self.move_misses += 1;
                } else {
                    self.read_misses += 1;
                }
                match down {
                    Some((since, missed)) => self.down = Some((since, missed + 1)),
                    None => {
                        self.down = Some((now, 1));
                        self.drops += 1;
                        let quiet = self
                            .last_edge
                            .is_none_or(|t| now.duration_since(t) >= EDGE_EVERY);
                        self.down_said = quiet;
                        if quiet {
                            self.last_edge = Some(now);
                            lines.push(format!("{label} stopped answering: {why}, after {ms} ms"));
                        }
                    }
                }
            }
        }
        lines
    }

    fn reset_window(&mut self, now: Instant) {
        self.window = Some(now);
        self.reads = 0;
        self.read_misses = 0;
        self.moves = 0;
        self.move_misses = 0;
        self.drops = 0;
        self.latency_ms.clear();
    }

    /// The window's summary, or `None` when it was healthy: every command answered, none slow.
    fn summary(&self, label: &str) -> Option<String> {
        let mut lat = self.latency_ms.clone();
        if lat.is_empty() {
            return None;
        }
        lat.sort_unstable();
        let at = |q: usize| lat[(lat.len() * q / 100).min(lat.len() - 1)];
        let (p50, p95, max) = (at(50), at(95), lat[lat.len() - 1]);
        let missed = self.read_misses + self.move_misses;
        if missed == 0 && p95 < SLOW_MS {
            return None;
        }
        Some(format!(
            "{label} over the last {} s: {} reads, {} missed; {} moves, {} failed; {} drop-out{}; \
             answer time p50 {p50} ms, p95 {p95} ms, max {max} ms",
            WINDOW.as_secs(),
            self.reads,
            self.read_misses,
            self.moves,
            self.move_misses,
            self.drops,
            if self.drops == 1 { "" } else { "s" },
        ))
    }
}

/// How a rotctld address is written in the log: as it is when it is on this computer, and as a
/// remote rotctld and its port when it is not (see the module header).
pub(crate) fn label(addr: &str) -> String {
    let (host, port) = addr.rsplit_once(':').unwrap_or((addr, ""));
    if is_local(host) {
        format!("the rotator (rotctld {addr})")
    } else {
        format!("the rotator (a remote rotctld, port {port})")
    }
}

/// Is `host` (an address's host part, IPv6 brackets allowed) this computer?
pub(crate) fn is_local(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// `text` as the diagnostic log may carry it when it names the rotator's Port box `port`: a serial
/// device (`COM5`, `/dev/ttyUSB0`) stays as it is, and so does a network address on this computer;
/// a network address off it (a serial server or a controller's own network port, `host:port`) has
/// its host replaced, keeping the port number. Every place the text carries the host is masked,
/// Hamlib's own words included.
pub(crate) fn mask_port(text: &str, port: &str) -> String {
    let port = port.trim();
    let Some((host, number)) = port.rsplit_once(':') else {
        return text.to_string();
    };
    if port.starts_with('/') || host.is_empty() || is_local(host) {
        return text.to_string();
    }
    text.replace(port, &format!("a network address:{number}"))
        .replace(host, "a network address")
}

static LINKS: Mutex<Option<HashMap<String, LinkWatch>>> = Mutex::new(None);

/// Note one exchange with the rotctld at `addr` and log what it changed.
pub(crate) fn note(addr: &str, line: &str, heard: Heard, elapsed: Duration) {
    let mover = matches!(line.trim_start().chars().next(), Some('P' | 'S'));
    let label = label(addr);
    let lines = {
        let Ok(mut links) = LINKS.lock() else { return };
        links
            .get_or_insert_with(HashMap::new)
            .entry(addr.to_string())
            .or_default()
            .observe(Instant::now(), &heard, mover, elapsed, &label)
    };
    for l in lines {
        tempo_core::applog::info("rotator", &l);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: &str = "the rotator";

    fn ok() -> Heard {
        Ok(())
    }

    fn miss() -> Heard {
        Err("no answer within 3000 ms".into())
    }

    #[test]
    fn a_healthy_link_logs_nothing_at_all() {
        let mut w = LinkWatch::default();
        let t0 = Instant::now();
        for i in 0..200 {
            let at = t0 + Duration::from_secs(i);
            let lines = w.observe(at, &ok(), i % 10 == 0, Duration::from_millis(80), L);
            assert!(lines.is_empty(), "second {i}: {lines:?}");
        }
    }

    #[test]
    fn a_drop_out_is_one_line_down_and_one_line_back_not_one_per_poll() {
        let mut w = LinkWatch::default();
        let t0 = Instant::now();
        assert!(w
            .observe(t0, &ok(), false, Duration::from_millis(90), L)
            .is_empty());
        let down = w.observe(
            t0 + Duration::from_secs(2),
            &miss(),
            false,
            Duration::from_millis(3000),
            L,
        );
        assert_eq!(
            down,
            ["the rotator stopped answering: no answer within 3000 ms, after 3000 ms"]
        );
        // Still down: nothing more, however many polls miss.
        for s in [4, 6, 8] {
            let at = t0 + Duration::from_secs(s);
            assert!(w
                .observe(at, &miss(), false, Duration::from_millis(3000), L)
                .is_empty());
        }
        let back = w.observe(
            t0 + Duration::from_secs(10),
            &ok(),
            false,
            Duration::from_millis(120),
            L,
        );
        assert_eq!(
            back,
            ["the rotator is answering again, after 4 missed commands over 8 s"]
        );
        // …and healthy again means quiet again.
        assert!(w
            .observe(
                t0 + Duration::from_secs(12),
                &ok(),
                false,
                Duration::from_millis(90),
                L
            )
            .is_empty());
    }

    #[test]
    fn a_link_that_flaps_says_so_once_a_minute_and_counts_the_rest() {
        let mut w = LinkWatch::default();
        let t0 = Instant::now();
        let mut said = 0;
        // A drop-out every 4 s for 50 s: one miss, one answer.
        for i in 0..25u64 {
            let at = t0 + Duration::from_secs(i * 2);
            let heard = if i % 2 == 0 { miss() } else { ok() };
            said += w
                .observe(at, &heard, false, Duration::from_millis(500), L)
                .len();
        }
        // One "stopped" and its "again": the other eleven episodes are rate-limited.
        assert_eq!(said, 2);
        // The minute ends: the summary counts every one of them.
        let lines = w.observe(
            t0 + Duration::from_secs(61),
            &ok(),
            false,
            Duration::from_millis(500),
            L,
        );
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("13 missed"), "{lines:?}");
        assert!(lines[0].contains("13 drop-outs"), "{lines:?}");
    }

    #[test]
    fn a_slow_minute_is_summarised_with_its_latency_spread_and_a_fast_one_is_not() {
        let mut w = LinkWatch::default();
        let t0 = Instant::now();
        for i in 0..30u64 {
            let ms = if i % 10 == 9 { 1_700 } else { 900 + i };
            let at = t0 + Duration::from_secs(i * 2);
            assert!(w
                .observe(at, &ok(), i == 5, Duration::from_millis(ms), L)
                .is_empty());
        }
        let lines = w.observe(
            t0 + Duration::from_secs(60),
            &ok(),
            false,
            Duration::from_millis(100),
            L,
        );
        assert_eq!(
            lines,
            ["the rotator over the last 60 s: 29 reads, 0 missed; 1 moves, 0 failed; 0 drop-outs; \
              answer time p50 916 ms, p95 1700 ms, max 1700 ms"]
        );
        // The next minute is quick: no summary.
        for i in 1..30u64 {
            let at = t0 + Duration::from_secs(60 + i * 2);
            assert!(w
                .observe(at, &ok(), false, Duration::from_millis(80), L)
                .is_empty());
        }
        assert!(w
            .observe(
                t0 + Duration::from_secs(125),
                &ok(),
                false,
                Duration::from_millis(80),
                L
            )
            .is_empty());
    }

    #[test]
    fn hamlibs_own_timeout_is_a_miss_and_a_refusal_is_an_answer() {
        let rprt = |s: &str| heard(&Ok(s.to_string()), true, 3000);
        assert!(rprt("RPRT 0\n").is_ok());
        assert!(rprt("123.0\n45.0\n").is_ok());
        assert!(
            rprt("RPRT -1\n").is_ok(),
            "the rotator answered: a bearing it will not take"
        );
        assert!(rprt("RPRT -11\n").is_ok());
        assert!(rprt("RPRT -5\n").unwrap_err().contains("RPRT -5"));
        assert!(rprt("RPRT -6\n").unwrap_err().contains("RPRT -6"));
        let refused = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
        assert!(heard(&Err(refused), false, 3000)
            .unwrap_err()
            .contains("refused"));
        let late = std::io::Error::from(std::io::ErrorKind::TimedOut);
        assert_eq!(
            heard(&Err(late), true, 3000).unwrap_err(),
            "no answer within 3000 ms"
        );
    }

    #[test]
    fn a_port_box_holding_a_network_address_is_masked_and_a_serial_port_is_not() {
        let line = "rotctld launched (model 603 on 203.0.113.9:4001 @ 9600, :4533)";
        let masked = mask_port(line, "203.0.113.9:4001");
        assert_eq!(
            masked,
            "rotctld launched (model 603 on a network address:4001 @ 9600, :4533)"
        );
        // Hamlib's own words name the host alone too.
        assert_eq!(
            mask_port("network_open: 203.0.113.9 refused", "203.0.113.9:4001"),
            "network_open: a network address refused"
        );
        // The controls: a serial device, a Mac device path, and this computer stay as they are.
        for port in [
            "COM5",
            "/dev/ttyUSB0",
            "/dev/cu.usbmodem13201",
            "127.0.0.1:4001",
            "localhost:4001",
            "",
        ] {
            let line = format!("rotctld launched (model 603 on {port} @ 9600, :4533)");
            assert_eq!(mask_port(&line, port), line, "{port}");
        }
    }

    #[test]
    fn an_address_off_this_computer_is_written_without_its_host() {
        assert_eq!(
            label("127.0.0.1:4533"),
            "the rotator (rotctld 127.0.0.1:4533)"
        );
        assert_eq!(
            label("localhost:4533"),
            "the rotator (rotctld localhost:4533)"
        );
        assert_eq!(label("[::1]:4533"), "the rotator (rotctld [::1]:4533)");
        let far = label("203.0.113.9:4533");
        assert_eq!(far, "the rotator (a remote rotctld, port 4533)");
        assert!(!label("shack.example.net:4533").contains("example"));
    }
}
