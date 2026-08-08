//! Station-wide state — the half of the engine that belongs to the OPERATOR, not to a radio.
//!
//! [`Engine`](crate::engine::Engine) mixes two kinds of state: things that are true of the
//! STATION (one logbook, one ADIF file, one connector-upload funnel, one DXCC table, one
//! POTA activation, one PC clock) and things that are true of ONE RECEIVE/TRANSMIT CHAIN
//! (this waterfall, this slot clock, this dial, this TX queue). Only the second kind can
//! meaningfully exist more than once.
//!
//! `StationCore` is the first kind, lifted out verbatim. Today [`Engine`](crate::engine::Engine)
//! owns exactly one by value and the chain count is hard-capped at one, so this is a pure
//! relocation with no behavior change — the point is that the seam now EXISTS and the
//! compiler enforces which side each field is on. When a second chain arrives, N engines
//! share one core instead of each growing a divergent copy of the operator's log.
//!
//! What is deliberately NOT here: `settings`, `app` (identity/roster/conversations),
//! `pending_log`, `highlights`, `clear_tick`, `work_tick`, `broker_ptt` and `radio_live`.
//! Each is genuinely both-sided and needs a design ruling, not a default; they stay on
//! [`Engine`](crate::engine::Engine) untouched.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use tempo_core::logbook::{Logbook, QsoRecord};

use crate::engine::{
    now_unix_secs, LotwResolver, PendingUpload, HUNT_TTL_SECS, MAX_UPLOAD_RETRIES, SSTV_GALLERY_CAP,
};

/// The shared `log.adi`'s freshness fingerprint — `(mtime, byte length)`, or `None`
/// if it cannot be statted. See [`StationCore::last_log_mtime`] for why the length
/// rides along; `None` never gates anything, because the recovery must never skip on
/// uncertainty.
fn log_file_stamp(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok().map(|t| (t, m.len())))
}

/// Canonical band key for the per-band worked indices.
///
/// Lower-cased, because the band spellings that actually reach the log differ by
/// source (Nexus writes "20m", a LoTW export "20M", and `parse_adif` passes BAND
/// through verbatim), and with the band-plan's FM channel suffix stripped
/// ("2m-fm" → "2m") since FM on 2 m is still 2 m for award purposes. The result is
/// byte-identical to `propagation::Band::label()` for every band that crate models,
/// so this side and the awards/needs side agree on what "2 m" is without tempo-app
/// having to depend on the propagation crate (it deliberately does not — see
/// [`StationCore::set_dxcc_resolver`]). A record with no BAND keys as "", which
/// matches no live band and so never suppresses a need.
fn band_key(band: &str) -> String {
    // THE canonicaliser (bandplan::canonical_band), then lower-cased: the old
    // hand-rolled strip here handled only "-fm", so a legacy "6m-2"/"2m-call"
    // row keyed as its own phantom band and never suppressed a need.
    crate::bandplan::canonical_band(band).to_ascii_lowercase()
}

/// The operator's station: one log, one identity of record, one set of outbound
/// connector queues — shared by every receive/transmit chain the app runs.
pub struct StationCore {
    /// Real PC-clock-vs-UTC offset (ms) from the NTP probe, or None if disabled/offline.
    pub(crate) clock_offset_ms: Option<i64>,
    /// WSJT-X-format ALL.TXT decode lines pending flush to disk (when
    /// `settings.write_all_txt`). The engine is I/O-free, so the shell drains this via
    /// [`Self::take_all_txt_pending`] and appends to the log file. Capped so a
    /// never-draining shell can't grow it without bound.
    pub(crate) all_txt_pending: Vec<String>,
    /// Freshly-logged QSOs awaiting the shell's connector auto-upload worker
    /// (QRZ / ClubLog / eQSL). EVERY `Engine::log_qso` path queues here — the
    /// engine auto-log included — so "logged locally but never uploaded" can't
    /// happen for any log path. Drained by [`Self::take_pending_uploads`];
    /// bounded so a worker outage can't grow it without limit.
    pub(crate) pending_uploads: VecDeque<PendingUpload>,
    /// Last connector-upload outcome (operator-facing toast text) + whether it
    /// succeeded; `upload_tick` bumps on every note so the UI can toast changes.
    pub(crate) upload_note: Option<String>,
    pub(crate) upload_ok: bool,
    pub(crate) upload_tick: u32,
    /// Persistent QSO logbook (worked-before / ADIF), loaded from `log_path`.
    pub(crate) logbook: Logbook,
    /// ADIF file the logbook is persisted to, if the shell set one.
    pub(crate) log_path: Option<PathBuf>,
    /// Last-seen (mtime, byte length) of the SHARED `log.adi` — the freshness
    /// fingerprint for the two-instance watcher and the pre-stamp recovery gate:
    /// re-read + reconcile only when another instance has touched the file. The
    /// length rides along because mtime granularity can be as coarse as 2 s on
    /// FAT/SMB shares (a NAS-shared log is supported), where a same-tick sibling
    /// write would otherwise be invisible.
    pub(crate) last_log_mtime: Option<(std::time::SystemTime, u64)>,
    /// ADIF journal for the Field Day contest log, if the shell set one — the
    /// in-memory FD log (which lives only inside `Mode::FieldDay`) is
    /// rewritten here on every logged contact and merged back in when a Field
    /// Day mode starts, so a mid-event restart loses nothing.
    pub(crate) fd_log_path: Option<PathBuf>,
    /// Durable journal for the single QSO held by the prompt-to-log popup.
    pub(crate) pending_qso_path: Option<PathBuf>,
    /// Journal path for the store-and-forward outbound queue (pending_msgs.json) —
    /// written on every queue mutation so held Tempo messages survive a restart.
    pub(crate) pending_msgs_path: Option<PathBuf>,
    /// Callsign → DXCC entity resolver, injected by the command layer (which owns
    /// the cty.dat table) so tempo-app stays DXCC-free. `None` in headless tests
    /// (new-DXCC highlighting simply stays off). See [`Self::set_dxcc_resolver`].
    #[allow(clippy::type_complexity)]
    pub(crate) dxcc_resolve: Option<Box<dyn Fn(&str) -> Option<String> + Send + Sync>>,
    /// (Callsign, heard grid) → US state resolver, injected by the command layer (which owns
    /// the FCC callsign index) — same pattern as [`Self::set_dxcc_resolver`], and injected for
    /// the same reason: tempo-app has no `propagation` dependency, so it cannot reach the index
    /// directly. `None` in headless tests (state simply stays unresolved, never guessed).
    ///
    /// ⚠️ It MUST take the same inputs as the heard side's `us_state_hint(call, grid)`. Passing
    /// only the call would re-open the exact split this exists to close: a call the FCC file
    /// does not list still resolves on the heard side via its grid, so a call-only resolver
    /// would leave `worked_states` unable to match it and NewState would stick forever.
    #[allow(clippy::type_complexity)]
    pub(crate) state_resolve:
        Option<Box<dyn Fn(&str, Option<&str>) -> Option<String> + Send + Sync>>,
    /// Grid → rarity tier (0–3) resolver, injected by the command layer (which
    /// owns the geography table in the propagation crate) — same pattern as
    /// [`Self::set_dxcc_resolver`]. `None` in headless tests (gems stay off).
    #[allow(clippy::type_complexity)]
    pub(crate) grid_rarity_resolve: Option<Box<dyn Fn(&str) -> Option<u8> + Send + Sync>>,
    /// Injected "is this call an active LoTW uploader" check (the shell owns the
    /// ARRL user-activity file + recency window). Presentational only.
    pub(crate) lotw_resolve: Option<LotwResolver>,
    /// DXCC entities already worked (from the logbook), keyed PER BAND —
    /// `(entity, band_key)` — for new-entity decode highlighting. Rebuilt on log
    /// load + each log mutation. Per band because DXCC is a per-band award
    /// (Challenge slots, VHF DXCC): see [`worked_grids`](Self::worked_grids).
    pub(crate) worked_entities: HashSet<(String, String)>,
    /// Maidenhead grids already worked (uppercased), keyed PER BAND —
    /// `(grid, band_key)` — for new-grid highlighting. A grid square is an award
    /// slot on EACH band (VUCC is per band), so FN31 worked on 20 m is genuinely
    /// new again on 2 m, where a grid is a far rarer achievement.
    pub(crate) worked_grids: HashSet<(String, String)>,
    /// POTA/SOTA references already in the log (hunter side, `ota.their_ref`)
    /// — drives the NEW PARK badge like worked_entities drives new-DXCC.
    pub(crate) worked_parks: HashSet<String>,
    /// Park references the operator imported from their POTA "Hunted Parks.CSV"
    /// (uppercased). Unioned into `park_worked` so hunts made on CW — where the
    /// park ref is never in the exchange, so the log can't know it — still count
    /// as worked. Persisted by the shell; seeded on import + at startup.
    pub(crate) hunted_parks_import: HashSet<String>,
    /// Pending HUNT target (program, normalized ref, activator call, set-at
    /// unix): set by a one-click hunt; the next QSO logged with that call
    /// auto-tags SIG/SIG_INFO (their_*) and the pend clears. Expires after
    /// [`HUNT_TTL_SECS`] — activations end; a forgotten pend must never stamp
    /// a park on an unrelated contact hours later. Session-only.
    pub(crate) pending_hunt: Option<(String, String, String, u64)>,
    /// Per-launch salt for the hound pileup spread (stock re-randomizes each
    /// session; a pure callsign hash parked every operator on the same offset
    /// at every event).
    pub(crate) session_salt: u32,
    /// Directory for saved RX-period WAVs (settings.save_wav) — set by the shell.
    pub(crate) periods_dir: Option<String>,
    /// The latest reconcile summary from the last LoTW / eQSL sync (in-memory,
    /// this session) — its `orphans` drive the confirmation diagnostics. Per source
    /// so a later eQSL sync doesn't clobber the LoTW orphans. Resets on restart
    /// until the next sync.
    pub(crate) last_lotw_reconcile: Option<tempo_core::reconcile::ReconcileSummary>,
    pub(crate) last_eqsl_reconcile: Option<tempo_core::reconcile::ReconcileSummary>,
    /// Orphans from the last QRZ two-way sync (own its slot so an eQSL/LoTW sync
    /// doesn't clobber them). Resets on restart until the next sync.
    pub(crate) last_qrz_reconcile: Option<tempo_core::reconcile::ReconcileSummary>,
    /// Current Parks/Summits On The Air activation `(program, reference)` — when set,
    /// each logged QSO is tagged as your activation (POTA/SOTA). Transient (an
    /// activation ends), so not persisted. `None` = not activating.
    pub(crate) activation: Option<(String, String)>,
    /// Session gallery of saved SSTV images, newest last. Seeded from the
    /// persisted `gallery.json` at startup; the decode thread appends on each
    /// completed image. Capped at [`SSTV_GALLERY_CAP`].
    pub(crate) sstv_gallery: Vec<crate::dto::SstvGalleryEntry>,
}

