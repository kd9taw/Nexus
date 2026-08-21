// The opt-in WebGL 3-D globe (react-globe.gl → globe.gl → three.js) for higher-end
// machines. The 2-D Canvas globe (MapView) stays the universal default; this is lazy-
// loaded, so a low-end shack PC never downloads three.js unless the operator turns it on.
// It reuses the SAME propagation data as MapView (spots, the operator's QTH, the selected
// station) and renders it on a real textured sphere with a dark night-earth mood, a
// subsolar day/night terminator, band-colored spots, selected/heard-me great-circle arcs,
// a QTH ping, a starfield, and bloom. Phase A of the 3-D plan (look + foundation).
// On a tracked satellite pass it ALSO becomes the "this pass" view (satellite visual
// design §3.3): the orbit arc ahead/behind, the bird's footprint, and a line-of-sight
// ray from the QTH to the bird — the 2-D map stays the "everything at once" view.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Satellite names, bands, grids,
// bearings, elevations, km and the layer ids are technical tokens and stay here; `MUF` is
// the acronym itself and is a named constant below. The prose is in the catalog under
// `globe.*`, and the two legends + the ★-filter hint come from `map.*` because the 2-D map
// and this globe are deliberately identical there.
import { Fragment, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { heatPulse, sectorPulse } from '../features/pulse'
import {
  filterSatsToChased,
  isSatChased,
  satChaseKeys,
  satFavOnly,
  setSatFavOnly,
  SAT_CHASE_EVENT,
} from '../features/satChase'
import { workedGridSet } from '../coverage'
import * as THREE from 'three'
import { UnrealBloomPass } from 'three/examples/jsm/postprocessing/UnrealBloomPass.js'
import Globe, { type GlobeMethods } from 'react-globe.gl'
import earthUrl from '../assets/earth-relief.webp'
import earthNightUrl from '../assets/earth-night.webp'
import { gridToLatLon } from '../grid'
import { bandColor, openingModeColor } from '../bandColors'
import {
  subsolarPoint,
  usStateBorders,
  flareField,
  flareRScale,
  destinationPoint,
  rangeRing,
  sectorRing,
} from '../mapGeo'
import { getAurora, getPca, getSatellites, getSatTrackStatus, getLog } from '../api'
import cqzonesUrl from '../data/cqzones.geojson?url'
import { spotTooltip } from '../propViz'
import { t, type MessageKey } from '../i18n'
import { MapInsightRail } from './prop/MapInsightRail'
import { MapLegend, MufLegend } from './MapLegend'
import type {
  PropagationSnapshot,
  PathPrediction,
  MufStation,
  AuroraPoint,
  PcaView,
  SatView,
  SatTrackStatus,
  Station,
} from '../types'

const EARTH_KM = 6371 // for altKm → globe-radius altitude units

/**
 * Opening-sector stacking radii, in globe-radius units.
 *
 * ⚠️ THESE MUST ALL DIFFER, and the outline MUST clear every fill. The fills and their outlines
 * were both at a flat 0.006 — identical radius — so each outline was coplanar with its own cap,
 * and every overlapping pair of wedges was coplanar with the other. Two surfaces at the same
 * depth have no correct draw order, so the GPU resolves it per-pixel and per-frame: the
 * flickering the operator reported once the geometry stopped tearing.
 *
 * `SECTOR_ALT_STEP` × 6371 km ≈ 2.5 km — invisible as height at any usable zoom, but far coarser
 * than the depth buffer at this range, which is what breaks the tie.
 */
const SECTOR_FILL_ALT = 0.006
const SECTOR_ALT_STEP = 0.0004
/** Above the highest fill any plausible number of simultaneous openings can reach. */
const SECTOR_OUTLINE_ALT = 0.014

/** Interpolate a satellite's subpoint from its per-minute ground track at unix `tSec`. */
function satPosAt(track: [number, number, number][], tSec: number): { lat: number; lon: number } | null {
  if (track.length === 0) return null
  if (tSec <= track[0][0]) return { lat: track[0][1], lon: track[0][2] }
  for (let i = 1; i < track.length; i++) {
    if (tSec <= track[i][0]) {
      const [t0, la0, lo0] = track[i - 1]
      const [t1, la1, lo1] = track[i]
      const f = (tSec - t0) / (t1 - t0 || 1)
      let dlon = lo1 - lo0
      if (dlon > 180) dlon -= 360
      if (dlon < -180) dlon += 360
      return { lat: la0 + (la1 - la0) * f, lon: lo0 + dlon * f }
    }
  }
  const last = track[track.length - 1]
  return { lat: last[1], lon: last[2] }
}

/**
 * The tracked pass's palette. Teal is the 2-D map's CHASED-bird colour (2D↔3D
 * parity — one bird must not change colour between the two surfaces). The sight
 * line is deliberately the only WARM, SOLID, off-surface line the globe draws:
 * every hue here is already spoken for (the band palette runs the whole wheel),
 * so shape and value are what separate it from the orbit, not hue alone.
 */
const PASS_TRACK = '#5eead4'
const PASS_LOS = '#ffe9c4'
/** Camera height for pass framing: a bird anywhere above the QTH's horizon is at
 * most ~25° of arc away, and both ends fit comfortably at this altitude. */
const PASS_ALT = 1.5

/**
 * Where the bird IS, from the tracker's own look angle + slant range: its
 * sub-point, its altitude, and the ground distance from the QTH.
 *
 * Plane geometry in the QTH's local vertical — the sight line's radial component
 * is `range·sin(el)` (on top of the Earth's radius) and its horizontal component
 * `range·cos(el)`, so the Earth-centred angle out to the sub-point is the atan2
 * of the two. Deriving the whole scene from the SAME numbers the rotor is
 * following is what keeps it honest: no second propagation of our own to drift
 * away from the tracker.
 */
export function birdFromLook(
  qth: { lat: number; lon: number },
  azDeg: number,
  elDeg: number,
  rangeKm: number,
): { lat: number; lon: number; altKm: number; groundKm: number } {
  const el = (elDeg * Math.PI) / 180
  const radial = EARTH_KM + rangeKm * Math.sin(el)
  const horiz = rangeKm * Math.cos(el)
  const groundKm = Math.atan2(horiz, radial) * EARTH_KM
  const sub = destinationPoint(qth, azDeg, groundKm)
  return { lat: sub.lat, lon: sub.lon, altKm: Math.hypot(radial, horiz) - EARTH_KM, groundKm }
}

/** Seconds → "m:ss", floored at zero (a pass never counts past LOS). */
function mmss(secs: number): string {
  const s = Math.max(0, Math.floor(secs))
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}

type RGB = [number, number, number]
interface CloudSample {
  lat: number
  lng: number
  rgb: RGB
  alt?: number
}

/** Create/update a GPU point-cloud layer (space-weather fields) on the globe scene. One
 * THREE.Points per layer, per-vertex colored, additively blended — cheap for dense fields
 * and always bright (no lighting). `store` persists the Points across renders. */
function syncCloud(
  g: GlobeMethods,
  store: Record<string, THREE.Points>,
  key: string,
  samples: CloudSample[],
  size: number,
  visible: boolean,
  sprite?: THREE.Texture,
) {
  let pts = store[key]
  if (!visible || samples.length === 0) {
    if (pts) pts.visible = false
    return
  }
  const pos = new Float32Array(samples.length * 3)
  const col = new Float32Array(samples.length * 3)
  for (let i = 0; i < samples.length; i++) {
    const s = samples[i]
    const c = g.getCoords(s.lat, s.lng, s.alt ?? 0.004)
    pos[i * 3] = c.x
    pos[i * 3 + 1] = c.y
    pos[i * 3 + 2] = c.z
    col[i * 3] = s.rgb[0]
    col[i * 3 + 1] = s.rgb[1]
    col[i * 3 + 2] = s.rgb[2]
  }
  if (!pts) {
    const mat = new THREE.PointsMaterial({
      size,
      map: sprite ?? null,
      vertexColors: true,
      transparent: true,
      opacity: 0.9,
      sizeAttenuation: false,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
    })
    pts = new THREE.Points(new THREE.BufferGeometry(), mat)
    store[key] = pts
    g.scene().add(pts)
  }
  pts.geometry.setAttribute('position', new THREE.BufferAttribute(pos, 3))
  pts.geometry.setAttribute('color', new THREE.BufferAttribute(col, 3))
  ;(pts.material as THREE.PointsMaterial).size = size
  pts.visible = true
}

/** Create/update a line-overlay layer (range rings, CQ zones) as a Group of THREE.Lines. */
function syncLines(
  g: GlobeMethods,
  store: Record<string, THREE.Group>,
  key: string,
  polylines: [number, number][][], // each = [lat, lng][]
  color: string,
  opacity: number,
  visible: boolean,
  alt = 0.002,
) {
  const prev = store[key]
  if (prev) {
    g.scene().remove(prev)
    prev.traverse((o) => (o as THREE.Line).geometry?.dispose?.())
    delete store[key]
  }
  if (!visible || polylines.length === 0) return
  const grp = new THREE.Group()
  const mat = new THREE.LineBasicMaterial({ color, transparent: true, opacity })
  for (const line of polylines) {
    const pts = line.map(([la, lo]) => {
      const c = g.getCoords(la, lo, alt)
      return new THREE.Vector3(c.x, c.y, c.z)
    })
    if (pts.length > 1) grp.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints(pts), mat))
  }
  store[key] = grp
  g.scene().add(grp)
}

