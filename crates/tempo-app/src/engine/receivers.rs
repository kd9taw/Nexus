//! ⭐ MAIN AND SUB AS RECEIVERS — the engine's model of a radio with two of them.
//!
//! On a radio with two receivers every receive-side value exists twice: two dials, two
//! S-meters, two AGCs, two filter chains. Until this module each was "the radio's", which on
//! such a radio silently means "whichever receiver it happened to come from". [`Receiver`] is
//! one receiver's values; [`Receivers`] is the radio's set, from [`Engine::receivers`].
//!
//! ## What a receiver holds, and what it never will
//! The dual-receiver survey's classification of the snapshot decides it. The 25 PER-RECEIVER
//! values are here. The 35 per-radio ones (the one transmitter and what a radio has once:
//! power, the TX meters, split, XIT, the ATU, CAT health) and the 23 per-station ones (clock,
//! slot, decoder, soundcard, logbook link) are not, and do not move. The ten the survey left
//! open are resolved here:
//!
//! | | Field | Resolved as |
//! |---|---|---|
//! | U1 | `tx_allowed` | per radio (one transmitter, one verdict), judged against the TX SOURCE (D1, [`Engine::tx_source`]): [`Engine::tx_source_verdict`], which `tx_allowed` reads |
//! | U2, U3 | `phone_seg_lo/hi` | the same: the TX source's band, which the snapshot shades |
//! | U4 | `active_vfo` | per receiver (D5): each receiver has its own A/B pair |
//! | U5 | `rx_ranges_mhz` | per receiver; the radio's `\dump_state` list describes Main. What depends on BOTH receivers is a pairing, not a list: [`Receivers::pairing`] (D6) |
//! | U6, U7 | `att_steps_db`, `preamp_steps_db` | per receiver; the radio's lists describe Main's front end |
//! | U8 | `sideband_override` | per receiver: the Phone picker commands the dial the cockpit shows, which is Main's |
//! | U9 | `rx_level` | per station: the soundcard peak on the decode feed |
//! | U10 | `rx_offset_hz` | per station: the decoder's offset — the Sub is not in the decode path (D4) |
//!
//! ## ADDITIVE: Main is read from the flat fields, never copied
//! Every flat engine field keeps its storage and its meaning, and every existing consumer reads
//! exactly what it did. [`Engine::receivers`] BUILDS Main from those fields on each call,
//! composed exactly as the snapshot composes them (the radio's read-back wins, else the value
//! last commanded). A stored copy would be a second truth about Main that could drift from the
//! first, and D1's verdict below would then judge a different Main from the one the gate
//! judges. The storage moves into [`Receiver`] when the flat fields are removed, the
//! programme's last step.
//!
//! ## Which receivers exist (D2, D3, D10)
//! Main, always. The Sub only where this build OFFERS one: independent receivers with a vendor
//! citation (`dualrx::sub_receiver_offered`). [`Receivers::sub_capability`] carries the
//! three-state answer beside it, so a missing Sub always says why: ABSENT (one receiver, a
//! vendor-sourced no), UNKNOWN (no manual read — never a no, and no Sub is offered on it, D3),
//! or PRESENT without an offer (a shared front end, category 2, which v1 does not offer, D2).
//!
//! ## What the Sub holds, in this build
//! Only what the engine knows, which is two facts. On a satellite cross-band pair the native
//! CI-V daemon writes the uplink into the Sub band and reads it back, and the radio loop says so
//! ([`Engine::rig_split_applied_on`]): while that acknowledged split stands, the Sub's dial is
//! the uplink and its sideband the one Nexus commands for it. And the RF, AF and squelch levels
//! the radio ACCEPTED from Nexus for the Sub (`engine::sub_controls`) — values Nexus set, not
//! readings. Everything else is `None`: a Sub the radio has and this build cannot read is
//! present and silent (the configured-but-silent state of `amp: Option<AmpStatusDto>`), never a
//! zero and never a guess.
//!
//! Nothing polls the Sub over CAT yet. The broker's per-receiver verbs (`CivBackend::*_on`)
//! address it, and the Sub's level WRITES reach them through `L Sub …`; but on an IC-9700, which
//! has no band-directed command, every Sub read is a select-Sub / read / select-Main round trip,
//! so a poll at the loop's meter rate would flip the operator's selection several times a second.
//!
//! ## ⚠️ "Main" is what the broker addresses as Main
//! On an IC-9700 an operator who selects Sub at the front panel is followed by every receive
//! command — the broker holds the selection it set and does not re-read it — so Main's values
//! then describe the Sub band. Accepted as the 9700's limit (operator ruling, 2026-09-23):
//! catching it would mean flipping the radio's selection several times a second. The IC-7610
//! names Main on the wire for the receive-chain commands its CI-V reference marks; its dial and
//! mode verbs are unmarked and follow the panel the same way.
//!
//! ## D1: the licence gate judges the TX source
//! [`Engine::tx_source_verdict`] is the licence gate (operator sign-off, 2026-09-23):
//! `tx_allowed`, which every transmit path ANDs in, is its answer, and the snapshot's phone
//! segment is its shade. While Main transmits it is the gate exactly as it stood before the
//! switch; while the Sub does, the same judgement of the uplink plus one rule (see the
//! verdict), so it can only ever refuse more. The engine's gate table
//! (`engine::tx_gate_table`) pins the answers row by row, and the five rows the switch moved.