impl StationCore {
    /// A fresh station: empty log, no paths, no injected resolvers. The shell wires the
    /// real ones in at startup (log path, cty.dat/rarity/LoTW resolvers, journals).
    pub(crate) fn new() -> Self {
        Self {
            clock_offset_ms: None,
            all_txt_pending: Vec::new(),
            pending_uploads: VecDeque::new(),
            upload_note: None,
            upload_ok: false,
            upload_tick: 0,
            logbook: Logbook::new(),
            log_path: None,
            last_log_mtime: None,
            fd_log_path: None,
            pending_qso_path: None,
            pending_msgs_path: None,
            dxcc_resolve: None,
            state_resolve: None,
            grid_rarity_resolve: None,
            lotw_resolve: None,
            worked_entities: HashSet::new(),
            worked_grids: HashSet::new(),
            worked_parks: HashSet::new(),
            hunted_parks_import: HashSet::new(),
            pending_hunt: None,
            session_salt: now_unix_secs() as u32,
            periods_dir: None,
            last_lotw_reconcile: None,
            last_eqsl_reconcile: None,
            last_qrz_reconcile: None,
            activation: None,
            sstv_gallery: Vec::new(),
        }
    }

    /// Where saved RX-period WAVs go (the shell passes `<recordings>/periods`).
    pub fn set_periods_dir(&mut self, dir: &str) {
        self.periods_dir = Some(dir.to_string());
    }

    pub fn periods_dir(&self) -> Option<String> {
        self.periods_dir.clone()
    }

    /// Point the logbook at an ADIF file and load any existing contacts from it.
    /// Called once by the shell at startup so `worked_before` highlighting and
    /// the log view reflect prior sessions, and auto-log appends to this file.
    pub fn set_log_path(&mut self, path: PathBuf) {
        self.logbook = Logbook::load(&path);
        self.log_path = Some(path);
        self.backfill_country();
        self.backfill_state();
        self.refresh_worked_index();
    }

    /// Point the Field Day contest log at its durable ADIF journal. Called once
    /// by the shell at startup (beside [`Self::set_log_path`]); the journal is
    /// rewritten on every FD contact and restored when FD mode starts.
    pub fn set_fd_log_path(&mut self, path: PathBuf) {
        self.fd_log_path = Some(path);
    }

    /// Point the prompt-to-log hold at its durable journal. Called once by the shell at
    /// startup, beside [`Self::set_fd_log_path`].
    pub fn set_pending_qso_path(&mut self, path: PathBuf) {
        self.pending_qso_path = Some(path);
    }

    pub fn set_pending_msgs_path(&mut self, path: PathBuf) {
        self.pending_msgs_path = Some(path);
    }

    /// Inject the callsign → DXCC entity resolver (the command layer passes
    /// `propagation::dxcc::resolve`-backed closure). Rebuilds the worked-entity
    /// index so new-DXCC decode highlighting works from the next snapshot.
    pub fn set_dxcc_resolver(
        &mut self,
        resolve: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) {
        self.dxcc_resolve = Some(Box::new(resolve));
        self.backfill_country();
        self.refresh_worked_index();
    }

    /// Inject the (callsign, heard grid) → US state resolver (the command layer passes a
    /// closure over the FCC callsign index — the SAME `us_state_hint` the heard side uses).
    /// Backfills every record that lacks a STATE so the needed board, WAS and the awards
    /// matrix all see the states already worked.
    ///
    /// No `refresh_worked_index()` here, unlike [`Self::set_dxcc_resolver`]: worked STATES are
    /// not part of this struct's index (that holds entities/grids/parks). They are folded into
    /// `propagation::LogNeeds` from the records themselves, so backfilling the records is
    /// exactly what the needs side reads.
    pub fn set_state_resolver(
        &mut self,
        resolve: impl Fn(&str, Option<&str>) -> Option<String> + Send + Sync + 'static,
    ) {
        self.state_resolve = Some(Box::new(resolve));
        self.backfill_state();
    }

