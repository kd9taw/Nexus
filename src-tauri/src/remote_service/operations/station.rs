//! The station's radio context, as a browser's state carries it under `controls.context`. Every
//! hosted page still reads it there, though the lane no longer takes a station action.

use serde::Serialize;
use tempo_app::engine::Engine;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    radio_id: u32,
    radio_connection: Option<u64>,
    amp_connection: Option<u64>,
    amp_read_sequence: Option<u64>,
}

impl Context {
    pub fn capture(engine: &Engine) -> Self {
        let observation = engine.remote_monitor_observation();
        let amp = observation.amplifier.as_ref().and_then(|a| a.reading);
        Self {
            radio_id: observation.radio.id,
            radio_connection: observation
                .radio
                .readings
                .cat
                .map(|r| r.connection_generation),
            amp_connection: amp.map(|r| r.connection_generation),
            amp_read_sequence: amp.map(|r| r.read_sequence),
        }
    }
}
