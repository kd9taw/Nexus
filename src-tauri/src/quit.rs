//! The quit, when the logbook still has changes on their way to disk — SPEC-1 v3's **C10**.
//!
//! # What an operator used to get, and what they get now
//!
//! Closing the main window took it off the screen at once. The process then ran
//! [`crate::quit_cleanup`] with no window: take the transmitter off the air, give the logbook up
//! to ten seconds, and exit whatever state the log was in. A big log on a slow disk meant a
//! process lingering where nobody could see it, and anything still unsaved at ten seconds was
//! lost with nothing said except a line in the diagnostic log.
//!
//! Now, when the logbook has something on its way ([`tempo_app::logstore::Unsaved`]), the close
//! is HELD. The window stays; [`save_the_logbook`] waits for the changes and tells the window how
//! many are left (`logbook-saving`, which the UI shows only after 300 ms, so a quick quit stays a
//! quick quit); and it asks the operator only when it cannot finish on its own — after a minute
//! ([`QUIT_PATIENCE`]), or when the logbook refused a change (`logbook-save-failed`: **Keep
//! trying** / **Quit without the last N changes**, the answer written to the diagnostic log).
//! Then the window is closed again, for real, and the quit carries on exactly as it always did
//! — `quit_cleanup`'s own flush then finds nothing left to write.
//!
//! **Nothing on its way → the close is not held, and nothing about it changes.**
//!
//! # ⛔ TRANSMIT SAFETY — the order, and why it is this order
//!
//! A quit's first act is taking the transmitter off the air, and a save must never delay it. So
//! the held quit runs [`crate::stop_the_radio`] — the very steps `quit_cleanup` runs: SHUTDOWN
//! to the radio loop, a bounded wait for it to say it has unkeyed, and the sweep of any daemon a
//! wedged loop never dropped — BEFORE it waits on anything. [`prepare_quit`] is that order and
//! nothing else, and `a_pending_save_cannot_hold_the_rig_keyed` holds it down with a save stalled
//! on the disk. The radio loop is the only thing that keys the rig (the CAT broker, Remote and
//! every cockpit reach the transmitter through it), so once it has stopped, the minute a slow
//! save may take keeps nothing on the air.
//!
//! ```text
//!           before:  close → window gone → quit_cleanup: [stop the radio] → [flush the log ≤ 10 s] → journals → exit
//!  after, if saving:  close → HELD → [stop the radio] → [save, shown; ≤ 60 s, then ask] → close
//!                           → window gone → quit_cleanup: [stop the radio: already stopped] → [flush: nothing left] → journals → exit
//! ```
//!
//! The one path that saves WITHOUT stopping the radio is the Windows update
//! ([`save_before_the_installer`]): it has never stopped the radio, deliberately — a failed
//! install must leave a working station — and it runs only after `update_install_block` found
//! the radio idle with TX disarmed. The dialog covers every cockpit's stop controls, so on that
//! path the station tells it `radioLive` and it carries a Stop TX of its own.
//!
//! macOS Cmd+Q (`RunEvent::Exit` alone) and an OS shutdown never reach a window close; they keep
//! `quit_cleanup`'s capped flush, unchanged.

use crate::SharedEngine;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};
use tauri::Manager;
use tempo_app::engine::engine_lock;
use tempo_app::logstore::Standing;

/// How long a quit waits for the logbook before it asks the operator what to do — the same
/// minute an operator command waits for its own change ([`tempo_app::logstore::DURABLE_WAIT`]).
pub(crate) const QUIT_PATIENCE: Duration = tempo_app::logstore::DURABLE_WAIT;

/// How often the window hears how many changes are left.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// The events, by name — the UI's `LogbookSaving.tsx` listens for exactly these.
pub(crate) const LOGBOOK_SAVING: &str = "logbook-saving";
pub(crate) const LOGBOOK_SAVE_FAILED: &str = "logbook-save-failed";
pub(crate) const LOGBOOK_SAVE_DONE: &str = "logbook-save-done";

/// `logbook-saving`: changes still on their way (0 once only `log.adi` is left), and whether
/// the radio is still running under the dialog.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Saving {
    pub pending: usize,
    pub radio_live: bool,
}

/// `logbook-save-failed`: the question. `pending` changes may still land if the operator waits;
/// `refused` ones never will; `reason` is the writer's own words, shown untranslated.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveFailed {
    pub pending: usize,
    pub refused: usize,
    pub reason: Option<String>,
    pub radio_live: bool,
}

/// `logbook-save-done`: the quit has finished with the logbook — saved, or given up on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveDone {
    pub saved: bool,
}

/// What the window is told, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Notice {
    Saving(Saving),
    Failed(SaveFailed),
    Done(SaveDone),
}

/// The operator's answer to the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    KeepTrying,
    QuitWithout,
}

/// How the quit left the logbook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Nothing was on its way: nothing was held, shown or asked.
    NothingToSave,
    /// Every change reached the logbook.
    Saved,
    /// The operator chose to quit with these changes still not in the logbook.
    QuitWithout { unsaved: usize },
}

