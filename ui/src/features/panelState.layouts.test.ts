// @vitest-environment jsdom
//
// A COCKPIT ARRANGED PER LAYOUT (2026-10-07: the FT cockpit, the operator's "Two saved arrangements").
// FT has two stock layouts, Classic and Roster; each is arranged on its own (`arrangeBy`), the record
// keeps one placement per layout (`places`), and visibility stays one record for both, as it always
// was. Computed on the real vocabulary: what a stored or hand-edited record is coerced to, what an
// older build makes of it, the hook's moves and boxes in the layout they are made in, and the
// placement rules for a cockpit with fewer than three columns, or its columns in another order.
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import {
  BOX_IDS,
  OPERATE_ARRANGE,
  OPERATE_PANELS,
  arrangeSpecOf,
  boxIdsOf,
  coercePanelLayout,
  panelStorageKey,
  placeOf,
  usePanelLayout,
  type OperatePanelId,
  type PanelVocabulary,
} from './panelState'
import { canMoveArranged, coercePlacement, columnsOf, moveArranged, movePane, placedColumns, stockPlacement } from './panelPlace'
import { __resetDurableForTest } from './durableStore'

vi.mock('../api', () => ({
  uiStateLoad: async () => null,
  uiStateSave: async () => true,
}))

beforeEach(() => {
  localStorage.clear()
  __resetDurableForTest()
})

const ROSTER = OPERATE_ARRANGE.roster
const CLASSIC = OPERATE_ARRANGE.classic
const all = () => true
const KEY = panelStorageKey('operate')
const last = <T,>(xs: readonly T[]): T | undefined => xs[xs.length - 1]

describe('FT is arranged per layout, over each layout’s own panes', () => {
  it('the stock arrangements are the columns FT has always drawn', () => {
    const own = (c: Record<string, OperatePanelId[]>) =>
      Object.fromEntries(Object.entries(c).map(([k, ids]) => [k, ids.filter((id) => !(BOX_IDS as readonly string[]).includes(id))]))
    expect(own(placedColumns(CLASSIC, undefined))).toEqual({ a: ['bandActivity'], b: ['rxfreq', 'txmsgs'], log: ['recall', 'stations'] })
    expect(own(placedColumns(ROSTER, undefined))).toEqual({ a: ['callRoster'], b: [], log: ['recall', 'bandActivity', 'rxfreq'] })
    expect(columnsOf(CLASSIC)).toEqual(['a', 'b', 'log'])
    expect(columnsOf(ROSTER), 'Roster has its main column and the side rail').toEqual(['a', 'log'])
  })

  it('no layout arranges a pane outside the region: the waterfall, the RF scope pane and the meters have no place', () => {
    for (const spec of [CLASSIC, ROSTER]) {
      const ids = [...spec.columns.a, ...spec.columns.b, ...spec.columns.log]
      for (const id of ['waterfall', 'rfScope', 'txmeters'] as const) expect(ids, id).not.toContain(id)
      expect(spec.pinned).toEqual([])
    }
  })

  it('the vocabulary names its layouts, and its boxes are every layout’s', () => {
    expect(OPERATE_PANELS.arrange, 'FT has no single arrangement').toBeUndefined()
    expect(arrangeSpecOf(OPERATE_PANELS, 'roster')).toBe(ROSTER)
    expect(arrangeSpecOf(OPERATE_PANELS, 'classic')).toBe(CLASSIC)
    expect(arrangeSpecOf(OPERATE_PANELS), 'a move with no layout arranges nothing').toBeUndefined()
    expect([...boxIdsOf(OPERATE_PANELS)]).toEqual([...BOX_IDS])
  })
})

describe('the record keeps a placement per layout', () => {
  it('coerces each layout’s placement by that layout’s spec, and drops a layout FT does not have', () => {
    const rec = coercePanelLayout(OPERATE_PANELS, {
      v: 2,
      state: {},
      share: {},
      place: { rxfreq: { col: 'a', order: 0 } },
      places: {
        roster: { rxfreq: { col: 'a', order: 5 }, txmsgs: { col: 'a', order: 0 }, ptt: { col: 'a', order: 1 } },
        classic: { callRoster: { col: 'b', order: 0 }, stations: { col: 'a', order: 9 } },
        wide: { rxfreq: { col: 'a', order: 0 } },
      },
    })
    // Roster: Rx Frequency in the main column; the Tx messages (not a Roster pane) and a stop control's
    // name are no panes of it.
    expect(rec.places?.roster).toEqual({ rxfreq: { col: 'a', order: 0 } })
    // Classic: Stations in column 1; the Call Roster is not one of Classic's panes.
    expect(rec.places?.classic).toEqual({ stations: { col: 'a', order: 0 } })
    expect(Object.keys(rec.places ?? {}).sort()).toEqual(['classic', 'roster'])
    expect(rec.place, 'FT keeps no single placement').toBeUndefined()
  })

  it('a place in a column the layout does not have is dropped: Roster has no second column', () => {
    expect(coercePlacement(ROSTER, { rxfreq: { col: 'b', order: 0 }, callRoster: { col: 'log', order: 0 } })).toEqual({ callRoster: { col: 'log', order: 0 } })
    expect(coercePlacement(ROSTER, { rxfreq: { col: 'b', order: 0 } })).toBeUndefined()
  })

  it('an older build reads the record as stock: it knows no `places` and no boxes', () => {
    // An older build's FT vocabulary: no arrangeBy, no boxes, the RF scope pane hidden.
    const OLDER: PanelVocabulary<string> = {
      view: 'operate',
      panelIds: ['waterfall', 'rfScope', 'bandActivity', 'callRoster', 'rxfreq', 'txmsgs', 'stations', 'txmeters', 'recall'],
      defaultRemoved: ['rfScope'],
    }
    const newer = { v: 2, state: { box1: 'docked', stations: 'removed' }, share: { rxfreq: 0.8 }, boxes: { box1: 'clock' }, places: { roster: { rxfreq: { col: 'a', order: 0 } } } }
    const read = coercePanelLayout(OLDER, newer)
    expect(read).toEqual({ v: 2, state: { stations: 'removed' }, share: { rxfreq: 0.8 } })
  })
})