use super::Engine;
use crate::bandplan::band_for_dial;
use crate::dualrx::{self, CapState, Pairing, ReceiverId, RxStage, StageOwner};

/// D7 — WHICH OF THE RADIO'S RECEIVE STAGES THIS RECEIVER MAY BE CREDITED WITH, one answer per
/// stage, from `dualrx::stage_on`.
///
/// ⛔ ATTRIBUTION, NOT EXISTENCE. [`StageOwner::Own`] on Main's DSP says that if the radio has a
/// DSP it is Main's, not that it has one: what exists is the radio's own `\dump_state` to say.
/// And [`StageOwner::Unknown`] is never a "no" (D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RxStages {
    /// Attenuator, preamp, RF gain, AGC.
    pub front_end: StageOwner,
    /// Filter width, NB, NR, the automatic and manual notches.
    pub dsp: StageOwner,
    /// AF gain and squelch.
    pub audio: StageOwner,
}

impl RxStages {
    fn of(model: u32, rx: ReceiverId) -> Self {
        Self {
            front_end: dualrx::stage_on(model, rx, RxStage::FrontEnd),
            dsp: dualrx::stage_on(model, rx, RxStage::Dsp),
            audio: dualrx::stage_on(model, rx, RxStage::Audio),
        }
    }
}

/// ONE RECEIVER'S VALUES — the survey's 25 per-receiver fields, four of its unresolved ones
/// (U4–U8), and what this receiver may be credited with (D7).
///
/// Every value is an `Option` because a Sub may have told us nothing, and `None` means "not
/// known", never "off" or zero. Main's are filled from the engine's flat fields exactly as the
/// snapshot fills them, so Main's dial, band, sideband, A/B selection and RIT are always
/// `Some`, and every other value is `None` exactly where the snapshot's is.
#[derive(Debug, Clone, PartialEq)]
pub struct Receiver {
    /// Which receiver this is.
    pub id: ReceiverId,
    /// D7 — the receive stages this receiver may be credited with.
    pub stages: RxStages,

    // ── Tuning ──
    /// The receive dial, MHz.
    pub dial_mhz: Option<f64>,
    /// The band label; `""` off the bands, the flat field's own convention.
    pub band: Option<String>,
    /// The commanded sideband, `"USB"` / `"LSB"`.
    pub sideband: Option<String>,
    /// U8 — the Phone cockpit's transient mode pick (`"USB"`/`"LSB"`/`"FM"`/`"AM"`).
    pub sideband_override: Option<String>,
    /// The mode the radio reports. Display only; never overwrites the commanded sideband.
    pub rig_mode: Option<String>,
    /// U4, D5 — THIS receiver's A/B selection, `true` = VFO B.
    pub active_vfo_b: Option<bool>,
    /// The RIT offset Nexus commanded, Hz. Write-only: nothing reads RIT back.
    pub rit_hz: Option<i32>,
    /// The last dial the radio refused, MHz.
    pub refused_dial_mhz: Option<f64>,
    /// U5 — the receive coverage the radio declares, Hz, inclusive. `None` is unknown and fails
    /// OPEN.
    pub rx_ranges_hz: Option<Vec<(u64, u64)>>,

    // ── The receive chain ──
    /// S-meter, dB relative to S9.
    pub smeter_db: Option<i32>,
    /// AGC speed.
    pub agc: Option<String>,
    /// The AGC speed the radio refused.
    pub refused_agc: Option<String>,
    /// Receive passband, Hz.
    pub filter_width_hz: Option<u32>,
    /// Noise blanker.
    pub nb: Option<bool>,
    /// Noise reduction.
    pub nr: Option<bool>,
    /// Noise-reduction level, 0..1.
    pub nr_level: Option<f32>,
    /// Automatic notch.
    pub notch: Option<bool>,
    /// Manual notch.
    pub manual_notch: Option<bool>,
    /// Manual-notch frequency, Hz.
    pub notch_freq_hz: Option<f32>,
    /// RF (receive) gain, 0..1.
    pub rf_gain: Option<f32>,
    /// Squelch, 0..1.
    pub squelch: Option<f32>,
    /// Attenuator in dB (`0` = off).
    pub att_db: Option<u8>,
    /// Preamp in dB (`0` = off).
    pub preamp_db: Option<u8>,
    /// U6 — the attenuator steps the radio declares for this receiver.
    pub att_steps_db: Option<Vec<u8>>,
    /// U7 — the preamp steps the radio declares for this receiver.
    pub preamp_steps_db: Option<Vec<u8>>,
    /// AF gain, 0..1.
    pub af_gain: Option<f32>,