const lerp = (a: number, b: number, t: number) => a + (b - a) * Math.max(0, Math.min(1, t))
/** Fire palette for the flare D-RAP HAF (MHz): yellow (low) → deep red (high). */
const flareRgb = (haf: number): RGB => {
  const t = haf / 30
  return [1, lerp(0.95, 0.25, t), lerp(0.35, 0.1, t)]
}
/** Aurora probability (8–90%): green (low) → red (high). */
const auroraRgb = (prob: number): RGB => {
  const t = (prob - 8) / 82
  return [lerp(0.25, 0.95, t), lerp(0.9, 0.25, t), 0.28]
}
/** MUF (MHz, 7–30): cool blue (low) → warm red (high). */
const mufRgb = (mhz: number): RGB => {
  const t = (mhz - 7) / 23
  return [lerp(0.3, 1, t), lerp(0.55, 0.32, t), lerp(1, 0.2, t)]
}
/** #rrggbb → normalized RGB (for the band-colored heat layer). */
const hexRgb = (hex: string): RGB => {
  const c = new THREE.Color(hex)
  return [c.r, c.g, c.b]
}

interface Props {
  /** The operator's Maidenhead grid — places + frames the QTH. */
  myGrid: string
  /** The propagation snapshot (spots + the on-map insight rail's data). */
  prop: PropagationSnapshot | null | undefined
  /** The selected station's call (drives the highlighted arc), or null. */
  selectedCall: string | null
  /** Click a spot → select it (same handler as the 2-D map). */
  onSelectCall: (call: string | null) => void
  /** Path/band outlook for the insight rail's MUF ceiling. */
  outlook?: PathPrediction | null
  /** Focus a band from the insight rail. */
  onBandClick?: (band: string) => void
  /** The currently focused band. */
  activeBand?: string | null
  /** Ionosonde MUF stations (the only overlay feed that comes via a prop, like 2-D). */
  muf?: MufStation[]
  /** GOES long-band X-ray flux (W/m²) — drives the flare D-RAP layer. */
  xrayLong?: number | null
  /** The operator's own decoded stations (the 'My decodes' layer). */
  stations?: Station[]
  /** Draw US state borders (default on, matching the 2-D map). */
  showStates?: boolean
}

const GETTING_OUT = '#3ddc6a' // a station that heard ME (matches the 2-D map)

let glowTex: THREE.CanvasTexture | null = null
/** Soft radial sprite for the heat layer — one blob per spot, additive, so overlapping
 * spots build the same kernel-density aura the 2-D map paints (its radial-gradient
 * splats). Built once. */
function glowSprite(): THREE.CanvasTexture {
  if (glowTex) return glowTex
  const c = document.createElement('canvas')
  c.width = 64
  c.height = 64
  const ctx = c.getContext('2d')
  if (ctx) {
    const g = ctx.createRadialGradient(32, 32, 0, 32, 32, 32)
    g.addColorStop(0, 'rgba(255,255,255,0.9)')
    g.addColorStop(0.35, 'rgba(255,255,255,0.4)')
    g.addColorStop(1, 'rgba(255,255,255,0)')
    ctx.fillStyle = g
    ctx.fillRect(0, 0, 64, 64)
  }
  glowTex = new THREE.CanvasTexture(c)
  return glowTex
}

/** Canvas-text sprite for the opening-sector labels ("6m Sporadic-E") — the 2-D map
 * labels its sectors; the globe must too (2D↔3D parity). */
function textSprite(text: string, color: string): THREE.Sprite {
  const c = document.createElement('canvas')
  c.width = 256
  c.height = 48
  const ctx = c.getContext('2d')
  if (ctx) {
    ctx.font = 'bold 26px system-ui'
    ctx.textAlign = 'center'
    ctx.textBaseline = 'middle'
    ctx.shadowColor = 'rgba(0,0,0,0.8)'
    ctx.shadowBlur = 6
    ctx.fillStyle = color
    ctx.fillText(text, 128, 24)
  }
  const tex = new THREE.CanvasTexture(c)
  const mat = new THREE.SpriteMaterial({ map: tex, transparent: true, depthWrite: false })
  const sp = new THREE.Sprite(mat)
  sp.scale.set(22, 4.1, 1)
  return sp
}

/** `MUF` is the acronym for Maximum Usable Frequency — a technical token that reads the
 * same in every language, so it is a constant rather than a catalog entry. It is the one
 * layer in the list below whose whole name is a token. */
const MUF_LABEL = 'MUF'

type GlobeLayerKey =
  | 'spots'
  | 'decodes'
  | 'arcs'
  | 'dxped'
  | 'heat'
  | 'openings'
  | 'flare'
  | 'aurora'
  | 'muf'
  | 'pca'
  | 'greyline'
  | 'sats'
  | 'pass'
  | 'rings'
  | 'cqzones'
  | 'coverage'
  | 'states'
  | 'grid'
  | 'lights'

/** The layer rows in panel order. Each name is looked up when the panel RENDERS, never at
 * import — this is module state, and resolving it here would freeze the load-time locale. */
type GlobeLayerRow = { k: GlobeLayerKey } & ({ labelKey: MessageKey } | { label: string })
const LAYER_ROWS: readonly GlobeLayerRow[] = [
  { k: 'spots', labelKey: 'globe.layer.spots' },
  { k: 'decodes', labelKey: 'globe.layer.decodes' },
  { k: 'arcs', labelKey: 'globe.layer.arcs' },
  { k: 'dxped', labelKey: 'globe.layer.dxped' },
  { k: 'heat', labelKey: 'globe.layer.heat' },
  { k: 'openings', labelKey: 'globe.layer.openings' },
  { k: 'flare', labelKey: 'globe.layer.flare' },
  { k: 'aurora', labelKey: 'globe.layer.aurora' },
  { k: 'muf', label: MUF_LABEL },
  { k: 'pca', labelKey: 'globe.layer.pca' },
  { k: 'greyline', labelKey: 'globe.layer.greyline' },
  { k: 'sats', labelKey: 'globe.layer.sats' },
  { k: 'pass', labelKey: 'globe.layer.pass' },
  { k: 'rings', labelKey: 'globe.layer.rings' },
  { k: 'cqzones', labelKey: 'globe.layer.cqzones' },
  { k: 'coverage', labelKey: 'globe.layer.coverage' },
  { k: 'states', labelKey: 'globe.layer.states' },
  { k: 'grid', labelKey: 'globe.layer.grid' },
  { k: 'lights', labelKey: 'globe.layer.lights' },
]

/** Is a WebGL context creatable? Guards against a low-end GPU that flipped the toggle. */
function webglOk(): boolean {
  try {
    const c = document.createElement('canvas')
    return !!(c.getContext('webgl2') || c.getContext('webgl'))
  } catch {
    return false
  }
}

