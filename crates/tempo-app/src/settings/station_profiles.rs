//! "My station" profiles: named setups of the station an operator logs from — Home, a portable
//! kit, a club call — switched from the top bar, and picked by itself for a POTA activation whose
//! park one of them names.
//!
//! # The shape: the flat fields are the active profile
//!
//! Over three hundred readers take the station's call and grid from [`Settings::mycall`] and
//! [`Settings::mygrid`]: the encoders, the identity gate at the keying boundary, the feeds, the
//! log funnel. A profile does not add a second place to read them from. The flat fields ARE the
//! active profile's call, grid, state and power, exactly as the flat rig fields are the active
//! radio's ([`Settings::sync_active_from_flat`]); a switch writes the profile it leaves from the
//! flat fields, then the flat fields from the profile it goes to, and the save that follows
//! rebinds the identity the way a callsign typed in Settings always has. Every transmit check
//! therefore reads the active profile's call, with nothing to keep in step.
//!
//! The rest of a profile (county, zones, IOTA, park, antenna, a rig description) exists only in
//! the profile, and only the log funnel reads it (`Engine::stamp_station_profile`).
//!
//! # A station that never makes one
//!
//! Nothing is stored until the operator makes a profile: both fields are skipped when empty, so
//! the file is byte for byte what it was, and the log funnel stamps exactly what it stamped
//! before. Home is implicit until then: it is today's Settings ▸ Station. The first profile made
//! writes Home beside it, built from those values ([`Settings::ensure_station_profiles`]).
//!
//! # One writer
//!
//! The list and the active id are written by their own verbs (`Engine::switch_station_profile`,
//! `Engine::save_station_profile`, `Engine::remove_station_profile`), never by a whole-settings
//! save: a form is a snapshot from whenever it loaded, and a stale one must not switch the
//! station back to the call it had then. A save's flat station fields edit the profile the station
//! is on, and only when the form was on it ([`Settings::keep_station_form`]).

use super::Settings;
use serde::{Deserialize, Serialize};

/// The id of the implicit Home profile: today's Settings ▸ Station.
pub const HOME_STATION_PROFILE: &str = "home";

/// ⛔ Why nothing transmits while a profile is on a callsign other than Home's and a licence class
/// is set. The class in Settings ▸ Station is the operator's own; whether a club or another
/// operator's call shares it is not something Nexus can know, and a guess could put a signal
/// where that call has no privileges. So it does not guess.
pub const STATION_PROFILE_CALL_TX_REFUSED: &str = "This station profile uses another callsign than your Home profile. Your licence class in Settings ▸ Station is for your own call, and Nexus will not guess the privileges of another, so it will not transmit. Switch to a profile with your own call to transmit.";

/// ⛔ Why the station profile does not change now: a run, a contact or an over is under way. The
/// FT sequencer sends the call it was started with, so a switch now would put one call on the air
/// and log the contact under another (`Engine::station_profile_switch_refusal`).
pub const STATION_PROFILE_SWITCH_REFUSED: &str = "The station profile changes only between contacts. Stop transmitting and end the run or contact (S&P) first, then switch.";

/// ⛔ Why the station profile does not change now: a JS8 message is queued or going out. JS8 builds
/// its frames with the call when the message is queued, so they would go out under the call the
/// station has left.
pub const STATION_PROFILE_JS8_QUEUED_REFUSED: &str = "The station profile waits while a JS8 message is queued or going out under this call. Let it finish, or clear the JS8 queue, then switch.";

/// ⛔ Why the station profile does not change now: a contact is finished and not logged yet. Its
/// overs went out under this call, and the log takes the call the station has when it is logged.
pub const STATION_PROFILE_UNLOGGED_REFUSED: &str = "The station profile waits until the contact you just finished is logged. Press Log QSO, or Clear DX to leave it out, then switch.";

/// ⛔ Why the station profile does not change now: JTTY is asking whether to log the contact its
/// TU just ended, and the screen logs it under the call the station has when it is answered.
pub const STATION_PROFILE_JTTY_PROMPT_REFUSED: &str = "The station profile waits while JTTY asks to log the contact you just finished. Answer it (Log QSO or Not now), then switch.";

/// ⛔ Why a settings restore or reset is refused now: it would change the call the station
/// transmits under while a run, a contact, an over or a JS8 message is under way.
pub const STATION_PROFILE_RESTORE_REFUSED: &str = "Settings are not restored or reset while a run, a contact, an over or a JS8 message is under way: that would change the call the station transmits under. Stop transmitting and end the run or contact (S&P) first, then try again.";

