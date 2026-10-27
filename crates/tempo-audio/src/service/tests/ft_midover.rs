//! WSJT-X's mid-over message switch on the real loop: a decode of the period that just ended,
//! folded while this station's FT8/FT4 over is on the air, that moves the QSO switches the rest of
//! the over to the message the sequencer now sends (`widgets/mainwindow.cpp:9446` sets
//! `m_restart`; `:7880` regenerates the wave; `Modulator/Modulator.cpp:329` reads it on from the
//! sample it had reached). The over keeps its key, its window and its PTT.
//!
//! The loop is the real `RadioLoop::step`, a 20 ms tick at a time, on a rigctld that keys and a
//! mock sound card, which a scene tells how far its playback has got (`MockBackend::tx_unplayed`).
//! The decode is the real decoder's, on a real FT8/FT4 capture of the partner's over, handed to the
//! loop as its worker hands it over. Every scene runs 30 s or more ahead of the real clock, on an
//! even (this station's) boundary.

use super::decode_in_flight::ft8_frame;
use super::*;
use crate::rig::remote_tests::{retuning_peer, writes, Peer};
use tempo_app::engine::{run_decode_job_streaming, DecodePass, DecodeProgress, StreamedDecode};

const TX2: &str = "W1AW KD9TAW -10";
const RR73: &str = "W1AW KD9TAW RR73";

/// A route that cannot replace what it was handed, as one with nothing left to play cannot.
#[derive(Default)]
struct NoRetune(MockBackend);

impl AudioBackend for NoRetune {
    fn capture(&mut self) -> Vec<f32> {
        self.0.capture()
    }
    fn play(&mut self, samples: &[f32]) {
        self.0.play(samples)
    }
    fn flush_output(&mut self) -> usize {
        self.0.flush_output()
    }
}

/// W1AW's over `msg`, as this station's sound card captured it in the period before `slot`.
pub(super) fn capture_of(tier: Tier, msg: &str) -> Vec<f32> {
    if tier == Tier::Ft8 {
        return ft8_frame(msg);
    }
    use tempo_core::modes::{make_mode, ModeKind};
    let mode = make_mode(ModeKind::Ft4);
    let wave = mode.gen_wave(&mode.encode(msg), tempo_fast::SAMPLE_RATE, 1500.0);
    let mut frame = vec![0f32; mode.frame_samples()];
    for (s, &v) in frame.iter_mut().zip(&wave) {
        *s = v * 0.0305;
    }
    frame
}

/// A station working W1AW, which answered its CQ with a grid: the report (Tx2) keys at the next
/// own boundary, `t0`.
struct Qso<B: AudioBackend> {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: B,
    rig: Rig,
    peer: Peer,
    tier: Tier,
    t: f64,
    t0: f64,
    period_ms: f64,
    /// Hands the loop a decode as its worker does.
    worker: Option<std::sync::mpsc::Sender<WorkerOut>>,
    /// A logger on the WSJT-X UDP feed, when the scene has one.
    logger: Option<Logger>,
}

/// A logger on the loop's WSJT-X UDP feed (GridTracker, JTAlert), and the station sinks the loop
/// keeps the last Status in, kept from step to step as `run_radio` keeps them: a Status between
/// boundaries is told against the one sent before it.
pub(super) struct Logger {
    pub(super) server: WsjtxServer,
    socket: std::net::UdpSocket,
    pub(super) station: StationSinks,
}

impl Logger {
    pub(super) fn new() -> Logger {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_millis(200)))
            .unwrap();
        let server =
            WsjtxServer::new("127.0.0.1:0".parse().unwrap(), socket.local_addr().unwrap()).unwrap();
        Logger {
            server,
            socket,
            station: StationSinks::new(),
        }
    }

    /// The Tx Message of each Status the logger got while the loop was transmitting, in order and
    /// without repeats: what it was told is on the air.
    pub(super) fn told_on_the_air(&self) -> Vec<String> {
        let mut told: Vec<String> = Vec::new();
        let mut buf = [0u8; 4096];
        while let Ok((n, _)) = self.socket.recv_from(&mut buf) {
            let mut r = tempo_net::qds::QdsReader::new(&buf[..n]);
            r.read_u32();
            r.read_u32();
            if r.read_u32() != Some(1) {
                continue;
            }
            let f = wsjtx_status_fields(&buf[..n]);
            if f[7] == "true" && told.last() != Some(&f[21]) {
                told.push(f[21].clone());
            }
        }
        told
    }
}

/// The Tx Message WSJT-X reports for `text` on FT8 and FT4: `msgsent`, blank-padded to 37.
pub(super) fn msgsent(text: &str) -> String {
    format!("{text:<37}")
}

impl<B: AudioBackend + Default> Qso<B> {
    fn new(tier: Tier, backend: B) -> Qso<B> {
        Qso::heard_before(tier, backend, 0)
    }

    /// As [`Qso::new`], with W1AW's grid heard at the boundary `periods` before `t0`'s.
    fn heard_before(tier: Tier, backend: B, periods: u64) -> Qso<B> {
        Qso::station(tier, backend, periods, |_| {})
    }

    /// As [`Qso::new`], the station's settings first changed by `settings` (its radio's
    /// connection).
    fn with_settings(
        tier: Tier,
        backend: B,
        settings: impl FnOnce(&mut tempo_app::settings::Settings),
    ) -> Qso<B> {
        Qso::station(tier, backend, 0, settings)
    }

