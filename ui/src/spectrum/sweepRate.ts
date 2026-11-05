// HOW FAST THE RADIO'S OWN SCOPE IS SWEEPING, as the scope sees it: the sweeps that arrived over the
// last few seconds. It is a measurement of what reaches the screen, not a reading of the radio's
// SPEED menu. It cannot tell a radio left on SLOW from a link too busy to carry more, so it decides
// the cockpits' scope hint only until the station reports the rate itself (`scopeIsSlow` below), and
// nothing here is ever sent to the radio.
import type { ScopeRate } from '../types'

/** How far back the rate looks (ms). Long enough that 3 sweeps a second is a dozen or more
 *  samples, short enough that a radio switched to FAST stops being called slow within seconds. */
export const SWEEP_RATE_WINDOW_MS = 5000

/** Below this many sweeps a second, an Icom scope reads as slow. Icom's FAST is well above it; its
 *  SLOW, and the 3 a second the IC-9700 was reported at, are below it. */
export const SLOW_SWEEPS_PER_SEC = 4

/** The arrival times of the newest sweeps, and the rate they make. */
export class SweepRate {
  private times: number[] = []

  constructor(private readonly windowMs = SWEEP_RATE_WINDOW_MS) {}

  /** A new sweep arrived at `nowMs`. */
  sweep(nowMs: number): void {
    this.times.push(nowMs)
    this.prune(nowMs)
  }

  /** Sweeps a second over the window, or null until most of a window has been seen (a scope
   *  that just started, or one that stopped). */
  perSecond(nowMs: number): number | null {
    this.prune(nowMs)
    const n = this.times.length
    if (n < 2) return null
    const spanMs = this.times[n - 1] - this.times[0]
    if (spanMs < this.windowMs * 0.8) return null
    return (n - 1) / (spanMs / 1000)
  }

  private prune(nowMs: number): void {
    const from = nowMs - this.windowMs
    let i = 0
    while (i < this.times.length && this.times[i] < from) i++
    if (i > 0) this.times.splice(0, i)
  }
}

/** THE ONE RULE FOR THE SLOW-SCOPE HINT. The station's own report (`radio.scopeRate`) decides when
 *  it is there: the radio's SPEED where it can be read (SLOW is slow), else the sweeps a second it
 *  counts; null there means no rig scope, and no hint. Only while a station does not report it
 *  (`undefined`) does the rate this scope measured decide, for an Icom (`civ`), whose SPEED menu is the
 *  usual cause. Nothing here is ever sent to the radio. */
export function scopeIsSlow(
  reported: ScopeRate | null | undefined,
  source: string | null | undefined,
  measured: number | null,
): boolean {
  if (reported !== undefined) {
    if (reported === null) return false
    if (reported.speed != null) return reported.speed === 'slow'
    return reported.sweepsPerSec < SLOW_SWEEPS_PER_SEC
  }
  return source === 'civ' && measured != null && measured < SLOW_SWEEPS_PER_SEC
}
