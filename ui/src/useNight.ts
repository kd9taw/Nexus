import { useCallback, useEffect, useLayoutEffect, useState, useSyncExternalStore } from 'react'
import { gridToLatLon, isValidGrid } from './grid'
import { solarElevationDeg } from './mapGeo'
import { PALETTE_EVENT } from './usePaletteRoles'

// NIGHT — a darker, warmer screen for operating after dark: Off / On / Auto (Settings ▸
// Appearance ▸ Workspace, under High contrast). Operator picks of 2026-09-26: "Dim + warm, TX
// red" (the transmit, alert and signal colours do not change), "Settings + Auto by sun" (a row in
// Settings, NOT a top-bar chip: Field stays the only quick toggle), "Dim the light theme" (Night
// dims whichever theme is on) and "At dusk, sun 6° down".
//
// `data-night='1'` on <html> selects the NIGHT blocks in styles.css (styles-night-contrast
// .test.ts is their spec). AUTO is civil dusk to civil dawn at the station's grid square: Night
// shows while the sun is more than 6° below the horizon there, by the same `solarElevationDeg`
// the map's terminator draws with, looked at again once a minute. With no grid square Auto has
// nothing to go on and stays OFF — the Settings row says so.
//
// ⭐ THE CHOICE IS OWNED, THE ATTRIBUTE IS DERIVED — and this hook is the only writer of it in
// its document (App and DetachedPanel each call it, as they call useTheme). What is stored is the
// operator's choice, never the verdict: a stored "it is night" would be wrong by the next dawn.
// The verdict is recomputed from the choice, the grid and the clock.
//
// Per machine, webview-local and not in the durable store, the same classification as the theme
// and high contrast: a fact about this screen and the room it is in. index.html's preseed paints
// On from the first frame; Auto needs the grid square, which only arrives with the first snapshot,
// so the preseed leaves Auto to this hook (index-preseed.test.ts holds the two to parity).
//
// A flip fires PALETTE_EVENT: the canvases that cache token colours (the map, the waterfall's
// overlay) key their caches on usePaletteKey, which reads this attribute too, and the scopes'
// Auto palette reads useNightActive.

export type NightChoice = 'off' | 'on' | 'auto'

export const NIGHT_STORAGE_KEY = 'nexus-night'
/** Civil dusk and dawn: the sun 6° below the horizon. */
export const CIVIL_DUSK_DEG = -6
/** How often Auto looks at the sun again. */
export const NIGHT_TICK_MS = 60_000

/** Is the sun more than 6° below the horizon at this grid square now? `null` when the grid is not
 *  a real station square — Auto then has no verdict to give. */
export function sunDownAt(grid: string, nowMs: number): boolean | null {
  if (!isValidGrid(grid)) return null
  const ll = gridToLatLon(grid)
  if (!ll) return null
  return solarElevationDeg(ll.lat, ll.lon, nowMs) < CIVIL_DUSK_DEG
}

// ⚠️ The localStorage calls name NIGHT_STORAGE_KEY directly: storage-scope.test.ts resolves a
// key only when it is a literal or a same-file const at the call site (see useFieldMode.ts).
function readNight(): NightChoice {
  try {
    const v = localStorage.getItem(NIGHT_STORAGE_KEY)
    return v === 'on' || v === 'auto' ? v : 'off'
  } catch {
    return 'off'
  }
}

export interface NightPrefs {
  /** The operator's choice. */
  night: NightChoice
  setNight: (c: NightChoice) => void
  /** Night is showing: On, or Auto with the sun more than 6° down at the grid square. */
  active: boolean
  /** Auto has a grid square to work from. */
  gridKnown: boolean
}

export function useNight(grid: string): NightPrefs {
  const [night, setNightState] = useState<NightChoice>(readNight)
  const gridKnown = isValidGrid(grid)
  // In Auto the sun is judged on every render — a few trig operations — so a new grid square, or
  // a switch to Auto hours after the window opened, never paints a frame from a stale verdict.
  // The minute tick only wakes the hook: it stores what it saw, and React skips the render when
  // that did not change, so a quiet minute re-renders nothing. It re-reads the sun the moment it
  // (re)starts, so what it stored is never an old grid's or an old day's verdict — a stale one
  // could equal the next real flip and swallow it.
  const down = night === 'auto' && gridKnown && sunDownAt(grid, Date.now()) === true
  const [, setSeen] = useState(down)
  useEffect(() => {
    if (night !== 'auto' || !gridKnown) return
    const look = () => setSeen(sunDownAt(grid, Date.now()) === true)
    look()
    const id = setInterval(look, NIGHT_TICK_MS)
    return () => clearInterval(id)
  }, [night, grid, gridKnown])

  const active = night === 'on' || down

  // Before paint, so a flip never shows one frame of the other look.
  useLayoutEffect(() => {
    const root = document.documentElement
    if (active) root.setAttribute('data-night', '1')
    else root.removeAttribute('data-night')
    window.dispatchEvent(new Event(PALETTE_EVENT))
  }, [active])

  // Storage is written by the pick, never by the mount: opening a window must not rewrite a
  // choice made in another one.
  const setNight = useCallback((c: NightChoice) => {
    try {
      localStorage.setItem(NIGHT_STORAGE_KEY, c)
    } catch {
      /* storage unavailable — the choice still applies for this session */
    }
    setNightState(c)
  }, [])

  return { night, setNight, active, gridKnown }
}

const subscribe = (onChange: () => void) => {
  window.addEventListener(PALETTE_EVENT, onChange)
  return () => window.removeEventListener(PALETTE_EVENT, onChange)
}
const snapshot = () => document.documentElement.getAttribute('data-night') === '1'

/** Is Night showing in this document? For a reader that bakes colours of its own — the scopes'
 *  Auto palette resolves to Amber CRT at night. */
export function useNightActive(): boolean {
  return useSyncExternalStore(subscribe, snapshot, snapshot)
}
