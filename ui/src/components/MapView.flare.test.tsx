// @vitest-environment jsdom
//
// WHAT THE OPERATOR SEES OF A FLARE ON THE 2-D MAP — the surface, not the pure functions (flareNow.test.ts
// pins those):
// - the per-band rings are stroked, in the band's colour, dashed at 1 dB and solid at 10 dB;
// - a GOES sample 5 min old or more freezes the footprint (no breathing, no 1 s tick, no animated sun);
// - Preview's simulated flux is never drawn with NOAA's grid or event, even when a current one is there;
// - the chip's GOES line carries the sample time on the snapshot path too (no fast lane).
// Each has a below-M1 or real-flare control, so a test cannot pass by the map drawing nothing.
//
// jsdom has no 2-D canvas, so each canvas gets a context that RECORDS every stroke with the stroke ink
// and dash pattern at that moment. MapView is the REAL component; the clock is fixed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  getPotaActivator: vi.fn(async () => ({ calls: [], parks: [] })),
}))

import { MapView } from './MapView'
import { bandRings, flareFootprint, type FlareNoaa } from '../flareNow'
import { flareHafAt, flareHafMhz } from '../mapGeo'
import { bandColor } from '../bandColors'
import { t } from '../i18n'
import type { DrapGrid, FlareEvent, PropagationSnapshot } from '../types'

// The house budget for a real-MapView mount on a loaded box (MapView.sunMoon.test.tsx has the numbers).
vi.setConfig({ testTimeout: 15_000 })

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

/** Every stroke a canvas took: its ink and dash pattern at that moment. */
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

const W = 600
const H = 400
const NOW = Date.UTC(2026, 9, 30, 18, 0) // 18:00Z: the sun over the eastern Pacific, EN52 in daylight
const NOW_S = NOW / 1000
const M5 = 5e-5
const C5 = 5e-6

beforeEach(() => {
  // Installed here, before any test spies on setInterval, so the spy wraps the fake clock's.
  vi.useFakeTimers({ now: NOW })
  localStorage.clear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  const ctxs = new WeakMap<HTMLCanvasElement, unknown>()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    if (!ctxs.has(this)) ctxs.set(this, recordingContext(this))
    return ctxs.get(this) as RenderingContext
  })
  // jsdom reports every document as hidden, and the map skips its ticks for a hidden tab.
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(W)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(H)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.useRealTimers()
})

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
        projection="world"
        {...over}
      />,
    )
  })
  return r
}

const mapCanvas = (c: HTMLElement) => c.querySelector('.map-canvas-wrap > canvas') as HTMLCanvasElement
/** The flare layer's animated-sun overlay: mounted only while the flare breathes. */
const flareCanvas = (c: HTMLElement) => c.querySelector('.map-canvas-wrap > canvas[aria-hidden="true"]')
const chip = (c: HTMLElement) => c.querySelector('.wx-chip-stack .flare-chip') as HTMLElement | null

/** NOAA's grid, the analytic field × `gain`, valid `ageS` seconds before NOW. */
function grid(xray: number, gain: number, ageS = 60): DrapGrid {
  const lats = Array.from({ length: 90 }, (_, i) => 89 - 2 * i)
  const lons = Array.from({ length: 90 }, (_, i) => -178 + 4 * i)
  const haf: number[] = []
  for (const la of lats) for (const lo of lons) haf.push(flareHafAt(la, lo, NOW, xray) * gain)
  return { validAt: NOW_S - ageS, recoveryAt: null, lats, lons, haf }
}
const event: FlareEvent = {
  at: NOW_S - 60,
  beginAt: NOW_S - 20 * 60,
  beginClass: 'C5.0',
  maxAt: null,
  maxClass: null,
  maxFlux: null,
  endAt: null,
  currentClass: 'X2.0',
}
const lane = (o: Partial<FlareNoaa> = {}): FlareNoaa => ({ xrayAt: NOW_S - 60, series: [], recent: [], event: null, drap: null, ...o })

describe('the per-band rings on the 2-D map', () => {
  it('strokes each band’s 1 dB ring dashed and its 10 dB ring solid, in the band’s colour', async () => {
    const r = await mount({ xrayLong: M5, flareNoaa: lane() })
    const want = bandRings(flareFootprint(NOW, M5, null)!, NOW)
    expect(want.length, 'CONTROL: an M5 reaches several bands').toBeGreaterThan(4)
    expect(want.some((w) => w.db === 10), 'CONTROL: an M5 puts a band out').toBe(true)
    const strokes = strokesOf.get(mapCanvas(r.container))!
    for (const ring of want) {
      const ink = bandColor(ring.band)
      const dashed = ring.db === 1
      const hit = strokes.some((s) => s.ink === ink && (s.dash.length > 0) === dashed)
      expect(hit, `${ring.band} ${ring.db} dB ring not stroked ${dashed ? 'dashed' : 'solid'} in ${ink}`).toBe(true)
    }
  })

  it('NEGATIVE CONTROL — below M1 no ring is stroked', async () => {
    const r = await mount({ xrayLong: C5, flareNoaa: lane() })
    const strokes = strokesOf.get(mapCanvas(r.container))!
    for (const band of ['160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m']) {
      expect(
        strokes.some((s) => s.ink === bandColor(band) && s.dash.length > 0),
        `a dashed ${band} ring below M1`,
      ).toBe(false)
    }
  })
})

