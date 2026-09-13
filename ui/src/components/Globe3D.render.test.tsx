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
import { render, cleanup, act } from '@testing-library/react'
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
    controls: () => ({ autoRotate: false, autoRotateSpeed: 0, addEventListener() {}, removeEventListener() {} }),
    pointOfView: () => {},
    pauseAnimation: () => {},
    resumeAnimation: () => {},
  }
  const Globe = forwardRef<unknown, Record<string, unknown>>(function Globe(props, ref) {
    useImperativeHandle(ref, () => fake, [])
    renders.push(props)
    useEffect(() => {
      ;(props.onGlobeReady as (() => void) | undefined)?.()
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
    return <div data-testid="globe" />
  })
  return { default: Globe }
})

import Globe3D from './Globe3D'

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  renders.length = 0
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
