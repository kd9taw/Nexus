// THE 3-D GLOBES' BASE MAP — the 2-D map's own picture on the sphere, for Connect's globe
// (Globe3D) and the logbook's (QsoGlobe), so switching between the flat map and the globe never
// changes the look.
//
// THE TEXTURE is the flat map's paintEquirect, run once onto an offscreen equirectangular canvas:
// the theme's sea and land, the shaded relief, lakes and the major rivers. 4096 × 2048 where the
// GPU takes it (about 11 px per degree, 32 MB), else what it does take. It is repainted when the
// theme changes (the --map-* tokens), never per frame.
//
// THE LINES — the coast (land and lake shores), the country borders and the US states — are NOT in
// the texture: a texture blurs when the camera comes close, so they are GPU line segments a hair
// above the sphere, from the same Natural Earth data, sharp at any distance. The caller picks the
// scale (1:50m from afar, 1:10m close in) and swaps the group when it changes.
//
// This module is only imported by the two lazy-loaded globes, so three.js stays out of the main
// bundle, as their headers promise.
import * as THREE from 'three'
import {
  paintEquirect,
  paintRelief,
  reliefAlphaFor,
  type Basemap,
  type BasemapInks,
  type BasemapTile,
} from '../basemap'
import { STANDARD_MAP, type MapToken } from './skins'
import reliefUrl from '../assets/earth-relief.webp'

/** How the globes are lit now that they carry the map's own colours: the sun at the subsolar point
 *  brings the day side up to about the flat map's colours, and the ambient holds the night side at
 *  a little under half of them — about the flat map's greyline shading — with the city lights
 *  showing there. (three.js lights are physical: a directional light's diffuse is intensity / π.) */
export const GLOBE_SUN = { color: '#fff6e8', intensity: 1.75 }
export const GLOBE_AMBIENT = { color: '#ffffff', intensity: 0.45 }
/** Bloom on Connect's globe only lifts what is brighter than the lit land: spots, arcs, city lights. */
export const GLOBE_BLOOM_THRESHOLD = 0.85
/** Camera altitude (globe radii) below which the globe's lines switch to 1:10m, and above which they
 *  go back to 1:50m. In between, whichever is showing stays. */
export const GLOBE_TEN_BELOW = 0.9
export const GLOBE_FIFTY_ABOVE = 1.1

/** City lights only where it is night. three.js adds a material's emissive light everywhere, which
 *  on the map-coloured day side reads as white blots, so it is faded out across the terminator by
 *  the surface's angle to the sun (the scene's first directional light). The lights layer still
 *  sets the strength through `emissiveIntensity`. */
export function nightOnlyEmissive(mat: THREE.MeshPhongMaterial): void {
  mat.onBeforeCompile = (shader) => {
    shader.fragmentShader = shader.fragmentShader.replace(
      '#include <emissivemap_fragment>',
      [
        '#include <emissivemap_fragment>',
        '#if NUM_DIR_LIGHTS > 0',
        '  totalEmissiveRadiance *= 1.0 - smoothstep( -0.12, 0.08, dot( normal, directionalLights[ 0 ].direction ) );',
        '#endif',
      ].join('\n'),
    )
  }
}

/** The basemap's inks from the theme's --map-* tokens, as MapView reads them (the standard
 *  basemap where no sheet is loaded). */
export function readMapInks(): BasemapInks {
  const css = getComputedStyle(document.documentElement)
  const ink = (t: MapToken) => css.getPropertyValue(t).trim() || STANDARD_MAP[t]
  return { land: ink('--map-land'), water: ink('--map-ocean'), river: ink('--map-rim'), coast: ink('--map-coast'), state: ink('--map-state') }
}

let relief: Promise<HTMLImageElement | null> | null = null
/** The shaded relief, loaded once for every globe (null if it cannot be: the globe is then painted
 *  without it). */
export function loadRelief(): Promise<HTMLImageElement | null> {
  if (!relief) {
    relief = new Promise((ok) => {
      const img = new Image()
      img.onload = () => ok(img)
      img.onerror = () => ok(null)
      img.src = reliefUrl
    })
  }
  return relief
}

const WORLD = { kind: 'rect', west: -180, south: -90, east: 180, north: 90 } as const