/// ⛔ Why a message is not sent: it was made under another callsign than the one the station
/// transmits under now (the keying-time call check, `Engine::sent_call_refusal`). A CQ, a reply,
/// a queued text or a picture keeps the call it was built with; once the station's call has
/// changed it would go out under the old one, so it is dropped instead.
pub const STATION_CALL_CHANGED_TX_REFUSED: &str = "Not sent: this message was made under another callsign than the one your station uses now. Send it again, or end the run and start it again, to go out under your current call.";

/// One named station setup. Every field but the id and name is optional: blank means "not
/// stated", and a blank field stamps nothing on a contact.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StationProfile {
    /// Stable id: [`HOME_STATION_PROFILE`] for Home, `p1`, `p2`… for the others.
    pub id: String,
    /// The name the operator gave it. Home's is blank and the screen names it in its language.
    pub name: String,
    /// The call it transmits and logs under (`STATION_CALLSIGN`).
    pub call: String,
    /// Its grid: sent on the air, and `MY_GRIDSQUARE`.
    pub grid: String,
    /// `MY_STATE` (US state, or province).
    pub state: String,
    /// `MY_CNTY`. "Dane" with a state of WI is written "WI,Dane", the form ADIF asks for.
    pub county: String,
    /// `MY_CQ_ZONE`, 1–40.
    pub cq_zone: Option<u8>,
    /// `MY_ITU_ZONE`, 1–90.
    pub itu_zone: Option<u8>,
    /// `MY_IOTA` ("EU-005").
    pub iota: String,
    /// The POTA park it is set up for: `MY_POTA_REF` when no activation tags the contact, and
    /// the park an activation picks it by.
    pub park: String,
    /// `TX_PWR` in watts, where the radio did not measure the power.
    pub power_w: Option<f64>,
    /// `MY_RIG`, when stated: what the operator calls the rig here, over the radio's model.
    pub rig: String,
    /// `MY_ANTENNA`.
    pub antenna: String,
}

impl StationProfile {
    /// The profile as Nexus keeps it: trimmed, the call, grid, state, IOTA and park in the forms
    /// they are written in; `Err` names the first field that is not what it says it is. Home's
    /// name may be blank; every other profile needs one.
    pub fn normalized(mut self) -> Result<Self, String> {
        self.name = self.name.trim().to_string();
        if self.name.is_empty() && self.id != HOME_STATION_PROFILE {
            return Err("Give the profile a name.".into());
        }
        self.call = self.call.trim().to_ascii_uppercase();
        if !tempo_core::message::is_callsign(&self.call) {
            return Err(format!("'{}' isn't a callsign.", self.call));
        }
        self.grid = self.grid.trim().to_ascii_uppercase();
        if !tempo_core::message::is_valid_grid(&self.grid) {
            return Err(format!("'{}' isn't a grid square.", self.grid));
        }
        self.state = self.state.trim().to_ascii_uppercase();
        self.county = self.county.trim().to_string();
        if let Some(z) = self.cq_zone.filter(|z| !(1..=40).contains(z)) {
            return Err(format!("CQ zone {z} isn't 1 to 40."));
        }
        if let Some(z) = self.itu_zone.filter(|z| !(1..=90).contains(z)) {
            return Err(format!("ITU zone {z} isn't 1 to 90."));
        }
        let iota = self.iota.trim().to_ascii_uppercase();
        if !iota.is_empty() && !tempo_core::logbook::cleanup::valid_iota(&iota) {
            return Err(format!("'{iota}' isn't an IOTA reference (EU-005)."));
        }
        self.iota = iota;
        let park = self.park.trim();
        self.park = if park.is_empty() {
            String::new()
        } else {
            tempo_core::pota::normalize_pota_ref(park)
                .ok_or_else(|| format!("'{park}' isn't a POTA park reference."))?
        };
        self.power_w = self.power_w.filter(|w| w.is_finite() && *w > 0.0);
        self.rig = self.rig.trim().to_string();
        self.antenna = self.antenna.trim().to_string();
        Ok(self)
    }

