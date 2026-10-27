// "WORK THIS CONTEST IN CW / IN PHONE" — the route from a contest window to the cockpit that sends.
//
// The Contest screen and its windows log and keep score; they never transmit. Enter Sends Message,
// Running / S&P and the F-key messages are the contest strip's in the CW and Phone cockpits of the main
// window (a tester at the Illinois QSO Party, 2026-10-18, looked for them in the Contest window). So the
// screen offers one button per cockpit that brings the main window to it, with the contest's strip on
// it, and a hint, once, saying that sending happens there.
//
// In the main window the button is App's own view switch (`handleView`, which also enters the contest's
// S&P where the session dropped, never a CQ). From another window it can only ASK: it writes the
// request here and the main window, hearing it as a `storage` event (which only a write from another
// document raises), makes the same switch and brings itself to the front. No command is sent from the
// asking window: the logger window keeps its bridge exactly as it was (ContestLogger.txfree.test.tsx).
//
// Both keys are SHARED (storage-scope.test.ts): the request is the whole mechanism between two documents,
// and the hint, once dismissed, is dismissed for this computer, whichever window showed it.
import { useEffect, useRef } from 'react'
import { t } from '../i18n'

export type WorkInView = 'cw' | 'phone' | 'rtty'

/** The mode each cockpit's contacts log under, as a contest's `bannedModes` names it. */
const VIEW_MODE: Record<WorkInView, string> = { cw: 'CW', phone: 'SSB', rtty: 'RTTY' }

/** Each cockpit's button: its words. */
export function workInLabel(view: WorkInView): string {
  if (view === 'cw') return t('fieldDay.workIn.cw')
  if (view === 'phone') return t('fieldDay.workIn.phone')
  return t('fieldDay.workIn.rtty')
}

/** …and its tooltip. */
export function workInTitle(view: WorkInView): string {
  if (view === 'cw') return t('fieldDay.workIn.cw.title')
  if (view === 'phone') return t('fieldDay.workIn.phone.title')
  return t('fieldDay.workIn.rtty.title')
}

/** ⭐ **The cockpits to offer for working a contest** — CW and Phone, less any whose mode the contest
 *  does not allow; and where it allows neither, RTTY when it allows that (the ARRL RTTY Roundup,
 *  RTTY only). A contest that bans neither gets CW and Phone as it always did. `enabled` is the
 *  station's own Settings ▸ Features: a cockpit switched off is never offered. */
export function workInViewsFor(
  bannedModes: readonly string[] | undefined,
  enabled: (view: WorkInView) => boolean = () => true,
): WorkInView[] {
  const banned = new Set((bannedModes ?? []).map((m) => m.toUpperCase()))
  const allowed = (v: WorkInView) => !banned.has(VIEW_MODE[v])
  const voice = (['cw', 'phone'] as const).filter(allowed)
  const routes: WorkInView[] = voice.length > 0 ? voice : allowed('rtty') ? ['rtty'] : []
  return routes.filter(enabled)
}

export const CONTEST_WORK_IN_KEY = 'nexus.contest.workIn'
export const CONTEST_WORK_IN_HINT_KEY = 'nexus.contest.workInHint'

/** A request as stored, read back: the cockpit it names, or null for anything else (a hand edit, a
 *  value from a build that wrote something different). */
export function workInRequest(stored: string | null): WorkInView | null {
  if (!stored) return null
  try {
    const v = (JSON.parse(stored) as { view?: unknown }).view
    return v === 'cw' || v === 'phone' || v === 'rtty' ? v : null
  } catch {
    return null
  }
}

/** From a window other than the main one: ask the main window to go to `view`. The time makes a second
 *  press for the same cockpit a new value, which is what raises a second `storage` event. */
export function askMainToWorkIn(view: WorkInView): void {
  try {
    window.localStorage.setItem(CONTEST_WORK_IN_KEY, JSON.stringify({ view, at: Date.now() }))
  } catch {
    // A blocked store: the request cannot travel, and the hint beside the buttons still says where to go.
  }
}

/** The main window's half: `onWorkIn` for each request another window writes. */
export function useWorkInRequests(onWorkIn: (view: WorkInView) => void): void {
  const latest = useRef(onWorkIn)
  latest.current = onWorkIn
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key !== CONTEST_WORK_IN_KEY) return
      const view = workInRequest(e.newValue)
      if (view) latest.current(view)
    }
    window.addEventListener('storage', onStorage)
    return () => window.removeEventListener('storage', onStorage)
  }, [])
}

/** Whether the hint has been dismissed on this computer. */
export function workInHintSeen(): boolean {
  try {
    return window.localStorage.getItem(CONTEST_WORK_IN_HINT_KEY) === '1'
  } catch {
    return false
  }
}

export function dismissWorkInHint(): void {
  try {
    window.localStorage.setItem(CONTEST_WORK_IN_HINT_KEY, '1')
  } catch {
    // A blocked store: dismissed for this window until it closes.
  }
}
