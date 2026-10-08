// @vitest-environment jsdom
//
// THE LEFT SIDE IN THE PANEL RECORD (operator's pick, 2026-10-03; Phone): what a record may carry,
// what an older build and an older record make of it, the hook's one-step moves, and the operator-data
// rules it keeps — the arrangement round-trips through the durable store (ui-state.json, which is
// per PROFILE and is the `uiState` section of Settings ▸ Backup), an old record loads unchanged, and
// nothing about a window's width ever rewrites it. Where the side renders is PhoneCockpit.arrange's.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { CW_PANELS, PHONE_PANELS, coercePanelLayout, loadPanelLayout, panelStorageKey, usePanelLayout, type PanelLayout, type PhonePanelId } from './panelState'
import { canMoveArranged, coerceLeftSide, moveArranged, placedColumns } from './panelPlace'
import { __resetDurableForTest, flushDurable, loadDurable } from './durableStore'

const mockLoad = vi.fn<() => Promise<Record<string, string> | null>>()
const mockSave = vi.fn<(s: Record<string, string>) => Promise<boolean>>()
vi.mock('../api', () => ({
  uiStateLoad: () => mockLoad(),
  uiStateSave: (s: Record<string, string>) => mockSave(s),
}))

const SPEC = PHONE_PANELS.arrange!
const all = () => true
const KEY = panelStorageKey('phone', 'main')

beforeEach(() => {
  localStorage.clear()
  __resetDurableForTest()
  mockLoad.mockReset()
  mockSave.mockReset()
  mockSave.mockResolvedValue(true)
})
afterEach(() => __resetDurableForTest())

describe('what a record may put on the left side', () => {
  it('only the panes the spec lists, each once, in the order stored', () => {
    expect(coerceLeftSide(SPEC, ['needed', 'bandActivity', 'needed', 'spots'])).toEqual(['needed', 'bandActivity', 'spots'])
  })

  it('never the voice keyer, a rig strip, the log, an id outside the vocabulary or a stop control', () => {
    expect(coerceLeftSide(SPEC, ['voiceKeyer', 'receiver', 'rigscope', 'log', 'scope', 'txmeters', 'ptt', 'stopTx', 'tune', 7, null])).toBeUndefined()
    expect(coerceLeftSide(SPEC, ['ptt', 'spots'])).toEqual(['spots'])
  })

  it('reads junk, or an empty list, as "nothing there"', () => {
    for (const raw of [undefined, null, 'spots', { spots: true }, [], 42]) expect(coerceLeftSide(SPEC, raw)).toBeUndefined()
  })

  it('Phone’s list is its three feeds and its six boxes, none of them pinned, all of them region panes', () => {
    expect([...SPEC.leftSide!].sort()).toEqual(['bandActivity', 'box1', 'box2', 'box3', 'box4', 'box5', 'box6', 'needed', 'spots'])
    for (const id of SPEC.leftSide!) {
      expect(SPEC.pinned, `${id} is pinned: a pane that changes parent at a window's width would be remounted`).not.toContain(id)
      expect([...SPEC.columns.a, ...SPEC.columns.b]).toContain(id)
    }
  })

  it('a cockpit with no left side carries none (CW, which arranges, reads none)', () => {
    expect(coercePanelLayout(CW_PANELS, { v: 2, state: {}, share: {}, leftSide: ['spots'] }).leftSide).toBeUndefined()
  })
})

