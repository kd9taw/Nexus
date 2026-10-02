// Connect layout presets — Map first, List first and Dashboard (the UI look-and-feel redesign,
// 2026-09-26), Frame (the dashboard window's wall-display layout), and Frame + bar (the default view
// to try, 2026-10-01). Pure (no JSX, no storage), so every rule unit-tests without React.
//
// A preset is a WHOLE ARRANGEMENT over the machinery Connect already has, and nothing else:
//   · which pane sits in each slot (features/connectConfig — the permutation grid);
//   · which slots are closed and how each rail is split (features/panelState CONNECT_PANELS);
//   · each rail's width preference (features/connectRails, clamped on load against the window);
//   · whether the clock-and-indices bar runs across the top of the view (ConnectView, per surface).
// It stores nothing of its own. Which layout is on screen is READ BACK from those three records
// (`connectLayoutNow`), so the picker can never disagree with the screen: move or resize anything
// after picking one and it reads Custom, and nothing snaps back, because nothing remembers the
// pick to snap back to.
//
// FRAME + BAR IS THE DEFAULT, ONCE (step 5, the operator's "Everyone, once", 2026-10-01). Every surface
// opens in it once after the update, a fresh install included, through ONE switch that runs once per
// surface and never again (ConnectView `switchToDefaultOnce`). It keeps the arrangement it replaced
// when that was the operator's own (`layoutOf`), and the picker offers it as "Your earlier layout", so
// one tap brings back exactly what they had. Standard (DEFAULT_SLOTS; `STANDARD_LAYOUT` is it, by
// reference) stays in the picker and is still what ⊞ Reset layout gives. Apart from that switch, a tap
// is the only way a layout applies: no intent switch applies one.
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

export const CONNECT_PRESET_IDS = ['mapFirst', 'listFirst', 'dashboard', 'frame', 'frameBar'] as const
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
  /** The slots that hold tabs: each one's panes in tab order, the shown pane (`slots`) first. Absent is
   *  one pane per slot, which is every layout but Frame + bar. Written and read back like the slots. */
  tabs?: Readonly<Partial<Record<SlotId, readonly PaneId[]>>>
  /** The clock-and-indices bar (DashboardBar) across the top of the view. Absent is off. A tap writes
   *  it either way and it reads back, like the closed slots. Where the host draws the bar itself (the
   *  dashboard window, the TV page) the record is kept but the view draws no second one. */
  bar?: boolean
  /** The splits of the panel record. Absent is even, which is every preset; only the kept layout
   *  carries any. Written and read back like the closed slots. */
  share?: Readonly<Partial<Record<SlotId, number>>>
  /** The tabs' rotation (connectConfig `rotate`). Absent is none; only the kept layout carries any.
   *  Written by a tap, not read back: an interval is no arrangement. */
  rotate?: Readonly<Partial<Record<SlotId, number>>>
}

/** What the screen is built from right now — the three records a layout writes. */
export interface ConnectLayoutState {
  slots: Readonly<Record<SlotId, PaneId>>
  /** The slots holding more than one pane (connectConfig `tabs`). A layout without a tab plan is
   *  one pane per slot, so a screen with tabs it does not list is the operator's own arrangement: it
   *  reads Custom, and the layout's tap puts its own tabs (or one pane per slot) back. Absent is none. */
  tabs?: Readonly<Partial<Record<SlotId, readonly PaneId[]>>>
  panels: PanelLayout<SlotId>
  /** The STORED preferences, not the widths the window fitted them to: a list-first rail
   *  squeezed by a small window is still list-first, and reloading on a big one gives it back. */
  rails: RailWidths
  /** The bar is on (this surface's record). Absent is off. */
  bar?: boolean
}

