//! Which heard stations are worth interrupting the operator for, decided once, at the station.
//!
//! The desktop screen decides this today, in `ui/src/alerts.ts` (`processDecodes`) and
//! `ui/src/features/potaAlert.ts` (`processPotaAlert`): someone calling you, a new DXCC entity, a
//! new grid (a rare one loudly), a watch-list station, an opted-in CQ, and a park newly on the air.
//! A computer driving the station and a phone must hear the same alerts, so the decision belongs
//! here, beside the decodes it reads: [`NeededAlerts::decide`] and [`FreshParks::tick`] are the
//! screen's two former functions in Rust, rule for rule, with the same dedups, band scopes,
//! watch-list tier and partner rules. Each alert carries what its toast says and what its button
//! works, and [`Presentation`] how it sounds and shows, so a screen or a push renders it without
//! deciding anything.
//!
//! The station raises them (`Engine::needed_alerts`, on each look at the decode window, and the
//! parks on the OTA poll), as the `alert` event every window and a computer driving the station
//! hear; the screen only shows them. The decision reads the watch list the desktop sends, its gates
//! with it (`crate::watchlist`), and the geographic scope resolved to entity names
//! ([`scoped_entities`]). Both ends are held to one table of cases,
//! `tests/fixtures/needed-alerts.json`, captured from the screen's decision before it moved here:
//! this side decides it, the desktop's `alerts.agreement.test.ts` shows it.
//!
//! Left on the screen by design: the decode tick and the spoken batch summary (eyes-free cues for
//! the window that hears them, not alerts about a station) and the toast's click-to-work itself.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::dto::{DecodeRow, GridRarity, Tier};
use crate::settings::Settings;
use crate::watchlist::{watched, WatchEntry, WatchKind};

/// How long a station that called you stays announced: its answer, its report and its 73 are one
/// event, and the same station is news again after this.
const MY_CALL_REPEAT_MS: i64 = 10 * 60_000;
/// Decode keys remembered for freshness before the memory starts over (one repeated tick at most).
const SEEN_MAX: usize = 5000;
/// The repeating kinds' dedup cap, and how many of the oldest go when it is passed.
const REPEAT_CAP: usize = 2000;
const REPEAT_DROP: usize = REPEAT_CAP / 5;

/// What an alert is about, loudest tier first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NeededKind {
    /// A station the operator's watch list names.
    Watch,
    /// An entity never worked on any band.
    NewDxcc,
    /// A grid never worked, rare or water-only, where the rare scope admits the band.
    RareGrid,
    /// A grid never worked.
    NewGrid,
    /// A new station calling the operator.
    MyCall,
    /// A plain CQ (opt-in).
    Cq,
}

/// One alert: the parts of its toast. `call` is `None` for a station not parsed (the toast says
/// "a station"); `entity` only where the toast names one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeededAlert {
    pub kind: NeededKind,
    pub call: Option<String>,
    pub entity: Option<String>,
    /// A rare grid's square, as the decode carried it.
    pub grid: Option<String>,
    /// A rare grid's tier: `Rare` or `UltraRare`.
    pub tier: Option<GridRarity>,
    /// The watch entry's label, for a watch-list alert.
    pub watch: Option<String>,
    /// What the toast's button works: the decode, as double-clicking its row hands it over.
    /// `None` for a station not parsed, whose toast has no button.
    pub work: Option<Work>,
}

/// The decode an alert's button works, beside the alert's `call`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Work {
    pub message: String,
    pub snr: i32,
    pub freq_hz: f32,
    pub tier: Tier,
}

impl Work {
    fn of(d: &DecodeRow) -> Option<Self> {
        d.from.as_ref().map(|_| Self {
            message: d.message.clone(),
            snr: d.snr,
            freq_hz: d.freq_hz,
            tier: d.tier,
        })
    }
}

/// How an alert of one kind sounds and shows on the desktop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Presentation {
    /// Its tone in Hz, and whether it is the double beep (a second tone at 1.5× it, 130 ms on).
    pub tone: Option<(u32, bool)>,
    /// The toast's tone: `success` (loud) or `info` (quiet).
    pub toast: &'static str,
    pub ms: u32,
    pub prominent: bool,
    /// The toast's button: `work` the station, or `answer` it.
    pub action: &'static str,
}