    /// Resolve a US state for any logged record that lacks one. No-op without a resolver;
    /// persists the log if anything changed.
    ///
    /// WHY THIS EXISTS: one question — "what state is this call in?" — used to be answered by
    /// two different resolvers on the two sides of the same comparison. The heard side resolved
    /// it from the FCC index; the worked side could only read a logged ADIF STATE, and every
    /// record Nexus generated hardcoded `state: None`. So a state could be worked and still
    /// report as needed forever. Filling the record is what makes both sides read the same
    /// source. See [`Self::backfill_country`] — same shape, same reasons, same ordering.
    fn backfill_state(&mut self) {
        let Some(resolve) = self.state_resolve.take() else {
            return;
        };
        // Same ordering as backfill_country and for the same reason: pull in records a second
        // instance appended BEFORE the full-log rewrite below, or this silently drops them
        // (the M18 data-loss class). Doing it first also backfills the recovered records.
        self.recover_external_appends();
        let mut changed = false;
        for r in self.logbook.records_mut() {
            if r.state.is_none() {
                if let Some(st) = resolve(&r.call, r.grid.as_deref()) {
                    r.state = Some(st);
                    changed = true;
                }
            }
        }
        self.state_resolve = Some(resolve);
        if changed {
            self.save_log("backfill_state");
        }
    }

    /// Inject the grid → rarity-tier (0–3) resolver (the command layer passes a
    /// `propagation::gridrarity::tier_u8`-backed closure). Purely presentational
    /// — decodes/roster gain their rarity gems from the next snapshot.
    pub fn set_grid_rarity_resolver(
        &mut self,
        resolve: impl Fn(&str) -> Option<u8> + Send + Sync + 'static,
    ) {
        self.grid_rarity_resolve = Some(Box::new(resolve));
    }

    /// Rarity of a heard grid via the injected resolver; `None` when unwired,
    /// grid-less, or invalid.
    pub(crate) fn rarity_of(&self, grid: Option<&str>) -> Option<crate::dto::GridRarity> {
        let g = grid?.trim();
        if g.is_empty() {
            return None;
        }
        let f = self.grid_rarity_resolve.as_ref()?;
        f(g).map(crate::dto::GridRarity::from_tier)
    }

    /// Inject the callsign → active-LoTW-uploader check (the shell backs it with
    /// ARRL's lotw-user-activity.csv + the operator's recency window). Purely
    /// presentational — decodes/roster gain their LoTW marks from the next snapshot.
    pub fn set_lotw_resolver(&mut self, resolve: impl Fn(&str) -> bool + Send + Sync + 'static) {
        self.lotw_resolve = Some(Box::new(resolve));
    }

    /// Whether a heard call uploads to LoTW (via the injected resolver); `false`
    /// when unwired — the honest default is no highlight, never a guess.
    pub(crate) fn lotw_user(&self, call: Option<&str>) -> bool {
        let (Some(c), Some(f)) = (call, self.lotw_resolve.as_ref()) else {
            return false;
        };
        !c.trim().is_empty() && f(c.trim())
    }

    /// Resolve a DXCC country for any logged record that lacks one (e.g. a log
    /// loaded/imported from an ADIF without `COUNTRY`, or older Nexus records).
    /// No-op without a resolver; persists the log if anything changed. Run after
    /// load / import / resolver-set so the logbook + awards are country-complete.
    fn backfill_country(&mut self) {
        let Some(resolve) = self.dxcc_resolve.take() else {
            return;
        };
        // Pull in any records a second instance appended BEFORE the full-log
        // rewrite below, so backfill can't silently drop them (the M18 data-loss
        // class). Doing it before the loop also backfills the recovered records.
        self.recover_external_appends();
        let mut changed = false;
        for r in self.logbook.records_mut() {
            if r.country.is_none() {
                if let Some(c) = resolve(&r.call) {
                    r.country = Some(c);
                    changed = true;
                }
            }
        }
        self.dxcc_resolve = Some(resolve);
        if changed {
            self.save_log("backfill_country");
        }
    }

    /// One-click HUNT: remember the activator + park so the NEXT QSO logged
    /// with that call auto-tags `SIG`/`SIG_INFO` (POTA) / `SOTA_REF` — the
    /// hunter-side ADIF credit. Validates like [`Self::set_activation`].
    pub fn set_hunt_target(
        &mut self,
        call: &str,
        program: &str,
        reference: &str,
    ) -> Result<(), String> {
        let prog = tempo_core::pota::OtaProgram::from_code(program)
            .ok_or_else(|| format!("unknown program {program:?} (POTA/SOTA)"))?;
        let normalized = tempo_core::pota::normalize_ref(prog, reference)
            .ok_or_else(|| format!("invalid {} reference {reference:?}", prog.code()))?;
        let c = call.trim().to_uppercase();
        if c.is_empty() {
            return Err("no activator callsign".into());
        }
        self.pending_hunt = Some((prog.code().to_string(), normalized, c, now_unix_secs()));
        Ok(())
    }

    /// Drop the pending hunt target (operator cancelled / moved on).
    pub fn clear_hunt_target(&mut self) {
        self.pending_hunt = None;
    }

    /// The pending hunt (program, reference, activator call), for the UI chip.
    /// An expired pend reads as None (and is dropped lazily).
    pub fn hunt_target(&self) -> Option<(String, String, String)> {
        self.pending_hunt
            .as_ref()
            .filter(|(_, _, _, at)| now_unix_secs().saturating_sub(*at) <= HUNT_TTL_SECS)
            .map(|(p, r, c, _)| (p.clone(), r.clone(), c.clone()))
    }

    /// True when this POTA/SOTA reference is already worked — either in the log
    /// (hunter side) OR in the operator's imported POTA "Hunted Parks.CSV" (which
    /// covers CW hunts the log can't know about, since the park ref isn't exchanged).
    pub fn park_worked(&self, reference: &str) -> bool {
        let key = reference.trim().to_uppercase();
        self.worked_parks.contains(&key) || self.hunted_parks_import.contains(&key)
    }

    /// Seed the imported hunted-parks set from a POTA "Hunted Parks.CSV" (the shell
    /// parses the reference column). Replaces the set wholesale (a re-import is the
    /// full current picture). References are uppercased to match `park_worked`.
    pub fn set_hunted_parks_import(&mut self, refs: impl IntoIterator<Item = String>) {
        self.hunted_parks_import = refs
            .into_iter()
            .filter_map(|r| {
                let r = r.trim().to_uppercase();
                (!r.is_empty()).then_some(r)
            })
            .collect();
    }

    /// How many parks the operator has imported from their Hunted Parks.CSV.
    pub fn hunted_parks_import_count(&self) -> usize {
        self.hunted_parks_import.len()
    }

    /// Recompute the worked-entity and worked-grid sets from the logbook. Cheap
    /// (a few hundred records); run on log load and after each log mutation.
    pub(crate) fn refresh_worked_index(&mut self) {
        self.worked_grids.clear();
        self.worked_entities.clear();
        self.worked_parks.clear();
        for r in self.logbook.records() {
            // Parks are NOT per band: a POTA/SOTA reference is hunted once, on any
            // band, so this one stays a flat set.
            if let Some(p) = &r.ota.their_ref {
                let p = p.trim();
                if !p.is_empty() {
                    self.worked_parks.insert(p.to_uppercase());
                }
            }
            let band = band_key(&r.band);
            if let Some(g) = &r.grid {
                // A SATELLITE contact (PROP_MODE=SAT) earns Satellite-VUCC credit
                // only (the ARRL rule), so its grid must NOT enter the per-band
                // terrestrial index — a 2m sat-only FN31 muting the 2m NEW GRID
                // decode badge would hide a slot that is genuinely still open.
                // Mirrors the same exclusion in propagation's LogNeeds::add_qso.
                let sat = r
                    .prop_mode
                    .as_deref()
                    .is_some_and(|p| p.trim().eq_ignore_ascii_case("SAT"));
                // Index at 4-char granularity so a 6-char logged grid matches a 4-char decode.
                if !sat {
                    if let Some(g4) = Self::grid4(g) {
                        self.worked_grids.insert((g4, band.clone()));
                    }
                }
            }
            if let Some(resolve) = &self.dxcc_resolve {
                if let Some(entity) = resolve(&r.call) {
                    self.worked_entities.insert((entity, band));
                }
            }
        }
    }

