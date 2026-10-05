// @vitest-environment jsdom
//
// THE LOGBOOK GLOBE IS LIT BY THE SUN AND ITS LOW AMBIENT, AND BY NOTHING ELSE (it came out lit all
// round, washed out, with no night side, in most loads in some browsers).
//
// globe.gl hands three-render-objects a default pair of lights (an ambient at π and a directional at
// 0.6π) through its `lights`, and three-render-objects puts them in the scene in its update, which
// kapsule runs from a debounced timer. The globe says it is ready from another timer, and the Logbook's
// light effect runs from React's scheduler after that, so the defaults can reach the scene before the
// effect or after it, whichever the browser runs first: in real Chrome, 154 put them in first in every
// load measured, and 140 ran the effect first in most. The globe's lights were the scene's lights taken
// out by the effect, which worked only when the defaults were already in.
//
// jsdom has no WebGL, so `react-globe.gl` is a stub holding a real three.js scene, and its `lights` keeps
// three-render-objects' own books: the lights it was last given go into the scene at its next update,
// in place of the ones that went in at the update before (three-render-objects' update, with kapsule's
// record of each prop's value before its first change since the last update). Both orders are run.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { forwardRef, useEffect, useImperativeHandle, useMemo } from 'react'
import { act, cleanup, render } from '@testing-library/react'
import * as THREE from 'three'
import type { LogQuestion } from '../features/logAnswers'
import type { LoggedQso } from '../types'
import { GLOBE_AMBIENT, GLOBE_SUN } from '../features/globeBasemap'
import { subsolarPoint } from '../mapGeo'

const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
}))
// jsdom loads no images: the day picture is not what is under test, the lights are.
vi.mock('../features/globeBasemap', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../features/globeBasemap')>()),
  loadDayImage: () => Promise.resolve(null),
}))

/** three-globe's sphere radius, and where it puts (lat, lng) at `alt` globe radii up. */
const R = 100
function coords(lat: number, lng: number, alt = 0) {
  const phi = ((90 - lat) * Math.PI) / 180
  const theta = ((90 - lng) * Math.PI) / 180
  const r = R * (1 + alt)
  return { x: r * Math.sin(phi) * Math.cos(theta), y: r * Math.cos(phi), z: r * Math.sin(phi) * Math.sin(theta) }
}

/** The scene of the globe on show. */
let scene: THREE.Scene
/** When globe.gl's first update puts its lights in: before the globe says it is ready, or after the
 *  Logbook's effects for a ready globe have run. */
let defaultsLand: 'before ready' | 'after the effects'
/** globe.gl's timer firing: three-render-objects' update, as far as the lights go. */
let globeGlUpdate: () => void