/// The held quit's order, and all of it: the transmitter off the air FIRST — `stop_the_radio` is
/// [`crate::stop_the_radio`] in the app, the steps `quit_cleanup` runs — and only then the wait
/// on the logbook. A parameter only so a test can watch the order; see the module header.
pub(crate) fn prepare_quit(
    engine: &SharedEngine,
    stop_the_radio: impl FnOnce(),
    patience: Duration,
    tell: &mut dyn FnMut(Notice),
    ask: &mut dyn FnMut() -> Choice,
) -> Outcome {
    stop_the_radio();
    save_the_logbook(engine, patience, false, tell, ask)
}

/// Wait for the logbook's changes on their way to disk, telling the window as it goes, and ask
/// the operator when it cannot finish; then bring `log.adi` up to date. Blocks for as long as it
/// takes — never call it on the UI thread or holding a lock.
///
/// Nothing on its way: returns at once, having told the window nothing.
pub(crate) fn save_the_logbook(
    engine: &SharedEngine,
    patience: Duration,
    radio_live: bool,
    tell: &mut dyn FnMut(Notice),
    ask: &mut dyn FnMut() -> Choice,
) -> Outcome {
    // Handles, taken under the lock; everything after this runs with the lock released.
    let unsaved = engine_lock(engine).log_unsaved();
    if unsaved.is_empty() {
        return Outcome::NothingToSave;
    }
    let outcome = see_it_through(&mut |d| unsaved.wait(d), patience, radio_live, tell, ask);
    if outcome == Outcome::Saved {
        // Every change is in the logbook. `log.adi` next — a copy beside it for other programs,
        // so a problem there is the diagnostic log's, never a question for the operator.
        if let Some(problem) = unsaved.write_mirror(patience) {
            tempo_core::applog::warn("logbook", &problem);
        }
        tempo_core::applog::info("logbook", "the logbook is saved; quitting");
        tell(Notice::Done(SaveDone { saved: true }));
    }
    outcome
}

/// The wait and the question, over `standing_after` — "wait up to this long, then say where the
/// changes stand" ([`tempo_app::logstore::Unsaved::wait`] in the app).
fn see_it_through(
    standing_after: &mut dyn FnMut(Duration) -> Standing,
    patience: Duration,
    radio_live: bool,
    tell: &mut dyn FnMut(Notice),
    ask: &mut dyn FnMut() -> Choice,
) -> Outcome {
    let mut s = standing_after(Duration::ZERO);
    tempo_core::applog::info(
        "logbook",
        &format!(
            "quitting with {} change(s) still on their way to the logbook; the window stays \
             until they are saved",
            s.pending
        ),
    );
    tell(Notice::Saving(Saving {
        pending: s.pending,
        radio_live,
    }));
    let mut deadline = Instant::now() + patience;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        s = standing_after(left.min(PROGRESS_EVERY));
        if s.pending > 0 && Instant::now() < deadline {
            tell(Notice::Saving(Saving {
                pending: s.pending,
                radio_live,
            }));
            continue;
        }
        if s.saved() {
            return Outcome::Saved;
        }
        // The minute is up with changes still on their way, or the logbook refused one.
        let unsaved = s.pending + s.refused;
        tempo_core::applog::warn(
            "logbook",
            &format!(
                "the logbook is not saved: {} change(s) still on their way after {} s, {} \
                 refused{}; asking the operator",
                s.pending,
                patience.as_secs(),
                s.refused,
                s.reason
                    .as_deref()
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default()
            ),
        );
        tell(Notice::Failed(SaveFailed {
            pending: s.pending,
            refused: s.refused,
            reason: s.reason.clone(),
            radio_live,
        }));
        match ask() {
            Choice::KeepTrying if s.pending > 0 => {
                tempo_core::applog::info("logbook", "the operator chose to keep trying");
                deadline = Instant::now() + patience;
                tell(Notice::Saving(Saving {
                    pending: s.pending,
                    radio_live,
                }));
            }
            // Nothing is still on its way, and a refused change cannot be waited into the
            // logbook: the dialog does not offer this. Ask again — the quit is the only answer
            // that ends it.
            Choice::KeepTrying => {}
            Choice::QuitWithout => {
                tempo_core::applog::error(
                    "logbook",
                    &format!(
                        "the operator chose to quit without the last {unsaved} change(s) to the \
                         logbook"
                    ),
                );
                tell(Notice::Done(SaveDone { saved: false }));
                return Outcome::QuitWithout { unsaved };
            }
        }
    }
}

// ─── one quit at a time ─────────────────────────────────────────────────────────────────────

/// No quit is under way.
const IDLE: u8 = 0;
/// A quit is saving the logbook.
const SAVING: u8 = 1;
/// …and has asked the operator, and waits for the answer.
const ASKING: u8 = 2;
/// A quit has finished with the logbook: the next close goes through.
const READY: u8 = 3;

