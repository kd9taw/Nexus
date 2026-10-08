// Connect pane-grid configuration — the wrap-the-globe assignable grid (B1). Pure
// (no JSX) so it mirrors features/state.ts and can be unit-tested without React. The
// id vocabulary + DEFAULT_SLOTS live here; components/connect/* build on top.
import { useCallback, useState } from 'react'
import { coercePlacement, type PaneVocabulary } from './paneLayout'
import { surfaceGet, surfaceSet } from './windowScope'

/** The 7 wrap-the-globe slots. A SlotId === its CSS grid-area name (see styles.css). */
export const SLOT_IDS = ['left1', 'left2', 'right1', 'right2', 'bottom1', 'bottom2', 'bottom3'] as const
export type SlotId = (typeof SLOT_IDS)[number]

/** Every assignable pane. Core (B1) map to existing panels; B2 adds Tier-1 panes
 *  (pickable — DEFAULT_SLOTS keeps the approved core layout). B3 appends here too. */
export const PANE_IDS = [
  'advisory', 'bandAdvisor', 'selection', 'outlook', 'openings', 'openingsLog', 'spacewx', 'getout',
  'bestband', 'activity', 'beacons', 'insights', 'chase',
  'greyline', 'bandHours', 'esNowcast', 'measuredMuf', 'chaseFeed', 'satPasses', 'rotor', 'contests',
  'scope', 'amp', 'kpOutlook', 'bandTiles',
  'clock',
  'spots', 'pota',
  // The Needed board (2026-10-07), appended so a stored layout's repairs pick what they always picked.
  'needed',
] as const
export type PaneId = (typeof PANE_IDS)[number]

export function isPaneId(v: unknown): v is PaneId {
  return typeof v === 'string' && (PANE_IDS as readonly string[]).includes(v)
}

/** Recommended first-run Basic layout: static conditions reference framing the left,
 *  selection-driven on the right, live "now" ticker across the bottom (the wall-display model). */
export const DEFAULT_SLOTS: Record<SlotId, PaneId> = {
  left1: 'advisory',
  // "Bands for you" takes the Band Advisor's default slot; the ranked rows stay one pick away
  // (operator pick, 2026-09-28: "In the default slot"). A stored layout keeps what it has.
  left2: 'bandTiles',
  right1: 'chase', // flagship "work THIS now" — Selection stays one dropdown-click away
  right2: 'outlook',
  bottom1: 'openings',
  bottom2: 'spacewx',
  bottom3: 'getout',
}

export interface ConnectConfig {
  /** The pane SHOWN in each slot — complete after normalize (coerceEnabled idiom). With no tabs it
   *  is the slot's only pane, and it is all an older build reads: such a build opens every slot on
   *  the pane that was showing and ignores the rest. */
  slots: Record<SlotId, PaneId>
  /** TABS (2026-09-29): a slot's panes in tab order, when it holds more than one. Always contains
   *  `slots[s]`; absent for a one-pane slot, which is every slot of every layout saved before this.
   *  The placement rule is "each pane in at most one slot, a slot holds one or more" — the tab
   *  rules below keep it, and `coerceTabs` restores it against any stored value. */
  tabs: Partial<Record<SlotId, PaneId[]>>
  /** AUTO-ROTATE (2026-09-29): seconds between a slot's tabs, one of ROTATE_CHOICES. Offered and
   *  honoured only on the dashboard window and the TV page (ConnectView `autoRotate`); absent is
   *  off, and only a slot with tabs keeps one (`coerceRotate`). An older build ignores it. */
  rotate: Partial<Record<SlotId, number>>
  overlays: Record<string, boolean> // reserved for B2/B3 map overlays; inert in B1
}

// PER-SURFACE: which pane sits in which slot is literally this window's board layout.
const STORAGE_KEY = 'nexus.connect.config'
export function defaultConnectConfig(): ConnectConfig {
  return { slots: { ...DEFAULT_SLOTS }, tabs: {}, rotate: {}, overlays: {} }
}

