//! Keep the shack awake while a stream is attached (the operator's pick "Keep awake during
//! streams", 2026-09-28).
//!
//! A streamed operator's clicks and keys reach Nexus's own window as DOM events (security test A2:
//! no OS input call anywhere), so Windows never counts them as input, and its idle timers would turn
//! the shack's display off and put the PC to sleep in the middle of a stream. While at least one
//! stream is attached, the station holds ONE request for the system and the display to stay on;
//! when the last one ends, however it ended, the request goes.
//!
//! - **Counted.** [`KeepAwake::attach`] hands out an [`Attached`] guard: the first one holds the
//!   request, the last one dropped releases it. A second stream is still one request, and one of two
//!   ending keeps it. The guard goes when its stream's state does, whatever ended the stream: the
//!   operator, the "Still there?" prompt, a lapse of presence, the relay, a failure, a panic.
//! - **What it cannot do**, in Microsoft's own words for `SetThreadExecutionState`: it "does not
//!   stop the screen saver from executing", and it "cannot be used to prevent the user from putting
//!   the computer to sleep". A policy-enforced lock still happens (the operator's pick accepts that
//!   cost), and the 15-minute prompt still ends a forgotten stream.
//! - **A failed call is reported, never a panic**: the stream goes on without the request.
//!
//! Power management only: nothing here keys, holds or releases a transmitter.
use std::sync::{Arc, Mutex, PoisonError};

/// What asks the OS. The decision of WHEN is [`KeepAwake`]'s; this only does it.
pub trait Power: Send {
    /// Ask for the system and the display to stay on until [`Self::release`].
    fn hold(&mut self) -> Result<(), String>;
    /// Let them go.
    fn release(&mut self) -> Result<(), String>;
}

/// Where a failed call is reported: the application's log.
pub type Report = Arc<dyn Fn(&str) + Send + Sync>;

/// Nothing to ask: a platform with no stream, a test, a build without a window.
struct Nothing;

impl Power for Nothing {
    fn hold(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn release(&mut self) -> Result<(), String> {
        Ok(())
    }
}

struct State {
    /// Streams attached now.
    attached: usize,
    power: Box<dyn Power>,
}

/// The station's one keep-awake request. Cheap to clone; every clone is the same request.
#[derive(Clone)]
pub struct KeepAwake {
    state: Arc<Mutex<State>>,
    report: Report,
}

impl Default for KeepAwake {
    fn default() -> Self {
        Self::new(Box::new(Nothing), Arc::new(|_: &str| {}))
    }
}

impl KeepAwake {
    pub fn new(power: Box<dyn Power>, report: Report) -> Self {
        Self {
            state: Arc::new(Mutex::new(State { attached: 0, power })),
            report,
        }
    }

    /// The platform's own request: Windows' execution state, for the system and the display.
    /// Everywhere else a station cannot stream, and there is nothing to hold.
    pub fn system(report: Report) -> Self {
        #[cfg(windows)]
        let power: Box<dyn Power> = Box::new(OwnThread::new(set_thread_execution_state));
        #[cfg(not(windows))]
        let power: Box<dyn Power> = Box::new(Nothing);
        Self::new(power, report)
    }

    /// A stream is attached: the request is held until the returned guard is dropped.
    pub fn attach(&self) -> Attached {
        let failed = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.attached += 1;
            if state.attached == 1 {
                state.power.hold().err()
            } else {
                None
            }
        };
        if let Some(why) = failed {
            (self.report)(&format!("keep awake: {why}"));
        }
        Attached {
            awake: self.clone(),
        }
    }

    fn detach(&self) {
        let failed = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.attached = state.attached.saturating_sub(1);
            if state.attached == 0 {
                state.power.release().err()
            } else {
                None
            }
        };
        if let Some(why) = failed {
            (self.report)(&format!("keep awake: {why}"));
        }
    }
}

/// One attached stream. Dropping it detaches that stream, however the stream ended.
#[must_use = "the stream is detached the moment this is dropped"]
pub struct Attached {
    awake: KeepAwake,
}

impl Drop for Attached {
    fn drop(&mut self) {
        self.awake.detach();
    }
}

/// `SetThreadExecutionState`'s flags (winbase.h).
#[cfg(any(windows, test))]
const ES_CONTINUOUS: u32 = 0x8000_0000;
#[cfg(any(windows, test))]
const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
#[cfg(any(windows, test))]
const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;
/// Held: the system and the display, until the next call with `ES_CONTINUOUS`.
#[cfg(any(windows, test))]
const HOLD: u32 = ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED;
/// Released: `ES_CONTINUOUS` with every other flag cleared.
#[cfg(any(windows, test))]
const RELEASE: u32 = ES_CONTINUOUS;

