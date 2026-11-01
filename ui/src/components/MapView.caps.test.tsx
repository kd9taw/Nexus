// @vitest-environment jsdom
//
// WHAT THE OPERATOR SEES OF THE CAPS AND OVALS ON THE 2-D MAP — the surface, not the pure functions
// (capsNow.test.ts pins those):
// - the polar cap's approximate edge is stroked dashed in violet only beside the model's shading and
//   only at a measured Kp; beside NOAA's current grid there is none, and the chip says NOAA D-RAP;
// - the cap chip gives the S level, its sources with their times, and NOAA's rule-of-thumb duration,
//   which it drops (and dims) when the proton sample is old;
// - the aurora layer turns itself on in a storm (Kp ≥ 5, or a NOAA G watch), the operator's untick
//   sticks, and nothing is fetched without one;
// - a current OVATION draws its dots and its own edge; a stale or missing one draws no dots and no
//   line at all, and the row and chip say why.
// Each has a control, so a test cannot pass by the map drawing nothing.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'

const feeds = vi.hoisted(() => ({
  aurora: null as unknown,
  pca: null as unknown,
  kpForecast: { points: [] } as unknown,
  arrival: { watch: null, model: null, l1: null, rationale: null } as unknown,
}))
vi.mock('../api', () => ({
  getAurora: vi.fn(async () => feeds.aurora),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => feeds.pca),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  getPotaActivator: vi.fn(async () => ({ calls: [], parks: [] })),
  getKpForecast: vi.fn(async () => feeds.kpForecast),
  getArrivalOutlook: vi.fn(async () => feeds.arrival),
}))

import { MapView } from './MapView'
import { getAurora } from '../api'
import { t } from '../i18n'
import { kpLabel } from '../capsNow'
import { wxStampLabel } from '../propViz'
import type { AuroraOval, DrapGrid, PcaView, PropagationSnapshot } from '../types'
import type { FlareNoaa } from '../flareNow'

vi.setConfig({ testTimeout: 15_000 })

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

/** Every stroke and fill a canvas took, with its ink and dash pattern at that moment. */
type Mark = { op: 'stroke' | 'fill'; ink: unknown; dash: number[] }
const marksOf = new WeakMap<HTMLCanvasElement, Mark[]>()
function recordingContext(canvas: HTMLCanvasElement) {
  const marks: Mark[] = []
  marksOf.set(canvas, marks)
  let dash: number[] = []
  const store: Record<string | symbol, unknown> = {}
  return new Proxy(store, {
    get: (tgt, k) => {
      if (k in tgt) return tgt[k]
      if (k === 'canvas') return canvas
      return (...a: unknown[]) => {
        if (k === 'setLineDash') dash = [...(a[0] as number[])]
        if (k === 'stroke') marks.push({ op: 'stroke', ink: tgt.strokeStyle, dash })
        if (k === 'fill') marks.push({ op: 'fill', ink: tgt.fillStyle, dash })
        if (k === 'measureText') return { width: 0 }
        return { addColorStop() {} }
      }
    },
  })
}

const W = 600
const H = 400
const NOW = Date.UTC(2026, 9, 31, 1, 30)
const NOW_S = NOW / 1000
const CAP_INK = 'rgb(200, 110, 240)'
const AURORA_INK = 'rgb(120, 230, 140)'

function layersOn(on: string[], off: string[] = []): void {
  const table: Record<string, { visible: boolean }> = {}
  for (const k of ['daynight', 'sunMoon', 'flare', 'heat', 'openings', 'rings', 'paths', 'txPaths', ...off]) table[k] = { visible: false }
  for (const k of on) table[k] = { visible: true }
  localStorage.setItem('nexus.connect.intents', JSON.stringify({ dx: { layers: table } }))
}

beforeEach(() => {
  vi.useFakeTimers({ now: NOW })
  localStorage.clear()
  feeds.aurora = null
  feeds.pca = null
  feeds.kpForecast = { points: [] }
  feeds.arrival = { watch: null, model: null, l1: null, rationale: null }
  vi.mocked(getAurora).mockClear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  const ctxs = new WeakMap<HTMLCanvasElement, unknown>()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    if (!ctxs.has(this)) ctxs.set(this, recordingContext(this))
    return ctxs.get(this) as RenderingContext
  })
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(W)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(H)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.useRealTimers()
})

