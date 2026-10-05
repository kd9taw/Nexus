// @vitest-environment jsdom
//
// THE 3-D GLOBES SHOW THE EARTH AS NASA PHOTOGRAPHED IT: THE BLUE MARBLE BY DAY AND THE BLACK MARBLE,
// THE EARTH'S LIGHTS AT NIGHT, BY NIGHT, CROSSING AT THE LIVE TERMINATOR, SO THE GREYLINE IS THE
// BOUNDARY YOU SEE (operator's pick, 2026-10-05; the flat map keeps its own look). Both globes,
// Connect's (Globe3D) and the Logbook's (QsoGlobe), run through the same checks.
//
// jsdom has no WebGL, so `react-globe.gl` is a stub holding the material it is handed and the lights it
// is given (as globe.gl keeps them), and placing (lat, lng) where three-globe does. What a point of the
// globe shows is worked out the way three.js r185's Phong shader works it: the day picture times
// (ambient + sun × the cosine of the sun's angle there) / π, plus the night picture times the emissive
// strength, faded by the night mask — the line the globe's own material puts into three.js's own
// fragment shader, read back from it. The pictures' look on screen is checked in a real browser.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach, afterAll } from 'vitest'
import { forwardRef, useEffect, useImperativeHandle, useMemo, type ReactElement } from 'react'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import * as THREE from 'three'
import type { PropagationSnapshot } from '../types'
import dayUrl from '../assets/earth-day.webp'
import nightUrl from '../assets/earth-night.webp'
import reliefUrl from '../assets/earth-relief.webp'
import { GLOBE_AMBIENT, GLOBE_CITY_LIGHTS, GLOBE_SUN } from '../features/globeBasemap'
import { destinationPoint, subsolarPoint } from '../mapGeo'
import { gridToLatLon } from '../grid'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getAurora: vi.fn(async () => []),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getSatTrackStatus: vi.fn(async () => null),
  askLog: vi.fn(async () => []),
}))
vi.mock('three/examples/jsm/postprocessing/UnrealBloomPass.js', () => ({
  UnrealBloomPass: class {
    setSize() {}
    dispose() {}
  },
}))

/** three-globe's sphere radius, and where it puts (lat, lng) at `alt` globe radii up. */
const R = 100
function coords(lat: number, lng: number, alt = 0) {
  const phi = ((90 - lat) * Math.PI) / 180
  const theta = ((90 - lng) * Math.PI) / 180
  const r = R * (1 + alt)
  return { x: r * Math.sin(phi) * Math.cos(theta), y: r * Math.cos(phi), z: r * Math.sin(phi) * Math.sin(theta) }
}

/** The globe on show: the material it wears and the lights it was given. */
const shown: { material: THREE.MeshPhongMaterial | null; lights: THREE.Light[] } = { material: null, lights: [] }