impl NeededKind {
    pub fn presentation(self) -> Presentation {
        let (tone, toast, ms, prominent, action) = match self {
            NeededKind::Watch | NeededKind::NewDxcc => {
                (Some((520, true)), "success", 15000, true, "work")
            }
            NeededKind::RareGrid => (Some((740, true)), "success", 15000, true, "work"),
            NeededKind::NewGrid => (None, "info", 6000, false, "work"),
            NeededKind::MyCall => (Some((880, false)), "success", 20000, true, "answer"),
            NeededKind::Cq => (Some((620, false)), "info", 6000, false, "answer"),
        };
        Presentation {
            tone,
            toast,
            ms,
            prominent,
            action,
        }
    }
}

/// A new park's double beep, once per poll that finds one.
pub const NEW_PARK_TONE: u32 = 880;

/// The alert switches and band scopes, as the settings hold them (`None` = the setting's default:
/// every band for a new DXCC, VHF and up for both grid kinds).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Switches {
    pub alert_my_call: bool,
    pub alert_new: bool,
    pub alert_cq: bool,
    pub alert_dxcc_bands: Option<String>,
    pub alert_grid_bands: Option<String>,
    pub alert_rare_grid_bands: Option<String>,
}

impl Switches {
    pub fn of(settings: &Settings) -> Self {
        Self {
            alert_my_call: settings.alert_my_call,
            alert_new: settings.alert_new,
            alert_cq: settings.alert_cq,
            alert_dxcc_bands: Some(settings.alert_dxcc_bands.clone()),
            alert_grid_bands: Some(settings.alert_grid_bands.clone()),
            alert_rare_grid_bands: Some(settings.alert_rare_grid_bands.clone()),
        }
    }
}

/// The sequencer's state and the station being worked, as the screen reads them (Field Day's
/// before the QSO's).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct QsoContext {
    pub state: Option<String>,
    pub dxcall: Option<String>,
}

/// The desktop's `watchLabel`: the operator's label for the entry, else the entry itself.
pub fn watch_label(entry: &WatchEntry) -> String {
    match entry.label.as_deref().map(str::trim) {
        Some(label) if !label.is_empty() => label.to_string(),
        _ => match entry.kind {
            WatchKind::Dxcc => entry.value.clone(),
            WatchKind::Grid => format!("grid {}", entry.value.to_uppercase()),
            WatchKind::Call => entry.value.to_uppercase(),
        },
    }
}

/// Whether an entry's alert gates (CQ only, a minimum SNR) let this decode through.
fn watch_admits(entry: &WatchEntry, d: &DecodeRow) -> bool {
    (!entry.cq_only || d.is_cq) && entry.min_snr.is_none_or(|min| f64::from(d.snr) >= min)
}

/// cty.dat's six continent codes, the only ones a scope honours: a code nothing offers a box for
/// would filter rows with nothing on screen to account for them.
const CONTINENT_CODES: [&str; 6] = ["NA", "SA", "EU", "AF", "AS", "OC"];

/// The operator's geographic scope (`alertContinents`, `alertEntities`) as the entity NAMES it
/// admits, the decode rows' own vocabulary (the desktop's former `scopedEntities`), with `table`
/// the country file's entity → continent pairs. The two halves union: a decode has one entity, so
/// "Europe" and "Japan" both ticked must admit both. Every ambiguity resolves toward alerting: an
/// unknown continent code is dropped, and a scope that admits no entity at all is no scope
/// (`None`), because a filter that fails closed silences the radio with nothing on screen to say
/// why.
pub fn scoped_entities<'a>(
    continents: &[String],
    entities: &[String],
    table: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<HashSet<String>> {
    let codes: Vec<&str> = continents
        .iter()
        .map(String::as_str)
        .filter(|c| CONTINENT_CODES.contains(c))
        .collect();
    let mut out: HashSet<String> = entities.iter().filter(|e| !e.is_empty()).cloned().collect();
    if codes.is_empty() && out.is_empty() {
        return None;
    }
    if !codes.is_empty() {
        out.extend(
            table
                .into_iter()
                .filter(|(_, cont)| codes.contains(cont))
                .map(|(entity, _)| entity.to_string()),
        );
    }
    (!out.is_empty()).then_some(out)
}

/// What a look at the decode window is decided under.
pub struct Scene<'a> {
    pub switches: &'a Switches,
    pub qso: &'a QsoContext,
    pub watch: &'a [WatchEntry],
    /// The dial in MHz; `None` when not known (every scope but `off` then admits).
    pub dial_mhz: Option<f64>,
    /// The entities the operator narrowed the new ones to; `None` for no scope.
    pub geo_scope: Option<&'a HashSet<String>>,
    pub now_ms: i64,
}

