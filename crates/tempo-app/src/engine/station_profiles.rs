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
//! profile is. They go only on a contact with no park, or with the profile's own park: a contact an
//! activation tagged with another park was made somewhere else. Like the grid and state they are
//! marked as Nexus's own, so the LoTW upload leaves them out and TQSL signs from its Station
//! Location as before (`tempo_core::logbook::SELF_FILLED_FIELD`).

use super::{now_unix_secs, Engine, STATION_FIELDS_MAX_AGE_SECS};
use crate::dto::StationProfileChoice;
use crate::settings::{Settings, StationProfile, HOME_STATION_PROFILE};
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
    /// the new call. `Err` for a profile that does not exist.
    pub fn switch_station_profile(&mut self, id: &str) -> Result<(), String> {
        if !self.settings.station_profiles.iter().any(|p| p.id == id) {
            return Err("No such station profile.".into());
        }
        if self.settings.active_station_profile == id {
            return Ok(());
        }
        let mut next = self.settings.clone();
        next.sync_active_station_from_flat();
        next.active_station_profile = id.to_string();
        next.sync_flat_from_active_station();
        self.adopt_station_profiles(next);
        Ok(())
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
        self.adopt_station_profiles(next);
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
        self.adopt_station_profiles(next);
        Ok(())
    }

    /// Take `next`'s profiles and active id, and its flat station fields when they changed. A change
    /// of call, grid, state or power is applied as a Settings save is (identity rebound, the TX gate
    /// generation moved); a change of the list alone moves the TX gate generation only when it
    /// changes whether the licence refusal stands, so an edit of a profile the station is not on
    /// never cancels an over in flight.
    fn adopt_station_profiles(&mut self, next: Settings) {
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
        // The live list and id first: a save keeps the live ones (`keep_live_station_profiles`).
        self.settings.station_profiles = next.station_profiles.clone();
        self.settings.active_station_profile = next.active_station_profile.clone();
        if flat_changed {
            self.apply_settings(next);
        } else if self.settings.station_profile_tx_refusal() != refused_before {
            self.tx_gate_gen = self.tx_gate_gen.wrapping_add(1);
        }
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
    /// profile with another licensee's call: an activation started from another computer, or by
    /// a slip of the park box, must not change whose call the station transmits under. A portable
    /// form of the same call (`/P`) is the same licence and is picked.
    pub(super) fn pick_station_profile_for_activation(&mut self) {
        let Some(activation) = self.station.live_activation() else {
            return;
        };
        if activation.program != "POTA" {
            return;
        }
        let refs = activation.refs.clone();
        let base = tempo_core::message::base_call(&self.settings.mycall);
        let Some(target) = self
            .settings
            .station_profiles
            .iter()
            .find(|p| !p.park.is_empty() && refs.contains(&p.park))
            .filter(|p| tempo_core::message::base_call(&p.call) == base)
            .map(|p| p.id.clone())
        else {
            return;
        };
        let before = self.settings.active_station_profile.clone();
        if target == before {
            return;
        }
        if self.switch_station_profile(&target).is_ok() {
            self.station_profile_before_activation = Some((before, target));
        }
    }

    /// The activation ended: back to the profile the station was on before it picked the park's,
    /// unless the operator has switched since.
    pub(super) fn return_from_activation_station_profile(&mut self) {
        let Some((before, picked)) = self.station_profile_before_activation.take() else {
            return;
        };
        if self.settings.active_station_profile == picked {
            let _ = self.switch_station_profile(&before);
        }
    }

    /// The step of the log funnel that stamps the active profile's own fields (module header).
    pub(super) fn stamp_station_profile(&self, rec: &mut QsoRecord) {
        let Some(profile) = self.settings.active_station_profile() else {
            return;
        };
        if now_unix_secs().saturating_sub(rec.when_unix) > STATION_FIELDS_MAX_AGE_SECS {
            return;
        }
        // THE PROFILE'S PARK, on a contact no activation tagged. The activation's stamp ran first
        // and wins: a contact carrying any park keeps it.
        let park = (!profile.park.is_empty()).then_some(profile.park.as_str());
        if let (None, Some(park)) = (&rec.ota.my_ref, park) {
            rec.ota.my_program = Some("POTA".into());
            rec.ota.my_ref = Some(park.to_string());
        }
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
        // WHERE THE PROFILE IS: only on a contact made there.
        if rec.ota.my_ref.is_none() || at_profiles_park {
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
