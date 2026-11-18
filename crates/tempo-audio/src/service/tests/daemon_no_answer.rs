//! An over whose key Nexus's OWN daemon had no answer to is dropped, as it was when those daemons
//! answered the silence as a refusal; the same answer from Hamlib's rigctld plays with its warning,
//! as it always has. One table: every over the loop keys for audio it plays (the voice keyer, the
//! computer microphone, the soundcard CW keyer, RTTY, PSK, APRS and SSTV) against every way a daemon
//! of Nexus's own comes to answer a key `RPRT -5` (the CI-V radio's ack lost, the CI-V link gone, a
//! Flex or TCI session gone under the key), with Hamlib's own `RPRT -5` as each kind's control. The
//! operator's own PTT rides along: receive stays held for all of them.
//!
//! The loop is the real `RadioLoop::step` with the daemon of each cause as its CAT daemon, keying
//! through a stand-in rigctld that answers the key `RPRT -5`: that answer is what each cause puts on
//! the wire, which the daemons' own tests pin (`civ::broker` for the CI-V ack and link, and the
//! `try_set_ptt` of the Flex and TCI clients for a session gone). The loop tells a daemon of its
//! own from Hamlib by the daemon it runs, which is what the table varies.
use super::refused_key::{rigctld, Key, UNCONFIRMED};
use super::*;
use crate::civ::broker::CivDaemon;
use crate::civ::commands::IcomModel;
use crate::civ::engine::tests_support::FakeRadio;
use crate::flex::FlexDaemon;
use crate::tci::{registry, Options as TciOptions, TciDaemon};
use tempo_app::mic::PC_MIC;
use tempo_net::tci::Trx;

/// The PTT line for an over dropped because Nexus's own daemon had no answer to its key.
const DROPPED: &str =
    "The radio didn't confirm PTT, so nothing was sent — check the CAT link to the radio.";

/// Why the key was answered `RPRT -5`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cause {
    /// Nexus's CI-V daemon sent the key and the radio's ack never came.
    CivUnacked,
    /// Nexus's CI-V daemon had no engine left to carry the key (the port closed, the cable out).
    CivGone,
    /// Nexus's Flex client's session went away under the key.
    FlexDropped,
    /// Nexus's TCI client's session went away under the key.
    TciDropped,
    /// Hamlib's rigctld: the rig did not answer. The control.
    Hamlib,
}

const CAUSES: [Cause; 5] = [
    Cause::CivUnacked,
    Cause::CivGone,
    Cause::FlexDropped,
    Cause::TciDropped,
    Cause::Hamlib,
];

/// What keys.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Voice,
    Mic,
    Cw,
    Rtty,
    Psk,
    Aprs,
    Sstv,
    /// The operator's own PTT: nothing of Nexus's plays under it; receive is what it holds.
    Ptt,
}

const KINDS: [Kind; 8] = [
    Kind::Voice,
    Kind::Mic,
    Kind::Cw,
    Kind::Rtty,
    Kind::Psk,
    Kind::Aprs,
    Kind::Sstv,
    Kind::Ptt,
];

/// The CAT daemon of `cause`, and what must live as long as it does (a simulated radio or SDR
/// program). `None` for Hamlib: no daemon of Nexus's own.
fn daemon(cause: Cause) -> (Option<CatDaemon>, Box<dyn std::any::Any>) {
    match cause {
        Cause::CivUnacked | Cause::CivGone => {
            let (radio, push) = FakeRadio::new(0x94);
            let d = CivDaemon::start_with_io(Box::new(radio), 0x94, 0, 1, Some(IcomModel::Ic7300))
                .expect("the CI-V daemon starts");
            (Some(CatDaemon::Native(d)), Box::new(push))
        }
        Cause::FlexDropped => {
            let sim = tempo_flexsim::Simulator::start(
                tempo_flexsim::Session::v4_gui_client(),
                tempo_flexsim::Config::default(),
            )
            .expect("the simulated radio starts");
            let d = FlexDaemon::start(sim.tcp_addr(), 0, false).expect("the Flex client starts");
            (Some(CatDaemon::Flex(d)), Box::new(sim))
        }
        Cause::TciDropped => {
            let sim = tempo_tcisim::Simulator::start(tempo_tcisim::Config::dialect(
                tempo_tcisim::Dialect::expert_sdr3(),
            ))
            .expect("the simulator starts");
            registry::forget((sim.addr(), Trx::new(0).expect("trx 0")));
            let settings = tempo_app::settings::Settings {
                rig_model: 7,
                rig_conn: "tci".into(),
                tci_host: "127.0.0.1".into(),
                tci_port: sim.addr().port(),
                ptt_method: "cat".into(),
                ..tempo_app::settings::Settings::default()
            };
            let mut target = Transport::from_settings(&settings)
                .tci_target()
                .expect("a TCI radio");
            target.rigctld_port = 0;
            let d = TciDaemon::start_receiving(&target, TciOptions::default())
                .expect("the TCI client starts");
            (Some(CatDaemon::Tci(d)), Box::new(sim))
        }
        Cause::Hamlib => (None, Box::new(())),
    }
}

/// What came of one over.
#[derive(Debug)]
struct Outcome {
    /// Any of the over's audio was played.
    played: bool,
    /// The line the operator reads for it: the PTT line, or the over's own keyer or APRS line.
    line: Option<String>,
    /// An audio row published now reaches the receive display (no receive hold).
    receive_shown: bool,
}

