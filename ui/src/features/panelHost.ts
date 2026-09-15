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
  /** What unticking this pane ENDS — the CONSEQUENCE half of `notes`, in its own field so
   *  the pane's own ✕ can carry it and an availability reason cannot.
   *
   *  Both kinds used to live in `notes`, which is fine for a menu (the operator's question
   *  there is the same either way) and wrong for a ✕: "your radio is not reporting DSP
   *  functions over CAT" is not a warning about pressing Hide. Split, not duplicated —
   *  `menuItems` merges this back over `notes`, so the ⊞ entry reads exactly as it did and
   *  there is still ONE wording per pane. THE PRACTICE half of THE STOP LINE
   *  (features/panelState.ts) is courtesy, not safety, and it says the consequence goes
   *  BEFORE the act: the menu prints it under the checkbox, the ✕ carries it as its tooltip
   *  and its accessible description. Exactly one pane in the app populates this today
   *  (Phone's voiceKeyer). RTTY's `stream` hosts a stop control but its hide ends nothing —
   *  unmounting it calls no wire — so it correctly carries neither. */
  readonly endsOnHide?: Partial<Record<P, string | undefined>>
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
  /** THE PANE'S OWN ✕ — the props that make a pane header's close button do EXACTLY what
   *  unticking the same entry in ⊞ Panels does: the same `setPanelState(id, 'removed')`,
   *  the same record, the same persistence. There is one mechanism and this is a second
   *  DOOR onto it, which is the whole of the fix (operator, 2026-09-15: he went looking for
   *  a way to close a panel and could not find one — the capability shipped, the
   *  affordance did not).
   *
   *  `{}` for an id this layout does not list, because a pane with no entry is not
   *  removable and must get no ✕ — the same reason CockpitPaneFrame draws no button when
   *  `onRemove` is omitted. Spread it: `<CockpitPaneFrame {...host.closeProps('dsp')} …>`.
   *  Restoring stays the menu's job; nothing here brings a pane back. */
  closeProps: (id: P) => { onRemove?: () => void; hideNote?: string }
}

/**
 * Derive one cockpit layout's panel-render glue from its API + spec. Everything but
 * `closeProps` reads only `stateOf`, so the derivations stay trivially testable and cannot
 * mutate anything; `closeProps` needs the one writer (`setPanelState`) because a pane's ✕
 * must reach the SAME act as the menu tick rather than a second copy of it.
 */
export function panelHost<P extends string>(
  api: Pick<PanelLayoutApi<P>, 'stateOf' | 'setPanelState'>,
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
      // A consequence outranks an availability reason: a pane that is empty right now AND
      // ends something on its way out must say the second — the operator can act on it.
      note: spec.endsOnHide?.[id] ?? spec.notes?.[id],
    })),
    closeProps: (id) =>
      (spec.menu as readonly string[]).includes(id)
        ? { onRemove: () => api.setPanelState(id, 'removed'), hideNote: spec.endsOnHide?.[id] }
        : {},
  }
}
