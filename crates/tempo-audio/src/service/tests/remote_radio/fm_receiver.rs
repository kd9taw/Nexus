//! FM receiver adjustments run through the actual owner and later CAT polls.
use super::fm::{station, writes};
use super::*;
use crate::rig::remote_tests::filtering_peer;
use tempo_app::engine::remote_radio::{AgcSpeed, RadioLevel, ReceiverDsp, ReceiverFunction};

#[derive(Clone, Copy, Debug)]
enum Adjustment {
    Level(RadioLevel),
    Filter,
    Function(ReceiverFunction),
    Agc,
}
impl Adjustment {
    fn command(self, mode: &str) -> String {
        match self {
            Self::Level(level) => format!(
                "L {} {}",
                level.token(),
                if level == RadioLevel::NotchFrequency {
                    "1500"
                } else {
                    "0.350"
                }
            ),
            Self::Filter => format!("M {mode} 2300"),
            Self::Function(func) => format!("U {} 1", func.token()),
            Self::Agc => "L AGC 3".into(),
        }
    }
    fn queue(self, s: &mut Station, mode: &str) -> Completion {
        let read = s.state.remote_read(&s.engine).unwrap();
        let mut e = engine_lock(&s.engine);
        e.remote_observe_mode(Some(&read), Some(mode));
        let connection = e
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        let permit = s
            .authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap();
        match self {
            Self::Level(level) => {
                let before = if level == RadioLevel::NotchFrequency {
                    600.0
                } else {
                    0.5
                };
                match level {
                    RadioLevel::Power => e.observe_rig_power(before),
                    RadioLevel::MicGain => e.observe_rig_mic_gain(before),
                    RadioLevel::NoiseReduction => e.observe_rig_nr_level(before),
                    RadioLevel::Compression => e.observe_rig_comp_level(before),
                    RadioLevel::NotchFrequency => e.observe_rig_notch_freq_hz(before),
                }
                e.queue_remote_level(
                    "phone",
                    level,
                    before,
                    if level == RadioLevel::NotchFrequency {
                        1500.0
                    } else {
                        0.35
                    },
                    connection,
                    permit,
                )
                .unwrap()
            }
            Self::Filter => {
                e.observe_rig_passband(Some(2400));
                e.queue_remote_filter_width("phone", 2400, 2300, connection, permit)
                    .unwrap()
            }
            Self::Function(func) => {
                e.observe_rig_funcs([Some(false); 6]);
                e.queue_remote_receiver_dsp(
                    "phone",
                    ReceiverDsp::Function { func, on: false },
                    ReceiverDsp::Function { func, on: true },
                    connection,
                    permit,
                )
                .unwrap()
            }
            Self::Agc => {
                e.set_agc("fast");
                e.agc_to_command();
                s.state.last_agc = Some("fast".into());
                e.observe_rig_agc("fast".into());
                e.queue_remote_receiver_dsp(
                    "phone",
                    ReceiverDsp::Agc(AgcSpeed::Fast),
                    ReceiverDsp::Agc(AgcSpeed::Slow),
                    connection,
                    permit,
                )
                .unwrap()
            }
        }
    }
}
fn receiver_peer(mode: &str, ignore: Option<String>) -> Peer {
    let readings = Mutex::new(std::collections::HashMap::<String, String>::new());
    filtering_peer(145_500_000, mode, 2400, move |line, _, _| {
        if ignore.as_deref() == Some(line) {
            return Some("RPRT 0\n".into());
        }
        let mut values = readings.lock().unwrap();
        if line.starts_with("l ") || line.starts_with("u ") {
            let default = match line {
                "l AGC" => "2",
                "l NOTCHF" => "600",
                "l RFPOWER" | "l MICGAIN" | "l NR" | "l COMP" => "0.5",
                _ => "0",
            };
            return Some(format!(
                "{}\n",
                values.get(line).map(String::as_str).unwrap_or(default)
            ));
        }
        if line.starts_with("L ") || line.starts_with("U ") {
            let mut words = line.split_whitespace();
            let verb = words.next().unwrap().to_lowercase();
            let token = words.next().unwrap();
            values.insert(format!("{verb} {token}"), words.next().unwrap().into());
            return Some("RPRT 0\n".into());
        }
        None
    })
}

