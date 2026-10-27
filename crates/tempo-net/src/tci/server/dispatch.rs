//! What the server does with each command a program sends, as a closed table.
//!
//! [`classify`] turns every [`Inbound`] the codec can read into exactly one [`Action`], with
//! exhaustive matches and no wildcard arm: a command the codec learns to read does not compile
//! here until someone decides what it does. An unknown command never gets this far (the codec
//! refuses it), and nothing reaches the backend except through an arm of [`run`]. There is no
//! path that hands a program's text on: AetherSDR's TCI server had one, and it had no transmit
//! gate behind it.
//!
//! Every action that can put the transmitter on the air ([`Action::Key`], [`Action::TuneOn`],
//! [`Action::Cw`]) is a question to the backend, whose verdict is final, asked through the
//! program's transmit gate (`conn.rs`): never once the program's end has begun, and never while
//! that end is being told. So is [`Action::Mode`]: the backend keys a program's own audio only after
//! that program asked a voice mode, so the ask must not outlive the program. The server's own part
//! is to refuse more: receiver 0 only, sources it knows, the forms this table refuses outright. A
//! program is never told a key took that the backend did not admit.

use std::sync::{Mutex, MutexGuard};

use crate::tci::wire::{
    Call, Command, Extra, Flag, Hz, Inbound, Notice, Query, Request, ServerText, Setting, Source,
    Start, Stop, StreamSetting, Word,
};
use crate::tci::{Channel, Trx};

use super::backend::{
    ClientId, Cw, CwSpeed, Event, KeyKind, Mic, Refusal, Refused, Spot, TciBackend,
};
use super::state::{FormatSet, Out, Session, Snapshot, Value, MODULATIONS};

/// One command's disposition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Action {
    // ── Accepted: nothing changes on the radio ──
    /// Answered with the value in force: a read, `start;`, and the writes that ask for what
    /// already holds (`split_enable:…,false`, `rx_enable:…,true`, `rx_mute:…,false`,
    /// `cw_terminal:false`).
    Answer(Value),
    /// The program's own stream settings (§4.3).
    Format(FormatSet),
    /// A sensor subscription (§4.4).
    Sensors {
        tx: bool,
        on: bool,
        interval_ms: Option<u32>,
    },
    // ── Mapped: the backend decides ──
    /// `vfo:0,0,<hz>;`
    Dial(Hz),
    /// `vfo:0,1,<hz>;`: no split, so only the dial itself is accepted.
    SubDial(Hz),
    /// `modulation:0,<mode>;`
    Mode(Word),
    /// `drive:0,<level>;`
    Drive(u32),
    /// `tune_drive:0,<level>;`
    TuneDrive(u32),
    /// `trx:0,true[,<source>];`
    Key(KeyKind),
    /// `trx:0,false…;`
    Release,
    /// `tune:0,true;`
    TuneOn,
    /// `tune:0,false;`
    TuneOff,
    /// `stop;`: the program's own transmissions only.
    EndOwn,
    /// `audio_start:0;`
    AudioStart,
    /// `audio_stop:0;`
    AudioStop,
    /// `cw_macros`, `cw_msg`.
    Cw(Cw),
    /// `cw_macros_stop;`
    CwStop,
    CwSpeed(CwSpeed),
    Spot(Spot),
    SpotDelete(Call),
    SpotClear,
    // ── Refused ──
    /// Refused, and answered with the value in force.
    Refuse(Value, Refused),
    /// Refused, with no answer.
    Ignore(Refused),
}

/// The kinds of stop, which a program over the command rate still gets through (`conn.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum StopKind {
    Release,
    TuneOff,
    CwStop,
    EndOwn,
}

impl Action {
    /// Which stop this is, if it is one.
    pub(super) fn stop(&self) -> Option<StopKind> {
        match self {
            Action::Release => Some(StopKind::Release),
            Action::TuneOff => Some(StopKind::TuneOff),
            Action::CwStop => Some(StopKind::CwStop),
            Action::EndOwn => Some(StopKind::EndOwn),
            _ => None,
        }
    }
}

fn receiver_zero(trx: Trx) -> bool {
    trx.get() == 0
}

/// The disposition of one command. Total and exhaustive over what the codec reads.
pub(super) fn classify(inbound: Inbound) -> Action {
    match inbound {
        Inbound::Request(Request::Command(c)) => command(c),
        Inbound::Request(Request::Stop(s)) => stop(s),
        Inbound::Request(Request::Start(s)) => start(s),
        Inbound::Extra(e) => extra(e),
    }
}

