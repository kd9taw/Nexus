//! THE RF SCOPE PANE, from the radio loop's side (operator's pick, 2026-10-03: an opt-in RF scope pane
//! in the FT, JS8, RTTY, PSK and SSTV cockpits).
//!
//! In a DATA mode the Icom CI-V scope streams only while the pane reads the RF slot
//! (`SpectrumFeed::rf_frame_after` places the request, `rf_wanted` reads it), and the FT waterfall's
//! read never sees what it streams. Checked here BY COMMAND, on the wire of a mock IC-9700 behind
//! Nexus's own CI-V daemon, driven by the real `RadioLoop::step`.
//!
//! The stream shares the CAT link with the PTT of every over, so the last test (`#[ignore]`d — it
//! runs for minutes) MEASURES what it does to an FT over's keying: the latency from the over's
//! scheduled start (the slot boundary) to the PTT command on the wire, to the radio's ack, and to
//! the audio starting, with the pane off and on, at 3 and 10 sweeps a second. FT timing is the
//! operator's call (WSJT-X is the standard): this measures and reports, it decides nothing.
//!
//! THE MOCK'S ONE MODEL, stated because the numbers rest on it: every byte the radio sends is paced
//! at 115200 baud (ten bit times) through ONE queue, scope bursts and command replies alike, and is
//! readable on the next millisecond (USB delivers in frames). A command's reply therefore waits
//! behind any scope bytes already on their way. How a real IC-9700 orders its output while it
//! streams is not known here; the bench is the authority on that.
use super::*;
use crate::civ::broker::CivDaemon;
use crate::civ::commands::IcomModel;
use crate::civ::engine::tests_support::FakeRadio;
use crate::civ::frame::{freq_to_bcd, Frame, CONTROLLER};
use std::collections::VecDeque;
use std::io::{self, Read, Write};

/// The IC-9700's default CI-V address.
const ADDR: u8 = 0xA2;
/// One byte at 115200 baud, 8N1.
const BYTE_TIME: Duration = Duration::from_nanos(86_806);
/// A scope sweep: 475 points over the ten frames after the header (IC-9700 CI-V reference: eleven
/// frames a sweep over USB).
const POINTS: usize = 475;

/// What the test can see of the link, shared with the radio inside the daemon.
#[derive(Default)]
struct Wire {
    /// Every controller frame as it arrived: when, command, data.
    log: Vec<(Instant, u8, Vec<u8>)>,
    /// `27 11`: the waveform output the controller last asked for.
    wave_on: bool,
    /// Sweeps a second while the waveform output is on.
    rate: f64,
    /// Sweeps put on the wire, in all.
    sweeps: u32,
    /// Every PTT-on command as it arrived, and the instant its ack had fully reached the controller.
    ptt_on: Vec<(Instant, Instant)>,
    /// The PTT state the controller last set.
    keyed: bool,
}

/// A mock IC-9700 on a paced CI-V link: `FakeRadio`'s register file behind it, plus what that fixture
/// does not model — PTT, and the scope waveform at a set sweep rate. See the module header for the one
/// model the timing rests on.
struct PacedRig {
    inner: FakeRadio,
    wire: Arc<Mutex<Wire>>,
    /// Bytes on their way to the controller, each with the instant it can be read.
    out: VecDeque<(Instant, u8)>,
    /// When the line is free to start the next byte.
    line_free: Instant,
    /// The millisecond grid USB delivers on.
    epoch: Instant,
    next_sweep: Option<Instant>,
    sweep_n: u32,
    /// Where the first sweep after `27 11 01` falls inside its period, 0..1.
    phase: f64,
}

impl PacedRig {
    fn new(rate: f64, phase: f64) -> (Self, Arc<Mutex<Wire>>) {
        let wire = Arc::new(Mutex::new(Wire {
            rate,
            ..Wire::default()
        }));
        let now = Instant::now();
        let (inner, _push) = FakeRadio::new(ADDR);
        (
            PacedRig {
                inner,
                wire: wire.clone(),
                out: VecDeque::new(),
                line_free: now,
                epoch: now,
                next_sweep: None,
                sweep_n: 0,
                phase,
            },
            wire,
        )
    }