/// Whether a band scope (`off`, `hf`, `vhf`, `all`) admits the dial. A dial not known admits every
/// scope but `off`: dropping alerts on a band not verified would hide real ones.
fn band_scope_ok(scope: Option<&str>, dial_mhz: Option<f64>, fallback: &str) -> bool {
    match scope.unwrap_or(fallback) {
        "off" => false,
        "all" => true,
        _ if !dial_mhz.is_some_and(|dial| dial > 0.0) => true,
        "vhf" => dial_mhz.is_some_and(|dial| dial >= 50.0),
        _ => dial_mhz.is_some_and(|dial| dial < 50.0),
    }
}

/// A decode's identity for freshness: who, what, and the offset rounded as JavaScript rounds.
fn decode_key(d: &DecodeRow) -> String {
    let hz = (f64::from(d.freq_hz) + 0.5).floor() as i64;
    format!("{}|{}|{hz}", d.from.as_deref().unwrap_or("?"), d.message)
}

/// A decode's identity for the repeating kinds' dedup: who said what. The offset drifts a few Hz
/// a transmission, so it is a measurement, never identity.
fn alert_identity(d: &DecodeRow) -> String {
    format!("{}|{}", d.from.as_deref().unwrap_or("?"), d.message)
}

fn non_empty(text: &Option<String>) -> Option<String> {
    text.as_ref().filter(|t| !t.is_empty()).cloned()
}

/// The decision's memory across looks at the decode window: one per station.
#[derive(Default)]
pub struct NeededAlerts {
    /// Every decode key ever seen: a decode first seen in a look is fresh in it.
    seen: HashSet<String>,
    /// The once-ever dedups (a new DXCC, a new grid, a watch hit): never evicted.
    once: HashSet<String>,
    /// The repeating kinds' dedups (calling you, CQ), oldest first, bounded.
    repeat: VecDeque<String>,
    repeat_set: HashSet<String>,
    /// When each station calling you was last announced.
    my_call_at: HashMap<String, i64>,
}

impl NeededAlerts {
    pub fn new() -> Self {
        Self::default()
    }

