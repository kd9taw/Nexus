// THE ROTOR PANE'S GRACE — the pure half. How many polls a rotator may miss before the pane says
// it is not answering.
//
// ⭐ ONE MISSED POLL USED TO READ AS A LOST CONNECTION (2026-11-03, a tester's GS-232A and GS-232B
// rotators "losing connection" over the internet). The pane turned a single unanswered read into
// "The rotator controller isn't answering. Is it switched on and plugged in?", so a link that
// dropped one reply in twenty "disconnected" every forty seconds and came back. Now a rotator that
// has answered keeps its last reading, shown as stale, until ROTOR_GRACE_MISSES polls in a row have
// gone unanswered: at the 2 s poll that is about six seconds, long enough to ride out a lost packet
// or two, short enough that a controller really switched off is called so within a few seconds.
//
// A rotator that has NOT answered since the pane opened has no reading to show stale, so its first
// miss says so at once, as it always did: there is nothing to wait for.
import type { RotatorState } from '../types'

/** Polls in a row a rotator that has answered may miss before it is called not answering. */
export const ROTOR_GRACE_MISSES = 3

export interface Liveness {
  /** The last reading a rotator actually answered (a position, or "no position to give"). */
  answered: RotatorState | null
  /** Unanswered polls since then. */
  misses: number
  /** What the latest miss was: rotctld said nothing (`notAnswering`), or the read itself failed
   *  before reaching it (`null`, a browser's forwarded read). */
  miss: 'notAnswering' | null
}

export const FRESH: Liveness = { answered: null, misses: 0, miss: null }

/** Take one poll's answer: `null` = no rotator configured; `'failed'` = the read itself failed. */
export function observe(prev: Liveness, st: RotatorState | null | 'failed'): Liveness {
  if (st === null) return FRESH
  if (st === 'failed') return { ...prev, misses: prev.misses + 1, miss: null }
  if (st.reading === 'notAnswering') return { ...prev, misses: prev.misses + 1, miss: 'notAnswering' }
  return { answered: st, misses: 0, miss: null }
}

/** What the pane shows: the reading to draw, and whether it is the last one rather than a fresh
 *  one. `state` null = nothing to draw (no rotator, or a failed read with nothing to fall back on). */
export function shown(l: Liveness): { state: RotatorState | null; stale: boolean } {
  if (l.misses === 0) return { state: l.answered, stale: false }
  if (l.answered && l.misses < ROTOR_GRACE_MISSES) return { state: l.answered, stale: true }
  return {
    state: l.miss === 'notAnswering' ? { azDeg: null, reading: 'notAnswering', elDeg: null } : null,
    stale: false,
  }
}
