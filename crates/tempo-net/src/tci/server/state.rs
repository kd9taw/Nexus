//! What the server tells programs: the app's [`Snapshot`], what each program was last told
//! ([`Told`]), and the per-program protocol state ([`Session`]) the answers depend on. Pure: no
//! sockets, no locks, no backend.
//!
//! # The init burst
//!
//! A program is sent, each line its own WebSocket message: `vfo_limits`, `if_limits`, `trx_count`,
//! `channels_count` (plural: RF2K-S reads only it), `device:Nexus`, `receive_only`,
//! `modulations_list`, `protocol:ExpertSDR3,2.0` (WSJT-X and JTDX send their `,tci` key only to
//! that name), the state (`vfo` on both channels, `dds`, `modulation`, `rx_enable`,
//! `split_enable:0,false`, RIT and XIT off, `drive`, `tune_drive`, `trx`, `tune`, `tx_enable`,
//! `rx_mute`), the program's stream defaults, then `ready;` (skimmer-type clients latch their
//! settings there), then `start;` (WSJT-X treats a server that never sent it as switched off).
//! Every line uses a name the spec or eesdr-tci's table knows, with the spec's exact arity.
//!
//! # After the burst
//!
//! Each [`Snapshot`] the app publishes is compared with what each program was told, and only what
//! changed is sent ([`Session::publish`]). A command's own answer goes to the program that sent it
//! even when nothing changed (WSJT-X waits for the echo of a `vfo` it wrote), and to the others
//! only when it changed what they were told.

use std::time::Duration;

use crate::tci::frame::{AudioRate, Channels, SampleFormat};
use crate::tci::wire::{
    ExtraNotice, Flag, Hz, Modes, Notice, Real, ServerText, Setting, StreamSetting as Stream,
    Token, Word,
};
use crate::tci::{Channel, Trx};

use super::backend::{AudioFormat, ClientId, KeyKind, Refusal, Refused};

/// The modes Nexus offers programs (`modulations_list`), as TCI names them. A
/// program's `modulation` must name one of these.
pub const MODULATIONS: [&str; 7] = ["am", "lsb", "usb", "cw", "nfm", "digl", "digu"];

/// The range told when Nexus does not know the radio's own: 30 kHz to 1.3 GHz.
pub const UNKNOWN_LIMITS: (i64, i64) = (30_000, 1_300_000_000);

/// What the server says it is: ExpertSDR3 for the command forms WSJT-X and JTDX
/// choose on that name, and Nexus by device so nothing mistakes it for a SunSDR (WSJT-X halves the
/// transmit amplitude for `SunSDR2DX`/`SunSDR2PRO` outside ExpertSDR3).
pub const DEVICE: &str = "Nexus";
pub const PROTOCOL: (&str, &str) = ("ExpertSDR3", "2.0");

/// The IF range told: ±24 kHz, half of a 48 kHz panorama. Nexus moves no panorama, so this only
/// keeps a program's arithmetic sane.
const IF_LIMIT: i64 = 24_000;

/// The interval sensors are sent at when a program enables them without one: the spec's own
/// example (`RX_SENSORS_ENABLE:true,200;`).
pub(super) const SENSOR_DEFAULT: Duration = Duration::from_millis(200);

/// The interval range the spec allows (§4.4, 30–1000 ms); a program's own is clamped into it.
pub(super) const SENSOR_RANGE: (u64, u64) = (30, 1000);

