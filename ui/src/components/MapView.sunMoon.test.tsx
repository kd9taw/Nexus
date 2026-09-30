// @vitest-environment jsdom
//
// THE SUN IS ALWAYS ON THE MAP — WHERE IT IS OVERHEAD, ON A QUIET SUN AS WELL AS DURING A FLARE.
//
// Before this the 2-D map drew a sun only as part of the flare layer, whose canvas does not exist
// until an M-class flare: on a quiet sun there was no sun anywhere on the map. The marker is drawn
// on the map's own canvas, in the redraws the map already makes (its 60 s greyline clock moves it),
// so a quiet sun costs no animation. During a flare the flare layer's animated sun takes its place:
// the two are never drawn together.
//
// jsdom has no 2-D canvas, so each canvas gets a context that RECORDS every fill — its colour and
// the circles in its path. MapView is the REAL component; the clock is fixed so the sun's place is
// known.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
}))

import { MapView } from './MapView'
import { makeProjection, project, subsolarPoint } from '../mapGeo'
import { gridToLatLon } from '../grid'
import { STANDARD_SKY } from '../features/skins'

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

/** One `fill()`: the fillStyle it used and every circle in the path it filled. */
type Fill = { ink: unknown; arcs: Array<{ x: number; y: number; r: number }> }
const fills = new WeakMap<HTMLCanvasElement, Fill[]>()
function recordingContext(canvas: HTMLCanvasElement) {
  const list: Fill[] = []
  fills.set(canvas, list)
  let path: Fill['arcs'] = []
  const store: Record<string | symbol, unknown> = {}
  return new Proxy(store, {
    get: (t, k) => {
      if (k in t) return t[k]
      if (k === 'canvas') return canvas
      return (...a: unknown[]) => {
        if (k === 'beginPath') path = []
        if (k === 'arc') path.push({ x: a[0] as number, y: a[1] as number, r: a[2] as number })
        if (k === 'fill') list.push({ ink: t.fillStyle, arcs: path.slice() })
        if (k === 'measureText') return { width: 0 }
        return { addColorStop() {} }
      }
    },
  })
}

const SUN = STANDARD_SKY['--map-sun']
const W = 600
const H = 400
const EN52 = gridToLatLon('EN52')!
/** 2026-09-19 18:00 UTC: the sun is over the eastern Pacific, a quarter-globe from EN52 — on the
 *  face of a globe centred on the operator. 06:00 the same day puts it over India, on the far side. */
const DUSK = Date.UTC(2026, 8, 19, 18, 0)
const DAWN = Date.UTC(2026, 8, 19, 6, 0)

beforeEach(() => {
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

function props(over: Record<string, unknown> = {}) {
  return {
    myGrid: 'EN52',
    theme: 'dark' as never,
    stations: [],
    prop: null,
    selectedCall: null,
    onSelectCall: () => {},
    needByCall: new Map(),
    intent: 'dx' as const,
    ...over,
  }
}

async function mount(over: Record<string, unknown> = {}) {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<MapView {...props(over)} />)
  })
  return r
}
async function mountAt(nowMs: number, over: Record<string, unknown> = {}) {
  vi.useFakeTimers({ now: nowMs })
  return mount(over)
}

const mapCanvas = (c: HTMLElement) => c.querySelector('.map-canvas-wrap > canvas') as HTMLCanvasElement
/** The map's flare-effects overlay — the canvas the flare layer's animated sun lives on. */
const flareCanvas = (c: HTMLElement) => c.querySelector('.map-canvas-wrap > canvas[aria-hidden="true"]')
/** Filled circles in the sun's ink on `canvas`, newest last. */
const sunDiscs = (canvas: HTMLCanvasElement) =>
  (fills.get(canvas) ?? []).filter((f) => f.ink === SUN).flatMap((f) => f.arcs)
/** Where the map must put the sun at `nowMs` in projection `kind` (the component's own maths). */
const subsolarOnScreen = (kind: 'world' | 'globe' | 'aeqd', nowMs: number) =>
  project(makeProjection(kind, EN52, W, H, { zoom: 1, rotate: null, panX: 0, panY: 0 }), subsolarPoint(nowMs))!