/// Where the app is in a quit.
static STAGE: AtomicU8 = AtomicU8::new(IDLE);

/// The operator's answer, on its way to the quit that asked — one per question.
static ANSWER: Mutex<Option<Sender<Choice>>> = Mutex::new(None);

/// What asked the app to quit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Request {
    /// The main window's close.
    Close,
    /// `restart_app`, after an update was installed.
    Restart,
}

/// What to do about a quit request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    /// Go ahead exactly as before: nothing to save, or the save is done.
    Proceed,
    /// Start the held quit: stop the radio, save the logbook, then close or restart.
    Save,
    /// A quit is already saving: hold the window and do nothing else.
    Hold,
    /// The operator closed the window while being asked: take it as "quit without" — the one
    /// way out that needs no working webview — and hold until that quit closes the window.
    HoldAndQuitWithout,
    /// A quit is already under way and will end the process: nothing to do.
    Ignore,
}

/// The whole decision, as a table: where the app is in a quit, and whether the logbook has
/// something on its way (`None`: the engine was busy, so nobody can say — treated as "yes", and
/// the quit's own thread finds out).
pub(crate) fn decide(request: Request, stage: u8, waiting: Option<bool>) -> Act {
    match (request, stage) {
        (_, READY) => Act::Proceed,
        (Request::Close, ASKING) => Act::HoldAndQuitWithout,
        (Request::Close, SAVING) => Act::Hold,
        (Request::Restart, SAVING | ASKING) => Act::Ignore,
        _ => match waiting {
            Some(false) => Act::Proceed,
            Some(true) | None => Act::Save,
        },
    }
}

/// Whether the logbook has something on its way, asked WITHOUT waiting for the engine: this runs
/// on the UI thread, which must never block on the engine mutex (the radio loop can hold it
/// across a CAT read). `None`: the engine was busy.
pub(crate) fn logbook_waiting(engine: &SharedEngine) -> Option<bool> {
    let unsaved = match engine.try_lock() {
        Ok(e) => e.log_unsaved(),
        // Poison recovers, exactly as `engine_lock` does.
        Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner().log_unsaved(),
        Err(std::sync::TryLockError::WouldBlock) => return None,
    };
    Some(!unsaved.is_empty())
}

/// The main window was asked to close. `true`: hold it — the caller calls `prevent_close`.
pub(crate) fn hold_the_close(app: &tauri::AppHandle) -> bool {
    let engine = app.state::<SharedEngine>();
    match decide(
        Request::Close,
        STAGE.load(Ordering::SeqCst),
        logbook_waiting(engine.inner()),
    ) {
        Act::Proceed | Act::Ignore => false,
        Act::Hold => true,
        Act::HoldAndQuitWithout => {
            if answer(Choice::QuitWithout) {
                tempo_core::applog::warn(
                    "logbook",
                    "the window was closed while the operator was asked; taken as quit without",
                );
            }
            true
        }
        Act::Save => begin(app, Request::Close),
    }
}

/// `restart_app`: the same held quit as a close, ending in the restart.
pub(crate) fn restart_when_saved(app: &tauri::AppHandle) {
    let engine = app.state::<SharedEngine>();
    match decide(
        Request::Restart,
        STAGE.load(Ordering::SeqCst),
        logbook_waiting(engine.inner()),
    ) {
        Act::Proceed => app.request_restart(),
        Act::Save => {
            if !begin(app, Request::Restart) {
                app.request_restart();
            }
        }
        Act::Hold | Act::HoldAndQuitWithout | Act::Ignore => tempo_core::applog::info(
            "updater",
            "a quit is already saving the logbook; it ends the process",
        ),
    }
}

/// Start the held quit on a thread of its own. `false`: it could not start (another quit got
/// there first, or no thread) and the caller goes ahead as before — `quit_cleanup` still flushes.
fn begin(app: &tauri::AppHandle, request: Request) -> bool {
    if STAGE
        .compare_exchange(IDLE, SAVING, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return false;
    }
    let app = app.clone();
    let started = std::thread::Builder::new()
        .name("nexus-quit".into())
        .spawn(move || {
            let engine = app.state::<SharedEngine>().inner().clone();
            quit_then(
                &engine,
                crate::stop_the_radio,
                QUIT_PATIENCE,
                &mut |n| tell_window(&app, n),
                &mut ask_operator,
                || {
                    STAGE.store(READY, Ordering::SeqCst);
                    match request {
                        Request::Close => close_the_window(&app),
                        Request::Restart => app.request_restart(),
                    }
                },
            );
        });
    if started.is_err() {
        STAGE.store(READY, Ordering::SeqCst);
        return false;
    }
    true
}

