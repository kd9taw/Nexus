//! The status model: what the radio has reported, folded from decoded status lines.
//!
//! It holds the radio, its slices, pans, waterfalls, meters, transmitter settings, interlock,
//! ATU, profiles, connected clients and streams, each as the latest present-only delta
//! ([`super::status`]). It is Nexus's own, not a port: AetherSDR's model layer emits wire
//! commands from its setters, and that is the design this client keeps out. Here the model has
//! no write path at all. [`StatusModel::apply`] takes a decoded line and returns nothing, so no
//! status line can reach the radio through it.
//!
//! **Ownership is recorded, never assumed.** Every slice, pan, waterfall and stream keeps the
//! `client_handle` the radio reported for it, and [`StatusModel::owner`] says whether that is this
//! connection ([`Owner::Ours`]), another client ([`Owner::Foreign`]) or not yet known
//! ([`Owner::Unknown`]). Another client's objects are kept, so they can be shown as theirs; what
//! Nexus may do to an object is decided from this ([`super::session`] refuses any command aimed
//! at an object that is not ours).
//!
//! **The interlock sample is never merged.** A line carrying all five state keys replaces the
//! sample; a line carrying only some of them withdraws it ([`InterlockState::sample`] becomes
//! `None`) until a whole one arrives, because the radio's interlock updates are deltas and fields
//! from different lines do not describe one moment.

use std::collections::BTreeMap;

use super::status::{
    AtuDelta, ClientAction, ClientDelta, Decoded, InterlockConfig, InterlockDelta, InterlockSample,
    MeterDef, PanDelta, ProfileDelta, RadioDelta, SliceDelta, StreamDelta, TransmitDelta,
    WaterfallDelta,
};

/// Whose an object is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// This connection's client handle.
    Ours,
    /// Another client's handle.
    Foreign(u32),
    /// No owner reported yet, an owner of zero, or this connection has no handle.
    Unknown,
}

/// Classify a reported owner against this connection's handle. An owner of zero is no client,
/// and without a handle of our own nothing can be ours.
pub fn owner_of(client_handle: Option<u32>, ours: Option<u32>) -> Owner {
    match (client_handle, ours) {
        (Some(h), Some(o)) if h != 0 && h == o => Owner::Ours,
        (Some(h), _) if h != 0 && Some(h) != ours => Owner::Foreign(h),
        _ => Owner::Unknown,
    }
}

/// The interlock: the last whole state sample, the transmitter's last reported owner, and the
/// timing configuration.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InterlockState {
    /// The last whole sample, or `None` once a partial line has made it stale.
    pub sample: Option<InterlockSample>,
    /// `tx_client_handle` from the last line that carried a well-formed one. Unlike the sample it
    /// survives a partial line: it answers "whose transmitter was it last said to be".
    pub last_tx_client_handle: Option<u32>,
    pub amplifier: Option<String>,
    pub config: InterlockConfig,
}

impl InterlockState {
    fn apply(&mut self, d: &InterlockDelta) {
        if d.touches_state {
            self.sample = d.sample.clone();
        }
        if d.tx_client_handle.is_some() {
            self.last_tx_client_handle = d.tx_client_handle;
        }
        if d.amplifier.is_some() {
            self.amplifier = d.amplifier.clone();
        }
        self.config.merge(&d.config);
    }
}

/// One profile type's list and current entry (`tx`, `mic`, `global`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileState {
    pub list: Option<Vec<String>>,
    pub current: Option<String>,
}

/// Everything the radio has reported on this connection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatusModel {
    pub radio: RadioDelta,
    pub slices: BTreeMap<u8, SliceDelta>,
    pub pans: BTreeMap<u32, PanDelta>,
    pub waterfalls: BTreeMap<u32, WaterfallDelta>,
    pub meters: BTreeMap<u16, MeterDef>,
    pub transmit: TransmitDelta,
    pub interlock: InterlockState,
    pub atu: AtuDelta,
    /// By type.
    pub profiles: BTreeMap<String, ProfileState>,
    pub profiles_importing: Option<bool>,
    pub profiles_exporting: Option<bool>,
    pub clients: BTreeMap<u32, ClientDelta>,
    pub streams: BTreeMap<u32, StreamDelta>,
    /// The radio's mic inputs, from the `mic list` reply.
    pub mic_inputs: Option<Vec<String>>,
}