    /// The 4-character Maidenhead field+square, upper-cased — the granularity grids are
    /// AWARDED at (VUCC counts squares, not subsquares).
    ///
    /// Both the index and the lookup MUST go through this. They did not: the log stores
    /// whatever was logged ("FN31PR" from a QRZ import), while a decode carries 4 characters
    /// ("FN31"), so the compare never matched and every such square reported NOT worked —
    /// a NEW GRID badge that fires forever. Mirrors `needalert::grid4`, which already did
    /// this correctly on the alerting side; only this index disagreed.
    fn grid4(grid: &str) -> Option<String> {
        let g: String = grid.trim().to_ascii_uppercase().chars().take(4).collect();
        (g.len() == 4).then_some(g)
    }

    /// Is this grid already worked ON THIS BAND? (`band` is the raw band label —
    /// canonicalized here.) A grid worked only on another band reads as NOT worked,
    /// which is the point: per-band is how grids are awarded.
    pub(crate) fn grid_worked_on(&self, grid: &str, band: &str) -> bool {
        Self::grid4(grid).is_some_and(|g4| self.worked_grids.contains(&(g4, band_key(band))))
    }

    /// Is this DXCC entity already worked ON THIS BAND? Per band, like
    /// [`grid_worked_on`](Self::grid_worked_on).
    pub(crate) fn entity_worked_on(&self, entity: &str, band: &str) -> bool {
        self.worked_entities
            .contains(&(entity.to_string(), band_key(band)))
    }

    /// Whether the entity has been worked on ANY band — a true ATNO check (all-time), as opposed
    /// to the per-band [`entity_worked_on`]. Distinguishes the decode feed's `DXCC` (all-time
    /// new) tag from its `BAND` (worked elsewhere, new on this band) tag.
    pub(crate) fn entity_worked_ever(&self, entity: &str) -> bool {
        self.worked_entities.iter().any(|(e, _)| e == entity)
    }

    /// Drain the freshly-logged QSOs awaiting connector auto-upload (FIFO).
    /// Called by the shell's upload worker; empty when nothing was logged.
    pub fn take_pending_uploads(&mut self) -> Vec<PendingUpload> {
        self.pending_uploads.drain(..).collect()
    }

    /// Re-queue an upload for ONLY the legs that transiently failed (network down,
    /// service busy), so the worker retries them without re-pushing the legs that
    /// already succeeded — a permanently-rejected or successful leg is never in
    /// `legs`. Dropped once past [`MAX_UPLOAD_RETRIES`] or with nothing owed.
    pub fn requeue_upload(&mut self, rec: tempo_core::logbook::QsoRecord, legs: u8, attempts: u8) {
        if legs == 0 || attempts >= MAX_UPLOAD_RETRIES {
            return;
        }
        if self.pending_uploads.len() >= 256 {
            self.pending_uploads.pop_front();
        }
        self.pending_uploads.push_back(PendingUpload {
            rec,
            legs,
            attempts,
        });
    }

    /// Record a connector-upload outcome for the operator (toast text + level).
    /// Bumps `upload_tick` so the UI's snapshot poll notices the change.
    pub fn note_upload(&mut self, note: impl Into<String>, ok: bool) {
        self.upload_note = Some(note.into());
        self.upload_ok = ok;
        self.upload_tick = self.upload_tick.wrapping_add(1);
    }

    /// Begin a Parks/Summits On The Air activation — every QSO logged afterward is
    /// tagged as your activation until [`clear_activation`](Self::clear_activation).
    /// Validates + normalizes the reference; returns the normalized `(program, ref)`
    /// or an error string for an unknown program / malformed reference.
    pub fn set_activation(
        &mut self,
        program: &str,
        reference: &str,
    ) -> Result<(String, String), String> {
        let prog = tempo_core::pota::OtaProgram::from_code(program)
            .ok_or_else(|| format!("Unknown program '{program}' — use POTA or SOTA."))?;
        let normalized = tempo_core::pota::normalize_ref(prog, reference)
            .ok_or_else(|| format!("'{reference}' isn't a valid {} reference.", prog.code()))?;
        self.activation = Some((prog.code().to_string(), normalized.clone()));
        Ok((prog.code().to_string(), normalized))
    }

    /// End the current activation (subsequent QSOs are untagged).
    pub fn clear_activation(&mut self) {
        self.activation = None;
    }

    /// The current activation `(program, reference)`, if any.
    pub fn activation(&self) -> Option<(String, String)> {
        self.activation.clone()
    }

    /// How many logged QSOs carry the current activation reference (the live count
    /// for the activation panel). 0 when not activating.
    pub fn activation_qso_count(&self) -> usize {
        match &self.activation {
            Some((_, reference)) => self
                .logbook
                .records()
                .iter()
                .filter(|r| r.ota.my_ref.as_deref() == Some(reference.as_str()))
                .count(),
            None => 0,
        }
    }

    /// Before any full-log rewrite ([`Logbook::save`]), pull back any records that
    /// another writer — a second Nexus instance sharing this `log.adi`, since there
    /// is no single-instance guard — appended to the file after we loaded it. Our
    /// in-memory copy is otherwise stale, and `save` would `rename()` a truncated
    /// log over the file, silently discarding those QSOs.
    ///
    /// The reconcile ADDS the records we lack (appended to the end, leaving existing
    /// indices valid) and upgrades the shared ones monotonically — it never
    /// resurrects a record we just edited or deleted, PROVIDED callers run this
    /// BEFORE their mutation, while our copy still holds the record being changed.
    /// No-op without a log path or on a read error.
    /// Returns whether the disk was actually re-read (fingerprint moved).
    ///
    /// # What this costs: an EDIT made in the OTHER instance arrives as a DUPLICATE
    ///
    /// The contract above covers edits made HERE. It cannot cover an edit made
    /// THERE. A QSO has no stable id in the ADIF, so identity is
    /// (call, band, mode-class, contact second) — and a correction usually changes
    /// one of those. Instance A fixes a mis-logged time, 12:00 → 12:05, and rewrites
    /// the file; we still hold the 12:00 row; this recovery sees 12:05 as a contact
    /// we do not have and APPENDS it. From then on both instances hold two rows for
    /// one QSO — stably, permanently, and in the file — and both are eligible to be
    /// uploaded (`lotw_unsent_indices` counts them separately, so LoTW is offered two
    /// QSOs for one contact). Same for a corrected callsign or band. An edit that
    /// keeps all four key fields pairs normally, and OUR copy of the edited field is
    /// what the next rewrite writes back: that correction is silently reverted.
    ///
    /// **This is the chosen trade, not an oversight.** The only key that could
    /// recognise 12:05 as "the 12:00 row, edited" is a fuzzy one, and fuzz here is
    /// what mis-paired two distinct contacts with one station inside a day and
    /// destroyed one of them (see [`tempo_core::reconcile::merge_own_disk`]). A
    /// duplicate is on screen and one delete away; a silently reverted correction is
    /// invisible, and a mis-pairing is a QSO gone. Pinned by
    /// `merge_own_disk_leaves_a_cross_instance_edit_as_a_visible_duplicate`; told to
    /// the operator in the CHANGELOG. Making an edit detectable instead needs a
    /// per-record id persisted in the ADIF, which no existing log carries.
    fn recover_external_appends(&mut self) -> bool {
        let Some(path) = self.log_path.clone() else {
            return false;
        };
        // Fingerprint gate: the stamp/save paths run this up to three times per
        // logged QSO, and an unconditional read re-parsed the whole multi-MB
        // log each time. When the file's (mtime, len) matches what we last read
        // or WROTE (save_log records our own writes), the disk holds exactly
        // what we hold — skip the parse. A stat error falls through to the
        // full read: never skip on uncertainty.
        let stamp = log_file_stamp(&path);
        if stamp.is_some() && stamp == self.last_log_mtime {
            return false;
        }
        // A FAILED read records nothing: stamping the fingerprint here would
        // make the gate treat the failure as "reconciled", and the next full
        // rewrite would drop whatever the other instance wrote. Retry instead.
        let Ok(disk) = std::fs::read_to_string(&path) else {
            return false;
        };
        if !disk.is_empty() {
            // Field-level MERGE, not an additive import: fold in another instance's appends AND
            // upgrade shared records' confirmation/upload/QSL-sent state from disk, so this
            // instance's imminent full-file rewrite can't clobber what the other one wrote.
            self.logbook.reconcile_disk(&disk);
        }
        self.last_log_mtime = stamp;
        true
    }

