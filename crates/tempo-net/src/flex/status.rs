//! The status decoders: a status line from the radio translated into a typed, present-only delta.
//!
//! [`decode`] routes a [`Status`] by its object (`radio`, `slice <n>`, `display pan <id>`,
//! `display waterfall <id>`, `transmit`, `interlock`, `atu`, `meter`, `profile <type>`,
//! `client <handle>`, `stream <id>`) and returns a [`Decoded`]. Each delta field is `Some` only
//! when the line reported that key with a well-formed value ([`super::kv`]); the model
//! (`super::model`) applies exactly those fields and keeps the rest. A decoder has no write path
//! of any kind: its output is data, so no status line can make Nexus send a command.
//!
//! **The interlock is never merged across lines.** Interlock updates are deltas (FlexLib 4.2.18
//! `ParseInterlockStatus` consumes them so). [`InterlockDelta::sample`] is a state sample only
//! when one line carries all five state keys (`state`, `source`, `tx_client_handle`,
//! `tx_allowed`, `reason`) exactly once and well formed; a line that carries some of them makes
//! the previous sample stale ([`InterlockDelta::touches_state`]) without supplying a new one.
//! `interlock band <n>` is configuration, not a state sample, and is not decoded.
//!
//! **The keyed state comes only from the interlock.** The radio never sends `mox=` in transmit
//! status (port plan §4.3, from AetherSDR's 4.x behaviour), and the transmit decoder does not
//! read it: no field here can be mistaken for "the radio is keyed".
//!
//! PORTED from AetherSDR (https://github.com/aethersdr/AetherSDR, GPL-3.0; the upstream file
//! carries no per-file header, the licence is the repository's), the status decoders of
//! `src/core/backends/flex/FlexBackend.h` and `src/core/backends/flex/FlexBackend.cpp`, with the
//! delta types of `src/core/backends/SliceDelta.h`, `src/core/backends/TransmitDelta.h`,
//! `src/core/backends/RadioDelta.h`, `src/core/backends/MeterDef.h` and
//! `src/core/backends/ProfileDelta.h`, at commit `32fa50e4896a846a6970fa3f443bd49d667c139d`
//! (2026-10-03), translated from C++/Qt to Rust. Deliberate differences: decoders return deltas
//! instead of emitting Qt signals, and the object routing that upstream does in its model is done
//! here; every numeric field parses strictly (upstream's pan center and bandwidth did not), a
//! flag is `0` or `1`, ids keep their `0x` prefix and parse to numbers; the pan's extension
//! bundles are typed fields rather than raw strings; the slice, pan, waterfall and stream deltas
//! carry `client_handle` so ownership is decided on the decoded value; `mox` is not decoded; the
//! interlock state is a whole-line sample, never merged; meter definitions keep absent fields
//! absent; the amplifier, tuner (TGXL), APD, GPS, memory and TNF decoders are not ported.
//! Recorded in the repo-root NOTICE (AetherSDR entry).

use super::kv::{self, parse_flag, parse_int, parse_real};
use super::wire::{parse_dec_u32, parse_id, Kvs, Status};

/// Declare a present-only delta: every field optional, plus `merge` (apply a newer delta's
/// reported fields over this one) and `is_empty`.
macro_rules! delta {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            $( $(#[$fmeta:meta])* $field:ident : $ty:ty, )*
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Default, PartialEq)]
        pub struct $name {
            $( $(#[$fmeta])* pub $field: Option<$ty>, )*
        }

        impl $name {
            /// Apply `newer` over this one: each field it reports replaces ours; the rest stay.
            pub fn merge(&mut self, newer: &$name) {
                $( if let Some(v) = &newer.$field { self.$field = Some(v.clone()); } )*
            }

            /// Whether no field is reported.
            pub fn is_empty(&self) -> bool {
                true $( && self.$field.is_none() )*
            }
        }
    };
}

delta! {
    /// A slice (`slice <n>`). Frequencies in MHz, filter edges and offsets in Hz.
    pub struct SliceDelta {
        /// `client_handle`: the client that owns the slice.
        client_handle: u32,
        /// `pan`: the panadapter the slice is on.
        pan: u32,
        /// `index_letter`.
        letter: String,
        /// `RF_frequency`, MHz.
        frequency_mhz: f64,
        mode: String,
        /// `filter_lo`, Hz.
        filter_low: i32,
        /// `filter_hi`, Hz.
        filter_high: i32,
        /// `mode_list`, repeats removed.
        mode_list: Vec<String>,
        active: bool,
        /// `tx`: this is the transmit slice.
        tx: bool,
        /// `rfgain`.
        rf_gain: f64,
        /// `audio_level` (not `audio_gain`).
        audio_level: f64,
        audio_pan: i32,
        audio_mute: bool,
        /// `in_use`: `false` removes the slice.
        in_use: bool,
        /// `lock`.
        locked: bool,
        qsk: bool,
        diversity_child: bool,
        diversity_parent: bool,
        diversity: bool,
        diversity_index: i32,
        /// `esc`: `1`/`on` or `0`/`off`.
        esc: bool,
        esc_gain: f64,
        esc_phase_shift: f64,
        /// `rx_ant_list`, or `ant_list` when the radio sends only that.
        rx_antenna_list: Vec<String>,
        /// `tx_ant_list`.
        tx_antenna_list: Vec<String>,
        /// `rxant`.
        rx_antenna: String,
        /// `txant`.
        tx_antenna: String,
        nb: bool,
        nr: bool,
        anf: bool,
        /// 8000-series platforms only, as are `nrs`, `rnn` and `nrf`.
        nrl: bool,
        nrs: bool,
        rnn: bool,
        nrf: bool,
        anfl: bool,
        anft: bool,
        apf: bool,
        apf_level: i32,
        nb_level: i32,
        nr_level: i32,
        anf_level: i32,
        /// `lms_nr_level`.
        nrl_level: i32,
        /// `speex_nr_level`.
        nrs_level: i32,
        nrf_level: i32,
        /// `lms_anf_level`.
        anfl_level: i32,
        agc_mode: String,
        agc_threshold: i32,
        agc_off_level: i32,
        /// `squelch`.
        squelch: bool,
        squelch_level: i32,
        rit_on: bool,
        rit_freq: i32,
        xit_on: bool,
        xit_freq: i32,
        /// `dax`: the slice's DAX channel.
        dax_channel: i32,
        rtty_mark: i32,
        rtty_shift: i32,
        digl_offset: i32,
        digu_offset: i32,
        /// `record`.
        record: bool,
        /// `play`: three states on the wire (`disabled`, `1`, `0`), kept raw.
        play: String,
        /// `fm_tone_mode`, lowercased.
        fm_tone_mode: String,
        fm_tone_value: f64,
        /// `repeater_offset_dir`, lowercased.
        repeater_offset_dir: String,
        fm_repeater_offset_freq: f64,
        tx_offset_freq: f64,
        fm_deviation: i32,
        step: i32,
        /// `step_list`, raw.
        step_list: String,
    }
}

delta! {
    /// The transmitter's settings (`transmit`). Levels are clamped to the radio's ranges.
    pub struct TransmitDelta {
        /// `rfpower`, 0–100.
        rf_power: i32,
        /// `tunepower`, 0–100.
        tune_power: i32,
        /// `tune`: the radio's tune carrier is on.
        tune: bool,
        /// `freq`, MHz.
        transmit_freq_mhz: f64,
        /// `max_power_level`, unclamped.
        max_power_level: i32,
        tune_mode: String,
        tx_slice_mode: String,
        show_tx_in_waterfall: bool,
        met_in_rx: bool,
        /// `mic_selection`, uppercased.
        mic_selection: String,
        /// `mic_level`, 0–100.
        mic_level: i32,
        mic_acc: bool,
        /// `speech_processor_enable`.
        speech_processor: bool,
        /// `speech_processor_level`, 0–100.
        speech_processor_level: i32,
        /// `compander`, or `dexp` when the line has no `compander` key.
        compander: bool,
        /// `compander_level`, or `noise_gate_level` when the line has no `compander_level` key.
        compander_level: i32,
        /// `dax`: transmit audio from DAX.
        dax: bool,
        sb_monitor: bool,
        mon_gain_sb: i32,
        vox_enable: bool,
        vox_level: i32,
        vox_delay: i32,
        mic_boost: bool,
        mic_bias: bool,
        /// `synccwx`.
        sync_cwx: bool,
        am_carrier_level: i32,
        /// `lo`, the TX filter's low edge, 0–10000 Hz.
        tx_filter_low: i32,
        /// `hi`, the TX filter's high edge, 0–10000 Hz.
        tx_filter_high: i32,
        /// `speed`, 5–100 WPM.
        cw_speed: i32,
        /// `pitch`, 100–6000 Hz.
        cw_pitch: i32,
        /// `break_in`.
        cw_break_in: bool,
        /// `break_in_delay`, 0–2000 ms.
        cw_break_in_delay: i32,
        /// `sidetone`.
        cw_sidetone: bool,
        /// `iambic`.
        cw_iambic: bool,
        /// `iambic_mode`, 0–1.
        cw_iambic_mode: i32,
        /// `swap_paddles`.
        cw_swap_paddles: bool,
        cwl_enabled: bool,
        mon_gain_cw: i32,
        mon_pan_cw: i32,
    }
}

delta! {
    /// The interlock's timing configuration, carried on `interlock` lines beside (or instead of)
    /// the state keys. Not a state sample.
    pub struct InterlockConfig {
        /// `timeout`, ms: the radio unkeys after this long. Read and shown, never raised.
        timeout: i32,
        tx_delay: i32,
        acc_tx_delay: i32,
        tx1_delay: i32,
        tx2_delay: i32,
        tx3_delay: i32,
        acc_txreq_polarity: i32,
        rca_txreq_polarity: i32,
    }
}

/// One whole interlock state sample, from a single line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterlockSample {
    /// `RECEIVE`, `READY`, `NOT_READY`, `PTT_REQUESTED`, `TRANSMITTING`, `UNKEY_REQUESTED`, …
    pub state: String,
    /// What keyed the radio: empty, `SW`, `MIC`, `ACC`, `RCA`.
    pub source: String,
    /// The client assigned the transmitter; `0` means none.
    pub tx_client_handle: u32,
    pub tx_allowed: bool,
    /// Why transmit is not allowed (`OUT_OF_BAND`, …); empty when it is.
    pub reason: String,
}

