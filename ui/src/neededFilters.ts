// Pure filter predicate for the Needed panel — no React, no IO, fully testable.
// Imported by NeededPanel.tsx and tested in neededFilters.test.ts.

import type { NeedAlert, NeedTag } from './types'
import { bandOk, privilegeOk, searchMatcher, stateOk } from './spotFilters'

/** Need-type filter buckets surfaced in the filter bar. */
export type NeedTypeFilter =
  | 'all'
  | 'wanted'
  | 'atno'
  | 'newBand'
  | 'newMode'
  | 'newZone'
  | 'newGrid'
  | 'newState'
  | 'newIota'
  | 'newPark'
  | 'confirm'
  | 'dxped'
  | 'pota'
  | 'sota'

/** The operating-mode classes a need can carry. */
export type ModeClass = 'Digital' | 'CW' | 'Phone'
export const MODE_CLASSES: readonly ModeClass[] = ['Digital', 'CW', 'Phone']

/** Per-mode visibility — the operator ticks the modes they actually operate. Independent
 * per mode (multi-select), so a non-CW op can show Phone+Digital and hide CW. */
export type ModeSet = Record<ModeClass, boolean>
export const ALL_MODES_ON: ModeSet = { Digital: true, CW: true, Phone: true }

export interface NeededFilters {
  needTypes: NeedTypeFilter[]  // multi-select; empty = All. Never contains 'all'.
  bands: string[]              // multi-select; empty = All
  modes: ModeSet               // each mode independently on/off; default all on
  // The Spots board's filters, by its rules (spotFilters.ts). Each OFF by default, and off for a
  // record written before they existed, so an operator's board never hides more on upgrade.
  licensedOnly: boolean        // "My privileges"
  states: string[]             // US-state multi-select; empty = All
}

export const DEFAULT_FILTERS: NeededFilters = {
  needTypes: [],
  bands: [],
  modes: { ...ALL_MODES_ON },
  licensedOnly: false,
  states: [],
}

/** NeedTag → filter bucket mapping. TOTAL (`Record`, not `Partial`) on
 * purpose: the map used to omit `Wanted` — the TOP tier at 120 — so any active
 * filter hid the operator's own watch-list rows, with no chip to ask for them.
 * A new NeedTag now fails to compile until it names its bucket. */
const TAG_TO_BUCKET: Record<NeedTag, NeedTypeFilter> = {
  Wanted:    'wanted',
  NewEntity: 'atno',
  NewBand:   'newBand',
  NewMode:   'newMode',
  NewZone:   'newZone',
  NewGrid:   'newGrid',
  NewState:  'newState',
  NewIota:   'newIota',
  NewPark:   'newPark',
  Confirm:   'confirm',
  Dxped:     'dxped',
  Pota:      'pota',
  Sota:      'sota',
}

/** The valid persisted values — localStorage may hold a stale/renamed bucket
 * from an older build; an unknown value must fall back to 'all', not silently
 * empty the board with no active chip. */
export const NEED_TYPE_VALUES: readonly NeedTypeFilter[] = [
  'all', 'wanted', 'atno', 'newBand', 'newMode', 'newZone', 'newGrid', 'newState', 'newIota', 'newPark', 'confirm', 'dxped', 'pota', 'sota',
]

/** True when the alert matches the given filter set (all filters AND together). `query` is the
 *  head's search box, kept apart from the saved filters: it lives for the session, as on Spots. */
export function filterAlerts(alerts: NeedAlert[], filters: NeededFilters, query = ''): NeedAlert[] {
  const search = searchMatcher(query)
  return alerts.filter((a) => {
    // ---- The rules the Spots board shares (spotFilters.ts) ----
    if (!privilegeOk(filters.licensedOnly, a.licensed)) return false
    if (!stateOk(filters.states, a.state)) return false
    // What a need row holds that a search can mean: call, entity, mode and the exact mode it was
    // heard in (the Spots row's mode and submode), band, frequency. A need has no spotter.
    const freq = a.freqMhz != null ? a.freqMhz.toFixed(4) : ''
    if (search && !search([a.call, a.entity, a.mode, a.exactMode ?? '', a.band, freq])) {
      return false
    }

    // ---- Need-type multi-select (OR across the picked buckets; empty = all) ----
    if (filters.needTypes.length > 0) {
      const matches = a.tags.some((t) => {
        const bucket = TAG_TO_BUCKET[t]
        return bucket !== undefined && filters.needTypes.includes(bucket)
      })
      if (!matches) return false
    }

    // ---- Band multi-select ----
    if (!bandOk(filters.bands, a.band)) return false

    // ---- Mode multi-select: keep only the operator's enabled modes (an unknown mode
    // class always shows, so the board never silently swallows a need it can't classify).
    // RTTY/FT8/FT4/JTTY are Digital submodes (no separate chip), so the Digital toggle governs them. ----
    const cls = (
      a.mode === 'RTTY' || a.mode === 'FT8' || a.mode === 'FT4' || a.mode === 'JTTY' ? 'Digital' : a.mode
    ) as ModeClass
    if (MODE_CLASSES.includes(cls) && !filters.modes[cls]) return false

    return true
  })
}

/** Human-readable age string derived from an admittedAt unix-seconds timestamp.
 * Returns null when admittedAt is null/undefined. */
export function ageLabel(admittedAt: number | null | undefined): string | null {
  if (admittedAt == null || admittedAt <= 0) return null
  const diffSec = Math.max(0, Math.floor((Date.now() / 1000) - admittedAt))
  if (diffSec < 90) return 'just now'
  const mins = Math.round(diffSec / 60)
  return `${mins} min ago`
}
