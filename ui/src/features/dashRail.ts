// THE DASHBOARD RAIL'S RECORDS — a column of Connect boxes beside the operating cockpits. Pure apart
// from storage (connectConfig.ts's shape), so every rule unit-tests without React; the rail itself is
// components/DashRail.tsx.
//
// THE OPERATOR'S PICK (2026-09-29): off after an update; one click from ⊞ Panels or the NOW bar;
// remembered per section; never on small windows; nobody's cockpit narrows on update. So:
//   · ON/OFF is PER SECTION and absent means OFF (`nexus.dashrail.sections`) — a new install and an
//     update both open every cockpit exactly as wide as before.
//   · WHICH BOX SITS IN WHICH SLOT is one record for the window (`nexus.dashrail.config`), through the
//     placement rules every pane grid shares (features/paneLayout: defaults fill, unknown ids drop,
//     the column stays a permutation), so a slot's box is pickable from the whole Connect registry.
//     Whether a slot is shown, and the split between neighbours, is the panel record
//     (`DASH_PANELS` in features/panelState) — placement and visibility kept apart as Connect keeps
//     them.
//   · THE WIDTH is a stored preference fitted into the window on load and on every resize, through
//     Connect's rail clamps (features/connectRails): the stored value is never rewritten by a fit, so
//     a bigger window gets it back.
//   · "Never on small windows" is the published viewport class (useViewport): the rail renders
//     nothing below `lg`. Never a size-based @media.
//
// THE COCKPIT KEEPS ITS FLOOR. The rail may take only what the window has BEYOND the supported floor
// window (1024×768 at its own auto zoom): the cockpit beside it is never narrower than it is on a
// 1024×768 screen, where every cockpit is built and swept to work — whatever the navigation rail's
// width. Every cockpit's viewport rules are small-window compactions (sm/xs), none of which applies
// at lg, so a cockpit at least that wide is in a layout it already ships.
import { useCallback, useState } from 'react'
import { assignIn, coercePlacement, type PaneVocabulary } from './paneLayout'
import { PANE_IDS, type PaneId } from './connectConfig'
import { clampRail, fitRails } from './connectRails'
import { surfaceGet, surfaceSet } from './windowScope'
import { pickInitialZoom } from '../useScale'

/** The cockpits the rail stands beside — the operating sections whose shells size themselves to
 *  their own box. Tempo's conversation view is NOT one: its two rails are fitted against the WHOLE
 *  window (usePaneWidths), so a third column beside them would squeeze the conversation under the
 *  floor those fits promise it. The views that are not cockpits (Connect, the Logbook, Settings …)
 *  have no rail: Connect already is these boxes. */
export const DASH_RAIL_SECTIONS = ['operate', 'phone', 'cw', 'rtty', 'psk', 'sstv', 'aprs', 'js8'] as const
export type DashRailSection = (typeof DASH_RAIL_SECTIONS)[number]

export function isDashRailSection(v: string): v is DashRailSection {
  return (DASH_RAIL_SECTIONS as readonly string[]).includes(v)
}

/** The rail's four slots, top to bottom. */
export const DASH_SLOT_IDS = ['rail1', 'rail2', 'rail3', 'rail4'] as const
export type DashSlotId = (typeof DASH_SLOT_IDS)[number]

/** The boxes the operator picked for it: the time, what's open for you, space weather, who hears you. */
export const DASH_DEFAULT_SLOTS: Record<DashSlotId, PaneId> = {
  rail1: 'clock',
  rail2: 'bandTiles',
  rail3: 'spacewx',
  rail4: 'getout',
}

const DASH_PANES: PaneVocabulary<DashSlotId, PaneId> = {
  slotIds: DASH_SLOT_IDS,
  paneIds: PANE_IDS,
  defaults: DASH_DEFAULT_SLOTS,
}

// PER-SURFACE, like Connect's config: which box is in which slot is this window's arrangement.
const CONFIG_KEY = 'nexus.dashrail.config'

export function coerceDashSlots(raw: unknown): Record<DashSlotId, PaneId> {
  return coercePlacement(DASH_PANES, raw)
}

export function loadDashSlots(): Record<DashSlotId, PaneId> {
  try {
    const raw = surfaceGet(CONFIG_KEY)
    if (raw != null) return coerceDashSlots((JSON.parse(raw) as { slots?: unknown })?.slots)
  } catch {
    /* malformed — the defaults (connectConfig's rule) */
  }
  return { ...DASH_DEFAULT_SLOTS }
}

function saveDashSlots(slots: Record<DashSlotId, PaneId>): void {
  surfaceSet(CONFIG_KEY, JSON.stringify({ slots }))
}

export interface DashSlotsApi {
  slots: Record<DashSlotId, PaneId>
  /** A slot's box from the picker. Already in another slot, the two swap (the column stays a
   *  permutation — nothing vanishes). */
  assignPane: (slot: DashSlotId, pane: PaneId) => void
  /** Every slot back to its stock box (the rail's ⊞ Reset). */
  resetSlots: () => void
  /** A whole placement back (the rail's ⊞ Undo after a Reset). Coerced. */
  restoreSlots: (slots: Record<DashSlotId, PaneId>) => void
}