describe('the hook moves and adds in the layout it is told, and nowhere else', () => {
  it('a move in Roster writes Roster’s placement only, in one undoable step', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.movePane!('rxfreq', 'left', all, false, { layout: 'roster' }))
    expect(placedColumns(ROSTER, placeOf(OPERATE_PANELS, result.current.layout, 'roster')).a).toEqual(['callRoster', 'rxfreq'])
    expect(result.current.layout.places?.classic, 'the move reached Classic').toBeUndefined()
    expect(result.current.layout.place).toBeUndefined()
    expect(JSON.parse(localStorage.getItem(KEY)!).places.roster.rxfreq).toEqual({ col: 'a', order: 1 })
    act(() => result.current.undo())
    expect(result.current.layout.places, 'Undo left a placement').toBeUndefined()
  })

  it('a move with no layout, or a pane the layout does not have, is no step at all', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.movePane!('rxfreq', 'left', all))
    act(() => result.current.movePane!('callRoster', 'right', all, false, { layout: 'classic' }))
    expect(result.current.layout.places).toBeUndefined()
    expect(result.current.canUndo).toBe(false)
  })

  it('"+ Add a box" in a layout puts the box in that layout’s column, and the box shows in both', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.addBox!('a', 'roster'))
    expect(result.current.stateOf('box1')).toBe('docked')
    expect(placedColumns(ROSTER, placeOf(OPERATE_PANELS, result.current.layout, 'roster')).a).toEqual(['callRoster', 'box1'])
    // Visibility is one record for both layouts, as it has always been for FT's panes: in Classic the box
    // stands where Classic keeps it, at the foot of the side rail until moved there.
    expect(result.current.layout.places?.classic).toBeUndefined()
    expect(placedColumns(CLASSIC, undefined).log[placedColumns(CLASSIC, undefined).log.length - BOX_IDS.length]).toBe('box1')
  })

  it('Reset empties both layouts’ placements and hides every box', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.movePane!('rxfreq', 'left', all, false, { layout: 'roster' }))
    act(() => result.current.movePane!('txmsgs', 'right', all, false, { layout: 'classic' }))
    act(() => result.current.addBox!('log', 'classic'))
    act(() => result.current.reset())
    expect(result.current.layout.places).toBeUndefined()
    expect(BOX_IDS.map((b) => result.current.stateOf(b))).toEqual(BOX_IDS.map(() => 'removed'))
  })
})

describe('the placement rules, for fewer columns or another order on screen', () => {
  it('◀ ▶ reach only the columns the cockpit has: Roster’s main column and its rail are neighbours', () => {
    const toRail = movePane(ROSTER, undefined, undefined, 'callRoster', 'right', all)
    // At the foot of the rail, after the boxes listed there (each box hidden until added).
    expect(last(placedColumns(ROSTER, toRail!).log)).toBe('callRoster')
    expect(placedColumns(ROSTER, toRail!).a).toEqual([])
    expect(movePane(ROSTER, toRail!, undefined, 'callRoster', 'right', all), 'a column past the rail').toBeNull()
    expect(movePane(ROSTER, undefined, undefined, 'callRoster', 'left', all)).toBeNull()
  })

  it('with the rail on the left, ◀ ▶ follow the screen: the rail is left of the main column', () => {
    const order = ['log', 'a'] as const
    const arr = { place: stockPlacement(ROSTER) }
    expect(canMoveArranged(ROSTER, arr, 'callRoster', 'right', all, false, order)).toBe(false)
    const next = moveArranged(ROSTER, arr, 'callRoster', 'left', all, false, order)
    expect(next?.place?.callRoster?.col).toBe('log')
    // …and in the stock order the same press goes nowhere.
    expect(canMoveArranged(ROSTER, arr, 'callRoster', 'left', all, false)).toBe(false)
  })

  it('a cockpit with all three columns moves as it always did', () => {
    const next = movePane(CLASSIC, undefined, undefined, 'txmsgs', 'right', all)
    expect(last(placedColumns(CLASSIC, next!).log)).toBe('txmsgs')
    expect(placedColumns(CLASSIC, next!).b).toEqual(['rxfreq'])
  })
})
