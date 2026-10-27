//! What differs between TCI servers, as data: one [`Dialect`] per server Nexus targets.
//!
//! Every field is a behaviour a client can observe on the wire. The profiles are built from the
//! evidence below and say where that evidence stops; a field marked UNVERIFIED is
//! the profile's choice until a tester's recording (`tcirecord`) or a bench settles it.
//! A test that depends on one such choice should set the
//! field itself rather than rely on the profile.
//!
//! | Profile | Evidence |
//! |---|---|
//! | [`Dialect::expert_sdr3`] | the TCI 2.0 spec (Expert Electronics, 12 Jan 2024, MIT) |
//! | [`Dialect::thetis`] | the plan's notes on Thetis 2.10.3.13/14 (release notes, forum); its code was not read |
//! | [`Dialect::pihpsdr`] | piHPSDR's TCI server at `3b63530267` (`src/tci.c`), read for behaviour |
//! | [`Dialect::expert_sdr2`] | how WSJT-X 3.0.2's client treats a server that is neither ExpertSDR3 nor Thetis |
//! | [`Dialect::aether_sdr`] | AetherSDR at `05c5b805`: its TCI notes (`docs/architecture/tci-*.md`) and init burst |
//!
//! The profiles are not copies of those programs' code: each is a short list of facts about what
//! they send and when, written for the simulator.

use std::time::Duration;

use crate::frame::SampleType;

/// The identity lines an init burst opens with (spec §4.1), so a profile can give their order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ident {
    VfoLimits,
    IfLimits,
    TrxCount,
    ChannelCount,
    Device,
    ReceiveOnly,
    Modulations,
    Protocol,
}

/// How a server spells the channel count. The spec (§4.1) says `CHANNEL_COUNT`; WSJT-X's client
/// and the eesdr-tci Python client, both written against ExpertSDR, read only `channels_count`,
/// and piHPSDR and AetherSDR send that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spelling {
    /// `channel_count:<n>;`, as the spec writes it.
    Singular,
    /// `channels_count:<n>;`, as servers send it.
    Plural,
    /// Both lines.
    Both,
}

/// How `drive` is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveForm {
    /// `drive:<trx>,<0-100>;`, read `drive:<trx>;` (TCI 2.0).
    PerTrx,
    /// `drive:<0-100>;`, read `drive;` (the 1.x form WSJT-X parses for other servers).
    Bare,
}

/// How a server pushes the receive level once `rx_sensors_enable:true` is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorForm {
    /// `rx_channel_sensors:<trx>,<ch>,<dBm>;` (TCI 2.0).
    Channel,
    /// `rx_sensors:<trx>,<dBm>;` (1.9, deprecated in 2.0).
    Receiver,
}

/// Whose `trx` the server obeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    /// None (the spec): any client's `trx:<n>,false;` unkeys, whoever keyed.
    None,
    /// The first client to key owns the transmitter until it unkeys, sends `stop;` or leaves; any
    /// other client's `trx` and `tune` are answered with the actual state and do nothing
    /// (piHPSDR). A key left up by an owner that closed cleanly has no owner, and then a
    /// `trx:<n>,false;` alone does not unkey it either: only a client that keys first can.
    FirstKeyer,
    /// Only the client that keyed may release; anyone else's `trx` is answered with the actual
    /// state (AetherSDR's routing contract).
    Initiator,
}

/// What a `trx:<n>,true;` keys from (spec §4.2, TRX).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    /// TCI 2.0: the third argument names the source; `tci` takes the client's audio stream,
    /// absent takes the selected microphone.
    Argument,
    /// Before 2.0 there was no source argument: the key takes the client's audio stream when it is
    /// running and the microphone otherwise. Any third argument is ignored.
    RunningAudio,
}

/// How many values a TX_CHRONO asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChronoLength {
    /// A fixed `length`, whatever the client set (AetherSDR: 2048).
    Fixed(u32),
    /// The client's `audio_stream_samples` (or its rate's default): what the spec's TX_CHRONO
    /// paragraph reads as.
    ClientSamples,
}

