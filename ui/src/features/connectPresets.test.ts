import { describe, it, expect } from 'vitest'
import {
  CONNECT_PRESET_IDS,
  CONNECT_PRESETS,
  STANDARD_LAYOUT,
  TV_FRAME_BAR,
  TV_PRESETS,
  connectLayoutNow,
  layoutOf,
  layoutPanels,
  validateConnectLayout,
  type ConnectLayout,
  type ConnectLayoutState,
  type PresetMapLayer,
} from './connectPresets'
import { DEFAULT_SLOTS, PANE_IDS, SLOT_IDS, assignBox, slotBoxes, type PaneId, type SlotId } from './connectConfig'
import { RAIL_MAX, RAIL_MIN, RAIL_STEP } from './connectRails'

// CONNECT LAYOUT PRESETS — pure half. What a preset WRITES (placement, visibility, splits, rail
// widths) must read back as that preset, any manual change must read as Custom, and a preset
// that could not apply as written must be refused before it ships. ConnectView.panes.test.tsx
// drives the same presets through the real view.

/** Exactly what applying `layout` leaves behind: the slots and their tabs, the panel record, the rail
 *  prefs, and whether the bar is on. */
const applied = (layout: ConnectLayout): ConnectLayoutState => ({
  slots: { ...layout.slots },
  tabs: Object.fromEntries(Object.entries(layout.tabs ?? {}).map(([s, l]) => [s, [...l]])),
  panels: layoutPanels(layout),
  rails: { ...layout.rails },
  bar: !!layout.bar,
})
/** The picker's own move (connectConfig assignBox: with tabs, the tab list follows the shown pane). */
const picked = (now: ConnectLayoutState, slot: SlotId, id: PaneId): ConnectLayoutState => ({
  ...now,
  ...assignBox({ slots: { ...now.slots }, tabs: { ...(now.tabs as Partial<Record<SlotId, PaneId[]>>) } }, slot, id),
})

describe('the presets are well formed', () => {
  for (const id of CONNECT_PRESET_IDS) {
    it(`${id} validates: every slot filled from real panes, no pane twice, real slots hidden, widths in range`, () => {
      expect(validateConnectLayout(id, CONNECT_PRESETS[id])).toEqual([])
    })
  }

  it('POSITIVE CONTROL — a preset naming a pane that does not exist is refused', () => {
    const p = CONNECT_PRESETS.listFirst
    const bad: ConnectLayout = { ...p, slots: { ...p.slots, left1: 'bogusPane' as PaneId } }
    expect(validateConnectLayout('listFirst', bad)).toEqual([
      "listFirst: left1 names 'bogusPane', which is not a Connect pane",
    ])
  })

  it('POSITIVE CONTROL — every other way a preset can be malformed is refused too', () => {
    const p = CONNECT_PRESETS.dashboard
    const twice: ConnectLayout = { ...p, slots: { ...p.slots, left2: p.slots.left1 } }
    expect(validateConnectLayout('dashboard', twice)).toEqual([
      `dashboard: '${p.slots.left1}' is placed twice (left1, left2) — the grid is a permutation`,
    ])
    const { bottom3: _dropped, ...missing } = p.slots
    expect(validateConnectLayout('dashboard', { ...p, slots: missing as Record<SlotId, PaneId> })).toEqual([
      'dashboard: bottom3 has no pane',
    ])
    expect(validateConnectLayout('dashboard', { ...p, hidden: ['middle' as SlotId] })).toEqual([
      "dashboard: hides 'middle', which is not a Connect slot",
    ])
    expect(validateConnectLayout('dashboard', { ...p, rails: { left: RAIL_MAX + 1, right: RAIL_MIN - 1 } })).toEqual([
      `dashboard: the left rail's ${RAIL_MAX + 1} px is outside ${RAIL_MIN}–${RAIL_MAX} px`,
      `dashboard: the right rail's ${RAIL_MIN - 1} px is outside ${RAIL_MIN}–${RAIL_MAX} px`,
    ])
    expect(validateConnectLayout('dashboard', { ...p, mapLayers: ['heat' as PresetMapLayer] })).toEqual([
      "dashboard: turns on 'heat', which is not a map layer a layout may turn on",
    ])
  })

  it('the Standard layout IS the operator-approved DEFAULT_SLOTS, unchanged, and no preset touches it', () => {
    // Pinned by value: this is the approved first-run layout (connectConfig.ts — "don't change
    // without asking"). A preset is additive; it may never be edited into the default. Asked and
    // answered 2026-09-28: "Bands for you" takes the Band Advisor's slot ("In the default slot").
    expect(DEFAULT_SLOTS).toEqual({
      left1: 'advisory',
      left2: 'bandTiles',
      right1: 'chase',
      right2: 'outlook',
      bottom1: 'openings',
      bottom2: 'spacewx',
      bottom3: 'getout',
    })
    expect(STANDARD_LAYOUT).toEqual({ slots: DEFAULT_SLOTS, hidden: [], rails: { left: null, right: null } })
    expect(validateConnectLayout('standard', STANDARD_LAYOUT)).toEqual([])
  })

  it('no two layouts are the same arrangement — the picker could not tell them apart', () => {
    const all: Array<[string, ConnectLayout]> = [['standard', STANDARD_LAYOUT], ...CONNECT_PRESET_IDS.map((id) => [id, CONNECT_PRESETS[id]] as [string, ConnectLayout])]
    for (const [a, la] of all)
      for (const [b, lb] of all) {
        if (a >= b) continue
        expect(JSON.stringify(applied(la)), `${a} and ${b}`).not.toBe(JSON.stringify(applied(lb)))
      }
  })
})