/** Paint the globe's texture onto `canvas` (2:1): the flat map's picture without its lines. */
export function paintGlobeTexture(canvas: HTMLCanvasElement, map: Basemap, inks: BasemapInks, img: HTMLImageElement | null): void {
  const ctx = canvas.getContext('2d')
  if (!ctx) return
  const w = canvas.width
  const h = canvas.height
  const k = w / 360
  ctx.setTransform(1, 0, 0, 1, 0, 0)
  ctx.globalAlpha = 1
  ctx.globalCompositeOperation = 'source-over'
  ctx.fillStyle = inks.water
  ctx.fillRect(0, 0, w, h)
  paintEquirect(
    ctx,
    k,
    w / 2,
    h / 2,
    { map, region: WORLD, inks, pxPerDeg: k, coast: 0, states: 0 },
    img ? (c) => paintRelief(c, img, 0, 0, w, h, w, h, reliefAlphaFor(k)) : undefined,
  )
}

/** three-globe's sphere: radius 100, lat/lon placed by its polar2Cartesian. The lines ride 0.15%
 *  above it (about 10 km), clear of the sphere's flat facets and of depth-buffer ties, and well
 *  under every overlay three-globe draws. */
const GLOBE_R = 100
const LIFT = 1.0015

function put(out: Float32Array, at: number, lon: number, lat: number): void {
  const phi = ((90 - lat) * Math.PI) / 180
  const theta = ((90 - lon) * Math.PI) / 180
  const r = GLOBE_R * LIFT
  out[at] = r * Math.sin(phi) * Math.cos(theta)
  out[at + 1] = r * Math.cos(phi)
  out[at + 2] = r * Math.sin(phi) * Math.sin(theta)
}

/** Point-index pairs, one per segment, for the outlines of polygon tiles (their shore runs) or the
 *  lines of line tiles. */
function segmentsOf(tiles: BasemapTile[], shores: boolean): Float32Array {
  let n = 0
  for (const t of tiles) {
    if (shores) for (let k = 0; t.shore && k < t.shore.length; k += 3) n += t.shore[k + 1] - 1 + t.shore[k + 2]
    else for (let l = 0; l + 1 < t.parts.length; l++) n += t.parts[l + 1] - t.parts[l] - 1
  }
  const out = new Float32Array(n * 6)
  let at = 0
  const seg = (c: Float32Array, a: number, b: number) => {
    put(out, at, c[2 * a], c[2 * a + 1])
    put(out, at + 3, c[2 * b], c[2 * b + 1])
    at += 6
  }
  for (const t of tiles) {
    if (shores) {
      for (let k = 0; t.shore && k < t.shore.length; k += 3) {
        const s = t.shore[k]
        const e = s + t.shore[k + 1]
        for (let i = s; i + 1 < e; i++) seg(t.coords, i, i + 1)
        if (t.shore[k + 2]) seg(t.coords, e - 1, s)
      }
    } else {
      for (let l = 0; l + 1 < t.parts.length; l++) for (let i = t.parts[l]; i + 1 < t.parts[l + 1]; i++) seg(t.coords, i, i + 1)
    }
  }
  return out
}

function lines(name: string, positions: Float32Array, ink: string, opacity: number): THREE.LineSegments {
  const geom = new THREE.BufferGeometry()
  geom.setAttribute('position', new THREE.BufferAttribute(positions, 3))
  const mat = new THREE.LineBasicMaterial({ color: new THREE.Color(ink), transparent: true, opacity, depthWrite: false })
  const obj = new THREE.LineSegments(geom, mat)
  obj.name = name
  return obj
}

/** The globe's lines from `map`: children named `coast`, `borders` and `states`. */
export function globeLines(map: Basemap, inks: BasemapInks): THREE.Group {
  const g = new THREE.Group()
  g.add(lines('states', segmentsOf(map.states.tiles, false), inks.state, 0.85))
  g.add(lines('borders', segmentsOf(map.borders.tiles, false), inks.coast, 0.7))
  g.add(lines('coast', segmentsOf([...map.land.tiles, ...map.lakes.tiles], true), inks.coast, 0.95))
  return g
}

/** Free a group made by `globeLines`. */
export function disposeGlobeLines(g: THREE.Group): void {
  for (const c of g.children) {
    const l = c as THREE.LineSegments
    l.geometry.dispose()
    ;(l.material as THREE.Material).dispose()
  }
}
