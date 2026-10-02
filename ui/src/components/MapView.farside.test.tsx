// @vitest-environment jsdom
//
// ON THE 2-D GLOBE, A MARKER ON THE FAR SIDE OF THE PLANET IS NOT DRAWN THROUGH IT.
//
// d3 clips a PATH at the globe's horizon, but projects a lone POINT from the far side straight
// through the sphere onto the near face. Measured in Chrome: a 20 m spot in Sydney, on a globe
// centred on EN52, 133° away and behind the planet, was drawn over the eastern Pacific. Each point
// marker and point label below now asks mapGeo's `inView()` first. Each layer gets one point on the
// far side, which must not draw, and one on the near side, which must — so a check that hid
// everything could not pass either. The flat maps show the whole world, so there the far point IS
// drawn: the control that proves the test can see it at all.
//
// jsdom has no 2-D canvas, so every canvas gets a context that records its calls. MapView is the
// REAL component.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import type { MapSpot, OpeningView, PropagationSnapshot, SatView, Station, WorkableCard } from '../types'
import type { AprsStation } from '../api'

const feeds = vi.hoisted(() => ({
  sats: null as unknown,
  aurora: null as unknown,
  pca: null as unknown,
}))
vi.mock('../api', () => ({
  getAurora: vi.fn(async () => feeds.aurora),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => feeds.pca),
  getSatellites: vi.fn(async () => feeds.sats),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
}))

import { MapView } from './MapView'
import { destinationPoint, flareField, inView, makeProjection, project, type MapView3 } from '../mapGeo'
import { bearingDeg, gridToLatLon, haversineKm, type LatLon } from '../grid'

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

/** Every call a canvas's context took, in order. */
type Op = { k: string; a: unknown[] }
const opsOf = new Map<HTMLCanvasElement, Op[]>()
function recordingContext(canvas: HTMLCanvasElement) {
  const ops: Op[] = []
  opsOf.set(canvas, ops)
  const store: Record<string | symbol, unknown> = {}
  return new Proxy(store, {
    get: (t, k) => {
      if (k in t) return t[k]
      if (k === 'canvas') return canvas
      return (...a: unknown[]) => {
        ops.push({ k: String(k), a })
        if (k === 'measureText') return { width: 0 }
        return { addColorStop() {} }
      }
    },
  })
}

let W = 600
let H = 400
/** EN52's centre, which the map is centred on. */
const ME = gridToLatLon('EN52')!
/** Sydney: 133° from EN52 — behind a globe centred there. */
const FAR: LatLon = { lat: -33.87, lon: 151.21 }
/** Chicago: on the face of that globe. */
const NEAR: LatLon = { lat: 41.88, lon: -87.63 }
const HOME: MapView3 = { zoom: 1, rotate: null, panX: 0, panY: 0 }
const DUSK = Date.UTC(2026, 8, 19, 18, 0)

beforeEach(() => {
  localStorage.clear()
  opsOf.clear()
  feeds.sats = null
  feeds.aurora = null
  feeds.pca = null
  W = 600
  H = 400
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  const ctxs = new WeakMap<HTMLCanvasElement, unknown>()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    if (!ctxs.has(this)) ctxs.set(this, recordingContext(this))
    return ctxs.get(this) as RenderingContext
  })
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(() => W)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(() => H)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

/** Only the layers a test names are on, besides the basemap: nothing else can land on the points. */
function layers(on: string[]): void {
  // Every layer that is on by default and draws on the globe's face, so `on` is all there is: the Sun and Moon
  // layer (on by default) strokes the sun's rays wherever the subsolar point faces the viewer.
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
  // The feeds a layer fetches for itself (satellites, aurora, PCA, CQ zones) land a tick later.
  await act(async () => {})
  await act(async () => {})
  return r
}

const mapCanvas = (c: HTMLElement) => c.querySelector('.map-canvas-wrap > canvas') as HTMLCanvasElement
/** Where `ll` lands — THROUGH the globe for a far-side point, which is where it must not be drawn. */
const at = (ll: LatLon, kind: 'globe' | 'world' = 'globe', view: MapView3 = HOME) =>
  project(makeProjection(kind, ME, W, H, view), ll)!
/** Did `canvas` draw anything anchored within half a pixel of (x, y)? A circle's centre, a path's
 *  point, a translation or a rectangle's corner; a text's position. */