    /// The alerts one look at the decode window raises, in order. The window repeats rows from
    /// earlier looks; only a row arriving now may claim "is calling you", and every kind is
    /// raised at most once for what it is about.
    pub fn decide(&mut self, decodes: &[DecodeRow], scene: &Scene) -> Vec<NeededAlert> {
        let switches = scene.switches;
        let partner = scene
            .qso
            .dxcall
            .as_ref()
            .map(|call| call.to_uppercase())
            .filter(|call| !call.is_empty());

        if self.seen.len() > SEEN_MAX {
            self.seen.clear();
        }
        let mut fresh = HashSet::new();
        for d in decodes.iter().filter(|d| !d.mine) {
            let key = decode_key(d);
            if self.seen.insert(key.clone()) {
                fresh.insert(key);
            }
        }

        let dxcc_ok = band_scope_ok(switches.alert_dxcc_bands.as_deref(), scene.dial_mhz, "all");
        let grid_ok = band_scope_ok(switches.alert_grid_bands.as_deref(), scene.dial_mhz, "vhf");
        let rare_ok = band_scope_ok(
            switches.alert_rare_grid_bands.as_deref(),
            scene.dial_mhz,
            "vhf",
        );
        let state = scene.qso.state.as_deref();
        // They answered our CQ, or nothing is in progress: the partner may still be calling us.
        let i_am_initiator = matches!(state, None | Some("Listening" | "CallingCq" | "AwaitRoger"));

        let mut alerts = Vec::new();
        for d in decodes {
            let call = d.from.as_deref();
            let is_partner =
                partner.is_some() && call.map(str::to_uppercase).as_deref() == partner.as_deref();
            let station = call.map(str::to_uppercase).unwrap_or_default();
            let recently = self
                .my_call_at
                .get(&station)
                .is_some_and(|at| *at > scene.now_ms - MY_CALL_REPEAT_MS);
            // Decided before the partner skip: the decode that answers your CQ arrives with its
            // station already the partner.
            let calling_me = switches.alert_my_call
                && d.directed_to_me
                && !d.signoff
                && fresh.contains(&decode_key(d))
                && !recently
                && (!is_partner || i_am_initiator);
            if calling_me {
                self.my_call_at.insert(station, scene.now_ms);
            }
            // Nothing else about the station being worked is news; no key is spent.
            if is_partner && !calling_me {
                continue;
            }

            // The watch list first, above the band scopes, but never for the station being worked.
            if !is_partner {
                let hit = call.filter(|c| !c.is_empty()).and_then(|c| {
                    scene.watch.iter().find(|w| {
                        watch_admits(w, d)
                            && watched(
                                std::slice::from_ref(*w),
                                c,
                                d.country.as_deref(),
                                d.grid.as_deref(),
                            )
                            .is_some()
                    })
                });
                if let Some(hit) = hit {
                    let key = format!("watch:{}:{}", hit.id, call.unwrap_or("?"));
                    if self.once.insert(key) {
                        alerts.push(NeededAlert {
                            kind: NeededKind::Watch,
                            call: d.from.clone(),
                            entity: non_empty(&d.country),
                            grid: None,
                            tier: None,
                            watch: Some(watch_label(hit)),
                            work: Work::of(d),
                        });
                    }
                    continue;
                }
            }

            let is_rare = matches!(
                d.grid_rarity,
                Some(GridRarity::Rare | GridRarity::UltraRare)
            );
            let kind = if calling_me {
                NeededKind::MyCall
            } else if switches.alert_new && d.new_dxcc && dxcc_ok {
                NeededKind::NewDxcc
            } else if switches.alert_new
                && d.new_grid
                && if is_rare { rare_ok || grid_ok } else { grid_ok }
            {
                NeededKind::NewGrid
            } else if switches.alert_cq && d.is_cq {
                NeededKind::Cq
            } else {
                continue;
            };

            // The geographic scope, before the dedup so a refused decode spends no key. A station
            // calling you outranks it, and an entity not placed is admitted.
            if kind != NeededKind::MyCall {
                if let (Some(scope), Some(entity)) = (
                    scene.geo_scope,
                    d.country.as_deref().filter(|e| !e.is_empty()),
                ) {
                    if !scope.contains(entity) {
                        continue;
                    }
                }
            }

            let rare_grid = kind == NeededKind::NewGrid && is_rare && rare_ok;
            let grid = non_empty(&d.grid);
            let fallback = || d.from.as_deref().unwrap_or("?");
            let (key, once) = match kind {
                NeededKind::NewDxcc => (
                    format!("dxcc:{}", d.country.as_deref().unwrap_or_else(fallback)),
                    true,
                ),
                NeededKind::NewGrid => match &grid {
                    Some(grid) if rare_grid => (format!("rgrid:{}", grid.to_uppercase()), true),
                    _ => (format!("grid:{}", fallback()), true),
                },
                NeededKind::MyCall => (format!("mycall:{}", alert_identity(d)), false),
                _ => (format!("cq:{}", alert_identity(d)), false),
            };
            let new = if once {
                self.once.insert(key)
            } else if self.repeat_set.insert(key.clone()) {
                self.repeat.push_back(key);
                true
            } else {
                false
            };
            if !new {
                continue;
            }

            let entity = non_empty(&d.country);
            alerts.push(match kind {
                NeededKind::MyCall => NeededAlert {
                    kind,
                    call: d.from.clone(),
                    entity: None,
                    grid: None,
                    tier: None,
                    watch: None,
                    work: Work::of(d),
                },
                NeededKind::NewGrid if rare_grid => NeededAlert {
                    kind: NeededKind::RareGrid,
                    call: d.from.clone(),
                    entity,
                    grid,
                    tier: Some(if d.grid_rarity == Some(GridRarity::UltraRare) {
                        GridRarity::UltraRare
                    } else {
                        GridRarity::Rare
                    }),
                    watch: None,
                    work: Work::of(d),
                },
                _ => NeededAlert {
                    kind,
                    call: d.from.clone(),
                    entity,
                    grid: None,
                    tier: None,
                    watch: None,
                    work: Work::of(d),
                },
            });
        }

        // The repeating dedups stay bounded without a wholesale clear: the oldest go.
        if self.repeat.len() > REPEAT_CAP {
            for key in self.repeat.drain(..REPEAT_DROP) {
                self.repeat_set.remove(&key);
            }
        }
        alerts
    }
}

/// Parks newly spotted on the air, poll to poll. The first poll primes silently: the parks
/// already on the air were not just spotted.
#[derive(Default)]
pub struct FreshParks {
    seen: HashSet<String>,
    primed: bool,
}

impl FreshParks {
    pub fn new() -> Self {
        Self::default()
    }

    /// The references in this poll that the last one did not carry, or none on the first poll. A
    /// park that drops off and comes back is fresh again: that is a new spot.
    pub fn tick<'a>(&mut self, references: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut fresh = Vec::new();
        let mut next = HashSet::new();
        for reference in references {
            if !self.seen.contains(reference) {
                fresh.push(reference.to_string());
            }
            next.insert(reference.to_string());
        }
        self.seen = next;
        if !std::mem::replace(&mut self.primed, true) {
            return Vec::new();
        }
        fresh
    }
}

#[cfg(test)]
mod tests;