/** A live snapshot whose Kp is `kp`, NOAA's 1-minute estimate two minutes old. */
const snap = (kp: number) =>
  ({
    source: 'live',
    asOf: NOW_S,
    spots: [],
    openings: [],
    dxpeditions: { workableNow: [] },
    spaceWx: { sfi: 150, kp, aIndex: 5, xrayClass: 'B2.0', flare: false, xrayLong: 2e-7, fetched: true, sfiAt: NOW_S - 3600, kpAt: NOW_S - 120, xrayAt: NOW_S - 60, aDay: NOW_S - 86400 },
    insights: [],
  }) as unknown as PropagationSnapshot

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
  // The self-fetched layers and the feeds land a tick later.
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10)
  })
  return r
}

const mapCanvas = (c: HTMLElement) => c.querySelector('.map-canvas-wrap > canvas') as HTMLCanvasElement
const marks = (c: HTMLElement) => marksOf.get(mapCanvas(c)) ?? []
const strokes = (c: HTMLElement, ink: string) => marks(c).filter((m) => m.op === 'stroke' && m.ink === ink)
const capChip = (c: HTMLElement) => c.querySelector('.wx-chip-stack .pca-chip') as HTMLElement | null
const auroraChip = (c: HTMLElement) => c.querySelector('.wx-chip-stack .aurora-chip') as HTMLElement | null
function auroraRow(c: HTMLElement): HTMLElement {
  const row = [...c.querySelectorAll('.map-layer')].find((r) => r.textContent?.includes(t('map.layer.aurora.label')))
  return row as HTMLElement
}
const auroraBox = (c: HTMLElement) => auroraRow(c).querySelector('input[type="checkbox"]') as HTMLInputElement

const ring = (lat: number): [number, number][] => [[-170, lat], [-90, lat], [0, lat], [90, lat], [170, lat]]
const PCA: PcaView = {
  j10: 150,
  a30Day: 1.5,
  a30Night: 0.8,
  cutoffDeg: 60,
  points: [
    { lat: 72, lon: -100, db30: 1.4 },
    { lat: 74, lon: -60, db30: 1.2 },
  ],
  protonsAt: NOW_S - 300,
  sScale: 2,
  sepRecoveryH: 24.235,
  edge: [ring(62), ring(-62)],
}

/** A whole-globe D-RAP grid with NOAA's polar caps at 25 MHz (0.76 dB at 30 MHz). */
function capGrid(ageS = 60): DrapGrid {
  const lats = Array.from({ length: 90 }, (_, i) => 89 - 2 * i)
  const lons = Array.from({ length: 90 }, (_, i) => -178 + 4 * i)
  const haf: number[] = []
  for (const la of lats) for (let i = 0; i < lons.length; i++) haf.push(Math.abs(la) >= 70 ? 25 : 0)
  return { validAt: NOW_S - ageS, recoveryAt: null, lats, lons, haf }
}
const lane = (drap: DrapGrid | null): FlareNoaa => ({ xrayAt: NOW_S - 60, series: [], recent: [], event: null, drap })

describe('the polar cap on the 2-D map', () => {
  it('draws the model’s approximate edge, dashed violet, at a measured Kp, and the chip says so', async () => {
    layersOn(['pca'])
    feeds.pca = PCA
    const r = await mount()
    const edge = strokes(r.container, CAP_INK)
    expect(edge.length, 'one stroke per hemisphere').toBe(2)
    expect(edge.every((s) => s.dash.join() === '4,4')).toBe(true)
    const chip = capChip(r.container)!
    expect(chip.getAttribute('data-source')).toBe('model')
    expect(chip.textContent).toContain(t('map.pca.head', { scale: 2, db: '1.4' }))
    expect(chip.textContent).toContain(t('map.pca.src.model', { protons: t('map.pca.protons', { time: '01:25Z' }), deg: 60 }))
    expect(chip.textContent).toContain(t('map.pca.recovery', { h: 24 }))
  })

  it('without a measured Kp there is no edge, and the chip says the edge is unknown', async () => {
    layersOn(['pca'])
    feeds.pca = { ...PCA, cutoffDeg: null, edge: null }
    const r = await mount()
    expect(strokes(r.container, CAP_INK)).toEqual([])
    expect(capChip(r.container)!.textContent).toContain(t('map.pca.src.modelNoKp', { protons: t('map.pca.protons', { time: '01:25Z' }) }))
  })

  it('beside NOAA’s current grid the cap is NOAA’s, with no approximate edge; a stale grid hands it back', async () => {
    layersOn(['pca'])
    feeds.pca = PCA
    const r = await mount({ xrayLong: 2e-7, flareNoaa: lane(capGrid()) })
    expect(strokes(r.container, CAP_INK), 'no approximate line beside NOAA’s cap').toEqual([])
    const chip = capChip(r.container)!
    expect(chip.getAttribute('data-source')).toBe('noaa')
    expect(chip.textContent).toContain(t('map.pca.src.noaa', { time: '01:29Z', protons: t('map.pca.protons', { time: '01:25Z' }) }))
    cleanup()
    const s = await mount({ xrayLong: 2e-7, flareNoaa: lane(capGrid(600)) })
    expect(capChip(s.container)!.getAttribute('data-source')).toBe('model')
    expect(strokes(s.container, CAP_INK).length).toBe(2)
  })

  it('an old proton sample says how old, dims, and drops NOAA’s duration', async () => {
    layersOn(['pca'])
    feeds.pca = { ...PCA, protonsAt: NOW_S - 40 * 60 }
    const r = await mount()
    const chip = capChip(r.container)!
    expect(chip.hasAttribute('data-stale')).toBe(true)
    expect(chip.textContent).toContain(t('map.pca.protonsOld', { time: '00:50Z', ago: t('prop.opening.ago.mins', { mins: 40 }) }))
    expect(chip.textContent).not.toContain(t('map.pca.recovery', { h: 24 }))
  })

  it('a quiet sky has no cap chip', async () => {
    layersOn(['pca'])
    feeds.pca = { ...PCA, j10: 0.3, sScale: 0, points: [], sepRecoveryH: null }
    const r = await mount()
    expect(capChip(r.container)).toBeNull()
    expect(strokes(r.container, CAP_INK)).toEqual([])
  })
})

