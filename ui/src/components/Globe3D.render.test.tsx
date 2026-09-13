// @vitest-environment jsdom
//
// WHAT THE 3-D GLOBE HANDS globe.gl, AND WHEN IT LETS IT DRAW.
//
// jsdom has no WebGL, so `react-globe.gl` (the renderer BENEATH Globe3D) is replaced by a stub
// that records every prop it is given and exposes the handful of globe methods Globe3D calls.
// Globe3D itself is the REAL component — the seams under test are its own memos and effects.
//
// 1. ARCS ARE REBUILT ONLY WHEN WHAT THEY DRAW CHANGES (29324da8 regression).
//    react-kapsule forwards a prop to globe.gl only when it is `!==` the previous one, and
//    three-globe then disposes and recreates every arc object — material included, which is a
//    shader-program compile per arc. App hands Connect a NEW `stations` array on every 300 ms
//    snapshot, and 29324da8 put `stations` in the arcs memo, so the whole arc set was torn down
//    and recompiled ~3×/s with nothing on screen changing (65–72% main thread in headless Chrome).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { forwardRef, useEffect, useImperativeHandle } from 'react'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import type { MapSpot, PropagationSnapshot, Station } from '../types'

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => []),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getSatTrackStatus: vi.fn(async () => null),
  getLog: vi.fn(async () => []),
}))
vi.mock('three/examples/jsm/postprocessing/UnrealBloomPass.js', () => ({
  UnrealBloomPass: class {
    setSize() {}
    dispose() {}
  },
}))

/** Every prop set the stub was rendered with, oldest first. */
const renders: Array<Record<string, unknown>> = []

vi.mock('react-globe.gl', async () => {
  const THREE = await import('three')
  const fake = {
    lights: () => [],
    getCoords: () => ({ x: 0, y: 0, z: 0 }),
    scene: (() => {
      const s = new THREE.Scene()
      return () => s
    })(),
    postProcessingComposer: () => ({ addPass() {}, passes: [] }),
    controls: () => controls,
    pointOfView: () => {},
    // globe.gl's render loop, reduced to the one fact the tests read: is it running?
    paused: false,
    /** Frames drawn: globe.gl's resume draws one synchronously, so each resume is a frame. */
    frames: 0,
    pauseAnimation: () => {
      fake.paused = true
    },
    resumeAnimation: () => {
      fake.paused = false
      fake.frames++
    },
  }
  // ONE controls object, and a real event dispatcher: OrbitControls fires start/change/end.
  const controls = Object.assign(new THREE.EventDispatcher<Record<string, object>>(), {
    autoRotate: false,
    autoRotateSpeed: 0,
  })
  const Globe = forwardRef<unknown, Record<string, unknown>>(function Globe(props, ref) {
    useImperativeHandle(ref, () => fake, [])
    renders.push(props)
    useEffect(() => {
      ;(props.onGlobeReady as (() => void) | undefined)?.()
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
    return <div data-testid="globe" />
  })
  return { default: Globe, __fake: fake }
})

import Globe3D from './Globe3D'
import * as ReactGlobe from 'react-globe.gl'

type FakeGlobe = {
  paused: boolean
  frames: number
  controls: () => { autoRotate: boolean; dispatchEvent: (e: { type: string }) => void }
}
const fake = (ReactGlobe as unknown as { __fake: FakeGlobe }).__fake

/** The last ResizeObserver callback Globe3D installed — fired by hand to simulate a resize. */
let roCallback: (() => void) | null = null
class RO {
  constructor(cb: () => void) {
    roCallback = cb
  }
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  renders.length = 0
  fake.paused = false
  fake.frames = 0
  roCallback = null
  localStorage.clear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  // `webglOk()` asks for a context; jsdom has none. And the globe only mounts once its box has
  // a real size, which jsdom never lays out — give every element a 600×400 box.
  // Any context method is a no-op that returns a gradient-shaped object (the sprite canvases).
  const ctx = new Proxy({} as Record<string | symbol, unknown>, {
    get: (t, k) => (k in t ? t[k] : () => ({ addColorStop() {} })),
  })
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(
    () => ctx as unknown as RenderingContext,
  )
  // jsdom reports every document as hidden, and both maps skip their 1 s ticks for a hidden tab.
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(600)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(400)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const spot = (call: string, lat: number, lon: number, heardMe: boolean): MapSpot =>
  ({ call, lat, lon, band: '20m', heardMe, ageSecs: 60, approx: false, freqMhz: 14.074, mode: 'FT8', entity: 'X' }) as unknown as MapSpot

const station = (call: string, grid: string): Station =>
  ({ call, grid, snr: -10, lastHeardSlot: 1, heardCount: 2, presence: 'active', worked: false }) as unknown as Station

function snapshot(spots: MapSpot[]): PropagationSnapshot {
  return { source: 'live', asOf: 1, spots, openings: [], dxpeditions: { workableNow: [] } } as unknown as PropagationSnapshot
}

const ROSTER = [station('K1AB', 'FN42'), station('W6XY', 'CM87')]
const clone = (s: Station[]) => s.map((x) => ({ ...x }))

function props(prop: PropagationSnapshot, stations: Station[]) {
  return { myGrid: 'EN52', prop, selectedCall: null, onSelectCall: () => {}, stations }
}

const lastArcs = () => renders[renders.length - 1].arcsData as unknown[]

describe('Globe3D arcs follow their content, not the snapshot identity', () => {
  it('a new but identical stations array (the 300 ms snapshot) hands globe.gl the SAME arcs', async () => {
    const prop = snapshot([spot('DL1AA', 50, 8, true), spot('JA1ZZ', 35, 139, true)])
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<Globe3D {...props(prop, ROSTER)} />)
    })
    const before = lastArcs()
    expect(before.length, 'CONTROL: the two heard-me spots really are arcs').toBe(2)

    for (let i = 0; i < 3; i++) {
      await act(async () => {
        r.rerender(<Globe3D {...props(prop, clone(ROSTER))} />)
      })
    }
    expect(lastArcs(), 'identical content must not re-digest the arc layer').toBe(before)
  })

  it('POSITIVE CONTROL — a new heard-me report DOES rebuild the arcs', async () => {
    const one = snapshot([spot('DL1AA', 50, 8, true)])
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<Globe3D {...props(one, ROSTER)} />)
    })
    const before = lastArcs()
    await act(async () => {
      r.rerender(<Globe3D {...props(snapshot([spot('DL1AA', 50, 8, true), spot('VK2QQ', -33, 151, true)]), clone(ROSTER))} />)
    })
    expect(lastArcs()).not.toBe(before)
    expect(lastArcs()).toHaveLength(2)
  })

  it('POSITIVE CONTROL — with RX arcs on, a decode that MOVES rebuilds them', async () => {
    localStorage.setItem('nexus.connect.globe3d.layers', JSON.stringify({ rxarcs: true }))
    const prop = snapshot([])
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<Globe3D {...props(prop, ROSTER)} />)
    })
    const before = lastArcs()
    expect(before).toHaveLength(2)
    await act(async () => {
      r.rerender(<Globe3D {...props(prop, [station('K1AB', 'FN42'), station('W6XY', 'DM12')])} />)
    })
    expect(lastArcs()).not.toBe(before)
  })
})