/// How a server keeps the spots its clients send (spec §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotStore {
    /// As the spec reads: one spot per callsign (a new `spot` for a call replaces it),
    /// `spot_delete:<call>;` takes it off, `spot_clear;` takes off every spot, and the colour is a
    /// decimal ARGB, drawn as sent. UNVERIFIED on ExpertSDR3, ExpertSDR2 and Thetis.
    PerCall,
    /// AetherSDR's (`src/core/TciProtocol.cpp` and `SpectrumWidget.cpp` at `8394342`): every `spot`
    /// is a new spot, whatever its call; `spot_delete` needs the call and the frequency
    /// (`spot_delete:<call>,<hz>;`) and takes off the first spot of that call within 100 Hz of it;
    /// `spot_clear;` takes off every spot, from every source; the colour is kept as text and drawn
    /// only when it starts with `#`. Its 30-minute lifetime is not modelled.
    Appended,
}

/// Which spelling a spot click is reported in (spec §4.4); a server may send both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickForm {
    /// `clicked_on_spot:<call>,<hz>;` (legacy).
    Legacy,
    /// `rx_clicked_on_spot:<trx>,<ch>,<call>,<hz>;`.
    Receiver,
}

/// A client's audio stream settings (spec §4.3), and the defaults a server starts a client with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    pub rate: u32,
    pub sample_type: SampleType,
    pub channels: u32,
    /// Set by `audio_stream_samples`; `None` keeps each rate's default ([`default_samples`]).
    pub samples: Option<u32>,
}

impl AudioFormat {
    /// The spec's defaults: 48 kHz, float32, two channels, each rate's sample count.
    pub const SPEC: AudioFormat = AudioFormat {
        rate: 48_000,
        sample_type: SampleType::Float32,
        channels: 2,
        samples: None,
    };

    /// The `length` of one receive frame at these settings.
    pub fn samples(&self) -> u32 {
        self.samples.unwrap_or_else(|| default_samples(self.rate))
    }
}

/// The spec's default sample count per frame for an audio rate (§4.3, AUDIO_STREAM_SAMPLES).
pub fn default_samples(rate: u32) -> u32 {
    match rate {
        8_000 => 256,
        12_000 => 512,
        24_000 => 1024,
        _ => 2048,
    }
}

/// One transceiver's settings as a server comes up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrxInit {
    /// VFO A and B, Hz.
    pub vfo: [u64; 2],
    /// The panorama's centre, Hz.
    pub dds: u64,
    /// The IF of channels A and B within the panorama, Hz.
    pub if_hz: [i64; 2],
    pub modulation: String,
    /// The receive filter's edges, Hz.
    pub filter: (i64, i64),
    pub drive: u32,
    pub tune_drive: u32,
    pub split: bool,
    pub rit: bool,
    pub rit_offset: i64,
    pub xit: bool,
    pub xit_offset: i64,
    pub tx_enable: bool,
}

impl TrxInit {
    /// A transceiver on `hz` in `modulation`, with the rest at plain values.
    pub fn at(hz: u64, modulation: &str) -> TrxInit {
        TrxInit {
            vfo: [hz, hz],
            dds: hz + 6_000,
            if_hz: [-6_000, -6_000],
            modulation: modulation.to_string(),
            filter: (200, 2_900),
            drive: 50,
            tune_drive: 10,
            split: false,
            rit: false,
            rit_offset: 0,
            xit: false,
            xit_offset: 0,
            tx_enable: true,
        }
    }
}

/// The radio as the server comes up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Initial {
    /// One per transceiver; the dialect's `trx_count` is how many are announced, and any missing
    /// ones copy the first.
    pub trx: Vec<TrxInit>,
    /// The SDR is running (`start;`) rather than stopped.
    pub running: bool,
    pub cw_speed: u32,
    pub cw_delay: u32,
    /// Further state lines, sent in the burst as written and answered when read. Each is one
    /// command with its `;`, of a kind the simulator does not model (volume, AGC, noise blanker…).
    pub extra: Vec<String>,
}

