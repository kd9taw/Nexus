//! The Stats pane: what the operator's log holds for the station typed into a log entry, before
//! the contact is logged — its DXCC entity's band × mode grid, the same grid for its US state
//! (WAS) and its CQ zone (WAZ), the NEW ONE / NEW BAND / NEW MODE flags, and which confirmation
//! channels the log holds for its country and for this call.
//!
//! # One fold, the awards' own
//!
//! The index is filled by the award fold itself ([`crate::Awards::with_call_stats`]), at the
//! points where that fold credits DXCC, WAS and WAZ, so a slot here is filled exactly when a
//! contact counts toward the award and nowhere else:
//! - the entity is the one the contact's record names ([`counted_entity`]: LoTW's DXCC number,
//!   then the log's, else the call through cty.dat, a WAE area as its parent);
//! - the zone is [`counted_zone`]'s (the record's CQZ, else the call's);
//! - a state counts only for the United States, Alaska and Hawaii;
//! - no DXCC slot for a satellite contact, for a deleted entity, on 60 m and 4 m, which earn
//!   no DXCC credit, or on 70 cm, 33 cm and 23 cm, which keep no DXCC band award
//!   ([`Slot::NoCredit`]).
//!
//! "Confirmed" is what each award credits: DXCC and WAS a LoTW or paper QSL, WAZ also an eQSL from
//! an Authenticity-Guaranteed member ([`QslChannels::eqsl_ag_confirmed`]).
//!
//! # The station typed has no record
//!
//! Its entity and zone come from its call alone, as a station heard on the air does on the
//! Needed board; its state is the app's answer for the call (the FCC licence index), handed to
//! [`CallStatsIndex::stats_for`]. Asking costs a few lookups: the log was folded once, when it
//! last changed.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::awards::{counted_entity, counted_zone, fills_dxcc_band_slot, valid_state};
use crate::awards::{QslChannels, RecordFields};
use crate::dxcc;
use crate::model::{exact_mode, mode_key, Band, ModeClass};

/// The grids' rows, in the order the Awards view lists its mode awards.
const CLASSES: [ModeClass; 3] = [ModeClass::Cw, ModeClass::Phone, ModeClass::Digital];

/// The grids' columns: every band the awards keep a slot for.
const BANDS: usize = Band::ALL.len();

/// One grid: a slot per mode class ([`CLASSES`]) and band ([`Band::ALL`]).
type Grid = [[Slot; BANDS]; 3];

/// Where a slot stands for an award.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Slot {
    /// No contact in the log counts toward it.
    #[default]
    Unworked,
    /// Worked, and confirmed by nothing the award credits.
    Worked,
    /// Confirmed by a channel the award credits.
    Confirmed,
    /// The award keeps no slot on this band: DXCC on 60 m, 4 m, 70 cm, 33 cm and 23 cm.
    NoCredit,
}

fn band_at(b: Band) -> usize {
    Band::ALL.iter().position(|x| *x == b).unwrap_or(0)
}

fn class_at(c: ModeClass) -> usize {
    CLASSES.iter().position(|x| *x == c).unwrap_or(0)
}

/// Fill one slot, never lowering it: a confirmed slot stays confirmed whatever follows.
fn mark(grid: &mut Grid, band: Option<Band>, class: ModeClass, confirmed: bool) {
    if let Some(b) = band {
        let slot = &mut grid[class_at(class)][band_at(b)];
        *slot = (*slot).max(if confirmed {
            Slot::Confirmed
        } else {
            Slot::Worked
        });
    }
}

/// The Stats pane's index of the log, built by the award fold ([`crate::Awards::with_call_stats`])
/// and asked once per call typed ([`stats_for`](Self::stats_for)).
#[derive(Debug, Clone, Default)]
pub struct CallStatsIndex {
    /// DXCC slots, by the entity a contact counts toward.
    dxcc: HashMap<&'static str, Grid>,
    /// Every entity a contact earns DXCC credit for, with the modes it was worked in
    /// ([`mode_key`]) — a slot or not, since a band no slot names still credits the entity.
    modes: HashMap<&'static str, HashSet<String>>,
    /// WAS slots, by state code.
    was: HashMap<&'static str, Grid>,
    /// WAZ slots, by zone.
    waz: HashMap<u8, Grid>,
    /// The channels a confirmation came through, per entity counted (whatever the award
    /// credits) and per call (trimmed and upper-cased, as the callsign card matches one).
    entity_qsl: HashMap<&'static str, QslChannels>,
    call_qsl: HashMap<String, QslChannels>,
}

impl CallStatsIndex {
    /// A contact's confirmation channels, under the entity it counts toward and under its call.
    /// A contact confirmed by none adds nothing: an absent entry and an empty one read alike.
    pub(crate) fn note_qsl(&mut self, call: &str, entity: Option<&'static str>, qsl: QslChannels) {
        if qsl == QslChannels::default() {
            return;
        }
        if let Some(e) = entity {
            let held = self.entity_qsl.entry(e).or_default();
            *held = held.or(qsl);
        }
        let held = self
            .call_qsl
            .entry(call.trim().to_ascii_uppercase())
            .or_default();
        *held = held.or(qsl);
    }

    /// A contact the fold credits a DXCC entity for.
    pub(crate) fn note_dxcc(
        &mut self,
        entity: &'static str,
        band: Option<Band>,
        mode: &str,
        confirmed: bool,
    ) {
        self.modes.entry(entity).or_default().insert(mode_key(mode));
        mark(
            self.dxcc.entry(entity).or_default(),
            band,
            ModeClass::from_adif(mode),
            confirmed,
        );
    }

