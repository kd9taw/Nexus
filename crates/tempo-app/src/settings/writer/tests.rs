use super::*;
use crate::engine::{engine_lock, engine_try_lock, Engine};
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::mpsc;

/// A scratch folder, removed with the test.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let p = std::env::temp_dir().join(format!(
            "nexus-settings-writer-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }

    fn settings(&self) -> PathBuf {
        self.0.join("settings.json")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Settings carrying `n` in a field no load migrates, so a file names the snapshot it holds.
fn marked(n: u64) -> Settings {
    Settings {
        qrz_last_sync_unix: n,
        ..Settings::default()
    }
}

/// Which snapshot the file at `path` holds.
fn on_disk(path: &Path) -> u64 {
    Settings::load(path).qrz_last_sync_unix
}

/// Run `f` holding the Engine lock's count, as every command holds it while it hands a snapshot
/// over: the precondition `queue` and `save_now` assert. These writer tests hand over marked
/// copies rather than an Engine's settings, so they take the count alone
/// ([`tempo_core::logbook::io_fence::EngineHeld`], the token every `EngineGuard` carries); the race
/// tests take a real `EngineGuard`. Never around a flush or a ticket's wait, which assert the
/// opposite.
fn locked<T>(f: impl FnOnce() -> T) -> T {
    let _held = tempo_core::logbook::io_fence::EngineHeld::acquired();
    f()
}

/// What a recording write saw: the snapshot each write carried, in the order the writes started,
/// and the most writes that were ever on the file at once.
#[derive(Default)]
struct Recorder {
    started: Mutex<Vec<u64>>,
    in_flight: AtomicUsize,
    most: AtomicUsize,
}

impl Recorder {
    fn started(&self) -> Vec<u64> {
        lock(&self.started).clone()
    }
}

/// A write that records itself, is held for `delay(snapshot)` — the injected slow disk — and is
/// then made by [`Settings::save`], exactly as the app's.
fn recording(delay: impl Fn(u64) -> Duration + Send + Sync + 'static) -> (Write, Arc<Recorder>) {
    let rec = Arc::new(Recorder::default());
    let r = Arc::clone(&rec);
    let write: Write = Arc::new(move |s: &Settings, p: &Path| {
        let now = r.in_flight.fetch_add(1, SeqCst) + 1;
        r.most.fetch_max(now, SeqCst);
        lock(&r.started).push(s.qrz_last_sync_unix);
        std::thread::sleep(delay(s.qrz_last_sync_unix));
        let out = s.save(p);
        r.in_flight.fetch_sub(1, SeqCst);
        out
    });
    (write, rec)
}

/// A gate a write waits at until the test opens it: the writer held mid-write, its file lock with it.
struct Gate {
    shut: Mutex<bool>,
    opened: Condvar,
}

impl Gate {
    fn shut() -> Arc<Gate> {
        Arc::new(Gate {
            shut: Mutex::new(true),
            opened: Condvar::new(),
        })
    }

    fn pass(&self) {
        let mut shut = lock(&self.shut);
        while *shut {
            shut = self
                .opened
                .wait(shut)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn open(&self) {
        *lock(&self.shut) = false;
        self.opened.notify_all();
    }
}

/// Opens its gate when it drops. Declared after the writer, it drops first when a test fails, so
/// a write still held at the gate is let go before the writer's own drop waits for its thread:
/// a red test fails, it never hangs.
struct OpenOnDrop(Arc<Gate>);

impl Drop for OpenOnDrop {
    fn drop(&mut self) {
        self.0.open();
    }
}

/// A write of snapshot `held` waits at `gate` and says so on the returned channel; every write is
/// recorded, and one of snapshot `fails` fails the way a full disk does.
fn gated(held: u64, fails: u64) -> (Write, Arc<Gate>, mpsc::Receiver<()>, Arc<Recorder>) {
    let gate = Gate::shut();
    let (reached, at_gate) = mpsc::channel();
    let rec = Arc::new(Recorder::default());
    let (g, r) = (Arc::clone(&gate), Arc::clone(&rec));
    let write: Write = Arc::new(move |s: &Settings, p: &Path| {
        let n = s.qrz_last_sync_unix;
        lock(&r.started).push(n);
        if n == held {
            let _ = reached.send(());
            g.pass();
        }
        if n == fails {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "the disk is full",
            ));
        }
        s.save(p)
    });
    (write, gate, at_gate, rec)
}

/// A writer nothing is queued on starts no thread — Remote's saves reach every path they are
/// given through [`SettingsWriter::of`], and none of those may leave a thread behind — has nothing
/// to flush, and is the same writer every time its file is named.
#[test]
fn a_writer_never_queued_on_starts_no_thread() {
    let d = Dir::new("idle");
    let w = SettingsWriter::of(&d.settings());
    assert!(
        w.flush(Duration::ZERO) == Flushed::Saved,
        "nothing handed over, nothing to wait for"
    );
    locked(|| w.save_now(&marked(3))).expect("a synchronous save");
    assert_eq!(on_disk(&d.settings()), 3);
    assert!(
        w.thread.get().is_none(),
        "a synchronous save starts no thread"
    );
    assert!(
        Arc::ptr_eq(&w, &SettingsWriter::of(&d.settings())),
        "one writer per file"
    );
}

/// ⛔ THE ORDERING PRECONDITION, CHECKED. A snapshot is numbered as it is handed over, so it must
/// be handed over holding the Engine lock it was taken under: one handed over after that lock was
/// let go can be numbered after a newer one and win, and the newer value is gone from the disk.
/// A debug build refuses a `queue` made without the lock…
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "settings writer: queue without the Engine lock")]
fn a_queue_without_the_engine_lock_is_refused() {
    let d = Dir::new("unlocked-queue");
    let w = SettingsWriter::new(d.settings(), Arc::new(Settings::save));
    w.queue(marked(1), |_| {});
}

