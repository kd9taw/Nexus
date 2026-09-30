// @vitest-environment jsdom
//
// A CONNECT BOX'S OWN TEXT SIZE (⋯ ▸ A− / A+) — the record half. The factor lives in the panel
// record beside the splits, per slot, and every way into the record holds it to the range the
// menu writes: 80–160 % of the app's Text size. The rendered half (every box's text really grows)
// is computed against the sheet in components/connect/PaneFrame.textScale.test.tsx.
import { describe, it, expect, beforeEach } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import {
  BOX_SCALE_MAX,
  BOX_SCALE_MIN,
  CONNECT_PANELS,
  coercePanelLayout,
  loadPanelLayout,
  panelStorageKey,
  savePanelLayout,
  usePanelLayout,
} from './panelState'

const KEY = panelStorageKey('connect')

beforeEach(() => {
  localStorage.clear()
})

describe('the record keeps a box text size per slot', () => {
  it('an older record — every one saved before A− / A+ — loads every box at the app’s size', () => {
    const older = { v: 2, state: { left1: 'removed' }, share: { right1: 1.2 } }
    const got = coercePanelLayout(CONNECT_PANELS, older)
    expect(got.scale, 'no text size stored means none read').toBeUndefined()
    expect(got.state).toEqual({ left1: 'removed' })
    expect(got.share).toEqual({ right1: 1.2 })
  })

  it('keeps a stored size, clamps one outside 80–160 % into range, and drops junk', () => {
    const got = coercePanelLayout(CONNECT_PANELS, {
      v: 2,
      state: {},
      share: {},
      scale: { left1: 1.3, left2: 2.5, right1: 0.5, right2: 'big', bottom1: -1, bottom2: 1, bottom3: Number.NaN, nowhere: 1.2 },
    })
    expect(got.scale).toEqual({ left1: 1.3, left2: BOX_SCALE_MAX, right1: BOX_SCALE_MIN })
  })

  it('the range is the operator’s pick: 80 % to 160 %', () => {
    expect([BOX_SCALE_MIN, BOX_SCALE_MAX]).toEqual([0.8, 1.6])
  })

  it('a record whose sizes are all 100 % stores none (the stock record stays the stock record)', () => {
    const got = coercePanelLayout(CONNECT_PANELS, { v: 2, state: {}, share: {}, scale: { left1: 1 } })
    expect(got.scale).toBeUndefined()
  })

  it('a size survives a save and a reload', () => {
    savePanelLayout(KEY, { v: 2, state: {}, share: {}, scale: { bottom2: 1.4 } })
    expect(loadPanelLayout(CONNECT_PANELS).scale).toEqual({ bottom2: 1.4 })
  })
})

describe('usePanelLayout: scaleOf / setScale', () => {
  it('reads 1 for a box nobody sized, and a step lands exactly (no float drift from repeated steps)', () => {
    const { result } = renderHook(() => usePanelLayout(CONNECT_PANELS))
    expect(result.current.scaleOf('left1')).toBe(1)
    // Three A+ presses from 100 %: 1 + 0.1 + 0.1 + 0.1 is 1.3000000000000003 in floating point.
    act(() => result.current.setScale('left1', 1 + 0.1 + 0.1 + 0.1))
    expect(result.current.scaleOf('left1')).toBe(1.3)
    expect(JSON.parse(localStorage.getItem(KEY)!).scale).toEqual({ left1: 1.3 })
  })

  it('clamps what it writes, and 100 % clears the entry', () => {
    const { result } = renderHook(() => usePanelLayout(CONNECT_PANELS))
    act(() => result.current.setScale('right2', 3))
    expect(result.current.scaleOf('right2')).toBe(BOX_SCALE_MAX)
    act(() => result.current.setScale('right2', 0.1))
    expect(result.current.scaleOf('right2')).toBe(BOX_SCALE_MIN)
    act(() => result.current.setScale('right2', 1))
    expect(result.current.scaleOf('right2')).toBe(1)
    expect(JSON.parse(localStorage.getItem(KEY)!).scale, '100 % is no entry at all').toBeUndefined()
  })

  it('leaves the rest of the record alone, and is ONE undoable step like a split', () => {
    const { result } = renderHook(() => usePanelLayout(CONNECT_PANELS))
    act(() => result.current.setPanelState('bottom3', 'removed'))
    act(() => result.current.setShares({ left1: 1.2, left2: 0.8 }))
    act(() => result.current.setScale('left1', 1.5))
    expect(result.current.layout.state).toEqual({ bottom3: 'removed' })
    expect(result.current.layout.share).toEqual({ left1: 1.2, left2: 0.8 })
    act(() => result.current.undo())
    expect(result.current.scaleOf('left1'), 'Undo takes the size back').toBe(1)
    expect(result.current.layout.share, 'and nothing else').toEqual({ left1: 1.2, left2: 0.8 })
  })

  it('⊞ Reset puts every box back at the app’s size', () => {
    const { result } = renderHook(() => usePanelLayout(CONNECT_PANELS))
    act(() => result.current.setScale('left1', 1.5))
    act(() => result.current.reset())
    expect(result.current.scaleOf('left1')).toBe(1)
  })

  it('setLayout (a layout preset) keeps a size it is handed, coerced like a load', () => {
    const { result } = renderHook(() => usePanelLayout(CONNECT_PANELS))
    act(() => result.current.setLayout({ v: 2, state: { bottom1: 'removed' }, share: {}, scale: { right1: 9 } }))
    expect(result.current.scaleOf('right1')).toBe(BOX_SCALE_MAX)
  })
})