/// An `interlock` line.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InterlockDelta {
    /// The state sample, when the line carried all five state keys exactly once, well formed.
    pub sample: Option<InterlockSample>,
    /// The line carried at least one state key. The previous sample no longer describes the
    /// radio, whether or not this line supplies a new one.
    pub touches_state: bool,
    /// `tx_client_handle`, when the line carried a well-formed one, sample or not.
    pub tx_client_handle: Option<u32>,
    /// `amplifier`, raw.
    pub amplifier: Option<String>,
    pub config: InterlockConfig,
}

delta! {
    /// The radio's ATU (`atu`).
    pub struct AtuDelta {
        /// `status`, raw (`NONE`, `TUNE_IN_PROGRESS`, `TUNE_SUCCESSFUL`, `TUNE_BYPASS`, …).
        status: String,
        atu_enabled: bool,
        memories_enabled: bool,
        /// `using_mem`.
        using_memory: bool,
    }
}

delta! {
    /// Radio-wide status (`radio`).
    pub struct RadioDelta {
        model: String,
        /// `slices`: how many more slices the radio can create.
        slices_available: i32,
        callsign: String,
        nickname: String,
        region: String,
        radio_options: String,
        /// `bands`, raw.
        bands: String,
        remote_on_enabled: bool,
        /// `mf_enable`.
        multi_flex_enabled: bool,
        /// `enforce_private_ip_connections`.
        enforce_private_ip: bool,
        binaural_rx: bool,
        /// `full_duplex_enabled`.
        full_duplex: bool,
        /// `mute_local_audio_when_remote`.
        mute_local_when_remote: bool,
        auto_save: bool,
        /// `low_latency_digital_modes`.
        low_latency_digital: bool,
        tnf_enabled: bool,
        freq_error_ppb: i32,
        /// `cal_freq`, MHz.
        cal_freq_mhz: f64,
        rtty_mark_default: i32,
        lineout_gain: i32,
        lineout_mute: bool,
        headphone_gain: i32,
        headphone_mute: bool,
        front_speaker_mute: bool,
        daxiq_capacity: i32,
        daxiq_available: i32,
    }
}

delta! {
    /// A panadapter (`display pan <id>`). Frequencies in MHz.
    pub struct PanDelta {
        client_handle: u32,
        /// `waterfall`: the waterfall paired with this pan.
        waterfall: u32,
        /// `center`, MHz.
        center_mhz: f64,
        /// `bandwidth`, MHz.
        bandwidth_mhz: f64,
        /// `x_pixels`: FFT bins per frame.
        x_pixels: i32,
        /// `y_pixels`: FFT bins count rows from the top, so this sets the dBm scale.
        y_pixels: i32,
        min_dbm: f64,
        max_dbm: f64,
        fps: i32,
        /// `rfgain`.
        rf_gain: i32,
        /// `ant_list`.
        antenna_list: Vec<String>,
        /// `rxant`.
        rx_antenna: String,
        /// `wnb`: 0 or 1, anything else dropped.
        wnb: bool,
        /// `wnb_level`: 0–100, anything else dropped (not clamped).
        wnb_level: i32,
        /// `wnb_updating`: 0 or 1.
        wnb_updating: bool,
        wide: bool,
        loopa: bool,
        loopb: bool,
        average: i32,
        weighted_average: bool,
        /// `pre`, raw.
        preamp: String,
        daxiq_channel: i32,
        /// `band_zoom`: 0 or 1, anything else dropped.
        band_zoom: bool,
        /// `segment_zoom`: 0 or 1, anything else dropped.
        segment_zoom: bool,
    }
}

delta! {
    /// A waterfall (`display waterfall <id>`).
    pub struct WaterfallDelta {
        client_handle: u32,
        /// `panadapter`: the pan this waterfall belongs to.
        panadapter: u32,
        /// `line_duration`: a 1–100 rate, slow to fast, not milliseconds.
        line_duration: i32,
        auto_black: bool,
        black_level: i32,
        color_gain: i32,
    }
}

delta! {
    /// A connected client (`client <handle> …`).
    pub struct ClientDelta {
        client_id: String,
        program: String,
        station: String,
        /// The radio's local PTT (mic, footswitch) belongs to this client's station.
        local_ptt: bool,
    }
}

delta! {
    /// A stream (`stream <id>`).
    pub struct StreamDelta {
        /// `type`: `dax_rx`, `dax_tx`, `dax_iq`, `remote_audio_rx`, …
        kind: String,
        client_handle: u32,
        /// `ip`: `0.0.0.0` with owner zero marks a dead orphan.
        ip: String,
        dax_channel: i32,
        daxiq_channel: i32,
        compression: String,
    }
}

/// One meter's definition (`meter <id>.<key>=<value>#…`). Fields the line did not report, or
/// reported malformed, are `None`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeterDef {
    pub index: u16,
    /// `src`: `SLC`, `RAD`, `AMP`, `TX-`, …
    pub source: Option<String>,
    /// `num`: the source's own index (decimal, or `0x` hex).
    pub source_index: Option<u32>,
    /// `nam`: `LEVEL`, `FWDPWR`, `SWR`, `PATEMP`, …
    pub name: Option<String>,
    pub unit: Option<String>,
    /// `low`.
    pub low: Option<f64>,
    /// `hi`.
    pub high: Option<f64>,
    /// `desc`.
    pub description: Option<String>,
    pub fps: Option<i32>,
}

