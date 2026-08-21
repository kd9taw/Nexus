// Pure helpers for the mode-aware Needed board + click-to-work. No React, no IO —
// fully node-testable. The backend emits CW/Phone needs unconditionally (with an exact
// frequency); these gate them by the operator's enabled modes and resolve a click into
// a concrete QSY + cockpit target.

import type { BandChannel, GridRarity, NeedAlert, NeedTag } from '../types'
// ONE definition of what 'vhf' means (≥ 50 MHz), shared with the sound/toast path. The
// alert module owns it; a second copy here is exactly how the icons and the alerts drifted
// apart in the first place. (alerts.ts touches WebAudio only inside its functions, so this
// stays node-testable.)
import { bandScopeOk } from '../alerts'
import { bandRangeForLabel } from '../band'

/**
 * Chase-importance weight per need tag — the UI mirror of the backend's `NeedTag::tier()`
 * (crates/propagation/src/needalert.rs), carrying its VERBATIM numbers rather than an
 * ordinal shorthand. Keeping the real values puts a row ranked from a bare tag and a row
 * ranked from a backend `priority` on ONE scale, so the two paths in [`chaseRank`] can never
 * disagree about which of two stations matters more.
 *
 * The gradient: a call the operator asked for by name > a new entity > new zone > new state
 * > new grid > new band (entity worked, this band not) > new mode (band worked, this mode
 * class not) > worked-but-unconfirmed. Dxped/Pota/Sota are never a primary tier — the
 * backend appends them onto an existing award need and expresses their pull as a priority
 * bump instead, so they weigh 0 here exactly as they do there.
 *
 * NOTE there is no "new band-mode" rank, here or in the backend: `LogNeeds::need` decides
 * the DXCC axis with a short-circuit that never consults mode once the band is missing, so
 * a slot needed on band AND mode reports NewBand alone (dxped.rs). Band and mode needs are
 * mutually exclusive, not combinable.
 */
export const NEED_TIER: Record<NeedTag, number> = {
  Wanted: 120,
  NewEntity: 100,
  NewZone: 70,
  NewState: 60,
  NewGrid: 55,
  NewBand: 50,
  NewMode: 30,
  Confirm: 10,
  Dxped: 0,
  Pota: 0,
  Sota: 0,
}

/**
 * The strongest of one call's alerts. A station heard on several bands carries one alert per
 * band/mode, and what decides whether to work it now is its BEST reason, never an arbitrary
 * one. Highest `priority` wins; a tie keeps the earlier entry, which is the stronger one
 * given the backend hands these out priority-descending. Alerts with no tags are skipped —
 * they carry no reason to work anyone.
 */
export function strongestNeed(alerts: NeedAlert[] | null | undefined): NeedAlert | null {
  let best: NeedAlert | null = null
  for (const a of alerts ?? []) {
    if (a.tags.length === 0) continue
    if (best == null || a.priority > best.priority) best = a
  }
  return best
}

/**
 * Group gated alerts per UPPERCASE callsign — THE one grouping every host
 * (docked App, pop-out DetachedPanel, any future surface) derives its
 * colour/badge maps from. The pop-out once re-rolled this by hand with the
 * pre-fix last-tag-wins loop, so the torn-off band map coloured a new entity
 * as the weakest need while the docked window showed it correctly.
 */
export function alertsByCall(alerts: NeedAlert[]): Map<string, NeedAlert[]> {
  const m = new Map<string, NeedAlert[]>()
  for (const a of alerts) {
    const k = a.call.toUpperCase()
    const arr = m.get(k)
    if (arr) arr.push(a)
    else m.set(k, [a])
  }
  return m
}

/**
 * Activity TYPE per heard call (POTA / SOTA / DXpedition), independent of the
 * need tier: topNeedByCall keeps only the strongest tag, so a POTA activator
 * that is ALSO a new band would lose its program badge to the band colour —
 * this map surfaces the program regardless.
 */
export function activityTypeByCall(alerts: NeedAlert[]): Map<string, 'Pota' | 'Sota' | 'Dxped'> {
  const m = new Map<string, 'Pota' | 'Sota' | 'Dxped'>()
  for (const a of alerts) {
    const k = a.call.toUpperCase()
    if (m.has(k)) continue
    const t = a.tags.find((x) => x === 'Pota' || x === 'Sota' || x === 'Dxped')
    if (t) m.set(k, t as 'Pota' | 'Sota' | 'Dxped')
  }
  return m
}

