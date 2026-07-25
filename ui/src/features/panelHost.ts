// The reusable panel-render glue, lifted out of OperateCockpit so every cockpit gets
// panels from a declarative SPEC instead of a hand-rolled copy. Pure (no JSX, no React) — it
// only DERIVES render decisions from the panel record + a per-layout spec:
//   - shown(id): a panel occupies its dock slot unless explicitly removed
//   - sideShown / mainShown: is the rail / the main cell still populated
//   - dataCols: the two → one column collapse when a region empties
//   - menuItems: ready-made PanelsMenu items
// A cockpit with layout modes builds one spec per mode; a mode-less cockpit builds exactly
// one. This is what makes "panels everywhere" a small per-cockpit spec, not a copy of Operate.
import type { PanelLayoutApi, PanelState } from './panelState'

export interface PanelHostSpec<P extends string> {
  /** Menu order = the panels THIS layout can show. A panel with no place in the current
   *  layout isn't listed, so it can't be ticked into nowhere. */
  readonly menu: readonly P[]
  /** Side-rail occupants — the rail unmounts when all of them are removed. */
  readonly side: readonly P[]
  /** The single main-cell panel — drives the two/one column collapse with the rail. */
  readonly main: P
  /** Display label per panel. */
  readonly labels: Record<P, string>
}

export interface PanelHost<P extends string> {
  /** A panel occupies its dock slot unless explicitly removed (popped still counts as
   *  "has a slot" — its dock renders a re-dock affordance). */
  shown: (id: P) => boolean
  /** Any side-rail panel still docked. */
  sideShown: boolean
  /** The main-cell panel still shown. */
  mainShown: boolean
  /** 'two' when both the main cell and the rail hold content, else 'one' (grid collapse
   *  so the big pane reclaims the space). */
  dataCols: 'one' | 'two'
  /** Ready-made PanelsMenu items for this layout. */
  menuItems: Array<{ id: P; label: string; state: PanelState }>
}

/**
 * Derive one cockpit layout's panel-render glue from its API + spec. Takes only the READ
 * side of the panel API (`stateOf`) so it's trivially testable and can't mutate anything.
 */
export function panelHost<P extends string>(
  api: Pick<PanelLayoutApi<P>, 'stateOf'>,
  spec: PanelHostSpec<P>,
): PanelHost<P> {
  const shown = (id: P) => api.stateOf(id) !== 'removed'
  const mainShown = shown(spec.main)
  const sideShown = spec.side.some(shown)
  return {
    shown,
    sideShown,
    mainShown,
    dataCols: mainShown && sideShown ? 'two' : 'one',
    menuItems: spec.menu.map((id) => ({ id, label: spec.labels[id], state: api.stateOf(id) })),
  }
}
