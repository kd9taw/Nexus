// THE MAP STYLE (Map layers ▸ Map style; #444): how the flat map and the 2-D globe paint land and
// sea, chosen apart from the UI theme. The standard basemap is a light atlas (STANDARD_MAP) in both UI
// themes; until this, the only dark map came with a dark built-in theme, which also changes the whole
// UI. The 3-D globe keeps its photographs of the Earth and never reads this.
//
// ## Two styles, and the precedence
//
// Atlas (STANDARD_MAP) and Dark (DARK_MAP, below). The stored PICK is a style or nothing:
//   - a pick paints its own palette, on every theme (`mapPaletteFor`);
//   - no pick paints the THEME'S map: Atlas on Dark, Light and the light built-in themes, and a dark
//     built-in theme's own map (its `map` block in features/skins.ts), read from the sheet's tokens as
//     before.
// So a dark theme keeps its map until the operator picks one, and choosing what the theme already
// paints stores no pick (`mapStylePick`): Atlas chosen on the standard theme is "no pick", so moving to
// a dark theme later still brings that theme's map. Where the theme brings a map of its own, the
// control offers it as a third choice (`mapStyleOptions`), which is the way back to it.
//
// ## Dark's markers are the dark palette's
//
// Every marker, arc and label on the 2-D map is painted from the theme's inks, and the light theme's
// are dark (made to read on white): on a dark map they would vanish. So on a Dark map MapView reads its
// inks from a display well (styles.css DISPLAY WELLS), which IS the dark palette at the mode's own
// contrast, Night and colour-role picks included. In the dark themes that changes nothing.
// features/mapStyle.test.ts holds every overlay over DARK_MAP to its floor, in every mode.
//
// ## Stored per window
//
// One record per window (`nexus.map.style`, features/windowScope), shared by every 2-D map in that
// window (Connect's map and the APRS map) so the look does not change with the intent or the view.
// A pop-out starts on the main window's pick. Anything stored that is not a style reads as no pick.
import { useCallback, useSyncExternalStore } from 'react'
import { STANDARD_MAP, skinOf, type MapToken, type SkinId } from './skins'
import { surfaceGet, surfaceSet } from './windowScope'

export const MAP_STYLES = ['atlas', 'dark'] as const
export type MapStyle = (typeof MAP_STYLES)[number]
/** What the control lists: a style, or the theme's own map (only where the theme brings one). */
export type MapStyleOption = 'theme' | MapStyle

/** The Dark basemap: a deep blue-grey sea, slate land a step above it and pale steel lines, so the
 *  grid, the greyline, the arcs and every marker read on it in either UI theme. Plain hexes: the
 *  canvas reads them as strings. */
export const DARK_MAP: Readonly<Record<MapToken, string>> = {
  '--map-ocean': '#0b1520', // the sea and the lakes
  '--map-land': '#232f3b', // slate land on the flat World and Beam maps
  '--map-land-globe': '#1e2934', // a step darker on the 2-D globe, so its shading reads as a sphere
  '--map-coast': '#8aa2b8', // coastlines and borders, the grid and the range rings
  '--map-state': '#64788c', // US state lines, quieter than the coast
  '--map-rim': '#3d5a78', // the globe's edge, and the rivers
  '--map-ocean-lit': '#13243a', // the globe's lit sea, toward the light
  '--map-ocean-deep': '#050a10', // the globe's limb
}

/** A stored value as a style, or null for anything else (a store from another build included). */
export function mapStyleOf(v: unknown): MapStyle | null {
  return v === 'atlas' || v === 'dark' ? v : null
}

const STYLE_KEY = 'nexus.map.style'
const STYLE_EVENT = 'nexus-map-style'

/** The pick this session made when storage would not keep it (full, or unavailable): the pick still
 *  applies until the window closes, as surfaceSet promises. Undefined while storage keeps the record. */
let unstored: MapStyle | null | undefined

/** This window's pick, clamped on load. */
export function loadMapStyle(): MapStyle | null {
  return unstored !== undefined ? unstored : mapStyleOf(surfaceGet(STYLE_KEY))
}

/** Store this window's pick (null: no pick, the theme's map) and tell this window's other maps. No pick
 *  is stored as an empty record, so a pop-out that cleared its pick stops following the main window's. */
export function saveMapStyle(style: MapStyle | null): void {
  surfaceSet(STYLE_KEY, style ?? '')
  unstored = surfaceGet(STYLE_KEY) === (style ?? '') ? undefined : style
  window.dispatchEvent(new Event(STYLE_EVENT))
}

/** A pick made on another map in this window (STYLE_EVENT), or in another window (`storage`): a
 *  pop-out still reading the main window's record follows a change made there at once. */
const subscribe = (onChange: () => void) => {
  window.addEventListener(STYLE_EVENT, onChange)
  window.addEventListener('storage', onChange)
  return () => {
    window.removeEventListener(STYLE_EVENT, onChange)
    window.removeEventListener('storage', onChange)
  }
}

/** [this window's pick, its setter]: every 2-D map in the window follows a change made on any of them. */
export function useMapStyle(): [MapStyle | null, (style: MapStyle | null) => void] {
  const style = useSyncExternalStore(subscribe, loadMapStyle, loadMapStyle)
  const set = useCallback((s: MapStyle | null) => saveMapStyle(s), [])
  return [style, set]
}

/** The palette a pick paints, or null: no pick, the theme's tokens paint (see the precedence above). */
export function mapPaletteFor(pick: MapStyle | null): Readonly<Record<MapToken, string>> | null {
  return pick === 'dark' ? DARK_MAP : pick === 'atlas' ? STANDARD_MAP : null
}

/** Whether the theme painting now brings a map of its own (a dark built-in theme). */
const themeHasMap = (skin: SkinId | null) => skinOf(skin)?.map !== undefined

/** The control's choices under the theme painting now. */
export function mapStyleOptions(skin: SkinId | null): MapStyleOption[] {
  return themeHasMap(skin) ? ['theme', 'atlas', 'dark'] : ['atlas', 'dark']
}

/** The choice the control shows: the pick, or what the theme paints without one. */
export function mapStyleShown(pick: MapStyle | null, skin: SkinId | null): MapStyleOption {
  return pick ?? (themeHasMap(skin) ? 'theme' : 'atlas')
}

/** What choosing `option` stores: no pick where it is what the theme paints anyway. */
export function mapStylePick(option: MapStyleOption, skin: SkinId | null): MapStyle | null {
  if (option === 'theme') return null
  if (option === 'atlas' && !themeHasMap(skin)) return null
  return option
}