const LATS = Array.from({ length: 24 }, () => 60)
const OVAL: AuroraOval = {
  points: [{ lat: 66, lon: -100, prob: 50 }],
  observedAt: NOW_S - 20 * 60,
  forecastAt: NOW_S + 40 * 60,
  edge: { north: LATS, south: LATS.map((l) => -l) },
}
const auroraFills = (c: HTMLElement) => marks(c).filter((m) => m.op === 'fill' && /^rgb\(\d+, \d+, \d+\)$/.test(String(m.ink)) && m.ink !== CAP_INK)

describe('the aurora layer turns itself on in a storm', () => {
  it('Kp ≥ 5 turns it on (unsaved), fetches OVATION, and the chip says why', async () => {
    layersOn([])
    feeds.aurora = OVAL
    const r = await mount({ prop: snap(6) })
    expect(auroraBox(r.container).checked).toBe(true)
    expect(getAurora).toHaveBeenCalled()
    expect(auroraRow(r.container).textContent).toContain(t('map.layer.aurora.auto'))
    expect(auroraChip(r.container)!.textContent).toContain(
      t('map.aurora.chip.storm', { why: kpLabel({ kp: 6, source: 'estimated', at: NOW_S - 120 }) }),
    )
    // the operator's own choice is untouched
    expect(JSON.parse(localStorage.getItem('nexus.connect.intents')!).dx.layers.aurora?.visible ?? false).toBe(false)
  })

  it('the operator’s untick during the storm sticks', async () => {
    layersOn([])
    feeds.aurora = OVAL
    const r = await mount({ prop: snap(6) })
    await act(async () => {
      fireEvent.click(auroraBox(r.container))
    })
    expect(auroraBox(r.container).checked).toBe(false)
    expect(auroraChip(r.container)).toBeNull()
    const asked = vi.mocked(getAurora).mock.calls.length
    await act(async () => {
      await vi.advanceTimersByTimeAsync(11 * 60_000)
    })
    expect(vi.mocked(getAurora).mock.calls.length, 'no more OVATION asks').toBe(asked)
    expect(auroraBox(r.container).checked).toBe(false)
  })

  it('a NOAA G watch in force turns it on too', async () => {
    layersOn([])
    feeds.aurora = OVAL
    feeds.arrival = { watch: { serial: 7, code: 'WATA20', issued: NOW_S - 3600, days: [], comment: null, window: null, maxG: 2 }, model: null, l1: null, rationale: null }
    const r = await mount({ prop: snap(3) })
    expect(auroraBox(r.container).checked).toBe(true)
    expect(auroraChip(r.container)!.textContent).toContain(t('map.aurora.chip.storm', { why: t('map.aurora.why.watch', { g: 2 }) }))
  })

  it('CONTROL — Kp 4.67 and no watch: off, and OVATION is never asked', async () => {
    layersOn([])
    feeds.aurora = OVAL
    const r = await mount({ prop: snap(4.67) })
    expect(auroraBox(r.container).checked).toBe(false)
    expect(getAurora).not.toHaveBeenCalled()
  })
})