fn phone(e: &mut Engine) {
    e.set_operating_mode("phone", true);
    e.set_frequency(14.250, "20m", "USB");
}

/// One over of `kind` on a loop whose CAT daemon is `cause`'s, the key answered `RPRT -5`.
fn run(kind: Kind, cause: Cause) -> Outcome {
    let (proc_, _alive) = daemon(cause);
    let (addr, _log) = rigctld(Key::Answers(-5));
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        if kind == Kind::Mic {
            let mut s = e.settings().clone();
            s.pc_mic = true;
            s.pc_mic_device = "plughw:CARD=Headset,DEV=0".into();
            e.apply_settings(s);
        }
        e.set_license_class("extra");
        match kind {
            Kind::Cw => {
                e.set_cw_keyer("soundcard", 600.0);
                e.set_operating_mode("cw", false);
                e.set_frequency(14.03, "20m", "CW");
            }
            Kind::Rtty => e.set_operating_mode("rtty", false),
            Kind::Psk => e.set_operating_mode("keyboard", false),
            _ => phone(&mut e),
        }
    }
    let mut state = loop_state();
    state.mono_origin = Some(Instant::now());
    state.rigctld_proc = proc_;
    let mut backend = MockBackend::new();
    let mut rig = Rig::rigctld(&addr);
    let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    let mut t = 0.0;
    let mut tick = |state: &mut RadioLoop, backend: &mut MockBackend, rig: &mut Rig, t: f64| {
        if backend.pc_mic_open {
            backend.queue_pc_mic(vec![0.5; PC_MIC.samples(20) as usize]);
        }
        state
            .step(
                &engine,
                backend,
                rig,
                &sinks,
                t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
    };
    // The loop takes up its settings, then TX is armed as entering the section arms it.
    for _ in 0..3 {
        tick(&mut state, &mut backend, &mut rig, t);
        t += 20.0;
    }
    {
        let mut e = engine.lock().unwrap();
        e.set_tx_enabled(true);
        match kind {
            Kind::Voice => e.send_voice(vec![0.05; 12_000]).unwrap(),
            Kind::Mic | Kind::Ptt => e.set_ptt(true),
            Kind::Cw => e.send_cw("CQ TEST"),
            Kind::Rtty => e.rtty_send_text("CQ TEST").unwrap(),
            Kind::Psk => e.psk_send_text("CQ TEST").unwrap(),
            Kind::Aprs => e
                .aprs_beacon(41.88, -87.63, '/', '>', "", &[])
                .expect("the beacon is queued"),
            Kind::Sstv => e
                .sstv_send(vec![0.05; 12_000], "Scottie 1".to_string())
                .unwrap(),
        }
    }
    while t <= 3_000.0 {
        tick(&mut state, &mut backend, &mut rig, t);
        t += 20.0;
    }
    let e = engine.lock().unwrap();
    let line = match kind {
        Kind::Cw => e.cw_keyer_error(),
        Kind::Rtty => e.rtty_state().keyer_error,
        Kind::Psk => e.psk_state().keyer_error,
        Kind::Aprs => e.aprs_tx_notice().map(str::to_string),
        Kind::Voice | Kind::Mic | Kind::Sstv | Kind::Ptt => e.snapshot().radio.audio_error,
    };
    drop(e);
    state.spectrum_feed.publish_audio(tempo_app::dto::Spectrum {
        row: vec![0.5; 8],
        lo_hz: 0.0,
        hi_hz: 4_000.0,
        source: "audio".into(),
    });
    Outcome {
        played: !backend.played.is_empty(),
        line,
        receive_shown: state.spectrum_feed.row().is_some(),
    }
}

/// Whether `line` is the one `kind` gives a key of `cause` whose over is `dropped`.
fn line_is_right(kind: Kind, dropped: bool, line: Option<&str>) -> bool {
    let starts = |prefix: &str| line.is_some_and(|l| l.starts_with(prefix));
    match kind {
        Kind::Voice | Kind::Mic | Kind::Sstv => {
            line == Some(if dropped { DROPPED } else { UNCONFIRMED })
        }
        Kind::Ptt => line == Some(UNCONFIRMED),
        // The keyers' and APRS's own lines are the ones they gave such a drop before (the log line
        // carries the cause).
        Kind::Cw => starts("Soundcard keyer: the rig didn't accept PTT"),
        Kind::Rtty => starts("AFSK keyer: the rig didn't accept PTT"),
        Kind::Psk => starts("PSK keyer: the rig didn't accept PTT"),
        Kind::Aprs if dropped => starts("APRS not sent: the radio did not accept the key"),
        Kind::Aprs => line.is_none(),
    }
}

/// ⭐ THE TABLE. Every over through a daemon of Nexus's own whose key had no answer is dropped, and
/// says so; through Hamlib it plays with its warning, as it always has. The operator's PTT holds
/// receive for every cause.
#[test]
fn an_over_whose_key_nexuss_own_daemon_had_no_answer_to_is_dropped() {
    let mut wrong = Vec::new();
    for kind in KINDS {
        for cause in CAUSES {
            let o = run(kind, cause);
            let dropped = cause != Cause::Hamlib;
            let plays = match kind {
                Kind::Ptt => false, // the operator's voice, at the radio's microphone
                _ => !dropped,
            };
            let ok = o.played == plays
                && line_is_right(kind, dropped && kind != Kind::Ptt, o.line.as_deref())
                && (kind != Kind::Ptt || !o.receive_shown);
            if !ok {
                wrong.push(format!("{kind:?} × {cause:?}: {o:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
