//! Award progress computed from the operator's logbook — DXCC-first.
//!
//! Pure + offline: each worked contact counts toward the DXCC entity its record's own number
//! names (ADIF `DXCC`, LoTW's where it differs — [`RecordFields`]), else the one its call
//! resolves to via the vendored cty.dat ([`crate::dxcc`]); we tally distinct entities
//! worked/confirmed (current, and with the deleted ones), the entity×band "DXCC Challenge"
//! slots, a per-band breakdown (the elite DX tracker's view), and the chase list of
//! worked-but-unconfirmed entities.
//!
//! Online LoTW/eQSL/QRZ/ClubLog sync (which would flip a QSO's `confirmed`) is a
//! separate, later increment; this computes everything from what's already in the
//! log, so it is fully testable with no network.
//!
//! CQ WPX, DARC's WAE, WWFF, the whole-log DXCC chart and What-to-QSL ride the same fold
//! ([`crate::awards_more`]): each is fed exactly the slots this fold credits a contact with.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::achievements::{self, Achievement, AchievementStats};
use crate::awards_more::{
    Contact, Credited, DxccChart, MoreAwards, QslWant, SlotView, WaeProgress, WpxProgress,
    WpxWaeWorked, WwffProgress,
};
use crate::callstats::CallStatsIndex;
use crate::dxcc;
use crate::model::{Band, ModeClass};

/// The ARRL DXCC Challenge bands — exactly 160/80/40/30/20/17/15/12/10/6 m.
///
/// ⚠️ THE THREE BANDS OUTSIDE THIS LIST ARE OUTSIDE IT FOR DIFFERENT REASONS, and an earlier
/// version of this comment gave one rule for all of them, which was wrong for two:
///   * **2 m** — has its OWN DXCC award and counts toward Mixed. Only the CHALLENGE excludes it.
///   * **60 m** — earns NO ARRL DXCC credit whatsoever. Not Mixed, not per-band, not per-mode.
///     See [`earns_dxcc_credit`].
///   * **4 m** — not an ARRL award band at all (no US allocation), so likewise none.
const CHALLENGE_BANDS: &[Band] = &[
    Band::B160,
    Band::B80,
    Band::B40,
    Band::B30,
    Band::B20,
    Band::B17,
    Band::B15,
    Band::B12,
    Band::B10,
    Band::B6,
];

/// Whether a contact on `b` can earn ARRL DXCC credit of ANY kind.
///
/// ⭐ **60 m EARNS NONE — not Mixed, not per-band, not per-mode.** It is a secondary,
/// channelised allocation implemented differently country to country, and ARRL excludes it from
/// DXCC outright. This is not the same as "no 60 m band award": a 60 m contact does not count
/// toward the entity's basic/Mixed total either, so an entity worked ONLY on 60 m is not a DXCC
/// entity at all.
///
/// **4 m** is excluded for a different reason — there is no ARRL 4 m award and no US allocation,
/// so it cannot be credited either. (It IS a real band elsewhere; see the 4 m support added
/// 2026-08-05. This gate is about award credit, not about whether the band exists.)
///
/// ⚠️ SCOPE: this is DXCC only, deliberately. CQ's WAZ, RSGB's IOTA, ARRL's WAS and the VUCC
/// grid awards are separate programmes with their own band rules, and the fold applies this gate
/// AFTER them so none is affected. Do not widen it without checking each programme's own rules.
///
/// Reported by a field user via the DXpedition board (2026-08-05): the code asserted the
/// opposite, so anyone with 60 m contacts saw an inflated DXCC total and could believe an entity
/// was confirmed that ARRL will never credit.
///
/// Shared with the CHASE ranker ([`crate::predict::rank_for_chase`]) and the DXpedition tracker
/// so the award rule has ONE home: the logbook must not credit a 60 m QSO and the board must not
/// *recommend* one, and those are the same fact stated once, not two lists that drift apart.
pub(crate) fn earns_dxcc_credit(b: Band) -> bool {
    !matches!(b, Band::B60 | Band::B4)
}

/// Whether a contact on `b` fills a DXCC BAND slot (the per-band rows, the Challenge and the
/// needed-by-band lists), not merely the entity.
///
/// **70 cm, 33 cm and 23 cm count toward the entity and no band:** they earn Mixed and mode
/// credit like any band, but the ARRL keeps no DXCC award on them, so a slot there would claim
/// a "new band" nobody can apply for. That is exactly how these contacts counted before the
/// bands had a [`Band`] variant, when their label did not parse.
pub(crate) fn fills_dxcc_band_slot(b: Band) -> bool {
    earns_dxcc_credit(b) && !b.is_uhf()
}

/// Whether `b` is one of the DXCC Challenge bands.
pub(crate) fn is_challenge_band(b: Band) -> bool {
    CHALLENGE_BANDS.contains(&b)
}

/// Whether a normalised band label is a VUCC award band (50 MHz and up).
pub(crate) fn is_vucc_band(label: &str) -> bool {
    VUCC_THRESHOLDS.iter().any(|(b, _)| *b == label)
}

/// DXCC mode classes, in display order (separate CW/Phone/Digital DXCC awards).
const MODE_CLASSES: [ModeClass; 3] = [ModeClass::Cw, ModeClass::Phone, ModeClass::Digital];

/// The classic 5-Band DXCC bands. The WORK chase + 5BDXCC metric use these (the
/// bands an "all-band" chaser most wants every entity on).
pub(crate) const AWARD_BANDS: [Band; 5] = [Band::B80, Band::B40, Band::B20, Band::B15, Band::B10];
/// An entity must be worked on at least this many award bands to appear in the
/// WORK chase (so it surfaces "almost-complete" entities, not every partial).
const WORK_CHASE_MIN_BANDS: usize = 3;

/// The 50 US states (ADIF `STATE` postal codes) for Worked All States. WAS keys
/// on these codes directly — Hawaii/Alaska count even though they're separate
/// DXCC entities; DC and territories are not WAS states.
pub(crate) const WAS_STATES: [&str; 50] = [
    "AK", "AL", "AR", "AZ", "CA", "CO", "CT", "DE", "FL", "GA", "HI", "IA", "ID", "IL", "IN", "KS",
    "KY", "LA", "MA", "MD", "ME", "MI", "MN", "MO", "MS", "MT", "NC", "ND", "NE", "NH", "NJ", "NM",
    "NV", "NY", "OH", "OK", "OR", "PA", "RI", "SC", "SD", "TN", "TX", "UT", "VA", "VT", "WA", "WI",
    "WV", "WY",
];

/// The 13 Canadian provinces and territories, as ADIF Primary Administrative Subdivision codes
/// for DXCC entity 1 — the same codes `province::province_for_call` returns and the same ones an
/// ADIF export or a TQSL signing carries, so log and award agree without translation.
pub(crate) const WAP_PROVINCES: [&str; 13] = [
    "AB", "BC", "MB", "NB", "NL", "NS", "NT", "NU", "ON", "PE", "QC", "SK", "YT",
];

/// Canonicalize an ADIF subdivision code to one of the 13 provinces, else `None`.
pub(crate) fn valid_province(s: &str) -> Option<&'static str> {
    let up = s.trim().to_ascii_uppercase();
    WAP_PROVINCES.iter().copied().find(|p| *p == up)
}

/// Canonicalize an ADIF state code to one of the 50 WAS states, or `None` for a
/// junk/territory/empty code (which never advances WAS).
/// Canonical US postal code used by the WAS and descriptive-statistics folds.
pub fn valid_state(s: &str) -> Option<&'static str> {
    let up = s.trim().to_ascii_uppercase();
    WAS_STATES.iter().copied().find(|st| *st == up)
}

/// Representative callsigns for famously-rare, DXpedition-only DXCC entities —
/// resolved through cty.dat so names match the log side exactly (unresolvable
/// ones are simply skipped). Working one is a DXpedition-grade contact. A live
/// DXpedition feed can augment this later.
const MOST_WANTED_CALLS: &[&str] = &[
    "3Y0J",  // Bouvet
    "P5A",   // North Korea (DPRK)
    "BS7H",  // Scarborough Reef
    "FT5WQ", // Crozet Island
    "FT5XT", // Kerguelen
    "KH1A",  // Baker & Howland
    "VK0M",  // Macquarie Island
    "ZL9A",  // NZ Subantarctic
    "VP6D",  // Ducie Island
    "VP8S",  // South Sandwich
    "E30FB", // Eritrea
    "T31",   // Central Kiribati
    "FT5ZM", // Amsterdam & St Paul
    "3C0",   // Annobón
    "9U",    // Burundi
];

/// The set of DXpedition-grade ("most-wanted") DXCC entities, by canonical
/// cty.dat name.
fn most_wanted_entities() -> std::collections::HashSet<&'static str> {
    MOST_WANTED_CALLS
        .iter()
        .filter_map(|c| dxcc::resolve(c).map(|i| i.entity))
        .collect()
}

/// Per-band DXCC entity progress (e.g. "20m — 84 worked, 71 confirmed").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BandAward {
    pub band: String,
    pub worked: usize,
    pub confirmed: usize,
}

/// Per-mode DXCC entity progress — CW / Phone / Digital are separate DXCC awards
/// (each toward 100 confirmed entities).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeAward {
    pub mode: String,
    pub worked: usize,
    pub confirmed: usize,
}

/// A worked-but-unconfirmed DXCC entity — confirming any QSO on the listed bands
/// adds a new DXCC entity (a "new one" chase).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityNeed {
    pub entity: String,
    /// Bands the entity is worked-but-unconfirmed on.
    pub bands: Vec<String>,
}

/// DXCC Honor Roll standing — the elite chaser's headline metric. Honor Roll is
/// **current-entities-only** (deleted entities never count) and uses **confirmed**
/// (award-eligible) entities. ARRL rule: you make Honor Roll in the "numerical top
/// ten", i.e. confirmed ≥ `current_total − 9`; #1 Honor Roll is every current
/// entity confirmed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HonorRollProgress {
    /// Current DXCC entities (the denominator) — derived from cty.dat (non-WAE).
    pub current_total: usize,
    /// Confirmed current DXCC entities (the numerator).
    pub confirmed: usize,
    /// Entry threshold = `current_total − 9` ("numerical top ten").
    pub threshold: usize,
    /// True once `confirmed ≥ threshold`.
    pub achieved: bool,
    /// Confirmed entities still needed to reach Honor Roll entry (0 if achieved).
    pub needed: usize,
    /// True once every current entity is confirmed (#1 Honor Roll).
    pub number_one: bool,
    /// Confirmed entities still needed for #1 Honor Roll (0 if achieved).
    pub number_one_needed: usize,
}

/// A valid 4-char Maidenhead grid field+square ("EN37"), uppercased — or `None`. VUCC
/// counts grid SQUARES, so the subsquare (chars 5-6) is dropped, and a malformed locator
/// (junk in the ADIF GRIDSQUARE field) is rejected so it can't inflate the count. Field
/// letters are A–R, then two digits.
fn valid_grid4(grid: &str) -> Option<String> {
    let g = grid.trim().to_ascii_uppercase();
    let b = g.as_bytes();
    let ok = b.len() >= 4
        && (b'A'..=b'R').contains(&b[0])
        && (b'A'..=b'R').contains(&b[1])
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit();
    ok.then(|| g[..4].to_string())
}