    // ── The scope ──
    /// The radio's scope sweep-mode code.
    pub scope_mode_code: Option<u32>,
    /// The fixed-mode scope's start edge, MHz.
    pub scope_fix_start_mhz: Option<f64>,
    /// Why the radio refused a scope span.
    pub scope_span_refused: Option<String>,
    /// A scope failure to surface.
    pub scope_error: Option<String>,
}

impl Receiver {
    /// A receiver nothing has been read from: every value unknown.
    fn unread(id: ReceiverId, stages: RxStages) -> Self {
        Self {
            id,
            stages,
            dial_mhz: None,
            band: None,
            sideband: None,
            sideband_override: None,
            rig_mode: None,
            active_vfo_b: None,
            rit_hz: None,
            refused_dial_mhz: None,
            rx_ranges_hz: None,
            smeter_db: None,
            agc: None,
            refused_agc: None,
            filter_width_hz: None,
            nb: None,
            nr: None,
            nr_level: None,
            notch: None,
            manual_notch: None,
            notch_freq_hz: None,
            rf_gain: None,
            squelch: None,
            att_db: None,
            preamp_db: None,
            att_steps_db: None,
            preamp_steps_db: None,
            af_gain: None,
            scope_mode_code: None,
            scope_fix_start_mhz: None,
            scope_span_refused: None,
            scope_error: None,
        }
    }
}

/// THE RADIO'S RECEIVERS, as [`Engine::receivers`] builds them for the radio in play.
#[derive(Debug, Clone, PartialEq)]
pub struct Receivers {
    /// Always present: the receiver CAT describes by default.
    pub main: Receiver,
    /// Present only where this build OFFERS a second receiver for this radio (D2: independent
    /// receivers with a vendor citation). A Sub the radio has and nothing here can read is
    /// still present, with every value `None`.
    pub sub: Option<Receiver>,
    /// Does the radio HAVE a second receiver — three-state (D3), and the reason when
    /// [`Self::sub`] is `None`: `Absent` is a vendor-sourced no, `Unknown` is no manual read
    /// (never a no), and `Present` with no `sub` is a second receiver v1 does not offer.
    pub sub_capability: CapState,
    /// D6 — may the two receivers sit where they sit now? `None` when there is no Sub in the
    /// model. [`Pairing::Unknown`] is NOT a refusal (see [`Engine::receiver_pairing_for`]).
    pub pairing: Option<Pairing>,
}

/// THE LICENCE GATE'S VERDICT, judged against the TX SOURCE (D1): [`Engine::tx_allowed`] is its
/// `tx_allowed`, and the snapshot's phone segment its `phone_seg`.
#[derive(Debug, Clone, PartialEq)]
pub struct TxSourceVerdict {
    /// The receiver whose band the radio transmits from ([`Engine::tx_source`]).
    pub source: ReceiverId,
    /// The frequency judged, MHz: the source's transmit frequency.
    pub emission_mhz: f64,
    /// What the licence gate would answer, judging the source.
    pub tx_allowed: bool,
    /// The phone segment of the source's band, the band strip's shade.
    pub phone_seg: Option<(f64, f64)>,
}

/// What the engine knows about the Sub band while an acknowledged split rides it.
struct SubUplink {
    mhz: f64,
    sideband: Option<&'static str>,
    /// Does Nexus command the uplink VFO a mode word at all ([`Engine::sat_tx_mode`])? Not
    /// once the operator has taken the mode back mid-pass, nor with no pass holding it.
    commanded: bool,
}

/// The sideband a commanded rig mode puts a signal on; `None` for a mode with none (FM, CW, AM)
/// or one this does not recognise.
fn sideband_of_mode(mode: &str) -> Option<&'static str> {
    match mode.trim().to_ascii_uppercase().as_str() {
        "USB" | "PKTUSB" => Some("USB"),
        "LSB" | "PKTLSB" => Some("LSB"),
        _ => None,
    }
}

/// D6's answer for two dials, where both are known; [`Pairing::Unknown`] otherwise.
fn pair(model: u32, main_mhz: Option<f64>, sub_mhz: Option<f64>) -> Pairing {
    match (main_mhz, sub_mhz) {
        (Some(main), Some(sub)) => dualrx::may_pair(model, main, sub),
        _ => Pairing::Unknown,
    }
}

impl Engine {
    /// The receivers of the radio in play: Main always, the Sub where this build offers one.
    ///
    /// The model comes from the ACTIVE radio's Hamlib model (`settings.rig_model`, the flat
    /// mirror of the active profile), read on every call, so a radio switch can never leave the
    /// previous radio's Sub behind.
    pub fn receivers(&self) -> Receivers {
        let model = self.settings.rig_model;
        let main = self.main_receiver(model);
        let sub = dualrx::sub_receiver_offered(model).then(|| self.sub_receiver(model));
        let pairing = sub
            .as_ref()
            .map(|sub| pair(model, main.dial_mhz, sub.dial_mhz));
        Receivers {
            main,
            sub,
            sub_capability: dualrx::sub_receiver(model),
            pairing,
        }
    }

