//! A FREQUENCY READ-BACK THE RADIO CANNOT BE ON IS NOT A KNOB TURN. A wrong rig model (the
//! FT-1000MP driver on an FT-1000D) or a garbled reply can hand the loop a number far outside the
//! receive range the rig reported in its own `\dump_state`: 115.2 MHz off an HF-only backend. Adopted
//! as the dial, the next push sent it back to the radio. Checked through the real `RadioLoop::step`,
//! a `Rig` over TCP and the in-crate rigctld server in front of a backend whose reads the test sets.
use super::*;

/// A radio whose frequency read the test controls, recording every frequency written to it.
struct Garbled {
    read: Mutex<u64>,
    writes: Mutex<Vec<u64>>,
    ranges: Option<Vec<(u64, u64)>>,
    /// A dial the radio refuses, staying where it was.
    refuse: Mutex<Option<u64>>,
}

impl crate::rigctld_server::RigBackend for Garbled {
    fn freq_hz(&self) -> u64 {
        *self.read.lock().unwrap()
    }
    fn mode(&self) -> (String, u32) {
        ("USB".into(), 2400)
    }
    fn ptt(&self) -> bool {
        false
    }
    fn set_freq(&self, hz: u64) -> bool {
        self.writes.lock().unwrap().push(hz);
        if *self.refuse.lock().unwrap() == Some(hz) {
            return false;
        }
        *self.read.lock().unwrap() = hz;
        true
    }
    fn set_mode(&self, _m: &str, _p: u32) -> bool {
        true
    }
    fn set_ptt(&self, _on: bool) -> bool {
        true
    }
    fn rx_ranges(&self) -> Option<Vec<(u64, u64)>> {
        self.ranges.clone()
    }
}

struct Bench {
    radio: Arc<Garbled>,
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    rig: Rig,
    tick: f64,
}

impl Bench {
    /// A loop settled on 14.250 MHz USB, phone, the range probe done.
    fn new(ranges: Option<Vec<(u64, u64)>>) -> Self {
        let radio = Arc::new(Garbled {
            read: Mutex::new(14_250_000),
            writes: Mutex::new(Vec::new()),
            ranges,
            refuse: Mutex::new(None),
        });
        let backend: Arc<dyn crate::rigctld_server::RigBackend> = radio.clone();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || crate::rigctld_server::serve(listener, backend));
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        {
            let mut e = engine.lock().unwrap();
            e.set_operating_mode("phone", false);
            e.set_frequency(14.250, "20m", "USB");
        }
        let mut b = Bench {
            radio,
            engine,
            state: loop_state(),
            rig: Rig::rigctld(&format!("127.0.0.1:{port}")),
            tick: 0.0,
        };
        b.steps(6);
        assert_eq!(
            b.state.last_dial, 14_250_000,
            "the premise: settled on 20 m"
        );
        b
    }

    fn steps(&mut self, n: usize) {
        self.run(n, true);
    }

    /// Ticks on which only the fast dial mirror reads the radio, the heavy poll not being due.
    fn fast_steps(&mut self, n: usize) {
        self.run(n, false);
    }

    fn run(&mut self, n: usize, heavy: bool) {
        let mut backend = MockBackend::new();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        for _ in 0..n {
            self.state.last_rig_poll = if heavy {
                self.tick - RIG_POLL_MS - 1.0 // the heavy poll is due
            } else {
                self.tick + 400.0 // it is not, so the fast mirror reads
            };
            self.tick += 400.0;
            self.state
                .step(
                    &self.engine,
                    &mut backend,
                    &mut self.rig,
                    &sinks,
                    self.tick,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        }
    }

    /// The radio's next reads say `hz`, and the loop polls them.
    fn reads(&mut self, hz: u64) {
        *self.radio.read.lock().unwrap() = hz;
        self.steps(4);
    }

    /// The operator picks LSB on the Phone cockpit, and the mode change re-pushes the dial the
    /// cockpit shows (mode before dial). Returns every frequency written to the radio meanwhile.
    fn flip_sideband(&mut self) -> Vec<u64> {
        let before = self.radio.writes.lock().unwrap().len();
        self.engine
            .lock()
            .unwrap()
            .request_sideband_override(Some("LSB"));
        self.steps(4);
        self.radio.writes.lock().unwrap()[before..].to_vec()
    }

    fn dial_mhz(&self) -> f64 {
        self.engine.lock().unwrap().settings().dial_mhz
    }

    /// A Remote viewer (the stream page or a second PC) watches this radio, and one poll has
    /// shown it the settled dial.
    fn watched_remotely(mut self) -> Self {
        self.state.applied = Transport::from_settings(engine_lock(&self.engine).settings());
        self.state.remote_radio_id = Some(engine_lock(&self.engine).settings().active_radio);
        self.steps(2);
        assert_eq!(
            self.remote_dial_mhz(),
            Some(14.250),
            "the premise: Remote shows the radio's dial"
        );
        self
    }

    /// The radio's dial as the Remote reading reports it.
    fn remote_dial_mhz(&self) -> Option<f64> {
        engine_lock(&self.engine)
            .remote_monitor_observation()
            .radio
            .rig_dial_mhz
    }
}

