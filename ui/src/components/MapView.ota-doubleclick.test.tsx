// @vitest-environment jsdom
//
// Task 2 (gridtracker-pota-map-plan-2026-09-05): double-clicking a POTA park marker on
// the Connect map = the WSJT-X "work it" gesture (QSY + set mode + tag the hunt target),
// exactly like a live spot or a DXpedition marker. See onDoubleClick's `dxped`/`spot`
// branches for the existing precedent this follows.
//
// THE POINT OF THIS FILE is the safety property, not the wiring: the gesture must QSY +
// tag ONLY, and must NEVER key the transmitter or arm an FT answer/auto-sequence. A test
// that only checks `onWorkSpot` was called would still pass if the branch also called a
// transmit API — so this spies the api module's whole transmit-capable surface
// (`callStation`, `setPtt`, `setTxEnabled`, `startCq`, `callCq`, `haltTx`) and asserts
// none of it was touched.
//
// WHY THE FULL COMPONENT, NOT A STUB: `hit.kind === 'ota'` comes out of MapView's own
// `hitTest`, which reads `placedOta` — a real projection of the spot's lat/lon computed
// from `myGrid`. Faking that mapping would just be re-asserting the test's own arithmetic.
// Driving the real component needs a non-zero canvas `size` (both `placedOta` and the main
// draw effect gate on it), which means the real draw effect also runs — jsdom has no 2D
// canvas, so `getContext('2d')` is stubbed with a self-returning Proxy that answers any
// property/method access without throwing (MapView calls a wide swath of the context API
// purely to paint; none of it feeds back into hit-testing).
//
// THE PROJECTION-CENTER TRICK: rather than reimplement `makeProjection`'s math to find
// which screen pixel a given lat/lon lands on, the test places its one park spot AT the
// operator's own QTH. With the default (unrotated, unzoomed) globe projection, the
// operator's own position always rotates to screen dead-center — see `makeProjection`'s
// `globe` branch in `mapGeo.ts` (`rotate: [-c.lon, -c.lat]`, `translate: [w/2, h/2]`) — so
// the hit point is just the canvas center, independent of MapView's internals.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { MapView } from './MapView'
import { gridToLatLon } from '../grid'
import { bandLabelForMhz } from '../band'
import type { OtaMapSpot } from '../types'

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLog: vi.fn(async () => []),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  // The transmit-capable surface — spied so the test can PROVE the gesture never reaches
  // it, rather than merely omitting it from the mock (which would prove nothing).
  callStation: vi.fn(async () => ({})),
  setPtt: vi.fn(async () => ({})),
  setTxEnabled: vi.fn(async () => ({})),
  startCq: vi.fn(async () => ({})),
  callCq: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
}))

import { callStation, setPtt, setTxEnabled, startCq, callCq, haltTx } from '../api'

// jsdom has no ResizeObserver; MapView installs one on its wrap ref on mount.
class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

/** jsdom has no 2D canvas context — `getContext('2d')` returns null there, and MapView's
 *  draw effects call a wide surface of `CanvasRenderingContext2D` (gradients, paths,
 *  transforms...) purely to paint. A self-returning Proxy answers any property read or
 *  method call without throwing, including chained calls like
 *  `ctx.createLinearGradient(...).addColorStop(...)`, so the real effects can run without
 *  hand-modelling the whole context API. */
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

/** One POTA activator placed exactly at the operator's own QTH — see the file header for
 *  why that makes the hit point the canvas center. */
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
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(fakeCtx())
  // jsdom lays nothing out — clientWidth/clientHeight are 0 unless stubbed, and MapView
  // reads them once at mount to seed the canvas `size` that hit-testing depends on.
  clientWidthDesc = Object.getOwnPropertyDescriptor(Element.prototype, 'clientWidth')
  clientHeightDesc = Object.getOwnPropertyDescriptor(Element.prototype, 'clientHeight')
  Object.defineProperty(Element.prototype, 'clientWidth', { configurable: true, get: () => W })
  Object.defineProperty(Element.prototype, 'clientHeight', { configurable: true, get: () => H })
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  if (clientWidthDesc) Object.defineProperty(Element.prototype, 'clientWidth', clientWidthDesc)
  if (clientHeightDesc) Object.defineProperty(Element.prototype, 'clientHeight', clientHeightDesc)
})

function mount(onWorkSpot: (t: unknown) => void) {
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
      onWorkSpot={onWorkSpot}
    />,
  )
}

describe('MapView double-click a POTA park marker', () => {
  it('tunes and tags the park, and never touches the transmit/key API', () => {
    const onWorkSpot = vi.fn()
    const { container } = mount(onWorkSpot)
    const canvas = container.querySelector('canvas') as HTMLCanvasElement

    // jsdom's getBoundingClientRect is all-zero; canvasXY falls back to a 1:1 scale when
    // rect.width is 0, so clientX/clientY land straight in canvas space — see the file
    // header for why the park's projected position is exactly (W/2, H/2).
    fireEvent.doubleClick(canvas, { clientX: W / 2, clientY: H / 2 })

    expect(onWorkSpot).toHaveBeenCalledTimes(1)
    expect(onWorkSpot).toHaveBeenCalledWith({
      call: PARK.activator,
      band: bandLabelForMhz(PARK.freqMhz),
      mode: PARK.mode,
      freqMhz: PARK.freqMhz,
      program: PARK.program,
      reference: PARK.reference,
    })

    // THE SAFETY PROPERTY. Asserting these specific calls stayed at zero — rather than
    // just not mentioning them — is what would catch a regression that added a transmit
    // call to this branch.
    expect(callStation).not.toHaveBeenCalled()
    expect(setPtt).not.toHaveBeenCalled()
    expect(setTxEnabled).not.toHaveBeenCalled()
    expect(startCq).not.toHaveBeenCalled()
    expect(callCq).not.toHaveBeenCalled()
    expect(haltTx).not.toHaveBeenCalled()
  })
})
