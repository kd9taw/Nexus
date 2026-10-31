// @vitest-environment jsdom
//
// THE SELECTED STATION'S PATH RIBBON ON THE 3-D GLOBE.
//
// Both routes are drawn whenever a station is selected (short path solid, long path dashed), in
// place of the old single selected arc. Everything the ribbon puts into the scene — line groups,
// their materials, the bearing-label sprites and their textures — is removed AND disposed when the
// selection goes, and a 300 ms snapshot that changed nothing rebuilds nothing.
//
// jsdom has no WebGL, so `react-globe.gl` is a stub exposing the methods Globe3D calls; the scene
// is a real THREE.Scene and Globe3D is the real component (the pattern of Globe3D.render.test).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { forwardRef, useEffect, useImperativeHandle } from 'react'
import { render, cleanup, act } from '@testing-library/react'
import * as THREE from 'three'
import type { MapSpot, PathPrediction, PropagationSnapshot } from '../types'

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
  const scene = new THREE.Scene()
  const fake = {
    scene: () => scene,
    lights: () => [],
    getCoords: (lat: number, lng: number, alt = 0) => {
      const phi = ((90 - lat) * Math.PI) / 180
      const theta = ((90 - lng) * Math.PI) / 180
      const r = 100 * (1 + alt)
      return { x: r * Math.sin(phi) * Math.cos(theta), y: r * Math.cos(phi), z: r * Math.sin(phi) * Math.sin(theta) }
    },
    postProcessingComposer: () => ({ addPass() {}, passes: [] }),
    controls: () => controls,
    renderer: () => renderer,
    pointOfView: () => ({ lat: 0, lng: 0, altitude: 2.2 }),
    pauseAnimation: () => {},
    resumeAnimation: () => {},
  }
  const Globe = forwardRef<unknown, Record<string, unknown>>(function Globe(props, ref) {
    useImperativeHandle(ref, () => fake, [])
    useEffect(() => {
      ;(props.onGlobeReady as (() => void) | undefined)?.()
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
    return <div data-testid="globe" />
  })
  return { default: Globe, __scene: scene }
})

import Globe3D from './Globe3D'
import * as ReactGlobe from 'react-globe.gl'

vi.setConfig({ testTimeout: 15_000 })

const scene = (ReactGlobe as unknown as { __scene: THREE.Scene }).__scene

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

