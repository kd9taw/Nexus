// TABS: SEVERAL PANES IN ONE CONNECT SLOT (2026-09-29, the operator's pick: "Tabs (several boxes
// stacked in one slot)"). The placement rule changes from "the grid is a permutation — one pane per
// slot" to "each pane in at most one slot, a slot holds one or more", and everything that already
// exists must read exactly as before:
//   · a stored config with no tabs — every one saved before this — loads to the same slots;
//   · with one pane per slot, picking a pane is today's swap, pair for pair (assignIn);
//   · a layout preset lists its own tabs (only Frame + bar has any), and a tab it does not list makes
//     the screen the operator's own (Custom).
// The pure half, here; ConnectView.boxes.test.tsx drives the same rules through the real view.
import { describe, expect, it } from 'vitest'
import {
  DEFAULT_SLOTS,
  PANE_IDS,
  ROTATE_CHOICES,
  SLOT_IDS,
  addTab,
  addableTo,
  assignBox,
  normalizeConfig,
  removeTab,
  showTab,
  slotBoxes,
  coerceRotate,
  type ConnectConfig,
  type PaneId,
  type SlotId,
} from './connectConfig'
import { assignIn } from './paneLayout'
import { CONNECT_PRESETS, CONNECT_PRESET_IDS, connectLayoutNow, layoutPanels } from './connectPresets'

type Placement = Pick<ConnectConfig, 'slots' | 'tabs'>
const VOCAB = { slotIds: SLOT_IDS, paneIds: PANE_IDS, defaults: DEFAULT_SLOTS }
const at = (slots: Partial<Record<SlotId, PaneId>>, tabs: Partial<Record<SlotId, PaneId[]>> = {}): Placement => ({
  slots: { ...DEFAULT_SLOTS, ...slots },
  tabs,
})

/** Every pane placed anywhere, with the slot holding it. */
function placed(p: Placement): [PaneId, SlotId][] {
  return SLOT_IDS.flatMap((s) => slotBoxes(p, s).map((id): [PaneId, SlotId] => [id, s]))
}
/** The invariant, as one list of broken promises (empty = holds). */
function broken(p: Placement): string[] {
  const out: string[] = []
  const seen = new Map<PaneId, SlotId>()
  for (const [id, s] of placed(p)) {
    if (seen.has(id)) out.push(`${id} is in ${seen.get(id)} and ${s}`)
    seen.set(id, s)
  }
  for (const s of SLOT_IDS) {
    if (!slotBoxes(p, s).includes(p.slots[s])) out.push(`${s} shows ${p.slots[s]}, which is not one of its tabs`)
    const t = p.tabs[s]
    if (t && t.length < 2) out.push(`${s} stores a tab list of ${t.length}`)
  }
  return out
}

describe('a stored layout without tabs loads exactly as before', () => {
  it('no tabs field: the same slots, no tabs, one pane per slot', () => {
    const older = { slots: { ...DEFAULT_SLOTS, left1: 'greyline' }, overlays: { x: true } }
    const c = normalizeConfig(older)
    expect(c.slots).toEqual(older.slots)
    expect(c.tabs).toEqual({})
    for (const s of SLOT_IDS) expect(slotBoxes(c, s), s).toEqual([older.slots[s]])
    expect(c.overlays).toEqual({ x: true })
  })

  it('junk in the tabs field is dropped, never guessed at', () => {
    for (const tabs of [null, 7, 'x', [], { left1: 'spacewx' }, { left1: [] }, { nowhere: ['clock', 'greyline'] }]) {
      const c = normalizeConfig({ slots: DEFAULT_SLOTS, tabs })
      expect(c.tabs, JSON.stringify(tabs)).toEqual({})
      expect(c.slots).toEqual(DEFAULT_SLOTS)
    }
  })
})

describe('normalize keeps a pane in at most one slot, and a slot’s shown pane among its tabs', () => {
  it('keeps a valid tab list', () => {
    const c = normalizeConfig({ slots: DEFAULT_SLOTS, tabs: { left2: ['bandTiles', 'clock', 'greyline'] } })
    expect(c.tabs).toEqual({ left2: ['bandTiles', 'clock', 'greyline'] })
  })

  it('a pane SHOWN in another slot is dropped from a tab list (the shown pane wins)', () => {
    // spacewx is shown in bottom2 by default.
    const c = normalizeConfig({ slots: DEFAULT_SLOTS, tabs: { left2: ['bandTiles', 'spacewx', 'clock'] } })
    expect(c.tabs).toEqual({ left2: ['bandTiles', 'clock'] })
  })

  it('a pane in two slots’ tabs stays in the first slot in grid order', () => {
    const c = normalizeConfig({ slots: DEFAULT_SLOTS, tabs: { left2: ['bandTiles', 'clock'], right2: ['outlook', 'clock', 'greyline'] } })
    expect(c.tabs).toEqual({ left2: ['bandTiles', 'clock'], right2: ['outlook', 'greyline'] })
  })

  it('the shown pane missing from its own list is put first; duplicates and unknown ids go; a list of one goes', () => {
    const c = normalizeConfig({
      slots: DEFAULT_SLOTS,
      tabs: { left1: ['clock', 'clock', 'bogus'], right1: ['chase', 'chase'], bottom1: ['openings', 'openingsLog'] },
    })
    expect(c.tabs).toEqual({ left1: ['advisory', 'clock'], bottom1: ['openings', 'openingsLog'] })
    expect(broken(c)).toEqual([])
  })
})

