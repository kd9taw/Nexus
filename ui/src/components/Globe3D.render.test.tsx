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
//    The stub also applies three-globe's HTML-elements join, so the spot `div`s are real (§4).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef } from 'react'
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
  // three-globe's HTML-elements layer, reduced to the two rules that decide whether a spot's
  // `div` SURVIVES an update. Both are read off the shipped source, and §4 below leans on them:
  //   • data-bind-mapper `digest()` joins on DATUM IDENTITY (its id accessor is `d => d`): an
  //     unseen datum creates an element, a vanished datum removes one, the rest are left alone.
  //   • three-globe `htmlElementsLayer.update()` opens with
  //     `changedProps.hasOwnProperty('htmlElement') && state.dataMapper.clear()` — a new element
  //     factory tears the whole layer down — and react-kapsule forwards a prop to the layer at
  //     all only when it is `!==` the previous one.
  const htmlLayer = { els: new Map<object, HTMLElement>() }
  // three-globe's RINGS layer, reduced the same way (§5). Same `digest()`, so the same datum-
  // identity join — but a ring is a `THREE.Group`, not a DOM node, so what stands in for the
  // spot's `div` is the object the join hands back. `onCreateObj` returns a Group with no
  // `__nextRingTime`, which is why a rebuilt one restarts the ping from radius 0; `pings` counts
  // exactly that. NOTE the rings layer declares `ringColor`/`ringMaxRadius`/
  // `ringPropagationSpeed`/`ringRepeatPeriod`/`ringResolution` as `triggerUpdate: false`, so —
  // unlike `htmlElement` — a new accessor never reaches this join at all.
  const ringLayer = { objs: new Map<object, object>(), pings: 0 }
  const Globe = forwardRef<unknown, Record<string, unknown>>(function Globe(props, ref) {
    useImperativeHandle(ref, () => fake, [])
    renders.push(props)
    useEffect(() => {
      ;(props.onGlobeReady as (() => void) | undefined)?.()
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
    const host = useRef<HTMLDivElement>(null)
    const sentRings = useRef<unknown>(undefined)
    useLayoutEffect(() => {
      const data = (props.ringsData ?? []) as object[]
      if (sentRings.current === data) return // react-kapsule forwarded nothing: no digest
      sentRings.current = data
      for (const d of data) {
        if (ringLayer.objs.has(d)) continue
        ringLayer.pings++
        ringLayer.objs.set(d, { ping: ringLayer.pings })
      }
      const live = new Set(data)
      for (const d of [...ringLayer.objs.keys()]) if (!live.has(d)) ringLayer.objs.delete(d)
    })
    const sent = useRef<{ data: unknown; factory: unknown }>({ data: undefined, factory: undefined })
    useLayoutEffect(() => {
      const data = (props.htmlElementsData ?? []) as object[]
      const factory = props.htmlElement as ((d: object) => HTMLElement) | undefined
      const dataChanged = sent.current.data !== data
      const factoryChanged = sent.current.factory !== factory
      sent.current = { data, factory }
      if (!dataChanged && !factoryChanged) return // react-kapsule forwarded nothing: no digest
      if (factoryChanged) {
        htmlLayer.els.forEach((el) => el.remove())
        htmlLayer.els.clear()
      }
      if (!factory || !host.current) return
      for (const d of data) {
        if (htmlLayer.els.has(d)) continue
        const el = factory(d)
        htmlLayer.els.set(d, el)
        host.current.appendChild(el)
      }
      const live = new Set(data)
      for (const [d, el] of [...htmlLayer.els]) {
        if (live.has(d)) continue
        el.remove()
        htmlLayer.els.delete(d)
      }
    })
    return <div data-testid="globe" ref={host} />
  })
  return { default: Globe, __fake: fake, __htmlLayer: htmlLayer, __ringLayer: ringLayer }
})