/// The held quit ([`prepare_quit`]), and then `after` — the second close, or the restart —
/// HOWEVER the quit ended. A panic in it must not leave the window held: every later close would
/// be held too, and the operator would have no way to quit. The radio stop and the logbook's
/// final flush still run in `quit_cleanup` on the way out.
fn quit_then(
    engine: &SharedEngine,
    stop_the_radio: impl FnOnce(),
    patience: Duration,
    tell: &mut dyn FnMut(Notice),
    ask: &mut dyn FnMut() -> Choice,
    after: impl FnOnce(),
) {
    let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        prepare_quit(engine, stop_the_radio, patience, tell, ask)
    }));
    if ended.is_err() {
        tempo_core::applog::error(
            "logbook",
            "the quit stopped unexpectedly while saving the logbook; quitting anyway — the \
             logbook's final flush still runs",
        );
    }
    after();
}

/// Before a Windows update hands the machine to the installer: save the logbook with the
/// operator told, as a quit does — but with the radio left running, as the update path always
/// left it. Blocks for as long as the save takes: run it on the blocking pool.
pub(crate) fn save_before_the_installer(app: &tauri::AppHandle) {
    if STAGE
        .compare_exchange(IDLE, SAVING, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let engine = app.state::<SharedEngine>().inner().clone();
    // Caught for the reason `quit_then` gives: whatever happens here, the stage must move on.
    let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        save_the_logbook(
            &engine,
            QUIT_PATIENCE,
            true,
            &mut |n| tell_window(app, n),
            &mut ask_operator,
        )
    }));
    if ended.is_err() {
        tempo_core::applog::error(
            "logbook",
            "saving the logbook before the update stopped unexpectedly; the update goes on",
        );
    }
    // Not READY: a failed install leaves the station running, and its next close is an
    // ordinary one.
    STAGE.store(IDLE, Ordering::SeqCst);
}

/// The held close's second, real close. The window's own close runs the handler again, which
/// now lets it through; with no window left to close, the app exits the same way.
fn close_the_window(app: &tauri::AppHandle) {
    match app.get_webview_window("main") {
        Some(w) if w.close().is_ok() => {}
        _ => app.exit(0),
    }
}

fn tell_window(app: &tauri::AppHandle, notice: Notice) {
    use tauri::Emitter;
    let _ = match notice {
        Notice::Saving(p) => app.emit_to("main", LOGBOOK_SAVING, p),
        Notice::Failed(p) => app.emit_to("main", LOGBOOK_SAVE_FAILED, p),
        Notice::Done(p) => app.emit_to("main", LOGBOOK_SAVE_DONE, p),
    };
}

/// Ask, and wait for the answer. A dropped sender (no answer can come) is the quit.
fn ask_operator() -> Choice {
    let (tx, rx) = channel();
    *ANSWER.lock().unwrap_or_else(PoisonError::into_inner) = Some(tx);
    STAGE.store(ASKING, Ordering::SeqCst);
    let choice = rx.recv().unwrap_or(Choice::QuitWithout);
    STAGE.store(SAVING, Ordering::SeqCst);
    choice
}

/// Hand the waiting quit its answer. `false`: no question was open.
fn answer(choice: Choice) -> bool {
    let tx = ANSWER.lock().unwrap_or_else(PoisonError::into_inner).take();
    tx.is_some_and(|tx| tx.send(choice).is_ok())
}

