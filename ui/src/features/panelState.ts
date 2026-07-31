// Panel VISIBILITY record — which panels a surface renders docked, which are torn off
// into their own window, and which the operator deleted. Pure (no JSX) so it unit-tests
// without React, and modelled on features/paneLayout.ts: the same coerce/load/save
// shape, defaults fill, unknown-id drop, and try/catch on storage.
//
// Deliberately SEPARATE from paneLayout. Placement decides WHICH pane occupies a grid
// cell; visibility decides WHETHER it renders. Because coercePlacement never sees a
// removal it cannot resurrect a panel you deleted — the hazard is unrepresentable
// rather than guarded — and the Classic/Roster presets stop fighting the operator for
// the same reason.
//
// TX-safety: TX controls are NOT panels. The dial, the band/mode pickers, the Rx/Tx
// offset spinners and the QSO strip's TX On / Tune / Stop TX / Hold Tx have no id in
// any vocabulary here, so there is no menu entry, no stored value and no coercion rule
// that can reach them. That is strictly stronger than a `removable: false` flag, which
// a bad code path or a hand-edited blob could bypass.
import { useCallback, useMemo, useState } from 'react'
import { windowInstance } from './windowScope'

export type PanelState = 'docked' | 'popped' | 'removed'

const PANEL_STATES = ['docked', 'popped', 'removed'] as const

export interface PanelLayout<P extends string> {
  v: 1
  /** Absent ⇒ docked. Partial is deliberate: a panel added in a later release ships
   *  visible with no migration, and a removal is always an EXPLICIT stored value, so it
   *  can never be confused with "new". */
  state: Partial<Record<P, PanelState>>
  /** Flex/fr share within its region. Nothing writes it yet (seam resize is a later
   *  step); it rides in the record from commit one so that step needs no version bump. */
  share: Partial<Record<P, number>>
}

/** A view's panel vocabulary: its storage namespace plus the coercion whitelist. */
export interface PanelVocabulary<P extends string> {
  /** View id — the `<view>` in `nexus.panels.<view>.<instance>`. */
  readonly view: string
  /** Every panel this view can hide. TX chrome is deliberately absent (see above). */
  readonly panelIds: readonly P[]
}

export function isPanelState(v: unknown): v is PanelState {
  return typeof v === 'string' && (PANEL_STATES as readonly string[]).includes(v)
}

/** Stock layout: nothing stored, so every panel is docked. */
export function emptyPanelLayout<P extends string>(): PanelLayout<P> {
  return { v: 1, state: {}, share: {} }
}

/** `nexus.panels.<view>.<instance>` — one record per SURFACE (see windowScope).
 *
 *  Built here rather than via `surfaceKey`, and deliberately: this key shipped in 0.15.0
 *  ALREADY suffixed on the main window (`nexus.panels.operate.main`), so for it the
 *  byte-identical-across-upgrade string is the SUFFIXED one. Every other per-surface key
 *  predates instances and must stay bare on `main`, which is the rule `surfaceKey`
 *  encodes. Routing this key through it would rename it and lose saved layouts. */
export function panelStorageKey(view: string, instance?: string): string {
  return `nexus.panels.${view}.${instance ?? windowInstance()}`
}

/**
 * A valid record from any input. Unknown panel ids and unknown state strings are
 * dropped, a junk blob coerces to the stock layout, and a share that isn't a finite
 * positive number is discarded — one that is gets CLAMPED into the writers' own range.
 * Mirrors coercePlacement's "missing → safe default".
 */
