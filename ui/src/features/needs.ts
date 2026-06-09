// Pure helpers for the mode-aware Needed board + click-to-work. No React, no IO —
// fully node-testable. The backend emits CW/Phone needs unconditionally (with an exact
// frequency); these gate them by the operator's enabled modes and resolve a click into
// a concrete QSY + cockpit target.

import type { BandChannel, NeedAlert } from '../types'

/** Which need rows are visible given the enabled operating-mode features. Digital needs
 * always show; CW/Phone needs only when that mode is enabled — so a pure-digital op's
 * board is unchanged even though the backend sends voice/CW needs too. */
export function visibleNeeds(
  alerts: NeedAlert[],
  enabled: { cw: boolean; phone: boolean },
): NeedAlert[] {
  return alerts.filter((a) => {
    if (a.mode === 'CW') return enabled.cw
    if (a.mode === 'Phone') return enabled.phone
    return true // Digital (and any unknown class) always visible
  })
}

/** A resolved click-to-work target: where to QSY and the cockpit to open. The CALLER
 * owns the rig sideband when it QSYs — the rig-mode policy derives the actual CAT mode
 * (CW, or USB/LSB-by-band for phone) from the operating mode, so we never compute it
 * here. */
export interface WorkTarget {
  call: string
  /** Cockpit view to open; also the operating-mode argument ('cw' | 'phone'). */
  view: 'cw' | 'phone'
  freqMhz: number
  band: string
}

/** Resolve a CW/Phone need into a work target. Uses the spot's exact frequency when the
 * cluster carried one, else the band's default channel. Returns null for a Digital need
 * (handled by the existing band-QSY path) or when no frequency can be resolved. */
export function workTarget(alert: NeedAlert, bandPlan: BandChannel[]): WorkTarget | null {
  const view: 'cw' | 'phone' | null =
    alert.mode === 'CW' ? 'cw' : alert.mode === 'Phone' ? 'phone' : null
  if (!view) return null
  const freqMhz = alert.freqMhz ?? bandPlan.find((c) => c.band === alert.band)?.dialMhz ?? null
  if (freqMhz == null) return null
  return { call: alert.call, view, freqMhz, band: alert.band }
}
