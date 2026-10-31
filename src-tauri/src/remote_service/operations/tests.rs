#[path = "audio_tests.rs"]
mod audio;
#[path = "lan_lease_tests.rs"]
mod lan_lease;
#[path = "logging_tests.rs"]
mod logging;
#[path = "transmit_tests.rs"]
pub(crate) mod transmit;
use super::*;
use crate::remote_service::stored_log_tests::StoredLog;
use std::sync::Arc;
const DEVICE: &str = "10000000-0000-4000-8000-000000000001";
const SESSION: &str = "20000000-0000-4000-8000-000000000001";
const OTHER: &str = "30000000-0000-4000-8000-000000000001";
fn id() -> String {
    super::snapshot_id().unwrap()
}

/// Exclusive use of the process-wide satellite track badge for the length of one test.
///
/// `SAT_TRACK` and `SAT_TRACK_GEN` are process-wide, and THREE kinds of test reach them:
///
/// * the satellite tests, which arm a badge (`test_live_sat_track`) and then assert on it;
/// * **every test that issues a station Stop** — `stop_station` disarms the satellite track
///   (`transmit_stop` → `satellite::disarm_track` → `disarm_sat_track_locked`), which bumps the
///   generation and TAKES the badge, whoever put it there;
/// * **every test that points the rotator** — a point is REFUSED while a track is live
///   (`satellite_track_live` feeds `rotator::queue`), so a sibling's badge turns its "applied"
///   into a "rejected".
///
/// Only the first kind used to hold this, so a sibling landing inside a satellite test
/// (correctly) won, in three shapes seen in real runs:
///
/// * between `test_live_sat_track`'s `fetch_add` and its generation check, the badge is never
///   published at all — `assertion failed: crate::SAT_TRACK…is_some()`;
/// * after it is published, the sibling's disarm takes it, so that test's OWN Stop then sees
///   `was_live == false` and skips the dial handback it exists to prove — "the dial is the
///   operator's again";
/// * the other way round, a satellite test's live badge reaches a rotator test and its point is
///   refused — `left: String("rejected"), right: "applied"`.
///
/// Neither is a product race: the shipped app has one engine and one track, and the badge, the
/// generation and the handback are coherent for it. It is two tests sharing one global.
///
/// TAKE IT ONCE, at the top of the test body, and never again inside anything that body calls:
/// `std::sync::Mutex` is not reentrant, so a second take deadlocks rather than flakes. It also
/// starts the test from an idle badge, so "no track is running" is a fact the test established
/// rather than whatever the previous one happened to leave behind.
fn alone() -> std::sync::MutexGuard<'static, ()> {
    crate::sat_track_alone()
}

