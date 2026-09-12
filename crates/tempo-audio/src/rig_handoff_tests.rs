//! Complete incoming-radio configuration against the actual TCP CAT client.
use super::*;
use crate::rig::remote::{Handoff, RepeaterConfig};
use tempo_app::engine::remote_radio::{AgcSpeed, RadioLevel, ReceiverDsp};

const TOKENS: [&str; 6] = ["RFPOWER", "MICGAIN", "NR", "COMP", "NOTCHF", "AGC"];
struct Config {
    levels: [f32; 6],
    shift: String,
    offset: i64,
    tone: u32,
}
fn peer(
    intercept: impl Fn(&str, &mut RadioState, &mut Config) -> Option<String> + Send + 'static,
) -> Peer {
    let config = Mutex::new(Config {
        levels: [0.8, 0.5, 0.5, 0.5, 600.0, 3.0],
        shift: "None".into(),
        offset: 600_000,
        tone: 0,
    });
    retuning_peer(14_200_000, "USB", move |line, state| {
        let mut c = config.lock().unwrap();
        if let Some(reply) = intercept(line, state, &mut c) {
            return Some(reply);
        }
        if let Some(token) = line.strip_prefix("l ") {
            return TOKENS
                .iter()
                .position(|t| *t == token)
                .map(|i| format!("{}\n", c.levels[i]));
        }
        if let Some(set) = line.strip_prefix("L ") {
            let (token, value) = set.split_once(' ').unwrap();
            let i = TOKENS.iter().position(|t| *t == token).unwrap();
            c.levels[i] = value.parse().unwrap();
            return Some("RPRT 0\n".into());
        }
        match line {
            "r" => return Some(format!("{}\n", c.shift)),
            "o" => return Some(format!("{}\n", c.offset)),
            "c" => return Some(format!("{}\n", c.tone)),
            _ => (),
        }
        if let Some(v) = line.strip_prefix("R ") {
            c.shift = v.into();
        } else if let Some(v) = line.strip_prefix("O ") {
            c.offset = v.parse().unwrap();
        } else if let Some(v) = line.strip_prefix("C ") {
            c.tone = v.parse().unwrap();
        } else {
            return None;
        }
        Some("RPRT 0\n".into())
    })
}
fn levels() -> Vec<(RadioLevel, f32)> {
    vec![
        (RadioLevel::Power, 0.35),
        (RadioLevel::MicGain, 0.4),
        (RadioLevel::NoiseReduction, 0.3),
        (RadioLevel::Compression, 0.2),
        (RadioLevel::NotchFrequency, 1500.0),
    ]
}
fn configuration(mode: &str) -> Handoff {
    Handoff::new(
        Retune::new(
            Position::new(14_200_000, "USB").unwrap(),
            Position::new(146_520_000, mode).unwrap(),
        )
        .with_power_limit(0.5)
        .unwrap(),
        levels(),
        Some(AgcSpeed::Fast),
        matches!(mode, "FM" | "PKTFM").then(|| RepeaterConfig::new("plus", 0, 88.56).unwrap()),
    )
    .unwrap()
}
fn writes(peer: &Peer) -> Vec<String> {
    peer.lines
        .lock()
        .unwrap()
        .iter()
        .filter(|line| {
            matches!(
                line.as_bytes().first(),
                Some(b'M' | b'F' | b'L' | b'R' | b'O' | b'C' | b'T')
            )
        })
        .cloned()
        .collect()
}

#[test]
fn remote_handoff_configures_all_requested_settings_on_the_incoming_connection() {
    for mode in ["USB", "FM", "PKTFM"] {
        let peer = peer(|_, _, _| None);
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
        let result = rig
            .remote_handoff(configuration(mode), &permission)
            .unwrap();
        assert_eq!(
            result.radio().position(),
            &Position::new(146_520_000, mode).unwrap()
        );
        assert_eq!(result.levels(), levels());
        assert_eq!(result.radio().power(), Some(0.35));
        assert_eq!(
            result.radio().receiver_dsp(),
            Some(ReceiverDsp::Agc(AgcSpeed::Fast))
        );
        assert!(
            matches!(completion.outcome(), Outcome::Pending),
            "native owner has not committed"
        );
        let commands = writes(&peer);
        for command in [
            "L RFPOWER 0.500",
            "L RFPOWER 0.350",
            "L MICGAIN 0.400",
            "L NR 0.300",
            "L COMP 0.200",
            "L NOTCHF 1500",
            "L AGC 2",
            "F 146520000",
        ] {
            assert_eq!(
                commands.iter().filter(|s| s.as_str() == command).count(),
                1,
                "{command}"
            );
        }
        assert!(!commands.iter().any(|s| s.starts_with("T ")));
        if mode == "USB" {
            assert!(result.radio().repeater().is_none());
            assert!(!commands
                .iter()
                .any(|s| s.starts_with("R ") || s.starts_with("O ") || s.starts_with("C ")));
        } else {
            let fm = result.radio().repeater().unwrap();
            assert_eq!(fm.shift(), "plus");
            assert_eq!(
                fm.offset_hz(),
                600_000,
                "zero request retains actual offset"
            );
            assert_eq!(fm.tone_hz(), 88.6);
            assert!(commands.contains(&"R +".into()) && commands.contains(&"C 886".into()));
            let tone = commands.iter().position(|s| s == "C 886").unwrap();
            let power = commands
                .iter()
                .position(|s| s == "L RFPOWER 0.350")
                .unwrap();
            assert!(
                tone < power,
                "FM then desired levels follows the native loop"
            );
            assert!(!commands.iter().any(|s| s.starts_with("O ")));
        }
    }
}