    /// Put bytes on the line behind whatever is already on it. Returns when the last one has fully
    /// arrived.
    fn send(&mut self, bytes: &[u8]) -> Instant {
        let now = Instant::now();
        let mut at = self.line_free.max(now);
        for &b in bytes {
            at += BYTE_TIME;
            // USB hands bytes over on the next millisecond frame.
            let since = at.duration_since(self.epoch).as_nanos();
            let ms = since.div_ceil(1_000_000);
            let ready = self.epoch + Duration::from_nanos((ms * 1_000_000) as u64);
            self.out.push_back((ready, b));
        }
        self.line_free = at;
        at
    }

    fn reply(&mut self, cmd: u8, data: &[u8]) -> Instant {
        let f = Frame {
            to: CONTROLLER,
            from: ADDR,
            cmd,
            data: data.to_vec(),
        };
        self.send(&f.to_bytes())
    }

    /// One sweep: the header frame (centre mode, ±25 kHz around 144.174 MHz) and ten frames of points.
    fn sweep(&mut self) {
        self.sweep_n += 1;
        let bcd = |v: u8| ((v / 10) << 4) | (v % 10);
        let mut header = vec![0x00, 0x00, bcd(1), bcd(11), 0x00];
        header.extend_from_slice(&freq_to_bcd(144_174_000));
        header.extend_from_slice(&freq_to_bcd(25_000));
        header.push(0x00);
        let mut bursts = vec![header];
        let points: Vec<u8> = (0..POINTS)
            .map(|i| {
                let floor = 20 + ((i as u32 * 7 + self.sweep_n) % 13) as u8;
                if (230..250).contains(&i) {
                    120
                } else {
                    floor
                }
            })
            .collect();
        let mut at = 0;
        for seq in 2..=11u8 {
            let n = if seq <= 6 { 48 } else { 47 };
            let mut d = vec![0x00, 0x00, bcd(seq), bcd(11)];
            d.extend_from_slice(&points[at..at + n]);
            at += n;
            bursts.push(d);
        }
        for d in bursts {
            self.reply(0x27, &d);
        }
        self.wire.lock().unwrap().sweeps += 1;
    }

    /// Put every sweep whose time has come on the line.
    fn sweeps_due(&mut self, now: Instant) {
        let (on, rate) = {
            let w = self.wire.lock().unwrap();
            (w.wave_on, w.rate)
        };
        if !on || rate <= 0.0 {
            self.next_sweep = None;
            return;
        }
        let period = Duration::from_secs_f64(1.0 / rate);
        let next = *self
            .next_sweep
            .get_or_insert_with(|| now + period.mul_f64(self.phase));
        if now >= next {
            self.sweep();
            self.next_sweep = Some(next + period);
        }
    }
}