describe('with one pane per slot, the picker is exactly today’s swap', () => {
  const shuffled = at({ left1: 'clock', left2: 'spacewx', bottom2: 'bandTiles', right2: 'greyline' })
  for (const [label, base] of [['the default layout', at({})], ['a rearranged one', shuffled]] as const) {
    it(`${label}: assignBox === assignIn for every slot and every pane`, () => {
      for (const s of SLOT_IDS)
        for (const p of PANE_IDS) {
          const got = assignBox(base, s, p)
          expect(got.slots, `${s} ← ${p}`).toEqual(assignIn(VOCAB, base.slots, s, p))
          expect(got.tabs, `${s} ← ${p} made tabs`).toEqual({})
        }
    })
  }
})

describe('the picker with tabs', () => {
  const tabbed = at({ left2: 'clock' }, { left2: ['bandTiles', 'clock', 'greyline'] })

  it('replaces the SHOWN tab in its place', () => {
    const got = assignBox(tabbed, 'left2', 'insights')
    expect(got.slots.left2).toBe('insights')
    expect(got.tabs.left2).toEqual(['bandTiles', 'insights', 'greyline'])
    expect(broken(got)).toEqual([])
  })

  it('picking one of the slot’s own tabs shows it (nothing moves)', () => {
    const got = assignBox(tabbed, 'left2', 'greyline')
    expect(got.slots.left2).toBe('greyline')
    expect(got.tabs.left2).toEqual(['bandTiles', 'clock', 'greyline'])
  })

  it('picking a pane shown in another slot swaps them, place for place', () => {
    const got = assignBox(tabbed, 'left2', 'spacewx') // spacewx is shown in bottom2
    expect(got.slots.left2).toBe('spacewx')
    expect(got.tabs.left2).toEqual(['bandTiles', 'spacewx', 'greyline'])
    expect(got.slots.bottom2, 'the displaced tab takes its place').toBe('clock')
    expect(broken(got)).toEqual([])
  })

  it('picking a pane that is a tab BEHIND another slot’s pane: the displaced one becomes a tab there', () => {
    const got = assignBox(tabbed, 'right1', 'greyline')
    expect(got.slots.right1).toBe('greyline')
    expect(got.tabs.left2).toEqual(['bandTiles', 'clock', 'chase'])
    expect(got.slots.left2, 'the other slot still shows what it showed').toBe('clock')
    expect(broken(got)).toEqual([])
  })
})

describe('⋯ ▸ Add a tab', () => {
  it('appends the pane to the slot and shows it', () => {
    const got = addTab(at({}), 'left2', 'clock')!
    expect(got.slots.left2).toBe('clock')
    expect(got.tabs.left2).toEqual(['bandTiles', 'clock'])
    expect(broken(got)).toEqual([])
  })

  it('a pane that is a tab in another slot MOVES here — it is never in two slots', () => {
    const from = at({}, { right2: ['outlook', 'clock'] })
    const got = addTab(from, 'left2', 'clock')!
    expect(got.tabs.left2).toEqual(['bandTiles', 'clock'])
    expect(got.tabs.right2, 'the other slot is back to one pane').toBeUndefined()
    expect(got.slots.right2).toBe('outlook')
    expect(broken(got)).toEqual([])
  })

  it('a pane SHOWN in a slot with other tabs moves here, and that slot shows its next tab', () => {
    const from = at({ right2: 'clock' }, { right2: ['outlook', 'clock', 'greyline'] })
    const got = addTab(from, 'left2', 'clock')!
    expect(got.slots.right2).toBe('greyline')
    expect(got.tabs.right2).toEqual(['outlook', 'greyline'])
    expect(broken(got)).toEqual([])
  })

  it('refuses another slot’s ONLY pane (that slot would be empty) and a pane already here', () => {
    expect(addTab(at({}), 'left2', 'spacewx'), 'bottom2 holds only Space Wx').toBeNull()
    expect(addTab(at({}, { left2: ['bandTiles', 'clock'] }), 'left2', 'clock')).toBeNull()
  })

  it('offers exactly what it accepts: every pane not in the slot, except another slot’s only pane', () => {
    const cfg = at({}, { right2: ['outlook', 'clock'] })
    const offered = addableTo(cfg, 'left2')
    for (const p of PANE_IDS) expect(offered.includes(p), p).toBe(addTab(cfg, 'left2', p) !== null)
    expect(offered).toContain('clock') // a tab elsewhere
    expect(offered).toContain('outlook') // shown elsewhere, with a tab beside it
    expect(offered).not.toContain('spacewx') // bottom2's only pane
    expect(offered).not.toContain('bandTiles') // already here
    expect(offered, 'in the picker’s order').toEqual(PANE_IDS.filter((p) => offered.includes(p)))
  })
})