/// ⭐ THE GUARD ON THE GUARD — [`alone`] is a convention, and a convention nobody can see is one
/// the next test quietly breaks. This COMPUTES the rule over the source of the three files whose
/// tests reach the badge: every `#[test]` whose body writes it (a Stop) or reads it (a rotator
/// point) must hold the guard.
///
/// It is here rather than in any one of them because the rule spans all of them, and because the
/// collision it prevents is invisible until it flakes — it did, in two different agents' gate
/// runs and again in a third, before anyone went looking.
///
/// ⚠️ **WHAT THIS RULE STILL CANNOT REACH, and one misleading error on the way out.**
///
/// A test can touch the badge **transitively**, through a product function, with its body naming
/// nothing to match on. `query::navigation::tests::connect_uses_the_actual_prediction…` calls
/// `connect()`, which reaches `navigation::live`, which does
/// `crate::SAT_TRACK.try_lock().map_err(|_| "applicationBusy")` — so **`applicationBusy` on that
/// path is not always about the application being busy; a lost `try_lock` on the badge produces
/// the same word.** That test was seen red once in 60 oversubscribed runs on 2026-09-21 for
/// exactly that reason, and it is NOT covered below: a text predicate cannot see a lock taken two
/// calls away. Left as a known member of this family rather than papered over — chasing it with a
/// wider predicate would match half the suite.
///
/// Measured the same day, for whoever picks that up: guarding one more test added exactly ONE
/// `SAT_TRACK` acquisition per run (25 bodies calling `alone()` → 26) against a floor of 237 from
/// `disarm_sat_track_locked` alone, plus one per pass tick. The guard's own hold is unchanged —
/// a statement-scoped temporary, as it always was.
#[test]
fn every_test_that_touches_the_track_badge_takes_the_guard() {
    // ⚠️ FIVE FILES, NOT THREE. The list was the three beside this one, and a Stop in a fourth
    // walked straight past it: `remote_service/tests.rs`'s
    // `a_stop_is_admitted_while_the_stations_sends_are_stuck_on_a_slow_link` bumped
    // `SAT_TRACK_GEN` unguarded and killed a pass flying in `lib.rs` (measured 2026-09-21 — the
    // disarm and the pass's bail-out were caught on adjacent lines, named by thread). A rule
    // enforced over a subset of the files it applies to is the shape of every flake this guard
    // was built to stop, so the list is now every file that has a test reaching the badge.
    // The stream's tests are a sixth (2026-10-03): their Stops on the data channel reach
    // `stop_station` as any other does, and three of them ran unguarded outside this list.
    // The LAN road's two are a seventh and eighth (2026-10-04): a LAN controller's Stop reaches it
    // the same way, on the authority and on the LAN channel. Stop's second road is a ninth
    // (2026-10-07): its Stop runs every stop verb on a thread of its own, the satellite disarm
    // among them. The bench's road switch is a tenth: a Stop pressed on a client-mode screen goes
    // on that road too. The tests of a start in flight behind a Stop are an eleventh: one takes its
    // Stop on the road. Those of a start or a press a later Stop overtook, and of the road through a
    // relay, are a twelfth and a thirteenth: their Stops go on the road too.
    const SOURCES: [(&str, &str); 12] = [
        ("lan_lease_tests.rs", include_str!("lan_lease_tests.rs")),
        (
            "remote_service/lan/tests.rs",
            include_str!("../lan/tests.rs"),
        ),
        ("transmit_tests.rs", include_str!("transmit_tests.rs")),
        ("tests.rs", include_str!("tests.rs")),
        ("remote_service/tests.rs", include_str!("../tests.rs")),
        (
            "remote_service/stream/tests.rs",
            include_str!("../stream/tests.rs"),
        ),
        (
            "remote_service/lan/tests/stop_road.rs",
            include_str!("../lan/tests/stop_road.rs"),
        ),
        (
            "remote_service/lan/tests/stop_road/bench.rs",
            include_str!("../lan/tests/stop_road/bench.rs"),
        ),
        (
            "remote_service/lan/tests/stop_road/stand_down.rs",
            include_str!("../lan/tests/stop_road/stand_down.rs"),
        ),
        (
            "remote_service/lan/tests/stop_road/stop_count.rs",
            include_str!("../lan/tests/stop_road/stop_count.rs"),
        ),
        (
            "remote_service/lan/tests/stop_road/relayed.rs",
            include_str!("../lan/tests/stop_road/relayed.rs"),
        ),
        ("lib.rs", include_str!("../../lib.rs")),
    ];
    /// The two ways a test's outcome depends on the process-wide badge. Named for the rule, not
    /// for one of its halves: a reader is caught by exactly the same convention.
    ///
    /// WRITE — a Stop reaches `stop_station`, which disarms the satellite track: it bumps the
    /// generation and takes the badge, whoever put it there.
    ///
    /// READ — a rotator POINT is refused while a track is live (`station::satellite_track_live`
    /// feeds `rotator::queue`'s `tracking`), so a sibling's live badge turns an "applied" into a
    /// "rejected". That half was found the same day as the write half, from the same collision
    /// wearing the other hat: "left: String(\"rejected\"), right: \"applied\"" on two rotator
    /// tests, interleaved in the log with a satellite test that held a live badge. A Stop is
    /// never refused that way and needs no guard for reading.
    ///
    /// These are the shapes each reaches the badge in; a new shape belongs here beside them.
    /// ⚠️ **THE WIRE SPELLING IS A SHAPE OF ITS OWN, and leaving it out is what let the fourth
    /// file through.** A test that sends the Stop as JSON says `"stopTransmit"`, not the Rust
    /// enum's `StopTransmit`, so a case-sensitive match on the enum name saw nothing while the
    /// request reached `stop_station` exactly as any other Stop does. `run_simulated_pass` and
    /// `test_live_sat_track` are the shapes `lib.rs` reaches it in.
    fn touches_badge(body: &str) -> bool {
        body.contains("StopTransmit")
            || body.contains("\"stopTransmit\"")
            || body.contains("stop_request(")
            || body.contains("stop_v4(")
            || body.contains("Up::Stop")
            || body.contains("road_stop(")
            // The computer's own road, whose Stop now takes its number.
            || body.contains("road.stop(")
            // A Stop pressed on a client-mode screen, which goes by Stop's road as well.
            || body.contains("stop_from_the_screen(")
            || body.contains("rotator.point")
            || body.contains("test_live_sat_track")
            || body.contains("run_simulated_pass")
            || body.contains("TakeControl")
            || body.contains("\"takeControl\"")
            || body.contains("take_on(")
    }
    /// `run_simulated_pass` counts as guarded: its helper takes the same mutex around the whole
    /// pass (`lib.rs`, `run_simulated_pass_as`). That is a DIFFERENT shape from `alone()` — taken
    /// in the callee rather than the body — and it is admitted here rather than silently, so the
    /// next reader knows there are two and why.
    fn guarded(body: &str) -> bool {
        body.contains("alone()") || body.contains("run_simulated_pass")
    }

    let mut bodies: Vec<(&str, String, String)> = Vec::new();
    for (file, src) in SOURCES {
        for (name, body) in test_bodies(src) {
            bodies.push((file, name, body));
        }
    }
    // CONTROL: the extractor really found the tests. A brace-matching scan that silently returns
    // nothing — or merges every body into one — would otherwise report a clean sheet forever.
    assert!(
        bodies.len() >= 30,
        "the scan found only {} test bodies in three files",
        bodies.len()
    );
    let stopping: Vec<&(&str, String, String)> =
        bodies.iter().filter(|(_, _, b)| touches_badge(b)).collect();
    assert!(
        stopping.len() >= 16,
        "the scan found only {} tests reaching the badge",
        stopping.len()
    );
    // …and it sees the two specific tests this rule was written about, one per file, and a Stop
    // the stream's page sends on its data channel.
    for named in [
        "a_delayed_stop_still_stops_what_is_on_the_air_now",
        "a_remote_stop_ends_an_active_satellite_track_and_hands_the_dial_back",
        "blind_input_is_refused_and_a_blind_stop_still_stops",
    ] {
        assert!(
            stopping.iter().any(|(_, name, _)| name == named),
            "{named} reaches the badge and the scan did not see it"
        );
    }

    let offenders: Vec<String> = stopping
        .iter()
        .filter(|(_, _, body)| !guarded(body))
        .map(|(file, name, _)| format!("{file}::{name}"))
        .collect();
    assert!(
        offenders.is_empty(),
        "these tests reach the process-wide satellite badge without `let _alone = alone();` — a \
         Stop disarms the track and a rotator point is refused while one is live, so they race \
         every satellite test: {offenders:?}"
    );

    // CONTROL: the detector fires. Both directions, on bodies written to break each rule.
    assert!(
        touches_badge("let r = stop_request(&f, &state);") && !guarded("let r = stop_request(&f);")
    );
    assert!(
        touches_badge("json!({\"action\":\"rotator.point\"})")
            && !touches_badge("json!({\"action\":\"rotator.stop\"})")
    );
    assert!(guarded("let _alone = alone();") && !touches_badge("let _alone = alone();"));
}