/** The intervals a slot's tabs can rotate at, in seconds (10 s … 2 min). */
export const ROTATE_CHOICES = [10, 15, 30, 60, 120] as const

/** Stored intervals made valid: an offered interval, on a slot that holds tabs. Anything else is no
 *  entry, which is off — so a slot that goes back to one pane forgets its interval. */
export function coerceRotate(tabs: Partial<Record<SlotId, readonly PaneId[]>>, raw: unknown): Partial<Record<SlotId, number>> {
  const out: Partial<Record<SlotId, number>> = {}
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return out
  for (const s of SLOT_IDS) {
    const v = (raw as Record<string, unknown>)[s]
    if ((tabs[s]?.length ?? 0) > 1 && typeof v === 'number' && (ROTATE_CHOICES as readonly number[]).includes(v)) out[s] = v
  }
  return out
}

/** This view's pane-grid vocabulary. The placement RULES (defaults fill, unknown-id
 *  drop, permutation repair, swap-on-assign) live in features/paneLayout so Operate and
 *  later views share them; Connect's picker swaps through `assignBox` below, which is
 *  paneLayout's `assignIn` exactly until a slot holds tabs. Deliberately a PaneVocabulary and not a PaneLayoutSpec:
 *  Connect stores its placement inside its own config blob alongside its overlays, so it
 *  must not be able to reach load/savePlacement, which would overwrite that blob. */
const CONNECT_PANES: PaneVocabulary<SlotId, PaneId> = {
  slotIds: SLOT_IDS,
  paneIds: PANE_IDS,
  defaults: DEFAULT_SLOTS,
}

function coerceSlots(raw: unknown): Record<SlotId, PaneId> {
  return coercePlacement(CONNECT_PANES, raw)
}

function coerceOverlays(raw: unknown): Record<string, boolean> {
  const out: Record<string, boolean> = {}
  if (raw && typeof raw === 'object')
    for (const [k, v] of Object.entries(raw as Record<string, unknown>)) if (typeof v === 'boolean') out[k] = v
  return out
}

// Connect had a Basic/Expert detail toggle, defaulting NEW installs to Basic (one-sentence pane
// summaries). Removed 2026-07-26 (operator): every pane now renders its full panel. A stored
// `mode` key from an older install is simply ignored here — no migration needed, since there is
// no longer a setting for it to migrate to.
//
// ⚠️ The per-pane `basic()` projections SURVIVE and are still load-bearing: PaneFrame renders
// them whenever `expert()` returns null, which is how every pane shows its loading / no-data /
// offline hint. They are the empty state, not the removed mode.
export function normalizeConfig(raw: unknown): ConnectConfig {
  if (!raw || typeof raw !== 'object') return defaultConnectConfig()
  const obj = raw as Partial<ConnectConfig> & Record<string, unknown>
  const slots = coerceSlots(obj.slots)
  const tabs = coerceTabs(slots, obj.tabs)
  return { slots, tabs, rotate: coerceRotate(tabs, obj.rotate), overlays: coerceOverlays(obj.overlays) }
}

// ── TABS: several panes in one slot (2026-09-29) ──────────────────────────────────────────────
// A slot shows one pane at a time; its other panes wait behind it as tabs. The rules, all pure:
//   · a pane is in AT MOST ONE slot — shown there, or a tab behind the pane shown there;
//   · a slot's shown pane is always one of its tabs, and a slot never holds zero panes;
//   · with one pane per slot every rule is today's rule exactly (assignBox ≡ assignIn), which is
//     how every stored layout and every preset keeps behaving as it did.
// connectConfig.tabs.test.ts holds these over every slot and pane and a random walk of moves.

/** The slot/tab half of the config — what the tab rules read and write. */
export type ConnectPlacement = Pick<ConnectConfig, 'slots' | 'tabs'>

/** A slot's panes in tab order; a one-pane slot is `[its pane]`. */
export function slotBoxes(p: ConnectPlacement, s: SlotId): PaneId[] {
  const t = p.tabs[s]
  return t && t.length > 1 ? t : [p.slots[s]]
}

