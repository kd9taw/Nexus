// THE 3-D GLOBES' BASE MAP — the Earth as NASA's satellites photographed it, for Connect's globe
// (Globe3D) and the logbook's (QsoGlobe): the Blue Marble on the day side and the Black Marble, the
// Earth's lights at night, on the night side, crossing at the live terminator, so the greyline is the
// boundary you see. The flat map keeps its own painted map (MapView, basemap.ts).
//
// THE PICTURES are 4096 × 2048 equirectangular WebPs (about 11 px per degree, 32 MB each on the GPU;
// a GPU that takes less gets them scaled down), made by scripts/gen-globe-textures.py. The day picture
// is the sphere's colour, lit by the sun at the subsolar point; the night picture is its emissive glow,
// shown only where the sun is down (nightOnlyEmissive). Neither follows the theme.
//
// THE LINES — the coast (land and lake shores), the country borders and the US states — are NOT in
// the texture: a texture blurs when the camera comes close, so they are GPU line segments a hair
// above the sphere, from Natural Earth data, in the theme's map colours, sharp at any distance. The
// caller picks the scale (1:50m from afar, 1:10m close in) and swaps the group when it changes.
//
// This module is only imported by the two lazy-loaded globes, so three.js stays out of the main
// bundle, as their headers promise.
import * as THREE from 'three'
import type { Basemap, BasemapInks, BasemapTile } from '../basemap'
import { STANDARD_MAP, type MapToken } from './skins'
import dayUrl from '../assets/earth-day.webp'

/** How the globes are lit: the sun at the subsolar point brings the day picture up to its own
 *  colours where the sun is overhead, and the ambient is low, so the night side is the night picture
 *  and not a dimmed day. (three.js lights are physical: a light's diffuse is intensity / π.) */
export const GLOBE_SUN = { color: '#fff6e8', intensity: 2.8 }
export const GLOBE_AMBIENT = { color: '#ffffff', intensity: 0.08 }
/** The night picture's strength (the material's emissiveIntensity) while the city lights show: in
 *  full, the Black Marble as NASA made it. */
export const GLOBE_CITY_LIGHTS = 1
/** Bloom on Connect's globe only lifts what is brighter than the lit land: spots, arcs, city lights. */
export const GLOBE_BLOOM_THRESHOLD = 0.85
/** Camera altitude (globe radii) below which the globe's lines switch to 1:10m, and above which they
 *  go back to 1:50m. In between, whichever is showing stays. */
export const GLOBE_TEN_BELOW = 0.9
export const GLOBE_FIFTY_ABOVE = 1.1

/** The night picture only where it is night. three.js adds a material's emissive light everywhere,
 *  which would light the day side with the city lights, so it is faded in across the terminator by
 *  the surface's angle to the sun (the scene's first directional light): none where the sun is more
 *  than about 5° up, all of it where the sun is more than about 7° down. The lights layer still sets
 *  the strength through `emissiveIntensity`. */
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

let day: Promise<HTMLImageElement | null> | null = null
/** The day side's picture (the Blue Marble), loaded once for every globe (null if it cannot be: the
 *  globe then stays the theme's sea colour). */
export function loadDayImage(): Promise<HTMLImageElement | null> {
  if (!day) {
    day = new Promise((ok) => {
      const img = new Image()
      img.onload = () => ok(img)
      img.onerror = () => ok(null)
      img.src = dayUrl
    })
  }
  return day
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
