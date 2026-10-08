// @vitest-environment jsdom
//
// WHERE A GRID COCKPIT'S PANES STAND (layout L3): the placement rules, pure, and the panel record that
// carries them — coercion, migration from older records, what an older build makes of a newer one,
// and the hook's one-step move with Undo and Reset. The cockpit that renders a placement and the
// stop-line and fiber sweeps over random placements live beside the cockpits.
import { describe, it, expect, beforeEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import {
  PANE_COLUMNS,
  arrangeIds,
  coerceColumnOrder,
  coercePlacement,
  isStockPlacement,
  movePane,
  placedColumns,
  regionGroups,
  stockPlacement,
  type ArrangeSpec,
  type PanePlacement,
} from './panelPlace'
import { BOX_IDS, PHONE_PANELS, RTTY_PANELS, coercePanelLayout, panelStorageKey, usePanelLayout, type PanelLayout, type PhonePanelId } from './panelState'

const SPEC = PHONE_PANELS.arrange!
const all = () => true
// The cockpit's own panes on screen and its boxes hidden, as every box ships (features/panelState).
const stockShown = (id: PhonePanelId) => !(BOX_IDS as readonly string[]).includes(id)

describe('the placement a record may carry', () => {
  it('keeps a place only for the panes the cockpit arranges: an id outside it, or no id at all, is dropped', () => {
    const got = coercePlacement(SPEC, {
      receiver: { col: 'a', order: 0 },
      // Vocabulary ids that are not region panes — the scope above it, the meters in the dock.
      scope: { col: 'a', order: 1 },
      txmeters: { col: 'b', order: 0 },
      // Not an id at all: a stop control can never be placed, because it has no id.
      ptt: { col: 'a', order: 2 },
      stopTx: { col: 'log', order: 0 },
    })
    expect(got).toEqual({ receiver: { col: 'a', order: 0 } })
  })

  it('holds a pinned pane in its stock column, whatever the record says', () => {
    const got = coercePlacement(SPEC, { voiceKeyer: { col: 'log', order: 0 }, spots: { col: 'log', order: 1 } })
    expect(got!.voiceKeyer!.col).toBe('a')
    expect(got!.spots).toEqual({ col: 'log', order: 0 })
  })

  it('re-numbers each column from 0 in stored order, ties broken by the stock order — no gap, no duplicate', () => {
    const got = coercePlacement(SPEC, {
      transmitter: { col: 'b', order: 7 },
      receiver: { col: 'b', order: 7 },
      needed: { col: 'b', order: -3 },
      rigscope: { col: 'b', order: 2.5 },
    })
    expect(got).toEqual({
      needed: { col: 'b', order: 0 },
      rigscope: { col: 'b', order: 1 },
      receiver: { col: 'b', order: 2 },
      transmitter: { col: 'b', order: 3 },
    })
  })

  it('reads junk as "nothing arranged"', () => {
    for (const raw of [null, undefined, 3, 'a', [], {}, { receiver: 'a' }, { receiver: { col: 'c', order: 0 } }, { receiver: { col: 'a', order: NaN } }])
      expect(coercePlacement(SPEC, raw), JSON.stringify(raw)).toBeUndefined()
  })

  it('keeps a column order only when it is a permutation of the three columns', () => {
    expect(coerceColumnOrder(['log', 'a', 'b'])).toEqual(['log', 'a', 'b'])
    for (const raw of [['a', 'a', 'log'], ['a', 'b'], ['a', 'b', 'log', 'a'], ['a', 'b', 'c'], 'a,b,log', null])
      expect(coerceColumnOrder(raw), JSON.stringify(raw)).toBeUndefined()
  })
})

describe('the columns a placement gives', () => {
  it('with nothing arranged, is the stock grouping', () => {
    expect(placedColumns(SPEC, undefined)).toEqual({ a: [...SPEC.columns.a], b: [...SPEC.columns.b], log: [] })
  })

  it('puts every pane in exactly one column, and one the record does not name at the end of its stock column', () => {
    const cols = placedColumns(SPEC, { needed: { col: 'a', order: 0 }, bandActivity: { col: 'log', order: 0 } })
    expect(cols).toEqual({
      // The boxes too stand at the end of their stock column, the record naming none of them.
      a: ['needed', 'voiceKeyer', 'spots', ...BOX_IDS],
      b: ['rigscope', 'receiver', 'transmitter'],
      log: ['bandActivity'],
    })
    const seen = PANE_COLUMNS.flatMap((c) => cols[c])
    expect([...seen].sort()).toEqual([...arrangeIds(SPEC)].sort())
  })
})

describe('the groups each tier renders', () => {
  const shownOf = (hidden: PhonePanelId[]) => (id: PhonePanelId) => !hidden.includes(id)

  it('three tracks: a | b | log, only what is shown', () => {
    expect(regionGroups(SPEC, undefined, 3, shownOf(['spots', 'needed', ...BOX_IDS]))).toEqual([
      { col: 'a', ids: ['bandActivity', 'voiceKeyer'] },
      { col: 'b', ids: ['rigscope', 'receiver', 'transmitter'] },
      { col: 'log', ids: [] },
    ])
  })

  it('fewer tracks, nothing arranged: the stock merged order, the feeds after the rig strips — as today', () => {
    for (const tracks of [1, 2] as const)
      expect(regionGroups(SPEC, undefined, tracks, stockShown)).toEqual([
        { col: 'a', ids: ['bandActivity', 'voiceKeyer', 'rigscope', 'receiver', 'transmitter', 'spots', 'needed'] },
        { col: 'log', ids: [] },
      ])
  })

  it('fewer tracks, arranged: column b simply follows column a', () => {
    const place = movePane(SPEC, undefined, undefined, 'receiver', 'left', all)!
    expect(regionGroups(SPEC, place, 2, stockShown)[0].ids).toEqual(['bandActivity', 'voiceKeyer', 'spots', 'receiver', 'rigscope', 'transmitter', 'needed'])
  })

  it('a placement written out in full but equal to the stock one still renders the stock merged order', () => {
    expect(isStockPlacement(SPEC, stockPlacement(SPEC))).toBe(true)
    expect(regionGroups(SPEC, stockPlacement(SPEC), 2, stockShown)[0].ids).toEqual(SPEC.stockMerged)
  })
})

describe('a move', () => {
  it('up and down step within the column, past the panes that are hidden', () => {
    const hiddenRigscope = (id: PhonePanelId) => id !== 'rigscope'
    const place = movePane(SPEC, undefined, undefined, 'receiver', 'down', hiddenRigscope)!
    expect(placedColumns(SPEC, place).b).toEqual(['rigscope', 'transmitter', 'receiver', 'needed'])
    const back = movePane(SPEC, place, undefined, 'receiver', 'up', hiddenRigscope)!
    // Up from under Transmitter lands above it; the hidden rig scope strip is stepped over, not swapped.
    expect(placedColumns(SPEC, back).b).toEqual(['rigscope', 'receiver', 'transmitter', 'needed'])
  })

  it('does nothing at the top going up, at the foot going down, or with only hidden panes that way', () => {
    expect(movePane(SPEC, undefined, undefined, 'bandActivity', 'up', all)).toBeNull()
    expect(movePane(SPEC, undefined, undefined, 'needed', 'down', all)).toBeNull()
    expect(movePane(SPEC, undefined, undefined, 'transmitter', 'down', (id) => id !== 'needed')).toBeNull()
  })

  it('left and right go to the neighbouring column on screen, at its foot', () => {
    const toA = movePane(SPEC, undefined, undefined, 'transmitter', 'left', all)!
    expect(placedColumns(SPEC, toA).a).toEqual(['bandActivity', 'voiceKeyer', 'spots', ...BOX_IDS, 'transmitter'])
    const toLog = movePane(SPEC, undefined, undefined, 'needed', 'right', all)!
    expect(placedColumns(SPEC, toLog).log).toEqual(['needed'])
    expect(movePane(SPEC, undefined, undefined, 'bandActivity', 'left', all), 'no column left of a').toBeNull()
    expect(movePane(SPEC, toLog, undefined, 'needed', 'right', all), 'no column right of log').toBeNull()
    // The columns in another order on screen: left and right follow it.
    const swapped = movePane(SPEC, undefined, ['b', 'a', 'log'], 'bandActivity', 'left', all)!
    expect(placedColumns(SPEC, swapped).b).toContain('bandActivity')
  })

  it('never takes a pinned pane out of its column — the voice keyer only moves up and down', () => {
    expect(movePane(SPEC, undefined, undefined, 'voiceKeyer', 'left', all)).toBeNull()
    expect(movePane(SPEC, undefined, undefined, 'voiceKeyer', 'right', all)).toBeNull()
    const up = movePane(SPEC, undefined, undefined, 'voiceKeyer', 'up', all)!
    expect(placedColumns(SPEC, up).a).toEqual(['voiceKeyer', 'bandActivity', 'spots', ...BOX_IDS])
  })

  it('writes every pane’s place, so only a pane added later is ever missing from the record', () => {
    const place = movePane(SPEC, undefined, undefined, 'spots', 'up', all)!
    expect(Object.keys(place).sort()).toEqual([...arrangeIds(SPEC)].sort())
  })
})

// ── the panel record ───────────────────────────────────────────────────────────────────────

describe('the panel record carries the placement', () => {
  it('keeps a Phone placement through coercion, and drops one no vocabulary arranges', () => {
    const place: PanePlacement<PhonePanelId> = { receiver: { col: 'a', order: 0 } }
    expect(coercePanelLayout(PHONE_PANELS, { v: 2, state: {}, share: {}, place }).place).toEqual(place)
    // RTTY has no pane grid to arrange: a place in its record is not read.
    expect(coercePanelLayout(RTTY_PANELS, { v: 2, state: {}, share: {}, place: { stream: { col: 'b', order: 0 } } }).place).toBeUndefined()
  })

  it('a record written before L3 (v1, or v2 with column widths) opens on the stock grouping with everything else kept', () => {
    const v1 = coercePanelLayout(PHONE_PANELS, { v: 1, state: { receiver: 'removed' }, share: { spots: 1.2 } })
    expect(v1).toEqual({ v: 2, state: { receiver: 'removed' }, share: { spots: 1.2 } })
    const l2 = coercePanelLayout(PHONE_PANELS, { v: 2, state: {}, share: {}, cols: { log: 480 } })
    expect(l2).toEqual({ v: 2, state: {}, share: {}, cols: { log: 480 } })
    expect(l2.place).toBeUndefined()
  })

  it('an OLDER build reading this record keeps what it knows and opens on its own grouping', () => {
    // The pre-L3 coercion, as shipped (L2: state, share and cols), reading a record L3 wrote: it never
    // reads `place` or `colOrder`, so the operator's arrangement is simply not applied there, and
    // nothing it does understand is lost.
    const olderBuild = (raw: { state?: object; share?: object; cols?: object }) => ({ v: 2, state: { ...raw.state }, share: { ...raw.share }, ...(raw.cols ? { cols: { ...raw.cols } } : {}) })
    const written: PanelLayout<PhonePanelId> = {
      v: 2,
      state: { spots: 'docked' },
      share: { spots: 1.1 },
      cols: { log: 500 },
      place: movePane(SPEC, undefined, undefined, 'receiver', 'left', all)!,
      colOrder: ['a', 'b', 'log'],
    }
    const read = olderBuild(JSON.parse(JSON.stringify(written)))
    expect(read).toEqual({ v: 2, state: { spots: 'docked' }, share: { spots: 1.1 }, cols: { log: 500 } })
  })
})

describe('the hook moves a pane in one undoable step', () => {
  const KEY = panelStorageKey('phone', 'main')
  beforeEach(() => localStorage.clear())
  const hook = () => renderHook(() => usePanelLayout(PHONE_PANELS, 'main'))

  it('stores the move, and Undo puts the stock grouping back', () => {
    const { result } = hook()
    act(() => result.current.movePane!('receiver', 'left', all))
    expect(placedColumns(SPEC, result.current.layout.place).a).toContain('receiver')
    expect(JSON.parse(localStorage.getItem(KEY)!).place.receiver.col).toBe('a')
    expect(result.current.canUndo).toBe(true)
    act(() => result.current.undo())
    expect(result.current.layout.place).toBeUndefined()
  })

  it('a move that changes nothing is no step: it neither stores nor spends the one Undo', () => {
    const { result } = hook()
    act(() => result.current.movePane!('receiver', 'left', all))
    act(() => result.current.movePane!('voiceKeyer', 'right', all))
    // The Undo still reverts the receiver's move, not a no-op.
    act(() => result.current.undo())
    expect(result.current.layout.place).toBeUndefined()
  })

  it('Reset layout puts the stock grouping back, and a column width set later keeps the placement', () => {
    const { result } = hook()
    act(() => result.current.movePane!('needed', 'right', all))
    act(() => result.current.setCols!({ log: 520 }))
    expect(result.current.layout.place?.needed?.col).toBe('log')
    expect(result.current.layout.cols).toEqual({ log: 520 })
    act(() => result.current.reset())
    expect(result.current.layout.place).toBeUndefined()
    expect(result.current.layout.cols).toBeUndefined()
  })

  it('◀ ▶ follow the columns ON SCREEN: a stored column order, which no cockpit renders yet, does not steer them', () => {
    localStorage.setItem(KEY, JSON.stringify({ v: 2, state: {}, share: {}, colOrder: ['b', 'a', 'log'] }))
    const { result } = hook()
    expect(result.current.layout.colOrder, 'the record keeps it').toEqual(['b', 'a', 'log'])
    // On screen Band Activity is in the left-most column, so there is no column to its left.
    act(() => result.current.movePane!('bandActivity', 'left', all))
    expect(result.current.layout.place).toBeUndefined()
    act(() => result.current.movePane!('bandActivity', 'right', all))
    expect(placedColumns(SPEC, result.current.layout.place).b).toContain('bandActivity')
  })

  it('only a vocabulary that arranges offers a move at all', () => {
    const { result } = renderHook(() => usePanelLayout(RTTY_PANELS, 'main'))
    expect(result.current.movePane).toBeUndefined()
  })
})

describe('Phone’s arrange list', () => {
  it('lists every region pane of its vocabulary exactly once, and nothing that is not a region pane', () => {
    const ids = arrangeIds(SPEC)
    expect(new Set(ids).size).toBe(ids.length)
    const region = PHONE_PANELS.panelIds.filter((id) => id !== 'scope' && id !== 'txmeters')
    expect([...ids].sort()).toEqual([...region].sort())
  })

  it('pins the voice keyer, and its stock merged order is every pane of its own once', () => {
    expect(SPEC.pinned).toEqual(['voiceKeyer'])
    // The boxes are not in it: each joins the merged column where it stands (regionGroups).
    expect(SPEC.boxes).toEqual([...BOX_IDS])
    const own = [...SPEC.columns.a, ...SPEC.columns.b].filter((id) => !SPEC.boxes!.includes(id))
    expect([...(SPEC.stockMerged ?? [])].sort()).toEqual(own.sort())
  })

  it('a spec whose pinned pane moved columns would be caught by the coercion (control)', () => {
    const spec: ArrangeSpec<'x' | 'y'> = { columns: { a: ['x'], b: ['y'], log: [] }, pinned: ['y'] }
    expect(coercePlacement(spec, { y: { col: 'a', order: 0 } })).toEqual({ y: { col: 'b', order: 0 } })
  })
})