    /// THE way to APPEND to the logbook file: write the records on the end, then
    /// carry the freshness fingerprint forward so the recovery gate above does not
    /// re-parse a multi-MB log we extended ourselves.
    ///
    /// An append needs no recovery first — it cannot truncate anything — but it does
    /// move the file's `(mtime, len)`, and leaving the fingerprint behind made the
    /// gate MISS on every later call. That is a permanent per-call cost, not a
    /// one-time one: companion mode imports one ADIF per logged QSO, so a 26,000-QSO
    /// log was re-parsed from disk on every contact WSJT-X logged.
    ///
    /// Stamping is only sound while we can account for every byte on disk, so two
    /// checks fence it and BOTH are load-bearing:
    ///
    /// - the file must look exactly as we last read or WROTE it before we append —
    ///   otherwise our copy is already stale by another instance's records, and
    ///   nothing here would ever pull them back;
    /// - it must be exactly `written` bytes longer afterwards — otherwise another
    ///   instance appended in the window between our write and our stat, and the
    ///   post-write length is partly ITS bytes, which we do not hold.
    ///
    /// Fail either (or fail the write itself) and we drop the fingerprint rather than
    /// record it, so the next recovery re-reads. A stamp we cannot justify is exactly
    /// how a stale copy silently deletes a second instance's QSOs on the next full
    /// rewrite — the fault `recover_external_appends` exists to prevent. `written` is
    /// re-derived through the same [`adif_record`] the append writes; if the two ever
    /// drift the length check simply misses and we fall back to re-reading.
    ///
    /// # MEMORY FIRST — the caller's half of the contract
    ///
    /// `recs` must ALREADY be in `self.logbook`, as its last records, before this is
    /// called. The fingerprint recorded below says "the file holds exactly what we
    /// hold"; append a record we have not added to memory yet and that claim is false
    /// for the width of whatever runs next. Unwind in that window — `Engine::log_qso`
    /// spawns two threads there, and `std::thread::spawn` panics when the OS refuses
    /// one — and the contact is on disk, absent from memory, with the gate saying the
    /// two agree: the next full rewrite writes memory over the file and DELETES the
    /// contact just logged. Nothing pulls it back, because the gate is what would have
    /// re-read it. Memory is the source of truth for `save`, so memory is written
    /// first, always. Checked here in debug builds, where the invariant is relied on.
    ///
    /// [`adif_record`]: tempo_core::logbook::adif_record
    pub(crate) fn append_to_log(&mut self, recs: &[QsoRecord]) {
        let Some(path) = self.log_path.clone() else {
            return;
        };
        debug_assert!(
            self.logbook.records().ends_with(recs),
            "append_to_log: the records must already be in memory (see the contract above)"
        );
        let before = log_file_stamp(&path);
        let mut accountable = before.is_some() && before == self.last_log_mtime;
        let mut written = 0u64;
        for r in recs {
            match Logbook::append(&path, r) {
                Ok(()) => written += tempo_core::logbook::adif_record(r).len() as u64,
                Err(e) => {
                    eprintln!("tempo: logbook append failed: {e}");
                    accountable = false;
                }
            }
        }
        self.last_log_mtime = match (accountable, before, log_file_stamp(&path)) {
            (true, Some((_, was)), Some((mtime, now))) if now == was + written => {
                Some((mtime, now))
            }
            _ => None,
        };
    }

    /// THE way to persist the logbook: save, then record the file's fresh mtime
    /// so the recovery gate above doesn't re-parse our own write on the next
    /// stamp. Every full-log rewrite in this file funnels through here.
    fn save_log(&mut self, context: &str) {
        let Some(path) = self.log_path.clone() else {
            return;
        };
        match self.logbook.save(&path) {
            // The fingerprint comes from save() itself (statted pre-rename), so
            // a concurrent instance's rename can never be recorded as our write.
            Ok(stamp) => self.last_log_mtime = stamp,
            Err(e) => {
                eprintln!("tempo: {context} save failed: {e}");
                // Disk ≠ memory now: drop the gate so the next recovery
                // re-reads instead of trusting a stale fingerprint.
                self.last_log_mtime = None;
            }
        }
    }

    /// Two-instance freshness watcher: if the SHARED `log.adi`'s mtime moved since we last
    /// looked — i.e. the OTHER instance logged or confirmed something — fold its changes in and
    /// rebuild the worked-before/needs index. When nothing changed this is a single `stat`, so
    /// it is cheap to call on every Needed-board poll; that is what keeps a MONITORING radio
    /// (parked, not logging, so it never hits the stamp/save recovery path) from showing a DXCC
    /// as "needed" that the other radio just worked, with no save or restart required. Returns
    /// true when it actually re-read (so the caller can refresh anything derived downstream).
    pub fn sync_shared_log_if_changed(&mut self) -> bool {
        let Some(path) = self.log_path.clone() else {
            return false;
        };
        // A missing file / stat error leaves us on last-good; the recovery owns
        // the mtime gate and records what it read.
        if std::fs::metadata(&path).and_then(|m| m.modified()).is_err() {
            return false;
        }
        if !self.recover_external_appends() {
            return false;
        }
        self.refresh_worked_index();
        true
    }

    /// Edit an existing logbook entry (a correction — busted call, wrong band, etc).
    /// Sync-derived state is preserved by `Logbook::update_record`. Persists by
    /// rewriting the whole ADIF (an edit can't be an append). Returns false if
    /// `index` is out of range.
    pub fn update_qso(&mut self, index: usize, mut rec: QsoRecord) -> bool {
        // Keep country populated on edits (the edit form doesn't carry it).
        if rec.country.is_none() {
            if let Some(resolve) = &self.dxcc_resolve {
                rec.country = resolve(&rec.call);
            }
        }
        // Recover another instance's appends BEFORE applying the edit, so the
        // full-log rewrite below can't drop them (and so the pre-edit record is
        // still present to dedup against — no stale copy is re-added).
        self.recover_external_appends();
        let ok = self.logbook.update_record(index, rec);
        if ok {
            self.save_log("update_qso");
            self.refresh_worked_index();
        }
        ok
    }

    /// Mark logbook entry `index` as QSL-sent (operator-declared: I sent a
    /// card/request `via` bureau/direct/electronic, dated now). Only ADDS a request
    /// — never touches confirmation state. Persists by rewriting the ADIF. Returns
    /// false if `index` is out of range.
    pub fn mark_qsl_sent(&mut self, index: usize, via: tempo_core::logbook::QslVia) -> bool {
        self.recover_external_appends();
        let ok = self.logbook.mark_qsl_sent(index, via, now_unix_secs());
        if ok {
            self.save_log("mark_qsl_sent");
        }
        ok
    }

    /// Delete a logbook entry (a mis-logged contact). Persists by rewriting the
    /// ADIF. Returns false if `index` is out of range. Shifts later indices — the
    /// caller must reload the log afterward.
    pub fn delete_qso(&mut self, index: usize) -> bool {
        // Recover another instance's appends BEFORE the delete, so the rewrite
        // drops only THIS record (the deleted key is absent from our copy at save
        // time, so recovery can't re-add it) and keeps the other writer's QSOs.
        self.recover_external_appends();
        let ok = self.logbook.delete(index);
        if ok {
            self.save_log("delete_qso");
            self.refresh_worked_index();
        }
        ok
    }