/// A server's dialect: what it announces, how it orders and groups its burst, and how it behaves.
#[derive(Debug, Clone, PartialEq)]
pub struct Dialect {
    /// What the profile stands in for, for messages and the report.
    pub name: String,

    // --- What the init burst says (spec §4.1) ---
    /// `protocol:<program>,<version>;`.
    pub program: String,
    pub version: String,
    /// `device:<name>;`.
    pub device: String,
    pub trx_count: u32,
    pub channels: u32,
    pub channel_count: Spelling,
    pub vfo_limits: (u64, u64),
    pub if_limits: (i64, i64),
    /// The modulation names, spelled as the server spells them.
    pub modulations: Vec<String>,
    pub receive_only: bool,

    // --- How the burst is sent ---
    /// The identity lines, in the order this server sends them.
    pub identity_order: Vec<Ident>,
    /// The whole burst in one WebSocket message (AetherSDR), or one message per command.
    pub burst_in_one_message: bool,
    /// The default stream settings (`audio_samplerate` …) in the burst, before `ready;`.
    pub burst_stream_settings: bool,
    /// `start;` (or `stop;`) right after `ready;` rather than among the state lines.
    pub run_state_after_ready: bool,
    /// Lines the burst carries that are not well-formed commands, as sent, just before `ready;`.
    pub quirks: Vec<String>,

    // --- Behaviour ---
    /// The client commands this server acts on; any other is ignored. `None`: every command the
    /// simulator models (the spec's list). piHPSDR's is its command table, which has no
    /// `iq_start`, no spots, no `start` and no `keyer`.
    pub commands: Option<Vec<String>>,
    pub key_source: KeySource,
    /// A `tci` key while the client's audio stream is not running takes the microphone (spec
    /// §4.2: "This works if the TCI audio stream is enabled, otherwise the microphone signal is
    /// taken"). Off: the key takes TCI audio regardless.
    pub mic_fallback: bool,
    /// A `tci` key starts the client's transmit audio itself and tells it so, with its expected
    /// stream settings and `audio_start:0;` (piHPSDR).
    pub tci_key_starts_audio: bool,
    pub drive: DriveForm,
    /// The pushes `rx_sensors_enable` turns on.
    pub rx_sensors: Vec<SensorForm>,
    /// `rx_smeter:<trx>,<ch>;` is answered with `rx_smeter:<trx>,<ch>,<dBm>;` (an older poll).
    pub rx_smeter_poll: bool,
    /// The sensor interval when `rx_sensors_enable`/`tx_sensors_enable` gives none.
    pub sensor_interval: Duration,
    /// The server ignores the client's interval and uses its own (piHPSDR: once a second).
    pub fixed_sensor_interval: bool,
    /// From an accepted `trx:<n>,true` to the `trx:<n>,true;` echo (AetherSDR: once the radio
    /// transmits). UNVERIFIED for ExpertSDR3 and Thetis.
    pub trx_echo_delay: Duration,
    /// From the echo to the first TX_CHRONO. UNVERIFIED for ExpertSDR3 and Thetis.
    pub first_chrono: Duration,
    pub chrono_length: ChronoLength,
    pub ownership: Ownership,
    /// A client whose TCP connection dies while it keys is unkeyed. UNVERIFIED for ExpertSDR3
    /// and Thetis, whose profiles take the worse answer.
    pub unkey_on_drop: bool,
    /// A client that closes its WebSocket cleanly while it keys is unkeyed.
    pub unkey_on_close: bool,
    /// From `trx:<n>,false` to the unkey, while the client's TCI audio ran (piHPSDR: 50 ms).
    pub ptt_tail: Duration,
    /// `audio_start:<trx>;` is answered with the same line (WSJT-X waits for it).
    pub audio_ack: bool,
    /// `iq_start:<trx>;` is answered with the same line.
    pub iq_ack: bool,
    /// `audio_start` is refused, silently, unless the client's rate and sample type are 48000 and
    /// float32 (piHPSDR).
    pub audio_needs_default_format: bool,
    /// The stream settings a client sets (`audio_samplerate` …) are answered with the same line.
    pub echo_stream_settings: bool,
    /// A WebSocket ping this often (Thetis: every 20 s).
    pub ping_every: Option<Duration>,
    pub spot_click: Vec<ClickForm>,
    pub spot_store: SpotStore,
    /// `tx_frequency:<hz>;` follows each change of the transmit frequency.
    pub tx_frequency: bool,
    /// The audio settings a client starts with.
    pub audio: AudioFormat,
    /// The IQ rate a client starts with.
    pub iq_rate: u32,
    /// Sample values per IQ frame (I and Q count separately).
    pub iq_values: u32,
    /// The radio as the server comes up.
    pub initial: Initial,
}

