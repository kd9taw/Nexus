// @vitest-environment jsdom
//
// THE APRS MAP HAS THE CONNECT MAP'S ABILITIES. It is the same MapView, embedded for APRS, and it
// used to be a locked Globe with no toolbar, no Layers panel and no trails. What this pins:
//   - the host's map pick is drawn (Flat, Beam, Street…), not overridden to the Globe;
//   - a toolbar with the host's own controls, zoom and Centre (and no Full screen: it would cover
//     the cockpit's TX strip);
//   - a Layers panel offering only the layers that mean something on a packet map, kept per window
//     in the APRS map's own record, never the Connect map's;
//   - each moving station's trail drawn from its track, only while trails are on;
//   - labels for every station when Labels are on, with no two lettered on top of each other.
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ComponentProps } from 'react'
import type { AprsStation } from '../api'
import type { NeedTag, Station } from '../types'

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

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

/** Every stroke the map draws (its ink and the points of its path) and every text it letters. */
type Stroke = { ink: string; pts: Array<[number, number]> }
let strokes: Stroke[] = []
let texts: Array<{ text: string; x: number; y: number }> = []
function recordingContext(canvas: HTMLCanvasElement) {
  const state: Record<string | symbol, unknown> = {}
  let path: Array<[number, number]> = []
  return new Proxy(state, {
    get: (t, k) => {
      if (k in t) return t[k]
      if (k === 'canvas') return canvas
      return (...a: unknown[]) => {
        if (k === 'beginPath') path = []
        if (k === 'moveTo' || k === 'lineTo') path.push([Number(a[0]), Number(a[1])])
        if (k === 'stroke') strokes.push({ ink: String(t.strokeStyle), pts: path.slice() })
        if (k === 'fillText') texts.push({ text: String(a[0]), x: Number(a[1]), y: Number(a[2]) })
        if (k === 'measureText') return { width: String(a[0]).length * 6 }
        return { addColorStop() {} }
      }
    },
  })
}

const W = 1000
const H = 800
const NOW = Math.floor(Date.now() / 1000)
beforeEach(() => {
  localStorage.clear()
  strokes = []
  texts = []
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  // The symbols are glyph paths at the APRS map's local zoom; jsdom has no Path2D.
  ;(globalThis as unknown as { Path2D: unknown }).Path2D = class {
    moveTo() {}
    lineTo() {}
    closePath() {}
    arc() {}
    rect() {}
    bezierCurveTo() {}
    quadraticCurveTo() {}
    addPath() {}
  }
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
})

function station(call: string, over: Partial<AprsStation> = {}): AprsStation {
  return {
    call,
    lat: 39.0,
    lon: -95.7,
    symbolTable: '/',
    symbolCode: '-',
    kind: 'position',
    text: '',
    speedKnots: null,
    courseDeg: null,
    path: [],
    raw: '',
    lastHeardUnix: NOW - 30,
    lastRfUnix: NOW - 30,
    lastInetUnix: null,
    sourceKind: 'rf',
    packets: 3,
    firstHeardUnix: NOW - 900,
    wx: null,
    ...over,
  }
}
/** A car that drove north-east, its trail in the store's track. */
const CAR = station('W9CAR-9', {
  symbolCode: '>',
  lat: 39.06,
  lon: -95.6,
  speedKnots: 40,
  courseDeg: 45,
  track: [
    { lat: 39.0, lon: -95.7, atUnix: NOW - 600 },
    { lat: 39.03, lon: -95.65, atUnix: NOW - 300 },
    { lat: 39.06, lon: -95.6, atUnix: NOW - 30 },
  ],
})

const noStations: Station[] = []
const noNeeds = new Map<string, NeedTag>()
type P = Partial<ComponentProps<typeof MapView>>
async function mount(p: P = {}) {
  const view = render(
    <MapView
      embedded={{ aprs: true }}
      aprs={[CAR, station('W9FIX')]}
      aprsNowSec={NOW}
      myGrid="EM28"
      theme="dark"
      stations={noStations}
      prop={null}
      selectedCall={null}
      onSelectCall={() => {}}
      needByCall={noNeeds}
      {...p}
    />,
  )
  await act(async () => {
    for (let i = 0; i < 4; i++) await Promise.resolve()
  })
  return view
}

