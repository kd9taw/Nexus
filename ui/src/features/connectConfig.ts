// Connect pane-grid configuration — the wrap-the-globe assignable grid (B1). Pure
// (no JSX) so it mirrors features/state.ts and can be unit-tested without React. The
// id vocabulary + DEFAULT_SLOTS live here; components/connect/* build on top.
import { useCallback, useState } from 'react'
import { assignIn, coercePlacement, type PaneVocabulary } from './paneLayout'
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
  'scope', 'amp', 'kpOutlook',
] as const
export type PaneId = (typeof PANE_IDS)[number]

export function isPaneId(v: unknown): v is PaneId {
  return typeof v === 'string' && (PANE_IDS as readonly string[]).includes(v)
}

/** Recommended first-run Basic layout: static conditions reference framing the left,
 *  selection-driven on the right, live "now" ticker across the bottom (HamClock model). */
export const DEFAULT_SLOTS: Record<SlotId, PaneId> = {
  left1: 'advisory',
  left2: 'bandAdvisor',
  right1: 'chase', // flagship "work THIS now" — Selection stays one dropdown-click away
  right2: 'outlook',
  bottom1: 'openings',
  bottom2: 'spacewx',
  bottom3: 'getout',
}

export interface ConnectConfig {
  slots: Record<SlotId, PaneId> // complete record after normalize (coerceEnabled idiom)
  overlays: Record<string, boolean> // reserved for B2/B3 map overlays; inert in B1
}

// PER-SURFACE: which pane sits in which slot is literally this window's board layout.
const STORAGE_KEY = 'nexus.connect.config'
export function defaultConnectConfig(): ConnectConfig {
  return { slots: { ...DEFAULT_SLOTS }, overlays: {} }
}

/** This view's pane-grid vocabulary. The placement RULES (defaults fill, unknown-id
 *  drop, permutation repair, swap-on-assign) live in features/paneLayout so Operate and
 *  later views share them. Deliberately a PaneVocabulary and not a PaneLayoutSpec:
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
  return { slots: coerceSlots(obj.slots), overlays: coerceOverlays(obj.overlays) }
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
  if (SLOT_IDS.some((s) => cfg.slots[s] === 'chase')) return cfg // already placed (fresh default)
  const slots = { ...cfg.slots }
  const target = SLOT_IDS.find((s) => slots[s] === 'selection') ?? 'right1'
  slots[target] = 'chase'
  const next = { ...cfg, slots }
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
  /** Assign a pane to a slot; if it already lives elsewhere, the two SWAP so the
   *  displaced pane keeps a home (the grid stays a permutation — nothing vanishes). */
  assignPane: (slotId: SlotId, paneId: PaneId) => void
  setOverlay: (overlayId: string, on: boolean) => void
}

export function useConnectConfig(): ConnectConfigApi {
  const [cfg, setCfg] = useState<ConnectConfig>(loadConnectConfig)
  const commit = useCallback((next: ConnectConfig) => {
    saveConnectConfig(next)
    return next
  }, [])

  const assignPane = useCallback(
    (slotId: SlotId, paneId: PaneId) =>
      setCfg((c) => commit({ ...c, slots: assignIn(CONNECT_PANES, c.slots, slotId, paneId) })),
    [commit],
  )

  const setOverlay = useCallback(
    (overlayId: string, on: boolean) =>
      setCfg((c) => commit({ ...c, overlays: { ...c.overlays, [overlayId]: on } })),
    [commit],
  )

  return { ...cfg, assignPane, setOverlay }
}
