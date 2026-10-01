// @vitest-environment jsdom
//
// The satellite and DXpedition ALARMS are operator data: they are listed as durable
// (durableStore.ts DURABLE_KEYS), so an armed alarm must survive what localStorage does not, a
// reset of the webview's storage or a reinstall. The two alarm modules read and wrote
// localStorage directly, so ui-state.json got a copy once, at boot, and never read it back: the
// chase sets beside them, read and written through the durable store, survived; the alarms did
// not.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { loadDurable, flushDurable, __resetDurableForTest } from './durableStore'
import { satAlarmMap, toggleSatAlarm, markSatPassFired, passKey } from './satAlarm'
import { alarmMap, toggleAlarm } from './dxpedAlarm'

const mockLoad = vi.fn<() => Promise<Record<string, string>>>()
const mockSave = vi.fn<(s: Record<string, string>) => Promise<boolean>>()
vi.mock('../api', () => ({
  uiStateLoad: () => mockLoad(),
  uiStateSave: (s: Record<string, string>) => mockSave(s),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

beforeEach(() => {
  __resetDurableForTest()
  window.localStorage.clear()
  mockLoad.mockReset().mockResolvedValue({})
  mockSave.mockReset().mockResolvedValue(true)
})
afterEach(() => __resetDurableForTest())

/** What the last flush handed ui-state.json for `key`, parsed. */
async function flushed(key: string): Promise<unknown> {
  await flushDurable()
  const last = mockSave.mock.calls[mockSave.mock.calls.length - 1]?.[0] ?? {}
  return key in last ? JSON.parse(last[key]) : undefined
}

describe('armed alarms survive a reset of the webview storage', () => {
  it('reads a satellite alarm the durable file holds when localStorage has lost it', async () => {
    mockLoad.mockResolvedValue({ 'nexus.sats.alarms': JSON.stringify({ 'AO-91': { leadMin: 20 } }) })
    await loadDurable()
    expect(window.localStorage.getItem('nexus.sats.alarms'), 'premise: localStorage lost it').toBeNull()
    expect(satAlarmMap(), 'the armed alarm comes back from ui-state.json').toEqual({ 'AO-91': { leadMin: 20 } })
  })

  it('reads a DXpedition alarm the durable file holds when localStorage has lost it', async () => {
    mockLoad.mockResolvedValue({ 'nexus.dxped.alarms': JSON.stringify({ '3Y0K': { leadMin: 30 } }) })
    await loadDurable()
    expect(window.localStorage.getItem('nexus.dxped.alarms'), 'premise: localStorage lost it').toBeNull()
    expect(alarmMap(), 'the armed alarm comes back from ui-state.json').toEqual({ '3Y0K': { leadMin: 30 } })
  })

  it('writes an armed alarm through to the durable file', async () => {
    await loadDurable()
    toggleSatAlarm('AO-91')
    toggleAlarm('3Y0K')
    expect(await flushed('nexus.sats.alarms'), 'the satellite alarm reaches ui-state.json').toEqual({
      'AO-91': { leadMin: 15 },
    })
    expect(await flushed('nexus.dxped.alarms'), 'the DXpedition alarm reaches ui-state.json').toEqual({
      '3Y0K': { leadMin: 15 },
    })
  })

  it('keeps a pass that already fired from firing again after the reset', async () => {
    await loadDurable()
    markSatPassFired('AO-91', 1_790_778_153)
    expect(await flushed('nexus.sats.alarms.fired'), 'the fired pass reaches ui-state.json').toEqual([
      passKey('AO-91', 1_790_778_153),
    ])
  })

  it('control: a key the file does not hold reads as nothing, not as a stale copy', async () => {
    // Without this, the two reads above could pass on a map that answers from anywhere.
    await loadDurable()
    expect(satAlarmMap()).toEqual({})
    expect(alarmMap()).toEqual({})
  })
})
