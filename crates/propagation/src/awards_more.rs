//! The awards counted beside DXCC in the same fold: CQ WPX ([`crate::wpx`]), DARC's WAE
//! ([`crate::wae`]) and WWFF ([`crate::wwff`]); the whole-log DXCC chart (entities × bands, by
//! mode); and "What to QSL", the unconfirmed contacts whose confirmation would fill a slot an
//! award still needs.
//!
//! Every slot here is one the award fold in [`crate::awards`] credits: the fold hands this
//! module what it credited for each contact ([`Credited`]), so the chart can never show an
//! entity, band or mode the DXCC counts disagree with, and a QSL wanted is always for a slot the
//! Awards view counts as needed.
//!
//! The rule sources are cited in each award's own module; where a rule could not be verified the
//! module says so.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::awards::{BandAward, RecordFields};
use crate::model::{Band, ModeClass};
use crate::needalert::NeedAlert;

/// What the award fold credited one contact with — the slots it filled, whether or not they were
/// confirmed — handed here once the fold has placed it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Credited {
    /// The current DXCC entity it counts toward (terrestrial, on a band that earns credit).
    pub entity: Option<&'static str>,
    /// The DXCC band slot it fills, where its band has one.
    pub dxcc_band: Option<Band>,
    /// The entity it counts toward whatever the band or satellite — WWFF's DX entities read it.
    pub any_entity: Option<&'static str>,
    /// Its CQ zone, and whether WAZ counts it confirmed (an Authenticity-Guaranteed eQSL too).
    pub zone: Option<(u8, bool)>,
    /// Its WAS state and WAP province, entity-gated as the awards gate them.
    pub state: Option<&'static str>,
    pub province: Option<&'static str>,
    /// Its terrestrial grid square and band label, as VUCC keys them.
    pub grid: Option<(String, String)>,
    /// Its WAE country ([`crate::wae::COUNTRIES`] index).
    pub wae: Option<u16>,
}

/// One contact's own particulars, for the slots it fills and the What-to-QSL row it may become.
pub(crate) struct Contact<'a> {
    pub call: &'a str,
    pub band: &'a str,
    pub on_band: Option<Band>,
    pub mode: &'a str,
    pub class: ModeClass,
    /// LoTW or a card — what every award here but WAZ counts as confirmed.
    pub confirmed: bool,
    pub when_unix: u64,
    pub wwff: Option<&'a str>,
    pub qsl_via: Option<&'a str>,
}

/// CQ WPX: distinct prefixes worked / confirmed, overall and per band.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WpxProgress {
    pub worked: usize,
    pub confirmed: usize,
    /// Per-band prefix counts, low band first, bands with a contact only.
    pub bands: Vec<BandAward>,
    /// Prefixes worked but not confirmed, sorted.
    pub unconfirmed: Vec<String>,
}

/// One WAE country, as DARC's list names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WaeCountry {
    pub prefix: String,
    pub name: String,
}

/// DARC's Worked All Europe: countries and band points ([`crate::wae`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WaeProgress {
    pub worked: usize,
    pub confirmed: usize,
    /// The countries on DARC's list: 73.
    pub total: usize,
    /// Band points: one per confirmed country per band, at most 5 bands a country.
    pub points: usize,
    /// The same counted over worked contacts.
    pub points_worked: usize,
    /// The highest class reached ("TOP", "I", "II", "III"), by countries AND points confirmed.
    pub class: Option<String>,
    /// Every listed country confirmed on 5 bands (the WAE-Trophy).
    pub trophy: bool,
    /// Never worked, in DARC's order.
    pub unworked: Vec<WaeCountry>,
    /// Worked but not confirmed, in DARC's order.
    pub unconfirmed: Vec<WaeCountry>,
}

/// WWFF references worked ([`crate::wwff`]): WWFF credits only its own log search, so there is
/// no confirmed count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WwffProgress {
    /// Distinct references worked.
    pub worked: usize,
    /// Distinct DXCC entities among them (DXFF-H).
    pub entities: usize,
    /// Distinct national programmes among them ("KFF", "DLFF").
    pub programmes: usize,
    /// The highest WWFF-H level reached, if any.
    pub level: Option<usize>,
    /// The next WWFF-H level.
    pub next_level: usize,
}

/// The whole-log DXCC chart: every current entity × every DXCC band, by mode class.
///
/// A cell is six bits: worked / confirmed for CW (bits 0, 1), Phone (2, 3) and Digital (4, 5) —
/// [`CHART_WORKED`] and [`CHART_CONFIRMED`] for each class. Any mode is the OR of the three.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DxccChart {
    /// The column bands, low first: every band with a DXCC band slot.
    pub bands: Vec<String>,
    /// One row per current entity, alphabetical.
    pub rows: Vec<DxccChartRow>,
}

/// One entity's row of the [`DxccChart`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DxccChartRow {
    pub entity: String,
    /// The entity's own standing on any band, a band with no slot (70 cm) included.
    pub any: u8,
    /// One cell per [`DxccChart::bands`] column.
    pub cells: Vec<u8>,
}

/// The worked bit of `class` in a chart cell.
pub const fn chart_worked(class: ModeClass) -> u8 {
    1 << (2 * class_index(class))
}