/// …and a `save_now` made without it, which numbers the same way and supersedes the waiting
/// snapshot on the strength of its number.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "settings writer: save_now without the Engine lock")]
fn a_save_now_without_the_engine_lock_is_refused() {
    let d = Dir::new("unlocked-save");
    let w = SettingsWriter::new(d.settings(), Arc::new(Settings::save));
    let _ = w.save_now(&marked(1));
}

/// Eight threads make 25 changes each to a real Engine, every one under its lock (an
/// `EngineGuard`, as every command takes it), and hand the Engine's settings over before they let
/// go of it. Once `settle` returns, the file must hold the newest change.
fn race(hand_over: &(dyn Fn(Settings) + Sync), settle: &dyn Fn(), path: &Path) {
    let engine = Mutex::new(Engine::new("KD9TAW", "EN52", 0));
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                for _ in 0..25 {
                    let mut eng = engine_lock(&engine);
                    let next = eng.settings().qrz_last_sync_unix + 1;
                    eng.set_qrz_sync_cursor(next);
                    hand_over(eng.settings().clone());
                }
            });
        }
    });
    settle();
    let newest = engine_lock(&engine).settings().qrz_last_sync_unix;
    assert_eq!(newest, 200, "premise: every change was made");
    let file = on_disk(path);
    assert_eq!(
        file, newest,
        "lost update: the file holds change {file}, the Engine has made {newest}"
    );
}

/// ★ NO LOST UPDATE: concurrent changes, each handed over under the lock, leave the newest on the
/// disk — through a slow disk, with one write on the file at a time.
#[test]
fn concurrent_changes_leave_the_newest_on_the_disk() {
    let d = Dir::new("race");
    let (write, rec) = recording(|n| Duration::from_micros(200 + (n % 7) * 300));
    let w = SettingsWriter::new(d.settings(), write);
    let failures = Arc::new(AtomicUsize::new(0));
    race(
        &|s| {
            let failures = Arc::clone(&failures);
            w.queue(s, move |_| {
                failures.fetch_add(1, SeqCst);
            });
        },
        &|| assert_eq!(w.flush(Duration::from_secs(60)), Flushed::Saved),
        &d.settings(),
    );
    assert_eq!(failures.load(SeqCst), 0, "no write failed");
    assert_eq!(rec.most.load(SeqCst), 1, "one write on the file at a time");
}