    /// `MY_CNTY` as written: the county as typed when it carries its state ("WI,Dane"), else the
    /// state and the county; nothing without a county.
    pub fn my_cnty(&self) -> Option<String> {
        let county = self.county.trim();
        if county.is_empty() {
            return None;
        }
        let state = self.state.trim();
        Some(if county.contains(',') || state.is_empty() {
            county.to_string()
        } else {
            format!("{state},{county}")
        })
    }
}

impl Settings {
    /// The profile the station is on, when it has any.
    pub fn active_station_profile(&self) -> Option<&StationProfile> {
        self.station_profiles
            .iter()
            .find(|p| p.id == self.active_station_profile)
    }

    /// Home, built from today's Settings ▸ Station.
    fn home_from_flat(&self) -> StationProfile {
        StationProfile {
            id: HOME_STATION_PROFILE.into(),
            call: self.mycall.trim().to_ascii_uppercase(),
            grid: self.mygrid.trim().to_ascii_uppercase(),
            state: self.op_state.trim().to_ascii_uppercase(),
            power_w: self.station_power_w,
            ..StationProfile::default()
        }
    }

    /// Write Home beside the first profile made, from today's values, and put the station on it.
    /// A station with profiles is left as it is, except that an active id naming no profile is
    /// put back on Home.
    pub fn ensure_station_profiles(&mut self) {
        if self.station_profiles.is_empty() {
            self.station_profiles.push(self.home_from_flat());
            self.active_station_profile = HOME_STATION_PROFILE.into();
        } else {
            self.repair_active_station_profile();
        }
    }

    /// ⛔ An active id that names no profile (a hand-edited file, a bundle from another install)
    /// is put on Home, or on the first profile when there is no Home, and the station takes that
    /// profile's call, grid, state and power. Never the other way round: the flat call may be a
    /// club's, and folded into Home it would lift the licence refusal for good. Run at load and on
    /// a restore. With no profiles, or a valid id, nothing changes.
    pub fn repair_active_station_profile(&mut self) {
        if self.station_profiles.is_empty() || self.active_station_profile().is_some() {
            return;
        }
        let id = if self
            .station_profiles
            .iter()
            .any(|p| p.id == HOME_STATION_PROFILE)
        {
            HOME_STATION_PROFILE.to_string()
        } else {
            self.station_profiles[0].id.clone()
        };
        self.active_station_profile = id;
        self.sync_flat_from_active_station();
    }

    /// The active profile takes the flat call, grid, state and power: what Settings ▸ Station,
    /// a GPS feeder or a callsign edit last set them to.
    pub fn sync_active_station_from_flat(&mut self) {
        let id = self.active_station_profile.clone();
        self.fold_flat_into_station_profile(&id);
    }

    fn fold_flat_into_station_profile(&mut self, id: &str) {
        let (call, grid, state, power) = (
            self.mycall.clone(),
            self.mygrid.clone(),
            self.op_state.clone(),
            self.station_power_w,
        );
        if let Some(p) = self.station_profiles.iter_mut().find(|p| p.id == id) {
            p.call = call;
            p.grid = grid;
            p.state = state;
            p.power_w = power;
        }
    }

    /// The flat call, grid, state and power take the active profile's.
    pub fn sync_flat_from_active_station(&mut self) {
        let Some(p) = self.active_station_profile().cloned() else {
            return;
        };
        self.mycall = p.call;
        self.mygrid = p.grid;
        self.op_state = p.state;
        self.station_power_w = p.power_w;
    }

    /// After a whole-settings save adopted the LIVE list and active id: the save's flat station
    /// fields describe the profile its form was on (`form_profile`, the save's own active id).
    /// ⛔ A whole save edits the profile the station is on, and no other: when the form names
    /// another profile (a form loaded before a switch, or one that names it on purpose), its flat
    /// call, grid, state and power are left out, and the station keeps the active profile's. So a
    /// save never writes a call into a profile the station is not on: that would change the call
    /// another profile transmits under, or lift the licence refusal by giving Home the club's
    /// call. Those are changed in the profile editor, at the station. A save naming no profile, or
    /// one the list does not have (a payload from before profiles), describes the active one.
    pub fn keep_station_form(&mut self, form_profile: &str) {
        if self.station_profiles.is_empty() {
            return;
        }
        let another = form_profile != self.active_station_profile
            && self.station_profiles.iter().any(|p| p.id == form_profile);
        if another {
            self.sync_flat_from_active_station();
        } else {
            self.sync_active_station_from_flat();
        }
    }