    /// D6 — MAY `rx` BE TUNED TO `mhz`, given where the other receiver sits?
    ///
    /// `None` when the model has no Sub: there is no second receiver, so nothing between
    /// receivers constrains the move. Otherwise `dualrx::may_pair` against the other receiver's
    /// dial — and [`Pairing::Unknown`] where that dial is not known.
    ///
    /// ⚠️ ONLY [`Pairing::Refused`] MAY STOP ANYTHING. `Unknown` is permission to try: command the
    /// move and let the radio answer, which is `dualrx`'s rule and `Rig::read_rx_ranges`'s.
    ///
    /// Nothing asks this before a QSY yet: Main's tuning paths are unchanged in this build, and
    /// no path here tunes the Sub. It is the model's answer for the stage that offers a Sub tune.
    pub fn receiver_pairing_for(&self, rx: ReceiverId, mhz: f64) -> Option<Pairing> {
        let model = self.settings.rig_model;
        if !dualrx::sub_receiver_offered(model) {
            return None;
        }
        let sub_mhz = self.sub_uplink().map(|u| u.mhz);
        Some(match rx {
            ReceiverId::Main => pair(model, Some(mhz), sub_mhz),
            ReceiverId::Sub => pair(model, Some(self.settings.dial_mhz), Some(mhz)),
        })
    }

    /// D1 — THE LICENCE GATE, JUDGED AGAINST THE TX SOURCE ([`Engine::tx_source`]).
    /// [`Engine::tx_allowed`] is this verdict's `tx_allowed`, so every transmit path ANDs it in,
    /// and the snapshot's phone segment is its `phone_seg` (operator sign-off, 2026-09-23).
    ///
    /// - **Main transmits** (every state but an acknowledged Sub split): the gate exactly as it
    ///   stood before the switch, [`Engine::tx_frequency_allowed`] — the same frequency, the
    ///   same verdict — and the phone segment of Main's band.
    /// - **The Sub transmits** (an acknowledged split riding the Sub band): the same frequency,
    ///   the uplink plus the transmit offsets the gate adds (XIT; a repeater shift, which a
    ///   satellite pass forces to simplex), judged by that same gate. It already judges Phone's
    ///   passband and Digital's data carrier in the word the uplink VFO is commanded
    ///   (`tx_mode_effective`), so a cross-band pass is judged once, by one model. One rule is
    ///   added: where Nexus commands the uplink NO word (the operator took the mode back
    ///   mid-pass) the side of the carrier is unknown, and both sides must be legal, the gate's
    ///   own rule for an unreadable RTTY mode word. XIT on a split is judged both ways, the
    ///   gate's rule unchanged. The phone segment is the Sub's band's.
    ///
    /// ⚠️ SO ON THE SUB IT CAN ONLY EVER REFUSE MORE than the gate did before the switch, and
    /// only in the Digital section, the one section model that reads a side: Phone's
    /// convention, CW's carrier and RTTY's and Keyboard's own models read none. A commanded
    /// word that names no side (FM) adds nothing, as `digital_emission_allowed` has it.
    ///
    /// ⚠️ What it cannot see, stated so it is not over-trusted: the Sub's mode is the one
    /// COMMANDED (nothing reads it back on this path), exactly as XIT is the offset commanded;
    /// and RTTY takes its side from the radio's reported mode word, which on a two-receiver
    /// radio is Main's.
    pub fn tx_source_verdict(&self) -> TxSourceVerdict {
        let class = self.settings.license_class;
        let judged = self.tx_frequency_allowed();
        let Some(up) = self.sub_uplink() else {
            return TxSourceVerdict {
                source: ReceiverId::Main,
                emission_mhz: self.tx_emission_mhz(),
                tx_allowed: judged,
                phone_seg: crate::privileges::phone_segment(class, &self.settings.band),
            };
        };
        let om = self.settings.operating_mode;
        let xit = self.xit_offset_mhz();
        let emission = up.mhz + xit + self.rptr_shift_mhz();
        // No word commanded for the uplink: the side of the carrier is unknown, so both.
        let both_sides =
            |f: f64| self.emission_allowed(om, f, "USB") && self.emission_allowed(om, f, "LSB");
        let side_unknown_ok = up.commanded
            || if self.xit_hz != 0 {
                both_sides(emission) && both_sides(emission - xit)
            } else {
                both_sides(emission)
            };
        TxSourceVerdict {
            source: ReceiverId::Sub,
            emission_mhz: emission,
            tx_allowed: judged && side_unknown_ok,
            phone_seg: band_for_dial(up.mhz)
                .and_then(|band| crate::privileges::phone_segment(class, band)),
        }
    }