    /// [`Qso::heard_before`]'s scene, on the station's settings as `settings` leaves them.
    fn station(
        tier: Tier,
        backend: B,
        periods: u64,
        settings: impl FnOnce(&mut tempo_app::settings::Settings),
    ) -> Qso<B> {
        let peer = retuning_peer(14_074_000, "PKTUSB", |line, state| {
            (line == "T 1").then(|| {
                state.keyed = true;
                "RPRT 0\n".to_string()
            })
        });
        let mut base = tempo_app::settings::Settings {
            mycall: "KD9TAW".into(),
            mygrid: "EN52".into(),
            write_all_txt: true,
            ..Default::default()
        };
        settings(&mut base);
        let engine = Arc::new(Mutex::new(Engine::with_settings(base)));
        let period_ms = {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            e.set_tier(tier);
            e.set_frequency(14.074, "20m", "USB");
            e.set_tx_offset(1500.0);
            e.active_slot_secs() * 1000.0
        };
        let pair = 2.0 * period_ms;
        let t0 = ((now_unix_ms() + 30_000.0) / pair).ceil() * pair;
        let slot = (t0 / period_ms) as u64;
        {
            let mut e = engine.lock().unwrap();
            let line = "KD9TAW W1AW FN31";
            assert!(
                e.ingest(&capture_of(tier, line), slot - periods) >= 1,
                "premise: W1AW's grid decoded"
            );
            e.call_station_ctx("W1AW", Some("FN31"), Some(line), Some(-10), None)
                .expect("premise: W1AW worked");
            let _ = e.take_immediate_tx();
            let _ = e.take_all_txt_pending();
        }
        Qso {
            engine,
            state: loop_state(),
            backend,
            rig: Rig::rigctld(&peer.address),
            peer,
            tier,
            t: t0,
            t0,
            period_ms,
            worker: None,
            logger: None,
        }
    }

    /// The live slot at `t0`.
    fn slot(&self) -> u64 {
        (self.t0 / self.period_ms) as u64
    }

    /// Step the loop to `until` ms from `t0`, a 20 ms tick at a time.
    fn run_to(&mut self, until: f64) {
        let mut rr = mock_reopen_rig();
        let mut ra = |_t: &Transport| Ok(B::default());
        let mut fresh = StationSinks::new();
        let (sinks, station) = match &mut self.logger {
            Some(l) => (
                Sinks {
                    wsjtx: Some(&l.server),
                    ..no_sinks()
                },
                &mut l.station,
            ),
            None => (no_sinks(), &mut fresh),
        };
        let end = self.t0 + until;
        while self.t <= end {
            self.state
                .step(
                    &self.engine,
                    &mut self.backend,
                    &mut self.rig,
                    &sinks,
                    self.t,
                    &mut ra,
                    &mut rr,
                    station,
                )
                .unwrap();
            self.t += 20.0;
        }
    }

    /// Hand the loop the decode of W1AW's `msg` for the boundary of `slot`, as the decode worker
    /// hands it over — each decode as the pass found it, then the pass's result: the next tick
    /// drains them.
    fn decoded(&mut self, msg: &str, slot: u64) {
        let mut found = Found::default();
        let result = {
            let e = self.engine.lock().unwrap();
            run_decode_job_streaming(
                e.build_decode_job(capture_of(self.tier, msg), slot, DecodePass::Boundary),
                &mut found,
            )
        };
        assert!(
            result.decodes().iter().any(|d| d.message.trim() == msg),
            "premise: {msg} decoded, got {:?}",
            result.decodes()
        );
        if self.worker.is_none() {
            let (tx, rx) = std::sync::mpsc::channel::<WorkerOut>();
            self.state.decode = DecodeWorker {
                job_tx: None,
                result_rx: rx,
                handle: None,
            };
            self.worker = Some(tx);
        }
        let worker = self.worker.as_ref().unwrap();
        for d in found.0 {
            worker.send(WorkerOut::Found(d)).unwrap();
        }
        worker.send(WorkerOut::Done(result)).unwrap();
    }

    fn ptt_lines(&self) -> Vec<String> {
        writes(&self.peer)
            .into_iter()
            .filter(|l| l.starts_with("T "))
            .collect()
    }

    fn tx_rows(&self) -> Vec<String> {
        self.engine
            .lock()
            .unwrap()
            .take_all_txt_pending()
            .into_iter()
            .filter(|r| r.contains(" Tx "))
            .collect()
    }
}

impl Qso<MockBackend> {
    /// The mock card has played `ms` of the over.
    fn played_for(&mut self, ms: f64) {
        let played = (ms / 1000.0 * tempo_fast::SAMPLE_RATE as f64) as usize;
        self.backend.tx_unplayed = self.backend.played.len() - played;
    }
}

/// `got` is exactly `want`, or where not: an over is 150 000 samples, too many to print.
fn same_audio(got: &[f32], want: &[f32]) -> Result<(), String> {
    if got.len() != want.len() {
        return Err(format!("{} samples, expected {}", got.len(), want.len()));
    }
    match got.iter().zip(want).position(|(a, b)| a != b) {
        Some(i) => Err(format!("differs from sample {i}")),
        None => Ok(()),
    }
}