// FRAME + BAR — the default view to try (the operator's batch 60, 2026-10-01: "A: Frame + bar"; release
// step 4: a layout to pick, NOT yet the default). The default-view survey's candidate A as the
// side-by-side renders measured it: Frame's shape with Bands for you over Openings on the left and Chase
// over Getting Out on the right, the clock-and-indices bar across the top, and the tab plan behind every
// slot (the operator's Q9: "Ship the tab plan").
describe('Frame + bar — the default view to try', () => {
  const A = () => CONNECT_PRESETS.frameBar

  it('is the fifth choice, after the four that shipped, so no stored pick is renumbered', () => {
    expect(CONNECT_PRESET_IDS).toEqual(['mapFirst', 'listFirst', 'dashboard', 'frame', 'frameBar'])
  })

  it('Bands for you over Openings on the left, Chase over Getting Out on the right, the bottom row closed, 400 px columns', () => {
    expect([A().slots.left1, A().slots.left2]).toEqual(['bandTiles', 'openings'])
    expect([A().slots.right1, A().slots.right2]).toEqual(['chase', 'getout'])
    expect([...A().hidden].sort()).toEqual(['bottom1', 'bottom2', 'bottom3'])
    expect(A().rails).toEqual({ left: 400, right: 400 })
  })

  it('the clock-and-indices bar across the top, and nothing about the map (the Propagation card stays as it is)', () => {
    expect(A().bar).toBe(true)
    expect(A().mapLayers).toBeUndefined()
    for (const id of CONNECT_PRESET_IDS.filter((x) => x !== 'frameBar')) expect(CONNECT_PRESETS[id].bar, id).toBeFalsy()
    expect(STANDARD_LAYOUT.bar).toBeFalsy()
  })

  it('the tab plan: every box Connect keeps is on screen or behind one tab, the shown one first', () => {
    expect(A().tabs).toEqual({
      left1: ['bandTiles', 'bandAdvisor', 'bestband', 'activity', 'advisory'],
      left2: ['openings', 'esNowcast', 'openingsLog', 'insights', 'bandHours'],
      right1: ['chase', 'chaseFeed', 'selection', 'contests', 'satPasses'],
      right2: ['getout', 'spacewx', 'kpOutlook', 'measuredMuf', 'beacons', 'greyline'],
      bottom1: ['outlook', 'clock'],
      bottom2: ['rotor', 'amp'],
    })
    expect(A().slots.bottom3).toBe('scope')
    // Placed, shown or behind a tab: all but the three boards, which are whole screens of their own.
    const placed = SLOT_IDS.flatMap((s) => slotBoxes({ slots: A().slots, tabs: A().tabs as Partial<Record<SlotId, PaneId[]>> }, s))
    expect(PANE_IDS.filter((p) => !placed.includes(p))).toEqual(['spots', 'pota', 'needed'])
  })

  it('reads back as itself whichever tab a slot shows: showing a tab is not an arrangement change', () => {
    const base = applied(A())
    expect(connectLayoutNow(base), 'control: as written').toBe('frameBar')
    expect(connectLayoutNow({ ...base, slots: { ...base.slots, left1: 'advisory', right2: 'greyline' } })).toBe('frameBar')
  })

  it('a tab added, a tab taken out, or the tabs reordered reads Custom', () => {
    const base = applied(A())
    const tabs = base.tabs as Partial<Record<SlotId, PaneId[]>>
    const variants: Array<[string, Partial<Record<SlotId, PaneId[]>>]> = [
      ['a tab added', { ...tabs, bottom3: ['scope', 'spots'] }],
      ['a tab taken out', { ...tabs, left2: tabs.left2!.slice(0, -1) }],
      ['the tabs reordered', { ...tabs, right1: [...tabs.right1!].reverse() }],
    ]
    for (const [what, t] of variants) expect(connectLayoutNow({ ...base, tabs: t }), what).toBe('custom')
  })

  it('the stock arrangement with the bar on, or a tabbed slot under a one-pane layout, is nobody’s layout: Custom', () => {
    expect(connectLayoutNow({ ...applied(STANDARD_LAYOUT), bar: true })).toBe('custom')
    const frame = applied(CONNECT_PRESETS.frame)
    expect(connectLayoutNow({ ...frame, tabs: { left1: ['bandAdvisor', 'clock'] } })).toBe('custom')
  })

  it('POSITIVE CONTROL — a tab plan that breaks the placement rule is refused', () => {
    const p = A()
    const tabs = p.tabs as Partial<Record<SlotId, PaneId[]>>
    expect(validateConnectLayout('frameBar', { ...p, tabs: { ...tabs, bottom1: ['clock', 'scope'] } })).toEqual([
      "frameBar: bottom1's tabs leave out 'outlook', the pane the slot shows",
      "frameBar: 'scope' is placed twice (bottom3, bottom1) — the grid is a permutation",
    ])
    expect(validateConnectLayout('frameBar', { ...p, tabs: { ...tabs, bottom3: ['scope'] } })).toEqual([
      "frameBar: bottom3 lists one tab — a slot of one pane lists none",
    ])
    expect(validateConnectLayout('frameBar', { ...p, tabs: { ...tabs, bottom3: ['scope', 'bogus' as PaneId] } })).toEqual([
      "frameBar: bottom3's tabs name 'bogus', which is not a Connect pane",
    ])
  })
})

