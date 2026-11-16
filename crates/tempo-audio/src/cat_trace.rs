//! Record CAT traffic: an operator-started, self-ending trace of the radio-control commands Nexus
//! sends, and what the radio answered, written into the diagnostic log.
//!
//! It exists for the fault nobody can see from the log alone: a radio that clicks its relays on
//! a band change, where the question is WHICH commands went out, in what order, how far apart.
//!
//! ## Where it listens, and what that covers
//! Every single-line command Nexus sends a radio passes [`crate::rig::Rig::command_lines`],
//! whatever serves the other end of that link: Hamlib's rigctld, or Nexus's own rigctld-protocol
//! shims for Icom CI-V, OmniRig, FlexRadio and TCI. The trace is taken there, once the exchange
//! is over, so it sees every backend's commands in the one protocol they share. What it does NOT
//! see is the translation below that protocol: the bytes rigctld writes to the radio's serial
//! port, or the CI-V frames and TCI/Flex text the shims write (the CI-V bus has its own log).
//!
//! ## Frequencies and modes only
//! A command is written only when its verb is on the list below AND every argument is a number or
//! a short word (a mode or VFO name): frequency, mode, split, VFO and PTT, set or read. Every
//! other SET (levels, functions, raw CAT, Morse text, voice memories) is written as a bare
//! "other command" with nothing of its content, so the timeline still shows that something went
//! out; every other READ (meters, levels, capability dumps) is not written at all. A reply is
//! written only when each of its lines is a number, a short word or an `RPRT` code. A failed
//! exchange is written as its error KIND only, never the error's text, which can quote bytes.
//!
//! ## No behaviour change
//! The hook runs after the command was written AND its reply read, so nothing is added between
//! a command and the wire, or between a write and its read. Off, it costs one relaxed atomic
//! load. On, it formats one line and hands it to the diagnostic log's writer thread
//! ([`tempo_core::applog`]), which never blocks: a full queue drops the line and the log says
//! it did. Nothing here sends anything to the radio.
//!
//! ## Bounded, and never left running
//! - [`AUTO_OFF`]: the trace turns itself off 30 minutes after it was started, and says so.
//! - [`BYTE_CAP`]: it stops at 2 MB of trace lines, and says so. Recording STOPS at the cap
//!   rather than dropping the oldest lines: the diagnostic log is append-only and is never
//!   rewritten (it rotates by rename), and the start of a trace (the band change that was
//!   reproduced) is the part worth keeping.
//! - Off at every launch: the switch lives only in this process's memory and is not a setting,
//!   so a trace left on can never come back on after a restart.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a trace runs before it turns itself off. Long enough to set up, reproduce a fault a
/// few times and change bands back and forth; short enough that a trace forgotten at the end of
/// a session is over within half an hour.
pub const AUTO_OFF: Duration = Duration::from_secs(30 * 60);

/// The most trace bytes one session writes. The radio loop's polling at its fastest is about
/// 1 KB a second of trace, so 30 minutes fits; and it is half the diagnostic log's own file cap
/// (4 MB, two generations), so a whole trace always survives one rotation.
pub const BYTE_CAP: u64 = 2 * 1024 * 1024;

/// What the diagnostic log adds to each line: its `YYYY-MM-DD HH:MM:SSZ  INFO   ` prefix, the
/// `cat-trace: ` event name and the newline. Counted so the cap is the bytes on disk.
const LINE_OVERHEAD: u64 = 42;

/// Whether a trace is running: the one load the CAT path pays when it is not.
static ON: AtomicBool = AtomicBool::new(false);
/// The running trace, present only while [`ON`].
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// Lines written while testing, for the tests to read back (the diagnostic log is inert there).
#[cfg(test)]
pub(crate) static CAPTURED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Held by every test that starts or stops the trace: there is one trace per process.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

struct Session {
    started: Instant,
    bytes: u64,
}

/// What a running session does with a line of `len` bytes at `now`.
#[derive(Debug, PartialEq, Eq)]
enum Admit {
    Write,
    /// [`AUTO_OFF`] has passed: the session ends, and says so.
    TimeUp,
    /// The line would take it past [`BYTE_CAP`]: the session ends, and says so.
    Full,
}