/// The slot-positioned over `text` keys as on `tier`, at 1500 Hz.
pub(super) fn over_of(tier: Tier, text: &str) -> Vec<f32> {
    let kind = match tier {
        Tier::Ft8 => tempo_core::modes::ModeKind::Ft8,
        _ => tempo_core::modes::ModeKind::Ft4,
    };
    tempo_app::engine::TxWaveform::Native {
        kind,
        text: text.into(),
        f0: 1500.0,
    }
    .build()
}

/// ⭐ THE REPORTED CASE, FT8. W1AW's R-12 was not decoded before our boundary, so the report keyed
/// again; the full decode finds it 3 s into the over. WSJT-X switches the over to RR73. What had
/// played stays the report's, sample for sample; every sample after is RR73's, where an RR73 keyed
/// at the boundary would have it; the over is no longer, its audio stops at 13.64 s and PTT drops at
/// 13.84 s exactly as an over nobody switched; and a second Tx row says what went out.
#[test]
fn ft8_an_r_report_decoded_3_s_into_the_report_switches_the_rest_to_rr73() {
    let mut still = Qso::new(Tier::Ft8, MockBackend::new());
    still.run_to(14_000.0);
    let mut s = Qso::new(Tier::Ft8, MockBackend::new());
    let t0 = s.t0;
    s.run_to(2_980.0);
    let keyed = s.backend.played.clone();
    assert_eq!(
        keyed,
        over_of(Tier::Ft8, TX2),
        "premise: the report keyed at t0"
    );
    assert_eq!(s.tx_rows().len(), 1, "premise: its Tx row");
    let until = s.state.tx_until_ms;
    assert_eq!(until, Some(t0 + 13_840.0), "premise: WSJT-X's PTT off");

    s.played_for(3_000.0);
    s.decoded("KD9TAW W1AW R-12", s.slot());
    s.run_to(3_000.0);
    let now = &s.backend.played;
    let at = 36_000;
    let rr73 = over_of(Tier::Ft8, RR73);
    assert_eq!(
        same_audio(&now[at..], &rr73[at..]),
        Ok(()),
        "the rest is RR73's, in place"
    );
    assert_eq!(
        same_audio(&now[..at], &keyed[..at]),
        Ok(()),
        "what played stays the report's"
    );
    assert!(
        same_audio(&now[at..], &keyed[at..]).is_err(),
        "control: the two overs differ there"
    );
    assert_eq!(s.backend.retunes, 1, "the rest handed to the card once");
    assert_eq!(s.state.tx_until_ms, until, "PTT's end untouched");
    let rows = s.tx_rows();
    assert_eq!(rows.len(), 1, "a second Tx row: {rows:?}");
    assert!(rows[0].ends_with(RR73), "{}", rows[0]);

    s.run_to(14_000.0);
    assert_eq!(s.backend.retunes, 1, "one switch");
    assert_eq!(
        s.ptt_lines(),
        still.ptt_lines(),
        "PTT as for an over nobody switched"
    );
    assert_eq!(s.ptt_lines(), ["T 1", "T 0"]);
    assert_eq!(
        s.backend.flush_calls, still.backend.flush_calls,
        "the audio stopped as for an over nobody switched"
    );
}

/// FT4 the same way: R-12 decoded 2 s into the report switches the rest of it to RR73, and the over
/// ends at 6.04 s / 6.24 s as before.
#[test]
fn ft4_an_r_report_decoded_2_s_into_the_report_switches_the_rest_to_rr73() {
    let mut s = Qso::new(Tier::Ft4, MockBackend::new());
    let t0 = s.t0;
    s.run_to(1_980.0);
    let keyed = s.backend.played.clone();
    assert_eq!(
        keyed,
        over_of(Tier::Ft4, TX2),
        "premise: the report keyed at t0"
    );
    assert_eq!(s.state.tx_until_ms, Some(t0 + 6_240.0), "premise: PTT off");
    s.played_for(2_000.0);
    s.decoded("KD9TAW W1AW R-12", s.slot());
    s.run_to(2_000.0);
    let now = &s.backend.played;
    let at = 24_000;
    let rr73 = over_of(Tier::Ft4, RR73);
    assert_eq!(
        same_audio(&now[at..], &rr73[at..]),
        Ok(()),
        "the rest is RR73's, in place"
    );
    assert_eq!(
        same_audio(&now[..at], &keyed[..at]),
        Ok(()),
        "what played stays the report's"
    );
    assert_eq!(s.backend.retunes, 1);
    assert_eq!(
        s.state.tx_until_ms,
        Some(t0 + 6_240.0),
        "PTT's end untouched"
    );
    s.run_to(7_000.0);
    assert_eq!(s.ptt_lines(), ["T 1", "T 0"]);
}

/// ⛔ SLOT LIVENESS (the stale-boundary incident): a decode of an older period, folded during the
/// over, moves the QSO as it always did, and never touches the over on the air. Its message goes out
/// at the next over, as before.
#[test]
fn a_decode_of_an_older_period_never_switches_the_over() {
    let mut s = Qso::new(Tier::Ft8, MockBackend::new());
    s.run_to(2_980.0);
    let keyed = s.backend.played.clone();
    s.played_for(3_000.0);
    s.decoded("KD9TAW W1AW R-12", s.slot() - 1);
    s.run_to(3_000.0);
    assert_eq!(s.backend.retunes, 0, "nothing handed to the card");
    assert_eq!(s.backend.played, keyed, "the over as keyed");
    assert_eq!(s.tx_rows().len(), 1, "only the key's row");
    s.run_to(30_000.0);
    assert_eq!(
        same_audio(&s.backend.played[keyed.len()..], &over_of(Tier::Ft8, RR73)),
        Ok(()),
        "the RR73 keyed at the next own boundary, as before"
    );
}