    /// Purge the ENTIRE logbook (operator-confirmed, destructive, irreversible).
    /// Clears every contact in memory, rewrites the ADIF file to an empty log, and
    /// recomputes the worked-entity/grid sets (so the roster B4 highlighting and
    /// the needs/awards model reset too). Returns the number of contacts removed.
    pub fn clear_logbook(&mut self) -> usize {
        let n = self.logbook.clear();
        if n > 0 {
            self.save_log("clear_logbook");
            self.refresh_worked_index();
        }
        n
    }

    /// Import an external ADIF logbook: merge (deduped) into the persistent log,
    /// persist it, and return `(added, skipped, updated, total)` — where `updated`
    /// counts records ALREADY logged that the import upgraded. The next propagation
    /// snapshot derives real "needs" from the enlarged log (and roster B4
    /// highlighting updates).
    ///
    /// Two write paths, because the import has two effects. New contacts are
    /// APPENDED (cheap, and all an import used to do). But an import also upgrades
    /// records already in the log — the confirmations and credits a LoTW/eQSL/QRZ
    /// download restates — and an append cannot express a change to a record that
    /// is already in the file. Those need the full rewrite, or the confirmations
    /// live in memory until the next save and are lost outright if the app exits
    /// first.
    ///
    /// That second path is why this now recovers first, like every other full-log
    /// rewrite in this file. An append-only import was concurrency-safe by
    /// construction and was the one exemption from the contract above; the moment it
    /// could `rename()` a whole log over the file, the exemption stopped holding and
    /// a stale copy silently deleted a second instance's QSOs.
    ///
    /// ORDER — before the MERGE, not merely before the WRITE, matching every sibling
    /// site and the "BEFORE their mutation" requirement on
    /// [`Self::recover_external_appends`]. A confirmation report is about contacts
    /// already logged, and some of them may be logged only by the OTHER instance. Run
    /// first, and such a row matches the recovered record and confirms it. Run after,
    /// and the merge — looking at a log that does not contain it yet — reads the same
    /// row as a brand-new contact and logs it a second time. So the late order costs
    /// a duplicate and a lost confirmation even though it saves the file.
    pub fn import_adif(&mut self, text: &str) -> (usize, usize, usize, usize) {
        self.recover_external_appends();
        let (added, skipped, merged) = self.logbook.import_adif(text);
        if merged > 0 {
            self.save_log("import_adif"); // rewrites the whole log, `added` included
        } else {
            self.append_to_log(&added);
        }
        self.backfill_country();
        self.refresh_worked_index();
        (added.len(), skipped, merged, self.logbook.len())
    }

    /// Reconcile a confirmation/credit report (ADIF — e.g. a LoTW export) INTO the
    /// existing log: monotonically upgrade matched QSOs' confirmation + credit
    /// (which a plain dedup-import would skip and lose), rewrite the ADIF file, and
    /// return the reconcile summary (newly confirmed/credited + unmatched orphans).
    pub fn merge_lotw_report(&mut self, text: &str) -> tempo_core::reconcile::ReconcileSummary {
        self.recover_external_appends();
        let summary = self.logbook.merge_report(text);
        self.last_lotw_reconcile = Some(summary.clone());
        self.save_log("merge_lotw_report");
        summary
    }

    /// Stamp POTA/SOTA park refs from a pota.app hunter/activator export onto matching
    /// existing QSOs (stamp-only: never creates records, never overwrites a ref — the
    /// reviewed-adds half is a separate feature). Returns (stamped, already, unmatched).
    pub fn import_pota_log(&mut self, text: &str) -> (usize, usize, usize) {
        self.recover_external_appends();
        let out = self.logbook.stamp_ota_refs(text);
        if out.0 > 0 {
            self.save_log("import_pota_log");
        }
        out
    }

    /// Merge a LoTW own-QSO report (`qso_qsl=no`) INTO the log: promote in-flight
    /// uploads (Pending / never-marked) to `Accepted` where LoTW confirms it holds
    /// your record — the step that turns a just-uploaded QSO into "waiting on the
    /// partner" (R2) and clears false "never uploaded" (R1) for out-of-band uploads.
    /// Persists the log on any change. Returns the count newly promoted.
    pub fn merge_lotw_own_echo(&mut self, text: &str, when_unix: i64) -> usize {
        self.recover_external_appends();
        let promoted = self.logbook.merge_own_echo(text, when_unix);
        if promoted > 0 {
            self.save_log("merge_lotw_own_echo");
        }
        promoted
    }

    /// UTC date (`YYYY-MM-DD`) of the oldest QSO with an in-flight (Pending) LoTW
    /// upload — the lower bound for the own-QSO pull. `None` → nothing in flight, so
    /// the sync skips the own-echo step.
    pub fn oldest_pending_lotw_date(&self) -> Option<String> {
        self.logbook.oldest_pending_lotw_date()
    }

    /// Record a QRZ Logbook push outcome on the just-pushed QSO (`upload.qrz`), so
    /// the diagnostics can show "never uploaded to QRZ" (R1) / "QRZ upload bounced"
    /// (R9). Persists on change. Returns whether a record was stamped.
    pub fn stamp_qrz_upload(
        &mut self,
        pushed: &QsoRecord,
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<String>,
    ) -> bool {
        let status = tempo_core::logbook::UploadStatus {
            outcome,
            when_unix,
            detail,
        };
        self.recover_external_appends();
        let changed = self.logbook.stamp_qrz_upload(pushed, status);
        if changed {
            self.save_log("stamp_qrz_upload");
        }
        changed
    }

    /// Record a ClubLog realtime push outcome on the just-pushed QSO
    /// (`upload.clublog`). Persists on change. Returns whether a record was stamped.
    pub fn stamp_clublog_upload(
        &mut self,
        pushed: &QsoRecord,
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<String>,
    ) -> bool {
        let status = tempo_core::logbook::UploadStatus {
            outcome,
            when_unix,
            detail,
        };
        self.recover_external_appends();
        let changed = self.logbook.stamp_clublog_upload(pushed, status);
        if changed {
            self.save_log("stamp_clublog_upload");
        }
        changed
    }

    /// Record an eQSL ADIF-upload outcome on the just-pushed QSO (`upload.eqsl`).
    /// Persists on change. Returns whether a record was stamped.
    pub fn stamp_eqsl_upload(
        &mut self,
        pushed: &QsoRecord,
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<String>,
    ) -> bool {
        let status = tempo_core::logbook::UploadStatus {
            outcome,
            when_unix,
            detail,
        };
        self.recover_external_appends();
        let changed = self.logbook.stamp_eqsl_upload(pushed, status);
        if changed {
            self.save_log("stamp_eqsl_upload");
        }
        changed
    }

    /// Merge an eQSL confirmation report into the log. Same generic reconcile path
    /// as [`Self::merge_lotw_report`]; the award-grade distinction lives in the
    /// ADIF (eQSL carries `EQSL_QSL_RCVD`, not `QSL_RCVD`/`LOTW_QSL_RCVD`), so an
    /// eQSL confirmation lands `confirmed` but NOT `award_confirmed` by construction.
    pub fn merge_eqsl_report(&mut self, text: &str) -> tempo_core::reconcile::ReconcileSummary {
        self.recover_external_appends();
        let summary = self.logbook.merge_report(text);
        self.last_eqsl_reconcile = Some(summary.clone());
        self.save_log("merge_eqsl_report");
        summary
    }