fn command(c: Command) -> Action {
    match c {
        Command::RadioStart => Action::Answer(Value::Start),
        // The radio keeps running; only the program's own transmissions end.
        Command::RadioStop => Action::EndOwn,
        Command::Set(s) => setting(s),
        Command::Read(q) => query(q),
        Command::Stream(s) => stream(s),
        Command::Spot {
            call,
            mode,
            hz,
            colour,
            text,
        } => Action::Spot(Spot {
            call,
            mode,
            hz,
            argb: colour.argb(),
            text,
        }),
        Command::SpotDelete { call, .. } => Action::SpotDelete(call),
        Command::SpotClear => Action::SpotClear,
        Command::RxSensorsEnable { on, interval_ms } => Action::Sensors {
            tx: false,
            on,
            interval_ms,
        },
        Command::TxSensorsEnable { on, interval_ms } => Action::Sensors {
            tx: true,
            on,
            interval_ms,
        },
        Command::CwMacrosSpeedUp(n) => Action::CwSpeed(CwSpeed::Up(n)),
        Command::CwMacrosSpeedDown(n) => Action::CwSpeed(CwSpeed::Down(n)),
        Command::SetInFocus => Action::Ignore(Refused::Focus),
    }
}

fn setting(s: Setting) -> Action {
    let ignore = Action::Ignore(Refused::Receiver);
    match s {
        Setting::Dds { trx, .. } if receiver_zero(trx) => {
            Action::Refuse(Value::Dds, Refused::Panorama)
        }
        Setting::If { trx, channel, .. } if receiver_zero(trx) => {
            Action::Refuse(Value::If(channel), Refused::Panorama)
        }
        Setting::Vfo { trx, channel, hz } if receiver_zero(trx) => match channel.get() {
            0 => Action::Dial(hz),
            1 => Action::SubDial(hz),
            _ => Action::Ignore(Refused::Channel),
        },
        Setting::Modulation { trx, channel, mode } if receiver_zero(trx) => match channel {
            None => Action::Mode(mode),
            Some(c) if c == Channel::A => Action::Mode(mode),
            Some(_) => Action::Ignore(Refused::Channel),
        },
        Setting::Drive {
            trx: Some(trx),
            level,
        } if receiver_zero(trx) => Action::Drive(level),
        Setting::Drive { trx: None, .. } => Action::Ignore(Refused::OldDrive),
        Setting::TuneDrive { trx, level } if receiver_zero(trx) => Action::TuneDrive(level),
        Setting::Flag { flag, trx, on } if receiver_zero(trx) => match (flag, on) {
            // Asking for what already holds is accepted; MSHV writes `rx_mute:<trx>,false;` on its
            // first `vfo` (`network.cpp:2482-2491`).
            (Flag::Split | Flag::RxMute, false) => Action::Answer(Value::Flag(flag)),
            _ => Action::Refuse(Value::Flag(flag), Refused::Switch),
        },
        Setting::RitOffset { trx, .. } if receiver_zero(trx) => {
            Action::Refuse(Value::RitOffset, Refused::Offset)
        }
        Setting::XitOffset { trx, .. } if receiver_zero(trx) => {
            Action::Refuse(Value::XitOffset, Refused::Offset)
        }
        Setting::RxChannelEnable { trx, channel, .. } if receiver_zero(trx) => {
            Action::Refuse(Value::RxChannelEnable(channel), Refused::Channels)
        }
        Setting::Dds { .. }
        | Setting::If { .. }
        | Setting::Vfo { .. }
        | Setting::Modulation { .. }
        | Setting::Drive { .. }
        | Setting::TuneDrive { .. }
        | Setting::Flag { .. }
        | Setting::RitOffset { .. }
        | Setting::XitOffset { .. }
        | Setting::RxChannelEnable { .. } => ignore,
        Setting::RxFilterBand { .. } => Action::Ignore(Refused::Filter),
        Setting::CwMacrosSpeed(wpm) => Action::CwSpeed(CwSpeed::Set(wpm)),
        Setting::CwMacrosDelay(_) | Setting::CwKeyerSpeed(_) => Action::Ignore(Refused::CwSetting),
        Setting::Mute(_) => Action::Refuse(Value::Mute, Refused::Audio),
        Setting::MonEnable(_) => Action::Refuse(Value::MonEnable, Refused::Audio),
        Setting::Volume(_)
        | Setting::RxVolume { .. }
        | Setting::RxBalance { .. }
        | Setting::MonVolume(_) => Action::Ignore(Refused::Audio),
        Setting::AgcMode { .. }
        | Setting::AgcGain { .. }
        | Setting::NbParam { .. }
        | Setting::SqlLevel { .. } => Action::Ignore(Refused::Dsp),
        Setting::DiglOffset(_) | Setting::DiguOffset(_) => Action::Ignore(Refused::DigitalOffset),
    }
}