/// The station as the app publishes it, at most 20 times a second. Built and published under the
/// lock the backend's commands take (the backend module's lock rule), so a snapshot is never older
/// than an answer the server has already sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// Which radio this is. When it changes, every program is sent the device lines again, then
    /// the whole state.
    pub radio: u64,
    /// The radio's range, or [`UNKNOWN_LIMITS`]. A program's dial outside it is refused by the
    /// server before the backend is asked.
    pub vfo_limits: (Hz, Hz),
    /// The radio's connection cannot transmit (the Icom network Beta).
    pub receive_only: bool,
    /// The dial. Channel B always reads the same: no split.
    pub dial: Hz,
    /// The mode, as one of [`MODULATIONS`].
    pub mode: Word,
    /// Output power, 0–100.
    pub drive: u32,
    /// Output power in Tune, 0–100.
    pub tune_drive: u32,
    /// Anything is on the air, or a program's key is admitted and its over armed
    /// (WSJT-X needs `trx:0,true;` within a second of its key, before the first audio keys it).
    pub trx: bool,
    pub tune: bool,
    /// The program that owns the transmitter, when one does. A program the server admitted but
    /// that no longer owns it (Stop, the watchdog, a band change) loses its transmit audio here.
    pub owner: Option<ClientId>,
    /// The `tx_enable` notice for each program: the backend's verdict for a `tci` key from it
    /// (the same function that admits keys). A program not listed is told false.
    pub tx_enable: Vec<(ClientId, bool)>,
    /// CW speed, WPM.
    pub cw_speed: u32,
    /// Nexus's own transmit buffering, ms: lead-in plus look-ahead.
    pub tx_buffering_ms: u32,
    /// The receive level, dBm, when Nexus has one.
    pub rx_dbm: Option<f64>,
    /// The transmitter's readings while it transmits.
    pub tx: Option<TxMeters>,
}

/// The transmitter's readings (`tx_sensors`, §4.4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TxMeters {
    pub mic_dbm: f64,
    pub rms_w: f64,
    pub peak_w: f64,
    pub swr: f64,
}

impl Snapshot {
    fn tx_enable_for(&self, client: ClientId) -> bool {
        self.tx_enable
            .iter()
            .find(|(c, _)| *c == client)
            .is_some_and(|(_, on)| *on)
    }
}

/// What one program was last told, so a snapshot sends only what changed. `None`: never told.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Told {
    radio: Option<u64>,
    limits: Option<(Hz, Hz)>,
    receive_only: Option<bool>,
    dial: Option<Hz>,
    mode: Option<Word>,
    drive: Option<u32>,
    tune_drive: Option<u32>,
    trx: Option<bool>,
    tune: Option<bool>,
    tx_enable: Option<bool>,
}

impl Told {
    pub(super) fn trx(&self) -> Option<bool> {
        self.trx
    }
}

/// A state change a command's answer carries, sent to the program that sent the command and to
/// every other program it changes something for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Out {
    Dial(Hz),
    Mode(Word),
    Drive(u32),
    TuneDrive(u32),
    Trx(bool),
    Tune(bool),
}

/// A program's write of one of its own stream settings (§4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum FormatSet {
    Rate(AudioRate),
    SampleType(SampleFormat),
    Channels(Channels),
    Samples(u32),
}

/// One of the program's own stream settings (§4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum FormatField {
    Rate,
    SampleType,
    Channels,
    Samples,
}

/// A parameter a program is answered with: a read's reply, or a refused write's echo of the value
/// in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Value {
    /// `start;`
    Start,
    /// `vfo:0,<ch>,<dial>;`, channels A and B.
    Vfo(Channel),
    /// `dds:0,<dial>;`: the panorama is the dial.
    Dds,
    /// `if:0,<ch>,0;`
    If(Channel),
    Mode,
    Trx,
    /// `trx:0,false;`, the answer to a key that did not take.
    NotKeyed,
    Tune,
    Drive,
    TuneDrive,
    /// A per-receiver switch: always off. Nexus offers none of them through TCI.
    Flag(Flag),
    RitOffset,
    XitOffset,
    /// `rx_enable:0,true;`: the receiver is always on.
    RxEnable,
    /// Channel A on, B off.
    RxChannelEnable(Channel),
    /// Never locked.
    VfoLock(Channel),
    /// `rx_smeter:0,<ch>,<dBm>;`, when Nexus has a level.
    Smeter(Channel),
    Mute,
    MonEnable,
    CwSpeed,
    /// `cw_terminal:false;`: Nexus never holds the transmitter after a macro.
    CwTerminal,
    TxEnable,
    /// Nexus's own buffering, whatever the program asked.
    TxBuffering,
}