// FRAME — the wall-display layout for the dashboard window: two boxes down each side of a map that
// runs the full height, the arrangement a station keeps on a screen of its own beside the radio.
describe('Frame — the wall-display layout', () => {
  it('is the fourth choice, after the three that shipped, so no stored pick is renumbered', () => {
    expect(CONNECT_PRESET_IDS.slice(0, 4)).toEqual(['mapFirst', 'listFirst', 'dashboard', 'frame'])
  })

  it('two panes down each side, and the bottom row closed, so the map runs the full height', () => {
    const p = CONNECT_PRESETS.frame
    expect([...p.hidden].sort()).toEqual(['bottom1', 'bottom2', 'bottom3'])
    expect(SLOT_IDS.filter((s) => !p.hidden.includes(s))).toEqual(['left1', 'left2', 'right1', 'right2'])
  })

  it('its columns, top to bottom: band conditions over space weather on the left, who hears you over what to chase on the right', () => {
    const p = CONNECT_PRESETS.frame
    expect([p.slots.left1, p.slots.left2]).toEqual(['bandAdvisor', 'spacewx'])
    expect([p.slots.right1, p.slots.right2]).toEqual(['getout', 'chase'])
  })

  it('400 px columns, wide enough to read a box from across the desk', () => {
    expect(CONNECT_PRESETS.frame.rails).toEqual({ left: 400, right: 400 })
  })

  it('turns the satellites on (the operator’s pick), and it is the only layout that reaches into the map', () => {
    expect(CONNECT_PRESETS.frame.mapLayers).toEqual(['sats'])
    for (const id of CONNECT_PRESET_IDS.filter((x) => x !== 'frame')) expect(CONNECT_PRESETS[id].mapLayers, id).toBeUndefined()
    expect((STANDARD_LAYOUT as ConnectLayout).mapLayers).toBeUndefined()
  })
})