/// Grid-square progress — distinct Maidenhead grid squares worked / confirmed, overall
/// and per band. The overall counts are an ALL-BAND tracker (HF included, for the grid
/// chaser); the actual ARRL VUCC award is 50 MHz-and-up with per-band thresholds and
/// lives in [`VuccProgress::awards`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VuccProgress {
    /// Distinct grid squares worked / confirmed across all bands (terrestrial —
    /// satellite contacts live in `sat_worked`/`sat_confirmed`, never here).
    pub worked: usize,
    pub confirmed: usize,
    /// Per-band grid-square counts, in canonical 160m → 2m order.
    pub bands: Vec<BandAward>,
    /// Distinct grid squares worked / confirmed VIA SATELLITE (`PROP_MODE=SAT`)
    /// — the ARRL **Satellite VUCC** category. ARRL counts satellite contacts
    /// toward this award only (never the per-band grid awards), and the bucket
    /// is band-independent, so a 70cm sat QSO counts even though 70cm has no
    /// per-band slot.
    #[serde(default)]
    pub sat_worked: usize,
    #[serde(default)]
    pub sat_confirmed: usize,
    /// The REAL per-band VUCC standings — 50 MHz and up only, each band against its
    /// own ARRL threshold (6m/4m/2m 100; 1.25m/70cm 50; 33cm/23cm 25). The overall
    /// `worked`/`confirmed` above are an ALL-BAND grid tracker (HF included) and are
    /// not a VUCC claim; these are (N9UM audit, 2026-08-09).
    #[serde(default)]
    pub awards: Vec<VuccBandStanding>,
}

/// One band's standing against the ARRL VUCC threshold for that band.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VuccBandStanding {
    pub band: String,
    pub worked: usize,
    pub confirmed: usize,
    pub threshold: usize,
    pub achieved: bool,
}

/// ARRL VUCC bands + thresholds (50 MHz and up; grids on other bands are tracker-only).
const VUCC_THRESHOLDS: &[(&str, usize)] = &[
    ("6m", 100),
    ("4m", 100),
    ("2m", 100),
    ("1.25m", 50),
    ("70cm", 50),
    ("33cm", 25),
    ("23cm", 25),
    // ARRL VUCC: 2.3 GHz and above need 5 grids per band. All named since Batch 3
    // (QO-100 downlink = 3 cm), 9 cm included — VUCC is an operating award and the
    // band exists outside the US allocation table.
    ("13cm", 5),
    ("9cm", 5),
    ("6cm", 5),
    ("3cm", 5),
    ("1.25cm", 5),
];

/// Canonical listing order for the per-band grid table (HF tracker bands first,
/// then the VUCC bands above 2 m).
const GRID_BAND_ORDER: &[&str] = &[
    "160m", "80m", "60m", "40m", "30m", "20m", "17m", "15m", "12m", "10m", "6m", "4m", "2m",
    "1.25m", "70cm", "33cm", "23cm", "13cm", "9cm", "6cm", "3cm", "1.25cm",
];

/// Normalise a band label for grid keying: lowercase, trimmed. Grids key on the LABEL,
/// not the DXCC [`Band`] enum — the enum once topped out at 2 m, which silently dropped every
/// terrestrial 70 cm grid from a page whose caption said "all bands" (N9UM audit), and it
/// still names no band above 23 cm.
fn grid_band(label: &str) -> Option<String> {
    let l = label.trim().to_ascii_lowercase();
    (!l.is_empty()).then_some(l)
}

/// ARRL Fred Fish Memorial Award progress: the 488 grid squares of the 48 contiguous states
/// ([`crate::ffma`]), worked and confirmed on 6 m. Read from the same per-band grid sets as
/// VUCC, so it confirms the way VUCC does (LoTW or a paper card) and a satellite contact never
/// reaches it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FfmaProgress {
    /// FFMA squares worked / confirmed on 6 m.
    pub worked: usize,
    pub confirmed: usize,
    /// The squares the award needs: 488.
    pub total: usize,
    /// Squares never worked on 6 m, in the ARRL list's order.
    pub unworked: Vec<String>,
    /// Squares worked on 6 m but not confirmed, in the ARRL list's order.
    pub unconfirmed: Vec<String>,
}

/// IOTA (Islands On The Air) progress — distinct island-group references worked /
/// confirmed. Basic IOTA = 100 confirmed groups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IotaProgress {
    pub worked: usize,
    /// Confirmed by LoTW or QSL card (`award_confirmed`; eQSL never counts) — what the IOTA
    /// programme credits (cards, and LoTW-matched contacts since May 2020), so the award
    /// checkmark gates on this count.
    pub confirmed: usize,
    /// Confirmed by QSL CARD alone, shown beside the award count.
    #[serde(default)]
    pub card_confirmed: usize,
}

/// Worked All States progress (50 US states; LoTW/paper confirmed, eQSL excluded).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WasProgress {
    /// Distinct states worked / confirmed (50 confirmed = WAS).
    pub worked: usize,
    pub confirmed: usize,
    /// The states still to confirm (postal codes, sorted) — the WAS chase.
    pub needed: Vec<String>,
    /// 5-Band WAS standing: the WEAKEST award band's state count (the award needs
    /// all 50 on each of 80/40/20/15/10m, judged per band — 50 here = 5BWAS).
    pub five_band_worked: usize,
    pub five_band_confirmed: usize,
}

/// Worked All Provinces progress — the 13 Canadian provinces and territories.
///
/// No five-band tier, deliberately: 5BWAS exists because ARRL awards it, and there is no
/// equivalent standing here to report. Adding a number nobody can claim would be inventing an
/// award rather than tracking one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WapProgress {
    /// Distinct provinces worked / confirmed (13 confirmed = all of them).
    pub worked: usize,
    pub confirmed: usize,
    /// The provinces still to confirm (ADIF codes, sorted) — the chase list.
    pub needed: Vec<String>,
}

/// What a contact's own record carries beyond its call, for the awards that count by it: the DXCC
/// entity number (ADIF `DXCC`) and CQ zone (ADIF `CQZ`) the log holds, and the DXCC LoTW
/// confirmed it under where that differs from the log's. `Default` carries none of them, and the
/// contact then counts by its call through cty.dat, as it always did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecordFields {
    /// ADIF `DXCC` as the log holds it.
    pub dxcc: Option<u32>,
    /// LoTW's DXCC for the contact, kept beside the log's own where the two differ. ARRL credits
    /// LoTW's, so it counts; the log's value is never changed.
    pub lotw_dxcc: Option<u32>,
    /// ADIF `CQZ`.
    pub cqz: Option<u8>,
    /// When the contact was made, Unix seconds UTC — only to name it in
    /// [`AwardSummary::dxcc_disagreements`].
    pub when_unix: u64,
    /// Which channels hold a confirmation of the contact. The award fold's own `confirmed` stays
    /// the ARRL gate (LoTW or card); these add what a CQ award credits beyond it, and what the
    /// Stats pane lists per source.
    pub qsl: QslChannels,
}

/// The channels a contact's confirmation came through — the record's per-source truth
/// (`QSL_RCVD`, `LOTW_QSL_RCVD`, `EQSL_QSL_RCVD`, the QRZ logbook's status).
///
/// What each award credits is a matrix, not one flag: the ARRL's (DXCC, WAS) take LoTW and the
/// paper card only; CQ's WAZ also takes an eQSL from an Authenticity-Guaranteed member, and no
/// award takes a basic eQSL or QRZ.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QslChannels {
    pub card: bool,
    pub lotw: bool,
    pub eqsl: bool,
    /// The eQSL confirmation came from an Authenticity-Guaranteed member (eQSL's
    /// `APP_EQSL_AG:Y`). Meaningful only beside `eqsl`.
    pub eqsl_ag: bool,
    pub qrz: bool,
}

impl QslChannels {
    /// An eQSL from an Authenticity-Guaranteed member — what WAZ credits beyond LoTW and the card.
    pub fn eqsl_ag_confirmed(&self) -> bool {
        self.eqsl && self.eqsl_ag
    }

    /// Every channel either holds.
    pub fn or(self, o: QslChannels) -> QslChannels {
        QslChannels {
            card: self.card || o.card,
            lotw: self.lotw || o.lotw,
            eqsl: self.eqsl || o.eqsl,
            eqsl_ag: self.eqsl_ag || o.eqsl_ag,
            qrz: self.qrz || o.qrz,
        }
    }
}

/// A contact LoTW confirmed under another DXCC entity than the log records for it. The awards
/// count it by LoTW's; the log keeps its own, and this is where the operator sees the difference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DxccDisagreement {
    pub call: String,
    pub when_unix: u64,
    pub band: String,
    pub mode: String,
    /// The log's DXCC number, and the entity it names when the table knows it.
    pub logged_dxcc: u32,
    pub logged_entity: Option<String>,
    /// LoTW's, likewise.
    pub lotw_dxcc: u32,
    pub lotw_entity: Option<String>,
}

/// One band's chase for one award: what has never been worked on the band, and what has been
/// worked there but not confirmed by anything the award credits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BandNeed {
    pub band: String,
    /// Entity names (alphabetical), state codes (alphabetical) or zone numbers (1 to 40).
    pub unworked: Vec<String>,
    pub unconfirmed: Vec<String>,
}

/// What DXCC, WAS and WAZ still need, band by band, counted from the same contacts as the totals:
/// a contact fills a band's slot exactly when it counts toward that award at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AwardBandNeeds {
    /// The current DXCC entities, on every band that earns DXCC credit (not 60 m, not 4 m).
    pub dxcc: Vec<BandNeed>,
    /// The 50 states, on every band.
    pub was: Vec<BandNeed>,
    /// The 40 CQ zones, on every band; confirmed also by an Authenticity-Guaranteed eQSL.
    pub waz: Vec<BandNeed>,
}

/// DXCC-first award summary for the dashboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AwardSummary {
    pub qsos: usize,
    pub confirmed_qsos: usize,
    /// Distinct CURRENT DXCC entities worked / confirmed (100 confirmed = basic DXCC). A deleted
    /// entity counts only toward [`dxcc_worked_total`](Self::dxcc_worked_total).
    pub dxcc_worked: usize,
    pub dxcc_confirmed: usize,
    /// Distinct DXCC entities worked / confirmed, the deleted ones included — LoTW's "total"
    /// beside the current count above. A contact reaches a deleted entity only by the number its
    /// record carries (cty.dat lists none).
    #[serde(default)]
    pub dxcc_worked_total: usize,
    #[serde(default)]
    pub dxcc_confirmed_total: usize,
    /// Every contact LoTW confirmed under another DXCC entity than the log records, in the order
    /// the log holds them.
    #[serde(default)]
    pub dxcc_disagreements: Vec<DxccDisagreement>,
    /// Distinct DXCC entities with award credit **granted** by ARRL (official
    /// standing) — typically ≤ `dxcc_confirmed`.
    pub dxcc_credited: usize,
    /// Confirmed-but-not-yet-credited entities (`confirmed − credited`) — the
    /// "ready to submit to ARRL" gap an elite chaser closes.
    pub ready_to_submit: usize,
    /// Entity×band "DXCC Challenge" slots worked / confirmed.
    pub slots_worked: usize,
    pub slots_confirmed: usize,
    /// Per-band entity progress, band-ordered (160m → 2m).
    pub bands: Vec<BandAward>,
    /// Per-mode DXCC entity progress (CW / Phone / Digital — separate awards).
    pub modes: Vec<ModeAward>,
    /// Worked-but-unconfirmed entities (the "new one" chase — a new DXCC entity),
    /// most-bands first.
    pub needed: Vec<EntityNeed>,
    /// The DXCC-Challenge chase: entities you ALREADY have confirmed, but with
    /// worked-but-unconfirmed band slots — confirming each adds a Challenge slot.
    /// (Disjoint from `needed`, which is brand-new entities.) Most-bands first.
    pub slot_needed: Vec<EntityNeed>,
    /// Gamification milestones (unlocked + locked-with-progress), evaluated from
    /// the tallies above.
    pub achievements: Vec<Achievement>,
    /// 5-Band DXCC standing: the WEAKEST award band's entity count. ARRL grants
    /// 5BDXCC for 100 entities confirmed on EACH of 80/40/20/15/10m independently
    /// (different entities allowed per band), so the qualifying statistic is the
    /// minimum per-band count — an intersection across bands understates it
    /// (N9UM audit, 2026-08-09).
    pub five_band_worked: usize,
    pub five_band_confirmed: usize,
    /// Satellite DXCC — entities worked / confirmed via satellite. ARRL restricts
    /// satellite contacts to the satellite awards, so these never appear in the
    /// Mixed / band / mode DXCC counts above (LoTW's "Satellite" account column).
    #[serde(default)]
    pub sat_dxcc_worked: usize,
    #[serde(default)]
    pub sat_dxcc_confirmed: usize,
    /// Worked All Zones (CQ WAZ): distinct CQ zones worked / confirmed, out of 40.
    pub waz_worked: usize,
    pub waz_confirmed: usize,
    /// DXCC Honor Roll standing (current-entity, confirmed).
    pub honor_roll: HonorRollProgress,
    /// Worked All States (50 US states) + 5-Band WAS.
    pub was: WasProgress,
    pub wap: WapProgress,
    /// VUCC — Maidenhead grid squares worked / confirmed, overall and per band.
    pub vucc: VuccProgress,
    /// FFMA — the 488 squares of the 48 contiguous states, on 6 m.
    #[serde(default)]
    pub ffma: FfmaProgress,
    /// IOTA — island-group references worked / confirmed.
    pub iota: IotaProgress,
    /// CQ WPX — prefixes worked / confirmed ([`crate::wpx`]).
    #[serde(default)]
    pub wpx: WpxProgress,
    /// DARC's WAE — countries and band points ([`crate::wae`]).
    #[serde(default)]
    pub wae: WaeProgress,
    /// WWFF — references worked ([`crate::wwff`]).
    #[serde(default)]
    pub wwff: WwffProgress,
    /// The WORK chase (elite "every-band" tracker): entities already worked on
    /// most award bands but NOT yet on a few — the bands listed are ones to WORK
    /// (a new contact, not just a confirmation). Closest-to-complete first.
    pub band_targets: Vec<EntityNeed>,
    /// DXCC, WAS and WAZ needed per band: never worked, and worked but not confirmed.
    #[serde(default)]
    pub band_needs: AwardBandNeeds,
}