// 2. THE GLOBE DRAWS ON CHANGE, NOT ON EVERY FRAME.
//    globe.gl runs requestAnimationFrame forever and renders the bloom composer each time: ~76 fps
//    and 238 GL draws/s in headless Chrome with every layer OFF and nothing moving. Globe3D now
//    pauses that loop once the scene has been still for a moment, and resumes it for anything that
//    can change a pixel: a drag/zoom (the controls), spin, a data change, a resize. What globe.gl
//    does while resumed is measured in the browser harness, not here — this pins Globe3D's half.
describe('Globe3D renders on change only', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  const prop = snapshot([spot('DL1AA', 50, 8, true)]) // an animated-dash arc + the QTH ring are on

  async function mount() {
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<Globe3D {...props(prop, ROSTER)} />)
    })
    return r
  }
  const settle = () => act(async () => void vi.advanceTimersByTime(5_000))

  it('a still globe stops rendering — animated dashes and the QTH ring do not keep it awake', async () => {
    await mount()
    expect(fake.paused, 'CONTROL: it renders while the scene first comes up').toBe(false)
    await settle()
    expect(fake.paused).toBe(true)
  })

  it('identical 300 ms snapshots leave it asleep', async () => {
    const r = await mount()
    await settle()
    const frames = fake.frames
    for (let i = 0; i < 5; i++) {
      await act(async () => {
        r.rerender(<Globe3D {...props(prop, clone(ROSTER))} />)
        vi.advanceTimersByTime(300)
      })
      expect(fake.paused, `snapshot ${i} woke the render loop`).toBe(true)
    }
    expect(fake.frames, 'nothing changed, so nothing may be drawn').toBe(frames)
  })

  it('a drag wakes it for as long as the pointer is down, then it sleeps again', async () => {
    await mount()
    await settle()
    const c = fake.controls()
    await act(async () => c.dispatchEvent({ type: 'start' }))
    expect(fake.paused).toBe(false)
    await act(async () => {
      c.dispatchEvent({ type: 'change' })
      vi.advanceTimersByTime(10_000) // a long, slow drag
    })
    expect(fake.paused, 'must not sleep mid-drag').toBe(false)
    await act(async () => c.dispatchEvent({ type: 'end' }))
    expect(fake.paused, 'damping still settling right after release').toBe(false)
    await settle()
    expect(fake.paused).toBe(true)
  })

  it('a wheel zoom (a lone change event) wakes it', async () => {
    await mount()
    await settle()
    await act(async () => fake.controls().dispatchEvent({ type: 'change' }))
    expect(fake.paused).toBe(false)
    await settle()
    expect(fake.paused).toBe(true)
  })

  it('spin keeps it rendering until spin is turned off', async () => {
    const r = await mount()
    await settle()
    const spinBtn = r.container.querySelector('.globe3d-spin') as HTMLButtonElement
    await act(async () => spinBtn.click())
    expect(fake.controls().autoRotate).toBe(true)
    await settle()
    expect(fake.paused, 'spinning globe must keep rendering').toBe(false)
    await act(async () => spinBtn.click())
    await settle()
    expect(fake.paused).toBe(true)
  })

  it('the 1 s breath of an open band draws ONE frame per tick and goes straight back to sleep', async () => {
    // Heat + opening wedges are on by default and there is an opening: the pulse ticks every
    // second. Each tick changes two material opacities, which one frame shows — holding the loop
    // awake for a whole wake window per tick would be rendering continuously by another name.
    const open = {
      ...snapshot([spot('DL1AA', 50, 8, false)]),
      openings: [{ band: '20m', mode: 'F2', octant: 'E', bearingDeg: 60, maxKm: 7000, probability: 0.7 }],
    } as unknown as PropagationSnapshot
    await act(async () => {
      render(<Globe3D {...props(open, ROSTER)} />)
    })
    await settle()
    expect(fake.paused).toBe(true)
    const before = fake.frames
    // One act per tick: a single 3 s act would batch three ticks into one commit.
    for (let i = 0; i < 3; i++) await act(async () => void vi.advanceTimersByTime(1_000))
    expect(fake.frames - before, 'CONTROL: the breath really did draw').toBeGreaterThanOrEqual(3)
    expect(fake.paused, 'and never stayed awake between ticks').toBe(true)
  })

  it('a real data change wakes it', async () => {
    const r = await mount()
    await settle()
    await act(async () => {
      r.rerender(<Globe3D {...props(snapshot([spot('DL1AA', 50, 8, true), spot('VK2QQ', -33, 151, true)]), clone(ROSTER))} />)
    })
    expect(fake.paused).toBe(false)
  })

  it('a resize wakes it', async () => {
    await mount()
    await settle()
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(900)
    await act(async () => roCallback?.())
    expect(fake.paused).toBe(false)
  })
})

