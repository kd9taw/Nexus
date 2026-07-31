// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import {
  MIN_SHARE,
  OPERATE_PANELS,
  WATERFALL_DETACHED_KEY,
  coercePanelLayout,
  loadPanelLayout,
  panelStorageKey,
  redockStalePopouts,
  savePanelLayout,
  seamShares,
  usePanelLayout,
  SSTV_PANELS,
  PHONE_PANELS,
  CW_PANELS,
  RTTY_PANELS,
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

  it('clamps a loaded share into [MIN_SHARE, 2 − MIN_SHARE] — the range the writers enforce', () => {
    // setShare/setShares floor at MIN_SHARE and seamShares caps at 2 − MIN_SHARE, but
    // load accepted any v > 0 — so a hand-edited/foreign 1e-9 collapsed a pane to ~0
    // height on the one path the setters cannot guard.
    const l = coercePanelLayout(OPERATE_PANELS, {
      v: 1,
      state: {},
      share: { waterfall: 1e-9, bandActivity: 50, callRoster: 1.2 },
    })
    expect(l.share.waterfall).toBe(MIN_SHARE)
    expect(l.share.bandActivity).toBe(2 - MIN_SHARE)
    expect(l.share.callRoster).toBe(1.2)
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

describe('share (seam resize)', () => {
  it('defaults to 1, setShare persists synchronously and clamps to MIN_SHARE', () => {
    const { result, unmount } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    expect(result.current.shareOf('rxfreq')).toBe(1)
    act(() => result.current.setShare('rxfreq', 1.6))
    expect(result.current.shareOf('rxfreq')).toBe(1.6)
    // Saved by the updater itself, so a remount keeps the size (the remount-loss bug class).
    expect(JSON.parse(localStorage.getItem(KEY)!).share.rxfreq).toBe(1.6)
    // A seam can never drive a pane below MIN_SHARE — removal is the only route to gone.
    act(() => result.current.setShare('rxfreq', 0))
    expect(result.current.shareOf('rxfreq')).toBe(MIN_SHARE)
    unmount()
    const again = renderHook(() => usePanelLayout(OPERATE_PANELS))
    expect(again.result.current.shareOf('rxfreq')).toBe(MIN_SHARE)
  })

  it('setShares redistributes two adjacent panes in ONE undoable step', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setShares({ bandActivity: 1.4, rxfreq: 0.6 }))
    expect(result.current.shareOf('bandActivity')).toBe(1.4)
    expect(result.current.shareOf('rxfreq')).toBe(0.6)
    // A seam drag is a single history entry — one undo restores BOTH panes.
    act(() => result.current.undo())
    expect(result.current.shareOf('bandActivity')).toBe(1)
    expect(result.current.shareOf('rxfreq')).toBe(1)
  })

  it('reset clears shares back to default', () => {
    const { result } = renderHook(() => usePanelLayout(OPERATE_PANELS))
    act(() => result.current.setShare('rxfreq', 1.8))
    act(() => result.current.reset())
    expect(result.current.shareOf('rxfreq')).toBe(1)
  })
})

describe('cockpit vocabularies (Phase 3 TX-safety)', () => {
  // TX-safety BY CONSTRUCTION: keying/stop/tune/arm/macro controls have NO panel id, so no menu
  // entry, stored value, or coercion rule can ever remove them from any cockpit.
  const txCritical = [
    'send', 'stop', 'stopTx', 'tune', 'arm', 'ptt', 'enableTx', 'txbar',
    'keyer', 'wpm', 'macros', 'macro', 'compose', 'send', 'header', 'band', 'mode',
  ]
  const cases: Array<[string, readonly string[]]> = [
    ['SSTV', SSTV_PANELS.panelIds],
    ['Phone', PHONE_PANELS.panelIds],
    ['CW', CW_PANELS.panelIds],
    ['RTTY', RTTY_PANELS.panelIds],
  ]
  it.each(cases)('%s vocabulary excludes every TX control', (_name, ids) => {
    for (const id of txCritical) {
      expect((ids as readonly string[]).includes(id)).toBe(false)
    }
  })

  it('lists the expected content panels per cockpit', () => {
    expect([...SSTV_PANELS.panelIds]).toEqual(['txcompose', 'gallery'])
    expect([...PHONE_PANELS.panelIds]).toEqual(['rigscope', 'txmeters', 'dsp', 'dspLevels', 'bandActivity'])
    expect([...RTTY_PANELS.panelIds]).toEqual(['stream'])
    expect([...CW_PANELS.panelIds]).toEqual([
      'scopeCtl', 'dsp', 'txmeters', 'rxdsp', 'bandActivity', 'copilot', 'decode', 'sent',
    ])
  })
})

describe('seamShares', () => {
  it('centres to the stock [1, 1]', () => {
    expect(seamShares(0.5)).toEqual([1, 1])
  })

  it('is monotonic — dragging down grows the pane above', () => {
    const [aboveLo] = seamShares(0.3)
    const [aboveHi] = seamShares(0.7)
    expect(aboveHi).toBeGreaterThan(aboveLo)
  })

  it('always sums to 2 (relative flex, so the region stays full)', () => {
    for (const f of [0, 0.1, 0.42, 0.5, 0.88, 1]) {
      const [a, b] = seamShares(f)
      expect(a + b).toBeCloseTo(2, 10)
    }
  })

  it('clamps both extremes so neither pane drops below MIN_SHARE', () => {
    const [aTop, bTop] = seamShares(0) // dragged fully up
    expect(aTop).toBeGreaterThanOrEqual(MIN_SHARE)
    expect(bTop).toBeGreaterThanOrEqual(MIN_SHARE)
    const [aBot, bBot] = seamShares(1) // dragged fully down
    expect(aBot).toBeGreaterThanOrEqual(MIN_SHARE)
    expect(bBot).toBeGreaterThanOrEqual(MIN_SHARE)
  })
})
