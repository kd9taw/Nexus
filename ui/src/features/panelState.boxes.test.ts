// @vitest-environment jsdom
//
// THE BOXES IN THE PANEL RECORD (any pane in any area, 2026-10-07). Phone, CW and JS8 each have six
// box slots: a box shows one entry of the shared list (features/sharedPanes), the record's `boxes`
// says which, its `state` whether it is on screen (every box ships hidden) and its `place` / `leftSide`
// where it stands, like any pane. Computed on the real vocabularies: what a stock record shows, what a
// stored or hand-edited one is coerced to, "+ Add a box", a box's picker and Reset, and how the boxes
// join a cockpit's columns without moving its own panes.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import {
  BOX_IDS,
  CW_PANELS,
  JS8_PANELS,
  PHONE_PANELS,
  addBoxTo,
  boxEntries,
  coercePanelLayout,
  emptyPanelLayout,
  loadPanelLayout,
  panelStateIn,
  showInBox,
  usePanelLayout,
  type PanelLayout,
  type PanelVocabulary,
  type PhonePanelId,
} from './panelState'
import { SHARED_PANES } from './sharedPanes'
import { isStockPlacement, placedColumns, regionGroups, stockColumn } from './panelPlace'
import { __resetDurableForTest } from './durableStore'

vi.mock('../api', () => ({
  uiStateLoad: async () => null,
  uiStateSave: async () => true,
}))

beforeEach(() => {
  localStorage.clear()
  __resetDurableForTest()
})
afterEach(() => __resetDurableForTest())

const GRID = [PHONE_PANELS, CW_PANELS, JS8_PANELS] as unknown as PanelVocabulary<string>[]
const last = <T,>(xs: readonly T[]): T | undefined => xs[xs.length - 1]
const FIRST = SHARED_PANES.map((e) => e.id)

/** A record with these boxes on screen, showing these entries. */
function withBoxes<P extends string>(_spec: PanelVocabulary<P>, shown: Partial<Record<P, string | null>>, over: Partial<PanelLayout<P>> = {}): PanelLayout<P> {
  const state: PanelLayout<P>['state'] = { ...over.state }
  const boxes: Partial<Record<P, string>> = {}
  for (const [b, e] of Object.entries(shown) as [P, string | null][]) {
    state[b] = 'docked'
    if (e != null) boxes[b] = e
  }
  return { ...emptyPanelLayout<P>(), ...over, state, boxes }
}

describe('six boxes per grid cockpit, all shipped hidden', () => {
  it('Phone, CW and JS8 each list box1…box6: in the vocabulary, hidden by default, in the leading column', () => {
    for (const spec of GRID) {
      expect(spec.arrange?.boxes, spec.view).toEqual([...BOX_IDS])
      for (const b of BOX_IDS) {
        expect(spec.panelIds, `${spec.view} ${b}`).toContain(b)
        expect(spec.defaultRemoved, `${spec.view} ${b}`).toContain(b)
        expect(panelStateIn(spec, emptyPanelLayout(), b), `${spec.view} ${b}`).toBe('removed')
        expect(stockColumn(spec.arrange!, b), `${spec.view} ${b}`).toBe('a')
      }
    }
    // Phone's left side may hold them too; CW and JS8 have none.
    expect(PHONE_PANELS.arrange?.leftSide).toEqual(expect.arrayContaining([...BOX_IDS]))
  })

  it('a stock record shows no box, so nobody’s screen changes on update', () => {
    for (const spec of GRID) {
      expect(boxEntries(spec, emptyPanelLayout())).toEqual({})
      expect(coercePanelLayout(spec, { v: 2, state: {}, share: {} }).boxes).toBeUndefined()
    }
  })

  it('the cockpit’s own Spots and Needed boards are the shared list’s, for once per screen', () => {
    expect(PHONE_PANELS.sharedAs).toEqual({ spots: 'spotsBoard', needed: 'neededBoard' })
    expect(CW_PANELS.sharedAs).toEqual({ spots: 'spotsBoard', needed: 'neededBoard' })
    expect(JS8_PANELS.sharedAs).toBeUndefined()
  })
})