export function coercePanelLayout<P extends string>(
  spec: PanelVocabulary<P>,
  raw: unknown,
): PanelLayout<P> {
  const out = emptyPanelLayout<P>()
  if (!raw || typeof raw !== 'object') return out
  const obj = raw as { state?: unknown; share?: unknown }
  if (obj.state && typeof obj.state === 'object') {
    const src = obj.state as Record<string, unknown>
    for (const id of spec.panelIds) {
      const v = src[id]
      if (isPanelState(v)) out.state[id] = v
    }
  }
  if (obj.share && typeof obj.share === 'object') {
    const src = obj.share as Record<string, unknown>
    for (const id of spec.panelIds) {
      const v = src[id]
      // Clamp into [MIN_SHARE, 2 − MIN_SHARE] — the exact range the writers enforce
      // (setShare/setShares floor, seamShares two-pane cap). Load used to accept any
      // v > 0, so a hand-edited/foreign 1e-9 collapsed a pane to ~0 height on the one
      // path the setters cannot guard.
      if (typeof v === 'number' && Number.isFinite(v) && v > 0) {
        out.share[id] = Math.min(2 - MIN_SHARE, Math.max(MIN_SHARE, v))
      }
    }
  }
  return out
}

export function savePanelLayout<P extends string>(key: string, layout: PanelLayout<P>): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(layout))
  } catch {
    /* full/unavailable — in-memory state still applies this session */
  }
}

/** The app-global pop-out flag this record replaces. Still written by the torn-off
 *  waterfall window as its open/close signal until Step 4's real window event lands. */
export const WATERFALL_DETACHED_KEY = 'nexus.waterfall.detached'
/** Marker so the bridge below runs exactly once, ever (migrateChaseDefault idiom). */
const WATERFALL_MIGRATED_KEY = 'nexus.panels.wfDetached.v1'

/**
 * One-time bridge: an operator whose waterfall was popped out when this record landed
 * keeps it popped out instead of getting a surprise docked strip. Guarded by a
 * persisted marker so a later re-dock can never be undone by a stale global flag —
 * after this the record is the only source of truth.
 */
function migrateWaterfallDetached<P extends string>(
  spec: PanelVocabulary<P>,
  key: string,
  layout: PanelLayout<P>,
): PanelLayout<P> {
  const wf = 'waterfall' as P
  if (!(spec.panelIds as readonly string[]).includes(wf)) return layout
  let popped = false
  try {
    if (localStorage.getItem(WATERFALL_MIGRATED_KEY)) return layout
    popped = localStorage.getItem(WATERFALL_DETACHED_KEY) === '1'
    localStorage.setItem(WATERFALL_MIGRATED_KEY, '1')
  } catch {
    return layout // storage blocked — leave the layout untouched
  }
  if (!popped || layout.state[wf]) return layout
  const state: Partial<Record<P, PanelState>> = { ...layout.state }
  state[wf] = 'popped'
  const next: PanelLayout<P> = { ...layout, state }
  savePanelLayout(key, next)
  return next
}

export function loadPanelLayout<P extends string>(
  spec: PanelVocabulary<P>,
  instance?: string,
): PanelLayout<P> {
  const key = panelStorageKey(spec.view, instance)
  let layout = emptyPanelLayout<P>()
  try {
    const raw = window.localStorage.getItem(key)
    if (raw != null) layout = coercePanelLayout(spec, JSON.parse(raw))
  } catch {
    /* malformed — fall through (matches loadPlacement) */
  }
  return migrateWaterfallDetached(spec, key, layout)
}

/**
 * Fresh main-window boot: a torn-off panel window never survives an app restart (only
 * the main window is restored), so a stored 'popped' is stale — re-dock it, or the
 * operator relaunches to a re-dock bar and no window to re-dock from. This is the
 * record-level version of the boot-clear the app-global flag already had. A 'removed'
 * panel is an explicit choice and is left exactly as it is.
 */
export function redockStalePopouts<P extends string>(
  spec: PanelVocabulary<P>,
  instance?: string,
): void {
  const layout = loadPanelLayout(spec, instance)
  const state: Partial<Record<P, PanelState>> = { ...layout.state }
  let changed = false
  for (const id of spec.panelIds) {
    if (state[id] === 'popped') {
      state[id] = 'docked'
      changed = true
    }
  }
  if (changed) savePanelLayout(panelStorageKey(spec.view, instance), { ...layout, state })
}