/// ⛔ STOP TX WINS AT ONCE. A Stop pressed with the decode already waiting: the tick cuts the over
/// before the decode is folded, and nothing is switched. A Stop just after a switch cuts the
/// switched over on the next tick, as it cuts any over.
#[test]
fn stop_tx_cuts_the_over_whether_or_not_it_was_switched() {
    let mut s = Qso::new(Tier::Ft8, MockBackend::new());
    s.run_to(2_980.0);
    s.played_for(3_000.0);
    s.decoded("KD9TAW W1AW R-12", s.slot());
    s.engine.lock().unwrap().halt_tx();
    let flushes = s.backend.flush_calls;
    s.run_to(3_000.0);
    assert_eq!(s.backend.retunes, 0, "not switched");
    assert_eq!(s.backend.flush_calls, flushes + 1, "the over cut");
    assert_eq!(s.ptt_lines(), ["T 1", "T 0"], "unkeyed on the Stop's tick");
    assert_eq!(s.state.tx_until_ms, None);

    let mut s = Qso::new(Tier::Ft8, MockBackend::new());
    s.run_to(2_980.0);
    s.played_for(3_000.0);
    s.decoded("KD9TAW W1AW R-12", s.slot());
    s.run_to(3_000.0);
    assert_eq!(s.backend.retunes, 1, "premise: switched");
    s.engine.lock().unwrap().halt_tx();
    let flushes = s.backend.flush_calls;
    s.run_to(3_020.0);
    assert_eq!(s.backend.flush_calls, flushes + 1, "the switched over cut");
    assert_eq!(s.ptt_lines(), ["T 1", "T 0"], "unkeyed on the next tick");
    assert_eq!(s.state.tx_until_ms, None);
}

/// No time limit inside the over (WSJT-X has none), but nothing once its audio has stopped at
/// 13.64 s: WSJT-X's `m_transmitting` ends there.
#[test]
fn a_decode_after_the_audio_has_stopped_switches_nothing() {
    let mut s = Qso::new(Tier::Ft8, MockBackend::new());
    s.run_to(13_640.0);
    let keyed = s.backend.played.clone();
    s.backend.tx_unplayed = 0;
    s.decoded("KD9TAW W1AW R-12", s.slot());
    s.run_to(13_660.0);
    assert_eq!(s.backend.retunes, 0);
    assert_eq!(s.backend.played, keyed);

    let mut s = Qso::new(Tier::Ft8, MockBackend::new());
    s.run_to(13_600.0);
    s.backend.tx_unplayed = 0;
    s.decoded("KD9TAW W1AW R-12", s.slot());
    s.run_to(13_620.0);
    assert_eq!(s.backend.retunes, 1, "control: 20 ms earlier it switches");
}

/// A route that cannot replace what it was handed (nothing left to play): the over goes out as first chosen,
/// with no second row, and the RR73 keys at the next own boundary, as before.
#[test]
fn a_route_that_cannot_replace_its_audio_sends_the_over_as_first_chosen() {
    let mut s = Qso::new(Tier::Ft8, NoRetune::default());
    s.run_to(2_980.0);
    let keyed = s.backend.0.played.clone();
    s.tx_rows();
    s.decoded("KD9TAW W1AW R-12", s.slot());
    s.run_to(3_000.0);
    assert_eq!(s.backend.0.played, keyed, "the over as keyed");
    assert!(s.tx_rows().is_empty(), "no second row");
    s.run_to(30_000.0);
    assert_eq!(
        same_audio(
            &s.backend.0.played[keyed.len()..],
            &over_of(Tier::Ft8, RR73)
        ),
        Ok(()),
        "the RR73 keyed at the next own boundary"
    );
}

/// A Tx-slot click during the over (WSJT-X's `on_txb4_clicked`, `:8784`) switches it to the clicked
/// message on the next tick, and a logger on the WSJT-X feed is told so in that tick: WSJT-X sends
/// a Status when its Tx Message changes (`widgets/mainwindow.cpp:8072-8077`).
#[test]
fn a_tx_slot_click_during_the_over_switches_it_on_the_next_tick() {
    let mut s = Qso::new(Tier::Ft8, MockBackend::new());
    s.logger = Some(Logger::new());
    s.run_to(4_980.0);
    let keyed = s.backend.played.clone();
    s.played_for(5_000.0);
    s.engine
        .lock()
        .unwrap()
        .override_next_tx("W1AW", Some("FN31"), RR73);
    s.run_to(5_000.0);
    let at = 60_000;
    let now = &s.backend.played;
    assert_eq!(
        same_audio(&now[at..], &over_of(Tier::Ft8, RR73)[at..]),
        Ok(()),
        "the rest is RR73's"
    );
    assert_eq!(
        same_audio(&now[..at], &keyed[..at]),
        Ok(()),
        "what played stays the report's"
    );
    assert_eq!(s.backend.retunes, 1);
    assert_eq!(
        s.logger.as_ref().unwrap().told_on_the_air(),
        [msgsent(TX2), msgsent(RR73)],
        "the logger is told the report as keyed, then the RR73"
    );
}