    /// Two-way QRZ Logbook sync: merge a QRZ **FETCH** ADIF (the operator's whole
    /// book) INTO the log. QRZ returns both QSOs the operator logged elsewhere (e.g.
    /// a phone app in the field) AND confirmation status, so this runs two passes:
    /// first import genuinely-new QSOs (deduped), then reconcile confirmations onto
    /// the QSOs already present. A QRZ-native confirmation (`APP_QRZLOG_STATUS`) lands
    /// `confirmed` but NOT `award_confirmed`, by construction of the `qrz` channel, so
    /// it can't inflate DXCC/WAS counts. Returns `(added, reconcile_summary)`.
    pub fn merge_qrz_report(
        &mut self,
        text: &str,
    ) -> (usize, tempo_core::reconcile::ReconcileSummary) {
        self.recover_external_appends();
        // ONE consume-once pass: add the QSOs QRZ has that we lack AND upgrade
        // confirmations on the ones already present, keyed identically so a mode-
        // spelling difference (e.g. a phone QSO re-uploaded as USB vs our SSB) can't
        // double-log the same contact. A full save then captures both the appended
        // rows and the reconciled confirmations.
        let (added, summary) = self.logbook.merge_downloaded(text);
        self.last_qrz_reconcile = Some(summary.clone());
        self.save_log("merge_qrz_report");
        self.backfill_country();
        self.refresh_worked_index();
        (added.len(), summary)
    }

    /// A clone of all logbook records (oldest-first / newest-last).
    pub fn get_log(&self) -> Vec<QsoRecord> {
        self.logbook.records().to_vec()
    }

    /// Run the silent match-failure diagnostics over the log (Phase 1a). `resolve`
    /// maps a callsign to its DXCC entity name (for R4d's US-family gate) — the
    /// command layer passes `propagation::dxcc::resolve`, keeping the entity table
    /// out of tempo-app. Reads the last LoTW + eQSL reconcile orphans (this session).
    pub fn confirmation_diagnostics(
        &self,
        now: i64,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> tempo_core::diagnostics::DiagnosticsReport {
        let records = self.logbook.records();
        let entities: Vec<Option<String>> = records.iter().map(|r| resolve(&r.call)).collect();
        let mut recents: Vec<&tempo_core::reconcile::ReconcileSummary> = Vec::new();
        if let Some(s) = &self.last_lotw_reconcile {
            recents.push(s);
        }
        if let Some(s) = &self.last_eqsl_reconcile {
            recents.push(s);
        }
        if let Some(s) = &self.last_qrz_reconcile {
            recents.push(s);
        }
        tempo_core::diagnostics::diagnose(
            records,
            &entities,
            &recents,
            now,
            &tempo_core::diagnostics::DiagCfg::default(),
        )
    }

    /// Log indices (oldest-first) of QSOs not yet sent to LoTW: award-unconfirmed
    /// AND either never uploaded or a prior bounce. `UploadState` IS the per-QSO
    /// cursor — Pending/Accepted/Duplicate are excluded (don't re-send).
    pub fn lotw_unsent_indices(&self) -> Vec<usize> {
        self.logbook
            .records()
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.award_confirmed)
            .filter(|(_, r)| r.upload.lotw.as_ref().is_none_or(|s| !s.outcome.is_sent()))
            // LoTW matches on both operators' times agreeing (±30 min): a record
            // with NO known time can never match, so signing and sending it just
            // parks it at LoTW as unmatched forever — and, being re-sendable, it
            // kept the "Upload to LoTW (N)" count from ever clearing.
            .filter(|(_, r)| r.time_known)
            .map(|(i, _)| i)
            .collect()
    }

    /// Stamp `upload.lotw` on the given records after an upload attempt, then save.
    pub fn stamp_lotw_upload(
        &mut self,
        indices: &[usize],
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<String>,
    ) {
        // Recover another instance's appends before the full-log rewrite; the
        // recovered records land at the end, so `indices` still address the same
        // rows.
        self.recover_external_appends();
        for &i in indices {
            if let Some(r) = self.logbook.records_mut().get_mut(i) {
                r.upload.lotw = Some(tempo_core::logbook::UploadStatus {
                    outcome,
                    when_unix,
                    detail: detail.clone(),
                });
            }
        }
        self.save_log("lotw upload stamp");
    }

    /// Append a completed SSTV image to the session gallery (newest last),
    /// capped at [`SSTV_GALLERY_CAP`]. Decode-thread only.
    pub fn push_sstv_gallery(&mut self, entry: crate::dto::SstvGalleryEntry) {
        self.sstv_gallery.push(entry);
        if self.sstv_gallery.len() > SSTV_GALLERY_CAP {
            let excess = self.sstv_gallery.len() - SSTV_GALLERY_CAP;
            self.sstv_gallery.drain(0..excess);
        }
    }

    /// Attach a decoded FSK callsign ID to the newest gallery entry whose
    /// `path` matches (the image was just saved, so it's at/near the tail).
    /// Best-effort — a no-op if no entry matches (e.g. the gallery rolled past
    /// its cap before the burst decoded). Decode-thread only, like the other
    /// `sstv_gallery` mutators.
    /// Drop one image from the gallery by path. `true` when it was there. The FILE is the
    /// shell's business — this is only the in-memory index, and the two are kept in step by the
    /// single command that does both (`sstv_delete_image`), so they cannot drift the way the
    /// index and the directory used to.
    pub fn remove_sstv_gallery(&mut self, path: &str) -> bool {
        let before = self.sstv_gallery.len();
        self.sstv_gallery.retain(|e| e.path != path);
        self.sstv_gallery.len() != before
    }

    pub fn set_sstv_gallery_fsk_id(&mut self, path: &str, fsk_id: String) {
        if let Some(entry) = self.sstv_gallery.iter_mut().rev().find(|e| e.path == path) {
            entry.fsk_id = Some(fsk_id);
        }
    }

    /// Seed the session gallery from the persisted `gallery.json` (startup).
    pub fn load_sstv_gallery(&mut self, mut entries: Vec<crate::dto::SstvGalleryEntry>) {
        if entries.len() > SSTV_GALLERY_CAP {
            let excess = entries.len() - SSTV_GALLERY_CAP;
            entries.drain(0..excess);
        }
        self.sstv_gallery = entries;
    }

    /// The session SSTV gallery, oldest first.
    pub fn sstv_gallery(&self) -> &[crate::dto::SstvGalleryEntry] {
        &self.sstv_gallery
    }

    /// Set the measured PC-clock-vs-UTC offset (ms) from the NTP probe (`None`
    /// when the check is disabled or offline). Surfaced for the UI clock chip.
    pub fn set_clock_offset_ms(&mut self, ms: Option<i64>) {
        self.clock_offset_ms = ms;
    }

    /// The measured PC-clock-vs-UTC offset (ms), `local − UTC` (positive = the PC
    /// clock is ahead of UTC). `None` when the NTP check is off / offline. The
    /// radio loop subtracts this from the system clock so TX/RX slots land on the
    /// true UTC grid even when the OS clock is skewed.
    pub fn clock_offset_ms(&self) -> Option<i64> {
        self.clock_offset_ms
    }

    /// Drain the WSJT-X-format ALL.TXT lines buffered since the last call (the shell
    /// appends them to the on-disk log). Empty when ALL.TXT logging is off.
    pub fn take_all_txt_pending(&mut self) -> Vec<String> {
        std::mem::take(&mut self.all_txt_pending)
    }

    /// Export the **general** logbook (Chat/QSO contacts, any mode) as ADIF or
    /// CSV. Independent of Field Day's contest log (`Engine::export_log`).
    pub fn export_logbook(&self, format: &str) -> String {
        match format.to_ascii_lowercase().as_str() {
            "csv" => self.logbook.csv(),
            _ => self.logbook.adif(),
        }
    }
}

#[cfg(test)]
mod grid_tests {
    use super::*;
    use tempo_core::logbook::QsoRecord;

