// @vitest-environment jsdom
//
// MAP READABILITY on a wall display (operator, 2026-09-08: the map popped out full screen on
// a second monitor — "make the icons larger, sharper and brighter", "the text box larger and
// easier to read").
//
// Two properties are tested here because both were WRONG in a way no rendering test could
// see and no reviewer noticed by reading:
//
//  1. THE BACKING STORE MUST BE THE CANVAS'S REAL DEVICE-PIXEL BOX. MapView sized it
//     `wrap.clientWidth × window.devicePixelRatio`. `clientWidth` is LAYOUT px, and the app
//     magnifies the whole UI with `.app { zoom: var(--ui-zoom) }`, so the canvas actually
//     occupies `layout × zoom × dpr` device pixels. At any UI scale above 100% the bitmap was
//     therefore SMALLER than the box it paints into and the compositor upscaled it — that is
//     the blur, and it is exactly the bug `Waterfall.tsx` fixed with
//     `devicePixelContentBoxSize` (its comment names the same "zoom × dpr" quantity).
//     `dpr === 1 && zoom === 1` hides it completely, which is why it survived.
//
//  2. THE HOVER CARD MUST ANCHOR TO THE MARKER, NOT THE CURSOR. It was placed at
//     `cursor + 12px`, so it slid under the pointer through the whole ~20 px-wide hit target
//     of a single icon — the jitter reads as flicker on a big screen, and a card sized for
//     distance reading is a large object to have swimming around. Anchoring it to the hit's
//     projected position makes it dead still while the pointer is on one icon and move
//     exactly once when the pointer crosses to another.
//
// WHY THE FULL COMPONENT (and the jsdom canvas stub / projection-center trick): see the
// header of MapView.ota-doubleclick.test.tsx — the same harness, for the same reasons.
//
// WHAT THIS FILE CANNOT PROVE ON ITS OWN: jsdom performs no layout (`offsetWidth` is 0, every
// `getBoundingClientRect()` is all-zero), so no test here renders the card and measures it. The
// clamping and edge-flipping are therefore checked as the pure `placeHoverCard`, and the CARD
// SIZES it is checked against were measured in a real browser — see the last describe block,
// which is where the numbers and the method are recorded.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, fireEvent, act } from '@testing-library/react'
import { MapView, markerScaleFor, placeHoverCard } from './MapView'
import { gridToLatLon } from '../grid'
import type { OtaMapSpot } from '../types'

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLog: vi.fn(async () => []),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
}))

/** A ResizeObserver that records which element each callback was given, so a test can drive
 *  the CANVAS observer specifically (MapView runs one on its wrap for layout size and one on
 *  the canvas for the device-pixel scale). */
type Observed = { el: Element; cb: ResizeObserverCallback; box?: string }
let observers: Observed[] = []
class RecordingRO {
  cb: ResizeObserverCallback
  constructor(cb: ResizeObserverCallback) {
    this.cb = cb
  }
  observe(el: Element, opts?: { box?: string }) {
    observers.push({ el, cb: this.cb, box: opts?.box })
  }
  unobserve() {}
  disconnect() {}
}

/** See MapView.ota-doubleclick.test.tsx: jsdom has no 2D context, and MapView's draw effect
 *  calls a wide swath of it purely to paint. */
function fakeCtx(): CanvasRenderingContext2D {
  const self: object = new Proxy(function fakeCtxTarget() {}, {
    get(_t, prop) {
      if (prop === 'measureText') return () => ({ width: 10 })
      if (prop === 'getImageData')
        return (_x: number, _y: number, w: number, h: number) => ({
          data: new Uint8ClampedArray(Math.max(0, w) * Math.max(0, h) * 4),
          width: w,
          height: h,
        })
      return self
    },
    set() {
      return true
    },
    apply() {
      return self
    },
  })
  return self as CanvasRenderingContext2D
}

const W = 1000
const H = 800
const MY_GRID = 'EN52'
const ME = gridToLatLon(MY_GRID)!

/** One park at the operator's own QTH → it projects to the canvas centre under the default
 *  globe view (mapGeo's `globe` branch rotates the operator to centre). */
const PARK: OtaMapSpot = {
  program: 'POTA',
  reference: 'K-1234',
  name: 'Test Park',
  activator: 'W1AW',
  freqMhz: 14.074,
  mode: 'FT8',
  lat: ME.lat,
  lon: ME.lon,
  approx: false,
  ageSecs: 30,
  newRef: true,
}