/// The decodes a pass streamed, kept in the order it found them.
#[derive(Default)]
struct Found(Vec<StreamedDecode>);

impl DecodeProgress for Found {
    fn found(&mut self, d: StreamedDecode) {
        self.0.push(d);
    }
}

/// The loop's decode worker, held by the test: a job the loop dispatches waits here, and what it
/// hands back reaches the loop only when the test says, so a scene says when a decode lands. The
/// decode is the worker's own (`run_worker_job`), on the frame the loop built.
pub(super) struct HeldWorker {
    jobs: std::sync::mpsc::Receiver<WorkerJob>,
    results: std::sync::mpsc::Sender<WorkerOut>,
    /// The decodes the pass found, not yet handed to the loop.
    found: std::cell::RefCell<Vec<StreamedDecode>>,
    /// The last decoded job's pass and rescue deadline ([`DecodeJob::rescue_give_up_at`]).
    pub(super) last_job: std::cell::Cell<Option<(DecodePass, Option<std::time::Instant>)>>,
}

impl HeldWorker {
    pub(super) fn install(state: &mut RadioLoop) -> HeldWorker {
        let (job_tx, jobs) = std::sync::mpsc::channel::<WorkerJob>();
        let (results, result_rx) = std::sync::mpsc::channel::<WorkerOut>();
        state.decode = DecodeWorker {
            job_tx: Some(job_tx),
            result_rx,
            handle: None,
        };
        HeldWorker {
            jobs,
            results,
            found: Default::default(),
            last_job: Default::default(),
        }
    }

    /// The one decode the loop has dispatched, decoded as its worker decodes it. The loop has
    /// neither the decodes the pass found nor its result yet.
    pub(super) fn decode(&self) -> DecodeResult {
        let job = self.jobs.try_recv().expect("the loop dispatched a decode");
        assert!(self.jobs.try_recv().is_err(), "one decode at a time");
        self.note(&job);
        let mut results = Vec::new();
        run_worker_job(job, &mut |out| match out {
            WorkerOut::Found(d) => self.found.borrow_mut().push(d),
            WorkerOut::Done(r) => results.push(r),
        });
        assert_eq!(results.len(), 1, "a slot decode yields one result");
        results.remove(0)
    }

    /// [`HeldWorker::decode`], when the loop has dispatched a decode.
    pub(super) fn try_decode(&self) -> Option<DecodeResult> {
        let job = self.jobs.try_recv().ok()?;
        assert!(self.jobs.try_recv().is_err(), "one decode at a time");
        self.note(&job);
        let mut results = Vec::new();
        run_worker_job(job, &mut |out| match out {
            WorkerOut::Found(d) => self.found.borrow_mut().push(d),
            WorkerOut::Done(r) => results.push(r),
        });
        assert_eq!(results.len(), 1, "a slot decode yields one result");
        Some(results.remove(0))
    }

    fn note(&self, job: &WorkerJob) {
        if let WorkerJob::Slot(j) = job {
            self.last_job.set(Some((j.pass(), j.rescue_give_up_at())));
        }
    }

    /// The messages the pass found and has not handed to the loop yet, in the order it found them.
    pub(super) fn found_messages(&self) -> Vec<String> {
        self.found
            .borrow()
            .iter()
            .map(|d| d.decode().message.trim().to_string())
            .collect()
    }

    /// Hand the loop the decodes the pass found, as the worker hands each on the moment it finds
    /// it; the pass's end is still to land.
    pub(super) fn stream(&self) {
        for d in self.found.borrow_mut().drain(..) {
            self.results.send(WorkerOut::Found(d)).unwrap();
        }
    }

    /// Hand `result` to the loop, after any decode of its pass not yet handed over: its next tick
    /// drains them.
    pub(super) fn land(&self, result: DecodeResult) {
        self.stream();
        self.results.send(WorkerOut::Done(result)).unwrap();
    }
}

/// W1AW's RR73 to our report, and the 73 that answers it.
const RR73_TO_US: &str = "KD9TAW W1AW RR73";
const BYE73: &str = "W1AW KD9TAW 73";

