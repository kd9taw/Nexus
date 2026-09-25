#[path = "audio_tests.rs"]
mod audio;
#[path = "export_tests.rs"]
mod export;
#[cfg(feature = "radio")]
#[path = "level_tests.rs"]
mod level;
#[path = "logging_tests.rs"]
mod logging;
#[cfg(feature = "radio")]
#[path = "phone_mode_tests.rs"]
mod phone_mode;
#[cfg(feature = "radio")]
#[path = "dsp_tests.rs"]
mod receiver_dsp;
#[cfg(feature = "radio")]
#[path = "filter_tests.rs"]
mod receiver_filter;
#[cfg(feature = "radio")]
#[path = "selection_tests.rs"]
mod selection;
#[path = "settings_tests.rs"]
mod settings;
#[cfg(feature = "radio")]
#[path = "split_tests.rs"]
mod split;
#[cfg(feature = "radio")]
#[path = "spot_tests.rs"]
mod spot;
#[path = "transmit_tests.rs"]
mod transmit;
use super::*;
use crate::remote_service::stored_log_tests::StoredLog;
use std::sync::Arc;
#[cfg(feature = "radio")]
#[path = "ai_cw_tests.rs"]
mod ai_cw;
#[cfg(feature = "radio")]
#[path = "amp_follow_tests.rs"]
mod amp_follow;
#[cfg(feature = "radio")]
#[path = "aprs_tune_tests.rs"]
mod aprs_tune;
#[cfg(feature = "radio")]
#[path = "decoder_settings_tests.rs"]
mod decoder_settings;
#[cfg(feature = "radio")]
#[path = "digital_spot_tests.rs"]
mod digital_spot;
#[cfg(feature = "radio")]
#[path = "memory_tests.rs"]
mod memory;
#[cfg(feature = "radio")]
#[path = "repeater_tests.rs"]
mod repeater;
#[cfg(feature = "radio")]
#[path = "rotator_tests.rs"]
mod rotator;
#[cfg(feature = "radio")]
#[path = "rtty_spot_tests.rs"]
mod rtty_spot;
#[cfg(feature = "radio")]
#[path = "rx_gain_tests.rs"]
mod rx_gain;
#[cfg(feature = "radio")]
#[path = "satellite_tests.rs"]
mod satellite;
#[cfg(feature = "radio")]
#[path = "scope_tests.rs"]
mod scope;
#[cfg(feature = "radio")]
#[path = "sstv_gallery_tests.rs"]
mod sstv_gallery;
#[cfg(feature = "radio")]
#[path = "workspace_tests.rs"]
mod workspace;
const DEVICE: &str = "10000000-0000-4000-8000-000000000001";
const SESSION: &str = "20000000-0000-4000-8000-000000000001";
const OTHER: &str = "30000000-0000-4000-8000-000000000001";
fn id() -> String {
    super::super::query::snapshot_id().unwrap()
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
    const SOURCES: [(&str, &str); 5] = [
        ("transmit_tests.rs", include_str!("transmit_tests.rs")),
        ("satellite_tests.rs", include_str!("satellite_tests.rs")),
        ("rotator_tests.rs", include_str!("rotator_tests.rs")),
        ("remote_service/tests.rs", include_str!("../tests.rs")),
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
            || body.contains("rotator.point")
            || body.contains("test_live_sat_track")
            || body.contains("run_simulated_pass")
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
    // …and it sees the two specific tests this rule was written about, one per file.
    for named in [
        "a_replayed_stop_transmit_has_no_further_effect",
        "a_remote_stop_ends_an_active_satellite_track_and_hands_the_dial_back",
        "rotator_point_needs_v3_its_hint_and_one_rotctld_command_off_the_engine_lock",
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
    authority: Authority,
    engine: crate::SharedEngine,
    connection: u64,
    dir: std::path::PathBuf,
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
        let authority = Authority::default();
        // Wired exactly as `Service::new` wires it, so a stand-down at the shack retires
        // remote transmit here too — a fixture that skipped this would prove nothing.
        engine.set_remote_transmit_revocation(authority.transmit_revocation());
        let connection = authority.start_connection();
        Self {
            authority,
            engine: Arc::new(Mutex::new(engine)),
            connection,
            dir,
        }
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
    /// Every row the store holds, read through a connection of the test's own — what is on
    /// disk, not what the engine remembers.
    fn stored(&self) -> Vec<tempo_core::logbook::QsoRecord> {
        tempo_core::logbook::sqlite::LogDb::open(&self.dir.join("contacts.sqlite3"))
            .and_then(|db| db.load_all())
            .expect("the store reads")
    }
    fn run(&self, request: &Request, now: Instant) -> Result<Value, &'static str> {
        self.authority
            .handle(self.connection, SESSION, DEVICE, request, &self.engine, now)
    }
    fn state(&self, now: Instant) -> Value {
        self.run(&Request::State { request_id: id() }, now).unwrap()
    }
    fn acquire(&self, now: Instant) -> Value {
        self.authority.permit(DEVICE, true).unwrap();
        self.run(
            &Request::Acquire {
                request_id: id(),
                station_boot_id: self.state(now)["stationBootId"].as_str().unwrap().into(),
            },
            now,
        )
        .unwrap()
    }
    fn command(&self, state: &Value) -> Request {
        serde_json::from_value(json!({"type":"logManual","requestId":id(),"stationBootId":state["stationBootId"],"leaseId":state["leaseId"],"expectedRevision":state["revision"],"commandWindowId":state["commandWindowId"],"clientSequence":state["nextSequence"],"record":{"call":"W1AW","grid":"FN31","country":null,"state":"CT","band":"20m","freqMhz":14.25,"mode":"SSB","rstSent":"59","rstRcvd":"57","name":"Joe","qth":"Newington","comment":"Remote test","notes":"Keep this note","whenUnix":super::super::now_ms()/1000,"confirmed":false,"awardConfirmed":false}})).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

fn control_state(f: &Fixture, now: Instant) -> Value {
    control_state_version(f, now, 2)
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

fn control_request(state: &Value, action: Value) -> Request {
    serde_json::from_value(json!({"type":"stationControl", "requestId":id(), "stationBootId":state["stationBootId"],
        "leaseId":state["leaseId"], "expectedRevision":state["revision"], "commandWindowId":state["commandWindowId"],
        "clientSequence":state["nextSequence"], "context":state["controls"]["context"], "action":action})).unwrap()
}

#[test]
#[cfg(feature = "radio")]
fn frequency_admission_waits_for_the_radio_owner_and_revoke_cancels_the_pending_write() {
    for (dial, band, hz) in [(7.074, "40m", 7_074_000), (10.0, "", 10_000_000)] {
        let f = Fixture::new();
        {
            let mut e = f.engine.lock().unwrap();
            e.configure_remote_settings_store(f.dir.join("settings.json"));
            e.set_tx_enabled(false);
            e.set_frequency(14.074, "20m", "USB");
            e.take_immediate_retune();
            let connection = e.remote_open_radio().unwrap();
            let read = e.remote_radio_read(&connection, Instant::now()).unwrap();
            e.remote_observe_cat(Some(&read), Some(true));
            e.remote_observe_dial(Some(&read), Some(14_074_000));
            e.remote_observe_mode(Some(&read), Some("PKTUSB"));
            e.remote_observe_ptt(Some(&read), Some(false));
        }
        let now = Instant::now();
        let state = acquire_controls_version(&f, now, 3);
        let command = control_request(
            &state,
            json!({"action":"radio.frequency","dialMhz":dial,"band":band,"sideband":"USB"}),
        );
        let response = f
            .authority
            .handle_version((f.connection, 3), SESSION, DEVICE, &command, &f.engine, now)
            .unwrap();
        assert_eq!(response["outcome"], "pending");
        let work = {
            let mut e = f.engine.lock().unwrap();
            assert_eq!(e.settings().dial_hz(), 14_074_000);
            assert!(!e.take_immediate_retune());
            e.take_remote_radio().unwrap()
        };
        assert_eq!(work.target(), (hz, "PKTUSB"));
        assert!(work.permission().check(Instant::now()).is_ok());
        assert!(!f.dir.join("settings.json").exists());
        f.authority.permit_station(DEVICE, false).unwrap();
        assert!(work.permission().begin_write(Instant::now()).is_err());
        assert_eq!(f.engine.lock().unwrap().settings().dial_hz(), 14_074_000);
    }
}

#[test]
#[cfg(feature = "radio")]
fn mode_admission_is_pending_until_readback_and_duplicate_or_revoked_work_cannot_replay() {
    for revoke in [false, true] {
        let f = Fixture::new();
        let radio = {
            let mut e = f.engine.lock().unwrap();
            e.configure_remote_settings_store(f.dir.join("settings.json"));
            e.set_tx_enabled(false);
            e.set_frequency(14.074, "20m", "USB");
            e.take_immediate_retune();
            e.remote_open_radio().unwrap()
        };
        let sample = |hz, mode: &str| {
            let mut e = f.engine.lock().unwrap();
            let read = e.remote_radio_read(&radio, Instant::now()).unwrap();
            e.remote_observe_cat(Some(&read), Some(true));
            e.remote_observe_dial(Some(&read), Some(hz));
            e.remote_observe_mode(Some(&read), Some(mode));
            e.remote_observe_ptt(Some(&read), Some(false));
        };
        sample(14_074_000, "PKTUSB");
        let now = Instant::now();
        let state = acquire_controls_version(&f, now, 3);
        let command = control_request(
            &state,
            json!({"action":"radio.mode","mode":"cw","followFrequency":true}),
        );
        let run = |request: &Request| {
            f.authority.handle_version(
                (f.connection, 3),
                SESSION,
                DEVICE,
                request,
                &f.engine,
                Instant::now(),
            )
        };
        let response = run(&command).unwrap();
        assert_eq!(response["outcome"], "pending");
        assert_eq!(run(&command).unwrap(), response);
        let work = {
            let mut e = f.engine.lock().unwrap();
            assert_eq!(e.settings().dial_hz(), 14_074_000);
            assert!(!e.tx_enabled());
            let work = e.take_remote_radio().unwrap();
            assert!(e.take_remote_radio().is_none());
            work
        };
        assert_eq!(work.target(), (14_030_000, "CW"));
        assert!(!f.dir.join("settings.json").exists());
        if revoke {
            f.authority.permit_station(DEVICE, false).unwrap();
            assert!(work.permission().begin_write(Instant::now()).is_err());
            assert_eq!(f.engine.lock().unwrap().settings().dial_hz(), 14_074_000);
        } else {
            work.permission().begin_write(Instant::now()).unwrap();
            sample(work.target().0, work.target().1);
            assert!(work.commit(&mut f.engine.lock().unwrap()));
            let result = run(&Request::Result {
                request_id: id(),
                operation_id: command.id().into(),
            })
            .unwrap();
            assert_eq!(result["outcome"], "applied");
            assert_eq!(result["evidence"], "radioReadback");
            assert_eq!(run(&command).unwrap(), result);
            let mut e = f.engine.lock().unwrap();
            assert_eq!(e.settings().dial_hz(), 14_030_000);
            assert!(!e.tx_enabled());
            assert!(!e.take_immediate_retune());
            assert!(e.take_remote_radio().is_none());
            assert!(f.dir.join("settings.json").exists());
        }
    }
}

#[test]
#[cfg(feature = "radio")]
fn an_amplifier_command_has_one_native_receipt_and_needs_later_hardware_confirmation() {
    let f = Fixture::new();
    let (radio, amp) = {
        let mut e = f.engine.lock().unwrap();
        let mut settings = e.settings().clone();
        settings.ensure_radio_profiles();
        let active = settings.active_radio;
        let p = settings.radios.iter_mut().find(|p| p.id == active).unwrap();
        p.amp_model = "spe".into();
        p.amp_port = "fake-remote-amp".into();
        settings.sync_flat_from_active();
        e.apply_settings(settings);
        e.set_tx_enabled(false);
        (e.remote_open_radio().unwrap(), e.remote_open_amp().unwrap())
    };
    let sample = |operate| {
        let mut e = f.engine.lock().unwrap();
        let r = e.remote_radio_read(&radio, Instant::now()).unwrap();
        e.remote_observe_cat(Some(&r), Some(true));
        e.remote_observe_ptt(Some(&r), Some(false));
        let r = e.remote_amp_read(&amp, Instant::now()).unwrap();
        e.remote_observe_amp(
            Some(&r),
            tempo_app::dto::AmpStatusDto {
                family: "spe".into(),
                linked: true,
                operate: Some(operate),
                transmitting: Some(false),
                output_watts: Some(0),
                band_label: Some("20m".into()),
                ..Default::default()
            },
        );
    };
    sample(false);
    let now = Instant::now();
    let state = acquire_controls(&f, now);
    let command = control_request(
        &state,
        json!({"action":"amplifier.operate","expectedOperate":false,"operate":true}),
    );
    let run = |request: &Request| {
        f.authority.handle_version(
            (f.connection, 2),
            SESSION,
            DEVICE,
            request,
            &f.engine,
            Instant::now(),
        )
    };
    let first = run(&command).unwrap();
    assert_eq!(first["outcome"], "pending");
    assert_eq!(run(&command).unwrap(), first);
    let mut request = f.engine.lock().unwrap().take_remote_amp().unwrap();
    assert!(
        f.engine.lock().unwrap().take_remote_amp().is_none(),
        "a duplicate cannot enqueue a second toggle"
    );
    let result = Request::Result {
        request_id: id(),
        operation_id: command.id().into(),
    };
    assert_eq!(run(&result).unwrap()["outcome"], "pending");
    sample(false);
    request.begin(&f.engine.lock().unwrap()).unwrap();
    assert!(request.begin_write());
    sample(true);
    request.confirm(&f.engine.lock().unwrap());
    let confirmed = run(&result).unwrap();
    assert_eq!(confirmed["outcome"], "applied");
    assert_eq!(confirmed["evidence"], "amplifierReadback");
    assert_eq!(run(&command).unwrap(), confirmed);
    assert!(!f.engine.lock().unwrap().tx_enabled());
}

#[test]
#[cfg(feature = "radio")]
fn tier_admission_requires_v3_and_keeps_one_native_receipt_through_readback() {
    use tempo_app::dto::Tier;
    let f = Fixture::new();
    let connection = {
        let mut e = f.engine.lock().unwrap();
        e.configure_remote_settings_store(f.dir.join("settings.json"));
        e.set_frequency(14.074, "20m", "USB");
        e.take_immediate_retune();
        e.remote_open_radio().unwrap()
    };
    let sample = |hz| {
        let mut e = f.engine.lock().unwrap();
        let read = e.remote_radio_read(&connection, Instant::now()).unwrap();
        e.remote_observe_cat(Some(&read), Some(true));
        e.remote_observe_dial(Some(&read), Some(hz));
        e.remote_observe_mode(Some(&read), Some("PKTUSB"));
        e.remote_observe_ptt(Some(&read), Some(false));
    };
    sample(14_074_000);
    let now = Instant::now();
    let state = acquire_controls_version(&f, now, 3);
    assert_eq!(
        state["controls"]["capabilities"],
        json!([
            "decoder",
            "amplifier",
            "frequency",
            "mode",
            "tier",
            "ampFollowBand",
            "workspace",
            "decoderSettings",
            "receiverSettings",
            "receiverGain",
            "bandSelection",
            "receiverFilter",
            "receiverDsp",
            "phoneMode",
            "workSpot",
            "radioLevels",
            "radioSelection",
            "fmTuning",
            "fmReceiver",
            "aiCw",
            "redecode",
            "splitTuning",
            "ritTuning",
            "workDigitalSpot",
            "repeaterTuning",
            "memoryRecall",
            "aprsTuning",
            "rotator",
            "rigScope",
            "workRttySpot",
            "sstvGallery",
            "satellite"
        ])
    );
    let command = control_request(&state, json!({"action":"radio.tier","tier":"FT4"}));
    let run = |version, request: &Request| {
        f.authority.handle_version(
            (f.connection, version),
            SESSION,
            DEVICE,
            request,
            &f.engine,
            Instant::now(),
        )
    };
    assert_eq!(run(2, &command), Err("stationUnsupported"));
    assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
    let pending = run(3, &command).unwrap();
    assert_eq!(pending["outcome"], "pending");
    assert_eq!(run(3, &command).unwrap(), pending);
    let work = f.engine.lock().unwrap().take_remote_radio().unwrap();
    assert_eq!(f.engine.lock().unwrap().tier(), Tier::Ft8);
    assert_eq!(work.target(), (14_080_000, "PKTUSB"));
    work.permission().begin_write(Instant::now()).unwrap();
    sample(14_080_000);
    assert!(work.commit(&mut f.engine.lock().unwrap()));
    let result = Request::Result {
        request_id: id(),
        operation_id: command.id().into(),
    };
    let applied = run(3, &result).unwrap();
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(applied["evidence"], "radioReadback");
    assert_eq!(run(3, &command).unwrap(), applied);
    assert_eq!(
        run(2, &result).unwrap(),
        applied,
        "a legacy refresh may recover an existing receipt"
    );
    assert_eq!(f.engine.lock().unwrap().tier(), Tier::Ft4);
    assert!(!f.engine.lock().unwrap().tx_enabled());
    assert!(!f.dir.join("settings.json").exists());
}

#[test]
#[cfg(feature = "radio")]
fn legacy_v2_control_state_keeps_its_original_capability_vocabulary() {
    let f = Fixture::new();
    let now = Instant::now();
    acquire_controls(&f, now);
    let state = f
        .authority
        .handle_version(
            (f.connection, 2),
            SESSION,
            DEVICE,
            &Request::State { request_id: id() },
            &f.engine,
            now,
        )
        .unwrap();
    assert_eq!(
        state["controls"]["capabilities"],
        json!(["decoder", "amplifier"])
    );
    assert_eq!(state["phase"], "controlling");
    assert_eq!(state["txArmed"], false);
}

#[test]
fn logging_and_station_permissions_do_not_grant_each_other() {
    let f = Fixture::new();
    let now = Instant::now();
    f.acquire(now);
    let command = control_request(
        &control_state(&f, now),
        json!({"action":"decoder.arm","receiver":"rtty","on":true}),
    );
    assert_eq!(
        f.authority
            .handle_version((f.connection, 2), SESSION, DEVICE, &command, &f.engine, now),
        Err("localPermissionRequired")
    );
    assert!(!f.engine.lock().unwrap().rtty_armed());
    f.authority.permit(DEVICE, false).unwrap();
    let control = acquire_controls(&f, now);
    #[cfg(feature = "radio")]
    assert_eq!(
        control["controls"]["capabilities"],
        json!(["decoder", "amplifier"])
    );
    #[cfg(not(feature = "radio"))]
    assert_eq!(control["controls"]["capabilities"], json!(["decoder"]));
    assert_eq!(control["actions"], json!([]));
    assert_eq!(control["txArmed"], false);
    assert_eq!(
        f.run(&f.command(&control), now),
        Err("localPermissionRequired")
    );
    assert_eq!(
        f.run(
            &control_request(&control, json!({"action":"decoder.clear","receiver":"cw"})),
            now
        ),
        Err("stationUnsupported")
    );
}

#[test]
fn remote_receiver_gestures_use_real_native_state_without_arming_transmit() {
    let f = Fixture::new();
    let now = Instant::now();
    acquire_controls(&f, now);
    for receiver in ["rtty", "psk", "sstv", "aprs"] {
        for on in [true, false, true] {
            let request = control_request(
                &control_state(&f, now),
                json!({"action":"decoder.arm","receiver":receiver,"on":on}),
            );
            let r = f
                .authority
                .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now)
                .unwrap();
            assert_eq!(r["outcome"], "applied");
            assert_eq!(r["evidence"], "receiverState");
            let engine = f.engine.lock().unwrap();
            let armed = match receiver {
                "rtty" => engine.rtty_armed(),
                "psk" => engine.psk_armed(),
                "sstv" => engine.sstv_armed(),
                _ => engine.aprs_armed(),
            };
            assert_eq!(armed, on);
            assert!(!engine.tx_enabled());
            if receiver == "aprs" && on {
                assert_eq!(engine.aprs_arm_source(), tempo_app::engine::AprsArm::Auto);
            }
        }
    }
}

#[test]
fn a_replayed_clear_does_not_erase_text_received_after_the_original_gesture() {
    let f = Fixture::new();
    let now = Instant::now();
    let state = acquire_controls(&f, now);
    let request = control_request(&state, json!({"action":"decoder.clear","receiver":"rtty"}));
    let applied = f
        .authority
        .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now)
        .unwrap();
    f.engine.lock().unwrap().push_rtty_decode(
        &[tempo_core::textmode::DecodedChar {
            ch: 'X',
            confidence: 1.0,
        }],
        0.0,
        false,
    );
    let replay = f
        .authority
        .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now)
        .unwrap();
    assert_eq!(replay, applied);
    assert_eq!(f.engine.lock().unwrap().rtty_state().text, "X");
}

#[test]
fn receiver_net_reacquire_and_psk_mode_reach_the_decoder_without_changing_local_tx() {
    let f = Fixture::new();
    let now = Instant::now();
    f.engine.lock().unwrap().set_tx_enabled(true);
    acquire_controls(&f, now);
    for receiver in ["rtty", "psk"] {
        for action in [
            json!({"action":"decoder.net","receiver":receiver,"hz":1625.0}),
            json!({"action":"decoder.afcReset","receiver":receiver}),
        ] {
            let request = control_request(&control_state(&f, now), action);
            let result = f
                .authority
                .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now)
                .unwrap();
            assert_eq!(result["outcome"], "applied");
            let mut e = f.engine.lock().unwrap();
            if receiver == "rtty" {
                assert_eq!(e.rtty_center_hz(), 1625.0);
                assert!(e.take_rtty_afc_reset());
            } else {
                assert_eq!(e.psk_center_hz(), 1625.0);
                assert!(e.take_psk_afc_reset());
            }
            assert!(e.tx_enabled());
        }
    }
    let request = control_request(
        &control_state(&f, now),
        json!({"action":"decoder.pskMode","mode":"QPSK31","reverse":true}),
    );
    let result = f
        .authority
        .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now)
        .unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(
        f.engine.lock().unwrap().psk_mode(),
        (tempo_core::psk::PskModeKind::Qpsk31, true)
    );
    let invalid = control_request(
        &control_state(&f, now),
        json!({"action":"decoder.net","receiver":"psk","hz":1.0}),
    );
    let result = f
        .authority
        .handle_version((f.connection, 2), SESSION, DEVICE, &invalid, &f.engine, now)
        .unwrap();
    assert_eq!(result["outcome"], "rejected");
    assert_eq!(result["reason"], "invalidAction");
    assert_eq!(f.engine.lock().unwrap().psk_center_hz(), 1625.0);
}

