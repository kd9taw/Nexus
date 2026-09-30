// Connect layout presets — Map first, List first and Dashboard (the UI look-and-feel redesign,
// 2026-09-26), and Frame (the dashboard window's wall-display layout). Pure (no JSX, no storage),
// so every rule unit-tests without React.
//
// A preset is a WHOLE ARRANGEMENT over the machinery Connect already has, and nothing else:
//   · which pane sits in each slot (features/connectConfig — the permutation grid);
//   · which slots are closed and how each rail is split (features/panelState CONNECT_PANELS);
//   · each rail's width preference (features/connectRails, clamped on load against the window).
// It stores nothing of its own. Which layout is on screen is READ BACK from those three records
// (`connectLayoutNow`), so the picker can never disagree with the screen: move or resize anything
// after picking one and it reads Custom, and nothing snaps back, because nothing remembers the
// pick to snap back to.
//
// ADDITIVE, BY OPERATOR RULING. DEFAULT_SLOTS is the approved first-run layout and stays the
// default (`STANDARD_LAYOUT` is it, by reference): nobody's Connect changes until they tap a
// preset, and a tap is the only way one applies — no migration, no first-run seed, no intent
// switch applies one.
//
// Deliberately NOT part of a preset: the map's own choices (projection, layers, colour — the
// per-intent map setup in features/intentMapSettings). Those are the operator's picks per intent;
// a layout tap that overwrote them would be overwriting a saved choice nobody asked it to touch.
// A preset decides how much of the window the map GETS, never how the map looks — with ONE
// exception, by operator pick (2026-09-29, "satellites on in the Frame layout"): a layout may name
// map layers it turns ON when it is tapped (`mapLayers`; Frame's satellites). Only on, never off; in
// the map on screen and the one behind the picker (the 2-D map's record for the intent in use, the
// 3-D globe's); nothing else about either map; and the layers are not part of what reads back, so
// unticking one afterwards is the operator's choice and the layout still reads as picked. Undo takes
// back exactly what the tap turned on (ConnectView).
import { DEFAULT_SLOTS, PANE_IDS, SLOT_IDS, type PaneId, type SlotId } from './connectConfig'
import { RAIL_MAX, RAIL_MIN, type RailWidths } from './connectRails'
import type { PanelLayout } from './panelState'

export const CONNECT_PRESET_IDS = ['mapFirst', 'listFirst', 'dashboard', 'frame'] as const
export type ConnectPresetId = (typeof CONNECT_PRESET_IDS)[number]

/** The map layers a layout may turn on (the header's one exception). A layer id both maps share. */
export const PRESET_MAP_LAYERS = ['sats'] as const
export type PresetMapLayer = (typeof PRESET_MAP_LAYERS)[number]

export interface ConnectLayout {
  /** A complete placement — every slot, no pane twice (validateConnectLayout). */
  slots: Readonly<Record<SlotId, PaneId>>
  /** The slots this layout closes. Their panes stay placed, so ⊞ Panels brings back exactly
   *  what the layout parked there. */
  hidden: readonly SlotId[]
  /** Rail width preferences in CSS px; null = the tier default. Fitted into the window on load
   *  and on every resize like any dragged width (fitRails), never trusted raw. */
  rails: { left: number | null; right: number | null }
  /** Map layers the tap turns ON (never off), on both maps; not part of what reads back. */
  mapLayers?: readonly PresetMapLayer[]
}

/** What the screen is built from right now — the three records a layout writes. */
export interface ConnectLayoutState {
  slots: Readonly<Record<SlotId, PaneId>>
  /** The slots holding more than one pane (connectConfig `tabs`). A layout is one pane per slot,
   *  so a screen with any tabs is the operator's own arrangement: it reads Custom, and a layout's
   *  tap puts one pane back in each slot. Absent is none. */
  tabs?: Readonly<Partial<Record<SlotId, readonly PaneId[]>>>
  panels: PanelLayout<SlotId>
  /** The STORED preferences, not the widths the window fitted them to: a list-first rail
   *  squeezed by a small window is still list-first, and reloading on a big one gives it back. */
  rails: RailWidths
}