/// The race's check can fail: a single writer that holds every snapshot and then writes them
/// newest first — out of order — leaves the oldest on the disk, and the check names it.
#[test]
#[should_panic(expected = "lost update")]
fn the_race_check_catches_an_out_of_order_writer() {
    let d = Dir::new("race-control");
    let held = Mutex::new(Vec::new());
    race(
        &|s| lock(&held).push(s),
        &|| {
            for s in lock(&held).drain(..).rev() {
                s.save(&d.settings()).unwrap();
            }
        },
        &d.settings(),
    );
}

/// ★ A SLOW DISK NEVER LANDS AN OLDER SNAPSHOT AFTER A NEWER ONE. The older a snapshot, the longer
/// its write takes (40 ms down to 2) — the delays that land writes out of order when they run side
/// by side — and a synchronous save, Remote's, cuts in halfway.
#[test]
fn a_slow_writer_never_lands_an_older_snapshot_after_a_newer_one() {
    let d = Dir::new("order");
    let (write, rec) = recording(|n| Duration::from_millis(42u64.saturating_sub(2 * n).max(2)));
    let w = SettingsWriter::new(d.settings(), write);
    for n in 1..=20 {
        if n == 10 {
            locked(|| w.save_now(&marked(n))).expect("the synchronous save");
        } else {
            locked(|| w.queue(marked(n), |_| {}));
        }
    }
    assert_eq!(w.flush(Duration::from_secs(60)), Flushed::Saved);
    let started = rec.started();
    assert!(
        started.windows(2).all(|p| p[0] < p[1]),
        "every write carries a newer snapshot than the one before it: {started:?}"
    );
    assert_eq!(started.last(), Some(&20), "the newest is written last");
    assert_eq!(on_disk(&d.settings()), 20);
    assert_eq!(rec.most.load(SeqCst), 1, "one write on the file at a time");
}

/// The same four threads at a time, mixing queued snapshots with synchronous saves under one lock,
/// as the commands and Remote do under the Engine lock: whichever reaches the file first, nothing
/// older is ever written after something newer, and the newest ends on the disk.
#[test]
fn synchronous_saves_and_queued_snapshots_land_in_order() {
    let d = Dir::new("mixed");
    let (write, rec) = recording(|n| Duration::from_micros((n % 5) * 400));
    let w = SettingsWriter::new(d.settings(), write);
    let engine = Mutex::new(Engine::new("KD9TAW", "EN52", 0));
    std::thread::scope(|scope| {
        for t in 0..4u64 {
            let (w, engine) = (&w, &engine);
            scope.spawn(move || {
                for i in 0..50u64 {
                    let mut eng = engine_lock(engine);
                    let next = eng.settings().qrz_last_sync_unix + 1;
                    eng.set_qrz_sync_cursor(next);
                    if (i + t) % 3 == 0 {
                        w.save_now(eng.settings()).expect("the synchronous save");
                    } else {
                        w.queue(eng.settings().clone(), |_| {});
                    }
                }
            });
        }
    });
    assert_eq!(w.flush(Duration::from_secs(60)), Flushed::Saved);
    let started = rec.started();
    assert!(
        started.windows(2).all(|p| p[0] < p[1]),
        "every write carries a newer snapshot than the one before it: {started:?}"
    );
    assert_eq!(on_disk(&d.settings()), 200);
    assert_eq!(rec.most.load(SeqCst), 1, "one write on the file at a time");
}