/// A `profile` line: a list or current profile for a type, or the database flags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileDelta {
    /// `tx`, `mic` or `global`; empty for a flags-only line.
    pub kind: String,
    /// `list=`, split on `^` (names may contain spaces).
    pub list: Option<Vec<String>>,
    /// `current=`, trimmed.
    pub current: Option<String>,
    pub importing: Option<bool>,
    pub exporting: Option<bool>,
}

/// What a client line says happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientAction {
    Connected,
    Disconnected,
    /// Neither word: an update to a known client.
    Update,
}

/// A decoded status line. The larger deltas are boxed so the enum stays small.
#[derive(Debug, Clone, PartialEq)]
pub enum Decoded {
    Radio(Box<RadioDelta>),
    /// `removed` is set by `in_use=0` or a trailing `removed`.
    Slice {
        index: u8,
        delta: Box<SliceDelta>,
        removed: bool,
    },
    Pan {
        id: u32,
        delta: Box<PanDelta>,
        removed: bool,
    },
    Waterfall {
        id: u32,
        delta: WaterfallDelta,
        removed: bool,
    },
    Transmit(Box<TransmitDelta>),
    Interlock(InterlockDelta),
    Atu(AtuDelta),
    /// Definitions, in the order the line first named each meter.
    Meters(Vec<MeterDef>),
    MeterRemoved(u16),
    Profile(ProfileDelta),
    Client {
        handle: u32,
        action: ClientAction,
        delta: ClientDelta,
    },
    Stream {
        id: u32,
        delta: StreamDelta,
        removed: bool,
    },
    /// An object this client does not track (`interlock band`, `radio oscillator`, `gps`,
    /// `amplifier`, `tnf`, …), or a line whose object id did not parse.
    Other,
}

/// Decode one status line. Pure.
pub fn decode(status: &Status) -> Decoded {
    let body = status.body.as_str();
    if let Some(rest) = body.strip_prefix("meter ") {
        return decode_meter(rest);
    }
    if let Some(rest) = body.strip_prefix("profile ") {
        return decode_profile_line(rest);
    }
    let kvs = &status.kvs;
    let words: Vec<&str> = status.object.split(' ').filter(|w| !w.is_empty()).collect();
    let trailing_removed = words.last() == Some(&"removed") || kvs.has("removed");
    match words.as_slice() {
        ["radio"] => Decoded::Radio(Box::new(decode_radio(kvs))),
        ["slice", n] | ["slice", n, "removed"] => match parse_dec_u32(n) {
            Some(index) if index <= u32::from(u8::MAX) => {
                let delta = decode_slice(kvs);
                let removed = trailing_removed || delta.in_use == Some(false);
                Decoded::Slice {
                    index: index as u8,
                    delta: Box::new(delta),
                    removed,
                }
            }
            _ => Decoded::Other,
        },
        ["display", "pan", id] | ["display", "pan", id, "removed"] => match parse_id(id) {
            Some(id) => Decoded::Pan {
                id,
                delta: Box::new(decode_pan(kvs)),
                removed: trailing_removed,
            },
            None => Decoded::Other,
        },
        ["display", "waterfall", id] | ["display", "waterfall", id, "removed"] => {
            match parse_id(id) {
                Some(id) => Decoded::Waterfall {
                    id,
                    delta: decode_waterfall(kvs),
                    removed: trailing_removed,
                },
                None => Decoded::Other,
            }
        }
        ["transmit"] => Decoded::Transmit(Box::new(decode_transmit(kvs))),
        ["interlock"] => Decoded::Interlock(decode_interlock(kvs)),
        ["atu"] => Decoded::Atu(decode_atu(kvs)),
        ["client", handle, rest @ ..] if rest.len() <= 1 => match parse_id(handle) {
            Some(handle) => {
                let action = match rest.first() {
                    Some(&"disconnected") => ClientAction::Disconnected,
                    Some(&"connected") => ClientAction::Connected,
                    Some(_) => return Decoded::Other,
                    None if kvs.has("disconnected") => ClientAction::Disconnected,
                    None if kvs.has("connected") => ClientAction::Connected,
                    None => ClientAction::Update,
                };
                Decoded::Client {
                    handle,
                    action,
                    delta: decode_client(kvs),
                }
            }
            None => Decoded::Other,
        },
        ["stream", id] | ["stream", id, "removed"] => match parse_id(id) {
            Some(id) => Decoded::Stream {
                id,
                delta: decode_stream(kvs),
                removed: trailing_removed || kvs.get("in_use") == Some("0"),
            },
            None => Decoded::Other,
        },
        _ => Decoded::Other,
    }
}

/// `slice <n>` key/values.
pub fn decode_slice(kvs: &Kvs) -> SliceDelta {
    let rx_ant_list = kvs.get("rx_ant_list").or_else(|| kvs.get("ant_list"));
    SliceDelta {
        client_handle: kv::id(kvs, "client_handle"),
        pan: kv::id(kvs, "pan"),
        letter: kv::text(kvs, "index_letter"),
        frequency_mhz: kv::real(kvs, "RF_frequency"),
        mode: kv::text(kvs, "mode"),
        filter_low: kv::int(kvs, "filter_lo"),
        filter_high: kv::int(kvs, "filter_hi"),
        mode_list: kvs.get("mode_list").map(kv::unique_list),
        active: kv::flag(kvs, "active"),
        tx: kv::flag(kvs, "tx"),
        rf_gain: kv::real(kvs, "rfgain"),
        audio_level: kv::real(kvs, "audio_level"),
        audio_pan: kv::int(kvs, "audio_pan"),
        audio_mute: kv::flag(kvs, "audio_mute"),
        in_use: kv::flag(kvs, "in_use"),
        locked: kv::flag(kvs, "lock"),
        qsk: kv::flag(kvs, "qsk"),
        diversity_child: kv::flag(kvs, "diversity_child"),
        diversity_parent: kv::flag(kvs, "diversity_parent"),
        diversity: kv::flag(kvs, "diversity"),
        diversity_index: kv::int(kvs, "diversity_index"),
        esc: kvs.get("esc").and_then(|v| match v {
            "1" | "on" => Some(true),
            "0" | "off" => Some(false),
            _ => None,
        }),
        esc_gain: kv::real(kvs, "esc_gain"),
        esc_phase_shift: kv::real(kvs, "esc_phase_shift"),
        rx_antenna_list: rx_ant_list.map(kv::split_list),
        tx_antenna_list: kvs.get("tx_ant_list").map(kv::split_list),
        rx_antenna: kv::text(kvs, "rxant"),
        tx_antenna: kv::text(kvs, "txant"),
        nb: kv::flag(kvs, "nb"),
        nr: kv::flag(kvs, "nr"),
        anf: kv::flag(kvs, "anf"),
        nrl: kv::flag(kvs, "nrl"),
        nrs: kv::flag(kvs, "nrs"),
        rnn: kv::flag(kvs, "rnn"),
        nrf: kv::flag(kvs, "nrf"),
        anfl: kv::flag(kvs, "anfl"),
        anft: kv::flag(kvs, "anft"),
        apf: kv::flag(kvs, "apf"),
        apf_level: kv::int(kvs, "apf_level"),
        nb_level: kv::int(kvs, "nb_level"),
        nr_level: kv::int(kvs, "nr_level"),
        anf_level: kv::int(kvs, "anf_level"),
        nrl_level: kv::int(kvs, "lms_nr_level"),
        nrs_level: kv::int(kvs, "speex_nr_level"),
        nrf_level: kv::int(kvs, "nrf_level"),
        anfl_level: kv::int(kvs, "lms_anf_level"),
        agc_mode: kv::text(kvs, "agc_mode"),
        agc_threshold: kv::int(kvs, "agc_threshold"),
        agc_off_level: kv::int(kvs, "agc_off_level"),
        squelch: kv::flag(kvs, "squelch"),
        squelch_level: kv::int(kvs, "squelch_level"),
        rit_on: kv::flag(kvs, "rit_on"),
        rit_freq: kv::int(kvs, "rit_freq"),
        xit_on: kv::flag(kvs, "xit_on"),
        xit_freq: kv::int(kvs, "xit_freq"),
        dax_channel: kv::int(kvs, "dax"),
        rtty_mark: kv::int(kvs, "rtty_mark"),
        rtty_shift: kv::int(kvs, "rtty_shift"),
        digl_offset: kv::int(kvs, "digl_offset"),
        digu_offset: kv::int(kvs, "digu_offset"),
        record: kv::flag(kvs, "record"),
        play: kv::text(kvs, "play"),
        fm_tone_mode: kvs.get("fm_tone_mode").map(str::to_lowercase),
        fm_tone_value: kv::real(kvs, "fm_tone_value"),
        repeater_offset_dir: kvs.get("repeater_offset_dir").map(str::to_lowercase),
        fm_repeater_offset_freq: kv::real(kvs, "fm_repeater_offset_freq"),
        tx_offset_freq: kv::real(kvs, "tx_offset_freq"),
        fm_deviation: kv::int(kvs, "fm_deviation"),
        step: kv::int(kvs, "step"),
        step_list: kv::text(kvs, "step_list"),
    }
}

