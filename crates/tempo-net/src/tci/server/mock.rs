//! A backend that records every call and answers as a test sets it, for the server's tests (the
//! CAT broker's `MockBackend` pattern).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::tci::wire::{Call as Callsign, Hz, Word};

use super::backend::{
    AudioFormat, ClientId, Cw, CwSpeed, Event, KeyKind, Refusal, Spot, TciBackend,
};

/// One call the server made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Call {
    Connected(ClientId),
    Disconnected(ClientId),
    SetDial(ClientId, i64),
    SetMode(ClientId, String),
    SetDrive(ClientId, u32),
    SetTuneDrive(ClientId, u32),
    Key(ClientId, KeyKind, bool),
    Release(ClientId),
    TuneOn(ClientId),
    TuneOff(ClientId),
    EndOwn(ClientId),
    /// The frame's length in bytes.
    TxAudio(ClientId, usize),
    AudioStart(ClientId),
    AudioStop(ClientId),
    Cw(ClientId, Cw),
    CwStop(ClientId),
    CwSpeed(ClientId, CwSpeed),
    Spot(ClientId, String),
    SpotDelete(ClientId, String),
    SpotClear(ClientId),
}

impl Call {
    /// The calls that can put the transmitter on the air.
    pub(super) fn can_transmit(&self) -> bool {
        matches!(
            self,
            Call::Key(..) | Call::TuneOn(_) | Call::Cw(..) | Call::TxAudio(..)
        )
    }
}

/// How the mock answers.
#[derive(Debug, Clone)]
pub(super) struct Answers {
    /// `key`, `tune_on` and `cw`.
    pub(super) transmit: Result<(), Refusal>,
    pub(super) dial: Result<(), Refusal>,
    pub(super) mode: Result<(), Refusal>,
    pub(super) drive: Result<(), Refusal>,
    pub(super) audio: Result<(), Refusal>,
    pub(super) cw_speed: Result<u32, Refusal>,
}

impl Default for Answers {
    fn default() -> Self {
        Answers {
            transmit: Ok(()),
            dial: Ok(()),
            mode: Ok(()),
            drive: Ok(()),
            audio: Ok(()),
            cw_speed: Ok(25),
        }
    }
}

/// A call the mock holds inside the backend until the test lets it go: the server is then mid-call,
/// at a point the test chose.
#[derive(Debug, Default)]
struct Pause {
    at: Option<&'static str>,
    released: bool,
}

#[derive(Default)]
pub(super) struct Mock {
    calls: Mutex<Vec<Call>>,
    events: Mutex<Vec<Event>>,
    changed: Condvar,
    pub(super) answers: Mutex<Answers>,
    pause: Mutex<Pause>,
    resumed: Condvar,
    /// The mode each program last asked, kept as the app's engine keeps it (it keys a program's own
    /// audio only after a voice mode): by program number, from the ask until that program's
    /// `disconnected`, and never cleared when a program connects.
    asked: Mutex<HashMap<ClientId, String>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Mock {
    fn record(&self, call: Call) {
        lock(&self.calls).push(call);
        self.changed.notify_all();
    }

    pub(super) fn calls(&self) -> Vec<Call> {
        lock(&self.calls).clone()
    }

    pub(super) fn events(&self) -> Vec<Event> {
        lock(&self.events).clone()
    }

    pub(super) fn answers(&self) -> MutexGuard<'_, Answers> {
        lock(&self.answers)
    }

    /// The mode the backend keeps as `client`'s last ask, if any.
    pub(super) fn asked(&self, client: ClientId) -> Option<String> {
        lock(&self.asked).get(&client).cloned()
    }

    /// Hold the next calls named `at` (after recording them) until [`Mock::release`].
    pub(super) fn pause_at(&self, at: &'static str) {
        *lock(&self.pause) = Pause {
            at: Some(at),
            released: false,
        };
    }

    /// Let a held call return, and hold no more.
    pub(super) fn release(&self) {
        let mut pause = lock(&self.pause);
        pause.released = true;
        pause.at = None;
        self.resumed.notify_all();
    }

    /// Where a call may be held. Ten seconds at most, so a broken test cannot hang the suite.
    fn hold(&self, name: &'static str) {
        let mut pause = lock(&self.pause);
        if pause.at != Some(name) {
            return;
        }
        let until = Instant::now() + Duration::from_secs(10);
        while !pause.released && Instant::now() < until {
            pause = self
                .resumed
                .wait_timeout(pause, until.saturating_duration_since(Instant::now()))
                .map(|(g, _)| g)
                .unwrap_or_else(|e| e.into_inner().0);
        }
    }

