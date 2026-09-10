//! Read-only provenance, independent of desktop mirrors and the transmitter arbiter.
//! A producer binds the transport it actually owns before starting I/O. A settings
//! change or connection restart retires its tokens, including an away-and-back switch.

use super::{bounded, Amplifier, ReadAge, MAX_SEQUENCE, MEASUREMENT_STALE_MS, MODE_STALE_MS};
use crate::dto::AmpStatusDto;
use std::sync::Arc;
use std::time::Instant;

/// Opaque process-local authority to report observations, never to operate hardware.
#[derive(Clone, Debug)]
pub struct Connection {
    owner: Arc<()>,
    radio_id: u32,
    generation: u64,
}

/// Assigned before a transport read, so slow I/O cannot acquire a newer identity.
#[derive(Clone, Debug)]
pub struct Read {
    connection: Connection,
    sequence: u64,
    started: Instant,
}

#[derive(Default)]
struct Channel {
    owner: Arc<()>,
    current: Option<Connection>,
    generation: u64,
    sequence: u64,
}

impl Channel {
    fn open(&mut self, radio_id: u32) -> Option<Connection> {
        self.current = None;
        self.generation = self
            .generation
            .checked_add(1)
            .filter(|n| *n <= MAX_SEQUENCE)?;
        self.sequence = 0;
        let connection = Connection {
            owner: self.owner.clone(),
            radio_id,
            generation: self.generation,
        };
        self.current = Some(connection.clone());
        Some(connection)
    }

    fn accepts(&self, connection: &Connection) -> bool {
        self.current.as_ref().is_some_and(|current| {
            Arc::ptr_eq(&current.owner, &connection.owner)
                && current.radio_id == connection.radio_id
                && current.generation == connection.generation
        })
    }

    fn read(&mut self, connection: &Connection, started: Instant) -> Option<Read> {
        if !self.accepts(connection) {
            return None;
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .filter(|n| *n <= MAX_SEQUENCE)?;
        Some(Read {
            connection: connection.clone(),
            sequence: self.sequence,
            started,
        })
    }
}

struct Sample<T> {
    read: Read,
    value: Option<T>,
}

impl<T> Sample<T> {
    fn accept(
        slot: &mut Option<Self>,
        channel: &Channel,
        read: Option<&Read>,
        value: Option<T>,
    ) -> bool {
        let Some(read) = read.filter(|r| channel.accepts(&r.connection)) else {
            return false;
        };
        if slot
            .as_ref()
            .is_some_and(|old| old.read.sequence >= read.sequence)
        {
            return false;
        }
        // Keep a tombstone for a failed/unsupported read: a delayed older success
        // must not resurrect it. Each field has its own order and measurement age.
        *slot = Some(Self {
            read: read.clone(),
            value,
        });
        true
    }

    fn current(&self, channel: &Channel, now: Instant, limit_ms: u64) -> Option<(&T, ReadAge)> {
        if !channel.accepts(&self.read.connection) {
            return None;
        }
        let age_ms = now.checked_duration_since(self.read.started)?.as_millis();
        if age_ms >= u128::from(limit_ms) {
            return None;
        }
        Some((
            self.value.as_ref()?,
            ReadAge {
                connection_generation: self.read.connection.generation,
                read_sequence: self.read.sequence,
                age_ms: age_ms as u64,
            },
        ))
    }
}

#[derive(Default)]
pub(crate) struct Observations {
    radio: Channel,
    amp: Channel,
    cat: Option<Sample<bool>>,
    dial: Option<Sample<f64>>,
    mode: Option<Sample<String>>,
    ptt: Option<Sample<bool>>,
    amplifier: Option<Sample<AmpStatusDto>>,
}

impl Observations {
    pub(crate) fn invalidate(&mut self) {
        self.invalidate_radio();
        self.amp.current = None;
        self.amplifier = None;
    }

    pub(crate) fn invalidate_radio(&mut self) {
        self.radio.current = None;
        self.cat = None;
        self.dial = None;
        self.mode = None;
        self.ptt = None;
    }

    pub(crate) fn open_radio(&mut self, id: u32) -> Option<Connection> {
        self.invalidate_radio();
        self.radio.open(id)
    }