impl Session {
    fn admit(&mut self, now: Instant, len: usize) -> Admit {
        if now.saturating_duration_since(self.started) >= AUTO_OFF {
            return Admit::TimeUp;
        }
        let len = len as u64 + LINE_OVERHEAD;
        if self.bytes + len > BYTE_CAP {
            return Admit::Full;
        }
        self.bytes += len;
        Admit::Write
    }

    fn minutes(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.started).as_secs() / 60
    }
}

fn emit(text: &str) {
    tempo_core::applog::info("cat-trace", text);
    #[cfg(test)]
    CAPTURED.lock().unwrap().push(text.to_string());
}

/// Whether a trace is running. Callers check this before building anything to record.
pub fn is_on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Start a trace, or restart the one running with a fresh time limit and cap. Returns how long
/// it will run.
pub fn start() -> Duration {
    let mut session = SESSION.lock().unwrap_or_else(|p| p.into_inner());
    *session = Some(Session {
        started: Instant::now(),
        bytes: 0,
    });
    emit(&format!(
        "started: frequency, mode, split, VFO and PTT commands are recorded for {} min or {} MB, \
         whichever comes first",
        AUTO_OFF.as_secs() / 60,
        BYTE_CAP / (1024 * 1024)
    ));
    ON.store(true, Ordering::Relaxed);
    AUTO_OFF
}

/// Stop the trace, if one is running.
pub fn stop() {
    let mut session = SESSION.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = session.take() {
        ON.store(false, Ordering::Relaxed);
        emit(&format!(
            "stopped by the operator after {} min ({} KB)",
            s.minutes(Instant::now()),
            s.bytes / 1024
        ));
    }
}

/// How long the running trace has left, or `None` when none is running. A trace whose time is
/// up ends here if no command ended it first.
pub fn time_left() -> Option<Duration> {
    let mut session = SESSION.lock().unwrap_or_else(|p| p.into_inner());
    let now = Instant::now();
    let left = AUTO_OFF.checked_sub(now.saturating_duration_since(session.as_ref()?.started));
    match left {
        Some(left) if !left.is_zero() => Some(left),
        _ => {
            end(&mut session, Admit::TimeUp, now);
            None
        }
    }
}

fn end(session: &mut Option<Session>, why: Admit, now: Instant) {
    let Some(s) = session.take() else {
        return;
    };
    ON.store(false, Ordering::Relaxed);
    match why {
        Admit::TimeUp => emit(&format!(
            "stopped by itself: the {} min limit was reached",
            AUTO_OFF.as_secs() / 60
        )),
        Admit::Full => emit(&format!(
            "stopped by itself: the {} MB cap was reached after {} min",
            BYTE_CAP / (1024 * 1024),
            s.minutes(now)
        )),
        Admit::Write => {}
    }
}

/// Write `text` (built only once a session has it) to the running trace.
fn record(text: impl FnOnce(&Session) -> String) {
    let mut session = SESSION.lock().unwrap_or_else(|p| p.into_inner());
    let now = Instant::now();
    let Some(s) = session.as_mut() else {
        return;
    };
    let text = text(s);
    match s.admit(now, text.len()) {
        Admit::Write => emit(&text),
        why => end(&mut session, why, now),
    }
}

/// One command exchange on the link to `control` (a rigctld address): `line` as written, when it
/// was `sent`, and the reply or the kind of failure. Called only after the exchange is over.
/// Only the address's port is written, which tells two radios apart and says nothing else.
pub(crate) fn exchange(
    control: Option<&str>,
    line: &str,
    sent: Instant,
    reply: Result<&str, std::io::ErrorKind>,
) {
    let Some(command) = describe_command(line) else {
        return;
    };
    let took = sent.elapsed();
    let answer = match (&command, reply) {
        (Command::Other, Ok(_)) => "answered".to_string(),
        (_, Ok(reply)) => describe_reply(reply),
        (_, Err(kind)) => format!("failed ({kind:?})"),
    };
    let command = match command {
        Command::Recorded(c) => c,
        Command::Other => "other command (not recorded)".to_string(),
    };
    let port = control
        .and_then(|c| c.rsplit_once(':'))
        .map_or("", |(_, port)| port);
    record(|s| {
        format!(
            "+{:.3}s :{port} {command} -> {answer} ({} ms)",
            sent.saturating_duration_since(s.started).as_secs_f64(),
            took.as_millis()
        )
    });
}