impl StatusModel {
    /// Fold one decoded status line in. Present-only: a field the line did not report keeps its
    /// value. A removal drops the object. Returns nothing: the model cannot send.
    pub fn apply(&mut self, decoded: &Decoded) {
        match decoded {
            Decoded::Radio(d) => self.radio.merge(d),
            Decoded::Slice {
                index,
                delta,
                removed,
            } => {
                if *removed {
                    self.slices.remove(index);
                } else {
                    self.slices.entry(*index).or_default().merge(delta);
                }
            }
            Decoded::Pan { id, delta, removed } => {
                // The radio does not free a pan's waterfall with it; the waterfall's own removal
                // arrives separately, so it is not dropped here.
                if *removed {
                    self.pans.remove(id);
                } else {
                    self.pans.entry(*id).or_default().merge(delta);
                }
            }
            Decoded::Waterfall { id, delta, removed } => {
                if *removed {
                    self.waterfalls.remove(id);
                } else {
                    self.waterfalls.entry(*id).or_default().merge(delta);
                }
            }
            Decoded::Transmit(d) => self.transmit.merge(d),
            Decoded::Interlock(d) => self.interlock.apply(d),
            Decoded::Atu(d) => self.atu.merge(d),
            Decoded::Meters(defs) => {
                for def in defs {
                    // A definition is complete as sent: it replaces, it does not merge.
                    self.meters.insert(def.index, def.clone());
                }
            }
            Decoded::MeterRemoved(index) => {
                self.meters.remove(index);
            }
            Decoded::Profile(d) => self.apply_profile(d),
            Decoded::Client {
                handle,
                action,
                delta,
            } => match action {
                ClientAction::Disconnected => {
                    self.clients.remove(handle);
                }
                ClientAction::Connected | ClientAction::Update => {
                    self.clients.entry(*handle).or_default().merge(delta);
                }
            },
            Decoded::Stream { id, delta, removed } => {
                if *removed {
                    self.streams.remove(id);
                } else {
                    self.streams.entry(*id).or_default().merge(delta);
                }
            }
            Decoded::Other => {}
        }
    }

    fn apply_profile(&mut self, d: &ProfileDelta) {
        if d.importing.is_some() {
            self.profiles_importing = d.importing;
        }
        if d.exporting.is_some() {
            self.profiles_exporting = d.exporting;
        }
        if d.list.is_some() || d.current.is_some() {
            let entry = self.profiles.entry(d.kind.clone()).or_default();
            if d.list.is_some() {
                entry.list = d.list.clone();
            }
            if d.current.is_some() {
                entry.current = d.current.clone();
            }
        }
    }

    /// The owner of an object, by what the radio reported for it.
    pub fn owner(&self, object: ObjectRef, ours: Option<u32>) -> Owner {
        let reported = match object {
            ObjectRef::Slice(i) => self.slices.get(&i).and_then(|s| s.client_handle),
            ObjectRef::Pan(id) => self.pans.get(&id).and_then(|p| p.client_handle),
            ObjectRef::Waterfall(id) => self.waterfalls.get(&id).and_then(|w| w.client_handle),
            ObjectRef::Stream(id) => self.streams.get(&id).and_then(|s| s.client_handle),
        };
        owner_of(reported, ours)
    }

    /// The slices the radio reports as the transmit slice (`tx=1`). One radio has one; more than
    /// one is a contradiction the caller must refuse on.
    pub fn tx_slices(&self) -> Vec<u8> {
        self.slices
            .iter()
            .filter(|(_, s)| s.tx == Some(true))
            .map(|(i, _)| *i)
            .collect()
    }

    /// Whether the radio reports an antenna tuner fitted: its `atu` status says
    /// `atu_enabled=1`. Some radios have the tuner as an option (the FLEX-6300). ⚠️ Which key says
    /// a tuner is fitted is not documented: `atu_enabled` stands in until a tester's bench, on a
    /// radio with the tuner and on one without it. A radio that has not said so has none here.
    pub fn atu_fitted(&self) -> bool {
        self.atu.atu_enabled == Some(true)
    }

    /// The streams this connection owns, for teardown.
    pub fn our_streams(&self, ours: Option<u32>) -> Vec<u32> {
        self.streams
            .iter()
            .filter(|(_, s)| owner_of(s.client_handle, ours) == Owner::Ours)
            .map(|(id, _)| *id)
            .collect()
    }
}