/// `transmit` key/values. `mox` is deliberately not read: the keyed state is the interlock's.
pub fn decode_transmit(kvs: &Kvs) -> TransmitDelta {
    // `compander` and `dexp` (and their levels) are aliases on the wire: the radio sends one or
    // the other, so the alias is read only when the main key is absent altogether.
    let compander = if kvs.has("compander") {
        kv::flag(kvs, "compander")
    } else {
        kv::flag(kvs, "dexp")
    };
    let compander_level = if kvs.has("compander_level") {
        kv::clamped(kvs, "compander_level", 0, 100)
    } else {
        kv::clamped(kvs, "noise_gate_level", 0, 100)
    };
    TransmitDelta {
        rf_power: kv::clamped(kvs, "rfpower", 0, 100),
        tune_power: kv::clamped(kvs, "tunepower", 0, 100),
        tune: kv::flag(kvs, "tune"),
        transmit_freq_mhz: kv::real(kvs, "freq"),
        max_power_level: kv::int(kvs, "max_power_level"),
        tune_mode: kv::text(kvs, "tune_mode"),
        tx_slice_mode: kv::text(kvs, "tx_slice_mode"),
        show_tx_in_waterfall: kv::flag(kvs, "show_tx_in_waterfall"),
        met_in_rx: kv::flag(kvs, "met_in_rx"),
        mic_selection: kvs.get("mic_selection").map(str::to_uppercase),
        mic_level: kv::clamped(kvs, "mic_level", 0, 100),
        mic_acc: kv::flag(kvs, "mic_acc"),
        speech_processor: kv::flag(kvs, "speech_processor_enable"),
        speech_processor_level: kv::clamped(kvs, "speech_processor_level", 0, 100),
        compander,
        compander_level,
        dax: kv::flag(kvs, "dax"),
        sb_monitor: kv::flag(kvs, "sb_monitor"),
        mon_gain_sb: kv::clamped(kvs, "mon_gain_sb", 0, 100),
        vox_enable: kv::flag(kvs, "vox_enable"),
        vox_level: kv::clamped(kvs, "vox_level", 0, 100),
        vox_delay: kv::clamped(kvs, "vox_delay", 0, 100),
        mic_boost: kv::flag(kvs, "mic_boost"),
        mic_bias: kv::flag(kvs, "mic_bias"),
        sync_cwx: kv::flag(kvs, "synccwx"),
        am_carrier_level: kv::clamped(kvs, "am_carrier_level", 0, 100),
        tx_filter_low: kv::clamped(kvs, "lo", 0, 10_000),
        tx_filter_high: kv::clamped(kvs, "hi", 0, 10_000),
        cw_speed: kv::clamped(kvs, "speed", 5, 100),
        cw_pitch: kv::clamped(kvs, "pitch", 100, 6000),
        cw_break_in: kv::flag(kvs, "break_in"),
        cw_break_in_delay: kv::clamped(kvs, "break_in_delay", 0, 2000),
        cw_sidetone: kv::flag(kvs, "sidetone"),
        cw_iambic: kv::flag(kvs, "iambic"),
        cw_iambic_mode: kv::clamped(kvs, "iambic_mode", 0, 1),
        cw_swap_paddles: kv::flag(kvs, "swap_paddles"),
        cwl_enabled: kv::flag(kvs, "cwl_enabled"),
        mon_gain_cw: kv::clamped(kvs, "mon_gain_cw", 0, 100),
        mon_pan_cw: kv::clamped(kvs, "mon_pan_cw", 0, 100),
    }
}

/// The five keys that make up an interlock state sample.
const INTERLOCK_STATE_KEYS: [&str; 5] = [
    "state",
    "source",
    "tx_client_handle",
    "tx_allowed",
    "reason",
];

/// `interlock` key/values: a whole-line state sample (or none), the transmitter's owner when
/// reported, and the timing configuration. The sample's rules are the readback's
/// (`super::ptt_evidence`): every state key exactly once, `tx_client_handle` as `0x` and eight hex
/// digits, `tx_allowed` 0 or 1, and no bare words on the line.
pub fn decode_interlock(kvs: &Kvs) -> InterlockDelta {
    let touches_state = INTERLOCK_STATE_KEYS.iter().any(|k| kvs.has(k));
    let handle =
        kvs.get("tx_client_handle")
            .and_then(|h| if h.len() == 10 { parse_id(h) } else { None });
    let allowed = kvs.get("tx_allowed").and_then(parse_flag);
    let well_formed = INTERLOCK_STATE_KEYS.iter().all(|k| kvs.count(k) == 1)
        && kvs.bare_words().next().is_none()
        && handle.is_some()
        && allowed.is_some();
    let sample = if well_formed {
        Some(InterlockSample {
            state: kvs.get("state").unwrap_or_default().to_string(),
            source: kvs.get("source").unwrap_or_default().to_string(),
            tx_client_handle: handle.unwrap_or_default(),
            tx_allowed: allowed.unwrap_or_default(),
            reason: kvs.get("reason").unwrap_or_default().to_string(),
        })
    } else {
        None
    };
    InterlockDelta {
        sample,
        touches_state,
        tx_client_handle: handle,
        amplifier: kv::text(kvs, "amplifier"),
        config: InterlockConfig {
            timeout: kv::int(kvs, "timeout"),
            tx_delay: kv::int(kvs, "tx_delay"),
            acc_tx_delay: kv::int(kvs, "acc_tx_delay"),
            tx1_delay: kv::int(kvs, "tx1_delay"),
            tx2_delay: kv::int(kvs, "tx2_delay"),
            tx3_delay: kv::int(kvs, "tx3_delay"),
            acc_txreq_polarity: kv::int(kvs, "acc_txreq_polarity"),
            rca_txreq_polarity: kv::int(kvs, "rca_txreq_polarity"),
        },
    }
}

/// `atu` key/values. The status token stays raw; whoever shows it owns its vocabulary.
pub fn decode_atu(kvs: &Kvs) -> AtuDelta {
    AtuDelta {
        status: kv::text(kvs, "status"),
        atu_enabled: kv::flag(kvs, "atu_enabled"),
        memories_enabled: kv::flag(kvs, "memories_enabled"),
        using_memory: kv::flag(kvs, "using_mem"),
    }
}

