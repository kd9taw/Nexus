//! Phone's Lock from afar, end to end over loopback: this computer in client mode, its screen
//! re-asserting the Lock as Phone's does and its microphone (a stand-in headset) speaking, driving
//! the shack's real channel, Stop road, audio road and engine, with the shack's radio loop ticking
//! as Nexus's does. The Lock keys by its voice and stays on the air; a Stop, a presence lapse, a
//! gap in its audio and its cap each take it off, and nothing keys again while the screen goes on
//! re-asserting it; an answer to "Still talking?" keeps it on past the cap it would have met.
use super::super::computer::{behind_a_port, paired_record};
use super::*;
use crate::client_mode::tests::{ask, eventually, scratch, screens, Screens};
use crate::client_mode::{Client, Find, Phase, ThisComputer, Timing};
use crate::remote_service::vault::PairedStation;
use tauri::Manager;

/// The shack's radio loop's tick.
const TICK: Duration = super::super::drops::TICK;
/// The cap and its question for these tests, short enough to wait for.
const CAP: Duration = Duration::from_millis(2500);
const ASK: Duration = Duration::from_millis(1000);

/// What the screen was answered at each re-assertion of its Lock, and when.
type Said = Arc<Mutex<Vec<(Instant, Result<Value, Value>)>>>;
/// Each tick of the shack's loop: when, and whether the microphone over was armed and keyed.
type Ticks = Arc<Mutex<Vec<(Instant, bool, bool)>>>;

/// A computer driving the roadside's station with a Lock: the screen's poll, its microphone, its
/// Lock re-asserted every 50 ms with each answer kept, and the shack's loop.
struct Locked {
    client: Arc<Client>,
    s: Screens,
    driving: tauri::async_runtime::JoinHandle<()>,
    dir: std::path::PathBuf,
    done: Arc<AtomicBool>,
    headset: Arc<tempo_client::audio_road::StandIn>,
    speaking: Arc<AtomicBool>,
    answer: Arc<AtomicBool>,
    said: Said,
    ticks: Ticks,
    _listening: watch::Sender<bool>,
}