describe('the panel record carries the left side and its width', () => {
  it('keeps both through coercion, with everything else, and rounds the width', () => {
    const got = coercePanelLayout(PHONE_PANELS, {
      v: 2,
      state: { spots: 'docked' },
      share: {},
      cols: { log: 480, leftSide: 401.6 },
      place: { needed: { col: 'a', order: 0 } },
      leftSide: ['needed', 'voiceKeyer', 'spots'],
    })
    expect(got.leftSide).toEqual(['needed', 'spots'])
    expect(got.cols).toEqual({ log: 480, leftSide: 402 })
    expect(got.place?.needed).toEqual({ col: 'a', order: 0 })
  })

  it('drops a width no divider could have written (zero, negative, not a number)', () => {
    for (const w of [0, -300, 'wide', NaN, Infinity]) {
      expect(coercePanelLayout(PHONE_PANELS, { v: 2, state: {}, share: {}, cols: { leftSide: w } }).cols).toBeUndefined()
    }
  })

  it('AN OLD RECORD LOADS UNCHANGED: a record from before the left side is exactly what it was', () => {
    const old = { v: 2, state: { receiver: 'removed', spots: 'docked' }, share: { spots: 1.2 }, cols: { log: 480 }, place: { receiver: { col: 'a', order: 0 } } }
    localStorage.setItem(KEY, JSON.stringify(old))
    const got = loadPanelLayout(PHONE_PANELS, 'main')
    expect(got).toEqual({ ...old, place: { receiver: { col: 'a', order: 0 } } })
    expect(got.leftSide).toBeUndefined()
    expect(got.cols?.leftSide).toBeUndefined()
  })

  it('an OLDER build reading a record with a left side opens those panes in their columns and loses nothing it knows', () => {
    // The L3 coercion, as shipped: state, share, cols (a/b/log) and the placement — never `leftSide`.
    const written: PanelLayout<PhonePanelId> = {
      v: 2,
      state: { spots: 'docked' },
      share: {},
      cols: { log: 500, leftSide: 360 },
      place: { spots: { col: 'b', order: 0 } },
      leftSide: ['spots'],
    }
    const raw = JSON.parse(JSON.stringify(written))
    const olderBuild = { v: 2, state: raw.state, share: raw.share, cols: { log: raw.cols.log }, place: raw.place }
    expect(olderBuild).toEqual({ v: 2, state: { spots: 'docked' }, share: {}, cols: { log: 500 }, place: { spots: { col: 'b', order: 0 } } })
    // …so Spots stands in column 2 there: its place, which this build keeps for exactly that.
    expect(placedColumns(SPEC, olderBuild.place as PanelLayout<PhonePanelId>['place']).b[0]).toBe('spots')
  })
})

describe('a move with the left side', () => {
  const shown = (id: PhonePanelId) => ['bandActivity', 'voiceKeyer', 'spots', 'needed', 'receiver', 'transmitter'].includes(id)

  it('◀ in column 1 puts a listed pane at the foot of the side, and keeps its place for ▶', () => {
    const a = moveArranged(SPEC, {}, 'bandActivity', 'left', shown, true)!
    expect(a.leftSide).toEqual(['bandActivity'])
    expect(a.place).toBeUndefined()
    const b = moveArranged(SPEC, a, 'spots', 'left', shown, true)!
    expect(b.leftSide).toEqual(['bandActivity', 'spots'])
    const back = moveArranged(SPEC, b, 'bandActivity', 'right', shown, true)!
    expect(back.leftSide).toEqual(['spots'])
    expect(placedColumns(SPEC, back.place).a[0], 'back where it stood').toBe('bandActivity')
  })

  it('▲ ▼ step among the panes on the side, past a hidden one; ◀ there does nothing', () => {
    const arr = { leftSide: ['spots', 'bandActivity', 'needed'] as PhonePanelId[] }
    expect(moveArranged(SPEC, arr, 'needed', 'up', (id) => id !== 'bandActivity', true)!.leftSide).toEqual(['needed', 'spots', 'bandActivity'])
    expect(moveArranged(SPEC, arr, 'spots', 'down', shown, true)!.leftSide).toEqual(['bandActivity', 'spots', 'needed'])
    expect(moveArranged(SPEC, arr, 'spots', 'up', shown, true)).toBeNull()
    expect(moveArranged(SPEC, arr, 'needed', 'left', shown, true)).toBeNull()
  })

  it('never a pane the spec does not list, and never from column 2 straight to the side', () => {
    expect(moveArranged(SPEC, {}, 'voiceKeyer', 'left', shown, true)).toBeNull()
    // Receiver in column 1 still cannot go: not listed.
    const inA = moveArranged(SPEC, {}, 'receiver', 'left', shown, true)!
    expect(placedColumns(SPEC, inA.place).a).toContain('receiver')
    expect(moveArranged(SPEC, inA, 'receiver', 'left', shown, true)).toBeNull()
    // Needed stands in column 2: ◀ takes it to column 1 first, like any pane.
    const n = moveArranged(SPEC, {}, 'needed', 'left', shown, true)!
    expect(n.leftSide).toBeUndefined()
    expect(placedColumns(SPEC, n.place).a).toContain('needed')
  })

  it('a column move steps past the panes on the side, which are not in the column on screen', () => {
    // Column 1 on screen: Voice Keyer, Spots (Band Activity is on the side).
    const arr = { leftSide: ['bandActivity'] as PhonePanelId[] }
    const up = moveArranged(SPEC, arr, 'voiceKeyer', 'up', shown, true)
    expect(up, 'the keyer is already at the top of column 1 on screen').toBeNull()
    expect(canMoveArranged(SPEC, arr, 'voiceKeyer', 'down', shown, true)).toBe(true)
  })

  it('WITHOUT ROOM FOR THE SIDE no move reaches it: its panes move in their columns, the side is kept as stored', () => {
    const arr = { leftSide: ['bandActivity', 'spots'] as PhonePanelId[] }
    for (const id of SPEC.leftSide!) {
      for (const move of ['up', 'down', 'left', 'right'] as const) {
        const next = moveArranged(SPEC, arr, id, move, shown, false)
        if (next) expect(next.leftSide, `${id} ${move}`).toBe(arr.leftSide)
      }
    }
    expect(moveArranged(SPEC, arr, 'bandActivity', 'left', shown, false), 'there is nothing left of column 1 on a narrow window').toBeNull()
    expect(moveArranged(SPEC, arr, 'bandActivity', 'down', shown, false)!.place, 'it moved in its column').toBeDefined()
  })
})