    /// A contact the fold credits a WAS state for.
    pub(crate) fn note_was(
        &mut self,
        state: &'static str,
        band: Option<Band>,
        class: ModeClass,
        confirmed: bool,
    ) {
        mark(self.was.entry(state).or_default(), band, class, confirmed);
    }

    /// A contact the fold credits a WAZ zone for; `confirmed` as WAZ credits it.
    pub(crate) fn note_waz(
        &mut self,
        zone: u8,
        band: Option<Band>,
        class: ModeClass,
        confirmed: bool,
    ) {
        mark(self.waz.entry(zone).or_default(), band, class, confirmed);
    }

    /// What the log holds for `call`, typed in a log entry on `band` in `mode`. `state` is the
    /// app's answer for where the station is (its licence's state); it counts only for a station
    /// in the WAS family.
    pub fn stats_for(&self, call: &str, band: &str, mode: &str, state: Option<&str>) -> CallStats {
        let call = call.trim().to_ascii_uppercase();
        let resolved = dxcc::resolve(&call);
        // Heard, not logged: no record, so the call alone.
        let heard = RecordFields::default();
        let entity = counted_entity(resolved.as_ref(), &heard).filter(|c| !c.deleted);
        let was_family = entity.is_some_and(|c| c.is_us_state());
        let state = state.and_then(valid_state).filter(|_| was_family);
        let zone = counted_zone(resolved.as_ref(), &heard);
        let on_band = Band::from_label(band);
        let class = ModeClass::from_adif(mode);

        let dxcc_grid = entity.and_then(|e| self.dxcc.get(e.name));
        let modes = entity.and_then(|e| self.modes.get(e.name));
        let new_one = entity.is_some() && modes.is_none();
        let new_band = modes.is_some()
            && on_band.is_some_and(|b| {
                fills_dxcc_band_slot(b)
                    && dxcc_grid
                        .is_none_or(|g| g.iter().all(|row| row[band_at(b)] == Slot::Unworked))
            });
        let new_mode = modes.is_some_and(|worked| match exact_mode(mode) {
            Some(key) => !worked.contains(&key),
            None => !worked.iter().any(|m| ModeClass::from_adif(m) == class),
        });

        CallStats {
            bands: Band::ALL.iter().map(|b| b.label()).collect(),
            modes: CLASSES.iter().map(|c| c.label()).collect(),
            new_one,
            new_band,
            new_mode,
            dxcc: entity.map(|e| StatsGrid {
                label: e.name.to_string(),
                code: e.code,
                rows: rows(dxcc_grid, true),
            }),
            was_family,
            was: state.map(|s| StatsGrid {
                label: s.to_string(),
                code: None,
                rows: rows(self.was.get(s), false),
            }),
            waz: zone.map(|z| StatsGrid {
                label: z.to_string(),
                code: None,
                rows: rows(self.waz.get(&z), false),
            }),
            country_qsl: entity.map(|e| self.entity_qsl.get(e.name).copied().unwrap_or_default()),
            call_qsl: self.call_qsl.get(&call).copied().unwrap_or_default(),
            call,
        }
    }
}

/// A grid's rows as the pane draws them: DXCC's 60 m, 4 m and UHF columns are no slot at all.
fn rows(grid: Option<&Grid>, dxcc: bool) -> Vec<Vec<Slot>> {
    let empty = Grid::default();
    let grid = grid.unwrap_or(&empty);
    grid.iter()
        .map(|row| {
            Band::ALL
                .iter()
                .map(|b| {
                    if dxcc && !fills_dxcc_band_slot(*b) {
                        Slot::NoCredit
                    } else {
                        row[band_at(*b)]
                    }
                })
                .collect()
        })
        .collect()
}

/// One award's grid for the station typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsGrid {
    /// The entity's name, the state's postal code, or the zone's number.
    pub label: String,
    /// The entity's DXCC number, where cty.csv gives one (DXCC only).
    pub code: Option<u32>,
    /// One row per mode class ([`CallStats::modes`]), one slot per band ([`CallStats::bands`]).
    pub rows: Vec<Vec<Slot>>,
}

/// What the Stats pane shows for one call ([`CallStatsIndex::stats_for`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallStats {
    /// The call asked about, trimmed and upper-cased.
    pub call: String,
    /// The grids' columns, 160 m to 2 m.
    pub bands: Vec<&'static str>,
    /// The grids' rows: CW, Phone, Digital.
    pub modes: Vec<&'static str>,
    /// No contact in the log counts toward the station's DXCC entity.
    pub new_one: bool,
    /// The entity is worked, but on no slot of this band (a band that earns DXCC credit).
    pub new_band: bool,
    /// The entity is worked, but never in this mode — FT8 and FT4 are two modes, as everywhere
    /// a "new mode" is shown.
    pub new_mode: bool,
    /// `None`: the call names no DXCC entity.
    pub dxcc: Option<StatsGrid>,
    /// The station's entity is the United States, Alaska or Hawaii, so it has a state for WAS.
    pub was_family: bool,
    /// `None`: not in the WAS family, or no state is known for the call.
    pub was: Option<StatsGrid>,
    /// `None`: the call gives no CQ zone.
    pub waz: Option<StatsGrid>,
    /// The channels any contact with the entity was confirmed through; `None` with no entity.
    pub country_qsl: Option<QslChannels>,
    /// The channels any contact with this call was confirmed through.
    pub call_qsl: QslChannels,
}