/** Stored tabs made valid against the shown panes: unknown ids and repeats dropped, a pane shown in
 *  another slot dropped (the shown pane wins — it is what is on screen and what an older build
 *  reads), a pane in two slots' tabs kept in the first in grid order, the shown pane put first if
 *  its own list lost it, and a list of one dropped. */
export function coerceTabs(slots: Record<SlotId, PaneId>, raw: unknown): Partial<Record<SlotId, PaneId[]>> {
  const out: Partial<Record<SlotId, PaneId[]>> = {}
  if (!raw || typeof raw !== 'object') return out
  const used = new Set<PaneId>(SLOT_IDS.map((s) => slots[s]))
  for (const s of SLOT_IDS) {
    const list = (raw as Record<string, unknown>)[s]
    if (!Array.isArray(list)) continue
    const tabs: PaneId[] = []
    for (const p of list) {
      if (!isPaneId(p)) continue
      if (p === slots[s]) {
        if (!tabs.includes(p)) tabs.push(p)
      } else if (!used.has(p)) {
        used.add(p)
        tabs.push(p)
      }
    }
    if (!tabs.includes(slots[s])) tabs.unshift(slots[s])
    if (tabs.length > 1) out[s] = tabs
  }
  return out
}

/** Every slot's pane list, copied — the working form of the rules below. */
function listsOf(p: ConnectPlacement): Record<SlotId, PaneId[]> {
  return Object.fromEntries(SLOT_IDS.map((s) => [s, [...slotBoxes(p, s)]])) as Record<SlotId, PaneId[]>
}

/** Back from the working form: only a slot holding two or more panes stores a list. */
function tabsOf(lists: Record<SlotId, PaneId[]>): Partial<Record<SlotId, PaneId[]>> {
  const out: Partial<Record<SlotId, PaneId[]>> = {}
  for (const s of SLOT_IDS) if (lists[s].length > 1) out[s] = lists[s]
  return out
}

/** The slot `id` is in, shown or behind, if any. */
function slotOf(lists: Record<SlotId, PaneId[]>, id: PaneId): SlotId | undefined {
  return SLOT_IDS.find((s) => lists[s].includes(id))
}

/** THE PICKER: show `id` in `slot`, in the place of the pane shown there. If `id` is already one of
 *  this slot's tabs, that tab is shown and nothing moves. If it is in another slot — shown there or a
 *  tab behind — the two swap places, so the displaced pane keeps a home (for one-pane slots this is
 *  exactly assignIn's swap). Otherwise the displaced pane leaves the grid, as today. */
export function assignBox(p: ConnectPlacement, slot: SlotId, id: PaneId): ConnectPlacement {
  const lists = listsOf(p)
  if (lists[slot].includes(id)) return { slots: { ...p.slots, [slot]: id }, tabs: p.tabs }
  const displaced = p.slots[slot]
  const from = slotOf(lists, id)
  lists[slot][lists[slot].indexOf(displaced)] = id
  const slots = { ...p.slots, [slot]: id }
  if (from) {
    lists[from][lists[from].indexOf(id)] = displaced
    if (p.slots[from] === id) slots[from] = displaced
  }
  return { slots, tabs: tabsOf(lists) }
}

/** ⋯ ▸ ADD A TAB: `id` joins `slot` as its last tab, shown. A pane in another slot MOVES here (a
 *  pane is never in two slots): a tab leaves that slot's tabs, and a shown pane leaves that slot
 *  showing its next tab (or the one before, when it was last). Null — refused — for a pane already
 *  here, and for another slot's ONLY pane, which would leave that slot empty (addableTo does not
 *  offer it). */
export function addTab(p: ConnectPlacement, slot: SlotId, id: PaneId): ConnectPlacement | null {
  const lists = listsOf(p)
  if (lists[slot].includes(id)) return null
  const slots = { ...p.slots }
  const from = slotOf(lists, id)
  if (from) {
    if (lists[from].length < 2) return null
    const j = lists[from].indexOf(id)
    lists[from].splice(j, 1)
    if (slots[from] === id) slots[from] = lists[from][Math.min(j, lists[from].length - 1)]
  }
  lists[slot].push(id)
  slots[slot] = id
  return { slots, tabs: tabsOf(lists) }
}