#[test]
fn receiver_control_refuses_local_takeover_and_an_old_click_context() {
    let f = Fixture::new();
    let now = Instant::now();
    let state = acquire_controls(&f, now);
    let request = control_request(
        &state,
        json!({"action":"decoder.arm","receiver":"psk","on":true}),
    );
    f.engine.lock().unwrap().set_frequency(7.074, "40m", "USB");
    assert_eq!(
        f.authority
            .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now),
        Err("staleContext")
    );
    assert!(!f.engine.lock().unwrap().psk_armed());
    let request = control_request(
        &control_state(&f, now),
        json!({"action":"decoder.arm","receiver":"psk","on":true}),
    );
    f.authority.invalidate();
    assert_eq!(
        f.authority
            .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now),
        Err("localPermissionRequired")
    );
    assert!(!f.engine.lock().unwrap().psk_armed());
}

#[test]
fn local_amp_gestures_invalidate_old_windows_without_changing_the_tx_context() {
    let f = Fixture::new();
    let now = Instant::now();
    let state = acquire_controls(&f, now);
    let action = json!({"action":"decoder.arm","receiver":"psk","on":true});
    let request = control_request(&state, action.clone());
    {
        let e = f.engine.lock().unwrap();
        let tx_generation = e.remote_log_context_generation();
        let actuation = e.remote_actuation_context_generation();
        e.note_local_amplifier_command();
        e.note_local_amplifier_command();
        assert_eq!(e.remote_log_context_generation(), tx_generation);
        assert_eq!(e.remote_actuation_context_generation(), actuation + 2);
    }
    assert_eq!(
        f.authority
            .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now),
        Err("staleContext")
    );
    assert!(!f.engine.lock().unwrap().psk_armed());
    let fresh = control_request(&control_state(&f, now), action);
    let result = f
        .authority
        .handle_version((f.connection, 2), SESSION, DEVICE, &fresh, &f.engine, now)
        .unwrap();
    assert_eq!(result["outcome"], "applied");
    assert!(f.engine.lock().unwrap().psk_armed());
}