/** A pane can never be resized below this flex share — a seam drag clamps here so a pane
 *  stays grabbable and can't vanish. Removal (via the Panels menu) is the ONLY route to
 *  gone, which keeps the "a removed panel is unrepresentable-as-resurrectable" guarantee. */
export const MIN_SHARE = 0.15

/**
 * Split a region between two adjacent panes from the seam position, given as a fraction
 * (0..1) of the region from the top/left. Returns `[aboveShare, belowShare]` normalized to
 * sum to 2 — so the centre (0.5) is the stock `[1, 1]` — and clamped so neither pane drops
 * below `MIN_SHARE`. Pure + monotonic in `fraction`, which is why it can't oscillate: the
 * output depends only on the pointer position, never on the current size it is setting.
 */
export function seamShares(fraction: number): [number, number] {
  const f = Math.min(1, Math.max(0, fraction))
  // Clamp BOTH output shares to the MIN_SHARE floor (not the fraction) so the floor holds
  // exactly with no float-rounding slack; the pair still sums to ~2 (relative flex, so the
  // region stays full).
  const above = Math.max(MIN_SHARE, Math.min(2 - MIN_SHARE, 2 * f))
  const below = Math.max(MIN_SHARE, 2 - above)
  return [above, below]
}

export interface PanelLayoutApi<P extends string> {
  layout: PanelLayout<P>
  /** Absent ⇒ docked. */
  stateOf: (id: P) => PanelState
  setPanelState: (id: P, state: PanelState) => void
  /** Flex-grow share for a pane within its region (default 1). A sole surviving pane
   *  auto-fills because grow is relative; two co-resident panes split by their ratio. */
  shareOf: (id: P) => number
  /** Set one pane's flex share (clamped to at least MIN_SHARE). */
  setShare: (id: P, value: number) => void
  /** Set several panes' shares in ONE undoable step — a seam drag redistributes the two
   *  adjacent panes it sits between atomically (a single save + one undo entry). */
  setShares: (updates: Partial<Record<P, number>>) => void
  /** Restore the layout as it was before the last change (one level deep). */
  undo: () => void
  canUndo: boolean
  /** Back to stock — every panel docked, every share reset. Undoable like any change. */
  reset: () => void
}

/**
 * The visibility record for one surface. MUST be owned by a host that outlives the
 * view it describes (App's `.operate-host` keep-alive, not the cockpit itself) — and
 * every change SAVES SYNCHRONOUSLY INSIDE THE STATE UPDATER, never from a useEffect,
 * which is the exact shape of this app's remount-state-loss bugs.
 */
export function usePanelLayout<P extends string>(
  spec: PanelVocabulary<P>,
  instance?: string,
): PanelLayoutApi<P> {
  const key = useMemo(() => panelStorageKey(spec.view, instance), [spec.view, instance])
  // Current + previous in ONE state so the undo snapshot is taken by the same updater
  // that saves — two useStates could not do that atomically.
  const [hist, setHist] = useState<{ cur: PanelLayout<P>; prev: PanelLayout<P> | null }>(() => ({
    cur: loadPanelLayout(spec, instance),
    prev: null,
  }))
  const apply = useCallback(
    (next: (cur: PanelLayout<P>) => PanelLayout<P>) =>
      setHist((h) => {
        const cur = next(h.cur)
        savePanelLayout(key, cur)
        return { cur, prev: h.cur }
      }),
    [key],
  )
  const stateOf = useCallback((id: P) => hist.cur.state[id] ?? 'docked', [hist.cur])
  const setPanelState = useCallback(
    (id: P, s: PanelState) =>
      apply((cur) => {
        const state: Partial<Record<P, PanelState>> = { ...cur.state }
        state[id] = s
        return { ...cur, state }
      }),
    [apply],
  )
  const shareOf = useCallback((id: P) => hist.cur.share[id] ?? 1, [hist.cur])
  const setShare = useCallback(
    (id: P, value: number) =>
      apply((cur) => {
        const share: Partial<Record<P, number>> = { ...cur.share }
        share[id] = Math.max(MIN_SHARE, value)
        return { ...cur, share }
      }),
    [apply],
  )
  const setShares = useCallback(
    (updates: Partial<Record<P, number>>) =>
      apply((cur) => {
        const share: Partial<Record<P, number>> = { ...cur.share }
        for (const [id, v] of Object.entries(updates)) {
          if (typeof v === 'number' && Number.isFinite(v)) {
            share[id as P] = Math.max(MIN_SHARE, v)
          }
        }
        return { ...cur, share }
      }),
    [apply],
  )
  const undo = useCallback(
    () =>
      setHist((h) => {
        if (!h.prev) return h
        savePanelLayout(key, h.prev)
        return { cur: h.prev, prev: null }
      }),
    [key],
  )
  const reset = useCallback(() => apply(() => emptyPanelLayout<P>()), [apply])
  return {
    layout: hist.cur,
    stateOf,
    setPanelState,
    shareOf,
    setShare,
    setShares,
    undo,
    canUndo: hist.prev != null,
    reset,
  }
}