// kernel32, declared here: the `windows` crate this crate already has does not enable
// `Win32_System_Power`, and the operator's rule for tonight is no Cargo change.
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn SetThreadExecutionState(es_flags: u32) -> u32;
}

#[cfg(windows)]
fn set_thread_execution_state(flags: u32) -> u32 {
    // SAFETY: plain flags by value; it changes only the calling thread's execution state.
    unsafe { SetThreadExecutionState(flags) }
}

/// The request, on a thread of its own. Windows keeps an execution state PER THREAD: the call
/// returns "the previous thread execution state", and with `ES_CONTINUOUS` the state "should remain
/// in effect until the next call that uses ES_CONTINUOUS" (Microsoft's documentation, which says
/// nothing of a thread that exits holding one). The streams that attach and detach run on threads
/// that come and go, so the request is made, kept and released on the one thread that lives exactly
/// as long as it: `nexus-keep-awake`, started by the hold and ended by the release. `call` is
/// `SetThreadExecutionState` on Windows and a recorder in the tests, which is how the thread rule
/// is tested on every platform.
#[cfg(any(windows, test))]
struct OwnThread<F> {
    call: F,
    holder: Option<Holder>,
}

#[cfg(any(windows, test))]
struct Holder {
    release: std::sync::mpsc::Sender<()>,
    /// Answers the release call's result: the state the thread held before it let go.
    thread: std::thread::JoinHandle<u32>,
}

#[cfg(any(windows, test))]
impl<F: Fn(u32) -> u32 + Clone + Send + 'static> OwnThread<F> {
    fn new(call: F) -> Self {
        Self { call, holder: None }
    }

    /// Let go, on the thread that holds it. `None` when nothing was held; otherwise the state the
    /// thread held, which is the request as Windows took it.
    fn release_state(&mut self) -> Result<Option<u32>, String> {
        let Some(holder) = self.holder.take() else {
            return Ok(None);
        };
        let _ = holder.release.send(());
        match holder.thread.join() {
            Ok(0) => Err("SetThreadExecutionState refused the release".into()),
            Ok(before) => Ok(Some(before)),
            Err(_) => Err("the keep-awake thread panicked".into()),
        }
    }
}

#[cfg(any(windows, test))]
impl<F: Fn(u32) -> u32 + Clone + Send + 'static> Power for OwnThread<F> {
    fn hold(&mut self) -> Result<(), String> {
        if self.holder.is_some() {
            return Ok(());
        }
        let (release, released) = std::sync::mpsc::channel::<()>();
        let (answer, answered) = std::sync::mpsc::sync_channel::<u32>(1);
        let call = self.call.clone();
        let thread = std::thread::Builder::new()
            .name("nexus-keep-awake".into())
            .spawn(move || {
                let _ = answer.send(call(HOLD));
                // Held until released, or until the handle is gone: a dropped sender is a release too.
                let _ = released.recv();
                call(RELEASE)
            })
            .map_err(|e| format!("the keep-awake thread did not start: {e}"))?;
        let before = answered.recv();
        self.holder = Some(Holder { release, thread });
        match before {
            // A failed call answers NULL. The thread stays, so the release is still made on it.
            Ok(0) => Err("SetThreadExecutionState refused the request".into()),
            Ok(_) => Ok(()),
            Err(_) => Err("the keep-awake thread ended before it asked".into()),
        }
    }

    fn release(&mut self) -> Result<(), String> {
        self.release_state().map(|_| ())
    }
}