#[test]
fn a_receive_only_aprs_gesture_never_upgrades_the_ack_interlock() {
    let mut engine = tempo_app::engine::Engine::new("W9XYZ", "EN52", 0);
    engine.set_tx_enabled(true);
    for _ in 0..2 {
        engine.set_aprs_receive_only(false);
        engine.set_aprs_receive_only(true);
        assert_eq!(engine.aprs_arm_source(), tempo_app::engine::AprsArm::Auto);
        assert!(
            engine.tx_enabled(),
            "a receiver gesture preserves the existing local TX choice"
        );
    }
}
#[test]
fn manual_logging_requires_local_permission_and_one_controller() {
    let f = Fixture::new();
    let now = Instant::now();
    let s = f.state(now);
    assert_eq!(s["phase"], "localPermissionRequired");
    assert_eq!(s["txArmed"], false);
    assert_eq!(
        f.run(
            &Request::Acquire {
                request_id: id(),
                station_boot_id: s["stationBootId"].as_str().unwrap().into()
            },
            now
        ),
        Err("localPermissionRequired")
    );
    let s = f.acquire(now);
    assert_eq!(s["phase"], "controlling");
    f.authority.permit(OTHER, true).unwrap();
    assert_eq!(
        f.authority.handle(
            f.connection,
            OTHER,
            OTHER,
            &Request::Acquire {
                request_id: id(),
                station_boot_id: s["stationBootId"].as_str().unwrap().into()
            },
            &f.engine,
            now
        ),
        Err("controllerBusy")
    );
    assert!(f.engine.lock().unwrap().stored_log().is_empty());
}
#[test]
fn manual_logging_syncs_the_actual_adif_and_returns_one_receipt_on_replay() {
    let f = Fixture::new();
    let now = Instant::now();
    let state = f.acquire(now);
    let request = f.command(&state);
    let before = f.engine.lock().unwrap().settings().clone();
    let result = f.run(&request, now).unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(result["evidence"], "fileSynced");
    let bytes = std::fs::read(f.dir.join("contacts.adi")).unwrap();
    let adif = String::from_utf8(bytes.clone()).unwrap();
    assert!(adif.contains("W1AW") && adif.contains("Keep this note"));
    assert_eq!(
        f.run(&request, now + Duration::from_secs(8)).unwrap(),
        result
    );
    assert_eq!(std::fs::read(f.dir.join("contacts.adi")).unwrap(), bytes);
    assert_eq!(f.engine.lock().unwrap().stored_log().len(), 1);
    let mut reloaded = tempo_app::engine::Engine::new("W9XYZ", "EN52", 0);
    reloaded.set_log_path(f.dir.join("contacts.adi"));
    assert_eq!(reloaded.stored_log().len(), 1);
    assert_eq!(reloaded.stored_log()[0].call, "W1AW");
    assert_eq!(
        serde_json::to_value(f.engine.lock().unwrap().settings()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    let query = Request::Result {
        request_id: id(),
        operation_id: request.id().into(),
    };
    assert_eq!(
        f.authority
            .handle(f.connection, OTHER, DEVICE, &query, &f.engine, now)
            .unwrap(),
        result
    );
    assert_eq!(
        f.authority
            .handle(f.connection, OTHER, OTHER, &query, &f.engine, now),
        Err("localPermissionRequired")
    );
    let mut changed = request.clone();
    if let Request::LogManual { record, .. } = &mut changed {
        record.comment = Some("changed".into())
    }
    assert_eq!(f.run(&changed, now), Err("requestConflict"));
}
#[test]
fn manual_logging_preserves_memory_and_uncertainty_on_file_failure_without_retry() {
    let f = Fixture::new();
    std::fs::create_dir(f.dir.join("contacts.adi")).unwrap();
    let now = Instant::now();
    let command = f.command(&f.acquire(now));
    let result = f.run(&command, now).unwrap();
    assert_eq!(result["outcome"], "unknown");
    assert_eq!(f.engine.lock().unwrap().stored_log().len(), 1);
    assert_eq!(f.run(&command, now).unwrap(), result);
    assert_eq!(f.engine.lock().unwrap().stored_log().len(), 1);
    assert!(f.dir.join("contacts.adi").is_dir());
}
#[test]
fn manual_logging_refuses_expired_windows_context_changes_and_contest_recording() {
    for kind in ["window", "context", "fieldDay", "recording"] {
        let f = Fixture::new();
        let now = Instant::now();
        let command = f.command(&f.acquire(now));
        let expected = match kind {
            "window" => "windowExpired",
            "context" => {
                let mut engine = f.engine.lock().unwrap();
                let mut s = engine.settings().clone();
                s.mycall = "W2XYZ".into();
                engine.apply_settings(s);
                "staleContext"
            }
            "fieldDay" => {
                let mut engine = f.engine.lock().unwrap();
                let mut s = engine.settings().clone();
                s.fd_active = true;
                engine.apply_settings(s);
                "staleContext"
            }
            _ => {
                let mut engine = f.engine.lock().unwrap();
                let mut s = engine.settings().clone();
                s.save_qso_wav = true;
                engine.apply_settings(s);
                "staleContext"
            }
        };
        assert_eq!(
            f.run(&command, if kind == "window" { now + WINDOW } else { now }),
            Err(expected)
        );
        assert!(f.engine.lock().unwrap().stored_log().is_empty());
        if kind == "fieldDay" || kind == "recording" {
            let command = f.command(&f.state(now));
            assert_eq!(
                f.run(&command, now),
                Err(if kind == "fieldDay" {
                    "fieldDayUnsupported"
                } else {
                    "recordingUnsupported"
                })
            );
        }
    }
}
#[test]
fn manual_logging_disconnect_takeover_and_reconnect_cannot_restore_a_lease() {
    for kind in ["disconnect", "takeover", "socket"] {
        let mut f = Fixture::new();
        let now = Instant::now();
        let command = f.command(&f.acquire(now));
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
        assert!(f.engine.lock().unwrap().stored_log().is_empty());
    }
}
#[test]
fn a_revoked_device_gets_local_permission_required_even_while_the_engine_is_busy() {
    for revoke in ["takeover", "deny"] {
        let f = Fixture::new();
        let now = Instant::now();
        let state = f.acquire(now);
        let command = f.command(&state);
        let acquire = Request::Acquire {
            request_id: id(),
            station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        };
        // Positive control: the same busy Engine refuses an allowed device as busy.
        {
            let _busy = f.engine.lock().unwrap();
            assert_eq!(f.run(&command, now), Err("stationBusy"));
        }
        match revoke {
            "takeover" => f.authority.invalidate(),
            _ => {
                f.authority.permit(DEVICE, false).unwrap();
            }
        }
        // A refusal for permission never depends on Engine contention.
        let busy = f.engine.lock().unwrap();
        assert_eq!(
            f.run(&command, now),
            Err("localPermissionRequired"),
            "{revoke}"
        );
        assert_eq!(
            f.run(&acquire, now),
            Err("localPermissionRequired"),
            "{revoke}"
        );
        drop(busy);
        assert_eq!(
            f.run(&command, now),
            Err("localPermissionRequired"),
            "{revoke}"
        );
        assert!(f.engine.lock().unwrap().stored_log().is_empty());
    }
}
/// #318: a logging or station-control revoke must not fail because Core is busy — it used to
/// return `remoteBusy` and leave the grant standing. The transmit revoke already queued itself
/// (`transmit_local_revocation_does_not_wait_for_a_pending_durable_write`).
#[test]
fn a_logging_or_station_revoke_does_not_wait_for_a_busy_core() {
    for grant in ["logging", "station"] {
        let f = Fixture::new();
        let now = Instant::now();
        let (state, command) = if grant == "logging" {
            let state = f.acquire(now);
            let command = f.command(&state);
            (state, command)
        } else {
            let state = acquire_controls(&f, now);
            let arm = json!({"action":"decoder.arm","receiver":"rtty","on":true});
            (state.clone(), control_request(&state, arm))
        };
        assert_eq!(state["phase"], "controlling", "{grant}: holds the lease");
        let hardware = f.authority.hardware.permit(now + LEASE).unwrap();

        let core = f.authority.core.lock().unwrap(); // an operation in flight
        let revoke = match grant {
            "logging" => f.authority.permit(DEVICE, false),
            _ => f.authority.permit_station(DEVICE, false),
        };
        assert_eq!(revoke, Ok(()), "{grant}: revoke must not fail");
        // A grant still may, and must not widen anything.
        assert_eq!(f.authority.permit(OTHER, true), Err("remoteBusy"));
        if grant == "station" {
            assert!(
                !hardware.valid(now),
                "the controller's in-flight hardware permit ends at once"
            );
        }
        drop(core);

        // The device's next command is refused, and its lease is gone.
        let version = if grant == "logging" { 1 } else { 2 };
        let result = f.authority.handle_version(
            (f.connection, version),
            SESSION,
            DEVICE,
            &command,
            &f.engine,
            now,
        );
        assert_eq!(result, Err("localPermissionRequired"), "{grant}");
        assert!(!f.engine.lock().unwrap().rtty_armed(), "{grant}");
        assert!(f.engine.lock().unwrap().stored_log().is_empty(), "{grant}");
        let status = f.authority.local_status();
        assert_eq!(status["controller"], Value::Null, "{grant}: lease ended");
        assert_eq!(status["devices"], json!([]), "{grant}: OTHER not granted");
        assert!(!hardware.valid(now), "{grant}");
    }
}

#[test]
fn manual_logging_refuses_busy_engine_instead_of_queueing_and_counter_wrap() {
    let f = Fixture::new();
    let now = Instant::now();
    let command = f.command(&f.acquire(now));
    let lock = f.engine.lock().unwrap();
    assert_eq!(f.run(&command, now), Err("stationBusy"));
    drop(lock);
    f.authority.connection.store(u64::MAX, Ordering::SeqCst);
    assert_eq!(f.authority.start_connection(), u64::MAX);
    assert_eq!(
        f.authority
            .handle(u64::MAX, SESSION, DEVICE, &command, &f.engine, now),
        Err("authorityUnavailable")
    );
}

#[test]
fn authority_counter_exhaustion_revokes_already_issued_hardware_permission() {
    let f = Fixture::new();
    let now = Instant::now();
    acquire_controls(&f, now);
    let permit = f
        .authority
        .hardware
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

#[test]
fn manual_logging_releases_the_engine_before_waiting_for_file_sync() {
    let mut f = Fixture::new();
    let engine = f.engine.clone();
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = called.clone();
    f.authority.before_sync = Some(Box::new(move || {
        assert!(
            engine.try_lock().is_ok(),
            "storage sync must not block the engine scheduler or local controls"
        );
        observed.store(true, Ordering::SeqCst);
    }));
    let now = Instant::now();
    let command = f.command(&f.acquire(now));
    assert_eq!(f.run(&command, now).unwrap()["outcome"], "applied");
    assert!(called.load(Ordering::SeqCst));
}

#[test]
fn manual_logging_uses_station_time_unless_the_operator_explicitly_overrides_it() {
    let f = Fixture::new();
    let now = Instant::now();
    let mut command = f.command(&f.acquire(now));
    if let Request::LogManual { record, .. } = &mut command {
        record.when_unix = None;
    }
    let before = super::super::now_ms() / 1000;
    assert_eq!(f.run(&command, now).unwrap()["outcome"], "applied");
    let engine = f.engine.lock().unwrap();
    let qso = &engine.stored_log()[0];
    assert!(qso.when_unix >= before && qso.when_unix <= super::super::now_ms() / 1000);
}

#[test]
fn expired_receipt_never_reopens_an_old_sequence_under_a_live_lease() {
    let f = Fixture::new();
    let now = Instant::now();
    let state = f.acquire(now);
    let request = f.command(&state);
    assert_eq!(f.run(&request, now).unwrap()["outcome"], "applied");
    let adif = std::fs::read(f.dir.join("contacts.adi")).unwrap();
    let lease_id = state["leaseId"].as_str().unwrap().to_string();
    // Keep the same controller alive while the bounded result history expires.
    // A receipt's absence must never turn an old action into a new append.
    for second in 1..=601 {
        let heartbeat = Request::Heartbeat {
            request_id: id(),
            lease_id: lease_id.clone(),
        };
        assert_eq!(
            f.run(&heartbeat, now + Duration::from_secs(second))
                .unwrap()["phase"],
            "controlling"
        );
    }
    let later = now + Duration::from_secs(601);
    assert_eq!(
        f.run(
            &Request::Result {
                request_id: id(),
                operation_id: request.id().into()
            },
            later
        ),
        Err("resultExpired")
    );
    assert_eq!(f.run(&request, later), Err("resultExpired"));
    assert_eq!(f.engine.lock().unwrap().stored_log().len(), 1);
    assert_eq!(std::fs::read(f.dir.join("contacts.adi")).unwrap(), adif);
}

#[test]
fn a_desktop_log_collision_and_a_returned_profile_do_not_repeat_remote_work() {
    let f = Fixture::new();
    let now = Instant::now();
    let request = f.command(&f.acquire(now));
    if let Request::LogManual { record, .. } = &request {
        f.engine
            .lock()
            .unwrap()
            .log_qso(record.record().unwrap().into());
    }
    let adif = std::fs::read(f.dir.join("contacts.adi")).unwrap();
    let result = f.run(&request, now).unwrap();
    assert_eq!(result["outcome"], "rejected");
    assert_eq!(result["reason"], "alreadyPresent");
    assert_eq!(f.engine.lock().unwrap().stored_log().len(), 1);
    assert_eq!(std::fs::read(f.dir.join("contacts.adi")).unwrap(), adif);
    let next = f.command(&f.state(now));
    {
        let mut engine = f.engine.lock().unwrap();
        let original = engine.settings().clone();
        let mut changed = original.clone();
        changed.mycall = "W2XYZ".into();
        engine.apply_settings(changed);
        engine.apply_settings(original);
    }
    assert_eq!(f.run(&next, now), Err("staleContext"));
    assert_eq!(std::fs::read(f.dir.join("contacts.adi")).unwrap(), adif);
}

#[test]
#[cfg(feature = "radio")]
fn band_selection_requires_v3_and_a_later_native_owner_receipt() {
    for mode in ["cw", "phone"] {
        let f = Fixture::new();
        let (connection, target_hz, target_mode) = {
            let mut e = f.engine.lock().unwrap();
            e.configure_remote_settings_store(f.dir.join("settings.json"));
            e.set_operating_mode(mode, false);
            e.set_tx_enabled(false);
            e.set_frequency(14.275, "20m", "USB");
            e.take_immediate_retune();
            let mut expected = tempo_app::engine::Engine::with_settings(e.settings().clone());
            expected.pick_band("40m", Some(mode));
            (
                e.remote_open_radio().unwrap(),
                expected.settings().dial_hz(),
                expected.rig_mode_effective(),
            )
        };
        let sample = |hz, rig_mode: &str| {
            let mut e = f.engine.lock().unwrap();
            let read = e.remote_radio_read(&connection, Instant::now()).unwrap();
            e.remote_observe_cat(Some(&read), Some(true));
            e.remote_observe_dial(Some(&read), Some(hz));
            e.remote_observe_mode(Some(&read), Some(rig_mode));
            e.remote_observe_ptt(Some(&read), Some(false));
        };
        sample(14_275_000, if mode == "cw" { "CW" } else { "USB" });
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let command = control_request(
            &state,
            json!({"action":"radio.band", "band":"40m", "mode":mode}),
        );
        let run = |version, request: &Request| {
            f.authority.handle_version(
                (f.connection, version),
                SESSION,
                DEVICE,
                request,
                &f.engine,
                Instant::now(),
            )
        };
        assert_eq!(run(2, &command).unwrap_err(), "stationUnsupported");
        assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
        // Version refusal does not consume the v3 command window.
        let pending = run(3, &command).unwrap();
        assert_eq!(pending["outcome"], "pending");
        assert_eq!(run(3, &command).unwrap(), pending);
        let work = f.engine.lock().unwrap().take_remote_radio().unwrap();
        assert_eq!(work.target(), (target_hz, target_mode.as_str()));
        sample(target_hz, &target_mode);
        assert!(work.commit(&mut f.engine.lock().unwrap()));
        let applied = run(3, &command).unwrap();
        assert_eq!(applied["outcome"], "applied");
        assert_eq!(applied["evidence"], "radioReadback");
        assert!(!f.engine.lock().unwrap().tx_enabled());
        assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
    }
}

/// **A REPLAY IS ANSWERED FROM ITS RECEIPT, AND THE ENGINE HAS NOTHING TO DO WITH IT.**
///
/// A duplicate request's whole answer is a stored `Receipt`, which Core guards and the Engine
/// never touches — but `handle_version` took `shared_engine.try_lock()` 160 lines before the
/// receipt lookup, so a replay was refused `stationBusy` for a lock it never needed, whenever
/// the station's own radio loop held the Engine for a tick. The browser could then not retrieve
/// the receipt for an operation that had ALREADY applied.
///
/// This is the deterministic form of a CI flake: `native.test.mjs:782` replays a `logManual` and
/// asserts it gets the identical response back, and that assertion lost a race with a ~20 ms
/// radio-loop tick.
///
/// That the refusal was never intended is recorded in `Receipt::value`: a replay whose write is
/// still in flight answers `remoteBusy`, "so it waits rather than appending or posting twice".
/// The design has a considered answer for a contended replay, and `stationBusy` is not it.
///
/// Both directions, because one is half a test: the replay must be served while the Engine is
/// held, AND a genuinely new write must still be refused — this must not turn real contention
/// into a false success.
#[test]
fn a_replay_is_served_from_its_receipt_while_the_engine_is_busy() {
    let f = Fixture::new();
    let now = Instant::now();
    let command = f.command(&f.acquire(now));
    let applied = f.run(&command, now).expect("the first write applies");
    // Built before the lock is taken: reading state needs the Engine too.
    let fresh = f.command(&f.state(now));

    let held = f.engine.lock().unwrap();
    assert_eq!(
        f.run(&command, now),
        Ok(applied),
        "a replay's answer is its receipt; the Engine is not involved"
    );
    assert_eq!(
        f.run(&fresh, now),
        Err("stationBusy"),
        "a genuinely new write still needs the Engine and must still be refused"
    );
    drop(held);
}

/// **THE FINGERPRINT LIST MUST COVER EXACTLY THE RECEIPT-BEARING ARM'S OR-PATTERN.**
///
/// `handle_version` matches `LogManual | StationControl | LogChange` in ONE arm, and that arm
/// now reads its fingerprint from `replay_fingerprint`. A variant missing from that helper's
/// list gets `None` and the shared arm refuses it `invalidRequest` — which is exactly what the
/// first cut of the replay fix did, breaking twelve unrelated tests at once because it listed
/// `LogManual` alone.
///
/// A comment cannot hold that correspondence down; this can. Both directions: every variant the
/// arm serves must have one, and a request from a different arm must NOT gain a receipt lookup
/// it never had.
#[test]
fn every_variant_of_the_receipt_bearing_arm_has_a_replay_fingerprint() {
    let f = Fixture::new();
    let now = Instant::now();
    // v2 acquire: `control_request` needs the controls context the version-2 capture adds.
    let state = acquire_controls(&f, now);
    let change: Request = serde_json::from_value(json!({"type":"logChange","requestId":id(),
        "stationBootId":state["stationBootId"],"leaseId":state["leaseId"],
        "expectedRevision":state["revision"],"commandWindowId":state["commandWindowId"],
        "clientSequence":state["nextSequence"],
        "change":{"kind":"delete","target":{"call":"W1AW","whenUnix":1,"key":"0".repeat(64)}}}))
    .expect("a logChange request");
    for request in [
        f.command(&state),
        control_request(&state, json!({"action":"decoder.clear","receiver":"rtty"})),
        change,
    ] {
        assert!(
            Authority::replay_fingerprint(&request)
                .expect("a well-formed request serializes")
                .is_some(),
            "{} shares the receipt-bearing arm and needs a fingerprint",
            request.id()
        );
    }
    // The other direction: an arm that keeps no receipt must not gain a lookup.
    let state_request = Request::State { request_id: id() };
    assert!(
        Authority::replay_fingerprint(&state_request)
            .expect("a well-formed request serializes")
            .is_none(),
        "a request whose arm keeps no receipt must not gain a replay lookup"
    );
}

/// **A `Result` REQUEST IS A PURE RECEIPT READ, AND THE ENGINE HAS NOTHING TO DO WITH IT.**
///
/// The sibling of the replay defect, one arm over. `Request::Result` reads `c.receipts`, the
/// grants and `device` — Core and nothing else — yet it sat behind the same
/// `shared_engine.try_lock()`, so a browser asking for the answer already on file was refused
/// `stationBusy` whenever the station's own radio loop held the Engine for a tick.
///
/// No CI failure was ever attributed to this one; it is fixed because it is the same mechanism,
/// not because it was observed. The red below is real all the same.
///
/// Both directions, as for the replay: the receipt must be served while the Engine is held, and
/// a request that genuinely needs the Engine must still be refused.
#[test]
fn a_result_request_is_served_from_its_receipt_while_the_engine_is_busy() {
    let f = Fixture::new();
    let now = Instant::now();
    let command = f.command(&f.acquire(now));
    let applied = f.run(&command, now).expect("the first write applies");
    let result = Request::Result {
        request_id: id(),
        operation_id: command.id().into(),
    };
    // Built before the lock is taken: reading state needs the Engine too.
    let fresh = f.command(&f.state(now));

    let held = f.engine.lock().unwrap();
    assert_eq!(
        f.run(&result, now),
        Ok(applied),
        "a Result request reads a receipt; the Engine is not involved"
    );
    assert_eq!(
        f.run(&fresh, now),
        Err("stationBusy"),
        "a genuinely new write still needs the Engine and must still be refused"
    );
    drop(held);
}
