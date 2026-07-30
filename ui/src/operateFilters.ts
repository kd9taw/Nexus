// Persistence for the Operate cockpit's view filters: the Call Roster's Needed-only /
// Hide-worked checkboxes and the Band Activity filter chip. All three were plain useState,
// so every restart put them back to "show everything" and the operator re-ticked them at
// the start of each session (operator report).
//
// PER-SURFACE, the same classification as `neededFilters` (NeededPanel) and `nexus.ota.*`
// (PotaSotaView): a filter that answers "what does THIS pane show" belongs to its surface,
// and only person/station-wide preferences (nav order, theme, density, waterfall
// calibration) are shared. That is not theoretical here — `?panel=operate` is a real
// pop-out (DetachedPanel), and it renders both of these panes, so a torn-off Operate window
// can hold Needed-only on the DX roster while the docked one still shows the whole band.
//
// A surface that has never written INHERITS the main window's value (surfaceGet), so
// popping out opens on the filters already in use rather than on first-run defaults, and
// `main` writes the BARE key — nothing on disk to migrate.
//
// localStorage, not the sessionStorage SpotsPanel uses: that panel's contract is "retain
// until application exit", this one's is "survive a restart".

import { surfaceGet, surfaceSet } from './features/windowScope'
import { DECODE_FILTERS, type DecodeFilter } from './decodeHistory'

/** The Call Roster's two visibility checkboxes. */
export interface RosterFilters {
  /** Show only stations that carry a need tag. */
  neededOnly: boolean
  /** Drop already-worked stations (unless still needed). */
  hideWorked: boolean
}

/** Both off — the behaviour every build so far has started with, so an operator who never
 *  touches a checkbox sees no change. */
export const DEFAULT_ROSTER_FILTERS: RosterFilters = { neededOnly: false, hideWorked: false }

export const ROSTER_FILTER_KEY = 'nexus.roster.filters'
export const DECODE_FILTER_KEY = 'nexus.decodes.filter'

/**
 * Read the roster filters. Unparseable JSON, or a field that is not a boolean, falls back
 * to the default for THAT field: a half-written or hand-edited value must never leave the
 * roster hiding rows with no ticked checkbox to explain why.
 */
export function loadRosterFilters(): RosterFilters {
  const raw = surfaceGet(ROSTER_FILTER_KEY)
  if (!raw) return { ...DEFAULT_ROSTER_FILTERS }
  try {
    // A stored `null`, number or string parses fine and simply has no fields — each one
    // then takes its default, which is why this narrows per field instead of per object.
    const o = (JSON.parse(raw) ?? {}) as Partial<RosterFilters>
    return {
      neededOnly: typeof o.neededOnly === 'boolean' ? o.neededOnly : DEFAULT_ROSTER_FILTERS.neededOnly,
      hideWorked: typeof o.hideWorked === 'boolean' ? o.hideWorked : DEFAULT_ROSTER_FILTERS.hideWorked,
    }
  } catch {
    return { ...DEFAULT_ROSTER_FILTERS }
  }
}

export function saveRosterFilters(f: RosterFilters): void {
  surfaceSet(ROSTER_FILTER_KEY, JSON.stringify(f))
}

/**
 * Read the Band Activity chip. An unknown value — a chip renamed or dropped since it was
 * written — falls back to 'all' rather than filtering the pane down to nothing with no chip
 * lit, the same rule `NEED_TYPE_VALUES` enforces on the Needed board. Stored as the bare
 * chip name, not JSON: there is only ever one value.
 */
export function loadDecodeFilter(): DecodeFilter {
  const raw = surfaceGet(DECODE_FILTER_KEY)
  return DECODE_FILTERS.includes(raw as DecodeFilter) ? (raw as DecodeFilter) : 'all'
}

export function saveDecodeFilter(f: DecodeFilter): void {
  surfaceSet(DECODE_FILTER_KEY, f)
}