    /// Main, built from the flat fields exactly as `Engine::snapshot` composes them.
    fn main_receiver(&self, model: u32) -> Receiver {
        Receiver {
            id: ReceiverId::Main,
            stages: RxStages::of(model, ReceiverId::Main),
            dial_mhz: Some(self.settings.dial_mhz),
            band: Some(self.settings.band.clone()),
            sideband: Some(self.settings.sideband.clone()),
            sideband_override: self.sideband_override.clone(),
            rig_mode: self.rig_mode.clone(),
            active_vfo_b: Some(self.active_vfo_b_effective()),
            rit_hz: Some(self.rit_hz),
            refused_dial_mhz: self.rig_refused_dial_mhz,
            rx_ranges_hz: self.rig_rx_ranges.clone(),
            smeter_db: self.rig_smeter_db,
            agc: self.rig_agc.clone().or_else(|| self.agc.clone()),
            refused_agc: self.rig_refused_agc.clone(),
            filter_width_hz: self.rig_passband,
            nb: self.rig_funcs[0],
            nr: self.rig_funcs[1],
            nr_level: self.rig_nr_level.or(self.nr_level),
            notch: self.rig_funcs[2],
            manual_notch: self.rig_funcs[5],
            notch_freq_hz: self.rig_notch_freq_hz.or(self.notch_freq_hz),
            rf_gain: self.rig_rf_gain.or(self.rf_gain),
            squelch: self.rig_squelch.or(self.squelch),
            att_db: self.rig_att_db,
            preamp_db: self.rig_preamp_db,
            att_steps_db: self.rig_att_steps.clone(),
            preamp_steps_db: self.rig_preamp_steps.clone(),
            af_gain: self.rig_af_gain.or(self.af_gain),
            scope_mode_code: self.scope_mode_code,
            scope_fix_start_mhz: self.scope_fix_start_mhz,
            scope_span_refused: self.scope_span_refused.clone(),
            scope_error: self.scope_error.clone(),
        }
    }

    /// The Sub, from the one Sub fact the engine has: the uplink an acknowledged split wrote
    /// into its band. Everything else stays unknown.
    fn sub_receiver(&self, model: u32) -> Receiver {
        let mut sub = Receiver::unread(ReceiverId::Sub, RxStages::of(model, ReceiverId::Sub));
        if let Some(up) = self.sub_uplink() {
            sub.dial_mhz = Some(up.mhz);
            sub.band = Some(band_for_dial(up.mhz).unwrap_or("").to_string());
            sub.sideband = up.sideband.map(str::to_string);
        }
        // The levels Nexus set on the Sub and the radio ACCEPTED — never a read-back
        // (`engine::sub_controls`).
        use super::sub_controls::SubLevel;
        sub.rf_gain = self.sub_level_accepted(SubLevel::Rf);
        sub.af_gain = self.sub_level_accepted(SubLevel::Af);
        sub.squelch = self.sub_level_accepted(SubLevel::Sql);
        sub
    }