/** The operator-approved default, and what ⊞ Reset layout restores. It is DEFAULT_SLOTS itself,
 *  not a copy, so it cannot drift from it. */
export const STANDARD_LAYOUT: ConnectLayout = {
  slots: DEFAULT_SLOTS,
  hidden: [],
  rails: { left: null, right: null },
}

const STRIP: readonly SlotId[] = ['bottom1', 'bottom2', 'bottom3']

/**
 * The three presets. Every pane in them is one Connect already offers; the reasons for each
 * choice are written beside it because they are measured, not taste (1920×1080 and 1024×768).
 */
export const CONNECT_PRESETS: Record<ConnectPresetId, ConnectLayout> = {
  // MAP FIRST — the map dominant, the lists narrow. The bottom row is closed, so the map takes the
  // full height, and both rails sit at their narrowest (RAIL_MIN), so it takes all but 400 px of
  // the width: at 1920×1080 the map cell grows from 1160×504 to 1360×840. The rails keep the
  // default's panes except one swap: Band Outlook's table needs width and wraps into columns in a
  // 200 px rail, while Space Wx's gauges are built for a narrow one, so the two trade places. Band
  // Outlook waits in the closed row with Openings and Getting Out.
  mapFirst: {
    slots: {
      left1: 'advisory',
      left2: 'bandAdvisor',
      right1: 'chase',
      right2: 'spacewx',
      bottom1: 'openings',
      bottom2: 'outlook',
      bottom3: 'getout',
    },
    hidden: STRIP,
    rails: { left: RAIL_MIN, right: RAIL_MIN },
  },
  // LIST FIRST — the spot and needed lists dominant, the map small. Connect's four lists fill two
  // wide, full-height rails: Chase (needed stations heard now) over Chase Feed (the ranked chase
  // board, needs and DXpeditions) on the left, Getting Out (who is hearing you) over Openings on
  // the right. At 560 px a row of each reads on one line; the map keeps the middle (640 px at
  // 1920×1080, its 280 px floor at 1024×768). The bottom row is closed because it would take list
  // height; the conditions panes wait in it.
  listFirst: {
    slots: {
      left1: 'chase',
      left2: 'chaseFeed',
      right1: 'getout',
      right2: 'openings',
      bottom1: 'advisory',
      bottom2: 'bandAdvisor',
      bottom3: 'spacewx',
    },
    hidden: STRIP,
    rails: { left: 560, right: 560 },
  },
  // DASHBOARD — a balanced grid of the panes an operator watches, every slot open: conditions down
  // the left (Space Wx, Band Advisor), activity down the right (Chase, Getting Out), and what is
  // coming along the bottom (Openings, Band Outlook, Greyline), where Band Outlook's table gets
  // the width it needs. 400 px rails make the map one tile among the seven rather than the screen
  // (960×504 at 1920×1080), and fit Chase's rows and Getting Out's compass without wrapping.
  dashboard: {
    slots: {
      left1: 'spacewx',
      left2: 'bandAdvisor',
      right1: 'chase',
      right2: 'getout',
      bottom1: 'openings',
      bottom2: 'outlook',
      bottom3: 'greyline',
    },
    hidden: [],
    rails: { left: 400, right: 400 },
  },
  // FRAME — the wall-display layout for the dashboard window: two boxes down each side of a map
  // that runs the full height, the arrangement a station keeps on a screen of its own. Its columns
  // top to bottom: band conditions over the solar numbers on the left, who is hearing you (PSK
  // Reporter's side of it) over what to chase on the right. The bottom row is closed so the map
  // takes the height; the default's other panes wait in it. 400 px columns read across a desk; in
  // the dashboard window's 1600×1000 they leave the map 728×866 (measured in Chrome), and on a
  // 1024 window they narrow to fit around the map's 280 px floor. And the satellites on the map: the
  // one layer a layout turns on (the header's exception), so the wall display shows the birds move.
  frame: {
    slots: {
      left1: 'bandAdvisor',
      left2: 'spacewx',
      right1: 'getout',
      right2: 'chase',
      bottom1: 'openings',
      bottom2: 'outlook',
      bottom3: 'advisory',
    },
    hidden: STRIP,
    rails: { left: 400, right: 400 },
    mapLayers: ['sats'],
  },
}

