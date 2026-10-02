// THE 3-D GLOBE'S LAYER PICKS — its own per-surface record, apart from the 2-D map's per-intent one
// (features/intentMapSettings). It lives here, not in Globe3D.tsx, because that file is lazy-loaded
// with three.js and Connect writes this record too: a layout that turns a map layer on
// (features/connectPresets `mapLayers`; Frame: the satellites) turns it on here, and must not pull
// three.js in to do it.
import { surfaceGet, surfaceSet } from './windowScope'

/** The 3-D globe's toggleable layers. Persisted per-surface (#211) — the 2-D map already
 * remembered its layer picks (#199) but the globe reset to defaults on every mount, so a Connect
 * operator working on the 3-D map lost their choices each time. Same `surfaceGet/surfaceSet`
 * store the 2-D map uses, so a pop-out keeps its own picks. */
export type GlobeLayers = {
  spots: boolean
  arcs: boolean
  rxarcs: boolean
  states: boolean
  lights: boolean
  flare: boolean
  aurora: boolean
  muf: boolean
  pca: boolean
  heat: boolean
  openings: boolean
  grid: boolean
  sats: boolean
  pass: boolean
  rings: boolean
  cqzones: boolean
  coverage: boolean
  decodes: boolean
  dxped: boolean
  greyline: boolean
  sunMoon: boolean
}

const GLOBE_LAYERS_KEY = 'nexus.connect.globe3d.layers'

export const defaultGlobeLayers = (showStates: boolean): GlobeLayers => ({
  spots: true,
  arcs: true,
  // OFF by default, like the 2-D map's `rxPaths`: the decode roster on a busy band is 100+
  // stations, and default-on would web the globe for everyone on upgrade.
  rxarcs: false,
  states: showStates,
  lights: true,
  flare: true,
  aurora: false,
  muf: true,
  pca: true,
  heat: true,
  openings: true,
  grid: false,
  sats: false,
  pass: true, // the tracked-pass scene; nothing is drawn unless a pass is live
  rings: true,
  cqzones: false,
  coverage: false,
  decodes: true,
  dxped: false,
  greyline: true,
  // The sun and the moon where each is overhead, like the 2-D map's; moved on the 60 s sun clock,
  // never animated.
  sunMoon: true,
})

/** Parse a persisted layer object, keeping only the known boolean toggles — an unknown or
 * malformed store never poisons the defaults it is merged onto. */
export function globeLayersFromStored(v: string | null): Partial<GlobeLayers> {
  if (!v) return {}
  try {
    const raw: unknown = JSON.parse(v)
    if (!raw || typeof raw !== 'object') return {}
    const rec = raw as Record<string, unknown>
    const out: Partial<GlobeLayers> = {}
    for (const k of Object.keys(defaultGlobeLayers(true)) as (keyof GlobeLayers)[]) {
      if (typeof rec[k] === 'boolean') out[k] = rec[k] as boolean
    }
    return out
  } catch {
    return {}
  }
}

/** What the globe shows on this surface: its defaults, with the operator's own picks over them. */
export function loadGlobeLayers(showStates: boolean): GlobeLayers {
  return { ...defaultGlobeLayers(showStates), ...globeLayersFromStored(surfaceGet(GLOBE_LAYERS_KEY)) }
}

/** Keep every pick, so the next launch opens the globe the operator left. */
export function saveGlobeLayers(show: GlobeLayers): void {
  surfaceSet(GLOBE_LAYERS_KEY, JSON.stringify(show))
}

/** Turn one layer on or off in this surface's record, as a layout does (the globe, if it is on screen,
 *  reads it again when its host bumps `layersRev`). Returns whether anything changed. */
export function setGlobeLayer(key: keyof GlobeLayers, on: boolean): boolean {
  const now = loadGlobeLayers(true)
  if (now[key] === on) return false
  saveGlobeLayers({ ...now, [key]: on })
  return true
}