/// One program's protocol state.
#[derive(Debug, Default)]
pub(super) struct Session {
    /// Its stream settings.
    pub(super) format: AudioFormat,
    /// `audio_start` in force.
    pub(super) audio: bool,
    /// Its sensor subscriptions and their intervals.
    pub(super) rx_sensors: Option<Duration>,
    pub(super) tx_sensors: Option<Duration>,
    /// The key this server admitted for it and has not seen end: its transmit audio is taken only
    /// while this is a [`KeyKind::Tci`].
    pub(super) key: Option<KeyKind>,
    pub(super) told: Told,
    /// The init burst has been queued; until then a snapshot sends it nothing.
    pub(super) inited: bool,
    last_refusal: Option<Refusal>,
    logged: Vec<Refused>,
}

/// Every line below goes through one of these, so a line the codec cannot build is dropped rather
/// than sent half-made. Each holds by construction for the values here; the init-burst tests pin
/// the result.
fn token(text: &str) -> Option<Token> {
    Token::new(text)
}

fn trx0() -> Trx {
    // 0 is below MAX_TRX.
    Trx::new(0).unwrap_or_else(|| unreachable!("receiver 0"))
}

fn state(setting: Setting) -> ServerText {
    Notice::State(setting).render()
}

fn dial_lines(dial: Hz, out: &mut Vec<ServerText>) {
    for channel in [Channel::A, Channel::B] {
        out.push(state(Setting::Vfo {
            trx: trx0(),
            channel,
            hz: dial,
        }));
    }
    out.push(state(Setting::Dds {
        trx: trx0(),
        hz: dial,
    }));
}

fn device_lines(s: &Snapshot, out: &mut Vec<ServerText>) {
    out.push(
        Notice::VfoLimits {
            low: s.vfo_limits.0,
            high: s.vfo_limits.1,
        }
        .render(),
    );
    if let (Some(low), Some(high)) = (Hz::new(-IF_LIMIT), Hz::new(IF_LIMIT)) {
        out.push(Notice::IfLimits { low, high }.render());
    }
    out.push(Notice::TrxCount(1).render());
    out.push(Notice::ChannelCount(2).render());
    if let Some(device) = token(DEVICE) {
        out.push(Notice::Device(device).render());
    }
    out.push(Notice::ReceiveOnly(s.receive_only).render());
    out.extend(modulations_list());
}

/// `modulations_list:am,lsb,usb,cw,nfm,digl,digu;`
fn modulations_list() -> Option<ServerText> {
    let modes: Option<Vec<Word>> = MODULATIONS.iter().map(|m| Word::new(m)).collect();
    modes
        .and_then(Modes::new)
        .map(|modes| Notice::ModulationsList(modes).render())
}

fn off(flag: Flag) -> ServerText {
    state(Setting::Flag {
        flag,
        trx: trx0(),
        on: false,
    })
}

fn stream(setting: Stream) -> ServerText {
    Notice::Stream(setting).render()
}

fn format_line(format: &AudioFormat, field: FormatField) -> ServerText {
    stream(match field {
        FormatField::Rate => Stream::AudioSampleRate(format.rate),
        FormatField::SampleType => Stream::AudioSampleType(format.sample_type),
        FormatField::Channels => Stream::AudioChannels(format.channels),
        FormatField::Samples => Stream::AudioSamples(format.samples),
    })
}

fn sensor_interval(ms: Option<u32>) -> Duration {
    match ms {
        None => SENSOR_DEFAULT,
        Some(ms) => Duration::from_millis(u64::from(ms).clamp(SENSOR_RANGE.0, SENSOR_RANGE.1)),
    }
}

