// @vitest-environment jsdom
//
// THE SIX-HOUR REPLAY ON THE 2-D MAP — the surface (replay.test.ts pins the pure rules):
// - it reads REPLAY, with the replayed UTC time and how long ago, and the flare chip for now stands down;
// - the flare drawn is the one in the held GOES trace at that time, never now's, and is labelled
//   estimated from GOES, never NOAA's grid, even beside a current grid;
// - a replayed time with no held reading draws no flare and says so;
// - the slider never reaches before six hours ago or past the trace's newest reading;
// - back to now restores now.
// MapView is the REAL component; the clock is fixed; each canvas records its strokes.
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
import { wxStampLabel } from '../propViz'
import { bandRings, flareFootprint, type FlareNoaa } from '../flareNow'
import { flareHafAt } from '../mapGeo'
import { bandColor } from '../bandColors'
import { t } from '../i18n'
import type { DrapGrid, FlareEvent } from '../types'

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


/** 5-minute bucket peaks from eight hours ago to five minutes ago, C1 except `at` → `peak`; `skip`
 *  leaves out the samples inside it (a gap in the trace). */
function trace(at: number, peak: number, skip?: [number, number]): [number, number][] {
  const out: [number, number][] = []
  for (let t = NOW_S - 8 * 3600; t <= NOW_S - 60; t += 300) {
    if (skip && t >= skip[0] && t <= skip[1]) continue
    out.push([t, t === at ? peak : 1e-6])
  }
  return out
}
// A peak on the bucket grid, about two hours ago.
const PEAK_S = NOW_S - 8 * 3600 + 72 * 300
const BANDS = ['160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m']
const replayChip = (c: HTMLElement) => c.querySelector('.wx-chip-stack .replay-chip') as HTMLElement | null
const nowChip = (c: HTMLElement) => c.querySelector('.wx-chip-stack .flare-chip:not(.replay-chip)') as HTMLElement | null
const startBtn = (c: HTMLElement) => c.querySelector('.replay-start') as HTMLButtonElement
function ringStrokes(c: HTMLElement) {
  return strokesOf.get(mapCanvas(c))!.filter((s) => BANDS.some((b) => s.ink === bandColor(b)) && s.dash.length > 0)
}

describe('the replay button', () => {
  it('is disabled, saying why, while no X-ray trace is held', async () => {
    const r = await mount({ xrayLong: C5, flareNoaa: null })
    const b = startBtn(r.container)
    expect(b, 'no replay button in the flare row').not.toBeNull()
    expect(b.disabled).toBe(true)
    expect(b.title).toBe(t('map.replay.start.none'))
  })

  it('POSITIVE CONTROL — with a trace held it starts a replay at the trace’s highest reading', async () => {
    const r = await mount({ xrayLong: C5, flareNoaa: lane({ series: trace(PEAK_S, M5) }) })
    expect(startBtn(r.container).disabled).toBe(false)
    expect(replayChip(r.container), 'a replay before the button was pressed').toBeNull()
    await act(async () => void fireEvent.click(startBtn(r.container)))
    const c = replayChip(r.container)!
    expect(c, 'no replay bar').not.toBeNull()
    expect(c.querySelector('.replay-chip-head')!.textContent).toBe(
      t('map.replay.head', { time: wxStampLabel(PEAK_S), ago: '2h ago' }),
    )
    expect(c.textContent).toContain('REPLAY')
  })
})