/// A keying edge that is not a CAT command (a serial control line, VOX): the PTT event.
pub(crate) fn ptt_event(on: bool, via: &'static str) {
    record(|s| {
        format!(
            "+{:.3}s PTT {} via {via}",
            s.started.elapsed().as_secs_f64(),
            if on { "on" } else { "off" }
        )
    });
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    /// The command as written: a radio-control verb with radio-control arguments.
    Recorded(String),
    /// Some other set: that one went out, and nothing of what it said.
    Other,
}

/// The rigctld verbs the trace records, short form and long, set and read.
#[rustfmt::skip]
const RECORDED_VERBS: &[&str] = &[
    "F", "f", "\\set_freq", "\\get_freq",
    "M", "m", "\\set_mode", "\\get_mode",
    "I", "i", "\\set_split_freq", "\\get_split_freq",
    "X", "x", "\\set_split_mode", "\\get_split_mode",
    "S", "s", "\\set_split_vfo", "\\get_split_vfo",
    "V", "v", "\\set_vfo", "\\get_vfo",
    "T", "t", "\\set_ptt", "\\get_ptt",
];

/// A number, or a short word such as a mode or VFO name: nothing else is ever written.
fn plain_token(t: &str) -> bool {
    let number = {
        let digits = t.strip_prefix('-').unwrap_or(t);
        !digits.is_empty()
            && digits.len() <= 12
            && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
    };
    let word = !t.is_empty() && t.len() <= 8 && t.chars().all(|c| c.is_ascii_alphanumeric());
    number || word
}

/// What the trace writes for `line`, or `None` for a read it does not record.
fn describe_command(line: &str) -> Option<Command> {
    let mut words = line.split_whitespace();
    let verb = words.next()?;
    if RECORDED_VERBS.contains(&verb) {
        let args: Vec<&str> = words.collect();
        if args.len() <= 3 && args.iter().all(|a| plain_token(a)) {
            return Some(Command::Recorded(line.trim().to_string()));
        }
        return Some(Command::Recorded(format!(
            "{verb} (arguments not recorded)"
        )));
    }
    // Any other set, raw CAT (`w`/`W`, which can carry a keyer's text) and Morse (`b`): that it
    // went out, never what it said. Any other read is not written.
    let is_set = verb.starts_with(|c: char| c.is_ascii_uppercase())
        || verb == "w"
        || verb == "b"
        || verb.starts_with("\\set_")
        || verb.starts_with("\\send_")
        || verb.starts_with("\\stop_");
    is_set.then_some(Command::Other)
}

