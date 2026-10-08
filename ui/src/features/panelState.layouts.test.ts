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
  extrasIn,
  panelStorageKey,
  placeOf,
  usePanelLayout,
  type OperatePanelId,
  type PanelVocabulary,
} from './panelState'
import { arrangeIds, canMoveArranged, coercePlacement, columnsOf, moveArranged, movePane, placedColumns, stockPlacement } from './panelPlace'
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

describe('FT is arranged per layout, over every FT pane', () => {
  it('the stock arrangements are the columns FT has always drawn, with the other layout’s panes listed but not drawn', () => {
    const own = (c: Record<string, OperatePanelId[]>, extra: readonly OperatePanelId[] = []) =>
      Object.fromEntries(Object.entries(c).map(([k, ids]) => [k, ids.filter((id) => !(BOX_IDS as readonly string[]).includes(id) && !extra.includes(id))]))
    // What each layout draws until the operator adds a pane: today's columns.
    expect(own(placedColumns(CLASSIC, undefined), CLASSIC.extra)).toEqual({ a: ['bandActivity'], b: ['rxfreq', 'txmsgs'], log: ['recall', 'stations'] })
    expect(own(placedColumns(ROSTER, undefined), ROSTER.extra)).toEqual({ a: ['callRoster'], b: [], log: ['recall', 'bandActivity', 'rxfreq'] })
    // "Every FT pane in both": the Call Roster under Band Activity in Classic, the Tx messages and Stations in
    // Roster's rail under Rx Frequency, and nothing else is another layout's.
    expect(own(placedColumns(CLASSIC, undefined))).toEqual({ a: ['bandActivity', 'callRoster'], b: ['rxfreq', 'txmsgs'], log: ['recall', 'stations'] })
    expect(own(placedColumns(ROSTER, undefined))).toEqual({ a: ['callRoster'], b: [], log: ['recall', 'bandActivity', 'rxfreq', 'txmsgs', 'stations'] })
    expect(CLASSIC.extra).toEqual(['callRoster'])
    expect(ROSTER.extra).toEqual(['txmsgs', 'stations'])
    const panes = (spec: typeof CLASSIC) => arrangeIds(spec).filter((id) => !(BOX_IDS as readonly string[]).includes(id)).sort()
    expect(panes(CLASSIC), 'both layouts place every FT pane').toEqual(panes(ROSTER))
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
        roster: { rxfreq: { col: 'a', order: 5 }, waterfall: { col: 'a', order: 0 }, ptt: { col: 'a', order: 1 } },
        classic: { txmeters: { col: 'b', order: 0 }, stations: { col: 'a', order: 9 } },
        wide: { rxfreq: { col: 'a', order: 0 } },
      },
    })
    // Roster: Rx Frequency in the main column; the waterfall (outside the region) and a stop control's
    // name are no panes of it.
    expect(rec.places?.roster).toEqual({ rxfreq: { col: 'a', order: 0 } })
    // Classic: Stations in column 1; the TX meters have no place in any layout.
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

  it('a move with no layout, or a pane no layout places, is no step at all', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.movePane!('rxfreq', 'left', all))
    act(() => result.current.movePane!('waterfall', 'right', all, false, { layout: 'classic' }))
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

describe('every FT pane in both layouts: the other layout’s panes are added to a layout, and to it only', () => {
  // The operator's "Every FT pane in both" (2026-10-07). A layout's `extra` panes (the Call Roster in Classic,
  // the Tx messages and Stations in Roster) are on screen there once added (the record's `extras`) and not
  // hidden; visibility stays one record for both layouts, so a hide anywhere hides them.
  const inLayout = (layout: Parameters<typeof extrasIn>[1], name: string, id: OperatePanelId) => extrasIn(OPERATE_PANELS, layout, name, id)

  it('nothing added is each layout as it shipped: the other layout’s panes read hidden there, their own as ever', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    const rec = result.current.layout
    expect(inLayout(rec, 'classic', 'callRoster')).toBe('removed')
    expect(inLayout(rec, 'roster', 'callRoster')).toBe('docked')
    expect(['txmsgs', 'stations'].map((id) => inLayout(rec, 'roster', id as OperatePanelId))).toEqual(['removed', 'removed'])
    expect(['txmsgs', 'stations'].map((id) => inLayout(rec, 'classic', id as OperatePanelId))).toEqual(['docked', 'docked'])
  })

  it('adding the Call Roster to Classic shows it there, writes Classic’s stock placement, and leaves Roster alone', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setExtra!('classic', 'callRoster', true))
    expect(inLayout(result.current.layout, 'classic', 'callRoster')).toBe('docked')
    // Where Classic's stock arrangement lists it: under Band Activity in column 1. The placement is written,
    // so Classic keeps its arranged columns until Reset (a box's rule).
    expect(placedColumns(CLASSIC, placeOf(OPERATE_PANELS, result.current.layout, 'classic')).a).toEqual(['bandActivity', 'callRoster'])
    expect(result.current.layout.places?.roster, 'the add reached Roster').toBeUndefined()
    expect(JSON.parse(localStorage.getItem(KEY)!).extras).toEqual({ classic: ['callRoster'] })
    // Taken out of Classic, it is gone from Classic only: Roster's main column keeps it.
    act(() => result.current.setExtra!('classic', 'callRoster', false))
    expect(inLayout(result.current.layout, 'classic', 'callRoster')).toBe('removed')
    expect(inLayout(result.current.layout, 'roster', 'callRoster')).toBe('docked')
    expect(result.current.layout.extras).toBeUndefined()
    // One undoable step each.
    act(() => result.current.undo())
    expect(inLayout(result.current.layout, 'classic', 'callRoster')).toBe('docked')
  })

  it('a pane placed before keeps its place when it is added again', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setExtra!('roster', 'stations', true))
    act(() => result.current.movePane!('stations', 'left', all, false, { layout: 'roster' }))
    act(() => result.current.setExtra!('roster', 'stations', false))
    act(() => result.current.setExtra!('roster', 'stations', true))
    expect(placedColumns(ROSTER, placeOf(OPERATE_PANELS, result.current.layout, 'roster')).a).toEqual(['callRoster', 'stations'])
  })

  it('a hide anywhere hides it in both; adding it again shows it in both', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setExtra!('roster', 'stations', true))
    act(() => result.current.setPanelState('stations', 'removed'))
    expect(inLayout(result.current.layout, 'roster', 'stations')).toBe('removed')
    expect(inLayout(result.current.layout, 'classic', 'stations')).toBe('removed')
    act(() => result.current.setExtra!('roster', 'stations', true))
    expect(inLayout(result.current.layout, 'roster', 'stations')).toBe('docked')
    expect(inLayout(result.current.layout, 'classic', 'stations')).toBe('docked')
  })

  it('a change that changes nothing, or a pane the layout draws anyway, is no step at all', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setExtra!('classic', 'callRoster', false))
    act(() => result.current.setExtra!('classic', 'stations', true))
    act(() => result.current.setExtra!('wide', 'callRoster', true))
    expect(result.current.canUndo).toBe(false)
    expect(result.current.layout.extras).toBeUndefined()
  })

  it('the record keeps a layout’s own `extra` panes only, each once; Reset clears them', () => {
    const rec = coercePanelLayout(OPERATE_PANELS, {
      v: 2,
      state: {},
      share: {},
      extras: { classic: ['callRoster', 'callRoster', 'stations', 'ptt', 7], roster: ['txmsgs', 'waterfall'], wide: ['callRoster'] },
    })
    expect(rec.extras).toEqual({ classic: ['callRoster'], roster: ['txmsgs'] })
    expect(coercePanelLayout(OPERATE_PANELS, { v: 2, state: {}, share: {}, extras: 'junk' }).extras).toBeUndefined()
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setExtra!('roster', 'txmsgs', true))
    act(() => result.current.reset())
    expect(result.current.layout.extras).toBeUndefined()
    expect(inLayout(result.current.layout, 'roster', 'txmsgs')).toBe('removed')
  })

  it('an older build reads the record without them: each layout draws what it always drew', () => {
    const OLDER: PanelVocabulary<string> = {
      view: 'operate',
      panelIds: ['waterfall', 'rfScope', 'bandActivity', 'callRoster', 'rxfreq', 'txmsgs', 'stations', 'txmeters', 'recall'],
      defaultRemoved: ['rfScope'],
    }
    const newer = { v: 2, state: { stations: 'docked' }, share: {}, extras: { classic: ['callRoster'], roster: ['txmsgs', 'stations'] } }
    expect(coercePanelLayout(OLDER, newer)).toEqual({ v: 2, state: { stations: 'docked' }, share: {} })
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