/// An object a command can be aimed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectRef {
    Slice(u8),
    Pan(u32),
    Waterfall(u32),
    Stream(u32),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flex::status::decode;
    use crate::flex::wire::{parse_line, Line};

    const OURS: u32 = 0x2B6E_1F40;

    fn feed(model: &mut StatusModel, lines: &[&str]) {
        for line in lines {
            match parse_line(line) {
                Ok(Line::Status(s)) => model.apply(&decode(&s)),
                other => panic!("{line:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn owners_are_classified_against_our_handle() {
        assert_eq!(owner_of(Some(OURS), Some(OURS)), Owner::Ours);
        assert_eq!(
            owner_of(Some(0x7A3C_0001), Some(OURS)),
            Owner::Foreign(0x7A3C_0001)
        );
        assert_eq!(owner_of(None, Some(OURS)), Owner::Unknown);
        assert_eq!(
            owner_of(Some(0), Some(OURS)),
            Owner::Unknown,
            "zero is no client"
        );
        // Without a handle of our own, nothing is ours, and a reported owner is someone else.
        assert_eq!(owner_of(Some(OURS), None), Owner::Foreign(OURS));
        assert_eq!(owner_of(Some(0), None), Owner::Unknown);
        assert_eq!(
            owner_of(Some(0), Some(0)),
            Owner::Unknown,
            "a zero handle owns nothing"
        );
    }

    #[test]
    fn present_only_merge_and_removal() {
        let mut m = StatusModel::default();
        feed(
            &mut m,
            &[
                "S2B6E1F40|slice 0 in_use=1 RF_frequency=14.074000 mode=DIGU tx=1 client_handle=0x2B6E1F40",
                "S2B6E1F40|slice 0 mode=USB",
                "S2B6E1F40|slice 0 RF_frequency=garbage",
            ],
        );
        let s = &m.slices[&0];
        assert_eq!(s.mode.as_deref(), Some("USB"));
        assert_eq!(
            s.frequency_mhz,
            Some(14.074),
            "a malformed field leaves the value"
        );
        assert_eq!(m.owner(ObjectRef::Slice(0), Some(OURS)), Owner::Ours);
        assert_eq!(m.tx_slices(), [0]);
        feed(&mut m, &["S0|slice 0 in_use=0"]);
        assert!(m.slices.is_empty());
        assert_eq!(m.owner(ObjectRef::Slice(0), Some(OURS)), Owner::Unknown);
    }

    #[test]
    fn foreign_objects_are_kept_as_theirs() {
        let mut m = StatusModel::default();
        feed(
            &mut m,
            &[
                "S7A3C0001|slice 1 in_use=1 tx=1 client_handle=0x7A3C0001 pan=0x40000001",
                "S7A3C0001|display pan 0x40000001 client_handle=0x7A3C0001 waterfall=0x42000001",
                "S7A3C0001|display waterfall 0x42000001 client_handle=0x7A3C0001 panadapter=0x40000001",
            ],
        );
        for object in [
            ObjectRef::Slice(1),
            ObjectRef::Pan(0x4000_0001),
            ObjectRef::Waterfall(0x4200_0001),
        ] {
            assert_eq!(m.owner(object, Some(OURS)), Owner::Foreign(0x7A3C_0001));
        }
        // A pan's removal does not take its waterfall: the radio reports that separately.
        feed(&mut m, &["S0|display pan 0x40000001 removed"]);
        assert!(m.pans.is_empty());
        assert_eq!(m.waterfalls.len(), 1);
    }

    #[test]
    fn a_partial_interlock_line_withdraws_the_sample() {
        let mut m = StatusModel::default();
        let idle = "S0|interlock tx_client_handle=0x00000000 state=READY reason= source= tx_allowed=1 amplifier=";
        feed(&mut m, &[idle]);
        assert_eq!(m.interlock.sample.as_ref().unwrap().state, "READY");
        // A partial line: the old sample no longer describes the radio.
        feed(&mut m, &["S0|interlock tx_allowed=1"]);
        assert_eq!(m.interlock.sample, None);
        // Configuration does not touch the sample.
        feed(&mut m, &[idle, "S0|interlock timeout=120 tx_delay=0"]);
        assert!(m.interlock.sample.is_some());
        assert_eq!(m.interlock.config.timeout, Some(120));
        // The last owner survives a partial line that names no owner.
        feed(
            &mut m,
            &[
                "S0|interlock tx_client_handle=0x2B6E1F40 state=TRANSMITTING reason= source=SW tx_allowed=1",
                "S0|interlock state=UNKEY_REQUESTED",
            ],
        );
        assert_eq!(m.interlock.sample, None);
        assert_eq!(m.interlock.last_tx_client_handle, Some(OURS));
    }

    #[test]
    fn meters_profiles_clients_and_streams() {
        let mut m = StatusModel::default();
        feed(
            &mut m,
            &[
                "S0|meter 1.src=TX-#1.nam=FWDPWR#2.src=TX-#2.nam=SWR#",
                "S0|meter 1 removed",
                "S0|profile tx list=Default^DX",
                "S0|profile tx current=DX",
                "S0|profile importing=1",
                "S7A3C0001|client 0x7A3C0001 connected program=SmartSDR-Win",
                "S2B6E1F40|stream 0x04000001 type=dax_rx client_handle=0x2B6E1F40 ip=127.0.0.1",
                "S7A3C0001|stream 0x04000002 type=dax_rx client_handle=0x7A3C0001 ip=127.0.0.1",
            ],
        );
        assert_eq!(m.meters.keys().copied().collect::<Vec<_>>(), [2]);
        let tx = &m.profiles["tx"];
        assert_eq!(
            tx.list.as_deref(),
            Some(&["Default".to_string(), "DX".to_string()][..])
        );
        assert_eq!(tx.current.as_deref(), Some("DX"));
        assert_eq!(m.profiles_importing, Some(true));
        assert!(m.clients.contains_key(&0x7A3C_0001));
        assert_eq!(m.our_streams(Some(OURS)), [0x0400_0001]);
        feed(
            &mut m,
            &[
                "S0|client 0x7A3C0001 disconnected",
                "S0|stream 0x04000001 removed",
            ],
        );
        assert!(m.clients.is_empty());
        assert!(m.our_streams(Some(OURS)).is_empty());
    }
}