/// The FT-1000MP driver's own `\dump_state` RX range: 100 kHz to 30 MHz.
fn hf_only() -> Option<Vec<(u64, u64)>> {
    Some(vec![(100_000, 30_000_000)])
}

/// ⭐ THE BUG: 115.2 MHz read off a rig that reported 0.1–30 MHz is not adopted, the dial keeps
/// 14.250, and the sideband flip writes 14.250, never 115.2. The operator is told once, on the CAT
/// status line, which names the read and the dial that was kept.
#[test]
fn a_read_outside_the_rigs_own_range_is_not_adopted_or_written_back() {
    let mut b = Bench::new(hf_only());
    b.reads(115_200_000);
    let detail = b.engine.lock().unwrap().snapshot().radio.cat_detail.clone();
    assert!(
        detail.contains("115.2000") && detail.contains("14.2500"),
        "the CAT status names the read and the kept dial: {detail:?}"
    );
    let published = b.engine.lock().unwrap().cat_probe_gen();
    b.reads(113_184_000);
    assert_eq!(
        b.engine.lock().unwrap().cat_probe_gen(),
        published,
        "a radio that keeps answering garbage is named once, not on every poll"
    );
    let wrote = b.flip_sideband();

    assert!(
        (b.dial_mhz() - 14.250).abs() < 1e-9,
        "the dial keeps its last good value: {}",
        b.dial_mhz()
    );
    assert_eq!(
        b.state.last_dial, 14_250_000,
        "nor does the loop believe the read"
    );
    assert!(
        !wrote.is_empty() && wrote.iter().all(|&hz| hz == 14_250_000),
        "the flip re-pushed the dial it kept, and 115.2 MHz never reached the radio: {wrote:?}"
    );
}

/// The control: an in-range read is a knob turn, adopted as before.
#[test]
fn a_read_inside_the_rigs_range_is_adopted() {
    let mut b = Bench::new(hf_only());
    *b.radio.read.lock().unwrap() = 7_074_000;
    b.steps(4);
    assert!((b.dial_mhz() - 7.074).abs() < 1e-9, "{}", b.dial_mhz());
    assert_eq!(b.state.last_dial, 7_074_000);
}

/// Unknown coverage fails open: the same 115.2 MHz read is adopted and written back exactly as
/// before, which also proves the sideband flip is a write-back the first test would have seen.
/// An empty RX list in the dump is how a backend says it does not know (`RigBackend::rx_ranges`).
#[test]
fn with_unknown_coverage_a_read_is_adopted_as_before() {
    let mut b = Bench::new(Some(vec![]));
    assert_eq!(b.state.rx_ranges, None, "the premise: coverage unknown");
    b.reads(115_200_000);
    assert!((b.dial_mhz() - 115.2).abs() < 1e-9, "{}", b.dial_mhz());
    let wrote = b.flip_sideband();
    assert!(wrote.contains(&115_200_000), "{wrote:?}");
}

/// The fast dial mirror (the knob-tracking read between heavy polls) holds the same line. Its
/// positive control comes first: an in-range knob turn read on the fast path alone is adopted.
#[test]
fn the_fast_dial_mirror_does_not_adopt_it_either() {
    let mut b = Bench::new(hf_only());
    *b.radio.read.lock().unwrap() = 7_074_000;
    b.fast_steps(3);
    assert_eq!(
        b.state.last_dial, 7_074_000,
        "the premise: the fast path reads"
    );
    *b.radio.read.lock().unwrap() = 115_200_000;
    b.fast_steps(3);
    assert_eq!(b.state.last_dial, 7_074_000);
    assert!((b.dial_mhz() - 7.074).abs() < 1e-9, "{}", b.dial_mhz());
}

/// The heal after a refused dial reads the radio to say where it still is. A garbled answer there
/// is not where it is either.
#[test]
fn the_heal_after_a_refused_dial_does_not_adopt_it_either() {
    let mut b = Bench::new(hf_only());
    *b.radio.refuse.lock().unwrap() = Some(21_074_000);
    *b.radio.read.lock().unwrap() = 115_200_000;
    b.engine.lock().unwrap().set_frequency(21.074, "15m", "USB");
    b.steps(8);
    assert_eq!(
        b.state.dial_giveup,
        Some(21_074_000),
        "the premise: the loop gave up on the refused dial and healed"
    );
    assert_ne!(b.state.last_dial, 115_200_000);
    assert!((b.dial_mhz() - 115.2).abs() > 1e-3, "{}", b.dial_mhz());
}

/// Remote readings hold the same line: a Remote viewer keeps the last good dial the cockpit keeps,
/// never 115.2 MHz while the shack shows 14.250.
#[test]
fn a_remote_viewer_keeps_the_last_good_dial_too() {
    let mut b = Bench::new(hf_only()).watched_remotely();
    b.reads(115_200_000);
    assert_eq!(b.remote_dial_mhz(), Some(14.250));
}