let clientWidthDesc: PropertyDescriptor | undefined
let clientHeightDesc: PropertyDescriptor | undefined

beforeEach(() => {
  localStorage.clear()
  observers = []
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RecordingRO
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(fakeCtx())
  clientWidthDesc = Object.getOwnPropertyDescriptor(Element.prototype, 'clientWidth')
  clientHeightDesc = Object.getOwnPropertyDescriptor(Element.prototype, 'clientHeight')
  Object.defineProperty(Element.prototype, 'clientWidth', { configurable: true, get: () => W })
  Object.defineProperty(Element.prototype, 'clientHeight', { configurable: true, get: () => H })
})

afterEach(async () => {
  // DRAIN BEFORE UNMOUNT. MapView's mount effects kick off the space-weather/OTA fetches; their
  // promises settle in a later microtask+macrotask and schedule a concurrent React render. Tear
  // the jsdom environment down with one of those still queued and the scheduler flushes it into
  // a dead global — "ReferenceError: window is not defined" inside react-dom's commitRoot,
  // reported against whatever file the worker had moved on to (here, the node-env
  // useScale.popout.test.ts) and failing the whole run with every test passing. Two full-suite
  // control runs without this file were clean; two with it were not.
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0))
  })
  cleanup()
  vi.restoreAllMocks()
  if (clientWidthDesc) Object.defineProperty(Element.prototype, 'clientWidth', clientWidthDesc)
  if (clientHeightDesc) Object.defineProperty(Element.prototype, 'clientHeight', clientHeightDesc)
})

function mount() {
  return render(
    <MapView
      myGrid={MY_GRID}
      theme="dark"
      stations={[]}
      prop={null}
      selectedCall={null}
      onSelectCall={() => {}}
      needByCall={new Map()}
      intent="pota"
      ota={[PARK]}
    />,
  )
}

/** A ResizeObserverEntry carrying BOTH boxes: the layout box the app lays out in and the
 *  device-pixel box the compositor actually paints. `zoom × dpr` is the ratio between them. */
function entryFor(el: Element, cssW: number, cssH: number, scale: number): ResizeObserverEntry {
  return {
    target: el,
    contentRect: { width: cssW, height: cssH, x: 0, y: 0, top: 0, left: 0, right: cssW, bottom: cssH, toJSON: () => ({}) } as DOMRectReadOnly,
    contentBoxSize: [{ inlineSize: cssW, blockSize: cssH }],
    borderBoxSize: [{ inlineSize: cssW, blockSize: cssH }],
    devicePixelContentBoxSize: [
      { inlineSize: Math.round(cssW * scale), blockSize: Math.round(cssH * scale) },
    ],
  } as unknown as ResizeObserverEntry
}

describe('map canvas sharpness', () => {
  it('sizes the backing store to the real device-pixel box under UI zoom', () => {
    const { container } = mount()
    const canvas = container.querySelector('canvas') as HTMLCanvasElement

    // The operator's second-monitor case: UI scale pinned to 125% for distance reading, on a
    // 1× display. The canvas occupies 1250 x 1000 DEVICE pixels while laying out at 1000 x 800.
    const ZOOM = 1.25
    const canvasObs = observers.filter((o) => o.el === canvas)
    expect(canvasObs.length, 'MapView must observe the canvas for its device-pixel box').toBeGreaterThan(0)
    act(() => {
      for (const o of canvasObs) o.cb([entryFor(canvas, W, H, ZOOM)], {} as ResizeObserver)
    })

    expect(canvas.width).toBe(Math.round(W * ZOOM))
    expect(canvas.height).toBe(Math.round(H * ZOOM))
  })

  it('observes the canvas in the device-pixel-content-box (the only box that sees zoom)', () => {
    const { container } = mount()
    const canvas = container.querySelector('canvas') as HTMLCanvasElement
    expect(observers.some((o) => o.el === canvas && o.box === 'device-pixel-content-box')).toBe(true)
  })
})

