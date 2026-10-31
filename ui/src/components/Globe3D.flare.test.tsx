// @vitest-environment jsdom
//
// WHAT THE OPERATOR SEES OF A FLARE ON THE 3-D GLOBE: the per-band rings (one line group per ring,
// cleared when the flare ends) and the flare chip, with below-M1 controls. The pure ring maths is
// pinned in flareNow.test.ts; this pins that the globe draws it.
//
// The react-globe.gl stub is the one Globe3D.sky.test.tsx uses (jsdom has no WebGL); Globe3D is the
// REAL component.
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
import { bandRings, flareFootprint } from '../flareNow'
import { t } from '../i18n'

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

function globe(xrayLong: number, prop = snap()) {
  return <Globe3D myGrid="EN52" prop={prop} selectedCall={null} onSelectCall={() => {}} stations={[]} xrayLong={xrayLong} />
}
async function mount(xrayLong: number, prop = snap()) {
  vi.useFakeTimers({ now: NOW })
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(globe(xrayLong, prop))
  })
  return r
}
/** The flare-ring line groups in the scene, by their syncLines key. */
const ringKeys = () =>
  fake
    .scene()
    .children.map((o) => o.name)
    .filter((n) => n.startsWith('flareRing:'))
    .sort()

describe('the per-band flare rings on the 3-D globe', () => {
  it('creates one line per ring at M5, and clears them when the flare ends', async () => {
    const r = await mount(M5)
    const want = bandRings(flareFootprint(NOW, M5, null)!, NOW)
      .map((w) => `flareRing:${w.band}:${w.db}`)
      .sort()
    expect(want.length, 'CONTROL: an M5 reaches several bands').toBeGreaterThan(4)
    expect(ringKeys()).toEqual(want)
    // The flux falls below M1: every ring line goes with it.
    await act(async () => void r.rerender(globe(C5)))
    expect(ringKeys()).toEqual([])
  })

  it('NEGATIVE CONTROL — below M1 no ring is created', async () => {
    await mount(C5)
    expect(ringKeys()).toEqual([])
  })
})

describe('the flare chip on the 3-D globe', () => {
  it('renders the chip during a flare, with the snapshot’s GOES time on its source line', async () => {
    const r = await mount(M5, snap(NOW_S - 120))
    const chip = r.container.querySelector('.wx-chip-stack .flare-chip')
    expect(chip, 'no flare chip on the globe').not.toBeNull()
    expect(chip!.querySelector('.flare-chip-src')!.textContent).toBe(t('map.flare.src.goes', { xtime: '17:58Z' }))
  })

  it('NEGATIVE CONTROL — below M1 there is no chip', async () => {
    const r = await mount(C5)
    expect(r.container.querySelector('.flare-chip')).toBeNull()
  })
})