/// Every `#[test] fn NAME() { … }` in `src`, as `(name, body)`, by brace matching.
#[cfg(test)]
fn test_bodies(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(at) = rest.find("#[test]") {
        let after = &rest[at + "#[test]".len()..];
        let (Some(fn_at), Some(open)) = (after.find("fn "), after.find('{')) else {
            break;
        };
        let name: String = after[fn_at + 3..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let mut depth = 0usize;
        let mut end = open;
        for (i, b) in after.as_bytes().iter().enumerate().skip(open) {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push((name, after[open..=end].to_string()));
        rest = &after[end..];
    }
    out
}
struct Fixture {
    authority: Arc<Authority>,
    engine: crate::SharedEngine,
    connection: u64,
    dir: std::path::PathBuf,
    /// The test's clock ([`Fixture::now`]): an hour ahead of the box's, and still.
    clock: Instant,
    /// Declared last, so dropped last: the folder goes once the engine has, and with it every
    /// thread of the engine's that writes the log there (the 1.13 path's `log.adi` lane, the
    /// store's mirror), each joined after its last try at what it still owed.
    _folder: Folder,
}
/// A fixture's folder, removed when it is dropped.
struct Folder(std::path::PathBuf);
impl Drop for Folder {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("nexus-log-operation-{}", id()));
        std::fs::create_dir(&dir).unwrap();
        let mut engine = tempo_app::engine::Engine::new("W9XYZ", "EN52", 0);
        let mut settings = engine.settings().clone();
        settings.save_qso_wav = false;
        engine.apply_settings(settings);
        engine.set_log_path(dir.join("contacts.adi"));
        let authority = Arc::new(Authority::default());
        // The normal operating state: the station has heard the account's device list and it names
        // no sign-in device, so an ordinary controller is free. A test about the sign-in transmit
        // hold overrides this with `set_signin_devices`, or clears it with `forget_signin_devices`
        // to exercise the pre-list (restart) window.
        authority.set_signin_devices(std::collections::BTreeSet::new());
        // Wired exactly as `Service::start` wires it, so a stand-down at the shack retires
        // remote transmit here too — a fixture that skipped this would prove nothing.
        engine.set_remote_transmit_revocation(authority.transmit_revocation());
        let connection = authority.start_connection();
        Self {
            authority,
            engine: Arc::new(Mutex::new(engine)),
            connection,
            _folder: Folder(dir.clone()),
            dir,
            clock: Instant::now() + Duration::from_secs(3600),
        }
    }
    /// The time to hand the authority, which takes `now` with every request: the test's own
    /// clock, which moves only where a test asks at a later time (`f.now() + LEASE`). The box's
    /// clock let a five-second lease lapse in a test that never heartbeats, whenever a loaded box
    /// stopped the test's process for longer than that. Where the authority reads the box's clock
    /// itself (a permit's reconcile, a command's check as it runs, a hardware permit's deadline),
    /// that clock is an hour behind every deadline set on this one, so it ends nothing either.
    fn now(&self) -> Instant {
        self.clock
    }
    /// The same station with its log owned by the STORE — the ordinary launch since the
    /// logbook moved into its database — rather than by `contacts.adi` directly.
    fn with_store() -> Self {
        let f = Self::new();
        {
            let mut e = f.engine.lock().unwrap();
            let opened = tempo_app::logstore::open(
                &f.dir.join("contacts.adi"),
                Arc::new(|_| tempo_core::logbook::sqlite::Resolved::default()),
                None,
            )
            .expect("the store opens");
            e.attach_log_store(opened);
        }
        f
    }
    fn run(&self, request: &Request, now: Instant) -> Result<Value, &'static str> {
        self.authority.handle_version(
            (self.connection, 4),
            SESSION,
            DEVICE,
            request,
            &self.engine,
            now,
        )
    }
    fn state(&self, now: Instant) -> Value {
        self.run(&Request::State { request_id: id() }, now).unwrap()
    }
    fn acquire(&self, now: Instant) -> Value {
        self.authority.permit_station(DEVICE, true).unwrap();
        self.run(
            &Request::Acquire {
                request_id: id(),
                station_boot_id: self.state(now)["stationBootId"].as_str().unwrap().into(),
            },
            now,
        )
        .unwrap()
    }
    /// Install the real radio-loop hold source, exactly as `Service::start` does, so a
    /// `poll_remote_transmit` tick re-derives the sign-in transmit hold from the live lease. Used by
    /// the tests that model a held controller DEPARTING with no further request.
    fn wire_tx_hold(&self) {
        let authority = self.authority.clone();
        self.engine
            .lock()
            .unwrap()
            .set_remote_tx_hold_source(Arc::new(move |now| authority.current_transmit_hold(now)));
    }
}
/// ★ THE FOLDER GOES LAST. Since SPEC-2 v3 C19 (D1-A) a session on the 1.13 path writes its
/// `log.adi` a moment after each change, on a thread of its own (the log's lane), and the lane
/// makes what it still owes one last try as the engine goes: the engine's drop returns once it
/// has. A fixture that removed its folder while its engine lived raced that lane: a write landing
/// during the removal failed it (directory not empty), and one landing after it made the folder
/// again. Here the lane still owes a write when the fixture goes (the folder refuses the new
/// `contacts.adi`), and the folder is gone once the fixture is, and stays gone. The control:
/// once the folder takes the file, the engine's own drop writes it before it returns.
#[cfg(unix)]
#[test]
fn the_fixtures_folder_goes_after_its_engine_and_the_logs_lane() {
    use std::os::unix::fs::PermissionsExt;
    let refuse_new_files = |dir: &std::path::Path, refuse: bool| {
        let mode = if refuse { 0o500 } else { 0o700 };
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    let owing = || {
        let f = Fixture::new();
        refuse_new_files(&f.dir, true);
        f.engine.lock().unwrap().import_adif(
            "<CALL:4>W1AW<BAND:3>20m<MODE:3>SSB<QSO_DATE:8>20260909<TIME_ON:6>010000<EOR>",
        );
        assert!(
            f.engine
                .lock()
                .unwrap()
                .flush_log_store(Duration::from_millis(200))
                .is_err(),
            "premise: log.adi is still owed the contact"
        );
        f
    };

    let f = owing();
    refuse_new_files(&f.dir, false);
    let Fixture {
        engine,
        dir,
        _folder: folder,
        ..
    } = f;
    assert_eq!(Arc::strong_count(&engine), 1, "premise: the only handle");
    drop(engine);
    assert_eq!(
        tempo_core::logbook::Logbook::load(&dir.join("contacts.adi")).len(),
        1,
        "control: the engine's drop wrote what log.adi was owed"
    );
    drop(folder);

    let f = owing();
    let dir = f.dir.clone();
    drop(f);
    assert!(
        !dir.exists(),
        "the folder is gone, and nothing made it again"
    );
}

fn control_state_version(f: &Fixture, now: Instant, version: u8) -> Value {
    f.authority
        .handle_version(
            (f.connection, version),
            SESSION,
            DEVICE,
            &Request::State { request_id: id() },
            &f.engine,
            now,
        )
        .unwrap()
}

fn acquire_controls(f: &Fixture, now: Instant) -> Value {
    acquire_controls_version(f, now, 2)
}

fn acquire_controls_version(f: &Fixture, now: Instant, version: u8) -> Value {
    f.authority.permit_station(DEVICE, true).unwrap();
    f.authority
        .handle_version(
            (f.connection, version),
            SESSION,
            DEVICE,
            &Request::Acquire {
                request_id: id(),
                station_boot_id: control_state_version(f, now, version)["stationBootId"]
                    .as_str()
                    .unwrap()
                    .into(),
            },
            &f.engine,
            now,
        )
        .unwrap()
}

/// The radio loop's authority poll says whether a computer holds the lease, presence or not: the
/// receive decoders count it as attached (a watch-only internet client has a lease and no
/// presence). It goes with the lease, and it never holds anything from transmit.
#[test]
fn the_authority_poll_says_whether_a_lease_is_held() {
    let f = Fixture::new();
    let now = Instant::now();
    let before = f.authority.current_transmit_hold(now).unwrap();
    assert!(!before.lease_held, "nobody attached");
    f.acquire(now);
    let during = f.authority.current_transmit_hold(now).unwrap();
    assert!(during.lease_held, "the lease is held");
    assert!(!during.held, "and holding it holds nothing from transmit");
    f.authority.disconnect_session(SESSION);
    let after = f.authority.current_transmit_hold(now).unwrap();
    assert!(!after.lease_held, "the computer went: its lease with it");
}

#[test]
fn a_disconnect_takeover_or_reconnect_cannot_restore_a_lease() {
    for kind in ["disconnect", "takeover", "socket"] {
        let mut f = Fixture::new();
        let now = Instant::now();
        let command = Request::Heartbeat {
            request_id: id(),
            lease_id: f.acquire(now)["leaseId"].as_str().unwrap().into(),
        };
        match kind {
            "disconnect" => f.authority.disconnect_session(SESSION),
            "takeover" => f.authority.invalidate(),
            _ => {
                f.authority.retire_connection(f.connection);
                f.connection = f.authority.start_connection();
            }
        }
        assert!(f.run(&command, now).is_err());
        let state = f.state(now);
        assert_ne!(state["phase"], "controlling");
        assert_eq!(state["allowed"], kind != "takeover");
    }
}

#[test]
fn a_revoked_device_gets_local_permission_required_even_while_the_engine_is_busy() {
    for revoke in ["takeover", "deny"] {
        let f = Fixture::new();
        let now = Instant::now();
        let state = f.acquire(now);
        let acquire = Request::Acquire {
            request_id: id(),
            station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        };
        // Positive control: the same busy Engine refuses an allowed device as busy.
        {
            let _busy = f.engine.lock().unwrap();
            assert_eq!(f.run(&acquire, now), Err("stationBusy"));
        }
        match revoke {
            "takeover" => f.authority.invalidate(),
            _ => {
                f.authority.permit_station(DEVICE, false).unwrap();
            }
        }
        // A refusal for permission never depends on Engine contention.
        let busy = f.engine.lock().unwrap();
        assert_eq!(
            f.run(&acquire, now),
            Err("localPermissionRequired"),
            "{revoke}"
        );
        drop(busy);
        assert_eq!(
            f.run(&acquire, now),
            Err("localPermissionRequired"),
            "{revoke}"
        );
    }
}
/// The shack's grant (an Approve, the restrict switch, a sign-in's grant) is not refused
/// `remoteBusy` by the station's own traffic. A live computer's feed checks (`lan_controls`) and the
/// radio loop's tick (`current_transmit_hold`) each take Core many times a second, and a grant that
/// tried Core only once lost to one of them now and then. CONTROL: a Core held for longer than the
/// grant waits still refuses it (`a_station_revoke_does_not_wait_for_a_busy_core`).
#[test]
fn the_shacks_grant_is_not_refused_while_the_station_is_busy() {
    use std::sync::atomic::AtomicBool;
    let f = Fixture::new();
    f.wire_tx_hold();
    let now = f.now();
    f.acquire(now);
    let done = Arc::new(AtomicBool::new(false));
    let busy = |work: Box<dyn Fn() + Send>| {
        let done = done.clone();
        std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                work();
                std::thread::sleep(Duration::from_micros(20));
            }
        })
    };
    let (authority, connection) = (f.authority.clone(), f.connection);
    let feeds = busy(Box::new(move || {
        authority.lan_controls(connection, DEVICE, now);
    }));
    let engine = f.engine.clone();
    let ticks = busy(Box::new(move || {
        engine.lock().unwrap().poll_remote_transmit(now);
    }));
    let refused = (0..2000)
        .filter(|_| f.authority.permit_station(OTHER, true).is_err())
        .count();
    done.store(true, Ordering::SeqCst);
    feeds.join().unwrap();
    ticks.join().unwrap();
    assert_eq!(refused, 0, "the shack's grant was refused remoteBusy");
}
/// #318: a station-control revoke must not fail because Core is busy — it used to
/// return `remoteBusy` and leave the grant standing.
#[test]
fn a_station_revoke_does_not_wait_for_a_busy_core() {
    let f = Fixture::new();
    let now = Instant::now();
    let state = acquire_controls_version(&f, now, 4);
    let command = Request::Acquire {
        request_id: id(),
        station_boot_id: state["stationBootId"].as_str().unwrap().into(),
    };
    assert_eq!(state["phase"], "controlling", "holds the lease");
    let transmit = f.authority.transmit.permit(now + LEASE).unwrap();

    let core = f.authority.core.lock().unwrap(); // an operation in flight
    let revoke = f.authority.permit_station(DEVICE, false);
    assert_eq!(revoke, Ok(()), "revoke must not fail");
    // A grant still may, and must not widen anything.
    assert_eq!(f.authority.permit_station(OTHER, true), Err("remoteBusy"));
    assert!(
        !transmit.valid(now),
        "the controller's in-flight transmit permit ends at once"
    );
    drop(core);

    // The device's next command is refused, and its lease is gone.
    assert_eq!(f.run(&command, now), Err("localPermissionRequired"));
    let status = f.authority.local_status();
    assert_eq!(status["controller"], Value::Null, "lease ended");
    assert_eq!(status["controlDevices"], json!([]), "OTHER not granted");
    assert!(!transmit.valid(now));
}