/** The operator-approved default, what ⊞ Reset layout restores, and what the picker's Standard applies
 *  (ConnectView). It is DEFAULT_SLOTS itself, not a copy, so it cannot drift from it. */
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
  // FRAME + BAR — THE DEFAULT (the operator's batch 60, 2026-10-01: "A: Frame + bar"; the default since
  // step 5, "Frame + bar (default)" in the picker). The default-view survey's candidate A, as the side-by-side
  // renders measured it: Frame's shape with the boxes an operator reads first — Bands for you over
  // Openings on the left, Chase over Getting Out on the right — in 400 px columns, where the renders
  // found every band tile and every Chase row whole at 1024, 1366 and 1920 wide. The bar across the top
  // carries the clocks, the call and grid and the day's indices (so Space Wx's numbers stay in view with
  // its box behind a tab), and THE TAB PLAN (the operator: "Ship the tab plan") puts the rest of
  // Connect's own boxes one click behind the four: the band boxes behind Bands for you, the opening
  // boxes behind Openings, the activity boxes behind Chase, the conditions boxes behind Getting Out.
  // The closed row keeps Band Outlook and the Clock, the rotor and the amplifier, and the band scope,
  // for ⊞ Panels to bring back. The map is not this layout's: its card, its layers and its look stay as
  // the operator has them.
  frameBar: {
    slots: {
      left1: 'bandTiles',
      left2: 'openings',
      right1: 'chase',
      right2: 'getout',
      bottom1: 'outlook',
      bottom2: 'rotor',
      bottom3: 'scope',
    },
    tabs: {
      left1: ['bandTiles', 'bandAdvisor', 'bestband', 'activity', 'advisory'],
      left2: ['openings', 'esNowcast', 'openingsLog', 'insights', 'bandHours'],
      right1: ['chase', 'chaseFeed', 'selection', 'contests', 'satPasses'],
      right2: ['getout', 'spacewx', 'kpOutlook', 'measuredMuf', 'beacons', 'greyline'],
      bottom1: ['outlook', 'clock'],
      bottom2: ['rotor', 'amp'],
    },
    hidden: STRIP,
    rails: { left: 400, right: 400 },
    bar: true,
  },
}

/**
 * THE TV PAGE'S FRAME + BAR (step 5: "it gets a TV version of A without Chase, under the TV's
 * public-data-only rule"). The page is served public data only (tempo-app connect_web.rs
 * `RPC_ALLOWLIST`): no needs board, no click-through, no contest calendar, no station devices. So it is
 * A with every box the page can never fill taken out of the plan — Chase, Chase Feed, Selection and
 * Contests from Chase's tabs, the rotor, the amplifier and the band scope from the closed row. Chase's
 * place goes to Space Wx with the K outlook (from Getting Out's tabs), and Getting Out keeps its own
 * place with the rest of its tabs. The left column is A's. The closed row holds Band Outlook, Satellite
 * Passes and the Clock, so ⊞ Panels can only bring back a box that fills on a wall. The page draws the
 * bar itself; the record says on, as A's does, so the layout reads back as Frame + bar there.
 */
export const TV_FRAME_BAR: ConnectLayout = {
  slots: {
    left1: 'bandTiles',
    left2: 'openings',
    right1: 'spacewx',
    right2: 'getout',
    bottom1: 'outlook',
    bottom2: 'satPasses',
    bottom3: 'clock',
  },
  tabs: {
    left1: ['bandTiles', 'bandAdvisor', 'bestband', 'activity', 'advisory'],
    left2: ['openings', 'esNowcast', 'openingsLog', 'insights', 'bandHours'],
    right1: ['spacewx', 'kpOutlook'],
    right2: ['getout', 'measuredMuf', 'beacons', 'greyline'],
  },
  hidden: STRIP,
  rails: { left: 400, right: 400 },
  bar: true,
}

