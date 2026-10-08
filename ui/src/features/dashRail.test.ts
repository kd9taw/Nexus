// @vitest-environment jsdom
//
// The dashboard rail's records (features/dashRail): off until the operator turns it on, remembered
// per section, the stock boxes, and a width that never leaves the cockpit narrower than on the
// 1024×768 floor window. Pure apart from storage.
import { describe, it, expect, beforeEach } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import {
  DASH_DEFAULT_SLOTS,
  DASH_RAIL_SECTIONS,
  DASH_RAIL_DEFAULT_PX,
  FLOOR_EFFECTIVE_W,
  coerceDashConfig,
  coerceDashSlots,
  coerceRailSections,
  dashRailInstance,
  fitRailWidth,
  loadDashConfig,
  loadDashSlots,
  loadRailSections,
  parseRailWidth,
  railWidthMax,
  slotsOf,
  stepRailWidth,
  useDashRailSections,
  useDashSlots,
} from './dashRail'
import { DASH_PANELS, panelStorageKey, usePanelLayout } from './panelState'
import { windowInstance } from './windowScope'
import { isDurable } from './durableStore'
import { RAIL_MAX, RAIL_MIN } from './connectRails'
import { pickInitialZoom } from '../useScale'

beforeEach(() => localStorage.clear())

describe('off until the operator turns it on, remembered per section', () => {
  it('every section is OFF with nothing stored — nobody’s cockpit narrows on update', () => {
    const { result } = renderHook(() => useDashRailSections())
    for (const s of DASH_RAIL_SECTIONS) expect(result.current.isOn(s), s).toBe(false)
  })

  it('turning it on for one section leaves every other section off, and survives a reload', () => {
    const { result } = renderHook(() => useDashRailSections())
    act(() => result.current.setOn('cw', true))
    expect(result.current.isOn('cw')).toBe(true)
    for (const s of DASH_RAIL_SECTIONS.filter((x) => x !== 'cw')) expect(result.current.isOn(s), s).toBe(false)
    // A fresh read of storage is the next launch.
    expect(loadRailSections()).toEqual({ cw: true })
    const again = renderHook(() => useDashRailSections())
    expect(again.result.current.isOn('cw')).toBe(true)
    act(() => again.result.current.setOn('cw', false))
    expect(loadRailSections()).toEqual({ cw: false })
  })

  it('a section that is not an operating cockpit is never on, whatever is stored', () => {
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ connect: true, chat: true, settings: true }))
    const { result } = renderHook(() => useDashRailSections())
    for (const v of ['connect', 'chat', 'settings', 'logbook']) expect(result.current.isOn(v), v).toBe(false)
  })

  it('a junk or foreign record reads as OFF, never as a surprise rail', () => {
    expect(coerceRailSections(null)).toEqual({})
    expect(coerceRailSections('on')).toEqual({})
    expect(coerceRailSections([true])).toEqual({})
    expect(coerceRailSections({ operate: 'yes', phone: 1, cw: true, bogus: true })).toEqual({ cw: true })
    localStorage.setItem('nexus.dashrail.sections', '{not json')
    expect(loadRailSections()).toEqual({})
  })
})

describe('the stock boxes, and a column that stays a permutation', () => {
  it('opens on the operator’s four: Clock, Bands for you, Space Wx, Getting Out', () => {
    expect(DASH_DEFAULT_SLOTS).toEqual({ rail1: 'clock', rail2: 'bandTiles', rail3: 'spacewx', rail4: 'getout' })
    expect(loadDashSlots()).toEqual(DASH_DEFAULT_SLOTS)
  })

  it('an unknown box or a duplicate is repaired on load, never a gap', () => {
    const c = coerceDashSlots({ rail1: 'nope', rail2: 'getout', rail3: 'getout' })
    expect(c.rail1).toBe('clock')
    expect(new Set(Object.values(c)).size).toBe(4)
  })

  it('picking a box already in another slot swaps the two, and the pick is stored', () => {
    const { result } = renderHook(() => useDashSlots())
    act(() => result.current.assignPane('operate', 'rail1', 'getout'))
    expect(result.current.slotsOf('operate')).toEqual({ rail1: 'getout', rail2: 'bandTiles', rail3: 'spacewx', rail4: 'clock' })
    expect(loadDashConfig().sections.operate).toEqual(result.current.slotsOf('operate'))
    act(() => result.current.resetSlots('operate'))
    expect(loadDashConfig().sections.operate).toEqual(DASH_DEFAULT_SLOTS)
  })
})

