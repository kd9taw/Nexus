// @vitest-environment jsdom
//
// THE REGION BOX FOLLOWS THE STATION'S POWER AND GAIN THROUGH THE REAL CONNECT VIEW. The shack's
// ten-region sweep is computed from the grid, the engine, the station power and the antenna gain;
// the box asks for it again when one changes. Its own test (prop/PathDayStrip.test.tsx) hands it the
// props directly. This one mounts the real ConnectView with the box in a slot, so a link dropped
// anywhere between the view's props and the box (ConnectView → usePaneContext → the pane registry)
// leaves the box waiting for its 15-minute poll and fails here. Only the backend is stubbed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, render, act, waitFor } from '@testing-library/react'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
  getGettingOut: vi.fn(async () => null),
  getPathOutlook: vi.fn(async () => null),
  getPathRegions: vi.fn(async () => ({ engine: 'p533', computedUnix: 0, regions: [] })),
  getSpaceWxScales: vi.fn(async () => ({ scales: null, alerts: [] })),
  getKc2gMuf: vi.fn(async () => []),
  getXrayNow: vi.fn(async () => null),
  getDxpedWindows: vi.fn(async () => []),
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => ({ byZone: [] })),
  getOtaMapSpots: vi.fn(async () => []),
  getPotaActivator: vi.fn(async () => ({ calls: [], parks: [] })),
  getContests: vi.fn(async () => []),
  getSolarIndices: vi.fn(async () => ({ days: [] })),
}))
import { ConnectView } from './ConnectView'
import { getPathRegions } from '../api'
import { DEFAULT_SLOTS } from '../features/connectConfig'
import { pastTheSwitch } from './ConnectView.testkit'

vi.setConfig({ testTimeout: 15_000 })

const props = {
  myGrid: 'EN52',
  theme: 'dark' as const,
  stations: [],
  prop: null,
  selectedCall: null,
  onSelectCall: () => {},
  needByCall: new Map(),
  needAlerts: [],
  amp: null,
}

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  window.history.replaceState(null, '', '/')
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  // The region box in the left column's lower slot.
  localStorage.setItem(
    'nexus.connect.config',
    JSON.stringify({ slots: { ...DEFAULT_SLOTS, left2: 'pathRegions' }, overlays: {} }),
  )
})
afterEach(() => {
  cleanup()
  window.history.replaceState(null, '', '/')
})

describe('the region box on the Connect view', () => {
  it('asks the shack again when the station power or the antenna gain the view is given changes', async () => {
    const view = (power: number | null, gain: number) => (
      <ConnectView {...props} stationPowerW={power} antGainDbi={gain} />
    )
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(view(100, 0))
    })
    expect(r.container.querySelector('.pane-frame[data-pane="pathRegions"]'), 'the box is on screen').not.toBeNull()
    const asked = () => vi.mocked(getPathRegions).mock.calls.length
    await waitFor(() => expect(asked()).toBeGreaterThan(0))
    const before = asked()
    await act(async () => {
      r.rerender(view(100, 0))
    })
    expect(asked(), 'control: the same settings ask nothing').toBe(before)
    await act(async () => {
      r.rerender(view(5, 0))
    })
    await waitFor(() => expect(asked()).toBe(before + 1))
    await act(async () => {
      r.rerender(view(5, 6))
    })
    await waitFor(() => expect(asked()).toBe(before + 2))
  })
})
