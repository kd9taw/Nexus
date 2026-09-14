import { useEffect, useState } from 'react'

/** How long the station's readings must STAY stale before the workspace says so. `stale` already means no
 * sample for APPLICATION_TIMEOUT_MS (3 s); two seconds more makes five seconds of continuous loss. That
 * outlasts a gesture held through a control lapse (CONTROL_RESUME_MS, 1.5 s) plus the 500 ms tick that
 * re-reads the sample age, so a brief gap resolves before the display could change. */
export const STALE_DISPLAY_HOLD_MS = 2000

/** Hysteresis on the stale DISPLAY only (operator decision 2026-09-14): the fade and the "Station data
 * unavailable" wording appear after continuous loss and clear the moment readings return, so a brief gap
 * shows nothing and a flapping one never flashes. Nothing that acts reads this: controls and commands
 * keep refusing stale readings at once. */
export function useStaleDisplay(stale: boolean): boolean {
  const [held, setHeld] = useState(false)
  useEffect(() => {
    if (!stale) { setHeld(false); return }
    const timer = setTimeout(() => setHeld(true), STALE_DISPLAY_HOLD_MS)
    return () => clearTimeout(timer)
  }, [stale])
  // `stale &&` clears on recovery in the same render, before the effect resets the hold.
  return stale && held
}