#[test]
fn remote_handoff_unset_controls_are_never_copied_from_observations() {
    let peer = peer(|line, _, _| line.starts_with("l ").then(|| "RPRT -11\n".into()));
    let mut rig = Rig::rigctld(&peer.address);
    let (permission, _) = permission(&Revocation::default(), &Revocation::default());
    let request = Handoff::new(
        Retune::new(
            Position::new(14_200_000, "USB").unwrap(),
            Position::new(14_250_000, "USB").unwrap(),
        ),
        vec![],
        None,
        None,
    )
    .unwrap();
    let result = rig.remote_handoff(request, &permission).unwrap();
    assert!(result.levels().is_empty());
    assert!(result.radio().power().is_none());
    assert!(result.radio().receiver_dsp().is_none());
    assert!(!peer
        .lines
        .lock()
        .unwrap()
        .iter()
        .any(|s| s.starts_with("l ") || s.starts_with("L ")));
    assert!(writes(&peer).contains(&"F 14250000".into()));
}

#[test]
fn remote_handoff_preflight_refuses_unreadable_settings_before_any_write() {
    for missing in ["l NR", "l AGC", "c", "t", "s"] {
        let peer = peer(move |line, _, _| (line == missing).then(|| "RPRT -11\n".into()));
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
        assert!(rig
            .remote_handoff(configuration("FM"), &permission)
            .is_err());
        assert!(writes(&peer).is_empty(), "{missing}");
        assert!(!matches!(completion.outcome(), Outcome::Applied { .. }));
    }
}

#[test]
fn remote_handoff_rechecks_earlier_settings_after_later_commands() {
    for failure in [
        "level drift",
        "agc drift",
        "power drift",
        "fm drift",
        "dial drift",
        "keyed",
    ] {
        let peer = peer(move |line, state, c| {
            if line == "L AGC 2" {
                match failure {
                    "level drift" => c.levels[1] = 0.8,
                    "agc drift" => return Some("RPRT 0\n".into()),
                    "power drift" => c.levels[0] = 0.9,
                    "fm drift" => c.shift = "-".into(),
                    "dial drift" => state.dial += 100,
                    "keyed" => state.keyed = true,
                    _ => unreachable!(),
                }
            }
            None
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
        assert!(
            rig.remote_handoff(configuration("FM"), &permission)
                .is_err(),
            "{failure}"
        );
        assert!(
            matches!(completion.outcome(), Outcome::Unknown { .. }),
            "{failure}"
        );
        assert!(!writes(&peer).iter().any(|s| s.starts_with("T ")));
    }
}

#[test]
fn remote_handoff_revocation_stops_later_configuration_without_replay() {
    for stop_after in [
        "L RFPOWER 0.350",
        "L MICGAIN 0.400",
        "L NR 0.300",
        "L AGC 2",
        "R +",
    ] {
        let authority = Arc::new(Revocation::default());
        let revoke = authority.clone();
        let peer = peer(move |line, _, _| {
            if line == stop_after {
                revoke.revoke();
            }
            None
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&authority, &Revocation::default());
        assert!(rig
            .remote_handoff(configuration("FM"), &permission)
            .is_err());
        let commands = writes(&peer);
        assert_eq!(commands.last().map(String::as_str), Some(stop_after));
        assert!(matches!(completion.outcome(), Outcome::Unknown { .. }));
        // Reusing expired authority cannot restart the recipe or a later field.
        assert!(rig
            .remote_handoff(configuration("FM"), &permission)
            .is_err());
        assert_eq!(writes(&peer), commands);
    }
}

#[test]
fn remote_handoff_rejects_duplicate_invalid_and_incomplete_configuration() {
    let retune = || {
        Retune::new(
            Position::new(14_200_000, "USB").unwrap(),
            Position::new(146_520_000, "FM").unwrap(),
        )
        .with_power_limit(0.5)
        .unwrap()
    };
    for levels in [
        vec![(RadioLevel::Power, 0.6)],
        vec![(RadioLevel::MicGain, f32::NAN)],
        vec![(RadioLevel::MicGain, 0.2); 2],
    ] {
        assert!(Handoff::new(
            retune(),
            levels,
            None,
            Some(RepeaterConfig::new("simplex", 0, 0.0).unwrap())
        )
        .is_err());
    }
    assert!(Handoff::new(retune(), vec![], None, None).is_err());
}

#[test]
fn remote_handoff_checks_both_owners_at_the_socket_and_accepts_fresh_authority() {
    for local_takeover in [false, true] {
        let peer = peer(|_, _, _| None);
        let authority = Arc::new(Revocation::default());
        let native = Arc::new(Revocation::default());
        let (old, completion) = permission(&authority, &native);
        let revoke = if local_takeover {
            native.clone()
        } else {
            authority.clone()
        };
        let mut rig = Rig::rigctld(&peer.address);
        rig.before_remote_write = Some(Box::new(move || revoke.revoke()));
        assert!(rig.remote_handoff(configuration("FM"), &old).is_err());
        assert!(writes(&peer).is_empty());
        assert!(!matches!(completion.outcome(), Outcome::Applied { .. }));
        let (fresh, _) = permission(&authority, &native);
        let result = rig.remote_handoff(configuration("FM"), &fresh).unwrap();
        assert_eq!(result.levels(), levels());
        assert!(writes(&peer).contains(&"F 146520000".into()));
    }
}
