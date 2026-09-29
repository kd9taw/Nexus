import { describe, it, expect } from 'vitest'
import {
  CONNECT_PRESET_IDS,
  CONNECT_PRESETS,
  STANDARD_LAYOUT,
  connectLayoutNow,
  layoutPanels,
  validateConnectLayout,
  type ConnectLayout,
  type ConnectLayoutState,
} from './connectPresets'
import { DEFAULT_SLOTS, PANE_IDS, SLOT_IDS, type PaneId, type SlotId } from './connectConfig'
import { assignIn } from './paneLayout'
import { RAIL_MAX, RAIL_MIN, RAIL_STEP } from './connectRails'

// CONNECT LAYOUT PRESETS — pure half. What a preset WRITES (placement, visibility, splits, rail
// widths) must read back as that preset, any manual change must read as Custom, and a preset
// that could not apply as written must be refused before it ships. ConnectView.panes.test.tsx
// drives the same presets through the real view.

/** Exactly what applying `layout` leaves behind: the slots, the panel record, the rail prefs. */
const applied = (layout: ConnectLayout): ConnectLayoutState => ({
  slots: { ...layout.slots },
  panels: layoutPanels(layout),
  rails: { ...layout.rails },
})

const VOCAB = { slotIds: SLOT_IDS, paneIds: PANE_IDS, defaults: DEFAULT_SLOTS }

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
  })

  it('the Standard layout IS the operator-approved DEFAULT_SLOTS, unchanged, and no preset touches it', () => {
    // Pinned by value: this is the approved first-run layout (connectConfig.ts — "don't change
    // without asking"). A preset is additive; it may never be edited into the default.
    expect(DEFAULT_SLOTS).toEqual({
      left1: 'advisory',
      left2: 'bandAdvisor',
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

// FRAME — the wall-display layout for the dashboard window: two boxes down each side of a map that
// runs the full height, the arrangement a station keeps on a screen of its own beside the radio.
describe('Frame — the wall-display layout', () => {
  it('is the fourth choice, after the three that shipped, so no stored pick is renumbered', () => {
    expect(CONNECT_PRESET_IDS).toEqual(['mapFirst', 'listFirst', 'dashboard', 'frame'])
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
      const unplaced = PANE_IDS.find((p) => !Object.values(base.slots).includes(p))!
      const changes: Array<[string, ConnectLayoutState]> = [
        ['a new pane picked into a slot', { ...base, slots: assignIn(VOCAB, base.slots, 'left1', unplaced) }],
        ['two placed panes swapped', { ...base, slots: assignIn(VOCAB, base.slots, 'left1', base.slots.right1) }],
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