fn query(q: Query) -> Action {
    let on_zero = |trx: Trx, v: Value| {
        if receiver_zero(trx) {
            Action::Answer(v)
        } else {
            Action::Ignore(Refused::Receiver)
        }
    };
    match q {
        Query::Dds(trx) => on_zero(trx, Value::Dds),
        Query::If { trx, channel } => on_zero(trx, Value::If(channel)),
        Query::Vfo { trx, channel } => on_zero(trx, Value::Vfo(channel)),
        Query::Modulation(trx) => on_zero(trx, Value::Mode),
        Query::Trx(trx) => on_zero(trx, Value::Trx),
        Query::Tune(trx) => on_zero(trx, Value::Tune),
        Query::Drive(Some(trx)) => on_zero(trx, Value::Drive),
        Query::Drive(None) => Action::Answer(Value::Drive),
        Query::TuneDrive(trx) => on_zero(trx, Value::TuneDrive),
        Query::Flag { flag, trx } => on_zero(trx, Value::Flag(flag)),
        Query::RitOffset(trx) => on_zero(trx, Value::RitOffset),
        Query::XitOffset(trx) => on_zero(trx, Value::XitOffset),
        Query::RxChannelEnable { trx, channel } => on_zero(trx, Value::RxChannelEnable(channel)),
        Query::VfoLock { trx, channel } => on_zero(trx, Value::VfoLock(channel)),
        Query::RxSmeter { trx, channel } => on_zero(trx, Value::Smeter(channel)),
        Query::CwMacrosSpeed => Action::Answer(Value::CwSpeed),
        Query::Mute => Action::Answer(Value::Mute),
        Query::MonEnable => Action::Answer(Value::MonEnable),
        Query::RxFilterBand(_) => Action::Ignore(Refused::Filter),
        Query::CwMacrosDelay => Action::Ignore(Refused::CwSetting),
        Query::Volume | Query::RxVolume { .. } | Query::RxBalance { .. } | Query::MonVolume => {
            Action::Ignore(Refused::Audio)
        }
        Query::AgcMode(_) | Query::AgcGain(_) | Query::NbParam(_) | Query::SqlLevel(_) => {
            Action::Ignore(Refused::Dsp)
        }
        Query::DiglOffset | Query::DiguOffset => Action::Ignore(Refused::DigitalOffset),
    }
}

fn stream(s: StreamSetting) -> Action {
    match s {
        StreamSetting::AudioStart(trx) if receiver_zero(trx) => Action::AudioStart,
        StreamSetting::AudioStop(trx) if receiver_zero(trx) => Action::AudioStop,
        StreamSetting::AudioStart(_) | StreamSetting::AudioStop(_) => {
            Action::Ignore(Refused::Receiver)
        }
        StreamSetting::IqStart(_) | StreamSetting::IqStop(_) | StreamSetting::IqSampleRate(_) => {
            Action::Ignore(Refused::Iq)
        }
        StreamSetting::LineOutStart(_) | StreamSetting::LineOutStop(_) => {
            Action::Ignore(Refused::LineOut)
        }
        StreamSetting::AudioSampleRate(rate) => Action::Format(FormatSet::Rate(rate)),
        StreamSetting::AudioSampleType(t) => Action::Format(FormatSet::SampleType(t)),
        StreamSetting::AudioChannels(c) => Action::Format(FormatSet::Channels(c)),
        StreamSetting::AudioSamples(n) => Action::Format(FormatSet::Samples(n)),
        // Accepted; the answer is Nexus's own buffering, which is what the program gets.
        StreamSetting::TxAudioBuffering(_) => Action::Answer(Value::TxBuffering),
    }
}