/// ★ THE LAST CHANGE REACHES THE DISK: a quit's flush returns once the newest snapshot is on the
/// file, however slow the disk — and a bounded flush gives up rather than hold a quit forever.
#[test]
fn flush_waits_for_the_last_change() {
    let d = Dir::new("flush");
    let (write, _) = recording(|_| Duration::from_millis(30));
    let w = SettingsWriter::new(d.settings(), write);
    for n in 1..=10 {
        locked(|| w.queue(marked(n), |_| {}));
    }
    assert_eq!(w.flush(Duration::from_secs(60)), Flushed::Saved);
    assert_eq!(
        on_disk(&d.settings()),
        10,
        "the last change is on the disk the moment the flush returns"
    );

    // A write held at the gate outlasts a short flush, which says it gave up; open the gate and
    // the change lands, and the flush says so.
    let d = Dir::new("flush-bounded");
    let (write, gate, at_gate, _) = gated(1, 0);
    let w = SettingsWriter::new(d.settings(), write);
    let _release = OpenOnDrop(Arc::clone(&gate));
    locked(|| w.queue(marked(1), |_| {}));
    at_gate
        .recv_timeout(Duration::from_secs(10))
        .expect("the write reached the gate");
    let gave_up = w.flush(Duration::from_millis(50)) == Flushed::OutOfTime;
    gate.open();
    assert!(gave_up, "a flush that runs out of time says so");
    assert_eq!(w.flush(Duration::from_secs(60)), Flushed::Saved);
    assert_eq!(on_disk(&d.settings()), 1);
}

/// A flush says what it found: the newest change on the disk, the last write before it failed
/// (the file keeps the snapshot before that one), or time ran out. A quit logs the failure, which
/// the callers' own reports, mostly stderr, never showed an operator.
#[test]
fn a_flush_says_when_the_last_write_failed() {
    let d = Dir::new("flush-failed");
    let (write, _, _, _) = gated(u64::MAX, 2);
    let w = SettingsWriter::new(d.settings(), write);
    locked(|| w.queue(marked(1), |_| {}));
    assert_eq!(w.flush(Duration::from_secs(60)), Flushed::Saved);
    locked(|| w.queue(marked(2), |_| {}));
    assert_eq!(
        w.flush(Duration::from_secs(60)),
        Flushed::Failed("the disk is full".into()),
        "the last write before the flush failed"
    );
    assert_eq!(
        on_disk(&d.settings()),
        1,
        "and the file keeps the snapshot before it"
    );
    locked(|| w.queue(marked(3), |_| {}));
    assert_eq!(
        w.flush(Duration::from_secs(60)),
        Flushed::Saved,
        "a later write that lands clears it"
    );
}

/// A command that answers with the save's result waits for THE write that carried its change, and
/// hears that write's own error.
#[test]
fn a_ticket_answers_with_the_write_that_carried_its_change() {
    let d = Dir::new("ticket");
    let (write, _, _, _) = gated(u64::MAX, 1);
    let w = SettingsWriter::new(d.settings(), write);
    let e = locked(|| w.queue(marked(1), |_| {}))
        .wait()
        .expect_err("the write that carried change 1 failed");
    assert_eq!(e.kind(), io::ErrorKind::StorageFull);
    assert_eq!(e.to_string(), "the disk is full");
    locked(|| w.queue(marked(2), |_| {}))
        .wait()
        .expect("change 2 is on the disk");
    assert_eq!(on_disk(&d.settings()), 2);
}