import Globe3D from './Globe3D'
import * as ReactGlobe from 'react-globe.gl'

type FakeGlobe = {
  paused: boolean
  frames: number
  controls: () => { autoRotate: boolean; dispatchEvent: (e: { type: string }) => void }
}
const fake = (ReactGlobe as unknown as { __fake: FakeGlobe }).__fake
/** The stub's HTML-elements layer — its map outlives `cleanup()`, so it is reset per test. */
const htmlLayer = (ReactGlobe as unknown as { __htmlLayer: { els: Map<object, HTMLElement> } })
  .__htmlLayer
/** The stub's rings layer — same lifetime problem as `htmlLayer`, so also reset per test. */
const ringLayer = (
  ReactGlobe as unknown as { __ringLayer: { objs: Map<object, object>; pings: number } }
).__ringLayer

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
  htmlLayer.els.clear()
  ringLayer.objs.clear()
  ringLayer.pings = 0
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

// 4. A SPOT KEEPS ITS DOM NODE UNTIL THE DOT ITSELF CHANGES.
//    Operator report: "all the spots on the 3d connect map are flickering aggressively". A spot is
//    not drawn in WebGL at all — each one is a real `div` that three-globe's HTML-elements layer
//    parents to a CSS2DObject, and that layer pulls every one of them out of the DOM and builds
//    them again whenever EITHER of two references is new:
//      • `htmlElementsData` — the join is on DATUM IDENTITY, so a fresh snapshot (App deserialises
//        a whole new object graph per poll) means every datum is unseen and every element is
//        recreated;
//      • `htmlElement` — the layer's update opens by clearing itself when that accessor changed,
//        and react-kapsule forwards it whenever it is `!==` the last one, so an element factory
//        written inline in the JSX rebuilds the layer on EVERY render — and App re-renders the
//        globe on every 300 ms snapshot, which is the aggressive part.
//    The dots come back no earlier than the next drawn frame (CSS2DRenderer writes the DOM only
//    while rendering, and this globe renders on change), so each rebuild is a dropout.
describe('Globe3D spots keep their DOM nodes across a snapshot', () => {
  /** A FRESH object graph, as `getPropagation()` hands App one on every poll: new snapshot, new
   *  array, new spot objects — same three stations, same places. The suites above re-use ONE
   *  `prop` object, which is exactly why none of them could see this. */
  const feed = (ageSecs: number, dl1aaLat: number): PropagationSnapshot =>
    snapshot([
      { ...spot('DL1AA', dl1aaLat, 8, true), ageSecs },
      { ...spot('JA1ZZ', 35, 139, false), ageSecs },
      { ...spot('VK2QQ', -33, 151, false), ageSecs },
    ] as MapSpot[])

  const dots = (r: ReturnType<typeof render>) =>
    Array.from(r.container.querySelectorAll('.globe3d-spot')) as HTMLElement[]

  async function mount(p: PropagationSnapshot) {
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<Globe3D {...props(p, ROSTER)} />)
    })
    return r
  }
  const poll = (r: ReturnType<typeof render>, p: PropagationSnapshot) =>
    act(async () => {
      r.rerender(<Globe3D {...props(p, clone(ROSTER))} />)
    })

  it('five identical polls leave all three dots as the very same nodes', async () => {
    const r = await mount(feed(60, 50))
    const before = dots(r)
    expect(before, 'CONTROL: the three spots really did reach the layer').toHaveLength(3)
    for (let i = 0; i < 5; i++) await poll(r, feed(60, 50))
    const after = dots(r)
    expect(after, 'three spots on the air, three dots').toHaveLength(3)
    before.forEach((el, i) =>
      expect(after[i], `spot ${i} was torn out of the DOM and rebuilt`).toBe(el),
    )
  })

  // The two rebuild triggers, read one at a time: asserted together, whichever failed first would
  // hide the other, and they are separate defects with separate fixes.
  it('hands the layer the SAME data array across identical polls', async () => {
    const r = await mount(feed(60, 50))
    const first = renders[renders.length - 1]
    for (let i = 0; i < 5; i++) await poll(r, feed(60, 50))
    expect(
      renders[renders.length - 1].htmlElementsData,
      'a new array is a full re-join: every datum is unseen, so every element is recreated',
    ).toBe(first.htmlElementsData)
  })

  it('hands the layer the SAME element factory across identical polls', async () => {
    const r = await mount(feed(60, 50))
    const first = renders[renders.length - 1]
    for (let i = 0; i < 5; i++) await poll(r, feed(60, 50))
    expect(
      renders[renders.length - 1].htmlElement,
      'a new element factory makes the layer clear() itself before the join even starts',
    ).toBe(first.htmlElement)
  })

  it('an age tick rebuilds nothing, and the hover still reads the NEW age', async () => {
    const r = await mount(feed(60, 50))
    const before = dots(r)
    expect(before, 'CONTROL: the three spots really did reach the layer').toHaveLength(3)
    await poll(r, feed(120, 50))
    dots(r).forEach((el, i) =>
      expect(el, `nothing moved, so spot ${i} must still be the same node`).toBe(before[i]),
    )
    await act(async () => {
      fireEvent.mouseEnter(before[0])
    })
    expect(
      r.container.querySelector('.map-hover')?.textContent,
      'the dot is held by what it DRAWS, so its tooltip must be read live, not frozen at 1m',
    ).toContain('2m ago')
  })

  it('POSITIVE CONTROL — a dot that moves, and a spot that ages off, do rebuild', async () => {
    const r = await mount(feed(60, 50))
    const before = dots(r)
    expect(before, 'CONTROL: the three spots really did reach the layer').toHaveLength(3)
    await poll(r, feed(60, 52)) // DL1AA's reported position drifted 2° north
    expect(r.container.contains(before[0]), 'the dot moved: its old node must be gone').toBe(false)
    expect(dots(r), 'and the layer still draws three spots').toHaveLength(3)
    await poll(r, snapshot([{ ...spot('JA1ZZ', 35, 139, false), ageSecs: 60 }] as MapSpot[]))
    expect(dots(r), 'two spots aged off the map').toHaveLength(1)
  })
})