#[test]
fn a_command_refuses_a_busy_engine_instead_of_queueing_and_counter_wrap() {
    let f = Fixture::new();
    let now = Instant::now();
    f.acquire(now);
    let command = Request::State { request_id: id() };
    let lock = f.engine.lock().unwrap();
    assert_eq!(f.run(&command, now), Err("stationBusy"));
    drop(lock);
    f.authority.connection.store(u64::MAX, Ordering::SeqCst);
    assert_eq!(f.authority.start_connection(), u64::MAX);
    assert_eq!(
        f.authority
            .handle_version((u64::MAX, 4), SESSION, DEVICE, &command, &f.engine, now),
        Err("authorityUnavailable")
    );
}

#[test]
fn authority_counter_exhaustion_revokes_already_issued_transmit_permission() {
    let f = Fixture::new();
    let now = Instant::now();
    acquire_controls(&f, now);
    let permit = f
        .authority
        .transmit
        .permit(now + Duration::from_secs(5))
        .unwrap();
    assert!(permit.valid(now));
    {
        let mut core = f.authority.core.lock().unwrap();
        core.revision = MAX_COUNTER;
        assert_eq!(f.authority.advance(&mut core), Err("authorityUnavailable"));
        assert!(core.lease.is_none());
        assert!(core.windows.is_empty());
    }
    assert!(!permit.valid(now));
}

