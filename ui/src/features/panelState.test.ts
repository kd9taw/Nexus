// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import {
  OPERATE_PANELS,
  WATERFALL_DETACHED_KEY,
  coercePanelLayout,
  loadPanelLayout,
  panelStorageKey,
  redockStalePopouts,
  savePanelLayout,
  usePanelLayout,
  type PanelLayout,
  type OperatePanelId,
} from './panelState'
import { scopedKey, windowInstance } from './windowScope'

const KEY = panelStorageKey('operate')

beforeEach(() => {
  localStorage.clear()
})

describe('panel storage key', () => {
  it('is surface-scoped: nexus.panels.<view>.<instance>', () => {
    // jsdom has no ?panel= in the URL, so this window is the main surface.
    expect(windowInstance()).toBe('main')
    expect(KEY).toBe('nexus.panels.operate.main')
    // A torn-off surface gets its OWN record — the docked/popped collision the
    // app-global nexus.waterfall.detached flag used to have.
    expect(panelStorageKey('operate', 'w1')).toBe('nexus.panels.operate.w1')
    expect(scopedKey('nexus.panels.operate', 'global', 'w1')).toBe('nexus.panels.operate')
  })
})

describe('coercePanelLayout', () => {
  it('treats an absent panel as docked (a panel added later ships visible)', () => {
    const l = loadPanelLayout(OPERATE_PANELS)
    expect(l.state.waterfall).toBeUndefined()
    expect(l.state.bandActivity).toBeUndefined()
  })

  it('coerces junk to the stock layout instead of throwing', () => {
    for (const junk of [null, 42, 'nope', [], { state: 7, share: 'x' }]) {
      expect(coercePanelLayout(OPERATE_PANELS, junk)).toEqual({ v: 1, state: {}, share: {} })
    }
  })

  it('recovers the stock layout from an unparseable stored record', () => {
    localStorage.setItem(KEY, '{not json')
    expect(loadPanelLayout(OPERATE_PANELS)).toEqual({ v: 1, state: {}, share: {} })
  })

  it('drops unknown panel ids, unknown states, and non-positive shares', () => {
    const l = coercePanelLayout(OPERATE_PANELS, {
      v: 1,
      state: { waterfall: 'removed', stopTx: 'removed', bandActivity: 'gone' },
      share: { waterfall: 0.4, rxfreq: -1, stations: 'big', callRoster: Infinity },
    })
    expect(l.state).toEqual({ waterfall: 'removed' })
    expect(l.share).toEqual({ waterfall: 0.4 })
    // The whitelist is the vocabulary, so a hand-edited store cannot introduce an id
    // for a TX control that has no panel entry.
    expect('stopTx' in l.state).toBe(false)
  })
})

describe('persistence', () => {
  it('an explicit removal survives a reload', () => {
    const stored: PanelLayout<OperatePanelId> = {
      v: 1,
      state: { waterfall: 'removed' },
      share: {},
    }
    savePanelLayout(KEY, stored)
    expect(loadPanelLayout(OPERATE_PANELS).state.waterfall).toBe('removed')
  })
})

describe('nexus.waterfall.detached migration', () => {
  it('carries a popped-out waterfall into the record', () => {
    localStorage.setItem(WATERFALL_DETACHED_KEY, '1')
    expect(loadPanelLayout(OPERATE_PANELS).state.waterfall).toBe('popped')
    // …and persists it, so the bridge is not needed a second time.
    expect(JSON.parse(localStorage.getItem(KEY)!).state.waterfall).toBe('popped')
  })

  it('runs exactly once — a re-dock is never undone by the stale global flag', () => {
    localStorage.setItem(WATERFALL_DETACHED_KEY, '1')
    expect(loadPanelLayout(OPERATE_PANELS).state.waterfall).toBe('popped')
    // Operator re-docks (record back to stock) while the legacy flag is still '1'.
    localStorage.removeItem(KEY)
    expect(loadPanelLayout(OPERATE_PANELS).state.waterfall).toBeUndefined()
  })

  it('leaves the record alone when the flag was never set', () => {
    expect(loadPanelLayout(OPERATE_PANELS).state.waterfall).toBeUndefined()
    expect(localStorage.getItem(KEY)).toBeNull()
  })
})

describe('redockStalePopouts (fresh main-window boot)', () => {
  it('re-docks a stale pop-out but leaves an explicit removal alone', () => {
    savePanelLayout(KEY, {
      v: 1,
      state: { waterfall: 'popped', stations: 'removed' },
      share: {},
    } as PanelLayout<OperatePanelId>)
    redockStalePopouts(OPERATE_PANELS)
    const l = loadPanelLayout(OPERATE_PANELS)
    // No detached window survives a restart, so 'popped' would strand the operator on a
    // re-dock bar with nothing behind it.
    expect(l.state.waterfall).toBe('docked')
    expect(l.state.stations).toBe('removed')
  })

  it('does not write when there is nothing stale', () => {
    redockStalePopouts(OPERATE_PANELS)
    expect(localStorage.getItem(KEY)).toBeNull()
  })
})

describe('usePanelLayout', () => {
  it('saves synchronously on change, so a remount keeps the removal', () => {
    const { result, unmount } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    expect(result.current.stateOf('waterfall')).toBe('docked')
    act(() => result.current.setPanelState('waterfall', 'removed'))
    // Written by the state updater itself — not by an effect that a remount could skip.
    expect(JSON.parse(localStorage.getItem(KEY)!).state.waterfall).toBe('removed')
    unmount()
    const again = renderHook(() => usePanelLayout(OPERATE_PANELS))
    expect(again.result.current.stateOf('waterfall')).toBe('removed')
  })

  it('undo restores the previous layout, once', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    expect(result.current.canUndo).toBe(false)
    act(() => result.current.setPanelState('rxfreq', 'removed'))
    expect(result.current.canUndo).toBe(true)
    act(() => result.current.undo())
    expect(result.current.stateOf('rxfreq')).toBe('docked')
    expect(result.current.canUndo).toBe(false)
  })

  it('reset puts every panel back and is itself undoable', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setPanelState('waterfall', 'removed'))
    act(() => result.current.setPanelState('stations', 'removed'))
    act(() => result.current.reset())
    expect(result.current.stateOf('waterfall')).toBe('docked')
    expect(result.current.stateOf('stations')).toBe('docked')
    act(() => result.current.undo())
    expect(result.current.stateOf('waterfall')).toBe('removed')
    expect(result.current.stateOf('stations')).toBe('removed')
  })
})
