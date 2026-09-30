// @vitest-environment jsdom
//
// THE SUN ON THE 3-D GLOBE — WHERE IT IS OVERHEAD, AND NOTHING MORE TO DRAW WHILE IT SITS THERE.
//
// The globe lit its day side from a light at the subsolar point but showed no sun. The marker is a
// sprite just above that point, moved on the globe's existing 60 s sun clock with ONE drawn frame,
// so the render-on-change loop (Globe3D.render.test.tsx §2) still goes to sleep with it on screen.
//
// jsdom has no WebGL, so `react-globe.gl` is a stub whose `getCoords` is invertible — it hands back
// the latitude, longitude and altitude it was asked for as x, y and z — so the test reads where each
// marker was put straight off its position. Globe3D itself is the REAL component.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { forwardRef, useEffect, useImperativeHandle } from 'react'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import type { PropagationSnapshot } from '../types'
import type * as THREE_NS from 'three'

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
  const scene = new THREE.Scene()
  const fake = {
    lights: () => [],
    // Invertible: x = longitude, y = latitude, z = altitude — where a marker went, read back.
    getCoords: (lat: number, lng: number, alt = 0) => ({ x: lng, y: lat, z: alt }),
    scene: () => scene,
    postProcessingComposer: () => ({ addPass() {}, passes: [] }),
    controls: () => controls,
    pointOfView: () => {},
    paused: false,
    frames: 0,
    pauseAnimation: () => {
      fake.paused = true
    },
    resumeAnimation: () => {
      fake.paused = false
      fake.frames++
    },
  }
  const controls = Object.assign(new THREE.EventDispatcher<Record<string, object>>(), {
    autoRotate: false,
    autoRotateSpeed: 0,
  })
  const Globe = forwardRef<unknown, Record<string, unknown>>(function Globe(props, ref) {
    useImperativeHandle(ref, () => fake, [])
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
import { subsolarPoint } from '../mapGeo'

type FakeGlobe = { paused: boolean; frames: number; scene: () => THREE_NS.Scene }
const fake = (ReactGlobe as unknown as { __fake: FakeGlobe }).__fake

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  fake.paused = false
  fake.frames = 0
  localStorage.clear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  const ctx = new Proxy({} as Record<string | symbol, unknown>, {
    get: (t, k) => (k in t ? t[k] : () => ({ addColorStop() {} })),
  })
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => ctx as unknown as RenderingContext)
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(600)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(400)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.useRealTimers()
  // The stub's scene outlives a test; empty it so no marker from one test answers for the next.
  const s = fake.scene()
  for (const o of [...s.children]) s.remove(o)
})

const quiet = { source: 'live', asOf: 1, spots: [], openings: [], dxpeditions: { workableNow: [] } } as unknown as PropagationSnapshot
const DUSK = Date.UTC(2026, 8, 19, 18, 0)

async function mountAt(nowMs: number) {
  vi.useFakeTimers({ now: nowMs })
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<Globe3D myGrid="EN52" prop={quiet} selectedCall={null} onSelectCall={() => {}} stations={[]} />)
  })
  return r
}
const named = (name: string) => fake.scene().getObjectByName(name)
const settle = () => act(async () => void vi.advanceTimersByTime(5_000))

describe('the sun on the 3-D globe', () => {
  it('is on the globe where it is overhead, a little above the surface', async () => {
    await mountAt(DUSK)
    const sun = named('sky-sun')
    expect(sun, 'no sun in the scene').toBeTruthy()
    expect(sun!.visible).toBe(true)
    const want = subsolarPoint(DUSK)
    expect(sun!.position.y, 'latitude').toBeCloseTo(want.lat, 6)
    expect(sun!.position.x, 'longitude').toBeCloseTo(want.lon, 6)
    expect(sun!.position.z, 'altitude: off the surface, so the planet never cuts it in half').toBeGreaterThan(0)
  })

  it('the Sun and moon layer hides it, and shows it again', async () => {
    await mountAt(DUSK)
    const box = screen.getByRole('checkbox', { name: 'Sun and moon' })
    await act(async () => void fireEvent.click(box))
    expect(named('sky-sun')?.visible, 'unticked, the sun is still shown').toBe(false)
    await act(async () => void fireEvent.click(box))
    expect(named('sky-sun')?.visible).toBe(true)
  })

  it('follows the sun on the globe’s 60 s clock, and the globe goes back to sleep', async () => {
    await mountAt(DUSK)
    await settle()
    expect(fake.paused, 'CONTROL: a still globe sleeps with the sun on it').toBe(true)
    await act(async () => void vi.advanceTimersByTime(60_000))
    // The clock ticks a minute after the globe mounted, and the sun stands where it was at the tick.
    const want = subsolarPoint(DUSK + 60_000)
    expect(want.lon, 'CONTROL: the minute really moved the overhead point').not.toBeCloseTo(subsolarPoint(DUSK).lon, 2)
    expect(named('sky-sun')!.position.x, 'the sun did not move with the clock').toBeCloseTo(want.lon, 6)
    expect(fake.paused, 'moving the sun woke the render loop').toBe(true)
  })

  it('costs no frame of its own: a minute draws as many frames with the sun as without it', async () => {
    // The minute already draws (the day/night light follows the sun on it); the marker rides
    // those frames. Measured with the layer on, then off, over the same minute.
    const minute = async () => {
      await settle()
      const before = fake.frames
      await act(async () => void vi.advanceTimersByTime(60_000))
      return fake.frames - before
    }
    await mountAt(DUSK)
    expect(named('sky-sun')?.visible, 'CONTROL: the first globe really shows the sun').toBe(true)
    const withSun = await minute()
    cleanup()
    vi.useRealTimers()
    localStorage.setItem('nexus.connect.globe3d.layers', JSON.stringify({ sunMoon: false }))
    await mountAt(DUSK)
    expect(named('sky-sun')?.visible ?? false, 'CONTROL: the second globe really has the sun off').toBe(false)
    const without = await minute()
    expect(without, 'CONTROL: the minute draws something to compare against').toBeGreaterThan(0)
    expect(withSun).toBe(without)
  })
})
