// @vitest-environment jsdom
//
// THE SUN AND THE MOON ARE ALWAYS ON THE MAP — WHERE EACH IS OVERHEAD, THE MOON IN ITS PHASE, ON A
// QUIET SUN AS WELL AS DURING A FLARE.
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
import { makeProjection, moonAt, project, subsolarPoint } from '../mapGeo'
import { gridToLatLon } from '../grid'
import { STANDARD_SKY } from '../features/skins'

// THE BUDGET (2026-10-09). The slowest case here, "on a quiet sun it is drawn where it is overhead (world)", takes
// 0.22 s and 0.28 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

/** Every call a canvas's context took, in order, with the fill ink at that moment. */
type Op = { k: string; a: unknown[]; ink: unknown }
const opsOf = new WeakMap<HTMLCanvasElement, Op[]>()
function recordingContext(canvas: HTMLCanvasElement) {
  const ops: Op[] = []
  opsOf.set(canvas, ops)
  const store: Record<string | symbol, unknown> = {}
  return new Proxy(store, {
    get: (t, k) => {
      if (k in t) return t[k]
      if (k === 'canvas') return canvas
      return (...a: unknown[]) => {
        ops.push({ k: String(k), a, ink: t.fillStyle })
        if (k === 'measureText') return { width: 0 }
        return { addColorStop() {} }
      }
    },
  })
}
/** One `fill()`: the ink it used and every circle in the path it filled. */
type Fill = { ink: unknown; arcs: Array<{ x: number; y: number; r: number }> }
function fillsOf(canvas: HTMLCanvasElement): Fill[] {
  const out: Fill[] = []
  let path: Fill['arcs'] = []
  for (const o of opsOf.get(canvas) ?? []) {
    if (o.k === 'beginPath') path = []
    if (o.k === 'arc') path.push({ x: o.a[0] as number, y: o.a[1] as number, r: o.a[2] as number })
    if (o.k === 'fill') out.push({ ink: o.ink, arcs: path.slice() })
  }
  return out
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
  fillsOf(canvas).filter((f) => f.ink === SUN).flatMap((f) => f.arcs)
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
    opsOf.get(canvas)!.length = 0
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

// THE MOON, where it is overhead and in its phase. The phase is read off the drawing: the lit part
// is bounded by the limb and a terminator ellipse r·|2k − 1| wide (features/skyGlyphs.test.ts pins
// the glyph itself), drawn under a mirror when the lit limb is on the left.
const MOON_LIT = STANDARD_SKY['--map-moon-lit']
const MOON_DARK = STANDARD_SKY['--map-moon-dark']
/** 2026-09-13 21:00 UTC: a waxing crescent, 8.5 % lit, over the eastern Pacific (58° from EN52). */
const CRESCENT = Date.UTC(2026, 8, 13, 21, 0)
/** 2026-10-01 09:00 UTC: a waning gibbous, 74 % lit, over the Bahamas (18° from EN52). */
const WANING = Date.UTC(2026, 9, 1, 9, 0)
/** USNO's new and full moons (mapGeo.sky.test.ts holds the source). */
const NEW = Date.parse('2026-09-11T03:27Z')
const FULL = Date.parse('2026-09-26T16:49Z')

/** The dark disc of the last moon drawn on `canvas`: where it is and how big. */
function moonDisc(canvas: HTMLCanvasElement) {
  const discs = fillsOf(canvas).filter((f) => f.ink === MOON_DARK).flatMap((f) => f.arcs)
  return discs.length ? discs[discs.length - 1] : null
}
/** The lit part of the last moon drawn: the terminator's half-width and bow, and whether the lit
 *  limb was mirrored to the left. */
function moonLit(canvas: HTMLCanvasElement) {
  const ops = opsOf.get(canvas) ?? []
  const kinds = ops.map((o) => o.k)
  let fill = -1
  for (let i = ops.length - 1; i >= 0; i--) {
    if (ops[i].k === 'fill' && ops[i].ink === MOON_LIT) {
      fill = i
      break
    }
  }
  if (fill < 0) return null
  const begin = kinds.lastIndexOf('beginPath', fill)
  const save = kinds.lastIndexOf('save', begin)
  const ellipse = ops.slice(begin, fill).find((o) => o.k === 'ellipse')!
  return {
    halfWidth: ellipse.a[2] as number,
    r: ellipse.a[3] as number,
    crescent: ellipse.a[7] === true,
    mirrored: ops.slice(save, begin).some((o) => o.k === 'scale' && (o.a[0] as number) < 0),
  }
}
const sublunarOnScreen = (kind: 'world' | 'globe' | 'aeqd', nowMs: number, grid = 'EN52') =>
  project(makeProjection(kind, gridToLatLon(grid)!, W, H, { zoom: 1, rotate: null, panX: 0, panY: 0 }), moonAt(nowMs).sublunar)!

describe('the moon on the 2-D map', () => {
  for (const kind of ['world', 'aeqd', 'globe'] as const) {
    it(`is drawn where it is overhead (${kind})`, async () => {
      const r = await mountAt(CRESCENT, { projection: kind })
      const d = moonDisc(mapCanvas(r.container))
      expect(d, 'no moon on the map').not.toBeNull()
      const want = sublunarOnScreen(kind, CRESCENT)
      expect(Math.hypot(d!.x - want[0], d!.y - want[1]), `moon at (${d!.x}, ${d!.y}), overhead point at (${want[0]}, ${want[1]})`).toBeLessThan(0.5)
      expect(d!.r, 'a moon too small to read its phase').toBeGreaterThanOrEqual(5)
    })
  }

  it('shows its phase: a crescent’s terminator is r·|2k − 1| wide and bows toward the lit limb', async () => {
    const r = await mountAt(CRESCENT, { projection: 'world' })
    const lit = moonLit(mapCanvas(r.container))
    expect(lit, 'the moon has no lit part').not.toBeNull()
    const k = moonAt(CRESCENT).illuminated
    expect(k, 'CONTROL: this really is a thin crescent').toBeLessThan(0.15)
    expect(lit!.halfWidth).toBeCloseTo(lit!.r * Math.abs(2 * k - 1), 6)
    expect(lit!.crescent).toBe(true)
  })

  it('a waning gibbous moon is lit past the middle, the other way', async () => {
    const r = await mountAt(WANING, { projection: 'world' })
    const lit = moonLit(mapCanvas(r.container))!
    const k = moonAt(WANING).illuminated
    expect(k, 'CONTROL: this really is gibbous').toBeGreaterThan(0.6)
    expect(lit.halfWidth).toBeCloseTo(lit.r * Math.abs(2 * k - 1), 6)
    expect(lit.crescent).toBe(false)
  })

  it('from the northern hemisphere a waxing moon is lit on the right and a waning one on the left', async () => {
    const waxing = await mountAt(CRESCENT, { projection: 'world' })
    expect(moonAt(CRESCENT).waxing, 'CONTROL').toBe(true)
    expect(moonLit(mapCanvas(waxing.container))!.mirrored, 'waxing, seen from EN52: lit on the right').toBe(false)
    cleanup()
    vi.useRealTimers()
    const waning = await mountAt(WANING, { projection: 'world' })
    expect(moonAt(WANING).waxing, 'CONTROL').toBe(false)
    expect(moonLit(mapCanvas(waning.container))!.mirrored, 'waning, seen from EN52: lit on the left').toBe(true)
  })

  it('from the southern hemisphere it is the other way round', async () => {
    // QF56 is Sydney: the same waxing crescent is lit on the LEFT in its sky.
    const r = await mountAt(CRESCENT, { projection: 'world', myGrid: 'QF56' })
    expect(gridToLatLon('QF56')!.lat, 'CONTROL: QF56 is south of the equator').toBeLessThan(0)
    expect(moonLit(mapCanvas(r.container))!.mirrored).toBe(true)
  })

  it('a new moon is a dark disc with no lit part, and a full moon is lit to the far rim', async () => {
    const r = await mountAt(NEW, { projection: 'world' })
    expect(moonDisc(mapCanvas(r.container)), 'a new moon must still be on the map').not.toBeNull()
    expect(moonLit(mapCanvas(r.container)), 'a new moon has nothing lit').toBeNull()
    cleanup()
    vi.useRealTimers()
    const full = await mountAt(FULL, { projection: 'world' })
    const lit = moonLit(mapCanvas(full.container))!
    expect(lit.halfWidth / lit.r, 'a full moon is lit across the whole disc').toBeGreaterThan(0.99)
    expect(lit.crescent).toBe(false)
  })

  it('on the Globe, a moon on the far side of the planet is not drawn through it', async () => {
    // At DUSK the moon is over the South Atlantic off Namibia, 114° from EN52: behind a globe centred there.
    const r = await mountAt(DUSK, { projection: 'globe' })
    expect(moonDisc(mapCanvas(r.container))).toBeNull()
    cleanup()
    vi.useRealTimers()
    const flat = await mountAt(DUSK, { projection: 'world' })
    expect(moonDisc(mapCanvas(flat.container)), 'CONTROL: the flat map, which has no far side, does draw it').not.toBeNull()
  })

  it('the Sun and moon layer turns the moon off too', async () => {
    const r = await mountAt(CRESCENT, { projection: 'world' })
    const canvas = mapCanvas(r.container)
    expect(moonDisc(canvas), 'CONTROL: on by default').not.toBeNull()
    opsOf.get(canvas)!.length = 0
    await act(async () => void fireEvent.click(screen.getByRole('checkbox', { name: 'Sun and moon' })))
    expect(moonDisc(canvas)).toBeNull()
  })

  it('a flare does not take the moon’s place — only the sun’s', async () => {
    const r = await mountAt(CRESCENT, { projection: 'world', xrayLong: 2e-4 })
    expect(flareCanvas(r.container), 'CONTROL: the flare layer’s sun is up').not.toBeNull()
    expect(moonDisc(mapCanvas(r.container))).not.toBeNull()
  })
})