/// `radio` key/values.
pub fn decode_radio(kvs: &Kvs) -> RadioDelta {
    RadioDelta {
        model: kv::text(kvs, "model"),
        slices_available: kv::int(kvs, "slices"),
        callsign: kv::text(kvs, "callsign"),
        nickname: kv::text(kvs, "nickname"),
        region: kv::text(kvs, "region"),
        radio_options: kv::text(kvs, "radio_options"),
        bands: kv::text(kvs, "bands"),
        remote_on_enabled: kv::flag(kvs, "remote_on_enabled"),
        multi_flex_enabled: kv::flag(kvs, "mf_enable"),
        enforce_private_ip: kv::flag(kvs, "enforce_private_ip_connections"),
        binaural_rx: kv::flag(kvs, "binaural_rx"),
        full_duplex: kv::flag(kvs, "full_duplex_enabled"),
        mute_local_when_remote: kv::flag(kvs, "mute_local_audio_when_remote"),
        auto_save: kv::flag(kvs, "auto_save"),
        low_latency_digital: kv::flag(kvs, "low_latency_digital_modes"),
        tnf_enabled: kv::flag(kvs, "tnf_enabled"),
        freq_error_ppb: kv::int(kvs, "freq_error_ppb"),
        cal_freq_mhz: kv::real(kvs, "cal_freq"),
        rtty_mark_default: kv::int(kvs, "rtty_mark_default"),
        lineout_gain: kv::int(kvs, "lineout_gain"),
        lineout_mute: kv::flag(kvs, "lineout_mute"),
        headphone_gain: kv::int(kvs, "headphone_gain"),
        headphone_mute: kv::flag(kvs, "headphone_mute"),
        front_speaker_mute: kv::flag(kvs, "front_speaker_mute"),
        daxiq_capacity: kv::int(kvs, "daxiq_capacity"),
        daxiq_available: kv::int(kvs, "daxiq_available"),
    }
}

/// A 0/1 value in FlexLib's sense for the pan flags (`uint.TryParse`, anything above 1 refused):
/// digits only, at most 1.
fn small_flag(kvs: &Kvs, key: &str) -> Option<bool> {
    kvs.get(key)
        .and_then(parse_dec_u32)
        .filter(|v| *v <= 1)
        .map(|v| v == 1)
}

/// `display pan <id>` key/values.
pub fn decode_pan(kvs: &Kvs) -> PanDelta {
    PanDelta {
        client_handle: kv::id(kvs, "client_handle"),
        waterfall: kv::id(kvs, "waterfall"),
        center_mhz: kv::real(kvs, "center"),
        bandwidth_mhz: kv::real(kvs, "bandwidth"),
        x_pixels: kv::int(kvs, "x_pixels"),
        y_pixels: kv::int(kvs, "y_pixels"),
        min_dbm: kv::real(kvs, "min_dbm"),
        max_dbm: kv::real(kvs, "max_dbm"),
        fps: kv::int(kvs, "fps"),
        rf_gain: kv::int(kvs, "rfgain"),
        antenna_list: kvs.get("ant_list").map(kv::split_list),
        rx_antenna: kv::text(kvs, "rxant"),
        wnb: small_flag(kvs, "wnb"),
        wnb_level: kvs
            .get("wnb_level")
            .and_then(parse_dec_u32)
            .filter(|v| *v <= 100)
            .map(|v| v as i32),
        wnb_updating: small_flag(kvs, "wnb_updating"),
        wide: kv::flag(kvs, "wide"),
        loopa: kv::flag(kvs, "loopa"),
        loopb: kv::flag(kvs, "loopb"),
        average: kv::int(kvs, "average"),
        weighted_average: kv::flag(kvs, "weighted_average"),
        preamp: kv::text(kvs, "pre"),
        daxiq_channel: kv::int(kvs, "daxiq_channel"),
        band_zoom: small_flag(kvs, "band_zoom"),
        segment_zoom: small_flag(kvs, "segment_zoom"),
    }
}

/// `display waterfall <id>` key/values.
pub fn decode_waterfall(kvs: &Kvs) -> WaterfallDelta {
    WaterfallDelta {
        client_handle: kv::id(kvs, "client_handle"),
        panadapter: kv::id(kvs, "panadapter"),
        line_duration: kv::int(kvs, "line_duration"),
        auto_black: kv::flag(kvs, "auto_black"),
        black_level: kv::int(kvs, "black_level"),
        color_gain: kv::int(kvs, "color_gain"),
    }
}

/// `client <handle> …` key/values.
pub fn decode_client(kvs: &Kvs) -> ClientDelta {
    ClientDelta {
        client_id: kv::text(kvs, "client_id"),
        program: kv::text(kvs, "program"),
        station: kv::text(kvs, "station"),
        local_ptt: kv::flag(kvs, "local_ptt"),
    }
}

/// `stream <id>` key/values.
pub fn decode_stream(kvs: &Kvs) -> StreamDelta {
    StreamDelta {
        kind: kv::text(kvs, "type"),
        client_handle: kv::id(kvs, "client_handle"),
        ip: kv::text(kvs, "ip"),
        dax_channel: kv::int(kvs, "dax_channel"),
        daxiq_channel: kv::int(kvs, "daxiq_channel"),
        compression: kv::text(kvs, "compression"),
    }
}

/// The body of a meter line after `meter `: `<id>.<key>=<value>` tokens separated by `#`, or
/// `<id> removed`. Definitions are grouped by meter and returned in first-appearance order, which
/// a TX waveform block depends on (it follows the slice block that gives it context; observed on
/// a FLEX-8400M on 4.2.18, as upstream records).
pub fn decode_meter(body: &str) -> Decoded {
    if body.contains("removed") {
        return match body.split(' ').find(|w| !w.is_empty()).map(parse_dec_u32) {
            Some(Some(index)) if index <= u32::from(u16::MAX) => {
                Decoded::MeterRemoved(index as u16)
            }
            _ => Decoded::Other,
        };
    }
    // (meter, key, value) in wire order.
    let mut fields: Vec<(u16, &str, &str)> = Vec::new();
    let mut order: Vec<u16> = Vec::new();
    for token in body.split('#').filter(|t| !t.is_empty()) {
        let Some(dot) = token.find('.') else { continue };
        let Some(eq) = token[dot..].find('=').map(|e| dot + e) else {
            continue;
        };
        let Some(index) = parse_dec_u32(&token[..dot]).filter(|i| *i <= u32::from(u16::MAX)) else {
            continue;
        };
        let index = index as u16;
        if !order.contains(&index) {
            order.push(index);
        }
        fields.push((index, &token[dot + 1..eq], &token[eq + 1..]));
    }
    let defs = order
        .into_iter()
        .map(|index| {
            // A key given twice for the same meter is ambiguous, as anywhere else.
            let value = |key: &str| {
                let mut found = fields.iter().filter(|(i, k, _)| *i == index && *k == key);
                match (found.next(), found.next()) {
                    (Some((_, _, v)), None) => Some(*v),
                    _ => None,
                }
            };
            MeterDef {
                index,
                source: value("src").map(str::to_string),
                source_index: value("num").and_then(|v| parse_dec_u32(v).or_else(|| parse_id(v))),
                name: value("nam").map(str::to_string),
                unit: value("unit").map(str::to_string),
                low: value("low").and_then(parse_real),
                high: value("hi").and_then(parse_real),
                description: value("desc").map(str::to_string),
                fps: value("fps").and_then(parse_int),
            }
        })
        .collect();
    Decoded::Meters(defs)
}

/// The body of a profile line after `profile `: `<type> list=…` / `<type> current=…` (values with
/// spaces, so parsed by hand), or the space-free database flags (`importing=1`).
fn decode_profile_line(rest: &str) -> Decoded {
    let first = rest.split(' ').next().unwrap_or_default();
    if first.contains('=') {
        let kvs = Kvs::parse(rest);
        let delta = ProfileDelta {
            importing: kv::flag(&kvs, "importing"),
            exporting: kv::flag(&kvs, "exporting"),
            ..ProfileDelta::default()
        };
        return if delta.importing.is_some() || delta.exporting.is_some() {
            Decoded::Profile(delta)
        } else {
            Decoded::Other
        };
    }
    let body = rest[first.len()..].trim_start();
    match decode_profile(first, body) {
        Some(delta) => Decoded::Profile(delta),
        None => Decoded::Other,
    }
}