/// ★ THE NEWEST WINS, AND EVERY CALLER IT CARRIES HEARS HOW IT WENT. With the writer held
/// mid-write, three changes wait: they are written once, as the newest, and when that write fails
/// each of the three callers' own reports runs. A later write that succeeds reports nothing.
#[test]
fn waiting_changes_are_written_once_and_a_failure_reaches_each_caller() {
    let d = Dir::new("coalesce");
    let (write, gate, at_gate, rec) = gated(1, 4);
    let w = SettingsWriter::new(d.settings(), write);
    let _release = OpenOnDrop(Arc::clone(&gate));
    let told = Arc::new(Mutex::new(Vec::new()));
    let tell = |who: &'static str| {
        let told = Arc::clone(&told);
        move |e: &io::Error| lock(&told).push(format!("{who}: {e}"))
    };
    locked(|| w.queue(marked(1), tell("one")));
    at_gate
        .recv_timeout(Duration::from_secs(10))
        .expect("the first write reached the gate");
    locked(|| {
        w.queue(marked(2), tell("two"));
        w.queue(marked(3), tell("three"));
        w.queue(marked(4), tell("four"));
    });
    gate.open();
    assert_eq!(
        w.flush(Duration::from_secs(60)),
        Flushed::Failed("the disk is full".into()),
        "the flush says the last write failed"
    );
    assert_eq!(
        rec.started(),
        [1, 4],
        "the three waiting changes, written once"
    );
    assert_eq!(
        *lock(&told),
        [
            "two: the disk is full",
            "three: the disk is full",
            "four: the disk is full"
        ],
        "each caller the failed write carried is told, in order"
    );
    assert_eq!(
        on_disk(&d.settings()),
        1,
        "a failed write publishes nothing"
    );

    locked(|| w.queue(marked(5), tell("five")));
    assert_eq!(w.flush(Duration::from_secs(60)), Flushed::Saved);
    assert_eq!(lock(&told).len(), 3, "a write that succeeds tells no one");
    assert_eq!(on_disk(&d.settings()), 5);
}

/// A flush returns only once every caller a failed write carried has been told: a quit's last
/// failure is reported before the process goes, and the reports are in by the time anyone reads
/// them. Here the report is held part way, and the flush waits for it. (Seen red first under
/// load, as the test above reading its reports a moment before the thread made them.)
#[test]
fn a_flush_returns_once_a_failed_write_has_been_reported() {
    let d = Dir::new("told");
    let (write, _, _, _) = gated(u64::MAX, 1);
    let w = Arc::new(SettingsWriter::new(d.settings(), write));
    let report = Gate::shut();
    let _release = OpenOnDrop(Arc::clone(&report));
    let (telling, being_told) = mpsc::channel();
    {
        let report = Arc::clone(&report);
        locked(|| {
            w.queue(marked(1), move |_| {
                let _ = telling.send(());
                report.pass();
            })
        });
    }
    being_told
        .recv_timeout(Duration::from_secs(10))
        .expect("the failure is being reported");
    let (flushed, returned) = mpsc::channel();
    let flusher = {
        let w = Arc::clone(&w);
        std::thread::spawn(move || {
            let _ = flushed.send(w.flush(Duration::from_secs(60)));
        })
    };
    // Read, THEN open, THEN judge: a red here must not leave the report held, or the writer's
    // drop would wait on it forever.
    let early = returned.recv_timeout(Duration::from_millis(300));
    report.open();
    assert!(
        early.is_err(),
        "the flush waits while the report is still going out"
    );
    assert_eq!(
        returned.recv_timeout(Duration::from_secs(10)),
        Ok(Flushed::Failed("the disk is full".into())),
        "and returns once it is out, saying the write failed"
    );
    flusher.join().expect("the flush");
}

/// The base profile's mirrors read the file and write it back: a snapshot handed over before them
/// is written first, so the read sees it and their write keeps it. An edit that changes nothing
/// writes nothing.
#[test]
fn rewrite_reads_back_what_was_handed_over_before_it() {
    let d = Dir::new("rewrite");
    let (write, rec) = recording(|_| Duration::from_millis(40));
    let w = SettingsWriter::new(d.settings(), write);
    locked(|| w.queue(marked(5), |_| {}));
    let mut seen = None;
    w.rewrite(|s| {
        seen = Some(s.qrz_last_sync_unix);
        s.simultaneous_radios = true;
        true
    })
    .expect("rewritten");
    assert_eq!(
        seen,
        Some(5),
        "the read saw the snapshot handed over before it"
    );
    let back = Settings::load(&d.settings());
    assert_eq!(
        (back.qrz_last_sync_unix, back.simultaneous_radios),
        (5, true)
    );

    let writes = rec.started().len();
    w.rewrite(|_| false).expect("nothing to write");
    assert_eq!(
        rec.started().len(),
        writes,
        "an edit that changes nothing writes nothing"
    );
    assert_eq!(rec.most.load(SeqCst), 1, "one write on the file at a time");
}