vi.mock('react-globe.gl', () => ({
  default: forwardRef<unknown, Record<string, unknown>>(function Globe(props, ref) {
    shown.material = props.globeMaterial as THREE.MeshPhongMaterial
    const api = useMemo(() => {
      const scene = new THREE.Scene()
      const controls = Object.assign(new THREE.EventDispatcher<Record<string, object>>(), {
        autoRotate: false,
        autoRotateSpeed: 0,
        connect() {},
        disconnect() {},
      })
      const renderer = {
        domElement: document.createElement('canvas'),
        capabilities: { maxTextureSize: 4096, getMaxAnisotropy: () => 8 },
        getContext: () => ({ isContextLost: () => false }),
        dispose() {},
        forceContextLoss() {},
      }
      const methods = {
        scene: () => scene,
        renderer: () => renderer,
        controls: () => controls,
        camera: () => new THREE.PerspectiveCamera(),
        getCoords: coords,
        pointOfView: () => ({ lat: 0, lng: 0, altitude: 2.2 }),
        postProcessingComposer: () => ({ addPass() {}, passes: [] }),
        lights(next?: THREE.Light[]) {
          if (next === undefined) return shown.lights
          shown.lights = next
          return methods
        },
        pauseAnimation() {},
        resumeAnimation() {},
      }
      return methods
    }, [])
    useImperativeHandle(ref, () => api, [api])
    useEffect(() => {
      ;(props.onGlobeReady as (() => void) | undefined)?.()
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
    return <div data-testid="globe" />
  }),
}))

import Globe3D from './Globe3D'
import QsoGlobe from './QsoGlobe'

/** jsdom loads no images: this one loads as soon as it is given its address, so the day picture the
 *  globes ask for arrives, and says which file it is. */
class LoadingImage {
  onload: (() => void) | null = null
  onerror: (() => void) | null = null
  width = 4096
  height = 2048
  private url = ''
  get src() {
    return this.url
  }
  set src(url: string) {
    this.url = url
    queueMicrotask(() => this.onload?.())
  }
}
beforeAll(() => vi.stubGlobal('Image', LoadingImage))
afterAll(() => vi.unstubAllGlobals())

/** The addresses three.js's texture loader was asked for. */
let loaded: string[]
beforeEach(() => {
  shown.material = null
  shown.lights = []
  loaded = []
  localStorage.clear()
  const real = THREE.TextureLoader.prototype.load
  vi.spyOn(THREE.TextureLoader.prototype, 'load').mockImplementation(function (this: THREE.TextureLoader, url, onLoad, onProgress, onError) {
    loaded.push(url)
    return real.call(this, url, onLoad, onProgress, onError)
  })
  for (const name of ['ResizeObserver', 'IntersectionObserver'])
    (globalThis as unknown as Record<string, unknown>)[name] = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    }
  // Sprites paint on 2-D canvases; nothing here reads them back.
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(
    () =>
      new Proxy({ getExtension: () => null } as Record<string | symbol, unknown>, {
        get: (t, k) => (k in t ? t[k] : () => ({ addColorStop() {} })),
      }) as unknown as RenderingContext,
  )
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(1200)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(600)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.useRealTimers()
})

const quiet = { source: 'live', asOf: 1, spots: [], openings: [], dxpeditions: { workableNow: [] } } as unknown as PropagationSnapshot
const GLOBES: [string, () => ReactElement][] = [
  ['Connect', () => <Globe3D myGrid="EN52" prop={quiet} selectedCall={null} onSelectCall={() => {}} stations={[]} />],
  ['Logbook', () => <QsoGlobe logTick={1} />],
]

/** Mount a globe with the clock at `now`, and let it come up with its pictures. */
async function show(globe: () => ReactElement, now: number): Promise<THREE.MeshPhongMaterial> {
  vi.useFakeTimers({ now, toFake: ['Date', 'setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] })
  await act(async () => void render(globe()))
  await act(async () => void (await vi.advanceTimersByTimeAsync(10)))
  expect(shown.material, 'the globe was handed no material').toBeTruthy()
  return shown.material!
}

/** The night mask the material puts into three.js's own Phong fragment shader: the emissive is scaled by
 *  1 − smoothstep(lo, hi, cos of the sun's angle). */
function nightMask(m: THREE.MeshPhongMaterial): [number, number] {
  const shader = { fragmentShader: THREE.ShaderLib.phong.fragmentShader, vertexShader: '', uniforms: {} }
  m.onBeforeCompile(shader as unknown as THREE.WebGLProgramParametersWithUniforms, {} as THREE.WebGLRenderer)
  const hit = /totalEmissiveRadiance \*= 1\.0 - smoothstep\( (-?[\d.]+), (-?[\d.]+), dot\( normal, directionalLights\[ 0 \]\.direction \) \);/.exec(
    shader.fragmentShader,
  )
  expect(hit, "the globe's material puts no night mask into the shader").toBeTruthy()
  return [Number(hit![1]), Number(hit![2])]
}

const luma = (c: THREE.Color) => 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
/** What the globe shows at (lat, lng), as shares of each picture's own (linear) colour: `day`, the day
 *  picture lit by the ambient and the sun, and `night`, the night picture's glow; with `up`, the sun's
 *  height there in degrees. */
function shading(m: THREE.MeshPhongMaterial, at: { lat: number; lon: number }) {
  const sun = shown.lights.find((l): l is THREE.DirectionalLight => (l as THREE.DirectionalLight).isDirectionalLight)!
  const c = coords(at.lat, at.lon)
  const n = new THREE.Vector3(c.x, c.y, c.z).normalize()
  const cos = n.dot(sun.position.clone().sub(sun.target.position).normalize())
  const light = new THREE.Color(0, 0, 0)
  for (const l of shown.lights) if ((l as THREE.AmbientLight).isAmbientLight) light.add(l.color.clone().multiplyScalar(l.intensity))
  light.add(sun.color.clone().multiplyScalar(sun.intensity * Math.max(0, cos)))
  const [lo, hi] = nightMask(m)
  const t = Math.min(1, Math.max(0, (cos - lo) / (hi - lo)))
  return { up: (Math.asin(cos) * 180) / Math.PI, day: (luma(m.color) * luma(light)) / Math.PI, night: luma(m.emissive) * m.emissiveIntensity * (1 - t * t * (3 - 2 * t)) }
}

/** `deg` of arc from the point where the sun is overhead at `t`, along `bearing`. The greyline layer
 *  draws its circle at 90°. */
const fromSun = (t: number, deg: number, bearing: number) => destinationPoint(subsolarPoint(t), bearing, (6371 * deg * Math.PI) / 180)

/** 18:00Z puts the sun over the eastern Pacific off South America: the Americas in daylight, eastern
 *  Asia and Australia at night. At 23:00Z the greyline is just east of the QTH (EN52, the sun 5° up
 *  there): the West and Hawaii in daylight, Europe at night. */
const AFTERNOON = Date.parse('2026-10-04T18:00:00Z')
const LATE = Date.parse('2026-10-04T23:00:00Z')
const at = (grid: string) => gridToLatLon(grid)!

describe.each(GLOBES)("the %s globe's look", (_, globe) => {
  it('is the Blue Marble by day and the Black Marble by night, never the flat map’s relief', async () => {
    const m = await show(globe, AFTERNOON)
    expect(m.map, 'the day picture never went on').toBeTruthy()
    expect((m.map!.image as LoadingImage).src).toBe(dayUrl)
    expect(m.map!.colorSpace, 'the day picture is an sRGB photo').toBe(THREE.SRGBColorSpace)
    expect(m.color.getHexString(), 'the photo shows in its own colours, not tinted').toBe('ffffff')
    expect(m.emissiveMap, 'no night picture').toBeTruthy()
    expect(loaded).toContain(nightUrl)
    expect(m.emissiveMap!.colorSpace, 'the night picture is an sRGB photo').toBe(THREE.SRGBColorSpace)
    expect(m.emissiveIntensity, 'the night picture at its strength').toBe(GLOBE_CITY_LIGHTS)
    expect(reliefUrl, 'CONTROL: the flat map’s relief is a file of its own').not.toBe(dayUrl)
    expect([...loaded, (m.map!.image as LoadingImage).src], 'the globe wears the flat map’s relief').not.toContain(reliefUrl)
  })

  it('is lit by the sun where it is overhead, moving with the clock, and a low ambient', async () => {
    await show(globe, AFTERNOON)
    const named = (l: THREE.Light) => `${l.type} #${l.color.getHexString()} at ${l.intensity}`
    const hex = (c: string) => new THREE.Color(c).getHexString()
    expect(shown.lights.map(named).sort()).toEqual(
      [`AmbientLight #${hex(GLOBE_AMBIENT.color)} at ${GLOBE_AMBIENT.intensity}`, `DirectionalLight #${hex(GLOBE_SUN.color)} at ${GLOBE_SUN.intensity}`].sort(),
    )
    const sun = shown.lights.find((l) => (l as THREE.DirectionalLight).isDirectionalLight)!
    const toward = (t: number) => {
      const s = subsolarPoint(t)
      const c = coords(s.lat, s.lon)
      return new THREE.Vector3(c.x, c.y, c.z).normalize()
    }
    expect(sun.position.clone().normalize().distanceTo(toward(AFTERNOON))).toBeLessThan(1e-9)
    // An hour on, the terminator has moved 15° west, and the light with it.
    await act(async () => void (await vi.advanceTimersByTimeAsync(3600_000)))
    const later = AFTERNOON + 3600_000
    expect(toward(later).angleTo(toward(AFTERNOON)), 'CONTROL: an hour really moves the sun').toBeGreaterThan(0.2)
    expect(sun.position.clone().normalize().angleTo(toward(later)), 'the sun stayed where it was').toBeLessThan(0.005)
  })

  for (const [when, t, days, nights] of [
    ['18:00Z', AFTERNOON, ['EN52', 'FN31', 'GG66'], ['PM95', 'QF56', 'OL72']],
    ['23:00Z', LATE, ['CM87', 'DM79', 'BL11'], ['JO65', 'IO91', 'KP20']],
  ] as const) {
    it(`at ${when}: the day picture where the sun is up, the night picture where it is down`, async () => {
      const m = await show(globe, t)
      // Overhead, the day picture is at about its own colours (the sun's warm tint takes a little).
      expect(shading(m, subsolarPoint(t)).day).toBeGreaterThan(0.8)
      expect(shading(m, subsolarPoint(t)).night).toBe(0)
      for (const g of days) {
        const s = shading(m, at(g))
        expect(s.up, `CONTROL: the sun is up at ${g}`).toBeGreaterThan(5)
        expect(s.night, `the night picture shows by day at ${g}`).toBe(0)
        expect(s.day, `the day picture is dark at ${g}, the sun ${s.up.toFixed(0)}° up`).toBeGreaterThan(0.15)
      }
      for (const g of nights) {
        const s = shading(m, at(g))
        expect(s.up, `CONTROL: the sun is down at ${g}`).toBeLessThan(-7)
        expect(s.night, `the night picture is not in full at ${g}`).toBeCloseTo(1, 9)
        expect(s.day, `the day picture shows at night at ${g}`).toBeLessThan(0.03)
      }
    })

    it(`at ${when}: the two cross at the greyline`, async () => {
      const m = await show(globe, t)
      for (let bearing = 0; bearing < 360; bearing += 30) {
        // On the greyline the sun adds nothing, and the night picture is part of the way in.
        const line = shading(m, fromSun(t, 90, bearing))
        expect(Math.abs(line.up), `CONTROL: the point at bearing ${bearing} is on the greyline`).toBeLessThan(1e-6)
        expect(line.night).toBeGreaterThan(0.2)
        expect(line.night).toBeLessThan(0.6)
        expect(line.day).toBeLessThan(0.03)
        // 5° to the day side, none of the night picture; 7° to the night side, all of it.
        expect(shading(m, fromSun(t, 85, bearing)).night, `the night picture reaches 5° into the day at bearing ${bearing}`).toBe(0)
        expect(shading(m, fromSun(t, 97, bearing)).night, `the night picture is not in full 7° into the night at bearing ${bearing}`).toBeCloseTo(1, 9)
      }
    })
  }
})

describe("Connect's City lights layer", () => {
  it('takes the night picture away and puts it back in full', async () => {
    const m = await show(GLOBES[0][1], LATE)
    const box = screen.getByRole('checkbox', { name: 'City lights' })
    expect((box as HTMLInputElement).checked, 'CONTROL: the layer starts on').toBe(true)
    await act(async () => void fireEvent.click(box))
    expect(shading(m, at('JO65')).night, 'the lights stayed on').toBe(0)
    await act(async () => void fireEvent.click(box))
    expect(shading(m, at('JO65')).night).toBeCloseTo(1, 9)
  })
})