/// One profile type's `key=value`, where the value may hold spaces (`current=Default FHM-1`).
/// The database flags can ride the same line for any type. `None` for any other key.
pub fn decode_profile(kind: &str, body: &str) -> Option<ProfileDelta> {
    let eq = body.find('=')?;
    let key = body[..eq].trim();
    let value = body[eq + 1..].trim();
    let mut delta = ProfileDelta::default();
    match key {
        "importing" => delta.importing = Some(parse_flag(value)?),
        "exporting" => delta.exporting = Some(parse_flag(value)?),
        "list" => {
            delta.kind = kind.to_string();
            delta.list = Some(
                value
                    .split('^')
                    .filter(|n| !n.is_empty())
                    .map(str::to_string)
                    .collect(),
            );
        }
        "current" => {
            delta.kind = kind.to_string();
            delta.current = Some(value.to_string());
        }
        _ => return None,
    }
    Some(delta)
}

#[cfg(test)]
mod tests {
    //! Translated from upstream's decode tests: `aetherd_slice_decode_test.cpp`,
    //! `aetherd_radio_decode_test.cpp`, `aetherd_meter_decode_test.cpp`,
    //! `aetherd_pan_decode_test.cpp` (the decode facets; the model-sink facets test upstream's
    //! models, which are not ported), `aetherd_transmit_decode_test.cpp` (core, interlock and ATU;
    //! APD is not ported) and the profile part of `aetherd_residual_decode_test.cpp`. Each test
    //! names the upstream block it translates.
    use super::*;
    use crate::flex::wire::{parse_line, Line};

    fn status(line: &str) -> Status {
        match parse_line(line) {
            Ok(Line::Status(s)) => s,
            other => panic!("{line:?}: {other:?}"),
        }
    }

    fn kvs(text: &str) -> Kvs {
        Kvs::parse(text)
    }

    #[test]
    fn slice_key_renames_and_typed_values() {
        // aetherd_slice_decode_test: "key renames + typed values".
        let d = decode_slice(&kvs(
            "RF_frequency=14.25 mode=USB filter_lo=-2700 filter_hi=0 index_letter=A dax=2 \
             audio_level=60 rfgain=-5",
        ));
        assert_eq!(d.frequency_mhz, Some(14.25));
        assert_eq!(d.mode.as_deref(), Some("USB"));
        assert_eq!(d.filter_low, Some(-2700));
        assert_eq!(d.filter_high, Some(0));
        assert_eq!(d.letter.as_deref(), Some("A"));
        assert_eq!(d.dax_channel, Some(2));
        assert_eq!(d.audio_level, Some(60.0));
        assert_eq!(d.rf_gain, Some(-5.0));
        assert_eq!(d.qsk, None, "not on the wire: absent");
        assert_eq!(d.tx, None);
    }

    #[test]
    fn slice_flags_and_absence() {
        // "1"→bool; absent → disengaged.
        let d = decode_slice(&kvs("active=1 tx=0 lock=1"));
        assert_eq!(d.active, Some(true));
        assert_eq!(d.tx, Some(false));
        assert_eq!(d.locked, Some(true));
        assert_eq!(d.qsk, None);
        // Stricter than upstream: a flag that is neither 0 nor 1 is not read as false.
        assert_eq!(decode_slice(&kvs("tx=yes")).tx, None);
    }

    #[test]
    fn a_malformed_present_slice_field_is_dropped_not_zeroed() {
        // "ok-guarded numeric parse": a garbled RF_frequency must not retune to 0 Hz.
        let d = decode_slice(&kvs("RF_frequency=garbage filter_lo=notanint mode=LSB"));
        assert_eq!(d.frequency_mhz, None);
        assert_eq!(d.filter_low, None);
        assert_eq!(d.mode.as_deref(), Some("LSB"));
    }

    #[test]
    fn slice_esc_lists_and_normalisation() {
        // "esc 1/on → true, 0 → false".
        assert_eq!(decode_slice(&kvs("esc=on")).esc, Some(true));
        assert_eq!(decode_slice(&kvs("esc=1")).esc, Some(true));
        assert_eq!(decode_slice(&kvs("esc=0")).esc, Some(false));
        assert_eq!(decode_slice(&kvs("esc=maybe")).esc, None);
        // "antenna lists: rx_ant_list precedence, split+trim; mode_list de-dupe".
        let d = decode_slice(&kvs(
            "rx_ant_list=ANT1,RX_A,RX_B ant_list=SHOULD_BE_IGNORED \
             mode_list=USB,LSB,DSTR,DSTR,DSTR,CW",
        ));
        assert_eq!(d.rx_antenna_list.unwrap(), ["ANT1", "RX_A", "RX_B"]);
        assert_eq!(d.mode_list.unwrap(), ["USB", "LSB", "DSTR", "CW"]);
        assert_eq!(
            decode_slice(&kvs("ant_list=ANT1,ANT2"))
                .rx_antenna_list
                .unwrap(),
            ["ANT1", "ANT2"]
        );
        // "lowercase normalization; play/step_list carried raw".
        let d = decode_slice(&kvs(
            "fm_tone_mode=CTCSS play=disabled step_list=10,100,1000",
        ));
        assert_eq!(d.fm_tone_mode.as_deref(), Some("ctcss"));
        assert_eq!(d.play.as_deref(), Some("disabled"));
        assert_eq!(d.step_list.as_deref(), Some("10,100,1000"));
    }

    #[test]
    fn radio_key_renames_and_ok_guard() {
        // aetherd_radio_decode_test: "key renames + typed values + 1→bool".
        let d = decode_radio(&kvs(
            "model=FLEX-8600 slices=4 callsign=KK7GWY mf_enable=1 full_duplex_enabled=0 \
             cal_freq=10.0 rtty_mark_default=2295 lineout_gain=55",
        ));
        assert_eq!(d.model.as_deref(), Some("FLEX-8600"));
        assert_eq!(d.slices_available, Some(4));
        assert_eq!(d.callsign.as_deref(), Some("KK7GWY"));
        assert_eq!(d.multi_flex_enabled, Some(true));
        assert_eq!(d.full_duplex, Some(false));
        assert_eq!(d.cal_freq_mhz, Some(10.0));
        assert_eq!(d.rtty_mark_default, Some(2295));
        assert_eq!(d.lineout_gain, Some(55));
        assert_eq!(d.nickname, None);
        // "ok-guard: malformed present numeric dropped".
        let d = decode_radio(&kvs("slices=junk cal_freq=nope region=US"));
        assert_eq!(d.slices_available, None);
        assert_eq!(d.cal_freq_mhz, None);
        assert_eq!(d.region.as_deref(), Some("US"));
    }

    fn meters(body: &str) -> Vec<MeterDef> {
        match decode_meter(body) {
            Decoded::Meters(defs) => defs,
            other => panic!("{body:?}: {other:?}"),
        }
    }

    #[test]
    fn a_full_meter_definition_and_a_removal() {
        // aetherd_meter_decode_test: "full definition" and "removal".
        let defs = meters("7.src=SLC#7.num=0#7.nam=LEVEL#7.unit=dBm#7.low=-150.0#7.hi=20.0");
        assert_eq!(defs.len(), 1);
        let d = &defs[0];
        assert_eq!(d.index, 7);
        assert_eq!(d.source.as_deref(), Some("SLC"));
        assert_eq!(d.source_index, Some(0));
        assert_eq!(d.name.as_deref(), Some("LEVEL"));
        assert_eq!(d.unit.as_deref(), Some("dBm"));
        assert_eq!(d.low, Some(-150.0));
        assert_eq!(d.high, Some(20.0));
        assert_eq!(d.description, None);
        assert_eq!(decode_meter("7 removed"), Decoded::MeterRemoved(7));
        assert_eq!(
            decode(&status("S0|meter 7 removed")),
            Decoded::MeterRemoved(7)
        );
    }

