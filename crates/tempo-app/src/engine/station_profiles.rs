//! "My station" profiles at the Engine: the verbs that switch, save and remove them, the step of
//! the log funnel that stamps a profile's own fields on a contact, and the pick an activation makes.
//! The model, and why the flat call/grid/state/power ARE the active profile, is in
//! `settings/station_profiles.rs`.
//!
//! # The log stamp, and what it never touches
//!
//! [`Engine::stamp_station_profile`] runs between the activation's stamp and the station's
//! (`stamp_activation`, `stamp_station_now`), on the same terms as both: a field the record already
//! carries is kept (one the operator typed, one a park placed), and a contact more than
//! `STATION_FIELDS_MAX_AGE_SECS` old takes nothing. With no profile it stamps nothing at all, so a
//! station that never makes one logs exactly what it logged before. A contact already in the log
//! is never touched: only the funnel calls it.
//!
//! The location fields (`MY_CNTY`, `MY_CQ_ZONE`, `MY_ITU_ZONE`, `MY_IOTA`) describe where the
//! profile is. A profile with no park puts them on a contact with no park: a contact an activation
//! tagged with a park was made somewhere else. A profile that names a park describes the park, and
//! puts them only on that park's contacts, which only a running activation of it tags: the profile
//! never stamps a park itself. A contact held for the operator's confirm takes the profile when it
//! is held, not again at the confirm. Like the grid and state they are
//! marked as Nexus's own, so the LoTW upload leaves them out and TQSL signs from its Station
//! Location as before (`tempo_core::logbook::SELF_FILLED_FIELD`).

use super::{now_unix_secs, Engine, Mode, STATION_FIELDS_MAX_AGE_SECS};
use crate::dto::StationProfileChoice;
use crate::settings::{
    Settings, StationProfile, HOME_STATION_PROFILE, STATION_PROFILE_SWITCH_REFUSED,
};
use tempo_core::logbook::{fill_extra, mark_self_filled, QsoRecord};

impl Engine {
    /// The profiles the top bar offers, and the one the station is on. Both empty with none.
    pub(super) fn station_profile_choices(&self) -> (Vec<StationProfileChoice>, String) {
        let choices = self
            .settings
            .station_profiles
            .iter()
            .map(|p| StationProfileChoice {
                id: p.id.clone(),
                name: p.name.clone(),
                // The active one's call is the live one, whatever its stored copy says.
                call: if p.id == self.settings.active_station_profile {
                    self.settings.mycall.trim().to_ascii_uppercase()
                } else {
                    p.call.clone()
                },
            })
            .collect();
        (choices, self.settings.active_station_profile.clone())
    }

    /// Put the station on profile `id`: its call, grid, state and power become the station's, and
    /// the profile it leaves keeps what the station had. Exactly a Settings save of those four
    /// fields: the identity is rebound, an over planned under the old call does not key, and the
    /// licence gate judges the next one. Contacts already logged are not touched; new ones carry
    /// the new call. `Err` for a profile that does not exist, and while a run, a contact or an
    /// over is under way ([`Self::station_profile_switch_refusal`]): every switch, the top bar's,
    /// an activation's and Stop's, comes through here.
    pub fn switch_station_profile(&mut self, id: &str) -> Result<(), String> {
        if !self.settings.station_profiles.iter().any(|p| p.id == id) {
            return Err("No such station profile.".into());
        }
        if self.settings.active_station_profile == id {
            return Ok(());
        }
        if let Some(why) = self.station_profile_switch_refusal() {
            return Err(why.into());
        }
        let mut next = self.settings.clone();
        next.sync_active_station_from_flat();
        next.active_station_profile = id.to_string();
        next.sync_flat_from_active_station();
        self.adopt_station_profiles(next)
    }

