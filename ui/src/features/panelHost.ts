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
  /** Optional N-column geometry (the 3-col Classic grid): each entry lists the panels
   *  that populate one column. When present, `dataCols` becomes the POPULATED-column
   *  count — survivors flow into the smaller generic template exactly as the old
   *  two→one collapse did. */
  readonly columns?: readonly (readonly P[])[]
  /** Per-panel reason its pane cannot render at all in the CURRENT station state (no
   *  native scope streaming, say). The entry is listed but not checkable — ticking it
   *  would change nothing — and the reason rides along, so the operator learns what
   *  would bring it back instead of finding a checkbox that does nothing. Recompute it
   *  from the same condition the JSX gates on, or the menu drifts from the cockpit. */
  readonly unavailable?: Partial<Record<P, string | undefined>>
  /** Per-panel standing note for a pane that works but is only populated at certain
   *  times (TX meters read on transmit). Checkable, just annotated. */
  readonly notes?: Partial<Record<P, string | undefined>>
}

export interface PanelHost<P extends string> {
  /** A panel occupies its dock slot unless explicitly removed (popped still counts as
   *  "has a slot" — its dock renders a re-dock affordance). */
  shown: (id: P) => boolean
  /** Any side-rail panel still docked. */
  sideShown: boolean
  /** The main-cell panel still shown. */
  mainShown: boolean
  /** Populated-region count for the grid collapse. Without a `columns` spec: 'two'
   *  when both the main cell and the rail hold content, else 'one'. With one: the
   *  populated-column count ('one' | 'two' | 'three'), floored at 'one'. */
  dataCols: 'one' | 'two' | 'three'
  /** Ready-made PanelsMenu items for this layout (structurally a PanelsMenuItem). */
  menuItems: Array<{
    id: P
    label: string
    state: PanelState
    unavailable?: string
    note?: string
  }>
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
  const COUNT = ['one', 'one', 'two', 'three'] as const
  const dataCols = spec.columns
    ? COUNT[Math.max(1, spec.columns.filter((col) => col.some(shown)).length)]
    : mainShown && sideShown
      ? 'two'
      : 'one'
  return {
    shown,
    sideShown,
    mainShown,
    dataCols,
    menuItems: spec.menu.map((id) => ({
      id,
      label: spec.labels[id],
      state: api.stateOf(id),
      unavailable: spec.unavailable?.[id],
      note: spec.notes?.[id],
    })),
  }
}