fn stop(s: Stop) -> Action {
    match s {
        Stop::Trx(trx) if receiver_zero(trx) => Action::Release,
        Stop::Tune(trx) if receiver_zero(trx) => Action::TuneOff,
        // Nothing keys on another receiver, so there is nothing there to stop.
        Stop::Trx(_) | Stop::Tune(_) => Action::Ignore(Refused::Receiver),
        Stop::CwMacros => Action::CwStop,
        // Terminal mode is never on, so `cw_terminal:false;` asks for what holds.
        Stop::CwTerminal => Action::Answer(Value::CwTerminal),
        Stop::KeyUp { .. } => Action::Ignore(Refused::Keyer),
    }
}

fn start(s: Start) -> Action {
    match s {
        Start::Trx { trx, source } if receiver_zero(trx) => match source {
            Some(Source::Tci) => Action::Key(KeyKind::Tci),
            None => Action::Key(KeyKind::Mic(Mic::Selected)),
            Some(Source::Mic1) => Action::Key(KeyKind::Mic(Mic::Mic1)),
            Some(Source::Mic2) => Action::Key(KeyKind::Mic(Mic::Mic2)),
            Some(Source::MicPc) => Action::Key(KeyKind::Mic(Mic::MicPc)),
            Some(Source::Ecoder2) => Action::Key(KeyKind::Mic(Mic::Ecoder2)),
            // A source Nexus does not know is never guessed at: the key did not take.
            Some(Source::Other(_)) => Action::Refuse(Value::NotKeyed, Refused::Source),
        },
        Start::Tune(trx) if receiver_zero(trx) => Action::TuneOn,
        Start::CwMacros { trx, text } if receiver_zero(trx) => Action::Cw(Cw::Macro(text)),
        Start::CwMsg {
            trx,
            prefix,
            call,
            suffix,
        } if receiver_zero(trx) => Action::Cw(Cw::Message {
            prefix,
            call,
            suffix,
        }),
        Start::Trx { .. } | Start::Tune(_) | Start::CwMacros { .. } | Start::CwMsg { .. } => {
            Action::Ignore(Refused::Receiver)
        }
        Start::CwMsgCall(_) => Action::Ignore(Refused::CwCall),
        Start::CwTerminal => Action::Refuse(Value::CwTerminal, Refused::CwTerminal),
        Start::KeyDown { .. } => Action::Ignore(Refused::Keyer),
    }
}

fn extra(e: Extra) -> Action {
    match e {
        Extra::Split(false) => Action::Answer(Value::Flag(Flag::Split)),
        Extra::Split(true) => Action::Refuse(Value::Flag(Flag::Split), Refused::Switch),
        Extra::RxEnable { trx, on: true } | Extra::RxEnableRead(trx) if receiver_zero(trx) => {
            Action::Answer(Value::RxEnable)
        }
        Extra::RxEnable { trx, on: false } if receiver_zero(trx) => {
            Action::Refuse(Value::RxEnable, Refused::RxEnable)
        }
        Extra::TxEnable { trx, .. } if receiver_zero(trx) => {
            Action::Refuse(Value::TxEnable, Refused::TxEnable)
        }
        Extra::RxEnable { .. } | Extra::RxEnableRead(_) | Extra::TxEnable { .. } => {
            Action::Ignore(Refused::Receiver)
        }
        Extra::LineOutRecorder => Action::Ignore(Refused::Recorder),
    }
}

/// Where the lines for the program that sent a command go: its outbox. Called with that program's
/// session locked, so what it was told and what it is sent cannot be reordered by a snapshot
/// published at the same moment.
pub(super) trait Sink {
    fn send(&self, lines: Vec<ServerText>);
}

/// What a program's command needs to run: who sent it, the backend, the snapshot, the program's
/// session, its outbox, and its transmit gate (true once its end has begun).
pub(super) struct Cx<'a> {
    pub(super) client: ClientId,
    pub(super) backend: &'a dyn TciBackend,
    pub(super) model: &'a Mutex<Snapshot>,
    pub(super) session: &'a Mutex<Session>,
    pub(super) sink: &'a dyn Sink,
    pub(super) gate: &'a Mutex<bool>,
}