// 5. THE QTH PING RING KEEPS ITS OBJECT UNTIL THE QTH ITSELF MOVES.
//    §4's sibling, one layer over. `ringsData` was built inline as `qth ? [{ lat, lng }] : []`, so
//    every render handed three-globe a new array carrying a NEW DATUM — and App re-renders the
//    globe on every 300 ms snapshot. The rings layer joins on DATUM IDENTITY exactly as the HTML
//    layer does (data-bind-mapper's id accessor is `d => d`, and the rings layer never overrides
//    it), and its `onCreateObj` hands back a `THREE.Group` with no `__nextRingTime` — so the next
//    frame spawns a fresh ring at radius 0. The ping restarted ~3×/s and never reached full
//    radius. A ring is not a DOM node, so what stands in for §4's `div` is the object the join
//    hands back, and `pings` counts how many times a new one was made.
//
//    ⚠️ ONLY THE DATA TRIGGER — checked against the shipped layer, not assumed from §4.
//    `ringColor`, `ringResolution`, `ringMaxRadius`, `ringPropagationSpeed` and
//    `ringRepeatPeriod` are every one of them declared `triggerUpdate: false`, so the inline
//    `ringColor={() => …}` in the JSX, which IS a new function on every render, reaches the layer
//    and changes nothing. §4's second half has no counterpart here: pinning that reference would
//    assert something the layer ignores.
describe('Globe3D keeps the QTH ping ring across a snapshot', () => {
  const ring = () => [...ringLayer.objs.values()][0]

  async function mount(grid = 'EN52') {
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<Globe3D {...props(snapshot([]), ROSTER)} myGrid={grid} />)
    })
    return r
  }
  const poll = (r: ReturnType<typeof render>, grid = 'EN52') =>
    act(async () => {
      r.rerender(<Globe3D {...props(snapshot([]), clone(ROSTER))} myGrid={grid} />)
    })

  it('five identical polls leave the ping ring as the very same object', async () => {
    const r = await mount()
    const before = ring()
    expect(before, 'CONTROL: the QTH ring really did reach the layer').toBeTruthy()
    for (let i = 0; i < 5; i++) await poll(r)
    expect(ring(), 'the ring was torn out and rebuilt, restarting its ping').toBe(before)
    expect(ringLayer.pings, 'and the ping animation started exactly once').toBe(1)
  })

  it('hands the layer the SAME data array across identical polls', async () => {
    const r = await mount()
    const first = renders[renders.length - 1]
    for (let i = 0; i < 5; i++) await poll(r)
    expect(
      renders[renders.length - 1].ringsData,
      'a new array is a full re-join: the one datum is unseen, so the ring is recreated',
    ).toBe(first.ringsData)
  })

  it('POSITIVE CONTROL — the QTH moving DOES rebuild the ring', async () => {
    const r = await mount()
    const before = ring()
    await poll(r, 'FN31') // the operator corrects their grid
    expect(ring(), 'the ring must follow the QTH').not.toBe(before)
    expect(ringLayer.pings, 'and the new ring pings from its new place').toBe(2)
  })
})