describe('which layout is on screen', () => {
  it('nothing picked: the stock arrangement reads as Standard', () => {
    expect(connectLayoutNow({ slots: { ...DEFAULT_SLOTS }, panels: { v: 1, state: {}, share: {} }, rails: { left: null, right: null } })).toBe('standard')
    // A pane closed and ticked back is stored as an explicit 'docked' — still the stock layout.
    expect(connectLayoutNow({ slots: { ...DEFAULT_SLOTS }, panels: { v: 1, state: { left1: 'docked' }, share: {} }, rails: { left: null, right: null } })).toBe('standard')
  })

  for (const id of CONNECT_PRESET_IDS) {
    it(`${id} round-trips: what applying it writes reads back as ${id}`, () => {
      expect(connectLayoutNow(applied(CONNECT_PRESETS[id]))).toBe(id)
    })

    it(`${id}: a moved or resized pane reads as Custom`, () => {
      const base = applied(CONNECT_PRESETS[id])
      // Control: unchanged, it reads as itself — so a Custom below is the change, not a matcher
      // that answers Custom to everything.
      expect(connectLayoutNow(base), 'control: the untouched preset').toBe(id)
      const shown = SLOT_IDS.filter((s) => !CONNECT_PRESETS[id].hidden.includes(s))
      const placedAnywhere = SLOT_IDS.flatMap((s) => slotBoxes({ slots: base.slots, tabs: (base.tabs ?? {}) as Partial<Record<SlotId, PaneId[]>> }, s))
      const unplaced = PANE_IDS.find((p) => !placedAnywhere.includes(p))!
      const changes: Array<[string, ConnectLayoutState]> = [
        ['a new pane picked into a slot', picked(base, 'left1', unplaced)],
        ['two placed panes swapped', picked(base, 'left1', base.slots.right1)],
        [base.bar ? 'the bar turned off' : 'the bar turned on', { ...base, bar: !base.bar }],
        ['a shown pane closed', { ...base, panels: { ...base.panels, state: { ...base.panels.state, [shown[0]]: 'removed' } } }],
        ['a split moved', { ...base, panels: { ...base.panels, share: { left1: 1.1, left2: 0.9 } } }],
        ['the left rail widened', { ...base, rails: { ...base.rails, left: (base.rails.left ?? 300) + RAIL_STEP } }],
        ['the right rail narrowed', { ...base, rails: { ...base.rails, right: (base.rails.right ?? 300) - RAIL_STEP } }],
      ]
      const hiddenSlot = CONNECT_PRESETS[id].hidden[0]
      if (hiddenSlot) changes.push(['a closed pane opened', { ...base, panels: { ...base.panels, state: { ...base.panels.state, [hiddenSlot]: 'docked' } } }])
      for (const [what, state] of changes) expect(connectLayoutNow(state), what).toBe('custom')
    })
  }

  it('a rail the window squeezed is not a change: the STORED width decides, not the fitted one', () => {
    // fitRails narrows a wide preset on a small window without rewriting the preference, so a
    // 1024 px reload of List first must still read as List first (the clamp-on-load half of this
    // lives in ConnectView.panes.test.tsx, against the real view).
    const base = applied(CONNECT_PRESETS.listFirst)
    expect(connectLayoutNow({ ...base, rails: { ...base.rails, last: 'left' } })).toBe('listFirst')
  })
})

