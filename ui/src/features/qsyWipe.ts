// "Wipe an unlogged call when I QSY" (Settings ▸ Logbook ▸ Log line): a log line's un-logged
// contact is set aside when the dial moves off the frequency it was made on, as N1MM Logger+'s
// "QSYing wipes the call" option does. Default OFF: a band change already sets the line aside on
// the cockpits that ask for it (`LogEntry`'s `clearCallOnBandChange`); this adds a move within the
// band. What counts as a move, and when a line is never touched, is `LogEntry`'s to say.
//
// APP-GLOBAL, bare localStorage, the same classification as the station card on hover
// (features/callCardHover.ts): a standing statement about how this operator wants the app to
// behave, not a property of one window.

import { useEffect, useState } from 'react'
import type { SatTrackStatus } from '../types'

export const QSY_WIPE_KEY = 'nexus.logLine.qsyWipe'
const CHANGED_EVENT = 'nexus-qsy-wipe-changed'

/** Is the QSY wipe on? Off unless the operator turned it on. */
export function qsyWipeOn(): boolean {
  try {
    return localStorage.getItem(QSY_WIPE_KEY) === 'on'
  } catch {
    return false // blocked storage: keep the default
  }
}

/** Turn the QSY wipe on or off, and tell every mounted reader in this window at once. */
export function setQsyWipe(on: boolean): void {
  try {
    localStorage.setItem(QSY_WIPE_KEY, on ? 'on' : 'off')
  } catch {
    /* blocked storage: the change still applies to the readers for this session */
  }
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent<boolean>(CHANGED_EVENT, { detail: on }))
  }
}

/** The current setting, re-rendering when it changes here or in another window. */
export function useQsyWipe(): [boolean, (on: boolean) => void] {
  const [on, setOn] = useState<boolean>(qsyWipeOn)
  useEffect(() => {
    const onChange = (e: Event) => setOn((e as CustomEvent<boolean>).detail === true)
    const onStorage = (e: StorageEvent) => {
      if (e.key === QSY_WIPE_KEY) setOn(qsyWipeOn())
    }
    window.addEventListener(CHANGED_EVENT, onChange)
    window.addEventListener('storage', onStorage)
    return () => {
      window.removeEventListener(CHANGED_EVENT, onChange)
      window.removeEventListener('storage', onStorage)
    }
  }, [])
  return [on, setQsyWipe]
}

/** Does a satellite pass hold the dial? Any track at all, armed or tracking, Doppler on or not:
 *  Doppler may take a leg at any tick and the pass poll answers only every 2 s, so a contest line
 *  takes no move for a QSY while a pass is up (App's pass poll for the main window's Contest
 *  screen, and the logger window's own). */
export function passHoldsDial(track: SatTrackStatus | null): boolean {
  return track != null
}
