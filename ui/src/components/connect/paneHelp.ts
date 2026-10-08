// Where each Connect pane is described in the manual — the ⋯ menu's "? … in the manual" link
// (2026-09-29, the operator's pick: "a ? help link").
//
// The manual is docs/guide/, published chapter by chapter at hamradiotools.io/manual/<chapter>,
// whose heading ids are GitHub's slugs (the site renders the same markdown). Nexus has no manual
// viewer of its own, so the link opens the published page in the browser: the app-wide
// external-link interceptor routes it (externalLinks.ts), and the TV page's browser opens it itself.
//
// A pane gets the section that describes it: its own section where it has one, else the pane-grid
// section whose table has its row. A pane the manual does not describe yet has NO entry, and so no
// link — a link to a page that does not mention it would be worse than none. paneHelp.test.ts
// resolves every anchor against the chapter's own headings and names the panes without one.
//
// Pure data: no JSX, no strings shown to the operator (the words are BoxMenu's).
import type { PaneId } from '../../features/connectConfig'

export const MANUAL_BASE = 'https://hamradiotools.io/manual/'

/** A chapter (docs/guide/<chapter>.md) and a heading id in it. */
export interface PaneHelp {
  chapter: string
  anchor: string
}

const GRID: PaneHelp = { chapter: 'connect', anchor: 'the-pane-grid' }

export const PANE_HELP: Partial<Record<PaneId, PaneHelp>> = {
  advisory: GRID,
  bandAdvisor: GRID,
  bandTiles: GRID,
  // "Track propagation to a specific call" is about exactly these two panes.
  selection: { chapter: 'connect', anchor: 'track-propagation-to-a-specific-call' },
  outlook: { chapter: 'connect', anchor: 'track-propagation-to-a-specific-call' },
  openings: { chapter: 'connect', anchor: 'read-an-opening' },
  openingsLog: GRID,
  spacewx: GRID,
  kpOutlook: GRID,
  getout: GRID,
  bestband: GRID,
  activity: GRID,
  beacons: GRID,
  insights: GRID,
  chase: { chapter: 'connect', anchor: 'chase-whats-workable-now' },
  chaseFeed: { chapter: 'connect', anchor: 'chase-whats-workable-now' },
  greyline: GRID,
  bandHours: GRID,
  esNowcast: GRID,
  measuredMuf: GRID,
  satPasses: GRID,
  rotor: GRID,
  contests: GRID,
  scope: GRID,
  amp: { chapter: 'connect', anchor: 'the-amplifier-pane' },
  clock: GRID,
  spots: GRID,
  pota: GRID,
  needed: GRID,
}

/** The manual page for a pane, or null when the manual does not describe it. */
export function paneHelpUrl(id: PaneId): string | null {
  const h = PANE_HELP[id]
  return h ? `${MANUAL_BASE}${h.chapter}#${h.anchor}` : null
}