impl Write for PacedRig {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let now = Instant::now();
        let mut frame = Vec::new();
        for &b in buf {
            frame.push(b);
            if b != 0xFD {
                continue;
            }
            let bytes = std::mem::take(&mut frame);
            let Some(f) = Frame::parse(&bytes) else {
                continue;
            };
            self.wire
                .lock()
                .unwrap()
                .log
                .push((now, f.cmd, f.data.clone()));
            match (f.cmd, f.data.as_slice()) {
                // PTT set: ack it, and say when the ack has fully reached the controller.
                (0x1C, [0x00, v]) => {
                    let on = *v != 0;
                    let acked = self.reply(0xFB, &[]);
                    let mut w = self.wire.lock().unwrap();
                    w.keyed = on;
                    if on {
                        w.ptt_on.push((now, acked));
                    }
                }
                (0x1C, [0x00]) => {
                    let keyed = self.wire.lock().unwrap().keyed;
                    self.reply(0x1C, &[0x00, u8::from(keyed)]);
                }
                // The waveform output: on starts the stream (its first sweep `phase` into a
                // period), off stops it — what is already on the line still arrives.
                (0x27, [0x11, v]) => {
                    self.wire.lock().unwrap().wave_on = *v != 0;
                    self.reply(0xFB, &[]);
                }
                (0x27, [0x10, _]) | (0x27, [0x12, ..]) => {
                    self.reply(0xFB, &[]);
                }
                // Everything else: the register file answers, and the answer joins the queue.
                _ => {
                    self.inner.write_all(&bytes)?;
                    let mut tmp = [0u8; 256];
                    if let Ok(n) = self.inner.read(&mut tmp) {
                        let reply = tmp[..n].to_vec();
                        self.send(&reply);
                    }
                }
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for PacedRig {
    /// A serial port's read with the engine's timeout: whatever has arrived, else wait for the next
    /// byte (or the next sweep) up to the timeout.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let deadline = Instant::now() + crate::civ::engine::READ_TIMEOUT;
        loop {
            let now = Instant::now();
            self.sweeps_due(now);
            let mut n = 0;
            while n < buf.len() {
                match self.out.front() {
                    Some(&(ready, b)) if ready <= now => {
                        buf[n] = b;
                        n += 1;
                        self.out.pop_front();
                    }
                    _ => break,
                }
            }
            if n > 0 {
                return Ok(n);
            }
            if now >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no data"));
            }
            let mut wake = deadline;
            if let Some(&(ready, _)) = self.out.front() {
                wake = wake.min(ready);
            }
            if let Some(t) = self.next_sweep {
                wake = wake.min(t);
            }
            std::thread::sleep(
                wake.saturating_duration_since(now)
                    .max(Duration::from_micros(100)),
            );
        }
    }
}

/// An audio backend that captures nothing and notes when transmit audio starts.
#[derive(Clone, Default)]
struct StampBackend {
    played: Arc<Mutex<Vec<Instant>>>,
}
impl AudioBackend for StampBackend {
    fn capture(&mut self) -> Vec<f32> {
        Vec::new()
    }
    fn play(&mut self, _samples: &[f32]) {
        self.played.lock().unwrap().push(Instant::now());
    }
}

/// The radio loop on a mock IC-9700 behind Nexus's own CI-V daemon, 115200 baud, CAT PTT.
struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    rig: Rig,
    backend: StampBackend,
    feed: tempo_app::engine::SpectrumFeed,
    wire: Arc<Mutex<Wire>>,
    /// Set if the loop ever tried to rebuild the transport (it would drop the daemon under test).
    rebuilt: Arc<std::sync::atomic::AtomicBool>,
}

