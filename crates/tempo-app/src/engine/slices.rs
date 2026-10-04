//! ⭐ A FLEXRADIO'S SLICES — the numbered half of the receiver model, and the typed intents for
//! every slice the radio's dial does not reach.
//!
//! ## Where they come from
//! Only Nexus's own Flex client can see them (`tempo_audio::flex`, opt-in per radio): it is a
//! client of the radio's SmartSDR API and reads every slice's status. The radio loop hands the
//! engine that client's view on its poll ([`Engine::observe_flex_slices`]); with any other CAT
//! path (SmartSDR CAT, Hamlib) there is no report, the receiver set is empty and every intent is
//! refused as unreachable. Each slice becomes a [`Receiver`] keyed [`RxId::Slice`] in
//! [`super::receivers::Receivers::set`].
//!
//! ## Two doors, one per kind of slice
//! - **The transmit slice is the radio's dial.** The client serves it to the radio loop as
//!   rigctld, so every cockpit, the CAT broker and every transmit gate reach it exactly as they
//!   reach any radio's dial. An intent aimed at it here is REFUSED: retuning or re-moding the
//!   transmitter around the dial path would step around the licence gate, which judges the dial.
//! - **Every other slice of ours** is reached here, by a [`SliceIntent`], and only here.
//!
//! Another client's slice is never touched: an intent aimed at one is refused. The radio loop
//! takes intents only while nothing is keyed, and the client checks the transmit flag and the
//! owner again at the wire against the radio's latest report, because either can move between the
//! request and the write.
//!
//! Nothing here keys the radio or can: every intent is a receive setting, and moving the transmit
//! flag is not one of them.
//!
//! ## The readback is the radio's
//! A Flex reports every change to a slice, so nothing here records what the radio accepted: the
//! next report shows it. A refusal is kept ([`Engine::last_slice_refusal`]) so it can be said.
//!
//! ## Whose
//! Like the Sub's controls, everything here belongs to one (radio id, Hamlib model): a radio
//! switch starts from nothing known, and an answer taken for one radio never lands on another.

use serde::{Deserialize, Serialize};

use super::receivers::{Receiver, RxId, RxOwner, RxStages};
use super::Engine;
use crate::bandplan::band_for_dial;

/// One slice as Nexus's Flex client last reported it. `None` = the radio has not said.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceReport {
    /// The radio's index for the slice (0 is slice A).
    pub index: u8,
    /// The radio's letter for it.
    pub letter: Option<String>,
    pub owner: RxOwner,
    /// The radio's transmit slice (`tx=1`).
    pub transmits: bool,
    pub dial_mhz: Option<f64>,
    /// The mode, in the rigctld words Nexus uses for every radio (`PKTUSB`, `USB`, `CW`, …), or
    /// the radio's own word where rigctld has none.
    pub mode: Option<String>,
    /// The receive filter's edges, Hz from the slice's frequency.
    pub filter_low_hz: Option<i32>,
    pub filter_high_hz: Option<i32>,
    /// The AGC speed, in the engine's words (`fast`, `mid`, `slow`, `off`).
    pub agc: Option<String>,
    pub nb: Option<bool>,
    pub nr: Option<bool>,
    /// The automatic notch.
    pub anf: Option<bool>,
    /// Audio gain, 0..1.
    pub af_gain: Option<f32>,
    pub muted: Option<bool>,
    /// The RIT offset the radio applies, Hz; `0` while RIT is off.
    pub rit_hz: Option<i32>,
}

/// What Nexus may ask of a slice of ours that is not the transmit slice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SliceIntent {
    /// Tune the slice, MHz.
    Tune {
        mhz: f64,
    },
    /// The mode, in rigctld words (`USB`, `LSB`, `CW`, `PKTUSB`, `PKTLSB`, `AM`, `FM`, …).
    Mode {
        mode: String,
    },
    /// The receive filter's edges, Hz from the slice's frequency.
    Filter {
        low_hz: i32,
        high_hz: i32,
    },
    /// The AGC speed: `fast`, `mid`, `slow` or `off`.
    Agc {
        speed: String,
    },
    /// Audio gain, 0..1.
    AfGain {
        gain: f32,
    },
    Mute {
        muted: bool,
    },
    NoiseBlanker {
        on: bool,
    },
    NoiseReduction {
        on: bool,
    },
    /// The automatic notch.
    AutoNotch {
        on: bool,
    },
}