    #[test]
    fn meters_keep_first_appearance_order() {
        // "multiple meters in one body" and the FLEX-8400M ordering block: a lower, reused TX
        // meter id must not move ahead of the slice block that gives it context.
        let defs = meters("1.src=TX#1.nam=FWDPWR#1.unit=Watts#2.src=TX#2.nam=SWR#2.unit=SWR");
        assert_eq!(defs.len(), 2);
        assert_eq!(
            (defs[0].index, defs[0].name.as_deref()),
            (1, Some("FWDPWR"))
        );
        assert_eq!((defs[1].index, defs[1].name.as_deref()), (2, Some("SWR")));
        let defs = meters(
            "50.src=SLC#50.num=0#50.nam=LEVEL#50.unit=dBm#\
             20.src=TX-#20.num=0#20.nam=ALC#20.unit=dBFS#\
             70.src=SLC#70.num=1#70.nam=LEVEL#70.unit=dBm#\
             40.src=TX-#40.num=9#40.nam=ALC#40.unit=dBFS#20.desc=first-slice",
        );
        let order: Vec<u16> = defs.iter().map(|d| d.index).collect();
        assert_eq!(order, [50, 20, 70, 40]);
        assert_eq!(defs[1].description.as_deref(), Some("first-slice"));
    }

    #[test]
    fn malformed_meter_tokens() {
        // "malformed index token skipped, no emission".
        assert!(meters("x.src=SLC#x.nam=LEVEL").is_empty());
        // "malformed numerics": upstream leaves its 0 default; here a malformed field is absent,
        // which is the point the upstream note says its own test could not pin.
        let defs = meters("3.nam=LEVEL#3.low=junk#3.hi=20.0#3.num=nope");
        let d = &defs[0];
        assert_eq!(d.name.as_deref(), Some("LEVEL"));
        assert_eq!(d.high, Some(20.0));
        assert_eq!(d.low, None);
        assert_eq!(d.source_index, None);
        // A key given twice for one meter is ambiguous.
        assert_eq!(meters("4.nam=A#4.nam=B")[0].name, None);
        // `num` may be a hex id.
        assert_eq!(
            meters("5.num=0x40000000")[0].source_index,
            Some(0x4000_0000)
        );
    }

    fn pan(line: &str) -> PanDelta {
        match decode(&status(line)) {
            Decoded::Pan { delta, .. } => *delta,
            other => panic!("{line:?}: {other:?}"),
        }
    }

    #[test]
    fn pan_center_bandwidth_and_range() {
        // aetherd_pan_decode_test facets 1 and 1b. Upstream signals "unchanged" with -1 or NaN;
        // here an absent field is None.
        let d = pan("S0|display pan 0x40000000 min_dbm=-120");
        assert_eq!((d.center_mhz, d.bandwidth_mhz), (None, None));
        let d = pan("S0|display pan 0x40000000 center=7.15");
        assert_eq!((d.center_mhz, d.bandwidth_mhz), (Some(7.15), None));
        let d = pan("S0|display pan 0x40000000 center=14.2 bandwidth=0.2");
        assert_eq!((d.center_mhz, d.bandwidth_mhz), (Some(14.2), Some(0.2)));
        let d = pan("S0|display pan 0x40000000 min_dbm=-130");
        assert_eq!((d.min_dbm, d.max_dbm), (Some(-130.0), None));
        let d = pan("S0|display pan 0x40000000 min_dbm=-125 max_dbm=-40");
        assert_eq!((d.min_dbm, d.max_dbm), (Some(-125.0), Some(-40.0)));
        // A malformed min_dbm is dropped, never 0 dBm (which would collapse the scale).
        let d = pan("S0|display pan 0x40000000 min_dbm=junk max_dbm=-40");
        assert_eq!((d.min_dbm, d.max_dbm), (None, Some(-40.0)));
        // Stricter than upstream, which parsed center and bandwidth without a guard.
        let d = pan("S0|display pan 0x40000000 center=junk bandwidth=0.2");
        assert_eq!((d.center_mhz, d.bandwidth_mhz), (None, Some(0.2)));
    }

    #[test]
    fn pan_wideband_noise_blanker_guards() {
        // Facet 2: wnb and wnb_updating are 0 or 1; wnb_level is 0–100; anything else is dropped,
        // never coerced or clamped (FlexLib Panadapter.cs, as upstream cites it).
        let d = pan("S0|display pan 0x40000000 wnb=1 wnb_level=42");
        assert_eq!((d.wnb, d.wnb_level), (Some(true), Some(42)));
        let d = pan("S0|display pan 0x40000000 wnb=1 wnb_level=garbage");
        assert_eq!((d.wnb, d.wnb_level), (Some(true), None));
        let d = pan("S0|display pan 0x40000000 wnb=bogus wnb_updating=5 wnb_level=150");
        assert_eq!((d.wnb, d.wnb_updating, d.wnb_level), (None, None, None));
        let d = pan("S0|display pan 0x40000000 wnb=2 wnb_level=-5 wnb_updating=1");
        assert_eq!(
            (d.wnb, d.wnb_level, d.wnb_updating),
            (None, None, Some(true))
        );
        let d = pan("S0|display pan 0x40000000 wnb=0 wnb_level=100");
        assert_eq!((d.wnb, d.wnb_level), (Some(false), Some(100)));
    }

    #[test]
    fn pan_gain_antenna_state_and_zoom() {
        // Facet 1c: a signed gain survives; a malformed one is dropped.
        assert_eq!(pan("S0|display pan 0x40000000 rfgain=-8").rf_gain, Some(-8));
        assert_eq!(pan("S0|display pan 0x40000000 rfgain=nope").rf_gain, None);
        let d = pan("S0|display pan 0x40000000 rxant=ANT2 ant_list=ANT1,ANT2,RX_A");
        assert_eq!(d.rx_antenna.as_deref(), Some("ANT2"));
        assert_eq!(d.antenna_list.unwrap().len(), 3);
        // Facet 2b: the state keys, the owner and the paired waterfall; an absent fps stays absent.
        let d = pan(
            "S0|display pan 0x40000000 wide=1 loopa=1 daxiq_channel=3 client_handle=0x5C0FFEE0 \
             waterfall=0x42000000",
        );
        assert_eq!(d.wide, Some(true));
        assert_eq!(d.loopa, Some(true));
        assert_eq!(d.daxiq_channel, Some(3));
        assert_eq!(d.client_handle, Some(0x5C0F_FEE0));
        assert_eq!(d.waterfall, Some(0x4200_0000));
        assert_eq!(d.fps, None);
        // Facet 2c: the zoom flags take 0 or 1 only.
        let d = pan("S0|display pan 0x40000000 band_zoom=1 segment_zoom=0");
        assert_eq!((d.band_zoom, d.segment_zoom), (Some(true), Some(false)));
        let d = pan("S0|display pan 0x40000000 band_zoom=2 segment_zoom=nope");
        assert_eq!((d.band_zoom, d.segment_zoom), (None, None));
        assert_eq!(
            pan("S0|display pan 0x40000000 segment_zoom=-1").segment_zoom,
            None
        );
    }

    #[test]
    fn waterfall_line_duration_guard() {
        // Facet 1d: a malformed line_duration is dropped, not applied as 0.
        let wf = |line: &str| match decode(&status(line)) {
            Decoded::Waterfall { delta, .. } => delta,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            wf("S0|display waterfall 0x42000000 line_duration=100").line_duration,
            Some(100)
        );
        assert_eq!(
            wf("S0|display waterfall 0x42000000 line_duration=nope").line_duration,
            None
        );
        let d = wf(
            "S2B6E1F40|display waterfall 0x42000000 client_handle=0x2B6E1F40 panadapter=0x40000000 \
             line_duration=80 auto_black=1 black_level=15 color_gain=50",
        );
        assert_eq!(d.panadapter, Some(0x4000_0000));
        assert_eq!(d.client_handle, Some(0x2B6E_1F40));
        assert_eq!(d.auto_black, Some(true));
    }