/** ⋯ ▸ REMOVE: take the SHOWN pane out of `slot`, which then shows its next tab (or the one before,
 *  when it was last). The pane leaves the grid; any picker puts it back. Null for a slot's only pane:
 *  ✕ is what closes a slot. */
export function removeTab(p: ConnectPlacement, slot: SlotId): ConnectPlacement | null {
  const lists = listsOf(p)
  if (lists[slot].length < 2) return null
  const j = lists[slot].indexOf(p.slots[slot])
  lists[slot].splice(j, 1)
  return { slots: { ...p.slots, [slot]: lists[slot][Math.min(j, lists[slot].length - 1)] }, tabs: tabsOf(lists) }
}

/** Show one of `slot`'s tabs; anything that is not one of them changes nothing. */
export function showTab(p: ConnectPlacement, slot: SlotId, id: PaneId): ConnectPlacement {
  return slotBoxes(p, slot).includes(id) && p.slots[slot] !== id ? { slots: { ...p.slots, [slot]: id }, tabs: p.tabs } : p
}

/** What ⋯ ▸ Add a tab offers for `slot`, in the picker's order: every pane not already in it,
 *  except another slot's only pane. Exactly the panes addTab accepts. */
export function addableTo(p: ConnectPlacement, slot: SlotId): PaneId[] {
  const lists = listsOf(p)
  return PANE_IDS.filter((id) => {
    const from = slotOf(lists, id)
    return from !== slot && (!from || lists[from].length > 1)
  })
}

/** Flag so the one-time Chase promotion runs exactly once (persisted, survives edits). */
const CHASE_DEFAULT_KEY = 'nexus.connect.chaseDefault.v1'

/** One-time: give the flagship Chase pane a home for operators whose layout predates it.
 * A persisted config fully overrides DEFAULT_SLOTS, so a newly-defaulted pane never appears
 * otherwise. Chase takes the Selection slot (Selection stays available in the picker); the
 * migrated layout is persisted so the swap sticks even before the operator touches anything. */
function migrateChaseDefault(cfg: ConnectConfig): ConnectConfig {
  try {
    if (localStorage.getItem(CHASE_DEFAULT_KEY)) return cfg
    localStorage.setItem(CHASE_DEFAULT_KEY, '1')
  } catch {
    return cfg // storage blocked — leave the layout untouched
  }
  if (SLOT_IDS.some((s) => slotBoxes(cfg, s).includes('chase'))) return cfg // already placed, shown or a tab
  const slots = { ...cfg.slots }
  const target = SLOT_IDS.find((s) => slots[s] === 'selection') ?? 'right1'
  slots[target] = 'chase'
  // Re-read the tabs against the new shown pane: the one it replaced stays as a tab of its slot.
  const next = { ...cfg, slots, tabs: coerceTabs(slots, cfg.tabs) }
  saveConnectConfig(next)
  return next
}

export function loadConnectConfig(): ConnectConfig {
  try {
    const raw = surfaceGet(STORAGE_KEY)
    if (raw != null) return migrateChaseDefault(normalizeConfig(JSON.parse(raw)))
  } catch {
    /* malformed — fall through (matches useFeatures.readInitial) */
  }
  return migrateChaseDefault(defaultConnectConfig())
}

export function saveConnectConfig(c: ConnectConfig): void {
  surfaceSet(STORAGE_KEY, JSON.stringify(c))
}

