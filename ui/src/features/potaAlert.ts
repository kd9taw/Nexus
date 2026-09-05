// Audible new-POTA-activation alert (opt-in, App.tsx-wide). The operator hunts POTA and wants
// to know the moment a fresh park comes on the air without keeping the map's Parks layer open —
// App.tsx runs its own poll of get_ota_map_spots, gated on the `potaNewActivationAlert` setting
// (no interval, no IPC while it's off), and calls `processPotaAlert` on every tick.
//
// "Freshly spotted" is Set membership on `reference` across polls — deliberately NOT `newRef`
// (OtaMapSpot.newRef means "never logged before", the hunter's new-PARK flag). A park can be
// newRef=false — worked a dozen times — and still be a brand-new SPOT right now because the
// activator just keyed up; that moment is what this alert is for.
//
// COLD-START MUST BE SILENT. The first tick has no history to diff against, so every park
// already on the air (200+ on a good day) would read as "fresh" with no priming — a beep-burst
// for activations that were running long before the operator turned this on. `processPotaAlert`
// primes its seen-set on that first call instead of alerting from it: the operator hears about
// parks going on the air FROM NOW, never the backlog. State is module-level (survives App's
// poll effect stopping and restarting as the setting is flipped off/on within a session), so a
// re-enable does not re-prime — it just resumes the diff against what was already seen, and the
// one-beep-per-tick rate limit below keeps a long time-off from ever reading as a burst.
//
// Rate limit: the 120 s poll cadence already bounds frequency; one doubleBeep per tick
// regardless of how many references are newly spotted in that tick — no separate cooldown.

import { doubleBeep } from '../alerts'
import type { OtaMapSpot } from '../types'

const POTA_BEEP_HZ = 880

/**
 * Pure set-diff on `reference`: which of `current`'s references were not in `prev`, and the
 * next seen-set to carry forward (a reference that drops off the feed is not carried — it reads
 * as fresh again if the activator comes back later, which is correct: that is a new spot).
 */
export function newlySpottedRefs(
  prev: Set<string>,
  current: OtaMapSpot[],
): { fresh: string[]; next: Set<string> } {
  const fresh: string[] = []
  const next = new Set<string>()
  for (const sp of current) {
    if (!prev.has(sp.reference)) fresh.push(sp.reference)
    next.add(sp.reference)
  }
  return { fresh, next }
}

let seen = new Set<string>()
let primed = false

/** Test hook — forget the seen-set and the cold-start prime. */
export function resetPotaAlertsForTest(): void {
  seen = new Set()
  primed = false
}

/**
 * Call with each get_ota_map_spots poll while the setting is on (App.tsx does not poll at all
 * while it's off, so this is never called then). Primes silently on the first call, then beeps
 * once per tick whenever `newlySpottedRefs` finds at least one reference App hasn't seen before.
 */
export function processPotaAlert(spots: OtaMapSpot[]): void {
  const { fresh, next } = newlySpottedRefs(seen, spots)
  seen = next
  if (!primed) {
    primed = true // first tick establishes the baseline — never alerts
    return
  }
  if (fresh.length > 0) doubleBeep(POTA_BEEP_HZ)
}