/**
 * Top need tag per UPPERCASE callsign, taken from each call's STRONGEST alert — the map the
 * roster, band strip and map colour their rows from.
 *
 * The strongest, not an arbitrary one. A call heard on several bands carries one alert per
 * band/mode, and the map this replaces was built by writing every alert into the same slot
 * with no guard, so the LAST one won; since the backend hands alerts out priority-descending
 * that was reliably the WEAKEST. A new entity on 20 m that also wanted a confirmation on
 * 40 m was therefore coloured as the confirmation.
 *
 * Calls whose every alert is tagless are omitted, so `has(call)` still means "something is
 * needed here".
 */
export function topNeedByCall(alertsByCall: Map<string, NeedAlert[]>): Map<string, NeedTag> {
  const m = new Map<string, NeedTag>()
  for (const [call, alerts] of alertsByCall) {
    const tag = strongestNeed(alerts)?.tags[0]
    if (tag) m.set(call, tag)
  }
  return m
}

/**
 * How badly the operator should want this station right now — the weight behind "sort by
 * need".
 *
 * Prefers the backend's own `priority`, which is exactly what the Needed board sorts on
 * (NeededPanel's 'priority' key), so the roster and the board rank a station identically.
 * That number is `tier()` plus the grid-rarity boost plus the DXped/OTA bumps, so an
 * ultra-rare grid or a live park activation keeps the pull the backend gave it instead of
 * collapsing back into its bare tag.
 *
 * Falls back to the tag's tier for hosts that pass only the top-tag map, and to 0 for a
 * station with nothing needed — below every real need, the OTA floor of 20 included.
 */
export function chaseRank(
  alerts: NeedAlert[] | null | undefined,
  tag: NeedTag | null | undefined,
): number {
  const best = strongestNeed(alerts)
  if (best) return best.priority
  return tag ? NEED_TIER[tag] : 0
}

/**
 * The operator's per-type alert BAND SCOPES (Settings ▸ Spots & Alerts), as the need
 * VISUALS read them. Each is 'off' | 'hf' | 'vhf' | 'all'; absent = use that type's default.
 *
 * ⭐ These settings gated the sound and the toast (alerts.ts) and NOTHING ELSE, so a grid
 * scope of VHF+ left the GRID icon painting HF rows on the roster and the decode feed —
 * reported twice ("its still showing the grid icons in ft8 in both roster and classic mode
 * when on hf bands"). The scope is one operator intent — "grid chasing is a VHF thing" —
 * and it governs both halves through the SAME `bandScopeOk`.
 */
export interface NeedBandScopes {
  /** `settings.alertDxccBands` — the new-entity (ATNO) scope. Default 'all'. */
  dxcc?: string
  /** `settings.alertGridBands` — a plain new grid. Default 'vhf'. */
  grid?: string
  /** `settings.alertRareGridBands` — a rare/ultra-rare 💎 grid. Default 'vhf'. */
  rareGrid?: string
}

/** The three need kinds a band scope has anything to say about. Everything else — a new
 * band-slot, a zone, a state, a mode, a watch-list hit — is unscoped and never withheld. */
export type ScopedNeedKind = 'dxcc' | 'grid' | 'rareGrid'

/** A grid rare enough to follow the SEPARATE 💎 scope — the same two tiers the alert path
 * tests inline (alerts.ts `isRareGrid`); it cannot import this module without a cycle. */
export function isRareGrid(rarity: GridRarity | null | undefined): boolean {
  return rarity === 'rare' || rarity === 'ultraRare'
}

/**
 * The MHz a need should be judged at: the spot's own frequency when it carried one, else
 * the bottom edge of its band label (`band.ts` owns that table — no second copy). Returns
 * undefined when neither resolves, which `bandScopeOk` reads as "band unknown" and passes.
 *
 * The alert's OWN band, not the dial, because the roster and the band map show stations
 * heard on other bands; a 6 m grid need must keep its icon while the rig sits on 20 m. No
 * band straddles the 50 MHz line, so the band's bottom edge decides HF-vs-VHF exactly as
 * the live dial would.
 */
