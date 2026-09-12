use super::*;
use crate::{
    dto::AmpStatusDto, remote_control::Revocation, remote_monitor::provenance::Connection,
    settings::Settings,
};
use std::time::Duration;

struct Station {
    engine: Engine,
    radio: Connection,
    amp: Connection,
    authority: Revocation,
}
impl Station {
    fn new(family: &str) -> Self {
        let mut settings = Settings {
            amp_model: family.into(),
            amp_port: "remote-test-amp".into(),
            ..Default::default()
        };
        settings.ensure_radio_profiles();
        let mut engine = Engine::with_settings(settings);
        engine.set_tx_enabled(false);
        let radio = engine.remote_open_radio().unwrap();
        let amp = engine.remote_open_amp().unwrap();
        let mut s = Self {
            engine,
            radio,
            amp,
            authority: Revocation::default(),
        };
        s.sample(false, false, Some(false), Instant::now());
        s
    }
    fn sample(
        &mut self,
        operate: bool,
        amp_keyed: bool,
        radio_keyed: Option<bool>,
        radio_at: Instant,
    ) {
        let r = self
            .engine
            .remote_radio_read(&self.radio, radio_at)
            .unwrap();
        self.engine.remote_observe_cat(Some(&r), Some(true));
        self.engine.remote_observe_ptt(Some(&r), radio_keyed);
        let a = self
            .engine
            .remote_amp_read(&self.amp, Instant::now())
            .unwrap();
        let family = self
            .engine
            .settings()
            .active_profile()
            .unwrap()
            .amp_model
            .clone();
        self.engine.remote_observe_amp(
            Some(&a),
            AmpStatusDto {
                family: family.clone(),
                linked: true,
                operate: Some(operate),
                transmitting: (family == "spe").then_some(amp_keyed),
                output_watts: Some(0),
                band_label: Some("20m".into()),
                ..Default::default()
            },
        );
    }
    fn request(&self) -> Result<Request, Reason> {
        let o = self.engine.remote_monitor_observation();
        let a = o.amplifier.unwrap().reading.unwrap();
        Request::new(
            &self.engine,
            Target::Operate {
                expected: false,
                desired: true,
            },
            o.radio.readings.cat.unwrap().connection_generation,
            a.connection_generation,
            a.read_sequence,
            self.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
    }
}

#[test]
fn only_a_later_same_connection_readback_confirms_a_single_write() {
    for family in ["spe", "kpa"] {
        let mut s = Station::new(family);
        let mut request = s.request().unwrap();
        assert_eq!(request.begin(&s.engine), Err(Reason::ReadingUnavailable));
        s.sample(false, false, Some(false), Instant::now());
        request.begin(&s.engine).unwrap();
        assert!(request.begin_write());
        assert!(
            !request.begin_write(),
            "a worker cannot write a toggle twice"
        );
        assert_eq!(request.completion.outcome(), Outcome::Pending);
        s.sample(true, false, Some(false), Instant::now());
        request.confirm(&s.engine);
        assert_eq!(
            request.completion.outcome(),
            Outcome::Applied {
                evidence: Evidence::AmplifierReadback
            }
        );
    }
}

#[test]
fn missing_ptt_on_kpa_and_old_idle_samples_never_authorize_a_write() {
    let mut s = Station::new("kpa");
    s.sample(false, false, None, Instant::now());
    assert!(matches!(s.request(), Err(Reason::ReadingUnavailable)));
    s.sample(
        false,
        false,
        Some(false),
        Instant::now() - Duration::from_millis(1100),
    );
    assert!(matches!(s.request(), Err(Reason::ReadingUnavailable)));
    s.sample(false, false, Some(true), Instant::now());
    assert!(matches!(s.request(), Err(Reason::StationBusy)));
    s.sample(false, false, Some(false), Instant::now());
    assert!(
        s.request().is_ok(),
        "positive control: a fresh KPA/radio pair is supported"
    );
}

#[test]
fn a_changed_front_panel_or_a_new_transmission_after_poll_refuses_the_gesture() {
    let mut s = Station::new("spe");
    let mut request = s.request().unwrap();
    s.sample(true, false, Some(false), Instant::now());
    assert_eq!(request.begin(&s.engine), Err(Reason::ContextChanged));
    s.sample(false, true, Some(false), Instant::now());
    assert_eq!(request.begin(&s.engine), Err(Reason::StationBusy));
    s.sample(false, false, Some(false), Instant::now());
    request.begin(&s.engine).unwrap();
    s.engine.set_tx_enabled(true);
    s.engine.set_tx_enabled(false);
    assert!(
        !request.begin_write(),
        "native context changes reach the worker after it drops the engine lock"
    );
    request.refuse(Reason::ContextChanged);
    assert_eq!(
        request.completion.outcome(),
        Outcome::Rejected {
            reason: Reason::ContextChanged
        }
    );
}

#[test]
fn revocation_and_reconnect_cannot_replay_or_promote_a_sent_toggle() {
    let mut s = Station::new("spe");
    let mut request = s.request().unwrap();
    s.sample(false, false, Some(false), Instant::now());
    request.begin(&s.engine).unwrap();
    assert!(request.begin_write());
    s.authority.revoke();
    s.sample(true, false, Some(false), Instant::now());
    request.confirm(&s.engine);
    assert_eq!(
        request.completion.outcome(),
        Outcome::Unknown {
            reason: Reason::HardwareUnconfirmed
        }
    );
    let mut s = Station::new("spe");
    let mut request = s.request().unwrap();
    s.sample(false, false, Some(false), Instant::now());
    request.begin(&s.engine).unwrap();
    assert!(request.begin_write());
    s.amp = s.engine.remote_open_amp().unwrap();
    s.sample(true, false, Some(false), Instant::now());
    request.confirm(&s.engine);
    assert_eq!(
        request.completion.outcome(),
        Outcome::Unknown {
            reason: Reason::HardwareUnconfirmed
        }
    );
}

#[test]
fn local_gestures_and_link_replacement_after_poll_cancel_the_unsent_command() {
    for link in ["radio", "amplifier", "closed-radio", "local-amplifier"] {
        let mut s = Station::new("spe");
        let mut request = s.request().unwrap();
        s.sample(false, false, Some(false), Instant::now());
        request.begin(&s.engine).unwrap();
        match link {
            "radio" => {
                s.engine.remote_open_radio();
            }
            "amplifier" => {
                s.engine.remote_open_amp();
            }
            "local-amplifier" => s.engine.note_local_amplifier_command(),
            _ => s.engine.remote_close_radio(),
        }
        assert!(
            !request.begin_write(),
            "{link} changed after unlocking the engine"
        );
        request.refuse(Reason::ContextChanged);
        assert!(matches!(
            request.completion.outcome(),
            Outcome::Rejected { .. }
        ));
    }
}

#[test]
fn a_matching_pre_write_sample_is_not_a_hardware_receipt() {
    let mut s = Station::new("spe");
    let mut request = s.request().unwrap();
    s.sample(false, false, Some(false), Instant::now());
    request.begin(&s.engine).unwrap();
    assert!(request.begin_write());
    request.confirm(&s.engine);
    assert_eq!(
        request.completion.outcome(),
        Outcome::Unknown {
            reason: Reason::HardwareUnconfirmed
        }
    );
    s.sample(true, false, Some(false), Instant::now());
    request.confirm(&s.engine);
    assert_eq!(
        request.completion.outcome(),
        Outcome::Unknown {
            reason: Reason::HardwareUnconfirmed
        }
    );
}