/**
 * Why `layout` could not apply as written — empty when it can. A preset naming a pane this build
 * does not have, or placing one twice, would be silently repaired by coercePlacement on apply, so
 * the screen would show something else and the picker would read Custom straight after the tap.
 * This refuses it instead (connectPresets.test.ts runs it over every preset).
 */
export function validateConnectLayout(id: string, layout: ConnectLayout): string[] {
  const errs: string[] = []
  const where = new Map<string, SlotId[]>()
  for (const s of SLOT_IDS) {
    const p = layout.slots[s] as string | undefined
    if (p === undefined) {
      errs.push(`${id}: ${s} has no pane`)
      continue
    }
    if (!(PANE_IDS as readonly string[]).includes(p)) errs.push(`${id}: ${s} names '${p}', which is not a Connect pane`)
    where.set(p, [...(where.get(p) ?? []), s])
  }
  for (const [p, slots] of where)
    if (slots.length > 1) errs.push(`${id}: '${p}' is placed twice (${slots.join(', ')}) — the grid is a permutation`)
  for (const s of layout.hidden)
    if (!(SLOT_IDS as readonly string[]).includes(s)) errs.push(`${id}: hides '${s}', which is not a Connect slot`)
  for (const side of ['left', 'right'] as const) {
    const px = layout.rails[side]
    if (px !== null && !(Number.isInteger(px) && px >= RAIL_MIN && px <= RAIL_MAX))
      errs.push(`${id}: the ${side} rail's ${px} px is outside ${RAIL_MIN}–${RAIL_MAX} px`)
  }
  for (const layer of layout.mapLayers ?? [])
    if (!(PRESET_MAP_LAYERS as readonly string[]).includes(layer))
      errs.push(`${id}: turns on '${layer}', which is not a map layer a layout may turn on`)
  return errs
}

/** The panel record a layout writes: its closed slots removed, every split even. */
export function layoutPanels(layout: ConnectLayout): PanelLayout<SlotId> {
  const state: PanelLayout<SlotId>['state'] = {}
  for (const s of layout.hidden) state[s] = 'removed'
  return { v: 1, state, share: {} }
}

/** An even split, allowing for the float a seam's arithmetic can leave behind. */
const even = (share: number | undefined) => Math.abs((share ?? 1) - 1) < 1e-6

function matches(layout: ConnectLayout, now: ConnectLayoutState): boolean {
  if (SLOT_IDS.some((s) => (now.tabs?.[s]?.length ?? 0) > 1)) return false
  for (const s of SLOT_IDS) {
    if (now.slots[s] !== layout.slots[s]) return false
    if ((now.panels.state[s] === 'removed') !== layout.hidden.includes(s)) return false
    if (!even(now.panels.share[s])) return false
  }
  return now.rails.left === layout.rails.left && now.rails.right === layout.rails.right
}

/** Which layout the screen shows: 'standard' (the default), a preset, or 'custom' — anything the
 *  operator has arranged themselves, including a preset they have since moved or resized. */
export function connectLayoutNow(now: ConnectLayoutState): ConnectPresetId | 'standard' | 'custom' {
  if (matches(STANDARD_LAYOUT, now)) return 'standard'
  return CONNECT_PRESET_IDS.find((id) => matches(CONNECT_PRESETS[id], now)) ?? 'custom'
}