/** A drawn polyline through (about) these screen points, in order — the trail. */
const trailStrokes = () => strokes.filter((s) => s.pts.length === 3)

describe('the APRS map', () => {
  it('draws the map the host picked instead of locking itself to the Globe', async () => {
    const { container } = await mount({ projection: 'world' })
    expect(container.querySelector('.map-view')!.getAttribute('data-projection')).toBe('world')
  })

  it('has a toolbar: the host’s controls, zoom in and out, Centre — and no Full screen', async () => {
    const onCentre = vi.fn()
    await mount({ aprsToolbar: <button type="button">host-control</button>, onAprsCentre: onCentre })
    expect(screen.getByRole('button', { name: 'host-control' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Zoom in' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Zoom out' })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /Centre/ }))
    expect(onCentre).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('button', { name: /Full screen/i })).toBeNull()
  })

  it('offers only the packet map’s layers, and keeps the picks in its own record', async () => {
    await mount()
    const names = screen.getAllByRole('checkbox').map((c) => c.closest('label')!.textContent ?? '')
    expect(names).toEqual([
      'Relief (World view)',
      'Coastlines',
      'US states',
      'Grid (20°×10°)',
      'Grid labels (AA…RR)',
      'Satellites (amateur)',
      'APRS stations',
    ])
    // No Connect-only layer leaks in.
    expect(names.join(' ')).not.toMatch(/MUF|Aurora|Heat/)
    fireEvent.click(screen.getByRole('checkbox', { name: /Grid labels/ }))
    expect(JSON.parse(localStorage.getItem('nexus.aprs.map.layers')!).gridLabels).toBe(true)
    cleanup()
    await mount()
    expect((screen.getByRole('checkbox', { name: /Grid labels/ }) as HTMLInputElement).checked).toBe(true)
  })

  it('draws a moving station’s trail from its track while trails are on, and not while off', async () => {
    await mount({ aprsTracks: true })
    expect(trailStrokes().length).toBe(1)
    const [a, b, c] = trailStrokes()[0].pts
    // Heading north-east on screen: right and up.
    expect(b[0]).toBeGreaterThan(a[0])
    expect(c[0]).toBeGreaterThan(b[0])
    expect(c[1]).toBeLessThan(a[1])
    cleanup()
    strokes = []
    await mount({ aprsTracks: false })
    expect(trailStrokes().length).toBe(0)
  })

  it('holds the trail to the age filter', async () => {
    await mount({ aprsTracks: true, aprsTrailMin: 2 })
    // Two of the three points are older than two minutes: one point is left, which is no trail.
    expect(trailStrokes().length).toBe(0)
  })

  it('labels every station with Labels on, and only the moving one with Labels off', async () => {
    await mount({ aprsLabels: true })
    expect(new Set(texts.map((x) => x.text))).toEqual(new Set(['W9CAR-9', 'W9FIX']))
    cleanup()
    texts = []
    await mount({ aprsLabels: false })
    expect(texts.map((x) => x.text)).toEqual(['W9CAR-9'])
  })

  it('never letters two labels on top of each other in a crowd', async () => {
    const crowd = Array.from({ length: 12 }, (_, i) => station(`W9C${String.fromCharCode(65 + i)}`, { lat: 39.0 + i * 0.0005, lon: -95.7 }))
    await mount({ aprs: crowd, aprsLabels: true })
    const boxes = texts.map((x) => ({ x0: x.x, x1: x.x + x.text.length * 6, y0: x.y - 6, y1: x.y + 6 }))
    expect(boxes.length).toBeGreaterThan(0)
    expect(boxes.length).toBeLessThan(crowd.length)
    for (let i = 0; i < boxes.length; i++)
      for (let j = i + 1; j < boxes.length; j++) {
        const r = boxes[i]
        const q = boxes[j]
        expect(r.x0 < q.x1 && q.x0 < r.x1 && r.y0 < q.y1 && q.y0 < r.y1).toBe(false)
      }
  })

  it('follows a station: it is drawn in the middle of the map', async () => {
    await mount({ aprsFollow: 'W9CAR-9', aprsLabels: true })
    const label = texts.find((x) => x.text === 'W9CAR-9')!
    // The label sits just beside its symbol, which is at the centre.
    expect(Math.abs(label.y - H / 2)).toBeLessThan(30)
    expect(Math.abs(label.x - W / 2)).toBeLessThan(60)
  })
})
