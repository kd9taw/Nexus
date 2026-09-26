//! Closed station actions beneath the existing native authority. Receiver
//! changes use the same Engine verbs as the local cockpit. Radio/amplifier
//! work additionally needs the owning hardware worker's completion receipt.

use serde::{Deserialize, Serialize};
use tempo_app::{
    engine::Engine,
    remote_control::{Completion, Evidence, Outcome, Permit, Reason},
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

    fn matches_radio(&self, engine: &Engine) -> bool {
        self.radio_id == engine.settings().active_radio
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Receiver {
    Rtty,
    Psk,
    Sstv,
    Aprs,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TextReceiver {
    Cw,
    Rtty,
    Psk,
}

/// The section a recalled memory lands in.
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecallSection {
    Cw,
    Phone,
    Digital,
}

/// A Phone memory's own sideband.
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RecallSideband {
    Usb,
    Lsb,
}

/// An FM memory's machine: shift, offset (0 = band convention) and tone.
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecallFm {
    shift: RepeaterShift,
    offset_hz: u32,
    tone_hz: f32,
}

/// A repeater's shift: a closed set, so an unknown word never degrades to simplex.
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeaterShift {
    Simplex,
    Plus,
    Minus,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyboardReceiver {
    Rtty,
    Psk,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum Vfo {
    A,
    B,
}

/// A split request always names both values; an absent one is invalid, never simplex by omission.
fn present<'de, D: serde::Deserializer<'de>>(value: D) -> Result<Option<f64>, D::Error> {
    Option::<f64>::deserialize(value)
}

/// Which native panadapter setting a `radio.scope` request carries.
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ScopeSetting {
    Span,
    Ref,
    Position,
    PanSpan,
    PanRef,
}

/// The FT-710 scope position, by name; the station resolves the rig's display family.
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScopePlace {
    Center,
    Cursor,
    Fix,
}

/// A named `refDbm: null` (auto) stays distinct from an absent field.
fn named<'de, D: serde::Deserializer<'de>>(value: D) -> Result<Option<Option<i32>>, D::Error> {
    Option::<i32>::deserialize(value).map(Some)
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "action", deny_unknown_fields)]
pub enum Action {
    #[serde(rename = "ft.runtime")]
    FtRuntime {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        expected: tempo_app::engine::remote_transmit::runtime::FtRuntimeContext,
        change: tempo_app::engine::remote_transmit::runtime::FtRuntimeChange,
    },
    #[serde(rename = "ft.setting")]
    FtSetting {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        expected: tempo_app::engine::remote_transmit::settings::FtSettingsContext,
        change: tempo_app::engine::remote_transmit::settings::FtSettingChange,
    },
    #[serde(rename = "qso.logCurrent")]
    QsoLogCurrent {
        #[serde(rename = "expectedKey")]
        expected_key: String,
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "expectedQso")]
        expected_qso: tempo_app::engine::remote_transmit::FtExchangeContext,
    },
    #[serde(rename = "qso.confirm")]
    QsoConfirm {
        #[serde(rename = "expectedKey")]
        expected_key: String,
        edits: tempo_app::engine::remote_logging::PendingLogEdits,
    },
    #[serde(rename = "qso.discard")]
    QsoDiscard {
        #[serde(rename = "expectedKey")]
        expected_key: String,
    },
    #[serde(rename = "ft.message")]
    FtMessage {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        #[serde(rename = "expectedQso")]
        expected_qso: tempo_app::engine::remote_transmit::FtExchangeContext,
        call: String,
        grid: Option<String>,
        text: String,
    },
    #[serde(rename = "ft.exchange")]
    FtExchange {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        #[serde(rename = "expectedQso")]
        expected_qso: tempo_app::engine::remote_transmit::FtExchangeContext,
        change: tempo_app::engine::remote_transmit::FtExchangeChange,
    },
    #[serde(rename = "ft.cq")]
    FtCq {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        direction: Option<String>,
    },
    #[serde(rename = "ft.call")]
    FtCall {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        selection: tempo_app::engine::remote_transmit::FtCallSelection,
    },
    #[serde(rename = "ft.txEnabled")]
    FtTxEnabled {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        on: bool,
    },
    #[serde(rename = "radio.level")]
    Level {
        mode: String,
        level: String,
        expected: f32,
        value: f32,
    },
    /// A level on a dual-receiver radio's SECOND receiver — `rfGain`, `afGain` or `squelch` as a
    /// 0..1 fraction. A receive-side one-shot, the rig-scope settings' shape: no expected value
    /// (the Sub is not read back, so there is nothing to compare against) and no mode.
    #[serde(rename = "radio.subLevel")]
    SubLevel { level: String, value: f32 },
    #[serde(rename = "decoder.arm")]
    ReceiverArm { receiver: Receiver, on: bool },
    #[serde(rename = "decoder.clear")]
    ReceiverClear { receiver: TextReceiver },
    #[serde(rename = "decoder.afcReset")]
    ReceiverAfcReset { receiver: KeyboardReceiver },
    #[serde(rename = "decoder.net")]
    ReceiverNet { receiver: KeyboardReceiver, hz: f32 },
    #[serde(rename = "decoder.pskMode")]
    PskMode { mode: String, reverse: bool },
    #[serde(rename = "decoder.aiCw")]
    AiCw {
        #[serde(rename = "expectedOn")]
        expected_on: bool,
        on: bool,
    },
    #[serde(rename = "decoder.redecode")]
    Redecode {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
    },
    #[serde(rename = "radio.split")]
    Split {
        #[serde(rename = "expectedTxMhz", deserialize_with = "present")]
        expected_tx_mhz: Option<f64>,
        #[serde(rename = "txMhz", deserialize_with = "present")]
        tx_mhz: Option<f64>,
    },
    #[serde(rename = "radio.rit")]
    Rit {
        #[serde(rename = "expectedHz")]
        expected_hz: i32,
        hz: i32,
    },
    #[serde(rename = "radio.xit")]
    Xit {
        #[serde(rename = "expectedHz")]
        expected_hz: i32,
        hz: i32,
    },
    #[serde(rename = "radio.vfo")]
    Vfo {
        #[serde(rename = "expectedVfo")]
        expected_vfo: Vfo,
        vfo: Vfo,
    },
    #[serde(rename = "decoder.js8Speed")]
    Js8Speed {
        #[serde(rename = "expectedSpeed")]
        expected_speed: u8,
        speed: u8,
    },
    #[serde(rename = "decoder.msk144Period")]
    Msk144Period {
        #[serde(rename = "expectedPeriodSecs")]
        expected_period_secs: u16,
        #[serde(rename = "periodSecs")]
        period_secs: u16,
    },
    #[serde(rename = "decoder.depth")]
    DecodeDepth {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "expectedDepth")]
        expected_depth: u8,
        depth: u8,
    },
    #[serde(rename = "receiver.rxOffset")]
    RxOffset {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "expectedHz")]
        expected_hz: f32,
        hz: f32,
    },
    #[serde(rename = "receiver.rxGain")]
    RxGain {
        #[serde(rename = "radioId")]
        radio_id: u32,
        #[serde(rename = "expectedSettingsRevision")]
        expected_settings_revision: String,
        #[serde(rename = "expectedGain")]
        expected_gain: f32,
        gain: f32,
    },
    #[serde(rename = "radio.disarm")]
    Disarm {},
    #[serde(rename = "radio.frequency")]
    Frequency {
        #[serde(rename = "dialMhz")]
        dial_mhz: f64,
        band: String,
        sideband: String,
    },
    #[serde(rename = "radio.band")]
    Band { band: String, mode: String },
    #[serde(rename = "radio.filterWidth")]
    FilterWidth {
        mode: String,
        #[serde(rename = "expectedHz")]
        expected_hz: u32,
        hz: u32,
    },
    #[serde(rename = "radio.function")]
    ReceiverFunction {
        mode: String,
        func: String,
        #[serde(rename = "expectedOn")]
        expected_on: bool,
        on: bool,
    },
    #[serde(rename = "radio.agc")]
    Agc {
        mode: String,
        #[serde(rename = "expectedSpeed")]
        expected_speed: String,
        speed: String,
    },
    #[serde(rename = "radio.phoneMode")]
    PhoneMode {
        #[serde(rename = "expectedMode")]
        expected_mode: String,
        mode: String,
    },
    #[serde(rename = "radio.mode")]
    Mode {
        mode: String,
        #[serde(rename = "followFrequency")]
        follow_frequency: bool,
    },
    #[serde(rename = "radio.workSpot")]
    WorkSpot {
        mode: String,
        #[serde(rename = "dialMhz")]
        dial_mhz: f64,
        band: String,
        call: String,
    },
    /// RTTY Work names itself in its own action for the same reason `workDigitalSpot` does: an
    /// older desktop parses `workSpot`'s `mode` against `cw`/`phone` exactly and would refuse a
    /// third word, so a browser that learned RTTY must not send it as a `workSpot` variant.
    #[serde(rename = "radio.workRttySpot")]
    WorkRttySpot {
        #[serde(rename = "dialMhz")]
        dial_mhz: f64,
        band: String,
        call: String,
    },
    /// FT8/FT4 Work names its tier in its own action: an older desktop parses workSpot exactly.
    #[serde(rename = "radio.workDigitalSpot")]
    WorkDigitalSpot {
        tier: tempo_app::dto::Tier,
        #[serde(rename = "dialMhz")]
        dial_mhz: f64,
        band: String,
        call: String,
    },
    /// An FM repeater (the Program Tune): the machine only. The offset is the magnitude the rig
    /// keys, 0 = the band convention; the station judges the input it keys against the licence.
    #[serde(rename = "radio.repeater")]
    Repeater {
        #[serde(rename = "outputMhz")]
        output_mhz: f64,
        shift: RepeaterShift,
        #[serde(rename = "offsetHz")]
        offset_hz: u32,
        #[serde(rename = "toneHz")]
        tone_hz: f32,
    },
    /// An APRS tune (the APRS cockpit's channel pick): one of the regional 2 m APRS channels only.
    #[serde(rename = "radio.aprsTune")]
    AprsTune {
        #[serde(rename = "dialMhz")]
        dial_mhz: f64,
    },
    /// Point the rotator at an absolute azimuth: an operator gesture only.
    #[serde(rename = "rotator.point")]
    RotatorPoint {
        #[serde(rename = "azimuthDeg")]
        azimuth_deg: f64,
    },
    /// Point the rotator at a callsign's entity, bearing resolved at the station.
    #[serde(rename = "rotator.pointAtCall")]
    RotatorPointAtCall { call: String },
    /// Stop the rotator.
    #[serde(rename = "rotator.stop")]
    RotatorStop {},
    /// ⭐ ARM AUTO-TRACK for one pass of one bird. The gesture that makes the station steer the
    /// dial, the split and the mast on its own for the length of a pass, so it is an operator
    /// gesture and nothing else: no timer, no alarm and no spot reaches it, exactly as on the
    /// desktop. `aos_unix` names WHICH pass — the schedule row the operator clicked.
    ///
    /// The track it arms does not outlive the browser that armed it: the loop carries this
    /// gesture's authority and ends the pass, dial handback and all, the moment that authority
    /// stops being held (see `arm_sat_track`).
    #[serde(rename = "satellite.track")]
    SatTrack {
        name: String,
        #[serde(rename = "aosUnix")]
        aos_unix: i64,
    },
    /// Disarm auto-track: the rail's Stop. Never refused for being busy — a stop is always the
    /// safe direction.
    #[serde(rename = "satellite.stopTrack")]
    SatStopTrack {},
    /// Hand a transponder to the Doppler engine (`index`), or hand the dial back (`null`).
    /// `index` is the raw row into the list the `satellite` detail page returned, which is the
    /// list the station indexes. `auto` marks a pick made by the "Work this pass" chain rather
    /// than by a click on a card; the station refuses an auto re-pick against a pinned row.
    #[serde(rename = "satellite.transponder")]
    SatTransponder {
        name: String,
        index: Option<usize>,
        auto: bool,
    },
    /// The readiness rail's Doppler fix: turn the `satDopplerOff` switch on or off in place.
    #[serde(rename = "satellite.doppler")]
    SatDoppler { on: bool },
    /// The uplink VFO mapping and, in the same act, the operator's confirmation that it is the
    /// mapping for the radio Doppler is driving. `map` absent = confirm the mapping ALREADY IN
    /// FORCE, resolved at the station at write time. `radio_id` is the rig the browser's rail
    /// NAMED, so a radio switch between the poll and the click can never grant a rig the operator
    /// never saw named.
    #[serde(rename = "satellite.uplinkMap")]
    SatUplinkMap {
        map: Option<tempo_app::settings::SatVfoMap>,
        #[serde(rename = "radioId")]
        radio_id: Option<u32>,
    },
    /// Peg-lock the active radio from the satellite radio-binding line — the app-wide
    /// "don't auto-switch radios" override, the same switch as the TopBar's 🔒.
    #[serde(rename = "satellite.peg")]
    SatPeg { on: bool },
    /// The manual element refresh ("update elements"): one attempt, now. Every policy gate the
    /// desktop button obeys still applies at the station.
    #[serde(rename = "satellite.elements")]
    SatElements {},
    /// ⛔ Delete one received SSTV picture, permanently. The browser names the picture by what its
    /// own gallery row showed — the finish time and the mode — never by a path: the station finds
    /// the row itself, and refuses anything but exactly one match.
    #[serde(rename = "sstv.deleteImage")]
    SstvDeleteImage {
        #[serde(rename = "finishedUtc")]
        finished_utc: String,
        mode: String,
    },
    /// The native panadapter's span, reference or position: one setting and exactly its own field.
    /// The station decides which scope family is live; a browser's view of the feed is never used.
    #[serde(rename = "radio.scope")]
    Scope {
        setting: ScopeSetting,
        #[serde(default)]
        hz: Option<u32>,
        #[serde(default, rename = "tenthsDb")]
        tenths_db: Option<i32>,
        #[serde(default)]
        position: Option<ScopePlace>,
        #[serde(default, rename = "refDbm", deserialize_with = "named")]
        ref_dbm: Option<Option<i32>>,
    },
    /// A memory recall: the section, the memory's exact dial, its own sideband (Phone only) or
    /// its FM machine. Never a Settings form, a call or a tier.
    #[serde(rename = "radio.memoryRecall")]
    MemoryRecall {
        section: RecallSection,
        #[serde(rename = "dialMhz")]
        dial_mhz: f64,
        band: String,
        sideband: Option<RecallSideband>,
        fm: Option<RecallFm>,
    },
    #[serde(rename = "radio.tier")]
    Tier { tier: tempo_app::dto::Tier },
    #[serde(rename = "radio.workspace")]
    Workspace {
        workspace: tempo_app::engine::remote_radio::Workspace,
    },
    #[serde(rename = "radio.select")]
    Radio {
        #[serde(rename = "radioId")]
        radio_id: u32,
    },
    #[serde(rename = "amplifier.operate")]
    AmpOperate {
        #[serde(rename = "expectedOperate")]
        expected_operate: bool,
        operate: bool,
    },
    #[serde(rename = "amplifier.band")]
    AmpBand {
        #[serde(rename = "expectedBand")]
        expected_band: String,
        direction: i8,
    },
    #[serde(rename = "amplifier.followBand")]
    AmpFollowBand {
        #[serde(rename = "radioId")]
        radio_id: u32,
        #[serde(rename = "expectedSettingsRevision")]
        expected_settings_revision: String,
        #[serde(rename = "expectedFollow")]
        expected_follow: bool,
        follow: bool,
    },
}