// 6. THE LAYER ACCESSORS ARE THE SAME FUNCTIONS FROM ONE RENDER TO THE NEXT.
//    §4's second trigger, swept across the layers §4 did not look at. react-kapsule forwards any
//    prop that is `!==` the last one, and kapsule's setter ends `if (redigest) digest()` — where
//    `redigest` is the prop's `triggerUpdate`, defaulting to TRUE. three-globe's paths and
//    polygons layers declare their accessors without it, so an accessor written inline in the JSX
//    re-ran the layer's `update()` on every render; `PathsLayerKapsule.update` re-digests every
//    path it holds, recomputing `calcPath` and two vertex arrays across the state-border mesh's
//    302 line-strings and 11,664 coordinates — three times a second, States being on by default,
//    with nothing on the map changed.
//
//    This asserts the references, not the work: jsdom has no WebGL and three-globe is stubbed, so
//    what is provable here is exactly what react-kapsule reads — prop identity. `htmlElement` has
//    its own test in §4 because a new one CLEARS that layer rather than merely re-digesting it.
describe('Globe3D hands globe.gl the same layer accessors across a snapshot', () => {
  // Every accessor prop whose layer declares it `triggerUpdate: true` (the default). `ringColor`
  // and its four siblings are deliberately absent: the rings layer declares them
  // `triggerUpdate: false`, so pinning them would assert a reference the layer never reads back.
  const ACCESSORS = [
    'pathPointLat',
    'pathPointLng',
    'pathColor',
    'polygonGeoJsonGeometry',
    'polygonCapColor',
    'polygonSideColor',
    'polygonStrokeColor',
    'polygonAltitude',
  ] as const

  it('five identical polls change none of them', async () => {
    const prop = snapshot([spot('DL1AA', 50, 8, true)])
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<Globe3D {...props(prop, ROSTER)} />)
    })
    const first = renders[renders.length - 1]
    for (const k of ACCESSORS) {
      expect(typeof first[k], `CONTROL: ${k} really is being passed to globe.gl`).toBe('function')
    }
    for (let i = 0; i < 5; i++) {
      await act(async () => {
        r.rerender(<Globe3D {...props(prop, clone(ROSTER))} />)
      })
    }
    const last = renders[renders.length - 1]
    for (const k of ACCESSORS) {
      expect(last[k], `${k} is a new function, so its layer re-digests every object it holds`).toBe(
        first[k],
      )
    }
  })
})
