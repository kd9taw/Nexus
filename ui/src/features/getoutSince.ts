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
//
// ENDS BY ITSELF AFTER THE WINDOW: the box shows the station's last 30 minutes of reports, and Start
// fresh narrows that, never widens it. Once the start is more than 30 minutes ago the box is just
// its last 30 minutes again, which "since HH:MM:SS" would no longer describe — so the start ends
// there, in every open box (a timer wakes them) and on a restart, and the box reads as it does with
// no start. Inside the window a restart keeps it.
import { useSyncExternalStore } from 'react'
import { surfaceGet, surfaceSet } from './windowScope'

const SINCE_KEY = 'nexus.connect.getout.since'
/** The box's window: the station counts the last 30 minutes of reports (`get_getting_out`). */
export const GETOUT_WINDOW_SECS = 1800

/** Set by this window's own press; `undefined` until then, so the stored value is read. Kept here as
 *  well as stored: a blocked store must not undo a press for the rest of the session. */
let pressed: number | null | undefined
const listeners = new Set<() => void>()

let expiry: ReturnType<typeof setTimeout> | undefined

function startTime(): number | null {
  if (pressed !== undefined) return pressed
  const n = Number(surfaceGet(SINCE_KEY) || NaN)
  return Number.isSafeInteger(n) && n > 0 ? n : null
}

/** The start time (Unix seconds), or `null` when every report counts — including once the start is
 *  more than the window ago. */
export function getoutSince(nowMs = Date.now()): number | null {
  const s = startTime()
  return s != null && nowMs / 1000 - s <= GETOUT_WINDOW_SECS ? s : null
}

function notify(): void {
  for (const l of [...listeners]) l()
  armExpiry()
}

/** Wake the open boxes the moment the start leaves the window (capped, so a start the clock put in
 *  the future cannot overflow the timer; it simply re-arms). */
function armExpiry(): void {
  clearTimeout(expiry)
  expiry = undefined
  const s = getoutSince()
  if (s == null || listeners.size === 0) return
  const ms = (s + GETOUT_WINDOW_SECS) * 1000 - Date.now() + 1
  expiry = setTimeout(notify, Math.min(ms, GETOUT_WINDOW_SECS * 1000 + 1))
}

/** Start counting from `secs` (Start fresh), or from the beginning again with `null` (Show all). */
export function setGetoutSince(secs: number | null): void {
  pressed = secs
  // An empty value, not a removed key: a pop-out that pressed Show all must not fall back to the
  // main window's start time on its next open.
  surfaceSet(SINCE_KEY, secs == null ? '' : String(secs))
  notify()
}

/** "Start fresh": from now. */
export function startGetoutFresh(nowMs = Date.now()): void {
  setGetoutSince(Math.floor(nowMs / 1000))
}

function subscribe(l: () => void): () => void {
  listeners.add(l)
  armExpiry()
  return () => {
    listeners.delete(l)
    if (listeners.size === 0) armExpiry()
  }
}

/** This window's start time, re-rendering on a press and when the start leaves the window. */
export function useGetoutSince(): number | null {
  return useSyncExternalStore(subscribe, getoutSince)
}