/// The confirmed bit of `class` in a chart cell.
pub const fn chart_confirmed(class: ModeClass) -> u8 {
    2 << (2 * class_index(class))
}

/// Every worked bit, and every confirmed bit.
pub const CHART_WORKED: u8 = 0b01_0101;
pub const CHART_CONFIRMED: u8 = 0b10_1010;

const fn class_index(class: ModeClass) -> u8 {
    match class {
        ModeClass::Cw => 0,
        ModeClass::Phone => 1,
        ModeClass::Digital => 2,
    }
}

/// The awards a QSL can be wanted for, most valuable first — the What-to-QSL ranking's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QslFor {
    /// A DXCC entity not confirmed at all — a new one.
    DxccEntity,
    /// A DXCC Challenge band slot of an entity confirmed elsewhere.
    DxccBand,
    /// A CQ zone (WAZ).
    Waz,
    /// A US state (WAS).
    Was,
    /// A Canadian province or territory (WAP).
    Wap,
    /// A WAE country.
    Wae,
    /// A VUCC grid square on a VUCC band.
    Vucc,
    /// A WPX prefix.
    Wpx,
}

/// One slot a contact's confirmation would fill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QslSlot {
    pub award: QslFor,
    /// The entity, zone, state, province, country, grid or prefix — with the band where the slot
    /// is per band ("Japan 20m", "FN31 2m").
    pub item: String,
}

/// An unconfirmed contact whose confirmation fills a slot an award still needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QslWant {
    pub call: String,
    pub when_unix: u64,
    pub band: String,
    pub mode: String,
    /// The QSL route the log holds (ADIF `QSL_VIA`), if any.
    pub qsl_via: Option<String>,
    /// The slots it would fill, most valuable first.
    pub slots: Vec<QslSlot>,
}

/// The contacts kept per needed slot: the newest few, which are the likeliest to be confirmed
/// (a recent contact's station is the likeliest still to answer). A slot with more worked
/// contacts than this needs only one of them confirmed.
pub const WANTS_PER_SLOT: usize = 3;