// THE RAIL PER COCKPIT (the operator's "Per cockpit", 2026-10-07: FT's rail can differ from Phone's, and each
// starts from today's rail). The placement record keeps the window's shared rail — the one every cockpit had
// before, and the only part an older build reads — and each cockpit's own once it changes its rail; the panel
// record (which slots show, their splits, their text sizes) is one per cockpit, reading the window's shared
// one until the cockpit has its own. Computed on the real records, as panelState.layouts.test.ts does FT's.
describe('each cockpit’s rail: its own boxes, starting from today’s rail', () => {
  const TODAY = { rail1: 'needed', rail2: 'clock', rail3: 'spacewx', rail4: 'pota' }
  const seedToday = () => localStorage.setItem('nexus.dashrail.config', JSON.stringify({ slots: TODAY }))

  it('a cockpit with no rail of its own shows today’s: every cockpit’s first open is the rail it had', () => {
    seedToday()
    const { result } = renderHook(() => useDashSlots())
    for (const section of DASH_RAIL_SECTIONS) expect(result.current.slotsOf(section), section).toEqual(TODAY)
  })

  it('a change is that cockpit’s own: FT’s rail and Phone’s differ, and today’s rail is written back as it was', () => {
    seedToday()
    const { result } = renderHook(() => useDashSlots())
    act(() => result.current.assignPane('operate', 'rail4', 'getout'))
    expect(result.current.slotsOf('operate')).toEqual({ ...TODAY, rail4: 'getout' })
    expect(result.current.slotsOf('phone'), 'a change in FT reached Phone').toEqual(TODAY)
    const stored = JSON.parse(localStorage.getItem('nexus.dashrail.config')!)
    expect(stored.slots, 'the shared rail moved: a cockpit opened later would not inherit today’s').toEqual(TODAY)
    expect(Object.keys(stored.sections)).toEqual(['operate'])
    // A reload reads both back.
    expect(slotsOf(loadDashConfig(), 'operate').rail4).toBe('getout')
    expect(slotsOf(loadDashConfig(), 'cw')).toEqual(TODAY)
  })

  it('Reset and Undo’s restore are a cockpit’s own, and Reset is the stock four, not today’s rail', () => {
    seedToday()
    const { result } = renderHook(() => useDashSlots())
    act(() => result.current.resetSlots('phone'))
    expect(result.current.slotsOf('phone')).toEqual(DASH_DEFAULT_SLOTS)
    expect(result.current.slotsOf('operate')).toEqual(TODAY)
    act(() => result.current.restoreSlots('phone', { rail1: 'getout', rail2: 'getout' } as never))
    expect(new Set(Object.values(result.current.slotsOf('phone'))).size, 'a restore is coerced: never a duplicate').toBe(4)
  })

  it('coerces a stored record: each placement repaired, and only the operating cockpits keep a rail', () => {
    const c = coerceDashConfig({
      slots: { rail1: 'nope' },
      sections: { operate: { rail1: 'getout', rail2: 'getout' }, settings: { rail1: 'clock' }, phone: 'junk', js8: [] },
    })
    expect(new Set(Object.values(c.slots)).size).toBe(4)
    expect(c.slots.rail1).toBe('clock')
    expect(Object.keys(c.sections)).toEqual(['operate'])
    expect(new Set(Object.values(c.sections.operate!)).size).toBe(4)
    for (const junk of [null, 'x', [], 7, { sections: 'x' }]) {
      expect(coerceDashConfig(junk), JSON.stringify(junk)).toEqual({ slots: DASH_DEFAULT_SLOTS, sections: {} })
    }
  })

  it('an older build reads today’s rail and nothing else, beside every cockpit', () => {
    seedToday()
    const { result } = renderHook(() => useDashSlots())
    act(() => result.current.assignPane('operate', 'rail1', 'getout'))
    act(() => result.current.assignPane('phone', 'rail2', 'contests'))
    // The reader before rails were per cockpit, verbatim: `slots`, coerced.
    const olderRead = coerceDashSlots((JSON.parse(localStorage.getItem('nexus.dashrail.config')!) as { slots?: unknown }).slots)
    expect(olderRead).toEqual(TODAY)
    // A fresh install reads the stock four everywhere.
    localStorage.clear()
    expect(slotsOf(loadDashConfig(), 'operate')).toEqual(DASH_DEFAULT_SLOTS)
    expect(loadDashSlots()).toEqual(DASH_DEFAULT_SLOTS)
  })

  it('which slots show is per cockpit too, each reading the window’s shared record until it has its own', () => {
    // Today's visibility: Getting Out's slot closed, on the one shared record.
    localStorage.setItem(panelStorageKey('dashrail'), JSON.stringify({ v: 2, state: { rail4: 'removed' }, share: {} }))
    const ft = renderHook(() => usePanelLayout(DASH_PANELS, dashRailInstance('operate'), windowInstance()))
    const phone = renderHook(() => usePanelLayout(DASH_PANELS, dashRailInstance('phone'), windowInstance()))
    expect(ft.result.current.stateOf('rail4'), 'FT’s first open is not today’s rail').toBe('removed')
    act(() => ft.result.current.setPanelState('rail2', 'removed'))
    expect(ft.result.current.stateOf('rail2')).toBe('removed')
    expect(phone.result.current.stateOf('rail2'), 'a close in FT’s rail closed Phone’s').toBe('docked')
    expect(phone.result.current.stateOf('rail4')).toBe('removed')
    // FT's record is its own, on the main window a durable one like every main-window layout; the shared one
    // is untouched, so a cockpit opened later still starts from today's.
    const key = panelStorageKey('dashrail', dashRailInstance('operate'))
    expect(key).toBe('nexus.panels.dashrail.operate.main')
    expect(isDurable(key)).toBe(true)
    expect(JSON.parse(localStorage.getItem(key)!).state).toEqual({ rail4: 'removed', rail2: 'removed' })
    expect(JSON.parse(localStorage.getItem(panelStorageKey('dashrail'))!).state).toEqual({ rail4: 'removed' })
  })
})

