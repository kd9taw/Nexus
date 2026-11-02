// @vitest-environment jsdom
//
// THE SIX-HOUR REPLAY ON THE 3-D GLOBE: the replay bar heads the chip stack and reads REPLAY, the sun
// (its sprite and its light) stands where it stood at the replayed time, and the flare rings are the
// replayed flux's, from the held GOES trace — with a quiet now as the control. Back to now restores
// now. The react-globe.gl stub is Globe3D.flare.test.tsx's; Globe3D is the REAL component.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { forwardRef, useEffect, useImperativeHandle } from 'react'
import { render, cleanup, act } from '@testing-library/react'
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
import { bandRings, flareFootprint, type FlareNoaa } from '../flareNow'
import { subsolarPoint } from '../mapGeo'
import { fireEvent } from '@testing-library/react'

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
        // No extensions: these are the sprites' 2-D canvases.
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

const NOW = Date.UTC(2026, 9, 30, 18, 0)
const NOW_S = NOW / 1000
const M5 = 5e-5
const C5 = 5e-6
const snap = (xrayAt?: number) =>
  ({
    source: 'live',
    asOf: NOW_S,
    spots: [],
    openings: [],
    dxpeditions: { workableNow: [] },
    spaceWx: { sfi: 150, kp: 2, aIndex: 5, xrayClass: 'M5.0', flare: true, xrayLong: M5, xrayAt },
  }) as unknown as PropagationSnapshot

/** 5-minute bucket peaks from six hours ago to five minutes ago, C1 except an M5 two hours ago. */
const PEAK_S = NOW_S - 2 * 3600
function trace(): [number, number][] {
  const out: [number, number][] = []
  for (let t = NOW_S - 6 * 3600; t <= NOW_S - 300; t += 300) out.push([t, t === PEAK_S ? M5 : 1e-6])
  return out
}
const lane: FlareNoaa = { xrayAt: NOW_S - 60, series: trace(), recent: [], event: null, drap: null }
function globe(xrayLong: number, prop = snap()) {
  return (
    <Globe3D myGrid="EN52" prop={prop} selectedCall={null} onSelectCall={() => {}} stations={[]} xrayLong={xrayLong} flareNoaa={lane} />
  )
}
async function mount(xrayLong: number, prop = snap()) {
  vi.useFakeTimers({ now: NOW })
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(globe(xrayLong, prop))
  })
  return r
}
const named = (n: string) => fake.scene().children.find((o) => o.name === n)
/** The flare-ring line groups in the scene, by their syncLines key. */
const ringKeys = () =>
  fake
    .scene()
    .children.map((o) => o.name)
    .filter((n) => n.startsWith('flareRing:'))
    .sort()


describe('the replay on the 3-D globe', () => {
  it('reads REPLAY, moves the sun to the replayed time and draws the replayed flare', async () => {
    const r = await mount(C5)
    expect(ringKeys(), 'CONTROL: a quiet now draws no ring').toEqual([])
    const now = subsolarPoint(NOW)
    expect(named('sky-sun')!.position.x, 'CONTROL: the sun stands at now').toBeCloseTo(now.lon, 6)
    const btn = r.container.querySelector('.replay-start') as HTMLButtonElement
    expect(btn, 'no replay button in the globe’s Layers panel').not.toBeNull()
    await act(async () => void fireEvent.click(btn))
    const bar = r.container.querySelector('.wx-chip-stack .replay-chip')
    expect(bar, 'no replay bar on the globe').not.toBeNull()
    expect(bar!.textContent).toContain('REPLAY')
    const then = subsolarPoint(PEAK_S * 1000)
    expect(named('sky-sun')!.position.x, 'the sun did not move to the replayed time').toBeCloseTo(then.lon, 6)
    expect(named('sky-sun')!.position.y).toBeCloseTo(then.lat, 6)
    const want = bandRings(flareFootprint(PEAK_S * 1000, M5, null)!, PEAK_S * 1000)
      .map((w) => `flareRing:${w.band}:${w.db}`)
      .sort()
    expect(want.length, 'CONTROL: an M5 reaches several bands').toBeGreaterThan(4)
    expect(ringKeys()).toEqual(want)
    // Back to now.
    await act(async () => void fireEvent.click(bar!.querySelector('.replay-back')!))
    expect(r.container.querySelector('.replay-chip')).toBeNull()
    expect(ringKeys()).toEqual([])
    expect(named('sky-sun')!.position.x).toBeCloseTo(now.lon, 6)
  })

  it('stands the flare chip for now down while it replays', async () => {
    const r = await mount(M5)
    expect(r.container.querySelector('.flare-chip:not(.replay-chip)'), 'CONTROL: now’s chip').not.toBeNull()
    await act(async () => void fireEvent.click(r.container.querySelector('.replay-start')!))
    expect(r.container.querySelector('.flare-chip:not(.replay-chip)')).toBeNull()
    expect(r.container.querySelector('.replay-chip')).not.toBeNull()
  })
})