impl Drop for Locked {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        self.speaking.store(false, Ordering::SeqCst);
        self.driving.abort();
        tauri::async_runtime::block_on(self.client.let_go());
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Locked {
    /// TX armed from afar, then the Lock held and spoken into until it keys.
    fn on(r: &Roadside, name: &str) -> Self {
        let (at, listening) = tauri::async_runtime::block_on(async {
            r.s.shared
                .authority
                .permit_station(&r.s.device, true)
                .unwrap();
            let (stopper, stop) = watch::channel(false);
            (behind_a_port(&r.s, stop).await, stopper)
        });
        r.s.shared
            .engine
            .lock()
            .unwrap()
            .set_operating_mode("phone", false);
        let record = paired_record(&r.s, at);
        let dir = scratch(name);
        let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
        let client = Arc::new(Client::new(STATION.to_string(), this));
        let headset = tempo_client::audio_road::StandIn::new();
        client.hear_in_a_test(Arc::clone(&headset) as Arc<dyn tempo_client::audio_road::Headset>);
        client.time_with(Timing {
            lock_cap: CAP,
            lock_ask: ASK,
            ..Timing::default()
        });
        let s = screens(&client);
        let (device, key, pkcs8) = (
            record.device_id.clone(),
            record.station_key.clone(),
            record.pkcs8.clone(),
        );
        let find: Find = Arc::new(move |_| {
            Ok(Some(PairedStation {
                station_id: STATION.into(),
                device_id: device.clone(),
                station_key: key.clone(),
                pkcs8: pkcs8.clone(),
                addresses: vec![at.to_string()],
            }))
        });
        let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
            s.main.app_handle().clone(),
            find,
            Arc::new(|_| Ok(Vec::new())),
            Some(tempo_stream::lan::Typed::At(at)),
        ));
        eventually("control taken", || client.status().phase == Phase::Live);
        let done = Arc::new(AtomicBool::new(false));
        // The screen's own poll, as the main window's: presence wants its state fresh.
        {
            let (main, done) = (s.main.clone(), done.clone());
            std::thread::spawn(move || {
                while !done.load(Ordering::SeqCst) {
                    let _ = ask(&main, "get_snapshot", json!({}));
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
        }
        let ticks: Ticks = Arc::default();
        {
            let (engine, done, ticks) = (r.s.shared.engine.clone(), done.clone(), ticks.clone());
            std::thread::spawn(move || {
                while !done.load(Ordering::SeqCst) {
                    let now = Instant::now();
                    let tick = {
                        let mut e = engine.lock().unwrap();
                        e.poll_remote_transmit(now);
                        e.poll_mic(now, TICK.as_millis() as f64, 1);
                        (now, e.mic_armed(), e.mic_keyed())
                    };
                    ticks.lock().unwrap().push(tick);
                    std::thread::sleep(TICK);
                }
            });
        }
        within("presence live at the shack", || {
            r.s.shared
                .engine
                .lock()
                .unwrap()
                .remote_presence_live(Instant::now())
        });
        within("TX armed from afar", || {
            ask(&s.main, "set_tx_enabled", json!({ "enabled": true }))
                == Ok(json!({ "ran": "set_tx_enabled" }))
        });
        let speaking = speak(&headset);
        let answer = Arc::new(AtomicBool::new(false));
        let said: Said = Arc::default();
        {
            let (main, done, answer, said) = (
                s.main.clone(),
                done.clone(),
                answer.clone(),
                Arc::clone(&said),
            );
            std::thread::spawn(move || {
                while !done.load(Ordering::SeqCst) {
                    let still = answer.swap(false, Ordering::SeqCst);
                    let answered = ask(&main, "hold_ptt_lock", json!({ "stillTalking": still }));
                    lock_of(&said).push((Instant::now(), answered));
                    // Twice as often as Phone's screen: a loaded test box must not lapse the press
                    // at its 200 ms hold gap and so end it before the guard under test does.
                    std::thread::sleep(Duration::from_millis(50));
                }
            });
        }
        let locked = Self {
            client,
            s,
            driving,
            dir,
            done,
            headset,
            speaking,
            answer,
            said,
            ticks,
            _listening: listening,
        };
        within("the Lock keyed by its voice", || {
            r.s.shared.engine.lock().unwrap().mic_keyed()
        });
        locked
    }

    /// Whether every tick of the shack's loop from `from` to now was keyed, with ticks to judge.
    fn keyed_throughout(&self, from: Instant) -> bool {
        let ticks = self.ticks.lock().unwrap();
        let since: Vec<_> = ticks.iter().filter(|(at, ..)| *at >= from).collect();
        since.len() > 10 && since.iter().all(|&&(_, armed, keyed)| armed && keyed)
    }

    /// When the over first came off the air at the shack after `from`.
    fn off_after(&self, from: Instant) -> Option<Instant> {
        let ticks = self.ticks.lock().unwrap();
        ticks
            .iter()
            .find(|&&(at, armed, keyed)| at >= from && !armed && !keyed)
            .map(|&(at, ..)| at)
    }

    /// Whether any tick from `from` on was armed or keyed.
    fn on_since(&self, from: Instant) -> bool {
        let ticks = self.ticks.lock().unwrap();
        ticks
            .iter()
            .any(|&(at, armed, keyed)| at >= from && (armed || keyed))
    }

    /// What happened from `from` on, for a failure: each change of the shack's over, and each
    /// change in what the screen was answered, by milliseconds after `from`.
    fn story(&self, from: Instant) -> String {
        let ms = |at: Instant| at.saturating_duration_since(from).as_millis();
        let mut story = Vec::new();
        let mut last = None;
        for &(at, armed, keyed) in self.ticks.lock().unwrap().iter() {
            if at >= from && last != Some((armed, keyed)) {
                story.push(format!("{}ms armed={armed} keyed={keyed}", ms(at)));
                last = Some((armed, keyed));
            }
        }
        let mut said_last = None;
        for (at, said) in lock_of(&self.said).iter() {
            let what = match said {
                Ok(v) => format!("ok ask={}", v["ask"]),
                Err(e) => format!("err {e}"),
            };
            if *at >= from && said_last.as_ref() != Some(&what) {
                story.push(format!("{}ms said {what}", ms(*at)));
                said_last = Some(what);
            }
        }
        story.join("; ")
    }

    /// The screen's answers from `from` on.
    fn said_since(&self, from: Instant) -> Vec<Result<Value, Value>> {
        lock_of(&self.said)
            .iter()
            .filter(|(at, _)| *at >= from)
            .map(|(_, said)| said.clone())
            .collect()
    }
}

fn lock_of<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

/// Waits up to five seconds for `done`.
fn within(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "never: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A microphone speaking a 1 kHz tone into the headset until the flag is cleared, clocked as a
/// sound card is: each pass, about every 10 ms, hands over every sample due since it began, so a
/// pass the scheduler holds up catches up as a capture buffer would. Handing over 10 ms a pass fell
/// behind real time by every late wake-up (0.13 to 0.62 s on a loaded box, measured 2026-10-19),
/// and audio later than the station's lead-in is dropped at both ends, so the over ended at its
/// 200 ms gap before the guard under test.
fn speak(headset: &Arc<tempo_client::audio_road::StandIn>) -> Arc<AtomicBool> {
    let speaking = Arc::new(AtomicBool::new(true));
    let (rings, on) = (headset.rings.clone(), speaking.clone());
    std::thread::spawn(move || {
        let began = Instant::now();
        let mut n = 0usize;
        while on.load(Ordering::SeqCst) {
            let due = (began.elapsed().as_micros() * 48_000 / 1_000_000) as usize;
            rings.captured((n..due).map(|i| {
                let t = i as f32 / 48_000.0;
                0.4 * (2.0 * std::f32::consts::PI * 1_000.0 * t).sin()
            }));
            n = due;
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    speaking
}

/// The end of each Lock: Stop pressed on the screen (by both roads, as the button sends it), the
/// shack's loop finding presence lapsed, the microphone going silent, and the cap.
const ENDS: [&str; 4] = ["stop", "presence", "gap", "cap"];

/// ⛔ Each end takes a Lock from afar off the air, and nothing keys again while the screen goes on
/// re-asserting it (its voice back, for the gap). CONTROL: before each end the Lock was keyed by
/// its voice at every tick of the shack's loop for a second, five times the hold's and the audio's
/// gaps, with every re-assertion answered.
#[test]
fn a_lock_from_afar_keys_by_its_voice_and_each_guard_ends_it() {
    let _alone = alone();
    for end in ENDS {
        let app = App::new();
        let mut r = roadside_capped(&app, CAP);
        let _record = on_the_engine(&mut r, gate(false));
        let t = Locked::on(&r, &format!("lock-{end}"));
        let keyed = Instant::now();
        std::thread::sleep(Duration::from_millis(1000));
        assert!(
            t.keyed_throughout(keyed),
            "{end}: CONTROL, keyed by its voice"
        );
        assert!(
            t.said_since(keyed).iter().all(Result::is_ok),
            "{end}: CONTROL, every re-assertion held: {:?}",
            t.said_since(keyed)
        );
        let ended = Instant::now();
        let bound = match end {
            "stop" => {
                assert!(ask(&t.s.main, "halt_tx", json!({})).is_ok(), "{end}");
                Duration::from_millis(500)
            }
            "presence" => {
                // The shack's loop, one tick read with the clock past presence's deadline: what
                // it reads when this computer's heartbeats stopped coming.
                r.s.shared
                    .engine
                    .lock()
                    .unwrap()
                    .poll_remote_transmit(ended + PRESENCE + Duration::from_secs(1));
                Duration::from_millis(500)
            }
            "gap" => {
                t.speaking.store(false, Ordering::SeqCst);
                // The audio design's 200 ms, the frame in flight, and the loop's tick.
                Duration::from_millis(400)
            }
            _ => {
                // Unanswered, off at the cap: counted from the press, before it keyed.
                CAP + Duration::from_millis(500)
            }
        };
        within(&format!("{end}: off the air"), || {
            t.off_after(ended).is_some() || ended.elapsed() > bound + Duration::from_secs(1)
        });
        let off = t
            .off_after(ended)
            .unwrap_or_else(|| panic!("{end}: still on the air"));
        assert!(
            off.duration_since(ended) <= bound,
            "{end}: off {:?} after, the bound {bound:?}",
            off.duration_since(ended)
        );
        // For the gap, the voice back: it keys nothing.
        let again = (end == "gap").then(|| speak(&t.headset));
        std::thread::sleep(Duration::from_millis(800));
        if let Some(again) = again {
            again.store(false, Ordering::SeqCst);
        }
        assert!(
            !t.on_since(off + TICK),
            "{end}: keyed again while the screen re-asserted the Lock"
        );
        let refused = t.said_since(off + Duration::from_millis(300));
        assert!(
            !refused.is_empty() && refused.iter().all(Result::is_err),
            "{end}: the screen is told the Lock ended: {refused:?}"
        );
        if end == "cap" {
            assert!(
                refused[0] == Err(json!("nexusClient:lockCapped"))
                    || refused[0] == Err(json!("nexusClient:pressEnded")),
                "{refused:?}"
            );
            assert!(
                lock_of(&t.said)
                    .iter()
                    .any(|(_, said)| said == &Err(json!("nexusClient:lockCapped"))),
                "the cap said why: {}",
                t.story(keyed)
            );
        }
    }
}

/// ★ An answer to "Still talking?" keeps a Lock from afar on the air past the cap it would have
/// met: asked once inside the question's lead, answered, and keyed by its voice at every tick past
/// the first cap; then capped a whole cap after the answer. CONTROL: asked before the answer.
#[test]
fn an_answered_lock_from_afar_stays_on_past_its_first_cap() {
    let _alone = alone();
    let app = App::new();
    let mut r = roadside_capped(&app, CAP);
    let _record = on_the_engine(&mut r, gate(false));
    let t = Locked::on(&r, "lock-answered");
    let pressed = lock_of(&t.said).first().unwrap().0;
    within("asked", || {
        lock_of(&t.said)
            .last()
            .is_some_and(|(_, said)| said.as_ref().is_ok_and(|said| said["ask"] == true))
    });
    t.answer.store(true, Ordering::SeqCst);
    within("answered", || !t.answer.load(Ordering::SeqCst));
    let answered = Instant::now();
    while pressed.elapsed() < CAP + Duration::from_millis(700) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        t.keyed_throughout(pressed + CAP - Duration::from_millis(300)),
        "on the air past the first cap: {}",
        t.story(pressed)
    );
    within("capped a cap after the answer", || {
        t.off_after(answered).is_some() || answered.elapsed() > CAP + Duration::from_secs(2)
    });
    let off = t.off_after(answered).expect("capped");
    assert!(
        off.duration_since(answered) >= CAP - Duration::from_millis(200),
        "off {:?} after the answer",
        off.duration_since(answered)
    );
}