/// The fast dial mirror feeds Remote too. Positive control first: an in-range knob turn read on
/// the fast path alone reaches the Remote reading.
#[test]
fn the_fast_dial_mirror_keeps_remote_on_the_last_good_dial_too() {
    let mut b = Bench::new(hf_only()).watched_remotely();
    *b.radio.read.lock().unwrap() = 7_074_000;
    b.fast_steps(3);
    assert_eq!(
        b.remote_dial_mhz(),
        Some(7.074),
        "the premise: the fast path feeds Remote"
    );
    *b.radio.read.lock().unwrap() = 115_200_000;
    b.fast_steps(3);
    assert_eq!(b.remote_dial_mhz(), Some(7.074));
}

/// Unknown coverage: Remote reports what the radio said, as before.
#[test]
fn with_unknown_coverage_remote_reports_the_read_as_before() {
    let mut b = Bench::new(Some(vec![])).watched_remotely();
    b.reads(115_200_000);
    assert_eq!(b.remote_dial_mhz(), Some(115.2));
}

/// Test CAT's result is its own probe's. A dial note published while it waits is not taken as the
/// result, and neither is the note the probe's own tick puts up after it: the CAT status is
/// latest-wins, so `test_cat` used to report whichever line was published first, or last.
#[test]
fn a_dial_note_while_test_cat_waits_is_not_its_result() {
    let mut b = Bench::new(hf_only());
    {
        let mut e = engine_lock(&b.engine);
        let mut s = e.settings().clone();
        s.ptt_method = "cat".into();
        s.rig_model = 2; // Hamlib's NET rigctl: the bench's rigctld is the radio
        e.apply_settings(s);
    }
    b.state.applied = Transport::from_settings(engine_lock(&b.engine).settings());
    let asked = engine_lock(&b.engine).request_cat_reprobe();
    // A dial note, published the way the loop publishes one, before the loop runs the probe.
    engine_lock(&b.engine).set_cat_status(Some(true), "rig set to 14.2500 MHz USB".into());
    assert_eq!(
        engine_lock(&b.engine).cat_reprobe_answer(asked),
        None,
        "the dial note is not Test CAT's result"
    );
    // The probe's tick: it probes, then the poll reads 115.2 MHz and puts that note up.
    b.reads(115_200_000);
    let shown = engine_lock(&b.engine).snapshot().radio.cat_detail.clone();
    assert!(
        shown.contains("115.2000"),
        "the premise: the note is on screen: {shown:?}"
    );
    // The probe asked the radio before the poll did, and the radio said 115.2 MHz to both.
    assert_eq!(
        engine_lock(&b.engine).cat_reprobe_answer(asked),
        Some((
            Some(true),
            "Connected — 115.200 MHz (via a shared external rigctld)".to_string()
        )),
        "Test CAT's result is the probe's own line, not the note on screen"
    );
}

/// The ISS channel's uplink is confirmed by the radio reading back the dial the loop just pushed,
/// and that read is an UNCHANGED dial (the push already moved `last_dial`), so it is never a knob
/// adoption. Every successful read must still reach the engine: fed only on adoption, no frame on
/// the ISS would ever key, each dropped after its wait for the uplink. `heavy` picks the reader:
/// the heavy poll, or the fast dial mirror alone.
fn an_iss_frame_keys_on_the_read_back_of_the_uplink_the_loop_pushed(heavy: bool) {
    let mut b = Bench::new(Some(vec![]));
    {
        let mut e = engine_lock(&b.engine);
        e.set_license_class("technician");
        e.aprs_tune(145.825).unwrap();
    }
    b.steps(4);
    assert_eq!(
        b.state.last_dial, 145_825_000,
        "the premise: on the ISS channel"
    );
    let approaching = tempo_app::engine::aprs_iss::IssSky::Up {
        range_rate_km_s: -6.0,
    };
    assert!(
        engine_lock(&b.engine).aprs_iss_tick(approaching, std::time::Instant::now(), false),
        "the premise: the correction is live and moves the dial"
    );
    b.run(4, heavy);
    engine_lock(&b.engine)
        .aprs_beacon(41.88, -87.63, '/', '>', "", &[])
        .unwrap();
    let keyed = (0..6).any(|_| {
        b.run(1, heavy);
        b.state.tx_until_ms.is_some()
    });
    let uplink = engine_lock(&b.engine)
        .aprs_iss_doppler()
        .and_then(|d| d.tx_hz)
        .expect("the premise: the correction has an uplink");
    assert!(
        b.radio.writes.lock().unwrap().contains(&uplink),
        "the premise: the loop pushed the uplink"
    );
    assert!(
        keyed,
        "the radio read back the uplink it was sent, and the frame never keyed"
    );
}

#[test]
fn an_iss_frame_keys_on_the_heavy_polls_read_back_of_the_uplink() {
    an_iss_frame_keys_on_the_read_back_of_the_uplink_the_loop_pushed(true);
}

#[test]
fn an_iss_frame_keys_on_the_fast_dial_mirrors_read_back_of_the_uplink() {
    an_iss_frame_keys_on_the_read_back_of_the_uplink_the_loop_pushed(false);
}