impl Session {
    /// The init burst, in order, each line one message. Marks the session ready for
    /// snapshots.
    pub(super) fn greet(&mut self, s: &Snapshot, client: ClientId) -> Vec<ServerText> {
        let mut out = Vec::with_capacity(32);
        device_lines(s, &mut out);
        if let (Some(program), Some(version)) = (token(PROTOCOL.0), token(PROTOCOL.1)) {
            out.push(Notice::Protocol { program, version }.render());
        }
        dial_lines(s.dial, &mut out);
        out.push(state(Setting::Modulation {
            trx: trx0(),
            channel: None,
            mode: s.mode.clone(),
        }));
        out.push(
            ExtraNotice::RxEnable {
                trx: trx0(),
                on: true,
            }
            .render(),
        );
        out.push(off(Flag::Split));
        out.push(off(Flag::Rit));
        out.push(off(Flag::Xit));
        if let Some(zero) = Hz::new(0) {
            out.push(state(Setting::RitOffset {
                trx: trx0(),
                hz: zero,
            }));
            out.push(state(Setting::XitOffset {
                trx: trx0(),
                hz: zero,
            }));
        }
        out.push(state(Setting::Drive {
            trx: Some(trx0()),
            level: s.drive,
        }));
        out.push(state(Setting::TuneDrive {
            trx: trx0(),
            level: s.tune_drive,
        }));
        out.push(
            Notice::Trx {
                trx: trx0(),
                on: s.trx,
                source: None,
            }
            .render(),
        );
        out.push(
            Notice::Tune {
                trx: trx0(),
                on: s.tune,
            }
            .render(),
        );
        let tx_enable = s.tx_enable_for(client);
        out.push(
            Notice::TxEnable {
                trx: trx0(),
                on: tx_enable,
            }
            .render(),
        );
        out.push(off(Flag::RxMute));
        for field in [
            FormatField::Rate,
            FormatField::SampleType,
            FormatField::Channels,
            FormatField::Samples,
        ] {
            out.push(format_line(&self.format, field));
        }
        out.push(stream(Stream::TxAudioBuffering(s.tx_buffering_ms)));
        out.push(Notice::Ready.render());
        out.push(Notice::RadioStart.render());
        self.told = Told {
            radio: Some(s.radio),
            limits: Some(s.vfo_limits),
            receive_only: Some(s.receive_only),
            dial: Some(s.dial),
            mode: Some(s.mode.clone()),
            drive: Some(s.drive),
            tune_drive: Some(s.tune_drive),
            trx: Some(s.trx),
            tune: Some(s.tune),
            tx_enable: Some(tx_enable),
        };
        self.inited = true;
        out
    }

    /// What changed since this program was last told. Nothing before the init burst. A key this
    /// server admitted ends here when the snapshot shows the program no longer owns the
    /// transmitter.
    pub(super) fn publish(&mut self, s: &Snapshot, client: ClientId) -> Vec<ServerText> {
        if self.key.is_some() && s.owner != Some(client) {
            self.key = None;
        }
        let mut out = Vec::new();
        if !self.inited {
            return out;
        }
        let t = &mut self.told;
        let new_radio = t.radio.is_some_and(|r| r != s.radio);
        if new_radio {
            // A radio switch inside the instance: the device lines, then the whole state again.
            *t = Told::default();
        }
        if t.radio != Some(s.radio)
            || t.limits != Some(s.vfo_limits)
            || t.receive_only != Some(s.receive_only)
        {
            if let Some(device) = token(DEVICE) {
                out.push(Notice::Device(device).render());
            }
            out.push(
                Notice::VfoLimits {
                    low: s.vfo_limits.0,
                    high: s.vfo_limits.1,
                }
                .render(),
            );
            out.push(Notice::ReceiveOnly(s.receive_only).render());
            out.extend(modulations_list());
            t.radio = Some(s.radio);
            t.limits = Some(s.vfo_limits);
            t.receive_only = Some(s.receive_only);
        }
        for o in [
            Out::Dial(s.dial),
            Out::Mode(s.mode.clone()),
            Out::Drive(s.drive),
            Out::TuneDrive(s.tune_drive),
            Out::Trx(s.trx),
            Out::Tune(s.tune),
        ] {
            out.extend(self.echo(o, false));
        }
        let tx_enable = s.tx_enable_for(client);
        if self.told.tx_enable != Some(tx_enable) {
            self.told.tx_enable = Some(tx_enable);
            out.push(
                Notice::TxEnable {
                    trx: trx0(),
                    on: tx_enable,
                }
                .render(),
            );
        }
        out
    }

