// @vitest-environment jsdom
//
// WAKING THE SLEEPING 3-D GLOBE NEVER RE-ENTERS ITS OWN FIRST FRAME (#407).
//
// The globe's render loop sleeps while nothing moves, and Globe3D wakes it (`resumeAnimation`) for
// anything that moves the picture — 'change' from the controls among them. Two facts of the shipped
// libraries make a careless wake recurse until the stack runs out, which on the Conditions page read
// "RangeError: Maximum call stack size exceeded":
//   • globe.gl `resumeAnimation()` calls `_animationCycle()` straight away, and `_animationCycle` draws
//     its frame (three-render-objects `tick()`: `controls.update(dt)`, then the render) BEFORE it
//     records the next requestAnimationFrame — so while that first frame draws, the loop still reads
//     as stopped and another resume starts another frame.
//   • three's OrbitControls `update()` dispatches 'change' BEFORE it records the pose it compared
//     against, so an update nested inside that 'change' sees the same move and fires 'change' again.
// Real Chrome, the served TV page and the desktop's own Conditions view alike: ▶ Spin pressed while
// the globe slept went 8117 frames deep; so did the view jumping to the QTH while it slept.
//
// So the stub below is the REAL OrbitControls under globe.gl's resume order, and nothing else of the
// library. It counts how deep frames nest and stops a runaway at RUNAWAY, so a red reads as a number.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { forwardRef, useEffect, useImperativeHandle } from 'react'
import { render, cleanup, act } from '@testing-library/react'
import type { OrbitControls as OrbitControlsT } from 'three/examples/jsm/controls/OrbitControls.js'
import type { PerspectiveCamera, Vector3 } from 'three'
import type { MapSpot, PropagationSnapshot } from '../types'

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => []),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getSatTrackStatus: vi.fn(async () => null),
}))
vi.mock('three/examples/jsm/postprocessing/UnrealBloomPass.js', () => ({
  UnrealBloomPass: class {
    setSize() {}
    dispose() {}
  },
}))

