// GETTING OUT'S "START FRESH" — the time from which the Getting Out box and the map's "hearing you"
// dots count reception reports. For judging an antenna: press it, transmit, and read only what was
// heard after the change. Nothing is deleted; "Show all" clears the time and the earlier reports
// are back.
//
// PER SURFACE AND KEPT ACROSS A RESTART: an antenna test can outlive a restart, so the time is
// stored (features/windowScope), and each window keeps its own — a Connect pop-out starts on the
// main window's time and diverges at its first press. A Remote or LAN screen keeps its own in its
// own browser and sends it with each request, so it never moves the shack's own screen.
//
// One value per window, read by every box and map in it, so the box, the dashboard rail's copy and
// the map always agree on the start.
import { useSyncExternalStore } from 'react'
import { surfaceGet, surfaceSet } from './windowScope'

const SINCE_KEY = 'nexus.connect.getout.since'

/** Set by this window's own press; `undefined` until then, so the stored value is read. Kept here as
 *  well as stored: a blocked store must not undo a press for the rest of the session. */
let pressed: number | null | undefined
const listeners = new Set<() => void>()

/** The start time (Unix seconds), or `null` when every report counts. */
export function getoutSince(): number | null {
  if (pressed !== undefined) return pressed
  const n = Number(surfaceGet(SINCE_KEY) || NaN)
  return Number.isSafeInteger(n) && n > 0 ? n : null
}

/** Start counting from `secs` (Start fresh), or from the beginning again with `null` (Show all). */
export function setGetoutSince(secs: number | null): void {
  pressed = secs
  // An empty value, not a removed key: a pop-out that pressed Show all must not fall back to the
  // main window's start time on its next open.
  surfaceSet(SINCE_KEY, secs == null ? '' : String(secs))
  for (const l of [...listeners]) l()
}

/** "Start fresh": from now. */
export function startGetoutFresh(nowMs = Date.now()): void {
  setGetoutSince(Math.floor(nowMs / 1000))
}

function subscribe(l: () => void): () => void {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

/** This window's start time, re-rendering on a press. */
export function useGetoutSince(): number | null {
  return useSyncExternalStore(subscribe, getoutSince)
}