vi.mock('react-globe.gl', () => ({
  default: forwardRef<unknown, Record<string, unknown>>(function Globe(props, ref) {
    const api = useMemo(() => {
      const canvas = document.createElement('canvas')
      const controls = { autoRotate: false, autoRotateSpeed: 0, connect() {}, disconnect() {} }
      const renderer = {
        domElement: canvas,
        capabilities: { maxTextureSize: 4096, getMaxAnisotropy: () => 1 },
        getContext: () => ({ isContextLost: () => false }),
        dispose() {},
        forceContextLoss() {},
      }
      // globe.gl's defaults, given before the first update, which puts them in: an ambient 0xcccccc at π
      // and a directional 0xffffff at 0.6π.
      let lights: THREE.Light[] = [new THREE.AmbientLight(0xcccccc, Math.PI), new THREE.DirectionalLight(0xffffff, 0.6 * Math.PI)]
      let changed: { was: THREE.Light[] | undefined } | null = { was: undefined }
      globeGlUpdate = () => {
        if (!changed) return
        ;(changed.was ?? []).forEach((l) => scene.remove(l))
        lights.forEach((l) => scene.add(l))
        changed = null
      }
      const methods = {
        scene: () => scene,
        renderer: () => renderer,
        controls: () => controls,
        camera: () => new THREE.PerspectiveCamera(),
        getCoords: coords,
        lights(next?: THREE.Light[]) {
          if (next === undefined) return lights
          if (!changed) changed = { was: lights }
          lights = next
          return methods
        },
        pauseAnimation() {},
        resumeAnimation() {},
      }
      return methods
    }, [])
    useImperativeHandle(ref, () => api, [api])
    useEffect(() => {
      if (defaultsLand === 'before ready') globeGlUpdate()
      ;(props.onGlobeReady as (() => void) | undefined)?.()
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
    return <div data-testid="globe" />
  }),
}))

import QsoGlobe from './QsoGlobe'

/** The sun over 18:00Z, so the day side faces the Americas. */
const NOW = Date.parse('2026-10-04T18:00:00Z')

const LOG: LoggedQso[] = [['FN31', '20m'], ['JO65', '15m'], ['PM95', '40m']].map(
  ([grid, band], i) =>
    ({
      call: `K${i}TEST`, grid, band, freqMhz: 14.074, mode: 'FT8', rstSent: '-10', rstRcvd: '-12', name: null, qth: null,
      comment: null, notes: null, country: null, whenUnix: 1_700_000_000 + i, confirmed: false, awardConfirmed: false,
      qslRcvd: null, qslSent: null, ota: null, upload: undefined,
    }) as unknown as LoggedQso,
)

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'], now: NOW })
  scene = new THREE.Scene()
  localStorage.clear()
  engineLog.mockResolvedValue(LOG)
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  ;(globalThis as unknown as { IntersectionObserver: unknown }).IntersectionObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  // No 2-D context in jsdom (the dots' sprite is then left blank).
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(1200)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(320)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.useRealTimers()
})

/** The lights in the globe's scene once it is ready and globe.gl's timer has fired since. */
async function lightsOnShow(): Promise<THREE.Light[]> {
  render(<QsoGlobe logTick={1} />)
  // The effects for a ready globe have run once its dot cloud is in the scene (the light effect is
  // declared before the cloud's, and they run in one go).
  let ran = false
  for (let i = 0; i < 50 && !ran; i++) {
    await act(async () => {
      await new Promise((ok) => setTimeout(ok, 10))
    })
    scene.traverse((o) => {
      if ((o as THREE.Points).isPoints) ran = true
    })
  }
  expect(ran, "the globe's effects never ran: no dot cloud in its scene").toBe(true)
  await act(async () => globeGlUpdate())
  const lights: THREE.Light[] = []
  scene.traverse((o) => {
    if ((o as THREE.Light).isLight) lights.push(o as THREE.Light)
  })
  return lights
}

const named = (l: THREE.Light) => `${l.type} #${l.color.getHexString()} at ${+l.intensity.toFixed(3)}`
const hex = (c: string) => new THREE.Color(c).getHexString()

describe("the Logbook globe's lights", () => {
  for (const order of ['before ready', 'after the effects'] as const) {
    it(`are the sun at the subsolar point and the low ambient, when globe.gl's own lights reach the scene ${order}`, async () => {
      defaultsLand = order
      const lights = await lightsOnShow()
      expect(lights.map(named).sort()).toEqual(
        [`AmbientLight #${hex(GLOBE_AMBIENT.color)} at ${GLOBE_AMBIENT.intensity}`, `DirectionalLight #${hex(GLOBE_SUN.color)} at ${GLOBE_SUN.intensity}`].sort(),
      )
      // The terminator is real: the sun shines from where it is overhead, not from the camera.
      const sun = lights.find((l) => (l as THREE.DirectionalLight).isDirectionalLight)!
      const ss = subsolarPoint(NOW)
      const want = coords(ss.lat, ss.lon, 2)
      expect(sun.position.clone().normalize().distanceTo(new THREE.Vector3(want.x, want.y, want.z).normalize())).toBeLessThan(1e-9)
    })
  }
})