/// The AGC speeds a slice has a word for; the engine's `auto` has none on a Flex.
const SLICE_AGC_SPEEDS: [&str; 4] = ["fast", "mid", "slow", "off"];

impl SliceIntent {
    /// Whether the value can be sent at all, before anyone looks at the radio.
    fn valid(&self) -> bool {
        match self {
            // 100 GHz bounds nonsense, not a band plan; the radio answers for its coverage.
            SliceIntent::Tune { mhz } => mhz.is_finite() && *mhz > 0.0 && *mhz <= 100_000.0,
            SliceIntent::Mode { mode } => {
                (1..=8).contains(&mode.len())
                    && mode
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            }
            SliceIntent::Filter { low_hz, high_hz } => {
                low_hz < high_hz && low_hz.abs() <= 100_000 && high_hz.abs() <= 100_000
            }
            SliceIntent::Agc { speed } => SLICE_AGC_SPEEDS.contains(&speed.as_str()),
            SliceIntent::AfGain { gain } => gain.is_finite() && (0.0..=1.0).contains(gain),
            SliceIntent::Mute { .. }
            | SliceIntent::NoiseBlanker { .. }
            | SliceIntent::NoiseReduction { .. }
            | SliceIntent::AutoNotch { .. } => true,
        }
    }
}

/// Why a slice intent was refused, each a reason it could not reach the slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceRefusal {
    /// The radio in play is not served by Nexus's own Flex client, or it has not reported yet.
    NoClient,
    /// The radio reports no slice with this index.
    NoSuchSlice(u8),
    /// Another client's slice, or one whose owner the radio has not said.
    NotOurs(u8),
    /// The transmit slice: it follows the radio's dial, and the transmit gates with it.
    TransmitSlice(u8),
    /// A value that cannot be sent: a frequency or gain out of range, a mode that is not a mode
    /// word, an AGC speed a slice has no word for, a filter whose low edge is not below its high.
    Invalid,
}

impl std::fmt::Display for SliceRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let letter = |i: u8| char::from(b'A'.saturating_add(i));
        match self {
            SliceRefusal::NoClient => f.write_str(
                "this radio's slices need Nexus's own Flex client, which is not connected",
            ),
            SliceRefusal::NoSuchSlice(i) => write!(f, "the radio has no slice {}", letter(*i)),
            SliceRefusal::NotOurs(i) => write!(
                f,
                "slice {} belongs to another program on the radio — Nexus leaves it alone",
                letter(*i)
            ),
            SliceRefusal::TransmitSlice(i) => write!(
                f,
                "slice {} is the transmit slice — tune it with the radio's dial",
                letter(*i)
            ),
            SliceRefusal::Invalid => f.write_str("not a value a slice can take"),
        }
    }
}

/// An intent the radio loop is to send, and whose radio it was taken for.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceWrite {
    owner: (u32, u32),
    pub index: u8,
    pub intent: SliceIntent,
}

/// The slices of one (radio id, Hamlib model).
#[derive(Debug, Default)]
pub struct FlexSlices {
    /// Whose slices these are. `None` = nothing learned yet.
    owner: Option<(u32, u32)>,
    /// The client's last report; `None` = no client serves this radio.
    report: Option<Vec<SliceReport>>,
    /// Intents the loop has not taken yet, in order; the latest of a kind per slice wins.
    pending: Vec<(u8, SliceIntent)>,
    /// The last intent the radio or the client refused, and why.
    refused: Option<(u8, String)>,
}

impl FlexSlices {
    /// This state for `owner`, started from nothing when it belongs to another radio.
    fn for_owner(&mut self, owner: (u32, u32)) -> &mut Self {
        if self.owner != Some(owner) {
            *self = FlexSlices {
                owner: Some(owner),
                ..FlexSlices::default()
            };
        }
        self
    }

    /// This state, only if it is `owner`'s.
    fn of(&self, owner: (u32, u32)) -> Option<&Self> {
        (self.owner == Some(owner)).then_some(self)
    }
}

impl Engine {
    /// The radio in play, as the slices are keyed: its id and its Hamlib model.
    fn slices_owner(&self) -> (u32, u32) {
        (self.settings.active_radio, self.settings.rig_model)
    }