/// Nexus closing with a stream still attached: the request goes with it.
#[cfg(any(windows, test))]
impl<F> Drop for OwnThread<F> {
    fn drop(&mut self) {
        if let Some(holder) = self.holder.take() {
            let _ = holder.release.send(());
            let _ = holder.thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// The OS as the tests see it: every call, in order, and a switch that makes the next ones fail.
    #[derive(Clone, Default)]
    struct Recorder {
        calls: Arc<Mutex<Vec<&'static str>>>,
        failing: Arc<AtomicBool>,
    }

    impl Power for Recorder {
        fn hold(&mut self) -> Result<(), String> {
            self.calls.lock().unwrap().push("hold");
            if self.failing.load(Ordering::SeqCst) {
                return Err("the OS refused the request".into());
            }
            Ok(())
        }
        fn release(&mut self) -> Result<(), String> {
            self.calls.lock().unwrap().push("release");
            if self.failing.load(Ordering::SeqCst) {
                return Err("the OS refused the release".into());
            }
            Ok(())
        }
    }

    struct Scene {
        awake: KeepAwake,
        os: Recorder,
        reported: Arc<Mutex<Vec<String>>>,
    }

    fn scene() -> Scene {
        let os = Recorder::default();
        let reported = Arc::new(Mutex::new(Vec::new()));
        let log = reported.clone();
        let awake = KeepAwake::new(
            Box::new(os.clone()),
            Arc::new(move |line: &str| log.lock().unwrap().push(line.to_string())),
        );
        Scene {
            awake,
            os,
            reported,
        }
    }

    impl Scene {
        fn calls(&self) -> Vec<&'static str> {
            self.os.calls.lock().unwrap().clone()
        }
    }

    /// A stream attached holds the request, once; it is not released while the stream is there.
    #[test]
    fn a_stream_attached_holds_the_request() {
        let s = scene();
        let stream = s.awake.attach();
        assert_eq!(s.calls(), ["hold"], "attached, and nothing held");
        drop(stream);
    }

    /// A second stream is still ONE request, and one of the two ending keeps it held.
    #[test]
    fn a_second_stream_is_one_request_and_one_of_two_ending_keeps_it() {
        let s = scene();
        let first = s.awake.attach();
        let second = s.awake.clone().attach();
        assert_eq!(s.calls(), ["hold"], "a second request for a second stream");
        drop(first);
        assert_eq!(s.calls(), ["hold"], "released with a stream still attached");
        drop(second);
        assert_eq!(s.calls(), ["hold", "release"]);
    }

    /// The last stream ending releases the request, and a new stream after it holds it again.
    #[test]
    fn the_last_stream_ending_releases_the_request() {
        let s = scene();
        drop(s.awake.attach());
        assert_eq!(
            s.calls(),
            ["hold", "release"],
            "the last end did not release"
        );
        drop(s.awake.attach());
        assert_eq!(
            s.calls(),
            ["hold", "release", "hold", "release"],
            "a new stream after the last did not hold it again"
        );
    }

    /// Every way a stream ends at the station comes down to its state being dropped: on its own
    /// thread when the session loop returns (the operator, the "Still there?" prompt, a lapse of
    /// presence, the relay's `streamEnd`, a failure, shutdown), or while unwinding from a panic.
    /// Each of those drops the guard, and each releases the request.
    #[test]
    fn every_way_a_stream_ends_releases_the_request() {
        // Returned from its own thread, as the session loop returns on every end.
        let s = scene();
        let guard = s.awake.attach();
        std::thread::spawn(move || drop(guard)).join().unwrap();
        assert_eq!(s.calls(), ["hold", "release"], "a stream's thread ending");
        // Its thread ended with the guard still in scope.
        let s = scene();
        let awake = s.awake.clone();
        std::thread::spawn(move || {
            let _stream = awake.attach();
        })
        .join()
        .unwrap();
        assert_eq!(
            s.calls(),
            ["hold", "release"],
            "a stream's thread returning"
        );
        // A panic on the stream's thread: unwinding drops the guard too.
        let s = scene();
        let awake = s.awake.clone();
        let panicked = std::thread::spawn(move || {
            let _stream = awake.attach();
            panic!("the session thread panicked");
        })
        .join();
        assert!(panicked.is_err(), "premise: the thread panicked");
        assert_eq!(s.calls(), ["hold", "release"], "a panic mid-stream");
    }

    /// A failed call is reported, never a panic, and the count stays right: the release still comes
    /// when the last stream ends, and it is reported too if it fails.
    #[test]
    fn a_failed_call_is_reported_and_never_panics() {
        let s = scene();
        s.os.failing.store(true, Ordering::SeqCst);
        let first = s.awake.attach();
        let second = s.awake.attach();
        assert_eq!(s.calls(), ["hold"], "a failed hold was asked again");
        // Read once, then asserted: a failing assertion must not lock it a second time.
        let reported = s.reported.lock().unwrap().clone();
        assert_eq!(
            reported.len(),
            1,
            "the failed hold was not reported: {reported:?}"
        );
        drop(first);
        drop(second);
        assert_eq!(
            s.calls(),
            ["hold", "release"],
            "no release after a failed hold"
        );
        let reported = s.reported.lock().unwrap().clone();
        assert_eq!(
            reported.len(),
            2,
            "the failed release was not reported: {reported:?}"
        );
        // And it recovers: the OS answering again, a new stream holds it again.
        s.os.failing.store(false, Ordering::SeqCst);
        drop(s.awake.attach());
        assert_eq!(s.calls(), ["hold", "release", "hold", "release"]);
    }

    // ── The request on a thread of its own ─────────────────────────────────────────────────────

    type Calls = Arc<Mutex<Vec<(std::thread::ThreadId, Option<String>, u32)>>>;

    /// SetThreadExecutionState as the tests see it: which thread made each call, with what flags. It
    /// answers the previous state, as Windows does, or NULL when `refuse` is set.
    fn recorder(refuse: bool) -> (impl Fn(u32) -> u32 + Clone + Send + 'static, Calls) {
        let calls: Calls = Arc::default();
        let log = calls.clone();
        let state = Arc::new(Mutex::new(ES_CONTINUOUS));
        let call = move |flags: u32| {
            let me = std::thread::current();
            log.lock()
                .unwrap()
                .push((me.id(), me.name().map(String::from), flags));
            let before = std::mem::replace(&mut *state.lock().unwrap(), flags);
            if refuse {
                0
            } else {
                before
            }
        };
        (call, calls)
    }

    /// ★ The rule Windows makes: the request belongs to the thread that made it. So it is made and
    /// released on ONE thread of its own, never the caller's, even when the release comes from a
    /// different thread than the hold did, as a stream's does; and that thread ends with it.
    #[test]
    fn the_request_is_made_and_released_on_one_thread_of_its_own() {
        let (call, calls) = recorder(false);
        let mut request = OwnThread::new(call);
        request.hold().unwrap();
        request.hold().unwrap();
        let released = std::thread::spawn(move || request.release_state())
            .join()
            .unwrap();
        assert_eq!(released, Ok(Some(HOLD)), "released, but not what it held");
        let calls = calls.lock().unwrap().clone();
        assert_eq!(
            calls.iter().map(|c| c.2).collect::<Vec<_>>(),
            [HOLD, RELEASE],
            "held twice, or not released"
        );
        assert_eq!(
            calls[0].0, calls[1].0,
            "held on one thread and released on another"
        );
        assert_ne!(
            calls[0].0,
            std::thread::current().id(),
            "held on the caller's thread"
        );
        assert_eq!(calls[0].1.as_deref(), Some("nexus-keep-awake"));
    }

    /// A NULL answer is a failure, reported as one, and the release is still made on that thread.
    /// CONTROL: nothing held, nothing to release.
    #[test]
    fn a_refused_request_is_an_error_and_still_released_on_its_thread() {
        let (call, calls) = recorder(true);
        let mut request = OwnThread::new(call);
        assert!(request.hold().is_err(), "a NULL answer read as held");
        assert!(request.release().is_err(), "a NULL answer read as released");
        let calls = calls.lock().unwrap().clone();
        assert_eq!(
            calls.iter().map(|c| c.2).collect::<Vec<_>>(),
            [HOLD, RELEASE]
        );
        assert_eq!(calls[0].0, calls[1].0);
        assert_eq!(request.release_state(), Ok(None));
    }

    /// Dropped while held (Nexus closing with a stream attached): released on its thread.
    #[test]
    fn a_request_dropped_while_held_is_released() {
        let (call, calls) = recorder(false);
        let mut request = OwnThread::new(call);
        request.hold().unwrap();
        drop(request);
        let calls = calls.lock().unwrap().clone();
        assert_eq!(
            calls.iter().map(|c| c.2).collect::<Vec<_>>(),
            [HOLD, RELEASE]
        );
    }

    /// ★ On Windows itself (WSL interop): the real request. Released, the holder thread's previous
    /// state is exactly what it asked for, so Windows took the hold on that thread.
    #[cfg(windows)]
    #[test]
    fn windows_takes_the_request_on_its_own_thread_and_releases_it_there() {
        let mut request = OwnThread::new(set_thread_execution_state);
        request
            .hold()
            .expect("SetThreadExecutionState refused the hold");
        assert_eq!(request.release_state(), Ok(Some(HOLD)));
        assert_eq!(request.release_state(), Ok(None), "control: nothing held");
        // And through the station's own entry point: attached and detached, nothing reported.
        let reported = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = reported.clone();
        let awake = KeepAwake::system(Arc::new(move |line: &str| {
            log.lock().unwrap().push(line.to_string())
        }));
        drop(awake.attach());
        let reported = reported.lock().unwrap().clone();
        assert!(reported.is_empty(), "{reported:?}");
    }
}