describe('map hover card', () => {
  it('anchors to the marker, so it does not swim while the pointer crosses one icon', () => {
    const { container } = mount()
    const canvas = container.querySelector('canvas') as HTMLCanvasElement

    // Dead centre = the park (see PARK above).
    fireEvent.pointerMove(canvas, { clientX: W / 2, clientY: H / 2 })
    const first = container.querySelector('.map-hover') as HTMLElement
    expect(first, 'hovering the park must show the hover card').toBeTruthy()
    const at = (el: HTMLElement) => `${el.style.left}|${el.style.top}`
    const p0 = at(first)

    // Still the SAME park — 7 px away is well inside the 10 px hit target.
    fireEvent.pointerMove(canvas, { clientX: W / 2 + 7, clientY: H / 2 + 7 })
    const second = container.querySelector('.map-hover') as HTMLElement
    expect(second).toBeTruthy()
    expect(at(second), 'the card must not follow the pointer within one icon').toBe(p0)
  })

  it('never covers the marker it describes', () => {
    const { container } = mount()
    const canvas = container.querySelector('canvas') as HTMLCanvasElement
    fireEvent.pointerMove(canvas, { clientX: W / 2, clientY: H / 2 })
    const card = container.querySelector('.map-hover') as HTMLElement
    const left = parseFloat(card.style.left)
    const top = parseFloat(card.style.top)
    // The card's near corner must clear the icon by more than the hit radius.
    expect(Math.hypot(left - W / 2, top - H / 2)).toBeGreaterThan(10)
  })
})

describe('markerScaleFor', () => {
  it('never shrinks what already shipped, and never renders sub-pixel markers', () => {
    // The embedded detail globe and a small pane: unchanged from before this work.
    expect(markerScaleFor(320, 240)).toBe(1)
    expect(markerScaleFor(700, 420)).toBe(1)
    // Degenerate sizes (pre-layout, hidden host) must not produce 0 or NaN.
    expect(markerScaleFor(0, 0)).toBe(1)
    expect(markerScaleFor(1200, 0)).toBe(1)
  })

  it('grows with the map and stops before the dots merge', () => {
    // The operator's case: the map popped out full screen on a second monitor.
    const at1920 = markerScaleFor(1920, 980)
    const at2560 = markerScaleFor(2560, 1400)
    expect(at1920).toBeGreaterThan(1.4)
    expect(at2560).toBeGreaterThan(at1920)
    expect(at2560).toBeLessThanOrEqual(1.9)
    expect(markerScaleFor(3840, 2160)).toBe(1.9)
  })

  it('is governed by the SHORT side — a wide, short strip is not a wall display', () => {
    // A world map in a 1920x260 strip has no more room for a big icon than a small pane does.
    expect(markerScaleFor(1920, 260)).toBe(1)
    expect(markerScaleFor(1920, 260)).toBeLessThan(markerScaleFor(1920, 980))
  })
})

describe('placeHoverCard', () => {
  const V = { vw: 1600, vh: 900 }
  const CARD = { cw: 260, ch: 70 }

  it('sits clear of the marker, below and to the side', () => {
    const p = placeHoverCard({ ax: 800, ay: 400, ...CARD, ...V, clear: 19 })
    // The card's whole box is below the marker's row: the marker cannot be inside it whatever
    // the horizontal placement does. That is the property, not the exact offset.
    expect(p.top).toBeGreaterThanOrEqual(400 + 19)
    expect(p.left).toBeGreaterThan(800)
  })

  it('flips ABOVE the marker rather than off the bottom edge', () => {
    const p = placeHoverCard({ ax: 800, ay: 880, ...CARD, ...V, clear: 19 })
    expect(p.top + CARD.ch).toBeLessThan(880) // wholly above the marker
    expect(p.top).toBeGreaterThanOrEqual(0)
  })

  it('stays inside the map at the right edge, and clamping never re-covers the marker', () => {
    const ax = 1580
    const p = placeHoverCard({ ax, ay: 400, ...CARD, ...V, clear: 19 })
    expect(p.left + CARD.cw).toBeLessThanOrEqual(V.vw)
    expect(p.left).toBeLessThan(ax) // it HAD to move left of the anchor
    // ...which is only safe because the vertical placement already cleared the icon.
    expect(p.top).toBeGreaterThanOrEqual(400 + 19)
  })

  it('handles the bottom-right corner — both flips at once', () => {
    const p = placeHoverCard({ ax: 1580, ay: 880, ...CARD, ...V, clear: 19 })
    expect(p.left + CARD.cw).toBeLessThanOrEqual(V.vw)
    expect(p.top + CARD.ch).toBeLessThan(880)
    expect(p.left).toBeGreaterThanOrEqual(0)
    expect(p.top).toBeGreaterThanOrEqual(0)
  })

  it('does not flip before the card has been measured (cw/ch still 0)', () => {
    const p = placeHoverCard({ ax: 1580, ay: 880, cw: 0, ch: 0, ...V, clear: 19 })
    expect(p.left).toBe(1580 + 19)
    expect(p.top).toBe(880 + 19)
  })

  it('never goes negative in a box too small for the card either way', () => {
    const p = placeHoverCard({ ax: 40, ay: 40, cw: 400, ch: 300, vw: 200, vh: 120, clear: 19 })
    expect(p.left).toBeGreaterThanOrEqual(0)
    expect(p.top).toBeGreaterThanOrEqual(0)
  })
})