/// The operator's answer to `logbook-save-failed`. One answer per question: a second one, or one
/// with no question open, is ignored.
#[tauri::command]
pub fn logbook_save_choice(keep_trying: bool) {
    answer(if keep_trying {
        Choice::KeepTrying
    } else {
        Choice::QuitWithout
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::durable_command_tests::engine_on_store;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::{mpsc, Arc};
    use tempo_core::logbook::migrate::database_path;
    use tempo_core::logbook::sqlite::{LogDb, WriteHold};

    /// Every notice, in order, with whether the radio had been stopped when each was given.
    type Heard = Arc<Mutex<Vec<(bool, Notice)>>>;

    fn heard() -> Heard {
        Arc::new(Mutex::new(Vec::new()))
    }

    fn notices(h: &Heard) -> Vec<Notice> {
        h.lock().unwrap().iter().map(|(_, n)| n.clone()).collect()
    }

    /// Whether row `row`'s QSL-card mark is in the database, read through a connection of the
    /// test's own.
    fn marked_on_disk(dir: &Path, engine: &SharedEngine, row: usize) -> bool {
        let id = engine_lock(engine).log_records()[row].id;
        LogDb::open(&database_path(&dir.join("log.adi")))
            .and_then(|d| d.load_all())
            .expect("read")
            .into_iter()
            .find(|r| r.id == id)
            .is_some_and(|r| r.qsl_rcvd.card)
    }

    fn saving(pending: usize) -> Notice {
        Notice::Saving(Saving {
            pending,
            radio_live: false,
        })
    }

    // ── ⛔ the order ────────────────────────────────────────────────────────────────────────

    /// ⛔ A PENDING SAVE CANNOT HOLD THE RIG KEYED. The quit's wait on the logbook comes AFTER the
    /// radio is stopped — so a disk that will not take the log for a minute keeps nothing on the
    /// air for that minute.
    ///
    /// Driven with a real save stalled on a real disk: the store's write lock is held, a change
    /// is waiting behind it, and the quit starts. By the time the window hears the save has
    /// begun, the radio has already been stopped; and while the save is still stuck — the quit
    /// has not finished — the stop has long returned, so it never waited on the save.
    ///
    /// `stop_the_radio` stands in for [`crate::stop_the_radio`], which the app passes here and
    /// which is SHUTDOWN to the radio loop and the wait for its unkey — a process-wide flag no
    /// test can raise without stopping every other test's radio loop. That the app passes that
    /// function, and that it is those steps, is pinned by the wiring test below.
    #[test]
    fn a_pending_save_cannot_hold_the_rig_keyed() {
        let (dir, engine) = engine_on_store("quit-order", 10);
        let hold = WriteHold::take(&database_path(&dir.join("log.adi"))).expect("stall the store");
        assert!(
            engine_lock(&engine).mark_qsl_card(3, true),
            "a change waiting on the stalled disk"
        );

        let stopped = Arc::new(AtomicBool::new(false));
        let (first_tx, first) = mpsc::channel::<(bool, Notice)>();
        let quit = std::thread::spawn({
            let engine = Arc::clone(&engine);
            let stopped = Arc::clone(&stopped);
            move || {
                let seen = Arc::clone(&stopped);
                prepare_quit(
                    &engine,
                    move || stopped.store(true, Ordering::SeqCst),
                    Duration::from_secs(30),
                    &mut |n| {
                        let _ = first_tx.send((seen.load(Ordering::SeqCst), n));
                    },
                    &mut || Choice::QuitWithout,
                )
            }
        });

        let (stopped_before_the_wait, notice) = first
            .recv_timeout(Duration::from_secs(10))
            .expect("the quit says it is saving");
        assert_eq!(notice, saving(1), "one change on its way");
        assert!(
            stopped_before_the_wait,
            "⛔ the radio is stopped BEFORE the quit waits on the logbook"
        );
        std::thread::sleep(Duration::from_millis(300));
        assert!(!quit.is_finished(), "premise: the save is still stuck");
        assert!(
            stopped.load(Ordering::SeqCst),
            "⛔ and a stuck save holds nothing: the radio stop never waited for it"
        );

        drop(hold);
        assert_eq!(quit.join().expect("the quit"), Outcome::Saved);
        assert!(marked_on_disk(&dir, &engine, 3), "the change landed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The wiring the test above cannot see, source-scanned because what is under test is which
    /// function each quit path calls, and in what order — no type sees that.
    ///
    /// - `stop_the_radio` IS the radio stop: SHUTDOWN raised, the wait for SHUTDOWN_DONE, the
    ///   daemon sweep, in that order;
    /// - `quit_cleanup` still runs it first, before the log flush and the journals, on every
    ///   quit — the Cmd+Q and OS-shutdown paths included;
    /// - the held quit hands `prepare_quit` that very function, and `prepare_quit` runs it
    ///   before `save_the_logbook`;
    /// - the window's close asks `hold_the_close` and holds only on its say-so; `restart_app`
    ///   goes through the held quit; the Windows update saves before its journals.
    #[test]
    fn every_quit_path_stops_the_radio_before_it_waits_on_the_logbook() {
        let lib = include_str!("lib.rs");
        let quit = include_str!("quit.rs");
        let at = |hay: &str, needle: &str| {
            hay.find(needle)
                .unwrap_or_else(|| panic!("`{needle}` is there"))
        };

        let stop = body_of(lib, "fn stop_the_radio() {");
        assert!(
            at(stop, "SHUTDOWN.store(true") < at(stop, "SHUTDOWN_DONE.load(")
                && at(stop, "SHUTDOWN_DONE.load(") < at(stop, "kill_leftover_daemons()"),
            "stop_the_radio is the radio loop's SHUTDOWN handshake, then the daemon sweep"
        );

        let cleanup = body_of(lib, "fn quit_cleanup(");
        let stops = at(cleanup, "stop_the_radio();");
        for after in ["persist_journals(", "flush_logbook("] {
            assert!(
                stops < at(cleanup, after),
                "quit_cleanup stops the radio before `{after}`"
            );
        }

        let prepare = body_of(quit, "pub(crate) fn prepare_quit(");
        assert!(
            at(prepare, "stop_the_radio();") < at(prepare, "save_the_logbook("),
            "prepare_quit: the radio first, then the wait"
        );
        let begin = body_of(quit, "fn begin(");
        assert!(
            begin.contains(
                "quit_then(\n                &engine,\n                crate::stop_the_radio,"
            ),
            "the held quit hands the app's own radio stop on"
        );
        let then = body_of(quit, "fn quit_then(");
        assert!(
            then.contains("prepare_quit(engine, stop_the_radio, patience, tell, ask)"),
            "…to prepare_quit, which runs it first"
        );

        let handler = &lib[at(lib, ".on_window_event(|window, event| {")..];
        let handler = &handler[..at(handler, ".build(tauri::generate_context!())")];
        assert!(
            handler.contains(
                "if quit::hold_the_close(app) {\n                        api.prevent_close();"
            ),
            "the main window's close is held only when the quit says so"
        );

        let restart = body_of(lib, "fn restart_app(");
        assert!(
            restart.contains("quit::restart_when_saved(&app);")
                && !restart.contains("request_restart"),
            "restart_app goes through the held quit, never straight to the restart"
        );

        let update = body_of(lib, "async fn prepare_update_install(");
        assert!(
            at(update, "quit::save_before_the_installer(&app);")
                < at(update, "persist_journals(&app);"),
            "the Windows update saves the logbook, shown, before the journals and the installer"
        );

        let list = lib
            .split_once("tauri::generate_handler![")
            .expect("the handler list")
            .1
            .split_once("])")
            .expect("its end")
            .0;
        assert!(
            list.lines()
                .any(|l| l.trim() == "quit::logbook_save_choice,"),
            "the operator's answer is a registered command"
        );
    }

    /// However the quit ends, the window closes. A panic inside it — here the radio stop itself
    /// — still reaches the close, so no window is left held that no close can shut; on the way
    /// out, `quit_cleanup` runs the radio stop again and the logbook's final flush.
    #[test]
    fn a_quit_that_fails_still_closes_the_window() {
        let (dir, engine) = engine_on_store("quit-panic", 3);
        let mut closed = false;
        quit_then(
            &engine,
            || panic!("the radio stop failed"),
            Duration::from_secs(1),
            &mut |_| {},
            &mut || Choice::QuitWithout,
            || closed = true,
        );
        assert!(closed, "the close still happens");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The body of the top-level function whose signature starts `sig` in `src`.
    fn body_of<'a>(src: &'a str, sig: &str) -> &'a str {
        let start = src
            .find(&format!("\n{sig}"))
            .unwrap_or_else(|| panic!("{sig} is defined"));
        let rest = &src[start + 1..];
        &rest[..rest.find("\n}\n").expect("the end of the function")]
    }

    // ── what each quit request does ─────────────────────────────────────────────────────────

    /// The decision, every row of it. The row that matters most is the first: nothing on its way
    /// and no quit under way → the close goes ahead exactly as it always did.
    #[test]
    fn the_quit_decision_table() {
        use Act::*;
        use Request::*;
        assert_eq!(
            decide(Close, IDLE, Some(false)),
            Proceed,
            "nothing pending: as before"
        );
        assert_eq!(
            decide(Close, IDLE, Some(true)),
            Save,
            "pending: hold and save"
        );
        assert_eq!(
            decide(Close, IDLE, None),
            Save,
            "engine busy: cannot say, so the quit's own thread finds out"
        );
        assert_eq!(
            decide(Close, SAVING, Some(true)),
            Hold,
            "a second close while saving"
        );
        assert_eq!(
            decide(Close, ASKING, Some(true)),
            HoldAndQuitWithout,
            "a close while asked is the answer that needs no webview"
        );
        assert_eq!(
            decide(Close, READY, Some(true)),
            Proceed,
            "the held quit's own close"
        );

        assert_eq!(
            decide(Restart, IDLE, Some(false)),
            Proceed,
            "nothing pending: restart as before"
        );
        assert_eq!(decide(Restart, IDLE, Some(true)), Save);
        assert_eq!(decide(Restart, IDLE, None), Save);
        assert_eq!(
            decide(Restart, SAVING, Some(true)),
            Ignore,
            "a quit already under way ends it"
        );
        assert_eq!(
            decide(Restart, ASKING, Some(true)),
            Ignore,
            "and a restart is never an answer"
        );
        assert_eq!(decide(Restart, READY, None), Proceed);
    }

    /// The close asks the logbook without waiting for the engine, on the UI thread. It says "no"
    /// only when it can see there is nothing — and a busy engine is "cannot say", never "no".
    ///
    /// The control for the "no" is the stalled change: the same check on the same engine says
    /// "yes" while a change is on its way.
    #[test]
    fn the_close_asks_the_logbook_without_waiting_for_the_engine() {
        let (dir, engine) = engine_on_store("quit-waiting", 10);
        engine_lock(&engine)
            .flush_log_store(Duration::from_secs(60))
            .expect("written");
        assert_eq!(logbook_waiting(&engine), Some(false), "nothing on its way");

        let hold = WriteHold::take(&database_path(&dir.join("log.adi"))).expect("stall the store");
        assert!(engine_lock(&engine).mark_qsl_card(2, true));
        assert_eq!(
            logbook_waiting(&engine),
            Some(true),
            "control: a change on its way is seen"
        );

        let busy = engine_lock(&engine);
        let started = Instant::now();
        let asked = logbook_waiting(&engine);
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "it never waits for the engine"
        );
        assert_eq!(asked, None, "a busy engine cannot say");
        drop(busy);

        drop(hold);
        engine_lock(&engine)
            .flush_log_store(Duration::from_secs(60))
            .expect("written");
        assert_eq!(logbook_waiting(&engine), Some(false));
        let _ = std::fs::remove_dir_all(&dir);

        // The 1.13 path writes log.adi inline: there is never anything on its way.
        let plain: SharedEngine = Arc::new(Mutex::new(tempo_app::engine::Engine::new(
            "K2DEF", "FN31", 0,
        )));
        assert_eq!(logbook_waiting(&plain), Some(false));
    }

    // ── the save ────────────────────────────────────────────────────────────────────────────

    /// Nothing on its way: nothing is waited for, shown or asked — the radio stop still runs,
    /// because the quit is still a quit.
    #[test]
    fn nothing_on_its_way_shows_nothing_and_asks_nothing() {
        let (dir, engine) = engine_on_store("quit-nothing", 10);
        engine_lock(&engine)
            .flush_log_store(Duration::from_secs(60))
            .expect("written");
        let told = heard();
        let stopped = AtomicBool::new(false);
        let outcome = prepare_quit(
            &engine,
            || stopped.store(true, Ordering::SeqCst),
            Duration::from_secs(5),
            &mut |n| told.lock().unwrap().push((false, n)),
            &mut || panic!("nothing to ask about"),
        );
        assert_eq!(outcome, Outcome::NothingToSave);
        assert!(notices(&told).is_empty(), "the window is told nothing");
        assert!(stopped.load(Ordering::SeqCst), "the radio stop ran");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A save that lands: the window hears it begin and end, is asked nothing, and what the
    /// quit's own flush in `quit_cleanup` then finds is nothing left to write.
    #[test]
    fn a_save_that_lands_closes_with_nothing_left_for_the_final_flush() {
        let (dir, engine) = engine_on_store("quit-lands", 10);
        let hold = WriteHold::take(&database_path(&dir.join("log.adi"))).expect("stall the store");
        assert!(engine_lock(&engine).mark_qsl_card(4, true));
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            drop(hold);
        });
        let told = heard();
        let outcome = save_the_logbook(
            &engine,
            Duration::from_secs(30),
            false,
            &mut |n| told.lock().unwrap().push((false, n)),
            &mut || panic!("a save that lands asks nothing"),
        );
        release.join().expect("released");
        assert_eq!(outcome, Outcome::Saved);
        let said = notices(&told);
        assert_eq!(
            said.first(),
            Some(&saving(1)),
            "it says it is saving, and how much"
        );
        assert_eq!(
            said.last(),
            Some(&Notice::Done(SaveDone { saved: true })),
            "and that it is done"
        );
        assert!(
            said.iter().all(|n| !matches!(n, Notice::Failed(_))),
            "and never asks: {said:?}"
        );
        assert!(
            marked_on_disk(&dir, &engine, 4),
            "the change is in the logbook"
        );
        assert!(
            engine_lock(&engine).log_unsaved().is_empty(),
            "nothing left for quit_cleanup's own flush, log.adi included"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A save that cannot finish in time asks, offering both choices (pending > 0); Keep trying
    /// goes back to waiting — with a fresh minute — and a save that then lands ends it.
    #[test]
    fn a_slow_save_asks_and_keep_trying_waits_again() {
        let (dir, engine) = engine_on_store("quit-slow", 10);
        let hold = WriteHold::take(&database_path(&dir.join("log.adi"))).expect("stall the store");
        assert!(engine_lock(&engine).mark_qsl_card(1, true));
        let told = heard();
        let mut hold = Some(hold);
        let mut asked = 0;
        let outcome = save_the_logbook(
            &engine,
            Duration::from_millis(400),
            false,
            &mut |n| told.lock().unwrap().push((false, n)),
            &mut || {
                asked += 1;
                // The operator frees the disk, then says keep trying.
                drop(hold.take());
                Choice::KeepTrying
            },
        );
        assert_eq!(outcome, Outcome::Saved);
        assert_eq!(asked, 1, "asked once");
        let said = notices(&told);
        let q = said
            .iter()
            .position(|n| matches!(n, Notice::Failed(_)))
            .expect("the question");
        assert_eq!(
            said[q],
            Notice::Failed(SaveFailed {
                pending: 1,
                refused: 0,
                reason: None,
                radio_live: false
            }),
            "one change still on its way, none refused: both choices"
        );
        assert_eq!(
            said[q + 1],
            saving(1),
            "keep trying: back to the saving line at once"
        );
        assert_eq!(said.last(), Some(&Notice::Done(SaveDone { saved: true })));
        assert!(marked_on_disk(&dir, &engine, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Quit without: the quit gives up on what is still on its way, says how many, and tells
    /// the window it is done — with `saved: false`.
    #[test]
    fn quit_without_gives_up_and_says_how_many() {
        let (dir, engine) = engine_on_store("quit-without", 10);
        let hold = WriteHold::take(&database_path(&dir.join("log.adi"))).expect("stall the store");
        assert!(engine_lock(&engine).mark_qsl_card(1, true));
        assert!(engine_lock(&engine).mark_qsl_card(2, true));
        let told = heard();
        let outcome = save_the_logbook(
            &engine,
            Duration::from_millis(300),
            false,
            &mut |n| told.lock().unwrap().push((false, n)),
            &mut || Choice::QuitWithout,
        );
        assert_eq!(outcome, Outcome::QuitWithout { unsaved: 2 });
        let said = notices(&told);
        assert!(
            said.contains(&Notice::Failed(SaveFailed {
                pending: 2,
                refused: 0,
                reason: None,
                radio_live: false
            })),
            "asked, with the count: {said:?}"
        );
        assert_eq!(said.last(), Some(&Notice::Done(SaveDone { saved: false })));
        drop(hold);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refused change is not waited out: the question comes as soon as nothing is left on its
    /// way, with the count and the reason, and a Keep trying (the dialog does not offer one) is
    /// asked again rather than taken as "saved". Scripted, because a real refusal needs a store
    /// that refuses; `tempo-app`'s `a_quit_is_told_which_changes_the_store_refused_and_why`
    /// proves the store reports one this way.
    #[test]
    fn a_refused_change_is_asked_about_not_waited_out() {
        let mut calls = 0;
        let mut standing = |_: Duration| {
            calls += 1;
            if calls == 1 {
                Standing {
                    pending: 1,
                    refused: 0,
                    reason: None,
                }
            } else {
                Standing {
                    pending: 0,
                    refused: 1,
                    reason: Some("database or disk is full".into()),
                }
            }
        };
        let told = heard();
        let mut answers = vec![Choice::QuitWithout, Choice::KeepTrying];
        let started = Instant::now();
        let outcome = see_it_through(
            &mut standing,
            Duration::from_secs(60),
            false,
            &mut |n| told.lock().unwrap().push((false, n)),
            &mut || answers.pop().expect("asked no more than twice"),
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "not waited out: asked as soon as nothing is left on its way"
        );
        assert_eq!(outcome, Outcome::QuitWithout { unsaved: 1 });
        let said = notices(&told);
        let refused = Notice::Failed(SaveFailed {
            pending: 0,
            refused: 1,
            reason: Some("database or disk is full".into()),
            radio_live: false,
        });
        assert_eq!(
            said.iter().filter(|n| **n == refused).count(),
            2,
            "asked, then asked again after a keep trying: {said:?}"
        );
        assert_eq!(said.last(), Some(&Notice::Done(SaveDone { saved: false })));
    }

    /// Before a Windows update the radio is left running, and every notice says so — the
    /// dialog then carries its own Stop TX over the cockpit's.
    #[test]
    fn the_update_path_says_the_radio_is_still_running() {
        let (dir, engine) = engine_on_store("quit-update", 10);
        let hold = WriteHold::take(&database_path(&dir.join("log.adi"))).expect("stall the store");
        assert!(engine_lock(&engine).mark_qsl_card(1, true));
        let told = heard();
        let outcome = save_the_logbook(
            &engine,
            Duration::from_millis(300),
            true,
            &mut |n| told.lock().unwrap().push((false, n)),
            &mut || Choice::QuitWithout,
        );
        assert_eq!(outcome, Outcome::QuitWithout { unsaved: 1 });
        for n in notices(&told) {
            match n {
                Notice::Saving(s) => assert!(s.radio_live, "{s:?}"),
                Notice::Failed(f) => assert!(f.radio_live, "{f:?}"),
                Notice::Done(_) => {}
            }
        }
        drop(hold);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The payloads the UI reads, by name — `LogbookSaving.tsx` reads camelCase.
    #[test]
    fn the_events_carry_what_the_dialog_reads() {
        assert_eq!(
            serde_json::to_value(Saving {
                pending: 3,
                radio_live: true
            })
            .unwrap(),
            serde_json::json!({ "pending": 3, "radioLive": true })
        );
        assert_eq!(
            serde_json::to_value(SaveFailed {
                pending: 1,
                refused: 2,
                reason: Some("why".into()),
                radio_live: false
            })
            .unwrap(),
            serde_json::json!({ "pending": 1, "refused": 2, "reason": "why", "radioLive": false })
        );
        assert_eq!(
            serde_json::to_value(SaveDone { saved: true }).unwrap(),
            serde_json::json!({ "saved": true })
        );
        let ui = include_str!("../../ui/src/components/LogbookSaving.tsx");
        for name in [LOGBOOK_SAVING, LOGBOOK_SAVE_FAILED, LOGBOOK_SAVE_DONE] {
            assert!(
                ui.contains(&format!("'{name}'")),
                "the dialog listens for `{name}`"
            );
        }
    }
}