impl Dialect {
    /// ExpertSDR3 speaking TCI 2.0, from the spec. Where the spec says nothing the profile takes
    /// the answer that is harder on a client: no unkey on a lost client, echo and first
    /// chrono at once.
    pub fn expert_sdr3() -> Dialect {
        Dialect {
            name: "ExpertSDR3 (TCI 2.0)".into(),
            program: "ExpertSDR3".into(),
            version: "2.0".into(),
            // The spec's example device (§4.1).
            device: "SunSDR2DX".into(),
            trx_count: 2,
            channels: 2,
            channel_count: Spelling::Plural,
            // The spec's examples (§4.1).
            vfo_limits: (10_000, 30_000_000),
            if_limits: (-48_000, 48_000),
            // UNVERIFIED: the spec gives only an example list; these are the names WSJT-X maps.
            modulations: words("AM,SAM,DSB,LSB,USB,CW,NFM,WFM,DIGL,DIGU,DRM"),
            receive_only: false,
            // The spec's own order (§4.1).
            identity_order: vec![
                Ident::VfoLimits,
                Ident::IfLimits,
                Ident::TrxCount,
                Ident::ChannelCount,
                Ident::Device,
                Ident::ReceiveOnly,
                Ident::Modulations,
                Ident::Protocol,
            ],
            burst_in_one_message: false,
            burst_stream_settings: false,
            run_state_after_ready: false,
            quirks: Vec::new(),
            commands: None,
            key_source: KeySource::Argument,
            mic_fallback: true,
            tci_key_starts_audio: false,
            drive: DriveForm::PerTrx,
            rx_sensors: vec![SensorForm::Channel],
            rx_smeter_poll: false,
            sensor_interval: Duration::from_millis(200),
            fixed_sensor_interval: false,
            trx_echo_delay: Duration::ZERO,
            first_chrono: Duration::ZERO,
            chrono_length: ChronoLength::ClientSamples,
            ownership: Ownership::None,
            unkey_on_drop: false,
            unkey_on_close: false,
            ptt_tail: Duration::ZERO,
            audio_ack: true,
            // UNVERIFIED: answered like `audio_start`.
            iq_ack: true,
            audio_needs_default_format: false,
            // The spec gives each stream setting a reply.
            echo_stream_settings: true,
            ping_every: None,
            // UNVERIFIED which spellings ExpertSDR3 sends; both, as AetherSDR does after Thetis.
            spot_click: vec![ClickForm::Legacy, ClickForm::Receiver],
            spot_store: SpotStore::PerCall,
            tx_frequency: true,
            audio: AudioFormat::SPEC,
            iq_rate: 48_000,
            iq_values: 2048,
            initial: Initial {
                trx: vec![
                    TrxInit::at(14_074_000, "USB"),
                    TrxInit::at(7_074_000, "LSB"),
                ],
                running: true,
                cw_speed: 25,
                cw_delay: 50,
                extra: lines("volume:-12;mute:false;agc_mode:0,normal;rx_nb_enable:0,false;"),
            },
        }
    }

