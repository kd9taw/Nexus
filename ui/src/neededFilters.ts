// Pure filter predicate for the Needed panel — no React, no IO, fully testable.
// Imported by NeededPanel.tsx and tested in neededFilters.test.ts.

import type { NeedAlert, NeedTag } from './types'

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
}

export const DEFAULT_FILTERS: NeededFilters = {
  needTypes: [],
  bands: [],
  modes: { ...ALL_MODES_ON },
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
  'all', 'wanted', 'atno', 'newBand', 'newMode', 'newZone', 'newGrid', 'newState', 'newPark', 'confirm', 'dxped', 'pota', 'sota',
]

/** True when the alert matches the given filter set (all filters AND together).
 *
 * `watched` answers the Watch list chip: whether the operator's watch list names this row's
 * station — by call or prefix, DXCC entity or grid, the WATCH tile's own matcher
 * (`watchlist.ts` `watchedEntry`). The board's rows carry no watch-list tag of their own: the
 * station's `Wanted` tag came from the old hidden list, retired 2026-09-24. A row an OLDER station
 * still tags `Wanted` keeps matching the chip through the tag map, as it always did. */
export function filterAlerts(
  alerts: NeedAlert[],
  filters: NeededFilters,
  watched: (a: NeedAlert) => boolean = () => false,
): NeedAlert[] {
  const wantWatched = filters.needTypes.includes('wanted')
  return alerts.filter((a) => {
    // ---- Need-type multi-select (OR across the picked buckets; empty = all) ----
    if (filters.needTypes.length > 0) {
      const matches =
        a.tags.some((t) => {
          const bucket = TAG_TO_BUCKET[t]
          return bucket !== undefined && filters.needTypes.includes(bucket)
        }) ||
        (wantWatched && watched(a))
      if (!matches) return false
    }

    // ---- Band multi-select ----
    if (filters.bands.length > 0) {
      if (!filters.bands.includes(a.band)) return false
    }

    // ---- Mode multi-select: keep only the operator's enabled modes (an unknown mode
    // class always shows, so the board never silently swallows a need it can't classify).
    // RTTY/FT8/FT4 are Digital submodes (no separate chip), so the Digital toggle governs them. ----
    const cls = (
      a.mode === 'RTTY' || a.mode === 'FT8' || a.mode === 'FT4' ? 'Digital' : a.mode
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