    fn rec(call: &str, band: &str, grid: &str) -> QsoRecord {
        QsoRecord {
            call: call.into(),
            grid: Some(grid.into()),
            country: None,
            state: None,
            band: band.into(),
            freq_mhz: 14.074,
            mode: "FT8".into(),
            rst_sent: None,
            rst_rcvd: None,
            name: None,
            qth: None,
            comment: None,
            notes: None,
            tx_power: None,
            when_unix: 1_700_000_000,
            time_off_unix: None,
            confirmed: false,
            award_confirmed: false,
            qsl_rcvd: Default::default(),
            qsl_sent: Default::default(),
            credit_granted: Vec::new(),
            credit_submitted: Vec::new(),
            upload: Default::default(),
            ota: Default::default(),
            time_known: true,
            dxcc: None,
            prop_mode: None,
            sat_name: None,
            operator: None,
            station_callsign: None,
            extra: Vec::new(),
        }
    }

    #[test]
    fn a_six_char_logged_grid_matches_a_four_char_decode() {
        // The bug this pins: QRZ/LoTW imports log "FN31PR" while a decode carries "FN31",
        // so the index and the lookup never met and NEW GRID fired forever on every such
        // square. Both sides now normalize to the 4-char square grids are awarded at.
        let mut sc = StationCore::new();
        sc.logbook.add(rec("W1AW", "20m", "FN31PR"));
        sc.refresh_worked_index();

        assert!(
            sc.grid_worked_on("FN31", "20m"),
            "a 6-char logged grid must satisfy a 4-char decode on the same band"
        );
        // And the per-band rule still holds on top of it.
        assert!(
            !sc.grid_worked_on("FN31", "2m"),
            "worked on 20m only — still NEW on 2m"
        );
    }

    #[test]
    fn a_malformed_grid_never_counts_as_worked() {
        let mut sc = StationCore::new();
        sc.logbook.add(rec("W1AW", "20m", "FN"));
        sc.refresh_worked_index();
        assert!(
            !sc.grid_worked_on("FN", "20m"),
            "a 2-char fragment is not a square"
        );
    }

    /// 57bd9dba put a `recover_external_appends` in front of `import_adif` — right,
    /// and it stays — but the APPEND branch below it left the freshness fingerprint
    /// pointing at the file as it was BEFORE our own append. So the gate missed on
    /// every later call, and companion mode (one `import_adif` per QSO WSJT-X logs)
    /// re-parsed the whole log from disk on every contact: measured 132.7 ms per QSO
    /// against a 26,007-record / 3.67 MB log, forever, not once.
    #[test]
    fn our_own_import_append_leaves_the_recovery_gate_shut() {
        use tempo_core::logbook::{adif_header, adif_record};
        let path =
            std::env::temp_dir().join(format!("nexus_append_stamp_{}.adi", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            format!(
                "{}{}",
                adif_header(),
                adif_record(&rec("W1AW", "20m", "FN31"))
            ),
        )
        .unwrap();

        let mut sc = StationCore::new();
        sc.set_log_path(path.clone());
        assert!(
            sc.recover_external_appends(),
            "the first look reads the file"
        );
        assert!(
            !sc.recover_external_appends(),
            "...and shuts the gate behind it"
        );

        // A companion-logged QSO: one contact we lack, nothing already held to
        // upgrade, so the append branch runs and not the full rewrite.
        let (added, _, merged, _) = sc.import_adif(
            "<EOH>\n<CALL:5>K5XYZ<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260804<TIME_ON:6>120000<EOR>\n",
        );
        assert_eq!((added, merged), (1, 0), "the append path, not the rewrite");

        assert!(
            !sc.recover_external_appends(),
            "our own append must not reopen the gate — every later call re-parses the whole log"
        );
        assert_eq!(
            sc.logbook.len(),
            2,
            "and nothing was re-read or double-counted"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The other half, and it is the half that must never be traded for the first:
    /// an append may carry the fingerprint forward ONLY while we can account for
    /// every byte on disk. Here a second instance wrote after our last look and we
    /// append without looking again — exactly the `Engine::log_qso` shape, which has
    /// no recovery in front of it. Stamping there would record a file we do not hold,
    /// the gate would skip, and the next full rewrite would delete the other
    /// instance's QSO: the fault 57bd9dba exists to prevent. The stamp must be
    /// DROPPED instead, so the next look re-reads.
    #[test]
    fn an_append_onto_a_file_that_moved_under_us_drops_the_fingerprint() {
        use tempo_core::logbook::{adif_header, adif_record};
        let path =
            std::env::temp_dir().join(format!("nexus_append_stale_{}.adi", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            format!(
                "{}{}",
                adif_header(),
                adif_record(&rec("W1AW", "20m", "FN31"))
            ),
        )
        .unwrap();

        let mut sc = StationCore::new();
        sc.set_log_path(path.clone());
        assert!(sc.recover_external_appends(), "we hold W1AW, gate shut");

        // Instance A appends a contact we never see in memory.
        Logbook::append(&path, &rec("W3CCC", "40m", "IO91")).unwrap();

        // We log our own contact — memory first, then the file, as log_qso does.
        let k = rec("K5XYZ", "20m", "FN31");
        sc.logbook.add(k.clone());
        sc.append_to_log(std::slice::from_ref(&k));
        assert!(
            sc.last_log_mtime.is_none(),
            "a file we cannot account for must not be stamped as ours"
        );
        assert!(
            sc.recover_external_appends(),
            "so the next look re-reads and folds A's QSO in"
        );

        // ...and the full rewrite that follows keeps it.
        sc.save_log("test");
        let on_disk = Logbook::load(&path);
        let calls: Vec<&str> = on_disk.records().iter().map(|r| r.call.as_str()).collect();
        assert!(
            calls.contains(&"W3CCC"),
            "another instance's append survives our rewrite (on disk: {calls:?})"
        );
        assert_eq!(on_disk.len(), 3, "...and nothing is double-logged");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn freshness_watcher_folds_in_another_instances_appends_and_no_ops_when_unchanged() {
        use tempo_core::logbook::{adif_header, adif_record};
        let path =
            std::env::temp_dir().join(format!("nexus_sync_watcher_{}.adi", std::process::id()));
        let write = |recs: &[QsoRecord]| {
            let mut s = adif_header();
            for r in recs {
                s.push_str(&adif_record(r));
            }
            std::fs::write(&path, s).unwrap();
        };
        // The shared log starts with just QSO X.
        let x = rec("DL1ABC", "20m", "JO31");
        write(std::slice::from_ref(&x));
        let mut sc = StationCore::new();
        sc.log_path = Some(path.clone());

        // First look (last mtime = None) folds X in and indexes it.
        assert!(
            sc.sync_shared_log_if_changed(),
            "first look reads the shared log"
        );
        assert_eq!(sc.logbook.len(), 1);
        assert!(sc.grid_worked_on("JO31", "20m"), "X is now worked-before");

        // The OTHER instance appends QSO Y. Force the gate (mtime granularity is coarse in a
        // fast test); the point under test is the reconcile+refresh, which the gate triggers.
        let y = rec("JA1XYZ", "40m", "PM95");
        write(&[x, y]);
        sc.last_log_mtime = None;
        assert!(sc.sync_shared_log_if_changed(), "a changed log is re-read");
        assert_eq!(
            sc.logbook.len(),
            2,
            "the other instance's new QSO is folded in"
        );
        assert!(
            sc.grid_worked_on("PM95", "40m"),
            "Y is now worked-before without a restart"
        );

        // Nothing changed → a cheap no-op (stat only), so it's safe on every Needed-board poll.
        assert!(
            !sc.sync_shared_log_if_changed(),
            "unchanged mtime → no re-read"
        );

        let _ = std::fs::remove_file(&path);
    }
}