export interface ConnectConfigApi extends ConnectConfig {
  /** Show a pane in a slot, in the place of the pane shown there; if it already lives elsewhere,
   *  the two SWAP so the displaced pane keeps a home (assignBox — with one pane per slot, the grid
   *  stays the permutation it always was). */
  assignPane: (slotId: SlotId, paneId: PaneId) => void
  /** ⋯ ▸ Add a tab (addTab). A refused move changes nothing. */
  addTab: (slotId: SlotId, paneId: PaneId) => void
  /** ⋯ ▸ Remove the shown pane from its slot (removeTab). A slot's only pane stays. */
  removeTab: (slotId: SlotId) => void
  /** Show one of a slot's tabs. */
  showTab: (slotId: SlotId, paneId: PaneId) => void
  /** Rotate a slot's tabs every `secs` (one of ROTATE_CHOICES), or stop (null). */
  setRotate: (slotId: SlotId, secs: number | null) => void
  setOverlay: (overlayId: string, on: boolean) => void
  /** Every slot back to its DEFAULT_SLOTS pane, one pane per slot (⊞ Reset layout — operator
   *  2026-09-13). */
  resetSlots: () => void
  /** Put a whole placement back (⊞ Undo after a Reset or a layout; a layout, with no tabs, is one
   *  pane per slot). Coerced, so every rule above holds whatever it is handed. */
  restoreSlots: (
    slots: Record<SlotId, PaneId>,
    tabs?: Partial<Record<SlotId, PaneId[]>>,
    rotate?: Partial<Record<SlotId, number>>,
  ) => void
}

/** A new placement on `c`, keeping only the intervals its tabs still allow: a slot back to one pane
 *  forgets its rotation. */
function placed(c: ConnectConfig, next: ConnectPlacement): ConnectConfig {
  return { ...c, ...next, rotate: coerceRotate(next.tabs, c.rotate) }
}

export function useConnectConfig(): ConnectConfigApi {
  const [cfg, setCfg] = useState<ConnectConfig>(loadConnectConfig)
  const commit = useCallback((next: ConnectConfig) => {
    saveConnectConfig(next)
    return next
  }, [])

  const assignPane = useCallback(
    (slotId: SlotId, paneId: PaneId) => setCfg((c) => commit(placed(c, assignBox(c, slotId, paneId)))),
    [commit],
  )

  const addTabTo = useCallback(
    (slotId: SlotId, paneId: PaneId) =>
      setCfg((c) => {
        const next = addTab(c, slotId, paneId)
        return next ? commit(placed(c, next)) : c
      }),
    [commit],
  )

  const removeTabFrom = useCallback(
    (slotId: SlotId) =>
      setCfg((c) => {
        const next = removeTab(c, slotId)
        return next ? commit(placed(c, next)) : c
      }),
    [commit],
  )

  const showTabIn = useCallback(
    (slotId: SlotId, paneId: PaneId) =>
      setCfg((c) => {
        const next = showTab(c, slotId, paneId)
        return next === c ? c : commit({ ...c, ...next })
      }),
    [commit],
  )

  const setRotate = useCallback(
    (slotId: SlotId, secs: number | null) =>
      setCfg((c) => {
        const { [slotId]: _was, ...rest } = c.rotate
        return commit({ ...c, rotate: coerceRotate(c.tabs, secs == null ? rest : { ...rest, [slotId]: secs }) })
      }),
    [commit],
  )

  const setOverlay = useCallback(
    (overlayId: string, on: boolean) =>
      setCfg((c) => commit({ ...c, overlays: { ...c.overlays, [overlayId]: on } })),
    [commit],
  )

  const resetSlots = useCallback(
    () => setCfg((c) => commit({ ...c, slots: { ...DEFAULT_SLOTS }, tabs: {}, rotate: {} })),
    [commit],
  )

  const restoreSlots = useCallback(
    (slots: Record<SlotId, PaneId>, tabs?: Partial<Record<SlotId, PaneId[]>>, rotate?: Partial<Record<SlotId, number>>) =>
      setCfg((c) => {
        const s = coerceSlots(slots)
        const t = coerceTabs(s, tabs)
        return commit({ ...c, slots: s, tabs: t, rotate: coerceRotate(t, rotate) })
      }),
    [commit],
  )

  return {
    ...cfg,
    assignPane,
    addTab: addTabTo,
    removeTab: removeTabFrom,
    showTab: showTabIn,
    setRotate,
    setOverlay,
    resetSlots,
    restoreSlots,
  }
}
