//! The amplifier TX delay through the real radio loop (`RadioLoop::step`): every over Nexus makes
//! the audio for waits the delay after its key, as silence queued ahead of it, and holds PTT that
//! much longer; a word, chunk or frame on a key the loop already held waits nothing; Stop TX inside
//! the delay unkeys on the next tick with nothing on the air; and the tick that keys is no longer
//! for the delay, because the delay is never a sleep.

use super::cw_stop::{logging_rigctld, step};
use super::*;

/// The delay these tests set, in ms, and in 12 kHz samples.
const D_MS: u32 = 200;
const D_SAMPLES: usize = 2_400;

/// A licensed engine with the amplifier TX delay at `amp_ms`, then `setup`.
fn engine(amp_ms: u32, setup: impl FnOnce(&mut Engine)) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("N0CALL", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.amp_tx_delay_ms = amp_ms;
        e.apply_settings(s);
        e.set_license_class("extra");
        setup(&mut e);
    }
    engine
}

/// The radio loop on a rigctld that keys, a 20 ms tick at a time from 100 ms, recording when the
/// radio was keyed and unkeyed and how much had been queued at the key.
struct Loop {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    t: f64,
    keyed_at: Option<f64>,
    queued_at_key: usize,
    unkeyed_at: Option<f64>,
    /// The longest a single tick took on the wall clock, in ms.
    longest_tick_ms: f64,
}