vi.mock('react-globe.gl', async () => {
  const THREE = await import('three')
  const { OrbitControls } = await import('three/examples/jsm/controls/OrbitControls.js')
  const RADIUS = 100 // three-globe's globe radius
  // three-globe's polar2Cartesian: where a point at (lat, lng, altitude) sits.
  const at = (lat: number, lng: number, alt: number) => {
    const phi = ((90 - lat) * Math.PI) / 180
    const theta = ((90 - lng) * Math.PI) / 180
    const r = RADIUS * (1 + alt)
    return new THREE.Vector3(r * Math.sin(phi) * Math.cos(theta), r * Math.cos(phi), r * Math.sin(phi) * Math.sin(theta))
  }
  /** How deep frames nest, and the stop for a runaway (the browser runs on to the stack limit). */
  const loop = { next: null as ReturnType<typeof setTimeout> | null, depth: 0, maxDepth: 0, frames: 0, last: 0, runaway: 25 }
  let camera = new THREE.PerspectiveCamera()
  let controls = new OrbitControls(camera)
  // globe.gl's `_animationCycle`: the frame first, the next frame booked after it (a timer stands in
  // for requestAnimationFrame).
  function cycle() {
    loop.depth++
    loop.maxDepth = Math.max(loop.maxDepth, loop.depth)
    try {
      if (loop.depth > loop.runaway) return
      const now = Date.now()
      controls.update(Math.min(1, (now - loop.last) / 1000)) // three-render-objects `tick()`
      loop.last = now
      loop.frames++
      loop.next = setTimeout(cycle, 16)
    } finally {
      loop.depth--
    }
  }
  const renderer = {
    domElement: document.createElement('canvas'),
    getContext: () => ({ isContextLost: () => false }),
    dispose() {},
    forceContextLoss() {},
  }
  const fake = {
    loop,
    at,
    camera: () => camera,
    controls: () => controls,
    lights: () => [],
    getCoords: (lat: number, lng: number, alt = 0) => at(lat, lng, alt),
    scene: (() => {
      const s = new THREE.Scene()
      return () => s
    })(),
    postProcessingComposer: () => ({ addPass() {}, passes: [] }),
    renderer: () => renderer,
    // globe.gl's pointOfView. The setter is three-render-objects' cameraPosition: it moves the camera
    // itself and fires nothing — the next frame's update() is what sees the move. (A timed fly-to
    // ends in the same place; the stub goes there at once.)
    pointOfView: (pov?: { lat: number; lng: number; altitude: number }) => {
      if (!pov) return { lat: 0, lng: 0, altitude: camera.position.length() / RADIUS - 1 }
      camera.position.copy(at(pov.lat, pov.lng, pov.altitude))
      controls.target.set(0, 0, 0)
      return fake
    },
    // globe.gl: `if (animationFrameRequestId === null) this._animationCycle()`.
    resumeAnimation: () => {
      if (loop.next === null) cycle()
    },
    pauseAnimation: () => {
      if (loop.next !== null) clearTimeout(loop.next)
      loop.next = null
    },
  }
  const Globe = forwardRef<unknown, Record<string, unknown>>(function Globe(props, ref) {
    useImperativeHandle(ref, () => fake, [])
    useEffect(() => {
      // globe.gl's own setup: fresh controls as it configures them, the camera at its opening
      // height, and the loop started.
      camera = new THREE.PerspectiveCamera()
      camera.position.copy(at(0, 0, 2.5))
      controls = new OrbitControls(camera)
      controls.enableDamping = true
      controls.dampingFactor = 0.1
      controls.enablePan = false
      loop.last = Date.now()
      cycle()
      ;(props.onGlobeReady as (() => void) | undefined)?.()
      return () => fake.pauseAnimation()
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
    return <div data-testid="globe" />
  })
  return { default: Globe, __fake: fake }
})

import Globe3D from './Globe3D'
import * as ReactGlobe from 'react-globe.gl'

vi.setConfig({ testTimeout: 15_000 })

type Fake = {
  loop: { next: unknown; maxDepth: number; frames: number; runaway: number }
  at: (lat: number, lng: number, alt: number) => Vector3
  camera: () => PerspectiveCamera
  controls: () => OrbitControlsT
}
const fake = (ReactGlobe as unknown as { __fake: Fake }).__fake

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
beforeEach(() => {
  vi.useFakeTimers()
  fake.loop.next = null
  localStorage.clear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  // jsdom has no canvas contexts, and the globe mounts only once its box has a size jsdom never lays out.
  const ctx = new Proxy({ getExtension: () => null } as Record<string | symbol, unknown>, {
    get: (t, k) => (k in t ? t[k] : () => ({ addColorStop() {} })),
  })
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => ctx as unknown as RenderingContext)
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(600)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(400)
})
afterEach(() => {
  cleanup()
  vi.clearAllTimers()
  vi.useRealTimers()
  vi.restoreAllMocks()
})

const spot = { call: 'DL1AA', lat: 50, lon: 8, band: '20m', heardMe: false, ageSecs: 60, approx: false, freqMhz: 14.074, mode: 'FT8', entity: 'X' } as unknown as MapSpot
const quiet = { source: 'live', asOf: 1, spots: [spot], openings: [], dxpeditions: { workableNow: [] } } as unknown as PropagationSnapshot
// An open band: its wedge breathes once a second, and each breath draws one frame of a sleeping globe.
const breathing = {
  ...quiet,
  openings: [{ band: '20m', mode: 'F2', octant: 'E', bearingDeg: 60, maxKm: 7000, probability: 0.7 }],
} as unknown as PropagationSnapshot
const props = (myGrid: string, prop: PropagationSnapshot) => ({ myGrid, prop, selectedCall: null, onSelectCall: () => {} })

/** Mount, let the still globe fall asleep, and start counting nesting from there. */
async function asleep(myGrid = 'EN52', prop = quiet) {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<Globe3D {...props(myGrid, prop)} />)
  })
  await act(async () => void vi.advanceTimersByTime(5_000))
  expect(fake.loop.next, 'CONTROL: the still globe fell asleep').toBe(null)
  fake.loop.maxDepth = 0
  return r
}