/// THE REPORTED CASE, END TO END, up to the switch. We sent W1AW our report two periods before
/// `t0`, and W1AW rogered it with RR73 at once, too weakly for the early pass (it ran and found
/// nothing), so only the full pass finds it. The loop runs that pass at WSJT-X's `m_hsymStop`
/// (FT8 14.40 s, FT4 6.048 s), and it is still running at `t0`: the boundary keys the report again,
/// on time (WSJT-X keys at t = 0 whatever its decoder is doing), and the pass lands `land_ms` into
/// that over, the card having played that much of it. A logger listens on the WSJT-X feed. Returns
/// the scene after that tick and the over as keyed.
fn rr73_found_only_by_the_late_full_pass(tier: Tier, land_ms: f64) -> (Qso<MockBackend>, Vec<f32>) {
    let mut s = Qso::heard_before(tier, MockBackend::new(), 2);
    s.logger = Some(Logger::new());
    let period = s.period_ms;
    let worker = HeldWorker::install(&mut s.state);
    let rx_slot = s.slot() - 1;
    s.t = s.t0 - period;
    s.run_to(20.0 - period);
    s.state.early_done_slot = Some(rx_slot);
    // The full pass's moment, on the loop's 20 ms tick, and the audio it decodes: the period from
    // its start, W1AW's RR73 in it.
    let full_at = if tier == Tier::Ft8 { 14_400.0 } else { 6_048.0 };
    let due = (full_at / 20.0_f64).ceil() * 20.0;
    let heard = capture_of(tier, RR73_TO_US);
    let n = ((due / 1000.0 * tempo_fast::SAMPLE_RATE as f64) as usize).min(heard.len());
    s.run_to(due - period - 40.0);
    s.backend.queue_capture(heard[..n].to_vec());
    s.run_to(due - period - 20.0);
    assert!(
        !s.state.decode_in_flight,
        "premise: nothing decoded before the full pass's moment"
    );
    s.run_to(due - period);
    assert!(
        s.state.decode_in_flight && s.state.full_done_slot == Some(rx_slot),
        "the full pass dispatched at {full_at} ms"
    );
    let full = worker.decode();
    assert_eq!(full.pass(), DecodePass::Boundary, "the full pass");
    assert_eq!(full.slot(), s.slot(), "for t0's boundary");
    assert!(
        full.decodes()
            .iter()
            .any(|d| d.message.trim() == RR73_TO_US),
        "premise: the full pass finds W1AW's RR73, got {:?}",
        full.decodes()
    );

    s.run_to(0.0);
    let keyed = s.backend.played.clone();
    assert_eq!(
        same_audio(&keyed, &over_of(tier, TX2)),
        Ok(()),
        "the report keyed again at t0, as before the fix"
    );
    assert!(s.state.decode_in_flight, "the full pass still running");
    assert_eq!(s.state.dropped_decodes, 0, "nothing dropped");
    assert_eq!(s.tx_rows().len(), 1, "premise: the report's Tx row");

    s.run_to(land_ms - 20.0);
    assert_eq!(
        s.backend.retunes, 0,
        "premise: nothing switched before it lands"
    );
    s.played_for(land_ms);
    worker.land(full);
    s.run_to(land_ms);
    (s, keyed)
}

/// What a decoder on W1AW's side copies from the over `played`.
fn copied(tier: Tier, played: &[f32]) -> Vec<String> {
    let kind = match tier {
        Tier::Ft8 => tempo_core::modes::ModeKind::Ft8,
        _ => tempo_core::modes::ModeKind::Ft4,
    };
    wsjt_messages_played(kind, played)
}

/// The over after the switch: what played stays the report's, the rest is the 73's in place, and
/// because the switch was inside the clean-switch window (the first symbols are the Costas sync,
/// the same for every message) the over on the air is the 73's, sample for sample: W1AW copies a
/// 73, not the report again. The over keeps its length and its PTT end, a second Tx row says what
/// went out, TX goes off after it ("Disable Tx after sending 73"), and nothing more is sent (the
/// engine's `rr73_folded_while_the_report_is_on_the_air_switches_the_rest_to_73` holds the log).
/// The logger was told the report as keyed, then the 73 (WSJT-X sends a Status when its Tx
/// Message changes, `widgets/mainwindow.cpp:8072-8077`).
fn the_over_on_the_air_is_the_73(mut s: Qso<MockBackend>, keyed: Vec<f32>, at: usize) {
    let tier = s.tier;
    let bye = over_of(tier, BYE73);
    let played = s.backend.played.clone();
    assert_eq!(copied(tier, &played), [BYE73], "W1AW copies the 73");
    assert_eq!(
        copied(tier, &keyed),
        [TX2],
        "control: the over as keyed is the report"
    );
    assert_eq!(s.backend.retunes, 1, "the rest handed to the card once");
    assert_eq!(
        same_audio(&played[at..], &bye[at..]),
        Ok(()),
        "the rest is the 73's, in place"
    );
    assert_eq!(
        same_audio(&played[..at], &keyed[..at]),
        Ok(()),
        "what played stays the report's"
    );
    assert_eq!(
        same_audio(&played, &bye),
        Ok(()),
        "inside the clean-switch window the over on the air is the 73's"
    );
    assert!(
        same_audio(&keyed, &bye).is_err(),
        "control: the report's over is not the 73's"
    );
    let rows = s.tx_rows();
    assert_eq!(rows.len(), 1, "a second Tx row: {rows:?}");
    assert!(rows[0].ends_with(BYE73), "{}", rows[0]);

    s.run_to(s.period_ms - 500.0);
    assert_eq!(s.state.tx_until_ms, None, "the over ended");
    assert_eq!(s.ptt_lines(), ["T 1", "T 0"]);
    s.run_to(2.0 * s.period_ms + 1_000.0);
    assert_eq!(
        s.backend.played.len(),
        bye.len(),
        "nothing keyed after the 73"
    );
    assert_eq!(s.ptt_lines(), ["T 1", "T 0"]);
    let e = s.engine.lock().unwrap();
    assert!(!e.tx_enabled(), "TX off after the 73");
    assert_eq!(
        e.snapshot().qso.and_then(|q| q.tx_now),
        None,
        "the 73 went once: nothing left to send"
    );
    assert_eq!(
        s.logger.as_ref().unwrap().told_on_the_air(),
        [msgsent(TX2), msgsent(BYE73)],
        "the logger is told the report as keyed, then the 73"
    );
}

