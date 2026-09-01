//! Operator / station settings — persisted by the shell as JSON so the user
//! configures the app without recompiling.
//!
//! `#[serde(default)]` makes every field optional on load, so older settings
//! files (and UI forms that don't yet send every field) still deserialize.

use crate::dto::SourceKind;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// What kind of operating the active section is doing — the per-section rig-mode
/// policy. **Digital** OBEYS the rig (max compatibility; FT8/FT4 live in an audio
/// sub-carrier on USB/Data, so forcing the mode would break the operator's setup).
/// **Phone** and **CW** actively FORCE the correct mode, because a voice op must be
/// in USB/LSB and a CW op in CW. **Rtty** forces the mode per keying backend (rig
/// RTTY for true FSK, LSB for soundcard AFSK — see [`Settings::rig_mode`]) and OWNS
/// the rig for keying: the FT8/FT1 slot sequencer never keys while it's active, and
/// the RTTY keyer never keys outside it. The phone/CW/RTTY operating sections set
/// this; the digital cockpit leaves it `Digital`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OperatingMode {
    #[default]
    Digital,
    Phone,
    Cw,
    Rtty,
    /// The keyboard modes as ONE flat section (PSK31 today; QPSK31 and the rest
    /// select within the cockpit, never here). "keyboard" on the wire. All
    /// keyboard modes share one section policy — data privileges, the digital
    /// power cap, always-USB PKTUSB, soundcard-only — so every policy match in
    /// the app gains exactly ONE reviewed arm for the whole family. Rtty stays
    /// its own variant: its LSB convention and FSK backend are genuinely
    /// different policy.
    Keyboard,
}

/// Which VFO carries the uplink and which the downlink during a satellite pass.
///
/// ⚠️ **A WRONG MAPPING TRANSMITS ON YOUR OWN DOWNLINK.** That is why this is an
/// explicit, operator-visible enumeration rather than something inferred from
/// band edges: operators wire full-duplex stations differently, and the failure
/// mode is transmitting into the satellite's output passband — the single
/// rudest thing you can do on a linear bird.
///
/// This governs the **UPLINK**. The downlink needs no mapping: correcting the
/// receive dial cannot transmit, so it follows the pass automatically (see
/// [`Settings::sat_doppler_downlink`]) and the only mapping that stops it is
/// [`SatVfoMap::UplinkOnly`], where the operator has said their one VFO IS the
/// transmit leg. The default is [`SatVfoMap::Off`] — no uplink mapping chosen,
/// so nothing is ever written to a transmit VFO — and choosing one is only half
/// the consent: [`Settings::sat_uplink_radios`] records WHICH RADIO it was
/// chosen for, because a mapping is a fact about one station's wiring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SatVfoMap {
    /// No uplink mapping — Doppler corrects the receive dial and writes
    /// nothing to a transmit VFO (default — fail safe on the leg that can
    /// cause harm).
    #[default]
    Off,
    /// Half-duplex, receive only: one VFO, corrected for the downlink. The
    /// operator transmits nothing, or keys manually with no uplink correction.
    DownlinkOnly,
    /// Half-duplex, transmit only: one VFO, corrected for the uplink.
    UplinkOnly,
    /// VFO A = downlink, VFO B = uplink.
    ADownBUp,
    /// VFO A = uplink, VFO B = downlink.
    AUpBDown,
    /// Main = downlink, Sub = uplink — the natural IC-9700 full-duplex layout.
    MainDownSubUp,
    /// Main = uplink, Sub = downlink.
    MainUpSubDown,
}

impl Settings {
    /// The rotator policy these settings describe — one place that turns the
    /// flat persisted fields into the pointing rules.
    pub fn rotator_config(&self) -> tempo_core::rotator::RotatorConfig {
        use tempo_core::rotator::{PostPass, RotatorConfig};
        RotatorConfig {
            park_az: self.rot_park_az,
            park_el: self.rot_park_el,
            ready_az: self.rot_ready_az,
            ready_el: self.rot_ready_el,
            post_pass: match self.rot_post_pass.trim().to_ascii_lowercase().as_str() {
                "park" => PostPass::Park,
                "ready" => PostPass::Ready,
                // Anything unknown (including a value from a newer build)
                // degrades to leaving the mast alone — never to moving it.
                _ => PostPass::Stop,
            },
            tol_az_deg: self.rot_tol_az_deg,
            tol_el_deg: self.rot_tol_el_deg,
            cal_az_deg: self.rot_cal_az_deg,
            cal_el_deg: self.rot_cal_el_deg,
            allow_flip: self.rot_allow_flip,
        }
    }
}

impl SatVfoMap {
    /// Is the uplink under Doppler control in this mapping? (False for
    /// downlink-only, where we must never write a transmit frequency.)
    ///
    /// NOT the whole uplink consent on its own — the mapping also has to have
    /// been confirmed for the radio that will receive the write. Ask
    /// [`Settings::sat_doppler_uplink`], which is the one place both halves
    /// are checked together.
    pub fn drives_uplink(self) -> bool {
        matches!(
            self,
            SatVfoMap::UplinkOnly
                | SatVfoMap::ADownBUp
                | SatVfoMap::AUpBDown
                | SatVfoMap::MainDownSubUp
                | SatVfoMap::MainUpSubDown
        )
    }

    /// Is the RECEIVE dial Doppler's to correct under this mapping?
    ///
    /// True for every mapping except [`SatVfoMap::UplinkOnly`] — including
    /// [`SatVfoMap::Off`], which means "no uplink mapping", not "hands off the
    /// radio". Correcting the dial cannot transmit: the worst case is that the
    /// operator does not hear the bird. Uplink-only is the one operator
    /// statement that the single VFO in play is the TRANSMIT leg, and moving it
    /// to the downlink there would be the wrong write.
    pub fn drives_downlink(self) -> bool {
        !matches!(self, SatVfoMap::UplinkOnly)
    }

    /// True when the pair rides Main/Sub rather than VFO A/B — the IC-9700's
    /// real full-duplex mode, which our native CI-V engine can drive directly.
    pub fn is_main_sub(self) -> bool {
        matches!(self, SatVfoMap::MainDownSubUp | SatVfoMap::MainUpSubDown)
    }
}

/// Which steering surfaces a satellite track loop is actually allowed to
/// drive — the honesty label `sat_track_status` puts on the wire so the UI
/// never has to infer "is Doppler live?" from which fields happen to be null.
///
/// The rotor half is decided at arm time (the loop captures its rotator
/// address once); the Doppler half is `drives_a_leg` — [`Settings::sat_doppler_downlink`]
/// or [`Settings::sat_doppler_uplink`], re-read every tick so a mid-pass change
/// moves label and behaviour together — AND a held transponder (without one
/// `Engine::sat_doppler_tick` no-ops every tick: there is nothing to tune), so
/// an operator who picked "None — leave the dial to me" is never told Doppler
/// owns a dial the engine will not touch. Four values:
///
/// - `"rotor+doppler"` — both surfaces consented (the full appliance role)
/// - `"rotor-only"`    — pointing only; Doppler prereqs missing, dial untouched
/// - `"doppler-only"`  — no rotator (handheld/fixed antenna); Doppler consented
/// - `"pass-only"`     — neither: pass state and geometry only, which is legal
///   and useful for timing — the label says so instead of pretending more
///
/// WHICH LEG is deliberately not encoded here: the two legs are separately
/// consented now, and one string cannot carry two facts without lying about
/// one of them. The DTO reports them as their own fields (`dopplerDownlink` /
/// `dopplerUplink`) and this stays the rotor-vs-radio label.
///
/// This is a LABEL, never a gate: the actual refusals live in
/// `sat_doppler_tick` (radio) and the loop's rotor branch (mast). Pure so the
/// whole table is testable outside the shell.
pub fn sat_track_mode(has_rotor: bool, drives_a_leg: bool, has_transponder: bool) -> &'static str {
    let doppler = drives_a_leg && has_transponder;
    match (has_rotor, doppler) {
        (true, true) => "rotor+doppler",
        (true, false) => "rotor-only",
        (false, true) => "doppler-only",
        (false, false) => "pass-only",
    }
}

/// What Nexus can honestly PROPOSE for the uplink half on the radio in use —
/// the input to the readiness rail's one-time confirmation.
///
/// A confident default for a known full-duplex rig is not a licence to guess
/// for an unknown one: [`SatUplinkOffer::Confirm`] is only ever returned for a
/// layout the rig can express in exactly one way AND that this build can
/// actually drive. Everything else asks, or offers nothing. Nothing here
/// WRITES a mapping — the operator's click does that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SatUplinkOffer {
    /// No uplink to propose: one VFO (no full duplex), or a radio we cannot
    /// identify. Doppler corrects the downlink and the transmit VFO stays the
    /// operator's, unless they map it themselves.
    Nothing,
    /// Full duplex, but WHICH VFO carries which leg is this station's wiring —
    /// ask, never guess.
    Ask,
    /// Derived from the connected rig, unambiguously: pre-fill this and let
    /// the operator confirm it once.
    Confirm(SatVfoMap),
}

/// Hamlib model numbers for the Icom satellite rigs whose SATELLITE MODE fixes
/// transmit on Sub — so `Main = downlink / Sub = uplink` is the only
/// full-duplex layout they can express (`Engine::sat_split_tx_vfo` refuses the
/// reverse for exactly that reason). Numbers anchored on
/// `tempo_audio::rigmodels` (`model = 1000 * backend + index`, append-only in
/// Hamlib): IC-9700, IC-910, IC-9100, IC-905. Duplicated rather than imported
/// because `tempo-app` does not depend on `tempo-audio` — the arrow points the
/// other way.
pub const MAIN_SUB_SAT_RIGS: [u32; 4] = [3081, 3044, 3068, 3090];

/// The subset of [`MAIN_SUB_SAT_RIGS`] that Nexus's OWN CI-V daemon can ever
/// serve — the intersection with `tempo_audio::rigmodels::icom_scope_model`,
/// which is the table `native_civ_addr` gates on. **IC-9700 (3081) and
/// IC-905 (3090) only.**
///
/// The IC-910 (3044) and IC-9100 (3068) are classic satellite rigs with no
/// `0x27` spectrum scope, so they are absent from `icom_scope_model` and
/// `native_civ_addr` returns `None` for them under every setting. Only the
/// native daemon drives a Main/Sub satellite split (`Engine::sat_split_tx_vfo`),
/// so on those two models the mapping has no path at all and
/// [`Settings::sat_uplink_offer`] must not pre-fill it.
///
/// A second axis is NOT in this table because it is per-connection, not
/// per-model: `native_civ_addr` also refuses `is_network()`, so a
/// network-connected IC-9700 cannot reach the native daemon either. That axis
/// is [`rig_conn_is_network`], and [`native_civ_reachable`] is the two
/// together.
///
/// Duplicated here for the same reason as [`MAIN_SUB_SAT_RIGS`] (the crate
/// arrow points the other way) and pinned against the real table by
/// `tempo-audio`'s `the_native_capable_sat_rig_table_is_the_civ_scope_table`.
pub const NATIVE_CIV_SAT_RIGS: [u32; 2] = [3081, 3090];

/// Full-duplex satellite rigs with no Main/Sub CAT path in this build: the
/// VFO pair is A/B and which one is the uplink is a station wiring choice no
/// model string answers. FT-847, FT-736R, TS-2000, TS-790.
const FULL_DUPLEX_AB_RIGS: [u32; 4] = [1001, 1010, 2014, 2007];

/// Is this rig transport a NETWORK one — rigctld reaching the radio over TCP
/// (a Flex via SmartSDR, a remote rigctld) instead of a serial port?
///
/// ⭐ THE SINGLE SOURCE OF TRUTH for that question, for the whole app.
/// `tempo_audio::service::Transport::is_network` — which is what the CI-V
/// daemon's own gate `native_civ_addr` consults — calls THIS function, so the
/// daemon and every consumer of [`native_civ_reachable`] cannot answer
/// differently. They used to: Settings tested `rig_conn == "serial"`, and the
/// two parted company on the empty string, on mixed case, and on a "network"
/// pick with no address yet. The empty string is not hypothetical — `rig_conn`
/// is `#[serde(default)]` and a settings.json written before the field existed
/// loads as `""`, which this rule (and the field's own doc) treats as serial.
///
/// An ADDRESS is required: a "network" pick with nothing to connect to is not
/// a network rig, because rigctld would have nowhere to go.
pub fn rig_conn_is_network(rig_conn: &str, rig_addr: &str) -> bool {
    rig_conn == "network" && !rig_addr.is_empty()
}

/// Is this rig driven through **OmniRig** — VE3NEA's Windows COM rig-control
/// server — instead of by a rigctld Nexus launches?
///
/// ⭐ THE SINGLE SOURCE OF TRUTH for that question, for the same reason
/// [`rig_conn_is_network`] is: `tempo_audio::service::Transport::is_omnirig`
/// calls THIS, so the daemon-choice seam and every settings-side consumer
/// cannot answer differently.
///
/// Unlike the network test this needs NO second field: OmniRig owns the rig
/// type, the COM port and the baud, so there is nothing on the Nexus side that
/// could be missing. Case-insensitive, because an imported/hand-edited config
/// may carry "OmniRig".
pub fn rig_conn_is_omnirig(rig_conn: &str) -> bool {
    rig_conn.eq_ignore_ascii_case("omnirig")
}

/// Could Nexus's OWN CI-V daemon ever serve a radio wired like this — i.e. is
/// "turn on Native CI-V" a cure that EXISTS for it?
///
/// The two gates `tempo_audio::service::native_civ_addr` applies, in one
/// place: a model the CI-V engine knows ([`NATIVE_CIV_SAT_RIGS`]) on a
/// non-network transport ([`rig_conn_is_network`]). False means the Main/Sub
/// mapping has NO path in this build, so [`Settings::sat_uplink_offer`] must
/// not pre-fill it — `Engine::sat_split_tx_vfo` would refuse the write.
///
/// Not gated on the operator's `icom_native_cat` opt-in: that is the separate
/// "is it switched on" question, and it is answered where it is used.
pub fn native_civ_reachable(rig_model: u32, rig_conn: &str, rig_addr: &str) -> bool {
    NATIVE_CIV_SAT_RIGS.contains(&rig_model)
        && !rig_conn_is_network(rig_conn, rig_addr)
        // OmniRig is the third transport and it is a dead end for the CI-V daemon for the
        // same reason `network` is: OmniRig holds the COM port, so Nexus can never open it
        // to speak CI-V itself. Without this the satellite offer would pre-fill a Main/Sub
        // mapping whose write has no path at all.
        && !rig_conn_is_omnirig(rig_conn)
}

impl Settings {
    /// Is the RECEIVE dial Doppler's to correct?
    ///
    /// The operator's off switch, and the one mapping that claims the dial for
    /// the transmit leg. Deliberately NOT gated on a per-radio confirmation:
    /// this leg only ever moves the receive VFO, and the worst it can do is
    /// leave the operator unable to hear the bird. Nothing can transmit from
    /// here.
    ///
    /// The remaining preconditions are structural, not settings: the engine
    /// tunes nothing without a HELD transponder and a dial a pass actually
    /// owns (`Engine::steer_sat_dial` refuses otherwise), so a station that
    /// never opens Satellites is untouched by all of this.
    pub fn sat_doppler_downlink(&self) -> bool {
        !self.sat_doppler_off && self.sat_vfo_map.drives_downlink()
    }

    /// Is the TRANSMIT VFO Doppler's to correct — on the radio that would
    /// actually receive the write?
    ///
    /// BOTH halves, in one place: a mapping that drives the uplink, and that
    /// mapping confirmed for the ACTIVE radio (the rig the split one-shot
    /// lands on). Callers re-ask every tick rather than latching at arm time —
    /// a handoff can put a different rig under the split mid-pass, and a
    /// confirmation granted for the IC-9700 says nothing about the FTdx10.
    pub fn sat_doppler_uplink(&self) -> bool {
        !self.sat_doppler_off
            && self.sat_vfo_map.drives_uplink()
            && self.sat_uplink_confirmed(self.active_radio)
    }

    /// Has the operator confirmed the current mapping for this radio?
    ///
    /// `None` in [`Settings::sat_uplink_radios`] is TRANSIENT pre-migration
    /// state — a file from before per-radio confirmation, before
    /// [`Settings::load`] resolves what it meant — and it confirms NOTHING:
    /// an unresolved consent must fail safe on the leg that transmits. (It
    /// used to mean the honoured station-wide grant, but an in-memory
    /// sentinel with no serialized representation died on the first
    /// save+relaunch; the migration now MATERIALIZES the live legacy grant
    /// as concrete ids instead, so post-load this field is always `Some`.)
    /// Every write records the radio, and a removed radio's entry is pruned
    /// with it ([`Settings::ensure_routing_targets`]) because freed ids are
    /// reused.
    pub fn sat_uplink_confirmed(&self, radio_id: u32) -> bool {
        match &self.sat_uplink_radios {
            None => false,
            Some(ids) => ids.contains(&radio_id),
        }
    }

    /// Record the operator's uplink mapping confirmation for `radio_id` —
    /// their click, never inferred. (A transient pre-migration `None` simply
    /// becomes the explicit list: the operator is stating the mapping for
    /// the radio in front of them.)
    ///
    /// A CHANGE of mapping retires every OTHER radio's confirmation. What a
    /// radio was confirmed for is a specific mapping, and the mapping is one
    /// field: leaving the other ids in place would have a second rig driving
    /// its uplink under a layout nobody ever confirmed for it — the exact
    /// wrong-uplink shape the enumeration exists to prevent. Re-confirming the
    /// SAME mapping is not a change and leaves the others alone.
    ///
    /// The operator-facing writer is `Engine::confirm_sat_uplink` — the pass
    /// rail's confirmation and Settings ▸ Radio's mapping select both invoke
    /// it (the `confirm_sat_uplink` command), and `Engine::apply_settings`
    /// treats the pair as live state it captures across a form save, so this
    /// rule has exactly one operator-facing entry point.
    pub fn confirm_sat_uplink(&mut self, radio_id: u32, map: SatVfoMap) {
        if self.sat_vfo_map != map {
            self.sat_uplink_radios = Some(Vec::new());
        }
        self.sat_vfo_map = map;
        let ids = self.sat_uplink_radios.get_or_insert_with(Vec::new);
        if !ids.contains(&radio_id) {
            ids.push(radio_id);
        }
    }

    /// What can honestly be proposed for the uplink on the ACTIVE radio.
    ///
    /// Derived from the rig's Hamlib model — the identity Nexus persists per
    /// profile. The native CI-V broker can answer the same question by READING
    /// satellite mode (`16 5A`) off the radio, which is better evidence, but it
    /// lives in the device layer and only after a split has been attempted;
    /// this is the model table one crate away from it, named as such.
    ///
    /// A Main/Sub rig is only proposed while `icom_native_cat` owns its CI-V
    /// port: that is the path that engages satellite mode and select-writes the
    /// Sub band. Served by Hamlib rigctld instead, the reliable recipe
    /// (`U SATMODE 1` then per-VFO writes) is not wired — pre-filling a mapping
    /// this build may not drive would offer a one-click confirmation for an
    /// uplink the rail would then claim and never write.
    pub fn sat_uplink_offer(&self) -> SatUplinkOffer {
        let (model, _, _, native) = self.active_rig_facts();
        if MAIN_SUB_SAT_RIGS.contains(&model) {
            // `native` alone is not enough: a settings file can carry
            // `icom_native_cat` on for a rig the daemon can never serve
            // (the toggle is not rendered for the IC-910/IC-9100, but the
            // field is), and pre-filling Main/Sub there would offer a
            // one-click confirmation for a mapping the engine then refuses
            // as a dead end.
            return if native && self.sat_native_civ_reachable() {
                SatUplinkOffer::Confirm(SatVfoMap::MainDownSubUp)
            } else {
                SatUplinkOffer::Ask
            };
        }
        if FULL_DUPLEX_AB_RIGS.contains(&model) {
            return SatUplinkOffer::Ask;
        }
        SatUplinkOffer::Nothing
    }

    /// `(rig model, connection kind, network address, Native-CI-V opt-in)` for
    /// the ACTIVE radio — the profile if there is one, else the flat legacy
    /// fields. ONE resolution, because several callers reading a radio several
    /// ways is how a per-radio answer ends up describing a different radio.
    fn active_rig_facts(&self) -> (u32, &str, &str, bool) {
        self.radios
            .iter()
            .find(|p| p.id == self.active_radio)
            .map(|p| {
                (
                    p.rig_model,
                    p.rig_conn.as_str(),
                    p.rig_addr.as_str(),
                    p.icom_native_cat,
                )
            })
            .unwrap_or((
                self.rig_model,
                self.rig_conn.as_str(),
                self.rig_addr.as_str(),
                self.icom_native_cat,
            ))
    }

    /// Could the native CI-V daemon EVER serve the active radio — i.e. does the
    /// Main/Sub satellite split have a path on this station at all?
    ///
    /// [`native_civ_reachable`] answers it, and so does the daemon's own gate:
    /// `native_civ_addr` refuses a network transport through the same
    /// [`rig_conn_is_network`] this calls. Pinned across the crate boundary by
    /// tempo-audio's `one_reachability_rule_answers_for_the_daemon_and_for_the_refusal`.
    pub fn sat_native_civ_reachable(&self) -> bool {
        let (model, conn, addr, _) = self.active_rig_facts();
        native_civ_reachable(model, conn, addr)
    }

    /// Is the active radio one of the four Main/Sub satellite Icoms? On the
    /// IC-9700 the A/B pair is PER BAND (`0F 01` + `25 01` address the CURRENT
    /// band's other VFO — the 0.24.2 bug), and Nexus has no verified cross-band
    /// A/B split for any of the four, which is why `Engine::sat_split_tx_vfo`
    /// refuses one rather than sending it hopefully.
    pub fn sat_main_sub_rig(&self) -> bool {
        MAIN_SUB_SAT_RIGS.contains(&self.active_rig_facts().0)
    }
}

/// The operator's amateur license class — drives the transmit-privilege lockout + the
/// "jump to the start of my licensed segment" band dropdown. The US classes carry FCC
/// Part 97 (Region 2) sub-band privileges; **Open** = no transmit restrictions (for
/// operators outside the US — picked via the wizard's "Outside the US" choice). Defaults
/// to **Open** so an upgrading install is never silently TX-locked; the lockout is
/// operator-declared (wizard on first run, or Settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LicenseClass {
    Technician,
    General,
    Extra,
    #[default]
    Open,
}

/// How CW is transmitted. **Cat** = the rig's own keyer via Hamlib `send_morse` (rig in
/// CW; clean, but older rigs like the IC-756PRO III don't implement it). **Serial** = the
/// app toggles a DTR/RTS keyline into the rig's KEY jack (rig in CW; the classic
/// N1MM/fldigi method — clean, needs only a keying cable; see `cw_key_port`/`cw_key_line`).
/// **WinKeyer** = a K1EL hardware keyer (rig in CW; jitter-free). **Soundcard** = the app
/// keys an audio tone through SSB (rig in USB; works on any rig, but an SSB-audio workaround
/// — shape it and keep it below ALC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CwKeyerBackend {
    #[default]
    Cat,
    Soundcard,
    /// K1EL WinKeyer hardware keyer over serial (see `settings.winkeyer_port`).
    WinKeyer,
    /// Serial DTR/RTS keyline into the rig's KEY jack (see `cw_key_port`/`cw_key_line`).
    Serial,
}

/// How rare a spot must be before Pounce interrupts the operator.
///
/// This MIRRORS `propagation::pounce::PounceThreshold`. Duplicated deliberately: settings live in
/// tempo-app, the scoring logic lives in propagation, and neither crate depends on the other —
/// adding a whole crate dependency to share one enum is the heavier choice. The conversion lives
/// in src-tauri (which sees both) and is pinned by a totality test there, so the two cannot drift
/// silently. Add a variant here, add it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PounceThreshold {
    /// DEFAULT — no Pounce alerts until the operator opts in. The right threshold depends on how
    /// much they still have to chase, which we cannot know at install: rare for a big total, a
    /// siren for a small one. See `propagation::pounce::PounceThreshold`.
    #[default]
    Off,
    /// An all-time-new DXCC entity only.
    Atno,
    /// ATNO or a new CQ zone (5BWAZ chasers).
    AtnoOrZone,
    /// ATNO, zone, or a new US state — the widest tier; fires often enough to become a feed
    /// rather than an interrupt.
    AtnoZoneOrState,
}

/// Everything the operator configures: identity, band/frequency, Field Day
/// exchange, rig/PTT control, and network (WSJT-X UDP API + PSK Reporter).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    // --- identity / operating ---
    pub mycall: String,
    pub mygrid: String,
    /// The operator's name (e.g. "Seth") — the `{NAME}` token in CW/voice macros and
    /// a casual ragchew staple. Empty until set.
    pub op_name: String,
    /// The operator's US state / province (e.g. "WI") — the `{MYSTATE}` CW-macro token for a
    /// ragchew QTH exchange. Empty until set. `#[serde(default)]` so older settings load.
    #[serde(default)]
    pub op_state: String,
    /// Allow a foreign app on the CAT broker (WSJT-X/N1MM) to key PTT when Nexus
    /// is idle. ON by default (operator sign-off 2026-08-09): the broker is the
    /// advertised share endpoint, and a shared client keeps the keying it had via
    /// the Hamlib port. Every engine-side TX gate still applies. The serde default
    /// must MATCH the struct default (`default_true`) — a bare `#[serde(default)]`
    /// would hand older files `false` and silently split the two defaults.
    #[serde(default = "default_true")]
    pub cat_broker_ptt: bool,
    pub band: String,
    pub dial_mhz: f64,
    pub sideband: String,
    /// Phone sub-mode: "ssb" (default — sideband by band) or "fm" (FM voice; drives the
    /// rig to FM + the repeater shift / CTCSS below). VHF/UHF FM simplex + repeaters.
    pub phone_mode: String,
    /// FM repeater shift: "simplex" (no shift) | "plus" | "minus". Only when phone_mode=fm.
    pub rptr_shift: String,
    /// FM CTCSS (PL) tone in Hz for repeater access, e.g. 100.0; 0.0 = off.
    pub ctcss_tone_hz: f32,
    /// Q65 T/R period in seconds: 15, 30, 60, 120 or 300. Only meaningful at the
    /// Q65 tier. Defaults to 60 — EME on VHF/UHF, Q65's flagship use, works
    /// Q65-60A/B/C. (30 is the 6 m meteor/ionoscatter setting and 15 is
    /// troposcatter.) `#[serde(default)]` so settings files written before Q65
    /// existed still load.
    #[serde(default = "default_q65_period_s")]
    pub q65_period_s: u16,
    /// Percentage of beacon intervals to TRANSMIT on (WSPR / FST4W), 0..=100.
    ///
    /// Defaults to **0 — beaconing off**. A beacon keys the radio unattended, so
    /// the operator has to ask for it; an upgrade must never silently start
    /// transmitting on a mode the station was only listening to.
    ///
    /// The convention is a minority of intervals: a beacon that transmits every
    /// interval hears nothing, and WSPR's value is the two-way picture. Below 40 %
    /// the scheduler also avoids back-to-back transmissions while preserving the
    /// requested rate — see `tempo_core::beacon::BeaconScheduler`.
    #[serde(default)]
    pub beacon_tx_percent: u8,
    /// Transmit power in **dBm** reported in WSPR/FST4W beacons.
    ///
    /// ⚠️ NOT cosmetic, and deliberately has no useful default (0 dBm = 1 mW).
    /// WSPR reports feed a PUBLIC propagation database that other operators draw
    /// conclusions from, so a wrong figure corrupts their data as well as yours.
    /// The beacon refuses to transmit until this is set — see the guard in
    /// `Engine::beacon_message`.
    ///
    /// Common values: 23 dBm = 200 mW, 30 dBm = 1 W, 37 dBm = 5 W, 43 dBm = 20 W.
    #[serde(default)]
    pub beacon_power_dbm: i32,
    /// FST4W **Round Robin** slot for this station, 1-based; 0 = random scheduling.
    ///
    /// Several stations agreeing on `beacon_rr_slots` and each taking a different
    /// slot will never transmit in the same interval, because the assignment is a
    /// pure function of UTC. Use it when coordinating with others on one frequency;
    /// leave it 0 to use the transmit-percentage scheduler.
    #[serde(default)]
    pub beacon_rr_slot: u8,
    /// How many FST4W Round Robin slots are in the rotation. Ignored when
    /// `beacon_rr_slot` is 0.
    #[serde(default)]
    pub beacon_rr_slots: u8,
    /// FST4 / FST4W T/R period in seconds: 15, 30, 60, 120, 300, 900 or 1800.
    /// Shared by both tiers — they are the same decoder on the same slot clock.
    /// Defaults to 120, the shortest interval FST4W beacons actually use (FST4
    /// QSO work is more often 15–60, but 120 is a working value for both and a
    /// beacon receiver is the likelier reason to pick this mode at all).
    #[serde(default = "default_fst4_period_s")]
    pub fst4_period_s: u16,
    /// MSK144 T/R period in seconds: 5, 10, 15 or 30. Defaults to 15 — the period
    /// 6 m meteor scatter actually runs on. Shorter periods turn the exchange
    /// around faster on a busy shower; longer ones give the decoder more frames to
    /// stack when pings are sparse.
    #[serde(default = "default_msk144_period_s")]
    pub msk144_period_s: u16,
    /// JT65 submode: 0/1/2 for A/B/C. A is the HF standard; B and C widen the tone
    /// spacing 2x and 4x, which EME operators move up to as Doppler spread grows.
    /// JT65 has one fixed 60 s period, so there is no period setting to go with it.
    #[serde(default)]
    pub jt65_submode: u8,
    /// Q65 submode, 0..=4 for A..E — the tone spacing. Wider submodes tolerate
    /// more Doppler spread at the cost of sensitivity, which is why EME operators
    /// move up the letters as the path degrades. Defaults to A.
    #[serde(default)]
    pub q65_submode: u8,
    /// FM repeater offset override in Hz (0 = use the band convention from
    /// [`Self::rptr_offset_hz`]). Set by the Program section's tune-now so
    /// odd-split machines (e.g. +1 MHz on 2 m) key the right input.
    #[serde(default)]
    pub rptr_offset_override_hz: i64,
    /// Field Day MASTER SWITCH — the single source of truth for whether Field
    /// Day mode is engaged (spec §1.1). Persisted so it survives restarts
    /// mid-contest, but set true ONLY by the operator's explicit toggle: NO code
    /// path (default, date logic, first-run, migration) may ever turn it on.
    /// Default false — a fresh install is entirely Field-Day-free.
    #[serde(default)]
    pub fd_active: bool,
    /// ARRL Field Day class, e.g. "1D", "3A".
    pub fd_class: String,
    /// Which Field Day event: "arrlfd" (June) | "wfd" (Winter Field Day).
    #[serde(default)]
    pub fd_event: String,
    /// Power multiplier tier: 5 = QRP battery/natural, 2 = <=150 W, 1 = >150 W.
    #[serde(default = "default_fd_power")]
    pub fd_power_mult: u32,
    /// Claimed bonus ids (the UI checklist; each maps to points in the bonus
    /// table). Stored as ids so the table can evolve.
    #[serde(default)]
    pub fd_bonuses: Vec<String>,
    /// PLANNED bonus ids — what the club set out to earn, not what it earned.
    /// A sibling list rather than a second meaning for [`Self::fd_bonuses`],
    /// which stays exactly the EARNED set the score is made of: no scoring
    /// path, export or club report may ever read this one. `#[serde(default)]`
    /// so an older settings file loads with nothing planned and scores the
    /// same points it always did.
    #[serde(default)]
    pub fd_bonuses_planned: Vec<String>,
    /// N3FJP real-time push: each FD QSO lands in the club's N3FJP master log
    /// over its TCP API. Empty host = off.
    #[serde(default)]
    pub n3fjp_host: String,
    #[serde(default = "default_n3fjp_port")]
    pub n3fjp_port: u16,
    /// Push each Field Day contact with the contest-correct **ENTER sequence**
    /// (which N3FJP scores) instead of ADDDIRECT (which stores the class/section
    /// but may not score the contest log). On by default; a bulk/backfill path
    /// can still use ADDDIRECT.
    #[serde(default = "default_true")]
    pub n3fjp_use_enter: bool,
    /// Report THIS position's band to N3FJP (no CAT needed) so the club's
    /// Network Status Display band board shows where we are. Off by default.
    #[serde(default)]
    pub n3fjp_report_band: bool,
    /// DXKeeper (DXLab Suite) real-time push: each logged QSO goes to DXKeeper's TCP
    /// Network Service. Empty host = off.
    #[serde(default)]
    pub dxkeeper_host: String,
    /// DXLab **Base Port** — the number DXKeeper's own Configuration ▸ Defaults ▸ Network
    /// Service panel shows. DXKeeper listens on base + 1 (52001 by default); nothing
    /// listens on the base itself. Stored as the base, not the resolved port, so it matches
    /// what the operator reads off DXKeeper's screen — the single most common source of
    /// "your integration doesn't work" reports.
    #[serde(default = "default_dxkeeper_base_port")]
    pub dxkeeper_base_port: u16,
    /// Let DXKeeper do the LoTW/eQSL/ClubLog/QRZ uploads instead of Nexus. OFF by default:
    /// Nexus owns those connectors, so leaving this on would upload every QSO twice to four
    /// services.
    #[serde(default)]
    pub dxkeeper_uploads: bool,
    /// N1MM+ contact broadcast: emit the native <contactinfo> XML datagram per
    /// FD QSO. Empty = off; "host:port" or "host" (default port 12060).
    #[serde(default)]
    pub n1mm_addr: String,
    /// Broadcast the N1MM `<contactinfo>` datagram for EVERY logged QSO, not just
    /// Field Day contacts — the standing output a live map/dashboard consumer
    /// (OpenHamClock, GridTracker) needs to plot each contact as it is logged.
    /// Sends to the same `n1mm_addr`; empty address = off regardless.
    ///
    /// Off by default, and no code path may turn it on: an upgrade must never
    /// start putting an operator's QSOs on the network they did not ask for.
    /// Cannot double-send with the Field-Day emitter — the two read disjoint logs
    /// (`a_field_day_contact_never_enters_the_general_upload_queue`).
    #[serde(default)]
    pub n1mm_upload: bool,
    /// ARRL/RAC section, e.g. "WI".
    pub fd_section: String,
    /// The current OPERATOR at the key (call or initials) — Field Day rotates
    /// operators, so this differs from the station `mycall`. Pushed to N3FJP as
    /// the QSO's operator; empty = fall back to `mycall`.
    #[serde(default)]
    pub fd_operator: String,
    /// Host a Nexus↔Nexus club event on this box. **This toggle IS the LAN
    /// opt-in**: while on, the fdsync listener binds 0.0.0.0 (the app's one
    /// deliberate non-loopback inbound socket — data-plane only, see
    /// `tempo_net::fdsync`) and a discovery beacon broadcasts once a second.
    /// Default OFF; everything else keeps the loopback discipline.
    #[serde(default)]
    pub fd_host_enable: bool,
    /// TCP port the club-sync host listens on (and the beacon advertises).
    #[serde(default = "default_fd_host_port")]
    pub fd_host_port: u16,
    /// Operator-facing club event name ("W9ABC Field Day") — the beacon and
    /// the welcome carry it; also names the host's event journal.
    #[serde(default)]
    pub fd_event_name: String,
    /// Join a club event at `host:port` (manual entry, or filled by the
    /// "Find club events" discovery). Empty = not joining. Ignored while
    /// [`Self::fd_host_enable`] is on — the host joins itself over loopback.
    #[serde(default)]
    pub fd_join_addr: String,
    /// Friendly position label for the club band board ("CW tent").
    #[serde(default)]
    pub fd_position_name: String,
    /// This machine's club-sync position identity: 8 hex chars, generated
    /// once at startup when empty and persisted. Non-edited (no UI control):
    /// QSO ids are `(this, seq)`, so changing it would re-push every contact
    /// as new. Also suffixes the FD ADIF journal, which is what stops two
    /// instances sharing a settings dir from clobbering each other's backup.
    #[serde(default)]
    pub fd_position_id: String,
    /// Serve the read-only spectator scoreboard — a self-contained web page of
    /// the club score for a TV/projector on the site LAN. **This toggle IS the
    /// LAN opt-in**: while on, `tempo_app::fd_scoreboard`'s GET/HEAD-only
    /// server binds `0.0.0.0:fd_scoreboard_port` (threat model in that
    /// module's header — the data is what the event broadcasts on the air).
    /// Default OFF; shows real data only in the host role.
    #[serde(default)]
    pub fd_scoreboard: bool,
    /// TCP port the spectator scoreboard serves on ("73 73" — unassigned,
    /// memorable for hams).
    #[serde(default = "default_fd_scoreboard_port")]
    pub fd_scoreboard_port: u16,
    /// Serve Connect as a read-only web page for a shack TV or a browser on the
    /// house network. **This toggle IS the LAN opt-in**: while on,
    /// `tempo_app::connect_web` serves through the same GET/HEAD-only server on
    /// `0.0.0.0:connect_web_port`.
    ///
    /// ⚠️ Its threat model is NOT the spectator scoreboard's. That one is defensible
    /// partly because a contest log is already broadcast in clear on the air; this
    /// page is the station's own conditions picture. It carries the callsign, the
    /// grid and the propagation nowcast, and deliberately NOT the dial frequency,
    /// the log or the needs board — a payload-shape test in `connect_web` pins that.
    /// Default OFF, and the Settings copy must say what it exposes and to whom.
    #[serde(default)]
    pub connect_web: bool,
    /// TCP port the Connect web page serves on. Distinct from the scoreboard's so a
    /// Field Day host can serve both at once.
    #[serde(default = "default_connect_web_port")]
    pub connect_web_port: u16,
    /// Periodically transmit a presence beacon ("CQ <call> <grid>") in Chat
    /// mode. **Off by default** — the app starts passive (hunt-and-pounce):
    /// it listens and only transmits when the operator acts (sends a message,
    /// answers, calls CQ, or enables this).
    pub beacon: bool,
    /// IR-HARQ: buffer failed RV0 frames and joint-combine RV1/RV2
    /// retransmissions at the receiver, and escalate the redundancy version on
    /// unacknowledged QSO transmissions. **On by default.** Turn off to force
    /// RV0-only (each frame decoded independently) — useful for A/B comparison
    /// or as a fallback.
    pub harq_enabled: bool,

    // --- rig / PTT ---
    /// PTT method: "cat" (Tempo launches/uses rigctld), "rts", "dtr", or "vox".
    pub ptt_method: String,
    /// Hamlib rig model number (for rigctld `-m`). 0 = none / VOX.
    pub rig_model: u32,
    /// Friendly rig name (display only).
    pub rig_model_name: String,
    /// Serial port for CAT / serial-PTT, e.g. "COM5" or "/dev/ttyUSB0" ("" = none).
    pub serial_port: String,
    /// Serial port for RTS/DTR PTT when it is NOT the CAT port — e.g. an SO2R
    /// controller (microHAM u2R/MK2R, YCCC box) routes keying on its own COM port
    /// while CAT rides the radio's USB. Empty = fall back to `serial_port` (the prior
    /// behavior). Global on purpose: an SO2R controller routes ONE PTT line to the
    /// selected radio, so it isn't per-radio. `#[serde(default)]` so old files load.
    #[serde(default)]
    pub ptt_serial_port: String,
    /// What the CAT port's RTS / DTR control lines are held at for the whole session:
    /// `"low"` (default), `"high"`, or `"untouched"`. Parsed by
    /// `tempo_audio::rigctld_proc::LineState::from_setting`, which treats anything
    /// unrecognised — including the empty string — as `"low"`.
    ///
    /// **Why it exists.** A serial port's driver raises RTS and DTR on open and Hamlib does not
    /// put them back down (it lowers only a line it is itself keying), so on Nexus's `vox`
    /// default an interface wired to key from RTS was **keyed by the act of connecting**, all
    /// session. 1.0.2 holds both low; that is what `"low"` means and it is right for almost
    /// every station.
    ///
    /// The other two exist because holding a line low is not free for everyone:
    /// - `"high"` powers an accessory that draws its supply from the line — an RS-232-era
    ///   homebrew CI-V level converter, a K1EL-style serial keyer. Hamlib's own wording for
    ///   these parameters is "for external powering".
    /// - `"untouched"` says nothing at all: exactly 1.0.1 behaviour, and the in-app recovery
    ///   for anything this change breaks that nobody foresaw. Without it the only way back
    ///   would be a downgrade.
    ///
    /// WSJT-X ("Force Control Lines: DTR/RTS = High | Low | blank") and fldigi both expose the
    /// same per-line three-way; they default to leave-alone, Nexus defaults to low.
    ///
    /// ⚠️ Global, not per-radio, and that is a compromise rather than a finding: this is a
    /// property of the CABLE on a given radio's CAT port, so a two-radio station with a
    /// line-powered accessory on only one of them cannot express that yet.
    ///
    /// Neither value can key a line Hamlib is using: `rig_open` refuses `rts_state`/`dtr_state`
    /// on the keying line and on a hardware-handshake RTS, and Nexus asks the daemon which
    /// lines it will accept before emitting anything.
    #[serde(default)]
    pub cat_rts_state: String,
    /// See [`Settings::cat_rts_state`] — the same, for DTR.
    #[serde(default)]
    pub cat_dtr_state: String,
    /// "My interface keys PTT on the CAT port's RTS line" — the operator telling us something
    /// we cannot detect.
    ///
    /// ⚠️ THIS EXISTS BECAUSE AUTODETECTION CANNOT WORK HERE (issue #44). A single-cable
    /// interface like a Digirig Mobile keys RTS on the same port that carries CAT, and Nexus
    /// must then hold RTS low or the rig transmits from the moment the port opens. But a stock
    /// Digirig enumerates as a plain `CP2102 USB to UART Bridge` — byte-for-byte the USB
    /// identity an FTDX10 and several Xiegu radios present — so no VID/PID or product-string
    /// rule can tell "cable that keys RTS" from "radio that needs its hardware handshake".
    /// Guessing permissively is exactly what killed CAT on FTDX10/FT-991 at the bench
    /// (2026-08-09), and guessing conservatively is what left vk6mo's TS-2000 keyed from launch
    /// through four releases.
    ///
    /// So we ask. The operator can see which cable is plugged in; we cannot.
    ///
    /// DEFAULT FALSE, and that matters: ticking it lets Nexus drop the rig's declared hardware
    /// handshake so RTS becomes holdable, which is precisely the thing that must never happen
    /// to a rig that did not ask for it.
    #[serde(default)]
    pub cat_rts_keys_ptt: bool,
    /// The rig's serial HANDSHAKE, stated rather than inferred: `"auto"` (default) | `"none"` |
    /// `"hardware"` | `"xonxoff"`.
    ///
    /// ⚠️ #145 IS THE THIRD TIME AN INFERENCE HERE WAS WRONG, so this one is asked. `"auto"` is
    /// today's behaviour to the byte — Nexus reads the backend's declaration out of the
    /// operator's own Hamlib and drops the handshake only when [`Settings::cat_rts_keys_ptt`] or
    /// a recognised keying cable says RTS is deliberate. That inference cannot reach the
    /// reported case at all: with PTT = serial RTS on the CAT port, Hamlib reports RTS
    /// unsettable BECAUSE it is the keying line, which makes the handshake override's own
    /// precondition false — so rigctld launches saying nothing whatever about RTS and the
    /// line's idle state falls to Hamlib's default and the USB-serial driver.
    ///
    /// Anything other than `"auto"` is emitted verbatim as `-C serial_handshake=…` and REPLACES
    /// the inference. It is the operator's declaration about their own cable, which is the only
    /// place that fact has ever lived — see [`Settings::cat_rts_keys_ptt`] for why no VID/PID
    /// rule can supply it.
    #[serde(default = "default_cat_auto")]
    pub cat_serial_handshake: String,
    /// What the KEYING line (serial RTS/DTR PTT) is held at while idle: `"auto"` (default) |
    /// `"untouched"` | `"low"` | `"high"`.
    ///
    /// ⚠️ THIS IS THE LINE THAT KEYS THE TRANSMITTER (#145). [`Settings::cat_rts_state`] /
    /// [`Settings::cat_dtr_state`] are deliberately never emitted for the line Hamlib is keying
    /// with, so today the keying line's idle level is whatever `rig_open` and the serial driver
    /// leave it at — which on a CP210x can be ASSERTED, i.e. the rig keyed from launch. `"auto"`
    /// keeps exactly that (say nothing), so no working station changes under an upgrade.
    ///
    /// ⚠️ NEEDS-BENCH, and it is not a formality: Hamlib's `rig_open` REFUSES `rts_state` on the
    /// line it keys with (`rig.c:1253-1272`), and that refusal is the SILENT kind — `-RIG_ECONF`,
    /// rigctld does not exit, and it goes on serving a rig it never opened. So a non-`"auto"`
    /// value can cost this operator CAT entirely on some backends and fix a keyed-at-launch rig
    /// on others, and which one it does cannot be established from this machine — there is no
    /// serial rig here. It exists so the operator who can watch the pin has a knob to turn.
    #[serde(default = "default_cat_auto")]
    pub cat_ptt_line_state: String,
    /// Serial baud rate for CAT.
    pub baud: u32,
    /// Rig connection type: "serial" (default; rigctld talks to `serial_port`/`baud`) or
    /// "network" (rigctld talks to `rig_addr` over TCP — e.g. a FlexRadio via SmartSDR).
    /// Empty is treated as "serial". `#[serde(default)]` so older settings files still load.
    #[serde(default)]
    pub rig_conn: String,
    /// Network rig address `host:port` when `rig_conn == "network"` (e.g. a Flex's SmartSDR
    /// IP `192.168.1.50:4992`). Ignored for serial.
    #[serde(default)]
    pub rig_addr: String,
    /// OmniRig slot for the active radio (flat mirror of the profile field — see
    /// [`RadioProfile::omnirig_slot`]). 1 = RIG 1, 2 = RIG 2; 0 reads as RIG 1.
    #[serde(default)]
    pub omnirig_slot: u8,
    /// Native Icom CI-V for the active radio (flat mirror of the profile field — see
    /// [`RadioProfile::icom_native_cat`]). Default off.
    #[serde(default)]
    pub icom_native_cat: bool,
    /// Follow the radio's OWN split, rather than only a split Nexus set. Default OFF.
    ///
    /// Only meaningful on a rig whose capability dump says it can report both split state and
    /// the split TX frequency NATIVELY, with frequency targetable
    /// (`baud_ladder::SplitDetect::Native`). On anything else Nexus would have to move the radio
    /// to answer the question, so the setting is not offered and this flag has no effect — the
    /// gate never consults a reading it was told is emulated.
    ///
    /// ⚠️ NEEDS BENCH. Class-wide CAT behaviour; ships OFF so nothing changes for anyone who
    /// does not choose it, and wants a real radio in front of someone before it is called
    /// working — in particular the question no source answers: does a rig report a split set
    /// from its FRONT PANEL, as opposed to one set over CAT?
    #[serde(default)]
    pub split_detect_enabled: bool,
    /// Which Icom DATA mode the active radio uses (flat mirror — see
    /// [`RadioProfile::icom_data_mode`]). 1 is today's behaviour.
    #[serde(default = "one")]
    pub icom_data_mode: u8,
    /// Plain SSB instead of the DATA submode on the soundcard modes, for the active radio
    /// (flat mirror — see [`RadioProfile::data_modes_plain_ssb`]). Default off.
    #[serde(default)]
    pub data_modes_plain_ssb: bool,
    /// DEPRECATED / ignored. Digital now ALWAYS forces the DATA submode (like Phone/CW
    /// force their mode), so this opt-out is no longer consulted by
    /// [`rig_mode`](Self::rig_mode). Kept only so older settings files still deserialize.
    /// (A rig without a DATA submode is handled by the radio loop's bounded set_mode
    /// retry — it tries once, the rig rejects it, and it gives up.)
    pub set_rig_mode: bool,
    /// The active operating mode (Digital / Phone / CW) — the per-section rig-mode
    /// policy. Digital obeys the rig; Phone/CW force USB-LSB / CW. See [`rig_mode`].
    pub operating_mode: OperatingMode,
    /// Amateur license class — drives the transmit-privilege lockout + the licensed-segment
    /// band dropdown. `Open` (default) = no restrictions (non-US). See [`LicenseClass`].
    #[serde(default)]
    pub license_class: LicenseClass,
    /// How CW is keyed (CAT `send_morse` vs soundcard tone). Also picks the CW
    /// rig-mode: CAT → CW, Soundcard → USB (audio tone). See [`rig_mode`].
    pub cw_keyer: CwKeyerBackend,
    /// Serial port for the K1EL WinKeyer (when `cw_keyer == WinKeyer`), e.g. "COM6".
    pub winkeyer_port: String,
    /// Serial port for the DTR/RTS CW keyline (when `cw_keyer == Serial`), e.g. "COM7" —
    /// a SEPARATE port from CAT (the keying interface into the rig's KEY jack).
    #[serde(default)]
    pub cw_key_port: String,
    /// Which control line keys the rig for the serial keyline: "dtr" (default, the CW
    /// convention) or "rts". Parsed by `serial_keyer::KeyLine::parse`.
    #[serde(default = "default_cw_key_line")]
    pub cw_key_line: String,
    /// CW sidetone / keyed-tone pitch in Hz (soundcard keyer + UI marker). Default 600.
    pub cw_pitch_hz: f32,
    /// Operator CW sending speed in WPM. Persisted so it survives a restart — an operator
    /// who works at 15 shouldn't be reset to the 25 default every launch (SF ticket #2).
    /// Global, not per-radio: WPM is a property of the OPERATOR's fist, and the two sibling
    /// CW controls (`cw_keyer`, `cw_pitch_hz`) are already global, so scoping this one
    /// per-radio would split a single toolbar across two persistence scopes.
    #[serde(default = "default_cw_wpm")]
    pub cw_wpm: u32,
    /// How RTTY is keyed: "afsk" (default — soundcard two-tone audio through the rig in
    /// LSB; soundcard-clocked, the timing-cleanest path) or "fsk" (true FSK — bit-bang a
    /// serial control line into the rig's FSK input, rig in RTTY mode, unlocking its
    /// narrow RTTY filters). Also picks the RTTY rig-mode: see [`rig_mode`](Self::rig_mode).
    #[serde(default = "default_rtty_backend")]
    pub rtty_backend: String,
    /// Which control line carries the FSK data bits when `rtty_backend == "fsk"`:
    /// "dtr" (default — the common wiring, RTS left for PTT) or "rts". PTT must ride
    /// its OWN path (CAT or the PTT serial line) — never this line.
    #[serde(default = "default_rtty_fsk_line")]
    pub rtty_fsk_line: String,
    /// Serial port for the FSK keying line (when `rtty_backend == "fsk"`), e.g. the
    /// FTDX10's USB *Enhanced* COM port. Empty = the CAT `serial_port` (the same
    /// fallback the RTS/DTR PTT line uses).
    #[serde(default)]
    pub rtty_fsk_port: String,
    /// RTTY baud rate — true 45.45 by default (never integerized to 45); 75.0 is the
    /// other common amateur rate. Drives the TX bit clock AND the RX demodulator.
    #[serde(default = "default_rtty_baud")]
    pub rtty_baud: f64,
    /// RTTY mark/space shift in Hz (170 = the HF standard). Drives the TX tone pair
    /// (AFSK space = 2125 + shift) AND the RX demodulator.
    #[serde(default = "default_rtty_shift_hz")]
    pub rtty_shift_hz: u32,
    /// Reverse the mark/space sense (TX tones + RX demod). The standard convention
    /// is LSB with mark on the lower audio tone; set this when running the rig on
    /// the opposite sideband (e.g. AFSK in USB/DATA-U) so the RF sense stays correct.
    #[serde(default)]
    pub rtty_reverse: bool,
    /// AI CW decoder (DeepCW model): the PRIMARY CW decode — dramatically better
    /// low-SNR copy than the classic Goertzel decoder (which still supplies the WPM
    /// estimate underneath). On by default; the model ships with the app.
    ///
    /// ⚠️ Read this through [`Settings::ai_cw_active`], never directly, at any gate that
    /// decides whether the decoder RUNS — `unassisted_mode` overrides it.
    #[serde(default = "default_true")]
    pub ai_cw_enabled: bool,
    /// UNASSISTED MODE — the operator's declaration that this contest entry uses no
    /// QSO-finding assistance. One switch, so there is exactly one thing to get right
    /// before an event, and one thing to point at afterwards.
    ///
    /// While on it SUPPRESSES every assistance source at once: the DeepCW AI CW decoder,
    /// DX cluster / RBN spot ingestion, and the PSK Reporter reception-report feed that
    /// drives the Needed board. Each state change is journaled with a timestamp so the
    /// operator can show what was running during the event.
    ///
    /// **It overrides, never overwrites.** `ai_cw_enabled` / `cluster_enabled` /
    /// `pskreporter` keep the operator's own values untouched; the effective getters
    /// below ([`Settings::ai_cw_active`] and friends) fold this flag in. Turning the
    /// switch back off therefore restores the operator's setup exactly, with no saved
    /// shadow copy to drift out of sync.
    ///
    /// Persisted so it survives a restart mid-contest, and — like [`Settings::fd_active`]
    /// — set true ONLY by the operator's explicit toggle. Default false: assistance is a
    /// normal, legal way to operate outside a contest, and silently changing what an
    /// operator hears would be worse than the exposure this closes.
    #[serde(default)]
    pub unassisted_mode: bool,
    /// Local TCP port Tempo uses for rigctld (it spawns rigctld on this port).
    pub rigctld_port: u16,
    /// Antenna rotator, the INTEGRATED way: a Hamlib rotator model number
    /// (0 = no rotator) + serial port + baud — Nexus launches the bundled
    /// `rotctld` itself, exactly like the rig's rigctld. No command lines.
    #[serde(default)]
    pub rotator_model: u32,
    #[serde(default)]
    pub rotator_port: String,
    #[serde(default = "default_rotator_baud")]
    pub rotator_baud: u32,
    /// The amplifier family on this radio's amp port: "" = none (the default, and the state of
    /// most stations), "spe" = SPE Expert 1.3K-FA/1.5K-FA/2K-FA, "kpa" = Elecraft KPA500/KPA1500.
    ///
    /// PER RADIO, like the rotator: an SO2R station has an amplifier per radio, and a field that
    /// lived only on the flat `Settings` would let one radio's amp config overwrite the other's.
    #[serde(default)]
    pub amp_model: String,
    /// Serial port the amplifier is on. Empty = not configured, which is what makes every
    /// amplifier surface render NOTHING rather than an empty frame (the rotator's honesty rule:
    /// unconfigured shows nothing, configured-and-silent shows "—").
    #[serde(default)]
    pub amp_port: String,
    /// Step the amplifier to the band the radio is on, without being asked. **Off by default.**
    ///
    /// ⚠️ OFF IS DELIBERATE, AND NOT JUST CAUTION. The standing rule in this app is that Nexus
    /// notifies and never moves the station unattended; an amplifier is a slaved accessory
    /// rather than the thing making the QSO, which is why this is offered at all. But it is
    /// still Nexus putting a command on a kilowatt's wire with nobody's hand on it, so the
    /// operator turns it on rather than discovering it.
    ///
    /// ⭐ AND THE TWO FAMILIES DO NOT CARRY THE SAME RISK. Elecraft sets a band ABSOLUTELY
    /// (`^BNbb;`) against a table Elecraft publishes in full, so following is one command whose
    /// result is read back on the next poll. SPE can only STEP (`BAND-`/`BAND+`), and the middle
    /// of its ladder is derived from two published endpoints plus one measured point rather than
    /// published — so following there is several commands walking a table that has never been
    /// confirmed end to end on hardware. Both honour this switch; only one of them is proven.
    ///
    /// ⚠️ AND ON MOST SPE STATIONS THIS SHOULD STAY OFF FOR A REASON THAT IS NOT ABOUT RISK.
    /// An SPE is normally wired to follow the radio through its own band-data cable, in
    /// hardware. Where that cable is fitted, this setting is a SECOND thing steering one band —
    /// redundant at best, and at worst two controllers disagreeing about where the amplifier
    /// should be. Reported by the operator on 2026-08-29, whose own 1.5K-FA is wired exactly
    /// that way; it is also why the SPE ladder's middle is still unmeasured here, since testing
    /// the step would have meant unplugging a cable that is doing the job correctly.
    ///
    /// The setting's own hint says this, in all four catalogs. It is the difference between a
    /// switch an operator can judge and one they have to guess at.
    #[serde(default)]
    pub amp_follow_band: bool,
    /// ADVANCED override: an external `rotctld` daemon address `host:port`
    /// (for operators who already run their own). Non-empty wins over the
    /// integrated model/port spawn. Empty + model 0 = no rotator.
    pub rotator_host: String,
    /// Rotator manners and positions — see `tempo_core::rotator`. Defaults are
    /// the conservative ones: no flip (many rotators physically cannot), and
    /// post-pass STOP (never move a mast the operator didn't ask to move).
    #[serde(default)]
    pub rot_park_az: f64,
    #[serde(default)]
    pub rot_park_el: f64,
    #[serde(default)]
    pub rot_ready_az: f64,
    #[serde(default)]
    pub rot_ready_el: f64,
    /// "stop" | "park" | "ready".
    #[serde(default)]
    pub rot_post_pass: String,
    #[serde(default = "default_rot_tol_deg")]
    pub rot_tol_az_deg: f64,
    #[serde(default = "default_rot_tol_deg")]
    pub rot_tol_el_deg: f64,
    #[serde(default)]
    pub rot_cal_az_deg: f64,
    #[serde(default)]
    pub rot_cal_el_deg: f64,
    #[serde(default)]
    pub rot_allow_flip: bool,

    /// SATELLITE DOPPLER — the operator's OFF switch. Absent (false) = correct
    /// the downlink during a pass, which is what arming one and holding a
    /// transponder asks for.
    ///
    /// The polarity is inverted deliberately. This replaced a `satDoppler`
    /// OPT-IN that defaulted off, and every settings file written before 0.26
    /// carries `"satDoppler": false` whether or not its operator ever saw the
    /// switch — so flipping that default would have reached nobody, least of
    /// all the station that reported "I armed a pass and Doppler did not
    /// tune". An absent field is the only honest "never said". The old key
    /// never seeds THIS switch; [`Settings::load`] reads it for exactly one
    /// thing — deciding whether a pre-0.26 mapping was a live uplink grant or
    /// an inert pick (see the consent migration there).
    ///
    /// An off SWITCH is fine; a default-off GATE is not. This stops both legs.
    #[serde(default)]
    pub sat_doppler_off: bool,
    /// Which VFO carries the UPLINK. **This is the setting that can transmit on
    /// your own downlink if it is wrong**, so it is explicit, enumerated and
    /// operator-visible rather than inferred — see [`SatVfoMap`]. The downlink
    /// needs no mapping ([`Settings::sat_doppler_downlink`]).
    #[serde(default)]
    pub sat_vfo_map: SatVfoMap,
    /// The radios `sat_vfo_map` has been CONFIRMED for, by `RadioProfile::id`.
    ///
    /// A VFO mapping is a fact about one station's wiring, and the satellite
    /// path routes: peg-lock, a routing rule or a mid-pass handoff can put a
    /// different rig under the split than the one the operator was looking at
    /// when they chose. So the consent carries the radio it was granted for,
    /// and [`Settings::sat_doppler_uplink`] re-checks it against the rig that
    /// will actually receive the write.
    ///
    /// `None` = TRANSIENT pre-migration state (a file with no key — pre-0.26
    /// — or a `null`), and it confirms nothing: [`Settings::load`] always
    /// resolves it, so post-load this is `Some`. A pre-0.26 file whose
    /// retired `satDoppler` master switch was on beside an uplink-driving
    /// mapping (the pair that actually drove the transmit VFO pre-upgrade)
    /// has its station-wide grant MATERIALIZED as the ids of every radio in
    /// the file — durable across save/load, prunable with removed radios —
    /// and every other loaded file normalizes to `Some(vec![])`: a mapping
    /// picked while the master was off drove nothing, and drives nothing now
    /// until a radio is confirmed. A fresh install reaches the uplink only
    /// through a confirmation, which writes the list.
    #[serde(default)]
    pub sat_uplink_radios: Option<Vec<u32>>,
    /// Minimum correction (Hz) worth sending to the radio. Below this the dial
    /// is left alone: continuous CI-V writes fight the operator's knob and
    /// saturate the bus for a shift nobody can hear. 0 = send every update.
    #[serde(default = "default_sat_min_shift_hz")]
    pub sat_min_shift_hz: u32,
    /// Minimum interval (ms) between corrections — the other half of the rate
    /// limit. The radio is a serial device, not a socket.
    #[serde(default = "default_sat_update_ms")]
    pub sat_update_ms: u32,
    /// Audible AOS/LOS earcons for the ARMED pass track (the "Work this pass"
    /// alerts, satPassAlert.ts). An OFF switch, inverted like
    /// [`Settings::sat_doppler_off`]: absent/false = tones ON, which is what
    /// was asked for ("we need audible tones … when a pass starts and ends").
    /// Silences only the tones — the popups are not gated.
    #[serde(default)]
    pub sat_pass_alert_sound_off: bool,

    /// Run the rigctld-compatible CAT **broker** so other apps (WSJT-X / N1MM /
    /// loggers) share the radio THROUGH Nexus, on `cat_broker_port`. ON by default
    /// (#53): the broker answers from cached engine state and survives every
    /// Hamlib-daemon teardown, so it — not the daemon's port — is what the share
    /// block advertises. No field-level serde default: the container default
    /// applies, so pre-1.0.5 files (no field) come up ON; a 1.0.5 file's explicit
    /// `false` is kept.
    pub cat_broker: bool,
    /// TCP port the CAT broker listens on (Hamlib NET rigctl default 4532).
    pub cat_broker_port: u16,

    /// A FlexRadio's IP address for the SmartSDR Ethernet API (port 4992), used by the native
    /// panadapter worker. Distinct from the CAT `rig_addr` (which for the SmartSDR-CAT model 2036
    /// points at the *PC's* CAT port, not the radio). Empty = no native Flex scope.
    #[serde(default)]
    pub flex_radio_ip: String,
    /// Opt-in to the FlexRadio native SmartSDR panadapter (VITA-49 FFT). OFF by default: the
    /// worker + command syntax are UNVERIFIED on a real Flex, so a tester enables it here, and
    /// it becomes the default only once proven. Mirrors the Icom `icom_native_cat` opt-in.
    /// Takes effect on the next tick for an active network Flex.
    #[serde(default)]
    pub flex_native_pan: bool,
    /// Opt-in to native FlexRadio DAX audio (VITA-49 audio streams) instead of the WDM-KS "DAX
    /// Audio RX" / "DAX TX" soundcard devices — which break under Remote Desktop. OFF by default:
    /// the worker + SmartSDR command syntax are UNVERIFIED on a real Flex, so a tester enables it
    /// here. BOTH DIRECTIONS, not RX only (which is what this said while the opposite shipped):
    /// receive audio comes straight off the network and feeds the decoders like soundcard audio,
    /// and transmit audio is routed to the radio over DAX as well — which disconnects the rig's
    /// microphone for as long as the toggle is on. That is deliberate (operator ruling 2026-07-26):
    /// one toggle means native audio both ways, since a half-native path is a configuration that
    /// mostly exists to be got wrong. Turning it off, switching radio or exiting Nexus puts the mic
    /// back. Mirrors `flex_native_pan`.
    #[serde(default)]
    pub flex_native_audio: bool,

    // --- multi-radio (dual-radio) ---
    /// Configured radios. EMPTY in older settings files → migrated to a single profile 0 mirroring
    /// the flat rig/audio fields above (see `ensure_radio_profiles`). A single-radio station always
    /// has exactly one, and the flat fields are kept mirrored to the ACTIVE profile so every
    /// existing consumer (Transport::from_settings, sync_rotctld, rig_mode) reads them unchanged.
    #[serde(default)]
    pub radios: Vec<RadioProfile>,
    /// The id of the ACTIVE radio (the one the UI commands + the operating scope shows).
    #[serde(default)]
    pub active_radio: u32,
    /// Peg-lock: when true, band selection never auto-switches the active radio.
    #[serde(default)]
    pub radio_pegged: bool,
    /// Opt-in: run two radios at the SAME TIME, one per app window. When true AND ≥2 radios are
    /// configured, launching Nexus shows a "which radio?" picker (each window drives one radio,
    /// sharing one logbook). Off by default, so a single-radio station — or one with two radios
    /// it never runs together — is never bothered with the picker. See [[reference-multiradio-architecture]].
    #[serde(default)]
    pub simultaneous_radios: bool,
    /// Band+mode → radio routing rules, FIRST-MATCH-WINS (see [`RoutingRule`]). Lets a station
    /// split ONE band between two rigs by mode — "2 m FT8 to the IC-9700, 2 m FM/APRS to the
    /// FT-991A" — which the band-only `bands` coverage on each [`RadioProfile`] structurally
    /// cannot express. EMPTY by default: an upgrading install keeps exactly today's band-coverage
    /// behavior until the operator writes a rule.
    #[serde(default)]
    pub routing_rules: Vec<RoutingRule>,
    /// Fallback radio when no rule and no band coverage claims a (band, mode) — "everything else
    /// goes here". `None` (the default) = stay on the active radio, i.e. today's behavior.
    #[serde(default)]
    pub default_radio: Option<u32>,

    // --- network (WSJT-X parity) ---
    /// Emit the WSJT-X-compatible UDP protocol (for JTAlert/GridTracker/loggers).
    pub wsjtx_udp: bool,
    /// UDP address to send WSJT-X messages to (WSJT-X default is 127.0.0.1:2237).
    pub wsjtx_udp_addr: String,
    /// Append every decode to a WSJT-X-format `ALL.TXT` decode log in the app data dir —
    /// the running record loggers/GridTracker tail. Off by default.
    pub write_all_txt: bool,
    /// Write the DEBUG tier to the diagnostic log — per-over keying, per-period decode counts,
    /// CAT traffic. **Off by default and meant to stay off**: it is the "we are chasing
    /// something, turn it up for this session" switch, not a better log. It writes to the same
    /// file so there is still one thing to send, and the startup header records that it was on
    /// (a reader months later must never wonder whether the quiet was real or just the level).
    pub diag_debug_log: bool,
    /// Push each logged QSO to Ham Radio Deluxe Logbook over its QSO-Forwarding UDP
    /// listener (one raw ADIF record per datagram — the same standard WSJT-X/JTAlert
    /// use). Off by default. HRD Logbook must be running.
    pub hrd_logging: bool,
    /// HRD Logbook QSO-Forwarding address (UDP). HRD's default is 127.0.0.1:2333.
    pub hrd_udp_addr: String,
    /// UDP address to *listen* on for an upstream WSJT-X/JTDX/MSHV decode stream
    /// when the signal source is Companion (the sink those apps transmit to;
    /// WSJT-X default 127.0.0.1:2237).
    pub companion_addr: String,
    /// Persisted RX signal source — native decode vs a WSJT-X/JTDX/MSHV companion
    /// stream. Restored at startup so the operator's choice survives restart.
    pub source: SourceKind,
    /// Upload heard stations to PSK Reporter.
    pub pskreporter: bool,
    /// Connect to spot networks for need-aware spots (takes effect at startup). When on,
    /// the RBN CW (7000) + RBN digital (7001) skimmer firehoses are connected for the big
    /// CW + digital evidence, PLUS the human DX-cluster node in `cluster_host` for SSB/phone
    /// (which RBN doesn't carry). SpotCollector-style multi-source aggregation.
    pub cluster_enabled: bool,
    /// LEGACY single human DX-cluster endpoint — kept only to seed `cluster_hosts` on
    /// upgrade (and for back-compat). `cluster_hosts` is the live source of truth.
    pub cluster_host: String,
    /// The human DX-cluster node LIST — the SSB/phone aggregator. Each entry is a
    /// DXSpider/CC-Cluster telnet endpoint ("host:port"); we connect to ALL of them and
    /// union their human spots (the RBN CW/digital skimmer feeds are wired automatically, so
    /// RBN endpoints are ignored here). More nodes = wider phone coverage. Empty = RBN only
    /// (no phone). `#[serde(default)]` (empty) so an OLD config missing this field is detected
    /// in `load` and seeded from `cluster_host`; the Default impl seeds the community node.
    #[serde(default)]
    pub cluster_hosts: Vec<String>,

    // --- APRS-IS (the internet side of APRS) ---
    /// Connect to APRS-IS and plot internet-reported stations alongside the ones our own antenna
    /// hears.
    ///
    /// ⚠️ **Deliberately independent of the RF decoder's arm state.** Arming APRS is a decision
    /// about the receiver — it costs audio, and (armed explicitly) it is one of the two gates on an
    /// unattended auto-ack. The internet feed has neither cost: it consumes no RF resource and can
    /// never key a transmitter. Tying the two together would mean an operator who only wants to
    /// watch the network must first arm a decoder they are not using — and, worse, would remove
    /// the single most useful diagnostic this feed provides: internet stations plotting while the
    /// RF chip stays silent proves the fault is in the radio chain and not in the app. The
    /// *uplink* is different: it has nothing to send unless the RF decoder is running.
    #[serde(default)]
    pub aprs_is_enabled: bool,
    /// APRS-IS server hostname. The regional Tier 2 rotate addresses (`noam.aprs2.net`,
    /// `euro.aprs2.net`, …) are preferred; `rotate.aprs2.net` is the worldwide fallback.
    #[serde(default = "default_aprs_is_host")]
    pub aprs_is_host: String,
    /// APRS-IS port. 14580 is the user-defined filter port — the one clients and iGates should
    /// use. The full-feed ports (10152, 20152) would deliver the entire planet's traffic.
    #[serde(default = "default_aprs_is_port")]
    pub aprs_is_port: u16,
    /// Radius (km) around the station for the server-side range filter. APRS is a local mode;
    /// 150 km is a generous 2 m-plus-digipeater horizon. 0 = no range filter.
    #[serde(default = "default_aprs_is_radius_km")]
    pub aprs_is_radius_km: u32,
    /// Watched callsigns passed regardless of distance (the APRS-IS budlist) — the friend or
    /// club station you want to see wherever they are.
    #[serde(default)]
    pub aprs_is_watch_calls: Vec<String>,
    /// Include weather stations and positionless weather reports in the feed.
    #[serde(default = "default_true")]
    pub aprs_is_weather: bool,
    /// Include objects and items (repeaters, NWS alerts, event markers).
    #[serde(default = "default_true")]
    pub aprs_is_objects: bool,
    /// Include APRS text messages. Display only — Nexus does not reply to an internet message.
    #[serde(default = "default_true")]
    pub aprs_is_messages: bool,
    /// Run as a receive-only iGate: contribute packets THIS station heard on the air to APRS-IS.
    ///
    /// Off by default, and rightly so — it publishes under the operator's callsign to a global
    /// network. Requires a real callsign (the passcode is derived from it) and the RF decoder
    /// actually running, since there is nothing to contribute otherwise. Nexus never gates the
    /// other way: internet→RF transmits unattended, which the alerts doctrine forbids.
    #[serde(default)]
    pub aprs_is_uplink: bool,
    /// How long (minutes) a heard APRS station stays on the map after its last packet.
    ///
    /// The retention window for the STATION store, not the packet log. Default 60: mobiles beacon
    /// every one to two minutes and fixed stations commonly every ten — as slowly as thirty — so an
    /// hour survives two missed beacons from the slowest legitimate beaconer. Shorter windows make
    /// slow fixed stations blink off between their own beacons, which is the bug this store exists
    /// to fix. Stations begin to FADE after a third of this, which is derived rather than configured
    /// so the two cannot contradict each other.
    #[serde(default = "default_aprs_station_ttl_min")]
    pub aprs_station_ttl_min: u32,

    // --- APRS over the air (the RF side) ---
    /// The regional 2 m FM APRS channel (MHz), or None to FOLLOW THE OPERATOR'S GRID.
    ///
    /// None is not "unset": the UI resolves it to a real channel on every read from
    /// `mygrid`, so the operator never meets an empty box, and an operator who moves
    /// gets their new region's channel without touching anything. Writing a concrete
    /// 144.39 here instead would freeze a US channel onto disk the moment the file is
    /// first saved, and no later improvement to the derivation could ever reach that
    /// operator. Global rather than per-radio: it is a property of the NETWORK the
    /// operator is standing in, and the app already hands APRS to whichever rig covers
    /// 2 m FM ([`Engine::aprs_tune`]).
    #[serde(default)]
    pub aprs_channel_mhz: Option<f64>,
    /// Beacon symbol code — the character that picks the icon within the table below.
    /// ">" is Car, which is what the cockpit has always sent.
    #[serde(default = "default_aprs_symbol_code")]
    pub aprs_symbol_code: String,
    /// Beacon symbol TABLE: "/" (primary) or "\\" (alternate). The cockpit hardcoded
    /// "/" before this; the alternate table is what digipeater (`\#`) and iGate (`\&`)
    /// need, which are the two most likely fixed-station identities here.
    #[serde(default = "default_aprs_symbol_table")]
    pub aprs_symbol_table: String,
    /// The free-text beacon comment. ⚠️ THIS STRING GOES ON THE AIR, so the default is
    /// a deliberate choice and not a placeholder. APRS caps it at 43 characters.
    #[serde(default = "default_aprs_comment")]
    pub aprs_comment: String,
    /// The digipeater path (e.g. `["WIDE1-1","WIDE2-1"]`). Empty is a LEGITIMATE value
    /// meaning "direct, no digipeaters".
    ///
    /// ⚠️ MUST be the named default fn, never a bare `#[serde(default)]`. Bare yields
    /// an empty vec for a settings file written before this field existed — which reads
    /// as a deliberate "no digipeaters" — so every existing operator would silently lose
    /// their hops with nothing on screen to explain it.
    #[serde(default = "default_aprs_path")]
    pub aprs_path: Vec<String>,
    /// The SSID every APRS frame we originate carries (-9 mobile, -10 iGate, -7 HT …),
    /// or None to FOLLOW THE CALLSIGN — take whatever `mycall` already spells out.
    ///
    /// ⚠️ `Option`, and that is the whole point. `Address::parse` already splits
    /// `KD9TAW-9` and returns SSID 9, so an unconditional write of a `u8` field would
    /// demote an operator who spells their SSID into `mycall` from -9 to -0 on upgrade —
    /// an on-air identity change nobody asked for. None applies nothing, which is
    /// today's behaviour exactly.
    #[serde(default)]
    pub aprs_ssid: Option<u8>,

    // --- audio I/O ---
    /// Input (capture) device name. Empty = system default input.
    pub audio_in: String,
    /// Output (playback) device name. Empty = system default output.
    pub audio_out: String,
    /// Microphone device for RECORDING voice-keyer messages. Empty (default) = keep
    /// today's behavior: record from `audio_in`, the shared capture input. But on a
    /// typical digital setup that input is the RIG's RX codec / DAX, so recording a
    /// voice message from it captures the BAND, not the operator's voice. Set this to
    /// the operator's actual mic and each recording opens a SEPARATE transient input
    /// on it for the recording's duration (the decode path / shared input is untouched).
    /// A configured device that fails to open falls back to the shared input.
    #[serde(default)]
    pub voice_mic_device: String,
    /// Tx audio level (0.0–1.0) applied to outgoing samples before they reach
    /// the sound card.
    pub tx_level: f32,
    /// RX capture gain: a ≥1.0 multiplier applied to received audio before decode. Headroom for a
    /// quiet interface (e.g. a rig codec whose line-out reads low in Nexus). 1.0 = unchanged.
    pub rx_gain: f32,
    /// Headphone monitor (DARK, off by default): live pass-through of the exact RX
    /// audio the decoder hears to a chosen output device, so the operator can HEAR
    /// the band and diagnose levels / RFI. Best-effort name guard against the rig's TX device (System default resolved first)
    /// (`audio_out`) — monitoring into it would transmit the received band back out.
    #[serde(default)]
    pub monitor_enabled: bool,
    /// Headphone-monitor output device name. Empty = system default output.
    #[serde(default)]
    pub monitor_device: String,
    /// Headphone-monitor playback level (0.0–1.0). Default 0.5.
    #[serde(default = "default_monitor_level")]
    pub monitor_level: f32,
    /// Station transmit power in WATTS (RF out), used by the Journey miles-per-watt
    /// + QRP feats. `None` until the operator sets it (those feats stay gated).
    #[serde(default)]
    pub station_power_w: Option<f64>,
    /// Display units for distances/temperature/speed (F4MQS): `"auto"` (from the OS
    /// locale — imperial only for US/LR/MM), `"metric"`, or `"imperial"`. Display-only —
    /// every wire/protocol value stays native (APRS transmits °F/mph, RepeaterBook fetches
    /// km). Default auto.
    #[serde(default = "default_units")]
    pub units: String,
    /// Per-mode RF-power CEILING as a 0.0–1.0 fraction of the rig's max — a SAFETY cap, not a
    /// convenience. A 100%-duty mode (FT8/FT4/RTTY) at a level that's fine for SSB's ~25% duty
    /// can cook a finals stage or a linear. `None` = uncapped (full power, unchanged behavior).
    /// Enforced at the single `set_rf_power` chokepoint AND re-applied on mode change
    /// ([`Engine::set_operating_mode`]), so switching SSB→FT8 brings the rig DOWN to the cap
    /// instead of waiting for the operator to touch the slider. See [`Settings::rf_power_ceiling`].
    #[serde(default)]
    pub max_power_phone: Option<f32>,
    #[serde(default)]
    pub max_power_cw: Option<f32>,
    #[serde(default)]
    pub max_power_digital: Option<f32>,
    /// AM's ceiling, as a fraction of the rig's max. AM rides `OperatingMode::Phone`, so without
    /// this it took the SSB cap — and AM is not SSB.
    ///
    /// ⚠️ A RIG MAKING 100 W PEP ON SSB MAKES ABOUT 25 W OF CARRIER ON AM, because AM's power is
    /// in a carrier that is always there plus two sidebands, and PEP is reached on modulation
    /// peaks. Run the SSB drive into AM and the peaks flat-top. Most rigs' manuals say a quarter,
    /// which is where the 0.25 default comes from — it is a starting point, not a rule, and any
    /// operator who knows their rig can raise it.
    ///
    /// Applied as the LOWER of this and the phone cap (see [`Settings::rf_power_ceiling`]), never
    /// on its own: an operator who set AM above phone must not have AM lift their power past what
    /// the phone cap allows. That min-shape is the same one `rf_power_ceiling_high_duty` uses for
    /// SSTV, and for the same reason — it can only ever LOWER power, which is what makes it safe
    /// without a bench.
    #[serde(default = "default_max_power_am")]
    pub max_power_am: Option<f32>,
    /// Path-prediction engine: "heuristic" (physics-lite, the default) or
    /// "p533" (the native ITU-R P.533 engine). Unknown values fall back to
    /// the heuristic in the factory, so old configs can never break.
    #[serde(default = "default_prop_engine")]
    pub prop_engine: String,
    /// Save each received period's audio as a WAV: "none" (default) | "all"
    /// (every RX period — ~2 GB/day, debugging/archival) | "decodes" (only
    /// periods that produced at least one decode). WSJT-X's Save menu.
    #[serde(default = "default_save_wav")]
    pub save_wav: String,
    /// LoTW-user highlight window (days): a decoded call marks as a LoTW
    /// uploader only if ARRL's activity list shows an upload within this many
    /// days (WSJT-X default: 365).
    #[serde(default = "default_lotw_max_age_days")]
    pub lotw_max_age_days: u32,
    /// Antenna gains (dBi) for the P.533 engine's link budget — TX and RX.
    /// 0 = isotropic (the honest default for a wire). Plain dB adders to the
    /// modelled signal; the heuristic engine ignores them.
    #[serde(default)]
    pub ant_tx_gain_dbi: f64,
    #[serde(default)]
    pub ant_rx_gain_dbi: f64,
    /// Opt-in: track a gentle weekly "on the air" streak in the Journey view.
    /// Off by default (the achievement layer is opt-in, never coercive).
    #[serde(default)]
    pub journey_streak_enabled: bool,
    /// Transmit watchdog: auto-halt TX after this many minutes of continuous
    /// keying. 0 = off.
    pub tx_watchdog_min: u32,

    // --- timing & tuning (FT8-style) ---
    /// Transmit on the even ("1st") T/R slots when true, odd ("2nd") when false.
    /// Two stations must pick OPPOSITE periods to complete a QSO (like WSJT-X's
    /// "Tx even/1st").
    pub tx_even: bool,
    /// Receive audio offset (Hz) — the green waterfall marker; where the operator
    /// is listening for the station being worked.
    pub rx_offset_hz: f32,
    /// Transmit audio offset (Hz) — the red waterfall marker; where our signal is
    /// placed in the SSB passband.
    pub tx_offset_hz: f32,
    /// Keep the TX offset fixed when the RX offset changes (WSJT-X "Hold Tx Freq").
    /// When false, setting RX (left-click) also moves TX to match.
    pub hold_tx_freq: bool,
    /// Periodically query an NTP server to show the real PC-clock-vs-UTC offset.
    /// On by default; fails silently when off-grid. Disable for fully-offline use.
    pub clock_check: bool,

    // --- logbook ---
    /// Auto-log a contact to the ADIF logbook when a QSO completes. On by
    /// default — every completed auto-sequenced QSO is recorded once.
    pub auto_log: bool,
    /// Prompt the operator to confirm/edit the QSO before logging (WSJT-X
    /// "Prompt me to log QSO"). When true the snapshot exposes a pending log
    /// record instead of silently writing it; the UI shows a confirm popup.
    /// Off by default (silent auto-log). Has no effect unless `auto_log`.
    #[serde(default)]
    pub prompt_to_log: bool,
    /// Auto-save a WAV of the recent receive audio when a QSO is logged — an automatic
    /// per-contact recording, written to the recordings folder. Off by default.
    #[serde(default)]
    pub save_qso_wav: bool,

    // --- QSO behaviour ---
    /// Roger the final report with a bare `RRR` (partner still owes a 73) instead
    /// of the combined `RR73`. Off by default (RR73 — modern FT8 practice).
    #[serde(default)]
    pub prefer_rrr: bool,
    /// Stop a CQ run after this many unanswered calls, then wait [`Self::cq_pause_secs`]
    /// and start again. `Some(8)` by default (operator ruling); `None` = stock WSJT-X,
    /// which repeats CQ indefinitely with only the Tx watchdog as a backstop.
    ///
    /// ⚠️ A DELIBERATE DIVERGENCE FROM WSJT-X, and it is worth saying because an operator
    /// running both will see the difference and read it as a fault. WSJT-X calls CQ until
    /// something stops it; Nexus calls eight times, breathes, and calls again. The reason is
    /// band courtesy — an unanswered run holds a frequency other people could be using — and
    /// it costs nothing, because the pause still ANSWERS anyone who calls: only the outgoing
    /// CQ is withheld, never the sequencer's ability to reply.
    ///
    /// Counting is per-STEP, so it never bites a working run: `tx_count` resets the moment a
    /// QSO advances, which is why "if stations keep calling back, it keeps working them" is
    /// the behaviour rather than a special case.
    #[serde(default = "default_cq_max_calls")]
    pub cq_max_calls: Option<u32>,
    /// How long to wait after a CQ run hits [`Self::cq_max_calls`] before calling again,
    /// in seconds. 180 (three minutes) by default; `Some(0)` or `None` means do not resume —
    /// the run simply stops, which is what happened before this setting existed.
    ///
    /// The pause is a TRANSMIT pause only. The sequencer stays in `CallingCq` and keeps
    /// listening, so a station that answers during it is worked normally — a pause that made
    /// the operator deaf would defeat the point of running CQ at all.
    #[serde(default = "default_cq_pause_secs")]
    pub cq_pause_secs: Option<u32>,
    /// Stop calling a station that ANSWERED you and then went silent, after this many
    /// unanswered overs of the exchange (AwaitRoger/AwaitRr73) — the club station that
    /// works three people at once and drops you mid-contact. `Some(8)` by default
    /// (operator preference); `None` = stock WSJT-X (repeat until answered, only the Tx
    /// watchdog stops it). Distinct from `cq_max_calls`, which governs a CQ run.
    ///
    /// ⚠️ IT DOES NOT APPLY WHILE YOU ARE CALLING SOMEBODY WHO HAS NOT COME BACK
    /// (operator ruling 2026-08-23: "make it not apply to a station I picked
    /// deliberately"). It used to cover `AwaitReport` as well, which per
    /// `Station::start` is only ever reached BEFORE the DX has addressed you — so eight
    /// calls into a DXpedition pileup and Nexus went quiet, which is the whole of DX
    /// chasing governed by a setting written for the opposite case. The Tx watchdog is
    /// what bounds a call nobody answers, exactly as upstream.
    #[serde(default = "default_directed_max_calls")]
    pub directed_max_calls: Option<u32>,
    /// Tempo chat: max transmit cycles per directed message before it goes terminal
    /// "no-ack" (bounded ARQ — the fix for "it keeps sending and sending"). Applies on
    /// the chat tiers only (never FT8/FT4). `None` = the built-in default (3 on
    /// TempoFast; TempoDeep uses a higher plain-repeat budget since it has no HARQ).
    #[serde(default)]
    pub chat_max_cycles: Option<u32>,
    /// Tempo chat: treat a COMPLETED inbound directed message from the peer (heard after
    /// our message last went out) as an implicit "they can hear me" — stop the resend
    /// schedule and show "confirmed". The id-bearing RR73 remains the only source of
    /// "Delivered ✓". Default ON (also works against non-Nexus peers that never ACK).
    #[serde(default = "default_true")]
    pub chat_implicit_ack: bool,
    /// Auto-CQ run resilience: if a caller answers but then goes silent mid-QSO,
    /// abandon them and resume calling CQ after this many unanswered overs of the
    /// same in-QSO step (so a dead caller can't stall the run). `None` = the built-in
    /// default (3); `Some(0)` disables auto-abandon (stock: wait for the operator).
    #[serde(default)]
    pub cq_stall_overs: Option<u32>,
    /// WSJT-X Settings ▸ Behavior: "Disable Tx after sending 73" (stock default
    /// ON). After OUR final 73 of an S&P contact goes out, Enable-Tx drops —
    /// the next station is a deliberate arm. A CQ run is unaffected (it returns
    /// to CQ, stock Run behavior).
    #[serde(default = "default_on")]
    pub disable_tx_after_73: bool,
    /// Play a short audio cue when the dial crosses your TX privileges — a rising
    /// "ding" back in band, a falling "dong" past an edge. On by default (a safety
    /// awareness cue; it only sounds when you actually cross your license edge).
    #[serde(default = "default_on")]
    pub band_edge_tones: bool,
    /// WSJT-X "CW ID after 73": key MYCALL in CW once the final 73/RR73 over
    /// has finished transmitting (stock default off). Keys through the normal
    /// CW path (PTT + tone), not appended inside the FT8 waveform.
    #[serde(default)]
    pub cw_id_after_73: bool,
    /// WSJT-X: "Clear DX call and grid after logging" (stock default off).
    /// Consumed by the UI's DX-target fields.
    #[serde(default)]
    pub clear_dx_after_log: bool,
    /// WSJT-X: "Double-click on call sets Tx enable" (stock default ON). Off =
    /// a double-click sets everything up but the operator arms TX themselves.
    #[serde(default = "default_on")]
    pub double_click_sets_tx: bool,
    /// Tune carrier auto-release (seconds) — WSJT-X Settings ▸ General "Tune
    /// after t s". Default matches the loop's long-standing 12 s safety cap.
    #[serde(default = "default_tune_timeout")]
    pub tune_timeout_secs: u32,
    /// RF power for the TUNE carrier, percent — the level a tune-up keys at, whatever the
    /// operating slider says.
    ///
    /// `None` = LEAVE THE RIG'S POWER ALONE, which is today's behaviour to the byte: a tune
    /// keys at whatever the cockpit slider is on. Percent (u8) with a watts hint from
    /// [`Settings::station_power_w`], following [`Settings::sstv_tx_power_pct`] exactly — the
    /// control it seeds is a percent slider.
    ///
    /// ⚠️ SAFE-DIRECTION ONLY: the loop applies it as the LOWER of this and the level it has
    /// already commanded, so a tune power can turn the rig DOWN for the tune-up and can never
    /// turn it up past the operator's own setting or past the per-mode duty-cycle ceiling those
    /// levels are already clamped to ([`Settings::rf_power_ceiling`]). A tune carrier is 100%
    /// duty into a load or a mismatched antenna, so the only direction worth allowing is down.
    #[serde(default)]
    pub tune_power_pct: Option<u8>,
    /// WSJT-X Split Operation (Settings ▸ Radio): keep the TRANSMITTED audio in
    /// 1500–2000 Hz (harmonics land outside the TX filter) by shifting the TX
    /// dial in 500 Hz steps. `None` = stock default (transmit at the raw audio
    /// offset); `Rig` = shifted dial on VFO B (rig split); `FakeIt` = retune the
    /// single VFO for the over and restore after (works on any CAT rig).
    #[serde(default)]
    pub split_mode: SplitMode,
    /// FT8/FT4 decode depth (WSJT-X Fast/Normal/Deep = 1/2/3). Deep is the
    /// right default on modern hardware; Fast trades sensitivity for CPU.
    #[serde(default = "default_decode_depth")]
    pub decode_depth: u8,
    /// Decoder passband low edge (Hz) — WSJT-X "F Low". Signals below this are
    /// not searched. 200 = the modem floor. The explicit `rename` matches the UI's
    /// `decodeFLowHz` key exactly; the struct's `camelCase` rule would emit
    /// `decodeFlowHz` (lowercase L) and the setting would silently never round-trip.
    #[serde(default = "default_decode_flow", rename = "decodeFLowHz")]
    pub decode_flow_hz: u32,
    /// Decoder passband high edge (Hz) — WSJT-X "F High". Default 2900; raise it up to
    /// 4000 to decode stations calling above ~2.9 kHz (common on crowded FT8 bands).
    /// `rename` matches the UI's `decodeFHighHz` key — see `decode_flow_hz` above.
    #[serde(default = "default_decode_fhigh", rename = "decodeFHighHz")]
    pub decode_fhigh_hz: u32,
    /// WSJT-X "Enable AP" (Decode menu): a-priori decoding. ON is stock and the
    /// default; off = the decoder tries no hypothesis-assisted passes (and no a7
    /// cross-cycle replay). FT8 only — the vendored FT4 decoder has no AP
    /// on/off flag (its AP is part of Normal/Deep depth), which the UI states.
    /// Key emits as `apDecode` (plain camelCase, no rename trap).
    #[serde(default = "default_on")]
    pub ap_decode: bool,
    /// Restrict AP to the CQ hypothesis only (the decoders' `lapcqonly`;
    /// FT8 + FT4). WSJT-X flips this automatically after >5 idle minutes as a
    /// stale-context guard; Nexus exposes it as an explicit expert choice. OFF
    /// (all AP hypotheses) is stock and the default. Emits as `apCqOnly`.
    #[serde(default)]
    pub ap_cq_only: bool,
    /// Single decode: narrow the FT8/FT4 search band to the operator's RX
    /// offset ± 25 Hz — the same "decode this one station" window WSJT-X uses
    /// for its double-click redecode (decoder.f90 nagainfil). Applied host-side
    /// in `build_decode_job` (never touches the modem), and ONLY at the FT8/FT4
    /// tiers: 50 Hz is narrower than one signal in the other native tiers
    /// (JT65 ~178 Hz, Q65's wider submodes, MSK144's whole passband), where it
    /// would decode nothing rather than isolate anything. Applies only while
    /// the RX offset sits inside the decode passband, else the full passband is
    /// searched. OFF (full passband) is stock and the default. NOTE: stock
    /// WSJT-X's own "Single decode" checkbox is inert for FT8/FT4 (verified in
    /// 3.0.2 — decoder.f90 only reads the bit for JT65/Q65/FST4), so this is a
    /// deliberate Nexus improvement, not parity. Emits as `singleDecode`.
    #[serde(default)]
    pub single_decode: bool,
    /// WSJT-X "Special operating activity": Hound = work a DXpedition Fox
    /// (calls ≥ 1000 Hz, auto-move to the Fox's frequency for the R+report,
    /// Fox multi-payload messages split at ingest). Fox role: not yet.
    #[serde(default)]
    pub special_op: SpecialOp,
    /// Operator overrides of the working-frequency table (WSJT-X Settings ▸
    /// Frequencies). Empty = the stock WSJT-X table built into the band plan.
    /// An entry replaces the dial of the matching (band, mode) row; an entry
    /// for a band the built-in table lacks is appended.
    #[serde(default)]
    pub working_frequencies: Vec<WorkingFreq>,

    // --- coordinated QSY ("move together") — a SEPARATE, opt-in function ---
    /// Master opt-in for coordinated QSY. **Off by default** and fully isolated:
    /// while false, the engine never emits or acts on a QSY directive and the
    /// primary Chat/QSO/Field-Day modes behave exactly as without the feature.
    /// Announced-in-the-clear only — NOT encryption / NOT a secret hop.
    pub qsy_enabled: bool,
    /// The set of band-plan channel tokens (e.g. "20m", "40m", "70cm") the
    /// initiator round-robins through when hopping. Empty = nowhere to move.
    pub qsy_set: Vec<String>,
    /// Announce cadence: the initiator hops every this-many of its TX overs.
    /// Conservative by default so it reads as a normal QSY, not a hopping pattern.
    pub qsy_cadence: u64,

    // --- ISS SSTV auto-arm — a SEPARATE, opt-in convenience ---
    /// Opt-in: at the start of an ISS pass, auto-tune 145.800 FM and arm the SSTV
    /// decoder; at LOS, disarm and restore the operator's saved dial. **Off by
    /// default** and fully gated — every rig-touching action lives behind this
    /// flag, and it never retunes/disarms against the operator (the dial is
    /// restored only while still on 145.800 FM). ISS SSTV is an event-only
    /// downlink, so it fires on every pass; the arm/restore loop lives in the
    /// frontend (issAutoArm.ts, mirroring the sat alarm) since SSTV RX only runs
    /// while the app is open.
    pub iss_sstv_auto_arm: bool,

    // --- SSTV (Settings ▸ Digital ▸ SSTV) ---
    /// Whether opening the SSTV view starts the receiver.
    ///
    /// TRUE is today's behaviour and must stay the default: arming used to be manual,
    /// session-only and off, so the ordinary way to use SSTV — open the view, tune
    /// 14.230, wait — decoded nothing. [`Engine::sstv_auto_arm`] is the fix, and this
    /// is its opt-out for the operator who runs SSTV as a monitor on a shared rig and
    /// does not want an armed decoder every time they glance at the screen. The gate
    /// lives in the engine, not the view, so a remount cannot lose it. The ISS pass
    /// arm is unaffected — that path calls `set_sstv_armed`, an explicit act.
    #[serde(default = "default_true")]
    pub sstv_rx_auto_arm: bool,
    /// The transmit mode the SSTV screen starts on: a `SSTV_TX_MODES` slug, or "auto"
    /// for the band-aware pick (HF → Scottie 1, the NA calling-frequency convention;
    /// 2 m → PD-120, which is what ARISS transmits).
    ///
    /// A `String`, not a Rust enum mirroring 15 slugs, because the backend never reads
    /// it: the transmit path still resolves and validates the slug at `sstv_send` /
    /// `for_mode`, which refuses any raster mismatch. An unknown or retired slug falls
    /// back to the band-aware pick in the view, so a hand-edited or downgraded settings
    /// file can never strand the picker. Named `sstv_default_tx_mode` and not
    /// `sstv_tx_mode` on purpose — the latter is live Engine state meaning "the mode
    /// currently going out", and the two would differ by one token at every read site.
    #[serde(default = "default_sstv_default_tx_mode")]
    pub sstv_default_tx_mode: String,
    /// SSTV drive, percent — the level the picture is sent at, and the position the
    /// screen's power slider starts on.
    ///
    /// None = LEAVE THE RIG'S POWER ALONE, which is today's behaviour to the byte. A
    /// default of 100 would take an operator whose rig sits at 20 W for a reason to
    /// full power on their first Send after upgrading. Percent (u8) rather than the
    /// 0.0–1.0 fraction the `max_power_*` caps use, because the control it seeds is a
    /// percent slider — `beacon_tx_percent` is the precedent. It only ever reaches a
    /// radio through `set_rf_power`, which clamps to [`Settings::rf_power_ceiling`], so
    /// it can lower power past the operator's cap but never raise it past one.
    #[serde(default)]
    pub sstv_tx_power_pct: Option<u8>,
    /// Whether opening the PSK view starts the receiver.
    ///
    /// The SSTV/APRS auto-arm doctrine, applied to PSK31 from day one (operator
    /// ruling 2026-08-17): there is exactly one reason to be on a receive screen
    /// with a receiver, so entering the view arms it — and this is the opt-out
    /// for the operator monitoring on a shared rig. The gate lives in
    /// [`crate::engine::Engine::psk_auto_arm`], not the view, so a remount
    /// cannot lose it; stopping the receiver by hand is separately remembered
    /// for the session (the decline memory), exactly as SSTV/APRS do.
    #[serde(default = "default_true")]
    pub psk_rx_auto_arm: bool,
    /// Whether opening the RTTY view starts the receiver — [`Settings::psk_rx_auto_arm`]'s
    /// twin, same doctrine, same default, and the same interior-acronym trap on the wire key
    /// (`rttyRXAutoArm` compiles clean on both sides and never matches).
    #[serde(default = "default_true")]
    pub rtty_rx_auto_arm: bool,

    // --- alerts / comforts ---
    /// Alert (sound + visual) when your callsign is decoded (someone calling you).
    pub alert_my_call: bool,
    /// Alert on a decoded CQ.
    pub alert_cq: bool,
    /// Alert when a new (not previously heard) station is decoded.
    pub alert_new: bool,
    /// Put the exchanged dB reports into the logged QSO's COMMENT field, WSJT-X's
    /// "dB reports to comments" (`dBtoComments`, default false there — logqso.cpp:143
    /// builds `"<mode>  Sent: <rpt>  Rcvd: <rpt>"`, two spaces, parts omitted when
    /// absent, and this matches it byte for byte). Opt-in, exactly as WSJT-X ships it.
    #[serde(default)]
    pub log_reports_to_comments: bool,
    /// Show the "Confirm" tier — worked-but-unconfirmed award slots (LoTW confirmation
    /// opportunities) — on the Needed board and as decode/roster chips. Default ON:
    /// the tier ships lit and this is the opt-OUT for operators who chase contacts,
    /// not confirmations (operator ask, 2026-09-01). `default = "default_on"`, not a
    /// bare default: a settings.json from an older build must read TRUE, or the
    /// upgrade would silently turn the tier off for everyone.
    #[serde(default = "default_on")]
    pub alert_confirm_tier: bool,
    /// Band scope for new-DXCC alerts: "off" | "hf" | "vhf" | "all". `alert_new`
    /// stays the master gate (backward compat); these scopes refine it per type.
    #[serde(default = "default_alert_scope_all")]
    pub alert_dxcc_bands: String,
    /// Band scope for plain new-GRID alerts. Default "vhf" (6 m and up): grid
    /// chasing is a VHF pursuit (VUCC/FFMA start at 6 m) — on HF nearly every
    /// decode is an unworked grid, so the alert is noise (operator report).
    #[serde(default = "default_alert_grid_bands")]
    pub alert_grid_bands: String,
    /// Fold MODE into the worked-before (B4) and Dupe checks — WSJT-X's "highlight by mode"
    /// (`HighlightByMode`, default off there too). Off: working a station on 40m marks them
    /// B4-on-band for 40m in every mode, which is how most operators and WSJT-X count it.
    /// On: 40m FT8 and 40m phone are separate contacts for B4/Dupe purposes (operator-relayed
    /// ask, 2026-08-16 — a 'Dupe 40m' shown for a station worked on FT8 while running phone).
    #[serde(default)]
    pub b4_match_mode: bool,
    /// Band scope for the rare/ultra 💎 grid alerts — separate from plain grids
    /// so an operator CAN keep the open-water gems on HF by widening it.
    ///
    /// ⚠️ Default "vhf", matching the plain grid scope (operator ruling, 2026-08-15:
    /// "remove the grid alerts for HF bands by default"). It shipped as "all", which made
    /// the rare tier the one grid alert still firing on HF — and on HF nearly every decode
    /// is an unworked grid, so even the rare subset reads as chatter. Grid awards are
    /// VHF-centric; an HF grid-chaser opts in from Settings ▸ Spots & Alerts.
    #[serde(default = "default_alert_grid_bands")]
    pub alert_rare_grid_bands: String,
    /// Mouse-wheel tuning sensitivity multiplier (1.0 = stock). <1 = less sensitive
    /// (needs more scroll per step — for over-energetic / high-res "free-spin" mice),
    /// >1 = more sensitive. Applied to every wheel-tune surface (dial readout + scopes).
    #[serde(default = "default_wheel_tune_sensitivity")]
    pub wheel_tune_sensitivity: f32,
    /// Screen-reader speech for arriving decodes: "off" | "needed" (only the
    /// alert-worthy: calling-you / new one / watchlist) | "all" (adds every CQ).
    /// Inaudible without a screen reader running, so "needed" is a safe default.
    #[serde(default = "default_announce_verbosity")]
    pub announce_verbosity: String,
    /// Earcon on TX key/unkey (eyes-free operating). Off by default — FT8 keys
    /// every cycle and sighted operators see the TX pill.
    #[serde(default)]
    pub sound_tx_state: bool,
    /// Soft tick when a decode batch lands (the band's rhythm, eyes-free). Off
    /// by default.
    #[serde(default)]
    pub sound_decode_tick: bool,

    // --- Auto-CQ caller selection (W1.4) ---
    /// When running CQ and several stations answer, which one to work first:
    /// `"first"` = stock next-caller (WSJT-X behavior), `"strongest"` = highest
    /// SNR, `"farthest"` = greatest distance from my grid, `"cq_first"` = prefer
    /// a station that itself was calling CQ (a fresh contact over a tail-ender).
    #[serde(default = "default_best_caller")]
    pub best_caller: String,
    /// When picking a caller, ignore any answering station weaker than this SNR
    /// (dB). `None` = no floor. Guards against chasing an uncopyable caller.
    #[serde(default)]
    pub best_caller_min_snr: Option<i32>,
    /// Blocked callsigns (field ask): stations the AUTO-RESPONDER must never answer when
    /// they reply to my CQ, and the display's hide/dim list. Base-call matched
    /// (`same_call`), stored normalized (trim/uppercase, deduped). Empty = feature off.
    /// The one deliberate divergence from stock WSJT-X caller selection, alongside the
    /// W1.4 strategies — a listed caller is passed over for the next eligible one, and
    /// with every caller listed the run keeps calling CQ.
    #[serde(default)]
    pub blocked_calls: Vec<String>,

    // --- Wanted watch list / alert filters (W1.5) ---
    /// Operator "wanted" watch list: entries raise a LOUD need-alert when heard.
    /// Each entry is an exact call or a trailing-`*` wildcard prefix
    /// (e.g. `"VP8*"`, `"3Y0J"`, `"FT*"`). Empty = feature off.
    #[serde(default)]
    pub wanted_calls: Vec<String>,

    /// Pounce — how rare a spot must be before Nexus interrupts you the moment it appears.
    /// Defaults to all-time-new DXCC only: at a serious total an ATNO is genuinely rare, so the
    /// alert stays trustworthy. An alert that cries wolf gets ignored and is then worthless.
    #[serde(default)]
    pub pounce_threshold: PounceThreshold,

    // --- confirmations (LoTW) ---
    /// LoTW account **username** (usually but not always the callsign). The
    /// password is NOT stored here — it lives in the OS keychain (set via the
    /// `set_lotw_password` command). Empty = LoTW sync not configured.
    pub lotw_username: String,
    /// Incremental-sync high-water mark: the `APP_LoTW_LASTQSL` timestamp from the
    /// last successful download, passed back as `qso_qslsince`. Empty = full pull.
    /// Reset to empty when `lotw_username` changes (the cursor is query-bound).
    pub lotw_last_qsl: String,
    /// LoTW **upload** Station Location name (the `-l` arg passed to TQSL). Non-
    /// secret; TQSL owns the certificate. Empty = upload not configured.
    pub lotw_station_location: String,
    /// Sign LoTW uploads from the location EMBEDDED in the ADIF (STATION_CALLSIGN /
    /// MY_GRIDSQUARE) instead of a named TQSL Station Location — for travelers who set
    /// TQSL to "use the location in the ADIF file" and never create station locations.
    /// When true, the `-l` arg is omitted and `lotw_station_location` isn't required.
    #[serde(default)]
    pub lotw_use_adif_location: bool,
    /// Optional path to the `tqsl` binary (overrides auto-detect). Empty = search
    /// the OS default locations + PATH.
    pub tqsl_path: String,
    /// Hand the un-uploaded batch to TQSL on a timer, instead of waiting for the
    /// Logbook's "Upload to LoTW (N)" button.
    ///
    /// Deliberately NOT the same shape as the four per-QSO upload toggles beside it
    /// (`qrz_logbook_upload`/`clublog_upload`/`eqsl_upload`/`hrdlog_upload`): those push
    /// ONE record over HTTP as it is logged, this signs a whole batch through an external
    /// GUI-linked binary and gets back a single exit code for all of it. Off by default,
    /// harder than the siblings: it spawns a process and pushes to ARRL unattended.
    ///
    /// Refused outright while [`Self::lotw_use_adif_location`] is set — see
    /// [`crate::engine::lotw_auto_upload_due`], which is the gate that matters.
    #[serde(default)]
    pub lotw_auto_upload: bool,
    /// Hours between automatic LoTW batches. Six, NOT the QRZ one: each run spawns TQSL
    /// and pushes to ARRL's server, and partner confirmations take days to appear — an
    /// hourly cadence buys the operator nothing and multiplies load across every install.
    /// Settings-file only; there is no UI control, exactly like `qrz_sync_hours`.
    /// Clamped ≥ 1 at use.
    #[serde(default = "default_lotw_auto_upload_hours")]
    pub lotw_auto_upload_hours: u32,
    /// Unix seconds of the last automatic batch ATTEMPT — a rate limiter, not a delta
    /// high-water (see the cursor policy in the auto-upload worker). Persisted so the
    /// interval survives a restart; without it an operator who relaunches often would
    /// spawn TQSL every launch. 0 = never run, so the first tick after enabling is due.
    #[serde(default)]
    pub lotw_last_auto_upload_unix: u64,
    /// eQSL account **username** (callsign or account login). The password lives in
    /// the OS keychain (set via `set_eqsl_password`), never here. Empty = not set.
    pub eqsl_username: String,
    /// eQSL incremental-sync cursor: a `YYYYMMDDHHMM` timestamp (this sync's start,
    /// rolled back by a safety margin) sent as `RcvdSince`. Empty = full pull.
    /// Reset to empty when `eqsl_username` changes (the cursor is account-bound).
    pub eqsl_last_sync: String,
    /// QRZ.com account username for callsign lookup. The password lives in the OS
    /// keychain (set via `set_qrz_password`), never here; the session key is cached
    /// in memory only. Empty = QRZ lookup not configured.
    pub qrz_username: String,
    /// HamQTH.com account username — the FREE fallback for callsign lookup, used when
    /// QRZ isn't configured or has no match. The password lives in the OS keychain
    /// (set via `set_hamqth_password`), never here; the session id is cached in memory
    /// only. Empty = HamQTH lookup not configured.
    #[serde(default)]
    pub hamqth_username: String,
    /// Auto-upload each logged QSO to the QRZ.com logbook (push). Needs the QRZ
    /// Logbook **API key** in the keychain (distinct from the lookup password).
    /// Off by default.
    pub qrz_logbook_upload: bool,
    /// Pull confirmations DOWN from the QRZ logbook automatically, on a timer.
    ///
    /// The manual sync has always existed; this is the "as people confirm on QRZ,
    /// they should flow to Nexus" half. Off by default — it is repeated traffic to
    /// someone else's server, so the operator opts in.
    ///
    /// Uses a MODSINCE delta once seeded, so a run costs only what changed. QRZ
    /// confirmations set `confirmed` but never `award_confirmed`; see `QslRcvd::qrz`.
    #[serde(default)]
    pub qrz_auto_sync: bool,
    /// Hours between automatic QRZ pulls. Confirmations trickle in over days, so the
    /// 1 h default is already far faster than the data changes. Clamped ≥ 1 at use.
    #[serde(default = "default_qrz_sync_hours")]
    pub qrz_sync_hours: u32,
    /// Unix seconds of the last SUCCESSFUL automatic pull — the delta high-water.
    /// 0 = never, which makes the next run a full seeding fetch.
    #[serde(default)]
    pub qrz_last_sync_unix: u64,
    /// ClubLog account email (NOT a callsign). The app-password lives in the OS
    /// keychain; the api key + email are non-secret and live here.
    pub clublog_email: String,
    /// ClubLog logbook callsign to upload into (empty → use `mycall`).
    pub clublog_callsign: String,
    /// ClubLog developer/app API key. Non-secret per ClubLog, but NEVER committed
    /// (GPLv3 public repo → auto-revoked); empty → fall back to a build-time
    /// `option_env!("CLUBLOG_API_KEY")` default.
    pub clublog_api_key: String,
    /// Auto-upload each logged QSO to ClubLog (realtime push). Off by default.
    pub clublog_upload: bool,
    /// Auto-upload each logged QSO to eQSL.cc (ImportADIF). Off by default. The
    /// eQSL username is `eqsl_username`; the password lives in the OS keychain.
    pub eqsl_upload: bool,
    /// Auto-upload each logged QSO to HRDLog.net (the online logging/awards site,
    /// NOT the HRD Logbook UDP push above). Off by default. The station callsign is
    /// `mycall`; the upload code lives in the OS keychain. HRDLog.net is not an ARRL
    /// confirmation source — an upload here never earns DXCC/WAS credit.
    pub hrdlog_upload: bool,
    /// Auto-push each logged QSO to World Radio League (`POST /v1/contacts`).
    /// Flipped on by saving a WRL API key, off by clearing it — the credential IS
    /// the opt-in, like every other connector.
    #[serde(default)]
    pub wrl_upload: bool,
    /// The WRL logbook contacts go to. Resolved ONCE at key-save time (`GET /v1/me`,
    /// falling back to the account's single logbook) and stored here — an id, not a
    /// secret, so Settings not the keychain. Empty = omit `logbookId` and let the
    /// account's default take it.
    #[serde(default)]
    pub wrl_logbook_id: String,

    /// Auto-forward EVERY logged QSO (not just Field Day) to N3FJP over the same
    /// `n3fjp_host`/`n3fjp_port` — N3FJP ACLog / everyday general logging. ADDDIRECT with
    /// EXCLUDEDUPES, so it can't double-log a contact the Field-Day path also pushed. Off by
    /// default; empty host = off regardless.
    #[serde(default)]
    pub n3fjp_upload: bool,

    /// Cloudlog / Wavelog self-hosted logbook base URL (e.g. `https://log.example.com`). Empty = off.
    #[serde(default)]
    pub cloudlog_url: String,
    /// Cloudlog/Wavelog station-profile id to log each QSO against.
    #[serde(default)]
    pub cloudlog_station_id: String,
    /// Cloudlog/Wavelog instance API key. LEGACY-ONLY at rest: the key now lives in
    /// the OS keychain (see src-tauri `set_cloudlog_key`). `skip_serializing` keeps
    /// it OUT of settings.json on every save; it still DESERIALIZES an older file's
    /// plaintext key so the shell can migrate it into the keychain once, then clear
    /// it. Not sent to the frontend — the UI field is write-only.
    #[serde(default, skip_serializing)]
    pub cloudlog_key: String,
    /// Auto-forward each logged QSO to the Cloudlog/Wavelog instance above. Off by default.
    #[serde(default)]
    pub cloudlog_upload: bool,

    /// Watch near-region spots (not just your own paths) so opening detection can
    /// flag "a band is open around you" before you've worked anyone. On by default;
    /// the operator opt-out for the near-region MQTT feed (Phase 2).
    pub opening_regional: bool,

    /// Editable quick-reply macros per mode (the Composer chips).
    pub macros: Macros,

    /// Phone voice-keyer message slots (F-key → recorded 12 kHz mono WAV).
    /// Defaulted to six labelled-but-empty casual slots.
    #[serde(default = "default_voice_messages")]
    pub voice_messages: Vec<VoiceMessage>,
}

/// One phone voice-keyer slot: an F-key-numbered label bound to a recorded WAV. `file`
/// is empty until the operator records or imports a message into the slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceMessage {
    pub slot: u8,
    pub label: String,
    pub file: String,
}

/// The default six labelled (but empty) voice-keyer slots — a casual phone set (no
/// contest exchange). The operator records or imports the audio per slot.
/// WSJT-X "Special operating activity" (the DXpedition modes we support).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpecialOp {
    #[default]
    None,
    Hound,
    /// RETIRED (operator decision 2026-06-10): native SuperFox decode is off
    /// the table — the QPC code-table file is licensed "only for use with
    /// WSJT-X" and won't be vendored. The variant stays so a settings file
    /// that saved it still loads; it behaves exactly as [`SpecialOp::Hound`].
    #[serde(rename = "superhound")]
    SuperHound,
}

/// WSJT-X Split Operation choices. Serialized lowercase for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitMode {
    #[default]
    None,
    Rig,
    #[serde(rename = "fakeit")]
    FakeIt,
}

/// One operator-edited working-frequency row (band + mode + dial MHz).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingFreq {
    pub band: String,
    /// "FT8" | "FT4" (matched case-insensitively against the tier).
    pub mode: String,
    pub mhz: f64,
}

fn default_on() -> bool {
    true
}

fn default_tune_timeout() -> u32 {
    12
}

/// The shared default for the two #145 CAT declarations: "work it out as before".
fn default_cat_auto() -> String {
    "auto".to_string()
}

fn default_directed_max_calls() -> Option<u32> {
    Some(8)
}

/// Eight unanswered CQs before a breather (operator ruling). Enough to be heard through a
/// fade, short enough not to hold a frequency for a quarter of an hour.
/// A quarter of the rig's maximum — what most manuals say for AM, because the carrier is always
/// there and the peaks are what flat-top. A starting point an operator can raise.
fn default_max_power_am() -> Option<f32> {
    Some(0.25)
}

fn default_cq_max_calls() -> Option<u32> {
    Some(8)
}

/// Three minutes off the air after an unanswered run, then call again.
fn default_cq_pause_secs() -> Option<u32> {
    Some(180)
}

/// A G-5500's own resolution is about this; below it a command is noise rather
/// than motion, and the relays chatter for the whole pass.
fn default_rot_tol_deg() -> f64 {
    2.0
}

/// Doppler below this is not worth a CAT write: ~10 Hz is inaudible on SSB at
/// 435 MHz and well inside the tuning step most radios use.
fn default_sat_min_shift_hz() -> u32 {
    20
}

/// One correction per second is what a satellite pass actually needs — the
/// shift changes by tens of hertz per second at LEO, and a serial CI-V bus has
/// other work to do (meters, PTT, mode).
fn default_sat_update_ms() -> u32 {
    1_000
}

/// The rate a rotator starts on before a model is picked — nothing more.
///
/// ⚠️ It was read as more than that, and that is the 2026-08-18 field report ("one rotator model
/// does not work"). This one number was the ONLY rate any rotator ever got: the picker wrote
/// `rotator_model` and never touched the baud, the UI tooltip told every owner of every model
/// that 9600 was the GS-232 default, and `rotctld_args` forces `-s <baud>` onto the daemon
/// whenever a port is set — which OVERRIDES the backend's own declared rate. Five of the
/// thirteen real-hardware models the picker offered declare a single rate that is not 9600
/// (SPID Rot2Prog 600, Rot1Prog 1200, Rotor-EZ / DCU-1 / RT-21 4800), so they shipped unable to
/// talk to their controller at all.
///
/// The rate now comes from the model, through `ROT_FIXED_BAUD` / `baudForRotator` in the UI,
/// derived from the bundled Hamlib's own caps by `scripts/gen-hamlib-rotator-speeds.mjs` — the
/// same "only `min == max` is a fact" rule the rig picker took four rounds to learn. This value
/// survives only as the pre-model starting point and as the fallback for a backend that
/// declares a RANGE, where there is no fact to impose and the operator's own choice stands.
fn default_rotator_baud() -> u32 {
    9600
}

fn default_save_wav() -> String {
    "none".to_string()
}

fn default_best_caller() -> String {
    "first".to_string()
}

fn default_cw_key_line() -> String {
    "dtr".to_string()
}

/// Default CW sending speed (WPM) — matches the engine's historical seed.
fn default_cw_wpm() -> u32 {
    25
}

fn default_units() -> String {
    "auto".to_string()
}

fn default_rtty_backend() -> String {
    "afsk".to_string()
}

fn default_rtty_fsk_line() -> String {
    "dtr".to_string()
}

fn default_rtty_baud() -> f64 {
    45.45
}

fn default_rtty_shift_hz() -> u32 {
    170
}

/// "auto" = the SSTV screen's band-aware pick, which is what it does today.
fn default_sstv_default_tx_mode() -> String {
    "auto".to_string()
}

fn default_monitor_level() -> f32 {
    0.5
}

fn default_lotw_max_age_days() -> u32 {
    365
}

fn default_prop_engine() -> String {
    "heuristic".to_string()
}

fn default_qrz_sync_hours() -> u32 {
    1
}

fn default_lotw_auto_upload_hours() -> u32 {
    6
}

fn default_fd_host_port() -> u16 {
    tempo_net::fdsync::DEFAULT_TCP_PORT
}

fn default_fd_scoreboard_port() -> u16 {
    7373
}

/// One past the scoreboard, so a Field Day host can serve both boards at once.
fn default_connect_web_port() -> u16 {
    7374
}

fn default_fd_power() -> u32 {
    2
}

fn default_dxkeeper_base_port() -> u16 {
    // DXLab's documented default Base Port. Mirrors tempo_net::dxkeeper::DEFAULT_BASE_PORT,
    // duplicated because tempo-app does not depend on tempo-net (the push lives in the
    // src-tauri orchestration layer, which depends on both).
    52000
}

fn default_n3fjp_port() -> u16 {
    1100
}

fn default_decode_depth() -> u8 {
    3
}

fn default_decode_flow() -> u32 {
    200
}

fn default_decode_fhigh() -> u32 {
    2900
}

fn default_alert_scope_all() -> String {
    "all".to_string()
}

fn default_alert_grid_bands() -> String {
    "vhf".to_string()
}

fn default_wheel_tune_sensitivity() -> f32 {
    1.0
}

fn default_announce_verbosity() -> String {
    "needed".to_string()
}

pub fn default_voice_messages() -> Vec<VoiceMessage> {
    [
        (1, "CQ"),
        (2, "My Call"),
        (3, "Report"),
        (4, "QRZ?"),
        (5, "73"),
        (6, "Again"),
    ]
    .iter()
    .map(|(slot, label)| VoiceMessage {
        slot: *slot,
        label: label.to_string(),
        file: String::new(),
    })
    .collect()
}

/// Editable quick-reply macro sets per mode (shown as Composer chips). Field Day
/// uses the live class+section exchange, so it isn't user-editable here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Macros {
    pub chat: Vec<String>,
    pub qso: Vec<String>,
    pub band: Vec<String>,
    /// LEGACY single CW F-key macro list — kept only for one-way migration into
    /// `cw_profiles` (see [`Macros::migrate_cw_profiles`]). New reads/writes go through
    /// the named profiles; `load` empties this field once it has seeded the "Default"
    /// profile from it.
    #[serde(default)]
    pub cw: Vec<CwMacroDef>,
    /// Named CW F-key macro sets — one per operator/purpose, selectable in the cockpit
    /// (Field Day ops rotate profiles as operators change). Seeded on load with a single
    /// "Default" profile migrated from the legacy `cw` field. Each entry's macros carry
    /// the same {MYCALL}/{RST}/{NAME}/! tokens the engine expands; an EMPTY macro list
    /// means the cockpit's built-in defaults, so upgrades keep improving them.
    #[serde(default)]
    pub cw_profiles: Vec<CwMacroProfile>,
    /// Index into `cw_profiles` of the active set. Clamped in range on load.
    #[serde(default)]
    pub active_cw_profile: usize,
}

/// One customizable CW F-key macro.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CwMacroDef {
    pub key: String,
    pub label: String,
    pub text: String,
}

/// A named set of CW F-key macros (one operator / one purpose).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CwMacroProfile {
    pub name: String,
    pub macros: Vec<CwMacroDef>,
}

impl Default for Macros {
    fn default() -> Self {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect();
        Self {
            chat: v(&["73", "QSL", "Name?", "QTH?"]),
            qso: v(&["R-09", "RRR", "RR73", "73"]),
            // Genuine free-text band chatter only — a CQ goes through the structured
            // Call-CQ button (a "CQ CQ" free-text chip went out as a chunked, gridless
            // "DE <CALL> A12CQ CQ", never a real CQ).
            band: v(&["QRZ?", "Net check-in", "73 to all"]),
            cw: Vec::new(),
            cw_profiles: Vec::new(),
            active_cw_profile: 0,
        }
    }
}

impl Macros {
    /// The active CW profile's macros (the list the cockpit renders as F-keys).
    /// Bounds-checked: an out-of-range `active_cw_profile` or an unmigrated (empty)
    /// `cw_profiles` yields an empty slice, which the cockpit reads as "use built-in
    /// defaults".
    pub fn active_cw_macros(&self) -> &[CwMacroDef] {
        self.cw_profiles
            .get(self.active_cw_profile)
            .map(|p| p.macros.as_slice())
            .unwrap_or(&[])
    }

    /// One-way, idempotent migration of the legacy single `cw` list into the named
    /// `cw_profiles`. If no profiles exist yet, seed exactly one named "Default" from the
    /// legacy `cw` (or empty when `cw` is empty), select it, and clear `cw` so it can't
    /// diverge. Always clamps `active_cw_profile` into range (covers a corrupt/old index).
    pub fn migrate_cw_profiles(&mut self) {
        if self.cw_profiles.is_empty() {
            self.cw_profiles = vec![CwMacroProfile {
                name: "Default".to_string(),
                macros: std::mem::take(&mut self.cw),
            }];
            self.active_cw_profile = 0;
        }
        if self.active_cw_profile >= self.cw_profiles.len() {
            self.active_cw_profile = 0;
        }
    }
}

/// The mode granularity radio ROUTING decides on — coarse enough that the operator writes five
/// rules, fine enough to split one band between two rigs.
///
/// This is a REFINEMENT of the app's existing three-way `ModeClass` (CW / Phone / Digital, in
/// `propagation::model`), not a parallel taxonomy: `Fm`+`Ssb` are both Phone and `Digital`+`Rtty`
/// are both Digital. The refinement is forced by the station it serves — the coarse classes CANNOT
/// express "2 m FT8 on the IC-9700 but 2 m FM on the FT-991A", because FT8 and APRS are both
/// Digital and FM and SSB are both Phone. Two rigs on one band is exactly the case that made
/// band-only routing insufficient.
///
/// Derived from live engine state by `Engine::route_mode`, whose inputs are the same ones
/// [`Settings::rig_mode`] commands the rig from (`operating_mode`, `aprs_fm`, `phone_mode`,
/// `dial_mhz`) — so a routing decision can never disagree with the mode the rig is about to be put
/// in. SSTV is deliberately NOT a class: it has no operating section (it rides Phone, and
/// `rig_mode_effective` only diverges while an image is actually queued), so an SSTV rule could
/// never match at QSY time — a rule the operator could set and never see fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteMode {
    /// Weak-signal digital: FT8/FT4/FT1/DX1/JT/Q65/MSK144/WSPR — `OperatingMode::Digital`.
    Digital,
    /// FM voice and FM packet: repeaters, simplex, APRS. The class the FT-991A owns on 2 m.
    Fm,
    /// SSB/AM voice (and SSTV, which rides the phone segment on the same rig).
    Ssb,
    Cw,
    Rtty,
}

impl RouteMode {
    /// Operator-facing label (the Settings rule editor's dropdown).
    pub fn label(self) -> &'static str {
        match self {
            RouteMode::Digital => "Weak-signal digital",
            RouteMode::Fm => "FM & APRS",
            RouteMode::Ssb => "SSB phone",
            RouteMode::Cw => "CW",
            RouteMode::Rtty => "RTTY",
        }
    }
}

/// The non-mode CONTEXT a [`RoutingRule`] can be designated for instead of a mode class.
/// Serialized lowercase — the token the Settings rule editor's dropdown writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteContext {
    /// Satellite-originated tunes only (a transponder pick), matched at a tier ABOVE the mode
    /// rules by [`Settings::route_radio_satellite`] and INVISIBLE to every terrestrial tune.
    /// Exists because mode class alone cannot say it: a packet bird is honestly FM-class
    /// traffic, so it follows the operator's terrestrial "FM & APRS → FT-991A" rule — correct
    /// by the rules, wrong for satellites, which belong on the sat rig (the IC-9700).
    Satellite,
}

/// One band+mode → radio routing rule. Evaluated FIRST-MATCH-WINS in `Settings::routing_rules`
/// order, so a specific rule placed above a broad one wins.
///
/// Both selectors are "empty = any", which is how a rule stays readable: `bands: []` means every
/// band, `mode: None` means every mode class. `{bands: ["2m"], mode: Some(Fm)} -> 991A` is the
/// operator's APRS/repeater rule; `{bands: [], mode: Some(Digital)} -> 9700` would be "all digital
/// on the 9700" regardless of band. A rule may instead carry a `context` DESIGNATION (Satellite),
/// which moves it out of the mode tiers entirely — see [`Settings::route_radio_satellite`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RoutingRule {
    /// Bands this rule covers, e.g. `["2m", "70cm"]`. EMPTY = any band.
    pub bands: Vec<String>,
    /// Mode class this rule covers. `None` = any mode class.
    pub mode: Option<RouteMode>,
    /// `Some(Satellite)` designates this rule for satellite work: consulted only for
    /// satellite-originated tunes, above the mode rules, and never by a terrestrial tune.
    /// `None` — every rule stored before the field existed — is a plain mode rule, unchanged.
    ///
    /// ⚠️ The reverse load has no guard: a build predating this field drops it silently (the
    /// old struct has no `deny_unknown_fields`) and reads the rule as a plain mode rule — the
    /// recommended empty-selector shape becomes an any-band, any-mode catch-all, and a re-save
    /// there erases the designation for good. Nothing written HERE can intercept an old
    /// deserializer; the changelog carries the operator caution (delete Satellite rules before
    /// rolling back).
    pub context: Option<RouteContext>,
    /// The `RadioProfile::id` to hand off to.
    pub radio: u32,
}

impl RoutingRule {
    /// Does this rule cover `(band, mode)`? Band match is case-insensitive (the app writes "2m",
    /// an imported config might carry "2M").
    pub fn matches(&self, band: &str, mode: RouteMode) -> bool {
        let band_ok =
            self.bands.is_empty() || self.bands.iter().any(|b| b.eq_ignore_ascii_case(band));
        let mode_ok = self.mode.is_none_or(|m| m == mode);
        band_ok && mode_ok
    }

    /// Does this rule cover `mode` on a dial [`crate::bandplan::band_for_dial`] cannot NAME a
    /// band for (QO-100, the microwave birds — see [`Settings::route_radio_bandless`])?
    ///
    /// Only a rule with an EMPTY band selector can answer, and it answers the same as always:
    /// its verdict never read a band in the first place, so there is nothing about it to guess
    /// at. A rule that NAMES bands cannot be shown to contain a band we cannot name, so it does
    /// not match — the opposite of [`Self::matches`], where an empty selector and an empty band
    /// string would BOTH match and quietly turn "no band" into "every band".
    pub fn matches_bandless(&self, mode: RouteMode) -> bool {
        self.bands.is_empty() && self.mode.is_none_or(|m| m == mode)
    }
}

/// One radio's complete, independently-configurable connection profile. A single-radio station has
/// exactly one (migrated from the flat `Settings` rig/audio fields); adding a 2nd radio in Settings
/// appends another. Serde-defaulted throughout so partial/older records load.
/// serde default for `icom_data_mode`: D1, which is what every Icom has and what Nexus has
/// always selected.
fn one() -> u8 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RadioProfile {
    /// Stable id, never reused — routing / active-selection / per-radio state key on it.
    pub id: u32,
    /// Operator-facing name ("FTDX10", "IC-9700"); defaults to the rig model name.
    pub name: String,
    /// Configured but not driven when false (a rig temporarily unplugged).
    pub enabled: bool,
    // --- CAT (mirror of the flat rig fields) ---
    pub ptt_method: String,
    pub rig_model: u32,
    pub rig_model_name: String,
    pub serial_port: String,
    /// DEDICATED serial port for RTS/DTR PTT keying, when the keyline is NOT on the CAT port.
    /// Empty = key on `serial_port` (the common single-cable case).
    ///
    /// PER-RADIO because it has to be: an SO2R interface (U2R, MK2R) gives EACH radio its own
    /// keying port. This lived ONLY on the flat `Settings` until 2026-07-25, so switching radios
    /// moved CAT (which is per-radio) but left PTT pointing at whatever the previous radio used —
    /// operator report: "the PTT port does not follow the selected radio. CAT works fine."
    #[serde(default)]
    pub ptt_serial_port: String,
    pub baud: u32,
    pub rig_conn: String,
    pub rig_addr: String,
    /// Which OmniRig slot this radio drives when `rig_conn == "omnirig"`: **1 = RIG 1**
    /// (default), **2 = RIG 2**. OmniRig has exactly those two, each with its own rig type
    /// and COM port, so a two-radio station can put one Nexus radio on each.
    ///
    /// PER-RADIO because it has to be — it names WHICH radio inside OmniRig this profile is.
    /// 0 (a settings file written before the field existed) reads as RIG 1, the default; see
    /// `tempo_audio::omnirig::RigSlot::from_setting`.
    #[serde(default)]
    pub omnirig_slot: u8,
    /// UNIQUE across enabled profiles (validated) — each radio's own rigctld TCP port.
    pub rigctld_port: u16,
    /// Native Icom CI-V: Nexus itself owns this radio's serial CI-V port (instead of
    /// launching rigctld) and serves the same protocol on `rigctld_port` — unlocking the
    /// rig's real spectrum-scope waveform + instant transceive dial tracking. Only honored
    /// for a scope-capable Icom on a serial connection; off (default) = classic rigctld.
    /// Which Icom DATA mode to select for digital operating: 1, 2 or 3.
    ///
    /// ⚠️ **1 IS TODAY'S BEHAVIOUR AND THE DEFAULT.** On the multi-data-mode Icoms (IC-7610,
    /// IC-9700, IC-705, IC-905) the CI-V byte Nexus has always sent as "data mode on" is in
    /// fact the data mode NUMBER, so every one of those radios lands on D1 — and an operator
    /// who wires USB audio to D2 finds the radio moved back under them on every mode assert.
    /// An IC-7610 operator reported exactly that (2026-08-19): "in ft mode the radio goes to
    /// data-d1, I need data-d2 — did I miss a setting?" They had not; there was none.
    ///
    /// ⚠️ NEEDS BENCH. This is a CAT change across a class of radios and cannot be verified
    /// here. It ships defaulting to 1 so nothing changes for anyone who does not choose
    /// otherwise, and 2/3 want a real radio in front of someone before they are called working.
    #[serde(default = "one")]
    pub icom_data_mode: u8,
    pub icom_native_cat: bool,
    /// Command PLAIN SSB (USB/LSB by band) instead of the DATA submode on the soundcard
    /// modes — Digital (FT8/FT4/FT1), RTTY-AFSK and SSTV. Off by default: the DATA submode
    /// is correct for the overwhelming majority of stations and is what
    /// [`Settings::rig_mode`] forces.
    ///
    /// ⚠️ THIS IS WIRING-DEPENDENT, AND WRONG FOR MOST RIGS. On a normal setup the rig's
    /// USB codec feeds only the DATA path, so plain USB/LSB takes TX audio from the MIC and
    /// the radio transmits with **zero RF** — the "red light, no signal" failure. It is
    /// correct only when the transmit audio actually reaches the mic path: an interface
    /// wired into the MIC jack (several RIGblaster models), or a rig whose data port is live
    /// in SSB.
    ///
    /// PER RADIO, not global, because it is a property of the CABLE — a station can run a
    /// mic-jack interface on one rig and a data-port interface on the other.
    ///
    /// RTTY in FSK mode is unaffected: it commands the rig's own RTTY mode, which is neither
    /// a DATA submode nor SSB.
    #[serde(default)]
    pub data_modes_plain_ssb: bool,
    // --- audio (a rig's own RX codec) ---
    pub audio_in: String,
    pub audio_out: String,
    pub tx_level: f32,
    /// RX capture gain (≥1.0) applied to received audio before decode; 1.0 = unchanged.
    pub rx_gain: f32,
    // --- rotator (per-radio; replaces the old 4533 rotctld singleton) ---
    pub rotator_model: u32,
    pub rotator_port: String,
    pub rotator_baud: u32,
    pub amp_model: String,
    pub amp_port: String,
    pub amp_follow_band: bool,
    pub rotator_host: String,
    /// UNIQUE across enabled profiles (validated) — each radio's own rotctld TCP port.
    pub rotctld_port: u16,
    // --- band routing (auto-select this radio for these bands; EMPTY = covers everything) ---
    pub bands: Vec<String>,
    // --- per-radio persisted tune (restored when the radio becomes active) ---
    pub last_dial_mhz: f64,
    pub last_band: String,
    pub last_sideband: String,
    // --- native panadapter: "auto" | "none" | "flex" | "civ" ---
    pub native_scope: String,
    // --- FlexRadio native lane (PER-RADIO since 2026-08-18) ---
    /// THIS radio's FlexRadio LAN IP for the SmartSDR Ethernet API (port 4992) — the address the
    /// native panadapter / DAX workers connect to. Distinct from `rig_addr`, which on the
    /// SmartSDR-CAT model 2036 names the *PC* running SmartSDR CAT, not the radio.
    ///
    /// PER-RADIO because it has to be, and it was flat until the 2026-08-17 Flex audit found both
    /// halves of the cost (wave-1 #30/#46): a flat address cannot describe two Flexes, so the
    /// wrong radio's address was used after a switch, AND — the data-loss half — the Settings
    /// per-radio Edit flow routes through [`RadioProfilePatch`], which carried none of these three,
    /// so configuring radio 2 silently dropped the Flex config of radio 1. Exactly the
    /// `ptt_serial_port` class documented on that field, one screen up.
    #[serde(default)]
    pub flex_radio_ip: String,
    /// Opt-in to THIS radio's native SmartSDR panadapter (VITA-49 FFT). See
    /// [`Settings::flex_native_pan`] for what the feature is and why it is opt-in; per-radio for
    /// the same reason as `flex_radio_ip` — one Flex may run it while another rig does not.
    #[serde(default)]
    pub flex_native_pan: bool,
    /// Opt-in to THIS radio's native FlexRadio DAX audio (BOTH directions — see
    /// [`Settings::flex_native_audio`]). Per-radio, as above.
    #[serde(default)]
    pub flex_native_audio: bool,
}

/// The editable CAT/audio/PTT/rotator/native subset of a [`RadioProfile`], sent from the Settings
/// per-radio page to edit ONE radio in place (via [`Engine::update_radio_profile`]) without making
/// it active. Excludes identity (`id`/`name`/`enabled`), band coverage (its own command), and the
/// `last_*` tune memory (owned by the radio loop). Mirrors the field set of `sync_active_from_flat`.
///
/// [`Engine::update_radio_profile`]: crate::engine::Engine::update_radio_profile
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioProfilePatch {
    pub ptt_method: String,
    pub rig_model: u32,
    pub rig_model_name: String,
    pub serial_port: String,
    /// Dedicated RTS/DTR PTT port for THIS radio (see `RadioProfile::ptt_serial_port`).
    #[serde(default)]
    pub ptt_serial_port: String,
    pub baud: u32,
    pub rig_conn: String,
    pub rig_addr: String,
    /// See `RadioProfile::omnirig_slot` — RIG 1 / RIG 2. `#[serde(default)]` like
    /// `ptt_serial_port`: a payload written before the field existed still deserializes.
    #[serde(default)]
    pub omnirig_slot: u8,
    pub rigctld_port: u16,
    pub icom_native_cat: bool,
    /// See `RadioProfile::icom_data_mode` — D1/D2/D3. `#[serde(default = "one")]` so a payload
    /// written before the field existed still deserializes as today's behaviour.
    #[serde(default = "one")]
    pub icom_data_mode: u8,
    /// See `RadioProfile::data_modes_plain_ssb` — plain SSB instead of the DATA submode.
    #[serde(default)]
    pub data_modes_plain_ssb: bool,
    pub audio_in: String,
    pub audio_out: String,
    pub tx_level: f32,
    pub rx_gain: f32,
    pub rotator_model: u32,
    pub rotator_port: String,
    pub rotator_baud: u32,
    pub amp_model: String,
    pub amp_port: String,
    pub amp_follow_band: bool,
    pub rotator_host: String,
    pub rotctld_port: u16,
    pub native_scope: String,
    /// See `RadioProfile::flex_radio_ip` — the Flex API address of THIS radio. `#[serde(default)]`
    /// like `ptt_serial_port`: a payload written before these were per-radio still deserializes.
    #[serde(default)]
    pub flex_radio_ip: String,
    /// See `RadioProfile::flex_native_pan`.
    #[serde(default)]
    pub flex_native_pan: bool,
    /// See `RadioProfile::flex_native_audio`.
    #[serde(default)]
    pub flex_native_audio: bool,
}

impl RadioProfilePatch {
    /// Copy the patch fields onto a profile, leaving its identity / bands / tune-memory alone.
    pub fn apply_to(self, p: &mut RadioProfile) {
        p.ptt_method = self.ptt_method;
        // Was MISSING (shipped 0.18.0): the patch declared and the UI sent `ptt_serial_port`,
        // but apply_to never assigned it — so editing the dedicated RTS/DTR keying port of a
        // radio you are NOT currently operating silently saved nothing. Same per-radio-Edit
        // routing class as the 0.17.12 dual-radio clobber. `ptt_serial_port_is_assigned_by_the
        // _patch` pins it; `radio_profile_patch_assigns_every_profile_field` stops the next
        // added field from being dropped the same way.
        p.ptt_serial_port = self.ptt_serial_port;
        p.rig_model = self.rig_model;
        p.rig_model_name = self.rig_model_name;
        p.serial_port = self.serial_port;
        p.baud = self.baud;
        p.rig_conn = self.rig_conn;
        p.rig_addr = self.rig_addr;
        p.omnirig_slot = self.omnirig_slot;
        p.rigctld_port = self.rigctld_port;
        p.icom_native_cat = self.icom_native_cat;
        p.icom_data_mode = self.icom_data_mode;
        p.data_modes_plain_ssb = self.data_modes_plain_ssb;
        p.audio_in = self.audio_in;
        p.audio_out = self.audio_out;
        p.tx_level = self.tx_level;
        p.rx_gain = self.rx_gain;
        p.rotator_model = self.rotator_model;
        p.rotator_port = self.rotator_port;
        p.amp_model = self.amp_model;
        p.amp_port = self.amp_port;
        p.amp_follow_band = self.amp_follow_band;
        p.rotator_baud = self.rotator_baud;
        p.rotator_host = self.rotator_host;
        p.rotctld_port = self.rotctld_port;
        p.native_scope = self.native_scope;
        p.flex_radio_ip = self.flex_radio_ip;
        p.flex_native_pan = self.flex_native_pan;
        p.flex_native_audio = self.flex_native_audio;
    }
}

/// serde default helper: booleans that default ON for absent fields in older settings.
/// The worldwide Tier 2 rotate address. A regional rotate (`noam.aprs2.net`, `euro.aprs2.net`, …)
/// is slightly better, but requires knowing where the operator is; this works everywhere.
fn default_aprs_is_host() -> String {
    "rotate.aprs2.net".to_string()
}

/// The user-defined filter port — the one APRS-IS asks clients and iGates to use.
fn default_aprs_is_port() -> u16 {
    14580
}

/// One hour — see the field doc for why an hour and not less.
fn default_aprs_station_ttl_min() -> u32 {
    60
}

/// 150 km: a generous horizon for 2 m simplex plus a digipeater hop or two.
fn default_aprs_is_radius_km() -> u32 {
    150
}

/// ">" — Car, on the primary table. What the cockpit has always beaconed.
fn default_aprs_symbol_code() -> String {
    ">".to_string()
}

/// "/" — the primary symbol table, which is what the cockpit hardcoded before the
/// symbol became a setting.
fn default_aprs_symbol_table() -> String {
    "/".to_string()
}

fn default_aprs_comment() -> String {
    "Nexus APRS".to_string()
}

/// The near-universal two-hop path. ⚠️ NAMED, not `#[serde(default)]` — see the field
/// doc: an empty vec is a legitimate "direct, no digipeaters", so a bare default would
/// silently strip every upgrading operator's hops.
fn default_aprs_path() -> Vec<String> {
    vec!["WIDE1-1".to_string(), "WIDE2-1".to_string()]
}

fn default_true() -> bool {
    true
}

impl Default for RadioProfile {
    fn default() -> Self {
        RadioProfile {
            id: 0,
            name: String::new(),
            enabled: true,
            ptt_method: "vox".to_string(),
            rig_model: 0,
            rig_model_name: "None / VOX".to_string(),
            serial_port: String::new(),
            ptt_serial_port: String::new(),
            baud: 38400,
            rig_conn: "serial".to_string(),
            rig_addr: String::new(),
            omnirig_slot: 1,
            // 4534, not 4532: the CAT broker owns 4532 by default (#53) — a default that
            // collided with it would rely on load-time port repair everywhere, and open_cat
            // refuses a self-collision with dead CAT. 4533 is out too: it is the rotctld
            // default in this very profile (and Hamlib's rotctld convention). Hamlib's
            // rigctld number stays the BROKER's, so external loggers land on the broker.
            rigctld_port: 4534,
            icom_native_cat: false,
            icom_data_mode: 1,
            data_modes_plain_ssb: false,
            audio_in: String::new(),
            audio_out: String::new(),
            tx_level: 0.9,
            rx_gain: 1.0,
            rotator_model: 0,
            rotator_port: String::new(),
            rotator_baud: default_rotator_baud(),
            amp_model: String::new(),
            amp_port: String::new(),
            amp_follow_band: false,
            rotator_host: String::new(),
            rotctld_port: 4533,
            bands: Vec::new(),
            last_dial_mhz: 0.0,
            last_band: String::new(),
            last_sideband: String::new(),
            native_scope: "auto".to_string(),
            flex_radio_ip: String::new(),
            flex_native_pan: false,
            flex_native_audio: false,
        }
    }
}

/// Validate that every enabled profile's rigctld port + rotctld port (and the CAT broker port, if
/// on) are pairwise distinct — two daemons can't bind the same TCP port. Pure; used by the Settings
/// save path + the UI. Rotctld ports only count for profiles that actually have a rotator.
pub fn validate_radio_ports(radios: &[RadioProfile], broker: Option<u16>) -> Result<(), String> {
    let mut used: Vec<(u16, String)> = Vec::new();
    for p in radios.iter().filter(|p| p.enabled) {
        used.push((p.rigctld_port, format!("{}'s CAT", p.name)));
        if p.rotator_model > 0 || !p.rotator_host.is_empty() {
            used.push((p.rotctld_port, format!("{}'s rotator", p.name)));
        }
    }
    if let Some(b) = broker {
        used.push((b, "the CAT broker".to_string()));
    }
    for i in 0..used.len() {
        for j in (i + 1)..used.len() {
            if used[i].0 == used[j].0 {
                return Err(format!(
                    "TCP port {} is claimed by both {} and {} — give them different ports",
                    used[i].0, used[i].1, used[j].1
                ));
            }
        }
    }
    Ok(())
}

/// Two enabled radios cannot share a serial CAT port: the OS opens a COM port
/// exclusively, so the monitor radio's CAT can't open the busy port and reads as
/// failing (a confusing persistent-red pill). Unlike TCP ports, a serial port can't
/// be auto-bumped — it's real hardware — so this is a WARNING the operator must act
/// on, surfaced in the snapshot (it self-clears once the ports differ). Only counts
/// radios that actually use a serial CAT link (a real rig on a serial connection with
/// a port set); network CAT and VOX/none don't own a COM port. Case-insensitive
/// (`COM3` == `com3`). Returns the first collision message, else `None`.
pub fn serial_port_conflicts(radios: &[RadioProfile]) -> Option<String> {
    let mut used: Vec<(String, String)> = Vec::new(); // (port, radio name)
    for p in radios.iter().filter(|p| {
        p.enabled
            && p.rig_model > 0
            && p.rig_conn.eq_ignore_ascii_case("serial")
            && !p.serial_port.trim().is_empty()
    }) {
        let port = p.serial_port.trim();
        if let Some((_, other)) = used.iter().find(|(u, _)| u.eq_ignore_ascii_case(port)) {
            return Some(format!(
                "{other} and {} are both on serial port {port} — only one radio can own a COM port. \
                 Give them different ports (or disable one).",
                p.name
            ));
        }
        used.push((port.to_string(), p.name.clone()));
    }
    None
}

/// Two enabled radios pointed at ONE network CAT address — the network twin of
/// [`serial_port_conflicts`], and the gap that function's own doc-comment left open ("network CAT
/// … don't own a COM port", which is true of the COM port and says nothing about the endpoint).
///
/// Found by the 2026-08-17 Flex audit (wave-2 #20): `validate_radio_ports` de-duplicates the
/// rigctld/rotctld/broker ports between profiles and never looks at `rig_addr`, so two enabled Flex
/// profiles both left on SmartSDR CAT's default `127.0.0.1:5002` — the natural mistake when an
/// operator duplicates a profile for a second slice and forgets the port — passed every check.
/// Nexus then launches two rigctld daemons that both open one SmartSDR CAT slice port, and the two
/// chains fight over dial and mode; the symptom reads as a flaky radio.
///
/// A WARNING, never a save-block, and deliberately softer than the serial one: a shared address is
/// wrong for a Flex but legitimate for some rigctld setups (two profiles sharing one remote daemon
/// on purpose, differing only in audio). Same surface as its two siblings — the snapshot's
/// `radio_config_warning`, which self-clears once the addresses differ.
///
/// Host:port compared case-insensitively after trimming; the loopback spellings
/// (`localhost`/`127.0.0.1`/`::1`) are normalised, because they are the same endpoint and this
/// mistake is made ON loopback. Returns the first collision message, else `None`.
pub fn network_cat_address_conflicts(radios: &[RadioProfile]) -> Option<String> {
    /// `localhost:5002`, `127.0.0.1:5002` and `[::1]:5002` are one endpoint. Everything else is
    /// compared as written — resolving names is I/O, and this is a pure pre-save check.
    fn normalize(addr: &str) -> String {
        let a = addr.trim().to_ascii_lowercase();
        let (host, port) = match a.rsplit_once(':') {
            Some((h, p)) => (h.trim_matches(['[', ']']), p),
            None => return a,
        };
        let host = match host {
            "localhost" | "127.0.0.1" | "::1" => "localhost",
            other => other,
        };
        format!("{host}:{port}")
    }
    let mut used: Vec<(String, String)> = Vec::new(); // (normalized addr, radio name)
    for p in radios.iter().filter(|p| {
        p.enabled
            && p.rig_model > 0
            && p.rig_conn.eq_ignore_ascii_case("network")
            && !p.rig_addr.trim().is_empty()
    }) {
        let addr = normalize(&p.rig_addr);
        if let Some((_, other)) = used.iter().find(|(u, _)| *u == addr) {
            return Some(format!(
                "{other} and {} are both on network CAT address {} — two radio chains commanding \
                 one endpoint fight over dial and mode. For a FlexRadio, each SmartSDR CAT slice \
                 has its OWN port (A=5002, B=60001, C=60002, D=60003).",
                p.name,
                p.rig_addr.trim()
            ));
        }
        used.push((addr, p.name.clone()));
    }
    None
}

/// Two enabled radios cannot share a sound card. Each radio's chain opens its OWN cpal capture and
/// playback streams on that rig's codec, so a shared `audio_in` means both chains decode the SAME
/// receiver and a shared `audio_out` means both rigs are fed both chains' transmit audio. Neither
/// fails loudly — the operator just sees a second radio that hears (or says) the first radio's
/// traffic. Same WARNING surface as [`serial_port_conflicts`]; it self-clears once the devices
/// differ.
///
/// Compared on the exact stored name, which is the disambiguated picker string (`tempo-audio`
/// appends " #2"/" #3" to identically-named devices), so two rigs that both enumerate as the
/// generic "USB Audio CODEC" are correctly seen as DIFFERENT devices. A BLANK name means "the
/// system default" and is not compared: a second radio configured for CAT only has no audio set
/// and never opens a stream, so flagging blank-vs-blank would warn on a working config. The audio
/// probe (`tempo-audio`'s `audio_probe` example) refuses to run on a blank name instead, which is
/// where two defaults would actually collide. Returns the first collision, else `None`.
pub fn audio_device_conflicts(radios: &[RadioProfile]) -> Option<String> {
    shared_audio_device(radios, "input", |p| p.audio_in.as_str())
        .or_else(|| shared_audio_device(radios, "output", |p| p.audio_out.as_str()))
}

/// The one-direction body of [`audio_device_conflicts`]. Case-insensitive — device names come back
/// from the OS with inconsistent casing.
fn shared_audio_device(
    radios: &[RadioProfile],
    kind: &str,
    device: fn(&RadioProfile) -> &str,
) -> Option<String> {
    let mut used: Vec<(&str, &str)> = Vec::new(); // (device, radio name)
    for p in radios.iter().filter(|p| p.enabled) {
        let dev = device(p).trim();
        if dev.is_empty() {
            continue;
        }
        if let Some((_, other)) = used.iter().find(|(u, _)| u.eq_ignore_ascii_case(dev)) {
            return Some(format!(
                "{other} and {} are both using audio {kind} \"{dev}\" — each radio needs its own \
                 sound card. Give them different devices (or disable one).",
                p.name
            ));
        }
        used.push((dev, p.name.as_str()));
    }
    None
}

/// A serial CW keyline aimed at a radio's CAT serial port is dangerous: opening that
/// port toggles its DTR/RTS, which on most rigs is the PTT/keying line — so the keyer
/// would fight rigctld for the port and can key the rig just by connecting. Warn (same
/// surface as [`serial_port_conflicts`]) when the configured `cw_key_port` matches any
/// enabled serial radio's CAT port. Case-insensitive; only when the serial keyer is the
/// selected backend. The keyer ALSO deasserts both lines on open as a hard safety net —
/// this warning is so the operator fixes the config rather than relying on that.
pub fn cw_key_port_conflict(
    keyer: CwKeyerBackend,
    cw_key_port: &str,
    radios: &[RadioProfile],
) -> Option<String> {
    if keyer != CwKeyerBackend::Serial {
        return None;
    }
    let kp = cw_key_port.trim();
    if kp.is_empty() {
        return None;
    }
    radios
        .iter()
        .find(|p| {
            p.enabled
                && p.rig_model > 0
                && p.rig_conn.eq_ignore_ascii_case("serial")
                && p.serial_port.trim().eq_ignore_ascii_case(kp)
        })
        .map(|p| {
            format!(
                "The CW key port {kp} is also {}'s CAT port — the serial keyer would key that \
                 port's PTT line and fight the radio for it. Give the CW keyline its own port.",
                p.name
            )
        })
}

/// An amplifier configured on a port some OTHER device on this station already owns.
///
/// ⭐ SERIAL PORTS ARE EXCLUSIVE-OPEN. The amplifier poller holds `amp_port` for the whole
/// session the moment it is configured — the same thing `Engine::hold_cat_port` and its
/// release/ack handshake exist to manage for CAT, and the reason the baud ladder says "our own
/// live daemon holds the port even when the rig is mute". So an operator who types their CAT
/// port into the amplifier field does not get an amplifier that fails to answer, they get a
/// RADIO that fails to connect, and they will report it as a radio bug.
///
/// [`serial_port_conflicts`] cannot see this: it filters on `serial_port` alone and never looks
/// at `amp_port`. This is its amplifier twin, and the exact sibling of [`cw_key_port_conflict`]
/// — an auxiliary serial device colliding with something else that opens a port.
///
/// SOFT, like all three of its siblings in the warning chain: it is not a save-block. It rides
/// `radio_config_warning` and self-clears the moment the ports differ.
///
/// Checked against every port a live station actually opens: each enabled serial radio's CAT
/// port and its dedicated PTT keying port, the amplifier's own radio's rotator port, and the
/// three global auxiliary serial devices (`cw_key_port`, `winkeyer_port`, `rtty_fsk_port`).
pub fn amp_port_conflict(
    radios: &[RadioProfile],
    cw_key_port: &str,
    winkeyer_port: &str,
    rtty_fsk_port: &str,
) -> Option<String> {
    for p in radios
        .iter()
        .filter(|p| p.enabled && !p.amp_port.trim().is_empty() && !p.amp_model.trim().is_empty())
    {
        let ap = p.amp_port.trim();
        let same = |other: &str| !other.trim().is_empty() && other.trim().eq_ignore_ascii_case(ap);

        // Its own rotator, which this radio's rotctld opens.
        if p.rotator_model > 0 && same(&p.rotator_port) {
            return Some(format!(
                "{}'s amplifier port {ap} is also its rotator port — a serial port can only be \
                 open once, so one of the two will fail to connect. Give the amplifier its own \
                 port.",
                p.name
            ));
        }

        // Any enabled serial radio's CAT port or dedicated keying port — including this one's.
        for r in radios.iter().filter(|r| r.enabled) {
            let serial_cat = r.rig_model > 0 && r.rig_conn.eq_ignore_ascii_case("serial");
            if serial_cat && same(&r.serial_port) {
                return Some(format!(
                    "{}'s amplifier port {ap} is also {}'s CAT port — a serial port can only be \
                     open once, so the radio will fail to connect. Give the amplifier its own \
                     port.",
                    p.name, r.name
                ));
            }
            if same(&r.ptt_serial_port) {
                return Some(format!(
                    "{}'s amplifier port {ap} is also {}'s PTT keying port — a serial port can \
                     only be open once, so the rig will not key. Give the amplifier its own port.",
                    p.name, r.name
                ));
            }
        }

        // The global auxiliary serial devices.
        for (port, what) in [
            (cw_key_port, "the CW keyline"),
            (winkeyer_port, "the WinKeyer"),
            (rtty_fsk_port, "the RTTY FSK keyline"),
        ] {
            if same(port) {
                return Some(format!(
                    "{}'s amplifier port {ap} is also {what}'s port — a serial port can only be \
                     open once, so one of the two will fail to connect. Give the amplifier its \
                     own port.",
                    p.name
                ));
            }
        }
    }
    None
}

/// Q65-60: the EME working period. See [`Settings::q65_period_s`].
fn default_q65_period_s() -> u16 {
    60
}

/// FST4/FST4W at 120 s. See [`Settings::fst4_period_s`].
fn default_fst4_period_s() -> u16 {
    120
}

/// MSK144 at 15 s — the 6 m meteor-scatter period. See [`Settings::msk144_period_s`].
fn default_msk144_period_s() -> u16 {
    15
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // Empty by default — "configured" means the operator entered a real
            // call (drives feed-gating + first-run onboarding). Must NOT default to
            // a real call: that call's owner would then have every feed gated off.
            mycall: String::new(),
            mygrid: String::new(),
            dxkeeper_host: String::new(), // empty = off
            dxkeeper_base_port: default_dxkeeper_base_port(),
            dxkeeper_uploads: false, // Nexus owns the upload connectors
            op_name: String::new(),
            op_state: String::new(),
            // ON by default — the operator's explicit sign-off (2026-08-09): with the broker
            // as the advertised share endpoint, a shared WSJT-X/VarAC keeps the keying it has
            // via the Hamlib port today. NOT a TX gate bypass: a broker key request still
            // passes every engine-side gate (TX-enable latch, privilege lockout, watchdog).
            cat_broker_ptt: true,
            band: "20m".to_string(),
            dial_mhz: 14.074, // FT8 20m — the default mode/band
            sideband: "USB".to_string(),
            phone_mode: "ssb".to_string(),
            rptr_shift: "simplex".to_string(),
            ctcss_tone_hz: 0.0,
            q65_period_s: default_q65_period_s(),
            // Beaconing OFF by default: it keys the radio unattended, so it is
            // always an explicit operator decision.
            beacon_tx_percent: 0,
            beacon_power_dbm: 0,
            beacon_rr_slot: 0,
            beacon_rr_slots: 0,
            fst4_period_s: default_fst4_period_s(),
            msk144_period_s: default_msk144_period_s(),
            jt65_submode: 0,
            q65_submode: 0,
            rptr_offset_override_hz: 0, // 0 = band-convention offset
            fd_active: false,           // never auto-enabled — only the operator's toggle sets this
            fd_class: String::new(),
            fd_event: String::new(), // "" = arrlfd
            fd_power_mult: 2,
            fd_bonuses: Vec::new(),
            fd_bonuses_planned: Vec::new(),
            n3fjp_host: String::new(),
            n3fjp_port: 1100,
            n3fjp_use_enter: true,
            n3fjp_report_band: false,
            n1mm_addr: String::new(),
            n1mm_upload: false, // never opt an operator into a network broadcast
            // Deliberately EMPTY: a contest exchange goes on the air, so it must
            // be the operator's own — set_mode refuses Field Day until both the
            // class and section are set (a "WI" default sent wrong exchanges for
            // every operator outside Wisconsin).
            fd_section: String::new(),
            fd_operator: String::new(),
            fd_host_enable: false, // hosting exposes a LAN port — operator-only opt-in
            fd_host_port: default_fd_host_port(),
            fd_event_name: String::new(),
            fd_join_addr: String::new(),
            fd_position_name: String::new(),
            fd_position_id: String::new(), // generated (8-hex) at startup, then persisted
            fd_scoreboard: false,          // serving a LAN page is an operator-only opt-in
            fd_scoreboard_port: default_fd_scoreboard_port(),
            connect_web: false, // same rule: exposing the station on the LAN is opt-in
            connect_web_port: default_connect_web_port(),
            beacon: false,
            harq_enabled: true,
            ptt_method: "vox".to_string(),
            rig_model: 0,
            rig_model_name: "None / VOX".to_string(),
            serial_port: String::new(),
            ptt_serial_port: String::new(),
            // Held LOW by default — the 1.0.2 TX-safety fix. Struct-level `#[serde(default)]`
            // means a settings file written before 1.0.2 has no such key and therefore loads
            // THIS value, so the fix reaches existing stations without a migration.
            cat_rts_state: "low".to_string(),
            cat_dtr_state: "low".to_string(),
            // Default OFF, deliberately. Ticking it lets Nexus drop a rig's DECLARED hardware
            // handshake so RTS becomes holdable — right for a Digirig-class cable, and exactly
            // what must never happen to a rig that did not ask for it (the FTDX10/FT-991 bench
            // regression). An operator who ticks nothing gets today's behaviour unchanged.
            cat_rts_keys_ptt: false,
            // "auto" on both = today's behaviour exactly: infer the handshake as before, and
            // say nothing at all about the keying line. #145's knobs are opt-in, because each
            // of them can cost a working station its CAT (see the field docs).
            cat_serial_handshake: default_cat_auto(),
            cat_ptt_line_state: default_cat_auto(),
            baud: 38400,
            rig_conn: "serial".to_string(),
            rig_addr: String::new(),
            omnirig_slot: 1,
            icom_native_cat: false,
            split_detect_enabled: false,
            icom_data_mode: 1,
            data_modes_plain_ssb: false,
            set_rig_mode: true, // force the DATA submode for digital, so sections set the rig
            operating_mode: OperatingMode::Digital, // digital obeys; phone/CW force
            license_class: LicenseClass::Open, // no TX lockout until the operator declares a class
            cw_keyer: CwKeyerBackend::Cat, // rig keyer via send_morse (zero hardware)
            winkeyer_port: String::new(),
            cw_key_port: String::new(),
            cw_key_line: default_cw_key_line(),
            cw_pitch_hz: 600.0,
            cw_wpm: default_cw_wpm(),
            rtty_backend: default_rtty_backend(), // "afsk" — soundcard-clocked, the robust default
            rtty_fsk_line: default_rtty_fsk_line(), // "dtr" (RTS stays free for PTT)
            rtty_fsk_port: String::new(),         // "" = the CAT serial port
            rtty_baud: default_rtty_baud(),       // true 45.45, never 45
            rtty_shift_hz: default_rtty_shift_hz(), // 170 Hz — the HF standard
            rtty_reverse: false,
            ai_cw_enabled: true,
            // Assistance is a normal, legal way to operate; only the operator's explicit
            // toggle declares an unassisted entry. Never auto-enabled, never date-driven.
            unassisted_mode: false,
            // Matches the profile default above: 4534 (broker owns 4532, rotctld 4533; #53).
            rigctld_port: 4534,
            rotator_model: 0,
            rotator_port: String::new(),
            rotator_baud: default_rotator_baud(),
            amp_model: String::new(),
            amp_port: String::new(),
            amp_follow_band: false,
            rotator_host: String::new(),
            // Satellite Doppler is OFF and unmapped by default: a station
            // with no satellite interest must never have its dial moved.
            rot_park_az: 0.0,
            rot_park_el: 0.0,
            rot_ready_az: 0.0,
            rot_ready_el: 0.0,
            rot_post_pass: String::new(),
            rot_tol_az_deg: default_rot_tol_deg(),
            rot_tol_el_deg: default_rot_tol_deg(),
            rot_cal_az_deg: 0.0,
            rot_cal_el_deg: 0.0,
            rot_allow_flip: false,
            sat_doppler_off: false,
            sat_vfo_map: SatVfoMap::Off,
            sat_uplink_radios: None,
            sat_min_shift_hz: default_sat_min_shift_hz(),
            sat_update_ms: default_sat_update_ms(),
            sat_pass_alert_sound_off: false,
            // ON by default (operator decision, 2026-08-09, with catBrokerPtt): the broker is
            // the ADVERTISED share endpoint as of #53 — it answers from cached engine state in
            // microseconds and survives every daemon teardown (Test CAT, config saves), which
            // the Hamlib daemon's port does not. Default 4532 (Hamlib's own NET rigctl port) is
            // deliberate: `ensure_distinct_radio_ports` seeds the broker FIRST and bumps the
            // per-radio daemons off it, so an external logger already pointed at 4532 lands on
            // the broker after upgrade with no reconfiguration. Note: a settings file written
            // by 1.0.5 carries an explicit `false` (the struct persists whole) — those
            // operators flip it in the share block; pre-1.0.5 files lack the field and get ON.
            cat_broker: true,
            cat_broker_port: 4532,
            flex_radio_ip: String::new(),
            flex_native_pan: false,
            flex_native_audio: false,
            radios: Vec::new(), // migrated to a single profile on load()
            active_radio: 0,
            radio_pegged: false,
            simultaneous_radios: false,
            // No rules by default: routing stays band-only (per-radio `bands` coverage) until the
            // operator writes one, so upgrading changes nothing.
            routing_rules: Vec::new(),
            default_radio: None,
            wsjtx_udp: false,
            wsjtx_udp_addr: "127.0.0.1:2237".to_string(),
            write_all_txt: false,
            diag_debug_log: false,
            hrd_logging: false,
            hrd_udp_addr: "127.0.0.1:2333".to_string(),
            companion_addr: "127.0.0.1:2237".to_string(),
            source: SourceKind::Native,
            // Live by default (once a real call is set) — a ham dashboard should
            // arrive connected, like HamClock/GridTracker. Both are public read
            // feeds; cluster_host is the RBN endpoint, so this gives RBN spots free.
            pskreporter: true,
            cluster_enabled: true,
            // A public human DX-cluster node for SSB/phone + human spots (the RBN CW +
            // digital skimmer feeds are wired automatically). VE7CC-1 is the community
            // default — CC-Cluster, human-spot-rich, and skimmer OFF by default, so it
            // doesn't double the RBN firehose we already pull. Configurable; RBN-only
            // operators can blank this. (NOTE: dxc.nc7j.com:7373 is NC7J's *skimmer* port,
            // not its human port — don't use it here; the migration in `load` fixes it.)
            cluster_host: "ve7cc.net:23".to_string(),
            // The aggregator seeds with TWO diverse-port nodes: ve7cc on the standard telnet
            // port 23, plus wa9pie on 8000 — a firewall-friendly fallback, since some
            // networks/ISPs block outbound port 23 (which would silently kill phone while RBN
            // on 7000/7001 keeps working). The operator adds more in Settings ▸ Connections.
            // (RBN endpoints don't belong here — they're auto-wired; `load` strips any.)
            cluster_hosts: vec![
                "ve7cc.net:23".to_string(),
                "dxc.wa9pie.net:8000".to_string(),
            ],
            // APRS-IS is OFF until the operator asks for it: it is an outbound connection to a
            // public service under their callsign, which is theirs to opt into. The uplink is a
            // second, separate opt-in for the same reason, doubly so — it publishes.
            aprs_is_enabled: false,
            aprs_is_host: default_aprs_is_host(),
            aprs_is_port: default_aprs_is_port(),
            aprs_is_radius_km: default_aprs_is_radius_km(),
            aprs_is_watch_calls: Vec::new(),
            aprs_is_weather: true,
            aprs_is_objects: true,
            aprs_is_messages: true,
            aprs_is_uplink: false,
            aprs_station_ttl_min: default_aprs_station_ttl_min(),
            aprs_channel_mhz: None,
            aprs_symbol_code: default_aprs_symbol_code(),
            aprs_symbol_table: default_aprs_symbol_table(),
            aprs_comment: default_aprs_comment(),
            aprs_path: default_aprs_path(),
            aprs_ssid: None,
            audio_in: String::new(),
            audio_out: String::new(),
            voice_mic_device: String::new(),
            tx_level: 0.9,
            rx_gain: 1.0,
            monitor_enabled: false,
            monitor_device: String::new(),
            monitor_level: 0.5,
            station_power_w: None,
            units: default_units(),
            max_power_phone: None,
            max_power_cw: None,
            max_power_digital: None,
            max_power_am: default_max_power_am(),
            prop_engine: default_prop_engine(),
            save_wav: default_save_wav(),
            lotw_max_age_days: default_lotw_max_age_days(),
            ant_tx_gain_dbi: 0.0,
            ant_rx_gain_dbi: 0.0,
            journey_streak_enabled: false,
            tx_watchdog_min: 6,
            tx_even: true,
            rx_offset_hz: 1500.0,
            tx_offset_hz: 1500.0,
            hold_tx_freq: false,
            clock_check: true,
            auto_log: true,
            prompt_to_log: false,
            save_qso_wav: false,
            prefer_rrr: false,
            // Both of these MUST match their serde defaults above. A struct default that
            // disagrees with the serde one means a fresh install and a settings.json missing
            // the field behave differently — the same operator, two answers, and no way to
            // tell which they got.
            cq_max_calls: default_cq_max_calls(),
            cq_pause_secs: default_cq_pause_secs(),
            directed_max_calls: Some(8),
            chat_max_cycles: None,
            chat_implicit_ack: true,
            cq_stall_overs: None,
            disable_tx_after_73: true,
            band_edge_tones: true,
            cw_id_after_73: false,
            clear_dx_after_log: false,
            double_click_sets_tx: true,
            tune_timeout_secs: 12,
            tune_power_pct: None, // None = never touch the operator's power
            split_mode: SplitMode::None,
            special_op: SpecialOp::None,
            decode_depth: 3,
            decode_flow_hz: 200,
            decode_fhigh_hz: 2900,
            ap_decode: true,      // stock WSJT-X: Enable AP on
            ap_cq_only: false,    // stock: all AP hypotheses
            single_decode: false, // stock: full-passband decode
            working_frequencies: Vec::new(),
            qsy_enabled: false,
            qsy_set: vec!["20m".to_string(), "40m".to_string(), "30m".to_string()],
            qsy_cadence: tempo_core::qsy::DEFAULT_CADENCE,
            iss_sstv_auto_arm: false,
            sstv_rx_auto_arm: true,
            sstv_default_tx_mode: default_sstv_default_tx_mode(),
            sstv_tx_power_pct: None,
            psk_rx_auto_arm: true,
            rtty_rx_auto_arm: true,
            alert_my_call: true,
            alert_confirm_tier: true, // the tier ships lit; the setting is the opt-out
            log_reports_to_comments: false, // WSJT-X parity: dBtoComments defaults false
            best_caller: default_best_caller(),
            best_caller_min_snr: None,
            blocked_calls: Vec::new(),
            wanted_calls: Vec::new(),
            pounce_threshold: PounceThreshold::default(),
            alert_cq: false,
            // New-DXCC / new-grid alerts: ON by default — these are the "new ones"
            // worth chasing (not per-decode spam, which we never alert on).
            alert_new: true,
            alert_dxcc_bands: default_alert_scope_all(),
            alert_grid_bands: default_alert_grid_bands(),
            b4_match_mode: false,
            alert_rare_grid_bands: default_alert_grid_bands(),
            wheel_tune_sensitivity: default_wheel_tune_sensitivity(),
            announce_verbosity: default_announce_verbosity(),
            sound_tx_state: false,
            sound_decode_tick: false,
            lotw_username: String::new(),
            lotw_last_qsl: String::new(),
            lotw_station_location: String::new(),
            lotw_use_adif_location: false,
            tqsl_path: String::new(),
            lotw_auto_upload: false,
            lotw_auto_upload_hours: default_lotw_auto_upload_hours(),
            lotw_last_auto_upload_unix: 0,
            eqsl_username: String::new(),
            eqsl_last_sync: String::new(),
            qrz_username: String::new(),
            hamqth_username: String::new(),
            qrz_logbook_upload: false,
            qrz_auto_sync: false,
            qrz_sync_hours: default_qrz_sync_hours(),
            qrz_last_sync_unix: 0,
            clublog_email: String::new(),
            clublog_callsign: String::new(),
            clublog_api_key: String::new(),
            clublog_upload: false,
            eqsl_upload: false,
            hrdlog_upload: false,
            wrl_upload: false, // the credential is the opt-in
            wrl_logbook_id: String::new(),
            n3fjp_upload: false,
            cloudlog_url: String::new(),
            cloudlog_station_id: String::new(),
            cloudlog_key: String::new(),
            cloudlog_upload: false,
            opening_regional: true,
            macros: Macros::default(),
            voice_messages: default_voice_messages(),
        }
    }
}

impl Settings {
    /// Is the DeepCW AI CW decoder EFFECTIVELY running? The operator's own
    /// `ai_cw_enabled`, minus the [`Settings::unassisted_mode`] override.
    ///
    /// Every gate that decides whether the decoder runs must go through here rather than
    /// reading `ai_cw_enabled`, or Unassisted mode becomes a switch that claims more than
    /// it does. Turning the AI decoder off restores the classic Goertzel transcript, so
    /// the operator still copies CW — just without the model's help.
    pub fn ai_cw_active(&self) -> bool {
        self.ai_cw_enabled && !self.unassisted_mode
    }

    /// Is DX cluster / RBN spot ingestion EFFECTIVELY on? Cluster and RBN spots are
    /// callsign-and-frequency identification from other people's receivers — the textbook
    /// QSO-finding assistance, named by both CQ WW and ARRL.
    pub fn cluster_active(&self) -> bool {
        self.cluster_enabled && !self.unassisted_mode
    }

    /// Is the PSK Reporter feed EFFECTIVELY available as need EVIDENCE (who can I work)?
    ///
    /// Only the INBOUND direction is assistance. ARRL's glossary names PSKReporter in
    /// "Spotting/QSO Finding Assistance" but says plainly that "Generating spotting
    /// information for use by other stations is not considered to be spotting assistance"
    /// — so the operator's own OUTBOUND uploads (`pskreporter`) keep running in Unassisted
    /// mode, and only the reception reports we consume to build the Needed board stop.
    pub fn pskr_evidence_active(&self) -> bool {
        !self.unassisted_mode
    }

    /// The assistance sources this build knows how to suppress, as
    /// `(label, effectively_on)` — the single list the journal records and the UI reads,
    /// so a new assistance source cannot be added to the app and forgotten by Unassisted
    /// mode. Order is stable: it is display order and journal order.
    pub fn assistance_sources(&self) -> [(&'static str, bool); 3] {
        [
            ("AI CW decoder", self.ai_cw_active()),
            ("DX cluster / RBN", self.cluster_active()),
            ("PSK Reporter needs", self.pskr_evidence_active()),
        ]
    }

    /// Build a RadioProfile mirroring the current flat rig/audio fields — the migration seed for a
    /// single-radio station's profile 0.
    fn radio_profile_from_flat(&self, id: u32) -> RadioProfile {
        RadioProfile {
            id,
            name: if self.rig_model_name.trim().is_empty() || self.rig_model_name == "None / VOX" {
                format!("Radio {}", id + 1)
            } else {
                self.rig_model_name.clone()
            },
            enabled: true,
            ptt_method: self.ptt_method.clone(),
            rig_model: self.rig_model,
            rig_model_name: self.rig_model_name.clone(),
            serial_port: self.serial_port.clone(),
            ptt_serial_port: self.ptt_serial_port.clone(),
            baud: self.baud,
            rig_conn: self.rig_conn.clone(),
            rig_addr: self.rig_addr.clone(),
            omnirig_slot: self.omnirig_slot,
            rigctld_port: self.rigctld_port,
            icom_native_cat: self.icom_native_cat,
            icom_data_mode: self.icom_data_mode,
            data_modes_plain_ssb: self.data_modes_plain_ssb,
            audio_in: self.audio_in.clone(),
            audio_out: self.audio_out.clone(),
            tx_level: self.tx_level,
            rx_gain: self.rx_gain,
            rotator_model: self.rotator_model,
            rotator_port: self.rotator_port.clone(),
            rotator_baud: self.rotator_baud,
            rotator_host: self.rotator_host.clone(),
            // MIRRORS the flat value, not a blank: this is the migration seed for a
            // single-radio station's profile 0, and blanking here would lose an amplifier the
            // operator had already configured before profiles existed.
            amp_model: self.amp_model.clone(),
            amp_port: self.amp_port.clone(),
            amp_follow_band: self.amp_follow_band,
            rotctld_port: 4533,
            bands: Vec::new(),
            last_dial_mhz: self.dial_mhz,
            last_band: self.band.clone(),
            last_sideband: self.sideband.clone(),
            native_scope: "auto".to_string(),
            // The Flex three come from the flat mirror like every other rig field — this IS the
            // migration for a pre-multi-radio settings file (see `load`'s sibling for the file
            // that already HAS profiles).
            flex_radio_ip: self.flex_radio_ip.clone(),
            flex_native_pan: self.flex_native_pan,
            flex_native_audio: self.flex_native_audio,
        }
    }

    /// Ensure at least one radio profile exists (migrate the flat fields to profile 0 for older
    /// settings) and that `active_radio` names a real profile.
    pub fn ensure_radio_profiles(&mut self) {
        if self.radios.is_empty() {
            let p = self.radio_profile_from_flat(0);
            self.radios.push(p);
            self.active_radio = 0;
        }
        if !self.radios.iter().any(|p| p.id == self.active_radio) {
            self.active_radio = self.radios[0].id;
        }
    }

    /// The active radio profile (guaranteed present after `ensure_radio_profiles`).
    pub fn active_profile(&self) -> Option<&RadioProfile> {
        self.radios.iter().find(|p| p.id == self.active_radio)
    }

    /// How many enabled radios EXPLICITLY list `band`.
    ///
    /// More than one means a band activation had a real choice to make, and the operator cannot tell
    /// from the radio itself which way it went. Catch-all radios (empty `bands`) are excluded on
    /// purpose: they rank below an explicit claim in [`Settings::radio_for_band`], so a single
    /// explicit radio plus any number of catch-alls is not ambiguous — the explicit one always wins.
    pub fn radios_covering(&self, band: &str) -> u32 {
        self.radios
            .iter()
            .filter(|p| p.enabled && p.bands.iter().any(|b| b.eq_ignore_ascii_case(band)))
            .count() as u32
    }

    /// Does any ENABLED radio cover `band`? Drives the band dropdowns (#184).
    ///
    /// Same coverage semantics as [`Self::radio_for_band`]: an empty `bands` list is a
    /// catch-all ("this rig covers everything"), a non-empty one is an explicit claim.
    ///
    /// ⚠️ TRUE WHEN NOTHING IS CONFIGURED, and that is the whole safety of this filter. A
    /// station with no radios yet — the first-run wizard, or an operator who has not added one
    /// — must see the full band list, not an empty dropdown. Likewise any catch-all rig makes
    /// every band covered, so the single-radio majority is unaffected: the filter can only
    /// remove a band when EVERY enabled radio has named its bands and none of them named this
    /// one. It is a DISPLAY filter and must never be consulted by the transmit gate; privileges
    /// decide what may be keyed, this decides only what is worth offering.
    pub fn any_radio_covers(&self, band: &str) -> bool {
        let mut any_enabled = false;
        for p in self.radios.iter().filter(|p| p.enabled) {
            any_enabled = true;
            if p.bands.is_empty() || p.bands.iter().any(|b| b.eq_ignore_ascii_case(band)) {
                return true;
            }
        }
        !any_enabled
    }

    /// Which radio should own `band` (Dual-Radio P4 auto band-routing). Returns `Some(id)` only when a
    /// DIFFERENT enabled radio covers the band *better* than the active one — else `None` (stay put).
    ///
    /// Coverage rank: an EXPLICIT band listing (2) beats a catch-all/empty coverage set (1) beats no
    /// coverage (0). A radio that explicitly lists 2 m therefore wins 2 m even when the active radio is
    /// an unrestricted "covers everything" rig — this is the operator's mental model ("switch to the
    /// radio that has 2 m configured"). Ties (both catch-all, or both explicit) keep the active radio
    /// so a fine-tune inside a shared band never bounces. A band no radio claims stays on the active
    /// radio (TX-lock/out-of-range handles it as today). Peg-lock is honored by the caller.
    pub fn radio_for_band(&self, band: &str) -> Option<u32> {
        let rank = |p: &RadioProfile| -> u8 {
            if p.bands.is_empty() {
                1
            } else if p.bands.iter().any(|b| b.eq_ignore_ascii_case(band)) {
                2
            } else {
                0
            }
        };
        let active_rank = self.active_profile().map(&rank).unwrap_or(0);
        // THREE radios make ties reachable for the first time: with two radios there is only ever
        // ONE non-active candidate, so any tie was decided by the `> active_rank` filter. With
        // three, two rigs can both list 2 m.
        //
        // ⚠️ A TIE HAS NO CORRECT AUTOMATIC ANSWER, and picking one silently shipped a regression.
        // `max_by_key` returned the LAST maximum — roster order — which for KD9TAW's shack landed
        // on the FT-991A and decoded APRS. Switching to the lowest id landed on the IC-9700, whose
        // audio is configured for FT8, so the APRS tap followed the wrong radio and the section went
        // silent with nothing visibly wrong. Both rules are arbitrary; the bug was letting an
        // arbitrary rule decide something the operator cares about.
        //
        // So: prefer the radio the operator NOMINATED (`default_radio`) when it is one of the tied
        // candidates — a tie is exactly when an expressed preference should be consulted — and fall
        // back to the lowest id for determinism when there is none. A routing rule still outranks
        // all of this (see `route_radio`), and remains the way to state an unambiguous intent.
        let best = self
            .radios
            .iter()
            .filter(|p| p.enabled && p.id != self.active_radio)
            .map(&rank)
            .max()?;
        self.radios
            .iter()
            .filter(|p| p.enabled && p.id != self.active_radio && rank(p) == best)
            .min_by_key(|p| (self.default_radio != Some(p.id), p.id))
            .filter(|p| rank(p) > active_rank)
            .map(|p| p.id)
    }

    /// Which radio should own `(band, mode)` — the full routing decision. Returns `Some(id)` only
    /// when a DIFFERENT enabled radio should take over; `None` = stay on the active radio.
    ///
    /// Three tiers, in order:
    /// 1. **[`RoutingRule`]s, first-match-wins.** An explicit rule is an operator instruction, so
    ///    it hands off even when the active radio also covers the band — that is the whole point:
    ///    2 m FT8 must leave the FT-991A for the IC-9700 although the 991A does 2 m too.
    /// 2. **Band-only coverage** ([`Settings::radio_for_band`]) — the pre-rules behavior, kept so
    ///    an install that never writes a rule behaves exactly as before.
    /// 3. **[`Settings::default_radio`]** — the "everything else goes here" net.
    ///
    /// A rule/default naming a missing or DISABLED radio is skipped rather than obeyed (an
    /// unplugged rig must never become the handoff target). Peg-lock is honored by the caller.
    ///
    /// Satellite-DESIGNATED rules are invisible here — a terrestrial tune never matches them.
    /// A satellite-originated tune goes through [`Self::route_radio_satellite`], which checks
    /// them first and then falls through to exactly this decision.
    pub fn route_radio(&self, band: &str, mode: RouteMode) -> Option<u32> {
        let usable = |id: u32| self.radios.iter().any(|p| p.id == id && p.enabled);
        if let Some(rule) = self
            .routing_rules
            .iter()
            .find(|r| r.context.is_none() && r.matches(band, mode) && usable(r.radio))
        {
            // A matched rule is authoritative — INCLUDING "the rule points at the radio we are
            // already on", which resolves to None (stay put) rather than falling through to a
            // broader tier that would then move us off it.
            return (rule.radio != self.active_radio).then_some(rule.radio);
        }
        self.radio_for_band(band).or_else(|| {
            self.default_radio
                .filter(|id| usable(*id) && *id != self.active_radio)
        })
    }

    /// [`Self::route_radio`] for a SATELLITE-originated tune (a transponder pick): identical,
    /// plus one tier ABOVE the mode rules — the rules designated [`RouteContext::Satellite`],
    /// first-match-wins among themselves. List position never lets a mode rule outrank a
    /// satellite rule here: the designation is the operator saying "satellite work goes HERE",
    /// which must beat their terrestrial FM & APRS rule for a packet bird. With no satellite
    /// rule the tune falls through to exactly the terrestrial decision, so a station that
    /// never writes the designation is unchanged.
    pub fn route_radio_satellite(&self, band: &str, mode: RouteMode) -> Option<u32> {
        let usable = |id: u32| self.radios.iter().any(|p| p.id == id && p.enabled);
        if let Some(rule) = self.routing_rules.iter().find(|r| {
            r.context == Some(RouteContext::Satellite) && r.matches(band, mode) && usable(r.radio)
        }) {
            // Authoritative exactly like the mode tier — INCLUDING "the rule names the radio
            // we are already on", which is STAY PUT, never a fall-through to a LOWER tier
            // here. (One seam up, the engine's unreachable-rig last resort cannot tell this
            // stay-put `None` from "no answer" — see `sat_tune_nominal`.)
            return (rule.radio != self.active_radio).then_some(rule.radio);
        }
        self.route_radio(band, mode)
    }

    /// [`Self::route_radio_satellite`] for a downlink this app cannot NAME a band for — QO-100 at
    /// 10.489 GHz, the IC-905 microwave birds. Same tier ORDER (satellite-designated rules above
    /// plain mode rules, first-match-wins within each), asking only the tiers whose answer never
    /// read a band:
    ///
    /// - A rule with an empty band selector is consulted, because it answers identically for
    ///   every band and therefore for no band. The operator wrote "satellite work goes HERE" and
    ///   discarding that is not caution, it is dropping an explicit answer on the floor.
    /// - A rule that NAMES bands cannot match: nothing shows an unnameable dial is in its list.
    /// - [`Self::radio_for_band`] is skipped — it ranks rigs BY band coverage, and an empty
    ///   `bands` list ranks as catch-all, so it would answer "the rig that owns everything else"
    ///   for a dial it knows nothing about. The satellite path's own coverage fallback
    ///   (`Engine::sat_fallback_radio`) covers that case honestly, because it asks the rig's RX
    ///   RANGES about a FREQUENCY, which works perfectly at 10 GHz.
    /// - `default_radio` is skipped for the same reason, one tier weaker: "use this rig when no
    ///   band has an owner" is a band-coverage answer, and it is most often the HF rig.
    pub fn route_radio_bandless(&self, mode: RouteMode) -> Option<u32> {
        let usable = |id: u32| self.radios.iter().any(|p| p.id == id && p.enabled);
        let designated = |ctx: Option<RouteContext>| {
            self.routing_rules
                .iter()
                .find(|r| r.context == ctx && r.matches_bandless(mode) && usable(r.radio))
        };
        let rule = designated(Some(RouteContext::Satellite)).or_else(|| designated(None))?;
        // Authoritative exactly as in the band tiers, stay-put included.
        (rule.radio != self.active_radio).then_some(rule.radio)
    }

    /// Drop routing state that points at a radio which no longer EXISTS, so a removed radio can
    /// never leave a rule that silently never fires — or worse, a `default_radio` aimed at a gone
    /// rig. A merely DISABLED radio keeps its rules (unplugging a rig for the afternoon must not
    /// delete the routing table; `route_radio` skips a disabled target at resolve time instead).
    /// Idempotent; called on load, on every save, and after `remove_radio`.
    ///
    /// ALSO prunes `sat_uplink_radios`, and for a harder reason than the rules:
    /// `add_radio_profile` allocates max(id)+1 over SURVIVING profiles, so a
    /// freed id is REUSED — a consent entry that outlived its radio would hand
    /// the next rig added a Main/Sub uplink confirmation it never got, and the
    /// engine would drive its transmit VFO on it. A stale rule merely never
    /// fires; a stale consent transmits. (A transient pre-migration `None`
    /// has no ids to prune and confirms nothing; `load` resolves it.)
    pub fn ensure_routing_targets(&mut self) {
        let live: Vec<u32> = self.radios.iter().map(|p| p.id).collect();
        self.routing_rules.retain(|r| live.contains(&r.radio));
        if self.default_radio.is_some_and(|id| !live.contains(&id)) {
            self.default_radio = None;
        }
        if let Some(ids) = self.sat_uplink_radios.as_mut() {
            ids.retain(|id| live.contains(id));
        }
    }

    /// Append a new radio profile with a fresh (never-reused) id, a placeholder name, and CAT/rotator
    /// TCP ports guaranteed distinct from every existing radio's (two daemons can't bind one port).
    /// Returns the new profile's id. The operator then configures its CAT by switching to it (the
    /// flat rig form always edits the active radio). Does NOT change the active radio.
    pub fn add_radio_profile(&mut self) -> u32 {
        self.ensure_radio_profiles();
        let next_id = self.radios.iter().map(|p| p.id).max().unwrap_or(0) + 1;
        let mut used: Vec<u16> = self
            .radios
            .iter()
            .flat_map(|p| [p.rigctld_port, p.rotctld_port])
            .collect();
        if self.cat_broker {
            used.push(self.cat_broker_port);
        }
        let mut free_from = |start: u16| -> u16 {
            let mut port = start;
            while used.contains(&port) {
                port += 1;
            }
            used.push(port);
            port
        };
        let rigctld_port = free_from(4532);
        let rotctld_port = free_from(4533);
        // Walk to the first UNUSED "Radio N" rather than numbering by roster length. Length-based
        // numbering collides the moment a radio is removed from the middle (add A/B/C → remove
        // "Radio 2" → add → a SECOND "Radio 3"), and every conflict message the operator sees
        // (`serial_port_conflicts`, `shared_audio_device`) names radios, so duplicates make those
        // warnings ambiguous. Only reachable with three or more radios.
        let mut n = self.radios.len() + 1;
        let name = loop {
            let candidate = format!("Radio {n}");
            if !self.radios.iter().any(|p| p.name == candidate) {
                break candidate;
            }
            n += 1;
        };
        self.radios.push(RadioProfile {
            id: next_id,
            name,
            rigctld_port,
            rotctld_port,
            ..RadioProfile::default()
        });
        next_id
    }

    /// Auto-repair colliding daemon ports so every radio can run its OWN persistent rigctld/rotctld at
    /// the same time (true dual-radio needs two live daemons — a shared port would make the monitor
    /// connect through the active radio's daemon). Bumps any duplicate `rigctld_port`/`rotctld_port`
    /// (and any that clashes with the CAT broker) to the next free value, first-radio-wins. Idempotent;
    /// called on load. `add_radio_profile` already assigns distinct ports, so this only fixes older
    /// configs or hand-edited collisions.
    pub fn ensure_distinct_radio_ports(&mut self) {
        let broker = self.cat_broker.then_some(self.cat_broker_port);
        // Repair when ports COLLIDE, or when any profile has an INVALID (0) rigctld port. A lone 0
        // is technically "distinct" so `validate_radio_ports` alone wouldn't flag it, but connecting
        // to 127.0.0.1:0 fails on Windows with WSAEADDRNOTAVAIL ("the requested address is not valid
        // in its context", os error 10049) — so an older/imported config with a 0 port breaks CAT
        // for that one radio while its siblings work. Treat 0 like a collision and reassign it.
        let has_invalid_port = self.radios.iter().any(|p| p.rigctld_port == 0);
        if !has_invalid_port && validate_radio_ports(&self.radios, broker).is_ok() {
            return;
        }
        let mut used: Vec<u16> = broker.into_iter().collect();
        let free_from = |start: u16, used: &mut Vec<u16>| -> u16 {
            let mut port = start.max(1024);
            while used.contains(&port) {
                port = port.saturating_add(1);
            }
            used.push(port);
            port
        };
        for p in self.radios.iter_mut() {
            if p.rigctld_port == 0 || used.contains(&p.rigctld_port) {
                p.rigctld_port = free_from(4532, &mut used);
            } else {
                used.push(p.rigctld_port);
            }
            // Only radios that actually have a rotator claim a rotctld port.
            if p.rotator_model > 0 || !p.rotator_host.is_empty() {
                if p.rotctld_port == 0 || used.contains(&p.rotctld_port) {
                    p.rotctld_port = free_from(4533, &mut used);
                } else {
                    used.push(p.rotctld_port);
                }
            }
        }
    }

    /// Remove a radio profile by id. Refuses to remove the active radio or the last remaining one
    /// (there must always be ≥1, and the active must exist). Returns whether it removed anything.
    pub fn remove_radio_profile(&mut self, id: u32) -> bool {
        if id == self.active_radio || self.radios.len() <= 1 {
            return false;
        }
        let before = self.radios.len();
        self.radios.retain(|p| p.id != id);
        // A removed radio must not leave routing rules (or a default) aimed at it — otherwise the
        // rule silently never fires and its band+mode falls through to a tier the operator can't see.
        self.ensure_routing_targets();
        self.radios.len() != before
    }

    /// Copy the ACTIVE profile's rig/audio fields INTO the flat mirror, so every existing consumer
    /// (Transport::from_settings, sync_rotctld, rig_mode…) reads the active radio unchanged. No-op
    /// when the flat fields already equal the active profile (the single-radio case). Called on load.
    pub fn sync_flat_from_active(&mut self) {
        let Some(p) = self.active_profile().cloned() else {
            return;
        };
        self.ptt_method = p.ptt_method;
        self.rig_model = p.rig_model;
        self.rig_model_name = p.rig_model_name;
        self.serial_port = p.serial_port;
        self.ptt_serial_port = p.ptt_serial_port;
        self.baud = p.baud;
        self.rig_conn = p.rig_conn;
        self.rig_addr = p.rig_addr;
        self.omnirig_slot = p.omnirig_slot;
        self.rigctld_port = p.rigctld_port;
        self.icom_native_cat = p.icom_native_cat;
        self.data_modes_plain_ssb = p.data_modes_plain_ssb;
        self.audio_in = p.audio_in;
        self.audio_out = p.audio_out;
        self.tx_level = p.tx_level;
        self.rx_gain = p.rx_gain;
        self.rotator_model = p.rotator_model;
        self.rotator_port = p.rotator_port;
        self.amp_model = p.amp_model;
        self.amp_port = p.amp_port;
        self.rotator_baud = p.rotator_baud;
        self.rotator_host = p.rotator_host;
        // The Flex three ride the SAME mirror as every other rig field, so every existing consumer
        // (`reconcile_spectrum_source` reads `settings().flex_radio_ip`) keeps reading the ACTIVE
        // radio unchanged while the stored truth is per-radio.
        self.flex_radio_ip = p.flex_radio_ip;
        self.flex_native_pan = p.flex_native_pan;
        self.flex_native_audio = p.flex_native_audio;
    }

    /// Copy the flat mirror back INTO the active profile — so edits made through today's flat rig/
    /// audio form persist into the active radio's profile. Called before save. Keeps the two
    /// representations from diverging (the single writer, per the mirror invariant).
    pub fn sync_active_from_flat(&mut self) {
        self.ensure_radio_profiles();
        let active = self.active_radio;
        // Snapshot flat fields first (avoid borrowing self while mutating the profile).
        let (
            ptt_method,
            rig_model,
            rig_model_name,
            serial_port,
            ptt_serial_port,
            baud,
            rig_conn,
            rig_addr,
            omnirig_slot,
            rigctld_port,
            icom_native_cat,
            data_modes_plain_ssb,
            audio_in,
            audio_out,
            tx_level,
            rx_gain,
            rotator_model,
            rotator_port,
            rotator_baud,
            rotator_host,
            amp_model,
            amp_port,
            flex_radio_ip,
            flex_native_pan,
            flex_native_audio,
        ) = (
            self.ptt_method.clone(),
            self.rig_model,
            self.rig_model_name.clone(),
            self.serial_port.clone(),
            self.ptt_serial_port.clone(),
            self.baud,
            self.rig_conn.clone(),
            self.rig_addr.clone(),
            self.omnirig_slot,
            self.rigctld_port,
            self.icom_native_cat,
            self.data_modes_plain_ssb,
            self.audio_in.clone(),
            self.audio_out.clone(),
            self.tx_level,
            self.rx_gain,
            self.rotator_model,
            self.rotator_port.clone(),
            self.rotator_baud,
            self.rotator_host.clone(),
            self.amp_model.clone(),
            self.amp_port.clone(),
            self.flex_radio_ip.clone(),
            self.flex_native_pan,
            self.flex_native_audio,
        );
        if let Some(p) = self.radios.iter_mut().find(|p| p.id == active) {
            p.ptt_method = ptt_method;
            p.rig_model = rig_model;
            p.rig_model_name = rig_model_name;
            p.serial_port = serial_port;
            p.ptt_serial_port = ptt_serial_port;
            p.baud = baud;
            p.rig_conn = rig_conn;
            p.rig_addr = rig_addr;
            p.omnirig_slot = omnirig_slot;
            p.rigctld_port = rigctld_port;
            p.icom_native_cat = icom_native_cat;
            p.data_modes_plain_ssb = data_modes_plain_ssb;
            p.audio_in = audio_in;
            p.audio_out = audio_out;
            p.tx_level = tx_level;
            p.rx_gain = rx_gain;
            p.rotator_model = rotator_model;
            p.rotator_port = rotator_port;
            p.rotator_baud = rotator_baud;
            p.rotator_host = rotator_host;
            p.amp_model = amp_model;
            p.amp_port = amp_port;
            p.flex_radio_ip = flex_radio_ip;
            p.flex_native_pan = flex_native_pan;
            p.flex_native_audio = flex_native_audio;
        }
    }

    /// Load settings from `path`. A missing file (first run) returns defaults. A
    /// present-but-CORRUPT file is NOT silently defaulted — that would be
    /// indistinguishable from a first run, wiping the operator's identity/rig config
    /// and resetting `license_class` to `Open` (re-opening TX privileges). Instead the
    /// bad file is set aside as a sibling `.corrupt` file for recovery, then defaults
    /// apply.
    pub fn load(path: &Path) -> Self {
        // The RETIRED `satDoppler` opt-in, read from the raw file before serde
        // discards it as an unknown key. It is dead as a switch (the downlink
        // no longer asks), but it is the one signal that separates a pre-0.26
        // mapping that actually DROVE the uplink (master on) from one that was
        // picked and never enabled (master off — the pass rail rendered the
        // mapping select unconditionally, so that state is one click away).
        // Consumed by the consent migration below, and by nothing else — in
        // particular it must never seed `sat_doppler_off`, or the polarity
        // flip would reach nobody.
        let mut legacy_sat_doppler: Option<bool> = None;
        let mut s: Settings = match std::fs::read_to_string(path) {
            // Missing file: a normal first run.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
            // Present but UNREADABLE (permissions, an AV/backup tool's exclusive
            // lock): NOT a first run — set the intact file aside so a later save()
            // of the defaults can't clobber it (best-effort; a held lock can make
            // the rename fail too, but then the file survives in place).
            Err(e) => {
                eprintln!(
                    "tempo: cannot read {} ({e}); setting it aside as .corrupt and starting from defaults",
                    path.display()
                );
                let _ = std::fs::rename(path, path.with_extension("json.corrupt"));
                Settings::default()
            }
            Ok(text) => match serde_json::from_str(&text) {
                Ok(s) => {
                    #[derive(Deserialize)]
                    struct LegacyDoppler {
                        #[serde(rename = "satDoppler")]
                        sat_doppler: Option<bool>,
                    }
                    legacy_sat_doppler = serde_json::from_str::<LegacyDoppler>(&text)
                        .ok()
                        .and_then(|l| l.sat_doppler);
                    s
                }
                Err(e) => {
                    // Corrupt file: preserve the evidence (best-effort — defaults are
                    // still the right fallback even if the rename fails).
                    eprintln!(
                        "tempo: {} is corrupt ({e}); setting it aside as .corrupt and starting from defaults",
                        path.display()
                    );
                    let _ = std::fs::rename(path, path.with_extension("json.corrupt"));
                    Settings::default()
                }
            },
        };
        // One-time migration: drop the known-bad free-text "CQ"/"CQ CQ" macro chips that
        // persisted from older defaults. A CQ now goes through the structured Call-CQ
        // button; a free-text "CQ CQ" chip went out as a chunked, gridless "DE <CALL>
        // A12CQ CQ" — and broadcasts now auto-arm TX, making that chip a one-click
        // malformed-CQ footgun. Custom macros are preserved.
        s.macros
            .band
            .retain(|m| !matches!(m.trim().to_uppercase().as_str(), "CQ" | "CQ CQ"));
        s.macros
            .chat
            .retain(|m| !matches!(m.trim().to_uppercase().as_str(), "CQ" | "CQ CQ"));
        // Migration: fold the legacy single CW F-key list (`macros.cw`) into named CW
        // macro PROFILES — an old settings.json comes back as one "Default" profile with
        // the same macros. Idempotent, and clamps the active-profile index in range.
        s.macros.migrate_cw_profiles();
        // Migration: cluster_host used to BE the RBN endpoint (digital-only, port 7001),
        // which is why CW/Phone needs never appeared; a later build wrongly defaulted it to
        // NC7J's SKIMMER port (dxc.nc7j.com:7373), which just duplicates the RBN we pull.
        // RBN CW+digital are now wired automatically, so cluster_host is the HUMAN node for
        // SSB/phone — reset either bad value to the VE7CC-1 default so phone spots flow.
        let legacy_rbn_host =
            s.cluster_host.contains("reversebeacon.net") || s.cluster_host == "dxc.nc7j.com:7373";
        if legacy_rbn_host {
            s.cluster_host = "ve7cc.net:23".to_string();
            // That signature IS a pre-multi-cluster config: "cluster" pointed at an RBN/skimmer
            // port, never a human node, so the operator never had a phone source — and the
            // subsystem commonly persisted DISABLED from an older default, so even after fixing
            // the host no spots flow (which defeats this migration's whole purpose). Enable it,
            // and seed BOTH default human nodes (ve7cc + the wa9pie:8000 fallback for networks
            // that block telnet port 23) UNLESS the operator already has a real (non-RBN) node
            // configured — then just enable and keep theirs.
            s.cluster_enabled = true;
            let has_human_host = s
                .cluster_hosts
                .iter()
                .any(|h| !h.trim().is_empty() && !h.contains("reversebeacon.net"));
            if !has_human_host {
                s.cluster_hosts = vec![
                    "ve7cc.net:23".to_string(),
                    "dxc.wa9pie.net:8000".to_string(),
                ];
            }
        }
        // Migration: `cluster_hosts` (the multi-cluster aggregator) is newer than the single
        // `cluster_host`. An OLD config has no `clusterHosts` key → the field default leaves it
        // empty → seed it from the (now-migrated) single host so an upgrading operator keeps
        // their node. Then sanitize the list: trim, drop blanks + RBN endpoints (auto-wired,
        // never human/phone), and dedup case-insensitively while preserving order.
        if s.cluster_hosts.is_empty() && !s.cluster_host.trim().is_empty() {
            s.cluster_hosts = vec![s.cluster_host.clone()];
        }
        let mut seen = std::collections::HashSet::new();
        s.cluster_hosts = s
            .cluster_hosts
            .iter()
            .map(|h| h.trim().to_string())
            .filter(|h| {
                !h.is_empty()
                    && !h.contains("reversebeacon.net")
                    && seen.insert(h.to_ascii_lowercase())
            })
            .collect();
        // Migration: SATELLITE UPLINK CONSENT (0.26). A file with no
        // `satUplinkRadios` key predates per-radio confirmation. Its mapping
        // was only ever a live uplink grant when the retired `satDoppler`
        // master switch was ALSO on — with the master off (or absent), the
        // pair drove nothing, and promoting it now would put an unconfirmed
        // uplink on the transmit VFO the first time the operator launches this
        // build. So:
        //   * live pair  (satDoppler == true AND a mapping that drives the
        //     uplink) → the real station-wide grant: MATERIALIZED below, once
        //     the roster exists, as concrete consent for every radio in the
        //     file. The operator keeps their uplink and is never re-asked on
        //     the radios they had; a radio added later asks like any second
        //     radio. Materializing (rather than keeping the in-memory `None`
        //     sentinel) is what lets the grant survive save+relaunch: save()
        //     wrote `None` as `"satUplinkRadios": null` while dropping the
        //     retired `satDoppler` key, so the NEXT load saw no legacy
        //     evidence and normalized the grant away after one session.
        //   * anything else → `Some(vec![])`: the mapping is kept, the
        //     downlink corrects, and the uplink waits for the per-radio
        //     confirmation like a fresh install's would.
        // A file that HAS a `satUplinkRadios` key wrote its own consent under
        // the new rules; the legacy key means nothing beside it. (A 0.26 file
        // that carries `null` — there was a window in which save() could
        // write one — lands here too, and normalizes to ask: fail safe.)
        let legacy_live = s.sat_uplink_radios.is_none()
            && legacy_sat_doppler == Some(true)
            && s.sat_vfo_map.drives_uplink();
        if s.sat_uplink_radios.is_none() && !legacy_live {
            s.sat_uplink_radios = Some(Vec::new());
        }
        // Multi-radio: migrate an older (flat-only) settings file to a single radio profile, then
        // mirror the active profile into the flat fields so every existing consumer reads unchanged.
        s.ensure_radio_profiles();
        // The live legacy pair, materialized against the roster that now
        // exists: the station-wide grant becomes per-radio consent for the
        // radios present at upgrade — durable across any number of
        // save/load cycles, and prunable by `ensure_routing_targets` exactly
        // like consent written under the new rules. After this point
        // `sat_uplink_radios` is always `Some`.
        if legacy_live {
            s.sat_uplink_radios = Some(s.radios.iter().map(|p| p.id).collect());
        }
        s.ensure_distinct_radio_ports(); // two live daemons (dual-radio) need distinct ports
        s.ensure_routing_targets(); // drop rules aimed at radios this config no longer has

        // Migration: the FLEX THREE became per-radio on 2026-08-18 (Flex audit wave-1 #30/#46).
        // A file written before that carries them ONLY on the flat `Settings`; its profiles have
        // no such keys, so serde defaults them to ""/false — and the `sync_flat_from_active` at
        // the end of this function would then copy those defaults OVER the operator's real
        // address, losing it on the first launch of this build. Copy the flat value into the
        // ACTIVE radio's profile first, which is exactly what it always described.
        //
        // MUST run after `ensure_radio_profiles` (the profile has to exist) and BEFORE
        // `sync_flat_from_active` (which is the thing that would clobber it). Idempotent: after
        // one save the profile carries the value and the flat mirror equals it, so a later load
        // either finds nothing to copy or copies the identical value. Guarded on the profile being
        // EMPTY/off so it can never resurrect a setting the operator deliberately cleared — a
        // cleared field is written to both representations by `save`'s `sync_active_from_flat`.
        let (flat_ip, flat_pan, flat_audio) = (
            s.flex_radio_ip.clone(),
            s.flex_native_pan,
            s.flex_native_audio,
        );
        let active = s.active_radio;
        if let Some(p) = s.radios.iter_mut().find(|p| p.id == active) {
            if p.flex_radio_ip.trim().is_empty() && !flat_ip.trim().is_empty() {
                p.flex_radio_ip = flat_ip;
            }
            p.flex_native_pan |= flat_pan;
            p.flex_native_audio |= flat_audio;
        }
        s.sync_flat_from_active();
        s
    }

    /// Persist settings to `path` (creating parent directories). Writes a sibling
    /// `.tmp` file, fsyncs it, then renames it into place (the [`Logbook::save`]
    /// pattern), so a crash / power loss mid-write can't truncate `settings.json`. A
    /// torn write of the live file would silently collapse to [`Settings::default`] on
    /// the next load — blanking the operator's identity/rig config and resetting
    /// `license_class` to `Open`, which drops the Part 97 TX lockout. The rename makes
    /// a save all-or-nothing; the fsync stops a filesystem from committing the rename
    /// before the tmp's data blocks on power loss (which would publish a torn file).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Persist a copy whose active radio profile reflects any edits made through the flat rig/
        // audio form (the mirror invariant). `self` is left untouched.
        let mut to_save = self.clone();
        to_save.sync_active_from_flat();
        let json = serde_json::to_string_pretty(&to_save).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        let mut f = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut f, json.as_bytes())?;
        f.sync_all()?; // data on disk BEFORE the rename publishes it
        drop(f);
        // No pre-remove of `path`: rename replaces it atomically on Unix and Windows
        // (MOVEFILE_REPLACE_EXISTING); a remove-first would open a no-file crash window.
        std::fs::rename(&tmp, path)
    }

    /// Dial frequency in Hz (for the rig / PSK Reporter).
    pub fn dial_hz(&self) -> u64 {
        (self.dial_mhz * 1_000_000.0).round() as u64
    }

    /// The CAT mode to command the rig for the current section (the per-section policy):
    /// Phone forces USB/LSB by band, CW forces CW (or USB/LSB for a soundcard keyer),
    /// and Digital forces the DATA submode (Hamlib `PKTUSB`/`PKTLSB` → Yaesu DATA-U /
    /// Icom USB-D / Kenwood DATA) so FT8/FT4 sits in data mode. Returns "" — meaning
    /// "send NO `M` command, obey the rig" — only for Digital when the operator has
    /// turned [`set_rig_mode`](Self::set_rig_mode) OFF (rigs without a DATA submode).
    /// Standard FM repeater offset MAGNITUDE (Hz) for the current dial frequency — the
    /// band convention (10 m 100 k, 6 m 1 M, 2 m 600 k, 1.25 m 1.6 M, 70 cm 5 M, 23 cm
    /// 12 M). The shift DIRECTION comes from [`Self::rptr_shift`]; 0 below 28 MHz (no FM
    /// repeaters there).
    pub fn rptr_offset_hz(&self) -> i64 {
        // An explicit override (the Program section's tune-now for odd-split
        // machines) beats the band convention; 0 = no override.
        if self.rptr_offset_override_hz > 0 {
            return self.rptr_offset_override_hz;
        }
        let f = self.dial_mhz;
        if f >= 1240.0 {
            12_000_000
        } else if f >= 420.0 {
            5_000_000
        } else if f >= 222.0 {
            1_600_000
        } else if f >= 144.0 {
            600_000
        } else if f >= 50.0 {
            1_000_000
        } else if f >= 28.0 {
            100_000
        } else {
            0
        }
    }

    /// The RF-power ceiling (0.0–1.0) for the CURRENT operating mode, or 1.0 (uncapped) when the
    /// operator has set no per-mode cap. RTTY shares the digital cap (both are high-duty data
    /// modes). The safety guarantee lives on top of this: `set_rf_power` clamps to it and a mode
    /// change re-clamps to it.
    pub fn rf_power_ceiling(&self) -> f32 {
        let cap = match self.operating_mode {
            OperatingMode::Phone => self.max_power_phone,
            OperatingMode::Cw => self.max_power_cw,
            // The keyboard modes share the digital cap: PSK31 keys continuously
            // at high duty exactly as RTTY does.
            OperatingMode::Digital | OperatingMode::Rtty | OperatingMode::Keyboard => {
                self.max_power_digital
            }
        };
        cap.map(|c| c.clamp(0.0, 1.0)).unwrap_or(1.0)
    }

    /// The ceiling for a HIGH-DUTY transmission, whatever operating mode is nominally selected.
    ///
    /// ⚠️ THIS EXISTS BECAUSE SSTV HAS NO `OperatingMode` OF ITS OWN. It rides Phone (see the
    /// `RouteMode` notes), so [`Self::rf_power_ceiling`] handed it `max_power_phone` — the SSB
    /// cap — while an SSTV frame keys CONTINUOUSLY for up to 290 s at ~100% duty. That is the
    /// duty shape `max_power_digital` exists for, and RTTY (the same shape) already takes it.
    /// The one mode that keys hardest for longest was capped as though it were speech.
    ///
    /// The result is the LOWER of the digital cap and the selected mode's own cap, never the
    /// digital one alone: an operator who set digital ABOVE phone must not have SSTV lift their
    /// power past what the phone cap allows while the rig sits in a phone mode. Enforcement here
    /// may only ever LOWER power — which is what makes it safe to apply without bench proof.
    pub fn rf_power_ceiling_high_duty(&self) -> f32 {
        let digital = self
            .max_power_digital
            .map(|c| c.clamp(0.0, 1.0))
            .unwrap_or(1.0);
        digital.min(self.rf_power_ceiling())
    }

    /// The ceiling for an AM transmission, whatever the phone cap says.
    ///
    /// ⚠️ AM IS NOT SSB, AND THE SSB CAP LETS IT FLAT-TOP. A rig making 100 W PEP on SSB makes
    /// about 25 W of carrier on AM: the power is in a carrier that is always present plus two
    /// sidebands, and PEP is reached on modulation peaks. Run the SSB drive into AM and the peaks
    /// clip. Most manuals say a quarter, which is the [`default_max_power_am`] default.
    ///
    /// The LOWER of the AM cap and the selected mode's own cap, never the AM one alone — an
    /// operator who set AM above phone must not have AM lift their power past what the phone cap
    /// allows. Identical in shape to [`Self::rf_power_ceiling_high_duty`], and for the identical
    /// reason: enforcement here may only ever LOWER power, which is what makes it safe to apply
    /// without bench proof.
    pub fn rf_power_ceiling_am(&self) -> f32 {
        let am = self.max_power_am.map(|c| c.clamp(0.0, 1.0)).unwrap_or(1.0);
        am.min(self.rf_power_ceiling())
    }

    pub fn rig_mode(&self) -> String {
        // FM is BAND-GATED, and that gate is a bug fix, not a preference. `phone_mode`
        // is one station-wide field: nothing resets it when the operator changes band or
        // switches radios (`sync_flat_from_active` does not touch it, and RadioProfile
        // has no equivalent). So without the gate, working an FM repeater and then tuning
        // to 20 m phone commands the rig into FM on 20 m — a wideband signal on a band
        // whose plan has no room for it. Reachable on a SINGLE radio: an FT-991A or
        // IC-7100 covers both, so that is an ordinary evening, not a corner case.
        //
        // 29.0 MHz is the floor because 29.0-29.7 is the 10 m FM segment; below it FM is
        // not used. Above, every band an FM-capable rig reaches (10 m, 6 m, 2 m, 70 cm
        // and up) is FM territory. Falling back to the band's sideband — rather than
        // refusing or holding FM — matches what the operator would have set by hand, and
        // is the same "force the correct thing for this band" rule the sideband
        // convention below already applies. Pinned by
        // `fm_does_not_follow_the_operator_down_to_hf`.
        if self.operating_mode == OperatingMode::Phone
            && self.phone_mode.eq_ignore_ascii_case("fm")
            && self.dial_mhz >= 29.0
        {
            return "FM".to_string(); // FM voice (10 m FM segment, VHF/UHF simplex + repeaters)
        }
        // WHICH SIDE the section policy below works on. Two different questions,
        // deliberately not unified: Digital's side is a property of the tuned CHANNEL
        // (FT8 on 40 m is USB-side — the band convention would say LSB and be wrong),
        // while Phone and CW follow the hard band convention (LSB below 10 MHz).
        let lsb = match self.operating_mode {
            OperatingMode::Digital => self.sideband.trim().eq_ignore_ascii_case("LSB"),
            // Keyboard modes are ALWAYS USB-side: the PSK31 convention is USB on
            // every band (80/40 m included — unlike RTTY's LSB and phone's
            // below-10-MHz rule), so the band fallthrough would be wrong here.
            OperatingMode::Keyboard => false,
            _ => self.dial_mhz < 10.0,
        };
        self.rig_mode_on_sideband(lsb)
    }

    /// [`Self::rig_mode`]'s per-section policy with the SIDEBAND SIDE supplied
    /// rather than derived — the form the section needs (plain SSB, a DATA
    /// submode, the rig's CW or RTTY mode), on the side the caller names.
    ///
    /// It exists for the satellite path, which is the one caller that knows the
    /// side better than any band convention does: the transponder's own record
    /// declares it, and an inverting bird's uplink is derived by mirroring it.
    /// Splitting it out rather than duplicating it is deliberate — two copies of
    /// the section policy is exactly how the commanded mode and the routing
    /// class learn to disagree about one bird.
    ///
    /// NO FM arm: this answers for the LINEAR path only. FM is a class, not a
    /// side, and its callers gate on it before they get here.
    pub(crate) fn rig_mode_on_sideband(&self, lsb: bool) -> String {
        match self.operating_mode {
            // CW: force CW for the CAT keyer; for the soundcard keyer the rig must be in a
            // DATA submode so it transmits the keyed audio tone (band-aware: LSB <10 MHz).
            OperatingMode::Cw => match self.cw_keyer {
                // CAT, WinKeyer, and the serial keyline all key the rig in CW mode (the rig
                // shapes the envelope); only the soundcard keyer keys an audio tone, so that
                // one needs the rig on the SSB side — as a DATA submode, see its arm below.
                //
                // BAND-AWARE CW SIDEBAND (operator 2026-07-24, "40 m sets CW-U, should be
                // CW-L"): same 10 MHz convention as the sideband rules below — CW-L
                // (Hamlib `CWR`, Icom 0x07, Yaesu CW-L) on 160/80/40 m, CW-U (plain `CW`)
                // at 30 m and up. The waterfall/zero-beat math already signs CWR as
                // LSB-side, and the mode-apply helpers treat CWR exactly like CW.
                CwKeyerBackend::Cat | CwKeyerBackend::WinKeyer | CwKeyerBackend::Serial => {
                    if lsb { "CWR" } else { "CW" }.to_string()
                }
                // SOUNDCARD: a DATA submode, exactly like every other soundcard-audio path
                // here (Digital, Keyboard, RTTY-AFSK, and SSTV's `PKTFM`) — and for their
                // reason, which this arm was the only one not to apply: on a normally-wired
                // rig plain SSB takes TX audio from the MIC JACK, so a keyed tone played into
                // the USB codec never reaches the modulator and the over radiates ZERO RF.
                // That is the "keys but no audio" field report (Yaesu FTX-1, 2026-08-28).
                //
                // The SIDE is unchanged — the CW convention above still picks it — so this
                // moves USB→PKTUSB and LSB→PKTLSB and nothing else, and it inherits the
                // `data_modes_plain_ssb` opt-out that mic-jack interfaces need.
                //
                // ⚠️ NEEDS-BENCH (no rig on this box). What is proven here is the MODE WORD
                // Nexus commands. What is NOT proven is a radio putting RF out in DATA-U
                // where plain USB put none out — that is one key-down on a real rig with the
                // power meter watched, and it is the whole point of the change.
                CwKeyerBackend::Soundcard => {
                    self.plain_ssb_if_configured(if lsb { "PKTLSB" } else { "PKTUSB" })
                }
            },
            // Phone: force the correct sideband — the hard convention is LSB below
            // 10 MHz (160/80/40 m), USB at 30 m and up. (AM comes later as an explicit
            // choice in the Phone cockpit.) The FM arm lives in `rig_mode`, above the
            // side derivation, because FM is not a side.
            OperatingMode::Phone => if lsb { "LSB" } else { "USB" }.to_string(),
            // RTTY: the mode follows the keying backend, like CW's keyer split. True FSK
            // needs the rig in its RTTY mode (Hamlib "RTTY" → Yaesu RTTY-L etc.) so the FSK
            // input keys the shift AND the rig's narrow RTTY filters unlock. AFSK is an audio
            // tone pair the SOUNDCARD plays, so — exactly like FT8 — it needs a DATA submode
            // (PKTLSB → Yaesu DATA-L / Icom LSB-D / Kenwood DATA, rig-agnostically via Hamlib)
            // to route the USB codec to the modulator: plain LSB takes TX audio from the MIC
            // on the common Icom / default-Yaesu setup and radiates ZERO RF ("red light, no
            // signal"). LSB-side keeps the RTTY convention (mark = lower audio = higher RF);
            // `rtty_reverse` flips the TONES, not the sideband. A rig with no DATA submode is
            // handled by the loop's bounded set_mode retry (tries, is rejected, gives up).
            // FSK is deliberately NOT run through `plain_ssb_if_configured`: it commands the
            // rig's own RTTY mode, which is what unlocks the narrow RTTY filters and keys the
            // shift. "Plain SSB" has no meaning there — only the AFSK (soundcard) path, which
            // is a DATA submode for the same reason FT8 is, can be switched.
            OperatingMode::Rtty => {
                if self.rtty_backend.eq_ignore_ascii_case("fsk") {
                    "RTTY".to_string()
                } else {
                    self.plain_ssb_if_configured("PKTLSB")
                }
            }
            // Digital: force the DATA submode (PKTUSB/PKTLSB → Yaesu DATA-U / Icom USB-D
            // / Kenwood DATA), USB-side by default — UNCONDITIONALLY, like Phone forces
            // SSB and CW forces CW. (No opt-out: FT8/FT4 are a data mode, and a rig
            // without a DATA submode is handled by the radio loop's bounded set_mode
            // retry — it tries once, the rig rejects it, and it gives up, rather than
            // leaving the rig stuck in the previous section's SSB/CW mode.) Any non-LSB
            // sideband (incl. empty/garbled) maps to the USB-side PKTUSB that FT8 uses.
            OperatingMode::Digital => {
                self.plain_ssb_if_configured(if lsb { "PKTLSB" } else { "PKTUSB" })
            }
            // Keyboard modes (PSK31…): soundcard audio through a DATA submode,
            // exactly like Digital — plain SSB on a normally-wired rig radiates
            // ZERO RF, hence the same PKT/data forcing and the same mic-jack
            // opt-out. `rig_mode` derives the side as always-USB (the PSK31
            // convention); the side stays a parameter here for the one caller
            // that can know better (a transponder's declared side).
            OperatingMode::Keyboard => {
                self.plain_ssb_if_configured(if lsb { "PKTLSB" } else { "PKTUSB" })
            }
        }
    }

    /// Map a DATA submode to its plain-SSB equivalent when this radio is configured for
    /// [`RadioProfile::data_modes_plain_ssb`]. Identity otherwise, and identity for anything
    /// that is not a DATA submode — notably RTTY-FSK, which commands the rig's own `RTTY`
    /// mode and has no SSB equivalent to fall back to.
    ///
    /// ⚠️ Read the warning on `RadioProfile::data_modes_plain_ssb` before touching this: for a
    /// normally-wired rig, plain SSB on a soundcard mode transmits NO RF. This exists for
    /// mic-jack interfaces, and it is off by default for everyone else.
    pub(crate) fn plain_ssb_if_configured(&self, m: &str) -> String {
        if !self.data_modes_plain_ssb {
            return m.to_string();
        }
        match m {
            "PKTUSB" => "USB".to_string(),
            "PKTLSB" => "LSB".to_string(),
            // The FM data submode an SSTV image on an FM channel is sent in
            // (`Engine::fm_mode_word`). Same opt-out for the same reason: on a mic-jack
            // interface the picture has to go in the mic input, which is plain FM.
            "PKTFM" => "FM".to_string(),
            other => other.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::field_reassign_with_default)]
    use super::*;

    /// REGRESSION (shipped 0.18.0, found 2026-07-26): `apply_to` assigned 19 sibling fields and
    /// silently skipped `ptt_serial_port`, so editing the dedicated RTS/DTR keying port of a
    /// radio you were NOT operating saved nothing — the setting round-tripped through the UI and
    /// vanished. Same per-radio-Edit routing class as the 0.17.12 dual-radio clobber.
    ///
    /// This is deliberately a TOTALITY test driven by serde rather than a hand-written field
    /// list: a hand-written list is exactly what failed, because adding a field to the patch
    /// does not force anyone to remember `apply_to`. Every field the patch carries must land on
    /// the profile, so the NEXT field added cannot be dropped the same way. Both structs are
    /// `rename_all = "camelCase"`, so the keys line up by construction.
    #[test]
    fn radio_profile_patch_assigns_every_field_it_carries() {
        // Values chosen to differ from RadioProfile::default() in every field, so "was it
        // assigned?" cannot be confused with "did it already happen to equal the default?".
        let patch = RadioProfilePatch {
            ptt_method: "rts".into(),
            rig_model: 3085,
            rig_model_name: "Icom IC-7300".into(),
            serial_port: "COM7".into(),
            ptt_serial_port: "COM9".into(),
            baud: 115_200,
            rig_conn: "network".into(),
            rig_addr: "192.0.2.10:4992".into(),
            omnirig_slot: 2,
            rigctld_port: 4533,
            icom_native_cat: true,
            icom_data_mode: 2,
            data_modes_plain_ssb: true,
            audio_in: "USB Audio CODEC #2".into(),
            audio_out: "USB Audio CODEC #2 out".into(),
            tx_level: 0.42,
            rx_gain: 1.75,
            rotator_model: 202,
            rotator_port: "COM11".into(),
            rotator_baud: 19_200,
            amp_model: String::new(),
            amp_port: String::new(),
            amp_follow_band: false,
            rotator_host: "192.0.2.20".into(),
            rotctld_port: 4534,
            native_scope: "civ".into(),
            flex_radio_ip: "192.0.2.50".into(),
            flex_native_pan: true,
            flex_native_audio: true,
        };

        let sent = serde_json::to_value(&patch).expect("patch serializes");
        let mut profile = RadioProfile::default();
        patch.apply_to(&mut profile);
        let landed = serde_json::to_value(&profile).expect("profile serializes");

        let sent = sent.as_object().expect("patch is an object");
        let landed = landed.as_object().expect("profile is an object");

        let dropped: Vec<&str> = sent
            .iter()
            .filter(|(k, v)| landed.get(*k) != Some(*v))
            .map(|(k, _)| k.as_str())
            .collect();

        assert!(
            dropped.is_empty(),
            "apply_to dropped {} patch field(s): {dropped:?} — every field the patch carries \
             must be copied onto the profile, or a per-radio edit silently saves nothing",
            dropped.len()
        );
    }

    /// THE OTHER HALF, and the one that was missing when it mattered (2026-08-17 Flex audit,
    /// wave-1 #46/#30). The sibling above proves every field the patch CARRIES lands; it is
    /// silent about a per-radio field the patch does not carry at all — and that is exactly how
    /// `flexRadioIp` / `flexNativePan` / `flexNativeAudio` were lost: they were flat-only, the
    /// Settings per-radio Edit flow routes every save through `update_radio_profile(patch)`, and
    /// the patch enumerated 20 fields with none of the three. Save reported success and the
    /// operator's Flex address was gone.
    ///
    /// Computed from serde, not from a list, for the same reason as the sibling. The exclusions
    /// are the ones `RadioProfilePatch`'s own doc names — identity, band coverage and the
    /// `last_*` tune memory the radio loop owns — and adding a field to `RadioProfile` that is
    /// neither excluded nor in the patch fails here rather than in the field.
    #[test]
    fn editing_one_radios_amplifier_leaves_the_others_alone() {
        // The SO2R case, and the reason the amplifier is a PER-RADIO field rather than a station
        // one: two radios, an amplifier on each. The 2026-07-25 COM-port incident and the Flex
        // audit both found the same shape — a patch that omits a field silently blanks it on the
        // profile it touches, and the operator is told "saved".
        let mut s = Settings {
            radios: vec![
                RadioProfile {
                    id: 0,
                    amp_model: "spe".into(),
                    amp_port: "/dev/ttyUSB0".into(),
                    ..RadioProfile::default()
                },
                RadioProfile {
                    id: 1,
                    amp_model: "kpa".into(),
                    amp_port: "/dev/ttyUSB1".into(),
                    ..RadioProfile::default()
                },
            ],
            active_radio: 0,
            ..Settings::default()
        };

        // Edit radio 0's amplifier through the patch path the Settings form uses.
        let mut p0 = s.radios[0].clone();
        RadioProfilePatch {
            amp_model: "kpa".into(),
            amp_port: "/dev/ttyUSB9".into(),
            ..patch_of(&s.radios[0])
        }
        .apply_to(&mut p0);
        s.radios[0] = p0;

        assert_eq!(
            s.radios[0].amp_model, "kpa",
            "the edited radio took the change"
        );
        assert_eq!(s.radios[0].amp_port, "/dev/ttyUSB9");
        // …and the OTHER radio is untouched. This is the assertion that matters.
        assert_eq!(
            s.radios[1].amp_model, "kpa",
            "radio 1's amplifier model was disturbed by editing radio 0"
        );
        assert_eq!(
            s.radios[1].amp_port, "/dev/ttyUSB1",
            "radio 1's amplifier PORT was disturbed by editing radio 0"
        );
    }

    #[test]
    fn every_per_radio_field_is_reachable_through_the_patch() {
        const NOT_EDITABLE: [&str; 7] = [
            "id",
            "name",
            "enabled",
            "bands",
            "lastDialMhz",
            "lastBand",
            "lastSideband",
        ];
        let profile = serde_json::to_value(RadioProfile::default()).expect("profile serializes");
        let patch = serde_json::to_value(RadioProfilePatch {
            ptt_method: String::new(),
            rig_model: 0,
            rig_model_name: String::new(),
            serial_port: String::new(),
            ptt_serial_port: String::new(),
            baud: 0,
            rig_conn: String::new(),
            rig_addr: String::new(),
            omnirig_slot: 0,
            rigctld_port: 0,
            icom_native_cat: false,
            icom_data_mode: 3,
            data_modes_plain_ssb: false,
            audio_in: String::new(),
            audio_out: String::new(),
            tx_level: 0.0,
            rx_gain: 0.0,
            rotator_model: 0,
            rotator_port: String::new(),
            rotator_baud: 0,
            amp_model: String::new(),
            amp_port: String::new(),
            amp_follow_band: false,
            rotator_host: String::new(),
            rotctld_port: 0,
            native_scope: String::new(),
            flex_radio_ip: String::new(),
            flex_native_pan: false,
            flex_native_audio: false,
        })
        .expect("patch serializes");
        let patch_keys: Vec<&str> = patch
            .as_object()
            .expect("patch is an object")
            .keys()
            .map(String::as_str)
            .collect();
        let unreachable: Vec<&str> = profile
            .as_object()
            .expect("profile is an object")
            .keys()
            .map(String::as_str)
            .filter(|k| !NOT_EDITABLE.contains(k) && !patch_keys.contains(k))
            .collect();
        assert!(
            unreachable.is_empty(),
            "RadioProfile field(s) {unreachable:?} cannot be edited on a NON-ACTIVE radio: the \
             per-radio Edit flow saves through RadioProfilePatch, so a field missing from the \
             patch is silently dropped on Save. Add it to the patch + apply_to, or list it in \
             NOT_EDITABLE with a reason."
        );
    }

    /// The advisory UI matches assistance sources BY THEIR DISPLAY LABELS
    /// (`FieldDayStatus.assistance_on` carries `assistance_sources()`'s labels;
    /// `FdAdvisories.tsx` string-matches the spotting/cluster ones). A rename on
    /// either side would silently kill the match — same drift class as the
    /// sections mirror, same include_str! cure.
    #[test]
    fn the_advisory_ui_matches_real_assistance_source_labels() {
        let ts = include_str!("../../../ui/src/components/FdAdvisories.tsx");
        let labels: Vec<&str> = Settings::default()
            .assistance_sources()
            .iter()
            .map(|&(label, _)| label)
            .collect();
        // Control: the Rust list is the full known set, so a miss below is a
        // rename, not a parser hole.
        assert_eq!(
            labels.len(),
            3,
            "assistance_sources changed shape: {labels:?}"
        );
        for needed in ["DX cluster / RBN", "PSK Reporter needs"] {
            assert!(
                labels.contains(&needed),
                "{needed:?} left assistance_sources() — update FdAdvisories.tsx's match list too"
            );
            assert!(
                ts.contains(&format!("'{needed}'")),
                "FdAdvisories.tsx no longer matches {needed:?} — it would miss a live source"
            );
        }
    }

    /// ⭐ THE OTHER SIDE OF THE SAME DRIFT — and the half that was missing while the guard
    /// above was cited as covering it.
    ///
    /// `every_per_radio_field_is_reachable_through_the_patch` compares `RadioProfile` against
    /// `RadioProfilePatch` — **Rust against Rust**. It cannot see TypeScript, so it passes
    /// happily while the UI's own `RadioProfilePatch` is missing a field the backend requires.
    /// `SettingsPanel.tsx` nonetheless described it as the guard that "fails when a per-radio
    /// field is added without a home in this patch", which is the dangerous kind of wrong: a
    /// check believed to cover a gap it structurally cannot reach.
    ///
    /// What that cost, both found on 2026-08-27 and both live on main at the time:
    ///
    /// - `amp_model` / `amp_port` had **no serde default**, so a patch from the UI failed to
    ///   deserialize outright — `missing field ampModel` — taking the entire Save with it.
    /// - `icom_data_mode` **had** a default, so it deserialized fine and silently reset the
    ///   operator's Icom DATA submode to DATA1 on every edit of the rig form. A serde default
    ///   turns a loud failure into a quiet one; it does not make the drift safe.
    ///
    /// So this reads the TypeScript interface itself and compares it key for key. Adding a
    /// field to either side without the other now fails here, in CI, in seconds.
    #[test]
    fn the_typescript_patch_carries_every_field_the_rust_patch_does() {
        let ts_src = include_str!("../../../ui/src/api.ts");

        // Pull the body of `export interface RadioProfilePatch { … }`.
        let head = "export interface RadioProfilePatch {";
        let start = ts_src
            .find(head)
            .expect("the UI declares RadioProfilePatch")
            + head.len();
        let body = &ts_src[start..];
        let end = body.find("\n}").expect("the interface is closed");
        let body = &body[..end];

        // Field lines look like `  name: type` / `  name?: type`. Comments and blanks are not.
        let mut ts_keys: Vec<String> = Vec::new();
        for line in body.lines() {
            let t = line.trim();
            if t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
                continue;
            }
            let Some((name, _)) = t.split_once(':') else {
                continue;
            };
            let name = name.trim().trim_end_matches('?');
            if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                ts_keys.push(name.to_string());
            }
        }

        // A parser that found nothing would pass every assertion below it.
        assert!(
            ts_keys.len() > 20,
            "parsed only {} fields out of the TS interface — the parser is broken, not the \
             interface: {ts_keys:?}",
            ts_keys.len()
        );

        // Constructed field by field ON PURPOSE, as the guard above is: there is no Default, so
        // adding a field to the struct breaks this line and forces a look at both sides.
        let rust = serde_json::to_value(RadioProfilePatch {
            ptt_method: String::new(),
            rig_model: 0,
            rig_model_name: String::new(),
            serial_port: String::new(),
            ptt_serial_port: String::new(),
            baud: 0,
            rig_conn: String::new(),
            rig_addr: String::new(),
            omnirig_slot: 0,
            rigctld_port: 0,
            icom_native_cat: false,
            icom_data_mode: 1,
            data_modes_plain_ssb: false,
            audio_in: String::new(),
            audio_out: String::new(),
            tx_level: 0.0,
            rx_gain: 0.0,
            rotator_model: 0,
            rotator_port: String::new(),
            rotator_baud: 0,
            amp_model: String::new(),
            amp_port: String::new(),
            amp_follow_band: false,
            rotator_host: String::new(),
            rotctld_port: 0,
            native_scope: String::new(),
            flex_radio_ip: String::new(),
            flex_native_pan: false,
            flex_native_audio: false,
        })
        .expect("patch serializes");
        let rust_keys: Vec<&str> = rust
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();

        let missing_in_ts: Vec<&&str> = rust_keys
            .iter()
            .filter(|k| !ts_keys.iter().any(|t| t == *k))
            .collect();
        assert!(
            missing_in_ts.is_empty(),
            "RadioProfilePatch field(s) {missing_in_ts:?} exist in Rust but NOT in the UI's \
             interface at ui/src/api.ts. Without a serde default the save fails to deserialize \
             entirely; with one it silently resets the operator's value. Add them to the TS \
             interface AND to radioPatch() in SettingsPanel.tsx."
        );

        let missing_in_rust: Vec<&String> = ts_keys
            .iter()
            .filter(|t| !rust_keys.contains(&t.as_str()))
            .collect();
        assert!(
            missing_in_rust.is_empty(),
            "The UI sends RadioProfilePatch field(s) {missing_in_rust:?} that Rust does not \
             declare — serde will reject the whole payload as an unknown field, or drop it."
        );
    }

    /// THE FIELD-SPECIFIC HALF for OmniRig, written because yesterday's bug was exactly this
    /// and the generic guards above are only as good as the day they were remembered: a
    /// per-radio field the patch does not CARRY, or carries and `apply_to` does not ASSIGN,
    /// is silently wiped when the operator edits a radio they are not currently operating.
    ///
    /// So both halves are pinned here by name. `rig_conn` matters as much as the slot: the
    /// whole connection type is per-radio, and losing it on a save would put an OmniRig
    /// station back on a serial port it does not own.
    #[test]
    fn an_omnirig_pick_survives_an_edit_of_a_non_active_radio() {
        let mut profile = RadioProfile {
            id: 1,
            name: "IC-7300 via OmniRig".into(),
            rig_conn: "omnirig".into(),
            omnirig_slot: 2,
            ..RadioProfile::default()
        };
        // The Settings per-radio Edit flow: read the profile out, change ONE unrelated thing,
        // save it back through the patch. Everything else must come out the way it went in.
        let mut patch = patch_of(&profile);
        patch.ptt_serial_port = "COM9".into(); // the one edit
        patch.apply_to(&mut profile);
        assert_eq!(profile.ptt_serial_port, "COM9", "the edit landed");
        assert_eq!(profile.rig_conn, "omnirig", "the connection type survived");
        assert_eq!(profile.omnirig_slot, 2, "the RIG 2 pick survived");

        // The other direction — a patch that MOVES the slot really moves it, so the guard is
        // shown to fire as well as to hold.
        let mut moved = profile.clone();
        let mut p2 = patch_of(&profile);
        p2.omnirig_slot = 1;
        p2.apply_to(&mut moved);
        assert_eq!(moved.omnirig_slot, 1, "a changed slot is written through");
    }

    /// A full settings file with an OmniRig radio survives save → load, camelCase key and
    /// all, and the flat mirror describes the ACTIVE radio the way every existing consumer
    /// (`Transport::from_settings`) reads it.
    #[test]
    fn an_omnirig_radio_round_trips_through_save_and_load() {
        let dir = std::env::temp_dir().join("tempo_settings_omnirig");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("omnirig_{}.json", std::process::id()));
        let mut s = Settings::default();
        s.mycall = "KD9TAW".into();
        s.radios = vec![
            RadioProfile {
                id: 0,
                name: "FTDX10".into(),
                rigctld_port: 4534,
                ..RadioProfile::default()
            },
            RadioProfile {
                id: 1,
                name: "IC-7300 via OmniRig".into(),
                rig_conn: "omnirig".into(),
                omnirig_slot: 2,
                rigctld_port: 4535,
                ..RadioProfile::default()
            },
        ];
        s.active_radio = 1;
        // The mirror invariant: the flat rig fields describe the ACTIVE radio, and `save`
        // syncs flat→active. Skipping this is how the first draft of this test "failed" —
        // correctly: a save with a stale flat mirror really does overwrite the profile.
        s.sync_flat_from_active();
        s.save(&path).expect("saves");

        // The stored key is camelCase, like every other per-radio field.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            raw.contains("\"omnirigSlot\""),
            "camelCase wire key: {raw:.400}"
        );

        let back = Settings::load(&path);
        let p = back.active_profile().expect("active profile");
        assert_eq!(p.rig_conn, "omnirig");
        assert_eq!(p.omnirig_slot, 2);
        assert_eq!(
            back.omnirig_slot, 2,
            "the flat mirror describes the active radio"
        );
        assert_eq!(back.rig_conn, "omnirig");
        // …and the OTHER radio is untouched, which is the multi-radio half of the same rule.
        let other = back.radios.iter().find(|r| r.id == 0).unwrap();
        assert_eq!(other.rig_conn, "serial");
        let _ = std::fs::remove_file(&path);
    }

    /// The connection-kind rules, both directions, plus the one consequence that would
    /// otherwise be found on the air: OmniRig holds the COM port, so Nexus's own CI-V daemon
    /// can never reach that radio and the satellite offer must not pre-fill a mapping whose
    /// write has no path.
    #[test]
    fn omnirig_is_its_own_connection_kind_and_closes_the_native_civ_door() {
        assert!(rig_conn_is_omnirig("omnirig"));
        assert!(rig_conn_is_omnirig("OmniRig"), "case-insensitive");
        assert!(!rig_conn_is_omnirig("serial"));
        assert!(!rig_conn_is_omnirig("network"));
        assert!(
            !rig_conn_is_omnirig(""),
            "an absent field is serial, not OmniRig"
        );
        // Disjoint from the network rule in both directions.
        assert!(!rig_conn_is_network("omnirig", "192.0.2.1:4532"));
        assert!(!rig_conn_is_omnirig("network"));
        // 3081 = IC-9700, a native-CI-V satellite rig. Positive control first.
        assert!(
            native_civ_reachable(3081, "serial", ""),
            "control: a serial IC-9700 CAN reach the native daemon"
        );
        assert!(
            !native_civ_reachable(3081, "omnirig", ""),
            "…and an OmniRig one cannot — OmniRig owns the port"
        );
    }

    /// Helper for the patch test above: the patch a Save would build from a profile.
    fn patch_of(p: &RadioProfile) -> RadioProfilePatch {
        RadioProfilePatch {
            ptt_method: p.ptt_method.clone(),
            rig_model: p.rig_model,
            rig_model_name: p.rig_model_name.clone(),
            serial_port: p.serial_port.clone(),
            ptt_serial_port: p.ptt_serial_port.clone(),
            baud: p.baud,
            rig_conn: p.rig_conn.clone(),
            rig_addr: p.rig_addr.clone(),
            omnirig_slot: p.omnirig_slot,
            rigctld_port: p.rigctld_port,
            icom_native_cat: p.icom_native_cat,
            icom_data_mode: 1,
            data_modes_plain_ssb: p.data_modes_plain_ssb,
            audio_in: p.audio_in.clone(),
            audio_out: p.audio_out.clone(),
            tx_level: p.tx_level,
            rx_gain: p.rx_gain,
            rotator_model: p.rotator_model,
            rotator_port: p.rotator_port.clone(),
            rotator_baud: p.rotator_baud,
            amp_model: String::new(),
            amp_port: String::new(),
            amp_follow_band: false,
            rotator_host: p.rotator_host.clone(),
            rotctld_port: p.rotctld_port,
            native_scope: p.native_scope.clone(),
            flex_radio_ip: p.flex_radio_ip.clone(),
            flex_native_pan: p.flex_native_pan,
            flex_native_audio: p.flex_native_audio,
        }
    }

    /// A settings file written before the Flex three became per-radio keeps its address: the flat
    /// value migrates into the ACTIVE radio's profile on load, instead of being overwritten by
    /// the profile's serde default on the way back out through `sync_flat_from_active`.
    #[test]
    fn a_pre_per_radio_flex_config_migrates_into_the_active_profile() {
        let dir = std::env::temp_dir().join("tempo_settings_flexmigrate");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("flexmigrate_{}.json", std::process::id()));

        // The legacy shape: TWO radio profiles (so `ensure_radio_profiles` is a no-op and only
        // the migration can save this), neither carrying a Flex key, plus the flat trio the old
        // build wrote. Hand-built JSON — a serialized `Settings` would carry the NEW keys and
        // prove nothing.
        let legacy = serde_json::json!({
            "mycall": "KD9TAW",
            "flexRadioIp": "192.0.2.77",
            "flexNativePan": true,
            "flexNativeAudio": true,
            "activeRadio": 1,
            "radios": [
                { "id": 0, "name": "FTDX10", "rigModel": 1042 },
                { "id": 1, "name": "FLEX-6400", "rigModel": 2036, "rigConn": "network" },
            ],
        });
        std::fs::write(&path, serde_json::to_string(&legacy).unwrap()).unwrap();

        let s = Settings::load(&path);
        let active = s.active_profile().expect("active profile exists");
        assert_eq!(active.flex_radio_ip, "192.0.2.77", "the address survived");
        assert!(active.flex_native_pan, "the pan opt-in survived");
        assert!(active.flex_native_audio, "the audio opt-in survived");
        // …and the flat mirror still describes the active radio, so every existing consumer of
        // `settings().flex_radio_ip` reads it unchanged.
        assert_eq!(s.flex_radio_ip, "192.0.2.77");
        // The OTHER radio is untouched: the flat value described the active one only.
        let other = s.radios.iter().find(|p| p.id == 0).expect("radio 0");
        assert_eq!(other.flex_radio_ip, "");
        assert!(!other.flex_native_pan);

        let _ = std::fs::remove_file(&path);
    }

    /// Two enabled radios on ONE network CAT address warn; the cases that are not a collision
    /// stay silent. Both directions, because a guard shown to fire only one way is half a test.
    #[test]
    fn network_cat_address_conflicts_flags_two_radios_on_one_endpoint() {
        let rig = |name: &str, addr: &str, conn: &str, model: u32, enabled: bool| RadioProfile {
            name: name.into(),
            rig_addr: addr.into(),
            rig_conn: conn.into(),
            rig_model: model,
            enabled,
            ..Default::default()
        };
        // The audited mistake: a duplicated Flex profile left on SmartSDR CAT's slice-A port.
        let a = rig("FLEX slice A", "127.0.0.1:5002", "network", 2036, true);
        let b = rig("FLEX slice B", "127.0.0.1:5002", "network", 2036, true);
        let msg = network_cat_address_conflicts(&[a.clone(), b.clone()]).expect("conflict");
        assert!(
            msg.contains("FLEX slice A") && msg.contains("FLEX slice B"),
            "names both radios: {msg}"
        );
        assert!(msg.contains("127.0.0.1:5002"), "names the address: {msg}");
        // localhost and 127.0.0.1 are ONE endpoint — this mistake is made on loopback.
        assert!(
            network_cat_address_conflicts(&[
                a.clone(),
                rig("FLEX slice B", "localhost:5002", "network", 2036, true),
            ])
            .is_some(),
            "loopback spellings are the same endpoint"
        );
        // Distinct slice ports — the correct multi-slice setup — are silent.
        assert!(network_cat_address_conflicts(&[
            a.clone(),
            rig("FLEX slice B", "127.0.0.1:60001", "network", 2036, true),
        ])
        .is_none());
        // A DISABLED sibling owns nothing.
        assert!(network_cat_address_conflicts(&[
            a.clone(),
            rig("FLEX slice B", "127.0.0.1:5002", "network", 2036, false),
        ])
        .is_none());
        // A serial radio is the serial check's business, not this one.
        assert!(network_cat_address_conflicts(&[
            rig("FTDX10", "127.0.0.1:5002", "serial", 1042, true),
            rig("IC-9700", "127.0.0.1:5002", "serial", 23005, true),
        ])
        .is_none());
        // No model / no address configured yet is not a collision.
        assert!(network_cat_address_conflicts(&[
            rig("unset", "", "network", 2036, true),
            rig("unset 2", "", "network", 2036, true),
        ])
        .is_none());
    }

    #[test]
    fn rtty_rig_mode_follows_the_keying_backend() {
        let mut s = Settings::default();
        s.operating_mode = OperatingMode::Rtty;
        // AFSK (the default backend): soundcard audio tones, so a DATA submode (PKTLSB) on
        // EVERY band — LSB-side keeps the RTTY convention, and DATA routes the USB codec to
        // the modulator (plain LSB would take TX audio from the mic → no RF), like FT8.
        assert_eq!(s.rtty_backend, "afsk");
        s.dial_mhz = 14.083;
        assert_eq!(s.rig_mode(), "PKTLSB");
        s.dial_mhz = 7.080;
        assert_eq!(s.rig_mode(), "PKTLSB");
        // True FSK: the rig's RTTY mode (unlocks its narrow RTTY filters).
        s.rtty_backend = "fsk".to_string();
        assert_eq!(s.rig_mode(), "RTTY");
    }

    /// FM is a VHF/UHF (and 10 m) mode. Selecting it must NOT follow the operator down to an
    /// HF band where it does not belong — see `rig_mode`.
    ///
    /// This is reachable on ONE radio: an FT-991A or IC-7100 covers 2 m and 20 m, so working
    /// an FM repeater and then tuning to 20 m phone is an ordinary evening. It is not a
    /// multi-radio bug, and it was found while mapping multi-radio.
    #[test]
    fn fm_does_not_follow_the_operator_down_to_hf() {
        let mut s = Settings::default();
        s.operating_mode = OperatingMode::Phone;
        s.phone_mode = "fm".into();

        // Where FM belongs, it is commanded.
        for mhz in [146.520, 446.000, 52.525, 29.600] {
            s.dial_mhz = mhz;
            assert_eq!(s.rig_mode(), "FM", "FM is correct at {mhz} MHz");
        }
        // …and where it does not, the band's sideband convention wins. Commanding FM here
        // puts a wideband signal on a band whose plan has no room for it.
        s.dial_mhz = 14.250;
        assert_eq!(s.rig_mode(), "USB", "FM must not survive a move to 20 m");
        s.dial_mhz = 7.200;
        assert_eq!(s.rig_mode(), "LSB", "FM must not survive a move to 40 m");
        s.dial_mhz = 28.400;
        assert_eq!(
            s.rig_mode(),
            "USB",
            "28.4 is SSB — below the 10 m FM segment"
        );
    }

    #[test]
    fn cw_follows_the_band_sideband_convention() {
        // Operator 2026-07-24: 40 m CW must command CW-L (Hamlib CWR), not CW-U.
        // Same 10 MHz rule as phone LSB/USB and the soundcard-keyer arm.
        let mut s = Settings::default();
        s.operating_mode = OperatingMode::Cw;
        s.cw_keyer = CwKeyerBackend::Cat;
        s.dial_mhz = 7.030; // 40 m
        assert_eq!(s.rig_mode(), "CWR", "40 m CW is CW-L");
        s.dial_mhz = 3.550; // 80 m
        assert_eq!(s.rig_mode(), "CWR", "80 m CW is CW-L");
        s.dial_mhz = 14.030; // 20 m
        assert_eq!(s.rig_mode(), "CW", "20 m CW is CW-U");
        s.dial_mhz = 10.110; // 30 m — at/above the 10 MHz line
        assert_eq!(s.rig_mode(), "CW", "30 m CW is CW-U");
        // The soundcard keyer keeps the same SIDE (audio-tone keying) — as the DATA submode
        // its siblings use, see `the_soundcard_cw_keyer_commands_a_data_submode_…`.
        s.cw_keyer = CwKeyerBackend::Soundcard;
        s.dial_mhz = 7.030;
        assert_eq!(s.rig_mode(), "PKTLSB");
    }

    /// ⭐ THE SOUNDCARD CW KEYER WAS THE ONE SOUNDCARD PATH THAT SKIPPED THE DATA SUBMODE
    /// (field report, Yaesu FTX-1, 2026-08-28: "TX would send, but no audio heard while
    /// listening for it").
    ///
    /// Every other path in this app that transmits SOUNDCARD AUDIO commands a DATA submode —
    /// Digital/FT8, Keyboard/PSK31, RTTY-AFSK, and an SSTV image on FM — and the reason is
    /// written out three times in this file and once in the tune path: on a normally-wired rig
    /// plain SSB takes TX audio from the MIC JACK, so the codec audio never reaches the
    /// modulator and the over radiates ZERO RF. The CW soundcard keyer commanded plain
    /// `USB`/`LSB` and so keyed a carrier with nothing on it.
    ///
    /// This is a CLASS test, not an FTX-1 test: it pins the CW arm to the same rule as its four
    /// siblings, including the `data_modes_plain_ssb` mic-jack opt-out, which the arm did not
    /// consult either.
    #[test]
    fn the_soundcard_cw_keyer_commands_a_data_submode_like_every_other_soundcard_path() {
        let mut s = Settings::default();
        s.operating_mode = OperatingMode::Cw;
        s.cw_keyer = CwKeyerBackend::Soundcard;

        // USB-side above 10 MHz, LSB-side below — the CW sideband convention is unchanged;
        // only the SUBMODE moves, so the audio reaches the modulator instead of the mic jack.
        s.dial_mhz = 14.050;
        assert_eq!(
            s.rig_mode(),
            "PKTUSB",
            "20 m soundcard CW must be the DATA submode — plain USB radiates no RF"
        );
        s.dial_mhz = 7.030;
        assert_eq!(
            s.rig_mode(),
            "PKTLSB",
            "40 m soundcard CW keeps the LSB side AND gains the DATA submode"
        );

        // THE MIC-JACK OPT-OUT, which the old arm never consulted: an operator whose interface
        // feeds the mic input gets plain SSB back, exactly like FT8 and PSK31 do for him.
        s.data_modes_plain_ssb = true;
        assert_eq!(
            s.rig_mode(),
            "LSB",
            "mic-jack interface: plain SSB, as its siblings"
        );
        s.dial_mhz = 14.050;
        assert_eq!(s.rig_mode(), "USB", "mic-jack interface, USB side");
        s.data_modes_plain_ssb = false;

        // AND THE OTHER THREE KEYERS ARE UNTOUCHED — they key the rig in CW, and a DATA
        // submode there would be a different bug. This is the half that keeps the fix narrow.
        for k in [
            CwKeyerBackend::Cat,
            CwKeyerBackend::WinKeyer,
            CwKeyerBackend::Serial,
        ] {
            s.cw_keyer = k;
            s.dial_mhz = 14.050;
            assert_eq!(s.rig_mode(), "CW", "{k:?} keys the rig in CW");
            s.dial_mhz = 7.030;
            assert_eq!(
                s.rig_mode(),
                "CWR",
                "{k:?} keys the rig in CW-L below 10 MHz"
            );
        }
    }

    #[test]
    fn phone_fm_forces_fm_mode_else_sideband_by_band() {
        let mut s = Settings::default();
        s.operating_mode = OperatingMode::Phone;
        // FM sub-mode → FM on a band where FM is used.
        s.phone_mode = "fm".into();
        s.dial_mhz = 146.520;
        assert_eq!(s.rig_mode(), "FM");
        // SSB sub-mode → sideband by band (LSB <10 MHz, USB above).
        s.phone_mode = "ssb".into();
        s.dial_mhz = 14.250;
        assert_eq!(s.rig_mode(), "USB");
        s.dial_mhz = 7.200;
        assert_eq!(s.rig_mode(), "LSB");
    }

    #[test]
    fn radio_for_band_routes_to_the_radio_that_covers_the_band() {
        // Operator setup: FTDX10 (radio 0) + IC-9700 (radio 1, "2m" configured). Auto band-routing (P4)
        // must hand off 2 m to the IC-9700 and swing back to the FTDX10 for HF.
        let mut s = Settings::default();
        s.ensure_radio_profiles(); // radio 0
        let r1 = s.add_radio_profile(); // radio 1
        s.radios.iter_mut().find(|p| p.id == r1).unwrap().bands = vec!["2m".into()]; // IC-9700 explicitly covers 2 m

        // FTDX10 (radio 0) with EMPTY coverage (= "covers all") is still beaten by the IC-9700's
        // EXPLICIT 2 m claim — an explicit listing outranks a catch-all (the operator's mental model).
        s.active_radio = 0;
        assert_eq!(
            s.radio_for_band("2m"),
            Some(r1),
            "2 m routes to the IC-9700"
        );
        assert_eq!(
            s.radio_for_band("2M"),
            Some(r1),
            "band match is case-insensitive"
        );
        assert_eq!(
            s.radio_for_band("20m"),
            None,
            "the FTDX10 (catch-all) keeps HF — no needless switch"
        );

        // From the IC-9700, an HF band swings BACK to the FTDX10 (its explicit 2 m list does not cover
        // 20 m → rank 0; the FTDX10's catch-all rank 1 wins).
        s.active_radio = r1;
        assert_eq!(
            s.radio_for_band("20m"),
            Some(0),
            "HF swings back to the FTDX10"
        );
        assert_eq!(
            s.radio_for_band("2m"),
            None,
            "already on the 2 m radio — stay"
        );

        // With the FTDX10 given an EXPLICIT HF list, a band NEITHER radio claims stays put.
        s.radios.iter_mut().find(|p| p.id == 0).unwrap().bands = vec!["20m".into(), "40m".into()];
        s.active_radio = 0;
        assert_eq!(
            s.radio_for_band("2m"),
            Some(r1),
            "explicit 2 m still routes to the IC-9700"
        );
        assert_eq!(
            s.radio_for_band("40m"),
            None,
            "FTDX10 explicitly covers 40 m — stay"
        );
        assert_eq!(
            s.radio_for_band("6m"),
            None,
            "no radio covers 6 m — stay on active"
        );

        // A disabled radio is never a routing target (rig temporarily unplugged).
        s.radios.iter_mut().find(|p| p.id == r1).unwrap().enabled = false;
        assert_eq!(
            s.radio_for_band("2m"),
            None,
            "disabled IC-9700 is not routed to"
        );
    }

    #[test]
    fn radio_for_band_never_switches_with_a_single_radio() {
        let mut s = Settings::default();
        s.ensure_radio_profiles(); // exactly one radio
        assert_eq!(s.radio_for_band("2m"), None);
        assert_eq!(s.radio_for_band("20m"), None);
    }

    /// THE canonical fixture: the operator's actual three-radio shack.
    ///   radio 0 = FTdx10  — HF everything
    ///   radio 1 = IC-9700 — 2 m / 70 cm WEAK-SIGNAL DIGITAL (FT8/FT4/…)
    ///   radio 2 = FT-991A — APRS + 2 m FM / repeaters
    /// Band coverage alone CANNOT express this: the 9700 and the 991A both cover 2 m, and it is the
    /// MODE that decides which one. Every routing test below builds on this.
    fn three_radio_shack() -> Settings {
        let mut s = Settings::default();
        s.ensure_radio_profiles(); // radio 0 — FTdx10
        let ic9700 = s.add_radio_profile();
        let ft991a = s.add_radio_profile();
        {
            let mut set = |id: u32, name: &str, bands: Vec<String>| {
                let p = s.radios.iter_mut().find(|p| p.id == id).unwrap();
                p.name = name.to_string();
                p.bands = bands;
            };
            set(0, "FTdx10", Vec::new()); // catch-all: HF everything
            set(ic9700, "IC-9700", vec!["2m".into(), "70cm".into()]);
            set(ft991a, "FT-991A", vec!["2m".into(), "70cm".into()]);
        }
        // The operator's spec as rules, first-match-wins. The FM rule sits ABOVE the digital rule
        // only for readability — they can't both match, since a (band, mode) has one mode class.
        s.routing_rules = vec![
            RoutingRule {
                bands: vec!["2m".into(), "70cm".into()],
                mode: Some(RouteMode::Fm),
                context: None,
                radio: ft991a,
            },
            RoutingRule {
                bands: vec!["2m".into(), "70cm".into()],
                mode: Some(RouteMode::Digital),
                context: None,
                radio: ic9700,
            },
        ];
        s.default_radio = Some(0); // everything else → the FTdx10
        s.active_radio = 0;
        s
    }

    #[test]
    fn route_radio_splits_one_band_between_two_radios_by_mode() {
        // The whole point of the feature: 2 m goes to a DIFFERENT rig depending on the mode class,
        // which band-only routing structurally cannot do (both rigs list 2 m).
        let s = three_radio_shack();
        let (ic9700, ft991a) = (1, 2);

        assert_eq!(
            s.route_radio("2m", RouteMode::Digital),
            Some(ic9700),
            "2 m FT8 → the IC-9700"
        );
        assert_eq!(
            s.route_radio("2m", RouteMode::Fm),
            Some(ft991a),
            "2 m FM / APRS → the FT-991A"
        );
        assert_eq!(
            s.route_radio("70cm", RouteMode::Fm),
            Some(ft991a),
            "the rule's band SET covers 70 cm too"
        );
        assert_eq!(
            s.route_radio("2M", RouteMode::Digital),
            Some(ic9700),
            "band match is case-insensitive"
        );

        // HF, every mode, stays on the FTdx10 (we're already on it → None = stay put).
        for m in [
            RouteMode::Digital,
            RouteMode::Ssb,
            RouteMode::Cw,
            RouteMode::Rtty,
            RouteMode::Fm,
        ] {
            assert_eq!(
                s.route_radio("20m", m),
                None,
                "20 m {m:?} stays on the FTdx10"
            );
        }
    }

    #[test]
    fn route_radio_swings_back_to_hf_from_a_vhf_radio() {
        let mut s = three_radio_shack();
        let (ic9700, ft991a) = (1, 2);
        s.active_radio = ic9700;

        // No rule covers 20 m, so the band-coverage tier decides: the 9700's explicit 2m/70cm list
        // doesn't cover 20 m (rank 0) and the FTdx10's catch-all does (rank 1).
        assert_eq!(
            s.route_radio("20m", RouteMode::Ssb),
            Some(0),
            "20 m SSB swings back to the FTdx10"
        );
        assert_eq!(
            s.route_radio("40m", RouteMode::Cw),
            Some(0),
            "40 m CW swings back to the FTdx10"
        );
        // From the 9700, 2 m FM still crosses to the 991A — a rule fires even between two VHF rigs.
        assert_eq!(s.route_radio("2m", RouteMode::Fm), Some(ft991a));
        // …and 2 m digital is a rule match naming the radio we're ALREADY on → stay put, rather
        // than falling through to a broader tier that would move us off it.
        assert_eq!(s.route_radio("2m", RouteMode::Digital), None);
    }

    #[test]
    fn route_radio_is_first_match_wins() {
        let mut s = three_radio_shack();
        let (ic9700, ft991a) = (1, 2);
        s.active_radio = 0;
        // A broad catch-all rule ABOVE the specific ones swallows everything — order is the whole
        // precedence model, so this must be visible and testable.
        s.routing_rules.insert(
            0,
            RoutingRule {
                bands: Vec::new(), // any band
                mode: None,        // any mode
                context: None,
                radio: ft991a,
            },
        );
        assert_eq!(
            s.route_radio("2m", RouteMode::Digital),
            Some(ft991a),
            "the catch-all is first, so it wins over the 2 m digital rule"
        );
        // Move it back below and the specific rule wins again.
        let broad = s.routing_rules.remove(0);
        s.routing_rules.push(broad);
        assert_eq!(s.route_radio("2m", RouteMode::Digital), Some(ic9700));
        assert_eq!(
            s.route_radio("20m", RouteMode::Ssb),
            Some(ft991a),
            "the trailing catch-all now claims what nothing else did"
        );
    }

    #[test]
    fn route_radio_falls_back_to_band_coverage_then_the_default_radio() {
        let mut s = three_radio_shack();
        let (ic9700, ft991a) = (1, 2);

        // TIER 2 — no rules at all: routing is exactly the pre-rules band-coverage behavior, so an
        // install that never writes a rule is unchanged.
        s.routing_rules.clear();
        s.default_radio = None;
        s.active_radio = 0;
        assert_eq!(
            s.route_radio("2m", RouteMode::Digital),
            s.radio_for_band("2m"),
            "with no rules, routing == band coverage"
        );
        assert_eq!(s.route_radio("20m", RouteMode::Ssb), None);

        // TIER 3 — a band NOBODY covers falls to the default radio.
        s.radios.iter_mut().find(|p| p.id == 0).unwrap().bands = vec!["20m".into(), "40m".into()]; // FTdx10 no longer a catch-all
        s.active_radio = ic9700;
        assert_eq!(
            s.route_radio("6m", RouteMode::Ssb),
            None,
            "no coverage, no default → stay put"
        );
        s.default_radio = Some(0);
        assert_eq!(
            s.route_radio("6m", RouteMode::Ssb),
            Some(0),
            "no coverage → the default radio"
        );
        // The default never fires when it names the radio already active.
        s.active_radio = 0;
        assert_eq!(s.route_radio("6m", RouteMode::Ssb), None);
        // A DISABLED default is not obeyed — an unplugged rig must never become the target.
        s.active_radio = ic9700;
        s.default_radio = Some(ft991a);
        s.radios
            .iter_mut()
            .find(|p| p.id == ft991a)
            .unwrap()
            .enabled = false;
        assert_eq!(s.route_radio("6m", RouteMode::Ssb), None);
    }

    #[test]
    fn route_radio_skips_rules_aimed_at_a_disabled_radio() {
        let mut s = three_radio_shack();
        let ft991a = 2;
        s.radios
            .iter_mut()
            .find(|p| p.id == ft991a)
            .unwrap()
            .enabled = false;
        // The 2 m FM rule points at the unplugged 991A. Skip the RULE (don't obey it, don't let it
        // consume the match) and fall through — else an APRS tune would hand off to a dead rig.
        assert_eq!(
            s.route_radio("2m", RouteMode::Fm),
            Some(1),
            "falls through to band coverage: the enabled IC-9700 also lists 2 m"
        );
    }

    #[test]
    fn a_satellite_designation_outranks_the_mode_rules_for_satellite_tunes() {
        // The operator's request, verbatim: "Can we add designation in the rules in the settings
        // for satellites?" Their packet birds route through the FM & APRS rule to the FT-991A —
        // right by the rules, wrong for satellites: the IC-9700 is the sat rig.
        let mut s = three_radio_shack();
        let (ic9700, ft991a) = (1, 2);
        // The designation, appended LAST: a tier is not a row, so list position must not let
        // the FM & APRS rule above outrank it.
        s.routing_rules.push(RoutingRule {
            bands: Vec::new(),
            mode: None,
            context: Some(RouteContext::Satellite),
            radio: ic9700,
        });
        assert_eq!(
            s.route_radio_satellite("2m", RouteMode::Fm),
            Some(ic9700),
            "a packet bird beats the FM & APRS rule sitting above the designation"
        );
        assert_eq!(
            s.route_radio_satellite("70cm", RouteMode::Ssb),
            Some(ic9700),
            "a linear bird follows the same designation"
        );
        // Terrestrial tunes must NEVER match the satellite rule — 2 m APRS stays the 991A's.
        assert_eq!(s.route_radio("2m", RouteMode::Fm), Some(ft991a));
        // A satellite rule naming the radio we are already on is authoritative: stay put,
        // never fall through to a mode tier that would move us off it.
        s.active_radio = ic9700;
        assert_eq!(s.route_radio_satellite("2m", RouteMode::Fm), None);
    }

    #[test]
    fn without_a_satellite_rule_a_satellite_tune_routes_exactly_as_today() {
        // Pinned: a station that never writes the designation is byte-for-byte on today's
        // behavior — the packet bird follows the FM & APRS rule to the FT-991A.
        let s = three_radio_shack();
        let (ic9700, ft991a) = (1, 2);
        assert_eq!(s.route_radio_satellite("2m", RouteMode::Fm), Some(ft991a));
        assert_eq!(
            s.route_radio_satellite("2m", RouteMode::Digital),
            Some(ic9700)
        );
        for m in [RouteMode::Ssb, RouteMode::Cw] {
            assert_eq!(s.route_radio_satellite("20m", m), s.route_radio("20m", m));
        }
    }

    #[test]
    fn a_satellite_rule_is_scoped_by_its_bands_and_skipped_when_its_radio_is_disabled() {
        let mut s = three_radio_shack();
        let (ic9700, ft991a) = (1, 2);
        s.routing_rules.push(RoutingRule {
            bands: vec!["70cm".into()],
            mode: None,
            context: Some(RouteContext::Satellite),
            radio: ic9700,
        });
        // Band selectors still apply inside the satellite tier: only a 70 cm bird is claimed;
        // a 2 m bird falls through to the mode rules.
        assert_eq!(s.route_radio_satellite("2m", RouteMode::Fm), Some(ft991a));
        assert_eq!(s.route_radio_satellite("70cm", RouteMode::Fm), Some(ic9700));
        // An unplugged sat rig must never become the handoff target — same rule as every tier.
        s.radios
            .iter_mut()
            .find(|p| p.id == ic9700)
            .unwrap()
            .enabled = false;
        assert_eq!(
            s.route_radio_satellite("70cm", RouteMode::Fm),
            Some(ft991a),
            "falls through to the FM rule when the designated radio is disabled"
        );
    }

    #[test]
    fn a_bandless_tune_asks_only_the_rules_whose_answer_never_read_a_band() {
        // QO-100 at 10.489 GHz has no band label (`band_for_dial` stops at 23 cm),
        // and the satellite path now tunes it rather than refusing. Which rig?
        // Only the rules that never read a band can say — and they say it exactly
        // as they always did.
        let mut s = three_radio_shack();
        let (ic9700, ft991a) = (1, 2);
        assert_eq!(s.active_radio, 0, "precondition: the HF rig is active");

        // The three_radio_shack rules all NAME bands, so none of them claims it,
        // and no band-coverage tier is consulted at all — including the FTdx10's
        // empty (catch-all) band list, which would otherwise rank as coverage.
        assert_eq!(s.route_radio_bandless(RouteMode::Ssb), None);
        assert_eq!(s.route_radio_bandless(RouteMode::Fm), None);

        // A plain MODE rule with no band selector does claim it: its verdict is
        // the same for every band, and therefore for no band.
        s.routing_rules.push(RoutingRule {
            bands: Vec::new(),
            mode: Some(RouteMode::Ssb),
            context: None,
            radio: ft991a,
        });
        assert_eq!(s.route_radio_bandless(RouteMode::Ssb), Some(ft991a));
        assert_eq!(
            s.route_radio_bandless(RouteMode::Fm),
            None,
            "a mode selector still scopes it — mode is not a band"
        );

        // …and a satellite DESIGNATION outranks it, the same tier order as the
        // band path, whatever the list positions are.
        s.routing_rules.push(RoutingRule {
            bands: Vec::new(),
            mode: None,
            context: Some(RouteContext::Satellite),
            radio: ic9700,
        });
        assert_eq!(s.route_radio_bandless(RouteMode::Ssb), Some(ic9700));

        // Stay-put and the disabled-radio skip work as they do everywhere else.
        s.active_radio = ic9700;
        assert_eq!(s.route_radio_bandless(RouteMode::Ssb), None);
        s.active_radio = 0;
        s.radios
            .iter_mut()
            .find(|p| p.id == ic9700)
            .unwrap()
            .enabled = false;
        assert_eq!(
            s.route_radio_bandless(RouteMode::Ssb),
            Some(ft991a),
            "falls through to the mode rule when the designated radio is unplugged"
        );
    }

    #[test]
    fn removing_a_radio_drops_the_rules_that_pointed_at_it() {
        let mut s = three_radio_shack();
        let ft991a = 2;
        assert!(s.remove_radio_profile(ft991a));
        assert_eq!(
            s.routing_rules.len(),
            1,
            "the FT-991A's FM rule went with it"
        );
        assert_eq!(s.routing_rules[0].radio, 1, "the IC-9700 rule survives");
        // …and a default aimed at the removed radio is cleared, not left pointing at nothing.
        s.default_radio = Some(ft991a);
        s.ensure_routing_targets();
        assert_eq!(s.default_radio, None);
    }

    #[test]
    fn routing_rules_survive_a_settings_round_trip() {
        let s = three_radio_shack();
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            json.contains("\"routingRules\""),
            "the wire key the UI writes must be camelCase"
        );
        assert!(json.contains("\"defaultRadio\""));
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.routing_rules, s.routing_rules);
        assert_eq!(back.default_radio, s.default_radio);
        // The EXACT mode tokens the UI's <select> writes. A hand-written TS union that disagrees
        // fails INVISIBLY — serde falls back to the field default and the control appears dead —
        // which is why `aprs_is_settings_use_the_exact_wire_keys_the_ui_writes` exists too.
        for (m, wire) in [
            (RouteMode::Digital, "\"digital\""),
            (RouteMode::Fm, "\"fm\""),
            (RouteMode::Ssb, "\"ssb\""),
            (RouteMode::Cw, "\"cw\""),
            (RouteMode::Rtty, "\"rtty\""),
        ] {
            assert_eq!(serde_json::to_string(&m).unwrap(), wire);
        }
        // `mode: null` (any mode) must round-trip, and be what an omitted key yields.
        let any: RoutingRule = serde_json::from_str("{\"bands\":[\"2m\"],\"radio\":1}").unwrap();
        assert_eq!(any.mode, None, "an omitted mode means ANY mode");
        assert!(any.matches("2m", RouteMode::Cw) && any.matches("2m", RouteMode::Fm));
        // The context designation: a rule stored BEFORE the field existed carries no key and
        // must load as a plain terrestrial rule — and the designated form must round-trip on
        // the exact token the UI's dropdown writes.
        assert_eq!(
            any.context, None,
            "an omitted context means TERRESTRIAL, as before"
        );
        assert_eq!(
            serde_json::to_string(&RouteContext::Satellite).unwrap(),
            "\"satellite\""
        );
        let sat: RoutingRule =
            serde_json::from_str("{\"bands\":[],\"context\":\"satellite\",\"radio\":1}").unwrap();
        assert_eq!(sat.context, Some(RouteContext::Satellite));
        let rt: RoutingRule = serde_json::from_str(&serde_json::to_string(&sat).unwrap()).unwrap();
        assert_eq!(rt, sat, "the designation survives a round-trip");
        // An OLDER settings.json has neither key — it must load with no rules (today's behavior),
        // not fail.
        let old: Settings = serde_json::from_str("{\"mycall\":\"KD9TAW\"}").unwrap();
        assert!(old.routing_rules.is_empty());
        assert_eq!(old.default_radio, None);
    }

    #[test]
    fn band_coverage_ties_break_on_the_lowest_id_not_roster_order() {
        // Reachable only with THREE radios: two non-active rigs both explicitly listing the band.
        // `max_by_key` returned the LAST maximum, so the winner was roster order — silently.
        let mut s = three_radio_shack();
        s.routing_rules.clear();
        s.default_radio = None;
        s.active_radio = 0;
        s.radios.iter_mut().find(|p| p.id == 0).unwrap().bands = vec!["20m".into()];
        // Both the IC-9700 (id 1) and the FT-991A (id 2) explicitly claim 2 m.
        assert_eq!(
            s.route_radio("2m", RouteMode::Ssb),
            Some(1),
            "lowest id wins the tie, deterministically"
        );
        // Order in the roster must not change the answer.
        s.radios.swap(1, 2);
        assert_eq!(s.route_radio("2m", RouteMode::Ssb), Some(1));
    }

    #[test]
    fn band_coverage_hides_a_band_only_when_every_rig_named_its_bands() {
        // #184: akhepcat runs an FTdx10 (HF..4m) and an FT-817 (2m/70cm) and expected the band
        // dropdown to stop offering 23 cm, which neither rig can reach. The filter may only
        // subtract when EVERY enabled rig has named its bands — anything else and an operator
        // ends up staring at an empty dropdown.
        let mut s = three_radio_shack();
        let ids: Vec<u32> = s.radios.iter().map(|p| p.id).collect();
        for (i, id) in ids.iter().enumerate() {
            let p = s.radios.iter_mut().find(|p| p.id == *id).unwrap();
            p.enabled = true;
            p.bands = if i == 0 {
                vec!["20m".into(), "4m".into()]
            } else {
                vec!["2m".into(), "70cm".into()]
            };
        }
        assert!(
            s.any_radio_covers("20m"),
            "an explicitly claimed band is offered"
        );
        assert!(s.any_radio_covers("4m"), "…on either rig");
        assert!(s.any_radio_covers("2m"));
        assert!(
            !s.any_radio_covers("23cm"),
            "no rig reaches 23 cm, so it is not worth offering"
        );
        // Case matters to nobody.
        assert!(s.any_radio_covers("70CM"));

        // ONE catch-all rig restores everything — the single-radio majority is untouched.
        let first = ids[0];
        s.radios.iter_mut().find(|p| p.id == first).unwrap().bands = Vec::new();
        assert!(
            s.any_radio_covers("23cm"),
            "a rig that claims nothing claims everything"
        );

        // A disabled rig is not coverage…
        s.radios.iter_mut().find(|p| p.id == first).unwrap().enabled = false;
        assert!(
            !s.any_radio_covers("23cm"),
            "a disabled catch-all does not count"
        );

        // …and with NOTHING enabled the filter must open all the way up rather than
        // leaving a fresh install with an empty band list.
        for id in &ids {
            s.radios.iter_mut().find(|p| p.id == *id).unwrap().enabled = false;
        }
        assert!(
            s.any_radio_covers("23cm"),
            "no radios configured = no opinion, show every band"
        );
        s.radios.clear();
        assert!(
            s.any_radio_covers("23cm"),
            "…and likewise with an empty roster"
        );
    }

    #[test]
    fn radios_covering_counts_only_explicit_claims() {
        let mut s = three_radio_shack();
        // FTdx10 (id 0) is a catch-all; the 9700 and 991A both claim 2 m explicitly.
        s.radios.iter_mut().find(|p| p.id == 0).unwrap().bands = Vec::new();
        assert_eq!(
            s.radios_covering("2m"),
            2,
            "a catch-all does not make 2 m ambiguous — an explicit claim outranks it"
        );
        assert_eq!(s.radios_covering("20m"), 0, "nobody claims 20 m explicitly");
        // Disabling one removes the ambiguity.
        let ic = s.radios.iter().find(|p| p.name == "IC-9700").unwrap().id;
        s.radios.iter_mut().find(|p| p.id == ic).unwrap().enabled = false;
        assert_eq!(
            s.radios_covering("2m"),
            1,
            "a disabled radio is not a candidate"
        );
    }

    #[test]
    fn a_band_coverage_tie_prefers_the_operators_default_radio() {
        // ⚠️ THE 0.21.4 REGRESSION. The operator's shack is exactly the tie case: three radios, the
        // IC-9700 (id 1) and FT-991A (id 2) both listing 2 m, and their working APRS audio is on the
        // 991A. 0.21.3's `max_by_key` returned the LAST maximum, i.e. roster order, which happened
        // to land on the 991A and decoded. 0.21.4 broke the tie on the LOWEST id, which lands on the
        // 9700 — whose audio is set up for FT8 — so the tap followed the wrong radio and APRS went
        // silent with nothing visibly wrong.
        //
        // Neither rule was ever RIGHT: both are arbitrary. What is not arbitrary is that the
        // operator has already said which radio they prefer. A tie is exactly when to consult it.
        let mut s = three_radio_shack();
        s.routing_rules.clear();
        s.active_radio = 0;
        s.radios.iter_mut().find(|p| p.id == 0).unwrap().bands = vec!["20m".into()];
        let ft991a = s.radios.iter().find(|p| p.name == "FT-991A").unwrap().id;
        s.default_radio = Some(ft991a);
        assert_eq!(
            s.route_radio("2m", RouteMode::Fm),
            Some(ft991a),
            "a tie must go to the radio the operator nominated, not to whichever id is lower"
        );
        // And roster order still must not decide it.
        s.radios.swap(1, 2);
        assert_eq!(s.route_radio("2m", RouteMode::Fm), Some(ft991a));
    }

    #[test]
    fn a_default_radio_that_cannot_reach_the_band_does_not_win_the_tie() {
        // The preference is a tie-BREAK among capable candidates, never an override that sends a
        // 2 m activation to an HF-only rig.
        let mut s = three_radio_shack();
        s.routing_rules.clear();
        s.active_radio = 0;
        s.radios.iter_mut().find(|p| p.id == 0).unwrap().bands = vec!["20m".into()];
        s.default_radio = Some(0); // FTdx10, 20 m only
        assert_eq!(
            s.route_radio("2m", RouteMode::Fm),
            Some(1),
            "an incapable default is ignored; the deterministic tie-break still applies"
        );
    }

    #[test]
    fn without_a_default_radio_a_tie_stays_deterministic() {
        // Unchanged from 0.21.4: with no expressed preference, the same band always lands on the
        // same rig. Arbitrary, but stable — and the operator's lever is a routing rule.
        let mut s = three_radio_shack();
        s.routing_rules.clear();
        s.default_radio = None;
        s.active_radio = 0;
        s.radios.iter_mut().find(|p| p.id == 0).unwrap().bands = vec!["20m".into()];
        assert_eq!(s.route_radio("2m", RouteMode::Fm), Some(1));
    }

    #[test]
    fn add_radio_profile_never_duplicates_a_name() {
        // Length-based numbering ("Radio {len+1}") collides once a radio is removed from the middle
        // — and every port/audio conflict warning the operator reads names radios.
        let mut s = Settings::default();
        s.ensure_radio_profiles();
        let b = s.add_radio_profile(); // "Radio 2"
        let _c = s.add_radio_profile(); // "Radio 3"
        s.active_radio = 0;
        assert!(s.remove_radio_profile(b));
        let d = s.add_radio_profile();
        let name = |id: u32| s.radios.iter().find(|p| p.id == id).unwrap().name.clone();
        assert_ne!(name(d), name(2), "the 3rd radio's name is not reused");
        let mut names: Vec<String> = s.radios.iter().map(|p| p.name.clone()).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "every radio name is unique: {names:?}");
    }

    #[test]
    fn rptr_offset_follows_band_conventions() {
        let mut s = Settings::default();
        for (mhz, off) in [
            (29.6, 100_000),
            (52.5, 1_000_000),
            (146.5, 600_000),
            (223.5, 1_600_000),
            (446.0, 5_000_000),
        ] {
            s.dial_mhz = mhz;
            assert_eq!(s.rptr_offset_hz(), off, "{mhz} MHz offset");
        }
        s.dial_mhz = 14.250; // no FM repeaters on HF SSB bands
        assert_eq!(s.rptr_offset_hz(), 0);
    }

    #[test]
    fn rptr_offset_override_beats_band_convention() {
        let mut s = Settings::default();
        s.dial_mhz = 146.5; // band convention says 600 kHz…
        s.rptr_offset_override_hz = 1_000_000; // …but this machine is an odd +1 MHz split
        assert_eq!(s.rptr_offset_hz(), 1_000_000);
        s.rptr_offset_override_hz = 0; // cleared → back to the convention
        assert_eq!(s.rptr_offset_hz(), 600_000);
        // The key must round-trip settings JSON (the dead-key lesson).
        s.rptr_offset_override_hz = 1_000_000;
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"rptrOffsetOverrideHz\":1000000"), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.rptr_offset_override_hz, 1_000_000);
    }

    /// Operator ask 2026-07-26: run the soundcard modes in PLAIN SSB instead of the DATA
    /// submode, per radio, staying there.
    ///
    /// ⚠️ This is wiring-dependent and WRONG for most rigs — on a normal setup plain SSB takes
    /// TX audio from the MIC and the radio transmits no RF. It exists for an interface wired
    /// into the mic jack (several RIGblaster models). Hence: OFF by default, and per RADIO,
    /// because it is a property of the cable and a station can be wired differently per rig.
    #[test]
    fn plain_ssb_opt_out_covers_every_soundcard_mode_and_defaults_off() {
        let mut s = Settings::default();
        s.dial_mhz = 14.074;

        // DEFAULT: unchanged. The DATA submode is still forced everywhere.
        assert!(!s.data_modes_plain_ssb, "must stay off by default");
        s.operating_mode = OperatingMode::Digital;
        assert_eq!(s.rig_mode(), "PKTUSB");
        s.operating_mode = OperatingMode::Rtty;
        s.rtty_backend = "afsk".into();
        assert_eq!(s.rig_mode(), "PKTLSB");

        // OPTED IN: the DATA submode becomes its plain-SSB equivalent.
        s.data_modes_plain_ssb = true;
        s.operating_mode = OperatingMode::Digital;
        assert_eq!(s.rig_mode(), "USB", "Digital: PKTUSB → USB");
        s.sideband = "LSB".into();
        assert_eq!(s.rig_mode(), "LSB", "Digital LSB-side: PKTLSB → LSB");
        s.operating_mode = OperatingMode::Rtty;
        assert_eq!(s.rig_mode(), "LSB", "RTTY-AFSK: PKTLSB → LSB");

        // ⚠️ RTTY in FSK is NOT touched: it commands the rig's own RTTY mode, which is what
        // keys the shift and unlocks the narrow RTTY filters. "Plain SSB" is meaningless there,
        // and swapping it would silently break true FSK for anyone who enables this.
        s.rtty_backend = "fsk".into();
        assert_eq!(s.rig_mode(), "RTTY", "FSK keeps the rig's RTTY mode");

        // Phone and CW are their own policies and must be untouched by a DATA-mode opt-out.
        s.operating_mode = OperatingMode::Phone;
        s.sideband = String::new();
        assert_eq!(s.rig_mode(), "USB");
        s.operating_mode = OperatingMode::Cw;
        assert_eq!(s.rig_mode(), "CW");
    }

    /// THE UPGRADE PATH, which is the case that actually matters: an existing operator's
    /// settings.json has no `dataModesPlainSsb` key at all, and a new install writes none.
    /// Both must land on the DATA submode exactly as before — this option must be invisible
    /// to everyone who does not deliberately turn it on. Proven by deserializing a settings
    /// blob WITHOUT the key rather than by trusting the `#[serde(default)]`.
    #[test]
    fn an_older_settings_file_and_a_new_install_both_keep_the_data_submode() {
        // A settings blob from before this option existed.
        let before: Settings = serde_json::from_str(
            r#"{"mycall":"KD9TAW","mygrid":"EN51","operatingMode":"digital","dialMhz":14.074}"#,
        )
        .expect("an older settings file must still load");
        assert!(
            !before.data_modes_plain_ssb,
            "a missing key must mean DATA, never plain SSB — silently flipping an existing \
             station to plain SSB would take them off the air with a red TX light"
        );
        assert_eq!(before.rig_mode(), "PKTUSB");

        // And a radio profile from before the option existed.
        let p: RadioProfile =
            serde_json::from_str(r#"{"id":0,"name":"FTDX10"}"#).expect("older profile loads");
        assert!(!p.data_modes_plain_ssb);

        // A brand-new install.
        let fresh = Settings::default();
        assert!(!fresh.data_modes_plain_ssb);
        assert!(!RadioProfile::default().data_modes_plain_ssb);

        // And it round-trips once set, so an operator who DOES turn it on keeps it across a
        // restart (the dead-key lesson: a field that serialises but never deserialises is worse
        // than no field at all).
        let mut on = Settings::default();
        on.data_modes_plain_ssb = true;
        let json = serde_json::to_string(&on).unwrap();
        assert!(json.contains("\"dataModesPlainSsb\":true"), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert!(back.data_modes_plain_ssb);
    }

    /// THE UPGRADE PATH for the Field Day bonus PLAN. `fd_bonuses` has always meant
    /// EARNED — the score reads it — and the planned list is a new sibling rather than a
    /// reinterpretation of it, precisely so an existing settings.json keeps scoring the
    /// same points it scored yesterday. Proven by deserializing a blob that predates the
    /// key, not by trusting the `#[serde(default)]`.
    #[test]
    fn an_older_settings_file_loads_with_its_earned_bonuses_and_no_plan() {
        let before: Settings = serde_json::from_str(
            r#"{"mycall":"KD9TAW","fdClass":"3A","fdPowerMult":5,
                "fdBonuses":["w1aw-bulletin","web-submission"]}"#,
        )
        .expect("an older settings file must still load");
        assert_eq!(
            before.fd_bonuses,
            vec!["w1aw-bulletin".to_string(), "web-submission".to_string()],
            "the EARNED list is untouched — this is what the score is made of"
        );
        assert!(
            before.fd_bonuses_planned.is_empty(),
            "a file written before planning existed has planned nothing; inheriting the \
             earned list as a plan would be a lie about what the club intends"
        );
        assert!(Settings::default().fd_bonuses_planned.is_empty());

        // And it round-trips once set — a field that serialises but never deserialises
        // would lose the club's Friday plan at the first restart.
        let mut planned = Settings::default();
        planned.fd_bonuses_planned = vec!["youth".into(), "safety-officer".into()];
        let json = serde_json::to_string(&planned).unwrap();
        assert!(
            json.contains("\"fdBonusesPlanned\":[\"youth\",\"safety-officer\"]"),
            "{json}"
        );
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.fd_bonuses_planned, planned.fd_bonuses_planned);
        assert!(
            back.fd_bonuses.is_empty(),
            "planning never fills the earned list"
        );
    }

    /// The setting lives on the RADIO, so switching rigs must switch the behaviour with it —
    /// a mic-jack interface on one rig and a data-port interface on the other is the whole
    /// reason it is per-radio rather than global.
    #[test]
    fn plain_ssb_opt_out_follows_the_active_radio() {
        let mut s = Settings::default();
        s.ensure_radio_profiles();
        let r2 = s.add_radio_profile();
        let r1 = s.active_radio;

        // Radio 1: mic-jack interface → plain SSB. Radio 2: normal data port → DATA.
        // (`active_radio` is set directly + synced, the idiom the other radio tests use.)
        s.data_modes_plain_ssb = true;
        s.sync_active_from_flat();
        s.active_radio = r2;
        s.data_modes_plain_ssb = false;
        s.sync_active_from_flat();

        s.active_radio = r1;
        s.sync_flat_from_active();
        assert!(s.data_modes_plain_ssb, "radio 1 keeps its plain-SSB wiring");
        s.active_radio = r2;
        s.sync_flat_from_active();
        assert!(!s.data_modes_plain_ssb, "radio 2 keeps the DATA submode");
    }

    #[test]
    fn rig_mode_policy_obeys_digital_but_forces_phone_and_cw() {
        let mut s = Settings::default();

        // Digital: ALWAYS force the DATA submode so FT8/FT4 sets the rig (like Phone/CW).
        // USB-side by default (FT8/FT4 are USB-side); the default empty sideband → PKTUSB.
        assert_eq!(s.operating_mode, OperatingMode::Digital);
        assert_eq!(
            s.rig_mode(),
            "PKTUSB",
            "digital default → DATA submode (USB-side)"
        );
        s.sideband = "LSB".into();
        assert_eq!(s.rig_mode(), "PKTLSB", "digital LSB-side → PKTLSB");
        // Forced regardless of set_rig_mode (the old opt-out is gone) and robust against
        // a garbled sideband (anything non-LSB → USB-side PKTUSB).
        s.set_rig_mode = false;
        s.sideband = "USB".into();
        assert_eq!(
            s.rig_mode(),
            "PKTUSB",
            "digital always forces DATA, opt-out ignored"
        );
        s.sideband = "CW".into(); // corrupted sideband must not leak into the mode
        assert_eq!(
            s.rig_mode(),
            "PKTUSB",
            "garbled sideband → USB-side PKTUSB, never CW"
        );
        s.sideband = "USB".into();

        // CW with the CAT keyer: force CW.
        s.operating_mode = OperatingMode::Cw;
        assert_eq!(s.rig_mode(), "CW");
        // CW with the SOUNDCARD keyer: the rig must be on the SSB side to send the tone, and
        // in a DATA submode so the tone reaches the modulator rather than the mic jack.
        s.cw_keyer = CwKeyerBackend::Soundcard;
        s.dial_mhz = 14.050;
        assert_eq!(s.rig_mode(), "PKTUSB");
        s.dial_mhz = 7.030;
        assert_eq!(s.rig_mode(), "PKTLSB");
        s.cw_keyer = CwKeyerBackend::Cat;

        // Phone: band-aware sideband — LSB below 10 MHz, USB at/above.
        s.operating_mode = OperatingMode::Phone;
        s.dial_mhz = 7.200; // 40 m
        assert_eq!(s.rig_mode(), "LSB");
        s.dial_mhz = 14.250; // 20 m
        assert_eq!(s.rig_mode(), "USB");
        s.dial_mhz = 3.850; // 80 m
        assert_eq!(s.rig_mode(), "LSB");
    }

    /// ISSUE #44 — the one setting whose silence is a transmitting radio.
    ///
    /// Same trap as the APRS keys below: `rename_all = "camelCase"` mangles any hand-written TS
    /// key that disagrees, the backend falls back to the serde default, and the control appears
    /// to do nothing. Here "does nothing" means the operator ticks "my interface keys RTS",
    /// believes the rig will stop keying at launch, and it keys anyway — so this key earns its
    /// own guard rather than riding along in a list.
    #[test]
    fn the_rts_keying_declaration_uses_the_exact_wire_key_the_ui_writes() {
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert!(
            json.contains("\"catRtsKeysPtt\":false"),
            "ui/src/types.ts writes `catRtsKeysPtt`; if this key ever disagrees the tick box \
             silently stops working and the rig goes on transmitting at launch. json = {json}"
        );
        // CONTROL: the assertion is reading a real serialisation, not an empty haystack that
        // would make any `contains` claim vacuously checkable.
        assert!(
            json.contains("\"catRtsState\":\"low\""),
            "control: neighbouring keys serialise"
        );
    }

    #[test]
    fn aprs_is_settings_use_the_exact_wire_keys_the_ui_writes() {
        // The container `rename_all = "camelCase"` silently mangles any hand-written TS key that
        // disagrees with it, and the failure is invisible: the key never matches, the backend
        // quietly uses the serde default, and the control appears to do nothing. These are the
        // exact strings `ui/src/types.ts` and `defaultSettings.json` carry.
        let json = serde_json::to_string(&Settings::default()).unwrap();
        for key in [
            "\"aprsIsEnabled\":false",
            "\"aprsIsHost\":\"rotate.aprs2.net\"",
            "\"aprsIsPort\":14580",
            "\"aprsIsRadiusKm\":150",
            "\"aprsIsWatchCalls\":[]",
            "\"aprsIsWeather\":true",
            "\"aprsIsObjects\":true",
            "\"aprsIsMessages\":true",
            "\"aprsIsUplink\":false",
            "\"aprsStationTtlMin\":60",
            // The RF side. `aprsSsid`, NOT `aprsSSID` — SSID is the form every ham and every
            // developer writes, and the failure is invisible: the key never matches, serde
            // falls back to the default and the select appears dead.
            "\"aprsChannelMhz\":null",
            "\"aprsSymbolCode\":\">\"",
            "\"aprsSymbolTable\":\"/\"",
            "\"aprsComment\":\"Nexus APRS\"",
            "\"aprsPath\":[\"WIDE1-1\",\"WIDE2-1\"]",
            "\"aprsSsid\":null",
        ] {
            assert!(json.contains(key), "missing wire key {key} in {json}");
        }
    }

    #[test]
    fn lotw_auto_upload_settings_use_the_exact_wire_keys_the_ui_writes() {
        // Same invisible failure as the APRS keys above, with one token that is genuinely
        // contestable: serde emits the acronym ALL LOWERCASE, so `lotw…`. A TS author
        // writing `loTWAutoUpload` or `lotWAutoUpload` by ear would get a key that never
        // matches, a backend that silently keeps the default `false`, and a switch that
        // flips on screen and does nothing. The four shipped LoTW keys (`lotwUsername`,
        // `lotwLastQsl`, `lotwStationLocation`, `lotwUseAdifLocation`) fix the form.
        let json = serde_json::to_string(&Settings::default()).unwrap();
        for key in [
            "\"lotwAutoUpload\":false",
            "\"lotwAutoUploadHours\":6",
            "\"lotwLastAutoUploadUnix\":0",
        ] {
            assert!(json.contains(key), "missing wire key {key} in {json}");
        }
    }

    /// The upgrade path: no shipped settings.json carries these keys, so every existing
    /// operator must land on "off" — an unattended push to ARRL is never something an
    /// upgrade decides on the operator's behalf.
    #[test]
    fn an_old_settings_file_has_lotw_auto_upload_off() {
        let old: Settings =
            serde_json::from_str(r#"{"mycall":"KD9TAW","lotwStationLocation":"HOME"}"#).unwrap();
        assert!(!old.lotw_auto_upload, "must default OFF on upgrade");
        assert_eq!(old.lotw_auto_upload_hours, 6);
        assert_eq!(old.lotw_last_auto_upload_unix, 0, "0 = never run");
    }

    /// ⭐ THE UPGRADE PATH FOR THE APRS RF SETTINGS, and the digipeater path is why it
    /// exists. An empty `aprs_path` is a LEGITIMATE value — "direct, no digipeaters" —
    /// so a bare `#[serde(default)]` on the `Vec` would read a file written before the
    /// field existed as a deliberate choice and silently strip every existing operator's
    /// hops, with nothing on screen to explain it.
    #[test]
    fn an_old_settings_file_keeps_its_digipeater_path_and_its_callsign_ssid() {
        let old: Settings =
            serde_json::from_str(r#"{"mycall":"KD9TAW-9","mygrid":"EN51"}"#).unwrap();
        assert_eq!(
            old.aprs_path,
            vec!["WIDE1-1".to_string(), "WIDE2-1".to_string()],
            "a pre-field file must keep the two-hop path, not read as 'direct'"
        );
        assert_eq!(old.aprs_channel_mhz, None, "None = follow my grid");
        assert_eq!(old.aprs_ssid, None, "None = follow my callsign");
        assert_eq!(old.aprs_symbol_table, "/");
        assert_eq!(old.aprs_symbol_code, ">");
        assert_eq!(old.aprs_comment, "Nexus APRS");

        // The other direction of each gate — an explicit choice must survive, including
        // the empty path, or "direct" would be unrepresentable.
        // `r##`, because the digipeater symbol code is itself a `#`.
        let set: Settings = serde_json::from_str(
            r##"{"aprsPath":[],"aprsChannelMhz":144.8,"aprsSsid":9,"aprsSymbolTable":"\\","aprsSymbolCode":"#"}"##,
        )
        .unwrap();
        assert!(
            set.aprs_path.is_empty(),
            "an explicit empty path means direct"
        );
        assert_eq!(set.aprs_channel_mhz, Some(144.8));
        assert_eq!(set.aprs_ssid, Some(9));
        assert_eq!(set.aprs_symbol_table, "\\");
        assert_eq!(set.aprs_symbol_code, "#");
    }

    /// #53: the CAT broker is the advertised share endpoint, so it must arrive ON — including
    /// for an upgrade from a pre-1.0.5 file that has never heard of the field. The seed order
    /// in `ensure_distinct_radio_ports` puts the broker on 4532 and bumps the per-radio
    /// daemons off it, so an external logger already pointed at 4532 lands on the broker.
    /// A file that RECORDS false (every 1.0.5 save wrote the whole struct) keeps false — the
    /// share block is the affordance for those installs, not a forced migration.
    #[test]
    fn the_cat_broker_arrives_on_and_a_recorded_choice_is_kept() {
        let s = Settings::default();
        assert!(s.cat_broker, "fresh install: broker on");
        assert!(
            s.cat_broker_ptt,
            "shared clients keep keying (operator sign-off 2026-08-09); every TX gate still applies"
        );
        let old: Settings = serde_json::from_str(r#"{"mycall":"KD9TAW","mygrid":"EN51"}"#).unwrap();
        assert!(old.cat_broker, "pre-1.0.5 upgrade: broker on");
        assert!(old.cat_broker_ptt, "pre-1.0.5 upgrade: keying on");
        let recorded: Settings =
            serde_json::from_str(r#"{"catBroker":false,"catBrokerPtt":false}"#).unwrap();
        assert!(
            !recorded.cat_broker,
            "an explicit false in the file is kept"
        );
        assert!(
            !recorded.cat_broker_ptt,
            "an explicit false in the file is kept"
        );
    }

    #[test]
    fn aprs_is_is_off_until_the_operator_asks_and_the_uplink_is_a_second_choice() {
        // Both are outbound connections to a public amateur service under the operator's own
        // callsign, and the uplink PUBLISHES. Neither may ever arrive switched on.
        let s = Settings::default();
        assert!(!s.aprs_is_enabled);
        assert!(!s.aprs_is_uplink);
        // An old config predating APRS-IS must load without either turning itself on.
        let old: Settings = serde_json::from_str(r#"{"mycall":"KD9TAW","mygrid":"EN51"}"#).unwrap();
        assert!(
            !old.aprs_is_enabled,
            "an upgrade must not opt the operator in"
        );
        assert!(!old.aprs_is_uplink);
        // ...but the rest of the feed's defaults are present, so enabling it just works.
        assert_eq!(old.aprs_is_host, "rotate.aprs2.net");
        assert_eq!(old.aprs_is_port, 14580);
        assert_eq!(old.aprs_is_radius_km, 150);
        assert_eq!(
            old.aprs_station_ttl_min, 60,
            "the station window defaults on upgrade"
        );
        assert!(old.aprs_is_weather && old.aprs_is_objects && old.aprs_is_messages);
    }

    #[test]
    fn roundtrips_through_json_camelcase() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"mycall\":\"\"")); // default is empty (set on first run)
        assert!(json.contains("\"fdClass\"") && json.contains("\"pttMethod\""));
        assert!(json.contains("\"wsjtxUdpAddr\"") && json.contains("\"rigModel\""));
        assert!(json.contains("\"txEven\"") && json.contains("\"rxOffsetHz\""));
        assert!(json.contains("\"txOffsetHz\"") && json.contains("\"holdTxFreq\""));
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        assert_eq!(s.dial_hz(), 14_074_000); // default = FT8 20 m (the default mode)
    }

    /// The SSTV section's three fields, on the exact wire keys the UI hand-writes.
    ///
    /// All three carry an interior two-letter acronym, which is the `decodeFlowHz` /
    /// `decodeFLowHz` shape: a TS key written `sstvRXAutoArm` or `sstvTXPowerPct`
    /// compiles clean on both sides, never matches, and the control silently does
    /// nothing while the backend keeps the default. Literal strings, therefore, and
    /// `ui/src/types.ts` + `defaultSettings.json` are copy-pasted from here.
    #[test]
    fn sstv_settings_defaults_and_wire_keys() {
        let s = Settings::default();
        assert!(
            s.sstv_rx_auto_arm,
            "opening the SSTV view arms the receiver — today's behaviour"
        );
        assert_eq!(s.sstv_default_tx_mode, "auto");
        assert_eq!(
            s.sstv_tx_power_pct, None,
            "None = never touch the operator's power"
        );

        let json = serde_json::to_string(&s).unwrap();
        for key in [
            "\"sstvRxAutoArm\":true",
            "\"sstvDefaultTxMode\":\"auto\"",
            "\"sstvTxPowerPct\":null",
        ] {
            assert!(json.contains(key), "missing wire key {key} in {json}");
        }
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);

        // An upgrader's file predates all three keys: nobody's behaviour changes.
        let old: Settings = serde_json::from_str(r#"{"mycall":"W9XYZ"}"#).unwrap();
        assert!(old.sstv_rx_auto_arm);
        assert_eq!(old.sstv_default_tx_mode, "auto");
        assert_eq!(old.sstv_tx_power_pct, None);

        // …and an explicit opt-out survives the round trip (the other direction of the
        // same gate — a `default_true` that ignored the file would pass the line above).
        let off: Settings = serde_json::from_str(
            r#"{"sstvRxAutoArm":false,"sstvDefaultTxMode":"martin1","sstvTxPowerPct":40}"#,
        )
        .unwrap();
        assert!(!off.sstv_rx_auto_arm);
        assert_eq!(off.sstv_default_tx_mode, "martin1");
        assert_eq!(off.sstv_tx_power_pct, Some(40));
    }

    /// The PSK section's one field, on the exact wire key the UI hand-writes —
    /// the same interior-acronym trap the SSTV test above documents
    /// (`pskRXAutoArm` would compile clean on both sides and never match).
    #[test]
    fn psk_settings_default_and_wire_key() {
        let s = Settings::default();
        assert!(
            s.psk_rx_auto_arm,
            "opening the PSK view arms the receiver — the easy-defaults premise"
        );
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            json.contains("\"pskRxAutoArm\":true"),
            "missing wire key pskRxAutoArm in {json}"
        );
        // An upgrader's file predates the key: behaviour unchanged. And an
        // explicit opt-out survives the round trip (a default that ignored the
        // file would pass the first assertion alone).
        let old: Settings = serde_json::from_str(r#"{"mycall":"W9XYZ"}"#).unwrap();
        assert!(old.psk_rx_auto_arm);
        let off: Settings = serde_json::from_str(r#"{"pskRxAutoArm":false}"#).unwrap();
        assert!(!off.psk_rx_auto_arm);
    }

    /// RTTY's auto-arm field, added as the exact twin of the PSK one above — same default,
    /// same `#[serde(default_true)]` shape, same interior-acronym trap on the wire key.
    #[test]
    fn rtty_settings_default_and_wire_key() {
        let s = Settings::default();
        assert!(
            s.rtty_rx_auto_arm,
            "opening the RTTY view arms the receiver — the PSK/SSTV doctrine"
        );
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            json.contains("\"rttyRxAutoArm\":true"),
            "missing wire key rttyRxAutoArm in {json}"
        );
        let old: Settings = serde_json::from_str(r#"{"mycall":"W9XYZ"}"#).unwrap();
        assert!(old.rtty_rx_auto_arm, "an upgrader's file predates the key");
        let off: Settings = serde_json::from_str(r#"{"rttyRxAutoArm":false}"#).unwrap();
        assert!(!off.rtty_rx_auto_arm, "and an explicit opt-out survives");
    }

    /// The tune carrier's own power level, on the exact wire key the UI hand-writes.
    ///
    /// `None` is the whole safety story: an operator who upgrades and never opens the setting
    /// must find their tune-up keying at exactly the level it always did.
    #[test]
    fn tune_power_default_and_wire_key() {
        let s = Settings::default();
        assert_eq!(
            s.tune_power_pct, None,
            "None = never touch the operator's power, which is today's behaviour"
        );
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            json.contains("\"tunePowerPct\":null"),
            "missing wire key tunePowerPct in {json}"
        );
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);

        let old: Settings = serde_json::from_str(r#"{"mycall":"W9XYZ"}"#).unwrap();
        assert_eq!(
            old.tune_power_pct, None,
            "an upgrader's file predates the key"
        );
        let set: Settings = serde_json::from_str(r#"{"tunePowerPct":10}"#).unwrap();
        assert_eq!(set.tune_power_pct, Some(10), "and a set level survives");
    }

    /// ⭐ #145's two declarations: BOTH default to "auto", which is today's behaviour to the
    /// byte — the inference for the handshake, and silence for the keying line. Nobody's
    /// working station may change under them, which is the entire reason they are opt-in
    /// rather than a fourth guess (see the field docs for the first three).
    #[test]
    fn the_cat_line_declarations_default_to_todays_behaviour() {
        let s = Settings::default();
        assert_eq!(s.cat_serial_handshake, "auto");
        assert_eq!(s.cat_ptt_line_state, "auto");
        let json = serde_json::to_string(&s).unwrap();
        for key in [
            "\"catSerialHandshake\":\"auto\"",
            "\"catPttLineState\":\"auto\"",
        ] {
            assert!(json.contains(key), "missing wire key {key} in {json}");
        }
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);

        // An upgrader's file predates both keys.
        let old: Settings = serde_json::from_str(r#"{"mycall":"W9XYZ"}"#).unwrap();
        assert_eq!(old.cat_serial_handshake, "auto");
        assert_eq!(old.cat_ptt_line_state, "auto");
        // …and an explicit declaration survives the round trip — the other direction of the
        // same gate, which a default that ignored the file would pass on its own.
        let set: Settings =
            serde_json::from_str(r#"{"catSerialHandshake":"none","catPttLineState":"low"}"#)
                .unwrap();
        assert_eq!(set.cat_serial_handshake, "none");
        assert_eq!(set.cat_ptt_line_state, "low");
    }

    #[test]
    fn monitor_defaults_and_roundtrip() {
        let s = Settings::default();
        assert!(!s.monitor_enabled, "monitor ships DARK (off by default)");
        assert_eq!(s.monitor_device, "");
        assert_eq!(s.monitor_level, 0.5);
        // Round-trips as camelCase and reloads identically.
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"monitorEnabled\":false"));
        assert!(json.contains("\"monitorLevel\":0.5"));
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
        // An old settings file without the monitor keys still loads (serde defaults).
        let partial = r#"{"mycall":"W9XYZ","audioOut":"USB CODEC"}"#;
        let old: Settings = serde_json::from_str(partial).unwrap();
        assert!(!old.monitor_enabled);
        assert_eq!(old.monitor_level, 0.5);
    }

    /// The decode-config settings ride the UI round-trip on exactly the keys
    /// the frontend writes (the `decodeFLowHz` rename trap is why wire keys are
    /// asserted literally), and an old settings file without them loads with
    /// stock WSJT-X behaviour — absent = current behaviour, the decode-parity
    /// contract for this surface.
    #[test]
    fn decode_config_defaults_and_roundtrip() {
        let s = Settings::default();
        assert!(s.ap_decode, "stock: Enable AP on");
        assert!(!s.ap_cq_only, "stock: all AP hypotheses");
        assert!(!s.single_decode, "stock: full-passband decode");
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"apDecode\":true"));
        assert!(json.contains("\"apCqOnly\":false"));
        assert!(json.contains("\"singleDecode\":false"));
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
        // A pre-decode-config settings file: stock behaviour, not zeroed bools.
        let partial = r#"{"mycall":"W9XYZ"}"#;
        let old: Settings = serde_json::from_str(partial).unwrap();
        assert!(old.ap_decode, "absent apDecode must default ON (stock)");
        assert!(!old.ap_cq_only);
        assert!(!old.single_decode);
    }

    #[test]
    fn voice_mic_device_defaults_and_roundtrips() {
        let s = Settings::default();
        assert_eq!(
            s.voice_mic_device, "",
            "empty default = record from the shared input (today's behavior)"
        );
        // Round-trips as camelCase and reloads identically.
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"voiceMicDevice\":\"\""));
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
        // An old settings file without the key still loads (serde default → empty).
        let partial = r#"{"mycall":"W9XYZ","audioIn":"USB CODEC"}"#;
        let old: Settings = serde_json::from_str(partial).unwrap();
        assert_eq!(old.voice_mic_device, "");
        // A configured mic survives a save/load round-trip.
        let mut s2 = Settings::default();
        s2.voice_mic_device = "USB Microphone".into();
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s2).unwrap()).unwrap();
        assert_eq!(back.voice_mic_device, "USB Microphone");
    }

    #[test]
    fn partial_json_fills_defaults() {
        // An old/partial settings file with only identity fields still loads.
        let partial = r#"{"mycall":"W9XYZ","mygrid":"EN37"}"#;
        let s: Settings = serde_json::from_str(partial).unwrap();
        assert_eq!(s.mycall, "W9XYZ");
        assert_eq!(s.ptt_method, "vox"); // default
        assert_eq!(s.rigctld_port, 4534); // default — broker owns 4532, rotctld 4533 (#53)
        assert_eq!(s.wsjtx_udp_addr, "127.0.0.1:2237"); // default
    }

    #[test]
    fn fd_sync_settings_round_trip_and_default_safe() {
        // A pre-sync settings file: hosting OFF (the LAN bind is opt-in and
        // an upgrade must never open a port), the default port, no identity.
        let partial = r#"{"mycall":"W9XYZ","mygrid":"EN37"}"#;
        let s: Settings = serde_json::from_str(partial).unwrap();
        assert!(!s.fd_host_enable, "an upgrade never turns hosting on");
        assert_eq!(s.fd_host_port, 42073);
        assert_eq!(s.fd_join_addr, "");
        assert_eq!(s.fd_position_id, "");

        let path = std::env::temp_dir()
            .join("tempo_settings_fdsync")
            .join("settings.json");
        let s = Settings {
            fd_host_enable: true,
            fd_host_port: 42111,
            fd_event_name: "W9ABC Field Day".into(),
            fd_join_addr: "192.168.1.10:42073".into(),
            fd_position_name: "CW tent".into(),
            fd_position_id: "a1b2c3d4".into(),
            ..Settings::default()
        };
        s.save(&path).unwrap();
        let back = Settings::load(&path);
        assert!(back.fd_host_enable);
        assert_eq!(back.fd_host_port, 42111);
        assert_eq!(back.fd_event_name, "W9ABC Field Day");
        assert_eq!(back.fd_join_addr, "192.168.1.10:42073");
        assert_eq!(back.fd_position_name, "CW tent");
        assert_eq!(back.fd_position_id, "a1b2c3d4");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn fd_scoreboard_settings_default_off_and_round_trip() {
        // A pre-scoreboard settings file: the board OFF (serving a LAN page is
        // opt-in and an upgrade must never open a port), the default port.
        let partial = r#"{"mycall":"W9XYZ","mygrid":"EN37"}"#;
        let s: Settings = serde_json::from_str(partial).unwrap();
        assert!(!s.fd_scoreboard, "an upgrade never turns the board on");
        assert_eq!(s.fd_scoreboard_port, 7373);

        let path = std::env::temp_dir()
            .join("tempo_settings_fdboard")
            .join("settings.json");
        let s = Settings {
            fd_scoreboard: true,
            fd_scoreboard_port: 7474,
            ..Settings::default()
        };
        s.save(&path).unwrap();
        let back = Settings::load(&path);
        assert!(back.fd_scoreboard);
        assert_eq!(back.fd_scoreboard_port, 7474);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn save_then_load() {
        let path = std::env::temp_dir()
            .join("tempo_settings_test2")
            .join("settings.json");
        let s = Settings {
            mycall: "W9XYZ".into(),
            serial_port: "/dev/ttyUSB0".into(),
            ptt_method: "cat".into(),
            ..Settings::default()
        };
        s.save(&path).unwrap();
        let back = Settings::load(&path);
        assert_eq!(back.mycall, "W9XYZ");
        assert_eq!(back.serial_port, "/dev/ttyUSB0");
        assert_eq!(back.ptt_method, "cat");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_persists_via_temp_rename_leaving_no_tmp() {
        // save() writes a sibling `.tmp` then renames it onto the target, so a save is
        // all-or-nothing (a crash mid-write can't truncate the live file). After a
        // successful save the temp file must be gone (renamed into place).
        let dir = std::env::temp_dir().join("tempo_settings_atomic");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");
        let s = Settings {
            mycall: "W9XYZ".into(),
            ..Settings::default()
        };
        s.save(&path).unwrap();
        assert!(path.exists(), "settings.json written");
        assert!(
            !path.with_extension("json.tmp").exists(),
            "temp file renamed away, none left behind"
        );
        assert_eq!(Settings::load(&path).mycall, "W9XYZ");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_save_preserves_prior_good_settings() {
        // A save that fails mid-write must NOT clobber the previously-saved good file.
        // Because we write-tmp then rename, a failing tmp write returns Err before the
        // rename, so settings.json is untouched — the operator's callsign, license_class
        // (the Part 97 TX lockout), and rig config survive instead of collapsing to
        // Settings::default() (license = Open → lockout removed) on the next load.
        let dir = std::env::temp_dir().join("tempo_settings_torn");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");
        let good = Settings {
            mycall: "W9XYZ".into(),
            license_class: LicenseClass::Technician,
            serial_port: "/dev/ttyUSB0".into(),
            ..Settings::default()
        };
        good.save(&path).unwrap();
        // Block the sibling temp path (a directory can't be overwritten by write()), a
        // stand-in for a torn write / full disk / power loss at the write-tmp step.
        let tmp = path.with_extension("json.tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        let doomed = Settings {
            mycall: "OTHER".into(),
            ..Settings::default()
        };
        assert!(
            doomed.save(&path).is_err(),
            "save whose tmp write fails returns Err"
        );
        // The prior good config is intact — never overwritten, never reset to defaults.
        let back = Settings::load(&path);
        assert_eq!(
            back.mycall, "W9XYZ",
            "callsign preserved after a failed save"
        );
        assert_eq!(
            back.license_class,
            LicenseClass::Technician,
            "TX lockout (license class) preserved, not reset to Open"
        );
        assert_eq!(back.serial_port, "/dev/ttyUSB0", "rig config preserved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_preserves_corrupt_file_instead_of_silently_defaulting() {
        // A present-but-corrupt settings.json must NOT be silently collapsed to
        // Settings::default() — that's indistinguishable from a first run, wipes the
        // operator's callsign/rig config, and resets license_class to Open (re-opening
        // TX privileges). load() must set the bad file aside as a sibling `.corrupt`
        // file so the operator (or support) can recover it, then fall back to defaults.
        let dir = std::env::temp_dir().join("tempo_settings_corrupt");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");
        let good = Settings {
            mycall: "W9XYZ".into(),
            license_class: LicenseClass::Technician,
            ..Settings::default()
        };
        good.save(&path).unwrap();
        // Simulate a torn write / disk corruption of the live file.
        let truncated = r#"{"mycall":"W9X"#;
        std::fs::write(&path, truncated).unwrap();
        let back = Settings::load(&path);
        assert_eq!(back.mycall, "", "corrupt file falls back to defaults");
        assert_eq!(back.license_class, LicenseClass::Open);
        let corrupt = path.with_extension("json.corrupt");
        assert!(
            corrupt.exists(),
            "corrupt settings.json set aside for recovery, not discarded"
        );
        assert_eq!(
            std::fs::read_to_string(&corrupt).unwrap(),
            truncated,
            "the .corrupt file holds the original bad bytes"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn load_sets_aside_an_unreadable_file_instead_of_leaving_it_for_save_to_clobber() {
        // An UNREADABLE (permissions / AV-locked) settings.json is not a first run
        // either: if load() just defaulted and left the intact file in place, the
        // session's first save() would clobber the operator's real config with
        // defaults once the lock cleared. load() must set the file aside like the
        // corrupt case. (unix-only: permission bits don't model a Windows lock,
        // but they exercise the same read-Err arm.)
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join("tempo_settings_unreadable");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");
        let good = Settings {
            mycall: "W9XYZ".into(),
            license_class: LicenseClass::Technician,
            ..Settings::default()
        };
        good.save(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let back = Settings::load(&path);
        assert_eq!(back.mycall, "", "unreadable file falls back to defaults");
        let corrupt = path.with_extension("json.corrupt");
        assert!(
            corrupt.exists(),
            "the intact-but-unreadable file is set aside, not left for save() to clobber"
        );
        let _ = std::fs::set_permissions(&corrupt, std::fs::Permissions::from_mode(0o600));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_drops_stale_cq_macros_but_keeps_custom() {
        let path = std::env::temp_dir()
            .join("tempo_settings_cqmacro")
            .join("settings.json");
        let mut s = Settings::default();
        s.macros.band = vec!["CQ CQ".into(), "QRZ?".into(), "73 to all".into()];
        s.macros.chat = vec!["73".into(), "CQ".into(), "QSL".into()];
        s.save(&path).unwrap();
        let back = Settings::load(&path);
        assert!(
            !back.macros.band.iter().any(|m| m == "CQ CQ"),
            "stale CQ CQ dropped"
        );
        assert!(
            back.macros.band.iter().any(|m| m == "QRZ?"),
            "custom band macro kept"
        );
        assert!(
            !back.macros.chat.iter().any(|m| m == "CQ"),
            "stale chat CQ dropped"
        );
        assert!(
            back.macros.chat.iter().any(|m| m == "73"),
            "custom chat macro kept"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn migrate_cw_profiles_seeds_default_from_legacy_cw_and_is_idempotent() {
        let mut m = Macros {
            cw: vec![
                CwMacroDef {
                    key: "F1".into(),
                    label: "CQ".into(),
                    text: "CQ CQ DE {MYCALL}".into(),
                },
                CwMacroDef {
                    key: "F2".into(),
                    label: "Rprt".into(),
                    text: "! {RST}".into(),
                },
            ],
            ..Macros::default()
        };
        m.migrate_cw_profiles();
        assert_eq!(m.cw_profiles.len(), 1, "seeds exactly one profile");
        assert_eq!(m.cw_profiles[0].name, "Default");
        assert_eq!(m.cw_profiles[0].macros.len(), 2, "legacy macros carried in");
        assert_eq!(m.cw_profiles[0].macros[0].key, "F1");
        assert_eq!(m.active_cw_profile, 0);
        assert!(m.cw.is_empty(), "legacy cw cleared after migration");

        // A 2nd call must not re-seed, duplicate, or resurrect the legacy list.
        let before = m.cw_profiles.clone();
        m.migrate_cw_profiles();
        assert_eq!(m.cw_profiles, before, "idempotent");
        assert!(m.cw.is_empty());
    }

    #[test]
    fn migrate_cw_profiles_seeds_empty_default_when_no_legacy_macros() {
        let mut m = Macros::default(); // fresh: legacy cw empty, no profiles
        m.migrate_cw_profiles();
        assert_eq!(m.cw_profiles.len(), 1);
        assert_eq!(m.cw_profiles[0].name, "Default");
        assert!(
            m.cw_profiles[0].macros.is_empty(),
            "empty legacy → empty Default (cockpit uses built-in defaults)"
        );
    }

    #[test]
    fn active_cw_macros_returns_the_active_profiles_macros() {
        let mac = |k: &str| CwMacroDef {
            key: k.into(),
            label: k.into(),
            text: k.into(),
        };
        let m = Macros {
            cw_profiles: vec![
                CwMacroProfile {
                    name: "Alice".into(),
                    macros: vec![mac("F1"), mac("F2")],
                },
                CwMacroProfile {
                    name: "Bob".into(),
                    macros: vec![mac("F3")],
                },
            ],
            active_cw_profile: 1,
            ..Macros::default()
        };
        assert_eq!(m.active_cw_macros().len(), 1);
        assert_eq!(m.active_cw_macros()[0].key, "F3");
    }

    #[test]
    fn active_cw_macros_clamps_out_of_range_index() {
        // A corrupt/stale active index must never panic — migrate clamps it, and the
        // accessor also falls back to an empty slice for an unmigrated Macros.
        let mut m = Macros {
            cw_profiles: vec![CwMacroProfile {
                name: "Default".into(),
                macros: vec![CwMacroDef {
                    key: "F1".into(),
                    label: "CQ".into(),
                    text: "CQ".into(),
                }],
            }],
            active_cw_profile: 9, // out of range
            ..Macros::default()
        };
        // Accessor is safe even before clamping (empty-slice fallback).
        assert!(m.active_cw_macros().is_empty());
        m.migrate_cw_profiles();
        assert_eq!(m.active_cw_profile, 0, "clamped into range");
        assert_eq!(m.active_cw_macros().len(), 1);

        // A bare/default Macros with no profiles also yields an empty slice, no panic.
        assert!(Macros::default().active_cw_macros().is_empty());
    }

    #[test]
    fn load_migrates_legacy_cw_into_a_default_profile() {
        let path = std::env::temp_dir()
            .join("tempo_settings_cwprofiles")
            .join("settings.json");
        let mut s = Settings::default();
        s.macros.cw_profiles.clear(); // force the legacy (unmigrated) shape
        s.macros.active_cw_profile = 0;
        s.macros.cw = vec![CwMacroDef {
            key: "F1".into(),
            label: "CQ".into(),
            text: "CQ CQ DE {MYCALL}".into(),
        }];
        s.save(&path).unwrap();
        let back = Settings::load(&path);
        assert_eq!(back.macros.cw_profiles.len(), 1);
        assert_eq!(back.macros.cw_profiles[0].name, "Default");
        assert_eq!(back.macros.cw_profiles[0].macros.len(), 1);
        assert_eq!(back.macros.cw_profiles[0].macros[0].key, "F1");
        assert!(back.macros.cw.is_empty(), "legacy cw cleared on load");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cw_profiles_survive_a_settings_round_trip() {
        let path = std::env::temp_dir()
            .join("tempo_settings_cwprofile_rt")
            .join("settings.json");
        let mut s = Settings::default();
        s.macros.cw_profiles = vec![
            CwMacroProfile {
                name: "Alice".into(),
                macros: vec![CwMacroDef {
                    key: "F1".into(),
                    label: "CQ".into(),
                    text: "CQ DE {MYCALL}".into(),
                }],
            },
            CwMacroProfile {
                name: "Bob".into(),
                macros: vec![CwMacroDef {
                    key: "F2".into(),
                    label: "73".into(),
                    text: "73".into(),
                }],
            },
        ];
        s.macros.active_cw_profile = 1;
        s.save(&path).unwrap();
        let back = Settings::load(&path);
        assert_eq!(
            back.macros.cw_profiles.len(),
            2,
            "both named profiles preserved"
        );
        assert_eq!(back.macros.cw_profiles[0].name, "Alice");
        assert_eq!(back.macros.cw_profiles[1].name, "Bob");
        assert_eq!(back.macros.active_cw_profile, 1, "active index preserved");
        assert_eq!(back.macros.active_cw_macros()[0].key, "F2");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_migrates_old_rbn_cluster_host_to_a_human_node() {
        // cluster_host used to BE the RBN endpoint (digital-only) — that's why CW/Phone
        // needs never appeared. RBN is now wired automatically; an old RBN value must
        // migrate to a human node so SSB/phone spots start flowing.
        let path = std::env::temp_dir()
            .join("tempo_settings_clustermig")
            .join("settings.json");
        let mut s = Settings::default();
        s.cluster_host = "telnet.reversebeacon.net:7001".into();
        s.save(&path).unwrap();
        let back = Settings::load(&path);
        assert!(
            !back.cluster_host.contains("reversebeacon.net"),
            "old RBN cluster_host migrated to a human node, got {:?}",
            back.cluster_host
        );
        assert!(
            !back.cluster_host.is_empty(),
            "migrated to a real node, not blank"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_seeds_cluster_hosts_from_legacy_single_host() {
        // An upgrading config has a single cluster_host but an empty cluster_hosts list
        // (the field is new); load must seed the aggregator from the legacy host so the
        // operator's node isn't lost.
        let path = std::env::temp_dir()
            .join("tempo_settings_hostsmig")
            .join("settings.json");
        let mut s = Settings::default();
        s.cluster_hosts = vec![]; // simulate a pre-aggregator config
        s.cluster_host = "dxc.example.net:7300".into();
        s.save(&path).unwrap();
        let back = Settings::load(&path);
        assert_eq!(back.cluster_hosts, vec!["dxc.example.net:7300".to_string()]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_sanitizes_cluster_hosts_list() {
        // The aggregator list must never contain RBN endpoints (auto-wired), blanks, or
        // dups — load strips them, preserving order and the first occurrence.
        let path = std::env::temp_dir()
            .join("tempo_settings_hostssan")
            .join("settings.json");
        let mut s = Settings::default();
        s.cluster_hosts = vec![
            " ve7cc.net:23 ".into(),                // trimmed
            "telnet.reversebeacon.net:7000".into(), // RBN → dropped
            "VE7CC.NET:23".into(),                  // case-insensitive dup → dropped
            "".into(),                              // blank → dropped
            "dxc.example.net:7300".into(),
        ];
        s.save(&path).unwrap();
        let back = Settings::load(&path);
        assert_eq!(
            back.cluster_hosts,
            vec![
                "ve7cc.net:23".to_string(),
                "dxc.example.net:7300".to_string()
            ]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_recovers_a_disabled_legacy_rbn_config() {
        // The exact stale state that silently killed phone spots: a pre-multi-cluster config
        // whose single `cluster_host` is an RBN port, an empty `cluster_hosts` list, and the
        // whole subsystem left DISABLED. Load must rewrite the host to a human node, RE-ENABLE
        // the cluster, and seed both default human nodes (incl. the port-23 fallback) so phone
        // flows — otherwise fixing the host alone leaves the operator with no spots at all.
        let path = std::env::temp_dir()
            .join("tempo_settings_legacyrbn")
            .join("settings.json");
        let mut s = Settings::default();
        s.cluster_enabled = false;
        s.cluster_host = "telnet.reversebeacon.net:7001".into();
        s.cluster_hosts = vec![]; // pre-aggregator config: no human node
        s.save(&path).unwrap();
        let back = Settings::load(&path);
        assert!(back.cluster_enabled, "re-enabled the disabled cluster");
        assert!(
            !back.cluster_host.contains("reversebeacon.net"),
            "RBN host rewritten to a human node, got {:?}",
            back.cluster_host
        );
        assert!(
            back.cluster_hosts.iter().any(|h| h.contains("ve7cc")),
            "seeded the human node so phone flows: {:?}",
            back.cluster_hosts
        );
        assert!(
            back.cluster_hosts.iter().any(|h| h.contains("wa9pie")),
            "seeded the port-23-blocked fallback too: {:?}",
            back.cluster_hosts
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_leaves_a_deliberately_disabled_modern_config_alone() {
        // Guard the migration's scope: a MODERN config (human host, no RBN signature) that the
        // operator deliberately disabled must stay disabled — the re-enable is only for the
        // legacy RBN-host signature, never a blanket override of the operator's choice.
        let path = std::env::temp_dir()
            .join("tempo_settings_moderndisabled")
            .join("settings.json");
        let mut s = Settings::default();
        s.cluster_enabled = false;
        s.cluster_host = "ve7cc.net:23".into();
        s.cluster_hosts = vec!["ve7cc.net:23".into()];
        s.save(&path).unwrap();
        let back = Settings::load(&path);
        assert!(
            !back.cluster_enabled,
            "a deliberately-disabled modern config stays disabled"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn migrates_a_flat_config_to_a_single_radio_profile() {
        // An older settings.json (no `radios`) loads as exactly one profile mirroring the flat
        // rig/audio fields; the flat fields stay identical (single-radio behavior unchanged).
        let path = std::env::temp_dir()
            .join("tempo_settings_radiomigrate")
            .join("settings.json");
        let mut legacy = Settings::default();
        legacy.rig_model = 1042;
        legacy.rig_model_name = "Yaesu FTDX10".into();
        legacy.serial_port = "COM5".into();
        legacy.audio_in = "USB Audio CODEC".into();
        legacy.radios = Vec::new(); // force the legacy (unmigrated) shape
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string(&legacy).unwrap()).unwrap();

        let s = Settings::load(&path);
        assert_eq!(s.radios.len(), 1, "migrated to exactly one profile");
        let p = &s.radios[0];
        assert_eq!(p.id, 0);
        assert_eq!(p.rig_model, 1042);
        assert_eq!(p.name, "Yaesu FTDX10");
        assert_eq!(p.serial_port, "COM5");
        assert_eq!(p.audio_in, "USB Audio CODEC");
        assert_eq!(p.rotctld_port, 4533);
        assert_eq!(s.active_radio, 0);
        // Flat mirror unchanged — every existing consumer reads it as before.
        assert_eq!(s.rig_model, 1042);
        assert_eq!(s.serial_port, "COM5");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_mirrors_a_flat_edit_into_the_active_profile() {
        // The mirror invariant: editing the flat rig fields (today's UI) and saving persists the
        // edit into the active profile, so a reload preserves it.
        let path = std::env::temp_dir()
            .join("tempo_settings_radiomirror")
            .join("settings.json");
        let mut s = Settings::default();
        s.ensure_radio_profiles();
        s.rig_model = 3081;
        s.rig_model_name = "Icom IC-9700".into();
        s.serial_port = "COM7".into();
        s.save(&path).unwrap();

        let back = Settings::load(&path);
        assert_eq!(back.radios.len(), 1);
        assert_eq!(
            back.radios[0].rig_model, 3081,
            "flat edit persisted into the active profile"
        );
        assert_eq!(back.radios[0].rig_model_name, "Icom IC-9700"); // synced flat field
        assert_eq!(back.rig_model, 3081, "flat mirror intact after reload");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn validate_radio_ports_rejects_duplicate_ports() {
        let a = RadioProfile {
            id: 0,
            name: "A".into(),
            rigctld_port: 4532,
            ..Default::default()
        };
        let b = RadioProfile {
            id: 1,
            name: "B".into(),
            rigctld_port: 4532,
            ..Default::default()
        };
        assert!(
            validate_radio_ports(&[a.clone(), b.clone()], None).is_err(),
            "same rigctld port"
        );
        let b2 = RadioProfile {
            rigctld_port: 4534,
            ..b
        };
        assert!(
            validate_radio_ports(&[a.clone(), b2], None).is_ok(),
            "distinct ports OK"
        );
        assert!(
            validate_radio_ports(&[a], Some(4532)).is_err(),
            "broker collides with a rig"
        );
    }

    #[test]
    fn serial_port_conflicts_flags_two_radios_on_one_com() {
        let rig = |name: &str, port: &str, conn: &str, model: u32, enabled: bool| RadioProfile {
            name: name.into(),
            serial_port: port.into(),
            rig_conn: conn.into(),
            rig_model: model,
            enabled,
            ..Default::default()
        };
        let ftdx = rig("FTDX10", "COM3", "serial", 1042, true);
        // Same COM port (case-insensitive) → a conflict message naming both radios.
        let ic = rig("IC-9700", "com3", "serial", 23005, true);
        let msg = serial_port_conflicts(&[ftdx.clone(), ic.clone()]).expect("conflict expected");
        assert!(
            msg.contains("FTDX10") && msg.contains("IC-9700"),
            "message names both radios: {msg}"
        );
        // Distinct ports → no conflict.
        assert!(serial_port_conflicts(&[
            ftdx.clone(),
            rig("IC-9700", "COM5", "serial", 23005, true)
        ])
        .is_none());
        // A disabled radio is ignored.
        assert!(serial_port_conflicts(&[
            ftdx.clone(),
            rig("IC-9700", "COM3", "serial", 23005, false)
        ])
        .is_none());
        // Network CAT doesn't own a COM port.
        assert!(serial_port_conflicts(&[
            ftdx.clone(),
            rig("IC-9700", "COM3", "network", 23005, true)
        ])
        .is_none());
        // VOX / no-rig (model 0) doesn't count.
        assert!(serial_port_conflicts(&[ftdx, rig("VOX", "COM3", "serial", 0, true)]).is_none());
    }

    #[test]
    fn audio_device_conflicts_flags_two_radios_on_one_codec() {
        let rig = |name: &str, ain: &str, aout: &str, enabled: bool| RadioProfile {
            name: name.into(),
            audio_in: ain.into(),
            audio_out: aout.into(),
            enabled,
            ..Default::default()
        };
        // The operator's actual hardware: two rigs that BOTH enumerate as "USB Audio CODEC". The
        // stored names carry the picker's " #N" ordinal, so these are distinct devices → no warning.
        assert!(audio_device_conflicts(&[
            rig("FTDX10", "USB Audio CODEC", "USB Audio CODEC", true),
            rig("IC-9700", "USB Audio CODEC #2", "USB Audio CODEC #2", true),
        ])
        .is_none());
        // The same codec on both radios: chain 2 would decode chain 1's receiver.
        let msg = audio_device_conflicts(&[
            rig("FTDX10", "USB Audio CODEC", "Speakers", true),
            rig("IC-9700", "usb audio codec", "Headphones", true),
        ])
        .expect("input conflict expected");
        assert!(
            msg.contains("FTDX10") && msg.contains("IC-9700") && msg.contains("input"),
            "message names both radios and the direction: {msg}"
        );
        // Outputs collide independently of inputs — both rigs would be fed both chains' TX audio.
        let msg = audio_device_conflicts(&[
            rig("FTDX10", "CODEC A", "USB Audio CODEC", true),
            rig("IC-9700", "CODEC B", "USB Audio CODEC", true),
        ])
        .expect("output conflict expected");
        assert!(msg.contains("output"), "{msg}");
        // A disabled radio is ignored.
        assert!(audio_device_conflicts(&[
            rig("FTDX10", "USB Audio CODEC", "USB Audio CODEC", true),
            rig("IC-9700", "USB Audio CODEC", "USB Audio CODEC", false),
        ])
        .is_none());
        // Blank = "system default", which a CAT-only second radio has and never opens; not compared.
        assert!(audio_device_conflicts(&[
            rig("FTDX10", "", "", true),
            rig("IC-9700", "", "", true),
        ])
        .is_none());
    }

    #[test]
    fn cw_key_port_conflict_flags_keyline_on_the_cat_port() {
        let rig = |name: &str, port: &str, conn: &str, model: u32, enabled: bool| RadioProfile {
            name: name.into(),
            serial_port: port.into(),
            rig_conn: conn.into(),
            rig_model: model,
            enabled,
            ..Default::default()
        };
        let radios = [rig("FTDX10", "COM3", "serial", 1042, true)];
        // Serial keyer pointed at the CAT port (case-insensitive) → warn, naming the radio.
        let msg = cw_key_port_conflict(CwKeyerBackend::Serial, "com3", &radios).expect("conflict");
        assert!(
            msg.contains("FTDX10") && msg.contains("COM3") || msg.contains("com3"),
            "{msg}"
        );
        // A separate key port → fine.
        assert!(cw_key_port_conflict(CwKeyerBackend::Serial, "COM7", &radios).is_none());
        // Only when the serial keyer is the selected backend.
        assert!(cw_key_port_conflict(CwKeyerBackend::Cat, "COM3", &radios).is_none());
        assert!(cw_key_port_conflict(CwKeyerBackend::WinKeyer, "COM3", &radios).is_none());
        // Empty key port → nothing to collide.
        assert!(cw_key_port_conflict(CwKeyerBackend::Serial, "", &radios).is_none());
        // Network-CAT radio owns no COM port, so no collision even on a matching string.
        let net = [rig("Flex", "COM3", "network", 2036, true)];
        assert!(cw_key_port_conflict(CwKeyerBackend::Serial, "COM3", &net).is_none());
    }

    /// ⭐ SERIAL PORTS ARE EXCLUSIVE-OPEN, and the amplifier poller takes a hold on `amp_port`
    /// the moment it is configured. An operator who types their CAT port into the amplifier
    /// field gets a CAT failure — a dead radio — and will report it as a radio bug, because
    /// nothing on screen connects the two. `serial_port_conflicts` cannot see this: it filters
    /// on `serial_port` alone and never looks at `amp_port`.
    #[test]
    fn amp_port_conflict_flags_an_amplifier_sharing_a_port_with_the_radio() {
        let rig = |name: &str, port: &str, amp: &str| RadioProfile {
            name: name.into(),
            serial_port: port.into(),
            rig_conn: "serial".into(),
            rig_model: 1042,
            amp_model: "spe".into(),
            amp_port: amp.into(),
            enabled: true,
            ..Default::default()
        };

        // The amplifier on this radio's OWN CAT port, case-insensitively.
        let radios = [rig("FTDX10", "COM3", "com3")];
        let msg = amp_port_conflict(&radios, "", "", "").expect("conflict");
        assert!(msg.contains("FTDX10"), "the message names the radio: {msg}");
        assert!(msg.to_lowercase().contains("com3"), "and the port: {msg}");

        // CONTROL, and it must NOT trip: a separate port for the amplifier is the normal
        // station and must stay silent, or the warning lane cries wolf for everyone.
        assert!(
            amp_port_conflict(&[rig("FTDX10", "COM3", "COM7")], "", "", "").is_none(),
            "an amplifier on its own port is the ordinary case"
        );

        // The OTHER radio's CAT port — an SO2R station's amplifier pointed at radio 2.
        let so2r = [rig("FTDX10", "COM3", "COM4"), rig("IC-7300", "COM4", "")];
        assert!(amp_port_conflict(&so2r, "", "", "").is_some());

        // Its own rotator's port.
        let mut rot = rig("FTDX10", "COM3", "COM8");
        rot.rotator_model = 401;
        rot.rotator_port = "COM8".into();
        assert!(amp_port_conflict(&[rot], "", "", "").is_some());

        // The global auxiliary serial devices, one at a time.
        let aux = [rig("FTDX10", "COM3", "COM9")];
        assert!(
            amp_port_conflict(&aux, "COM9", "", "").is_some(),
            "CW keyline"
        );
        assert!(
            amp_port_conflict(&aux, "", "COM9", "").is_some(),
            "WinKeyer"
        );
        assert!(
            amp_port_conflict(&aux, "", "", "COM9").is_some(),
            "RTTY FSK"
        );

        // No amplifier configured — nothing to collide, whatever the ports say.
        let none = [rig("FTDX10", "COM3", "")];
        assert!(amp_port_conflict(&none, "COM3", "", "").is_none());

        // A network-CAT radio owns no COM port, so a matching string is not a collision.
        let mut net = rig("Flex", "COM3", "COM3");
        net.rig_conn = "network".into();
        assert!(
            amp_port_conflict(&[net], "", "", "").is_none(),
            "the radio never opens COM3, so the amplifier may have it"
        );

        // A DISABLED radio's ports are not held.
        let mut off = rig("FTDX10", "COM3", "");
        off.enabled = false;
        let live = rig("IC-7300", "COM7", "COM3");
        assert!(amp_port_conflict(&[off, live], "", "", "").is_none());
    }

    #[test]
    fn add_radio_profile_assigns_a_fresh_id_and_distinct_ports() {
        // Adding a 2nd radio must never collide daemon ports with radio 1 (or the CAT broker) — two
        // rigctld/rotctld instances can't bind the same TCP port.
        let mut s = Settings::default();
        s.ensure_radio_profiles(); // radio 0 on the shipped defaults
        let r0 = s.radios[0].clone();
        // Point the broker somewhere unusual so "dodges the broker" is a real assertion,
        // not a coincidence of the defaults.
        s.cat_broker = true;
        s.cat_broker_port = r0.rigctld_port + 7;
        let id = s.add_radio_profile();
        assert_eq!(s.radios.len(), 2);
        assert_eq!(id, 1, "fresh, non-reused id");
        let new = s.radios.iter().find(|p| p.id == id).unwrap();
        assert_eq!(new.name, "Radio 2");
        // Distinct from radio 0's pair AND the broker.
        assert_ne!(new.rigctld_port, r0.rigctld_port);
        assert_ne!(new.rigctld_port, r0.rotctld_port);
        assert_ne!(
            new.rigctld_port, s.cat_broker_port,
            "dodges the CAT broker port too"
        );
        assert_ne!(new.rigctld_port, new.rotctld_port);
        // The whole roster must pass the port validator (broker included).
        assert!(validate_radio_ports(&s.radios, Some(s.cat_broker_port)).is_ok());
    }

    #[test]
    fn ensure_distinct_radio_ports_repairs_collisions() {
        // Two live daemons (true dual-radio) need distinct ports; an old/hand-edited config that
        // shares one is auto-repaired on load. The CAT broker (ON by default since #53) is
        // seeded FIRST and owns its port — 4532, Hamlib's own NET rigctl number, deliberately:
        // an external logger already pointed at 4532 lands on the broker after upgrade. Every
        // per-radio daemon is therefore bumped off it.
        let mut s = Settings::default();
        s.ensure_radio_profiles();
        let r0_port = s.radios[0].rigctld_port;
        let r1 = s.add_radio_profile();
        s.radios
            .iter_mut()
            .find(|p| p.id == r1)
            .unwrap()
            .rigctld_port = r0_port; // force a collision with radio 0
        assert!(validate_radio_ports(&s.radios, None).is_err());
        s.ensure_distinct_radio_ports();
        assert!(
            validate_radio_ports(&s.radios, None).is_ok(),
            "collision repaired"
        );
        for p in &s.radios {
            assert_ne!(
                p.rigctld_port, s.cat_broker_port,
                "no daemon may sit on the broker's port (radio {})",
                p.id
            );
        }
        let (p0, p1) = (
            s.radios.iter().find(|p| p.id == 0).unwrap().rigctld_port,
            s.radios.iter().find(|p| p.id == r1).unwrap().rigctld_port,
        );
        assert_ne!(p0, p1, "the colliding radios ended up distinct");
        // With the broker OFF, the old contract stands: the first radio keeps its own port.
        let mut s2 = Settings {
            cat_broker: false,
            ..Settings::default()
        };
        s2.ensure_radio_profiles();
        let before = s2.radios[0].rigctld_port;
        s2.ensure_distinct_radio_ports();
        assert_eq!(
            s2.radios.iter().find(|p| p.id == 0).unwrap().rigctld_port,
            before,
            "broker off: first radio keeps its port"
        );
        // And an operator file that predates the broker default — daemon EXPLICITLY on 4532 —
        // is bumped off it on load, which is what hands 4532 to the broker for their logger.
        let mut s3 = Settings::default();
        s3.ensure_radio_profiles();
        s3.radios[0].rigctld_port = 4532;
        s3.ensure_distinct_radio_ports();
        assert_ne!(
            s3.radios[0].rigctld_port, 4532,
            "a pre-broker file's daemon moves off 4532 so the broker can own it"
        );
    }

    #[test]
    fn ensure_distinct_radio_ports_repairs_zero_port() {
        // Regression: a profile with rigctld_port == 0 (e.g. an older/imported config) is "distinct"
        // from its siblings, so validate_radio_ports passes and the old early-return skipped repair —
        // leaving Nexus to connect to 127.0.0.1:0, which fails with WSAEADDRNOTAVAIL (os error 10049).
        // The repair must reassign a 0 port even when nothing collides.
        let mut s = Settings::default();
        s.ensure_radio_profiles(); // radio 0 @ 4532
        let r1 = s.add_radio_profile(); // radio 1 @ 4533
        s.radios
            .iter_mut()
            .find(|p| p.id == r1)
            .unwrap()
            .rigctld_port = 0; // the broken Xiegu-profile case
                               // Distinct (0 != 4532), so validate alone does NOT catch it.
        assert!(validate_radio_ports(&s.radios, None).is_ok());
        s.ensure_distinct_radio_ports();
        assert_ne!(
            s.radios.iter().find(|p| p.id == r1).unwrap().rigctld_port,
            0,
            "the 0 port was reassigned to a real one"
        );
        assert!(
            s.radios.iter().all(|p| p.rigctld_port != 0),
            "no profile is left on port 0"
        );
        assert!(
            validate_radio_ports(&s.radios, None).is_ok(),
            "still pairwise-distinct after repair"
        );
    }

    /// The track loop's honesty label: which steering surfaces the loop is
    /// actually allowed to drive. The Doppler half must be exactly what
    /// `sat_doppler_tick` will touch the radio for — EITHER leg driven, plus a
    /// held transponder — if this table and that gate ever disagree, the status
    /// lies about whether the radio can be touched.
    #[test]
    fn sat_track_mode_reports_the_drivable_surfaces() {
        // Both halves live: the full appliance replacement.
        assert_eq!(sat_track_mode(true, true, true), "rotor+doppler");
        // No rotor, a leg driven: the Arrow-antenna operator's mode. One leg is
        // enough — a downlink-only station IS being tuned.
        assert_eq!(sat_track_mode(false, true, true), "doppler-only");
        // Rotor only — the operator switched Doppler off entirely.
        assert_eq!(sat_track_mode(true, false, true), "rotor-only");
        // Neither surface consented: pass state/geometry only — legal and
        // useful for timing, and the label says exactly that.
        assert_eq!(sat_track_mode(false, false, true), "pass-only");
        // The other consent: no held transponder means the tick tunes nothing
        // (`sat_tune` is None ⇒ every tick no-ops), so the label must not
        // claim the dial — an operator who picked "None — leave the dial to
        // me" was being told "SAT ⟳ · dial at AOS" by the app-wide chip.
        assert_eq!(sat_track_mode(false, true, false), "pass-only");
        assert_eq!(sat_track_mode(true, true, false), "rotor-only");
    }

    /// The consent split the 0.26 ruling turned on: the receive dial is
    /// automatic, the transmit VFO is confirmed per radio. Every row here is a
    /// station that must not be surprised by the change.
    #[test]
    fn the_downlink_is_automatic_and_the_uplink_is_confirmed_per_radio() {
        // A fresh install has chosen nothing: the dial is corrected, and
        // nothing can reach a transmit VFO.
        let mut s = Settings::default();
        s.ensure_radio_profiles();
        assert!(s.sat_doppler_downlink());
        assert!(!s.sat_doppler_uplink());

        // The operator's OFF switch stops both legs.
        s.sat_doppler_off = true;
        assert!(!s.sat_doppler_downlink());
        s.sat_doppler_off = false;

        // An UPLINK-ONLY mapping is the one operator statement that the single
        // VFO in play is the transmit leg — the dial is theirs.
        s.sat_vfo_map = SatVfoMap::UplinkOnly;
        assert!(!s.sat_doppler_downlink());

        // Confirming records the radio it was confirmed FOR.
        s.confirm_sat_uplink(s.active_radio, SatVfoMap::MainDownSubUp);
        assert!(s.sat_doppler_uplink());
        assert_eq!(s.sat_uplink_radios.as_deref(), Some(&[s.active_radio][..]));
        // Another radio is another station's wiring question.
        assert!(!s.sat_uplink_confirmed(s.active_radio + 1));

        // A second radio confirms the SAME mapping: both stand.
        s.confirm_sat_uplink(9, SatVfoMap::MainDownSubUp);
        assert!(s.sat_uplink_confirmed(9) && s.sat_uplink_confirmed(s.active_radio));
        // CHANGING the mapping retires the others. They confirmed Main/Sub;
        // there is one mapping field, so leaving them listed would drive radio
        // 9's uplink under a layout nobody confirmed for it.
        s.confirm_sat_uplink(s.active_radio, SatVfoMap::ADownBUp);
        assert!(s.sat_uplink_confirmed(s.active_radio));
        assert!(
            !s.sat_uplink_confirmed(9),
            "9 confirmed a different mapping"
        );

        // A raw `None` list is TRANSIENT pre-migration state, and it confirms
        // NOTHING: `Settings::load` materializes a live legacy grant into
        // concrete ids (see the round-trip pin below), so an unresolved
        // absence reaching a consent check can only mean something skipped
        // the migration — and an unresolved consent must fail safe on the
        // leg that transmits. (Round 2 read `None` as a station-wide grant;
        // that sentinel had no serialized form and died on the first save.)
        let mut old = Settings::default();
        old.ensure_radio_profiles();
        old.sat_vfo_map = SatVfoMap::ADownBUp;
        old.sat_uplink_radios = None;
        assert!(
            !old.sat_doppler_uplink(),
            "unresolved consent drives nothing"
        );
        assert!(!old.sat_uplink_confirmed(7));
    }

    /// THE UPGRADE GATE (round 2, defect 1). A pre-0.26 settings.json could
    /// carry a VFO mapping that drove NOTHING, because the retired `satDoppler`
    /// opt-in was the first gate in front of it. Loading that file must not
    /// promote the inert mapping into live transmit-side consent: only a pair
    /// that was actually LIVE pre-upgrade (`satDoppler: true` AND an
    /// uplink-driving mapping) is a station-wide grant worth keeping. An inert
    /// mapping upgrades to downlink-only plus the confirmation offer.
    #[test]
    fn load_promotes_only_a_live_legacy_pair_to_uplink_consent() {
        let dir = std::env::temp_dir().join(format!(
            "tempo_settings_satdop_migration_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let load_raw = |json: &str| {
            std::fs::write(&path, json).unwrap();
            Settings::load(&path)
        };

        // (a) The INERT pair: a mapping picked while the old master switch was
        // off. Under 0.25.0 this station drove nothing — the mapping was one
        // click away on the pass rail's own select, next to a separate "turn
        // on" button, so it is a realistic state for the reporting operator.
        let s =
            load_raw(r#"{"mycall":"KD9TAW","satDoppler":false,"satVfoMap":"main-down-sub-up"}"#);
        assert!(
            s.sat_doppler_downlink(),
            "the downlink upgrade is the point"
        );
        assert_eq!(
            s.sat_vfo_map,
            SatVfoMap::MainDownSubUp,
            "the mapping itself is kept — never silently overwritten"
        );
        assert_eq!(
            s.sat_uplink_radios.as_deref(),
            Some(&[][..]),
            "an inert mapping upgrades to ASK (empty consent), never to a grant"
        );
        assert!(
            !s.sat_doppler_uplink(),
            "no transmit VFO may be driven off a setting that never drove one"
        );
        assert!(!s.sat_uplink_confirmed(0) && !s.sat_uplink_confirmed(41));

        // (b) No `satDoppler` key at all (even older file, or hand-edited):
        // there is no evidence the pair was ever live — same as inert.
        let s = load_raw(r#"{"mycall":"KD9TAW","satVfoMap":"main-down-sub-up"}"#);
        assert_eq!(s.sat_uplink_radios.as_deref(), Some(&[][..]));
        assert!(!s.sat_doppler_uplink());

        // (c) The LIVE pair: the operator turned the old switch on AND mapped
        // the uplink — that combination really did drive the transmit VFO
        // under 0.25.0, so the station-wide grant is kept: MATERIALIZED as
        // consent for every radio in the file (here the single migrated
        // profile, id 0), and the upgrading operator is not re-asked or
        // downgraded — including across save/load, which the round-trip pin
        // below proves.
        let s = load_raw(r#"{"mycall":"KD9TAW","satDoppler":true,"satVfoMap":"main-down-sub-up"}"#);
        assert_eq!(
            s.sat_uplink_radios.as_deref(),
            Some(&[0u32][..]),
            "a live legacy grant is materialized for the radios that exist"
        );
        assert!(s.sat_doppler_uplink());
        assert!(
            !s.sat_uplink_confirmed(7),
            "a radio this station does not have gets its own confirmation"
        );

        // (d) The old switch on but NO uplink-driving mapping: nothing was
        // driven, so there is nothing to grant. (Off = no mapping chosen.)
        let s = load_raw(r#"{"mycall":"KD9TAW","satDoppler":true,"satVfoMap":"off"}"#);
        assert_eq!(s.sat_uplink_radios.as_deref(), Some(&[][..]));

        // (e) A post-0.26 file has its own consent list — the legacy key means
        // nothing beside it and the list rides through untouched. (The roster
        // must contain the id: consent for a radio that does not exist is
        // pruned, which is defect 2's rule, not this one's.)
        let s = load_raw(
            r#"{"mycall":"KD9TAW","satDoppler":true,"satVfoMap":"main-down-sub-up",
                "satUplinkRadios":[1],
                "radios":[{"id":0,"name":"FTdx10"},{"id":1,"name":"IC-9700"}]}"#,
        );
        assert_eq!(s.sat_uplink_radios.as_deref(), Some(&[1][..]));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ROUND 3, DEFECT 1 — the round-trip pin. The honoured pre-0.26
    /// station-wide grant must survive ARBITRARILY MANY save/load cycles, and
    /// still prune with removed radios. Round 2 carried it as the in-memory
    /// sentinel `None`, which `save()` wrote as `"satUplinkRadios": null`
    /// while (correctly) dropping the retired `satDoppler` key — so the NEXT
    /// load found no legacy evidence and normalized to `Some([])`: the grant
    /// silently died after one save+relaunch and the upgraded operator lost
    /// uplink Doppler with no prompt. The fix MATERIALIZES the grant at
    /// migration time as concrete consent for the radios in the file — the
    /// station-wide meaning, made durable and prunable.
    #[test]
    fn the_legacy_grant_survives_every_save_reload_cycle_and_prunes_with_removed_radios() {
        let dir = std::env::temp_dir().join(format!(
            "tempo_settings_satdop_roundtrip_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");

        // The LIVE legacy pair, on a two-radio station: satDoppler on beside
        // an uplink-driving mapping really drove the transmit VFO pre-0.26,
        // on whichever rig was under the split.
        std::fs::write(
            &path,
            r#"{"mycall":"KD9TAW","satDoppler":true,"satVfoMap":"main-down-sub-up",
                "activeRadio":0,
                "radios":[{"id":0,"name":"FTdx10"},{"id":1,"name":"IC-9700"}]}"#,
        )
        .unwrap();
        let mut s = Settings::load(&path);
        assert!(
            s.sat_doppler_uplink(),
            "the grant is honoured on first load"
        );
        assert!(s.sat_uplink_confirmed(0) && s.sat_uplink_confirmed(1));

        // THE DEFECT: any ordinary save (every settings apply, a rail click)
        // plus a relaunch must keep the grant — for as many cycles as the
        // operator has sessions.
        for cycle in 1..=3 {
            s.save(&path).unwrap();
            s = Settings::load(&path);
            assert!(
                s.sat_doppler_uplink(),
                "the honoured grant survives save+relaunch (cycle {cycle})"
            );
            assert!(
                s.sat_uplink_confirmed(0) && s.sat_uplink_confirmed(1),
                "…for the radios it was granted to (cycle {cycle})"
            );
        }

        // MATERIALIZED, not remembered: the grant is concrete per-radio
        // consent for the radios that existed at upgrade — a shape save()
        // can round-trip, unlike the `None` sentinel.
        assert_eq!(
            s.sat_uplink_radios.as_deref(),
            Some(&[0u32, 1][..]),
            "the station-wide grant is materialized for the radios in the file"
        );

        // …and it is PRUNABLE, exactly like consent written under the new
        // rules: a removed radio takes its share of the grant with it, and a
        // replacement that REUSES the freed id starts unconfirmed — across a
        // relaunch too.
        assert!(s.remove_radio_profile(1));
        assert!(!s.sat_uplink_confirmed(1), "consent died with the radio");
        assert!(
            s.sat_uplink_confirmed(0),
            "the surviving radio keeps its share"
        );
        s.save(&path).unwrap();
        s = Settings::load(&path);
        assert!(s.sat_uplink_confirmed(0) && !s.sat_uplink_confirmed(1));
        let n = s.add_radio_profile();
        assert_eq!(n, 1, "max(id)+1 over survivors reuses the freed id");
        assert!(
            !s.sat_uplink_confirmed(n),
            "the reused id starts unconfirmed — the grant never leaks to a new rig"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Radio ids ARE reused (`add_radio_profile` = max(id)+1 over SURVIVORS),
    /// so per-radio consent must die with the radio it names — otherwise the
    /// next rig added inherits a Main/Sub uplink confirmation it never got
    /// (round 2, defect 2).
    #[test]
    fn a_removed_radios_uplink_consent_dies_with_it() {
        let mut s = Settings::default();
        s.ensure_radio_profiles(); // radio 0
        let b = s.add_radio_profile(); // highest id
        s.confirm_sat_uplink(b, SatVfoMap::MainDownSubUp);
        assert!(s.sat_uplink_confirmed(b), "precondition");

        assert!(s.remove_radio_profile(b));
        assert_eq!(
            s.sat_uplink_radios.as_deref(),
            Some(&[][..]),
            "consent is pruned with the radio, like the routing rules beside it"
        );

        // The replacement REUSES the id — that is the trap this test pins.
        let n = s.add_radio_profile();
        assert_eq!(n, b, "max(id)+1 over survivors reuses a freed id");
        assert!(
            !s.sat_uplink_confirmed(n),
            "a brand-new rig starts unconfirmed, whatever id it was dealt"
        );
    }

    /// Derivation is only ever a PROPOSAL, and only where the radio can
    /// express the layout one way and this build can drive it.
    #[test]
    fn the_uplink_offer_derives_only_what_it_can_prove() {
        let mut s = Settings::default();
        s.ensure_radio_profiles();
        // Nothing known about the rig: nothing to propose.
        assert_eq!(s.sat_uplink_offer(), SatUplinkOffer::Nothing);

        // The IC-9700 in satellite mode transmits on Sub, full stop — but only
        // our own CI-V path drives that, so Hamlib-served asks instead.
        s.rig_model = 3081;
        s.icom_native_cat = false;
        s.sync_active_from_flat();
        assert_eq!(s.sat_uplink_offer(), SatUplinkOffer::Ask);
        s.icom_native_cat = true;
        s.sync_active_from_flat();
        assert_eq!(
            s.sat_uplink_offer(),
            SatUplinkOffer::Confirm(SatVfoMap::MainDownSubUp)
        );

        // …and the IC-910 / IC-9100 are Main/Sub rigs the native daemon can
        // NEVER serve (no `0x27` scope ⇒ absent from `icom_scope_model`), so
        // Main/Sub has no path on them at all. `icom_native_cat` can still be
        // ON in a settings file, and pre-filling Main/Sub off that flag would
        // be a one-click confirmation for a mapping the engine then refuses.
        // They stay at Ask, flag or no flag.
        for model in [3044u32, 3068] {
            s.rig_model = model;
            for on in [false, true] {
                s.icom_native_cat = on;
                s.sync_active_from_flat();
                assert_eq!(
                    s.sat_uplink_offer(),
                    SatUplinkOffer::Ask,
                    "model {model}, icom_native_cat {on}"
                );
                assert!(!s.sat_native_civ_reachable(), "model {model}");
            }
        }
        // Same for a NETWORK-connected IC-9700: `native_civ_addr` refuses a TCP
        // transport, so the second axis has to be checked too. That axis is
        // the DAEMON's — [`rig_conn_is_network`], which needs BOTH the pick and
        // an address, exactly as `Transport::is_network` does. The table below
        // is the whole of it, and every row is the daemon's own answer.
        s.rig_model = 3081;
        s.icom_native_cat = true;
        for (conn, addr, reachable) in [
            ("network", "192.168.1.50:4992", false),
            // A "network" pick with nowhere to connect is not a network rig.
            ("network", "", true),
            // The legacy file: `rig_conn` is `#[serde(default)]` and its own
            // doc says empty is serial. This is the field-report station, and
            // a `rig_conn == "serial"` test put it in the dead-end bucket.
            ("", "", true),
            ("serial", "", true),
        ] {
            s.rig_conn = conn.into();
            s.rig_addr = addr.into();
            s.sync_active_from_flat();
            assert_eq!(
                s.sat_native_civ_reachable(),
                reachable,
                "rig_conn {conn:?} rig_addr {addr:?}"
            );
            assert_eq!(
                s.sat_uplink_offer(),
                if reachable {
                    SatUplinkOffer::Confirm(SatVfoMap::MainDownSubUp)
                } else {
                    SatUplinkOffer::Ask
                },
                "rig_conn {conn:?} rig_addr {addr:?}"
            );
        }
        s.rig_conn = "serial".into();
        s.rig_addr = String::new();
        s.icom_native_cat = false;
        s.sync_active_from_flat();

        // Full duplex on VFO A/B: which one is the uplink is station wiring.
        for model in [1001u32, 1010, 2014, 2007] {
            s.rig_model = model;
            s.sync_active_from_flat();
            assert_eq!(s.sat_uplink_offer(), SatUplinkOffer::Ask, "model {model}");
        }
        // Half-duplex HF rigs have no uplink leg to offer at all.
        for model in [3073u32, 1042, 2031, 0] {
            s.rig_model = model;
            s.sync_active_from_flat();
            assert_eq!(
                s.sat_uplink_offer(),
                SatUplinkOffer::Nothing,
                "model {model}"
            );
        }
    }

    #[test]
    fn remove_radio_profile_guards_active_and_last() {
        let mut s = Settings::default();
        s.ensure_radio_profiles();
        let two = s.add_radio_profile();
        // Can't remove the active radio.
        assert!(
            !s.remove_radio_profile(s.active_radio),
            "refuses the active radio"
        );
        assert_eq!(s.radios.len(), 2);
        // Can remove a non-active radio.
        assert!(s.remove_radio_profile(two));
        assert_eq!(s.radios.len(), 1);
        // Can't remove the last remaining one.
        assert!(
            !s.remove_radio_profile(s.active_radio),
            "refuses the last radio"
        );
        assert_eq!(s.radios.len(), 1);
    }

    /// The whole point of Unassisted mode: ONE action must turn off EVERY assistance
    /// source. This walks the source list rather than naming them, so adding an
    /// assistance source to `assistance_sources` without wiring the override fails here.
    #[test]
    fn unassisted_mode_is_one_switch_that_silences_every_source() {
        let mut s = Settings::default();
        // A maximally-assisted station: everything the operator can turn on, on.
        s.ai_cw_enabled = true;
        s.cluster_enabled = true;
        s.pskreporter = true;
        assert!(
            s.assistance_sources().iter().all(|(_, on)| *on),
            "the fixture must start fully assisted: {:?}",
            s.assistance_sources()
        );

        s.unassisted_mode = true; // the one action

        for (label, on) in s.assistance_sources() {
            assert!(
                !on,
                "{label} still active in Unassisted mode — the switch must silence every source"
            );
        }
    }

    /// It OVERRIDES, never OVERWRITES. The operator's own settings must survive untouched,
    /// so switching back restores their setup exactly — no shadow copy to drift.
    #[test]
    fn unassisted_mode_leaves_the_operators_own_settings_alone() {
        let mut s = Settings::default();
        s.ai_cw_enabled = true;
        s.cluster_enabled = true;
        s.pskreporter = true;

        s.unassisted_mode = true;
        assert!(
            s.ai_cw_enabled,
            "the operator's own preference must not be rewritten"
        );
        assert!(s.cluster_enabled);
        assert!(s.pskreporter);

        s.unassisted_mode = false;
        assert!(
            s.assistance_sources().iter().all(|(_, on)| *on),
            "switching back must restore the operator's setup exactly"
        );
    }

    /// An operator who had a source off keeps it off after leaving Unassisted mode — the
    /// switch must not turn anything ON on the way out.
    #[test]
    fn leaving_unassisted_mode_never_enables_something_the_operator_had_off() {
        let mut s = Settings::default();
        s.ai_cw_enabled = false;
        s.cluster_enabled = false;
        s.unassisted_mode = true;
        s.unassisted_mode = false;
        assert!(!s.ai_cw_active(), "AI CW was off before; it must stay off");
        assert!(
            !s.cluster_active(),
            "cluster was off before; it must stay off"
        );
    }

    /// Outbound PSK Reporter uploads are explicitly NOT assistance — ARRL's glossary says
    /// "Generating spotting information for use by other stations is not considered to be
    /// spotting assistance." Only the inbound evidence we consume stops.
    #[test]
    fn unassisted_mode_stops_inbound_pskr_evidence_not_outbound_uploads() {
        let mut s = Settings::default();
        s.pskreporter = true;
        s.unassisted_mode = true;
        assert!(
            !s.pskr_evidence_active(),
            "inbound reception reports are assistance"
        );
        assert!(
            s.pskreporter,
            "the outbound upload is explicitly not assistance and must keep running"
        );
    }

    /// Default false, and nothing but the operator's toggle may set it — same doctrine as
    /// `fd_active`. A default-on Unassisted mode would silently disable the decoder an
    /// operator paid attention to.
    #[test]
    fn unassisted_mode_is_off_on_a_fresh_install() {
        let s = Settings::default();
        assert!(!s.unassisted_mode);
        assert!(
            s.ai_cw_active(),
            "a fresh install keeps its shipped AI CW decoder"
        );
    }
}

#[cfg(test)]
mod cq_pause_wire_tests {
    use super::*;

    /// The UI reads `cqPauseSecs`; the Rust field is `cq_pause_secs`. That only lines up because
    /// of the container's rename_all, and a mismatch here is invisible in both languages — the
    /// setting silently reverts to its default on every save, which is exactly the shape of bug
    /// the settings-plumbing notes warn about. So the WIRE NAME is asserted, not assumed.
    #[test]
    fn the_auto_cq_settings_use_the_names_the_ui_sends() {
        let json = serde_json::to_value(Settings::default()).expect("serialises");
        assert!(
            json.get("cqPauseSecs").is_some(),
            "cqPauseSecs must be on the wire"
        );
        assert!(
            json.get("cqMaxCalls").is_some(),
            "cqMaxCalls must be on the wire"
        );
        assert!(
            json.get("cq_pause_secs").is_none(),
            "snake_case on the wire would mean the UI never sees it"
        );
    }

    /// The operator's ruling: eight CQs, then three minutes. Defaults are the whole feature for
    /// anyone who never opens Settings, which is most people.
    #[test]
    fn the_defaults_are_eight_calls_and_three_minutes() {
        let d = Settings::default();
        assert_eq!(d.cq_max_calls, Some(8));
        assert_eq!(d.cq_pause_secs, Some(180));
        // And a settings.json written before these existed must load with the same values —
        // otherwise an upgrading operator gets different behaviour from a fresh install.
        let old: Settings = serde_json::from_str("{}").expect("an empty settings file loads");
        assert_eq!(
            old.cq_max_calls,
            Some(8),
            "serde default must match the struct default"
        );
        assert_eq!(old.cq_pause_secs, Some(180));
    }
}

#[cfg(test)]
mod am_power_tests {
    use super::*;

    /// AM'S CAP MAY ONLY EVER LOWER POWER. That property is the whole reason this can ship
    /// without a rig on the bench, exactly as `rf_power_ceiling_high_duty` did for SSTV.
    ///
    /// Why it needs a cap at all: a rig making 100 W PEP on SSB makes about 25 W of carrier on
    /// AM. The power sits in a carrier that is always present plus two sidebands, and PEP is
    /// reached on modulation peaks — so the SSB drive clips the peaks.
    #[test]
    fn am_lowers_the_phone_ceiling_and_never_lifts_it() {
        let mut s = Settings {
            operating_mode: OperatingMode::Phone,
            // Default: a quarter, and below an uncapped phone.
            max_power_phone: None,
            ..Default::default()
        };
        assert_eq!(s.rf_power_ceiling(), 1.0, "phone uncapped");
        assert_eq!(s.rf_power_ceiling_am(), 0.25, "AM still capped");

        // An operator who set AM ABOVE phone must not have AM lift them past the phone cap.
        s.max_power_phone = Some(0.30);
        s.max_power_am = Some(0.90);
        assert_eq!(
            s.rf_power_ceiling_am(),
            0.30,
            "the LOWER of the two, always"
        );

        // And the ordinary case: AM below phone.
        s.max_power_am = Some(0.20);
        assert_eq!(s.rf_power_ceiling_am(), 0.20);
    }

    /// An operator who deliberately clears the AM cap gets the phone cap — not 1.0, and not a
    /// silent re-imposition of the default.
    #[test]
    fn clearing_the_am_cap_falls_back_to_phone_not_to_full_power() {
        let s = Settings {
            operating_mode: OperatingMode::Phone,
            max_power_am: None,
            max_power_phone: Some(0.5),
            ..Default::default()
        };
        assert_eq!(s.rf_power_ceiling_am(), 0.5);
    }

    /// A settings.json written before AM existed must load with the cap ON. An upgrading
    /// operator is exactly the person who has never thought about AM drive.
    #[test]
    fn an_old_settings_file_gains_the_am_cap() {
        let old: Settings = serde_json::from_str("{}").expect("empty settings loads");
        assert_eq!(old.max_power_am, Some(0.25));
    }
}