// THE TV PAGE'S FRAME + BAR and THE KEPT LAYOUT (step 5: the one-time switch to Frame + bar, which keeps the
// operator's own arrangement for one tap back). ConnectView.switch.test.tsx drives both through the real view.
describe('the TV page’s Frame + bar: A without the boxes the page can never fill', () => {
  const A = CONNECT_PRESETS.frameBar
  // What the TV page is never served (tempo-app connect_web.rs RPC_ALLOWLIST), so the boxes that can never
  // fill there: the needs board (Chase, Chase Feed), a click-through (Selection), the contest calendar
  // (get_contests), the station's devices (rotor, amplifier, band scope), and the three boards the page lends
  // none of (tv/ConnectTv passes no spotsFeed, no otaBoard and no neededBoard).
  const NEVER_FILLS: readonly PaneId[] = ['chase', 'chaseFeed', 'selection', 'contests', 'rotor', 'amp', 'scope', 'spots', 'pota', 'needed']
  const placed = (l: ConnectLayout) =>
    SLOT_IDS.flatMap((s) => slotBoxes({ slots: { ...l.slots }, tabs: { ...(l.tabs as Partial<Record<SlotId, PaneId[]>>) } }, s))

  it('validates like every preset, and is the TV table’s Frame + bar; its other layouts are the app’s', () => {
    expect(validateConnectLayout('tvFrameBar', TV_FRAME_BAR)).toEqual([])
    expect(TV_PRESETS.frameBar).toBe(TV_FRAME_BAR)
    for (const id of CONNECT_PRESET_IDS.filter((x) => x !== 'frameBar')) expect(TV_PRESETS[id], id).toBe(CONNECT_PRESETS[id])
  })

  it('places no box the page can never fill: none on screen, none behind a tab, none in the closed row', () => {
    expect(placed(TV_FRAME_BAR).filter((p) => NEVER_FILLS.includes(p))).toEqual([])
  })

  it('POSITIVE CONTROL — the app’s Frame + bar places seven of them, so the check above can see one', () => {
    expect(placed(A).filter((p) => NEVER_FILLS.includes(p)).sort()).toEqual(
      ['amp', 'chase', 'chaseFeed', 'contests', 'rotor', 'scope', 'selection'].sort(),
    )
  })

  it('is A’s shape: A’s left column, Getting Out where A has it, the bottom row closed, 400 px columns, the bar', () => {
    for (const s of ['left1', 'left2'] as const) {
      expect(TV_FRAME_BAR.slots[s], s).toBe(A.slots[s])
      expect(TV_FRAME_BAR.tabs?.[s], s).toEqual(A.tabs?.[s])
    }
    expect(TV_FRAME_BAR.slots.right2).toBe(A.slots.right2)
    expect(TV_FRAME_BAR.hidden).toEqual(A.hidden)
    expect(TV_FRAME_BAR.rails).toEqual(A.rails)
    expect(TV_FRAME_BAR.bar).toBe(true)
  })

  it('reads back as Frame + bar on the TV, and as Custom against the app’s table', () => {
    expect(connectLayoutNow(applied(TV_FRAME_BAR), TV_PRESETS)).toBe('frameBar')
    expect(connectLayoutNow(applied(TV_FRAME_BAR))).toBe('custom')
  })

  it('neither default reaches into the map, so the one-time switch never touches it', () => {
    expect(A.mapLayers).toBeUndefined()
    expect(TV_FRAME_BAR.mapLayers).toBeUndefined()
  })
})

describe('the kept layout: what the one-time switch keeps, and how it reads back', () => {
  // An operator's own arrangement: two boxes swapped, a tab, a closed box, an uneven split, dragged widths.
  const own: ConnectLayoutState = {
    slots: { ...DEFAULT_SLOTS, left1: 'spacewx', bottom2: 'advisory' },
    tabs: { right2: ['outlook', 'clock'] },
    panels: { v: 1, state: { bottom3: 'removed' }, share: { left1: 1.4, left2: 0.6 } },
    rails: { left: 304, right: 512 },
    bar: false,
  }

  it('is the arrangement on screen as a layout: a tap on it writes back exactly that, rotation included', () => {
    expect(applied(layoutOf(own))).toEqual(own)
    expect(layoutOf(own, { right2: 30 }).rotate).toEqual({ right2: 30 })
    expect(validateConnectLayout('kept', layoutOf(own))).toEqual([])
  })

  it('reads back as the kept layout while nothing has moved, and as Custom once anything does', () => {
    const kept = layoutOf(own)
    expect(connectLayoutNow(own, CONNECT_PRESETS, kept)).toBe('kept')
    // Without it the same screen is Custom: the reason the switch keeps it.
    expect(connectLayoutNow(own)).toBe('custom')
    expect(connectLayoutNow({ ...own, panels: { ...own.panels, share: {} } }, CONNECT_PRESETS, kept)).toBe('custom')
    expect(connectLayoutNow({ ...own, rails: { left: 304, right: 520 } }, CONNECT_PRESETS, kept)).toBe('custom')
    expect(connectLayoutNow(picked(own, 'left2', 'greyline'), CONNECT_PRESETS, kept)).toBe('custom')
  })

  it('Standard or a preset on screen reads as itself, with a kept layout or without', () => {
    const kept = layoutOf(own)
    expect(connectLayoutNow(applied(STANDARD_LAYOUT), CONNECT_PRESETS, kept)).toBe('standard')
    expect(connectLayoutNow(applied(CONNECT_PRESETS.frameBar), CONNECT_PRESETS, kept)).toBe('frameBar')
  })
})