describe('a stale GOES sample freezes the flare', () => {
  it('a sample 5 min old stops the breathing: no 1 s tick, no animated sun', async () => {
    const tick = vi.spyOn(globalThis, 'setInterval')
    const r = await mount({ xrayLong: M5, flareNoaa: lane({ xrayAt: NOW_S - 5 * 60 }) })
    expect(chip(r.container), 'CONTROL: the frozen flare is still shown').not.toBeNull()
    expect(flareCanvas(r.container), 'a stale sample still mounted the animated sun').toBeNull()
    expect(tick.mock.calls.filter((c) => c[1] === 1000), 'a stale sample still runs the 1 s pulse').toEqual([])
  })

  it('POSITIVE CONTROL — a fresh sample breathes', async () => {
    const tick = vi.spyOn(globalThis, 'setInterval')
    const r = await mount({ xrayLong: M5, flareNoaa: lane({ xrayAt: NOW_S - 4 * 60 }) })
    expect(flareCanvas(r.container)).not.toBeNull()
    expect(tick.mock.calls.some((c) => c[1] === 1000)).toBe(true)
  })

  it('NEGATIVE CONTROL — below M1 nothing breathes, fresh or not', async () => {
    const tick = vi.spyOn(globalThis, 'setInterval')
    const r = await mount({ xrayLong: C5, flareNoaa: lane() })
    expect(flareCanvas(r.container)).toBeNull()
    expect(tick.mock.calls.filter((c) => c[1] === 1000)).toEqual([])
  })
})

describe('Preview never wears NOAA’s name', () => {
  const X2 = 2e-4
  // A current grid 30 % hotter than the GOES estimate and an ongoing event: if Preview took them, the
  // headline number and the source would change.
  const noaa = lane({ event, drap: grid(X2, 1.3) })
  const goesHaf = `≤${Math.round(flareHafMhz(X2))} MHz`

  it('POSITIVE CONTROL — a real X2 with that grid is drawn from NOAA', async () => {
    const r = await mount({ xrayLong: X2, flareNoaa: noaa })
    const c = chip(r.container)!
    expect(c.querySelector('.flare-chip-src')!.getAttribute('data-source')).toBe('noaa')
    expect(c.textContent).toContain('NOAA')
    expect(c.textContent, 'CONTROL: NOAA’s number differs from the GOES estimate').not.toContain(goesHaf)
  })

  it('a Preview beside a current NOAA grid and event says PREVIEW, nothing of NOAA, and is drawn from GOES', async () => {
    const r = await mount({ xrayLong: 1e-7, flareNoaa: noaa })
    expect(chip(r.container), 'CONTROL: a quiet sun shows no chip').toBeNull()
    await act(async () => void fireEvent.click(r.container.querySelector('.flare-preview')!))
    const c = chip(r.container)!
    expect(c.textContent).toContain(t('map.flare.phase.preview'))
    expect(c.textContent).not.toContain('NOAA')
    expect(c.getAttribute('data-source')).toBe('goes')
    expect(c.textContent).toContain(goesHaf)
  })
})

describe('the chip’s GOES line has a time on the snapshot path', () => {
  const at = NOW_S - 2 * 60
  const snap = (xrayAt?: number) =>
    ({
      source: 'live',
      asOf: NOW_S,
      spots: [],
      openings: [],
      dxpeditions: { workableNow: [] },
      spaceWx: { sfi: 150, kp: 2, aIndex: 5, xrayClass: 'M5.0', flare: true, xrayLong: M5, xrayAt },
      insights: [],
    }) as unknown as PropagationSnapshot

  it('with no fast lane, the class carries the snapshot’s GOES sample time', async () => {
    const r = await mount({ xrayLong: M5, flareNoaa: null, prop: snap(at) })
    expect(chip(r.container)!.querySelector('.flare-chip-src')!.textContent).toBe(
      t('map.flare.src.goes', { xtime: '17:58Z' }),
    )
  })

  it('CONTROL — an undated snapshot has no time to give', async () => {
    const r = await mount({ xrayLong: M5, flareNoaa: null, prop: snap(undefined) })
    expect(chip(r.container)!.querySelector('.flare-chip-src')!.textContent).toBe(t('map.flare.src.goesOnly'))
  })
})