function drewAt(canvas: HTMLCanvasElement, x: number, y: number, kinds?: string[]): boolean {
  return (opsOf.get(canvas) ?? []).some((o) => {
    if (kinds && !kinds.includes(o.k)) return false
    const xy =
      o.k === 'fillText' || o.k === 'strokeText'
        ? [o.a[1], o.a[2]]
        : ['arc', 'moveTo', 'lineTo', 'translate', 'fillRect', 'rect'].includes(o.k)
          ? [o.a[0], o.a[1]]
          : null
    return !!xy && Math.hypot((xy[0] as number) - x, (xy[1] as number) - y) < 0.5
  })
}
/** Is one of `ops` anchored within half a pixel of (x, y)? */
const drawAt = (ops: Op[], x: number, y: number) =>
  ops.some((o) => Math.hypot((o.a[0] as number) - x, (o.a[1] as number) - y) < 0.5)
/** Texts drawn on any canvas (the base map's labels live on its own offscreen canvas). */
const texts = () =>
  [...opsOf.values()].flat().filter((o) => o.k === 'fillText').map((o) => String(o.a[0]))

const station = (call: string, grid: string): Station =>
  ({ call, grid, snr: -10, lastHeardSlot: 1, heardCount: 2, presence: 'active', worked: false }) as unknown as Station
const spot = (call: string, ll: LatLon): MapSpot =>
  ({ call, lat: ll.lat, lon: ll.lon, band: '20m', heardMe: false, ageSecs: 60, approx: false, freqMhz: 14.074, mode: 'FT8', entity: 'X' }) as unknown as MapSpot
const snap = (over: Partial<PropagationSnapshot>): PropagationSnapshot =>
  ({
    source: 'live',
    asOf: 1,
    spots: [],
    openings: [],
    dxpeditions: { workableNow: [] },
    spaceWx: { sfi: 150, kp: 2, aIndex: 5, xrayClass: 'B2', flare: false, xrayLong: 1e-7 },
    insights: [],
    ...over,
  }) as unknown as PropagationSnapshot
/** A live opening: its wedge spans `bearingDeg` ±22.5° from the QTH, out to `maxKm`. */
const opening = (band: string, mode: string, bearingDeg: number, maxKm: number): OpeningView =>
  ({ band, mode, octant: 'N', bearingDeg, maxKm, probability: 0.8, stations: 3, confidence: 'Likely', confidenceScore: 0.8, reciprocalPairs: 1, anomalyZ: 4, onsetSecs: 600, isNew: false, note: '' }) as OpeningView
/** The corners of an opening's far edge, left to right, as the map has always sampled it: 17 across 45°. */
const farEdge = (o: OpeningView) => Array.from({ length: 17 }, (_, i) => destinationPoint(ME, o.bearingDeg - 22.5 + (45 * i) / 16, o.maxKm))
/** The path an area on `canvas` was drawn with, from the beginPath before its first point at `p` to its fill. */
function pathThrough(canvas: HTMLCanvasElement, p: [number, number]): Op[] {
  const ops = opsOf.get(canvas) ?? []
  const i = ops.findIndex((o) => (o.k === 'moveTo' || o.k === 'lineTo') && Math.hypot((o.a[0] as number) - p[0], (o.a[1] as number) - p[1]) < 0.5)
  if (i < 0) return []
  const fill = ops.findIndex((o, j) => j > i && o.k === 'fill')
  return ops.slice(ops.map((o) => o.k).lastIndexOf('beginPath', i), fill < 0 ? undefined : fill)
}

describe('the control: this test can see the far point, where the map shows it', () => {
  it('on the flat map, which shows the whole world, the far station IS drawn', async () => {
    layers(['stations'])
    const r = await mount({ projection: 'world', stations: [station('VK2FAR', 'QF56'), station('W9NEAR', 'EN61')] })
    const far = at(gridToLatLon('QF56')!, 'world')
    expect(drewAt(mapCanvas(r.container), far[0], far[1], ['arc']), 'the far station, on the flat map').toBe(true)
  })
  it('and the far point really is behind the globe', () => {
    const proj = makeProjection('globe', ME, W, H, HOME)
    expect(inView('globe', proj, FAR), 'Sydney, behind a globe centred on EN52').toBe(false)
    expect(inView('globe', proj, NEAR), 'Chicago, on its face').toBe(true)
    expect(inView('world', makeProjection('world', ME, W, H, HOME), FAR), 'the flat maps have no far side').toBe(true)
  })
  it('on the flat map, an opening wedge 15,000 km long is drawn to its far edge, and tagged there', async () => {
    layers(['openings'])
    const o = opening('20m', 'F2', 340, 15_000)
    const r = await mount({ projection: 'world', prop: snap({ openings: [o] }) })
    const tip = at(farEdge(o)[8], 'world')
    expect(drewAt(mapCanvas(r.container), tip[0], tip[1], ['moveTo', 'lineTo']), 'the far edge, on the flat map').toBe(true)
    expect(drewAt(mapCanvas(r.container), tip[0], tip[1] - 3, ['fillText']), 'its tag, at the far edge').toBe(true)
  })
})