/// Accumulates award progress from logged QSOs (fed one [`add`](Awards::add) per
/// contact), then snapshot with [`summary`](Awards::summary).
#[derive(Debug, Clone, Default)]
pub struct Awards {
    qsos: usize,
    confirmed_qsos: usize,
    worked_entity: HashSet<&'static str>,
    confirmed_entity: HashSet<&'static str>,
    /// DXCC entities with award credit **granted** (ARRL credited it — a QSO with
    /// `DXCC` in its `credit_granted`). Subset of confirmed in practice.
    credited_entity: HashSet<&'static str>,
    worked_slot: HashSet<(&'static str, Band)>,
    confirmed_slot: HashSet<(&'static str, Band)>,
    /// band → (worked entities, confirmed entities)
    per_band: HashMap<Band, (HashSet<&'static str>, HashSet<&'static str>)>,
    /// mode class → (worked entities, confirmed entities)
    per_mode: HashMap<ModeClass, (HashSet<&'static str>, HashSet<&'static str>)>,
    /// CQ zones worked / confirmed (WAZ — 40 zones), and per band.
    worked_zones: HashSet<u8>,
    confirmed_zones: HashSet<u8>,
    worked_zone_band: HashSet<(u8, Band)>,
    confirmed_zone_band: HashSet<(u8, Band)>,
    /// US states worked / confirmed (WAS — 50 states), and per band (5BWAS reads the award bands).
    worked_states: HashSet<&'static str>,
    confirmed_states: HashSet<&'static str>,
    worked_provinces: HashSet<&'static str>,
    confirmed_provinces: HashSet<&'static str>,
    worked_state_band: HashSet<(&'static str, Band)>,
    confirmed_state_band: HashSet<(&'static str, Band)>,
    /// VUCC — distinct Maidenhead grid squares (4-char field) worked / confirmed per band.
    /// Keyed on the normalised band LABEL (not the DXCC [`Band`] enum): VUCC's real bands
    /// run past 2 m, and the enum key silently dropped terrestrial 70 cm grids.
    worked_grid_band: HashSet<(String, String)>,
    confirmed_grid_band: HashSet<(String, String)>,
    /// Satellite VUCC — grids worked / confirmed via satellite (`PROP_MODE=SAT`),
    /// band-independent. Disjoint from the per-band sets by construction (see
    /// [`add_qso`](Self::add_qso)).
    worked_grid_sat: HashSet<String>,
    confirmed_grid_sat: HashSet<String>,
    /// IOTA — distinct island-group references ("NA-001") worked / confirmed / card-confirmed.
    worked_iota: HashSet<String>,
    confirmed_iota: HashSet<String>,
    card_iota: HashSet<String>,
    /// Satellite DXCC — entities via satellite (excluded from every terrestrial DXCC fold).
    worked_entity_sat: HashSet<&'static str>,
    confirmed_entity_sat: HashSet<&'static str>,
    /// Deleted DXCC entities worked / confirmed, by number — in the totals only.
    worked_deleted: HashSet<u32>,
    confirmed_deleted: HashSet<u32>,
    /// Contacts LoTW confirmed under another DXCC entity than the log records, in log order.
    disagreements: Vec<DxccDisagreement>,
    /// The operator's own DXCC entity (resolved from mycall), so DX-oriented
    /// achievements can count entities OTHER than home. None until set.
    home_entity: Option<&'static str>,
    /// The Stats pane's index, kept only by an accumulator made for it
    /// ([`with_call_stats`](Self::with_call_stats)): each slot filled exactly where the fold
    /// below credits one, so the pane's grids are the awards' own counts.
    call_stats: Option<CallStatsIndex>,
    /// WPX, WAE, WWFF, the DXCC chart and What-to-QSL, fed what this fold credits each contact.
    more: MoreAwards,
}

/// What a contact's record carries for the awards counted beside DXCC ([`Awards::add_contact`]):
/// its WWFF reference and its QSL route. `Default` carries neither.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContactExtras<'a> {
    /// The WWFF reference worked (ADIF `WWFF_REF`, or `SIG_INFO` beside `SIG`=WWFF).
    pub wwff: Option<&'a str>,
    /// ADIF `QSL_VIA`.
    pub qsl_via: Option<&'a str>,
}

/// The DXCC entity a contact counts toward ([`counted_entity`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counted {
    /// Its number, where the record or cty.csv gives one.
    pub code: Option<u32>,
    /// A current entity's cty.dat name, or a deleted one's.
    pub name: &'static str,
    pub deleted: bool,
}

impl Counted {
    /// The WAS family — the United States, Alaska, Hawaii — by number where there is one.
    pub fn is_us_state(&self) -> bool {
        match self.code {
            Some(code) => dxcc::is_us_state_dxcc(code),
            None => dxcc::is_us_state_entity(self.name),
        }
    }
}

/// Which DXCC entity a contact counts toward, or `None` for no entity at all.
///
/// **The record's own number first** — LoTW's, then the log's — wherever the table knows it:
/// it is what the ARRL credits, and the call cannot tell it. A 1×1 call like K1N is Navassa
/// in 2015 and a US special event another year, and only the record knows which. ADIF's 0 says
/// the station was in no entity (a ship at sea), so it earns none. A number the table does not
/// know (garbled, or newer than the table) is passed over.
///
/// **Else the call, through cty.dat.** A WAE/CQ-only area counts as the parent cty.csv gives it
/// (Sicily as Italy), which is how the ARRL credits it; an entity with no number keeps the old
/// rule, its name and no credit for a WAE area.
///
/// The ONE answer to "which entity is this logged contact": the awards, the Needed board's needs
/// model ([`crate::dxped::LogNeeds`]), the log statistics ([`crate::stats`], and the dashboard's
/// counts the app makes), Worked All Provinces, and the home entity "First DX" is judged against
/// all read it, so none of them can call an entity needed, or count it, differently from the
/// others. A station heard on the air has no record: its entity comes from its call alone.
pub fn counted_entity(resolved: Option<&dxcc::DxccInfo>, rec: &RecordFields) -> Option<Counted> {
    for code in [rec.lotw_dxcc, rec.dxcc].into_iter().flatten() {
        if code == dxcc::NO_DXCC_ENTITY {
            return None;
        }
        if let Some(e) = dxcc::entity_by_dxcc(code) {
            return Some(Counted {
                code: Some(code),
                name: e.name,
                deleted: e.deleted,
            });
        }
    }
    let info = resolved?;
    match info.dxcc.and_then(dxcc::entity_by_dxcc) {
        Some(e) => Some(Counted {
            code: Some(e.code),
            name: e.name,
            deleted: e.deleted,
        }),
        None => info.is_dxcc.then_some(Counted {
            code: None,
            name: info.entity,
            deleted: false,
        }),
    }
}

/// The CQ zone a contact counts toward (WAZ), beside [`counted_entity`]: its record's CQZ where
/// that is a zone (1–40) — a W1 licensee living in California is zone 3, whatever the W1 prefix
/// says — else the zone cty.dat gives its call; `None` when neither gives one.
pub fn counted_zone(resolved: Option<&dxcc::DxccInfo>, rec: &RecordFields) -> Option<u8> {
    let zone = |z: &u8| (1..=40).contains(z);
    rec.cqz
        .filter(zone)
        .or_else(|| resolved.map(|i| i.cq_zone).filter(zone))
}

impl Awards {
    pub fn new() -> Self {
        Self::default()
    }

    /// An accumulator that also keeps the Stats pane's index ([`CallStatsIndex`]), taken with
    /// [`take_call_stats`](Self::take_call_stats) once the log is folded.
    pub fn with_call_stats() -> Self {
        Self {
            call_stats: Some(CallStatsIndex::default()),
            ..Self::default()
        }
    }

    /// The Stats pane's index, when this accumulator keeps one.
    pub fn take_call_stats(&mut self) -> Option<CallStatsIndex> {
        self.call_stats.take()
    }

    /// Record the operator's own callsign so DX-oriented achievements ("First DX")
    /// count entities OTHER than home. Without it, a US op whose only worked entity
    /// is foreign would never unlock "First DX". Safe to call zero or many times.
    ///
    /// Home is the entity a contact from this call counts toward ([`counted_entity`]), so the
    /// two compare alike: an IT9 operator is home in Italy, as the ARRL credits Sicily, and an
    /// Italian contact is not DX for them.
    pub fn set_home_call(&mut self, call: &str) {
        self.home_entity =
            counted_entity(dxcc::resolve(call).as_ref(), &RecordFields::default()).map(|c| c.name);
    }

    /// Fold one logged contact in. `band` is an ADIF band label ("20m"); `mode`
    /// an ADIF MODE ("CW"/"SSB"/"FT8"…, classed CW/Phone/Digital); `confirmed` is
    /// the **award-eligible** QSL state (LoTW/paper). A call cty.dat can't resolve
    /// still counts toward total QSOs but not DXCC; a band label that doesn't
    /// parse counts the entity but no slot.
    pub fn add(&mut self, call: &str, band: &str, mode: &str, confirmed: bool) {
        self.add_with_credit(call, band, mode, confirmed, false, None, None, None)
    }

    /// As [`add`](Self::add), plus `credited` — whether ARRL has **granted** DXCC
    /// credit for this QSO (its `credit_granted` contains a `DXCC` code) — and the
    /// contact's US `state` (ADIF STATE) for WAS. Drives the "confirmed vs
    /// officially credited" gap and the Worked-All-States tier. Terrestrial only —
    /// a caller folding real log records uses [`add_qso`](Self::add_qso), which
    /// also takes the satellite flag.
    #[allow(clippy::too_many_arguments)]
    pub fn add_with_credit(
        &mut self,
        call: &str,
        band: &str,
        mode: &str,
        confirmed: bool,
        credited: bool,
        state: Option<&str>,
        grid: Option<&str>,
        iota: Option<&str>,
    ) {
        self.add_qso(
            call,
            band,
            mode,
            confirmed,
            credited,
            false,
            state,
            grid,
            iota,
            false,
            RecordFields::default(),
        )
    }

    /// The full fold: [`add_with_credit`](Self::add_with_credit) plus `sat` —
    /// whether the contact was made through a satellite (`PROP_MODE=SAT`) — and
    /// what the contact's record carries beyond its call ([`RecordFields`]): DXCC,
    /// WAS and WAZ count by the record's numbers where it has them. Production log
    /// folds go through here so satellite credit and the record's numbers are
    /// stated, not defaulted.
    #[allow(clippy::too_many_arguments)]
    pub fn add_qso(
        &mut self,
        call: &str,
        band: &str,
        mode: &str,
        confirmed: bool,
        credited: bool,
        card_confirmed: bool,
        state: Option<&str>,
        grid: Option<&str>,
        iota: Option<&str>,
        sat: bool,
        rec: RecordFields,
    ) {
        self.add_contact(
            call,
            band,
            mode,
            confirmed,
            credited,
            card_confirmed,
            state,
            grid,
            iota,
            sat,
            rec,
            ContactExtras::default(),
        )
    }

    /// [`add_qso`](Self::add_qso), plus what the record carries for the awards counted beside
    /// DXCC ([`ContactExtras`]): WPX, WAE, WWFF, the DXCC chart and What-to-QSL are fed exactly
    /// the slots the fold below credits.
    #[allow(clippy::too_many_arguments)]
    pub fn add_contact(
        &mut self,
        call: &str,
        band: &str,
        mode: &str,
        confirmed: bool,
        credited: bool,
        card_confirmed: bool,
        state: Option<&str>,
        grid: Option<&str>,
        iota: Option<&str>,
        sat: bool,
        rec: RecordFields,
        extras: ContactExtras<'_>,
    ) {
        let mut cr = Credited::default();
        self.fold(
            call,
            band,
            mode,
            confirmed,
            credited,
            card_confirmed,
            state,
            grid,
            iota,
            sat,
            rec,
            &mut cr,
        );
        self.more.add(
            &Contact {
                call,
                band,
                on_band: Band::from_label(band),
                mode,
                class: ModeClass::from_adif(mode),
                confirmed,
                when_unix: rec.when_unix,
                wwff: extras.wwff,
                qsl_via: extras.qsl_via,
            },
            &cr,
        );
    }

    /// The fold itself; `cr` collects each slot it credits, confirmed or not.
    #[allow(clippy::too_many_arguments)]
    fn fold(
        &mut self,
        call: &str,
        band: &str,
        mode: &str,
        confirmed: bool,
        credited: bool,
        card_confirmed: bool,
        state: Option<&str>,
        grid: Option<&str>,
        iota: Option<&str>,
        sat: bool,
        rec: RecordFields,
        cr: &mut Credited,
    ) {
        self.qsos += 1;
        if confirmed {
            self.confirmed_qsos += 1;
        }
        // VUCC — grid squares, independent of DXCC (a grid is a grid, on any
        // entity). A SATELLITE contact's grid credits Satellite VUCC only —
        // ARRL excludes satellite QSOs from the per-band grid awards — and the
        // satellite bucket is band-independent, so it counts whatever the band
        // label says. Terrestrial grids keep the
        // per-band slots (valid 4-char locator + parseable band).
        if let Some(g) = grid.and_then(valid_grid4) {
            if sat {
                self.worked_grid_sat.insert(g.clone());
                if confirmed {
                    self.confirmed_grid_sat.insert(g);
                }
            } else if let Some(b) = grid_band(band) {
                cr.grid = Some((g.clone(), b.clone()));
                self.worked_grid_band.insert((g.clone(), b.clone()));
                if confirmed {
                    self.confirmed_grid_band.insert((g, b));
                }
            }
        }
        // IOTA — island groups (independent of DXCC/band). The ref is validated at parse
        // time; trim + uppercase for a canonical key.
        if let Some(i) = iota
            .map(|s| s.trim().to_ascii_uppercase())
            .filter(|s| !s.is_empty())
        {
            self.worked_iota.insert(i.clone());
            if confirmed {
                self.confirmed_iota.insert(i.clone());
            }
            if card_confirmed {
                self.card_iota.insert(i);
            }
        }
        let resolved = dxcc::resolve(call);
        // The entity DXCC and the WAS and WAP gates count by: the record's number, else the call's.
        let counted = counted_entity(resolved.as_ref(), &rec);
        let on_band = Band::from_label(band);
        let class = ModeClass::from_adif(mode);
        if let Some(stats) = &mut self.call_stats {
            stats.note_qsl(call, counted.map(|c| c.name), rec.qsl);
        }
        cr.any_entity = counted.filter(|c| !c.deleted).map(|c| c.name);
        cr.wae = crate::wae::wae_country(resolved.as_ref(), &rec);
        if let (Some(lotw_dxcc), Some(logged_dxcc)) = (rec.lotw_dxcc, rec.dxcc) {
            if lotw_dxcc != logged_dxcc {
                let name = |code| dxcc::entity_by_dxcc(code).map(|e| e.name.to_string());
                self.disagreements.push(DxccDisagreement {
                    call: call.to_string(),
                    when_unix: rec.when_unix,
                    band: band.to_string(),
                    mode: mode.to_string(),
                    logged_dxcc,
                    logged_entity: name(logged_dxcc),
                    lotw_dxcc,
                    lotw_entity: name(lotw_dxcc),
                });
            }
        }
        // WAS — keyed on the STATE code, but GATED on a US-family DXCC entity
        // (United States / Alaska / Hawaii). The entity gate keeps HI/AK counting
        // while rejecting non-US subdivision codes that collide with US postal
        // codes (e.g. Australian "WA" = Western Australia vs Washington). eQSL
        // excluded: WAS uses award-eligible `confirmed`. The entity is the record's
        // where it carries one: a KP4 licensee signing LoTW from Florida is DXCC 291.
        let is_us_state = counted.is_some_and(|c| c.is_us_state());
        if is_us_state {
            if let Some(code) = state.and_then(valid_state) {
                cr.state = Some(code);
                self.worked_states.insert(code);
                if confirmed {
                    self.confirmed_states.insert(code);
                }
                if let Some(b) = on_band {
                    self.worked_state_band.insert((code, b));
                    if confirmed {
                        self.confirmed_state_band.insert((code, b));
                    }
                }
                if let Some(stats) = &mut self.call_stats {
                    stats.note_was(code, on_band, class, confirmed);
                }
            }
        }
        // WAP — Worked All Provinces. Same shape as WAS above and gated the same way, for the
        // same reason: an ADIF subdivision code is only meaningful INSIDE its entity, and these
        // collide across countries. "NT" is Northwest Territories here and Australia's Northern
        // Territory there; "WA" is Washington for WAS and Western Australia elsewhere. Without
        // the entity gate a VK contact would quietly advance a Canadian award.
        //
        // Credit comes from the logged ADIF STATE, never from the callsign hint: a licensee who
        // moved keeps the old numeral, so `province_for_call` labels a roster row but must not
        // decide an award (see the note at the top of `province.rs`). The entity is the one the
        // contact counts toward, like WAS's: the record's DXCC number where it carries one.
        let is_canada = counted.is_some_and(|c| c.name == "Canada");
        if is_canada {
            if let Some(code) = state.and_then(valid_province) {
                cr.province = Some(code);
                self.worked_provinces.insert(code);
                if confirmed {
                    self.confirmed_provinces.insert(code);
                }
            }
        }
        // WAZ: CQ zones 1..=40 — [`counted_zone`]: the record's CQZ where it carries one, else
        // cty.dat's zone for the call (0 = cty.dat couldn't supply one — skip it). The zone is
        // valid even on a non-DXCC (WAE) entity — Sicily, African Italy etc. are real CQ-zone
        // contacts — so this is OUTSIDE the DXCC gate. CQ credits what the ARRL does and an
        // eQSL from an Authenticity-Guaranteed member as well.
        if let Some(zone) = counted_zone(resolved.as_ref(), &rec) {
            let zone_confirmed = confirmed || rec.qsl.eqsl_ag_confirmed();
            cr.zone = Some((zone, zone_confirmed));
            self.worked_zones.insert(zone);
            if zone_confirmed {
                self.confirmed_zones.insert(zone);
            }
            if let Some(b) = on_band {
                self.worked_zone_band.insert((zone, b));
                if zone_confirmed {
                    self.confirmed_zone_band.insert((zone, b));
                }
            }
            if let Some(stats) = &mut self.call_stats {
                stats.note_waz(zone, on_band, class, zone_confirmed);
            }
        }
        // DXCC awards credit ARRL DXCC entities only ([`counted_entity`]): a WAE/CQ-only area
        // (Sicily, European Turkey, African Italy, Shetland, Bear Island, Vienna) counts as its
        // parent, and a contact no entity takes earns no entity / slot / band / mode credit.
        // (It still counted for the QSO total + WAZ above.)
        let Some(counted) = counted else {
            return;
        };
        // ARRL restricts satellite contacts to the SATELLITE awards: a bird QSO earns
        // Satellite DXCC here and never a Mixed / band / mode entity — counting a
        // SAT-tagged 2 m QSO in 2 m band-DXCC is exactly the overcount LoTW's own
        // 2M column refuses (N9UM audit, 2026-08-09). QSO totals, WAZ, WAS, IOTA and
        // the grid fold above all follow their own programs' rules and are untouched.
        if sat {
            if counted.deleted {
                return;
            }
            let entity = counted.name;
            self.worked_entity_sat.insert(entity);
            if confirmed {
                self.confirmed_entity_sat.insert(entity);
            }
            return;
        }
        // …and neither does a contact on a band ARRL does not credit. Placed HERE, after the QSO
        // total, WAZ, IOTA, the VUCC grids and WAS, so only DXCC is affected — see
        // [`earns_dxcc_credit`]. An unparseable band label still earns entity/mode credit as
        // before; only a band we recognise AND exclude is refused.
        if on_band.is_some_and(|b| !earns_dxcc_credit(b)) {
            return;
        }
        // A deleted entity counts toward the totals and nothing else: no current count, Honor
        // Roll, Challenge slot, band, mode or chase.
        if counted.deleted {
            if let Some(code) = counted.code {
                self.worked_deleted.insert(code);
                if confirmed {
                    self.confirmed_deleted.insert(code);
                }
            }
            return;
        }
        let entity = counted.name; // &'static str (cty.dat is leaked once)
        cr.entity = Some(entity);
        self.worked_entity.insert(entity);
        if confirmed {
            self.confirmed_entity.insert(entity);
        }
        // Credit implies a held confirmation — gate on `confirmed` so credited can
        // never exceed confirmed (keeps the "confirmed ≥ credited" dashboard model
        // and ready_to_submit = confirmed − credited honest, even for a malformed
        // report row carrying CREDIT_GRANTED without a QSL field).
        if credited && confirmed {
            self.credited_entity.insert(entity);
        }
        // Per-mode DXCC (CW/Phone/Digital are separate awards).
        let pm = self.per_mode.entry(class).or_default();
        pm.0.insert(entity);
        if confirmed {
            pm.1.insert(entity);
        }
        // The band slot, where the band has one: a UHF contact is entity and mode credit only.
        let on_band = on_band.filter(|b| fills_dxcc_band_slot(*b));
        if let Some(stats) = &mut self.call_stats {
            stats.note_dxcc(entity, on_band, mode, confirmed);
        }
        cr.dxcc_band = on_band;
        if let Some(b) = on_band {
            self.worked_slot.insert((entity, b));
            let pb = self.per_band.entry(b).or_default();
            pb.0.insert(entity);
            if confirmed {
                self.confirmed_slot.insert((entity, b));
                pb.1.insert(entity);
            }
        }
    }

    /// Snapshot the award progress.
    pub fn summary(&self) -> AwardSummary {
        // The "new one" chase: entities worked but not yet confirmed anywhere.
        // Built from the final slot state so a later confirmed QSO removes an
        // entity from the chase. (A confirmed entity that still needs specific
        // *band* slots is a Challenge need — a later, richer view.)
        let mut needed_map: HashMap<&'static str, BTreeSet<String>> = HashMap::new();
        for (entity, band) in &self.worked_slot {
            if self.confirmed_entity.contains(entity) {
                continue;
            }
            needed_map
                .entry(entity)
                .or_default()
                .insert(band.label().to_string());
        }
        let mut needed: Vec<EntityNeed> = needed_map
            .into_iter()
            .map(|(entity, bands)| EntityNeed {
                entity: entity.to_string(),
                bands: bands.into_iter().collect(),
            })
            .collect();
        needed.sort_by(|a, b| {
            b.bands
                .len()
                .cmp(&a.bands.len())
                .then_with(|| a.entity.cmp(&b.entity))
        });

        // The Challenge-slot chase: entities already CONFIRMED (so not a new DXCC
        // entity) that still have worked-but-unconfirmed band slots — confirming
        // each adds a Challenge slot.
        let mut slot_map: HashMap<&'static str, BTreeSet<String>> = HashMap::new();
        for (entity, band) in &self.worked_slot {
            if !CHALLENGE_BANDS.contains(band) {
                continue; // 60m/2m/4m are not Challenge slots to fill
            }
            if !self.confirmed_entity.contains(entity) {
                continue; // brand-new entity → the `needed` chase, not here
            }
            if self.confirmed_slot.contains(&(*entity, *band)) {
                continue; // this slot is already confirmed
            }
            slot_map
                .entry(entity)
                .or_default()
                .insert(band.label().to_string());
        }
        let mut slot_needed: Vec<EntityNeed> = slot_map
            .into_iter()
            .map(|(entity, bands)| EntityNeed {
                entity: entity.to_string(),
                bands: bands.into_iter().collect(),
            })
            .collect();
        slot_needed.sort_by(|a, b| {
            b.bands
                .len()
                .cmp(&a.bands.len())
                .then_with(|| a.entity.cmp(&b.entity))
        });

        // Per-band, in canonical 160m → 2m order.
        let bands: Vec<BandAward> = Band::ALL
            .iter()
            .filter_map(|b| {
                self.per_band.get(b).map(|(w, c)| BandAward {
                    band: b.label().to_string(),
                    worked: w.len(),
                    confirmed: c.len(),
                })
            })
            .collect();

        // Per-mode DXCC (CW / Phone / Digital).
        let modes: Vec<ModeAward> = MODE_CLASSES
            .iter()
            .filter_map(|m| {
                self.per_mode.get(m).map(|(w, c)| ModeAward {
                    mode: m.label().to_string(),
                    worked: w.len(),
                    confirmed: c.len(),
                })
            })
            .collect();

        // VUCC — distinct grid squares overall (deduped across bands) + per band.
        let vucc_worked: HashSet<&str> = self
            .worked_grid_band
            .iter()
            .map(|(g, _)| g.as_str())
            .collect();
        let vucc_confirmed: HashSet<&str> = self
            .confirmed_grid_band
            .iter()
            .map(|(g, _)| g.as_str())
            .collect();
        let mut vucc_wb: HashMap<&str, HashSet<&str>> = HashMap::new();
        let mut vucc_cb: HashMap<&str, HashSet<&str>> = HashMap::new();
        for (g, b) in &self.worked_grid_band {
            vucc_wb.entry(b.as_str()).or_default().insert(g.as_str());
        }
        for (g, b) in &self.confirmed_grid_band {
            vucc_cb.entry(b.as_str()).or_default().insert(g.as_str());
        }
        // Canonical order first, then any label the order table doesn't know
        // (sorted) — a log's odd band string still shows rather than vanishing.
        let mut band_labels: Vec<&str> = GRID_BAND_ORDER
            .iter()
            .copied()
            .filter(|b| vucc_wb.contains_key(b))
            .collect();
        let mut extras: Vec<&str> = vucc_wb
            .keys()
            .copied()
            .filter(|b| !GRID_BAND_ORDER.contains(b))
            .collect();
        extras.sort_unstable();
        band_labels.extend(extras);
        let vucc = VuccProgress {
            worked: vucc_worked.len(),
            confirmed: vucc_confirmed.len(),
            bands: band_labels
                .iter()
                .map(|b| BandAward {
                    band: (*b).to_string(),
                    worked: vucc_wb.get(b).map_or(0, |w| w.len()),
                    confirmed: vucc_cb.get(b).map_or(0, |c| c.len()),
                })
                .collect(),
            sat_worked: self.worked_grid_sat.len(),
            sat_confirmed: self.confirmed_grid_sat.len(),
            // The REAL award standings: VUCC bands only, each against its own threshold.
            awards: VUCC_THRESHOLDS
                .iter()
                .filter(|(b, _)| vucc_wb.contains_key(b))
                .map(|(b, threshold)| {
                    let confirmed = vucc_cb.get(b).map_or(0, |c| c.len());
                    VuccBandStanding {
                        band: (*b).to_string(),
                        worked: vucc_wb.get(b).map_or(0, |w| w.len()),
                        confirmed,
                        threshold: *threshold,
                        achieved: confirmed >= *threshold,
                    }
                })
                .collect(),
        };
        // FFMA — the ARRL's 488 squares, on 6 m only. The per-band grid sets already hold only
        // terrestrial contacts, keyed by the normalised band label.
        let six_w = vucc_wb.get("6m");
        let six_c = vucc_cb.get("6m");
        let mut ffma = FfmaProgress {
            total: crate::ffma::FFMA_GRID_COUNT,
            ..FfmaProgress::default()
        };
        for g in crate::ffma::ffma_grids() {
            let g = g.as_str();
            if six_c.is_some_and(|c| c.contains(g)) {
                ffma.worked += 1;
                ffma.confirmed += 1;
            } else if six_w.is_some_and(|w| w.contains(g)) {
                ffma.worked += 1;
                ffma.unconfirmed.push(g.to_string());
            } else {
                ffma.unworked.push(g.to_string());
            }
        }
        let iota = IotaProgress {
            worked: self.worked_iota.len(),
            confirmed: self.confirmed_iota.len(),
            card_confirmed: self.card_iota.len(),
        };

        // 5-Band DXCC + the WORK chase: per-entity award-band completeness.
        let aw_set: std::collections::HashSet<Band> = AWARD_BANDS.iter().copied().collect();
        let mut worked_aw: HashMap<&'static str, std::collections::HashSet<Band>> = HashMap::new();
        let mut conf_aw: HashMap<&'static str, std::collections::HashSet<Band>> = HashMap::new();
        for (e, b) in &self.worked_slot {
            if aw_set.contains(b) {
                worked_aw.entry(e).or_default().insert(*b);
            }
        }
        for (e, b) in &self.confirmed_slot {
            if aw_set.contains(b) {
                conf_aw.entry(e).or_default().insert(*b);
            }
        }
        // 5BDXCC standing = the WEAKEST award band (100 entities on EACH band
        // independently — different entities allowed per band). The intersection
        // across bands, which this used to compute, understates the award for
        // every real log (N9UM audit, 2026-08-09).
        let five_band_worked = AWARD_BANDS
            .iter()
            .map(|b| self.per_band.get(b).map_or(0, |(w, _)| w.len()))
            .min()
            .unwrap_or(0);
        let five_band_confirmed = AWARD_BANDS
            .iter()
            .map(|b| self.per_band.get(b).map_or(0, |(_, c)| c.len()))
            .min()
            .unwrap_or(0);
        // WORK chase: entities worked on >= MIN but not all award bands; list the
        // award bands NOT yet worked (in canonical order), closest-first.
        let mut band_targets: Vec<EntityNeed> = worked_aw
            .iter()
            .filter(|(_, s)| s.len() >= WORK_CHASE_MIN_BANDS && s.len() < AWARD_BANDS.len())
            .map(|(entity, worked)| EntityNeed {
                entity: entity.to_string(),
                bands: AWARD_BANDS
                    .iter()
                    .filter(|b| !worked.contains(b))
                    .map(|b| b.label().to_string())
                    .collect(),
            })
            .collect();
        // Fewest-missing (closest to 5-band) first, then alphabetical.
        band_targets.sort_by(|a, b| {
            a.bands
                .len()
                .cmp(&b.bands.len())
                .then_with(|| a.entity.cmp(&b.entity))
        });

        let most_wanted = most_wanted_entities();
        let rare_worked = self
            .worked_entity
            .iter()
            .filter(|e| most_wanted.contains(*e))
            .count();

        // DXCC Honor Roll (current-entity, confirmed). Denominator derived from
        // cty.dat (non-WAE entities); ARRL "numerical top ten" ⇒ threshold = N−9.
        let current_total = dxcc::current_dxcc_entities();
        let confirmed_dxcc = self.confirmed_entity.len();
        let threshold = current_total.saturating_sub(9);
        let honor_roll = HonorRollProgress {
            current_total,
            confirmed: confirmed_dxcc,
            threshold,
            achieved: confirmed_dxcc >= threshold && current_total > 0,
            needed: threshold.saturating_sub(confirmed_dxcc),
            number_one: confirmed_dxcc >= current_total && current_total > 0,
            number_one_needed: current_total.saturating_sub(confirmed_dxcc),
        };

        // "DX" = distinct worked entities other than the operator's own, so the
        // "First DX" milestone unlocks on the first genuinely-foreign entity even
        // if no home-entity QSO has been logged yet.
        let dx_entities_worked = self
            .worked_entity
            .iter()
            .filter(|e| self.home_entity != Some(**e))
            .count() as u32;
        let achievements = achievements::evaluate(&AchievementStats {
            qsos: self.qsos as u32,
            confirmed_qsos: self.confirmed_qsos as u32,
            dxcc_worked: self.worked_entity.len() as u32,
            dx_entities_worked,
            dxcc_confirmed: confirmed_dxcc as u32,
            slots_confirmed: self.confirmed_slot.len() as u32,
            rare_worked: rare_worked as u32,
            zones_confirmed: self.confirmed_zones.len() as u32,
            dxcc_current_total: current_total as u32,
            states_confirmed: self.confirmed_states.len() as u32,
        });

        // Worked All States + 5-Band WAS.
        let was_needed: Vec<String> = WAS_STATES
            .iter()
            .filter(|s| !self.confirmed_states.contains(*s))
            .map(|s| s.to_string())
            .collect();
        // 5BWAS, same per-band rule as 5BDXCC: the standing is the weakest award
        // band's state count (all 50 on each band = the award; 50 here means it).
        let states_on = |set: &HashSet<(&'static str, Band)>, b: Band| {
            set.iter().filter(|(_, sb)| *sb == b).count()
        };
        let wap = WapProgress {
            worked: self.worked_provinces.len(),
            confirmed: self.confirmed_provinces.len(),
            needed: WAP_PROVINCES
                .iter()
                .filter(|p| !self.confirmed_provinces.contains(*p))
                .map(|p| p.to_string())
                .collect(),
        };
        let was = WasProgress {
            worked: self.worked_states.len(),
            confirmed: self.confirmed_states.len(),
            needed: was_needed,
            five_band_worked: AWARD_BANDS
                .iter()
                .map(|b| states_on(&self.worked_state_band, *b))
                .min()
                .unwrap_or(0),
            five_band_confirmed: AWARD_BANDS
                .iter()
                .map(|b| states_on(&self.confirmed_state_band, *b))
                .min()
                .unwrap_or(0),
        };

        let band_needs = self.band_needs();

        // Credited (granted) entities + the confirmed-but-not-credited gap.
        let dxcc_credited = self.credited_entity.len();
        let ready_to_submit = self
            .confirmed_entity
            .iter()
            .filter(|e| !self.credited_entity.contains(*e))
            .count();

        AwardSummary {
            qsos: self.qsos,
            confirmed_qsos: self.confirmed_qsos,
            dxcc_worked: self.worked_entity.len(),
            dxcc_confirmed: self.confirmed_entity.len(),
            dxcc_worked_total: self.worked_entity.len() + self.worked_deleted.len(),
            dxcc_confirmed_total: self.confirmed_entity.len() + self.confirmed_deleted.len(),
            dxcc_disagreements: self.disagreements.clone(),
            dxcc_credited,
            ready_to_submit,
            // Challenge counts ONLY the 10 Challenge bands (worked_slot itself keeps every
            // band so a 60m-only new entity still counts for basic DXCC and the per-band view).
            slots_worked: self
                .worked_slot
                .iter()
                .filter(|(_, b)| CHALLENGE_BANDS.contains(b))
                .count(),
            slots_confirmed: self
                .confirmed_slot
                .iter()
                .filter(|(_, b)| CHALLENGE_BANDS.contains(b))
                .count(),
            bands,
            modes,
            needed,
            slot_needed,
            achievements,
            five_band_worked,
            five_band_confirmed,
            sat_dxcc_worked: self.worked_entity_sat.len(),
            sat_dxcc_confirmed: self.confirmed_entity_sat.len(),
            waz_worked: self.worked_zones.len(),
            waz_confirmed: self.confirmed_zones.len(),
            honor_roll,
            was,
            wap,
            vucc,
            ffma,
            iota,
            wpx: self.more.wpx(),
            wae: self.more.wae(),
            wwff: self.more.wwff(),
            band_targets,
            band_needs,
        }
    }

    /// The WPX prefixes and WAE countries worked on each band in each mode class, for the Needed
    /// board's opt-in chips ([`crate::awards_more::mark_award_needs`]).
    pub fn wpx_wae_worked(&self) -> WpxWaeWorked {
        self.more.slots_worked.clone()
    }

    /// The whole-log DXCC chart: every current entity × every DXCC band, by mode class
    /// ([`DxccChart`]), from the same slots the DXCC counts above are made of.
    pub fn dxcc_chart(&self) -> DxccChart {
        self.more.chart()
    }

    /// What to QSL ([`QslWant`]): the unconfirmed contacts whose confirmation would fill a slot
    /// an award still needs, most valuable first. Each slot keeps its newest
    /// [`WANTS_PER_SLOT`](crate::awards_more::WANTS_PER_SLOT) contacts.
    pub fn what_to_qsl(&self) -> Vec<QslWant> {
        self.more.wants(|slot| match slot {
            SlotView::Entity(e) => self.confirmed_entity.contains(e),
            SlotView::EntityBand(e, b) => self.confirmed_slot.contains(&(*e, *b)),
            SlotView::Zone(z) => self.confirmed_zones.contains(z),
            SlotView::State(s) => self.confirmed_states.contains(s),
            SlotView::Province(p) => self.confirmed_provinces.contains(p),
            SlotView::Grid(g, b) => self
                .confirmed_grid_band
                .contains(&((*g).to_string(), (*b).to_string())),
        })
    }

    /// DXCC, WAS and WAZ band by band ([`AwardBandNeeds`]): each band's never-worked and
    /// worked-not-confirmed, from the slots the fold above filled — the DXCC ones of current
    /// entities only, as every DXCC count here is.
    fn band_needs(&self) -> AwardBandNeeds {
        fn need<T: Copy>(
            band: Band,
            all: impl Iterator<Item = T>,
            worked: impl Fn(T) -> bool,
            confirmed: impl Fn(T) -> bool,
            name: impl Fn(T) -> String,
        ) -> BandNeed {
            let (mut unworked, mut unconfirmed) = (Vec::new(), Vec::new());
            for key in all {
                if !worked(key) {
                    unworked.push(name(key));
                } else if !confirmed(key) {
                    unconfirmed.push(name(key));
                }
            }
            BandNeed {
                band: band.label().to_string(),
                unworked,
                unconfirmed,
            }
        }
        let entities = dxcc::dxcc_entity_names();
        let none = (HashSet::new(), HashSet::new());
        AwardBandNeeds {
            dxcc: Band::ALL
                .into_iter()
                .filter(|b| fills_dxcc_band_slot(*b))
                .map(|b| {
                    let (w, c) = self.per_band.get(&b).unwrap_or(&none);
                    need(
                        b,
                        entities.iter().copied(),
                        |e| w.contains(e),
                        |e| c.contains(e),
                        str::to_string,
                    )
                })
                .collect(),
            was: Band::ALL
                .into_iter()
                .map(|b| {
                    need(
                        b,
                        WAS_STATES.into_iter(),
                        |s| self.worked_state_band.contains(&(s, b)),
                        |s| self.confirmed_state_band.contains(&(s, b)),
                        str::to_string,
                    )
                })
                .collect(),
            waz: Band::ALL
                .into_iter()
                .map(|b| {
                    need(
                        b,
                        1..=40u8,
                        |z| self.worked_zone_band.contains(&(z, b)),
                        |z| self.confirmed_zone_band.contains(&(z, b)),
                        |z| z.to_string(),
                    )
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dxcc_worked_confirmed_slots_and_chase() {
        // Resolve expected entity names dynamically so the test doesn't hardcode
        // cty.dat's exact strings.
        let usa = dxcc::resolve("W1AW").unwrap().entity;
        let japan = dxcc::resolve("JA1XYZ").unwrap().entity;
        let germany = dxcc::resolve("DL1ABC").unwrap().entity;
        assert_ne!(usa, japan);

        let mut a = Awards::new();
        a.add("W1AW", "20m", "CW", true); // USA, 20m CW, confirmed
        a.add("K1ABC", "40m", "SSB", false); // USA again, 40m phone, unconfirmed
        a.add("JA1XYZ", "20m", "FT8", false); // Japan, 20m digital, unconfirmed
        a.add("DL1ABC", "20m", "CW", true); // Germany, 20m CW, confirmed
        let s = a.summary();

        assert_eq!(s.qsos, 4);
        assert_eq!(s.confirmed_qsos, 2);
        assert_eq!(s.dxcc_worked, 3, "USA, Japan, Germany");
        assert_eq!(s.dxcc_confirmed, 2, "USA + Germany confirmed");
        assert_eq!(s.slots_worked, 4, "(USA,20),(USA,40),(JA,20),(DL,20)");
        assert_eq!(s.slots_confirmed, 2, "(USA,20),(DL,20)");

        // WAZ: W1/K1 → CQ 5, JA → CQ 25, DL → CQ 14 ⇒ 3 zones worked; only the
        // confirmed contacts (W1AW=5, DL1ABC=14) count as confirmed zones.
        assert_eq!(s.waz_worked, 3, "zones 5 (US), 25 (JA), 14 (DL)");
        assert_eq!(s.waz_confirmed, 2, "zones 5 + 14 confirmed");

        // Only Japan is a "new one" chase — USA is already confirmed even though
        // its 40m slot is unconfirmed (that's a Challenge need, not a new entity).
        assert_eq!(s.needed.len(), 1);
        assert_eq!(s.needed[0].entity, japan);
        assert_eq!(s.needed[0].bands, vec!["20m".to_string()]);

        // The Challenge-slot chase: USA is confirmed (20m) but its 40m slot is
        // worked-unconfirmed → a slot need. Germany has no unconfirmed slot.
        assert_eq!(s.slot_needed.len(), 1, "only USA needs a Challenge slot");
        assert_eq!(s.slot_needed[0].entity, usa);
        assert_eq!(s.slot_needed[0].bands, vec!["40m".to_string()]);
        let _ = germany;

        // 20m band: worked USA/Japan/Germany = 3; confirmed USA/Germany = 2.
        let b20 = s.bands.iter().find(|b| b.band == "20m").unwrap();
        assert_eq!(b20.worked, 3);
        assert_eq!(b20.confirmed, 2);
        // Bands are 160→2 ordered: 40m comes after 20m? No — 40m is lower band,
        // so 40m precedes 20m in 160→2 order.
        let order: Vec<&str> = s.bands.iter().map(|b| b.band.as_str()).collect();
        assert_eq!(order, vec!["40m", "20m"]);

        // Per-mode DXCC: CW worked USA+Germany=2 (both confirmed); Phone worked USA=1 (0 conf).
        let cw = s.modes.iter().find(|m| m.mode == "CW").unwrap();
        assert_eq!((cw.worked, cw.confirmed), (2, 2));
        let phone = s.modes.iter().find(|m| m.mode == "Phone").unwrap();
        assert_eq!((phone.worked, phone.confirmed), (1, 0));
    }

    #[test]
    fn sixty_metres_earns_no_dxcc_credit_at_all() {
        // ⭐ 60 m IS NOT A DXCC BAND, AND THIS USED TO ASSERT THE OPPOSITE. The old test was
        // named `challenge_slots_exclude_60m_2m_4m_but_dxcc_still_counts_them` and it grouped
        // three bands that are excluded for three DIFFERENT reasons, then asserted one rule for
        // all of them:
        //   2 m  — has its OWN DXCC award and counts toward Mixed. Not a CHALLENGE band. ✓ was right.
        //   60 m — earns NO ARRL DXCC credit whatsoever: not Mixed, not per-band, not per-mode.
        //   4 m  — not an ARRL award band at all (no US allocation), so likewise none.
        // Only the 2 m half of that grouping was correct.
        //
        // The consequence of getting it wrong is not cosmetic: an operator with 60 m contacts saw
        // an INFLATED DXCC total and could believe an entity was confirmed when ARRL will never
        // credit it. Reported by a field user via the DXpedition board, 2026-08-05.
        let mut a = Awards::new();
        a.add("W1AW", "20m", "CW", true); // USA / 20m — real DXCC credit
        a.add("JA1XYZ", "60m", "FT8", true); // Japan / 60m — NO credit of any kind
        a.add("DL1ABC", "2m", "SSB", true); // Germany / 2m — DXCC yes, Challenge no
        let s = a.summary();

        // Japan was worked ONLY on 60 m, so it must not appear as a DXCC entity anywhere.
        assert_eq!(
            s.dxcc_worked, 2,
            "60 m must not add a DXCC entity (USA + Germany only)"
        );
        assert_eq!(s.dxcc_confirmed, 2);
        assert!(
            !s.bands.iter().any(|b| b.band == "60m"),
            "60 m must not appear in the per-band DXCC breakdown at all"
        );
        // 2 m is unaffected — it has its own DXCC award; it is only the CHALLENGE it is outside.
        assert!(s.bands.iter().any(|b| b.band == "2m" && b.worked == 1));
        assert_eq!(s.slots_worked, 1, "only (USA,20m) is a Challenge slot");
        assert_eq!(s.slots_confirmed, 1);
    }

    #[test]
    fn a_sixty_metre_contact_still_counts_as_a_qso_and_for_non_arrl_awards() {
        // The exclusion is DXCC-specific and must not silently delete the contact. The QSO
        // happened; CQ's WAZ and RSGB's IOTA are different award programmes with their own
        // rules, and this fix deliberately does not touch them.
        let mut a = Awards::new();
        a.add("JA1XYZ", "60m", "FT8", true);
        let s = a.summary();
        assert_eq!(s.qsos, 1, "the contact is still a logged QSO");
        assert_eq!(s.dxcc_worked, 0, "…but earns no DXCC entity");
        assert!(
            s.waz_worked > 0,
            "CQ WAZ is a different programme — untouched"
        );
    }

    #[test]
    fn five_band_dxcc_and_work_chase() {
        let usa = dxcc::resolve("W1AW").unwrap().entity;
        let japan = dxcc::resolve("JA1XYZ").unwrap().entity;

        let mut a = Awards::new();
        // USA worked on 4 of the 5 award bands (confirmed 80/40/20, 15m worked).
        a.add("W1AW", "80m", "FT8", true);
        a.add("W1AW", "40m", "FT8", true);
        a.add("W1AW", "20m", "FT8", true);
        a.add("W1AW", "15m", "FT8", false);
        // Japan worked + confirmed on all 5 award bands.
        for b in ["80m", "40m", "20m", "15m", "10m"] {
            a.add("JA1XYZ", b, "FT8", true);
        }
        let s = a.summary();

        assert_eq!(s.five_band_worked, 1, "only Japan worked on all 5");
        assert_eq!(s.five_band_confirmed, 1, "only Japan confirmed on all 5");

        // USA is worked on 4 award bands (≥3, <5) → WORK chase, missing 10m only.
        let usa_t = s.band_targets.iter().find(|t| t.entity == usa).unwrap();
        assert_eq!(
            usa_t.bands,
            vec!["10m".to_string()],
            "work USA on 10m for 5-band"
        );
        // Japan is complete → not a target.
        assert!(!s.band_targets.iter().any(|t| t.entity == japan));
    }

    /// Sicily (IT9) is a WAE/CQ-only area, NOT an ARRL DXCC entity of its own: the ARRL credits it
    /// as Italy, the entity whose number cty.csv gives it. (This test used to pin no credit at all,
    /// which left every Sicily contact out of the DXCC count.)
    #[test]
    fn a_wae_area_counts_as_its_parent_entity() {
        assert!(!dxcc::resolve("IT9ABC").unwrap().is_dxcc);
        let mut a = Awards::new();
        a.add("IT9ABC", "20m", "FT8", true);
        let s = a.summary();
        assert_eq!(s.qsos, 1, "the QSO counts");
        assert_eq!(s.dxcc_worked, 1, "as Italy");
        assert_eq!(s.dxcc_confirmed, 1);
        assert_eq!(s.slots_worked, 1, "and its band slot");
        assert_eq!(s.waz_worked, 1, "CQ zone 15 counts for WAZ");
        assert_eq!(s.waz_confirmed, 1);
        assert_eq!(
            s.bands
                .iter()
                .map(|b| (b.band.as_str(), b.worked))
                .collect::<Vec<_>>(),
            [("20m", 1)]
        );
        // The SAME entity as an Italian call, never a second one beside it.
        a.add("I1ABC", "20m", "FT8", true);
        assert_eq!(
            a.summary().dxcc_confirmed,
            1,
            "Sicily and Italy are one entity"
        );

        // Named Italy where an entity is named, and its own zone kept: African Italy is CQ zone
        // 33, not Italy's 15.
        let mut b = Awards::new();
        b.add("IG9ABC", "20m", "FT8", false);
        let s = b.summary();
        assert_eq!(
            s.needed
                .iter()
                .map(|n| n.entity.as_str())
                .collect::<Vec<_>>(),
            ["Italy"]
        );
        b.add("I1ABC", "20m", "FT8", false);
        assert_eq!(b.summary().waz_worked, 2, "zones 33 and 15");
    }

    #[test]
    fn credited_tier_and_ready_to_submit() {
        let mut a = Awards::new();
        a.add_with_credit("W1AW", "20m", "CW", true, true, None, None, None); // confirmed + DXCC credited
        a.add_with_credit("JA1XYZ", "20m", "FT8", true, false, None, None, None); // confirmed, not credited
        a.add_with_credit("DL1ABC", "20m", "CW", false, false, None, None, None); // worked only
        let s = a.summary();
        assert_eq!(s.dxcc_worked, 3);
        assert_eq!(s.dxcc_confirmed, 2, "USA + Japan confirmed");
        assert_eq!(s.dxcc_credited, 1, "only USA is credited");
        assert_eq!(s.ready_to_submit, 1, "Japan: confirmed but not credited");
        // plain add() never credits.
        let mut b = Awards::new();
        b.add("W1AW", "20m", "CW", true);
        assert_eq!(b.summary().dxcc_credited, 0);
        assert_eq!(b.summary().ready_to_submit, 1);

        // credited without a confirmation must NOT inflate credited (credited ≤
        // confirmed always holds — a malformed credit-only report row).
        let mut c = Awards::new();
        c.add_with_credit("W1AW", "20m", "CW", false, true, None, None, None);
        let s = c.summary();
        assert_eq!(s.dxcc_confirmed, 0);
        assert_eq!(s.dxcc_credited, 0, "credit without confirmation is ignored");
    }

    #[test]
    fn wap_provinces_confirmed_needed_and_gated_on_canada() {
        let mut a = Awards::new();
        // Four provinces confirmed across the prefix families that all resolve to Canada:
        // VE (mainland), VO (Newfoundland), VY (the territories/PEI), VA (the second block).
        a.add_with_credit("VE3ABC", "20m", "FT8", true, false, Some("ON"), None, None);
        a.add_with_credit("VO1AA", "20m", "CW", true, false, Some("nl"), None, None); // lowercase ok
        a.add_with_credit("VY1AB", "20m", "FT8", true, false, Some("YT"), None, None);
        a.add_with_credit("VA7XYZ", "20m", "FT8", true, false, Some("BC"), None, None);
        // Worked but not confirmed — counts as worked, still needed.
        a.add_with_credit("VE6QQ", "20m", "FT8", false, false, Some("AB"), None, None);
        // Junk subdivision code → ignored entirely.
        a.add_with_credit("VE2ZZ", "20m", "FT8", true, false, Some("XX"), None, None);

        // ⭐ THE COLLISION THE ENTITY GATE EXISTS FOR. "NT" is Northwest Territories in Canada
        // and Australia's NORTHERN TERRITORY in VK. Without gating on the DXCC entity, this VK8
        // contact would silently complete a Canadian province the operator has never worked —
        // the same hazard WAS carries with Australian "WA" against Washington.
        a.add_with_credit("VK8AA", "20m", "FT8", true, false, Some("NT"), None, None);

        let s = a.summary();
        assert_eq!(s.wap.confirmed, 4, "ON, NL, YT, BC — VK8/NT rejected");
        assert_eq!(s.wap.worked, 5, "+ AB worked but unconfirmed");
        assert_eq!(s.wap.needed.len(), 9, "13 − 4 confirmed");
        assert!(!s.wap.needed.contains(&"ON".to_string()));
        assert!(
            s.wap.needed.contains(&"AB".to_string()),
            "worked is not confirmed"
        );
        assert!(
            s.wap.needed.contains(&"NT".to_string()),
            "Northwest Territories must still be needed after the VK8 contact"
        );
    }

    /// Credit comes from the logged STATE, never from the callsign's regional numeral. A
    /// licensee who moves keeps the old call, so the numeral labels a roster row and must not
    /// decide an award — the rule `province.rs` states and this pins.
    #[test]
    fn wap_credit_comes_from_the_log_not_the_callsign_numeral() {
        let mut a = Awards::new();
        // A VE3 (Ontario numeral) whose log says Alberta — the operator moved and kept the call.
        a.add_with_credit("VE3ABC", "20m", "FT8", true, false, Some("AB"), None, None);
        let s = a.summary();
        assert!(
            s.wap.needed.contains(&"ON".to_string()),
            "the ON numeral must not have credited Ontario"
        );
        assert!(
            !s.wap.needed.contains(&"AB".to_string()),
            "the LOG said Alberta"
        );

        // And a Canadian contact with no STATE logged credits nothing at all.
        let mut b = Awards::new();
        b.add_with_credit("VE7XYZ", "20m", "FT8", true, false, None, None, None);
        assert_eq!(b.summary().wap.confirmed, 0, "no STATE, no credit");
    }

    #[test]
    fn was_states_confirmed_needed_and_five_band() {
        let mut a = Awards::new();
        // 3 states confirmed (incl. Hawaii/Alaska — separate DXCC entities), one
        // worked-but-unconfirmed, plus a junk STATE that must be ignored.
        a.add_with_credit("KH6AA", "20m", "FT8", true, false, Some("HI"), None, None);
        a.add_with_credit("KL7AA", "20m", "FT8", true, false, Some("ak"), None, None); // lowercase ok
        a.add_with_credit("W1AW", "20m", "CW", true, false, Some("CT"), None, None);
        a.add_with_credit("K5XYZ", "20m", "FT8", false, false, Some("TX"), None, None); // worked, unconf
        a.add_with_credit("DL1ABC", "20m", "FT8", true, false, Some("ZZ"), None, None); // junk → ignored
                                                                                        // Australian "WA" (Western Australia) must NOT credit Washington — WAS is
                                                                                        // gated on a US-family entity, so a VK6 contact never advances WA.
        a.add_with_credit("VK6AA", "20m", "FT8", true, false, Some("WA"), None, None);
        let s = a.summary();
        assert_eq!(s.was.confirmed, 3, "HI, AK, CT confirmed (VK6/WA rejected)");
        assert_eq!(s.was.worked, 4, "+ TX worked");
        assert_eq!(s.was.needed.len(), 47, "50 − 3 confirmed");
        assert!(!s.was.needed.contains(&"HI".to_string()));
        assert!(s.was.needed.contains(&"TX".to_string()));
        assert!(
            s.was.needed.contains(&"WA".to_string()),
            "Washington still needed"
        );

        // 5BWAS: Rhode Island confirmed on all 5 award bands.
        let mut b = Awards::new();
        for band in ["80m", "40m", "20m", "15m", "10m"] {
            b.add_with_credit("W1RI", band, "FT8", true, false, Some("RI"), None, None);
        }
        let s = b.summary();
        assert_eq!(s.was.five_band_confirmed, 1, "RI on all 5 bands");
    }

    #[test]
    fn vucc_counts_distinct_grids_per_band_and_rejects_junk() {
        let mut a = Awards::new();
        a.add_with_credit("K1ABC", "6m", "FT8", true, false, None, Some("FN31"), None); // confirmed
        a.add_with_credit("K1ABC", "6m", "FT8", false, false, None, Some("FN31"), None); // dup grid
        a.add_with_credit("W2DEF", "6m", "FT8", false, false, None, Some("FN20"), None);
        a.add_with_credit("K1ABC", "2m", "FT8", false, false, None, Some("FN31"), None); // same grid, new band
        a.add_with_credit(
            "W3GHI",
            "6m",
            "FT8",
            false,
            false,
            None,
            Some("FN31aa"),
            None,
        ); // 6-char → FN31 square
        a.add_with_credit(
            "W4JKL",
            "6m",
            "FT8",
            false,
            false,
            None,
            Some("garbage"),
            None,
        ); // rejected
        let s = a.summary();
        assert_eq!(
            s.vucc.worked, 2,
            "FN31 + FN20 = 2 distinct grids overall (FN31 not double-counted across bands)"
        );
        assert_eq!(s.vucc.confirmed, 1, "only FN31 was confirmed");
        let six = s.vucc.bands.iter().find(|b| b.band == "6m").unwrap();
        assert_eq!(
            (six.worked, six.confirmed),
            (2, 1),
            "6m: FN31 + FN20, one confirmed"
        );
        let two = s.vucc.bands.iter().find(|b| b.band == "2m").unwrap();
        assert_eq!(two.worked, 1, "2m: FN31 only");
    }

    /// ARRL 5BDXCC (N9UM audit, 2026-08-09): the award is 100 entities confirmed on EACH
    /// of the five bands INDEPENDENTLY — different entities allowed per band — so the
    /// standing is the WEAKEST band's count, not the intersection across bands. Five
    /// different entities, one per band: the old intersection said 0; ARRL says 1.
    #[test]
    fn five_band_standing_is_the_weakest_band_not_the_intersection() {
        let mut a = Awards::new();
        for (call, b) in [
            ("W1AW", "80m"),
            ("JA1XYZ", "40m"),
            ("G4ABC", "20m"),
            ("VK2DEF", "15m"),
            ("PY2GHI", "10m"),
        ] {
            a.add(call, b, "FT8", true);
        }
        let s = a.summary();
        assert_eq!(
            s.five_band_confirmed, 1,
            "one entity confirmed on every band independently = standing 1"
        );
        assert_eq!(s.five_band_worked, 1);
        // A band with nothing at all pins the standing to zero.
        let mut b = Awards::new();
        b.add("W1AW", "80m", "FT8", true);
        b.add("W1AW", "40m", "FT8", true);
        assert_eq!(b.summary().five_band_confirmed, 0, "three empty bands → 0");
    }

    /// Same rule for 5-Band WAS: the standing is the weakest award band's state count.
    #[test]
    fn five_band_was_standing_is_the_weakest_band() {
        let mut a = Awards::new();
        // One state per band, five different states — intersection 0, ARRL standing 1.
        for (call, st, b) in [
            ("W1AW", "CT", "80m"),
            ("W2XYZ", "NY", "40m"),
            ("W3ABC", "PA", "20m"),
            ("W4DEF", "GA", "15m"),
            ("W5GHI", "TX", "10m"),
        ] {
            a.add_with_credit(call, b, "FT8", true, false, Some(st), None, None);
        }
        assert_eq!(a.summary().was.five_band_confirmed, 1);
    }

    /// ARRL restricts satellite contacts to the SATELLITE awards: a SAT-tagged QSO must
    /// not enter Mixed/band/mode DXCC (the tester's 2m band-DXCC counted a bird QSO LoTW
    /// itself refuses there) — it feeds the Satellite DXCC bucket instead.
    #[test]
    fn satellite_qsos_feed_satellite_dxcc_not_band_dxcc() {
        let mut a = Awards::new();
        a.add_qso(
            "JA1XYZ",
            "2m",
            "FT8",
            true,
            false,
            false,
            None,
            Some("PM95"),
            None,
            true,
            RecordFields::default(),
        );
        let s = a.summary();
        assert_eq!(s.dxcc_worked, 0, "a sat QSO earns no Mixed-DXCC entity");
        assert!(
            !s.bands.iter().any(|b| b.band == "2m"),
            "…and no 2m band-DXCC slot: {:?}",
            s.bands
        );
        assert!(s.modes.is_empty(), "…and no mode-DXCC entity");
        assert_eq!(
            (s.sat_dxcc_worked, s.sat_dxcc_confirmed),
            (1, 1),
            "it stands in the Satellite DXCC bucket, like LoTW's Satellite column"
        );
    }

    /// A terrestrial 70/33/23 cm contact counts toward the DXCC entity and its mode, as it did
    /// before those bands had a [`Band`] variant, and fills no DXCC band slot: no per-band row,
    /// no Challenge slot, no needed-by-band row — the ARRL keeps no DXCC award there.
    #[test]
    fn a_uhf_contact_counts_the_entity_and_fills_no_dxcc_band_slot() {
        for band in ["70cm", "33cm", "23cm"] {
            let mut a = Awards::new();
            a.add("DL1ABC", band, "FT8", true);
            let s = a.summary();
            assert_eq!(
                (s.dxcc_worked, s.dxcc_confirmed),
                (1, 1),
                "{band}: the entity counts"
            );
            assert_eq!(s.modes.len(), 1, "{band}: the mode counts");
            assert!(
                s.bands.is_empty(),
                "{band}: no DXCC band row: {:?}",
                s.bands
            );
            assert_eq!((s.slots_worked, s.slots_confirmed), (0, 0), "{band}");
            assert!(
                !s.band_needs.dxcc.iter().any(|n| n.band == band),
                "{band}: no DXCC needed-by-band row"
            );
            assert!(
                s.band_needs.waz.iter().any(|n| n.band == band),
                "{band}: WAZ lists every band Nexus names"
            );
        }
    }

    /// The Fred Fish Memorial Award counts the 488 squares of the 48 states on 6 m only:
    /// worked and confirmed apart, a 2 m or 20 m contact in one of them earns nothing, a 6 m
    /// contact outside them (Hawaii, Alaska, Puerto Rico) earns nothing, a satellite contact
    /// earns nothing, and the needed lists say which squares are left and why.
    #[test]
    fn ffma_counts_the_488_squares_on_6m_only() {
        let mut a = Awards::new();
        let mut qso = |call: &str, band: &str, grid: &str, confirmed: bool, sat: bool| {
            a.add_qso(
                call,
                band,
                "FT8",
                confirmed,
                false,
                false,
                None,
                Some(grid),
                None,
                sat,
                RecordFields::default(),
            )
        };
        qso("W1AW", "6m", "FN31pr", true, false); // confirmed, subsquare dropped
        qso("K1ABC", "6m", "FN31", false, false); // same square again, unconfirmed
        qso("W9XYZ", "6m", "EN52", false, false); // worked, not confirmed
        qso("K2ABC", "2m", "FN20", true, false); // on the list, wrong band
        qso("K3ABC", "20m", "FN21", true, false); // on the list, wrong band
        qso("KH6ABC", "6m", "BL11", true, false); // 6 m, outside the 48
        qso("KP4ABC", "6m", "FK68", true, false); // 6 m, outside the 48
        qso("K4ABC", "6m", "EM73", true, true); // via satellite
        let f = a.summary().ffma;
        assert_eq!(f.total, 488);
        assert_eq!(
            (f.worked, f.confirmed),
            (2, 1),
            "FN31 confirmed, EN52 worked"
        );
        assert_eq!(f.unconfirmed, ["EN52"]);
        assert_eq!(f.unworked.len(), 488 - 2);
        for g in ["FN20", "FN21", "EM73"] {
            assert!(
                f.unworked.iter().any(|u| u == g),
                "{g} is still unworked on 6 m"
            );
        }
        assert!(!f.unworked.iter().any(|u| u == "FN31" || u == "EN52"));
        assert!(!f.unworked.iter().any(|u| u == "BL11" || u == "FK68"));
    }

    /// An empty log still lists all 488 as needed, so the operator sees the whole chase.
    #[test]
    fn ffma_with_no_6m_contacts_needs_every_square() {
        let f = Awards::new().summary().ffma;
        assert_eq!((f.worked, f.confirmed, f.total), (0, 0, 488));
        assert_eq!(f.unworked.len(), 488);
        assert!(f.unconfirmed.is_empty());
    }

    /// A terrestrial 70cm grid is a real ARRL VUCC band (threshold 50) — it must count
    /// in the per-band listing and the all-band total. (It earns no DXCC band slot — the
    /// ARRL keeps no DXCC award on 70 cm, see [`fills_dxcc_band_slot`] — and that fold is
    /// separate.)
    #[test]
    fn terrestrial_uhf_grids_count_for_vucc() {
        let mut a = Awards::new();
        a.add_qso(
            "K1ABC",
            "70cm",
            "FT8",
            true,
            false,
            false,
            None,
            Some("FN31"),
            None,
            false,
            RecordFields::default(),
        );
        let s = a.summary();
        assert_eq!(s.vucc.worked, 1, "the all-band total sees the 70cm grid");
        let row = s.vucc.bands.iter().find(|b| b.band == "70cm");
        assert_eq!(
            row.map(|b| (b.worked, b.confirmed)),
            Some((1, 1)),
            "70cm appears in the per-band grid listing"
        );
    }

    /// The real per-band VUCC standing: 6m needs 100, 70cm needs 50; an achieved band
    /// reports achieved with its own threshold.
    #[test]
    fn vucc_standing_is_per_band_with_arrl_thresholds() {
        let mut a = Awards::new();
        // 100 distinct confirmed 6m grids → 6m VUCC achieved.
        for i in 0..100 {
            let g = format!("EN{}{}", i / 10, i % 10);
            a.add_qso(
                "K1ABC",
                "6m",
                "FT8",
                true,
                false,
                false,
                None,
                Some(&g),
                None,
                false,
                RecordFields::default(),
            );
        }
        let s = a.summary();
        let six = s.vucc.awards.iter().find(|x| x.band == "6m").unwrap();
        assert_eq!(
            (six.threshold, six.confirmed, six.achieved),
            (100, 100, true)
        );
        // HF grids never appear in the standings list (VUCC is 50 MHz and up).
        assert!(
            !s.vucc.awards.iter().any(|x| x.band == "20m"),
            "20m is not a VUCC band"
        );
    }

    /// IOTA credits LoTW-matched contacts as well as QSL cards: a LoTW-only confirmation and a
    /// card both count toward the award; the card count is kept beside it.
    #[test]
    fn iota_award_counts_lotw_and_card_confirmations() {
        let mut a = Awards::new();
        // LoTW-confirmed (card_confirmed = false).
        a.add_qso(
            "K1ABC",
            "20m",
            "FT8",
            true,
            false,
            false,
            None,
            None,
            Some("NA-001"),
            false,
            RecordFields::default(),
        );
        // Card-confirmed.
        a.add_qso(
            "W2DEF",
            "40m",
            "CW",
            true,
            false,
            true,
            None,
            None,
            Some("EU-005"),
            false,
            RecordFields::default(),
        );
        let s = a.summary();
        assert_eq!(s.iota.confirmed, 2, "LoTW and card both count toward IOTA");
        assert_eq!(s.iota.card_confirmed, 1, "one of them is on a card");
    }

    #[test]
    fn satellite_qsos_feed_satellite_vucc_only() {
        let mut a = Awards::new();
        // Satellite QSOs (PROP_MODE=SAT): grids count toward Satellite VUCC —
        // on 70cm too, band-independent — and NOT toward
        // the per-band awards (ARRL counts satellite contacts toward the
        // Satellite VUCC only).
        a.add_qso(
            "K1ABC",
            "2m",
            "FM",
            true,
            false,
            false,
            None,
            Some("FN31"),
            None,
            true,
            RecordFields::default(),
        );
        a.add_qso(
            "W2DEF",
            "70cm",
            "SSB",
            false,
            false,
            false,
            None,
            Some("FN20"),
            None,
            true,
            RecordFields::default(),
        );
        // A terrestrial 2m QSO keeps the per-band slot.
        a.add_qso(
            "W3GHI",
            "2m",
            "FT8",
            false,
            false,
            false,
            None,
            Some("FN42"),
            None,
            false,
            RecordFields::default(),
        );
        let s = a.summary();
        assert_eq!(
            (s.vucc.sat_worked, s.vucc.sat_confirmed),
            (2, 1),
            "FN31 (confirmed) + FN20 via satellite — the 70cm one included"
        );
        let two = s.vucc.bands.iter().find(|b| b.band == "2m").unwrap();
        assert_eq!(
            two.worked, 1,
            "2m per-band VUCC holds only the terrestrial FN42; sat QSOs excluded"
        );
        assert_eq!(
            s.vucc.worked, 1,
            "the terrestrial overall count doesn't absorb satellite grids"
        );
    }

    #[test]
    fn iota_counts_distinct_island_groups_worked_and_confirmed() {
        let mut a = Awards::new();
        a.add_with_credit(
            "GM0ABC",
            "20m",
            "CW",
            true,
            false,
            None,
            None,
            Some("EU-008"),
        ); // confirmed
        a.add_with_credit(
            "GM1DEF",
            "40m",
            "FT8",
            false,
            false,
            None,
            None,
            Some("EU-008"),
        ); // same group
        a.add_with_credit(
            "KH6XYZ",
            "20m",
            "FT8",
            false,
            false,
            None,
            None,
            Some("OC-019"),
        );
        a.add_with_credit("W1AW", "20m", "CW", true, false, None, None, None); // no island
        let s = a.summary();
        assert_eq!(s.iota.worked, 2, "EU-008 + OC-019 distinct groups");
        assert_eq!(s.iota.confirmed, 1, "only EU-008 was confirmed");
    }

    #[test]
    fn honor_roll_progress_against_current_total() {
        let s = Awards::new().summary();
        let total = s.honor_roll.current_total;
        assert_eq!(total, 340, "current DXCC entities (cty.dat non-WAE)");
        assert_eq!(s.honor_roll.threshold, total - 9, "top ten ⇒ 331");
        assert!(!s.honor_roll.achieved && !s.honor_roll.number_one);
        // From an empty log: need the whole threshold for HR, the whole total for #1.
        assert_eq!(s.honor_roll.needed, total - 9);
        assert_eq!(s.honor_roll.number_one_needed, total);

        // One confirmed DXCC entity decrements both "needed" counts by one.
        let mut a = Awards::new();
        a.add("W1AW", "20m", "CW", true);
        let s = a.summary();
        assert_eq!(s.honor_roll.confirmed, 1);
        assert_eq!(s.honor_roll.needed, total - 9 - 1);
        assert_eq!(s.honor_roll.number_one_needed, total - 1);
    }

    #[test]
    fn working_a_most_wanted_entity_unlocks_dxpedition_achievement() {
        // Pick a most-wanted call that cty.dat actually resolves (and assert at
        // least one does — catches a stale prefix list).
        let mw = MOST_WANTED_CALLS
            .iter()
            .copied()
            .find(|c| dxcc::resolve(c).is_some())
            .expect("at least one most-wanted call resolves in cty.dat");

        let mut a = Awards::new();
        a.add("W1AW", "20m", "FT8", true); // a common entity — not most-wanted
        let before = a.summary();
        assert!(
            !before
                .achievements
                .iter()
                .any(|x| x.id == "rare-1" && x.unlocked),
            "a common entity is not a DXpedition contact"
        );

        a.add(mw, "20m", "FT8", false);
        let after = a.summary();
        let rare = after
            .achievements
            .iter()
            .find(|x| x.id == "rare-1")
            .unwrap();
        assert!(
            rare.unlocked,
            "working most-wanted {mw} unlocks DXpedition Contact"
        );
        assert!(rare.current >= 1);
    }
}