    pub(crate) fn open_amp(&mut self, id: u32) -> Option<Connection> {
        self.amplifier = None;
        self.amp.open(id)
    }

    pub(crate) fn radio_read(&mut self, connection: &Connection, now: Instant) -> Option<Read> {
        self.radio.read(connection, now)
    }

    pub(crate) fn amp_read(&mut self, connection: &Connection, now: Instant) -> Option<Read> {
        self.amp.read(connection, now)
    }

    pub(crate) fn cat(&mut self, read: Option<&Read>, value: Option<bool>) {
        Sample::accept(&mut self.cat, &self.radio, read, value);
        if value != Some(true) {
            Sample::accept(&mut self.dial, &self.radio, read, None);
            Sample::accept(&mut self.mode, &self.radio, read, None);
            Sample::accept(&mut self.ptt, &self.radio, read, None);
        }
    }

    pub(crate) fn dial(&mut self, read: Option<&Read>, hz: Option<u64>) {
        Sample::accept(
            &mut self.dial,
            &self.radio,
            read,
            hz.map(|hz| hz as f64 / 1e6),
        );
    }

    pub(crate) fn mode(&mut self, read: Option<&Read>, value: Option<&str>) {
        Sample::accept(&mut self.mode, &self.radio, read, value.map(bounded));
    }

    /// Reports an accepted transition to keyed, not another poll of steady PTT.
    pub(crate) fn ptt(&mut self, read: Option<&Read>, value: Option<bool>) -> bool {
        let was_keyed = self
            .ptt
            .as_ref()
            .is_some_and(|s| self.radio.accepts(&s.read.connection) && s.value == Some(true));
        Sample::accept(&mut self.ptt, &self.radio, read, value) && value == Some(true) && !was_keyed
    }

    pub(crate) fn amp(&mut self, read: Option<&Read>, value: AmpStatusDto) {
        Sample::accept(&mut self.amplifier, &self.amp, read, Some(value));
    }

    /// The exact completed poll, still owned by this connection. A matching
    /// model/port string cannot revive a retired read after an away-and-back edit.
    pub(crate) fn amp_for_read(
        &self,
        read: &Read,
        now: Instant,
    ) -> Option<(&AmpStatusDto, ReadAge)> {
        if !self.amp.accepts(&read.connection) {
            return None;
        }
        let sample = self.amplifier.as_ref()?;
        if sample.read.sequence != read.sequence {
            return None;
        }
        sample.current(&self.amp, now, MEASUREMENT_STALE_MS)
    }

    pub(crate) fn project_radio(&self, radio: &mut super::Radio, now: Instant) {
        if let Some((value, age)) = self
            .cat
            .as_ref()
            .and_then(|s| s.current(&self.radio, now, MEASUREMENT_STALE_MS))
        {
            radio.cat_connected = Some(*value);
            radio.readings.cat = Some(age);
        }
        if let Some((value, age)) = self
            .dial
            .as_ref()
            .and_then(|s| s.current(&self.radio, now, MEASUREMENT_STALE_MS))
        {
            radio.rig_dial_mhz = Some(*value);
            radio.readings.dial = Some(age);
        }
        if let Some((value, age)) = self
            .mode
            .as_ref()
            .and_then(|s| s.current(&self.radio, now, MODE_STALE_MS))
        {
            radio.rig_mode = Some(value.clone());
            radio.readings.mode = Some(age);
        }
        if let Some((value, age)) = self
            .ptt
            .as_ref()
            .and_then(|s| s.current(&self.radio, now, MEASUREMENT_STALE_MS))
        {
            radio.rig_keyed = Some(*value);
            radio.readings.ptt = Some(age);
        }
    }

    pub(crate) fn project_amp(&self, family: &str, follow: bool, now: Instant) -> Amplifier {
        let current = self
            .amplifier
            .as_ref()
            .and_then(|s| s.current(&self.amp, now, MEASUREMENT_STALE_MS));
        if let Some((value, age)) = current.filter(|(v, _)| v.family == family) {
            let mut amp = Amplifier::from_status(value, follow);
            amp.reading = Some(age);
            amp
        } else {
            Amplifier::from_status(
                &AmpStatusDto {
                    family: family.into(),
                    ..Default::default()
                },
                follow,
            )
        }
    }
}