describe('on the Globe, a far-side marker is not drawn through the planet — and a near-side one is', () => {
  it('stations (my decodes)', async () => {
    layers(['stations'])
    const r = await mount({ stations: [station('VK2FAR', 'QF56'), station('W9NEAR', 'EN61')] })
    const canvas = mapCanvas(r.container)
    const far = at(gridToLatLon('QF56')!)
    const near = at(gridToLatLon('EN61')!)
    expect(drewAt(canvas, near[0], near[1], ['arc']), 'the near station').toBe(true)
    expect(drewAt(canvas, far[0], far[1], ['arc']), 'the far station, through the globe').toBe(false)
    fireEvent.pointerMove(canvas, { clientX: far[0], clientY: far[1] })
    expect(r.container.querySelector('.map-hover'), 'a card for a station behind the planet').toBeNull()
    fireEvent.pointerMove(canvas, { clientX: near[0], clientY: near[1] })
    expect(r.container.querySelector('.map-hover')?.textContent, 'CONTROL: the near station hovers').toContain('W9NEAR')
  })

  it('live spots', async () => {
    layers(['liveSpots'])
    const r = await mount({ prop: snap({ spots: [spot('VK2FAR', FAR), spot('W9NEAR', NEAR)] }) })
    const far = at(FAR)
    const near = at(NEAR)
    expect(drewAt(mapCanvas(r.container), near[0], near[1], ['arc']), 'the near spot').toBe(true)
    expect(drewAt(mapCanvas(r.container), far[0], far[1], ['arc']), 'the far spot, through the globe').toBe(false)
  })

  it('a far-side spot cannot be hovered through the globe either', async () => {
    layers(['liveSpots'])
    const r = await mount({ prop: snap({ spots: [spot('VK2FAR', FAR), spot('W9NEAR', NEAR)] }) })
    const canvas = mapCanvas(r.container)
    const far = at(FAR)
    fireEvent.pointerMove(canvas, { clientX: far[0], clientY: far[1] })
    expect(r.container.querySelector('.map-hover'), 'a card for a spot behind the planet').toBeNull()
    const near = at(NEAR)
    fireEvent.pointerMove(canvas, { clientX: near[0], clientY: near[1] })
    expect(r.container.querySelector('.map-hover')?.textContent, 'CONTROL: the near spot hovers').toContain('W9NEAR')
  })

  it('the band heat glow of the live spots', async () => {
    layers(['heat'])
    await mount({ prop: snap({ spots: [spot('VK2FAR', FAR), spot('W9NEAR', NEAR)] }) })
    // The glow's own offscreen canvas, a third of the map's size, and its last frame.
    const heat = [...opsOf.keys()].find((c) => c.width === Math.floor(W / 3) && c.height === Math.floor(H / 3))!
    const ops = opsOf.get(heat) ?? []
    const splats = ops.slice(ops.map((o) => o.k).lastIndexOf('clearRect')).filter((o) => o.k === 'arc')
    const near = at(NEAR)
    expect(drawAt(splats, near[0] / 3, near[1] / 3), 'CONTROL: the near spot glows').toBe(true)
    expect(splats.length, 'glows drawn: the near spot’s alone, none through the globe').toBe(1)
  })

  it('DXpeditions', async () => {
    layers(['dxped'])
    const card = (call: string, brg: number, km: number) =>
      ({ call, entity: 'X', need: 'Atno', band: '20m', bearingDeg: brg, distanceKm: km, octant: 'W', status: 'WorkNow', likelihood: 'Good', likelihoodScore: 0.7, liveConfirmed: false, howToCall: '', windowHint: '', priority: 1 }) as unknown as WorkableCard
    const r = await mount({ prop: snap({ dxpeditions: { workableNow: [card('FAR1X', 270, 15_000), card('NEAR1X', 90, 1_000)] } as never }) })
    const far = at(destinationPoint(ME, 270, 15_000))
    const near = at(destinationPoint(ME, 90, 1_000))
    expect(drewAt(mapCanvas(r.container), near[0], near[1], ['fillText']), 'the near DXpedition').toBe(true)
    expect(drewAt(mapCanvas(r.container), far[0], far[1], ['fillText']), 'the far DXpedition, through the globe').toBe(false)
  })

  it('parks on the air', async () => {
    layers(['ota'])
    const park = (activator: string, ll: LatLon) => ({ program: 'POTA', reference: 'X-0001', name: 'Park', activator, freqMhz: 14.074, mode: 'SSB', lat: ll.lat, lon: ll.lon, approx: false, ageSecs: 60, newRef: true })
    const r = await mount({ ota: [park('VK2FAR', FAR), park('W9NEAR', NEAR)] })
    // The triangle's apex, 5 px above the park.
    const far = at(FAR)
    const near = at(NEAR)
    expect(drewAt(mapCanvas(r.container), near[0], near[1] - 5, ['moveTo']), 'the near park').toBe(true)
    expect(drewAt(mapCanvas(r.container), far[0], far[1] - 5, ['moveTo']), 'the far park, through the globe').toBe(false)
  })

  it('APRS stations', async () => {
    layers(['aprs'])
    const now = 1_790_000_000
    const aprs = (call: string, ll: LatLon) =>
      ({ call, lat: ll.lat, lon: ll.lon, symbolTable: '/', symbolCode: '-', kind: 'position', text: '', speedKnots: null, courseDeg: null, path: [], raw: '', lastHeardUnix: now, lastRfUnix: now, lastInetUnix: null, sourceKind: 'rf', packets: 1, firstHeardUnix: now, wx: null }) as unknown as AprsStation
    const r = await mount({ aprs: [aprs('VK2FAR', FAR), aprs('W9NEAR', NEAR)], aprsNowSec: now })
    const far = at(FAR)
    const near = at(NEAR)
    expect(drewAt(mapCanvas(r.container), near[0], near[1], ['arc']), 'the near APRS station').toBe(true)
    expect(drewAt(mapCanvas(r.container), far[0], far[1], ['arc']), 'the far APRS station, through the globe').toBe(false)
  })

  it('satellites: the bird, and its trail', async () => {
    layers(['sats'])
    vi.useFakeTimers({ now: DUSK })
    const t0 = DUSK / 1000
    // Each bird's ground track: three points around its position, a minute apart, now in the middle.
    const track = (ll: LatLon): [number, number, number][] => [
      [t0 - 60, ll.lat - 1, ll.lon - 2],
      [t0, ll.lat, ll.lon],
      [t0 + 60, ll.lat + 1, ll.lon + 2],
    ]
    const bird = (name: string, ll: LatLon) => ({ name, norad: null, lat: ll.lat, lon: ll.lon, altKm: 420, footprintKm: 2200, track: track(ll), amateur: true, status: 'alive', classes: ['fm'] })
    feeds.sats = { tleAgeDays: 1, usableCount: 2, agingCount: 0, heldBackCount: 0, tleFetchedAt: 0, tleSource: 'bundled', birds: [bird('FARBIRD', FAR), bird('NEARBIRD', NEAR)] } as unknown as SatView
    const r = await mount()
    const canvas = mapCanvas(r.container)
    const far = at(FAR)
    const near = at(NEAR)
    expect(drewAt(canvas, near[0], near[1], ['translate']), 'the near bird').toBe(true)
    expect(drewAt(canvas, far[0], far[1], ['translate']), 'the far bird, through the globe').toBe(false)
    const farTrail = at({ lat: FAR.lat + 1, lon: FAR.lon + 2 })
    const nearTrail = at({ lat: NEAR.lat + 1, lon: NEAR.lon + 2 })
    expect(drewAt(canvas, nearTrail[0], nearTrail[1], ['lineTo']), 'the near bird’s track').toBe(true)
    expect(drewAt(canvas, farTrail[0], farTrail[1], ['lineTo']), 'the far bird’s track, through the globe').toBe(false)
  })

  it('a chased bird behind the globe: the part of its footprint on the face is still drawn', async () => {
    layers(['sats'])
    // 11 000 km west of EN52, 99° round the planet: the bird is behind the limb, and its 2 200 km
    // footprint reaches back over it. The footprint is an area, which the map clips at the horizon.
    const ll = destinationPoint(ME, 270, 11_000)
    const bird = { name: 'FARBIRD', norad: null, lat: ll.lat, lon: ll.lon, altKm: 420, footprintKm: 2200, track: [], amateur: true, status: 'alive', classes: ['fm'] }
    feeds.sats = { tleAgeDays: 1, usableCount: 1, agingCount: 0, heldBackCount: 0, tleFetchedAt: 0, tleSource: 'bundled', birds: [bird] } as unknown as SatView
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['FARBIRD']))
    const r = await mount()
    expect(inView('globe', makeProjection('globe', ME, W, H, HOME), ll), 'CONTROL: the bird is behind the globe').toBe(false)
    // A chased bird's footprint ring is the one dashed [4, 4].
    const ops = opsOf.get(mapCanvas(r.container)) ?? []
    const dash = ops.map((o) => o.k === 'setLineDash' && JSON.stringify(o.a[0]) === '[4,4]').lastIndexOf(true)
    const stroke = ops.findIndex((o, i) => i > dash && o.k === 'stroke')
    const ring = dash < 0 ? [] : ops.slice(dash, stroke).filter((o) => o.k === 'moveTo' || o.k === 'lineTo')
    expect(ring.length, 'the part of its footprint facing the viewer').toBeGreaterThan(0)
  })

  it('ionosonde MUF diamonds, drawn and hovered', async () => {
    layers(['muf'])
    const sonde = (ll: LatLon) => ({ lat: ll.lat, lon: ll.lon, mufMhz: 14, fof2Mhz: 5, ageSecs: 60, confidence: 1 })
    const r = await mount({ muf: [sonde(FAR), sonde(NEAR)] })
    const canvas = mapCanvas(r.container)
    // The diamond's top point, 3.8 px above the sonde.
    const far = at(FAR)
    const near = at(NEAR)
    expect(drewAt(canvas, near[0], near[1] - 3.8, ['moveTo']), 'the near sonde').toBe(true)
    expect(drewAt(canvas, far[0], far[1] - 3.8, ['moveTo']), 'the far sonde, through the globe').toBe(false)
    fireEvent.pointerMove(canvas, { clientX: far[0], clientY: far[1] })
    expect(r.container.querySelector('.map-hover'), 'a card for a sonde behind the planet').toBeNull()
  })

  it('the aurora oval', async () => {
    layers(['aurora'])
    feeds.aurora = [{ ...FAR, prob: 50 }, { ...NEAR, prob: 50 }]
    const r = await mount()
    const far = at(FAR)
    const near = at(NEAR)
    expect(drewAt(mapCanvas(r.container), near[0], near[1], ['arc']), 'the near aurora point').toBe(true)
    expect(drewAt(mapCanvas(r.container), far[0], far[1], ['arc']), 'the far aurora point, through the globe').toBe(false)
  })

  it('the proton polar cap', async () => {
    layers(['pca'])
    feeds.pca = { j10: 100, a30Day: 5, a30Night: 2, cutoffDeg: 60, points: [{ ...FAR, db30: 5 }, { ...NEAR, db30: 5 }] }
    const r = await mount()
    const far = at(FAR)
    const near = at(NEAR)
    expect(drewAt(mapCanvas(r.container), near[0], near[1], ['arc']), 'the near PCA point').toBe(true)
    expect(drewAt(mapCanvas(r.container), far[0], far[1], ['arc']), 'the far PCA point, through the globe').toBe(false)
  })

  it('the flare field: only the sunlit samples on the face of the globe are splatted', async () => {
    layers(['flare'])
    vi.useFakeTimers({ now: DUSK })
    await mount({ xrayLong: 2e-4 })
    const proj = makeProjection('globe', ME, W, H, HOME)
    const samples = flareField(DUSK, 2e-4)
    const facing = samples.filter((s) => inView('globe', proj, { lat: s.lat, lon: s.lon }))
    expect(samples.length - facing.length, 'CONTROL: at this hour part of the sunlit side is behind the globe').toBeGreaterThan(0)
    // The field's own offscreen canvas, a third of the map's size.
    const field = [...opsOf.keys()].find((c) => c.width === Math.floor(W / 3) && c.height === Math.floor(H / 3))!
    // The last frame the field was drawn in: everything after its last clear.
    const ops = opsOf.get(field) ?? []
    const last = ops.map((o) => o.k).lastIndexOf('clearRect')
    const splats = ops.slice(last).filter((o) => o.k === 'arc')
    expect(splats.length, 'splats drawn: one per sunlit sample the globe shows').toBe(facing.length)
  })

  it('my own QTH, when the globe is turned away from it (the satellite detail globe centres on the bird)', async () => {
    const bird = (name: string, ll: LatLon) => ({ name, norad: null, lat: ll.lat, lon: ll.lon, altKm: 420, footprintKm: 2200, track: [], amateur: true, status: 'alive', classes: ['fm'] })
    feeds.sats = { tleAgeDays: 1, usableCount: 1, agingCount: 0, heldBackCount: 0, tleFetchedAt: 0, tleSource: 'bundled', birds: [bird('FARBIRD', FAR), bird('NEARBIRD', NEAR)] } as unknown as SatView
    const qthGlow = (c: HTMLCanvasElement, view: MapView3) => {
      const p = at(ME, 'globe', view)
      return (opsOf.get(c) ?? []).some((o) => o.k === 'arc' && Math.hypot((o.a[0] as number) - p[0], (o.a[1] as number) - p[1]) < 0.5 && Math.abs((o.a[2] as number) - 16) < 0.01)
    }
    const away = await mount({ embedded: { focusSat: 'FARBIRD' } })
    expect(qthGlow(mapCanvas(away.container), { ...HOME, rotate: [-FAR.lon, -FAR.lat] }), 'the QTH, drawn through the globe').toBe(false)
    cleanup()
    opsOf.clear()
    const toward = await mount({ embedded: { focusSat: 'NEARBIRD' } })
    expect(qthGlow(mapCanvas(toward.container), { ...HOME, rotate: [-NEAR.lon, -NEAR.lat] }), 'CONTROL: facing it, the QTH is drawn').toBe(true)
  })

  it('the long path of a selected station: the stretch behind the globe is not drawn across its face', async () => {
    layers(['stations', 'paths'])
    const r = await mount({ stations: [station('W9NEAR', 'EN61')], selectedCall: 'W9NEAR' })
    await act(async () => void fireEvent.click(screen.getByRole('button', { name: 'LP' })))
    const canvas = mapCanvas(r.container)
    // The long path's own samples, as the map takes them: 48 steps round the other way.
    const sll = gridToLatLon('EN61')!
    const brg = (bearingDeg(ME, sll) + 180) % 360
    const spKm = haversineKm(ME, sll)
    const proj = makeProjection('globe', ME, W, H, HOME)
    const samples = Array.from({ length: 49 }, (_, i) => destinationPoint(ME, brg, ((40_075 - spKm) * i) / 48))
    const facing = samples.filter((s) => inView('globe', proj, s))
    expect(samples.length - facing.length, 'CONTROL: most of a long path is behind the globe').toBeGreaterThan(10)
    // The long path's stroke — its dashes are the [5, 4] pattern — draws one point per sample it
    // shows. Counted rather than looked up by position: near the horizon a point behind the globe and
    // one in front of it land on the same pixel, so "was something drawn there" cannot tell them apart.
    const ops = opsOf.get(canvas) ?? []
    const dash = ops.map((o) => o.k === 'setLineDash' && JSON.stringify(o.a[0]) === '[5,4]').lastIndexOf(true)
    const stroke = ops.findIndex((o, i) => i > dash && o.k === 'stroke')
    const drawn = ops.slice(dash, stroke).filter((o) => o.k === 'moveTo' || o.k === 'lineTo')
    expect(dash, 'CONTROL: the long path was drawn').toBeGreaterThan(-1)
    expect(drawn.length, 'points of the long path drawn: only those facing the viewer').toBe(facing.length)
    for (const s of facing) {
      const p = at(s)
      expect(drawAt(drawn, p[0], p[1]), `the facing sample at ${s.lat.toFixed(1)}, ${s.lon.toFixed(1)}`).toBe(true)
    }
  })

  it('a roster whose only station is behind the globe is not called empty', async () => {
    layers(['stations'])
    const hint = (c: HTMLElement) => [...c.querySelectorAll('.map-empty-hint')].find((e) => /No located stations/.test(e.textContent ?? ''))
    const r = await mount({ stations: [station('VK2FAR', 'QF56')] })
    expect(hint(r.container), '"No located stations yet", with one located behind the planet').toBeUndefined()
    cleanup()
    const empty = await mount({ stations: [] })
    expect(hint(empty.container), 'CONTROL: with none located, the hint is there').toBeDefined()
  })

  it('the grid labels on the base map (Maidenhead fields)', async () => {
    // Big enough that a 20° field clears the 70 px the labels need.
    W = 1400
    H = 1000
    layers(['gridLabels'])
    await mount()
    expect(texts(), 'CONTROL: the near field is labelled').toContain('EN')
    expect(texts(), 'Sydney’s field, labelled through the globe').not.toContain('QF')
  })

  it('the grid-square labels of a zoomed-in globe (Maidenhead squares)', async () => {
    // A wall display, the globe turned to centre 45°S 0°E and zoomed in twice: a field this close
    // is labelled square by square, and some of its squares are behind the planet's limb.
    W = 3840
    H = 2160
    layers(['gridLabels'])
    const r = await mount()
    const canvas = mapCanvas(r.container)
    // The drag the map turns 0.32° per pixel at zoom 1: EN52's centre to 45°S 0°E.
    const dx = -278.125
    const dy = -273.4375
    fireEvent.pointerDown(canvas, { clientX: 1000, clientY: 1000, pointerId: 1 })
    fireEvent.pointerMove(canvas, { clientX: 1000 + dx, clientY: 1000 + dy, pointerId: 1 })
    fireEvent.pointerUp(canvas, { clientX: 1000 + dx, clientY: 1000 + dy, pointerId: 1 })
    const zoomIn = () => act(async () => void fireEvent.click(screen.getByRole('button', { name: 'Zoom in' })))
    await zoomIn()
    // Only what is drawn at the final view counts.
    for (const ops of opsOf.values()) ops.length = 0
    await zoomIn()
    const view: MapView3 = { zoom: 1 * 1.3 * 1.3, rotate: [-ME.lon + dx * 0.32, -ME.lat - dy * 0.32], panX: 0, panY: 0 }
    const proj = makeProjection('globe', ME, W, H, view)
    const squares = [...new Set(texts().filter((t) => /^[A-R]{2}\d\d$/.test(t)))]
    expect(squares.length, 'CONTROL: zoomed in this far, the squares are labelled').toBeGreaterThan(0)
    // The fields labelled square by square, and every square of them that lands on the screen.
    const fields = new Set(squares.map((q) => q.slice(0, 2)))
    const onScreen: string[] = []
    for (const f of fields) {
      for (let i = 0; i < 100; i++) {
        const q = `${f}${Math.floor(i / 10)}${i % 10}`
        const p = project(proj, gridToLatLon(q)!)!
        if (p[0] >= 0 && p[0] <= W && p[1] >= 0 && p[1] <= H) onScreen.push(q)
      }
    }
    const behind = onScreen.filter((q) => !inView('globe', proj, gridToLatLon(q)!))
    expect(behind.length, 'CONTROL: some squares of those fields are behind the globe, over the screen').toBeGreaterThan(0)
    expect(squares.filter((q) => behind.includes(q)), 'squares labelled through the globe').toEqual([])
  })

  it('the CQ-zone numbers', async () => {
    layers(['cqzones'])
    const zone = (n: number, ll: LatLon) => ({
      type: 'Feature',
      properties: { cq_zone_number: n, cq_zone_name: `Z${n}`, cq_zone_name_loc: [ll.lat, ll.lon] },
      geometry: { type: 'Polygon', coordinates: [[[ll.lon - 1, ll.lat - 1], [ll.lon + 1, ll.lat - 1], [ll.lon + 1, ll.lat + 1], [ll.lon - 1, ll.lat - 1]]] },
    })
    vi.stubGlobal('fetch', vi.fn(async () => ({ json: async () => ({ type: 'FeatureCollection', features: [zone(30, FAR), zone(4, NEAR)] }) })))
    await mount()
    expect(texts(), 'CONTROL: the near zone is numbered').toContain('4')
    expect(texts(), 'the far zone, numbered through the globe').not.toContain('30')
  })

  it('the opening sectors: a wedge reaching behind the globe stops at its horizon', async () => {
    layers(['openings'])
    // A 20 m F2 opening over the pole toward SE Asia: its far edge, 15,000 km out, is 135° from EN52.
    const o = opening('20m', 'F2', 340, 15_000)
    const r = await mount({ prop: snap({ openings: [o] }) })
    const canvas = mapCanvas(r.container)
    const proj = makeProjection('globe', ME, W, H, HOME)
    const edge = farEdge(o)
    expect(edge.filter((ll) => inView('globe', proj, ll)), 'CONTROL: the whole far edge is behind the globe').toEqual([])
    // Between the two radials: a radial's own end lands on that radial's line, where its facing part is drawn.
    for (const ll of edge.slice(1, -1)) {
      const p = at(ll)
      expect(drewAt(canvas, p[0], p[1], ['moveTo', 'lineTo']), `the far edge at ${ll.lat.toFixed(1)}, ${ll.lon.toFixed(1)}, through the globe`).toBe(false)
    }
    // What faces the viewer is still drawn: from the QTH out to the horizon, between the wedge's own radials.
    const wedge = pathThrough(canvas, at(ME))
    expect(wedge.length, 'the facing part of the wedge, from the QTH').toBeGreaterThan(0)
    const R = (Math.min(W, H) / 2) * 0.92
    const onLimb = wedge.filter((op) => (op.k === 'moveTo' || op.k === 'lineTo') && Math.abs(Math.hypot((op.a[0] as number) - W / 2, (op.a[1] as number) - H / 2) - R) < 0.5)
    expect(onLimb.length, 'the facing part of the wedge, out to the horizon').toBeGreaterThan(0)
    // The globe is centred on the QTH, so a point's direction from the centre is its bearing from home.
    const brg = (op: Op) => (Math.atan2((op.a[0] as number) - W / 2, H / 2 - (op.a[1] as number)) * 180) / Math.PI
    const outside = onLimb.filter((op) => Math.abs(((brg(op) - o.bearingDeg + 540) % 360) - 180) > 23)
    expect(outside.length, "the horizon it fills to, between its radials (not the rest of the globe's)").toBe(0)
  })

  it('the opening sectors: a far edge behind the globe is not tagged through it, and a near one still is', async () => {
    layers(['openings'])
    const near = opening('2m', 'Tropo', 90, 1_200)
    const r = await mount({ prop: snap({ openings: [opening('20m', 'F2', 340, 15_000), near] }) })
    const tag = at(farEdge(near)[8])
    expect(drewAt(mapCanvas(r.container), tag[0], tag[1] - 3, ['fillText']), 'CONTROL: the near opening, tagged at its far edge').toBe(true)
    expect(texts(), 'CONTROL: its tag reads band and mode').toContain('2m Tropo')
    expect(texts(), 'the far opening, tagged through the globe').not.toContain('20m F2')
  })

  it('the opening sectors, on a globe turned away from the QTH: neither the wedge nor its tag', async () => {
    layers(['openings'])
    const o = opening('2m', 'Tropo', 90, 1_200)
    const r = await mount({ prop: snap({ openings: [o] }) })
    const canvas = mapCanvas(r.container)
    expect(drewAt(canvas, at(ME)[0], at(ME)[1], ['moveTo', 'lineTo']), 'CONTROL: facing the QTH, its wedge is drawn').toBe(true)
    expect(texts(), 'CONTROL: and tagged').toContain('2m Tropo')
    // Spin the globe to face Australia, as the operator drags it (0.32° per pixel at zoom 1): 25°S 135°E, 140° from EN52.
    const dx = 425
    const dy = -211
    const view: MapView3 = { ...HOME, rotate: [-ME.lon + dx * 0.32, -ME.lat - dy * 0.32] }
    for (const ops of opsOf.values()) ops.length = 0
    await act(async () => {
      fireEvent.pointerDown(canvas, { clientX: 1000, clientY: 1000, pointerId: 1 })
      fireEvent.pointerMove(canvas, { clientX: 1000 + dx, clientY: 1000 + dy, pointerId: 1 })
      fireEvent.pointerUp(canvas, { clientX: 1000 + dx, clientY: 1000 + dy, pointerId: 1 })
    })
    expect((opsOf.get(canvas) ?? []).some((op) => op.k === 'drawImage'), 'CONTROL: the turned globe was drawn').toBe(true)
    const proj = makeProjection('globe', ME, W, H, view)
    const corners = [ME, ...farEdge(o)]
    expect(corners.filter((ll) => inView('globe', proj, ll)), 'CONTROL: the whole wedge is behind the turned globe').toEqual([])
    for (const ll of corners) {
      const p = at(ll, 'globe', view)
      expect(drewAt(canvas, p[0], p[1], ['moveTo', 'lineTo']), `the wedge's corner at ${ll.lat.toFixed(1)}, ${ll.lon.toFixed(1)}, through the globe`).toBe(false)
    }
    expect(texts(), 'its tag, through the globe').not.toContain('2m Tropo')
    const traced = (opsOf.get(canvas) ?? []).filter((op) => op.k === 'moveTo' || op.k === 'lineTo')
    expect(traced.length, 'any outline at all on the face of the turned globe, the wedge being wholly behind it').toBe(0)
  })
})