/** The Operate cockpit's removable panels — the first consumer's vocabulary. */
export const OPERATE_PANEL_IDS = [
  'waterfall',
  'bandActivity',
  'callRoster',
  'rxfreq',
  'txmsgs',
  'stations',
  'txmeters',
] as const
export type OperatePanelId = (typeof OPERATE_PANEL_IDS)[number]

export const OPERATE_PANELS: PanelVocabulary<OperatePanelId> = {
  view: 'operate',
  panelIds: OPERATE_PANEL_IDS,
}

/** SSTV view's removable panels (Phase 3). The RX image (scope), the TX bar (mode/Send/Stop/
 *  progress) and all header chrome are NOT panels — TX-safety by construction. */
export const SSTV_PANEL_IDS = ['txcompose', 'gallery'] as const
export type SstvPanelId = (typeof SSTV_PANEL_IDS)[number]

export const SSTV_PANELS: PanelVocabulary<SstvPanelId> = {
  view: 'sstv',
  panelIds: SSTV_PANEL_IDS,
}

/** Phone cockpit's removable panels (Phase 3) — the panes UNDER the scope. The scope, the
 *  whole CockpitHeader (mode/band/power/Tune/StopTX/split/CAT/mic/BW), the PTT row, the voice
 *  keyer, and the log strip are NOT panels (TX-safety by construction). */
export const PHONE_PANEL_IDS = ['rigscope', 'txmeters', 'dsp', 'dspLevels', 'bandActivity'] as const
export type PhonePanelId = (typeof PHONE_PANEL_IDS)[number]

export const PHONE_PANELS: PanelVocabulary<PhonePanelId> = {
  view: 'phone',
  panelIds: PHONE_PANEL_IDS,
}

/** CW cockpit's removable panels (Phase 3) — the panes under the scope. The scope, the whole
 *  CockpitHeader + keyer (WPM/backend/pitch/BW/Tune/StopTX), the F-key macros, the type-to-send
 *  input, and the log strip are NOT panels (TX-safety by construction). */
export const CW_PANEL_IDS = [
  'scopeCtl',
  'dsp',
  'txmeters',
  'rxdsp',
  'bandActivity',
  'copilot',
  'decode',
  'sent',
] as const
export type CwPanelId = (typeof CW_PANEL_IDS)[number]

export const CW_PANELS: PanelVocabulary<CwPanelId> = {
  view: 'cw',
  panelIds: CW_PANEL_IDS,
}

/** RTTY cockpit's removable panels (Phase 3). The waterfall, the CockpitHeader + StopTX + TX-arm,
 *  the auto-sequence QSO strip, the F-key macros, and the type-to-send compose are NOT panels. */
export const RTTY_PANEL_IDS = ['stream'] as const
export type RttyPanelId = (typeof RTTY_PANEL_IDS)[number]

export const RTTY_PANELS: PanelVocabulary<RttyPanelId> = {
  view: 'rtty',
  panelIds: RTTY_PANEL_IDS,
}