impl Scene {
    /// `ft8`: the default FT8 operating (a DATA mode, PKTUSB); else Phone (USB).
    fn new(rate: f64, phase: f64, ft8: bool) -> Self {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let settings = {
            let mut e = engine.lock().unwrap();
            let mut s = e.settings().clone();
            s.rig_model = 3081; // IC-9700
            s.baud = 115_200; // the rig refuses `27 11 01` below this
            s.ptt_method = "cat".into();
            e.apply_settings(s);
            if ft8 {
                e.set_tier(Tier::Ft8);
                e.set_tx_enabled(true);
            } else {
                e.set_operating_mode("phone", false);
            }
            e.settings().clone()
        };
        let (radio, wire) = PacedRig::new(rate, phase);
        let daemon =
            CivDaemon::start_with_io(Box::new(radio), ADDR, 0, 1, Some(IcomModel::Ic9700)).unwrap();
        let rig = Rig::rigctld(&format!("127.0.0.1:{}", daemon.local_addr().port()));
        let feed = tempo_app::engine::SpectrumFeed::default();
        let cfg = RadioConfig {
            rig_model: settings.rig_model,
            spectrum_feed: feed.clone(),
            ..RadioConfig::default()
        };
        // The transport the loop compares its settings against: the same settings, so the loop never
        // rebuilds the link (which would replace the daemon under test with a stub).
        let state = RadioLoop::new(
            Transport::from_settings(&settings),
            Some(CatDaemon::Native(daemon)),
            &cfg,
        );
        Scene {
            engine,
            state,
            rig,
            backend: StampBackend::default(),
            feed,
            wire,
            rebuilt: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn step(&mut self, now: f64) {
        let sinks = no_sinks();
        let mut station = StationSinks::new();
        let backend = self.backend.clone();
        let mut reopen_audio = move |_t: &Transport| Ok::<_, String>(backend.clone());
        let rebuilt = self.rebuilt.clone();
        let mut reopen_rig = move |_t: &Transport, _coexist: bool| {
            rebuilt.store(true, std::sync::atomic::Ordering::Relaxed);
            (Rig::vox(), None, CatProbe::status(None, ""))
        };
        self.state
            .step(
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                &sinks,
                now,
                &mut reopen_audio,
                &mut reopen_rig,
                &mut station,
            )
            .unwrap();
        assert!(
            !self.rebuilt.load(std::sync::atomic::Ordering::Relaxed),
            "the loop rebuilt its transport: the scene's daemon is gone"
        );
    }

    /// Run the loop as `run_radio` does — a step, then 20 ms — for `ms` of real time, on the real
    /// clock; `pane` reads the RF slot every tick, as an RF scope pane on screen polls.
    fn run(&mut self, ms: u64, pane: bool) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            if pane {
                let _ = self.feed.rf_frame_after(0);
            }
            self.step(now_unix_ms());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The `27 11` commands on the wire, in order: true = on.
    fn wave_commands(&self) -> Vec<bool> {
        self.wire
            .lock()
            .unwrap()
            .log
            .iter()
            .filter(|(_, c, d)| *c == 0x27 && d.first() == Some(&0x11))
            .map(|(_, _, d)| d.get(1) == Some(&1))
            .collect()
    }

    fn sweeps(&self) -> u32 {
        self.wire.lock().unwrap().sweeps
    }
}

/// THE GATE, BY COMMAND: in a data mode the scope streams only while the RF scope pane reads, and
/// stops when it stops. FT8 is the default mode, which is why this matters on an IC-9700.
#[test]
fn in_a_data_mode_the_scope_streams_only_while_the_rf_pane_reads() {
    let mut s = Scene::new(10.0, 0.5, true);
    // No pane: FT8 operating. The loop starts from its configured mode word (USB) until its first
    // mode command lands, and streams for that tick as it always has; once the rig is in DATA mode
    // the stream is off, and it stays off.
    s.run(1000, false);
    assert_eq!(
        s.state.last_mode, "PKTUSB",
        "premise: the loop commanded FT8's DATA mode"
    );
    assert_eq!(
        s.wave_commands().last(),
        Some(&false),
        "in a data mode with no pane the scope is not left streaming: {:?}",
        s.wave_commands()
    );
    let (commands, sweeps) = (s.wave_commands().len(), s.sweeps());
    s.run(1000, false);
    assert_eq!(
        s.wave_commands().len(),
        commands,
        "the scope was switched again in a data mode with no pane: {:?}",
        s.wave_commands()
    );
    assert_eq!(
        s.sweeps(),
        sweeps,
        "the radio streamed with no pane on screen"
    );

    // The pane comes on screen: its reads ask for the stream.
    s.run(800, true);
    assert_eq!(
        s.wave_commands().last(),
        Some(&true),
        "the pane's reads did not switch the waveform output on"
    );
    assert!(
        s.sweeps() > 0,
        "the radio was asked to stream and sent nothing"
    );
    let frame = s.feed.rf_frame_after(0).expect("an answer");
    assert_eq!(
        (frame.source.as_str(), frame.bins.len()),
        ("civ", POINTS),
        "the pane is drawing the radio's sweep"
    );

    // The pane goes away (hidden, or its cockpit behind another screen): the request lapses two
    // seconds after the last read, and the stream stops by command.
    s.run(2600, false);
    assert_eq!(
        s.wave_commands().last(),
        Some(&false),
        "the stream outlived the pane"
    );
    let settled = s.sweeps();
    s.run(400, false);
    assert_eq!(
        s.sweeps(),
        settled,
        "the radio is still streaming for nobody"
    );
    assert!(
        s.feed.peek_scope_row().is_none_or(|r| r.source != "civ"),
        "the last sweep was left in the RF slot"
    );
}

/// Outside a data mode nothing changed: Phone's scope streams with no RF scope pane at all (the
/// control that the gate above is the data-mode rule and nothing wider).
#[test]
fn outside_a_data_mode_the_scope_streams_as_before_with_no_pane() {
    let mut s = Scene::new(10.0, 0.5, false);
    s.run(1000, false);
    assert!(
        !mode_is_data(&s.state.last_mode),
        "premise: Phone commanded a voice mode, got {:?}",
        s.state.last_mode
    );
    assert_eq!(
        s.wave_commands().last(),
        Some(&true),
        "Phone's scope no longer streams"
    );
    assert!(s.sweeps() > 0);
}

/// THE FT WATERFALL STILL NEVER DRAWS AN RF ROW. Its command answers from `audio_row`, and with the
/// pane streaming the radio's sweep into the RF slot in a data mode, that read still never hands it
/// a sweep — the row the RF pane draws is the one thing it must not see.
#[test]
fn the_ft_waterfall_read_never_sees_the_sweep_the_pane_streams() {
    let mut s = Scene::new(10.0, 0.5, true);
    s.run(600, false);
    let end = Instant::now() + Duration::from_millis(1200);
    let mut sweeps_seen = 0;
    while Instant::now() < end {
        if s.feed.rf_frame_after(0).is_some_and(|f| f.source == "civ") {
            sweeps_seen += 1;
        }
        if let Some(row) = s.feed.audio_row() {
            assert_ne!(
                row.source, "civ",
                "the FT waterfall's read handed out a sweep"
            );
        }
        s.step(now_unix_ms());
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        sweeps_seen > 0,
        "control: the pane really was drawing sweeps while the waterfall read"
    );
}

/// The percentiles of a sample, in ms.
fn summary(mut v: Vec<f64>) -> String {
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    let pick = |p: f64| v[((p * (n - 1) as f64).round() as usize).min(n - 1)];
    let mean = v.iter().sum::<f64>() / n as f64;
    format!(
        "n={n} p50={:.1} p90={:.1} p99={:.1} max={:.1} mean={:.1}",
        pick(0.5),
        pick(0.9),
        pick(0.99),
        v[n - 1],
        mean
    )
}

/// THE EVIDENCE FOR AN FT-TIMING SIGN-OFF: what the stream does to an FT over's keying, measured.
/// Not an assertion about the timing: the operator decides what a shift is worth.
///
/// Four configurations — 3 and 10 sweeps a second, the pane off and on — each its own loop, daemon
/// and radio, taken in turn trial by trial, so whatever else the box is doing falls on all four
/// alike. Each trial: the loop runs on a virtual clock that crosses an even FT8 slot boundary (our TX
/// period, an over queued) in real time, ticking as `run_radio` does (a step, then 20 ms), started a
/// random 0.6–1.5 s before the boundary so the stream is in steady state and the boundary falls
/// anywhere in a tick. Measured from the boundary's real instant: the start of the tick that keyed,
/// the PTT command reaching the radio, the radio's ack reaching the controller, and the transmit audio
/// starting. Between a configuration's trials its clock jumps over the rest of the over and the next
/// receive period, as the loop's own tests do; a pane-on loop that is not ticking keeps its request
/// alive, as a pane on screen would.
///
/// `RF_PANE_TRIALS` (default 40) sets the trials per configuration; `RF_PANE_LATENCY_OUT` names a
/// file for the raw samples (CSV). Run alone, optimised as shipped: `cargo test --release -p tempo-audio
/// --features device,serial --lib rf_pane_ptt_latency -- --ignored --nocapture`.
#[test]
#[ignore = "measurement: minutes of real time; run explicitly for the FT-timing sign-off"]
fn rf_pane_ptt_latency_measurement() {
    let trials: usize = std::env::var("RF_PANE_TRIALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let mut csv =
        String::from("rate,pane,trial,pre_ms,tick_ms,ptt_write_ms,ptt_ack_ms,audio_ms,sweeps\n");
    // A fixed-seed generator: the same pre-rolls and sweep phases on every run.
    let mut seed: u64 = 0x2026_1004;
    let mut rnd = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as f64) / ((1u64 << 31) as f64)
    };
    let configs = [(3.0, false), (3.0, true), (10.0, false), (10.0, true)];
    let mut scenes: Vec<Scene> = configs
        .iter()
        .map(|&(rate, _)| Scene::new(rate, rnd(), true))
        .collect();
    // Settle: DATA mode commanded, the read-only latch asserted, the stream as the pane says.
    for (s, &(_, pane)) in scenes.iter_mut().zip(&configs) {
        s.run(1500, pane);
        assert_eq!(s.state.last_mode, "PKTUSB", "premise: FT8's DATA mode");
    }
    // Virtual time runs ahead of the real clock (the loop's own key-time re-read takes the later of
    // the two), on even FT8 boundaries, 30 s apart.
    let period = 15_000.0;
    let mut first = ((now_unix_ms() / period).floor() + 8.0) * period;
    if (first / period) as u64 % 2 == 1 {
        first += period;
    }
    let mut boundary = [first; 4];
    // Per configuration: (tick, ptt write, ptt ack, audio), ms from the boundary.
    let mut samples: Vec<Vec<[f64; 4]>> = vec![Vec::new(); 4];
    let ms = |t: Instant, at: Instant| -> f64 {
        if t >= at {
            t.duration_since(at).as_secs_f64() * 1000.0
        } else {
            -(at.duration_since(t).as_secs_f64() * 1000.0)
        }
    };
    for trial in 0..trials {
        for (i, &(rate, pane)) in configs.iter().enumerate() {
            let pre = 600.0 + 900.0 * rnd();
            {
                let mut e = scenes[i].engine.lock().unwrap();
                e.broadcast("CQ TEST W9XYZ EN37");
                // The boundary path, as every over after the first in a run keys.
                let _ = e.take_immediate_tx();
            }
            let pings = scenes[i].wire.lock().unwrap().ptt_on.len();
            let plays = scenes[i].backend.played.lock().unwrap().len();
            let sweeps0 = scenes[i].sweeps();
            let w0 = Instant::now();
            let at_boundary = w0 + Duration::from_secs_f64(pre / 1000.0);
            let start_v = boundary[i] - pre;
            let mut tick_at = None;
            loop {
                let v = start_v + w0.elapsed().as_secs_f64() * 1000.0;
                // Every pane on screen polls, the idle loops' included.
                for (other, &(_, on)) in scenes.iter().zip(&configs) {
                    if on {
                        let _ = other.feed.rf_frame_after(0);
                    }
                }
                let t = Instant::now();
                scenes[i].step(v);
                if scenes[i].backend.played.lock().unwrap().len() > plays {
                    tick_at = Some(t);
                    break;
                }
                if v > boundary[i] + 2000.0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let s = &scenes[i];
            let (ptt_write, ptt_ack) =
                *s.wire.lock().unwrap().ptt_on.get(pings).unwrap_or_else(|| {
                    panic!("rate {rate} pane {pane} trial {trial}: no PTT on the wire")
                });
            let audio = s.backend.played.lock().unwrap()[plays];
            let tick = tick_at.expect("the over keyed");
            let sweeps = s.sweeps() - sweeps0;
            let row = [
                ms(tick, at_boundary),
                ms(ptt_write, at_boundary),
                ms(ptt_ack, at_boundary),
                ms(audio, at_boundary),
            ];
            csv.push_str(&format!(
                "{rate},{pane},{trial},{pre:.1},{:.2},{:.2},{:.2},{:.2},{sweeps}\n",
                row[0], row[1], row[2], row[3]
            ));
            samples[i].push(row);
            // The controls, every trial: the pane-off link carries no sweep, the pane-on link does
            // (the trial spans at least 0.6 s of stream before the boundary).
            if pane {
                assert!(
                    sweeps > 0,
                    "rate {rate} trial {trial}: the pane was on and nothing streamed"
                );
            } else {
                assert_eq!(
                    sweeps, 0,
                    "rate {rate} trial {trial}: the pane was off and the radio streamed"
                );
            }
            boundary[i] += 2.0 * period;
        }
    }
    for (i, &(rate, pane)) in configs.iter().enumerate() {
        let col = |k: usize| samples[i].iter().map(|r| r[k]).collect::<Vec<f64>>();
        let in_tick = samples[i].iter().map(|r| r[1] - r[0]).collect::<Vec<f64>>();
        let ack_wait = samples[i].iter().map(|r| r[2] - r[1]).collect::<Vec<f64>>();
        println!(
            "RF-PANE LATENCY rate={rate}/s pane={}\n  tick start : {}\n  ptt write  : {}\n  ptt ack    : {}\n  audio      : {}\n  tick->write: {}\n  write->ack : {}",
            if pane { "ON " } else { "OFF" },
            summary(col(0)),
            summary(col(1)),
            summary(col(2)),
            summary(col(3)),
            summary(in_tick),
            summary(ack_wait)
        );
    }
    if let Ok(path) = std::env::var("RF_PANE_LATENCY_OUT") {
        std::fs::write(&path, csv).expect("write the samples");
        println!("samples: {path}");
    }
}