    /// ⛔ Why the station profile cannot change now, or `None` when it can. The FT sequencer sends
    /// the call and grid it was started with (`QsoStation::mycall`), and the log takes the call
    /// the station has when the contact is logged, so a switch while either is in use puts one
    /// call on the air and logs another, or keys a call the licence refusal never judged. So the
    /// profile stays while:
    ///
    /// - anything is on the air, the radio loop is still playing an over, or the slot sequencer
    ///   is armed to key ([`Self::transmit_needs_the_lock`]): every planned over is an armed one,
    ///   since `plan_tx` plans nothing with TX off;
    /// - a CQ run is up, or the FT sequencer holds a message to send (a CQ, a contact part-way, a
    ///   73 still owed), armed or not: Enable TX would send it under the call it was built with.
    ///
    /// It reads the transmit state and changes none of it. S&P (monitoring) or Clear DX with TX
    /// off leaves nothing to send, and the profile can change.
    pub fn station_profile_switch_refusal(&self) -> Option<&'static str> {
        let sequencer_holds_a_message = match &self.mode {
            Mode::Qso { station, .. } => station.pending.is_some(),
            Mode::FieldDay { station, .. } => station.pending.is_some(),
            Mode::Chat => false,
        };
        (self.transmit_needs_the_lock() || self.cq_running || sequencer_holds_a_message)
            .then_some(STATION_PROFILE_SWITCH_REFUSED)
    }

    /// Save one profile (an empty id adds a new one), and return its id. The first saved writes
    /// Home beside it from today's Settings ▸ Station. Saving the profile the station is on makes
    /// its call, grid, state and power the station's, as a switch does. `Err` says what is wrong
    /// with it, and nothing is saved.
    pub fn save_station_profile(&mut self, profile: StationProfile) -> Result<String, String> {
        let mut next = self.settings.clone();
        next.ensure_station_profiles();
        next.sync_active_station_from_flat();
        let mut profile = profile;
        if profile.id.is_empty() {
            profile.id = next.next_station_profile_id();
        } else if !next.station_profiles.iter().any(|p| p.id == profile.id) {
            return Err("No such station profile.".into());
        }
        let profile = profile.normalized()?;
        let id = profile.id.clone();
        if let Some(slot) = next.station_profiles.iter_mut().find(|p| p.id == id) {
            *slot = profile;
        } else {
            next.station_profiles.push(profile);
        }
        if id == next.active_station_profile {
            next.sync_flat_from_active_station();
        }
        self.adopt_station_profiles(next)?;
        Ok(id)
    }

    /// Remove profile `id`. Home and the profile the station is on stay: switch away first.
    pub fn remove_station_profile(&mut self, id: &str) -> Result<(), String> {
        if id == HOME_STATION_PROFILE {
            return Err("Home stays: it is your station.".into());
        }
        if id == self.settings.active_station_profile {
            return Err("Switch to another station profile before removing this one.".into());
        }
        let mut next = self.settings.clone();
        let before = next.station_profiles.len();
        next.station_profiles.retain(|p| p.id != id);
        if next.station_profiles.len() == before {
            return Err("No such station profile.".into());
        }
        self.adopt_station_profiles(next)
    }

    /// Take `next`'s profiles and active id, and its flat station fields when they changed. A change
    /// of call, grid, state or power is applied as a Settings save is (identity rebound, the TX gate
    /// generation moved); a change of the list alone moves the TX gate generation only when it
    /// changes whether the licence refusal stands, so an edit of a profile the station is not on
    /// never cancels an over in flight. A change of call, grid, state or power is refused, with
    /// nothing taken, while [`Self::station_profile_switch_refusal`] says so: saving the profile
    /// the station is on is a switch of those four.
    fn adopt_station_profiles(&mut self, next: Settings) -> Result<(), String> {
        let flat = |s: &Settings| {
            (
                s.mycall.clone(),
                s.mygrid.clone(),
                s.op_state.clone(),
                s.station_power_w,
            )
        };
        let refused_before = self.settings.station_profile_tx_refusal();
        let flat_changed = flat(&next) != flat(&self.settings);
        if flat_changed {
            if let Some(why) = self.station_profile_switch_refusal() {
                return Err(why.into());
            }
        }
        // The live list and id first: a save keeps the live ones (`keep_live_station_profiles`).
        self.settings.station_profiles = next.station_profiles.clone();
        self.settings.active_station_profile = next.active_station_profile.clone();
        if flat_changed {
            self.apply_settings(next);
        } else if self.settings.station_profile_tx_refusal() != refused_before {
            self.tx_gate_gen = self.tx_gate_gen.wrapping_add(1);
        }
        Ok(())
    }

    /// A whole-settings save keeps the live profiles and active id (`live`, `live_active`, taken
    /// before the save landed); the save's flat station fields go to the profile its form was on
    /// ([`Settings::keep_station_form`]), and if that put the flat fields back to another
    /// profile's, the identity follows them.
    pub(super) fn keep_live_station_profiles(
        &mut self,
        live: Vec<StationProfile>,
        live_active: String,
    ) {
        let form = std::mem::replace(&mut self.settings.active_station_profile, live_active);
        self.settings.station_profiles = live;
        let bound = (self.settings.mycall.clone(), self.settings.mygrid.clone());
        self.settings.keep_station_form(&form);
        if (self.settings.mycall.clone(), self.settings.mygrid.clone()) != bound {
            let (call, grid) = (self.settings.mycall.clone(), self.settings.mygrid.clone());
            self.app.set_identity(&call, &grid);
        }
    }

    /// An activation just started: when a profile names one of its parks, the station goes onto
    /// it, and Stop comes back ([`Self::return_from_activation_station_profile`]). Never onto a
    /// profile with another licensee's call: a slip of the park box must not change whose call the
    /// station transmits under. A portable form of the same call (`/P`) is the same licence and is
    /// picked. Not while a run, a contact or an over is under way (the switch refuses it): the
    /// activation starts and the station stays on its profile. From a computer driving the
    /// station (`from_afar`), never onto a profile with a different call: the call the station
    /// transmits under is switched at the station only, as the top bar's picker is.
    pub(super) fn pick_station_profile_for_activation(&mut self, from_afar: bool) {
        let Some(activation) = self.station.live_activation() else {
            return;
        };
        if activation.program != "POTA" {
            return;
        }
        let refs = activation.refs.clone();
        let base = tempo_core::message::base_call(&self.settings.mycall);
        let Some((target, call)) = self
            .settings
            .station_profiles
            .iter()
            .find(|p| !p.park.is_empty() && refs.contains(&p.park))
            .filter(|p| tempo_core::message::base_call(&p.call) == base)
            .map(|p| (p.id.clone(), p.call.clone()))
        else {
            return;
        };
        let now = self.settings.active_station_profile.clone();
        if target == now || (from_afar && !self.same_call_as_now(&call)) {
            return;
        }
        // Where Stop goes back to: an activation that replaced one which had picked keeps that
        // one's starting point.
        let before = self
            .station
            .activation_station_profile_return()
            .map_or(now, |(before, _)| before);
        if self.switch_station_profile(&target).is_ok() {
            self.station
                .set_activation_station_profile_return(Some((before, target)));
        }
    }

    /// The activation ended (`back`, taken from it before it went): back to the profile the
    /// station was on before it picked the park's, unless the operator has switched since. Not
    /// while a run, a contact or an over is under way (the switch refuses it), and from a computer
    /// driving the station not onto a different call: the station then stays where it is.
    pub(super) fn return_from_activation_station_profile(
        &mut self,
        back: Option<(String, String)>,
        from_afar: bool,
    ) {
        let Some((before, picked)) = back else {
            return;
        };
        if self.settings.active_station_profile != picked {
            return;
        }
        if from_afar {
            let call = self
                .settings
                .station_profiles
                .iter()
                .find(|p| p.id == before)
                .map(|p| p.call.clone())
                .unwrap_or_default();
            if !self.same_call_as_now(&call) {
                return;
            }
        }
        let _ = self.switch_station_profile(&before);
    }

    /// Whether `call` is the call the station transmits under now.
    fn same_call_as_now(&self, call: &str) -> bool {
        call.trim()
            .eq_ignore_ascii_case(self.settings.mycall.trim())
    }

    /// The step of the log funnel that stamps the active profile's own fields (module header).
    pub(super) fn stamp_station_profile(&self, rec: &mut QsoRecord) {
        let Some(profile) = self.settings.active_station_profile() else {
            return;
        };
        if now_unix_secs().saturating_sub(rec.when_unix) > STATION_FIELDS_MAX_AGE_SECS {
            return;
        }
        // THE PROFILE'S PARK is never stamped from the profile: a park goes on a contact only from
        // an activation of it that is running (`stamp_activation`, which ran first). A profile
        // left on after its activation, or taken up again at a launch whose activation waits for
        // "Still activating", tags nothing.
        let park = (!profile.park.is_empty()).then_some(profile.park.as_str());
        let at_profiles_park = park.is_some() && rec.ota.my_ref.as_deref() == park;
        // A park contact takes no home grid or state in `stamp_station_now`; on the profile's own
        // park the profile's are the right ones, where the activation placed none.
        if at_profiles_park {
            let activation_grid = self
                .station
                .live_activation()
                .is_some_and(|a| Some(a.my_ref()) == rec.ota.my_ref && a.grid.is_some());
            if !activation_grid
                && rec.my_grid.is_none()
                && !rec.extra.iter().any(|(k, _)| k == "MY_GRIDSQUARE")
            {
                if let Some(grid) = self.log_grid_now() {
                    rec.my_grid = Some(grid);
                    mark_self_filled(rec, "MY_GRIDSQUARE");
                }
            }
            let st = self.settings.op_state.trim().to_ascii_uppercase();
            if !st.is_empty() && fill_extra(rec, "MY_STATE", st) {
                mark_self_filled(rec, "MY_STATE");
            }
        }
        // WHERE THE PROFILE IS: only on a contact made there. A profile that names a park
        // describes the park, so its place goes only on that park's contacts; one that names none,
        // on contacts with no park.
        let made_there = if park.is_some() {
            at_profiles_park
        } else {
            rec.ota.my_ref.is_none()
        };
        if made_there {
            let place = [
                ("MY_CNTY", profile.my_cnty()),
                ("MY_CQ_ZONE", profile.cq_zone.map(|z| z.to_string())),
                ("MY_ITU_ZONE", profile.itu_zone.map(|z| z.to_string())),
                (
                    "MY_IOTA",
                    (!profile.iota.is_empty()).then(|| profile.iota.clone()),
                ),
            ];
            for (tag, value) in place {
                if let Some(value) = value {
                    if fill_extra(rec, tag, value) {
                        mark_self_filled(rec, tag);
                    }
                }
            }
        }
        // WHAT IT WAS MADE WITH: the rig as the operator described it here (over the radio's
        // model, which `stamp_station_now` fills when this leaves it empty), and the antenna.
        if rec.my_rig.is_none() && !profile.rig.is_empty() {
            rec.my_rig = Some(profile.rig.clone());
        }
        if !profile.antenna.is_empty() {
            fill_extra(rec, "MY_ANTENNA", profile.antenna.clone());
        }
    }
}