    /// Thetis 2.10.3.14 and later, which emulates ExpertSDR3 by default (its 2.10.3.14 notes say
    /// the emulation is what lets WSJT-X and JTDX transmit TCI audio). From the plan's notes only:
    /// the identity is UNVERIFIED, the drive is per transceiver, it answers the older
    /// `rx_smeter` poll WSJT-X sends it, and it pings every 20 s (its developer, on its forum).
    pub fn thetis() -> Dialect {
        Dialect {
            name: "Thetis 2.10.3.14+ (ExpertSDR3 emulation)".into(),
            device: "ANAN-G2".into(),
            vfo_limits: (10_000, 61_440_000),
            modulations: words("LSB,USB,DSB,CWL,CWU,FM,AM,DIGU,SPEC,DIGL,SAM,DRM"),
            rx_smeter_poll: true,
            ping_every: Some(Duration::from_secs(20)),
            initial: Initial {
                trx: vec![
                    TrxInit::at(14_074_000, "USB"),
                    TrxInit::at(7_074_000, "LSB"),
                ],
                running: true,
                cw_speed: 25,
                cw_delay: 50,
                extra: Vec::new(),
            },
            ..Dialect::expert_sdr3()
        }
    }

    /// piHPSDR's TCI server (read at `3b63530267`). It announces itself as ExpertSDR3 2.0 on a
    /// SunSDR2 QRP; serves one transceiver; sends `ready;` then `start;`; carries two burst lines
    /// with no terminator; echoes a key at once; gives the transmitter to the first client that
    /// keys; starts a client's transmit audio itself on a `tci` key; holds the key 50 ms after an
    /// unkey while TCI audio ran; pushes both sensor spellings once a second; answers
    /// `audio_start` only at 48000/float32; unkeys a client whose connection drops, but not one
    /// that closes cleanly.
    pub fn pihpsdr() -> Dialect {
        Dialect {
            name: "piHPSDR".into(),
            program: "ExpertSDR3".into(),
            version: "2.0".into(),
            device: "SunSDR2QRP".into(),
            trx_count: 1,
            channels: 2,
            channel_count: Spelling::Plural,
            vfo_limits: (0, 61_440_000),
            if_limits: (-24_000, 24_000),
            modulations: words("LSB,USB,DSB,CW,FMN,AM,DIGU,SPEC,DIGL,SAM,DRM"),
            receive_only: false,
            identity_order: vec![
                Ident::Protocol,
                Ident::Device,
                Ident::ReceiveOnly,
                Ident::TrxCount,
                Ident::ChannelCount,
                Ident::VfoLimits,
                Ident::IfLimits,
                Ident::Modulations,
            ],
            burst_in_one_message: false,
            burst_stream_settings: true,
            run_state_after_ready: true,
            quirks: vec![
                "audio_stream_channels: 2".into(),
                "audio_stream_samples: 1024".into(),
            ],
            commands: Some(words(
                "trx_count,trx,split_enable,lock,vfo_lock,sql_enable,sql_level,rx_anf_enable,\
                 rx_apf_enable,rx_nb_enable,rx_nf_enable,rx_bin_enable,rx_nr_enable,rit_enable,\
                 xit_enable,rit_offset,xit_offset,digl_offset,digu_offset,tune,mute,rx_mute,volume,\
                 rx_volume,agc_gain,agc_mode,iq_samplerate,mon_volume,mon_enable,audio_samplerate,\
                 audio_stream_sample_type,audio_stream_channels,audio_stream_samples,\
                 rx_sensors_enable,tx_sensors_enable,audio_start,audio_stop,modulation,vfo,dds,\
                 rx_smeter,drive,tune_drive,rx_filter_band,cw_macros,cw_macros_stop,cw_msg,\
                 cw_terminal,cw_macros_speed,cw_keyer_speed,cw_macros_delay,cw_macros_speed_up,\
                 cw_macros_speed_down,stop",
            )),
            key_source: KeySource::Argument,
            mic_fallback: false,
            tci_key_starts_audio: true,
            drive: DriveForm::PerTrx,
            rx_sensors: vec![SensorForm::Receiver, SensorForm::Channel],
            rx_smeter_poll: true,
            sensor_interval: Duration::from_secs(1),
            fixed_sensor_interval: true,
            trx_echo_delay: Duration::ZERO,
            first_chrono: Duration::ZERO,
            // UNVERIFIED: the constant is in a header not read; it asks clients for 1024.
            chrono_length: ChronoLength::Fixed(1024),
            ownership: Ownership::FirstKeyer,
            unkey_on_drop: true,
            unkey_on_close: false,
            ptt_tail: Duration::from_millis(50),
            audio_ack: true,
            iq_ack: false,
            audio_needs_default_format: true,
            echo_stream_settings: true,
            ping_every: None,
            spot_click: Vec::new(),
            spot_store: SpotStore::PerCall,
            tx_frequency: false,
            audio: AudioFormat {
                samples: Some(1024),
                ..AudioFormat::SPEC
            },
            iq_rate: 48_000,
            iq_values: 2048,
            initial: Initial {
                trx: vec![TrxInit::at(14_074_000, "USB")],
                running: true,
                cw_speed: 20,
                cw_delay: 10,
                extra: lines("mute:false;volume:-20;"),
            },
        }
    }