// 3. THE 3-D LAYERS PANEL FOLDS LIKE THE 2-D ONE, AND SHARES ITS RECORD.
//    Same component, same place, same per-window record (MapView.layersCollapse.test.tsx drives the
//    2-D half): folding the panel on one surface leaves it folded across the 2D/3D toggle.
describe('the 3-D Layers panel', () => {
  const panel = () => screen.queryByRole('complementary', { name: 'Layers' })

  it('collapses to a pill and back, with the state exposed to assistive tech', async () => {
    await act(async () => {
      render(<Globe3D {...props(snapshot([]), ROSTER)} />)
    })
    expect(panel(), 'CONTROL: open by default').not.toBeNull()
    const btn = screen.getByRole('button', { name: 'Collapse Layers' })
    expect(btn.getAttribute('aria-expanded')).toBe('true')
    fireEvent.click(btn)
    expect(panel()).toBeNull()
    const pill = screen.getByRole('button', { name: 'Layers' })
    expect(pill.getAttribute('aria-expanded')).toBe('false')
    fireEvent.click(pill)
    expect(panel()).not.toBeNull()
  })

  it('opens folded when the panel was left folded on the 2-D map — one place, one record', async () => {
    localStorage.setItem('nexus.connect.layersPanel.collapsed', '1')
    await act(async () => {
      render(<Globe3D {...props(snapshot([]), ROSTER)} />)
    })
    expect(panel()).toBeNull()
    expect(screen.getByRole('button', { name: 'Layers' })).toBeTruthy()
  })
})