/// What running a command leaves for the caller: state changes for every program (the sender
/// always told, the others when it changed what they were told), and lines for the connection
/// log. The caller delivers both with no lock held.
#[derive(Debug, Default)]
pub(super) struct Effects {
    pub(super) all: Vec<Out>,
    pub(super) events: Vec<Event>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ask the backend something that can transmit, or a spot or a mode ask that must not outlive the
/// program, through the program's gate: only while its end has not begun, and holding that end off
/// until the call returns. `None`: the end has begun, nothing reached the backend, and nothing is
/// answered (the program is being closed).
fn gated<T>(cx: &Cx<'_>, call: impl FnOnce() -> T) -> Option<T> {
    let ended = lock(cx.gate);
    (!*ended).then(call)
}

impl Effects {
    /// Answer the sender with a value (the snapshot, then the session: the lock order).
    fn answer(&mut self, cx: &Cx<'_>, v: Value) {
        let model = lock(cx.model);
        let mut session = lock(cx.session);
        if let Some(line) = session.answer(v, &model, cx.client) {
            cx.sink.send(vec![line]);
        }
    }

    /// Tell the sender a state, whether or not it changed.
    fn echo(&mut self, cx: &Cx<'_>, o: Out) {
        let mut session = lock(cx.session);
        let lines = session.echo(o, true);
        cx.sink.send(lines);
    }

    /// A line for the sender that no state rides on.
    fn say(&mut self, cx: &Cx<'_>, line: ServerText) {
        let _session = lock(cx.session);
        cx.sink.send(vec![line]);
    }

    /// The backend refused: log it once for this program until the answer changes.
    fn refusal(&mut self, cx: &Cx<'_>, refusal: Refusal) {
        if lock(cx.session).new_refusal(refusal) {
            self.events.push(Event::Refusal {
                client: cx.client,
                refusal,
            });
        }
    }

