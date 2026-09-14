// PER-INTENT MAP SETUP — each Connect intent (Chase DX, POTA/SOTA, Ragchew, 6m/VHF) keeps its own
// map pick (Globe · 3D · Flat · Beam — the one picker), layer picks and colour mode.
//
// Before this, the map held ONE projection and ONE layer table (`nexus.connect.projection`,
// `nexus.connect.layers`) and the colour mode was not stored at all. Switching intent wrote that
// intent's preset over the shared values, so an operator who tuned Chase DX, looked at POTA/SOTA
// and came back found Chase DX reset to its preset (tester report, 2026-09-13). Now a preset is a
// FIRST-USE default only: the first time an intent is opened on a surface its preset applies, and
// every later visit restores what the operator left there.
//
// ONE per-surface key holding every intent's record, not a key per intent: MapView (layers, colour)
// and ConnectView (the map pick) write different halves of the same record, so each write is
// read-merge-write and neither can clobber the other's half.
//
// ## The map pick
//
// Records written before the picker held a projection (`kind`) and a separate 2-D/3-D flag
// (`map3d`). They collapse into the one `map` choice on read: `map3d: true` → 3D, otherwise the
// stored kind, and anything unrecognised → Globe. A record is rewritten in the new shape the next
// time anything in it is saved.
//
// ## Surface inheritance (see features/windowScope.ts and `MapView`'s `dedicatedIntent`)
//
// A torn-off Connect map INHERITS the primary surface's record on first open (`surfaceGet`), the
// #199 carry-over, exactly as the old projection/layer keys did. A surface DEDICATED to one intent
// (the POTA map pop-out) reads and migrates only what was written ON ITSELF: an inherited record is
// another surface's setup, and inheriting the old shared layer table is precisely what once opened
// the POTA map with Parks off.
//
// ## Migration
//
// The first read on a surface that has no record migrates the old shared values into the intent
// that is active at that moment, so nobody loses their setup on upgrade. The other intents then
// get their presets on first use. The legacy keys are read, never written again. The legacy 3-D flag
// wins over the legacy projection, exactly as `map3d: true` does inside a record.
import type { MapIntent } from '../components/MapView'
import type { Projection } from '../mapGeo'
import { surfaceGet, surfaceHasOwn, surfaceSet } from './windowScope'

const INTENTS_KEY = 'nexus.connect.intents'
// The pre-per-intent keys, read once for migration.
const LEGACY_PROJECTION_KEY = 'nexus.connect.projection'
const LEGACY_LAYERS_KEY = 'nexus.connect.layers'
const LEGACY_MAP3D_KEY = 'nexus.connect.map3d'

/** The picker's four choices: the 2-D orthographic globe, the WebGL globe, the flat world map and
 *  the azimuthal beam map. The three 2-D ones are the 2-D map's own projection ids. */
export type MapChoice = Projection | '3d'

/** One intent's remembered map. Every field optional: absent = never set on this surface. */
export interface IntentMapSetup {
  map?: MapChoice
  /** The 2-D layer table as stored — MapView clamps it against the current table on load. */
  layers?: unknown
  colorBy?: 'need' | 'snr'
}

type Store = Partial<Record<MapIntent, IntentMapSetup>>

const INTENTS: readonly MapIntent[] = ['dx', 'pota', 'casual', 'vhf']
const isProjection = (v: unknown): v is Projection => v === 'globe' || v === 'aeqd' || v === 'world'
const isChoice = (v: unknown): v is MapChoice => v === '3d' || isProjection(v)

/** Parse one intent's record, keeping only well-formed fields (a store from another build is
 *  exactly the input this will meet). */
function cleanSetup(raw: unknown): IntentMapSetup | undefined {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return undefined
  const r = raw as Record<string, unknown>
  const out: IntentMapSetup = {}
  if (r.map !== undefined) out.map = isChoice(r.map) ? r.map : 'globe'
  else if (r.map3d === true) out.map = '3d'
  else if (r.kind !== undefined) out.map = isProjection(r.kind) ? r.kind : 'globe'
  if (typeof r.layers === 'object' && r.layers !== null && !Array.isArray(r.layers)) out.layers = r.layers
  if (r.colorBy === 'need' || r.colorBy === 'snr') out.colorBy = r.colorBy
  return out
}

// Every read below names its key directly (no pass-through helper) so storage-scope.test.ts can
// see which keys this module routes. On a DEDICATED surface each read counts only what that surface
// wrote itself.

function readStore(dedicated: boolean): Store | null {
  const v = dedicated && !surfaceHasOwn(INTENTS_KEY) ? null : surfaceGet(INTENTS_KEY)
  if (!v) return null
  let raw: unknown
  try {
    raw = JSON.parse(v)
  } catch {
    return null
  }
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return null
  const store: Store = {}
  for (const i of INTENTS) {
    const s = cleanSetup((raw as Record<string, unknown>)[i])
    if (s) store[i] = s
  }
  return store
}

/** The old shared values, as a record for `intent` — or null when this surface never had any. */
function legacySetup(dedicated: boolean): IntentMapSetup | null {
  const out: IntentMapSetup = {}
  const kind =
    dedicated && !surfaceHasOwn(LEGACY_PROJECTION_KEY) ? null : surfaceGet(LEGACY_PROJECTION_KEY)
  if (isProjection(kind)) out.map = kind
  const layers = dedicated && !surfaceHasOwn(LEGACY_LAYERS_KEY) ? null : surfaceGet(LEGACY_LAYERS_KEY)
  if (layers) {
    try {
      const parsed: unknown = JSON.parse(layers)
      if (typeof parsed === 'object' && parsed !== null && !Array.isArray(parsed)) out.layers = parsed
    } catch {
      /* unusable — the intent's preset applies instead */
    }
  }
  const map3d = dedicated && !surfaceHasOwn(LEGACY_MAP3D_KEY) ? null : surfaceGet(LEGACY_MAP3D_KEY)
  if (map3d === '1') out.map = '3d'
  return Object.keys(out).length > 0 ? out : null
}

/**
 * What the operator left on `intent` on this surface, or null when the intent has never been
 * used here (its preset then applies). The first read on a surface with no record at all
 * migrates the legacy shared values into `intent` — the intent active on that first load.
 */
export function loadIntentSetup(intent: MapIntent, dedicated = false): IntentMapSetup | null {
  const store = readStore(dedicated)
  if (store) return store[intent] ?? null
  const legacy = legacySetup(dedicated)
  if (!legacy) return null
  surfaceSet(INTENTS_KEY, JSON.stringify({ [intent]: legacy }))
  return legacy
}

/** Merge `patch` into `intent`'s record on this surface. */
export function saveIntentSetup(intent: MapIntent, patch: IntentMapSetup, dedicated = false): void {
  const store = readStore(dedicated) ?? {}
  store[intent] = { ...store[intent], ...patch }
  surfaceSet(INTENTS_KEY, JSON.stringify(store))
}