    #[test]
    fn transmit_clamps_flags_and_guards() {
        // aetherd_transmit_decode_test: "core transmit: clamp, 1→bool, ok-guard".
        let d = decode_transmit(&kvs(
            "rfpower=150 tune=1 freq=14.2 mic_selection=acc max_power_level=500 iambic_mode=9",
        ));
        assert_eq!(d.rf_power, Some(100));
        assert_eq!(d.tune, Some(true));
        assert_eq!(d.transmit_freq_mhz, Some(14.2));
        assert_eq!(d.mic_selection.as_deref(), Some("ACC"));
        assert_eq!(d.max_power_level, Some(500));
        assert_eq!(d.cw_iambic_mode, Some(1));
        // "ok-guard: malformed present numeric is dropped". Upstream's block also reads mox=1;
        // here mox is not decoded at all, so this line carries nothing.
        let d = decode_transmit(&kvs("rfpower=junk mox=1"));
        assert_eq!(d, TransmitDelta::default());
    }

    #[test]
    fn transmit_compander_and_dexp_alias() {
        // compander present → carried; dexp ignored.
        let d = decode_transmit(&kvs(
            "compander=1 dexp=0 compander_level=33 noise_gate_level=9",
        ));
        assert_eq!((d.compander, d.compander_level), (Some(true), Some(33)));
        // dexp only → aliases into compander.
        let d = decode_transmit(&kvs("dexp=1 noise_gate_level=8"));
        assert_eq!((d.compander, d.compander_level), (Some(true), Some(8)));
    }

    #[test]
    fn interlock_configuration_and_atu() {
        // "interlock": the timing keys; nothing of the transmit plane.
        let d = decode_interlock(&kvs("tx1_delay=25 timeout=120"));
        assert_eq!(d.config.tx1_delay, Some(25));
        assert_eq!(d.config.timeout, Some(120));
        assert!(!d.touches_state);
        assert_eq!(d.sample, None);
        // "ATU: raw status token carried".
        let d = decode_atu(&kvs("status=TUNE_SUCCESSFUL atu_enabled=1"));
        assert_eq!(d.status.as_deref(), Some("TUNE_SUCCESSFUL"));
        assert_eq!(d.atu_enabled, Some(true));
    }

    #[test]
    fn an_interlock_sample_is_one_whole_line() {
        let idle = decode_interlock(&kvs(
            "tx_client_handle=0x00000000 state=READY reason= source= tx_allowed=1 amplifier=",
        ));
        assert_eq!(
            idle.sample,
            Some(InterlockSample {
                state: "READY".into(),
                source: String::new(),
                tx_client_handle: 0,
                tx_allowed: true,
                reason: String::new(),
            })
        );
        assert!(idle.touches_state);
        assert_eq!(idle.tx_client_handle, Some(0));
        // A partial line touches the state but is no sample: deltas are never merged.
        for partial in [
            "tx_allowed=0",
            "state=READY",
            "source=",
            "reason=",
            "tx_client_handle=0x00000000",
        ] {
            let d = decode_interlock(&kvs(partial));
            assert!(d.touches_state, "{partial}");
            assert_eq!(d.sample, None, "{partial}");
        }
        // Malformed, repeated or extra-word lines are no sample either.
        for bad in [
            "tx_client_handle=garbage state=READY reason= source= tx_allowed=1",
            "tx_client_handle=0x0 state=READY reason= source= tx_allowed=1",
            "tx_client_handle=0x00000000 state=READY state=TRANSMITTING reason= source= tx_allowed=1",
            "tx_client_handle=0x00000000 state=READY reason= source= SW tx_allowed=1",
            "tx_client_handle=0x00000000 state=READY reason= source= tx_allowed=yes",
        ] {
            let d = decode_interlock(&kvs(bad));
            assert!(d.touches_state, "{bad}");
            assert_eq!(d.sample, None, "{bad}");
        }
        // `interlock band <n>` is configuration and is not decoded as the interlock.
        assert_eq!(
            decode(&status(
                "S0|interlock band 24 band_name=GEN acc_tx_enabled=0"
            )),
            Decoded::Other
        );
    }

    #[test]
    fn profiles() {
        // aetherd_residual_decode_test, the profile blocks.
        let d = decode_profile("tx", "list=Default^Default FHM-1^Default FHM-1 DX").unwrap();
        assert_eq!(d.kind, "tx");
        assert_eq!(
            d.list.unwrap(),
            ["Default", "Default FHM-1", "Default FHM-1 DX"]
        );
        assert_eq!(d.current, None);
        let d = decode_profile("mic", "current= Default FHM-1 ").unwrap();
        assert_eq!(
            (d.kind.as_str(), d.current.as_deref()),
            ("mic", Some("Default FHM-1"))
        );
        assert_eq!(d.list, None);
        let d = decode_profile("global", "importing=1").unwrap();
        assert_eq!(d.importing, Some(true));
        assert_eq!((d.list, d.current), (None, None));
        assert_eq!(decode_profile("tx", "bogus=1"), None);
        // Through the router: the type form and the flags-only fallback.
        match decode(&status("S0|profile tx list=A^B C")) {
            Decoded::Profile(d) => assert_eq!(d.list.unwrap(), ["A", "B C"]),
            other => panic!("{other:?}"),
        }
        match decode(&status("S0|profile exporting=1")) {
            Decoded::Profile(d) => assert_eq!(d.exporting, Some(true)),
            other => panic!("{other:?}"),
        }
        assert_eq!(decode(&status("S0|profile unrelated=x")), Decoded::Other);
    }

    #[test]
    fn the_router_reads_objects_and_removals() {
        match decode(&status("S0|slice 3 in_use=0")) {
            Decoded::Slice { index, removed, .. } => assert_eq!((index, removed), (3, true)),
            other => panic!("{other:?}"),
        }
        match decode(&status("S0|display pan 0x40000001 removed")) {
            Decoded::Pan { id, removed, .. } => assert_eq!((id, removed), (0x4000_0001, true)),
            other => panic!("{other:?}"),
        }
        match decode(&status("S0|stream 0x04000001 removed")) {
            Decoded::Stream { id, removed, .. } => assert_eq!((id, removed), (0x0400_0001, true)),
            other => panic!("{other:?}"),
        }
        match decode(&status(
            "S7A3C0001|client 0x7A3C0001 connected local_ptt=1 program=SmartSDR-Win station=Shack",
        )) {
            Decoded::Client {
                handle,
                action,
                delta,
            } => {
                assert_eq!((handle, action), (0x7A3C_0001, ClientAction::Connected));
                assert_eq!(delta.local_ptt, Some(true));
                assert_eq!(delta.station.as_deref(), Some("Shack"));
            }
            other => panic!("{other:?}"),
        }
        match decode(&status("S0|client 0x7A3C0001 disconnected forced=0")) {
            Decoded::Client { action, .. } => assert_eq!(action, ClientAction::Disconnected),
            other => panic!("{other:?}"),
        }
        // An id without its 0x prefix, or a slice index that is not a number, is not routed.
        assert_eq!(
            decode(&status("S0|display pan 40000000 center=1")),
            Decoded::Other
        );
        assert_eq!(decode(&status("S0|slice x in_use=1")), Decoded::Other);
        assert_eq!(decode(&status("S0|slice 300 in_use=1")), Decoded::Other);
        assert_eq!(
            decode(&status("S0|radio oscillator state=tcxo locked=1")),
            Decoded::Other
        );
        assert_eq!(
            decode(&status("S0|atu 0x00000001 status=x")),
            Decoded::Other
        );
    }
}