    /// The server refused: log it once for this program.
    fn refused(&mut self, cx: &Cx<'_>, what: Refused) {
        if lock(cx.session).new_refused(what) {
            self.events.push(Event::CommandRefused {
                client: cx.client,
                what,
            });
        }
    }
}

/// Run one command. The backend is called with no lock held; its verdict decides every change and
/// every key.
pub(super) fn run(action: Action, cx: &Cx<'_>) -> Effects {
    let mut fx = Effects::default();
    let snapshot = || lock(cx.model).clone();
    match action {
        Action::Answer(v) => fx.answer(cx, v),
        Action::Format(set) => {
            let refused = {
                let mut session = lock(cx.session);
                let (line, refused) = session.set_format(set);
                cx.sink.send(vec![line]);
                refused
            };
            if let Some(what) = refused {
                fx.refused(cx, what);
            }
        }
        Action::Sensors {
            tx,
            on,
            interval_ms,
        } => lock(cx.session).set_sensors(tx, on, interval_ms),
        Action::Dial(hz) => {
            let s = snapshot();
            if hz < s.vfo_limits.0 || hz > s.vfo_limits.1 {
                fx.refusal(cx, Refusal::Limits);
                fx.echo(cx, Out::Dial(s.dial));
            } else {
                match cx.backend.set_dial(cx.client, hz) {
                    Ok(dial) => fx.all.push(Out::Dial(dial)),
                    Err(r) => {
                        fx.refusal(cx, r);
                        fx.echo(cx, Out::Dial(s.dial));
                    }
                }
            }
        }
        Action::SubDial(hz) => {
            let s = snapshot();
            if hz != s.dial {
                fx.refused(cx, Refused::Split);
            }
            fx.answer(cx, Value::Vfo(Channel::B));
        }
        Action::Mode(mode) => {
            let s = snapshot();
            if !MODULATIONS.contains(&mode.as_str()) {
                fx.refused(cx, Refused::Mode);
                fx.echo(cx, Out::Mode(s.mode));
            } else {
                // Through the gate: the backend keeps what a program asked until its end and keys
                // its own audio only after a voice mode, and each server numbers its programs from
                // 1 again. An ask kept past the program's end would stand for the next program
                // given its number; the end waits for one in flight, then forgets it.
                match gated(cx, || cx.backend.set_mode(cx.client, &mode)) {
                    None => {}
                    Some(Ok(now)) => fx.all.push(Out::Mode(now)),
                    Some(Err(r)) => {
                        fx.refusal(cx, r);
                        fx.echo(cx, Out::Mode(s.mode));
                    }
                }
            }
        }
        Action::Drive(level) => match cx.backend.set_drive(cx.client, level) {
            Ok(now) => fx.all.push(Out::Drive(now)),
            Err(r) => {
                fx.refusal(cx, r);
                let now = snapshot().drive;
                fx.echo(cx, Out::Drive(now));
            }
        },
        Action::TuneDrive(level) => match cx.backend.set_tune_drive(cx.client, level) {
            Ok(now) => fx.all.push(Out::TuneDrive(now)),
            Err(r) => {
                fx.refusal(cx, r);
                let now = snapshot().tune_drive;
                fx.echo(cx, Out::TuneDrive(now));
            }
        },
        Action::Key(kind) => {
            let audio = lock(cx.session).audio;
            match gated(cx, || cx.backend.key(cx.client, kind, audio)) {
                None => {}
                Some(Ok(())) => {
                    lock(cx.session).admitted(kind);
                    // At admission, not when the radio keys: WSJT-X fails a key whose echo takes
                    // over a second, and a slow serial rig can take longer.
                    fx.all.push(Out::Trx(true));
                }
                Some(Err(r)) => {
                    fx.refusal(cx, r);
                    fx.echo(cx, Out::Trx(false));
                }
            }
        }
        Action::Release => {
            // The server stops taking the program's transmit audio before the backend hears the
            // release, never after.
            let told = {
                let mut session = lock(cx.session);
                session.key = None;
                session.told.trx()
            };
            cx.backend.release(cx.client);
            // A program told something is on the air hears `trx:0,false;` when the radio
            // unkeys (the next snapshot); one that was not told so gets its echo now.
            if told != Some(true) {
                fx.echo(cx, Out::Trx(false));
            }
        }
        Action::TuneOn => match gated(cx, || cx.backend.tune_on(cx.client)) {
            None => {}
            Some(Ok(())) => fx.all.push(Out::Tune(true)),
            Some(Err(r)) => {
                fx.refusal(cx, r);
                let now = snapshot().tune;
                fx.echo(cx, Out::Tune(now));
            }
        },
        Action::TuneOff => {
            cx.backend.tune_off(cx.client);
            fx.echo(cx, Out::Tune(false));
        }
        Action::EndOwn => {
            lock(cx.session).key = None;
            cx.backend.end_own(cx.client);
        }
        Action::AudioStart => {
            let format = lock(cx.session).format;
            match cx.backend.audio_start(cx.client, format) {
                Ok(()) => {
                    lock(cx.session).audio = true;
                    fx.say(
                        cx,
                        Notice::Stream(StreamSetting::AudioStart(trx0())).render(),
                    );
                }
                // No echo: WSJT-X waits for one, and reports the failure.
                Err(r) => fx.refusal(cx, r),
            }
        }
        Action::AudioStop => {
            cx.backend.audio_stop(cx.client);
            lock(cx.session).audio = false;
            fx.say(
                cx,
                Notice::Stream(StreamSetting::AudioStop(trx0())).render(),
            );
        }
        Action::Cw(cw) => {
            if let Some(Err(r)) = gated(cx, || cx.backend.cw(cx.client, cw)) {
                fx.refusal(cx, r);
            }
        }
        Action::CwStop => cx.backend.cw_stop(cx.client),
        Action::CwSpeed(speed) => {
            let wpm = match cx.backend.cw_speed(cx.client, speed) {
                Ok(wpm) => wpm,
                Err(r) => {
                    fx.refusal(cx, r);
                    snapshot().cw_speed
                }
            };
            fx.say(cx, Notice::State(Setting::CwMacrosSpeed(wpm)).render());
        }
        // Through the gate: a spot is never kept for a program whose end has begun, and the end
        // waits for one in flight, then takes it away with the program's others.
        Action::Spot(spot) => {
            if let Some(Err(r)) = gated(cx, || cx.backend.spot(cx.client, spot)) {
                fx.refusal(cx, r);
            }
        }
        Action::SpotDelete(call) => cx.backend.spot_delete(cx.client, &call),
        Action::SpotClear => cx.backend.spot_clear(cx.client),
        Action::Refuse(v, what) => {
            fx.answer(cx, v);
            fx.refused(cx, what);
        }
        Action::Ignore(what) => fx.refused(cx, what),
    }
    fx
}

fn trx0() -> Trx {
    Trx::new(0).unwrap_or_else(|| unreachable!("receiver 0"))
}