/// ★ THE RADIO LOOP NO LONGER WAITS ON THE DISK. A command changes a setting under the Engine lock
/// and hands the snapshot over before it lets go, as every converted command does. With the write
/// held at a gate, the command has returned and the Engine lock is free while the file is still
/// being written. (Made synchronously instead, as the commands saved before this writer, the
/// command cannot return while the write is held: that is how this test was first seen red.)
#[test]
fn the_engine_lock_is_free_while_the_change_is_being_written() {
    let d = Dir::new("lock");
    let (write, gate, at_gate, _) = gated(7, 0);
    let w = Arc::new(SettingsWriter::new(d.settings(), write));
    let _release = OpenOnDrop(Arc::clone(&gate));
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    let (returned, answered) = mpsc::channel();
    let command = {
        let (w, engine) = (Arc::clone(&w), Arc::clone(&engine));
        std::thread::spawn(move || {
            let mut eng = engine_lock(&engine);
            eng.set_qrz_sync_cursor(7);
            let saved = w.queue(eng.settings().clone(), |_| {});
            drop(eng);
            let _ = returned.send(());
            saved
        })
    };
    at_gate
        .recv_timeout(Duration::from_secs(10))
        .expect("the write started");
    let answered = answered.recv_timeout(Duration::from_secs(2));
    let free = engine_try_lock(&engine).is_ok();
    let written_meanwhile = d.settings().exists();
    gate.open();
    let saved = command.join().expect("the command");
    assert!(
        answered.is_ok(),
        "the command held the Engine lock across the disk write: it could not return while the \
         write was held"
    );
    assert!(
        free,
        "the Engine lock is free while the change is being written"
    );
    assert!(!written_meanwhile, "premise: the write really was held");
    saved.wait().expect("written");
    assert_eq!(on_disk(&d.settings()), 7);
    assert_eq!(engine_lock(&engine).settings().qrz_last_sync_unix, 7);
}

/// The lock-hold measurement behind this writer: how long a settings change holds the Engine lock
/// with the save made inside the hold (the old commands) and with the snapshot handed over (the
/// new). `cargo test -p tempo-app --lib settings::writer -- --ignored --nocapture`; set
/// `NEXUS_MEASURE_DIR` to time a particular disk (the default is the temp folder).
#[test]
#[ignore = "a measurement, not a check"]
fn measure_how_long_a_settings_change_holds_the_engine_lock() {
    let dir = std::env::var_os("NEXUS_MEASURE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("nexus-settings-measure-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    let engine = Mutex::new(Engine::new("KD9TAW", "EN52", 0));
    {
        // A three-radio station, like the operator's.
        let mut eng = engine_lock(&engine);
        eng.add_radio();
        eng.add_radio();
    }
    let rounds = 300;
    let hold = |persist: &dyn Fn(&Engine)| -> Vec<Duration> {
        (0..rounds)
            .map(|i| {
                let mut eng = engine_lock(&engine);
                let t = Instant::now();
                eng.set_tx_level((i % 100) as f32 / 100.0);
                persist(&eng);
                let held = t.elapsed();
                drop(eng);
                std::thread::sleep(Duration::from_millis(2));
                held
            })
            .collect()
    };
    let before = hold(&|eng| eng.settings().save(&path).expect("saved"));
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let w = SettingsWriter::new(path.clone(), Arc::new(Settings::save));
    let after = hold(&|eng| {
        w.queue(eng.settings().clone(), |e| panic!("the write failed: {e}"));
    });
    assert_eq!(w.flush(Duration::from_secs(60)), Flushed::Saved);
    let stats = |mut v: Vec<Duration>| {
        v.sort();
        let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
        format!(
            "median {:?}  p90 {:?}  p99 {:?}  max {:?}",
            at(0.5),
            at(0.9),
            at(0.99),
            v[v.len() - 1]
        )
    };
    println!("settings.json: {bytes} bytes in {}", dir.display());
    println!(
        "lock held, save inside the hold ({rounds} changes): {}",
        stats(before)
    );
    println!(
        "lock held, snapshot handed over ({rounds} changes): {}",
        stats(after)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
