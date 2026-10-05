// @vitest-environment jsdom
//
// THE LOGBOOK GLOBE'S QSO DOTS SHOW ON THE MAP UNDER THEM (operator, 2026-10-05: on the Logbook globe the
// map shows, but the QSOs are not visible on it).
//
// The dots were ADDED to the picture as light (THREE.AdditiveBlending), which reads only on a dark globe.
// The globe now carries the flat map's own light colours, and light added to them clips to white: in
// Chrome every dot became a pale speck, its band colour gone. They are painted now, over the coast,
// border and state lines, as the 2-D map paints its spots.
//
// jsdom has no WebGL, so `react-globe.gl` is a stub holding a real three.js scene, and what the GPU would
// put where a dot lands is worked out from the dot's material: three.js asks WebGL for NormalBlending as
// (SRC_ALPHA, ONE_MINUS_SRC_ALPHA) and for AdditiveBlending as (SRC_ALPHA, ONE), both on the canvas's
// sRGB values. The maps under the dots are the standard basemap and every built-in theme's, the colours
// the globe's texture is painted from (features/globeBasemap.ts).
//
// What a dot must keep is its COLOUR (OKLCH chroma): the band is what it says, and a dot that loses its
// colour reads as a speck of the map. Worked out this way, added dots kept 63 to 119 % of it on the dark
// map they were designed for and 0 to 25 % on the light one (Chrome, on the lit globe, read them back
// with about a third of the colour they had before); painted, they keep 94 to 99 % on every map. The
// dark edge that sets a dot off a map of its own colour is in the sprite, which jsdom cannot draw.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { forwardRef, useEffect, useImperativeHandle, useMemo } from 'react'
import { act, cleanup, render } from '@testing-library/react'
import * as THREE from 'three'
import type { LogQuestion } from '../features/logAnswers'
import type { LoggedQso } from '../types'
import { STANDARD_MAP, SKINS } from '../features/skins'
import { bandColor } from '../bandColors'
import { gridToLatLon } from '../grid'
import { deltaE, oklch, parseHex, type Rgb } from '../cssCascade'

const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
}))
// jsdom loads no images and has no Path2D: the texture is not what is under test, the dots are.
vi.mock('../features/globeBasemap', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../features/globeBasemap')>()),
  loadRelief: () => Promise.resolve(null),
  paintGlobeTexture: () => {},
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
      return {
        scene: () => scene,
        lights: () => [],
        renderer: () => renderer,
        controls: () => controls,
        camera: () => new THREE.PerspectiveCamera(),
        getCoords: coords,
        pauseAnimation() {},
        resumeAnimation() {},
      }
    }, [])
    useImperativeHandle(ref, () => api, [api])
    useEffect(() => {
      ;(props.onGlobeReady as (() => void) | undefined)?.()
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
    return <div data-testid="globe" />
  }),
}))

import QsoGlobe from './QsoGlobe'

/** One worked square per band, in every direction from EN52. */
const SQUARES: [string, string][] = [
  ['FN31', '20m'], ['CM87', '40m'], ['EM12', '80m'], ['JO65', '15m'], ['IO91', '17m'], ['KG33', '10m'],
  ['GG66', '12m'], ['FK68', '30m'], ['PM95', '160m'], ['QF56', '6m'], ['FN20', '2m'], ['RE78', '60m'],
]
const qso = (grid: string, band: string, i: number): LoggedQso =>
  ({
    call: `K${i}TEST`, grid, band, freqMhz: 14.074, mode: 'FT8', rstSent: '-10', rstRcvd: '-12', name: null, qth: null,
    comment: null, notes: null, country: null, whenUnix: 1_700_000_000 + i, confirmed: false, awardConfirmed: false,
    qslRcvd: null, qslSent: null, ota: null, upload: undefined,
  }) as unknown as LoggedQso
const LOG = SQUARES.flatMap(([grid, band], s) => [0, 1, 2].map((k) => qso(grid, band, s * 3 + k)))