    /// An ExpertSDR2-era server speaking TCI 1.x, as WSJT-X 3.0.2's client treats any server that
    /// does not announce ExpertSDR3 or Thetis: `drive:<v>;` with no transceiver, the receive
    /// level polled with `rx_smeter`, no source argument on `trx`. The identity is UNVERIFIED:
    /// `esdr,1.2` is the example in a comment of WSJT-X's client.
    pub fn expert_sdr2() -> Dialect {
        Dialect {
            name: "ExpertSDR2 (TCI 1.x)".into(),
            program: "esdr".into(),
            version: "1.2".into(),
            device: "SunSDR2PRO".into(),
            modulations: words("am,sam,dsb,lsb,usb,cw,nfm,wfm,digl,digu"),
            key_source: KeySource::RunningAudio,
            mic_fallback: true,
            drive: DriveForm::Bare,
            rx_sensors: Vec::new(),
            rx_smeter_poll: true,
            iq_ack: false,
            tx_frequency: false,
            spot_click: vec![ClickForm::Legacy],
            initial: Initial {
                trx: vec![
                    TrxInit::at(14_074_000, "usb"),
                    TrxInit::at(7_074_000, "lsb"),
                ],
                running: true,
                cw_speed: 25,
                cw_delay: 50,
                extra: Vec::new(),
            },
            ..Dialect::expert_sdr3()
        }
    }

    /// AetherSDR's TCI server in front of a FlexRadio (read at `05c5b805`): the whole burst in
    /// one message, ending `ready;start;`; it announces ExpertSDR3 1.5 on device `AetherSDR`;
    /// echoes `trx:true` only once the radio transmits and starts TX_CHRONO then, asking for 2048
    /// values (1024 stereo frames); only the keying client may unkey; a lost or closed owner is
    /// unkeyed; spot clicks in both spellings; spots kept its own way ([`SpotStore::Appended`],
    /// read at `8394342`).
    pub fn aether_sdr() -> Dialect {
        Dialect {
            name: "AetherSDR".into(),
            program: "ExpertSDR3".into(),
            version: "1.5".into(),
            device: "AetherSDR".into(),
            trx_count: 1,
            channels: 2,
            channel_count: Spelling::Plural,
            vfo_limits: (1_000, 75_000_000),
            if_limits: (-48_000, 48_000),
            modulations: words("usb,lsb,cw,cwr,am,sam,fm,nfm,digu,digl,rtty"),
            receive_only: false,
            identity_order: vec![
                Ident::VfoLimits,
                Ident::IfLimits,
                Ident::TrxCount,
                Ident::ChannelCount,
                Ident::Device,
                Ident::ReceiveOnly,
                Ident::Modulations,
                Ident::Protocol,
            ],
            burst_in_one_message: true,
            burst_stream_settings: true,
            run_state_after_ready: true,
            quirks: Vec::new(),
            commands: None,
            key_source: KeySource::Argument,
            mic_fallback: false,
            tci_key_starts_audio: false,
            drive: DriveForm::PerTrx,
            rx_sensors: vec![SensorForm::Channel],
            rx_smeter_poll: true,
            sensor_interval: Duration::from_millis(200),
            fixed_sensor_interval: false,
            // UNVERIFIED value: the Flex's own time to TRANSMITTING.
            trx_echo_delay: Duration::from_millis(60),
            first_chrono: Duration::ZERO,
            chrono_length: ChronoLength::Fixed(2048),
            ownership: Ownership::Initiator,
            unkey_on_drop: true,
            unkey_on_close: true,
            ptt_tail: Duration::ZERO,
            audio_ack: true,
            iq_ack: true,
            audio_needs_default_format: false,
            echo_stream_settings: true,
            ping_every: None,
            spot_click: vec![ClickForm::Legacy, ClickForm::Receiver],
            spot_store: SpotStore::Appended,
            tx_frequency: true,
            audio: AudioFormat::SPEC,
            iq_rate: 48_000,
            iq_values: 2048,
            initial: Initial {
                trx: vec![TrxInit::at(14_074_000, "digu")],
                running: true,
                cw_speed: 25,
                cw_delay: 50,
                extra: lines("volume:-10;"),
            },
        }
    }