describe('what a box shows, from any record', () => {
  const spec = PHONE_PANELS

  it('a stored entry stands; a hidden box shows nothing on screen but keeps what it showed', () => {
    const rec = coercePanelLayout(spec, {
      state: { box1: 'docked', box2: 'docked', box3: 'removed' },
      boxes: { box1: 'clock', box2: 'pota', box3: 'spacewx' },
    })
    expect(boxEntries(spec, rec)).toEqual({ box1: 'clock', box2: 'pota' })
    expect(rec.boxes).toEqual({ box1: 'clock', box2: 'pota', box3: 'spacewx' })
  })

  it('drops an entry the list does not have and an id that is no box, and gives the box a free entry', () => {
    const rec = coercePanelLayout(spec, {
      state: { box1: 'docked' },
      boxes: { box1: 'ghost', spots: 'clock', ptt: 'clock', box9: 'clock' },
    })
    expect(rec.boxes).toEqual({ box1: FIRST[0] })
    expect(boxEntries(spec, rec)).toEqual({ box1: FIRST[0] })
  })

  it('repairs a duplicate on one screen: the later box takes the first entry not on screen', () => {
    const rec = coercePanelLayout(spec, {
      state: { box1: 'docked', box4: 'docked' },
      boxes: { box1: 'clock', box4: 'clock' },
    })
    expect(boxEntries(spec, rec)).toEqual({ box1: 'clock', box4: FIRST[0] })
    // …and an entry stored in a LATER box is not taken from it by an earlier box being filled.
    const kept = coercePanelLayout(spec, { state: { box1: 'docked', box2: 'docked' }, boxes: { box2: FIRST[0] } })
    expect(boxEntries(spec, kept)).toEqual({ box1: FIRST[1], box2: FIRST[0] })
  })

  it('counts the cockpit’s own Spots and Needed panes: a box never shows the board beside its twin', () => {
    // Spots and Needed ship hidden: the box keeps the boards while they are.
    const hidden = coercePanelLayout(spec, { state: { box1: 'docked', box2: 'docked' }, boxes: { box1: 'spotsBoard', box2: 'neededBoard' } })
    expect(boxEntries(spec, hidden)).toEqual({ box1: 'spotsBoard', box2: 'neededBoard' })
    // Ticked, the cockpit's own pane is the board on screen, and each box shows something else.
    const ticked = coercePanelLayout(spec, {
      state: { spots: 'docked', needed: 'docked', box1: 'docked', box2: 'docked' },
      boxes: { box1: 'spotsBoard', box2: 'neededBoard' },
    })
    expect(boxEntries(spec, ticked)).toEqual({ box1: FIRST[0], box2: FIRST[1] })
    // JS8 has no Spots or Needed pane of its own: a box there may show either.
    const js8 = coercePanelLayout(JS8_PANELS, { state: { box1: 'docked' }, boxes: { box1: 'spotsBoard' } })
    expect(boxEntries(JS8_PANELS, js8)).toEqual({ box1: 'spotsBoard' })
  })

  it('an older record, with no boxes at all, loads exactly as it was stored', () => {
    const old = { v: 2, state: { spots: 'docked', receiver: 'removed' }, share: { spots: 1.2 }, place: { spots: { col: 'b', order: 0 } } }
    localStorage.setItem('nexus.panels.phone.main', JSON.stringify(old))
    const rec = loadPanelLayout(spec, 'main')
    expect(rec.boxes).toBeUndefined()
    expect(rec.state).toEqual(old.state)
    expect(boxEntries(spec, rec)).toEqual({})
  })
})

describe('+ Add a box', () => {
  const spec = PHONE_PANELS

  it('puts the first hidden box at the foot of the area, showing the first entry not on screen', () => {
    const once = addBoxTo(spec, emptyPanelLayout<PhonePanelId>(), 'b')!
    expect(panelStateIn(spec, once, 'box1')).toBe('docked')
    expect(boxEntries(spec, once)).toEqual({ box1: FIRST[0] })
    expect(last(placedColumns(spec.arrange!, once.place).b)).toBe('box1')
    const twice = addBoxTo(spec, once, 'a')!
    expect(boxEntries(spec, twice)).toEqual({ box1: FIRST[0], box2: FIRST[1] })
    expect(last(placedColumns(spec.arrange!, twice.place).a)).toBe('box2')
    expect(last(placedColumns(spec.arrange!, twice.place).b)).toBe('box1')
  })

  it('moves none of the cockpit’s own panes: the grouping is still the stock one', () => {
    let rec = emptyPanelLayout<PhonePanelId>()
    for (const area of ['a', 'b', 'log', 'a'] as const) rec = addBoxTo(spec, rec, area)!
    expect(isStockPlacement(spec.arrange!, rec.place)).toBe(true)
    const own = (ids: readonly string[]) => ids.filter((id) => !id.startsWith('box'))
    const cols = placedColumns(spec.arrange!, rec.place)
    for (const c of ['a', 'b', 'log'] as const) expect(own(cols[c])).toEqual(own(spec.arrange!.columns[c]))
  })

  it('on the left side: at its foot, and at the foot of column 1 whenever the side does not show', () => {
    const rec = addBoxTo(spec, emptyPanelLayout<PhonePanelId>(), 'side')!
    expect(rec.leftSide).toEqual(['box1'])
    expect(last(placedColumns(spec.arrange!, rec.place).a)).toBe('box1')
    // A cockpit with no left side has nowhere to put one there.
    expect(addBoxTo(CW_PANELS, emptyPanelLayout(), 'side')).toBeNull()
  })

  it('a box re-added to a column leaves the left side', () => {
    const sided = addBoxTo(spec, emptyPanelLayout<PhonePanelId>(), 'side')!
    const hidden = { ...sided, state: { ...sided.state, box1: 'removed' as const } }
    const back = addBoxTo(spec, hidden, 'b')!
    expect(back.leftSide ?? []).not.toContain('box1')
    expect(last(placedColumns(spec.arrange!, back.place).b)).toBe('box1')
  })

  it('takes the next entry not on screen each time, and stops at six', () => {
    let rec = coercePanelLayout(spec, { state: { spots: 'docked' } })
    for (let i = 0; i < 6; i++) rec = addBoxTo(spec, rec, 'a')!
    expect(Object.values(boxEntries(spec, rec))).toEqual(FIRST.slice(0, 6))
    expect(addBoxTo(spec, rec, 'a'), 'a seventh box').toBeNull()
  })
})

