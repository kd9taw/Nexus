// @vitest-environment jsdom
//
// A PANE DROPPED BY DRAG (⊞ Arrange by drag, 2026-10-08): the drop is the arrows' own moves, run to the place
// the pane was dropped, so it writes exactly the record the arrows would. Checked BY VALUE against the arrows:
// for every cockpit that arranges, each of FT's layouts with its side rail on either side, Phone with its left
// side on and off, many arrangements and visibilities, every pane and every arrow — the drop onto the place
// the arrow puts the pane gives the arrow's arrangement, field for field. Then hand-worked drops that need
// several arrows, the drops no arrow can make (null), and the hook's one undoable step into the same record.
import { describe, it, expect, beforeEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import {
  PANE_COLUMNS,
  arrangeIds,
  columnsOf,
  dropArranged,
  moveArranged,
  placedColumns,
  type ArrangeSpec,
  type Arrangement,
  type PaneColumn,
  type PaneDrop,
  type PaneMove,
} from './panelPlace'
import {
  CW_PANELS,
  JS8_PANELS,
  OPERATE_ARRANGE,
  OPERATE_PANELS,
  PHONE_PANELS,
  panelStorageKey,
  usePanelLayout,
  type OperatePanelId,
  type PhonePanelId,
} from './panelState'

const MOVES: PaneMove[] = ['up', 'down', 'left', 'right']
const last = <T,>(xs: readonly T[]): T | undefined => xs[xs.length - 1]

function rng(seed: number) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

/** Where a pane stands, read independently of the code under test: on the side while the side shows and
 *  lists it, else in its column. */
function areaOf<P extends string>(spec: ArrangeSpec<P>, arr: Arrangement<P>, id: P, sideShows: boolean): PaneColumn | 'side' {
  if (sideShows && spec.leftSide && (arr.leftSide ?? []).includes(id)) return 'side'
  const cols = placedColumns(spec, arr.place)
  return PANE_COLUMNS.find((c) => cols[c].includes(id))!
}
/** The panes on screen in a place, top to bottom. */
function inArea<P extends string>(spec: ArrangeSpec<P>, arr: Arrangement<P>, area: PaneColumn | 'side', shown: (id: P) => boolean, sideShows: boolean): P[] {
  const side = sideShows && spec.leftSide ? (arr.leftSide ?? []) : []
  if (area === 'side') return side.filter(shown)
  return placedColumns(spec, arr.place)[area].filter((x) => shown(x) && !side.includes(x))
}
/** The drop that names where `id` stands in `after`: its place, and the pane right below it there. */
function dropOnto<P extends string>(spec: ArrangeSpec<P>, after: Arrangement<P>, id: P, shown: (id: P) => boolean, sideShows: boolean): PaneDrop<P> {
  const area = areaOf(spec, after, id, sideShows)
  const list = inArea(spec, after, area, shown, sideShows)
  return { area, before: list[list.indexOf(id) + 1] ?? null }
}

interface Setting {
  name: string
  spec: ArrangeSpec<string>
  sideShows: boolean
  order?: readonly PaneColumn[]
}
const railLeft = (spec: ArrangeSpec<string>): PaneColumn[] => ['log', ...columnsOf(spec).filter((c) => c !== 'log')]
const SETTINGS: Setting[] = [
  { name: 'Phone', spec: PHONE_PANELS.arrange! as ArrangeSpec<string>, sideShows: false },
  { name: 'Phone, the left side showing', spec: PHONE_PANELS.arrange! as ArrangeSpec<string>, sideShows: true },
  { name: 'CW', spec: CW_PANELS.arrange! as ArrangeSpec<string>, sideShows: false },
  { name: 'JS8', spec: JS8_PANELS.arrange! as ArrangeSpec<string>, sideShows: false },
  { name: 'FT Classic', spec: OPERATE_ARRANGE.classic as ArrangeSpec<string>, sideShows: false },
  { name: 'FT Classic, rail on the left', spec: OPERATE_ARRANGE.classic as ArrangeSpec<string>, sideShows: false, order: railLeft(OPERATE_ARRANGE.classic as ArrangeSpec<string>) },
  { name: 'FT Roster', spec: OPERATE_ARRANGE.roster as ArrangeSpec<string>, sideShows: false },
  { name: 'FT Roster, rail on the left', spec: OPERATE_ARRANGE.roster as ArrangeSpec<string>, sideShows: false, order: railLeft(OPERATE_ARRANGE.roster as ArrangeSpec<string>) },
]

/** Arrangements to start from: the stock one, then ones the arrows built (the side in play where it shows). */
function starts(s: Setting, n: number, seed: number): Array<Arrangement<string>> {
  const next = rng(seed)
  const ids = arrangeIds(s.spec)
  const pick = <T,>(xs: readonly T[]) => xs[Math.floor(next() * xs.length)]
  const out: Array<Arrangement<string>> = [{}]
  while (out.length < n) {
    let arr: Arrangement<string> = {}
    for (let k = 1 + Math.floor(next() * 12); k > 0; k--) {
      arr = moveArranged(s.spec, arr, pick(ids), pick(MOVES), () => true, s.sideShows, s.order) ?? arr
    }
    out.push(arr)
  }
  return out
}
/** What is on screen: everything, then random halves (a hidden pane is stepped past by the arrows). */
function visibilities(s: Setting, n: number, seed: number): Array<(id: string) => boolean> {
  const next = rng(seed)
  const out: Array<(id: string) => boolean> = [() => true]
  while (out.length < n) {
    const off = new Set(arrangeIds(s.spec).filter(() => next() < 0.4))
    out.push((id) => !off.has(id))
  }
  return out
}

describe('a drop lands exactly where the arrow does, into the same arrangement', () => {
  it.each(SETTINGS.map((s) => [s.name, s] as const))('%s: every pane, every arrow, many arrangements', (_name, s) => {
    let checked = 0
    let multi = 0
    for (const arr of starts(s, 12, 20261008)) {
      for (const shown of visibilities(s, 4, 9_000 + checked)) {
        for (const id of arrangeIds(s.spec).filter(shown)) {
          for (const move of MOVES) {
            const arrow = moveArranged(s.spec, arr, id, move, shown, s.sideShows, s.order)
            if (!arrow) continue
            const drop = dropOnto(s.spec, arrow, id, shown, s.sideShows)
            const got = dropArranged(s.spec, arr, id, drop, shown, s.sideShows, s.order)
            expect(got, `${s.name}: ${id} ${move} from ${JSON.stringify(arr)}`).toEqual(arrow)
            checked++
          }
          // Every other place the pane can be dropped is a RUN of arrows: the drop equals that run, pressed.
          for (const area of [...columnsOf(s.spec), ...(s.sideShows ? (['side'] as const) : [])]) {
            const list = inArea(s.spec, arr, area, shown, s.sideShows).filter((x) => x !== id)
            for (const before of [...list, null]) {
              const got = dropArranged(s.spec, arr, id, { area, before }, shown, s.sideShows, s.order)
              if (!got) continue
              // The pane stands where it was dropped …
              const after = inArea(s.spec, got, area, shown, s.sideShows)
              expect(after[after.indexOf(id) + 1] ?? null, `${s.name}: ${id} dropped above ${before} in ${area}`).toBe(before)
              // … and nothing else on screen changed its place relative to the rest.
              for (const c of [...columnsOf(s.spec), ...(s.sideShows ? (['side'] as const) : [])]) {
                const was = inArea(s.spec, arr, c, shown, s.sideShows).filter((x) => x !== id)
                const now = inArea(s.spec, got, c, shown, s.sideShows).filter((x) => x !== id)
                expect(now, `${s.name}: dropping ${id} in ${area} reordered ${c}`).toEqual(was)
              }
              multi++
            }
          }
        }
      }
    }
    expect(checked, `${s.name}: the arrows moved nothing — the comparison is reading nothing`).toBeGreaterThan(100)
    expect(multi).toBeGreaterThan(checked)
  })
})

describe('drops that need several arrows, by hand', () => {
  const PHONE = PHONE_PANELS.arrange!
  const all = () => true
  it('Phone: the Transmitter from column 2 to the top of column 1 is ◀ then ▲ until it heads the column', () => {
    const got = dropArranged(PHONE, {}, 'transmitter', { area: 'a', before: 'bandActivity' }, all, false)!
    expect(placedColumns(PHONE, got.place).a.slice(0, 2)).toEqual(['transmitter', 'bandActivity'])
    expect(placedColumns(PHONE, got.place).b).toEqual(['rigscope', 'receiver', 'needed'])
    let arrows: Arrangement<PhonePanelId> = {}
    arrows = moveArranged(PHONE, arrows, 'transmitter', 'left', all, false)!
    while (placedColumns(PHONE, arrows.place).a[0] !== 'transmitter') arrows = moveArranged(PHONE, arrows, 'transmitter', 'up', all, false)!
    expect(got).toEqual(arrows)
  })
  it('FT Classic: Stations from the side rail to between Rx Frequency and the Tx messages is ◀ then ▲', () => {
    const CLASSIC = OPERATE_ARRANGE.classic
    const got = dropArranged(CLASSIC, {}, 'stations', { area: 'b', before: 'txmsgs' }, all, false)!
    expect(placedColumns(CLASSIC, got.place).b).toEqual(['rxfreq', 'stations', 'txmsgs'])
    expect(placedColumns(CLASSIC, got.place).log.slice(0, 1)).toEqual(['recall'])
  })
  it('FT Classic: Band Activity across two columns, into the rail, is ▶ ▶', () => {
    const CLASSIC = OPERATE_ARRANGE.classic
    const got = dropArranged(CLASSIC, {}, 'bandActivity', { area: 'log', before: null }, all, false)!
    let arrows: Arrangement<OperatePanelId> = moveArranged(CLASSIC, {}, 'bandActivity', 'right', all, false)!
    arrows = moveArranged(CLASSIC, arrows, 'bandActivity', 'right', all, false)!
    expect(got).toEqual(arrows)
  })
  it('FT Classic with the rail on the left: the rail is LEFT of column 1, so Band Activity gets there by ◀', () => {
    const CLASSIC = OPERATE_ARRANGE.classic
    const order: PaneColumn[] = ['log', 'a', 'b']
    const got = dropArranged(CLASSIC, {}, 'bandActivity', { area: 'log', before: 'stations' }, all, false, order)!
    expect(placedColumns(CLASSIC, got.place).log.slice(0, 3)).toEqual(['recall', 'bandActivity', 'stations'])
  })
  it('Phone: Needed from column 2 onto the left side is ◀ (to column 1) then ◀ (to the side)', () => {
    const got = dropArranged(PHONE, {}, 'needed', { area: 'side', before: null }, all, true)!
    expect(got.leftSide).toEqual(['needed'])
    // Its place in the columns is where the first ◀ left it: the foot of column 1, where ▶ brings it back.
    expect(last(placedColumns(PHONE, got.place).a)).toBe('needed')
  })
  it('Phone: a pane on the left side dropped into column 2 is ▶ (back to its column) and on from there', () => {
    const on: Arrangement<PhonePanelId> = { leftSide: ['spots'] }
    const got = dropArranged(PHONE, on, 'spots', { area: 'b', before: 'receiver' }, all, true)!
    expect(got.leftSide ?? []).toEqual([])
    expect(placedColumns(PHONE, got.place).b).toEqual(['rigscope', 'spots', 'receiver', 'transmitter', 'needed'])
  })
  it('hidden panes are stepped past exactly as the arrows step past them', () => {
    const shown = (id: PhonePanelId) => id !== 'receiver'
    const got = dropArranged(PHONE, {}, 'needed', { area: 'b', before: 'rigscope' }, shown, false)!
    // The hidden Receiver keeps its place among the others; the on-screen order is what was asked for.
    expect(placedColumns(PHONE, got.place).b.filter(shown)).toEqual(['needed', 'rigscope', 'transmitter'])
  })
})

describe('drops no run of arrows can make, and drops that change nothing, are null', () => {
  const PHONE = PHONE_PANELS.arrange!
  const all = () => true
  it('the pinned voice keyer never leaves its column; up and down in it, it drops like any pane', () => {
    expect(dropArranged(PHONE, {}, 'voiceKeyer', { area: 'b', before: null }, all, false)).toBeNull()
    expect(dropArranged(PHONE, {}, 'voiceKeyer', { area: 'side', before: null }, all, true)).toBeNull()
    const up = dropArranged(PHONE, {}, 'voiceKeyer', { area: 'a', before: 'bandActivity' }, all, false)!
    expect(placedColumns(PHONE, up.place).a.slice(0, 2)).toEqual(['voiceKeyer', 'bandActivity'])
  })
  it('the left side takes only the panes it lists, and only while it shows', () => {
    expect(dropArranged(PHONE, {}, 'receiver', { area: 'side', before: null }, all, true)).toBeNull()
    expect(dropArranged(PHONE, {}, 'spots', { area: 'side', before: null }, all, false)).toBeNull()
  })
  it('a drop where the pane already stands is no change', () => {
    expect(dropArranged(PHONE, {}, 'voiceKeyer', { area: 'a', before: 'spots' }, all, false)).toBeNull()
    expect(dropArranged(PHONE, {}, 'needed', { area: 'b', before: null }, all, false)).toBeNull()
    expect(dropArranged(PHONE, {}, 'receiver', { area: 'b', before: 'receiver' }, all, false)).toBeNull()
  })
  it('a drop above a pane that is hidden, in another place, or no pane of the cockpit is null', () => {
    const shown = (id: PhonePanelId) => id !== 'receiver'
    expect(dropArranged(PHONE, {}, 'needed', { area: 'b', before: 'receiver' }, shown, false)).toBeNull()
    expect(dropArranged(PHONE, {}, 'needed', { area: 'b', before: 'bandActivity' }, all, false)).toBeNull()
    expect(dropArranged(PHONE, {}, 'needed', { area: 'b', before: 'ptt' as PhonePanelId }, all, false)).toBeNull()
  })
  it('a pane that is not on screen, or not one the cockpit arranges, is dropped nowhere', () => {
    expect(dropArranged(PHONE, {}, 'receiver', { area: 'a', before: null }, (id) => id !== 'receiver', false)).toBeNull()
    expect(dropArranged(PHONE, {}, 'stopTx' as PhonePanelId, { area: 'a', before: null }, all, false)).toBeNull()
    expect(dropArranged(PHONE, {}, 'txmeters' as PhonePanelId, { area: 'a', before: null }, all, false)).toBeNull()
  })
  it('a column the layout does not have takes nothing (FT Roster has no column 2)', () => {
    expect(dropArranged(OPERATE_ARRANGE.roster, {}, 'rxfreq', { area: 'b', before: null }, all, false)).toBeNull()
  })
})

describe('the hook stores a drop in one undoable step, as the arrows store theirs', () => {
  beforeEach(() => localStorage.clear())
  const all = () => true
  it('Phone: the drop and the arrows that make it leave the same stored record', () => {
    const { result } = renderHook(() => usePanelLayout(PHONE_PANELS))
    act(() => result.current.dropPane!('transmitter', { area: 'a', before: 'voiceKeyer' }, all))
    const dropped = localStorage.getItem(panelStorageKey('phone'))
    expect(result.current.canUndo).toBe(true)
    act(() => result.current.undo())
    expect(result.current.layout.place).toBeUndefined()
    localStorage.clear()
    const arrows = renderHook(() => usePanelLayout(PHONE_PANELS))
    act(() => arrows.result.current.movePane!('transmitter', 'left', all))
    for (let i = 0; i < 20 && placedColumns(PHONE_PANELS.arrange!, arrows.result.current.layout.place).a.indexOf('transmitter') > 1; i++) {
      act(() => arrows.result.current.movePane!('transmitter', 'up', all))
    }
    expect(placedColumns(PHONE_PANELS.arrange!, arrows.result.current.layout.place).a.slice(0, 3)).toEqual(['bandActivity', 'transmitter', 'voiceKeyer'])
    expect(JSON.parse(dropped!)).toEqual(JSON.parse(localStorage.getItem(panelStorageKey('phone'))!))
  })
  it('a drop that changes nothing is no step: nothing stored, the one Undo not spent', () => {
    const { result } = renderHook(() => usePanelLayout(PHONE_PANELS))
    act(() => result.current.dropPane!('needed', { area: 'b', before: null }, all))
    expect(result.current.canUndo).toBe(false)
    expect(localStorage.getItem(panelStorageKey('phone'))).toBeNull()
  })
  it('FT: the drop goes into the layout it was made in (`places`), the other layout untouched', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.dropPane!('stations', { area: 'b', before: 'txmsgs' }, all, false, { layout: 'classic' }))
    expect(placedColumns(OPERATE_ARRANGE.classic, result.current.layout.places?.classic).b).toEqual(['rxfreq', 'stations', 'txmsgs'])
    expect(result.current.layout.places?.roster).toBeUndefined()
    expect(result.current.layout.place).toBeUndefined()
  })
  it('the left side: a drop there is stored in `leftSide`, and its place in the columns is the arrows’', () => {
    const { result } = renderHook(() => usePanelLayout(PHONE_PANELS))
    act(() => result.current.dropPane!('spots', { area: 'side', before: null }, all, true))
    expect(result.current.layout.leftSide).toEqual(['spots'])
    act(() => result.current.dropPane!('spots', { area: 'b', before: null }, all, true))
    expect(result.current.layout.leftSide).toBeUndefined()
    expect(last(placedColumns(PHONE_PANELS.arrange!, result.current.layout.place).b)).toBe('spots')
  })
  it('only a vocabulary that arranges offers a drop at all', () => {
    const { result } = renderHook(() => usePanelLayout({ view: 'nothing', panelIds: ['a'] as const }))
    expect(result.current.dropPane).toBeUndefined()
  })
})
