// Geomagnetic storm heads-up — the missing half of the space-weather alerting.
//
// A solar flare already alerts app-wide (flareAlert.ts) and a band coming alive
// alerts app-wide (openingAlert.ts). A GEOMAGNETIC STORM only ever showed up in the
// Space Weather pane, which means it reached an operator only if they happened to be
// looking at Connect with that pane placed. That is backwards for HF: a flare is
// minutes of dayside absorption, a storm is hours-to-days of degraded paths, and it
// is the one that decides whether tonight is worth sitting down for.
//
// Two independent edges, because they answer different questions:
//   • processStorm()         — it is happening NOW, from the measured Kp.
//   • processStormForecast() — NOAA expects one, from the three-day outlook.
//
// ⚠️ NOTIFY, NEVER ACT. Nothing here touches the radio: no QSY, no band change, no
// mode change. A storm alert is information for the operator to act on, and an
// unattended rig must never be moved by a space-weather feed.
import { doubleBeep } from './alerts'
import { pushToast } from './toast'
import { t } from './i18n'
import type { KpForecast } from './types'

const STORM_BEEP_HZ = 440
/** Kp 5 is the G1 boundary. Below it the sky is unsettled, not stormy. */
const STORM_KP = 5
/** Re-arm only once it falls back to quiet-ish, so a Kp wobbling either side of a
 *  boundary cannot re-fire — the same hysteresis the flare watcher uses. */
const RESET_BELOW = 4
const COOLDOWN_MS = 60 * 60_000

/** NOAA G-scale from Kp: 5→G1 … 9→G5. 0 = below storm level. */
export function gScale(kp: number): number {
  if (!Number.isFinite(kp) || kp < STORM_KP) return 0
  return Math.min(5, Math.floor(kp) - 4)
}

let alertedG = 0
const lastFired = new Map<number, number>()
/** Onset timestamps already announced, so one predicted storm alerts once. */
const forecastSeen = new Set<number>()

/** Test hook — clears the edge/dedup state. */
export function resetStormAlerts(): void {
  alertedG = 0
  lastFired.clear()
  forecastSeen.clear()
}

/**
 * Edge-triggered storm alert over the MEASURED planetary K. Call on every
 * propagation snapshot; it toasts only when a storm first reaches a G level or
 * escalates to a higher one.
 */
export function processStorm(kp: number | null | undefined): void {
  if (kp == null || !Number.isFinite(kp)) return
  const g = gScale(kp)
  if (g === 0) {
    if (kp < RESET_BELOW) alertedG = 0 // storm over → re-arm for the next one
    return
  }
  if (g <= alertedG) return // already announced at or above this level
  const now = Date.now()
  if (now - (lastFired.get(g) ?? 0) < COOLDOWN_MS) return // flap guard
  // Mark the edge consumed ONLY when actually firing, so a cooldown-suppressed
  // escalation is delayed rather than silently dropped (the flare watcher's rule).
  alertedG = g
  lastFired.set(g, now)

  const msg = t('prop.stormAlert.now', { g, kp: kp.toFixed(1) })
  if (g >= 2) {
    doubleBeep(STORM_BEEP_HZ)
    pushToast(msg, 'error', 15000, { prominent: true })
  } else {
    pushToast(msg, 'info', 8000)
  }
}

/**
 * Heads-up from the three-day outlook: NOAA expects a storm that has not started.
 *
 * Deduped by the onset period's own timestamp, so a predicted storm announces once
 * however often the forecast is polled — and a LATER revision naming a different
 * onset is a different event and does announce again, which is the point: the
 * forecast moving is news.
 *
 * Silent while a storm is already running: `processStorm` owns that, and saying
 * "expected" about weather already on top of you is noise.
 */
export function processStormForecast(fc: KpForecast | null | undefined, nowKp?: number | null): void {
  if (!fc || fc.points.length === 0) return
  if (nowKp != null && gScale(nowKp) > 0) return // already stormy — not a heads-up
  const onset = fc.points.find((p) => p.kind !== 'observed' && p.kp >= STORM_KP)
  if (!onset || forecastSeen.has(onset.timeUnix)) return
  forecastSeen.add(onset.timeUnix)
  const when = new Date(onset.timeUnix * 1000)
  pushToast(
    t('prop.stormAlert.forecast', {
      g: gScale(onset.kp),
      kp: onset.kp.toFixed(1),
      when: `${when.toLocaleDateString(undefined, { weekday: 'short', timeZone: 'UTC' })} ${String(
        when.getUTCHours(),
      ).padStart(2, '0')}z`,
    }),
    'info',
    12000,
  )
}