/// ⭐⭐ THE REPORTED CASE, END TO END, FT8: the RR73 only the late full pass finds lands 1 s into
/// the report keyed again, inside the first 1.4 s, and the rest of the over becomes the 73.
#[test]
fn ft8_an_rr73_only_the_late_full_pass_finds_turns_the_report_on_the_air_into_73() {
    let (s, keyed) = rr73_found_only_by_the_late_full_pass(Tier::Ft8, 1_000.0);
    let until = s.state.tx_until_ms;
    assert_eq!(until, Some(s.t0 + 13_840.0), "PTT's end untouched");
    the_over_on_the_air_is_the_73(s, keyed, 12_000);
}

/// ⭐⭐ THE SAME, FT4: the 6.048 s pass lands 400 ms into the report keyed again, before its
/// tones start, and the over on the air is the 73.
#[test]
fn ft4_an_rr73_only_the_late_full_pass_finds_turns_the_report_on_the_air_into_73() {
    let (s, keyed) = rr73_found_only_by_the_late_full_pass(Tier::Ft4, 400.0);
    let until = s.state.tx_until_ms;
    assert_eq!(until, Some(s.t0 + 6_240.0), "PTT's end untouched");
    the_over_on_the_air_is_the_73(s, keyed, 4_800);
}

// ── The TCI route ────────────────────────────────────────────────────────────────────────────

/// A sound card with a transmit tee, as the loop installs Nexus's TCI client's: an over goes to the
/// tee instead of the card, and a Stop's flush and a mid-over switch go to it too.
#[derive(Default)]
struct TeeCard {
    tee: Option<crate::backend::TxTeeHandle>,
    played: Vec<f32>,
    /// What was handed to the tee.
    teed: Vec<f32>,
    retunes: usize,
}

impl AudioBackend for TeeCard {
    fn capture(&mut self) -> Vec<f32> {
        Vec::new()
    }
    fn play(&mut self, samples: &[f32]) {
        match &self.tee {
            Some(tee) => {
                self.teed.extend_from_slice(samples);
                tee.feed(samples);
            }
            None => self.played.extend_from_slice(samples),
        }
    }
    fn set_tx_tee(&mut self, tee: Option<crate::backend::TxTeeHandle>) {
        self.tee = tee;
    }
    fn flush_output(&mut self) -> usize {
        self.tee.as_ref().map_or(0, |t| t.flush())
    }
    fn retune_tx(&mut self, tail: &mut dyn FnMut(usize, usize) -> Vec<f32>) -> bool {
        let switched = self.tee.as_ref().is_some_and(|t| t.retune(tail));
        self.retunes += usize::from(switched);
        switched
    }
}