    /// The Sub band's frequency and commanded sideband, while an acknowledged split rides it.
    fn sub_uplink(&self) -> Option<SubUplink> {
        if self.tx_source() != ReceiverId::Sub {
            return None;
        }
        let hz = self.tx_split_confirmed_hz?;
        let word = self.sat_tx_mode();
        Some(SubUplink {
            mhz: hz as f64 / 1e6,
            sideband: word.as_deref().and_then(sideband_of_mode),
            commanded: word.is_some(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tx_gate_table::{loop_applies_sat_split, sat_pass, station, RS44, VV_BIRD};
    use crate::engine::SatCatBackend;

    const OWN_ALL: RxStages = RxStages {
        front_end: StageOwner::Own,
        dsp: StageOwner::Own,
        audio: StageOwner::Own,
    };

    fn near(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-9)
    }

    /// D2 + D7 — the receiver set comes from the capability table: which receivers exist, and
    /// what each may be credited with, per receiver rather than per radio.
    #[test]
    fn the_receiver_set_is_populated_from_the_capability_model() {
        // IC-7610: two independent receivers, offered.
        let rs = station(3078, "general", "phone", 14.250, "20m", "USB").receivers();
        assert_eq!(rs.sub_capability, CapState::Present, "IC-7610 has a Sub");
        let sub = rs.sub.expect("IC-7610 is offered a Sub");
        assert_eq!((rs.main.id, sub.id), (ReceiverId::Main, ReceiverId::Sub));
        // ⭐ D7 IN ONE RADIO: Main owns every stage; the Sub owns its front end and its AF (Icom:
        // "independent AF/RF knobs"), but no vendor statement gives it its own DSP.
        assert_eq!(rs.main.stages, OWN_ALL);
        assert_eq!(
            sub.stages,
            RxStages {
                front_end: StageOwner::Own,
                dsp: StageOwner::Unknown,
                audio: StageOwner::Own,
            },
            "IC-7610 Sub"
        );
        // FTDX101D: Yaesu documents every stage per receiver.
        let rs = station(1040, "general", "phone", 14.250, "20m", "USB").receivers();
        assert_eq!(rs.sub.expect("FTDX101D is offered a Sub").stages, OWN_ALL);
        // IC-910: the DSP is an owner-fitted option with two sockets — the Sub's is Unknown,
        // on the same independent architecture as the FTDX101D above.
        let rs = station(3044, "general", "phone", 144.200, "2m", "USB").receivers();
        assert_eq!(
            rs.sub.expect("IC-910 is offered a Sub").stages.dsp,
            StageOwner::Unknown
        );
        // ⚠️ THE DISCRIMINATOR (D2): an IC-7600 HAS a second receiver, but it shares one front
        // end, and v1 offers category 1 only. Present, and no Sub in the model.
        let rs = station(3063, "general", "phone", 14.250, "20m", "USB").receivers();
        assert_eq!(rs.sub_capability, CapState::Present, "IC-7600 has one");
        assert!(rs.sub.is_none(), "…and v1 does not offer it");
        assert_eq!(rs.pairing, None, "no Sub, nothing to pair");
    }

    /// D3 — UNKNOWN IS NEVER ABSENT, and no Sub is offered on it.
    #[test]
    fn an_unread_radio_is_unknown_never_absent_and_offers_no_sub() {
        let unread = station(3073, "general", "phone", 14.250, "20m", "USB").receivers();
        let single = station(1037, "general", "phone", 14.250, "20m", "USB").receivers();
        assert_eq!(
            unread.sub_capability,
            CapState::Unknown,
            "IC-7300: no manual read"
        );
        assert_eq!(
            single.sub_capability,
            CapState::Absent,
            "FTDX3000: one receiver"
        );
        assert_ne!(
            unread.sub_capability, single.sub_capability,
            "an unread radio and a vendor-sourced single receiver must not answer alike"
        );
        assert!(unread.sub.is_none(), "no Sub on UNKNOWN");
        assert!(single.sub.is_none());
        // No CAT at all (VOX) is not a "no" either.
        let vox = station(0, "general", "phone", 14.250, "20m", "USB").receivers();
        assert_eq!(vox.sub_capability, CapState::Unknown);
        // ⚠️ AND IT FOLLOWS THE RADIO IN PLAY: an offered Sub is not kept once the active
        // radio's model is one with no offer.
        let mut e = station(3078, "general", "phone", 14.250, "20m", "USB");
        assert!(e.receivers().sub.is_some());
        e.settings.rig_model = 3073;
        assert!(
            e.receivers().sub.is_none(),
            "the IC-7300 has no offered Sub"
        );
        assert_eq!(e.receivers().sub_capability, CapState::Unknown);
    }

    /// Main's receiver, read against the flat snapshot field by field.
    fn assert_main_matches_the_snapshot(e: &Engine, state: &str) {
        let m = e.receivers().main;
        let r = e.snapshot().radio;
        assert!(near(m.dial_mhz, r.dial_mhz), "{state}: dial");
        assert_eq!(m.band.as_deref(), Some(r.band.as_str()), "{state}: band");
        assert_eq!(
            m.sideband.as_deref(),
            Some(r.sideband.as_str()),
            "{state}: sideband"
        );
        assert_eq!(
            m.sideband_override, r.sideband_override,
            "{state}: override"
        );
        assert_eq!(m.rig_mode, r.rig_mode, "{state}: rig mode");
        let vfo = m.active_vfo_b.map(|b| if b { "B" } else { "A" });
        assert_eq!(vfo, Some(r.active_vfo.as_str()), "{state}: A/B");
        assert_eq!(m.rit_hz, Some(r.rit_hz), "{state}: RIT");
        assert_eq!(
            m.refused_dial_mhz, r.refused_dial_mhz,
            "{state}: refused dial"
        );
        let ranges_mhz: Vec<(f64, f64)> = m
            .rx_ranges_hz
            .clone()
            .unwrap_or_default()
            .iter()
            .map(|(lo, hi)| (*lo as f64 / 1e6, *hi as f64 / 1e6))
            .collect();
        assert_eq!(ranges_mhz, r.rx_ranges_mhz, "{state}: coverage");
        assert_eq!(m.smeter_db, r.smeter_db, "{state}: S-meter");
        assert_eq!(m.agc, r.agc, "{state}: AGC");
        assert_eq!(m.refused_agc, r.refused_agc, "{state}: refused AGC");
        assert_eq!(m.filter_width_hz, r.filter_width_hz, "{state}: width");
        assert_eq!(m.nb, r.nb, "{state}: NB");
        assert_eq!(m.nr, r.nr, "{state}: NR");
        assert_eq!(m.nr_level, r.nr_level, "{state}: NR level");
        assert_eq!(m.notch, r.notch, "{state}: notch");
        assert_eq!(m.manual_notch, r.manual_notch, "{state}: manual notch");
        assert_eq!(m.notch_freq_hz, r.notch_freq_hz, "{state}: notch freq");
        assert_eq!(m.rf_gain, r.rf_gain, "{state}: RF gain");
        assert_eq!(m.squelch, r.squelch, "{state}: squelch");
        assert_eq!(m.att_db, r.att_db, "{state}: ATT");
        assert_eq!(m.preamp_db, r.preamp_db, "{state}: preamp");
        assert_eq!(m.att_steps_db, r.att_steps_db, "{state}: ATT steps");
        assert_eq!(
            m.preamp_steps_db, r.preamp_steps_db,
            "{state}: preamp steps"
        );
        assert_eq!(m.af_gain, r.af_gain, "{state}: AF gain");
        assert_eq!(m.scope_mode_code, r.scope_mode_code, "{state}: scope mode");
        assert_eq!(
            m.scope_fix_start_mhz, r.scope_fix_start_mhz,
            "{state}: FIX start"
        );
        assert_eq!(
            m.scope_span_refused, r.scope_span_refused,
            "{state}: span refusal"
        );
        assert_eq!(m.scope_error, r.scope_error, "{state}: scope error");
    }

    /// ADDITIVE — MAIN IS EXACTLY WHAT THE FLAT SNAPSHOT SAYS, every field, including how a
    /// read-back and a commanded value combine. Two states with values chosen so that no two
    /// fields hold the same one in both, so a crossed wire cannot pass.
    #[test]
    fn main_reads_exactly_what_the_flat_snapshot_reads() {
        let mut e = station(3078, "general", "phone", 14.250, "20m", "USB");
        assert_main_matches_the_snapshot(&e, "fresh");

        e.rig_mode = Some("USB".into());
        e.sideband_override = Some("LSB".into());
        e.rig_smeter_db = Some(-7);
        e.agc = Some("fast".into()); // commanded, nothing read back
        e.rig_refused_agc = Some("slow".into());
        e.rig_passband = Some(2_400);
        e.rig_funcs = [
            Some(true),
            Some(false),
            None,
            Some(false),
            None,
            Some(true),
            None,
        ];
        e.nr_level = Some(0.11);
        e.rig_nr_level = Some(0.22); // read back: wins
        e.notch_freq_hz = Some(1_234.0);
        e.rf_gain = Some(0.33);
        e.rig_rf_gain = Some(0.44);
        e.squelch = Some(0.55);
        e.af_gain = Some(0.66);
        e.rig_af_gain = Some(0.77);
        e.rig_att_db = Some(12);
        e.rig_preamp_db = Some(20);
        e.rig_att_steps = Some(vec![6, 12, 18]);
        e.rig_preamp_steps = Some(vec![10, 20]);
        e.request_rit(150);
        e.request_vfo(true);
        e.rig_refused_dial_mhz = Some(14.999);
        e.rig_rx_ranges = Some(vec![(30_000, 60_000_000), (144_000_000, 148_000_000)]);
        e.scope_mode_code = Some(2);
        e.scope_fix_start_mhz = Some(14.0);
        e.scope_span_refused = Some("span".into());
        e.scope_error = Some("scope".into());
        assert_main_matches_the_snapshot(&e, "first");

        // The second permutation: every boolean pair the first left equal now differs.
        e.rig_funcs = [
            Some(false),
            Some(true),
            Some(true),
            None,
            Some(true),
            None,
            Some(false),
        ];
        e.rig_agc = Some("slow".into()); // read back: wins over the commanded "fast"
        e.agc = Some("fast".into());
        e.rig_nr_level = None; // commanded only again
        e.take_vfo_apply();
        e.observe_rig_vfo(false); // the rig's own answer, after the command landed
        assert_main_matches_the_snapshot(&e, "second");
    }

    /// Per receiver — every value belongs to the receiver it describes.
    #[test]
    fn per_receiver_values_belong_to_the_receiver_they_describe() {
        // RS-44 on the native daemon: Main is the 70 cm downlink, the Sub the 2 m uplink.
        let mut e = sat_pass(3081, "general", "phone", RS44);
        assert_eq!(
            loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv),
            Some("Sub")
        );
        e.rig_smeter_db = Some(-3);
        let rs = e.receivers();
        let sub = rs.sub.expect("IC-9700 is offered a Sub");
        assert!(near(rs.main.dial_mhz, 435.640), "{:?}", rs.main.dial_mhz);
        assert_eq!(rs.main.band.as_deref(), Some("70cm"));
        assert_eq!(rs.main.sideband.as_deref(), Some("USB"));
        assert!(near(sub.dial_mhz, 145.965), "{:?}", sub.dial_mhz);
        assert_eq!(sub.band.as_deref(), Some("2m"));
        assert_eq!(
            sub.sideband.as_deref(),
            Some("LSB"),
            "an inverting bird: USB down, LSB up"
        );
        assert_eq!(rs.main.smeter_db, Some(-3));
        assert_eq!(sub.smeter_db, None, "Main's reading never lands on the Sub");
        assert_eq!(sub.rit_hz, None, "Main's RIT is Main's");

        // ⚠️ THE DISCRIMINATOR: a V/V pass rides VFO B on Main, so the split is NOT the Sub's
        // and the Sub stays unread — the uplink must not be credited to it by frequency alone.
        let mut vv = sat_pass(3081, "general", "phone", VV_BIRD);
        assert_eq!(
            loop_applies_sat_split(&mut vv, SatCatBackend::NativeCiv),
            Some("VFOB")
        );
        assert_eq!(vv.receivers().sub.expect("offered").dial_mhz, None);

        // A QSY voids the acknowledged split: the Sub goes back to unread instead of keeping a
        // stale uplink.
        e.set_frequency(435.700, "70cm", "USB");
        assert_eq!(e.receivers().sub.expect("offered").dial_mhz, None);
    }

    /// D5 — each receiver has its own A/B pair; Main's selection is not the Sub's.
    #[test]
    fn each_receiver_has_its_own_ab_selection() {
        let mut e = station(3078, "general", "phone", 14.250, "20m", "USB");
        assert_eq!(e.receivers().main.active_vfo_b, Some(false));
        assert_eq!(
            e.receivers().sub.expect("offered").active_vfo_b,
            None,
            "nothing reads the Sub's A/B in this build"
        );
        e.request_vfo(true);
        assert_eq!(e.receivers().main.active_vfo_b, Some(true), "the command");
        assert_eq!(e.receivers().sub.expect("offered").active_vfo_b, None);
        // The rig's own answer wins once the command has landed, as in the snapshot.
        e.take_vfo_apply();
        e.observe_rig_vfo(false);
        assert_eq!(
            e.receivers().main.active_vfo_b,
            Some(false),
            "the read-back"
        );
    }

    /// D6 — THE CONSTRAINT BETWEEN THE RECEIVERS lives in the model: where one may go depends on
    /// where the other is.
    #[test]
    fn the_constraint_between_the_receivers_is_in_the_model() {
        // IC-9700 on RS-44: the pair it sits in is legal (430 and 144 are different groups).
        let mut e = sat_pass(3081, "general", "phone", RS44);
        loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv);
        assert_eq!(e.receivers().pairing, Some(Pairing::Allowed));
        // Moving the Sub onto Main's band is refused in Icom's terms, and so is moving Main onto
        // the Sub's; elsewhere is fine.
        assert!(matches!(
            e.receiver_pairing_for(ReceiverId::Sub, 435.100),
            Some(Pairing::Refused(_))
        ));
        assert!(matches!(
            e.receiver_pairing_for(ReceiverId::Main, 145.900),
            Some(Pairing::Refused(_))
        ));
        assert_eq!(
            e.receiver_pairing_for(ReceiverId::Sub, 1296.200),
            Some(Pairing::Allowed)
        );

        // IC-9100: the Sub's legal bands depend on where MAIN is — HF/50 is a group of its own.
        // Its Sub is unread, so the pair it sits in is Unknown; a Sub move is judged against
        // Main, and a Main move against an unread Sub fails open.
        let e = station(3068, "general", "phone", 14.250, "20m", "USB");
        assert_eq!(e.receivers().pairing, Some(Pairing::Unknown));
        assert!(matches!(
            e.receiver_pairing_for(ReceiverId::Sub, 14.100),
            Some(Pairing::Refused(_))
        ));
        assert_eq!(
            e.receiver_pairing_for(ReceiverId::Sub, 144.200),
            Some(Pairing::Allowed)
        );
        assert_eq!(
            e.receiver_pairing_for(ReceiverId::Main, 7.100),
            Some(Pairing::Unknown)
        );

        // ⚠️ CONTROL: the same same-band Sub move on a radio with no pairing rule is allowed —
        // so the refusal above comes from the model, not from the frequencies.
        let e = station(3078, "general", "phone", 14.250, "20m", "USB");
        assert_eq!(
            e.receiver_pairing_for(ReceiverId::Sub, 14.100),
            Some(Pairing::Allowed)
        );
        // Undocumented is Unknown, never a refusal (IC-910: implied, never stated).
        let e = station(3044, "general", "phone", 144.200, "2m", "USB");
        assert_eq!(
            e.receiver_pairing_for(ReceiverId::Sub, 145.500),
            Some(Pairing::Unknown)
        );
        // No Sub in the model: nothing between receivers to constrain anything.
        let e = station(3073, "general", "phone", 14.250, "20m", "USB");
        assert_eq!(e.receiver_pairing_for(ReceiverId::Sub, 14.100), None);
    }
}