describe('once per screen with the dashboard rail beside the cockpit (`elsewhere`)', () => {
  // The rail's slots come before the cockpit's boxes (App computes what the rail shows): a box whose entry the
  // rail shows shows another, and "+ Add a box" takes none of them. The record is never rewritten by it.
  const spec = PHONE_PANELS

  it('a box gives the rail its entry on screen and keeps it in the record', () => {
    const rec = coercePanelLayout(spec, { state: { box1: 'docked', box2: 'docked' }, boxes: { box1: 'clock', box2: 'pota' } })
    const shown = boxEntries(spec, rec, undefined, ['clock', FIRST[0]])
    expect(shown.box2).toBe('pota')
    expect(shown.box1, 'the Clock is on the screen twice').not.toBe('clock')
    expect([...Object.values(shown)].some((e) => e === 'clock' || e === FIRST[0]), 'a box took one of the rail’s').toBe(false)
    expect(rec.boxes?.box1, 'the rail rewrote the record').toBe('clock')
    // With the rail gone the box shows its own again.
    expect(boxEntries(spec, rec).box1).toBe('clock')
  })

  it('“+ Add a box” takes no entry the rail shows', () => {
    const added = addBoxTo(spec, emptyPanelLayout<PhonePanelId>(), 'a', undefined, [FIRST[0], FIRST[1]])!
    expect(boxEntries(spec, added, undefined, [FIRST[0], FIRST[1]])).toEqual({ box1: FIRST[2] })
    expect(added.boxes?.box1).toBe(FIRST[2])
  })
})

describe('a box’s picker', () => {
  const spec = PHONE_PANELS

  it('shows the entry it is given', () => {
    const rec = showInBox(spec, withBoxes(spec, { box1: 'clock' }), 'box1', 'spacewx')!
    expect(boxEntries(spec, rec)).toEqual({ box1: 'spacewx' })
  })

  it('an entry another box shows moves here, and that box takes this one’s (the rail’s swap)', () => {
    const rec = showInBox(spec, withBoxes(spec, { box1: 'clock', box2: 'spacewx' }), 'box2', 'clock')!
    expect(boxEntries(spec, rec)).toEqual({ box1: 'spacewx', box2: 'clock' })
  })

  it('a board the cockpit already shows as its own pane moves here: that pane is hidden, in the same step', () => {
    const rec = showInBox(spec, withBoxes(spec, { box1: 'clock' }, { state: { spots: 'docked' } }), 'box1', 'spotsBoard')!
    expect(panelStateIn(spec, rec, 'spots')).toBe('removed')
    expect(boxEntries(spec, rec)).toEqual({ box1: 'spotsBoard' })
  })

  it('refuses what is no entry, what the box already shows, and what is no box', () => {
    const rec = withBoxes(spec, { box1: 'clock' })
    expect(showInBox(spec, rec, 'box1', 'ghost')).toBeNull()
    expect(showInBox(spec, rec, 'box1', 'clock')).toBeNull()
    expect(showInBox(spec, rec, 'spots', 'clock')).toBeNull()
  })
})