export function needScopeMhz(band: string, freqMhz?: number | null): number | undefined {
  if (freqMhz != null && freqMhz > 0) return freqMhz
  return bandRangeForLabel((band ?? '').trim().toLowerCase())?.lo
}

/**
 * Does the operator's band scope admit this need kind at `mhz`? No scopes supplied (a host
 * that has not loaded settings yet) = permissive: a missing icon is worse than an extra one,
 * and that is the stance `bandScopeOk` already takes for an unknown dial.
 *
 * A rare grid is admitted when EITHER grid scope opens — the same rule the toast tier uses,
 * so silencing plain HF grids keeps the gems and vice versa.
 */
export function needScopeOk(
  kind: ScopedNeedKind,
  mhz: number | undefined,
  scopes: NeedBandScopes | undefined,
): boolean {
  if (!scopes) return true
  if (kind === 'dxcc') return bandScopeOk(scopes.dxcc, mhz, 'all')
  const plain = bandScopeOk(scopes.grid, mhz, 'vhf')
  if (kind === 'grid') return plain
  return bandScopeOk(scopes.rareGrid, mhz, 'vhf') || plain
}

/** Which need rows are visible given the enabled operating-mode features, with each row
 * narrowed to the tags the operator's band scopes admit on ITS band.
 *
 * Digital needs always show; CW/Phone needs only when that mode is enabled — so a
 * pure-digital op's board is unchanged even though the backend sends voice/CW needs too.
 *
 * The band scope filters TAGS, not alerts: one station can be a new BAND (unscoped) and a
 * new GRID (scoped out on HF) at once, and dropping the whole alert would take its band
 * icon and its roster row with it. An alert left with no tag at all carries no reason to
 * work anyone, so it goes. Omit `scopes` and only the mode gate applies. */
export function visibleNeeds(
  alerts: NeedAlert[],
  enabled: { cw: boolean; phone: boolean },
  scopes?: NeedBandScopes,
): NeedAlert[] {
  const out: NeedAlert[] = []
  for (const a of alerts) {
    if (a.mode === 'CW' && !enabled.cw) continue
    if (a.mode === 'Phone' && !enabled.phone) continue // Digital/unknown always visible
    if (!scopes) {
      out.push(a)
      continue
    }
    const mhz = needScopeMhz(a.band, a.freqMhz)
    const rare = isRareGrid(a.gridRarity)
    const tags = a.tags.filter((t) => {
      if (t === 'NewEntity') return needScopeOk('dxcc', mhz, scopes)
      if (t === 'NewGrid') return needScopeOk(rare ? 'rareGrid' : 'grid', mhz, scopes)
      return true
    })
    if (tags.length === 0) continue
    out.push(retag(a, tags))
  }
  return out
}

/** A resolved click-to-work target: where to QSY and the cockpit to open. The CALLER
 * owns the rig sideband when it QSYs — the rig-mode policy derives the actual CAT mode
 * (CW, USB/LSB-by-band for phone, or DATA-U for digital) from the operating mode, so we
 * never compute it here. */
export interface WorkTarget {
  call: string
  /** Cockpit view to open. 'operate' = the digital (FT8/FT4) cockpit; its operating-mode
   * argument is 'digital'. CW/Phone/RTTY map 1:1 to their cockpit + operating mode. */
  view: 'cw' | 'phone' | 'rtty' | 'operate'
  freqMhz: number
  band: string
}

/** Coarse operating-mode CLASS for a source-reported mode string — the router for
 * click-to-work (which cockpit + rig-mode policy). Voice modes → 'Phone'; CW → 'CW';
 * everything else (FT8/FT4/RTTY/PSK/unknown/null) → 'Digital'. Mirrors the backend's
 * NeedAlert.mode classes so map spots and Needed rows route identically. */
export function modeClassOf(mode: string | null | undefined): 'CW' | 'Phone' | 'Digital' {
  const m = (mode ?? '').trim().toUpperCase()
  if (m === 'CW') return 'CW'
  // Accept both ADIF tokens (SSB/USB/…) AND our own class LABEL ("PHONE") — a spot's mode
  // can be either, and missing PHONE here silently routed a phone need to the Digital cockpit.
  if (m === 'SSB' || m === 'USB' || m === 'LSB' || m === 'FM' || m === 'AM' || m === 'PHONE')
    return 'Phone'
  return 'Digital'
}

