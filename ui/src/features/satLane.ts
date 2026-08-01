// The Now-Bar `sat` lane's decision: which element-currency condition (if
// any) deserves a persistent warning chip. Pure — App.tsx feeds it the polled
// TleStatus and pipes the result straight to setStatus('sat', …) — so the
// precedence rules are testable: the Celestrak hard stop outranks an
// all-rotten cache outranks the 14 d stale line, and a lane with nothing the
// operator can act on is null (the chip clears).

import type { TleStatus } from '../api'
import type { StatusItem } from '../status'

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
  // lane just goes quiet. (Review catch: the warning outlived recovery.)
  const healthy = s.usableCount > 0 && s.elementAgeDays != null && s.elementAgeDays <= 14
  if (s.blockedUntil > nowSecs && !healthy) {
    return {
      tier: 'warning',
      message: 'Sat: Celestrak blocked',
      detail:
        'Celestrak refused direct element fetches (HTTP 403/404) — direct attempts are stopped for 24 h. The hamradiotools.io mirror keeps retrying; elements may age until it lands.',
    }
  }
  if (s.count > 0 && s.usableCount === 0) {
    return {
      tier: 'warning',
      message: 'Sat: elements unusable',
      detail:
        'Every cached element set is over 30 days old — satellite surfaces refuse to point or tune on them. Refresh in Settings ▸ Radio ▸ Orbital elements, or import a fresh file.',
    }
  }
  if (s.elementAgeDays != null && s.elementAgeDays > 14) {
    return {
      tier: 'warning',
      message: `Sat: elements ${Math.round(s.elementAgeDays)} d old`,
      detail:
        'Orbital elements are past the 14-day stale line — pass times, pointing and Doppler drift with age. Refresh from the Satellites section or Settings ▸ Radio ▸ Orbital elements.',
    }
  }
  return null
}