    /// The lines for a state change: always when `force` (the answer to this program's own
    /// command), otherwise only when it differs from what the program was told.
    pub(super) fn echo(&mut self, o: Out, force: bool) -> Vec<ServerText> {
        let t = &mut self.told;
        let mut out = Vec::new();
        match o {
            Out::Dial(hz) => {
                if force || t.dial != Some(hz) {
                    t.dial = Some(hz);
                    dial_lines(hz, &mut out);
                    // The transmitter's frequency is the dial: no split.
                    out.push(Notice::TxFrequency(hz).render());
                }
            }
            Out::Mode(mode) => {
                if force || t.mode.as_ref() != Some(&mode) {
                    out.push(state(Setting::Modulation {
                        trx: trx0(),
                        channel: None,
                        mode: mode.clone(),
                    }));
                    t.mode = Some(mode);
                }
            }
            Out::Drive(level) => {
                if force || t.drive != Some(level) {
                    t.drive = Some(level);
                    out.push(state(Setting::Drive {
                        trx: Some(trx0()),
                        level,
                    }));
                }
            }
            Out::TuneDrive(level) => {
                if force || t.tune_drive != Some(level) {
                    t.tune_drive = Some(level);
                    out.push(state(Setting::TuneDrive { trx: trx0(), level }));
                }
            }
            Out::Trx(on) => {
                if force || t.trx != Some(on) {
                    t.trx = Some(on);
                    out.push(
                        Notice::Trx {
                            trx: trx0(),
                            on,
                            source: None,
                        }
                        .render(),
                    );
                }
            }
            Out::Tune(on) => {
                if force || t.tune != Some(on) {
                    t.tune = Some(on);
                    out.push(Notice::Tune { trx: trx0(), on }.render());
                }
            }
        }
        out
    }

    /// The answer to a read, or a refused write's echo. `None` where Nexus has nothing true to
    /// say (an S-meter with no level, a channel past B).
    pub(super) fn answer(
        &mut self,
        v: Value,
        s: &Snapshot,
        client: ClientId,
    ) -> Option<ServerText> {
        let ab = |c: Channel| c == Channel::A || c == Channel::B;
        Some(match v {
            Value::Start => Notice::RadioStart.render(),
            Value::Vfo(channel) if ab(channel) => {
                self.told.dial = Some(s.dial);
                state(Setting::Vfo {
                    trx: trx0(),
                    channel,
                    hz: s.dial,
                })
            }
            Value::Dds => {
                self.told.dial = Some(s.dial);
                state(Setting::Dds {
                    trx: trx0(),
                    hz: s.dial,
                })
            }
            Value::If(channel) if ab(channel) => state(Setting::If {
                trx: trx0(),
                channel,
                hz: Hz::new(0)?,
            }),
            Value::Mode => return self.echo(Out::Mode(s.mode.clone()), true).pop(),
            Value::Trx => return self.echo(Out::Trx(s.trx), true).pop(),
            Value::NotKeyed => return self.echo(Out::Trx(false), true).pop(),
            Value::Tune => return self.echo(Out::Tune(s.tune), true).pop(),
            Value::Drive => return self.echo(Out::Drive(s.drive), true).pop(),
            Value::TuneDrive => return self.echo(Out::TuneDrive(s.tune_drive), true).pop(),
            Value::Flag(flag) => off(flag),
            Value::RitOffset => state(Setting::RitOffset {
                trx: trx0(),
                hz: Hz::new(0)?,
            }),
            Value::XitOffset => state(Setting::XitOffset {
                trx: trx0(),
                hz: Hz::new(0)?,
            }),
            Value::RxEnable => ExtraNotice::RxEnable {
                trx: trx0(),
                on: true,
            }
            .render(),
            Value::RxChannelEnable(channel) if ab(channel) => state(Setting::RxChannelEnable {
                trx: trx0(),
                channel,
                on: channel == Channel::A,
            }),
            Value::VfoLock(channel) if ab(channel) => Notice::VfoLock {
                trx: trx0(),
                channel,
                on: false,
            }
            .render(),
            Value::Smeter(channel) if ab(channel) => Notice::RxSmeter {
                trx: trx0(),
                channel,
                dbm: s.rx_dbm.and_then(Real::from_f64)?,
            }
            .render(),
            Value::Mute => state(Setting::Mute(false)),
            Value::MonEnable => state(Setting::MonEnable(false)),
            Value::CwSpeed => state(Setting::CwMacrosSpeed(s.cw_speed)),
            Value::CwTerminal => ExtraNotice::CwTerminal(false).render(),
            Value::TxEnable => {
                let on = s.tx_enable_for(client);
                self.told.tx_enable = Some(on);
                Notice::TxEnable { trx: trx0(), on }.render()
            }
            Value::TxBuffering => stream(Stream::TxAudioBuffering(s.tx_buffering_ms)),
            Value::Vfo(_)
            | Value::If(_)
            | Value::RxChannelEnable(_)
            | Value::VfoLock(_)
            | Value::Smeter(_) => return None,
        })
    }