#[cfg(feature = "radio")]
#[path = "rotator.rs"]
pub(super) mod rotator;

#[cfg(feature = "radio")]
#[path = "satellite.rs"]
pub(super) mod satellite;

/// Whether the desktop's satellite loop is steering the mast right now. An unreadable marker
/// answers yes, so a remote point never fights a track it could not see.
#[cfg(feature = "radio")]
fn satellite_track_live() -> bool {
    crate::SAT_TRACK.lock().map(|g| g.is_some()).unwrap_or(true)
}

/// `shared` is the same Engine as `engine`, unlocked: the satellite verbs take the Engine lock
/// themselves and so cannot be handed the guard, and they never run on this thread. Nothing else
/// here may use it — an arm that locked it would deadlock against the guard above it.
pub fn execute(
    engine: &mut Engine,
    shared: &crate::SharedEngine,
    context: &Context,
    action: &Action,
    permit: Permit,
    spots: Option<&crate::SharedSpots>,
) -> Result<Completion, Reason> {
    #[cfg(not(feature = "radio"))]
    let _ = (spots, shared);
    if !permit.valid(std::time::Instant::now()) {
        return Err(Reason::AuthorityExpired);
    }
    if !context.matches_radio(engine) {
        return Err(Reason::ContextChanged);
    }
    if engine.remote_receiver_context_generation() == u64::MAX
        || engine.remote_actuation_context_generation() == u64::MAX
    {
        return Err(Reason::ContextChanged);
    }
    match action {
        Action::QsoLogCurrent { .. }
        | Action::QsoConfirm { .. }
        | Action::QsoDiscard { .. }
        | Action::FtCq { .. }
        | Action::FtTxEnabled { .. }
        | Action::FtCall { .. }
        | Action::FtExchange { .. }
        | Action::FtSetting { .. }
        | Action::FtRuntime { .. }
        | Action::FtMessage { .. } => return Err(Reason::UnsupportedAction),
        #[cfg(feature = "radio")]
        Action::Radio { radio_id } => {
            if !engine.remote_selection_host_ready() {
                return Err(Reason::UnsupportedAction);
            }
            return engine.queue_remote_radio_selection(
                *radio_id,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::Level {
            mode,
            level,
            expected,
            value,
        } => {
            let level = tempo_app::engine::remote_radio::RadioLevel::from_name(level)
                .ok_or(Reason::InvalidAction)?;
            return engine.queue_remote_level(
                mode,
                level,
                *expected,
                *value,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        // The Sub receiver's levels: the desktop Sub row's own engine verb, with every refusal it
        // makes, behind the receive-display admission. `stationState`: the station took it, and
        // what the radio accepted arrives in the snapshot — there is no read-back to claim.
        #[cfg(feature = "radio")]
        Action::SubLevel { level, value } => {
            use tempo_app::engine::sub_controls::SubLevel;
            let level = match level.as_str() {
                "rfGain" => SubLevel::Rf,
                "afGain" => SubLevel::Af,
                "squelch" => SubLevel::Sql,
                _ => return Err(Reason::InvalidAction),
            };
            engine.queue_remote_sub_level(
                level,
                *value,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            return Ok(station_state());
        }
        #[cfg(feature = "radio")]
        Action::WorkSpot {
            mode,
            dial_mhz,
            band,
            call,
        } => {
            // `workSpot` is exactly two words wide and stays that way. RTTY has its own action,
            // so widening the engine verb to take it must not widen this one by the back door —
            // the whole point of the separate action is that the grammars cannot drift apart.
            if mode != "cw" && mode != "phone" {
                return Err(Reason::InvalidAction);
            }
            // Split needs a complete radio-worker transaction. Until then,
            // refuse a station-resolved pile-up rather than silently tuning
            // simplex. Failure to inspect the buffer is not evidence of none.
            let buffer = spots
                .ok_or(Reason::ReadingUnavailable)?
                .try_lock()
                .map_err(|_| Reason::ReadingUnavailable)?;
            if crate::work_spot_split_offset(&buffer, call, *dial_mhz, std::time::Instant::now())
                .is_some()
            {
                return Err(Reason::UnsupportedAction);
            }
            drop(buffer);
            return engine.queue_remote_spot(
                mode,
                *dial_mhz,
                band,
                call,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        // RTTY Work: the same station-owned pile-up evidence as CW/Phone Work, and the same
        // not-arming receive QSY — `queue_remote_spot` enters the RTTY section through
        // `work_spot_split_with_arming(.., arm_manual: false)`, so the TX-enable latch is never
        // touched and no transmit authority is taken.
        #[cfg(feature = "radio")]
        Action::WorkRttySpot {
            dial_mhz,
            band,
            call,
        } => {
            let buffer = spots
                .ok_or(Reason::ReadingUnavailable)?
                .try_lock()
                .map_err(|_| Reason::ReadingUnavailable)?;
            if crate::work_spot_split_offset(&buffer, call, *dial_mhz, std::time::Instant::now())
                .is_some()
            {
                return Err(Reason::UnsupportedAction);
            }
            drop(buffer);
            return engine.queue_remote_spot(
                "rtty",
                *dial_mhz,
                band,
                call,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        // FT8/FT4 Work: the same station-owned pile-up evidence as CW/Phone Work, then the tiered
        // receive QSY. Refused while TX is armed; it never enables TX or calls the station.
        #[cfg(feature = "radio")]
        Action::WorkDigitalSpot {
            tier,
            dial_mhz,
            band,
            call,
        } => {
            let buffer = spots
                .ok_or(Reason::ReadingUnavailable)?
                .try_lock()
                .map_err(|_| Reason::ReadingUnavailable)?;
            if crate::work_spot_split_offset(&buffer, call, *dial_mhz, std::time::Instant::now())
                .is_some()
            {
                return Err(Reason::UnsupportedAction);
            }
            drop(buffer);
            return engine.queue_remote_digital_spot(
                *tier,
                *dial_mhz,
                band,
                call,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        // A memory recall through the readback transaction. Refused while TX is armed, when an FM
        // machine's input is outside the licence, and when another radio owns the band.
        #[cfg(feature = "radio")]
        Action::MemoryRecall {
            section,
            dial_mhz,
            band,
            sideband,
            fm,
        } => {
            let shift = |shift: RepeaterShift| match shift {
                RepeaterShift::Simplex => "simplex",
                RepeaterShift::Plus => "plus",
                RepeaterShift::Minus => "minus",
            };
            return engine.queue_remote_memory_recall(
                match section {
                    RecallSection::Cw => "cw",
                    RecallSection::Phone => "phone",
                    RecallSection::Digital => "digital",
                },
                *dial_mhz,
                band,
                sideband.map(|s| match s {
                    RecallSideband::Usb => "USB",
                    RecallSideband::Lsb => "LSB",
                }),
                fm.map(|f| (shift(f.shift), i64::from(f.offset_hz), f.tone_hz)),
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        // The Program Tune through the readback transaction. Refused while TX is armed and when
        // the input the offset keys is outside the licence; it never arms or keys.
        #[cfg(feature = "radio")]
        Action::Repeater {
            output_mhz,
            shift,
            offset_hz,
            tone_hz,
        } => {
            return engine.queue_remote_repeater(
                *output_mhz,
                match shift {
                    RepeaterShift::Simplex => "simplex",
                    RepeaterShift::Plus => "plus",
                    RepeaterShift::Minus => "minus",
                },
                i64::from(*offset_hz),
                *tone_hz,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        // The APRS channel pick through the readback transaction: a regional APRS channel only,
        // refused while TX is armed; it never arms, keys or queues an APRS transmission.
        #[cfg(feature = "radio")]
        Action::AprsTune { dial_mhz } => {
            return engine.queue_remote_aprs_tune(
                *dial_mhz,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        // The rotator: resolved from station Settings here, written by its own worker thread so
        // this lock is never held across rotctld. It never arms or keys anything.
        #[cfg(feature = "radio")]
        Action::RotatorPoint { azimuth_deg } => {
            if !rotator::valid_azimuth(*azimuth_deg) {
                return Err(Reason::InvalidAction);
            }
            return rotator::queue(
                engine.settings(),
                rotator::Command::Point(*azimuth_deg),
                permit,
                satellite_track_live(),
            );
        }
        #[cfg(feature = "radio")]
        Action::RotatorPointAtCall { call } => {
            if !rotator::valid_call(call) {
                return Err(Reason::InvalidAction);
            }
            let bearing = rotator::bearing_to_call(engine.settings(), call)?;
            return rotator::queue(
                engine.settings(),
                rotator::Command::Point(bearing),
                permit,
                satellite_track_live(),
            );
        }
        #[cfg(feature = "radio")]
        Action::RotatorStop {} => {
            return rotator::queue(engine.settings(), rotator::Command::Stop, permit, false);
        }
        // The satellite section. Each verb is the desktop's own, run on its own worker because the
        // Engine lock held here is the lock each of them takes — see `satellite`'s module header.
        #[cfg(feature = "radio")]
        Action::SatTrack { name, aos_unix } => {
            return satellite::queue(
                satellite::Command::Track {
                    name: name.clone(),
                    aos_unix: *aos_unix,
                },
                permit,
                shared,
            );
        }
        #[cfg(feature = "radio")]
        Action::SatStopTrack {} => {
            return satellite::queue(satellite::Command::StopTrack, permit, shared);
        }
        #[cfg(feature = "radio")]
        Action::SatTransponder { name, index, auto } => {
            return satellite::queue(
                satellite::Command::Transponder {
                    name: name.clone(),
                    index: *index,
                    auto: *auto,
                },
                permit,
                shared,
            );
        }
        #[cfg(feature = "radio")]
        Action::SatDoppler { on } => {
            return satellite::queue(satellite::Command::Doppler { on: *on }, permit, shared);
        }
        #[cfg(feature = "radio")]
        Action::SatUplinkMap { map, radio_id } => {
            return satellite::queue(
                satellite::Command::UplinkMap {
                    map: *map,
                    radio_id: *radio_id,
                },
                permit,
                shared,
            );
        }
        #[cfg(feature = "radio")]
        Action::SatPeg { on } => {
            return satellite::queue(satellite::Command::Peg { on: *on }, permit, shared);
        }
        #[cfg(feature = "radio")]
        Action::SatElements {} => {
            return satellite::queue(satellite::Command::Elements, permit, shared);
        }
        #[cfg(feature = "radio")]
        Action::RxGain {
            radio_id,
            expected_settings_revision,
            expected_gain,
            gain,
        } => {
            if expected_settings_revision.len() != 64
                || !expected_settings_revision
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(Reason::InvalidAction);
            }
            if *radio_id != context.radio_id
                || super::super::query::settings_revision(engine.settings())
                    .map_err(|_| Reason::ReadingUnavailable)?
                    != *expected_settings_revision
            {
                return Err(Reason::ContextChanged);
            }
            engine.save_remote_rx_gain(
                *radio_id,
                *expected_gain,
                *gain,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            });
            return Ok(result);
        }
        #[cfg(feature = "radio")]
        Action::DecodeDepth {
            expected_tier,
            expected_depth,
            depth,
        } => {
            engine.save_remote_decode_depth(
                *expected_tier,
                *expected_depth,
                *depth,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            });
            return Ok(result);
        }
        #[cfg(feature = "radio")]
        Action::RxOffset {
            expected_tier,
            expected_hz,
            hz,
        } => {
            engine.save_remote_rx_offset(
                *expected_tier,
                *expected_hz,
                *hz,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            });
            return Ok(result);
        }
        #[cfg(feature = "radio")]
        Action::Js8Speed {
            expected_speed,
            speed,
        } => {
            engine.save_remote_js8_speed(
                *expected_speed,
                *speed,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            });
            return Ok(result);
        }
        #[cfg(feature = "radio")]
        Action::Msk144Period {
            expected_period_secs,
            period_secs,
        } => {
            engine.save_remote_msk144_period(
                *expected_period_secs,
                *period_secs,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            });
            return Ok(result);
        }
        #[cfg(feature = "radio")]
        Action::AiCw { expected_on, on } => {
            engine.save_remote_ai_cw(*expected_on, *on, &permit)?;
            // The AI decoder is an assistance source: journal it exactly as the local switch does.
            // Unit tests never write the operator's real config directory.
            #[cfg(not(test))]
            crate::journal_assistance(engine.settings(), "AI CW decoder toggled", false);
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            });
            return Ok(result);
        }
        // Receive-only: re-runs the decoder over retained audio, refused while TX is enabled.
        #[cfg(feature = "radio")]
        Action::Redecode { expected_tier } => engine.remote_redecode(*expected_tier)?,
        // Split, RIT, XIT and VFO use the local one-shot verbs the radio loop writes next pass.
        // The engine refuses a transmit-frequency change while TX is armed or outside the licence.
        #[cfg(feature = "radio")]
        Action::Split {
            expected_tx_mhz,
            tx_mhz,
        } => {
            engine.queue_remote_split(
                *expected_tx_mhz,
                *tx_mhz,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            return Ok(station_state());
        }
        #[cfg(feature = "radio")]
        Action::Rit { expected_hz, hz } => {
            engine.queue_remote_rit(
                *expected_hz,
                *hz,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            return Ok(station_state());
        }
        #[cfg(feature = "radio")]
        Action::Xit { expected_hz, hz } => {
            engine.queue_remote_xit(
                *expected_hz,
                *hz,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            return Ok(station_state());
        }
        #[cfg(feature = "radio")]
        Action::Vfo { expected_vfo, vfo } => {
            engine.queue_remote_vfo(
                *expected_vfo == Vfo::B,
                *vfo == Vfo::B,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            return Ok(station_state());
        }
        // Receive-display one-shots with the local scope verbs, only for the family this station
        // runs and never while the transmitter is owned or a tune carrier is up.
        #[cfg(feature = "radio")]
        Action::Scope {
            setting,
            hz,
            tenths_db,
            position,
            ref_dbm,
        } => {
            use tempo_app::engine::remote_radio::RemoteScope;
            let scope = match (setting, hz, tenths_db, position, ref_dbm) {
                (ScopeSetting::Span, Some(hz), None, None, None) => RemoteScope::Span(*hz),
                (ScopeSetting::Ref, None, Some(tenths), None, None) => RemoteScope::Ref(*tenths),
                (ScopeSetting::Position, None, None, Some(place), None) => {
                    use tempo_audio::yaesu_wf::{mode_code_for, ScopePosition};
                    // The desktop command's own resolution: keep the display family the rig
                    // reports, falling back to W/F NORMAL before anything has been read.
                    let current = engine
                        .snapshot()
                        .radio
                        .scope_mode_code
                        .unwrap_or(u32::from(b'4')) as u8;
                    RemoteScope::Position(mode_code_for(
                        match place {
                            ScopePlace::Center => ScopePosition::Center,
                            ScopePlace::Cursor => ScopePosition::Cursor,
                            ScopePlace::Fix => ScopePosition::Fix,
                        },
                        current,
                    ))
                }
                (ScopeSetting::PanSpan, Some(hz), None, None, None) => RemoteScope::PanSpan(*hz),
                (ScopeSetting::PanRef, None, None, None, Some(reference)) => {
                    RemoteScope::PanRef(*reference)
                }
                _ => return Err(Reason::InvalidAction),
            };
            let family = scope_family(engine.settings());
            engine.queue_remote_scope(
                scope,
                family,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            return Ok(station_state());
        }
        // A destructive change to the operator's own received pictures, so it is the row the
        // browser showed or nothing: zero matches and more than one are both ContextChanged, and
        // the path is resolved here from the station's own gallery.
        Action::SstvDeleteImage { finished_utc, mode } => {
            let mut found = engine
                .sstv_gallery()
                .iter()
                .filter(|g| g.finished_utc == *finished_utc && g.mode == *mode);
            let path = found.next().map(|g| g.path.clone());
            if found.next().is_some() {
                return Err(Reason::ContextChanged);
            }
            let path = path.ok_or(Reason::ContextChanged)?;
            crate::delete_sstv_gallery_image(engine, &path)
                .map_err(|_| Reason::PersistenceFailed)?;
            return Ok(station_state());
        }
        Action::ReceiverArm { receiver, on } => match receiver {
            Receiver::Rtty => engine.set_rtty_armed(*on),
            Receiver::Psk => engine.set_psk_armed(*on),
            Receiver::Sstv => engine.set_sstv_armed(*on),
            Receiver::Aprs => engine.set_aprs_receive_only(*on),
        },
        Action::ReceiverClear { receiver } => match receiver {
            TextReceiver::Cw => engine.cw_clear(),
            TextReceiver::Rtty => engine.rtty_clear(),
            TextReceiver::Psk => engine.psk_clear(),
        },
        Action::ReceiverAfcReset { receiver } => match receiver {
            KeyboardReceiver::Rtty => engine.request_rtty_afc_reset(),
            KeyboardReceiver::Psk => engine.request_psk_afc_reset(),
        },
        Action::ReceiverNet { receiver, hz } => {
            if !hz.is_finite() || !(300.0..=3700.0).contains(hz) {
                return Err(Reason::InvalidAction);
            }
            match receiver {
                KeyboardReceiver::Rtty => engine.rtty_net(*hz),
                KeyboardReceiver::Psk => engine.psk_net(*hz),
            }
        }
        Action::PskMode { mode, reverse } => {
            use tempo_core::psk::PskModeKind;
            let mode = match mode.as_str() {
                "PSK31" => PskModeKind::Bpsk31,
                "QPSK31" => PskModeKind::Qpsk31,
                _ => return Err(Reason::InvalidAction),
            };
            engine
                .set_psk_mode(mode, *reverse)
                .map_err(|_| Reason::StationBusy)?;
        }
        Action::Disarm {} => {
            engine.set_tx_enabled(false);
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::StationState,
            });
            return Ok(result);
        }
        #[cfg(feature = "radio")]
        Action::Frequency {
            dial_mhz,
            band,
            sideband,
        } => {
            return engine.queue_remote_frequency(
                *dial_mhz,
                band,
                sideband,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::Band { band, mode } => {
            return engine.queue_remote_band(
                band,
                mode,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::FilterWidth {
            mode,
            expected_hz,
            hz,
        } => {
            return engine.queue_remote_filter_width(
                mode,
                *expected_hz,
                *hz,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::ReceiverFunction {
            mode,
            func,
            expected_on,
            on,
        } => {
            use tempo_app::engine::remote_radio::{ReceiverDsp, ReceiverFunction};
            let func = ReceiverFunction::from_name(func).ok_or(Reason::InvalidAction)?;
            return engine.queue_remote_receiver_dsp(
                mode,
                ReceiverDsp::Function {
                    func,
                    on: *expected_on,
                },
                ReceiverDsp::Function { func, on: *on },
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::Agc {
            mode,
            expected_speed,
            speed,
        } => {
            use tempo_app::engine::remote_radio::{AgcSpeed, ReceiverDsp};
            return engine.queue_remote_receiver_dsp(
                mode,
                ReceiverDsp::Agc(AgcSpeed::from_name(expected_speed).ok_or(Reason::InvalidAction)?),
                ReceiverDsp::Agc(AgcSpeed::from_name(speed).ok_or(Reason::InvalidAction)?),
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::PhoneMode {
            expected_mode,
            mode,
        } => {
            return engine.queue_remote_phone_mode(
                (expected_mode != "auto").then_some(expected_mode.as_str()),
                (mode != "auto").then_some(mode.as_str()),
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::Workspace { workspace } => {
            return engine.queue_remote_workspace(
                *workspace,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::Mode {
            mode,
            follow_frequency,
        } => {
            return engine.queue_remote_mode(
                mode,
                *follow_frequency,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::Tier { tier } => {
            return engine.queue_remote_tier(
                *tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::AmpFollowBand {
            radio_id,
            expected_settings_revision,
            expected_follow,
            follow,
        } => {
            if expected_settings_revision.len() != 64
                || !expected_settings_revision
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || expected_follow == follow
            {
                return Err(Reason::InvalidAction);
            }
            if *radio_id != context.radio_id
                || super::super::query::settings_revision(engine.settings())
                    .map_err(|_| Reason::ReadingUnavailable)?
                    != *expected_settings_revision
            {
                return Err(Reason::ContextChanged);
            }
            let profile = engine
                .settings()
                .active_profile()
                .ok_or(Reason::ContextChanged)?;
            if !["spe", "kpa"].contains(&profile.amp_model.trim().to_lowercase().as_str())
                || profile.amp_port.trim().is_empty()
            {
                return Err(Reason::HardwareUnavailable);
            }
            if *follow {
                tempo_app::remote_control::amplifier::follow_ready(
                    engine,
                    context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                    context.amp_connection.ok_or(Reason::ReadingUnavailable)?,
                    context
                        .amp_read_sequence
                        .ok_or(Reason::ReadingUnavailable)?,
                )?;
            }
            engine.save_remote_amp_follow_band(*radio_id, *expected_follow, *follow, &permit)?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            });
            return Ok(result);
        }
        #[cfg(feature = "radio")]
        Action::AmpOperate {
            expected_operate,
            operate,
        } => {
            use tempo_app::remote_control::amplifier::{Request, Target};
            if expected_operate == operate {
                return Err(Reason::InvalidAction);
            }
            let request = Request::new(
                engine,
                Target::Operate {
                    expected: *expected_operate,
                    desired: *operate,
                },
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                context.amp_connection.ok_or(Reason::ReadingUnavailable)?,
                context
                    .amp_read_sequence
                    .ok_or(Reason::ReadingUnavailable)?,
                permit,
            )?;
            return engine.queue_remote_amp(request);
        }
        #[cfg(feature = "radio")]
        Action::AmpBand {
            expected_band,
            direction,
        } => {
            use tempo_app::remote_control::amplifier::{Request, Target};
            use tempo_audio::amplifier::{band_index_for_label, kpa_band_label, spe_band_label};
            if ![-1, 1].contains(direction) {
                return Err(Reason::InvalidAction);
            }
            let next = i16::from(band_index_for_label(expected_band).ok_or(Reason::InvalidAction)?)
                + i16::from(*direction);
            let next = u8::try_from(next).map_err(|_| Reason::InvalidAction)?;
            let family = engine
                .settings()
                .active_profile()
                .map(|p| p.amp_model.trim().to_lowercase())
                .ok_or(Reason::HardwareUnavailable)?;
            // The shared SPE "15K" model token does not identify the hardware
            // series: early 1.5K manuals include 4 m, third-series rev. 3.2 does
            // not. Do not guess that a front-panel step beyond 6 m is supported
            // or that it wraps. 13K explicitly reports the 4 m-capable model.
            let model = engine
                .remote_monitor_observation()
                .amplifier
                .map(|a| a.model)
                .ok_or(Reason::ReadingUnavailable)?;
            if family == "spe" && next > if model == "13K" { 11 } else { 10 } {
                return Err(Reason::InvalidAction);
            }
            let desired = match family.as_str() {
                "spe" => spe_band_label(next),
                "kpa" => kpa_band_label(next),
                _ => None,
            }
            .ok_or(Reason::InvalidAction)?;
            let request = Request::new(
                engine,
                Target::Band {
                    expected: expected_band.clone(),
                    desired: desired.into(),
                    direction: *direction,
                },
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                context.amp_connection.ok_or(Reason::ReadingUnavailable)?,
                context
                    .amp_read_sequence
                    .ok_or(Reason::ReadingUnavailable)?,
                permit,
            )?;
            return engine.queue_remote_amp(request);
        }
        // A build without the radio owner cannot dispatch hardware actions.
        #[cfg(not(feature = "radio"))]
        _ => return Err(Reason::UnsupportedAction),
    }
    let result = Completion::default();
    result.finish(Outcome::Applied {
        evidence: Evidence::ReceiverState,
    });
    Ok(result)
}

/// The native panadapter this station's configuration runs: the same rig-model and opt-in test the
/// radio loop starts its scope worker from. What a browser drew is never evidence of the family.
#[cfg(feature = "radio")]
fn scope_family(
    settings: &tempo_app::settings::Settings,
) -> tempo_app::engine::remote_radio::ScopeFamily {
    use tempo_app::engine::remote_radio::ScopeFamily;
    use tempo_audio::rigmodels::{native_spectrum_kind, SpectrumKind};
    let conn = if tempo_app::settings::rig_conn_is_network(&settings.rig_conn, &settings.rig_addr) {
        "network"
    } else {
        "serial"
    };
    match native_spectrum_kind(settings.rig_model, conn) {
        Some(SpectrumKind::IcomCiv { .. }) => ScopeFamily::IcomCiv,
        Some(SpectrumKind::FlexVita) if settings.flex_native_pan => ScopeFamily::Flex,
        _ => ScopeFamily::None,
    }
}

fn station_state() -> Completion {
    let result = Completion::default();
    result.finish(Outcome::Applied {
        evidence: Evidence::StationState,
    });
    result
}

impl Action {
    pub fn is_logging(&self) -> bool {
        matches!(
            self,
            Self::QsoLogCurrent { .. } | Self::QsoConfirm { .. } | Self::QsoDiscard { .. }
        )
    }

    pub fn minimum_version(&self) -> u8 {
        match self {
            Self::QsoLogCurrent { .. }
            | Self::QsoConfirm { .. }
            | Self::QsoDiscard { .. }
            | Self::FtCq { .. }
            | Self::FtTxEnabled { .. }
            | Self::FtCall { .. }
            | Self::FtExchange { .. }
            | Self::FtSetting { .. }
            | Self::FtRuntime { .. }
            | Self::FtMessage { .. } => 4,
            Self::Level { .. }
            | Self::Frequency { .. }
            | Self::Band { .. }
            | Self::FilterWidth { .. }
            | Self::ReceiverFunction { .. }
            | Self::Agc { .. }
            | Self::PhoneMode { .. }
            | Self::Mode { .. }
            | Self::WorkSpot { .. }
            | Self::WorkRttySpot { .. }
            | Self::WorkDigitalSpot { .. }
            | Self::Repeater { .. }
            | Self::AprsTune { .. }
            | Self::RotatorPoint { .. }
            | Self::RotatorPointAtCall { .. }
            | Self::RotatorStop { .. }
            | Self::SatTrack { .. }
            | Self::SatStopTrack { .. }
            | Self::SatTransponder { .. }
            | Self::SatDoppler { .. }
            | Self::SatUplinkMap { .. }
            | Self::SatPeg { .. }
            | Self::SatElements { .. }
            | Self::SstvDeleteImage { .. }
            | Self::MemoryRecall { .. }
            | Self::Tier { .. }
            | Self::Workspace { .. }
            | Self::Js8Speed { .. }
            | Self::Msk144Period { .. }
            | Self::DecodeDepth { .. }
            | Self::RxOffset { .. }
            | Self::RxGain { .. }
            | Self::Radio { .. }
            | Self::AmpFollowBand { .. }
            | Self::AiCw { .. }
            | Self::Redecode { .. }
            | Self::Split { .. }
            | Self::Rit { .. }
            | Self::Xit { .. }
            | Self::Vfo { .. }
            | Self::Scope { .. }
            | Self::SubLevel { .. } => 3,
            _ => 2,
        }
    }
}

pub fn capabilities(version: u8) -> Vec<&'static str> {
    #[cfg(feature = "radio")]
    {
        if version == 2 {
            vec!["decoder", "amplifier"]
        } else {
            vec![
                "decoder",
                "amplifier",
                "frequency",
                "mode",
                "tier",
                "ampFollowBand",
                "workspace",
                "decoderSettings",
                "receiverSettings",
                "receiverGain",
                "bandSelection",
                "receiverFilter",
                "receiverDsp",
                "phoneMode",
                "workSpot",
                "radioLevels",
                "radioSelection",
                "fmTuning",
                "fmReceiver",
                // Remote parity batch 1: each hint ships with its action.
                "aiCw",
                "redecode",
                "splitTuning",
                "ritTuning",
                "workDigitalSpot",
                "repeaterTuning",
                "memoryRecall",
                "aprsTuning",
                "rotator",
                "rigScope",
                // Remote parity leftovers: each hint ships with its action, so a station that
                // predates one never names it and a page never sends that action to it.
                "workRttySpot",
                "sstvGallery",
                // The satellite section. One hint for the whole section, because its verbs are one
                // operator act: a transponder pick tunes the radio, an arm steers it for the pass,
                // and the Stop that ends both has to be live wherever they are.
                "satellite",
                // The Sub receiver's levels (dual-receiver radios). Ships with `radio.subLevel`,
                // so a station that predates the action never names it and a page never sends it.
                "subReceiverLevels",
            ]
        }
    }
    #[cfg(not(feature = "radio"))]
    {
        let _ = version;
        vec!["decoder"]
    }
}

impl Action {
    pub fn transmit_epoch(&self) -> Option<&str> {
        match self {
            Self::FtCq { transmit_epoch, .. }
            | Self::FtTxEnabled { transmit_epoch, .. }
            | Self::FtCall { transmit_epoch, .. }
            | Self::FtExchange { transmit_epoch, .. }
            | Self::FtSetting { transmit_epoch, .. }
            | Self::FtRuntime { transmit_epoch, .. }
            | Self::FtMessage { transmit_epoch, .. } => Some(transmit_epoch),
            _ => None,
        }
    }
}

pub fn execute_transmit(
    engine: &mut Engine,
    context: &Context,
    action: &Action,
    permit: tempo_app::remote_control::transmit::TransmitPermit,
) -> Result<Completion, Reason> {
    if !context.matches_radio(engine) {
        return Err(Reason::ContextChanged);
    }
    match action {
        Action::FtRuntime {
            expected_tier,
            expected,
            change,
            ..
        } => {
            engine.validate_remote_ft_radio(
                *expected_tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            engine.change_remote_ft_runtime(&permit, expected, change)?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: if matches!(
                    change,
                    tempo_app::engine::remote_transmit::runtime::FtRuntimeChange::RxOffset { .. }
                ) {
                    Evidence::SettingsSaved
                } else {
                    Evidence::StationState
                },
            });
            return Ok(result);
        }
        Action::FtSetting {
            expected_tier,
            expected,
            change,
            ..
        } => {
            engine.validate_remote_ft_radio(
                *expected_tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            engine.change_remote_ft_setting(&permit, expected, change)?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: if matches!(
                    change,
                    tempo_app::engine::remote_transmit::settings::FtSettingChange::Auto { .. }
                ) {
                    Evidence::StationState
                } else {
                    Evidence::SettingsSaved
                },
            });
            return Ok(result);
        }
        Action::FtMessage {
            expected_tier,
            expected_qso,
            call,
            grid,
            text,
            ..
        } => {
            engine.validate_remote_ft_radio(
                *expected_tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            engine.set_remote_ft_message(permit, expected_qso, call, grid.as_deref(), text)?;
        }
        Action::FtExchange {
            expected_tier,
            expected_qso,
            change,
            ..
        } => {
            engine.validate_remote_ft_radio(
                *expected_tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            engine.change_remote_ft_exchange(permit, expected_qso, change)?;
        }
        Action::FtCq {
            expected_tier,
            direction,
            ..
        } => {
            if let Some(direction) = direction {
                let letters = (1..=4).contains(&direction.len())
                    && direction.bytes().all(|b| b.is_ascii_uppercase());
                let digits = direction.len() == 3 && direction.bytes().all(|b| b.is_ascii_digit());
                if !letters && !digits {
                    return Err(Reason::InvalidAction);
                }
            }
            engine.validate_remote_ft_radio(
                *expected_tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            engine.start_remote_ft_cq(permit, direction.as_deref())?;
        }
        Action::FtCall {
            expected_tier,
            selection,
            ..
        } => {
            engine.validate_remote_ft_radio(
                *expected_tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            engine.call_remote_ft_selection(permit, selection)?;
        }
        Action::FtTxEnabled {
            expected_tier, on, ..
        } => {
            if engine.tier() != *expected_tier {
                return Err(Reason::ContextChanged);
            }
            if *on {
                engine.validate_remote_ft_radio(
                    *expected_tier,
                    context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                    &permit,
                )?;
            }
            engine.set_remote_ft_tx_enabled(permit, *on)?;
        }
        _ => return Err(Reason::UnsupportedAction),
    }
    let result = Completion::default();
    result.finish(Outcome::Applied {
        evidence: Evidence::StationState,
    });
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE SAFETY PIN UNDER OPTIMISTIC TUNING. The browser is allowed to draw a dial the radio has
    /// not reached yet, and the first argument for why that is safe is this: **the whole remote
    /// transmit surface is FT-sequencer arming.** There is no remote PTT, no remote CW key and no
    /// remote Tune, so no browser-side number can ever become a keying decision — the engine keys
    /// through its own gates against its own CAT readback.
    ///
    /// That argument is only as good as the code, so it is pinned here rather than written down:
    /// every `Action` carrying a `transmit_epoch` is an `Ft*` action, and every `Ft*` action
    /// carries one. The day someone adds a remote PTT, this fails at that commit.
    ///
    /// It cannot be skipped by accident. `ordinal` matches EXHAUSTIVELY — no wildcard arm — so a
    /// new variant does not compile until it is listed, and the test then demands a dense set of
    /// ordinals, so the author must also add an instance to `every_action` (a gap fails, a reused
    /// number fails, a number past the end fails). Whatever they do, the assertion runs on it.
    fn ordinal(action: &Action) -> (usize, &'static str) {
        match action {
            Action::FtRuntime { .. } => (0, "FtRuntime"),
            Action::FtSetting { .. } => (1, "FtSetting"),
            Action::QsoLogCurrent { .. } => (2, "QsoLogCurrent"),
            Action::QsoConfirm { .. } => (3, "QsoConfirm"),
            Action::QsoDiscard { .. } => (4, "QsoDiscard"),
            Action::FtMessage { .. } => (5, "FtMessage"),
            Action::FtExchange { .. } => (6, "FtExchange"),
            Action::FtCq { .. } => (7, "FtCq"),
            Action::FtCall { .. } => (8, "FtCall"),
            Action::FtTxEnabled { .. } => (9, "FtTxEnabled"),
            Action::Level { .. } => (10, "Level"),
            Action::ReceiverArm { .. } => (11, "ReceiverArm"),
            Action::ReceiverClear { .. } => (12, "ReceiverClear"),
            Action::ReceiverAfcReset { .. } => (13, "ReceiverAfcReset"),
            Action::ReceiverNet { .. } => (14, "ReceiverNet"),
            Action::PskMode { .. } => (15, "PskMode"),
            Action::AiCw { .. } => (16, "AiCw"),
            Action::Redecode { .. } => (17, "Redecode"),
            Action::Split { .. } => (18, "Split"),
            Action::Rit { .. } => (19, "Rit"),
            Action::Xit { .. } => (20, "Xit"),
            Action::Vfo { .. } => (21, "Vfo"),
            Action::Js8Speed { .. } => (22, "Js8Speed"),
            Action::Msk144Period { .. } => (23, "Msk144Period"),
            Action::DecodeDepth { .. } => (24, "DecodeDepth"),
            Action::RxOffset { .. } => (25, "RxOffset"),
            Action::RxGain { .. } => (26, "RxGain"),
            Action::Disarm { .. } => (27, "Disarm"),
            Action::Frequency { .. } => (28, "Frequency"),
            Action::Band { .. } => (29, "Band"),
            Action::FilterWidth { .. } => (30, "FilterWidth"),
            Action::ReceiverFunction { .. } => (31, "ReceiverFunction"),
            Action::Agc { .. } => (32, "Agc"),
            Action::PhoneMode { .. } => (33, "PhoneMode"),
            Action::Mode { .. } => (34, "Mode"),
            Action::WorkSpot { .. } => (35, "WorkSpot"),
            Action::WorkRttySpot { .. } => (36, "WorkRttySpot"),
            Action::WorkDigitalSpot { .. } => (37, "WorkDigitalSpot"),
            Action::Repeater { .. } => (38, "Repeater"),
            Action::AprsTune { .. } => (39, "AprsTune"),
            Action::RotatorPoint { .. } => (40, "RotatorPoint"),
            Action::RotatorPointAtCall { .. } => (41, "RotatorPointAtCall"),
            Action::RotatorStop { .. } => (42, "RotatorStop"),
            Action::SatTrack { .. } => (43, "SatTrack"),
            Action::SatStopTrack { .. } => (44, "SatStopTrack"),
            Action::SatTransponder { .. } => (45, "SatTransponder"),
            Action::SatDoppler { .. } => (46, "SatDoppler"),
            Action::SatUplinkMap { .. } => (47, "SatUplinkMap"),
            Action::SatPeg { .. } => (48, "SatPeg"),
            Action::SatElements { .. } => (49, "SatElements"),
            Action::SstvDeleteImage { .. } => (50, "SstvDeleteImage"),
            Action::Scope { .. } => (51, "Scope"),
            Action::MemoryRecall { .. } => (52, "MemoryRecall"),
            Action::Tier { .. } => (53, "Tier"),
            Action::Workspace { .. } => (54, "Workspace"),
            Action::Radio { .. } => (55, "Radio"),
            Action::AmpOperate { .. } => (56, "AmpOperate"),
            Action::AmpBand { .. } => (57, "AmpBand"),
            Action::AmpFollowBand { .. } => (58, "AmpFollowBand"),
            Action::SubLevel { .. } => (59, "SubLevel"),
        }
    }

    /// One of every variant. The field VALUES are placeholders and mean nothing — `transmit_epoch`
    /// is read for its presence, never its content.
    fn every_action() -> Vec<Action> {
        use tempo_app::dto::Tier;
        use tempo_app::engine::remote_logging::PendingLogEdits;
        use tempo_app::engine::remote_radio::Workspace;
        use tempo_app::engine::remote_transmit::runtime::{FtRuntimeChange, FtRuntimeContext};
        use tempo_app::engine::remote_transmit::settings::{FtSettingChange, FtSettingsContext};
        use tempo_app::engine::remote_transmit::{
            FtCallSelection, FtExchangeChange, FtExchangeContext,
        };

        let ft_settings = || FtSettingsContext {
            key: "x".into(),
            tx_offset_hz: 0.0,
            rx_offset_hz: 0.0,
            hold_tx_freq: false,
            tx_even: false,
            tx_cycle_auto: false,
        };
        let ft_qso = || FtExchangeContext {
            dxcall: None,
            state: "x".into(),
            tx_now: None,
            cq_running: false,
        };
        vec![
            Action::FtRuntime {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                expected: FtRuntimeContext {
                    settings: ft_settings(),
                    skip_tx1: false,
                },
                change: FtRuntimeChange::RxOffset { hz: 0.0 },
            },
            Action::FtSetting {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                expected: ft_settings(),
                change: FtSettingChange::TxOffset { hz: 0.0 },
            },
            Action::QsoLogCurrent {
                expected_key: "x".into(),
                expected_tier: Tier::Ft8,
                expected_qso: ft_qso(),
            },
            Action::QsoConfirm {
                expected_key: "x".into(),
                edits: PendingLogEdits {
                    call: "x".into(),
                    grid: None,
                    rst_sent: None,
                    rst_rcvd: None,
                },
            },
            Action::QsoDiscard {
                expected_key: "x".into(),
            },
            Action::FtMessage {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                expected_qso: ft_qso(),
                call: "x".into(),
                grid: None,
                text: "x".into(),
            },
            Action::FtExchange {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                expected_qso: ft_qso(),
                change: FtExchangeChange::Resend,
            },
            Action::FtCq {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                direction: None,
            },
            Action::FtCall {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                selection: FtCallSelection {
                    call: "x".into(),
                    grid: None,
                    message: None,
                    snr: None,
                    freq: None,
                },
            },
            Action::FtTxEnabled {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                on: false,
            },
            Action::Level {
                mode: "x".into(),
                level: "x".into(),
                expected: 0.0,
                value: 0.0,
            },
            Action::ReceiverArm {
                receiver: Receiver::Rtty,
                on: false,
            },
            Action::ReceiverClear {
                receiver: TextReceiver::Cw,
            },
            Action::ReceiverAfcReset {
                receiver: KeyboardReceiver::Rtty,
            },
            Action::ReceiverNet {
                receiver: KeyboardReceiver::Rtty,
                hz: 0.0,
            },
            Action::PskMode {
                mode: "x".into(),
                reverse: false,
            },
            Action::AiCw {
                expected_on: false,
                on: false,
            },
            Action::Redecode {
                expected_tier: Tier::Ft8,
            },
            Action::Split {
                expected_tx_mhz: None,
                tx_mhz: None,
            },
            Action::Rit {
                expected_hz: 0,
                hz: 0,
            },
            Action::Xit {
                expected_hz: 0,
                hz: 0,
            },
            Action::Vfo {
                expected_vfo: Vfo::A,
                vfo: Vfo::A,
            },
            Action::Js8Speed {
                expected_speed: 0,
                speed: 0,
            },
            Action::Msk144Period {
                expected_period_secs: 0,
                period_secs: 0,
            },
            Action::DecodeDepth {
                expected_tier: Tier::Ft8,
                expected_depth: 0,
                depth: 0,
            },
            Action::RxOffset {
                expected_tier: Tier::Ft8,
                expected_hz: 0.0,
                hz: 0.0,
            },
            Action::RxGain {
                radio_id: 0,
                expected_settings_revision: "x".into(),
                expected_gain: 0.0,
                gain: 0.0,
            },
            Action::Disarm {},
            Action::Frequency {
                dial_mhz: 0.0,
                band: "x".into(),
                sideband: "x".into(),
            },
            Action::Band {
                band: "x".into(),
                mode: "x".into(),
            },
            Action::FilterWidth {
                mode: "x".into(),
                expected_hz: 0,
                hz: 0,
            },
            Action::ReceiverFunction {
                mode: "x".into(),
                func: "x".into(),
                expected_on: false,
                on: false,
            },
            Action::Agc {
                mode: "x".into(),
                expected_speed: "x".into(),
                speed: "x".into(),
            },
            Action::PhoneMode {
                expected_mode: "x".into(),
                mode: "x".into(),
            },
            Action::Mode {
                mode: "x".into(),
                follow_frequency: false,
            },
            Action::WorkSpot {
                mode: "x".into(),
                dial_mhz: 0.0,
                band: "x".into(),
                call: "x".into(),
            },
            Action::WorkRttySpot {
                dial_mhz: 0.0,
                band: "x".into(),
                call: "x".into(),
            },
            Action::WorkDigitalSpot {
                tier: Tier::Ft8,
                dial_mhz: 0.0,
                band: "x".into(),
                call: "x".into(),
            },
            Action::Repeater {
                output_mhz: 0.0,
                shift: RepeaterShift::Simplex,
                offset_hz: 0,
                tone_hz: 0.0,
            },
            Action::AprsTune { dial_mhz: 0.0 },
            Action::RotatorPoint { azimuth_deg: 0.0 },
            Action::RotatorPointAtCall { call: "x".into() },
            Action::RotatorStop {},
            Action::SatTrack {
                name: "x".into(),
                aos_unix: 0,
            },
            Action::SatStopTrack {},
            Action::SatTransponder {
                name: "x".into(),
                index: None,
                auto: false,
            },
            Action::SatDoppler { on: false },
            Action::SatUplinkMap {
                map: None,
                radio_id: None,
            },
            Action::SatPeg { on: false },
            Action::SatElements {},
            Action::SstvDeleteImage {
                finished_utc: "x".into(),
                mode: "x".into(),
            },
            Action::Scope {
                setting: ScopeSetting::Span,
                hz: None,
                tenths_db: None,
                position: None,
                ref_dbm: None,
            },
            Action::MemoryRecall {
                section: RecallSection::Cw,
                dial_mhz: 0.0,
                band: "x".into(),
                sideband: None,
                fm: None,
            },
            Action::Tier { tier: Tier::Ft8 },
            Action::Workspace {
                workspace: Workspace::Ft,
            },
            Action::Radio { radio_id: 0 },
            Action::AmpOperate {
                expected_operate: false,
                operate: false,
            },
            Action::AmpBand {
                expected_band: "x".into(),
                direction: 0,
            },
            Action::AmpFollowBand {
                radio_id: 0,
                expected_settings_revision: "x".into(),
                expected_follow: false,
                follow: false,
            },
            Action::SubLevel {
                level: "x".into(),
                value: 0.0,
            },
        ]
    }

    /// Every variant name on the `Action` declaration, read out of this file's own source. A
    /// variant is one line at the enum's own indentation beginning with an upper-case name; its
    /// fields are indented deeper and named in lower case, so nothing else can match.
    fn declared_variants() -> Vec<&'static str> {
        let source = include_str!("station.rs");
        let body = source
            .split_once("pub enum Action {")
            .expect("the Action declaration")
            .1;
        let mut names = Vec::new();
        for line in body.lines() {
            if line == "}" {
                break;
            }
            let Some(rest) = line.strip_prefix("    ") else {
                continue;
            };
            if !rest.starts_with(|c: char| c.is_ascii_uppercase()) {
                continue;
            }
            let name = rest
                .split(|c: char| !c.is_ascii_alphanumeric())
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                names.push(name);
            }
        }
        names
    }

    #[test]
    fn every_action_carrying_a_transmit_epoch_is_ft_arming() {
        let actions = every_action();
        let mut seen: Vec<Option<&'static str>> = vec![None; actions.len()];
        for action in &actions {
            let (at, name) = ordinal(action);
            assert!(
                at < seen.len(),
                "{name} has ordinal {at} but only {} actions are built: add one to every_action",
                seen.len()
            );
            assert!(
                seen[at].is_none(),
                "{name} reuses ordinal {at}, already taken by {}: every variant needs its own",
                seen[at].unwrap()
            );
            seen[at] = Some(name);
        }
        for (at, name) in seen.iter().enumerate() {
            assert!(
                name.is_some(),
                "no action was built for ordinal {at}: every_action is missing a variant"
            );
        }
        // AND THE VARIANTS THEMSELVES, READ OFF THE DECLARATION. The exhaustive match alone does
        // not close this: an author who adds `Ptt`, gives it the next free ordinal and builds no
        // instance compiles and passes, because nothing ever calls `ordinal` on a variant that has
        // no instance. (Found by running exactly that mutation against the first version of this
        // test, which went green.) So the enum's own declaration is the roll call, and every name
        // on it has to have answered above.
        let declared = declared_variants();
        assert!(
            declared.len() >= 50
                && declared.contains(&"FtCq")
                && declared.contains(&"AmpFollowBand"),
            "the Action declaration was not parsed: {} name(s) found",
            declared.len()
        );
        let built: Vec<&str> = seen.iter().map(|n| n.unwrap()).collect();
        for name in &declared {
            assert!(
                built.contains(name),
                "{name} is declared on Action but every_action builds no instance of it, so the \
                 transmit-epoch check below never ran against it"
            );
        }
        assert_eq!(
            declared.len(),
            built.len(),
            "declared vs built action variants"
        );
        for action in &actions {
            let (_, name) = ordinal(action);
            assert_eq!(
                action.transmit_epoch().is_some(),
                name.starts_with("Ft"),
                "{name}: the remote transmit surface is FT arming only. An action that carries a \
                 transmit epoch can key the radio, and every one of them must be an Ft* action — \
                 adding a remote PTT, CW key or Tune breaks the safety case that lets the browser \
                 show an optimistic dial at all. Re-read that case before changing this test."
            );
        }
    }
}