impl Qso<TeeCard> {
    /// Step the loop to `until` ms from `t0`, a 20 ms tick at a time on the scene's grid, each tick
    /// no sooner than the real clock reaches it: the SDR program pulls the over in real time, so
    /// the loop runs at its pace.
    fn run_in_real_time_to(&mut self, until: f64) {
        let mut rr = mock_reopen_rig();
        let mut ra = |_t: &Transport| Ok(TeeCard::default());
        let mut station = StationSinks::new();
        let (from, w0) = (self.t, Instant::now());
        while self.t <= self.t0 + until {
            let due = w0 + Duration::from_secs_f64((self.t - from) / 1000.0);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
            self.state
                .step(
                    &self.engine,
                    &mut self.backend,
                    &mut self.rig,
                    &no_sinks(),
                    self.t,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
            self.t += 20.0;
        }
    }
}

/// One TX_CHRONO's ask at the simulated ExpertSDR3's stream: 1024 frames at 48 kHz.
const CHRONO_FRAMES: usize = 1_024;

/// What the TCI route makes of the modem-rate `over` for a server at 48 kHz, from its start, at
/// full scale.
fn tci_48k(over: &[f32]) -> Vec<f32> {
    crate::capture_resample::CaptureResampler::new(12_000, 48_000)
        .process(over)
        .into_iter()
        .map(|v| v.clamp(-1.0, 1.0))
        .collect()
}

/// Where `a` and `b` first differ by more than float noise, or `None`.
fn first_difference(a: &[f32], b: &[f32]) -> Option<usize> {
    a.iter().zip(b).position(|(x, y)| (x - y).abs() > 1e-6)
}

/// How many transmit frames the simulated SDR program has read from its first client.
fn answers(sim: &tempo_tcisim::Simulator) -> usize {
    sim.log()
        .iter()
        .filter(|l| matches!(l.event, tempo_tcisim::Event::FrameIn { conn: 0, .. }))
        .count()
}

/// ⭐⭐ THE REPORTED CASE ON THE TCI ROUTE, FT8: Nexus's TCI client carries the over to a simulated
/// SDR program, which pulls it in real time, and R-12 decoded 3 s into the report switches the rest
/// to RR73 there. What the program pulled before the switch stays the report's, sample for sample;
/// the rest is RR73's in place, spliced at one point as if the over had been built that way; the
/// over is no longer; PTT's end is untouched (`trx:0,false` at the over's end); and a second Tx row
/// says what went out. The key is Nexus's audio (`trx:0,true,tci`), and nothing is played on the
/// sound card. The station's radio is on the TCI connection, end to end: the engine plans the over
/// under its rules for a TCI radio, and the loop keys it through the client. NEEDS-BENCH: the
/// operator's bench of FT on TCI times what this simulator cannot.
#[test]
fn ft8_a_switch_on_the_tci_route_replaces_only_what_the_sdr_program_has_not_pulled() {
    let sim = tempo_tcisim::Simulator::start(tempo_tcisim::Config::dialect(
        tempo_tcisim::Dialect::expert_sdr3(),
    ))
    .expect("the simulator starts");
    let trx = tempo_net::tci::Trx::new(0).unwrap();
    crate::tci::registry::forget((sim.addr(), trx));
    let target = crate::tci::Target {
        host: "127.0.0.1".into(),
        port: sim.addr().port(),
        trx: 0,
        dialect: tempo_app::settings::TciDialect::Auto,
        audio: true,
        rigctld_port: 0,
    };
    let d = crate::tci::TciDaemon::start_receiving(&target, crate::tci::Options::default())
        .expect("the client starts");
    assert!(
        d.session()
            .wait_until(Duration::from_secs(10), |s| s.rx_audio_fresh
                && s.model.trx(trx).audio.is_true()),
        "premise: the SDR program took Nexus's audio stream"
    );
    let mut s = Qso::with_settings(Tier::Ft8, TeeCard::default(), |settings| {
        settings.rig_model = 7;
        settings.rig_conn = "tci".into();
        settings.tci_host = "127.0.0.1".into();
        settings.tci_port = sim.addr().port();
        settings.ptt_method = "cat".into();
    });
    let t0 = s.t0;
    let applied = Transport::from_settings(s.engine.lock().unwrap().settings());
    assert_eq!(
        applied.tci_target().map(|t| t.port),
        Some(target.port),
        "premise: the engine's radio is the client's TCI server"
    );
    s.rig = Rig::rigctld(&format!("127.0.0.1:{}", d.local_addr().port()));
    s.rig.set_slow_transport(true);
    let cfg = RadioConfig {
        rig_model: 7,
        ..RadioConfig::default()
    };
    // The loop's transport is the engine's own settings, so it never rebuilds the client.
    s.state = RadioLoop::new(applied, Some(CatDaemon::Tci(d)), &cfg);
    // The loop settles the dial and the mode on the client before the boundary.
    s.t = t0 - 600.0;
    s.run_in_real_time_to(2_980.0);
    assert!(
        s.backend.tee.is_some(),
        "premise: the loop installed the TCI tee"
    );
    let until = s.state.tx_until_ms;
    assert_eq!(until, Some(t0 + 13_840.0), "premise: WSJT-X's PTT off");
    assert_eq!(s.tx_rows().len(), 1, "premise: the report's Tx row");
    assert_eq!(
        same_audio(&s.backend.teed, &over_of(Tier::Ft8, TX2)),
        Ok(()),
        "premise: the report, keyed at t0, handed to the TCI route whole"
    );
    let pulled_then = answers(&sim);
    s.decoded("KD9TAW W1AW R-12", s.slot());
    s.run_in_real_time_to(3_100.0);
    assert_eq!(
        s.backend.retunes, 1,
        "the rest handed to the TCI route once"
    );
    assert_eq!(s.state.tx_until_ms, until, "PTT's end untouched");
    let rows = s.tx_rows();
    assert_eq!(rows.len(), 1, "a second Tx row: {rows:?}");
    assert!(rows[0].ends_with(RR73), "{}", rows[0]);
    s.run_in_real_time_to(14_400.0);
    assert_eq!(s.state.tx_until_ms, None, "the over ended");
    assert_eq!(s.backend.retunes, 1, "one switch");
    assert!(s.backend.played.is_empty(), "played on the sound card");
    let keys: Vec<String> = sim
        .commands_from(0)
        .into_iter()
        .filter(|c| c.starts_with("trx:"))
        .collect();
    assert_eq!(
        keys,
        ["trx:0,true,tci", "trx:0,false"],
        "the key and its end"
    );
    // What the SDR program pulled.
    let sent = sim.tx_audio(0);
    let keyed = over_of(Tier::Ft8, TX2);
    let rr73 = over_of(Tier::Ft8, RR73);
    let unswitched = tci_48k(&keyed);
    // Where the over starts in what was pulled: its first tone after its silent lead-in.
    let lead = unswitched
        .iter()
        .position(|v| *v != 0.0)
        .expect("tones in the over");
    let k = sent
        .iter()
        .position(|v| *v != 0.0)
        .expect("the over reached the SDR program")
        - lead;
    assert!(sent.len() >= k + unswitched.len(), "the whole over pulled");
    let over = &sent[k..k + unswitched.len()];
    assert!(
        sent[k + unswitched.len()..].iter().all(|v| *v == 0.0),
        "the over is no longer"
    );
    let pulled_before = (pulled_then - k / CHRONO_FRAMES) * CHRONO_FRAMES / 4;
    let splice = first_difference(over, &unswitched).expect("the over was switched");
    assert!(
        splice >= pulled_before * 4,
        "what was pulled before the decode stays the report's: first change at {splice}, {} \
         pulled",
        pulled_before * 4
    );
    let at = (splice / 4).saturating_sub(64)..splice / 4 + 160;
    let j = at
        .clone()
        .find(|&j| {
            let mut spliced = keyed[..j].to_vec();
            spliced.extend_from_slice(&rr73[j..]);
            first_difference(over, &tci_48k(&spliced)).is_none()
        })
        .unwrap_or_else(|| panic!("no single splice in {at:?} makes what was pulled"));
    assert!(
        j >= pulled_before,
        "spliced after what was pulled: {j} < {pulled_before}"
    );
}