describe('the hook: one undoable step, and Reset', () => {
  const hook = () => renderHook(() => usePanelLayout(PHONE_PANELS, 'main'))

  it('stores a move to the side, Undo takes it back, Reset clears the side and its width', () => {
    const { result } = hook()
    act(() => result.current.movePane!('bandActivity', 'left', all, true))
    expect(JSON.parse(localStorage.getItem(KEY)!).leftSide).toEqual(['bandActivity'])
    act(() => result.current.undo())
    expect(result.current.layout.leftSide).toBeUndefined()
    act(() => result.current.movePane!('bandActivity', 'left', all, true))
    act(() => result.current.setCols!({ leftSide: 380 }))
    expect(result.current.layout.leftSide, 'a width set later keeps the side').toEqual(['bandActivity'])
    expect(result.current.layout.cols).toEqual({ leftSide: 380 })
    act(() => result.current.reset())
    expect(result.current.layout.leftSide).toBeUndefined()
    expect(result.current.layout.cols).toBeUndefined()
  })

  it('a move with no room for the side is no step at all toward it (the CW and JS8 callers pass none)', () => {
    const { result } = hook()
    act(() => result.current.movePane!('bandActivity', 'left', all))
    expect(result.current.canUndo).toBe(false)
    expect(localStorage.getItem(KEY)).toBeNull()
  })

  it('the last pane off the side leaves no empty list behind', () => {
    const { result } = hook()
    act(() => result.current.movePane!('spots', 'left', all, true))
    act(() => result.current.movePane!('spots', 'right', all, true))
    expect('leftSide' in result.current.layout).toBe(false)
  })
})

describe('THE PROFILE ROUND TRIP: the arrangement survives ui-state.json, and so a profile and a backup', () => {
  // ui-state.json is per profile (it sits beside settings.json in the profile's config directory) and
  // is the `uiState` section Settings ▸ Backup exports and restores, string for string. So the round
  // trip that matters is through the durable store: written by the hook, flushed to the station,
  // loaded back on the next launch, and read by the cockpit's own loader.
  it('a left side and its width written in one session open the same in the next', async () => {
    mockLoad.mockResolvedValue({})
    await loadDurable()
    const { result, unmount } = renderHook(() => usePanelLayout(PHONE_PANELS, 'main'))
    act(() => result.current.setPanelState('spots', 'docked'))
    act(() => result.current.movePane!('spots', 'left', all, true))
    act(() => result.current.movePane!('bandActivity', 'left', all, true))
    act(() => result.current.setCols!({ leftSide: 412 }))
    const before = result.current.layout
    unmount()
    await flushDurable()
    const saved = mockSave.mock.calls[mockSave.mock.calls.length - 1][0]
    expect(Object.keys(saved), 'the main window’s Phone record is not in ui-state.json').toContain(KEY)
    expect(JSON.parse(saved[KEY]).leftSide).toEqual(['spots', 'bandActivity'])

    // The next launch: a fresh webview (no localStorage), ui-state.json as the station kept it.
    localStorage.clear()
    __resetDurableForTest()
    mockLoad.mockResolvedValue({ ...saved })
    await loadDurable()
    const after = loadPanelLayout(PHONE_PANELS, 'main')
    expect(after).toEqual(before)
    expect(after.leftSide).toEqual(['spots', 'bandActivity'])
    expect(after.cols?.leftSide).toBe(412)
  })

  it('control: a pop-out’s record is not in ui-state.json, so this is the main window’s round trip and only that', async () => {
    mockLoad.mockResolvedValue({})
    await loadDurable()
    const { result } = renderHook(() => usePanelLayout(PHONE_PANELS, 'panel-2'))
    act(() => result.current.movePane!('spots', 'left', all, true))
    await flushDurable()
    const calls = mockSave.mock.calls
    expect(calls.length === 0 || !(panelStorageKey('phone', 'panel-2') in calls[calls.length - 1][0])).toBe(true)
  })
})