let disposed: Set<unknown>
beforeEach(() => {
  scene.clear()
  localStorage.clear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  const ctx = new Proxy({ getExtension: () => null } as Record<string | symbol, unknown>, {
    get: (t, k) => (k in t ? t[k] : () => ({ addColorStop() {} })),
  })
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => ctx as unknown as RenderingContext)
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(600)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(400)
  disposed = new Set()
  const md = THREE.Material.prototype.dispose
  vi.spyOn(THREE.Material.prototype, 'dispose').mockImplementation(function (this: THREE.Material) {
    disposed.add(this)
    md.call(this)
  })
  const td = THREE.Texture.prototype.dispose
  vi.spyOn(THREE.Texture.prototype, 'dispose').mockImplementation(function (this: THREE.Texture) {
    disposed.add(this)
    td.call(this)
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const VK = { call: 'VK2ABC', lat: -33.87, lon: 151.21, band: '20m', heardMe: false, ageSecs: 60, approx: false, freqMhz: 14.074, mode: 'FT8', entity: 'Australia' } as unknown as MapSpot
const snap = (): PropagationSnapshot =>
  ({ source: 'live', asOf: 1, spots: [{ ...VK }], openings: [], dxpeditions: { workableNow: [] } }) as unknown as PropagationSnapshot

/** The ribbon's objects in the scene: lines drawn with a dashed or basic material that is not one
 * of the cartographic overlays (those are off in a fresh store except the grey line, whose ink is
 * the terminator gold), and the label sprites. */
function ribbon() {
  const lines: THREE.Line[] = []
  const sprites: THREE.Sprite[] = []
  scene.traverse((o) => {
    if (o instanceof THREE.Sprite && o.material.map instanceof THREE.CanvasTexture) sprites.push(o)
    if (o instanceof THREE.Line) lines.push(o)
  })
  return { lines, dashed: lines.filter((l) => l.material instanceof THREE.LineDashedMaterial), sprites }
}

function outlook(): PathPrediction {
  const geometry = (distKm: number, bearingDeg: number) => ({
    distKm,
    bearingDeg,
    maxGmlatDeg: 70,
    nowHour: 0,
    hours: [],
    segmentsNow: [{ from: 0.4, to: 0.6, kind: 'polar' as const }],
  })
  return {
    engine: 'p533',
    bands: [],
    mufNow: 0,
    mufHourly: [],
    // EN52 (the QTH) to Sydney: ~14 900 km short path.
    geometry: geometry(14_900, 245),
    longPath: { engine: 'p533', bands: [], mufNow: 0, mufHourly: [], geometry: geometry(25_130, 65) },
    routes: [{ band: '20m', verdict: 'long', confidence: 'possible', shortBcr: 0, longBcr: 0.9, reasons: [{ kind: 'oneClosed', open: 'long' }] }],
  }
}

describe('the path ribbon on the globe', () => {
  it('draws both routes for a selected station, rebuilds nothing on an identical snapshot, and disposes it all when the selection goes', async () => {
    const base = { myGrid: 'EN52', onSelectCall: () => {}, stations: [] }
    const r = render(<Globe3D {...base} prop={snap()} selectedCall={null} />)
    await act(async () => {})
    const before = ribbon()
    r.rerender(<Globe3D {...base} prop={snap()} selectedCall="VK2ABC" />)
    await act(async () => {})
    const on = ribbon()
    // Short path + long path, the long one dashed; one bearing label each.
    expect(on.lines.length - before.lines.length).toBe(2)
    expect(on.dashed.length - before.dashed.length).toBe(1)
    expect(on.sprites.length - before.sprites.length).toBe(2)

    // The 300 ms snapshot: a new but identical prop. The same objects stay in the scene.
    r.rerender(<Globe3D {...base} prop={snap()} selectedCall="VK2ABC" />)
    await act(async () => {})
    const again = ribbon()
    // (Other overlays rebuild on every snapshot; the ribbon's own objects are its dashed long path
    // and its two labels.)
    expect(again.dashed).toEqual(on.dashed)
    for (const l of on.dashed) expect(again.dashed.includes(l)).toBe(true)
    for (const sp of on.sprites) expect(again.sprites.includes(sp)).toBe(true)

    // The outlook arrives: inks and the polar stretch (one per route) join the picture.
    r.rerender(<Globe3D {...base} prop={snap()} selectedCall="VK2ABC" outlook={outlook()} />)
    await act(async () => {})
    const inked = ribbon()
    expect(inked.lines.length - before.lines.length).toBe(4)
    // Every material and texture of the first build was disposed when it was replaced.
    const firstMats = on.lines.filter((l) => !before.lines.includes(l)).map((l) => l.material)
    const firstTex = on.sprites.filter((s) => !before.sprites.includes(s)).map((s) => s.material.map)
    for (const m of [...firstMats, ...firstTex]) expect(disposed.has(m)).toBe(true)

    // Deselect: back to the baseline, and the last build disposed too.
    r.rerender(<Globe3D {...base} prop={snap()} selectedCall={null} outlook={outlook()} />)
    await act(async () => {})
    const off = ribbon()
    expect(off.lines.length).toBe(before.lines.length)
    expect(off.sprites.length).toBe(before.sprites.length)
    const lastMats = inked.lines.filter((l) => !before.lines.includes(l)).map((l) => l.material)
    for (const m of lastMats) expect(disposed.has(m)).toBe(true)
  })
})