    /// Wait until a call matches, or `within` passes. The matching call, if one came.
    pub(super) fn wait_call(&self, within: Duration, f: impl Fn(&Call) -> bool) -> Option<Call> {
        let until = Instant::now() + within;
        let mut calls = lock(&self.calls);
        loop {
            if let Some(c) = calls.iter().find(|c| f(c)) {
                return Some(c.clone());
            }
            let now = Instant::now();
            if now >= until {
                return None;
            }
            calls = self
                .changed
                .wait_timeout(calls, until - now)
                .map(|(g, _)| g)
                .unwrap_or_else(|e| e.into_inner().0);
        }
    }

    /// Wait until an event matches, or `within` passes.
    pub(super) fn wait_event(&self, within: Duration, f: impl Fn(&Event) -> bool) -> Option<Event> {
        let until = Instant::now() + within;
        let mut events = lock(&self.events);
        loop {
            if let Some(e) = events.iter().find(|e| f(e)) {
                return Some(e.clone());
            }
            let now = Instant::now();
            if now >= until {
                return None;
            }
            events = self
                .changed
                .wait_timeout(events, until - now)
                .map(|(g, _)| g)
                .unwrap_or_else(|e| e.into_inner().0);
        }
    }
}

impl TciBackend for Mock {
    fn connected(&self, client: ClientId, _peer: SocketAddr) {
        self.record(Call::Connected(client));
    }

    fn disconnected(&self, client: ClientId) {
        lock(&self.asked).remove(&client);
        self.record(Call::Disconnected(client));
    }

    fn event(&self, event: Event) {
        lock(&self.events).push(event);
        self.changed.notify_all();
    }

    fn set_dial(&self, client: ClientId, hz: Hz) -> Result<Hz, Refusal> {
        self.record(Call::SetDial(client, hz.get()));
        self.answers().dial.map(|()| hz)
    }

    fn set_mode(&self, client: ClientId, mode: &Word) -> Result<Word, Refusal> {
        self.record(Call::SetMode(client, mode.as_str().to_string()));
        self.hold("set_mode");
        // Kept whatever comes of the change, as the engine keeps it: after the call began, as the
        // engine's record waits on its lock.
        lock(&self.asked).insert(client, mode.as_str().to_string());
        self.answers().mode.map(|()| mode.clone())
    }

    fn set_drive(&self, client: ClientId, level: u32) -> Result<u32, Refusal> {
        self.record(Call::SetDrive(client, level));
        self.hold("set_drive");
        self.answers().drive.map(|()| level)
    }

    fn set_tune_drive(&self, client: ClientId, level: u32) -> Result<u32, Refusal> {
        self.record(Call::SetTuneDrive(client, level));
        self.answers().drive.map(|()| level)
    }

    fn key(&self, client: ClientId, kind: KeyKind, audio_started: bool) -> Result<(), Refusal> {
        self.record(Call::Key(client, kind, audio_started));
        self.hold("key");
        self.answers().transmit
    }

    fn release(&self, client: ClientId) {
        self.record(Call::Release(client));
        self.hold("release");
    }

    fn tune_on(&self, client: ClientId) -> Result<(), Refusal> {
        self.record(Call::TuneOn(client));
        self.answers().transmit
    }

    fn tune_off(&self, client: ClientId) {
        self.record(Call::TuneOff(client));
    }

    fn end_own(&self, client: ClientId) {
        self.record(Call::EndOwn(client));
    }

    fn tx_audio(&self, client: ClientId, _format: AudioFormat, frame: &[u8]) {
        self.record(Call::TxAudio(client, frame.len()));
        self.hold("tx_audio");
    }

    fn audio_start(&self, client: ClientId, _format: AudioFormat) -> Result<(), Refusal> {
        self.record(Call::AudioStart(client));
        self.answers().audio
    }

    fn audio_stop(&self, client: ClientId) {
        self.record(Call::AudioStop(client));
    }

    fn cw(&self, client: ClientId, cw: Cw) -> Result<(), Refusal> {
        self.record(Call::Cw(client, cw));
        self.answers().transmit
    }

    fn cw_stop(&self, client: ClientId) {
        self.record(Call::CwStop(client));
    }

    fn cw_speed(&self, client: ClientId, speed: CwSpeed) -> Result<u32, Refusal> {
        self.record(Call::CwSpeed(client, speed));
        self.answers().cw_speed
    }

    fn spot(&self, client: ClientId, spot: Spot) -> Result<(), Refusal> {
        self.record(Call::Spot(client, spot.call.as_str().to_string()));
        self.hold("spot");
        Ok(())
    }

    fn spot_delete(&self, client: ClientId, call: &Callsign) {
        self.record(Call::SpotDelete(client, call.as_str().to_string()));
    }

    fn spot_clear(&self, client: ClientId) {
        self.record(Call::SpotClear(client));
    }
}