describe('waking the sleeping 3-D globe never re-enters its own first frame', () => {
  it('▶ Spin on a sleeping globe draws one frame at a time, and turns', async () => {
    const r = await asleep()
    const frames = fake.loop.frames
    await act(async () => (r.container.querySelector('.globe3d-spin') as HTMLButtonElement).click())
    expect(fake.loop.maxDepth, 'frames nested inside the wake').toBe(1)
    expect(fake.loop.frames - frames, 'the wake drew its frame').toBe(1)
    expect(fake.loop.next, 'and the loop runs on').not.toBe(null)
    const a0 = fake.controls().getAzimuthalAngle()
    await act(async () => void vi.advanceTimersByTime(1_000))
    expect(Math.abs(fake.controls().getAzimuthalAngle() - a0), 'CONTROL: the globe really turns').toBeGreaterThan(0.01)
    expect(fake.loop.maxDepth, 'nor did any later frame nest').toBe(1)
  })

  it('the view moving to a new QTH while the globe sleeps is drawn there, one frame at a time', async () => {
    const r = await asleep('EN52')
    const before = fake.camera().position.clone()
    await act(async () => {
      r.rerender(<Globe3D {...props('JN58', quiet)} />)
    })
    expect(fake.camera().position.distanceTo(before), 'CONTROL: the view jumped').toBeGreaterThan(1)
    expect(fake.loop.maxDepth, 'frames nested inside the wake').toBe(1)
    await act(async () => void vi.advanceTimersByTime(5_000))
    expect(fake.loop.next, 'and it sleeps again').toBe(null)
  })

  it("a breath that finds the camera moved behind the loop's back draws it and stays awake to settle", async () => {
    await asleep('EN52', breathing)
    const frames = fake.loop.frames
    await act(async () => void vi.advanceTimersByTime(1_000))
    expect(fake.loop.frames - frames, 'CONTROL: a breath draws one frame').toBe(1)
    expect(fake.loop.next, 'CONTROL: and the globe sleeps straight after it').toBe(null)
    // Nothing announces this move: the next frame's update() is the first to see it.
    fake.camera().position.copy(fake.at(30, 60, 2.2))
    await act(async () => void vi.advanceTimersByTime(1_000))
    expect(fake.loop.maxDepth, 'frames nested inside the breath').toBe(1)
    expect(fake.loop.next, 'the move keeps it awake').not.toBe(null)
    await act(async () => void vi.advanceTimersByTime(5_000))
    expect(fake.loop.next, 'and then it sleeps').toBe(null)
  })
})

// UNSEEN, IT DRAWS NOTHING. A minimised window keeps its timers on every engine and its animation frames
// on some (WebKitGTK), and a globe out of sight in its pane costs the same as one in front of you; on a
// software renderer each frame is a large share of a core. So while the window is hidden or the globe is
// out of view, a wake or a breath draws nothing and only remembers that the picture is behind; coming
// back into view draws it once. The stub's timer loop stands in for an engine that keeps animating a
// hidden window.
class IO {
  static all: IO[] = []
  constructor(readonly cb: (e: Array<{ isIntersecting: boolean }>) => void) {
    IO.all.push(this)
  }
  observe() {}
  unobserve() {}
  disconnect() {
    IO.all = IO.all.filter((o) => o !== this)
  }
}
const inView = async (seen: boolean) =>
  act(async () => {
    for (const o of IO.all) o.cb([{ isIntersecting: seen }])
  })
const windowShown = async (shown: boolean) => {
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(!shown)
  await act(async () => void document.dispatchEvent(new Event('visibilitychange')))
}