/** The placement, stored synchronously inside each state update (the remount-state-loss rule). */
export function useDashSlots(): DashSlotsApi {
  const [slots, setSlots] = useState(loadDashSlots)
  const commit = (next: Record<DashSlotId, PaneId>) => {
    saveDashSlots(next)
    return next
  }
  const assignPane = useCallback(
    (slot: DashSlotId, pane: PaneId) => setSlots((cur) => commit(assignIn(DASH_PANES, cur, slot, pane))),
    [],
  )
  const resetSlots = useCallback(() => setSlots(() => commit({ ...DASH_DEFAULT_SLOTS })), [])
  const restoreSlots = useCallback(
    (next: Record<DashSlotId, PaneId>) => setSlots(() => commit(coerceDashSlots(next))),
    [],
  )
  return { slots, assignPane, resetSlots, restoreSlots }
}

// ── ON / OFF, PER SECTION ──────────────────────────────────────────────────────────────────────

// PER-SURFACE: whether this window shows the rail beside a cockpit, section by section.
const SECTIONS_KEY = 'nexus.dashrail.sections'

export type DashRailSections = Partial<Record<DashRailSection, boolean>>

/** A stored record from any input: only a known section with a real boolean survives, so a junk or
 *  foreign value reads as OFF — the default the operator picked, never a surprise rail. */
export function coerceRailSections(raw: unknown): DashRailSections {
  const out: DashRailSections = {}
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return out
  for (const s of DASH_RAIL_SECTIONS) {
    const v = (raw as Record<string, unknown>)[s]
    if (typeof v === 'boolean') out[s] = v
  }
  return out
}

export function loadRailSections(): DashRailSections {
  try {
    const raw = surfaceGet(SECTIONS_KEY)
    return raw == null ? {} : coerceRailSections(JSON.parse(raw))
  } catch {
    return {}
  }
}

export interface DashRailSectionsApi {
  /** Whether `section` shows the rail. A section never set is OFF. */
  isOn: (section: string) => boolean
  setOn: (section: DashRailSection, on: boolean) => void
}

export function useDashRailSections(): DashRailSectionsApi {
  const [sections, setSections] = useState(loadRailSections)
  const isOn = useCallback(
    (section: string) => isDashRailSection(section) && sections[section] === true,
    [sections],
  )
  const setOn = useCallback(
    (section: DashRailSection, on: boolean) =>
      setSections((cur) => {
        const next = { ...cur, [section]: on }
        surfaceSet(SECTIONS_KEY, JSON.stringify(next))
        return next
      }),
    [],
  )
  return { isOn, setOn }
}

// ── WIDTH ──────────────────────────────────────────────────────────────────────────────────────

// PER-SURFACE: a width is a statement about one window's shape (Connect's rail widths' reasoning).
const WIDTH_KEY = 'nexus.dashrail.width'

/** The rail's width until the operator sizes it: Connect's own rail default at md and up. */
export const DASH_RAIL_DEFAULT_PX = 300

/** The supported floor window, in EFFECTIVE (zoom-corrected) CSS px: 1024 wide at the zoom Nexus
 *  opens that window at — read from the zoom policy itself (85 % today, so ~1205 px), not copied. */
export const FLOOR_EFFECTIVE_W = 1024 / (pickInitialZoom(1024, 768) / 100)

/** The most the rail may take in a window this wide: what the window has beyond the floor window.
 *  Rounded down, so the cockpit never comes out a pixel narrower than at the floor. */
export function dashRailRoom(effW: number): number {
  return Math.floor(effW - FLOOR_EFFECTIVE_W)
}

/** A stored width from any input: junk, zero, negatives and non-numbers are "never sized". */
export function parseRailWidth(raw: string | null): number | null {
  const v = Number(raw)
  return raw != null && raw !== '' && Number.isFinite(v) && v > 0 ? Math.round(v) : null
}

export function loadRailWidth(): number | null {
  return parseRailWidth(surfaceGet(WIDTH_KEY))
}

/** Store the preference; null is "back to the default". */
export function saveRailWidth(px: number | null): void {
  surfaceSet(WIDTH_KEY, px == null ? '' : String(Math.round(px)))
}

/** The LOAD and RESIZE path: the preference (or the default) fitted into this window, never below
 *  RAIL_MIN (a rail whose handle cannot be grabbed back is worse than a cockpit a few pixels short on a
 *  window below lg, where the rail does not render anyway) and never past RAIL_MAX. Connect's one-rail
 *  fit, with the room the cockpit's floor leaves. */
export function fitRailWidth(pref: number | null, effW: number): number {
  const fitted = fitRails(
    { left: null, right: pref },
    { room: dashRailRoom(effW), present: { left: false, right: true }, defaultPx: DASH_RAIL_DEFAULT_PX },
  ).right
  // null is fitRails leaving an unsized rail at exactly its default (Connect's stylesheet default);
  // this rail's width is always written inline, so it is that default in px.
  return fitted ?? DASH_RAIL_DEFAULT_PX
}

/** A MOVE (a key's step, a drag's release): clamped against the room as it is now. */
export function stepRailWidth(px: number, effW: number): number {
  return clampRail(px, 0, dashRailRoom(effW))
}

/** The widest the rail may be dragged in this window. */
export function railWidthMax(effW: number): number {
  return clampRail(Infinity, 0, dashRailRoom(effW))
}