describe('⋯ ▸ Remove from this slot', () => {
  it('takes the SHOWN tab out and shows the next one, or the one before when it was last', () => {
    const cfg = at({ left2: 'clock' }, { left2: ['bandTiles', 'clock', 'greyline'] })
    const mid = removeTab(cfg, 'left2')!
    expect(mid.tabs.left2).toEqual(['bandTiles', 'greyline'])
    expect(mid.slots.left2).toBe('greyline')
    const last = removeTab(mid, 'left2')!
    expect(last.slots.left2).toBe('bandTiles')
    expect(last.tabs.left2, 'one pane left: no tab list').toBeUndefined()
    expect(broken(last)).toEqual([])
  })

  it('refuses a slot’s only pane — ✕ is what closes a slot', () => {
    expect(removeTab(at({}), 'left2')).toBeNull()
  })
})

describe('showing a tab', () => {
  it('shows one of the slot’s tabs, and nothing else', () => {
    const cfg = at({}, { left2: ['bandTiles', 'clock'] })
    expect(showTab(cfg, 'left2', 'clock').slots.left2).toBe('clock')
    expect(showTab(cfg, 'left2', 'greyline'), 'not one of its tabs').toEqual(cfg)
  })
})

describe('the invariant survives any sequence of the operator’s moves', () => {
  it('500 random picks, adds, removes and tab switches', () => {
    // A small deterministic generator, so a failure names a reproducible step.
    let seed = 20260929
    const rnd = (n: number) => {
      seed = (seed * 1103515245 + 12345) % 2 ** 31
      return seed % n
    }
    let p: Placement = at({})
    for (let step = 0; step < 500; step++) {
      const s = SLOT_IDS[rnd(SLOT_IDS.length)]
      const id = PANE_IDS[rnd(PANE_IDS.length)]
      const op = rnd(4)
      const next = op === 0 ? assignBox(p, s, id) : op === 1 ? addTab(p, s, id) : op === 2 ? removeTab(p, s) : showTab(p, s, slotBoxes(p, s)[rnd(slotBoxes(p, s).length)])
      if (next) p = { slots: next.slots, tabs: next.tabs }
      expect(broken(p), `step ${step}: op ${op} on ${s} with ${id}`).toEqual([])
      // …and what the store keeps round-trips through a reload unchanged.
      const again = normalizeConfig({ slots: p.slots, tabs: p.tabs })
      expect({ slots: again.slots, tabs: again.tabs }, `step ${step} reload`).toEqual(p)
    }
  })
})

describe('a tab a layout does not list is not that layout', () => {
  it('every preset reads as itself with exactly its own tabs, and as Custom the moment a slot holds one more', () => {
    for (const id of CONNECT_PRESET_IDS) {
      const pre = CONNECT_PRESETS[id]
      const own = Object.fromEntries(Object.entries(pre.tabs ?? {}).map(([s, l]) => [s, [...l]])) as Partial<Record<SlotId, PaneId[]>>
      const now = { slots: pre.slots, panels: layoutPanels(pre), rails: pre.rails, bar: !!pre.bar }
      expect(connectLayoutNow({ ...now, tabs: own }), id).toBe(id)
      const s = SLOT_IDS.find((x) => !pre.hidden.includes(x))!
      const placed = SLOT_IDS.flatMap((x) => slotBoxes({ slots: pre.slots, tabs: own }, x))
      const extra = PANE_IDS.find((p) => !placed.includes(p))!
      expect(connectLayoutNow({ ...now, tabs: { ...own, [s]: [...slotBoxes({ slots: pre.slots, tabs: own }, s), extra] } }), `${id} + a tab`).toBe('custom')
    }
  })
})

describe('auto-rotate: which slots may rotate, and how fast', () => {
  const tabbed = { left2: ['bandTiles', 'clock'] as PaneId[] }

  it('the operator picks from 10 s, 15 s, 30 s, 1 min and 2 min', () => {
    expect([...ROTATE_CHOICES]).toEqual([10, 15, 30, 60, 120])
  })

  it('a stored interval is kept for a slot with tabs, and dropped for a one-pane slot, an unoffered interval or junk', () => {
    expect(coerceRotate(tabbed, { left2: 15 })).toEqual({ left2: 15 })
    expect(coerceRotate(tabbed, { left1: 15 }), 'left1 holds one pane: nothing to rotate').toEqual({})
    for (const v of [0, 7, 45, -10, '15', null, Number.NaN]) expect(coerceRotate(tabbed, { left2: v }), String(v)).toEqual({})
    for (const junk of [null, 5, 'x', []]) expect(coerceRotate(tabbed, junk), JSON.stringify(junk)).toEqual({})
  })

  it('a layout saved before this rotates nothing: normalize reads no intervals', () => {
    expect(normalizeConfig({ slots: DEFAULT_SLOTS, tabs: tabbed }).rotate).toEqual({})
    expect(normalizeConfig({ slots: DEFAULT_SLOTS, tabs: tabbed, rotate: { left2: 30 } }).rotate).toEqual({ left2: 30 })
  })
})