// ⭐ MEASURED, NOT REASONED. jsdom lays nothing out, so the card sizes below were taken from the
// REAL cascade — headless Chrome, `ui/src/styles.css` as shipped, `--map-ui-scale` at the value
// MapView stamps for each map size, and the three tooltip shapes `hitText` actually produces
// (spot, park, APRS-with-a-path). Measured 2026-09-08:
//
//   1920x1000 and 2560x1400 (scale 1.6, 19.2px): spot 812x46 (2 lines), park/APRS 832x74 (3)
//   1024x700  (scale 1.0, 12px):                 spot 517x35 (2 lines), park/APRS 520x53 (3)
//   420x300   (embedded globe, 12px):            386x53 / 386x70
//
// Reasoning about them is exactly the mistake this replaces: at the 56ch cap first written here,
// the ONE-LINE spot tooltip reflowed to three lines at 1920 — bigger type, harder to read. Only
// the measurement said so. If the padding, font token or max-width changes, re-measure and
// update these numbers rather than relaxing the assertions.
describe('placeHoverCard against measured card sizes', () => {
  const CASES = [
    { view: '1920x1000 wall display', vw: 1920, vh: 1000, clear: 19, cards: [[812, 46], [832, 74]] },
    { view: '2560x1400 wall display', vw: 2560, vh: 1400, clear: 19, cards: [[812, 46], [832, 74]] },
    { view: '1024x700 supported floor', vw: 1024, vh: 700, clear: 10, cards: [[517, 35], [520, 53]] },
    { view: '420x300 embedded globe', vw: 420, vh: 300, clear: 10, cards: [[386, 53], [386, 70]] },
  ]

  for (const c of CASES) {
    it(`keeps the card inside the map and off the marker at ${c.view}`, () => {
      const offenders: string[] = []
      for (const [cw, ch] of c.cards) {
        // Sweep every position a marker can project to, edges and corners included.
        for (let ax = 0; ax <= c.vw; ax += 13) {
          for (let ay = 0; ay <= c.vh; ay += 13) {
            const p = placeHoverCard({ ax, ay, cw, ch, vw: c.vw, vh: c.vh, clear: c.clear })
            if (p.left < 0 || p.top < 0 || p.left + cw > c.vw || p.top + ch > c.vh)
              offenders.push(`outside at (${ax},${ay}) card ${cw}x${ch} -> ${p.left},${p.top}`)
            const covers =
              ax >= p.left && ax <= p.left + cw && ay >= p.top && ay <= p.top + ch
            if (covers) offenders.push(`covers marker at (${ax},${ay}) card ${cw}x${ch}`)
          }
        }
      }
      expect(offenders.slice(0, 5)).toEqual([])
    })
  }

  it('FIRES: the sweep really would catch a card that runs off the edge (positive control)', () => {
    // Same sweep, but with the clamp defeated by claiming a zero-size card — the shape the
    // pre-measurement first render uses, which is exactly why the layout effect must correct it.
    const bad: string[] = []
    const cw = 832
    for (let ax = 0; ax <= 1920; ax += 13) {
      const p = placeHoverCard({ ax, ay: 500, cw: 0, ch: 0, vw: 1920, vh: 1000, clear: 19 })
      if (p.left + cw > 1920) bad.push(`${ax}`)
    }
    expect(bad.length).toBeGreaterThan(0)
  })
})