/// True once the process-wide badge is clear. The stop runs on a worker, so this is a wait.
#[cfg(feature = "radio")]
fn track_ended() -> bool {
    for _ in 0..400 {
        if crate::SAT_TRACK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none()
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// A station holding a transponder, with no track running or (`live`) with a live track badge.
#[cfg(feature = "radio")]
fn holding_a_transponder(live: bool) -> Fixture {
    let f = Fixture::new();
    {
        let mut e = f.engine.lock().unwrap();
        e.configure_remote_settings_store(f.dir.join("settings.json"));
        e.set_tx_enabled(false);
        e.set_sat_transponder(Some((
            "SO-50|FM repeater".into(),
            0,
            tempo_core::doppler::Transponder::channel(145_850_000, 436_795_000),
        )));
    }
    if live {
        crate::test_live_sat_track("SO-50");
    }
    f
}

/// The Stop a controlling browser sends, from `state`.
#[cfg(feature = "radio")]
fn stop_request(f: &Fixture, state: &Value) -> Request {
    Request::StopTransmit {
        request_id: id(),
        station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        lease_id: state["leaseId"].as_str().unwrap().into(),
        transmit_epoch: format!("{:016x}", f.authority.transmit.generation()),
    }
}

/// A browser has one Stop, and a satellite track is the app's only standing instruction to keep
/// moving the radio and the mast by itself: the Stop ends it and hands the dial back.
#[test]
#[cfg(feature = "radio")]
fn a_remote_stop_ends_an_active_satellite_track_and_hands_the_dial_back() {
    let _alone = alone();
    let f = holding_a_transponder(true);
    let now = Instant::now();
    // POSITIVE CONTROL: the badge and the hold really are live before the Stop, so "gone
    // afterwards" is about the Stop and not about a fixture that was never set up.
    assert!(crate::SAT_TRACK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some());
    assert!(f.engine.lock().unwrap().sat_transponder_held().is_some());

    let state = acquire_controls_version(&f, now, 3);
    let accepted = f
        .authority
        .handle_version(
            (f.connection, 4),
            SESSION,
            DEVICE,
            &stop_request(&f, &state),
            &f.engine,
            now,
        )
        .unwrap();
    assert_eq!(accepted, json!({"stop":"accepted"}));

    assert!(track_ended(), "the Stop ended the track");
    // Stopping a live track IS the dial handback, exactly as the rail Stop's own verb performs it.
    assert!(
        f.engine.lock().unwrap().sat_transponder_held().is_none(),
        "the dial is the operator's again"
    );
    // ⛔ And it is still a STOP: nothing was armed, keyed or re-enabled on the way.
    assert!(!f.engine.lock().unwrap().tx_enabled());
}

/// A Stop with no track running must leave a transponder the operator STAGED for a later pass
/// exactly where it is — releasing a hold nobody asked to release is doing more than was asked.
/// This is the desktop's own idle-stop rule (`stop_sat_track`'s `was_live`), reached from here.
#[test]
#[cfg(feature = "radio")]
fn a_remote_stop_with_no_track_running_leaves_a_staged_transponder_alone() {
    let _alone = alone();
    let f = holding_a_transponder(false);
    let now = Instant::now();
    let state = acquire_controls_version(&f, now, 3);
    f.authority
        .handle_version(
            (f.connection, 4),
            SESSION,
            DEVICE,
            &stop_request(&f, &state),
            &f.engine,
            now,
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(60));
    assert!(
        f.engine.lock().unwrap().sat_transponder_held().is_some(),
        "an idle Stop must not release a pick staged for a later pass"
    );
}

/// ⭐ **Giving a club position is refused from afar.** The host's Give button is on its own
/// desktop and is none of Remote's operations: a request naming it is refused before anything
/// reads it, whichever spelling it tries. CONTROL: a request the page does send is read.
#[test]
fn giving_a_club_position_is_refused_from_afar() {
    for name in [
        "fdClubGivePosition",
        "fd_club_give_position",
        "givePosition",
        "club.givePosition",
    ] {
        assert!(
            serde_json::from_value::<Request>(json!({"type": name, "requestId": "r", "handle": 1}))
                .is_err(),
            "a request named {name} was read"
        );
    }
    assert!(
        serde_json::from_value::<Request>(json!({"type": "state", "requestId": "r"})).is_ok(),
        "CONTROL: a request the page sends"
    );
}

/// ⭐ **Removing a club position, or letting one back, is refused from afar**, as Give is: both
/// buttons are on the host's own board and neither is one of Remote's operations, whichever
/// spelling a request tries. CONTROL: a request the page does send is read.
#[test]
fn removing_a_club_position_or_letting_one_back_is_refused_from_afar() {
    for name in [
        "fdClubRemovePosition",
        "fd_club_remove_position",
        "removePosition",
        "club.removePosition",
        "fdClubAllowBack",
        "fd_club_allow_back",
        "allowBack",
        "club.allowBack",
    ] {
        assert!(
            serde_json::from_value::<Request>(
                json!({"type": name, "requestId": "r", "posid": "bbbb0002"})
            )
            .is_err(),
            "a request named {name} was read"
        );
    }
    assert!(
        serde_json::from_value::<Request>(json!({"type": "state", "requestId": "r"})).is_ok(),
        "CONTROL: a request the page sends"
    );
}

/// The state's `txArmed` is the station's TX-enable latch, by value, for the browser holding the
/// lease: on → true, off → false. Another browser watching the same station is not told (the
/// page refuses a `txArmed` without the lease), and neither is one older than the stop token.
#[test]
fn tx_armed_reports_the_stations_tx_latch_to_the_controller() {
    let f = Fixture::new();
    let now = Instant::now();
    let state = acquire_controls_version(&f, now, 4);
    assert_eq!(state["phase"], "controlling");
    assert_eq!(state["txArmed"], false, "TX is off at the start");
    for on in [true, false, true] {
        f.engine.lock().unwrap().set_tx_enabled(on);
        assert_eq!(f.engine.lock().unwrap().tx_enabled(), on, "premise");
        let state = control_state_version(&f, now, 4);
        assert_eq!(state["txArmed"], on, "the latch is {on}");
        assert!(state["transmitEpoch"].is_string());
    }
    f.authority.permit_station(OTHER, true).unwrap();
    let watching = f
        .authority
        .handle_version(
            (f.connection, 4),
            OTHER,
            OTHER,
            &Request::State { request_id: id() },
            &f.engine,
            now,
        )
        .unwrap();
    assert_eq!(watching["phase"], "occupied");
    assert_eq!(watching["txArmed"], false, "only the controller is told");
    assert_eq!(control_state_version(&f, now, 3)["txArmed"], false);
}
