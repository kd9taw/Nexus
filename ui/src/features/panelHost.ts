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

// ── The ⊞ menu's reason notes that more than one cockpit needs ─────────────────────────
// Copy for a menu entry lives with its SUBJECT when the subject renders it too
// (NO_NATIVE_SCOPE_REASON in waterfall.ts, TX_METERS_WHEN in TxMeters.tsx, which prints it
// as its own idle hint). These three exist only for the menu and are worded once here so
// Phone and CW cannot describe the same empty entry two different ways.

/** DSP function toggles (NB/NR/Notch, plus COMP/VOX on Phone) — capability-gated on what
 *  the rig reports over CAT, exactly as the DSP row itself has always been. */
export const NO_DSP_FUNCS_REASON =
  'your radio is not reporting DSP functions over CAT — these appear on a rig that does'

/** The NR-level / AGC sliders — the same CAT-capability gate, different fields. */
export const NO_DSP_LEVELS_REASON =
  'your radio is not reporting an NR level or AGC over CAT — these appear on a rig that does'

/** CW's Sent Echo: empty at every session start, so its entry is dead until the first over. */
export const NOTHING_SENT_REASON =
  'nothing has been sent this session — the echo appears with your first transmission'

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
  /** Per-panel line the entry carries. Usually the reason its pane has nothing on screen
   *  right now — because the station cannot render it at all (no native scope streaming,
   *  say) or because it is only populated at certain times (TX meters read on transmit).
   *  ONE field for both: the operator's question is the same either way, and the entry
   *  stays operable in both cases, because the tick is his preference rather than a claim
   *  about the station. Recompute it from the same condition the JSX gates on, or the menu
   *  drifts from the cockpit — `undefined` when the pane does have something to show.
   *
   *  The other use is a CONSEQUENCE: unticking some panes ENDS something already in flight
   *  (Phone's voice keyer stops a message and discards a recording). Its note is what makes
   *  that informed rather than surprising — THE PRACTICE half of THE STOP LINE
   *  (features/panelState.ts), courtesy rather than the safety rule, and still not optional:
   *  PhoneCockpit.keyerHide.test.tsx computes the pairing by hiding every id and asking the
   *  wire which hides stopped anything. A pane that merely TRANSMITS needs no note — hiding
   *  Operate's Tx messages ends nothing, and a warning there would be noise. */
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
      note: spec.notes?.[id],
    })),
  }
}