impl Loop {
    /// `radio`: the dial (Hz) and mode the rigctld reads back; `None` for 40 m CW.
    fn new(engine: &Arc<Mutex<Engine>>, radio: Option<(u64, &'static str)>) -> Loop {
        let (addr, _log) = logging_rigctld(move |l| match (l, radio) {
            ("f", Some((hz, _))) => Some(format!("{hz}\n")),
            ("m", Some((_, mode))) => Some(format!("{mode}\n3000\n")),
            _ => None,
        });
        Loop {
            engine: Arc::clone(engine),
            state: loop_state_for(engine),
            backend: MockBackend::new(),
            rig: Rig::rigctld(&addr),
            t: 100.0,
            keyed_at: None,
            queued_at_key: 0,
            unkeyed_at: None,
            longest_tick_ms: 0.0,
        }
    }

    fn tick(&mut self) {
        let was = self.rig.keyed;
        let queued = self.backend.played.len();
        let began = std::time::Instant::now();
        step(
            &mut self.state,
            &self.engine,
            &mut self.backend,
            &mut self.rig,
            self.t,
        );
        self.longest_tick_ms = self
            .longest_tick_ms
            .max(began.elapsed().as_secs_f64() * 1000.0);
        if !was && self.rig.keyed && self.keyed_at.is_none() {
            self.keyed_at = Some(self.t);
            self.queued_at_key = queued;
        }
        if was && !self.rig.keyed && self.keyed_at.is_some() && self.unkeyed_at.is_none() {
            self.unkeyed_at = Some(self.t);
        }
        self.t += 20.0;
    }

    fn run_to(&mut self, until_ms: f64) {
        while self.t <= until_ms {
            self.tick();
        }
    }

    /// What was queued from the key on.
    fn since_key(&self) -> &[f32] {
        &self.backend.played[self.queued_at_key..]
    }

    /// How long the radio stayed keyed, in ms.
    fn keyed_ms(&self, name: &str) -> f64 {
        self.unkeyed_at
            .unwrap_or_else(|| panic!("{name}: premise: unkeyed by {} ms", self.t))
            - self.keyed_at.expect("keyed")
    }
}

/// The 20 m phone dial, read back from the radio.
const PHONE: Option<(u64, &str)> = Some((14_250_000, "USB"));

fn phone(e: &mut Engine) {
    e.set_operating_mode("phone", true);
    e.set_frequency(14.250, "20m", "USB");
}

fn first_tone(s: &[f32]) -> Option<usize> {
    s.iter().position(|&x| x != 0.0)
}

/// What sets an over going.
type Setup = fn(&mut Engine);
/// The dial (Hz) and mode the radio reads back.
type Radio = Option<(u64, &'static str)>;

/// Each non-slot over Nexus plays, by name, with what sets it going, the radio it reads back and how
/// long the run takes.
fn overs() -> Vec<(&'static str, Setup, Radio, f64)> {
    vec![
        (
            "soundcard CW",
            |e| {
                e.set_cw_keyer("soundcard", 600.0);
                e.set_operating_mode("cw", false);
                e.set_frequency(7.03, "40m", "CW");
                e.set_cw_wpm(25);
                e.send_cw("CQ TEST DE");
            },
            None,
            15_000.0,
        ),
        (
            "PSK message",
            |e| {
                e.set_operating_mode("keyboard", false);
                e.psk_send_text("CQ TEST").unwrap();
            },
            None,
            10_000.0,
        ),
        (
            "RTTY message",
            |e| {
                e.set_operating_mode("rtty", false);
                e.rtty_send_text("CQ TEST").unwrap();
            },
            None,
            10_000.0,
        ),
        (
            "voice keyer",
            |e| {
                phone(e);
                e.send_voice(vec![0.05; 12_000]).unwrap();
            },
            PHONE,
            10_000.0,
        ),
        (
            "APRS frame",
            |e| {
                phone(e);
                e.aprs_beacon(41.88, -87.63, '/', '>', "", &[])
                    .expect("the beacon is queued");
            },
            PHONE,
            10_000.0,
        ),
        (
            "SSTV picture",
            |e| {
                phone(e);
                e.sstv_send(vec![0.05f32; 12_000 * 3], "Scottie 1".to_string())
                    .unwrap();
            },
            PHONE,
            10_000.0,
        ),
    ]
}

/// ⭐ T6. EVERY OVER NEXUS PLAYS WHOLE WAITS THE DELAY AFTER ITS KEY AND IS HELD THAT MUCH LONGER:
/// the over queued with the delay is the over queued without it, behind the delay's silence, and
/// the radio stays keyed the delay longer (to the 20 ms tick). CONTROL: with no delay nothing waits.
#[test]
fn every_over_nexus_plays_waits_the_delay_after_its_key() {
    for (name, setup, radio, until) in overs() {
        let run = |amp_ms| {
            let e = engine(amp_ms, setup);
            let mut l = Loop::new(&e, radio);
            l.run_to(until);
            l
        };
        let (off, on) = (run(0), run(D_MS));
        assert!(
            first_tone(off.since_key()).is_some(),
            "{name}: premise: the over played"
        );
        assert!(
            first_tone(off.since_key()).unwrap() < D_SAMPLES,
            "{name}: CONTROL: with no delay the over starts at its key"
        );
        assert!(
            first_tone(on.since_key()).unwrap() >= D_SAMPLES,
            "{name}: a tone {:.1} ms after the key",
            first_tone(on.since_key()).unwrap() as f64 / 12.0
        );
        let mut moved = vec![0.0f32; D_SAMPLES];
        moved.extend_from_slice(off.since_key());
        assert!(
            on.since_key() == &moved[..],
            "{name}: not the same over behind the delay ({} against {} samples)",
            on.since_key().len(),
            moved.len()
        );
        let longer = on.keyed_ms(name) - off.keyed_ms(name);
        assert!(
            (f64::from(D_MS) - 20.0..=f64::from(D_MS) + 20.0).contains(&longer),
            "{name}: keyed {longer} ms longer, not the delay"
        );
    }
}

/// T6, the streams: RTTY and PSK typed live wait the delay on their first key only; the chunks
/// after it ride the key the loop already holds.
#[test]
fn a_keyboard_stream_waits_the_delay_once() {
    type Latch = fn(&mut Engine, bool) -> Result<(), String>;
    type Type = fn(&mut Engine, &str) -> Result<(), String>;
    let modes: [(&str, Latch, Type); 2] = [
        ("rtty", Engine::set_rtty_latched, Engine::rtty_type),
        ("keyboard", Engine::set_psk_latched, Engine::psk_type),
    ];
    for (mode, latch, type_text) in modes {
        let run = |amp_ms| {
            let e = engine(amp_ms, |e| {
                e.set_operating_mode(mode, false);
                latch(e, true).unwrap();
                type_text(e, "CQ CQ").unwrap();
            });
            let mut l = Loop::new(&e, None);
            l.run_to(1_500.0);
            l
        };
        let (off, on) = (run(0), run(D_MS));
        let first =
            |l: &Loop| first_tone(l.since_key()).expect("premise: the stream is on the air");
        assert!(first(&off) < D_SAMPLES, "{mode}: CONTROL");
        assert!(first(&on) >= D_SAMPLES, "{mode}: a tone inside the delay");
        // Exactly one delay's silence: the stream's own audio from there on is unbroken by another.
        let silent_run = on.since_key()[D_SAMPLES..]
            .iter()
            .take_while(|&&x| x == 0.0)
            .count();
        assert_eq!(
            first(&on) - D_SAMPLES,
            silent_run,
            "{mode}: premise: one run of silence"
        );
        assert!(
            first(&on) - D_SAMPLES <= first(&off),
            "{mode}: more than one delay before the first tone"
        );
    }
}

/// T6, Tune: the carrier Nexus plays comes the delay after the key.
#[test]
fn a_tune_carrier_waits_the_delay() {
    for amp_ms in [0, D_MS] {
        let e = engine(amp_ms, |e| {
            phone(e);
            e.set_tune(true);
        });
        let mut l = Loop::new(&e, PHONE);
        l.run_to(1_000.0);
        let first = first_tone(l.since_key()).expect("premise: the carrier went out");
        if amp_ms == 0 {
            assert!(first < D_SAMPLES, "CONTROL: no delay, carrier at the key");
        } else {
            assert!(
                first >= D_SAMPLES,
                "carrier {:.1} ms after the key",
                first as f64 / 12.0
            );
        }
    }
}

/// ⭐ T8. STOP TX INSIDE THE DELAY: at every tick of a 500 ms delay, the next tick unkeys and drops
/// what was queued, before the delay's end, so none of the over sounded. The voice keyer's over and
/// FT2's (keyed on a late start), each with the delay at 500 ms.
#[test]
fn stop_inside_the_delay_unkeys_with_nothing_on_the_air() {
    let cases: [(&str, Setup, Radio); 2] = [
        (
            "voice keyer",
            |e| {
                phone(e);
                e.send_voice(vec![0.05; 12_000]).unwrap();
            },
            PHONE,
        ),
        (
            "FT2",
            |e| {
                e.set_tier(Tier::Ft2);
                e.set_frequency(14.074, "20m", "USB");
                e.broadcast("CQ N0CALL EN52");
            },
            Some((14_074_000, "PKTUSB")),
        ),
    ];
    for (name, setup, radio) in cases {
        for after_ms in (0..500).step_by(20) {
            let e = engine(500, setup);
            let mut l = Loop::new(&e, radio);
            while l.keyed_at.is_none() && l.t < 3_000.0 {
                l.tick();
            }
            let keyed = l
                .keyed_at
                .unwrap_or_else(|| panic!("{name}: premise: keyed"));
            assert!(
                first_tone(l.since_key()).is_some_and(|i| i >= 6_000),
                "{name}: premise: the over waits 500 ms"
            );
            while l.t < keyed + f64::from(after_ms) {
                l.tick();
            }
            let flushes = l.backend.flush_calls;
            e.lock().unwrap().halt_tx();
            let stop_tick = l.t;
            l.tick();
            assert!(
                !l.rig.keyed,
                "{name}: Stop {after_ms} ms into the delay: still keyed"
            );
            assert!(
                l.backend.flush_calls > flushes,
                "{name}: Stop {after_ms} ms into the delay: nothing flushed"
            );
            assert!(
                stop_tick - keyed < 500.0,
                "{name}: the unkey came {} ms after the key, past the delay",
                stop_tick - keyed
            );
        }
    }
}

/// T8, the tick: the delay is silence queued, never a wait, so the tick that keys takes no longer
/// with a 500 ms delay than without one.
#[test]
fn the_delay_is_never_a_wait_in_the_tick() {
    let e = engine(500, |e| {
        phone(e);
        e.send_voice(vec![0.05; 12_000]).unwrap();
    });
    let mut l = Loop::new(&e, PHONE);
    l.run_to(400.0);
    assert!(l.keyed_at.is_some(), "premise: keyed");
    assert!(
        l.longest_tick_ms < 250.0,
        "a tick took {:.0} ms: the delay waited in it",
        l.longest_tick_ms
    );
}
