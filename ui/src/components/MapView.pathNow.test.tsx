// @vitest-environment jsdom
//
// The selected station's ribbon on the 2-D map under a flare: the stretch the shack's overlay says
// a flare hits NOW on the drawn band is stroked over the route in its own ink (out = the flare-out
// ink, weakened = the flare-weak ink), the long route from its own overlay, dashed like its route;
// and with no overlay (the flare over, or none) no such stroke is drawn at all.
//
// jsdom has no 2-D canvas: each canvas gets a context that RECORDS every stroke with its ink and dash
// pattern at that moment (the MapView.flare.test.tsx harness). MapView is the REAL component.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  getPotaActivator: vi.fn(async () => ({ calls: [], parks: [] })),
}))

import { MapView, invalidateCssVarCache } from './MapView'
import { gridToLatLon, haversineKm } from '../grid'
import type { PathHourOverlay, PathPrediction, Station } from '../types'

vi.setConfig({ testTimeout: 15_000 })

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

type Stroke = { ink: unknown; dash: number[] }
const strokesOf = new WeakMap<HTMLCanvasElement, Stroke[]>()
function recordingContext(canvas: HTMLCanvasElement) {
  const strokes: Stroke[] = []
  strokesOf.set(canvas, strokes)
  let dash: number[] = []
  const store: Record<string | symbol, unknown> = {}
  return new Proxy(store, {
    get: (tgt, k) => {
      if (k in tgt) return tgt[k]
      if (k === 'canvas') return canvas
      return (...a: unknown[]) => {
        if (k === 'setLineDash') dash = [...(a[0] as number[])]
        if (k === 'stroke') strokes.push({ ink: tgt.strokeStyle, dash })
        if (k === 'measureText') return { width: 0 }
        return { addColorStop() {} }
      }
    },
  })
}

const NOW = Date.UTC(2026, 9, 30, 18, 0)
// Distinct inks for every ribbon token, so a stroke says which stretch it is.
const INKS: Record<string, string> = {
  '--route-flare-out': '#f00001',
  '--route-flare-weak': '#f00002',
  '--route-pca': '#f00003',
  '--route-polar': '#f00004',
  '--route-grey': '#f00005',
}

beforeEach(() => {
  vi.useFakeTimers({ now: NOW })
  localStorage.clear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  for (const [k, v] of Object.entries(INKS)) document.documentElement.style.setProperty(k, v)
  invalidateCssVarCache()
  const ctxs = new WeakMap<HTMLCanvasElement, unknown>()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    if (!ctxs.has(this)) ctxs.set(this, recordingContext(this))
    return ctxs.get(this) as RenderingContext
  })
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(600)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(400)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.useRealTimers()
  for (const k of Object.keys(INKS)) document.documentElement.style.removeProperty(k)
  invalidateCssVarCache()
})

const VK = { call: 'VK2ABC', grid: 'QF56', snr: -10, lastHeardSlot: 1, heardCount: 2, presence: 'active', worked: false } as unknown as Station
const SP_KM = haversineKm(gridToLatLon('EN52')!, gridToLatLon('QF56')!)

function over(kind: 'flareOut' | 'flareWeak'): PathHourOverlay {
  return {
    unix: NOW / 1000,
    kp: 2,
    kpSource: 'measured',
    capLatDeg: 63.6,
    source: { xrayAt: NOW / 1000 - 60, xrayLong: 1e-4, haf: 'goes' },
    bands: [{ band: '20m', hops: 4, hopsModelled: true, flareDb: 40, segments: [{ from: 0.1, to: 0.4, kind }] }],
  }
}

function outlook(withOverlay: boolean): PathPrediction {
  const geometry = (distKm: number, bearingDeg: number) => ({ distKm, bearingDeg, maxGmlatDeg: 40, nowHour: 18, hours: [], segmentsNow: [] })
  return {
    engine: 'p533',
    bands: [],
    mufNow: 0,
    mufHourly: [],
    geometry: geometry(SP_KM, 245),
    longPath: {
      engine: 'p533',
      bands: [],
      mufNow: 0,
      mufHourly: [],
      geometry: geometry(40_030 - SP_KM, 65),
      ...(withOverlay ? { overlay: over('flareWeak') } : {}),
    },
    routes: [{ band: '20m', verdict: 'short', confidence: 'likely', shortBcr: 0.8, longBcr: 0.05, reasons: [{ kind: 'oneClosed', open: 'short' }] }],
    ...(withOverlay ? { overlay: over('flareOut') } : {}),
  }
}

async function strokes(o: PathPrediction): Promise<Stroke[]> {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(
      <MapView
        myGrid="EN52"
        theme={'dark' as never}
        stations={[VK]}
        prop={null}
        selectedCall="VK2ABC"
        onSelectCall={() => {}}
        needByCall={new Map()}
        intent="dx"
        projection="world"
        outlook={o}
      />,
    )
  })
  await act(async () => {})
  const c = r.container.querySelector('.map-canvas-wrap > canvas') as HTMLCanvasElement
  return strokesOf.get(c) ?? []
}

describe('the ribbon under a flare on the 2-D map', () => {
  it('strokes the stretch a flare hits now in its own ink, each route from its own overlay', async () => {
    const s = await strokes(outlook(true))
    const out = s.filter((x) => x.ink === INKS['--route-flare-out'])
    const weak = s.filter((x) => x.ink === INKS['--route-flare-weak'])
    expect(out.length).toBeGreaterThan(0)
    expect(weak.length).toBeGreaterThan(0)
    // The short route's stretch is solid, the long route's dashed like its route.
    expect(out.every((x) => x.dash.length === 0)).toBe(true)
    expect(weak.every((x) => x.dash.length > 0)).toBe(true)
  })

  it('draws no flare stretch without the overlay (control: the route itself is still drawn)', async () => {
    const s = await strokes(outlook(false))
    expect(s.some((x) => x.ink === INKS['--route-flare-out'] || x.ink === INKS['--route-flare-weak'])).toBe(false)
    expect(s.length).toBeGreaterThan(0)
  })
})
