// The log prompt's sound (Settings `soundLogPrompt`, off by default, #541): an operator who is not
// watching the screen hears that a contact wants logging. Two moments make the sound:
//   - the confirm-before-log popup comes up: no popup, then a contact. The next contact queued
//     behind it taking its place is not one (the popup never went away), nor is a re-render.
//   - Auto-log writes a contact with no popup to show it: the station's `autoLoggedTick` moves
//     (a contact the operator logs, or one held for the popup, leaves it alone).
// What a window opens on is adopted, not announced: `undefined` is "no snapshot yet", and the
// first snapshot's popup and tick are the baseline, so a launch with a contact waiting is quiet.
import { useEffect, useRef } from 'react'
import { logPromptEarcon } from './alerts'

/** `prompt`: the popup's contact, `null` for no popup, `undefined` before the first snapshot.
 *  `autoLoggedTick`: the snapshot's, `undefined` before the first. */
export function useLogPromptSound(prompt: object | null | undefined, autoLoggedTick: number | undefined, on: boolean): void {
  const shown = useRef<boolean | null>(null)
  const tick = useRef<number | null>(null)
  const up = prompt === undefined ? null : prompt !== null
  useEffect(() => {
    if (up === null) return
    const was = shown.current
    shown.current = up
    if (on && was === false && up) logPromptEarcon()
  }, [up, on])
  useEffect(() => {
    if (autoLoggedTick === undefined) return
    const was = tick.current
    tick.current = autoLoggedTick
    if (on && was !== null && was !== autoLoggedTick) logPromptEarcon()
  }, [autoLoggedTick, on])
}