    /// Apply a program's stream setting (§4.3, inside the spec's ranges) and answer with the value
    /// now in force. A value outside them is refused (named), and the answer is the value
    /// unchanged.
    pub(super) fn set_format(&mut self, setting: FormatSet) -> (ServerText, Option<Refused>) {
        let (field, refused) = match setting {
            FormatSet::Rate(rate) => {
                self.format.rate = rate;
                (FormatField::Rate, None)
            }
            FormatSet::SampleType(t) => {
                self.format.sample_type = t;
                (FormatField::SampleType, None)
            }
            FormatSet::Channels(c) => {
                self.format.channels = c;
                (FormatField::Channels, None)
            }
            FormatSet::Samples(n) => {
                if AudioFormat::SAMPLES.contains(&n) {
                    self.format.samples = n;
                    (FormatField::Samples, None)
                } else {
                    (FormatField::Samples, Some(Refused::Samples))
                }
            }
        };
        (format_line(&self.format, field), refused)
    }

    /// Subscribe to, or stop, a sensor stream, the interval clamped into the spec's range.
    pub(super) fn set_sensors(&mut self, tx: bool, on: bool, interval_ms: Option<u32>) {
        let interval = on.then(|| sensor_interval(interval_ms));
        if tx {
            self.tx_sensors = interval;
        } else {
            self.rx_sensors = interval;
        }
    }

    /// Whether this refusal is new for this program: report it, and remember it until the answer
    /// changes.
    pub(super) fn new_refusal(&mut self, r: Refusal) -> bool {
        let new = self.last_refusal != Some(r);
        self.last_refusal = Some(r);
        new
    }

    /// A key was admitted: the next refusal is new again.
    pub(super) fn admitted(&mut self, kind: KeyKind) {
        self.key = Some(kind);
        self.last_refusal = None;
    }

    /// Whether this refused command is new for this program (each kind is reported once).
    pub(super) fn new_refused(&mut self, what: Refused) -> bool {
        if self.logged.contains(&what) {
            false
        } else {
            self.logged.push(what);
            true
        }
    }
}

/// The receive sensors (§4.4): `rx_sensors` (the 1.9 form, the only one WSJT-X reads,
/// `TCITransceiver.cpp:650-653`) and `rx_channel_sensors` (2.0). Nothing when Nexus has no level.
pub(super) fn rx_sensor_lines(s: &Snapshot) -> Vec<ServerText> {
    let Some(dbm) = s.rx_dbm.and_then(Real::from_f64) else {
        return Vec::new();
    };
    vec![
        Notice::RxSensors { trx: trx0(), dbm }.render(),
        Notice::RxChannelSensors {
            trx: trx0(),
            channel: Channel::A,
            dbm,
        }
        .render(),
    ]
}

/// The transmit sensors while transmitting: `tx_sensors`, and `tx_power` and `tx_swr` beside it
/// (WSJT-X maps those, `TCITransceiver.cpp:661-668`; tuner boxes read `tx_swr`). Every value has a
/// decimal point: WSJT-X and JTDX split power and SWR at the `.` and read the fraction.
pub(super) fn tx_sensor_lines(s: &Snapshot) -> Vec<ServerText> {
    let Some(m) = s.tx else {
        return Vec::new();
    };
    let (Some(mic_dbm), Some(rms_w), Some(peak_w), Some(swr)) = (
        Real::from_f64(m.mic_dbm),
        Real::from_f64(m.rms_w),
        Real::from_f64(m.peak_w),
        Real::from_f64(m.swr),
    ) else {
        return Vec::new();
    };
    vec![
        Notice::TxSensors {
            trx: trx0(),
            mic_dbm,
            rms_w,
            peak_w,
            swr,
        }
        .render(),
        Notice::TxPower(rms_w).render(),
        Notice::TxSwr(swr).render(),
    ]
}