#[test]
fn remote_fm_receiver_commands_preserve_tuning_and_do_not_replay_after_later_polls() {
    for mode in ["FM", "PKTFM"] {
        for adjustment in [
            Adjustment::Level(RadioLevel::Power),
            Adjustment::Level(RadioLevel::MicGain),
            Adjustment::Level(RadioLevel::NoiseReduction),
            Adjustment::Level(RadioLevel::Compression),
            Adjustment::Level(RadioLevel::NotchFrequency),
            Adjustment::Filter,
            Adjustment::Function(ReceiverFunction::Nb),
            Adjustment::Function(ReceiverFunction::Nr),
            Adjustment::Function(ReceiverFunction::Notch),
            Adjustment::Function(ReceiverFunction::ManualNotch),
            Adjustment::Agc,
        ] {
            for confirmed in [true, false] {
                let command = adjustment.command(mode);
                let peer = receiver_peer(mode, (!confirmed).then(|| command.clone()));
                let mut s = station(&peer, true, 0);
                s.state.last_rig_poll = 0.0;
                s.state.last_freq_poll = 0.0;
                let receipt = adjustment.queue(&mut s, mode);
                engine_lock(&s.engine).settings().save(&s.path).unwrap();
                let saved = std::fs::read(&s.path).unwrap();
                let fm = s.state.last_fm.clone();
                let canonical = s.state.last_mode.clone();
                s.step();
                assert_eq!(
                    matches!(
                        receipt.outcome(),
                        Outcome::Applied {
                            evidence: Evidence::RadioReadback
                        }
                    ),
                    confirmed,
                    "{mode} {adjustment:?}: {:?}",
                    receipt.outcome()
                );
                if !confirmed {
                    assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
                }
                assert_eq!(
                    writes(&peer),
                    vec![command.clone()],
                    "{mode} {adjustment:?}"
                );
                let reads = peer
                    .lines
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|s| *s == "f")
                    .count();
                for now in [200.0, 400.0, 800.0, 1000.0, 1600.0, 2400.0] {
                    s.state
                        .step(
                            &s.engine,
                            &mut s.backend,
                            &mut s.rig,
                            &no_sinks(),
                            now,
                            &mut mock_reopen_audio(),
                            &mut mock_reopen_rig(),
                            &mut StationSinks::new(),
                        )
                        .unwrap();
                }
                assert!(
                    peer.lines
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|s| *s == "f")
                        .count()
                        > reads
                );
                assert_eq!(
                    writes(&peer)
                        .into_iter()
                        .filter(|s| s != "T 0")
                        .collect::<Vec<_>>(),
                    vec![command],
                    "{mode} {adjustment:?} confirmed={confirmed}"
                );
                assert_eq!(s.state.last_fm, fm);
                assert_eq!(s.state.last_mode, canonical);
                assert_eq!(std::fs::read(&s.path).unwrap(), saved);
                assert!(!engine_lock(&s.engine).tx_enabled());
                assert!(s.backend.played.is_empty());
            }
        }
    }
}

#[test]
fn remote_fm_receiver_owner_refuses_unsettled_or_unknown_tuning_before_any_write() {
    for problem in ["unasserted", "unknown", "pending-fm"] {
        let peer = receiver_peer("FM", None);
        let mut s = station(&peer, true, 0);
        let receipt = Adjustment::Filter.queue(&mut s, "FM");
        match problem {
            "unasserted" => s.state.rig_asserted = false,
            "unknown" => s.state.remote_retune_uncertain = true,
            _ => s.state.last_fm = None,
        }
        s.state.apply_remote_radio(&s.engine, &mut s.rig, 0.0);
        assert_eq!(
            receipt.outcome(),
            Outcome::Rejected {
                reason: Reason::StationBusy
            }
        );
        assert!(writes(&peer).is_empty());
    }
}