    /// What Nexus's Flex client reports for the radio in play, on the radio loop's poll: every
    /// slice the radio has, or `None` when no client of ours serves this radio. `None` also drops
    /// whatever was waiting: nothing could carry it.
    pub fn observe_flex_slices(&mut self, report: Option<Vec<SliceReport>>) {
        let owner = self.slices_owner();
        let state = self.flex_slices.for_owner(owner);
        if report.is_none() {
            state.pending.clear();
        }
        state.report = report;
    }

    /// Ask for a change to a slice of ours that is not the transmit slice. The radio loop sends
    /// it while nothing is keyed; the radio's next report shows the result.
    ///
    /// REFUSED, with the reason, where it could not or must not reach the slice: no Flex client
    /// serving this radio, no such slice, another client's slice, the transmit slice, or a value
    /// that cannot be sent.
    pub fn request_slice(&mut self, index: u8, intent: SliceIntent) -> Result<(), SliceRefusal> {
        if !intent.valid() {
            return Err(SliceRefusal::Invalid);
        }
        let owner = self.slices_owner();
        let report = self
            .flex_slices
            .of(owner)
            .and_then(|s| s.report.as_ref())
            .ok_or(SliceRefusal::NoClient)?;
        let slice = report
            .iter()
            .find(|s| s.index == index)
            .ok_or(SliceRefusal::NoSuchSlice(index))?;
        if slice.owner != RxOwner::Ours {
            return Err(SliceRefusal::NotOurs(index));
        }
        if slice.transmits {
            return Err(SliceRefusal::TransmitSlice(index));
        }
        let state = self.flex_slices.for_owner(owner);
        let kind = std::mem::discriminant(&intent);
        state
            .pending
            .retain(|(i, p)| !(*i == index && std::mem::discriminant(p) == kind));
        state.pending.push((index, intent));
        Ok(())
    }

    /// The intents waiting for the radio loop, in the order they were asked, each carrying the
    /// radio it was taken for.
    pub fn take_slice_writes(&mut self) -> Vec<SliceWrite> {
        let owner = self.slices_owner();
        if self.flex_slices.owner != Some(owner) {
            return Vec::new();
        }
        std::mem::take(&mut self.flex_slices.pending)
            .into_iter()
            .map(|(index, intent)| SliceWrite {
                owner,
                index,
                intent,
            })
            .collect()
    }

    /// What became of a write the loop sent: `Err` with the reason when the client or the radio
    /// refused it. Credited only to the radio the write was taken for (the loop writes with the
    /// engine lock released, and a radio switch can land in between).
    pub fn observe_slice_write(&mut self, write: &SliceWrite, outcome: Result<(), String>) {
        if self.slices_owner() != write.owner || self.flex_slices.owner != Some(write.owner) {
            return;
        }
        if let Err(why) = outcome {
            self.flex_slices.refused = Some((write.index, why));
        }
    }

    /// The last slice intent that was refused after it was taken, and why: the slice's index and
    /// the sentence.
    pub fn last_slice_refusal(&self) -> Option<(u8, String)> {
        self.flex_slices
            .of(self.slices_owner())
            .and_then(|s| s.refused.clone())
    }

    /// The radio's slices as receivers, in the radio's order; empty with no report.
    pub(super) fn slice_receivers(&self) -> Vec<Receiver> {
        self.flex_slices
            .of(self.slices_owner())
            .and_then(|s| s.report.as_ref())
            .map(|report| report.iter().map(receiver_of).collect())
            .unwrap_or_default()
    }
}