beforeEach(() => {
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
  // No 2-D context in jsdom (the sprite is then left blank: its look is checked in a real browser).
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(1200)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(320)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

/** The globe shown with the log's dots in it (and whatever else its effects add by then). */
async function dots(): Promise<THREE.Points> {
  render(<QsoGlobe logTick={1} />)
  let cloud: THREE.Points | undefined
  for (let i = 0; i < 50 && !cloud; i++) {
    await act(async () => {
      await new Promise((ok) => setTimeout(ok, 10))
    })
    scene.traverse((o) => {
      if ((o as THREE.Points).isPoints && (o as THREE.Points).geometry.attributes.position?.count === SQUARES.length) cloud = o as THREE.Points
    })
  }
  expect(cloud, `no cloud of ${SQUARES.length} dots in the globe's scene`).toBeDefined()
  return cloud!
}

/** The colour the dot's vertex gives it (three.js keeps it linear; the canvas gets sRGB). */
function dotColour(cloud: THREE.Points, i: number): Rgb {
  const c = new THREE.Color().fromBufferAttribute(cloud.geometry.attributes.color as THREE.BufferAttribute, i)
  const { r, g, b } = c.getRGB({ r: 0, g: 0, b: 0 }, THREE.SRGBColorSpace)
  return [r, g, b].map((v) => Math.round(Math.min(1, Math.max(0, v)) * 255)) as unknown as Rgb
}

/** What lands on the canvas where the dot's core (the sprite's opaque white) is drawn over `under`. */
function composite(m: THREE.PointsMaterial, dot: Rgb, under: Rgb): Rgb {
  const a = m.opacity
  const out = (f: (d: number, u: number) => number) => dot.map((d, i) => Math.round(Math.min(255, f(d, under[i])))) as unknown as Rgb
  if (m.blending === THREE.NormalBlending) return m.transparent ? out((d, u) => d * a + u * (1 - a)) : dot
  if (m.blending === THREE.AdditiveBlending) return out((d, u) => d * a + u)
  throw new Error(`the dots' blending (${m.blending}) is not one this test works out`)
}

/** The maps a dot can land on: the standard one (the light map), and each built-in theme's own. */
const MAPS: [string, Record<string, string>][] = [
  ['the standard map', STANDARD_MAP],
  ...SKINS.filter((s) => s.map).map((s): [string, Record<string, string>] => [`the ${s.id} theme's map`, s.map as Record<string, string>]),
]

describe('the Logbook globe draws every worked square', () => {
  it('as one dot just above the globe, on its square', async () => {
    const cloud = await dots()
    const pos = cloud.geometry.attributes.position
    const v = new THREE.Vector3()
    SQUARES.forEach(([grid], i) => {
      v.fromBufferAttribute(pos, i)
      const ll = gridToLatLon(grid)!
      const want = coords(ll.lat, ll.lon)
      expect(v.length(), `${grid} under the globe's surface`).toBeGreaterThan(R)
      expect(v.length(), `${grid} far off the surface`).toBeLessThan(R * 1.01)
      expect(v.clone().normalize().distanceTo(new THREE.Vector3(want.x, want.y, want.z).normalize()), `${grid} off its square`).toBeLessThan(1e-6)
    })
  })

  it('keeping its colour on whatever map is under it, light or dark', async () => {
    const cloud = await dots()
    const m = cloud.material as THREE.PointsMaterial
    const washed: string[] = []
    let checked = 0
    for (const [name, map] of MAPS) {
      for (const surface of ['--map-land', '--map-ocean'] as const) {
        const under = parseHex(map[surface])!
        SQUARES.forEach(([, band], i) => {
          const dot = dotColour(cloud, i)
          const shown = composite(m, dot, under)
          const kept = oklch(shown).C / oklch(dot).C
          checked++
          if (!(kept >= 0.5))
            washed.push(`${band} on ${name}'s ${surface === '--map-land' ? 'land' : 'sea'} ${map[surface]}: rgb(${dot}) shows as rgb(${shown}), ${Math.round(kept * 100)} % of its colour`)
        })
      }
    }
    expect(checked).toBe(MAPS.length * 2 * SQUARES.length)
    expect(washed, washed.join('\n')).toEqual([])
  })

  it("in its band's colour", async () => {
    const cloud = await dots()
    SQUARES.forEach(([grid, band], i) => {
      // The busier squares read brighter, so a three-QSO square is a little darker than the band.
      expect(deltaE(dotColour(cloud, i), parseHex(bandColor(band))!), `${grid} (${band})`).toBeLessThan(0.1)
    })
  })

  it('over the coast, border and state lines', async () => {
    const cloud = await dots()
    // The lines arrive with the basemap, after the globe is ready.
    let lines: THREE.Object3D[] = []
    for (let i = 0; i < 50 && lines.length === 0; i++) {
      await act(async () => {
        await new Promise((ok) => setTimeout(ok, 10))
      })
      lines = []
      scene.traverse((o) => {
        if ((o as THREE.LineSegments).isLineSegments) lines.push(o)
      })
    }
    expect(lines.length, "no coast, border or state lines in the globe's scene").toBeGreaterThan(0)
    // Both are transparent, so three.js draws them by renderOrder first, then back to front: only a
    // higher renderOrder puts every dot over every line from every side of the globe.
    for (const l of lines) expect(cloud.renderOrder, `the ${l.name} lines draw over the dots`).toBeGreaterThan(l.renderOrder)
  })
})