/// The reply as written: its lines when each is a number, a short word or an `RPRT` code.
fn describe_reply(reply: &str) -> String {
    let lines: Vec<&str> = reply
        .split(['\n', '\0'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return "(empty)".to_string();
    }
    let plain = |l: &&str| match l.strip_prefix("RPRT ") {
        Some(code) => plain_token(code),
        None => plain_token(l),
    };
    if lines.len() <= 4 && lines.iter().all(plain) {
        lines.join(" / ")
    } else {
        "(reply not recorded)".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radio_control_commands_are_written_as_sent() {
        for line in [
            "F 14074000\n",
            "f\n",
            "M PKTUSB 3000\n",
            "S 1 VFOB\n",
            "I 14076000\n",
            "X USB 2400\n",
            "V VFOA\n",
            "T 1\n",
            "T 3\n",
            "T 0\n",
            "t\n",
        ] {
            assert_eq!(
                describe_command(line),
                Some(Command::Recorded(line.trim().to_string())),
                "{line:?}"
            );
        }
    }

    #[test]
    fn nothing_but_radio_control_is_ever_written() {
        // Morse and raw CAT carry text (a callsign, a keyer's message): only that one went out.
        for line in [
            "b CQ W1AW W1AW\n",
            "w KY CQ TEST K1ABC;\n",
            "W KY W9ABC; 0\n",
            "L RFPOWER 0.5\n",
            "U TUNER 1\n",
            "\\send_voice_mem 1\n",
            "\\send_morse CQ\n",
            "\\stop_morse\n",
        ] {
            assert_eq!(describe_command(line), Some(Command::Other), "{line:?}");
        }
        // Other reads are not written at all.
        for line in ["l STRENGTH\n", "u NB\n", "\\dump_state\n", "\\chk_vfo\n"] {
            assert_eq!(describe_command(line), None, "{line:?}");
        }
        // A recorded verb carrying anything but numbers and short words is written bare.
        assert_eq!(
            describe_command("F hello, this is W1AW\n"),
            Some(Command::Recorded("F (arguments not recorded)".into()))
        );
        assert_eq!(
            describe_command("M USB 2400 extra words here\n"),
            Some(Command::Recorded("M (arguments not recorded)".into()))
        );
    }

    #[test]
    fn replies_are_written_only_when_plain() {
        assert_eq!(describe_reply("RPRT 0\n"), "RPRT 0");
        assert_eq!(describe_reply("RPRT -5\n"), "RPRT -5");
        assert_eq!(describe_reply("14074000\n"), "14074000");
        assert_eq!(describe_reply("PKTUSB\n3000\n"), "PKTUSB / 3000");
        assert_eq!(describe_reply("FA014074000;\0"), "(reply not recorded)");
        assert_eq!(describe_reply("CQ CQ de W1AW\n"), "(reply not recorded)");
        assert_eq!(describe_reply(""), "(empty)");
    }

    #[test]
    fn a_session_ends_at_its_time_limit() {
        let t0 = Instant::now();
        let mut s = Session {
            started: t0,
            bytes: 0,
        };
        assert_eq!(
            s.admit(t0 + AUTO_OFF - Duration::from_secs(1), 40),
            Admit::Write
        );
        assert_eq!(s.admit(t0 + AUTO_OFF, 40), Admit::TimeUp);
    }

    #[test]
    fn a_session_ends_at_its_byte_cap() {
        let t0 = Instant::now();
        let mut s = Session {
            started: t0,
            bytes: 0,
        };
        let line = 100usize;
        let fits = BYTE_CAP / (line as u64 + LINE_OVERHEAD);
        for _ in 0..fits {
            assert_eq!(s.admit(t0, line), Admit::Write);
        }
        assert_eq!(s.admit(t0, line), Admit::Full);
        assert!(s.bytes <= BYTE_CAP);
    }

    #[test]
    fn the_trace_starts_off_records_while_on_and_stops() {
        let _one = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        stop();
        assert!(!is_on());
        assert_eq!(time_left(), None);
        let sent = Instant::now();
        exchange(Some("127.0.0.1:4532"), "F 7074123\n", sent, Ok("RPRT 0\n"));
        let mine = |c: &Vec<String>| c.iter().filter(|l| l.contains("F 7074123")).count();
        assert_eq!(mine(&CAPTURED.lock().unwrap()), 0, "recorded while off");

        assert_eq!(start(), AUTO_OFF);
        assert!(is_on());
        assert!(time_left().is_some_and(|l| l <= AUTO_OFF));
        exchange(Some("127.0.0.1:4532"), "F 7074123\n", sent, Ok("RPRT 0\n"));
        exchange(
            Some("127.0.0.1:4532"),
            "T 1\n",
            sent,
            Err(std::io::ErrorKind::TimedOut),
        );
        ptt_event(true, "serial line");
        stop();
        assert!(!is_on());
        let captured = CAPTURED.lock().unwrap();
        assert_eq!(mine(&captured), 1);
        assert!(captured
            .iter()
            .any(|l| l.contains(":4532 F 7074123 -> RPRT 0 (")));
        assert!(captured
            .iter()
            .any(|l| l.contains(":4532 T 1 -> failed (TimedOut)")));
        assert!(captured
            .iter()
            .any(|l| l.contains("PTT on via serial line")));
        assert!(captured
            .iter()
            .any(|l| l.contains("stopped by the operator")));
    }
}