describe('the hook: one undoable step each, stored as it goes', () => {
  const hook = () => renderHook(() => usePanelLayout(PHONE_PANELS, 'main'))

  it('Add a box and the picker are each one Undo; Reset hides every box again', () => {
    const { result } = hook()
    act(() => result.current.addBox!('b'))
    act(() => result.current.addBox!('a'))
    expect(boxEntries(PHONE_PANELS, result.current.layout)).toEqual({ box1: FIRST[0], box2: FIRST[1] })
    act(() => result.current.setBox!('box2', 'clock'))
    expect(boxEntries(PHONE_PANELS, result.current.layout)).toEqual({ box1: FIRST[0], box2: 'clock' })
    act(() => result.current.undo())
    expect(boxEntries(PHONE_PANELS, result.current.layout)).toEqual({ box1: FIRST[0], box2: FIRST[1] })
    act(() => result.current.reset())
    expect(boxEntries(PHONE_PANELS, result.current.layout)).toEqual({})
    for (const b of BOX_IDS) expect(result.current.stateOf(b)).toBe('removed')
  })

  it('ticking the cockpit’s own Spots while a box shows the Spots board gives the box another entry', () => {
    const { result } = hook()
    act(() => result.current.addBox!('a'))
    act(() => result.current.setBox!('box1', 'spotsBoard'))
    act(() => result.current.setPanelState('spots', 'docked'))
    expect(boxEntries(PHONE_PANELS, result.current.layout)).toEqual({ box1: FIRST[0] })
    // What the record stores is what the screen shows: a reload reads the same.
    expect(boxEntries(PHONE_PANELS, loadPanelLayout(PHONE_PANELS, 'main'))).toEqual({ box1: FIRST[0] })
  })

  it('a move does nothing to a box’s entry, and the stored arrangement round-trips', () => {
    const { result, unmount } = hook()
    act(() => result.current.addBox!('a'))
    act(() => result.current.movePane!('box1', 'up', () => true))
    unmount()
    const again = hook()
    expect(boxEntries(PHONE_PANELS, again.result.current.layout)).toEqual({ box1: FIRST[0] })
    expect(again.result.current.stateOf('box1')).toBe('docked')
  })
})

describe('the boxes in a cockpit’s columns', () => {
  const spec = PHONE_PANELS.arrange!
  const shownAll = () => true

  it('below three tracks the stock merged order of the own panes stands, each box after the pane above it', () => {
    // Stock own panes; box1 at the top of column 1, box2 after Band Activity, box3 at the foot of
    // column 2 (after Needed), box4 at the top of column 2.
    let rec = emptyPanelLayout<PhonePanelId>()
    for (const area of ['a', 'a', 'b', 'b'] as const) rec = addBoxTo(PHONE_PANELS, rec, area)!
    // Lift box1 to the top of column 1 and box4 to the top of column 2.
    const up = (r: PanelLayout<PhonePanelId>, id: PhonePanelId, n: number) => {
      let place = r.place
      for (let i = 0; i < n; i++) {
        const cols = placedColumns(spec, place)
        const col = (['a', 'b', 'log'] as const).find((c) => cols[c].includes(id))!
        const at = cols[col].indexOf(id)
        if (at === 0) break
        cols[col].splice(at, 1)
        cols[col].splice(at - 1, 0, id)
        place = Object.fromEntries((['a', 'b', 'log'] as const).flatMap((c) => cols[c].map((x, k) => [x, { col: c, order: k }])))
      }
      return { ...r, place }
    }
    rec = up(up(rec, 'box1', 20), 'box4', 20)
    // box2 sits directly under Band Activity.
    {
      const cols = placedColumns(spec, rec.place)
      cols.a = cols.a.filter((x) => x !== 'box2')
      cols.a.splice(cols.a.indexOf('bandActivity') + 1, 0, 'box2')
      rec = { ...rec, place: Object.fromEntries((['a', 'b', 'log'] as const).flatMap((c) => cols[c].map((x, k) => [x, { col: c, order: k }]))) }
    }
    expect(isStockPlacement(spec, rec.place), 'the own panes are where they shipped').toBe(true)
    const shown = (id: PhonePanelId) => !id.startsWith('box') || boxEntries(PHONE_PANELS, rec)[id] != null
    const merged = regionGroups(spec, rec.place, 2, shown)[0].ids
    expect(merged).toEqual([
      'box1',
      'bandActivity',
      'box2',
      'voiceKeyer',
      'box4',
      'rigscope',
      'receiver',
      'transmitter',
      'spots',
      'needed',
      'box3',
    ])
    // Without the boxes it is exactly the stock merged order.
    expect(regionGroups(spec, rec.place, 2, (id) => !id.startsWith('box'))[0].ids).toEqual([...spec.stockMerged!])
    // At three tracks each box stands where it was put.
    const three = regionGroups(spec, rec.place, 3, shownAll)
    expect(three[0].ids.slice(0, 3)).toEqual(['box1', 'bandActivity', 'box2'])
    expect(three[1].ids[0]).toBe('box4')
  })

  it('moving an own pane is still an arrangement, boxes or not', () => {
    const moved = { spots: { col: 'b' as const, order: 0 } }
    expect(isStockPlacement(spec, moved)).toBe(false)
  })
})
