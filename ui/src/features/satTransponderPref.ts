// Per-bird transponder choice: the one the operator picked LAST, and an optional ★ FAVORITE
// that wins over it. An operator who only ever works one mode on a bird (the ISS FM repeater)
// should not have to find it again in the chooser on every pass.
//
// WHAT IT DECIDES, AND WHAT IT NEVER DOES. The choice only says WHICH row "Work this pass"
// picks when it picks one, and which row the chooser keeps on screen and marks. It never makes a
// pick by itself: nothing here calls set_sat_transponder, so storing, starring or loading a
// choice never moves the radio. Every rule the auto-pick already obeys still holds around it —
// it never runs over a transponder the engine is already holding, nor over the operator's
// explicit "None" (SatellitesView `workPass`).
//
// IDENTITY. A transponder is named by its uplink, its downlink and its mode, never by its place
// in the SatNOGS list: the list reorders and gains entries, and an index that pointed at the FM
// repeater yesterday can point at a different uplink today. (SatNOGS has a per-transmitter uuid,
// but the app does not carry it to the screen.) Mode is part of the identity: two rows on the same
// frequencies in different modes are different transponders. Two rows that match on all three are
// taken as one, and the first is picked; the per-leg modes and the inverting flag are not compared
// (no listed bird differs only there today).
//
// WORKABLE ONLY. Only an alive transponder or repeater can be saved or resolved — the same rule
// as the auto-pick. A saved one that SatNOGS no longer lists as working resolves to nothing,
// so the usual pick stands, and `gone` names it for one quiet line in the chooser. It stays
// saved: a SatNOGS outage or a transmitter coming back finds it again.
//
// STORAGE. One durable STATION key (durableStore `STATION_KEYS`), beside the ★ birds: it is in
// ui-state.json, so it is in a backup and a computer driving the station reads the station's
// own. Keyed by the upper-case bird name, the ★ set's identity. Validated on every read: a
// corrupt or hand-edited file yields nothing saved, never a half-read entry.

import { durableGet, durableSet } from './durableStore'
import type { SatTransmitter } from '../types'

const KEY = 'nexus.sats.transponders'
/** A description longer than this is not one; it is cut rather than refused. */
const LABEL_MAX = 200

/** One saved transponder: what identifies it, and the description it had when saved (shown
 *  only when it is gone). */
export interface SatTpRef {
  up: number | null
  down: number | null
  mode: string | null
  label: string
}

export interface SatTpPref {
  fav?: SatTpRef
  last?: SatTpRef
}

/** An alive transponder (linear) or transceiver (FM repeater): something to work. Never a
 *  beacon, never a dead entry. */
export function isWorkableTp(tx: SatTransmitter): boolean {
  return tx.alive && (tx.kind === 'Transponder' || tx.kind === 'Transceiver')
}

function readHz(v: unknown): number | null | undefined {
  if (v === null) return null
  return typeof v === 'number' && Number.isFinite(v) && v > 0 ? v : undefined
}

function readRef(v: unknown): SatTpRef | null {
  if (!v || typeof v !== 'object' || Array.isArray(v)) return null
  const o = v as Record<string, unknown>
  const up = readHz(o.up)
  const down = readHz(o.down)
  if (up === undefined || down === undefined) return null
  // A ref with neither leg identifies nothing.
  if (up == null && down == null) return null
  if (o.mode !== null && typeof o.mode !== 'string') return null
  return {
    up,
    down,
    mode: o.mode as string | null,
    label: typeof o.label === 'string' ? o.label.slice(0, LABEL_MAX) : '',
  }
}

/** Every saved choice by upper-case bird name. Empty when storage is blocked or corrupt. */
export function satTpPrefs(): Record<string, SatTpPref> {
  try {
    const raw = durableGet(KEY)
    if (!raw) return {}
    const obj = JSON.parse(raw)
    if (!obj || typeof obj !== 'object' || Array.isArray(obj)) return {}
    const out: Record<string, SatTpPref> = {}
    for (const [bird, v] of Object.entries(obj as Record<string, unknown>)) {
      if (!v || typeof v !== 'object' || Array.isArray(v)) continue
      const fav = readRef((v as Record<string, unknown>).fav)
      const last = readRef((v as Record<string, unknown>).last)
      if (fav == null && last == null) continue
      const p: SatTpPref = {}
      if (fav) p.fav = fav
      if (last) p.last = last
      out[bird.toUpperCase()] = p
    }
    return out
  } catch {
    return {}
  }
}

function save(map: Record<string, SatTpPref>): void {
  try {
    durableSet(KEY, JSON.stringify(map))
  } catch {
    /* storage blocked — nothing is remembered, and nothing else changes */
  }
}

function refOf(tx: SatTransmitter): SatTpRef {
  return {
    up: tx.uplinkLowHz,
    down: tx.downlinkLowHz,
    mode: tx.mode,
    label: tx.description.slice(0, LABEL_MAX),
  }
}

/** Does `tx` put the radio where `ref` does? */
export function tpMatches(ref: SatTpRef, tx: SatTransmitter): boolean {
  return ref.up === tx.uplinkLowHz && ref.down === tx.downlinkLowHz && ref.mode === tx.mode
}

/** Remember the operator's pick on `bird`. A row that is not workable is not remembered. */
export function rememberSatTp(bird: string, tx: SatTransmitter): void {
  if (!isWorkableTp(tx)) return
  const map = satTpPrefs()
  const key = bird.toUpperCase()
  map[key] = { ...map[key], last: refOf(tx) }
  save(map)
}

/** Star `tx` as the bird's favorite, or clear the star when it already is. Returns the NEW
 *  state (true = it is the favorite now). A row that is not workable cannot be starred. */
export function toggleSatTpFavorite(bird: string, tx: SatTransmitter): boolean {
  if (!isWorkableTp(tx)) return false
  const map = satTpPrefs()
  const key = bird.toUpperCase()
  const p: SatTpPref = { ...map[key] }
  const on = !(p.fav && tpMatches(p.fav, tx))
  if (on) p.fav = refOf(tx)
  else delete p.fav
  if (p.fav || p.last) map[key] = p
  else delete map[key]
  save(map)
  return on
}

export interface SatTpResolved {
  /** The RAW index into `transmitters` to prefer, or null (the usual pick stands). */
  index: number | null
  source: 'favorite' | 'last' | null
  /** The description of a saved transponder the list no longer has as workable, or null. */
  gone: string | null
}

/** Which of `transmitters` (the get_sat_detail list, dead entries included) the saved choice
 *  for `bird` names: the favorite first, then the last pick. An empty list says nothing
 *  (SatNOGS not fetched yet), so nothing is called gone. */
export function resolveSatTp(
  bird: string,
  transmitters: readonly SatTransmitter[],
  prefs: Record<string, SatTpPref>,
): SatTpResolved {
  const p = prefs[bird.toUpperCase()]
  if (p == null || transmitters.length === 0) return { index: null, source: null, gone: null }
  let gone: string | null = null
  for (const [source, ref] of [['favorite', p.fav], ['last', p.last]] as const) {
    if (ref == null) continue
    const index = transmitters.findIndex((tx) => isWorkableTp(tx) && tpMatches(ref, tx))
    if (index >= 0) return { index, source, gone }
    gone ??= ref.label
  }
  return { index: null, source: null, gone }
}
