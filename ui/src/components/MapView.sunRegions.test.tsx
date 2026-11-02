// @vitest-environment jsdom
//
// NOAA'S ACTIVE REGIONS IN THE SUN'S HOVER ON THE MAP — only while the shack holds the list, and
// gone at its `until` by this map's own clock with no new poll. The harness is MapView.units.test's:
// the REAL component, a context that records nothing, the pointer moved onto the sun's projected
// position.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import type { SolarRegionsView } from '../types'

const feeds = vi.hoisted(() => ({ regions: null as unknown }))
vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  getPotaActivator: vi.fn(async () => ({ calls: [], parks: [] })),
  getSolarRegions: vi.fn(async () => feeds.regions),
}))

import { MapView } from './MapView'
import { makeProjection, project, subsolarPoint, type MapView3 } from '../mapGeo'
import { gridToLatLon } from '../grid'
import { t } from '../i18n'
import { regionsDayLabel } from '../sunRegions'

vi.setConfig({ testTimeout: 15_000 })

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

const W = 600
const H = 400
const ME = gridToLatLon('EN52')!
const HOME: MapView3 = { zoom: 1, rotate: null, panX: 0, panY: 0 }
const NOW = Date.UTC(2026, 9, 30, 18, 0)
const DAY29 = Date.UTC(2026, 9, 29) / 1000

const region = (n: number, lat: number, lon: number, location: string) => ({
  region: n,
  latDeg: lat,
  lonDeg: lon,
  location,
  magClass: 'A',
  spotClass: 'Axx',
  area: 10,
  numberSpots: 1,
  mFlareProbability: 1,
  xFlareProbability: 1,
})
/** Held until `untilS`. */
const held = (untilS: number): SolarRegionsView => ({
  held: true,
  why: 'held',
  observedDay: DAY29,
  until: untilS,
  regions: [region(4563, -18, -81, 'S18W81'), region(4564, 5, 68, 'N05E68')],
})

beforeEach(() => {
  vi.useFakeTimers({ now: NOW })
  localStorage.clear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  const noop = new Proxy({} as Record<string | symbol, unknown>, {
    get: (_t, k) => {
      if (k === 'measureText') return () => ({ width: 0 })
      return () => ({ addColorStop() {} })
    },
  })
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => noop as unknown as RenderingContext)
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(() => W)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(() => H)
  // Only the sun on, so nothing else lands under the pointer.
  const table: Record<string, { visible: boolean }> = {}
  for (const k of ['daynight', 'rings', 'txPaths', 'heat', 'openings', 'muf', 'pca', 'flare', 'liveSpots', 'stations', 'paths'])
    table[k] = { visible: false }
  table.sunMoon = { visible: true }
  localStorage.setItem('nexus.connect.intents', JSON.stringify({ dx: { layers: table } }))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.useRealTimers()
})

async function mount() {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(
      <MapView
        myGrid="EN52"
        theme={'dark' as never}
        stations={[]}
        prop={null}
        selectedCall={null}
        onSelectCall={() => {}}
        needByCall={new Map()}
        intent="dx"
        projection="globe"
      />,
    )
  })
  await act(async () => {})
  await act(async () => {})
  return r
}

function sunHover(r: ReturnType<typeof render>): string {
  const [x, y] = project(makeProjection('globe', ME, W, H, HOME), subsolarPoint(Date.now()))!
  fireEvent.pointerMove(r.container.querySelector('.map-canvas-wrap > canvas')!, { clientX: x, clientY: y })
  const card = r.container.querySelector('.map-hover')
  expect(card, 'the pointer is on the sun').toBeTruthy()
  return card!.textContent ?? ''
}

const LIST = `4563 S18W81 (${t('map.sun.regions.westLimb')}), 4564 N05E68 (${t('map.sun.regions.eastLimb')})`

describe('the sun’s hover names NOAA’s active regions', () => {
  it('lists the held regions with their day and limb', async () => {
    feeds.regions = held(NOW / 1000 + 3600)
    const r = await mount()
    expect(sunHover(r)).toContain(t('map.hover.sunRegions', { day: regionsDayLabel(DAY29), list: LIST }))
  })

  it('drops them at the list’s until, by this map’s clock, with no new poll', async () => {
    feeds.regions = held(NOW / 1000 + 90) // between two 60 s ticks
    const r = await mount()
    expect(sunHover(r), 'CONTROL: held before its until').toContain('4564 N05E68')
    feeds.regions = null // every later poll would fail; none is due anyway
    await act(async () => void vi.advanceTimersByTime(89_000))
    expect(sunHover(r), 'CONTROL: still held a second before').toContain('4564 N05E68')
    await act(async () => void vi.advanceTimersByTime(2_000))
    expect(sunHover(r)).not.toContain('4564')
  })

  it('NEGATIVE CONTROL — a stale list names no region', async () => {
    feeds.regions = { ...held(NOW / 1000 + 3600), held: false, why: 'stale', until: null, regions: [] }
    const r = await mount()
    const text = sunHover(r)
    expect(text).not.toContain('4564')
    expect(text).not.toContain(t('map.hover.sunRegions', { day: regionsDayLabel(DAY29), list: '' }).split(':')[0])
  })
})
