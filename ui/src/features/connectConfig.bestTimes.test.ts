// @vitest-environment jsdom
//
// The Best times box reaches existing installs: a stored layout gains it ONCE, as the last tab of the
// right column's top slot, with every slot still showing what it showed and no other box moved. A
// layout that already has it is left alone, and a box the operator removes is never put back. A
// fresh install gets it from the preset plans instead (Frame + bar's right column), and a stored Frame
// + bar that gains it still reads as Frame + bar.
import { beforeEach, describe, expect, it } from 'vitest'
import { DEFAULT_SLOTS, SLOT_IDS, loadConnectConfig, removeTab, saveConnectConfig, showTab, slotBoxes, type ConnectConfig } from './connectConfig'
import { CONNECT_PRESETS, CONNECT_PRESET_IDS, TV_PRESETS, connectLayoutNow, layoutPanels, tableOffers } from './connectPresets'

const stored = (c: ConnectConfig) => localStorage.setItem('nexus.connect.config', JSON.stringify(c))
const where = (c: ConnectConfig) => SLOT_IDS.filter((s) => slotBoxes(c, s).includes('bestTimes'))

beforeEach(() => {
  localStorage.clear()
  window.history.replaceState(null, '', '/')
})

describe('the Best times box on a stored layout', () => {
  it('appears once, as a tab of right1, displacing nothing', () => {
    const before: ConnectConfig = { slots: { ...DEFAULT_SLOTS }, tabs: { left1: ['advisory', 'bandAdvisor'] }, rotate: {}, overlays: {} }
    stored(before)
    const after = loadConnectConfig()
    expect(where(after)).toEqual(['right1'])
    expect(after.slots).toEqual(before.slots)
    expect(after.tabs.left1).toEqual(['advisory', 'bandAdvisor'])
    expect(after.tabs.right1).toEqual([before.slots.right1, 'bestTimes'])
    // Written back, so it holds before the operator touches anything.
    expect(where(JSON.parse(localStorage.getItem('nexus.connect.config')!))).toEqual(['right1'])
  })

  it('is never put back once the operator removes it', () => {
    stored({ slots: { ...DEFAULT_SLOTS }, tabs: {}, rotate: {}, overlays: {} })
    const first = loadConnectConfig()
    const shown = showTab(first, 'right1', 'bestTimes')
    const removed = removeTab(shown, 'right1')!
    saveConnectConfig({ ...first, ...removed })
    expect(where(loadConnectConfig())).toEqual([])
  })

  it('leaves a layout that already has it where the operator put it', () => {
    const before: ConnectConfig = { slots: { ...DEFAULT_SLOTS, left2: 'bestTimes' }, tabs: {}, rotate: {}, overlays: {} }
    stored(before)
    const after = loadConnectConfig()
    expect(after.slots).toEqual(before.slots)
    expect(where(after)).toEqual(['left2'])
  })

  it('keeps a stored Frame + bar reading as Frame + bar', () => {
    const fb = CONNECT_PRESETS.frameBar
    const plan = Object.fromEntries(
      Object.entries(fb.tabs ?? {}).map(([s, list]) => [s, (list ?? []).filter((p) => s !== 'right1' || p !== 'bestTimes')]),
    )
    stored({ slots: { ...fb.slots }, tabs: plan, rotate: {}, overlays: {} } as ConnectConfig)
    const after = loadConnectConfig()
    expect(after.tabs.right1).toEqual([...fb.tabs!.right1!])
    expect(connectLayoutNow({ slots: after.slots, tabs: after.tabs, panels: layoutPanels(fb), rails: fb.rails, bar: fb.bar })).toBe('frameBar')
  })

  it('leaves every layout reading as the preset it was, with the box in it', () => {
    for (const id of CONNECT_PRESET_IDS) {
      localStorage.clear()
      const l = CONNECT_PRESETS[id]
      const plan = Object.fromEntries(Object.entries(l.tabs ?? {}).map(([s, list]) => [s, (list ?? []).filter((p) => p !== 'bestTimes')]))
      stored({ slots: { ...l.slots }, tabs: plan, rotate: {}, overlays: {} } as ConnectConfig)
      const after = loadConnectConfig()
      expect(where(after), id).toEqual(['right1'])
      const now = { slots: after.slots, tabs: after.tabs, panels: layoutPanels(l), rails: l.rails, bar: l.bar }
      expect(connectLayoutNow(now), id).toBe(id)
      // Even with the box's tab showing: showing a tab is no change of arrangement.
      expect(connectLayoutNow({ ...now, slots: { ...now.slots, right1: 'bestTimes' } }), id).toBe(id)
      // Control: any other box added the same way is the operator's own arrangement.
      const other = { ...now, tabs: { ...now.tabs, right1: [...(now.tabs.right1 ?? []), 'clock' as const].filter((p, i, a) => a.indexOf(p) === i) } }
      if (!slotBoxes(after, 'bottom1').includes('clock') && !slotBoxes(after, 'bottom3').includes('clock')) expect(connectLayoutNow(other), id).toBe('custom')
    }
  })

  it('a pop-out with no layout of its own neither gains it nor stops following the main window', () => {
    // The main window's layout, the box already removed there (its one-time insertion done).
    stored({ slots: { ...DEFAULT_SLOTS }, tabs: {}, rotate: {}, overlays: {} })
    localStorage.setItem('nexus.connect.bestTimes.v1', '1')
    // The dashboard window, which has never saved a layout: it reads the main window's.
    window.history.replaceState(null, '', '/?panel=connect')
    const popped = loadConnectConfig()
    expect(where(popped)).toEqual([])
    expect(localStorage.getItem('nexus.connect.config.connect'), 'no layout of its own was written').toBeNull()
    // Control: once it has a layout of its own, it gets the box once like any stored layout.
    localStorage.setItem('nexus.connect.config.connect', JSON.stringify({ slots: { ...DEFAULT_SLOTS }, tabs: {}, rotate: {}, overlays: {} }))
    expect(where(loadConnectConfig())).toEqual(['right1'])
  })

  it('never goes into the TV page, whose table does not offer it', () => {
    expect(tableOffers(CONNECT_PRESETS, 'bestTimes')).toBe(true)
    expect(tableOffers(TV_PRESETS, 'bestTimes')).toBe(false)
    stored({ slots: { ...DEFAULT_SLOTS }, tabs: {}, rotate: {}, overlays: {} })
    expect(where(loadConnectConfig(false))).toEqual([])
  })
})