/// A slot, keyed for the What-to-QSL fold.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Slot {
    Entity(&'static str),
    EntityBand(&'static str, Band),
    Zone(u8),
    State(&'static str),
    Province(&'static str),
    Wae(u16),
    Grid(String, String),
    Wpx(String),
}

impl Slot {
    fn describe(&self) -> QslSlot {
        let (award, item) = match self {
            Slot::Entity(e) => (QslFor::DxccEntity, (*e).to_string()),
            Slot::EntityBand(e, b) => (QslFor::DxccBand, format!("{e} {}", b.label())),
            Slot::Zone(z) => (QslFor::Waz, z.to_string()),
            Slot::State(s) => (QslFor::Was, (*s).to_string()),
            Slot::Province(p) => (QslFor::Wap, (*p).to_string()),
            Slot::Wae(i) => (
                QslFor::Wae,
                crate::wae::COUNTRIES[*i as usize].0.to_string(),
            ),
            Slot::Grid(g, b) => (QslFor::Vucc, format!("{g} {b}")),
            Slot::Wpx(p) => (QslFor::Wpx, p.clone()),
        };
        QslSlot { award, item }
    }
}

/// The WPX prefixes and WAE countries the log has worked on each band in each mode class (CW,
/// Phone, Digital), from the award fold: what the Needed board's opt-in WPX and WAE chips ask of a
/// station heard ([`mark_award_needs`]).
#[derive(Debug, Clone, Default)]
pub struct WpxWaeWorked {
    wpx: HashSet<(String, Band, ModeClass)>,
    wae: HashSet<(u16, Band, ModeClass)>,
}

/// The Needed board's opt-in WPX and WAE chips (operator ruling 2026-10-25: both off by default,
/// because almost every station heard is an unworked WPX prefix). With `wpx` on, a row whose
/// station would be a new WPX prefix on the row's band in its mode class names that prefix
/// ([`NeedAlert::wpx_need`]); with `wae` on, one that would be a new WAE country names the country
/// ([`NeedAlert::wae_need`]). Nothing else on a row moves — its tags, headline and priority, and
/// so every alert, stay as they were — and with both off no row is touched at all.
///
/// The station heard has no record, so its prefix and country come from its call alone, through
/// the resolvers the fold credits a logged contact's call with ([`crate::wpx::wpx_prefix`],
/// [`crate::wae::wae_country`]).
pub fn mark_award_needs(alerts: &mut [NeedAlert], worked: &WpxWaeWorked, wpx: bool, wae: bool) {
    if !wpx && !wae {
        return;
    }
    for a in alerts {
        let Some(band) = Band::from_label(&a.band) else {
            continue;
        };
        let class = ModeClass::from_adif(&a.mode);
        if wpx {
            a.wpx_need = crate::wpx::wpx_prefix(&a.call)
                .filter(|p| !worked.wpx.contains(&(p.clone(), band, class)));
        }
        if wae {
            let resolved = crate::dxcc::resolve(&a.call);
            a.wae_need = crate::wae::wae_country(resolved.as_ref(), &RecordFields::default())
                .filter(|i| !worked.wae.contains(&(*i, band, class)))
                .map(|i| crate::wae::COUNTRIES[i as usize].0.to_string());
        }
    }
}

/// A contact kept as a What-to-QSL candidate, and how many slots still keep it.
#[derive(Debug, Clone)]
struct Kept {
    call: String,
    when_unix: u64,
    band: String,
    mode: String,
    qsl_via: Option<String>,
    refs: u16,
}

/// The fold for everything in this module, kept inside [`crate::Awards`].
#[derive(Debug, Clone, Default)]
pub(crate) struct MoreAwards {
    seq: u32,
    wpx_worked: HashSet<String>,
    wpx_confirmed: HashSet<String>,
    wpx_band: HashSet<(String, Band)>,
    wpx_band_confirmed: HashSet<(String, Band)>,
    wae_worked: HashSet<u16>,
    wae_confirmed: HashSet<u16>,
    wae_band: HashSet<(u16, Band)>,
    wae_band_confirmed: HashSet<(u16, Band)>,
    pub(crate) slots_worked: WpxWaeWorked,
    wwff: HashSet<String>,
    wwff_entities: HashSet<&'static str>,
    /// `(entity, band)` → chart cell; `entity` → its any-band cell.
    chart: HashMap<(&'static str, Band), u8>,
    chart_any: HashMap<&'static str, u8>,
    /// Needed-slot candidates: the newest [`WANTS_PER_SLOT`] unconfirmed contacts of each slot
    /// not (yet) confirmed, by contact number.
    wants: HashMap<Slot, Vec<u32>>,
    kept: HashMap<u32, Kept>,
}

impl MoreAwards {
    /// Fold one contact in, with what the award fold credited it.
    pub(crate) fn add(&mut self, c: &Contact<'_>, cr: &Credited) {
        let n = self.seq;
        self.seq = self.seq.wrapping_add(1);
        // The slots this contact fills that its own confirmation would complete.
        let mut slots: Vec<Slot> = Vec::new();
        fn fill(slot: Slot, confirmed: bool, slots: &mut Vec<Slot>) {
            if !confirmed {
                slots.push(slot);
            }
        }

        if let Some(prefix) = crate::wpx::wpx_prefix(c.call) {
            self.wpx_worked.insert(prefix.clone());
            if let Some(b) = c.on_band {
                self.slots_worked.wpx.insert((prefix.clone(), b, c.class));
                self.wpx_band.insert((prefix.clone(), b));
                if c.confirmed {
                    self.wpx_band_confirmed.insert((prefix.clone(), b));
                }
            }
            if c.confirmed {
                self.wpx_confirmed.insert(prefix.clone());
            }
            fill(Slot::Wpx(prefix), c.confirmed, &mut slots);
        }
        if let Some(country) = cr.wae {
            self.wae_worked.insert(country);
            if let Some(b) = c.on_band {
                self.slots_worked.wae.insert((country, b, c.class));
                self.wae_band.insert((country, b));
                if c.confirmed {
                    self.wae_band_confirmed.insert((country, b));
                }
            }
            if c.confirmed {
                self.wae_confirmed.insert(country);
            }
            fill(Slot::Wae(country), c.confirmed, &mut slots);
        }
        if let Some(r) = c.wwff.and_then(crate::wwff::valid_wwff_ref) {
            self.wwff.insert(r);
            if let Some(e) = cr.any_entity {
                self.wwff_entities.insert(e);
            }
        }
        if let Some(entity) = cr.entity {
            let bits = chart_worked(c.class)
                | if c.confirmed {
                    chart_confirmed(c.class)
                } else {
                    0
                };
            *self.chart_any.entry(entity).or_default() |= bits;
            fill(Slot::Entity(entity), c.confirmed, &mut slots);
            if let Some(b) = cr.dxcc_band {
                *self.chart.entry((entity, b)).or_default() |= bits;
                if crate::awards::is_challenge_band(b) {
                    fill(Slot::EntityBand(entity, b), c.confirmed, &mut slots);
                }
            }
        }
        if let Some((zone, confirmed)) = cr.zone {
            fill(Slot::Zone(zone), confirmed, &mut slots);
        }
        if let Some(s) = cr.state {
            fill(Slot::State(s), c.confirmed, &mut slots);
        }
        if let Some(p) = cr.province {
            fill(Slot::Province(p), c.confirmed, &mut slots);
        }
        if let Some((g, b)) = &cr.grid {
            if crate::awards::is_vucc_band(b) {
                fill(Slot::Grid(g.clone(), b.clone()), c.confirmed, &mut slots);
            }
        }
        if slots.is_empty() {
            return;
        }
        let mut refs = 0u16;
        for slot in slots {
            let list = self.wants.entry(slot).or_default();
            if list.len() < WANTS_PER_SLOT {
                list.push(n);
                refs += 1;
                continue;
            }
            // Full: replace the oldest kept contact if this one is newer.
            let kept = &self.kept;
            let (pos, oldest) = list
                .iter()
                .enumerate()
                .map(|(i, k)| (i, kept.get(k).map_or(0, |k| k.when_unix)))
                .min_by_key(|(_, w)| *w)
                .expect("a full list is not empty");
            if c.when_unix > oldest {
                let out = list[pos];
                list[pos] = n;
                refs += 1;
                if let Some(k) = self.kept.get_mut(&out) {
                    k.refs -= 1;
                    if k.refs == 0 {
                        self.kept.remove(&out);
                    }
                }
            }
        }
        if refs > 0 {
            self.kept.insert(
                n,
                Kept {
                    call: c.call.trim().to_ascii_uppercase(),
                    when_unix: c.when_unix,
                    band: c.band.to_string(),
                    mode: c.mode.to_string(),
                    qsl_via: c
                        .qsl_via
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(str::to_string),
                    refs,
                },
            );
        }
    }

    pub(crate) fn wpx(&self) -> WpxProgress {
        let mut unconfirmed: Vec<String> = self
            .wpx_worked
            .difference(&self.wpx_confirmed)
            .cloned()
            .collect();
        unconfirmed.sort_unstable();
        WpxProgress {
            worked: self.wpx_worked.len(),
            confirmed: self.wpx_confirmed.len(),
            bands: per_band(&self.wpx_band, &self.wpx_band_confirmed),
            unconfirmed,
        }
    }

    pub(crate) fn wae(&self) -> WaeProgress {
        use crate::wae::{CLASSES, COUNTRIES, MAX_BANDS_PER_COUNTRY};
        let points = |set: &HashSet<(u16, Band)>| {
            let mut per: HashMap<u16, usize> = HashMap::new();
            for (c, _) in set {
                *per.entry(*c).or_default() += 1;
            }
            per.values()
                .map(|n| (*n).min(MAX_BANDS_PER_COUNTRY))
                .sum::<usize>()
        };
        let confirmed_points = points(&self.wae_band_confirmed);
        let confirmed = self.wae_confirmed.len();
        let country = |i: usize| WaeCountry {
            prefix: COUNTRIES[i].0.to_string(),
            name: COUNTRIES[i].1.to_string(),
        };
        let (mut unworked, mut unconfirmed) = (Vec::new(), Vec::new());
        for i in 0..COUNTRIES.len() {
            let k = i as u16;
            if !self.wae_worked.contains(&k) {
                unworked.push(country(i));
            } else if !self.wae_confirmed.contains(&k) {
                unconfirmed.push(country(i));
            }
        }
        let trophy = (0..COUNTRIES.len() as u16).all(|k| {
            self.wae_band_confirmed
                .iter()
                .filter(|(c, _)| *c == k)
                .count()
                >= MAX_BANDS_PER_COUNTRY
        });
        WaeProgress {
            worked: self.wae_worked.len(),
            confirmed,
            total: COUNTRIES.len(),
            points: confirmed_points,
            points_worked: points(&self.wae_band),
            class: CLASSES
                .iter()
                .find(|(_, countries, pts)| confirmed >= *countries && confirmed_points >= *pts)
                .map(|(name, _, _)| (*name).to_string()),
            trophy,
            unworked,
            unconfirmed,
        }
    }

    pub(crate) fn wwff(&self) -> WwffProgress {
        let worked = self.wwff.len();
        let programmes: BTreeSet<&str> = self
            .wwff
            .iter()
            .map(|r| crate::wwff::wwff_program(r))
            .collect();
        let reached = crate::wwff::hunter_levels_reached(worked);
        let level = reached.last().copied();
        WwffProgress {
            worked,
            entities: self.wwff_entities.len(),
            programmes: programmes.len(),
            level,
            next_level: crate::wwff::next_level_after(level.unwrap_or(0)),
        }
    }

    pub(crate) fn chart(&self) -> DxccChart {
        let bands: Vec<Band> = Band::ALL
            .into_iter()
            .filter(|b| crate::awards::fills_dxcc_band_slot(*b))
            .collect();
        DxccChart {
            bands: bands.iter().map(|b| b.label().to_string()).collect(),
            rows: crate::dxcc::dxcc_entity_names()
                .into_iter()
                .map(|e| DxccChartRow {
                    entity: e.to_string(),
                    any: self.chart_any.get(e).copied().unwrap_or(0),
                    cells: bands
                        .iter()
                        .map(|b| self.chart.get(&(e, *b)).copied().unwrap_or(0))
                        .collect(),
                })
                .collect(),
        }
    }

    /// The What-to-QSL list: each kept contact with the slots it would fill that are still not
    /// confirmed (`confirmed` answers by the award fold's final state), ranked by the most
    /// valuable slot it fills, then by how many it fills, then newest first.
    pub(crate) fn wants(&self, confirmed: impl Fn(&SlotView<'_>) -> bool) -> Vec<QslWant> {
        let mut by_contact: HashMap<u32, Vec<QslSlot>> = HashMap::new();
        for (slot, contacts) in &self.wants {
            let still_needed = match slot {
                Slot::Wpx(p) => !self.wpx_confirmed.contains(p),
                Slot::Wae(i) => !self.wae_confirmed.contains(i),
                Slot::Entity(e) => !confirmed(&SlotView::Entity(e)),
                Slot::EntityBand(e, b) => !confirmed(&SlotView::EntityBand(e, *b)),
                Slot::Zone(z) => !confirmed(&SlotView::Zone(*z)),
                Slot::State(s) => !confirmed(&SlotView::State(s)),
                Slot::Province(p) => !confirmed(&SlotView::Province(p)),
                Slot::Grid(g, b) => !confirmed(&SlotView::Grid(g, b)),
            };
            if !still_needed {
                continue;
            }
            for n in contacts {
                by_contact.entry(*n).or_default().push(slot.describe());
            }
        }
        let mut out: Vec<QslWant> = by_contact
            .into_iter()
            .filter_map(|(n, mut slots)| {
                let k = self.kept.get(&n)?;
                slots.sort_by(|a, b| a.award.cmp(&b.award).then_with(|| a.item.cmp(&b.item)));
                Some(QslWant {
                    call: k.call.clone(),
                    when_unix: k.when_unix,
                    band: k.band.clone(),
                    mode: k.mode.clone(),
                    qsl_via: k.qsl_via.clone(),
                    slots,
                })
            })
            .collect();
        out.sort_by(|a, b| {
            a.slots[0]
                .award
                .cmp(&b.slots[0].award)
                .then_with(|| b.slots.len().cmp(&a.slots.len()))
                .then_with(|| b.when_unix.cmp(&a.when_unix))
                .then_with(|| a.call.cmp(&b.call))
        });
        out
    }
}

/// A slot whose confirmed state the award fold owns, as [`MoreAwards::wants`] asks about it.
pub(crate) enum SlotView<'a> {
    Entity(&'a str),
    EntityBand(&'a str, Band),
    Zone(u8),
    State(&'a str),
    Province(&'a str),
    Grid(&'a str, &'a str),
}

/// Per-band distinct counts, low band first, bands with a contact only.
fn per_band(
    worked: &HashSet<(String, Band)>,
    confirmed: &HashSet<(String, Band)>,
) -> Vec<BandAward> {
    Band::ALL
        .into_iter()
        .filter_map(|b| {
            let w = worked.iter().filter(|(_, x)| *x == b).count();
            (w > 0).then(|| BandAward {
                band: b.label().to_string(),
                worked: w,
                confirmed: confirmed.iter().filter(|(_, x)| *x == b).count(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::awards::{Awards, ContactExtras, RecordFields};

    /// One contact into a fold: call, band, mode, confirmed, US state, when.
    fn log(a: &mut Awards, call: &str, band: &str, mode: &str, confirmed: bool, when: u64) {
        contact(
            a,
            call,
            band,
            mode,
            confirmed,
            None,
            when,
            ContactExtras::default(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn contact(
        a: &mut Awards,
        call: &str,
        band: &str,
        mode: &str,
        confirmed: bool,
        state: Option<&str>,
        when: u64,
        extras: ContactExtras<'_>,
    ) {
        a.add_contact(
            call,
            band,
            mode,
            confirmed,
            false,
            false,
            state,
            None,
            None,
            false,
            RecordFields {
                when_unix: when,
                ..RecordFields::default()
            },
            extras,
        );
    }

    /// WPX counts prefixes by CQ's rule, overall and per band, confirmed by LoTW or a card.
    #[test]
    fn wpx_counts_prefixes_worked_and_confirmed_per_band() {
        let mut a = Awards::new();
        log(&mut a, "N8BJQ", "20m", "CW", true, 1);
        log(&mut a, "W8AA", "40m", "SSB", false, 2);
        log(&mut a, "N8XYZ", "40m", "FT8", false, 3);
        log(&mut a, "PA/N8BJQ", "40m", "FT8", false, 4);
        let w = a.summary().wpx;
        assert_eq!((w.worked, w.confirmed), (3, 1), "N8, W8, PA0; N8 confirmed");
        assert_eq!(w.unconfirmed, vec!["PA0", "W8"]);
        let band = |b: &str| {
            w.bands
                .iter()
                .find(|x| x.band == b)
                .map(|x| (x.worked, x.confirmed))
        };
        assert_eq!(band("20m"), Some((1, 1)));
        assert_eq!(band("40m"), Some((3, 0)));
        assert_eq!(band("15m"), None);
    }

    /// A Needed-board row for `call` heard on `band` in mode class `mode`, needed as a new band.
    fn row(call: &str, band: &str, mode: &str) -> NeedAlert {
        NeedAlert {
            call: call.into(),
            entity: "Test".into(),
            band: band.into(),
            zone: 5,
            tags: vec![crate::needalert::NeedTag::NewBand],
            priority: 50,
            headline: format!("New band — {call}"),
            mode: mode.into(),
            exact_mode: None,
            freq_mhz: Some(14.025),
            admitted_at: Some(1),
            evidence: None,
            grid_rarity: None,
            grid: None,
            park: None,
            state_from: None,
            wpx_need: None,
            wae_need: None,
        }
    }

    /// The log both chip tests read: N8 on 20 m CW, PA0 (worked portable) on 20 m CW, and
    /// Germany on 20 m Phone.
    fn chip_log() -> WpxWaeWorked {
        let mut a = Awards::new();
        log(&mut a, "N8BJQ", "20m", "CW", false, 1);
        log(&mut a, "PA/N8BJQ", "20m", "CW", false, 2);
        log(&mut a, "DL1ABC", "20m", "SSB", true, 3);
        a.wpx_wae_worked()
    }

    /// A row as the chip tests read it: call, band, mode, its WPX chip and its WAE chip.
    type Chips<'a> = (&'a str, &'a str, &'a str, Option<&'a str>, Option<&'a str>);

    fn chips(rows: &[NeedAlert]) -> Vec<Chips<'_>> {
        rows.iter()
            .map(|r| {
                (
                    r.call.as_str(),
                    r.band.as_str(),
                    r.mode.as_str(),
                    r.wpx_need.as_deref(),
                    r.wae_need.as_deref(),
                )
            })
            .collect()
    }

    fn chip_rows() -> Vec<NeedAlert> {
        vec![
            row("N8XYZ", "20m", "CW"),
            row("N8XYZ", "20m", "Phone"),
            row("N8XYZ", "40m", "CW"),
            row("W8AA", "20m", "CW"),
            row("DL2XX", "20m", "Phone"),
            row("DL2XX", "20m", "CW"),
            row("DL2XX", "20m", "Digital"),
            row("IT9ABC", "20m", "Phone"),
            row("W1AW", "20m", "Phone"),
        ]
    }

    /// Each chip lights only when its own setting is on, and only on a row whose station would
    /// be new on that row's band in its mode class: a prefix or country worked on another band
    /// or in another mode is still new here.
    #[test]
    fn each_award_chip_marks_only_new_slots_and_only_when_its_setting_is_on() {
        let worked = chip_log();

        let mut wpx_only = chip_rows();
        mark_award_needs(&mut wpx_only, &worked, true, false);
        assert_eq!(
            chips(&wpx_only),
            vec![
                ("N8XYZ", "20m", "CW", None, None),
                ("N8XYZ", "20m", "Phone", Some("N8"), None),
                ("N8XYZ", "40m", "CW", Some("N8"), None),
                ("W8AA", "20m", "CW", Some("W8"), None),
                ("DL2XX", "20m", "Phone", Some("DL2"), None),
                ("DL2XX", "20m", "CW", Some("DL2"), None),
                ("DL2XX", "20m", "Digital", Some("DL2"), None),
                ("IT9ABC", "20m", "Phone", Some("IT9"), None),
                ("W1AW", "20m", "Phone", Some("W1"), None),
            ]
        );

        let mut wae_only = chip_rows();
        mark_award_needs(&mut wae_only, &worked, false, true);
        assert_eq!(
            chips(&wae_only),
            vec![
                ("N8XYZ", "20m", "CW", None, None),
                ("N8XYZ", "20m", "Phone", None, None),
                ("N8XYZ", "40m", "CW", None, None),
                ("W8AA", "20m", "CW", None, None),
                ("DL2XX", "20m", "Phone", None, None),
                ("DL2XX", "20m", "CW", None, Some("DL")),
                ("DL2XX", "20m", "Digital", None, Some("DL")),
                // Sicily is its own WAE country, so Germany worked does not cover it.
                ("IT9ABC", "20m", "Phone", None, Some("IT")),
                ("W1AW", "20m", "Phone", None, None),
            ]
        );

        let mut both = chip_rows();
        mark_award_needs(&mut both, &worked, true, true);
        assert_eq!(
            both[5].wpx_need.as_deref().zip(both[5].wae_need.as_deref()),
            Some(("DL2", "DL"))
        );
    }

    /// With both settings off the board is exactly what it was: every row equal, and its wire
    /// form byte for byte the same, with no chip key in it. Turning a setting on (the positive
    /// control) does change the same rows, so the equality is not a no-op's.
    #[test]
    fn with_both_award_chips_off_no_row_changes() {
        let worked = chip_log();
        let before = chip_rows();
        let wire = serde_json::to_string(&before).unwrap();
        assert!(!wire.contains("wpxNeed") && !wire.contains("waeNeed"));

        let mut off = chip_rows();
        mark_award_needs(&mut off, &worked, false, false);
        assert_eq!(off, before);
        assert_eq!(serde_json::to_string(&off).unwrap(), wire);

        let mut on = chip_rows();
        mark_award_needs(&mut on, &worked, true, true);
        let on_wire = serde_json::to_string(&on).unwrap();
        assert_ne!(on_wire, wire, "a setting on changes the board");
        assert!(on_wire.contains("\"wpxNeed\":\"W8\"") && on_wire.contains("\"waeNeed\":\"DL\""));
        // The chips add a key and touch nothing else on the row.
        for (a, b) in on.iter().zip(&before) {
            let mut stripped = a.clone();
            stripped.wpx_need = None;
            stripped.wae_need = None;
            assert_eq!(&stripped, b);
        }
    }

    /// The prefix a chip names is the prefix CQ's rule gives the call ([`crate::wpx::wpx_prefix`]),
    /// the one the WPX award card counts: a portable designator becomes the prefix, one without
    /// a digit takes a zero, a bare-digit one replaces the call's numeral, and a mobile or
    /// license-class suffix is not a prefix. A call the log worked portable as PA0 leaves PA0
    /// worked, so another PA-portable station is not new.
    #[test]
    fn the_chip_prefix_is_the_wpx_rule_prefix() {
        let worked = chip_log();
        let calls = [
            "PA/K1ABC",
            "N8BJQ/4",
            "K1ABC/M",
            "XEFTJW",
            "LY1000X",
            "WD8ABC",
            "K1ABC/KH9",
        ];
        let mut rows: Vec<NeedAlert> = calls.iter().map(|c| row(c, "20m", "CW")).collect();
        mark_award_needs(&mut rows, &worked, true, false);
        for r in &rows {
            let rule = crate::wpx::wpx_prefix(&r.call);
            let expected = rule.filter(|p| p != "PA0" && p != "N8");
            assert_eq!(r.wpx_need, expected, "{}", r.call);
        }
        assert_eq!(
            chips(&rows).iter().map(|c| (c.0, c.3)).collect::<Vec<_>>(),
            vec![
                ("PA/K1ABC", None),
                ("N8BJQ/4", Some("N4")),
                ("K1ABC/M", Some("K1")),
                ("XEFTJW", Some("XE0")),
                ("LY1000X", Some("LY1000")),
                ("WD8ABC", Some("WD8")),
                ("K1ABC/KH9", Some("KH9")),
            ]
        );
    }

    /// WAE: a WAE-only country counts as itself, a country's band points stop at 5, and a
    /// class needs its countries AND its points.
    #[test]
    fn wae_counts_countries_and_capped_band_points() {
        let mut a = Awards::new();
        log(&mut a, "IT9ABC", "20m", "CW", true, 1);
        for band in ["160m", "80m", "40m", "20m", "15m", "10m"] {
            log(&mut a, "I2ABC", band, "CW", true, 2);
        }
        log(&mut a, "DL1ABC", "40m", "SSB", false, 3);
        log(&mut a, "W1AW", "20m", "SSB", true, 4);
        let w = a.summary().wae;
        assert_eq!((w.worked, w.confirmed, w.total), (3, 2, 73));
        assert_eq!(
            w.points,
            1 + 5,
            "Sicily one band, Italy six bands capped at five"
        );
        assert_eq!(w.points_worked, 7);
        assert_eq!(
            w.unconfirmed
                .iter()
                .map(|c| c.prefix.as_str())
                .collect::<Vec<_>>(),
            ["DL"]
        );
        assert_eq!(w.unworked.len(), 70);
        assert_eq!(w.class, None);
        assert!(!w.trophy);

        // 40 countries on three bands each: 40 countries and 120 points is WAE III.
        let mut b = Awards::new();
        let calls = [
            "1A0KM", "3A2ABC", "4O3A", "9A1A", "9H1A", "C31A", "CT1A", "CU2A", "DL1A", "E71A",
            "EA1A", "EA6A", "EI1A", "ER1A", "ES1A", "EU1A", "F1A", "G3A", "GD4A", "GI4A", "GJ4A",
            "GM4A", "GU4A", "GW4A", "HA1A", "HB9A", "HB0A", "HV1A", "I1A", "IS0A", "IT9A", "JW1A",
            "JX1A", "LA1A", "LX1A", "LY1A", "LZ1A", "OE1A", "OH1A", "OH0A",
        ];
        for call in calls {
            for band in ["40m", "20m", "15m"] {
                log(&mut b, call, band, "CW", true, 1);
            }
        }
        let w = b.summary().wae;
        assert_eq!((w.confirmed, w.points), (40, 120), "{:?}", w.unworked);
        assert_eq!(w.class.as_deref(), Some("III"));
    }

    /// WWFF counts the valid references logged, their DX entities and programmes; a malformed
    /// reference counts for nothing.
    #[test]
    fn wwff_counts_logged_references() {
        let mut a = Awards::new();
        for (call, r) in [
            ("W1AW", "KFF-0001"),
            ("K1ABC", "kff-0001"),
            ("W2ABC", "KFF-0002"),
            ("DL1ABC", "DLFF-0001"),
            ("G3ABC", "not a ref"),
        ] {
            let extras = ContactExtras {
                wwff: Some(r),
                ..ContactExtras::default()
            };
            contact(&mut a, call, "20m", "SSB", false, None, 1, extras);
        }
        let w = a.summary().wwff;
        assert_eq!((w.worked, w.entities, w.programmes), (3, 2, 2));
        assert_eq!((w.level, w.next_level), (None, 10));
    }

    /// The chart's cells are the DXCC slots by mode class: a 60 m contact earns none, a 70 cm one
    /// marks the entity but no band column, a satellite contact neither.
    #[test]
    fn chart_cells_hold_worked_and_confirmed_by_mode() {
        let mut a = Awards::new();
        log(&mut a, "JA1ABC", "20m", "CW", true, 1);
        log(&mut a, "JA1XYZ", "20m", "FT8", false, 2);
        log(&mut a, "JA2ABC", "60m", "SSB", true, 3);
        log(&mut a, "JA3ABC", "70cm", "SSB", false, 4);
        a.add_qso(
            "JA4ABC",
            "15m",
            "SSB",
            true,
            false,
            false,
            None,
            None,
            None,
            true,
            RecordFields::default(),
        );
        let chart = a.dxcc_chart();
        assert!(!chart.bands.contains(&"60m".to_string()));
        assert!(!chart.bands.contains(&"70cm".to_string()));
        assert_eq!(chart.bands.first().map(String::as_str), Some("160m"));
        let row = chart.rows.iter().find(|r| r.entity == "Japan").unwrap();
        let col = |b: &str| chart.bands.iter().position(|x| x == b).unwrap();
        let cw_w = chart_worked(ModeClass::Cw);
        let cw_c = chart_confirmed(ModeClass::Cw);
        let dig_w = chart_worked(ModeClass::Digital);
        let ph_w = chart_worked(ModeClass::Phone);
        assert_eq!(row.cells[col("20m")], cw_w | cw_c | dig_w);
        assert_eq!(
            row.cells[col("15m")],
            0,
            "a satellite contact fills no band"
        );
        assert_eq!(
            row.any,
            cw_w | cw_c | dig_w | ph_w,
            "70 cm phone marks the entity"
        );
        assert_eq!(chart.rows.len(), crate::dxcc::dxcc_entity_names().len());
        assert!(chart
            .rows
            .iter()
            .find(|r| r.entity == "Fed. Rep. of Germany")
            .unwrap()
            .cells
            .iter()
            .all(|c| *c == 0));
    }

    /// What to QSL: only slots still unconfirmed at the end, the most valuable first, the newest
    /// few contacts of each slot, and the QSL route carried through.
    #[test]
    fn what_to_qsl_ranks_by_award_need() {
        let mut a = Awards::new();
        contact(
            &mut a,
            "W1AW",
            "20m",
            "CW",
            true,
            Some("CT"),
            1,
            ContactExtras::default(),
        );
        contact(
            &mut a,
            "K1ABC",
            "20m",
            "CW",
            false,
            Some("MA"),
            2,
            ContactExtras::default(),
        );
        log(&mut a, "DL1ABC", "20m", "CW", true, 3);
        log(&mut a, "DL1XYZ", "40m", "CW", false, 4);
        let via = ContactExtras {
            qsl_via: Some(" JA1QSL "),
            ..ContactExtras::default()
        };
        contact(&mut a, "JA1ABC", "20m", "CW", false, None, 5, via);
        // Unconfirmed first, confirmed later: the slot is filled, so no QSL is wanted for it.
        log(&mut a, "VK2ABC", "20m", "CW", false, 6);
        log(&mut a, "VK2XYZ", "20m", "CW", true, 7);
        // Four contacts for one WPX-only slot: the three newest are kept.
        for (i, call) in ["N2AAA", "N2AAB", "N2AAC", "N2AAD"].into_iter().enumerate() {
            log(&mut a, call, "20m", "CW", false, 100 - i as u64);
        }
        let wants = a.what_to_qsl();
        let calls: Vec<&str> = wants.iter().map(|w| w.call.as_str()).collect();
        assert_eq!(
            calls,
            ["JA1ABC", "DL1XYZ", "K1ABC", "N2AAA", "N2AAB", "N2AAC"]
        );
        let ja = &wants[0];
        assert_eq!(ja.qsl_via.as_deref(), Some("JA1QSL"));
        let kinds: Vec<QslFor> = ja.slots.iter().map(|s| s.award).collect();
        assert_eq!(
            kinds,
            [
                QslFor::DxccEntity,
                QslFor::DxccBand,
                QslFor::Waz,
                QslFor::Wpx
            ]
        );
        assert_eq!(ja.slots[1].item, "Japan 20m");
        assert_eq!(wants[1].slots[0].award, QslFor::DxccBand);
        assert_eq!(
            wants[1].slots.len(),
            1,
            "Germany, zone 14, DL and DL1 are confirmed"
        );
        let k1: Vec<QslFor> = wants[2].slots.iter().map(|s| s.award).collect();
        assert_eq!(k1, [QslFor::Was, QslFor::Wpx]);
        assert!(!calls.contains(&"VK2ABC"));
    }

    /// The fold, the chart and What-to-QSL over a 150,000-contact log — a measurement, run by
    /// hand in release (`cargo test --release -p propagation -- --ignored fold_at_150k`).
    #[test]
    #[ignore]
    fn fold_at_150k() {
        let prefixes = [
            "W1", "K2", "JA1", "DL1", "G3", "VK2", "PY1", "ZS6", "UA3", "LU1", "VE3", "IT9",
        ];
        let bands = [
            "160m", "80m", "40m", "30m", "20m", "17m", "15m", "12m", "10m", "6m",
        ];
        let modes = ["CW", "SSB", "FT8"];
        let calls: Vec<String> = (0..150_000u32)
            .map(|i| {
                let suffix: String = (0..3)
                    .map(|k| (b'A' + ((i >> (5 * k)) % 26) as u8) as char)
                    .collect();
                format!("{}{suffix}", prefixes[i as usize % prefixes.len()])
            })
            .collect();
        let t = std::time::Instant::now();
        let mut a = Awards::new();
        for (i, call) in calls.iter().enumerate() {
            let extras = ContactExtras {
                wwff: (i % 50 == 0).then_some("KFF-0001"),
                qsl_via: (i % 7 == 0).then_some("BUREAU"),
                cnty: None,
            };
            contact(
                &mut a,
                call,
                bands[i % bands.len()],
                modes[i % 3],
                i % 3 == 0,
                None,
                i as u64,
                extras,
            );
        }
        let fold = t.elapsed();
        let s = a.summary();
        let chart = a.dxcc_chart();
        let wants = a.what_to_qsl();
        let total = t.elapsed();
        eprintln!(
            "150k: fold {fold:?}, fold+summary+chart+wants {total:?}; {} entities, {} rows, {} wants",
            s.dxcc_worked,
            chart.rows.len(),
            wants.len()
        );
        assert_eq!(s.qsos, 150_000);
    }
}
