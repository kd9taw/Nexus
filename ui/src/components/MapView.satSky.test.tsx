// @vitest-environment jsdom
//
// WHICH SATELLITES THE MAP'S SATELLITE LAYER DRAWS (operator report, 2026-10-04: "it all of a sudden
// shows every satellite ever launched - but only on the popped out screen").
//
// The sky follows the operator's ★ birds (2026-08-01). With none starred it drew EVERY bird holding
// current elements: a rule written that morning for Celestrak's 97-bird amateur group, which the
// catalog stopped being three hours later. The mirror's union holds 347 birds with elements today
// (measured 2026-10-04 21:45Z), 280 of them beacon-only telemetry cubesats that nobody can work. With
// no stars the sky now shows the birds that can be worked; an unclassified bird still shows (absent
// `classes` is never a guess), and All still shows every bird in the file.
//
// And the pop-out's sky read the ★/All choice out of storage on every draw, so with no choice of its
// own it followed the main window's chip live: the main window flipped to All and the pop-out filled
// with every bird within a second, its own chip still reading ★.
//
// jsdom has no 2-D canvas, so the canvas gets a recording context; a bird the map draws has its name
// measured for its label, and that is what these count. MapView is the REAL component.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, screen } from '@testing-library/react'
import type { SatView } from '../types'

const BIRDS = [
  { name: 'ISS (ZARYA)', norad: 25544, classes: ['fm', 'digital', 'beacon'] },
  { name: 'RS-44', norad: 44909, classes: ['linear', 'beacon'] },
  { name: 'CUBESAT-A', norad: 90001, classes: ['beacon'] },
  { name: 'CUBESAT-B', norad: 90002, classes: [] },
  // Never classified (an old payload, the Celestrak fallback): nothing is known, so nothing is hidden.
  { name: 'OLD-PAYLOAD', norad: 90003 },
]
const NOW = Math.floor(Date.now() / 1000)
const VIEW = {
  tleAgeDays: 1,
  usableCount: BIRDS.length,
  agingCount: 0,
  heldBackCount: 0,
  tleFetchedAt: NOW,
  tleSource: 'mirror',
  birds: BIRDS.map((b, i) => ({
    ...b,
    lat: 10 + i * 5,
    lon: -100 + i * 20,
    altKm: 500,
    footprintKm: 2500,
    track: [
      [NOW - 60, 10 + i * 5, -101 + i * 20],
      [NOW + 3600, 10 + i * 5, -99 + i * 20],
    ],
    status: 'alive',
    amateur: true,
  })),
  passes: [],
  excluded: [],
} as unknown as SatView

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => VIEW),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
}))
import { MapView, DEFAULT_LAYERS } from './MapView'

// THE BUDGET (2026-10-09). The slowest case here, "not a flip in the main window", takes 1.21 s and 1.20 s on one
// core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one core, past vitest's
// 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

/** Every string the map measured: a drawn bird's label is measured once per draw. */
const measured: string[] = []
function recordingCtx(): CanvasRenderingContext2D {
  const self: object = new Proxy(function ctx() {}, {
    get(_t, prop) {
      if (prop === 'measureText')
        return (s: string) => {
          measured.push(s)
          return { width: 10 }
        }
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
class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

let cw: PropertyDescriptor | undefined
let ch: PropertyDescriptor | undefined
beforeEach(() => {
  localStorage.clear()
  measured.length = 0
  window.history.replaceState(null, '', '/')
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(recordingCtx())
  // A laid-out map, so the draw effect runs (it bails at 0×0).
  cw = Object.getOwnPropertyDescriptor(Element.prototype, 'clientWidth')
  ch = Object.getOwnPropertyDescriptor(Element.prototype, 'clientHeight')
  Object.defineProperty(Element.prototype, 'clientWidth', { configurable: true, get: () => 1000 })
  Object.defineProperty(Element.prototype, 'clientHeight', { configurable: true, get: () => 800 })
})
afterEach(async () => {
  // Drain the mount effects' fetches before unmounting (see MapView.readability.test.tsx).
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0))
  })
  cleanup()
  vi.restoreAllMocks()
  if (cw) Object.defineProperty(Element.prototype, 'clientWidth', cw)
  if (ch) Object.defineProperty(Element.prototype, 'clientHeight', ch)
})

const NAMES = new Set(BIRDS.map((b) => b.name))
/** The birds drawn since the last call (and start counting afresh). */
function drawn(): string[] {
  const out = [...new Set(measured.filter((m) => NAMES.has(m)))].sort()
  measured.length = 0
  return out
}
/** Wait out the satellite tick: the layer repaints every second while birds are up. */
const nextTick = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 1100))
  })

/** The map with its Satellites layer ticked on this surface, on the flat map so every bird is in view. */
async function mountWithSats(surface: 'main' | 'connect' = 'main') {
  const key = surface === 'main' ? 'nexus.connect.intents' : 'nexus.connect.intents.connect'
  localStorage.setItem(key, JSON.stringify({ dx: { layers: { ...DEFAULT_LAYERS, sats: { visible: true, opacity: 0.9 } } } }))
  window.history.replaceState(null, '', surface === 'main' ? '/' : '/?panel=connect')
  await act(async () => {
    render(
      <MapView
        myGrid="EN52"
        theme="dark"
        stations={[]}
        prop={null}
        selectedCall={null}
        onSelectCall={() => {}}
        needByCall={new Map()}
        intent="dx"
        projection="world"
      />,
    )
  })
  await act(async () => {
    await new Promise((r) => setTimeout(r, 50))
  })
}
const chip = () => screen.getByRole('button', { name: 'Filter satellites to ★ birds' })

describe('the satellite layer draws the birds an operator can work', () => {
  it('with no ★ birds, it shows the satellites that can be worked, not the beacon-only ones', async () => {
    await mountWithSats()
    expect(chip().textContent, 'CONTROL: the ★ filter is on (the default)').toBe('★')
    expect(drawn()).toEqual(['ISS (ZARYA)', 'OLD-PAYLOAD', 'RS-44'])
  })

  it('says so on the chip', async () => {
    await mountWithSats()
    expect(chip().title).toMatch(/^No ★ birds yet/)
  })

  it('All shows every bird in the file', async () => {
    localStorage.setItem('nexus.sats.favOnly', '0')
    await mountWithSats()
    expect(chip().textContent, 'CONTROL').toBe('All')
    expect(drawn()).toEqual(['CUBESAT-A', 'CUBESAT-B', 'ISS (ZARYA)', 'OLD-PAYLOAD', 'RS-44'])
  })

  it('with ★ birds, it shows exactly those, a beacon-only one included', async () => {
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['RS-44', 'CUBESAT-A']))
    await mountWithSats()
    expect(drawn()).toEqual(['CUBESAT-A', 'RS-44'])
    expect(chip().title, 'CONTROL: the starred wording').toMatch(/^Showing your ★ birds/)
  })
})

describe('the pop-out’s sky follows its own ★/All', () => {
  it('not a flip in the main window', async () => {
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['ISS (ZARYA)']))
    await mountWithSats('connect')
    expect(drawn(), 'CONTROL: ★ shows the one starred bird').toEqual(['ISS (ZARYA)'])
    // The main window's chip goes to All: its own key, the bare one. Nothing reaches this window.
    localStorage.setItem('nexus.sats.favOnly', '0')
    await nextTick()
    expect(chip().textContent, 'CONTROL: this window’s chip still reads ★').toBe('★')
    expect(drawn(), 'the pop-out drew every bird after a flip in the main window').toEqual(['ISS (ZARYA)'])
  })
})