    /// ⛔ [`STATION_PROFILE_CALL_TX_REFUSED`] while the station is on a profile whose call is not
    /// Home's (the base call: Home's own call with a portable affix, `KD9TAW/P`, is the same
    /// licence) and a licence class is set. `Open` applies no privilege gate to any call, so there
    /// is nothing to decide. Home itself never refuses.
    pub fn station_profile_tx_refusal(&self) -> Option<&'static str> {
        if self.license_class == super::LicenseClass::Open || self.station_profiles.is_empty() {
            return None;
        }
        // An active id that names no profile refuses until it is repaired: it never lifts this.
        let Some(active) = self.active_station_profile() else {
            return Some(STATION_PROFILE_CALL_TX_REFUSED);
        };
        if active.id == HOME_STATION_PROFILE {
            return None;
        }
        let home = self
            .station_profiles
            .iter()
            .find(|p| p.id == HOME_STATION_PROFILE);
        let same = home.is_some_and(|h| {
            let base = tempo_core::message::base_call(&h.call);
            !base.is_empty() && base == tempo_core::message::base_call(&self.mycall)
        });
        (!same).then_some(STATION_PROFILE_CALL_TX_REFUSED)
    }

    /// An id for a new profile: `p1`, `p2`… the first not taken.
    pub fn next_station_profile_id(&self) -> String {
        (1..)
            .map(|n| format!("p{n}"))
            .find(|id| !self.station_profiles.iter().any(|p| &p.id == id))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn station() -> Settings {
        Settings {
            mycall: "KD9TAW".into(),
            mygrid: "EN52".into(),
            op_state: "WI".into(),
            station_power_w: Some(100.0),
            ..Settings::default()
        }
    }

    fn portable() -> StationProfile {
        StationProfile {
            id: "p1".into(),
            name: "Portable".into(),
            call: "KD9TAW/P".into(),
            grid: "EN53".into(),
            state: "WI".into(),
            power_w: Some(10.0),
            ..StationProfile::default()
        }
    }

    /// A station that never makes a profile writes the file it always wrote: neither key appears.
    #[test]
    fn a_station_with_no_profiles_writes_no_profile_keys() {
        let json = serde_json::to_string(&station()).unwrap();
        assert!(!json.contains("stationProfiles"), "{json}");
        assert!(!json.contains("activeStationProfile"), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
    }

    /// The first profile writes Home beside it from today's values, and the pair round-trips.
    #[test]
    fn home_is_todays_station_and_profiles_round_trip() {
        let mut s = station();
        s.ensure_station_profiles();
        s.station_profiles.push(portable());
        let home = &s.station_profiles[0];
        assert_eq!(
            (home.id.as_str(), home.call.as_str(), home.grid.as_str()),
            (HOME_STATION_PROFILE, "KD9TAW", "EN52")
        );
        assert_eq!(home.power_w, Some(100.0));
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.station_profiles, s.station_profiles);
        assert_eq!(back.active_station_profile, HOME_STATION_PROFILE);
    }

    /// A form loaded on Home and saved after the station switched to Portable: its call goes
    /// nowhere (Home keeps its own), and the flat fields stay Portable's.
    #[test]
    fn a_stale_form_writes_no_call_into_the_profile_it_was_on() {
        let mut s = station();
        s.ensure_station_profiles();
        s.station_profiles.push(portable());
        s.active_station_profile = "p1".into();
        s.sync_flat_from_active_station();
        // The stale form: Home's flat values, with the call edited.
        s.mycall = "KD9TAX".into();
        s.mygrid = "EN52".into();
        s.keep_station_form(HOME_STATION_PROFILE);
        assert_eq!(s.station_profiles[0].call, "KD9TAW", "Home keeps its call");
        assert_eq!(s.mycall, "KD9TAW/P", "the station stays on Portable");
        assert_eq!(s.mygrid, "EN53");
    }

    /// A club profile (W9ABC) beside Home, under a General licence, with an active id that names
    /// no profile (a hand-edited file, a bundle from another install) and the club's call in the
    /// flat field.
    fn dangling(home: bool) -> Settings {
        let mut s = station();
        s.license_class = crate::settings::LicenseClass::General;
        s.ensure_station_profiles();
        s.station_profiles.push(StationProfile {
            call: "W9ABC".into(),
            ..portable()
        });
        if !home {
            s.station_profiles.remove(0);
        }
        s.active_station_profile = "p9".into();
        s.mycall = "W9ABC".into();
        s
    }

    /// ⛔ An active id that names no profile never lifts the licence refusal: it refuses until it
    /// is repaired, and loading the file puts the station on Home, with Home's own call, or on the
    /// first profile when there is no Home.
    #[test]
    fn a_dangling_active_id_is_repaired_at_load_and_never_lifts_the_refusal() {
        assert_eq!(
            dangling(true).station_profile_tx_refusal(),
            Some(STATION_PROFILE_CALL_TX_REFUSED),
            "refused before the repair"
        );
        let path = std::env::temp_dir().join(format!(
            "tempo_station_profiles_dangling_{}.json",
            std::process::id()
        ));
        for (home, active, call, refused) in [
            (true, HOME_STATION_PROFILE, "KD9TAW", false),
            (false, "p1", "W9ABC", true),
        ] {
            dangling(home).save(&path).unwrap();
            let back = Settings::load(&path);
            assert_eq!(
                (
                    back.active_station_profile.as_str(),
                    back.mycall.as_str(),
                    back.station_profile_tx_refusal().is_some(),
                ),
                (active, call, refused),
                "with Home: {home}"
            );
        }
        let _ = std::fs::remove_file(&path);
    }

    /// A station with no profiles: a full stored settings file (every field the UI's fixture
    /// carries, and one written from the defaults) loads and is written back with exactly the keys
    /// it had, none of them a profile key, and a second load writes it byte for byte. (The first
    /// load's own migrations, as on a station before profiles, may reword a value: the macros'
    /// defaults, an empty uplink list.)
    #[test]
    fn a_full_stored_file_with_no_profiles_round_trips_with_no_new_keys() {
        let dir = std::env::temp_dir().join(format!(
            "tempo_station_profiles_roundtrip_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let fixture: Settings = serde_json::from_str(include_str!(
            "../../../../ui/src/components/__fixtures__/defaultSettings.json"
        ))
        .unwrap();
        let keys = |text: &str| {
            let v: serde_json::Value = serde_json::from_str(text).unwrap();
            v.as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
        };
        for (name, start) in [("default", Settings::default()), ("fixture", fixture)] {
            assert!(start.station_profiles.is_empty(), "premise: {name}");
            let file = |n: &str| dir.join(format!("{name}-{n}.json"));
            start.save(&file("a")).unwrap();
            Settings::load(&file("a")).save(&file("b")).unwrap();
            Settings::load(&file("b")).save(&file("c")).unwrap();
            let read = |n: &str| std::fs::read_to_string(file(n)).unwrap();
            let (a, b, c) = (read("a"), read("b"), read("c"));
            assert_eq!(keys(&a), keys(&b), "{name}: the same keys written back");
            assert_eq!(b, c, "{name}: written back byte for byte");
            for key in [
                "stationProfiles",
                "activeStationProfile",
                "stationProfileBack",
            ] {
                assert!(!a.contains(key) && !b.contains(key), "{name}: no {key}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalizing_refuses_what_is_not_what_it_says() {
        assert!(portable().normalized().is_ok());
        let bad = |f: fn(&mut StationProfile)| {
            let mut p = portable();
            f(&mut p);
            p.normalized().is_err()
        };
        assert!(bad(|p| p.call = "NOTACALL!".into()));
        assert!(bad(|p| p.grid = "12AB".into()));
        assert!(bad(|p| p.name = " ".into()));
        assert!(bad(|p| p.cq_zone = Some(41)));
        assert!(bad(|p| p.itu_zone = Some(0)));
        assert!(bad(|p| p.iota = "EU-5".into()));
        assert!(bad(|p| p.park = "nowhere".into()));
        let mut p = portable();
        p.park = "us-1234".into();
        p.iota = "eu-005".into();
        let p = p.normalized().unwrap();
        assert_eq!((p.park.as_str(), p.iota.as_str()), ("US-1234", "EU-005"));
    }

    #[test]
    fn the_county_carries_its_state() {
        let mut p = portable();
        p.county = "Dane".into();
        assert_eq!(p.my_cnty().as_deref(), Some("WI,Dane"));
        p.county = "IL,Cook".into();
        assert_eq!(p.my_cnty().as_deref(), Some("IL,Cook"));
        p.county.clear();
        assert_eq!(p.my_cnty(), None);
    }
}