/** The TV page's layout table (tv/ConnectTv): the app's, with its own Frame + bar. */
export const TV_PRESETS: Readonly<Record<ConnectPresetId, ConnectLayout>> = { ...CONNECT_PRESETS, frameBar: TV_FRAME_BAR }

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
  // The tab rules (connectConfig): a slot's list holds the pane it shows and more, real panes only,
  // and a pane behind a tab is in no other slot — coerceTabs would quietly repair anything else.
  for (const s of SLOT_IDS) {
    const list = layout.tabs?.[s] as readonly string[] | undefined
    if (!list) continue
    if (list.length < 2) errs.push(`${id}: ${s} lists one tab — a slot of one pane lists none`)
    if (!list.includes(layout.slots[s])) errs.push(`${id}: ${s}'s tabs leave out '${layout.slots[s]}', the pane the slot shows`)
    for (const p of list) {
      if (!(PANE_IDS as readonly string[]).includes(p)) errs.push(`${id}: ${s}'s tabs name '${p}', which is not a Connect pane`)
      else if (p !== layout.slots[s]) where.set(p, [...(where.get(p) ?? []), s])
    }
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

/** The panel record a layout writes: its closed slots removed, its splits (every one even but the kept
 *  layout's). */
export function layoutPanels(layout: ConnectLayout): PanelLayout<SlotId> {
  const state: PanelLayout<SlotId>['state'] = {}
  for (const s of layout.hidden) state[s] = 'removed'
  return { v: 1, state, share: { ...layout.share } }
}

/** The same split, allowing for the float a seam's arithmetic can leave behind. Absent is even. */
const sameShare = (a: number | undefined, b: number | undefined) => Math.abs((a ?? 1) - (b ?? 1)) < 1e-6

function matches(layout: ConnectLayout, now: ConnectLayoutState): boolean {
  for (const s of SLOT_IDS) {
    const want = layout.tabs?.[s]
    const have = now.tabs?.[s]
    if (want && want.length > 1) {
      // A slot of tabs: the same panes in the same order. Which one is SHOWN is not the arrangement —
      // a tab click is no change of layout, and costs no Undo — as long as it is one of them.
      if (!have || have.length !== want.length || have.some((p, i) => p !== want[i])) return false
      if (!want.includes(now.slots[s])) return false
    } else {
      if ((have?.length ?? 0) > 1) return false
      if (now.slots[s] !== layout.slots[s]) return false
    }
    if ((now.panels.state[s] === 'removed') !== layout.hidden.includes(s)) return false
    if (!sameShare(now.panels.share[s], layout.share?.[s])) return false
  }
  if (!!now.bar !== !!layout.bar) return false
  return now.rails.left === layout.rails.left && now.rails.right === layout.rails.right
}

/** Which layout the screen shows: 'standard', a preset (of this host's table: the TV page's Frame + bar
 *  is its own), 'kept' (the arrangement the one-time switch kept), or 'custom' — anything the operator
 *  has arranged themselves, including a layout they have since moved or resized. */
export function connectLayoutNow(
  now: ConnectLayoutState,
  presets: Readonly<Record<ConnectPresetId, ConnectLayout>> = CONNECT_PRESETS,
  kept?: ConnectLayout | null,
): ConnectPresetId | 'standard' | 'kept' | 'custom' {
  if (matches(STANDARD_LAYOUT, now)) return 'standard'
  const preset = CONNECT_PRESET_IDS.find((id) => matches(presets[id], now))
  if (preset) return preset
  return kept && matches(kept, now) ? 'kept' : 'custom'
}

/** The arrangement on screen as a layout a tap can put back — what the one-time switch keeps when the
 *  screen reads Custom. Everything a layout writes, as it is: the placement with its tabs and their
 *  rotation, the closed slots and the splits, the stored widths and the bar. The panes' text sizes are
 *  not in it; they ride through every layout. */
export function layoutOf(now: ConnectLayoutState, rotate?: Partial<Record<SlotId, number>>): ConnectLayout {
  const tabs: Partial<Record<SlotId, PaneId[]>> = {}
  const share: Partial<Record<SlotId, number>> = {}
  for (const s of SLOT_IDS) {
    const list = now.tabs?.[s]
    if (list && list.length > 1) tabs[s] = [...list]
    const sh = now.panels.share[s]
    if (sh !== undefined && !sameShare(sh, undefined)) share[s] = sh
  }
  return {
    slots: { ...now.slots },
    hidden: SLOT_IDS.filter((s) => now.panels.state[s] === 'removed'),
    rails: { left: now.rails.left, right: now.rails.right },
    ...(Object.keys(tabs).length ? { tabs } : {}),
    ...(Object.keys(share).length ? { share } : {}),
    ...(rotate && Object.keys(rotate).length ? { rotate: { ...rotate } } : {}),
    ...(now.bar ? { bar: true } : {}),
  }
}
