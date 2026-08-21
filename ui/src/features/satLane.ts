// The Now-Bar `sat` lane's decision: which element-currency condition (if
// any) deserves a persistent warning chip. Pure — App.tsx feeds it the polled
// TleStatus and pipes the result straight to setStatus('sat', …) — so the
// precedence rules are testable: the Celestrak hard stop outranks an
// all-rotten cache outranks the 14 d stale line outranks a catalog whose
// MAJORITY is past that line (the one degradation an honest median reads as
// calm), and a lane with nothing the operator can act on is null (the chip
// clears).

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The
// chip and its detail come from the catalog; the 14 d / 30 d lines and the age
// itself are the facts the lane exists to report and stay here.

import type { TleStatus } from '../api'
import type { StatusItem } from '../status'
import { elementsMostlyPastLine, elementsPastLineLabel } from './elementBands'
import { t } from '../i18n'

/**
 * The `sat` status-lane entry for the polled element-currency status, or null
 * when nothing needs the operator's attention. `nowSecs` is unix seconds
 * (Date.now() / 1000).
 */
export function satElementsLane(s: TleStatus, nowSecs: number): Omit<StatusItem, 'id'> | null {
  // A standing Celestrak block only warns while it can still hurt: once the
  // mirror lands a fresh usable set the operator has nothing to act on, and
  // "elements may age until it lands" would be a lie. The backend keeps the
  // block itself — etiquette toward Celestrak is not health-dependent — the
  // lane just goes quiet. (Review catch: the warning outlived recovery. On
  // the shipped catalog it came back silently while `elementAgeDays` was a max
  // over the usable set — the slow-cadence tail held that max in the 20s, so
  // `healthy` stayed false however fresh the fetch was. A catalog with no such
  // tail did recover; this one could not.)
  const healthy = s.usableCount > 0 && s.elementAgeDays != null && s.elementAgeDays <= 14
  if (s.blockedUntil > nowSecs && !healthy) {
    return {
      tier: 'warning',
      message: t('sat.lane.blocked.message'),
      detail: t('sat.lane.blocked.detail'),
    }
  }
  // A COUNT test, not an age test — "not one bird is usable" is exactly what
  // this says, and it stays right whatever the age statistic is. The band
  // counts are deliberately not spent here: this is a two-word chip and the
  // detail already names the ceiling.
  if (s.count > 0 && s.usableCount === 0) {
    return {
      tier: 'warning',
      message: t('sat.lane.unusable.message'),
      detail: t('sat.lane.unusable.detail'),
    }
  }
  // The 14 d line, on the MEDIAN of the usable sets — a persistent chip on
  // every screen is the loudest surface in the app, so the statistic behind
  // it has to describe the set and not its slowest-cadence bird.
  if (s.elementAgeDays != null && s.elementAgeDays > 14) {
    return {
      tier: 'warning',
      message: t('sat.lane.stale.message', { days: Math.round(s.elementAgeDays) }),
      detail: t('sat.lane.stale.detail'),
    }
  }
  // The one shape the median above cannot see: it stays calm until half the
  // USABLE birds cross the line, and the birds held back past 30 d never move
  // it at all — so "half my catalog sits at 29 d and the rest arrived this
  // morning" reads exactly like a fresh set. A majority past the line is a
  // different situation from a fresh catalog and says so, and only a majority:
  // the shipped catalog's permanent slow-cadence tail must not put a chip on
  // every screen (elementBands.ts owns that threshold, shared with the Connect
  // Passes pane so the two cannot disagree).
  if (elementsMostlyPastLine(s)) {
    return {
      tier: 'warning',
      message: t('sat.lane.mostlyStale.message', { label: elementsPastLineLabel(s) }),
      detail: t('sat.lane.mostlyStale.detail', { label: elementsPastLineLabel(s) }),
    }
  }
  return null
}