describe('the replayed flare', () => {
  it('is drawn from the held trace at the replayed time, while now is quiet', async () => {
    const r = await mount({ xrayLong: C5, flareNoaa: lane({ series: trace(PEAK_S, M5) }) })
    expect(ringStrokes(r.container), 'CONTROL: a quiet now draws no ring').toEqual([])
    await act(async () => void fireEvent.click(startBtn(r.container)))
    const want = bandRings(flareFootprint(PEAK_S * 1000, M5, null)!, PEAK_S * 1000)
    expect(want.length, 'CONTROL: an M5 reaches several bands').toBeGreaterThan(4)
    const strokes = strokesOf.get(mapCanvas(r.container))!
    for (const ring of want) {
      const hit = strokes.some((s) => s.ink === bandColor(ring.band) && (s.dash.length > 0) === (ring.db === 1))
      expect(hit, `${ring.band} ${ring.db} dB ring of the replayed M5 not stroked`).toBe(true)
    }
    const c = replayChip(r.container)!
    expect(c.textContent).toContain(t('map.replay.flux', { cls: 'M5.0', time: '16:00Z' }))
    expect(c.querySelector('[data-source]')!.getAttribute('data-source')).toBe('goes')
  })

  it('never wears NOAA’s name, and the flare chip for now stands down, beside a current grid', async () => {
    const X2 = 2e-4
    const noaa = lane({ event, drap: grid(X2, 1.3), series: trace(PEAK_S, M5) })
    const r = await mount({ xrayLong: X2, flareNoaa: noaa })
    expect(nowChip(r.container), 'CONTROL: now’s flare chip is up').not.toBeNull()
    expect(nowChip(r.container)!.textContent, 'CONTROL: now is NOAA’s').toContain('NOAA')
    await act(async () => void fireEvent.click(startBtn(r.container)))
    expect(nowChip(r.container), 'the chip for now stayed up during the replay').toBeNull()
    const c = replayChip(r.container)!
    expect(c.textContent).toContain(t('map.replay.est'))
    expect(c.textContent).not.toContain('D-RAP')
    // Back to now: the replay goes and now's chip, NOAA's, returns.
    await act(async () => void fireEvent.click(c.querySelector('.replay-back')!))
    expect(replayChip(r.container)).toBeNull()
    expect(nowChip(r.container)!.textContent).toContain('NOAA')
  })

  it('a replayed time with no held reading draws no flare and says so', async () => {
    // The peak sits in the trace; the hour around four hours ago is a gap.
    const gap: [number, number] = [NOW_S - 4.5 * 3600, NOW_S - 3.5 * 3600]
    const r = await mount({ xrayLong: C5, flareNoaa: lane({ series: trace(PEAK_S, M5, gap) }) })
    await act(async () => void fireEvent.click(startBtn(r.container)))
    expect(ringStrokes(r.container).length, 'CONTROL: the peak draws rings').toBeGreaterThan(0)
    strokesOf.get(mapCanvas(r.container))!.length = 0
    const slider = replayChip(r.container)!.querySelector('.replay-slider') as HTMLInputElement
    await act(async () => void fireEvent.change(slider, { target: { value: String((NOW_S - 4 * 3600) * 1000) } }))
    const c = replayChip(r.container)!
    expect(c.textContent).toContain(t('map.replay.noFlux'))
    expect(c.textContent).not.toContain(t('map.replay.est'))
    expect(ringStrokes(r.container), 'rings drawn for a time with no reading').toEqual([])
  })

  it('reaches no further back than six hours and no later than the newest reading', async () => {
    const r = await mount({ xrayLong: C5, flareNoaa: lane({ series: trace(PEAK_S, M5) }) })
    await act(async () => void fireEvent.click(startBtn(r.container)))
    const slider = replayChip(r.container)!.querySelector('.replay-slider') as HTMLInputElement
    expect(Number(slider.min)).toBe(NOW - 6 * 3600 * 1000)
    // The trace's newest bucket is five minutes ago: the replay never goes past it toward now.
    expect(Number(slider.max)).toBe(NOW - 300 * 1000)
    // The trace held eight hours: the two oldest are never offered.
    expect(Number(slider.min)).toBeGreaterThan((NOW_S - 8 * 3600) * 1000)
  })

  it('is the GOES estimate even at a time NOAA’s current grid is valid for', async () => {
    // The replay lands on the newest reading, the minute NOAA's grid (30 % hotter) is valid for: the
    // replayed footprint is still the analytic field's, never the grid's.
    const X2 = 2e-4
    const at = NOW_S - 60
    const series: [number, number][] = [
      [NOW_S - 3600, 1e-6],
      [at, X2],
    ]
    const g = grid(X2, 1.3)
    const r = await mount({ xrayLong: X2, flareNoaa: lane({ drap: g, series }) })
    const est = bandRings(flareFootprint(at * 1000, X2, null)!, at * 1000)
    const noaa = bandRings(flareFootprint(at * 1000, X2, g)!, at * 1000)
    const key = (w: { band: string; db: number }) => `${w.band}:${w.db}`
    const extra = noaa.filter((w) => !est.some((e) => key(e) === key(w)))
    expect(extra.length, 'CONTROL: NOAA’s grid would draw a ring the estimate does not').toBeGreaterThan(0)
    strokesOf.get(mapCanvas(r.container))!.length = 0
    await act(async () => void fireEvent.click(startBtn(r.container)))
    const strokes = strokesOf.get(mapCanvas(r.container))!
    for (const w of est) {
      expect(strokes.some((s) => s.ink === bandColor(w.band) && (s.dash.length > 0) === (w.db === 1)), `${key(w)} missing`).toBe(true)
    }
    for (const w of extra) {
      expect(
        strokes.some((s) => s.ink === bandColor(w.band) && (s.dash.length > 0) === (w.db === 1)),
        `${key(w)}: the replay drew NOAA’s grid`,
      ).toBe(false)
    }
  })
})