describe('the sun on the 2-D map', () => {
  for (const kind of ['world', 'aeqd', 'globe'] as const) {
    it(`on a quiet sun it is drawn where it is overhead (${kind})`, async () => {
      const r = await mountAt(DUSK, { projection: kind, xrayLong: 1e-7 })
      const want = subsolarOnScreen(kind, DUSK)
      const got = sunDiscs(mapCanvas(r.container))
      expect(got.length, 'no disc in the sun’s colour was drawn on the map').toBeGreaterThan(0)
      const d = got[got.length - 1]
      expect(Math.hypot(d.x - want[0], d.y - want[1]), `sun at (${d.x}, ${d.y}), overhead point at (${want[0]}, ${want[1]})`).toBeLessThan(0.5)
      expect(d.r, 'a sun too small to see').toBeGreaterThanOrEqual(4)
      expect(flareCanvas(r.container), 'a quiet sun must not mount the flare canvas').toBeNull()
    })
  }

  it('the Sun and moon layer turns it off, and back on', async () => {
    const r = await mountAt(DUSK, { projection: 'world' })
    const canvas = mapCanvas(r.container)
    expect(sunDiscs(canvas).length, 'CONTROL: the sun is on by default').toBeGreaterThan(0)
    const box = screen.getByRole('checkbox', { name: 'Sun and moon' })
    expect((box as HTMLInputElement).checked).toBe(true)
    fills.get(canvas)!.length = 0
    await act(async () => void fireEvent.click(box))
    expect(sunDiscs(canvas), 'unticked, the redraw still drew the sun').toEqual([])
    await act(async () => void fireEvent.click(box))
    expect(sunDiscs(canvas).length).toBeGreaterThan(0)
  })

  it('on the Globe, a sun on the far side of the planet is not drawn through it', async () => {
    const r = await mountAt(DAWN, { projection: 'globe' })
    // A point marker cannot lean on the projection here: d3 clips a PATH at the globe's horizon,
    // but projects a lone point from the far side straight through the sphere onto the near face.
    expect(sunDiscs(mapCanvas(r.container))).toEqual([])
  })

  it('during a flare the flare layer’s animated sun takes its place — never both', async () => {
    const r = await mountAt(DUSK, { projection: 'world', xrayLong: 2e-4 }) // an X2
    expect(flareCanvas(r.container), 'CONTROL: an X2 mounts the flare layer’s own sun').not.toBeNull()
    expect(sunDiscs(mapCanvas(r.container)), 'the plain sun was drawn under the flare’s').toEqual([])
  })

  it('with the flare layer off, a flare leaves the plain sun where it is', async () => {
    localStorage.setItem(
      'nexus.connect.intents',
      JSON.stringify({ dx: { layers: { flare: { visible: false, opacity: 0.8 } } } }),
    )
    const r = await mountAt(DUSK, { projection: 'world', xrayLong: 2e-4 })
    expect(flareCanvas(r.container), 'CONTROL: the flare layer is off').toBeNull()
    expect(sunDiscs(mapCanvas(r.container)).length).toBeGreaterThan(0)
  })

  it('a quiet sun adds no clock: nothing redraws for ten seconds and no animation frame is asked for', async () => {
    vi.useFakeTimers({ now: DUSK })
    const raf = vi.spyOn(globalThis, 'requestAnimationFrame')
    const r = await mount({ projection: 'world', xrayLong: 1e-7 })
    const canvas = mapCanvas(r.container)
    const drawn = sunDiscs(canvas).length
    expect(drawn, 'CONTROL: the sun was drawn').toBeGreaterThan(0)
    for (let i = 0; i < 10; i++) await act(async () => void vi.advanceTimersByTime(1_000))
    expect(sunDiscs(canvas).length, 'the map redrew with nothing new to show').toBe(drawn)
    expect(raf, 'something asked for animation frames on a quiet sun').not.toHaveBeenCalled()
  })

  it('POSITIVE CONTROL — the flare layer’s sun does run an animation, so the spy above is live', async () => {
    vi.useFakeTimers({ now: DUSK })
    const raf = vi.spyOn(globalThis, 'requestAnimationFrame')
    await mount({ projection: 'world', xrayLong: 2e-4 })
    expect(raf).toHaveBeenCalled()
  })

  it('the 60 s greyline clock moves it with the sun', async () => {
    const r = await mountAt(DUSK, { projection: 'world' })
    const canvas = mapCanvas(r.container)
    // Ten ticks: the sun moves 2.5° of longitude, about 4 px here — well outside the tolerance, so
    // a map that never redrew could not pass by standing still.
    for (let i = 0; i < 10; i++) await act(async () => void vi.advanceTimersByTime(60_000))
    const discs = sunDiscs(canvas)
    expect(discs.length, 'no sun on the map after ten minutes').toBeGreaterThan(0)
    const d = discs[discs.length - 1]
    const want = subsolarOnScreen('world', DUSK + 600_000)
    const was = subsolarOnScreen('world', DUSK)
    expect(Math.hypot(want[0] - was[0], want[1] - was[1]), 'CONTROL: ten minutes really move it').toBeGreaterThan(3)
    expect(Math.hypot(d.x - want[0], d.y - want[1])).toBeLessThan(0.5)
  })
})
