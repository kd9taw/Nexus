// @vitest-environment jsdom
//
// THE SUN AND THE MOON ON THE 3-D GLOBE — WHERE EACH IS OVERHEAD, THE MOON IN ITS PHASE, AND NOTHING
// MORE TO DRAW WHILE THEY SIT THERE.
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
    // What globeWebgl.tsx hands back when the globe goes (globeWebgl.test.tsx tests that part).
    renderer: () => renderer,
    // globe.gl's getter: where the camera is (its setter form is the same call with arguments).
    pointOfView: () => ({ lat: 0, lng: 0, altitude: 2.2 }),
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
    connect() {},
    disconnect() {},
  })
  const renderer = {
    domElement: document.createElement('canvas'),
    getContext: () => ({ isContextLost: () => false }),
    dispose() {},
    forceContextLoss() {},
  }
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
import { moonAt, subsolarPoint } from '../mapGeo'

type FakeGlobe = { paused: boolean; frames: number; scene: () => THREE_NS.Scene }
const fake = (ReactGlobe as unknown as { __fake: FakeGlobe }).__fake

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

type Op = { k: string; a: unknown[] }
const opsOf = new WeakMap<HTMLCanvasElement, Op[]>()

beforeEach(() => {
  fake.paused = false
  fake.frames = 0
  localStorage.clear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  // A recording context per canvas: the moon's sprite canvas is where its phase is painted.
  const ctxs = new WeakMap<HTMLCanvasElement, unknown>()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    if (!ctxs.has(this)) {
      const ops: Op[] = []
      opsOf.set(this, ops)
      ctxs.set(
        this,
        // The probe's context has no extensions to hand it back with.
        new Proxy({ getExtension: () => null } as Record<string | symbol, unknown>, {
          get: (t, k) =>
            k in t
              ? t[k]
              : (...a: unknown[]) => {
                  ops.push({ k: String(k), a })
                  return { addColorStop() {} }
                },
        }),
      )
    }
    return ctxs.get(this) as RenderingContext
  })
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

async function mountAt(nowMs: number, grid = 'EN52') {
  vi.useFakeTimers({ now: nowMs })
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<Globe3D myGrid={grid} prop={quiet} selectedCall={null} onSelectCall={() => {}} stations={[]} />)
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

/** 2026-09-13 21:00 UTC: a waxing crescent, 8.5 % lit, 58° from EN52 — on the face of the globe. */
const CRESCENT = Date.UTC(2026, 8, 13, 21, 0)
const moonSprite = () => named('sky-moon') as THREE_NS.Sprite | undefined
/** The terminator last painted on the moon's sprite, as a fraction of its radius (|2k − 1|), whether
 *  it bows toward the lit limb (a crescent), and whether the lit limb was mirrored to the left. */
function painted(): { width: number; crescent: boolean; mirrored: boolean } | null {
  const image = (moonSprite()?.material.map as { image?: HTMLCanvasElement } | null)?.image
  const ops = image ? (opsOf.get(image) ?? []) : []
  const kinds = ops.map((o) => o.k)
  const e = kinds.lastIndexOf('ellipse')
  if (e < 0) return null
  const save = kinds.lastIndexOf('save', e)
  return {
    width: (ops[e].a[2] as number) / (ops[e].a[3] as number),
    crescent: ops[e].a[7] === true,
    mirrored: ops.slice(save, e).some((o) => o.k === 'scale' && (o.a[0] as number) < 0),
  }
}

describe('the moon on the 3-D globe', () => {
  it('is on the globe where it is overhead, a little above the surface', async () => {
    await mountAt(CRESCENT)
    const moon = moonSprite()
    expect(moon, 'no moon in the scene').toBeTruthy()
    const want = moonAt(CRESCENT).sublunar
    expect(moon!.position.y, 'latitude').toBeCloseTo(want.lat, 6)
    expect(moon!.position.x, 'longitude').toBeCloseTo(want.lon, 6)
    expect(moon!.position.z).toBeGreaterThan(0)
    expect(moon!.visible).toBe(true)
  })

  it('its picture is its phase, lit on the right for a waxing moon seen from the north', async () => {
    await mountAt(CRESCENT)
    const k = moonAt(CRESCENT).illuminated
    const p = painted()
    expect(p, 'nothing painted on the moon').not.toBeNull()
    expect(p!.width).toBeCloseTo(Math.abs(2 * k - 1), 6)
    expect(p!.crescent).toBe(true)
    expect(p!.mirrored).toBe(false)
  })

  it('and lit on the left from the southern hemisphere', async () => {
    await mountAt(CRESCENT, 'QF56')
    expect(painted()!.mirrored).toBe(true)
  })

  it('follows the phase on the globe’s clock: a week later the crescent is gibbous', async () => {
    await mountAt(CRESCENT)
    vi.setSystemTime(CRESCENT + 7 * 86_400_000)
    await act(async () => void vi.advanceTimersByTime(60_000))
    const k = moonAt(Date.now()).illuminated
    expect(k, 'CONTROL: a week really fills it past half').toBeGreaterThan(0.6)
    const p = painted()!
    expect(p.width).toBeCloseTo(Math.abs(2 * k - 1), 3)
    expect(p.crescent, 'still painted as a crescent').toBe(false)
    const want = moonAt(Date.now()).sublunar
    expect(moonSprite()!.position.x, 'and it moved with the moon').toBeCloseTo(want.lon, 3)
  })

  it('the Sun and moon layer hides the moon too', async () => {
    await mountAt(CRESCENT)
    await act(async () => void fireEvent.click(screen.getByRole('checkbox', { name: 'Sun and moon' })))
    expect(moonSprite()?.visible).toBe(false)
  })
})

// A LAYOUT'S REACH INTO THE GLOBE (Connect's Frame turns the satellites on): the host rewrites this
// surface's stored layer picks, then bumps `layersRev`, and the globe reads them again.
describe('the globe reads its layers again when the host has rewritten them', () => {
  it('a new layersRev shows what the record now says', async () => {
    vi.useFakeTimers({ now: DUSK })
    const el = (rev: number) => (
      <Globe3D myGrid="EN52" prop={quiet} selectedCall={null} onSelectCall={() => {}} stations={[]} layersRev={rev} />
    )
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(el(0))
    })
    const sats = () => (screen.getByRole('checkbox', { name: 'Satellites' }) as HTMLInputElement).checked
    expect(sats(), 'CONTROL: off by default').toBe(false)
    const record = JSON.parse(localStorage.getItem('nexus.connect.globe3d.layers') ?? '{}')
    localStorage.setItem('nexus.connect.globe3d.layers', JSON.stringify({ ...record, sats: true }))
    await act(async () => r.rerender(el(0)))
    expect(sats(), 'CONTROL: a record changed under the globe is not read without the bump').toBe(false)
    await act(async () => r.rerender(el(1)))
    expect(sats()).toBe(true)
  })
})