/// One reported slice as a receiver: what the radio said, and nothing else.
fn receiver_of(s: &SliceReport) -> Receiver {
    let mut r = Receiver::unread(RxId::Slice(s.index), RxStages::FLEX_SLICE);
    r.letter = s.letter.clone();
    r.owner = Some(s.owner);
    r.transmits = Some(s.transmits);
    r.muted = s.muted;
    r.dial_mhz = s.dial_mhz;
    r.band = s
        .dial_mhz
        .map(|mhz| band_for_dial(mhz).unwrap_or("").to_string());
    r.rig_mode = s.mode.clone();
    r.filter_width_hz = match (s.filter_low_hz, s.filter_high_hz) {
        (Some(low), Some(high)) if high > low => u32::try_from(high - low).ok(),
        _ => None,
    };
    r.agc = s.agc.clone();
    r.nb = s.nb;
    r.nr = s.nr;
    r.notch = s.anf;
    r.af_gain = s.af_gain;
    r.rit_hz = s.rit_hz;
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tx_gate_table::station;

    /// A slice of ours on 40 m, not transmitting.
    fn ours(index: u8) -> SliceReport {
        SliceReport {
            index,
            letter: Some(char::from(b'A' + index).to_string()),
            owner: RxOwner::Ours,
            transmits: false,
            dial_mhz: Some(7.074),
            mode: Some("PKTUSB".into()),
            filter_low_hz: Some(0),
            filter_high_hz: Some(3000),
            agc: Some("mid".into()),
            nb: Some(false),
            nr: Some(true),
            anf: Some(false),
            af_gain: Some(0.5),
            muted: Some(false),
            rit_hz: Some(0),
        }
    }

    /// A Flex station whose client reports slice A (ours, transmitting), B (ours) and C (another
    /// client's).
    fn flex() -> Engine {
        let mut e = station(2036, "general", "phone", 14.250, "20m", "USB");
        let mut a = ours(0);
        a.transmits = true;
        a.dial_mhz = Some(14.25);
        let mut c = ours(2);
        c.owner = RxOwner::Foreign;
        e.observe_flex_slices(Some(vec![a, ours(1), c]));
        e
    }

    fn taken(e: &mut Engine) -> Vec<(u8, SliceIntent)> {
        e.take_slice_writes()
            .into_iter()
            .map(|w| (w.index, w.intent))
            .collect()
    }

    /// ⭐ An intent for a slice of ours that is not the transmit slice goes to the loop for that
    /// slice, and nothing else moves.
    #[test]
    fn an_intent_for_our_other_slice_goes_to_the_loop_for_that_slice() {
        let mut e = flex();
        let dial = e.settings().dial_mhz;
        e.request_slice(1, SliceIntent::Tune { mhz: 7.0355 })
            .expect("slice B is ours and receives only");
        e.request_slice(1, SliceIntent::Mute { muted: true })
            .unwrap();
        assert_eq!(
            taken(&mut e),
            vec![
                (1, SliceIntent::Tune { mhz: 7.0355 }),
                (1, SliceIntent::Mute { muted: true }),
            ]
        );
        assert!(taken(&mut e).is_empty(), "taken once");
        assert_eq!(e.settings().dial_mhz, dial, "the radio's dial never moved");
    }

    /// The three refusals that keep the gates whole: the transmit slice is the dial's, another
    /// client's slice is never touched, and a slice the radio does not report does not exist.
    #[test]
    fn the_transmit_slice_and_other_clients_slices_are_refused() {
        let mut e = flex();
        assert_eq!(
            e.request_slice(0, SliceIntent::Tune { mhz: 14.3 }),
            Err(SliceRefusal::TransmitSlice(0))
        );
        assert_eq!(
            e.request_slice(0, SliceIntent::Mode { mode: "LSB".into() }),
            Err(SliceRefusal::TransmitSlice(0))
        );
        assert_eq!(
            e.request_slice(2, SliceIntent::AfGain { gain: 0.1 }),
            Err(SliceRefusal::NotOurs(2))
        );
        assert_eq!(
            e.request_slice(5, SliceIntent::Mute { muted: true }),
            Err(SliceRefusal::NoSuchSlice(5))
        );
        assert!(taken(&mut e).is_empty(), "nothing refused reaches the loop");
    }

    /// An owner the radio has not said is not ours.
    #[test]
    fn a_slice_whose_owner_is_unknown_is_not_ours() {
        let mut e = flex();
        let mut d = ours(3);
        d.owner = RxOwner::Unknown;
        e.observe_flex_slices(Some(vec![d]));
        assert_eq!(
            e.request_slice(3, SliceIntent::NoiseBlanker { on: true }),
            Err(SliceRefusal::NotOurs(3))
        );
    }

    /// No client, no slices, and nothing queued for a client that is not there.
    #[test]
    fn without_a_flex_client_there_are_no_slices_and_nothing_is_queued() {
        let mut e = station(3073, "general", "phone", 14.250, "20m", "USB");
        assert_eq!(
            e.request_slice(1, SliceIntent::Mute { muted: true }),
            Err(SliceRefusal::NoClient)
        );
        assert!(e.receivers().set.is_empty());
        // A client that goes away takes its pending intents with it.
        let mut e = flex();
        e.request_slice(1, SliceIntent::Mute { muted: true })
            .unwrap();
        e.observe_flex_slices(None);
        assert!(taken(&mut e).is_empty());
        assert!(e.receivers().set.is_empty());
    }

    #[test]
    fn values_that_cannot_be_sent_are_refused_before_the_radio_is_asked() {
        let mut e = flex();
        for intent in [
            SliceIntent::Tune { mhz: f64::NAN },
            SliceIntent::Tune { mhz: 0.0 },
            SliceIntent::Mode { mode: "".into() },
            SliceIntent::Mode { mode: "usb".into() },
            SliceIntent::Mode {
                mode: "USB\nT 1".into(),
            },
            SliceIntent::Filter {
                low_hz: 3000,
                high_hz: 100,
            },
            SliceIntent::Agc {
                speed: "auto".into(),
            },
            SliceIntent::AfGain { gain: 1.5 },
            SliceIntent::AfGain { gain: f32::NAN },
        ] {
            assert_eq!(
                e.request_slice(1, intent.clone()),
                Err(SliceRefusal::Invalid),
                "{intent:?}"
            );
        }
        assert!(taken(&mut e).is_empty());
    }

    /// A dragged control sends its last value, once per slice and kind.
    #[test]
    fn the_latest_request_of_a_kind_wins_per_slice() {
        let mut e = flex();
        let mut d = ours(3);
        d.dial_mhz = Some(3.573);
        let mut a = ours(0);
        a.transmits = true;
        e.observe_flex_slices(Some(vec![a, ours(1), d]));
        e.request_slice(1, SliceIntent::AfGain { gain: 0.2 })
            .unwrap();
        e.request_slice(3, SliceIntent::AfGain { gain: 0.3 })
            .unwrap();
        e.request_slice(1, SliceIntent::AfGain { gain: 0.4 })
            .unwrap();
        assert_eq!(
            taken(&mut e),
            vec![
                (3, SliceIntent::AfGain { gain: 0.3 }),
                (1, SliceIntent::AfGain { gain: 0.4 }),
            ]
        );
    }

    /// ⭐ THE ROUND TRIP, engine side: what the client reports becomes the indexed set, each
    /// slice keyed by the radio's index with its owner and transmit flag, while Main stays the
    /// flat fields.
    #[test]
    fn the_report_becomes_the_indexed_receiver_set() {
        let e = flex();
        let rs = e.receivers();
        assert_eq!(rs.main.id, RxId::Main);
        assert_eq!(rs.main.owner, None, "Main carries no slice fields");
        let ids: Vec<RxId> = rs.set.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![RxId::Slice(0), RxId::Slice(1), RxId::Slice(2)]);
        let b = &rs.set[1];
        assert_eq!(b.letter.as_deref(), Some("B"));
        assert_eq!(b.owner, Some(RxOwner::Ours));
        assert_eq!(b.transmits, Some(false));
        assert_eq!(b.dial_mhz, Some(7.074));
        assert_eq!(b.band.as_deref(), Some("40m"));
        assert_eq!(b.rig_mode.as_deref(), Some("PKTUSB"));
        assert_eq!(b.filter_width_hz, Some(3000));
        assert_eq!(
            (b.nb, b.nr, b.notch),
            (Some(false), Some(true), Some(false))
        );
        assert_eq!((b.af_gain, b.muted), (Some(0.5), Some(false)));
        assert_eq!(b.stages, RxStages::FLEX_SLICE);
        assert_eq!(rs.set[0].transmits, Some(true));
        assert_eq!(rs.set[2].owner, Some(RxOwner::Foreign));
    }

    /// A radio switch starts from nothing: another radio's slices are not this radio's, and an
    /// answer taken for one radio never lands on the other.
    #[test]
    fn slices_belong_to_the_radio_they_were_reported_for() {
        let mut e = flex();
        e.request_slice(1, SliceIntent::Mute { muted: true })
            .unwrap();
        let writes = e.take_slice_writes();
        let mut s = e.settings().clone();
        s.rig_model = 3073;
        e.apply_settings(s);
        assert!(e.receivers().set.is_empty(), "the IC-7300 has no slices");
        assert_eq!(
            e.request_slice(1, SliceIntent::Mute { muted: true }),
            Err(SliceRefusal::NoClient)
        );
        e.observe_slice_write(&writes[0], Err("refused".into()));
        assert_eq!(e.last_slice_refusal(), None, "the old radio's answer");
    }

    #[test]
    fn a_refused_write_is_kept_to_be_said() {
        let mut e = flex();
        e.request_slice(1, SliceIntent::Tune { mhz: 7.03 }).unwrap();
        let writes = e.take_slice_writes();
        e.observe_slice_write(&writes[0], Ok(()));
        assert_eq!(e.last_slice_refusal(), None);
        e.observe_slice_write(&writes[0], Err("out of range".into()));
        assert_eq!(e.last_slice_refusal(), Some((1, "out of range".into())));
    }

    /// One of every intent. The match has no wildcard, so a new variant fails to compile here
    /// until it is listed.
    fn every_intent() -> Vec<SliceIntent> {
        let all = vec![
            SliceIntent::Tune { mhz: 7.0 },
            SliceIntent::Mode { mode: "USB".into() },
            SliceIntent::Filter {
                low_hz: 100,
                high_hz: 2900,
            },
            SliceIntent::Agc {
                speed: "mid".into(),
            },
            SliceIntent::AfGain { gain: 0.5 },
            SliceIntent::Mute { muted: true },
            SliceIntent::NoiseBlanker { on: true },
            SliceIntent::NoiseReduction { on: true },
            SliceIntent::AutoNotch { on: true },
        ];
        for i in &all {
            match i {
                SliceIntent::Tune { .. }
                | SliceIntent::Mode { .. }
                | SliceIntent::Filter { .. }
                | SliceIntent::Agc { .. }
                | SliceIntent::AfGain { .. }
                | SliceIntent::Mute { .. }
                | SliceIntent::NoiseBlanker { .. }
                | SliceIntent::NoiseReduction { .. }
                | SliceIntent::AutoNotch { .. } => {}
            }
        }
        all
    }

    /// ⭐ THE TYPESCRIPT MIRROR (`types.ts`'s `SliceIntent`) spells every intent the way the
    /// engine reads it — the same kinds, and per kind the same keys — in both directions. A guard
    /// on one side of a language boundary is not a guard on the boundary.
    #[test]
    fn the_typescript_mirror_spells_every_intent_as_the_engine_reads_it() {
        let ts = include_str!("../../../../ui/src/types.ts");
        let start = ts
            .find("export type SliceIntent =")
            .expect("types.ts declares SliceIntent");
        let mut mirror: Vec<(String, Vec<String>)> = ts[start..]
            .lines()
            .skip(1)
            .take_while(|l| l.trim_start().starts_with('|'))
            .map(|l| {
                let inner = l.trim().trim_start_matches('|').trim();
                let inner = inner.trim_start_matches('{').trim_end_matches('}');
                let mut kind = String::new();
                let mut keys = Vec::new();
                for part in inner.split(';').map(str::trim).filter(|p| !p.is_empty()) {
                    let (key, value) = part.split_once(':').expect("key: type");
                    if key.trim() == "kind" {
                        kind = value.trim().trim_matches('\'').to_string();
                    } else {
                        keys.push(key.trim().to_string());
                    }
                }
                keys.sort();
                (kind, keys)
            })
            .collect();
        mirror.sort();
        // Parser sanity: a scan that found nothing would pass the comparison vacuously.
        assert!(mirror.len() >= 9, "parsed only {mirror:?}");
        let mut wire: Vec<(String, Vec<String>)> = every_intent()
            .iter()
            .map(|i| {
                let v = serde_json::to_value(i).unwrap();
                let o = v.as_object().unwrap();
                let mut keys: Vec<String> = o.keys().filter(|k| *k != "kind").cloned().collect();
                keys.sort();
                (o["kind"].as_str().unwrap().to_string(), keys)
            })
            .collect();
        wire.sort();
        assert_eq!(mirror, wire, "ui/src/types.ts SliceIntent vs the engine's");
    }

    /// The UI's spelling of an intent: `kind` and camelCase fields, nothing else accepted.
    #[test]
    fn an_intent_crosses_the_wire_by_kind() {
        let i: SliceIntent =
            serde_json::from_str(r#"{"kind":"filter","lowHz":100,"highHz":2900}"#).unwrap();
        assert_eq!(
            i,
            SliceIntent::Filter {
                low_hz: 100,
                high_hz: 2900
            }
        );
        let i: SliceIntent = serde_json::from_str(r#"{"kind":"afGain","gain":0.5}"#).unwrap();
        assert_eq!(i, SliceIntent::AfGain { gain: 0.5 });
        assert!(
            serde_json::from_str::<SliceIntent>(r#"{"kind":"tune","mhz":7.0,"tx":1}"#).is_err()
        );
        assert!(serde_json::from_str::<SliceIntent>(r#"{"kind":"transmit"}"#).is_err());
    }
}