    /// Every bundled profile.
    pub fn all() -> Vec<Dialect> {
        vec![
            Dialect::expert_sdr3(),
            Dialect::thetis(),
            Dialect::pihpsdr(),
            Dialect::expert_sdr2(),
            Dialect::aether_sdr(),
        ]
    }

    /// The modulation as this server spells it, matched in any case; `None` when it has no such
    /// mode.
    pub fn modulation(&self, name: &str) -> Option<&str> {
        self.modulations
            .iter()
            .find(|m| m.eq_ignore_ascii_case(name))
            .map(String::as_str)
    }

    /// The starting settings of transceiver `trx`.
    pub fn initial_trx(&self, trx: usize) -> TrxInit {
        self.initial
            .trx
            .get(trx)
            .or_else(|| self.initial.trx.first())
            .cloned()
            .unwrap_or_else(|| TrxInit::at(14_074_000, "USB"))
    }
}

impl Default for Dialect {
    fn default() -> Self {
        Dialect::expert_sdr3()
    }
}

fn words(list: &str) -> Vec<String> {
    list.split(',').map(|w| w.trim().to_string()).collect()
}

fn lines(list: &str) -> Vec<String> {
    list.split_inclusive(';').map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_profile_is_self_consistent() {
        for d in Dialect::all() {
            assert!(d.trx_count >= 1, "{}", d.name);
            assert_eq!(
                d.identity_order.len(),
                8,
                "{}: every identity line once",
                d.name
            );
            for t in 0..d.trx_count as usize {
                let trx = d.initial_trx(t);
                assert!(
                    d.modulation(&trx.modulation).is_some(),
                    "{}: starts in {} which it does not list",
                    d.name,
                    trx.modulation
                );
                for hz in trx.vfo {
                    assert!(
                        (d.vfo_limits.0..=d.vfo_limits.1).contains(&hz),
                        "{}",
                        d.name
                    );
                }
            }
            for line in &d.initial.extra {
                assert!(
                    line.ends_with(';') && line.matches(';').count() == 1,
                    "{line}"
                );
            }
        }
    }

    #[test]
    fn modulations_match_in_any_case_and_answer_in_the_servers_spelling() {
        let d = Dialect::aether_sdr();
        assert_eq!(d.modulation("DIGU"), Some("digu"));
        assert_eq!(Dialect::expert_sdr3().modulation("digu"), Some("DIGU"));
        assert_eq!(d.modulation("pktusb"), None);
    }

    #[test]
    fn default_sample_counts_follow_the_spec() {
        assert_eq!(default_samples(48_000), 2048);
        assert_eq!(default_samples(24_000), 1024);
        assert_eq!(default_samples(12_000), 512);
        assert_eq!(default_samples(8_000), 256);
        let f = AudioFormat {
            samples: Some(300),
            ..AudioFormat::SPEC
        };
        assert_eq!(f.samples(), 300);
    }
}