export default function Globe3D({
  myGrid,
  prop,
  selectedCall,
  onSelectCall,
  outlook,
  onBandClick,
  activeBand,
  muf,
  xrayLong,
  stations,
  showStates = true,
}: Props) {
  const spots = useMemo(() => prop?.spots ?? [], [prop])
  const wrapRef = useRef<HTMLDivElement>(null)
  const globeRef = useRef<GlobeMethods | undefined>(undefined)
  const cloudsRef = useRef<Record<string, THREE.Points>>({})
  const linesRef = useRef<Record<string, THREE.Group>>({})
  // Opening-sector label sprites, rebuilt with the openings (disposed each pass).
  const openingLabelsRef = useRef<THREE.Sprite[]>([])
  const satGroupRef = useRef<THREE.Group | null>(null)
  const satMarkersRef = useRef<Record<string, THREE.Object3D>>({})
  // Name-designation sprites, one per drawn bird — moved with their markers.
  const satLabelsRef = useRef<Record<string, THREE.Sprite>>({})
  const bloomRef = useRef<UnrealBloomPass | null>(null)
  const [size, setSize] = useState({ w: 0, h: 0 })
  // Spot hover tooltip (mirrors the 2-D map's .map-hover) — text + wrap-relative position.
  const [hover, setHover] = useState<{ x: number; y: number; text: string } | null>(null)
  const [ready, setReady] = useState(false)
  const [ok] = useState(webglOk)
  const [spin, setSpin] = useState(false) // idle auto-rotate; OFF by default (continuous 60fps
  // GPU load on weak/laptop iGPUs); operator-toggleable
  const [nowMs, setNowMs] = useState(() => Date.now())
  // Self-fetched space-weather feeds (aurora + PCA come from their own polls, like the 2-D map).
  const [auroraPts, setAuroraPts] = useState<AuroraPoint[]>([])
  const [pca, setPca] = useState<PcaView | null>(null)
  const [sats, setSats] = useState<SatView | null>(null)
  // ★/All chip state + star-set revision, synced by SAT_CHASE_EVENT. Without
  // these in the sat-scene deps the chip's flip reached the 3-D sky only on
  // the next 30 s poll — the pane beside it had already changed (the "one
  // choice, three surfaces" promise held in storage but not on screen).
  const [satFav, setSatFav] = useState(() => satFavOnly())
  const [satChaseRev, setSatChaseRev] = useState(0)
  useEffect(() => {
    const onChange = () => {
      setSatFav(satFavOnly())
      setSatChaseRev((r) => r + 1)
    }
    window.addEventListener(SAT_CHASE_EVENT, onChange)
    return () => window.removeEventListener(SAT_CHASE_EVENT, onChange)
  }, [])
  const [cqzones, setCqzones] = useState<[number, number][][]>([]) // each zone → boundary lines
  const [workedGrids, setWorkedGrids] = useState<{ lat: number; lon: number }[]>([])
  // Toggleable 3-D layers. Default-on mirrors the 2-D map (aurora off by default).
  const [show, setShow] = useState({
    spots: true,
    arcs: true,
    states: showStates,
    lights: true,
    flare: true,
    aurora: false,
    muf: true,
    pca: true,
    heat: true,
    openings: true,
    grid: false,
    sats: false,
    pass: true, // the tracked-pass scene; nothing is drawn unless a pass is live
    rings: true,
    cqzones: false,
    coverage: false,
    decodes: true,
    dxped: false,
    greyline: true,
  })

  // Measure the container BEFORE paint so the globe is never sized to the whole window
  // (react-globe.gl's default when width/height are undefined) — that was painting over
  // the Connect rails/strip. We also gate the <Globe> render on a real size below.
  useLayoutEffect(() => {
    const el = wrapRef.current
    if (!el) return
    const measure = () => setSize({ w: el.clientWidth, h: el.clientHeight })
    measure()
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const qth = useMemo(() => gridToLatLon(myGrid), [myGrid])

  // Opening-sector FILLS (2D↔3D parity): the 2-D map fills each wedge at ~16% alpha;
  // the globe's outline-only sectors read as stray arcs (operator report). Same wedge
  // geometry as the syncLines outlines, rendered via globe.gl's native polygons layer.
  const sectorPolys = useMemo(() => {
    type Poly = {
      geometry: { type: 'Polygon'; coordinates: number[][][] }
      fill: string
      alt: number
    }
    if (!qth) return [] as Poly[]
    const polys: Poly[] = []
    let i = 0
    for (const o of prop?.openings ?? []) {
      if (!(o.maxKm > 0)) continue
      const ring = sectorRing(qth, o.bearingDeg, o.maxKm) as unknown as number[][]
      const c = new THREE.Color(openingModeColor(o.mode))
      polys.push({
        geometry: { type: 'Polygon', coordinates: [ring] },
        fill: `rgba(${Math.round(c.r * 255)}, ${Math.round(c.g * 255)}, ${Math.round(c.b * 255)}, 0.16)`,
        // ⚠️ EVERY SECTOR GETS ITS OWN RADIUS (operator report: heavy flickering across the
        // non-green wedges, after the tearing fix). These were ALL at a flat 0.006, so any two
        // openings whose wedges overlapped had exactly coplanar caps — the GPU has no
        // consistent way to order two surfaces at the same depth, so it picks per-pixel,
        // per-frame, and the overlap boils. Fanning them out by index gives the depth buffer an
        // unambiguous order. The step is ~2.5 km on a 6371 km globe: far too small to see as
        // height, far larger than the depth buffer's resolution at this range.
        alt: SECTOR_FILL_ALT + i * SECTOR_ALT_STEP,
      })
      i++
    }
    return polys
  }, [qth, prop])

  // Spots → globe points (band-colored; green = heard me). `label` carries the SAME
  // hover-tooltip line the 2-D map shows (shared builder), so the two read identically.
  const points = useMemo(
    () =>
      spots.map((s) => ({
        lat: s.lat,
        lng: s.lon,
        call: s.call,
        color: s.heardMe ? GETTING_OUT : bandColor(s.band),
        label: spotTooltip(s),
      })),
    [spots],
  )

  // Great-circle arcs from the QTH to the SELECTED station + every heard-me station.
  const arcs = useMemo(() => {
    if (!qth) return []
    return spots
      .filter((s) => s.heardMe || s.call === selectedCall)
      .map((s) => ({
        startLat: qth.lat,
        startLng: qth.lon,
        endLat: s.lat,
        endLng: s.lon,
        color: s.call === selectedCall ? '#a9d4ff' : s.heardMe ? GETTING_OUT : bandColor(s.band),
      }))
  }, [spots, selectedCall, qth])

  // US state borders as globe paths (one path per border line-string).
  const statePaths = useMemo(() => {
    // usStateBorders() returns a GeoJSON MultiLineString mesh (lon/lat coords).
    const geo = usStateBorders() as unknown as { coordinates?: [number, number][][] }
    return (geo.coordinates ?? []).map((line) => line.map(([lon, lat]) => [lat, lon] as [number, number]))
  }, [])

  const rings = qth ? [{ lat: qth.lat, lng: qth.lon }] : []

  // The globe surface material: the day-side texture darkened toward the 2-D globe's
  // night-earth mood. Built here (not via a ref getter — react-globe.gl takes it as a
  // prop) so it's ready before first paint. Lit by the subsolar light set up below.
  const globeMat = useMemo(() => {
    const loader = new THREE.TextureLoader()
    const day = loader.load(earthUrl)
    day.colorSpace = THREE.SRGBColorSpace
    const night = loader.load(earthNightUrl)
    night.colorSpace = THREE.SRGBColorSpace
    return new THREE.MeshPhongMaterial({
      map: day,
      color: new THREE.Color('#28323d'), // cool dark blue-grey — moody, less green than the raw relief
      // City lights as a DIMMED emissive glow: brightest on the dark (night) side, washed
      // out by the sun on the day side. This is the "dark earth, less lights" look.
      emissiveMap: night,
      emissive: new THREE.Color('#ffffff'),
      emissiveIntensity: 0.35, // dimmed city lights — a faint glow, not a blaze
      shininess: 4,
    })
  }, [])

  // One-time three.js setup once the globe is ready: dark material, a subsolar
  // day/night light, a starfield, and bloom. Guarded so a GPU quirk degrades to a
  // plain lit globe rather than a blank panel.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready) return
    try {
      // Day/night: a warm directional light at the subsolar point + a low ambient so
      // the night side isn't pure black. Replaces globe.gl's camera-following light.
      const sun = new THREE.DirectionalLight('#fff2dc', 1.7)
      const ss = subsolarPoint(Date.now())
      const p = g.getCoords(ss.lat, ss.lon, 2)
      sun.position.set(p.x, p.y, p.z)
      // Enough ambient that the night side reads (dark land + coasts + the city lights),
      // but low enough that the lights aren't washed out — a moonlit night, not daylight.
      g.lights([new THREE.AmbientLight('#4a5566', 0.7), sun])
      // Starfield: a shell of points around the scene (no texture asset needed).
      const N = 1400
      const pos = new Float32Array(N * 3)
      for (let i = 0; i < N; i++) {
        const v = new THREE.Vector3().randomDirection().multiplyScalar(1400 + Math.random() * 600)
        pos.set([v.x, v.y, v.z], i * 3)
      }
      const geom = new THREE.BufferGeometry()
      geom.setAttribute('position', new THREE.BufferAttribute(pos, 3))
      const stars = new THREE.Points(
        geom,
        new THREE.PointsMaterial({ color: '#cdd9ec', size: 2.2, sizeAttenuation: false, transparent: true, opacity: 0.8 }),
      )
      g.scene().add(stars)
      // Bloom so spots/arcs/lights glow. Added ONCE — globe.gl resizes the composer (and this
      // pass) itself whenever the <Globe> width/height change, so this effect must NOT depend on
      // size. Re-running it on every resize stacked a second UnrealBloomPass onto the composer each
      // time (and a second starfield), and stacked bloom compounds the glow into a full brightness
      // blowout — the "globe goes massively bright after resizing the window, and only a 2D↔3D
      // toggle resets it" bug. Size the pass off the live container so the first frame is correct.
      const el = wrapRef.current
      const bloom = new UnrealBloomPass(
        new THREE.Vector2(el?.clientWidth || 1, el?.clientHeight || 1),
        0.6,
        0.7,
        0.2,
      )
      const composer = g.postProcessingComposer()
      composer.addPass(bloom)
      bloomRef.current = bloom
      // Gentle idle auto-rotate speed; the on/off state is driven by the spin effect.
      const controls = g.controls() as { autoRotateSpeed: number }
      controls.autoRotateSpeed = 0.3
      // Remove what we added so a remount / re-ready can never accumulate a second bloom or field.
      return () => {
        try {
          composer.passes = composer.passes.filter((pass) => pass !== bloom)
          bloom.dispose()
          bloomRef.current = null
          g.scene().remove(stars)
          stars.geometry.dispose()
          ;(stars.material as THREE.Material).dispose()
        } catch {
          /* best-effort teardown */
        }
      }
    } catch (e) {
      // eslint-disable-next-line no-console
      console.warn('[Globe3D] cinematic setup skipped:', e)
    }
  }, [ready])

  // Keep the ONE bloom pass matched to the canvas on resize (resize it, never re-add it — that
  // was the brightness-blowout bug). Idempotent, so it's harmless if globe.gl also resizes it.
  useEffect(() => {
    if (size.w > 0 && size.h > 0) bloomRef.current?.setSize(size.w, size.h)
  }, [size.w, size.h])

  // Frame the globe on the QTH once it's ready.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready || !qth) return
    g.pointOfView({ lat: qth.lat, lng: qth.lon, altitude: 2.2 }, 0)
  }, [ready, qth])

  // Drive idle auto-rotate from the operator's spin toggle.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready) return
    ;(g.controls() as { autoRotate: boolean }).autoRotate = spin
  }, [ready, spin])

  // City-lights on/off from the layers panel (dim the emissive to 0 when off).
  useEffect(() => {
    globeMat.emissiveIntensity = show.lights ? 0.35 : 0
    globeMat.needsUpdate = true
  }, [globeMat, show.lights])

  // Keep the day/night light following the sun (~1 min cadence, cheap).
  useEffect(() => {
    if (!ready) return
    const id = setInterval(() => {
      const g = globeRef.current
      if (!g) return
      const sun = g.lights().find((l) => l instanceof THREE.DirectionalLight) as THREE.DirectionalLight | undefined
      if (!sun) return
      const ss = subsolarPoint(Date.now())
      const p = g.getCoords(ss.lat, ss.lon, 2)
      sun.position.set(p.x, p.y, p.z)
    }, 60_000)
    return () => clearInterval(id)
  }, [ready])

  // Slow clock for the flare field + sun position (60 s, like the 2-D map's tick).
  useEffect(() => {
    const id = setInterval(() => setNowMs(Date.now()), 60_000)
    return () => clearInterval(id)
  }, [])

  // Gated 1 s pulse tick — the 2-D map's pattern: only while a layer that
  // BREATHES is visible and something is actually open, and never for a hidden
  // tab. This is what lets the heat/sector pulses above read live wall time
  // instead of freezing on the 60 s clock.
  const [pulseTick, setPulseTick] = useState(0)
  const hasOpening = (prop?.openings ?? []).length > 0
  const heatPulsing = show.heat && hasOpening
  const sectorsPulsing = show.openings && hasOpening
  useEffect(() => {
    if (!heatPulsing && !sectorsPulsing) return
    const id = setInterval(() => {
      if (!document.hidden) setPulseTick((t) => t + 1)
    }, 1_000)
    return () => clearInterval(id)
  }, [heatPulsing, sectorsPulsing])

  // Self-fetch aurora while its layer is on (server caches ~10 min).
  useEffect(() => {
    if (!show.aurora) {
      setAuroraPts([])
      return
    }
    let live = true
    const poll = () =>
      getAurora()
        .then((a) => live && setAuroraPts(a ?? []))
        .catch(() => {})
    poll()
    const id = setInterval(poll, 600_000)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [show.aurora])

  // Self-fetch PCA while its layer is on (~5 min).
  useEffect(() => {
    if (!show.pca) {
      setPca(null)
      return
    }
    let live = true
    const poll = () =>
      getPca()
        .then((p) => live && setPca(p))
        .catch(() => {})
    poll()
    const id = setInterval(poll, 300_000)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [show.pca])

  // Sync the space-weather point clouds when their data / toggles / readiness change.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready) return
    const store = cloudsRef.current
    // Solar flare D-RAP absorption on the sunlit hemisphere — only during an M/X flare.
    const xrayEff = xrayLong ?? 0
    const flareOn = show.flare && flareRScale(xrayEff) >= 1
    syncCloud(
      g,
      store,
      'flare',
      flareOn
        ? flareField(nowMs, xrayEff).map((s) => ({ lat: s.lat, lng: s.lon, rgb: flareRgb(s.haf), alt: 0.006 }))
        : [],
      5,
      flareOn,
    )
    // Aurora oval.
    syncCloud(
      g,
      store,
      'aurora',
      auroraPts
        .filter((a) => a.prob >= 8)
        .map((a) => ({ lat: a.lat, lng: a.lon, rgb: auroraRgb(a.prob), alt: 0.01 })),
      4,
      show.aurora,
    )
    // Ionosonde MUF (measured stations).
    syncCloud(
      g,
      store,
      'muf',
      (muf ?? [])
        .filter((m) => m.mufMhz != null)
        .map((m) => ({ lat: m.lat, lng: m.lon, rgb: mufRgb(m.mufMhz as number), alt: 0.008 })),
      6,
      show.muf,
    )
    // Proton polar-cap absorption.
    syncCloud(
      g,
      store,
      'pca',
      (pca?.points ?? []).map((p) => ({ lat: p.lat, lng: p.lon, rgb: [0.72, 0.34, 1] as RGB, alt: 0.009 })),
      5,
      show.pca,
    )
    // Band-heat openings (live spots as an additive glow).
    // Heat = the 2-D map's kernel-density aura, rebuilt for the GPU: one soft radial
    // blob per spot, additive so overlaps sum into the glow; brightness carries the
    // 2-D layer's age fade × open-band pulse (flat 7 px dots read as nothing — the
    // operator's "heat missing on 3D" report).
    {
      const openBands = new Set((prop?.openings ?? []).map((o) => o.band))
      // Colours are baked at the STEADY level; the breath rides the material
      // opacity in the 1 s animation effect below (see `pulseTick`). Rebuilding
      // this whole cloud once a second — hundreds of spots, new typed arrays —
      // is what the narrow effect avoids.
      const pulse = 1
      syncCloud(
        g,
        store,
        'heat',
        spots.map((s) => {
          const ageMin = s.ageSecs / 60
          const fade = ageMin < 10 ? 1 : ageMin < 30 ? 0.55 : 0.25
          const boost = openBands.has(s.band) ? pulse : 0.55
          const k = 0.45 * fade * boost
          const rgb = hexRgb(s.heardMe ? GETTING_OUT : bandColor(s.band))
          return {
            lat: s.lat,
            lng: s.lon,
            rgb: [rgb[0] * k, rgb[1] * k, rgb[2] * k] as RGB,
            alt: 0.0025,
          }
        }),
        30,
        show.heat,
        glowSprite(),
      )
    }
  }, [ready, nowMs, xrayLong, show.flare, show.aurora, show.muf, show.pca, show.heat, auroraPts, muf, pca, spots])

  // THE BREATH — the only per-second work: two material writes, no geometry,
  // no rebuild. The heat cloud dims/brightens and the opening wedges pulse,
  // exactly like the 2-D map's, while the heavy overlay effects above stay on
  // their own (60 s / data-driven) cadences.
  useEffect(() => {
    if (!ready) return
    const now = Date.now()
    const heat = cloudsRef.current['heat']
    if (heat) {
      ;(heat.material as THREE.PointsMaterial).opacity = 0.9 * heatPulse(now)
    }
    const sector = sectorPulse(now)
    for (const [key, grp] of Object.entries(linesRef.current)) {
      if (!key.startsWith('openings-')) continue
      grp.traverse((o) => {
        const m = (o as THREE.Line).material as THREE.LineBasicMaterial | undefined
        if (m && 'opacity' in m) m.opacity = sector
      })
    }
  }, [ready, pulseTick])

  // ── Pass mode ───────────────────────────────────────────────────────────────
  // Self-POLLED rather than taken as a prop: this component's parents are owned
  // elsewhere, and self-fetching is already the pattern here (aurora, PCA,
  // satellites). 2 s matches the Satellites section and the rotor strip; the
  // backend's track loop ticks every 3 s, so nothing is missed — and nothing is
  // invented between ticks, which is why the bird's position is never
  // interpolated: it moves when the tracker says it moved.
  const [pass, setPass] = useState<SatTrackStatus | null>(null)
  const passRef = useRef<SatTrackStatus | null>(null)
  useEffect(() => {
    if (!show.pass) {
      passRef.current = null
      setPass(null)
      return
    }
    let live = true
    const poll = () =>
      getSatTrackStatus()
        .then((t) => {
          if (!live) return
          passRef.current = t
          setPass(t)
        })
        .catch(() => {})
    poll()
    const id = setInterval(poll, 2_000)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [show.pass])

  // The pass the globe DRAWS: the scene lights up at AOS and goes dark at LOS.
  // Before then there is no look angle and no measured range — the bird is
  // below the horizon, and the backend says so by reporting them ABSENT rather
  // than zero, so there is nothing here to guess at. ("armed" is also
  // deliberately hands-off: the operator still owns the rotor and the bird can
  // be hours out.) Narrowed once here rather than re-checked at each use.
  const livePass = useMemo(() => {
    if (!pass) return null
    const { satAzDeg, satElDeg, rangeKm } = pass
    if (satAzDeg == null || satElDeg == null || rangeKm == null) return null
    return { ...pass, satAzDeg, satElDeg, rangeKm }
  }, [pass])
  // Both stable ACROSS polls (a pass never changes bird or AOS mid-flight), so
  // the effects keyed on them don't rebuild every 2 s.
  const passBirdKey = livePass ? livePass.name.trim().toUpperCase() : null
  const passKey = livePass ? `${livePass.name}|${livePass.aosUnix}` : null

  // Self-fetch satellites while the layer is on — or while a pass is tracked,
  // which needs the tracked bird's ground track for its orbit arc (~30 s, like
  // the 2-D map).
  const needSats = show.sats || passBirdKey != null
  useEffect(() => {
    if (!needSats) {
      setSats(null)
      return
    }
    let live = true
    const poll = () =>
      getSatellites()
        .then((s) => live && setSats(s))
        .catch(() => {})
    poll()
    const id = setInterval(poll, 30_000)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [needSats])

  // Build the satellite scene: a REAL 3-D orbit per bird (the ground track lifted to its
  // orbital altitude), a footprint ring on the surface, and a live marker. This is the
  // 3-D-native payoff — 2-D could only show a flat ground track.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready || !show.sats || !sats) return
    // ★-only filter — the Passes-pane chip's choice, ONE surface-scoped key
    // shared with the 2-D map (zero stars = filter inert, all birds show).
    // satFav/satChaseRev in the deps make a chip flip or star toggle rebuild
    // NOW, not on the next 30 s poll.
    const chaseKeys = satChaseKeys()
    const shownBirds = satFav ? filterSatsToChased(sats.birds, chaseKeys) : sats.birds
    const group = new THREE.Group()
    const markers: Record<string, THREE.Object3D> = {}
    const labels: Record<string, THREE.Sprite> = {}
    for (const bird of shownBirds) {
      // The tracked bird belongs to the pass scene below, which draws its own
      // arc, footprint and marker. Drawing it here too would put two identical
      // lines at exactly the same radius — the coplanar-surface flicker this
      // file has already been bitten by twice (see SECTOR_ALT_STEP).
      if (passBirdKey && bird.name.trim().toUpperCase() === passBirdKey) continue
      const alt = bird.altKm / EARTH_KM
      // Orbit line: the ground track lifted to orbital altitude.
      const pts = bird.track.map(([, la, lo]) => {
        const c = g.getCoords(la, lo, alt)
        return new THREE.Vector3(c.x, c.y, c.z)
      })
      if (pts.length > 1) {
        group.add(
          new THREE.Line(
            new THREE.BufferGeometry().setFromPoints(pts),
            new THREE.LineBasicMaterial({ color: '#7ad0ff', transparent: true, opacity: 0.55 }),
          ),
        )
      }
      // Footprint ring (radio horizon) on the surface.
      const fp: THREE.Vector3[] = []
      for (let b = 0; b <= 360; b += 15) {
        const d = destinationPoint({ lat: bird.lat, lon: bird.lon }, b, bird.footprintKm)
        const c = g.getCoords(d.lat, d.lon, 0.002)
        fp.push(new THREE.Vector3(c.x, c.y, c.z))
      }
      group.add(
        new THREE.Line(
          new THREE.BufferGeometry().setFromPoints(fp),
          new THREE.LineBasicMaterial({ color: '#7ad0ff', transparent: true, opacity: 0.28 }),
        ),
      )
      // Live marker at the sat's current position + altitude.
      const c = g.getCoords(bird.lat, bird.lon, alt)
      const marker = new THREE.Mesh(
        new THREE.SphereGeometry(1.6, 10, 10),
        new THREE.MeshBasicMaterial({ color: '#eaffff' }),
      )
      marker.position.set(c.x, c.y, c.z)
      markers[bird.name] = marker
      group.add(marker)
      // Designation label riding just above the marker — the pass label's
      // sprite idiom (textSprite + alt offset); teal = chased. (The 2-D map's
      // labels are uniform --text ink — only the MARKERS share the teal/slate
      // chased coding across surfaces.)
      const label = textSprite(
        bird.name,
        isSatChased(bird.name, bird.norad, chaseKeys) ? '#5eead4' : 'rgba(203, 213, 225, 0.95)',
      )
      const lc = g.getCoords(bird.lat, bird.lon, alt + 0.03)
      label.position.set(lc.x, lc.y, lc.z)
      labels[bird.name] = label
      group.add(label)
    }
    g.scene().add(group)
    satGroupRef.current = group
    satMarkersRef.current = markers
    satLabelsRef.current = labels
    // Cleanup-return, the pass-scene idiom above: dispose-at-next-run leaked
    // the LAST group on unmount (every 3-D→2-D toggle stranded N 256×48 canvas
    // textures), and its sprite-only material branch leaked the two line
    // materials + marker material per bird on every 30 s rebuild.
    return () => {
      g.scene().remove(group)
      group.traverse((o) => {
        const obj = o as unknown as {
          isSprite?: boolean
          geometry?: THREE.BufferGeometry
          material?: THREE.Material & { map?: THREE.Texture | null }
        }
        // Sprites SHARE one module-level geometry inside three.js — disposing it
        // would break every other sprite on the globe (the opening labels). Only
        // the material and its canvas texture are ours to free.
        if (!obj.isSprite) obj.geometry?.dispose?.()
        obj.material?.map?.dispose?.()
        obj.material?.dispose?.()
      })
      satGroupRef.current = null
      satMarkersRef.current = {}
      satLabelsRef.current = {}
    }
  }, [ready, sats, show.sats, passBirdKey, satFav, satChaseRev])

  // Animate the sat markers along their tracks each second (real-time motion between polls).
  useEffect(() => {
    if (!show.sats || !sats) return
    const id = setInterval(() => {
      const g = globeRef.current
      if (!g) return
      const now = Date.now() / 1000
      for (const bird of sats.birds) {
        // Filtered-out birds (★-only view) simply have no marker to move.
        const marker = satMarkersRef.current[bird.name]
        if (!marker) continue
        const pos = satPosAt(bird.track, now)
        if (!pos) continue
        const c = g.getCoords(pos.lat, pos.lon, bird.altKm / EARTH_KM)
        marker.position.set(c.x, c.y, c.z)
        const label = satLabelsRef.current[bird.name]
        if (label) {
          const lc = g.getCoords(pos.lat, pos.lon, bird.altKm / EARTH_KM + 0.03)
          label.position.set(lc.x, lc.y, lc.z)
        }
      }
    }, 1000)
    return () => clearInterval(id)
  }, [show.sats, sats])

  // THE PASS SCENE: orbit arc ahead/behind, footprint, sight line, bird.
  // Rebuilt on each status poll — that rebuild IS the motion, and it is the only
  // motion: nothing here breathes on a clock of its own (the shared pulse in
  // features/pulse.ts exists precisely because a decorative pulse on the wrong
  // clock froze once already). Everything this adds it also disposes —
  // geometry AND material, including the label's canvas texture.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready || !qth || !livePass) return
    const vec = (la: number, lo: number, alt: number) => {
      const c = g.getCoords(la, lo, alt)
      return new THREE.Vector3(c.x, c.y, c.z)
    }
    const b = birdFromLook(qth, livePass.satAzDeg, livePass.satElDeg, livePass.rangeKm)
    const satAlt = b.altKm / EARTH_KM
    const satVec = vec(b.lat, b.lon, satAlt)
    const qthVec = vec(qth.lat, qth.lon, 0.002)
    const group = new THREE.Group()

    // 1. THE ORBIT ARC, behind and ahead — the same per-minute ground track the
    //    satellite layer lifts to orbital altitude, split at now: solid behind,
    //    dashed ahead, so the direction of travel reads. (The 2-D map draws the
    //    trail solid and the projection dashed; the two surfaces must agree.)
    const bird = sats?.birds.find((x) => x.name.trim().toUpperCase() === passBirdKey)
    if (bird) {
      const trackAlt = bird.altKm / EARTH_KM
      const nowSec = Date.now() / 1000
      const behind = bird.track
        .filter(([t]) => t <= nowSec)
        .map(([, la, lo]) => vec(la, lo, trackAlt))
      behind.push(satVec) // the trail ends AT the bird, not at the last sample
      const ahead = [
        satVec,
        ...bird.track.filter(([t]) => t > nowSec).map(([, la, lo]) => vec(la, lo, trackAlt)),
      ]
      if (behind.length > 1) {
        group.add(
          new THREE.Line(
            new THREE.BufferGeometry().setFromPoints(behind),
            new THREE.LineBasicMaterial({ color: PASS_TRACK, transparent: true, opacity: 0.35 }),
          ),
        )
      }
      if (ahead.length > 1) {
        const line = new THREE.Line(
          new THREE.BufferGeometry().setFromPoints(ahead),
          new THREE.LineDashedMaterial({
            color: PASS_TRACK,
            transparent: true,
            opacity: 0.9,
            dashSize: 1.8,
            gapSize: 1.2,
          }),
        )
        line.computeLineDistances() // dashes are measured in world units, not vertices
        group.add(line)
      }
    }

    // 2. THE FOOTPRINT — the bird's radio horizon on the surface, the same ring
    //    the satellite layer draws, at its live sub-point. Its own radius (0.003)
    //    because the range rings sit at 0.002 and two rings at one radius is the
    //    coplanar flicker again.
    const fpKm = bird?.footprintKm ?? EARTH_KM * Math.acos(EARTH_KM / (EARTH_KM + b.altKm))
    const fp: THREE.Vector3[] = []
    for (let brg = 0; brg <= 360; brg += 6) {
      const d = destinationPoint({ lat: b.lat, lon: b.lon }, brg, fpKm)
      fp.push(vec(d.lat, d.lon, 0.003))
    }
    group.add(
      new THREE.Line(
        new THREE.BufferGeometry().setFromPoints(fp),
        new THREE.LineBasicMaterial({ color: PASS_TRACK, transparent: true, opacity: 0.45 }),
      ),
    )

    // 3. THE SIGHT LINE — QTH → bird, straight through the sky. This is the
    //    range number drawn: it stretches as the bird sets and shortens toward
    //    TCA. A 1 px line disappears over a busy sphere, so a slim additive beam
    //    (narrow at the antenna, wider at the bird) carries it; both are warm and
    //    solid against the orbit's cool dashed teal, and it is the only line on
    //    the globe that leaves the surface.
    group.add(
      new THREE.Line(
        new THREE.BufferGeometry().setFromPoints([qthVec, satVec]),
        new THREE.LineBasicMaterial({ color: PASS_LOS, transparent: true, opacity: 0.95 }),
      ),
    )
    const dir = new THREE.Vector3().subVectors(satVec, qthVec)
    const beam = new THREE.Mesh(
      // Cylinders are built along +Y: radiusTop is the BIRD end, radiusBottom the
      // antenna end, and the quaternion below swings that axis onto the ray.
      new THREE.CylinderGeometry(0.9, 0.22, dir.length(), 6, 1, true),
      new THREE.MeshBasicMaterial({
        color: PASS_LOS,
        transparent: true,
        opacity: 0.16,
        depthWrite: false,
        blending: THREE.AdditiveBlending,
        side: THREE.DoubleSide,
      }),
    )
    beam.position.copy(qthVec).addScaledVector(dir, 0.5)
    beam.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), dir.clone().normalize())
    group.add(beam)

    // 4. THE BIRD, at satAzDeg/satElDeg — where it IS, never where the antenna
    //    was commanded.
    const marker = new THREE.Mesh(
      new THREE.SphereGeometry(2, 12, 12),
      new THREE.MeshBasicMaterial({ color: '#eaffff' }),
    )
    marker.position.copy(satVec)
    group.add(marker)
    const label = textSprite(livePass.name, PASS_TRACK)
    label.position.copy(vec(b.lat, b.lon, satAlt + 0.03))
    group.add(label)

    g.scene().add(group)
    return () => {
      g.scene().remove(group)
      group.traverse((o) => {
        const obj = o as unknown as {
          isSprite?: boolean
          geometry?: THREE.BufferGeometry
          material?: THREE.Material & { map?: THREE.Texture | null }
        }
        // Sprites SHARE one module-level geometry inside three.js — disposing it
        // would break every other sprite on the globe (the opening labels). Only
        // the material and its canvas texture are ours to free.
        if (!obj.isSprite) obj.geometry?.dispose?.()
        obj.material?.map?.dispose?.()
        obj.material?.dispose?.()
      })
    }
  }, [ready, qth, livePass, passBirdKey, sats])

  // Frame the QTH and the bird together when a pass STARTS (or the tracked bird
  // changes): aim the camera at the great-circle midpoint, once. Keyed on the
  // PASS, not on the position — re-aiming every poll would fight the operator,
  // who is free to spin and zoom for the rest of the pass.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready || !qth || !passKey) return
    const p = passRef.current
    if (!p) return
    const { satAzDeg, satElDeg, rangeKm } = p
    if (satAzDeg == null || satElDeg == null || rangeKm == null) return
    const b = birdFromLook(qth, satAzDeg, satElDeg, rangeKm)
    // Half the ground arc along the same bearing IS the great-circle midpoint.
    const mid = destinationPoint(qth, satAzDeg, b.groundKm / 2)
    g.pointOfView({ lat: mid.lat, lng: mid.lon, altitude: PASS_ALT }, 900)
  }, [ready, qth, passKey])

  // CQ-zone boundaries (self-fetch the bundled GeoJSON while the layer is on).
  useEffect(() => {
    if (!show.cqzones) {
      setCqzones([])
      return
    }
    let live = true
    fetch(cqzonesUrl)
      .then((r) => r.json())
      .then((gj: { features?: { geometry: { type: string; coordinates: unknown } }[] }) => {
        if (!live) return
        const lines: [number, number][][] = []
        for (const f of gj.features ?? []) {
          const geom = f.geometry
          const polys =
            geom.type === 'MultiPolygon'
              ? (geom.coordinates as [number, number][][][])
              : geom.type === 'Polygon'
                ? [geom.coordinates as [number, number][][]]
                : []
          for (const poly of polys) for (const ring of poly) lines.push(ring.map(([lo, la]) => [la, lo]))
        }
        setCqzones(lines)
      })
      .catch(() => {})
    return () => {
      live = false
    }
  }, [show.cqzones])

  // My coverage: worked 4-char grids from the log (self-fetch while the layer is on).
  useEffect(() => {
    if (!show.coverage) {
      setWorkedGrids([])
      return
    }
    let live = true
    getLog()
      .then((log) => {
        if (!live) return
        const grids = workedGridSet(log)
        const pts: { lat: number; lon: number }[] = []
        grids.forEach((gr) => {
          const ll = gridToLatLon(gr)
          if (ll) pts.push({ lat: ll.lat, lon: ll.lon })
        })
        setWorkedGrids(pts)
      })
      .catch(() => {})
    return () => {
      live = false
    }
  }, [show.coverage])

  // Sync the cartographic line/point overlays (range rings, CQ zones, coverage).
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready) return
    const ringLines: [number, number][][] = []
    if (show.rings && qth) {
      for (const km of [1000, 3000, 5000, 10000]) {
        const gc = rangeRing(qth, km) as unknown as { coordinates?: [number, number][][] }
        const ring = gc.coordinates?.[0]
        if (ring) ringLines.push(ring.map(([lo, la]) => [la, lo]))
      }
    }
    syncLines(g, linesRef.current, 'rings', ringLines, '#4ea1ff', 0.4, show.rings)
    syncLines(g, linesRef.current, 'cqzones', cqzones, '#e0a94d', 0.5, show.cqzones)
    // Greyline: the day/night terminator = a circle 90° (a quarter of the globe) from the
    // subsolar point, in the 2-D map's warm gold. Follows the sun via the nowMs tick.
    const greylineLines: [number, number][][] = []
    if (show.greyline) {
      const ss = subsolarPoint(nowMs)
      const QUARTER_KM = (EARTH_KM * Math.PI) / 2
      const circle: [number, number][] = []
      for (let b = 0; b <= 360; b += 4) {
        const d = destinationPoint(ss, b, QUARTER_KM)
        circle.push([d.lat, d.lon])
      }
      greylineLines.push(circle)
    }
    syncLines(g, linesRef.current, 'greyline', greylineLines, '#ffc86e', 0.75, show.greyline, 0.004)
    // Opening sectors — mode-colored wedge outlines from the QTH toward each live
    // opening's bearing (±22.5°) out to its longest path, matching the 2-D map's
    // sector layer (tropo amber / Es green / aurora violet / F2 cyan). One
    // syncLines key per mode so each keeps its own color; empty modes clear.
    const openingsByMode = new Map<string, [number, number][][]>()
    if (show.openings && qth) {
      for (const o of prop?.openings ?? []) {
        if (!(o.maxKm > 0)) continue
        // Same subdivided ring as the fill (see `sectorRing`) — as [lat, lon] for syncLines.
        // These two used to build the wedge from two copies of the geometry, so the outline
        // tore through the globe exactly like the fill did.
        const outline: [number, number][] = sectorRing(qth, o.bearingDeg, o.maxKm).map(
          ([lon, lat]) => [lat, lon] as [number, number],
        )
        const arr = openingsByMode.get(o.mode) ?? []
        arr.push(outline)
        openingsByMode.set(o.mode, arr)
      }
    }
    for (const mode of ['Tropo', 'Sporadic-E', 'Aurora', 'F2', 'Unknown']) {
      const lines = openingsByMode.get(mode) ?? []
      syncLines(
        g,
        linesRef.current,
        `openings-${mode}`,
        lines,
        openingModeColor(mode),
        // Steady level at build time; the 1 s animation effect above breathes
        // it (the 2-D map's wedges pulse, and the parity contract this file
        // states for labels applies to the pulse too).
        0.9,
        show.openings && lines.length > 0,
        // Above EVERY fill (see SECTOR_OUTLINE_ALT): the outline used to sit at exactly the
        // fills' 0.006, so each wedge's border was coplanar with its own cap.
        SECTOR_OUTLINE_ALT,
      )
    }
    // Sector LABELS — the 2-D map tags each wedge "6m Sporadic-E"; without them the
    // globe's outlines were unreadable (operator report). One text sprite at each
    // sector's far edge; rebuilt (and disposed) with the openings.
    {
      const scene = g.scene()
      for (const sp of openingLabelsRef.current) {
        scene.remove(sp)
        sp.material.map?.dispose()
        sp.material.dispose()
      }
      openingLabelsRef.current = []
      if (show.openings && qth) {
        for (const o of prop?.openings ?? []) {
          if (!(o.maxKm > 0)) continue
          const tip = destinationPoint(qth, o.bearingDeg, o.maxKm)
          const pos = g.getCoords(tip.lat, tip.lon, 0.03)
          const sp = textSprite(`${o.band} ${o.mode}`, openingModeColor(o.mode))
          sp.position.set(pos.x, pos.y, pos.z)
          scene.add(sp)
          openingLabelsRef.current.push(sp)
        }
      }
    }
    syncCloud(
      g,
      cloudsRef.current,
      'coverage',
      workedGrids.map((w) => ({ lat: w.lat, lng: w.lon, rgb: [0.3, 0.64, 1] as RGB, alt: 0.001 })),
      4,
      show.coverage,
    )
  }, [ready, nowMs, qth, show.rings, show.cqzones, show.coverage, show.greyline, show.openings, cqzones, workedGrids, prop])

  // My decodes + DXpeditions as distinct point clouds.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready) return
    syncCloud(
      g,
      cloudsRef.current,
      'decodes',
      show.decodes
        ? (stations ?? []).flatMap((s) => {
            const ll = s.grid ? gridToLatLon(s.grid) : null
            return ll ? [{ lat: ll.lat, lng: ll.lon, rgb: [0.87, 0.91, 0.96] as RGB, alt: 0.004 }] : []
          })
        : [],
      6,
      show.decodes,
    )
    const cards = prop?.dxpeditions?.workableNow ?? []
    syncCloud(
      g,
      cloudsRef.current,
      'dxped',
      show.dxped && qth
        ? cards.map((c) => {
            const d = destinationPoint(qth, c.bearingDeg, c.distanceKm)
            return { lat: d.lat, lng: d.lon, rgb: [1, 0.62, 0.24] as RGB, alt: 0.006 }
          })
        : [],
      8,
      show.dxped,
    )
  }, [ready, qth, show.decodes, show.dxped, stations, prop])

  // Pointer event → wrap LAYOUT coords (the .map-hover tooltip is positioned in the
  // same layout space the globe is sized in). The .app UI zoom makes visual px ≠ layout
  // px, so undo it via the rect ratio — same fix as the 2-D map's canvasXY. Reads the
  // live client size off the ref so a window resize never strands a stale scale.
  const wrapXY = (e: MouseEvent): [number, number] => {
    const el = wrapRef.current
    if (!el) return [e.clientX, e.clientY]
    const rect = el.getBoundingClientRect()
    const sx = rect.width > 0 ? el.clientWidth / rect.width : 1
    const sy = rect.height > 0 ? el.clientHeight / rect.height : 1
    return [(e.clientX - rect.left) * sx, (e.clientY - rect.top) * sy]
  }

  // Stars exist but NONE matched (starred bird aged past the 30-day element
  // cutoff or left the group file) → the ★ filter hides every bird. Say so —
  // the 2-D map renders the same hint; a silently blank sky reads as broken.
  const satAllHidden = useMemo(() => {
    if (!show.sats || !satFav || !sats || sats.birds.length === 0) return 0
    const keys = satChaseKeys()
    if (keys.names.size === 0) return 0 // zero stars = filter inert, sky full
    return filterSatsToChased(sats.birds, keys).length === 0 ? sats.birds.length : 0
    // satChaseRev: star toggles land in storage, not props — the rev is the rerender.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [show.sats, satFav, sats, satChaseRev])

  if (!ok) {
    return <div className="globe3d-fallback">{t('globe.unsupported')}</div>
  }

  return (
    <div ref={wrapRef} className="globe3d-wrap">
      <button
        type="button"
        className={`globe3d-spin${spin ? ' active' : ''}`}
        onClick={() => setSpin((s) => !s)}
        title={spin ? t('globe.spin.stop.title') : t('globe.spin.start.title')}
      >
        {spin ? t('globe.spin.pause') : t('globe.spin.play')}
      </button>
      {/* Layers panel, matching the 2-D map. Grows as Phase B adds layers. (Was gated on the
          Expert detail level, removed 2026-07-26 — the layer list is now always available.) */}
      {(
        <div className="globe3d-layers">
          <span className="globe3d-layers-h">{t('globe.layers.head')}</span>
          {LAYER_ROWS.map((row) => (
            <Fragment key={row.k}>
              <label>
                <input
                  type="checkbox"
                  checked={show[row.k]}
                  onChange={(e) => setShow((s) => ({ ...s, [row.k]: e.target.checked }))}
                />
                {'labelKey' in row ? t(row.labelKey) : row.label}
              </label>
              {row.k === 'sats' && show.sats && (
                // The ★/All chip on the 3-D surface too — same reachability
                // argument as the 2-D Layers panel (the Passes pane that also
                // carries it may not be placed in the layout at all).
                <button
                  type="button"
                  className={`sat-fav-toggle${satFav ? ' on' : ''}`}
                  aria-label={t('globe.sats.filter.aria')}
                  aria-pressed={satFav}
                  title={satFav ? t('globe.sats.filter.on.title') : t('globe.sats.filter.off.title')}
                  onClick={() => setSatFavOnly(!satFav)}
                >
                  {satFav ? '★' : t('globe.sats.filter.all')}
                </button>
              )}
            </Fragment>
          ))}
        </div>
      )}
      {satAllHidden > 0 && (
        // The 2-D map renders this same hint word for word — one key, deliberately.
        <div className="map-empty-hint sats">{t('map.sats.allHidden', { count: satAllHidden })}</div>
      )}
      {/* The same on-map insight rail (openings / band advisor / MUF) the 2-D map shows —
          overlaid on the right, so the 3-D globe has the same operating windows. */}
      {prop && (
        <MapInsightRail
          prop={prop}
          outlook={outlook}
          onBandClick={onBandClick}
          activeBand={activeBand}
        />
      )}
      {/* The pass in WORDS. The scene above is WebGL, which a screen reader
          cannot see (this app has a standing a11y commitment), and it also
          answers "what am I looking at" for everyone else. The bird's own az/el
          — never the commanded antenna pair — and no range at all rather than a
          zero when there is none. Re-rendered by the 2 s status poll itself, so
          the countdown needs no clock of its own.

          Deliberately NOT a live region: every number here changes on the 2 s
          poll, so announcing them would talk over the operator continuously.
          The text is in the DOM to be read on demand — which is what makes the
          WebGL scene accessible — not to interrupt. (The sky dome's own text
          equivalent follows the same rule.) */}
      {livePass && (
        <div className="globe3d-pass" role="group" aria-label={t('globe.pass.aria', { name: livePass.name })}>
          <b>{livePass.name}</b>
          <span>
            {t('globe.pass.elAz', {
              el: Math.round(livePass.satElDeg),
              az: Math.round(livePass.satAzDeg),
            })}
          </span>
          <span>{t('globe.pass.range', { km: Math.round(livePass.rangeKm).toLocaleString() })}</span>
          <span>{t('globe.pass.losIn', { mmss: mmss(livePass.losUnix - Date.now() / 1000) })}</span>
        </div>
      )}
      {/* The same legends the 2-D map shows (shared component) — the globe was
          rendering the data with no key to read it by (2D↔3D parity). */}
      <MapLegend />
      {show.muf && <MufLegend />}
      {size.w > 0 && size.h > 0 && (
        <Globe
          ref={globeRef}
          width={size.w}
          height={size.h}
          onGlobeReady={() => setReady(true)}
          backgroundColor="rgba(0,0,0,0)"
          globeMaterial={globeMat}
          showAtmosphere
          atmosphereColor="#68a8e2"
          atmosphereAltitude={0.18}
          showGraticules={show.grid}
          htmlElementsData={show.spots ? points : []}
          htmlLat="lat"
          htmlLng="lng"
          htmlAltitude={0.01}
          htmlElement={(d: object) => {
            const p = d as { call: string; color: string; label: string }
            const el = document.createElement('div')
            el.className = 'globe3d-spot'
            el.style.setProperty('--c', p.color)
            el.onclick = () => onSelectCall(p.call)
            // Rich hover tooltip matching the 2-D map (call · band · mode · freq · age …),
            // rendered as the shared .map-hover element near the cursor.
            const showHover = (e: MouseEvent) => {
              const [x, y] = wrapXY(e)
              setHover({ x, y, text: p.label })
            }
            el.onmouseenter = showHover
            el.onmousemove = showHover
            el.onmouseleave = () => setHover(null)
            return el
          }}
          arcsData={show.arcs ? arcs : []}
          arcColor="color"
          arcStroke={0.5}
          arcDashLength={0.5}
          arcDashGap={0.25}
          arcDashAnimateTime={2200}
          arcAltitudeAutoScale={0.4}
          polygonsData={show.openings ? sectorPolys : []}
          polygonGeoJsonGeometry={(d: object) =>
            // react-globe.gl declares coordinates as number[] — wrong for polygons
            // (runtime accepts the standard nested GeoJSON rings) — so cast to its shape.
            (d as { geometry: unknown }).geometry as { type: string; coordinates: number[] }
          }
          polygonCapColor={(d: object) => (d as { fill: string }).fill}
          polygonSideColor={() => 'rgba(0,0,0,0)'}
          polygonStrokeColor={() => 'rgba(0,0,0,0)'}
          polygonAltitude={(d: object) => (d as { alt: number }).alt}
          polygonsTransitionDuration={0}
          pathsData={show.states ? statePaths : []}
          pathPointLat={(p: unknown) => (p as [number, number])[0]}
          pathPointLng={(p: unknown) => (p as [number, number])[1]}
          pathColor={() => 'rgba(126,158,180,0.8)'}
          pathStroke={1.1}
          ringsData={rings}
          ringColor={() => '#4ea1ff'}
          ringMaxRadius={1.6}
          ringPropagationSpeed={0.7}
          ringRepeatPeriod={2600}
        />
      )}
      {hover && (
        <div className="map-hover" style={{ left: hover.x + 12, top: hover.y + 12 }}>
          {hover.text}
        </div>
      )}
    </div>
  )
}
