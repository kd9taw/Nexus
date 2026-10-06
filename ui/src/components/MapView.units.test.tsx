// @vitest-environment jsdom
//
// THE MAP'S DISTANCES FOLLOW THE UNITS SETTING.
//
// With Settings ▸ Units on Imperial, the map's hover line still ended a station's row in "1,234 km",
// the selected station's short/long-path figure read in km, and a satellite's hover gave its
// altitude in km: each formatted the raw kilometres and never asked the setting. In every case
// below the setting and the OS locale DISAGREE, so a map that read only one of them cannot pass:
// Imperial on a British locale, Metric on a US one.
//
// The harness is MapView.farside.test.tsx's: the REAL component, every canvas given a context that
// records nothing, and the pointer moved onto a marker's projected position.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import type { SatView, Station } from '../types'
import { setUnitsMirror } from '../units'

const feeds = vi.hoisted(() => ({ sats: null as unknown }))
vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => feeds.sats),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
}))

import { MapView } from './MapView'
import { makeProjection, project, type MapView3 } from '../mapGeo'
import { gridToLatLon, haversineKm, type LatLon } from '../grid'

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

const W = 600
const H = 400
const ME = gridToLatLon('EN52')!
/** W9NEAR's square: on the face of a globe centred on EN52, and under 1000 km from it. */
const NEAR = gridToLatLon('EN61')!
const HOME: MapView3 = { zoom: 1, rotate: null, panX: 0, panY: 0 }
const DUSK = Date.UTC(2026, 8, 19, 18, 0)
/** EN52 → EN61, the way the map measures it. Under 1000 km, so Metric reads the same with or
 *  without the thousands separator the old line used. */
const KM = haversineKm(ME, NEAR)
const MI = KM / 1.609344

beforeEach(() => {
  localStorage.clear()
  feeds.sats = null
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
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.useRealTimers()
})

/** The Units setting as App stores it, and the OS locale the 'auto' setting would read. */
function units(setting: 'imperial' | 'metric', locale: string): void {
  vi.spyOn(navigator, 'language', 'get').mockReturnValue(locale)
  setUnitsMirror(setting)
}

/** Only the named layers are on, so nothing else lands under the pointer. */
function layers(on: string[]): void {
  const off = ['daynight', 'rings', 'txPaths', 'heat', 'openings', 'muf', 'pca', 'flare', 'liveSpots', 'stations', 'paths', 'sunMoon']
  const table: Record<string, { visible: boolean }> = {}
  for (const k of off) table[k] = { visible: false }
  for (const k of on) table[k] = { visible: true }
  localStorage.setItem('nexus.connect.intents', JSON.stringify({ dx: { layers: table } }))
}

async function mount(over: Record<string, unknown> = {}) {
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
        {...over}
      />,
    )
  })
  // The feeds a layer fetches for itself (satellites among them) land a tick later.
  await act(async () => {})
  await act(async () => {})
  return r
}

const mapCanvas = (c: HTMLElement) => c.querySelector('.map-canvas-wrap > canvas') as HTMLCanvasElement
const at = (ll: LatLon) => project(makeProjection('globe', ME, W, H, HOME), ll)!
const station = (call: string, grid: string): Station =>
  ({ call, grid, snr: -10, lastHeardSlot: 1, heardCount: 2, presence: 'active', worked: false }) as unknown as Station

async function hoverText(r: ReturnType<typeof render>, ll: LatLon): Promise<string> {
  const [x, y] = at(ll)
  fireEvent.pointerMove(mapCanvas(r.container), { clientX: x, clientY: y })
  const card = r.container.querySelector('.map-hover')
  expect(card, 'the pointer is on the marker').toBeTruthy()
  return card!.textContent ?? ''
}

describe("a station's hover line ends in the operator's units", () => {
  it('Imperial beats a British locale: miles', async () => {
    units('imperial', 'en-GB')
    layers(['stations'])
    const r = await mount({ stations: [station('W9NEAR', 'EN61')] })
    const text = await hoverText(r, NEAR)
    expect(text).toContain('W9NEAR')
    expect(text.endsWith(` ${Math.round(MI)} mi`), text).toBe(true)
    expect(text).not.toMatch(/km/)
  })

  it('Metric beats a US locale: kilometres', async () => {
    units('metric', 'en-US')
    layers(['stations'])
    const r = await mount({ stations: [station('W9NEAR', 'EN61')] })
    const text = await hoverText(r, NEAR)
    expect(text.endsWith(` ${Math.round(KM)} km`), text).toBe(true)
  })
})

describe("the selected station's path figure is in the operator's units", () => {
  const figure = (r: ReturnType<typeof render>) => r.container.querySelector('.map-path-fig')?.textContent ?? ''

  it('Imperial beats a British locale: miles, short path and long path', async () => {
    units('imperial', 'en-GB')
    const r = await mount({ stations: [station('W9NEAR', 'EN61')], selectedCall: 'W9NEAR' })
    expect(figure(r)).toMatch(new RegExp(`° · ${Math.round(MI)} mi$`))
    // Long path: the rest of the great circle, 40 075 km around.
    await act(async () => {
      fireEvent.click(screen.getByTitle(/long path/i))
    })
    expect(figure(r)).toMatch(new RegExp(`° · ${Math.round((40_075 - KM) / 1.609344)} mi$`))
  })

  it('Metric beats a US locale: kilometres', async () => {
    units('metric', 'en-US')
    const r = await mount({ stations: [station('W9NEAR', 'EN61')], selectedCall: 'W9NEAR' })
    expect(figure(r)).toMatch(new RegExp(`° · ${Math.round(KM)} km$`))
  })
})

describe("a satellite's hover gives its altitude in the operator's units", () => {
  // 420 km is 261 mi.
  const view = () =>
    ({
      tleAgeDays: 1,
      usableCount: 1,
      agingCount: 0,
      heldBackCount: 0,
      tleFetchedAt: 0,
      tleSource: 'bundled',
      birds: [
        {
          name: 'NEARBIRD',
          norad: null,
          lat: NEAR.lat,
          lon: NEAR.lon,
          altKm: 420,
          footprintKm: 2200,
          track: [[DUSK / 1000, NEAR.lat, NEAR.lon]],
          amateur: true,
          status: 'alive',
          classes: ['fm'],
        },
      ],
      passes: [],
      excluded: [],
    }) as unknown as SatView

  it('Imperial beats a British locale: miles', async () => {
    units('imperial', 'en-GB')
    layers(['sats'])
    vi.useFakeTimers({ now: DUSK })
    feeds.sats = view()
    const r = await mount()
    const text = await hoverText(r, NEAR)
    expect(text).toContain('NEARBIRD')
    expect(text).toContain('alt 261 mi')
    expect(text).not.toMatch(/km/)
  })

  it('Metric beats a US locale: kilometres', async () => {
    units('metric', 'en-US')
    layers(['sats'])
    vi.useFakeTimers({ now: DUSK })
    feeds.sats = view()
    const r = await mount()
    expect(await hoverText(r, NEAR)).toContain('alt 420 km')
  })
})