describe('the aurora oval: OVATION while current, nothing when it is stale or missing', () => {
  it('a current OVATION draws its dots and its own edge, solid', async () => {
    layersOn(['aurora'])
    feeds.aurora = OVAL
    const r = await mount()
    const edge = strokes(r.container, AURORA_INK)
    expect(edge.length).toBe(2)
    expect(edge.every((s) => s.dash.length === 0)).toBe(true)
    expect(auroraFills(r.container).length).toBeGreaterThan(0)
    expect(auroraChip(r.container), 'no chip on a quiet night with a current forecast').toBeNull()
  })

  it('a stale OVATION draws no oval line and no dots, in a storm too; the row and chip say it is out of date', async () => {
    layersOn(['aurora'])
    const f = NOW_S - 3600
    feeds.aurora = { ...OVAL, forecastAt: f }
    const r = await mount({ prop: snap(6) })
    expect(strokes(r.container, AURORA_INK)).toEqual([])
    expect(auroraFills(r.container)).toEqual([])
    expect(auroraRow(r.container).textContent).toContain(t('map.layer.aurora.stale', { time: wxStampLabel(f) }))
    expect(auroraChip(r.container)!.textContent).toContain(t('map.aurora.chip.stale', { time: '00:30Z' }))
  })

  it('no OVATION forecast at all draws nothing, and says so', async () => {
    layersOn(['aurora'])
    feeds.aurora = { points: [], observedAt: null, forecastAt: null, edge: { north: [], south: [] } }
    const r = await mount()
    expect(strokes(r.container, AURORA_INK)).toEqual([])
    expect(auroraFills(r.container)).toEqual([])
    expect(auroraRow(r.container).textContent).toContain(t('map.layer.aurora.none'))
    expect(auroraChip(r.container)!.textContent).toContain(t('map.aurora.chip.none'))
  })
})

describe('round 3: the storm rule and the untick, on the 2-D map', () => {
  // A 3-hour period NOAA measured, which ended 90 minutes ago.
  const START = NOW_S - 90 * 60 - 3 * 3600
  const measured = (kp: number) => ({ points: [{ timeUnix: START, kp, kind: 'observed', noaaScale: null }] })

  it('an operator who had the aurora on and unticks it during a storm gets it off in one click, and it stays off', async () => {
    layersOn(['aurora'])
    feeds.aurora = OVAL
    const r = await mount({ prop: snap(6) })
    expect(auroraBox(r.container).checked).toBe(true)
    await act(async () => {
      fireEvent.click(auroraBox(r.container))
    })
    expect(auroraBox(r.container).checked).toBe(false)
    // across the next OVATION poll and the 60 s clock: what is drawn from here on has no oval
    marks(r.container).length = 0
    await act(async () => {
      await vi.advanceTimersByTimeAsync(11 * 60_000)
    })
    expect(auroraBox(r.container).checked).toBe(false)
    expect(marks(r.container).length, 'CONTROL: the map redrew').toBeGreaterThan(0)
    expect(strokes(r.container, AURORA_INK)).toEqual([])
    expect(auroraFills(r.container)).toEqual([])
  })

  it('measured Kp 4 (a period that ended 90 min ago) and an estimate of 6.3: the storm turns the layer on, said as the estimate', async () => {
    layersOn([])
    feeds.aurora = OVAL
    feeds.kpForecast = measured(4)
    const r = await mount({ prop: snap(6.3) })
    expect(auroraBox(r.container).checked).toBe(true)
    expect(auroraChip(r.container)!.textContent).toContain(
      t('map.aurora.chip.storm', { why: kpLabel({ kp: 6.3, source: 'estimated', at: NOW_S - 120 }) }),
    )
  })

  it('CONTROL — measured Kp 6 and an estimate of 3: on, said as NOAA’s measurement', async () => {
    layersOn([])
    feeds.aurora = OVAL
    feeds.kpForecast = measured(6)
    const r = await mount({ prop: snap(3) })
    expect(auroraBox(r.container).checked).toBe(true)
    expect(auroraChip(r.container)!.textContent).toContain(
      t('map.aurora.chip.storm', { why: kpLabel({ kp: 6, source: 'measured', at: START, until: START + 3 * 3600 }) }),
    )
  })

  it('an older station’s dated oval an hour past its forecast time draws no dots', async () => {
    layersOn(['aurora'])
    feeds.aurora = { points: [{ lat: 66, lon: -100, prob: 50 }], observedAt: NOW_S - 4000, forecastAt: NOW_S - 3600 }
    const r = await mount()
    expect(auroraFills(r.container)).toEqual([])
  })
})