describe('unseen, the 3-D globe draws nothing', () => {
  beforeEach(() => {
    IO.all = []
    ;(globalThis as unknown as { IntersectionObserver: unknown }).IntersectionObserver = IO
  })
  afterEach(() => {
    delete (globalThis as unknown as { IntersectionObserver?: unknown }).IntersectionObserver
  })

  it('a spinning globe stops drawing while the window is hidden, and turns again when shown', async () => {
    const r = await asleep()
    await act(async () => (r.container.querySelector('.globe3d-spin') as HTMLButtonElement).click())
    let frames = fake.loop.frames
    await act(async () => void vi.advanceTimersByTime(1_000))
    expect(fake.loop.frames - frames, 'CONTROL: shown, it spins').toBeGreaterThan(10)
    await windowShown(false)
    frames = fake.loop.frames
    await act(async () => void vi.advanceTimersByTime(5_000))
    expect(fake.loop.frames - frames, 'frames drawn while hidden').toBe(0)
    expect(fake.loop.next, 'the loop is stopped while hidden').toBe(null)
    await windowShown(true)
    frames = fake.loop.frames
    await act(async () => void vi.advanceTimersByTime(1_000))
    expect(fake.loop.frames - frames, 'shown again, it spins again').toBeGreaterThan(10)
    expect(fake.loop.maxDepth, 'no frame nested').toBe(1)
  })

  it('a change while hidden is drawn once the window is shown, and then the globe sleeps', async () => {
    const r = await asleep('EN52')
    await windowShown(false)
    const frames = fake.loop.frames
    await act(async () => {
      r.rerender(<Globe3D {...props('JN58', quiet)} />)
    })
    await act(async () => void vi.advanceTimersByTime(5_000))
    expect(fake.loop.frames - frames, 'frames drawn while hidden').toBe(0)
    await windowShown(true)
    expect(fake.loop.frames - frames, 'shown, the change is drawn').toBeGreaterThan(0)
    await act(async () => void vi.advanceTimersByTime(5_000))
    expect(fake.loop.next, 'and it sleeps again').toBe(null)
    expect(fake.loop.maxDepth, 'no frame nested').toBe(1)
  })

  it('a change still settling when the window hides is finished once it is shown', async () => {
    const r = await asleep('EN52')
    const more = { ...quiet, spots: [spot, { ...spot, call: 'JA1AA', lat: 35, lon: 139 }] } as unknown as PropagationSnapshot
    await act(async () => void r.rerender(<Globe3D {...props('EN52', more)} />))
    await act(async () => void vi.advanceTimersByTime(200))
    expect(fake.loop.next, 'CONTROL: awake, settling the change').not.toBe(null)
    await windowShown(false)
    expect(fake.loop.next, 'hidden mid-settle, the loop stops').toBe(null)
    const frames = fake.loop.frames
    await windowShown(true)
    expect(fake.loop.frames - frames, 'shown, the settle is finished').toBeGreaterThan(0)
  })

  it('showing a window whose still globe changed nothing draws nothing', async () => {
    await asleep('EN52')
    await windowShown(false)
    await act(async () => void vi.advanceTimersByTime(2_000))
    const frames = fake.loop.frames
    await windowShown(true)
    await act(async () => void vi.advanceTimersByTime(5_000))
    expect(fake.loop.frames - frames, 'frames drawn for nothing').toBe(0)
  })

  it('a breathing globe out of view draws nothing, and breathes again back in view', async () => {
    await asleep('EN52', breathing)
    // One second per act, so each breath's render is flushed before the next.
    const perSecond = async (n: number) => {
      const per: number[] = []
      for (let i = 0; i < n; i++) {
        const frames = fake.loop.frames
        await act(async () => void vi.advanceTimersByTime(1_000))
        per.push(fake.loop.frames - frames)
      }
      return per
    }
    expect(await perSecond(3), 'CONTROL: in view, it breathes once a second').toEqual([1, 1, 1])
    await inView(false)
    expect(await perSecond(5), 'frames drawn out of view').toEqual([0, 0, 0, 0, 0])
    await inView(true)
    await act(async () => void vi.advanceTimersByTime(5_000))
    expect(await perSecond(3), 'back in view, it breathes once a second again').toEqual([1, 1, 1])
  })
})