describe('the width: a stored preference fitted into the window, the cockpit keeping its floor', () => {
  it('the floor window is 1024 px at the zoom Nexus opens 1024×768 at (85 %)', () => {
    expect(pickInitialZoom(1024, 768)).toBe(85)
    expect(FLOOR_EFFECTIVE_W).toBeCloseTo(1024 / 0.85, 6)
  })

  it('whatever is stored, the cockpit is never narrower than on the 1024×768 floor window', () => {
    // Every lg/xl effective width, every stored preference, 1 px apart around the edges.
    const widths = [1600, 1607, 1700, 1920, 2048, 2290, 2399, 2400, 2560, 3440, 5120]
    const prefs = [null, 1, 150, 200, 250, 300, 402, 403, 600, 700, 720, 721, 5000]
    for (const ew of widths) {
      for (const pref of prefs) {
        const w = fitRailWidth(pref, ew)
        expect(ew - w, `ew ${ew}, stored ${pref}: rail ${w}`).toBeGreaterThanOrEqual(FLOOR_EFFECTIVE_W)
        expect(w).toBeGreaterThanOrEqual(RAIL_MIN)
        expect(w).toBeLessThanOrEqual(RAIL_MAX)
      }
    }
  })

  it('a width stored on a big monitor is fitted on a laptop and comes back on the monitor', () => {
    // 1366×768 opens at 85 %: 1607 effective px, so the rail may take 402.
    expect(fitRailWidth(700, 1366 / 0.85)).toBe(402)
    expect(fitRailWidth(700, 1920)).toBe(700)
    expect(fitRailWidth(null, 1920)).toBe(DASH_RAIL_DEFAULT_PX)
    expect(fitRailWidth(900, 3440)).toBe(RAIL_MAX)
    expect(fitRailWidth(120, 3440)).toBe(RAIL_MIN)
  })

  it('a move is clamped against the window as it is now', () => {
    expect(stepRailWidth(9999, 1607)).toBe(402)
    expect(stepRailWidth(10, 1607)).toBe(RAIL_MIN)
    expect(railWidthMax(1607)).toBe(402)
    expect(railWidthMax(3440)).toBe(RAIL_MAX)
  })

  it('a junk stored width is "never sized"', () => {
    for (const raw of [null, '', 'wide', '-3', '0', 'NaN', 'Infinity']) expect(parseRailWidth(raw), String(raw)).toBeNull()
    expect(parseRailWidth('333.6')).toBe(334)
  })
})