/** Band labels compare case- and whitespace-insensitively. A real logbook carries both
 * `30m` and `30M` (the operator's audited log has 699 of one and 212 of the other), and a
 * spot's label is canonicalised by the backend while the radio's comes from settings — a
 * raw `===` between the two silently drops every need on the band.
 *
 * An ABSENT band on either side is UNKNOWN, and unknown matches nothing — not a band, and
 * not another unknown. Off the ham bands (WWV, shortwave, CB, marine, the gaps between band
 * edges — a first-class supported use case, operator ruling 2026-08-13) the backend states
 * "no ham-band claim" by sending `radio.band` as the empty string. Read as a NAME, `'' === ''`
 * was a match, so every alert that carried no band of its own passed the per-band gate below
 * as a fully-qualified per-band claim: NewBand/NewZone/NewGrid/NewState/Confirm chips painted
 * on every roster row off band, choosing the row colour and its place in "sort by need". The
 * honest answer to "is this claim about the band in front of me?" with either side unknown is
 * no — which withholds the per-band claims and leaves the band-agnostic ones (an all-time-new
 * entity is new wherever you hear it) working exactly as they did. */
export function sameBand(a: string | null | undefined, b: string | null | undefined): boolean {
  const x = (a ?? '').trim().toUpperCase()
  const y = (b ?? '').trim().toUpperCase()
  if (!x || !y) return false
  return x === y
}

/** Need tags that survive on ANY band, because the predicate behind them carries no band:
 * an all-time-new entity is new everywhere; a DXpedition/POTA/SOTA/wanted flag is a
 * property of the station. `NewMode` belongs here too — the backend keys `worked_mode` as
 * (entity, mode-class) with no band, so a mode need is closable on whatever band you hear
 * the station on. Everything else (NewBand, and NewZone/NewGrid/NewState for 5BWAZ/VUCC/
 * 5BWAS, plus Confirm from the per-band `confirmed_band` set) is a per-band claim and must
 * match the band in front of the operator. */
const BAND_AGNOSTIC_TAGS: ReadonlySet<NeedTag> = new Set<NeedTag>([
  'NewEntity',
  'NewMode',
  'Dxped',
  'Pota',
  'Sota',
  'Wanted',
])

/** Need tags that are additionally MODE-specific: a CW-only "new mode", or an
 * unconfirmed contact, cannot be closed from a digital surface. NewBand is deliberately
 * absent — a band slot closes in any mode. */
const MODE_GATED_TAGS: ReadonlySet<NeedTag> = new Set<NeedTag>(['NewMode', 'Confirm'])

/**
 * The need tags an alert may legitimately claim on a surface showing stations heard on
 * `band` in `feedMode`. This is the gate the call-keyed need maps were missing.
 *
 * Field report 2026-07-29: the Needed system flagged a "new mode on 30m" for Asiatic
 * Russia against an operator with six 30m FT8 contacts there. The backend need was
 * genuine — they have never worked that entity on CW — but the roster/station-list chips
 * are keyed by CALLSIGN alone and unioned every alert's tags with no band or mode filter,
 * so a CW alert painted an unqualified MODE chip onto a 30m FT8 screen. Both sides go
 * through `modeClassOf` because the backend sends the specific submode (`FT8`/`FT4`/
 * `RTTY`) as a display label while a feed describes itself by class (`Digital`) — a raw
 * `===` between those two is never true, which dropped real pills on the decode feed.
 *
 * Off the ham bands the surface's `band` is `''` — unknown, not a band — so `sameBand` returns
 * false for every per-band claim and only the band-agnostic tags survive. That is the ruling
 * (2026-08-13): "an all-time new one" is answerable without knowing the band; "new on this
 * band" is not.
 */
export function tagsForSurface(alert: NeedAlert, band: string, feedMode: string): NeedTag[] {
  const sameModeClass = modeClassOf(alert.mode) === modeClassOf(feedMode)
  const sameBandLabel = sameBand(alert.band, band)
  return alert.tags.filter((t) => {
    if (MODE_GATED_TAGS.has(t) && !sameModeClass) return false
    return BAND_AGNOSTIC_TAGS.has(t) || sameBandLabel
  })
}

