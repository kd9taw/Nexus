//! Test CAT for a radio that is NOT the one in use, from the radio setup: a READ-ONLY look at what
//! the radio monitor's own connection reads from that radio.
//!
//! ⛔ IT NEVER SELECTS, SWITCHES OR KEYS A RADIO, AND IT SENDS THE RADIO NOTHING. Every enabled
//! radio that is not in use already has its own read-only CAT connection in the monitor pool
//! (`tempo-audio`'s `monitor_loop`: it reads the dial and, now and then, the mode and S-meter, and
//! never sets a frequency, a mode or PTT). This probe adds no command to that connection. It notes
//! how many reports the monitor has made for the radio, waits for the next one, and answers with
//! it, so the answer is a reading made after the press. It takes the engine only through `&self`
//! methods, so it cannot change the active radio, the settings or the transmit state.
//!
//! `test_cat` stays the probe for the radio in use: it asks the radio loop to re-open and re-probe
//! that radio's live connection, which is right for the radio being operated and wrong for any
//! other (making another radio active to test it would be a radio switch, with all of a switch's
//! consequences). So the radio in use is answered `InUse` here and the setup calls `test_cat`.
use super::{engine_lock, Engine};
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What the read-only probe found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RadioProbeState {
    /// A fresh reading came back: the radio answered (with its dial).
    Answered,
    /// A fresh report came back: the monitor could not read the radio.
    NotAnswering,
    /// No new report in the time allowed; `ok`/`dial_mhz` are the last ones known, if any.
    NoFreshReport,
    /// The radio in use: Test CAT (`test_cat`) is its probe.
    InUse,
    /// Turned off in Nexus, so nothing holds a connection to it.
    Disabled,
    /// No CAT model set (VOX only): there is nothing to read.
    NoCat,
    /// It has CAT, but the monitor does not watch it while another radio is in use (an OmniRig
    /// radio: its model lives in OmniRig), so there is no reading to give.
    NotMonitored,
    /// No radio with that id.
    Unknown,
}

/// The probe's answer for one radio.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioCatProbe {
    pub id: u32,
    pub state: RadioProbeState,
    /// The monitor's CAT health for the radio, as last reported.
    pub ok: Option<bool>,
    /// The dial it read, MHz.
    pub dial_mhz: Option<f64>,
}

impl Engine {
    /// How many CAT reports the monitor has made for radio `id` (0 = none yet).
    pub fn monitor_reads(&self, id: u32) -> u64 {
        self.monitor_reads.get(&id).copied().unwrap_or(0)
    }

    /// The answer that needs no waiting: the radio in use, a radio turned off, one with no CAT, one
    /// the monitor does not watch, or no such radio. `None` = a monitored radio, whose answer is the
    /// monitor's next report.
    fn radio_probe_without_reading(&self, id: u32) -> Option<RadioCatProbe> {
        let state = match self.settings.radios.iter().find(|p| p.id == id) {
            None => RadioProbeState::Unknown,
            Some(_) if id == self.settings.active_radio => RadioProbeState::InUse,
            Some(p) if !p.enabled => RadioProbeState::Disabled,
            // The monitor's own predicate (`monitor_want`): a radio it does not open has no reading.
            Some(p) if !p.monitored_while(self.settings.active_radio) => {
                if crate::settings::rig_conn_is_omnirig(&p.rig_conn) {
                    RadioProbeState::NotMonitored
                } else {
                    RadioProbeState::NoCat
                }
            }
            Some(_) => return None,
        };
        Some(RadioCatProbe {
            id,
            state,
            ok: None,
            dial_mhz: None,
        })
    }

    /// The monitor's latest report for radio `id`, as the probe's answer.
    fn radio_probe_report(&self, id: u32, fresh: bool) -> RadioCatProbe {
        let live = self.radio_live.get(&id);
        let ok = live.and_then(|l| l.cat_ok);
        let state = match (fresh, ok) {
            (true, Some(true)) => RadioProbeState::Answered,
            (true, _) => RadioProbeState::NotAnswering,
            (false, _) => RadioProbeState::NoFreshReport,
        };
        RadioCatProbe {
            id,
            state,
            ok,
            dial_mhz: live.and_then(|l| l.dial_mhz),
        }
    }
}

/// Probe radio `id` read-only: wait up to `wait` (checking every `step`) for the monitor's next
/// report on it, and answer with that report. Takes the engine lock briefly per check, never
/// across a sleep.
pub fn probe_radio_read_only(
    engine: &Mutex<Engine>,
    id: u32,
    wait: Duration,
    step: Duration,
) -> RadioCatProbe {
    let mark = {
        let e = engine_lock(engine);
        if let Some(answer) = e.radio_probe_without_reading(id) {
            return answer;
        }
        e.monitor_reads(id)
    };
    let deadline = Instant::now() + wait;
    loop {
        std::thread::sleep(step);
        let e = engine_lock(engine);
        if e.monitor_reads(id) > mark {
            return e.radio_probe_report(id, true);
        }
        if Instant::now() >= deadline {
            return e.radio_probe_report(id, false);
        }
    }
}
