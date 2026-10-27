//! What the engine says of a remote over's microphone: why an over ended, and the over as the
//! stream shows it to the page and Stop's road carries it to a computer driving the shack. The
//! engine's microphone path, which decides both, is tempo-app's `mic`.

/// Why an over ended, as the operator is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicEnded {
    /// The operator let go.
    Released,
    /// A stop at the station, TX turned off, leaving Phone, a tune, or the licence.
    Stopped,
    /// No audio for the engine's `MicMode::gap_ms` (M2).
    AudioGap,
    /// The stream's transmit presence lapsed.
    Presence,
    /// The per-over ceiling (C1).
    Ceiling,
    /// The wall-clock TX watchdog (C2).
    Watchdog,
    /// The transmit route changed under the over (G7).
    RouteChanged,
}

/// The over as the stream shows it to the page. Published by the engine, read by the stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MicStatus {
    /// An over is armed (or keyed).
    pub armed: bool,
    /// The operator's audio has keyed the rig.
    pub keyed: bool,
    /// Display only: voice for two seconds, and the rig reports no power out.
    pub no_power_out: bool,
    /// Why the last over ended, until the next one is armed.
    pub ended: Option<MicEnded>,
    /// Display only: the holes on the air in the keyed over so far, each a silence between two
    /// stretches of its audio, where audio that never came in time was played as silence (M4).
    pub holes: u32,
    /// Display only: how long those holes lasted in all, in milliseconds.
    pub hole_ms: u32,
}