/**
 * The alerts that still say something on a surface showing `band` in `feedMode`, each
 * rewritten to only the tags it may claim there. An alert left with no applicable tag is
 * dropped entirely — it carries no reason to work this station HERE.
 *
 * This is the composition point for the two halves of the need pipeline: the surface gate
 * decides what is ELIGIBLE, then [`strongestNeed`]/[`chaseRank`] rank what survives. Feeding
 * ungated alerts into the ranking would let a need the operator cannot close here choose the
 * row's colour and its place in the sort.
 *
 * `priority` is repaired when the gate removes the alert's LEAD tag. The backend computes
 * priority from `tags[0]` (plus the grid-rarity and OTA bumps), so once that tag is withheld
 * the number describes a claim this surface is not making — a 30m CW alert tagged
 * [NewMode, NewZone] seen on the 30m FT8 roster keeps its genuine new-zone need, but its
 * priority of 30 would rank that zone below a bare new band. Falling back to the surviving
 * lead's `NEED_TIER` re-states it honestly. The bumps are deliberately not carried over:
 * they were earned by the tag that just went away.
 */
export function alertsForSurface(
  alerts: NeedAlert[] | null | undefined,
  band: string,
  feedMode: string,
): NeedAlert[] {
  const out: NeedAlert[] = []
  for (const a of alerts ?? []) {
    const tags = tagsForSurface(a, band, feedMode)
    if (tags.length === 0) continue
    out.push(retag(a, tags))
  }
  return out
}

/** Rewrite an alert to a NARROWED tag set, repairing `priority` when the gate removed the
 * lead tag (see [`alertsForSurface`] for why). Shared by both gates — the surface gate and
 * the band-scope gate — so a withheld tag can never leave its number behind. */
function retag(a: NeedAlert, tags: NeedTag[]): NeedAlert {
  return tags[0] === a.tags[0] ? { ...a, tags } : { ...a, tags, priority: NEED_TIER[tags[0]] }
}

/** Resolve ANY need (CW / Phone / Digital) into a work target — N1MM-style: a single click
 * changes the band, mode, AND frequency to exactly the spot's. Uses the spot's exact
 * frequency when the cluster/RBN carried one, else the band's default channel. Returns null
 * only when no frequency can be resolved at all (a band-level need with no band-plan entry). */
/** CW / Phone ACTIVITY frequencies per band (MHz) — where the mode lives. Used ONLY when a
 * CW/Phone need carries no exact spot frequency, so click-to-work QSYs to the right part of
 * the band instead of the tier's DIGITAL dial (which parked CW/phone clicks on 14.074 / 21.074
 * etc.). CW mirrors bandplan::cw_activity_mhz; Digital falls through to the tier's FT8 dial. */
const CW_ACTIVITY_MHZ: Record<string, number> = {
  '160m': 1.81, '80m': 3.55, '40m': 7.03, '30m': 10.11, '20m': 14.03,
  '17m': 18.08, '15m': 21.03, '12m': 24.9, '10m': 28.03, '6m': 50.09,
}
const PHONE_ACTIVITY_MHZ: Record<string, number> = {
  '160m': 1.9, '80m': 3.8, '40m': 7.2, '20m': 14.25,
  '17m': 18.14, '15m': 21.3, '12m': 24.96, '10m': 28.4, '6m': 50.15,
}
function modeDefaultMhz(band: string, mode: string): number | null {
  if (mode === 'CW') return CW_ACTIVITY_MHZ[band] ?? null
  if (mode === 'Phone') return PHONE_ACTIVITY_MHZ[band] ?? null
  return null // Digital → the tier's FT8 dial is correct
}

export function workTarget(alert: NeedAlert, bandPlan: BandChannel[]): WorkTarget | null {
  const view: 'cw' | 'phone' | 'rtty' | 'operate' =
    alert.mode === 'CW' ? 'cw' : alert.mode === 'Phone' ? 'phone' : alert.mode === 'RTTY' ? 'rtty' : 'operate'
  // Prefer the spot's exact frequency; for a freq-less CW/Phone need, the band's CW/phone
  // ACTIVITY freq — NOT the tier's digital dial (that's what sent CW/phone clicks to FT8).
  const freqMhz =
    alert.freqMhz ??
    modeDefaultMhz(alert.band, alert.mode) ??
    bandPlan.find((c) => c.band === alert.band)?.dialMhz ??
    null
  if (freqMhz == null) return null
  return { call: alert.call, view, freqMhz, band: alert.band }
}
