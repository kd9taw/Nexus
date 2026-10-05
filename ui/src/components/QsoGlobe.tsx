// Lean 3-D "world of contacts" globe for the top of the Logbook: every logged QSO's
// grid square as a small flat band-colored dot hugging the earth — the SAME visual
// language as the 2-D map's live spots (small round dots in the app's band palette),
// on the same textured day/night earth as the Connect globe. Gentle 0.3°/frame
// auto-spin with a ▶/⏸ toggle. DELIBERATELY not Globe3D: no propagation layers, no
// insight rail, no background pollers — the logbook band shows *your* contacts and
// nothing else, and can never destabilize the Connect globe.
//
// The dots are ONE THREE.Points cloud (the same technique as the Connect globe's
// coverage/space-weather layers): per-vertex band colors, a round dark-edged sprite so
// they read as the 2-D map's dots (react-globe.gl's default points layer extrudes
// CYLINDERS — 1,500 squares looked like rivets on a ball; operator veto 2026-07-21).
//
// Resource story (the operator's hard requirement): the Logbook view is rendered
// inside App's view switch, so this component UNMOUNTS when you leave the Logbook, and
// hands its WebGL context back as it goes (globeWebgl.tsx: unmounting alone never did,
// so every closed globe kept one). A context lost while the band is shown comes back, or
// the band offers a Reload. While mounted, an IntersectionObserver pauses the globe's
// whole render loop (`pauseAnimation`) once the band scrolls out of view inside the
// log's scroll container, so reading old QSOs at the bottom of a long log costs nothing
// either.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Band names are technical
// tokens and stay here, in the <option> values AND in their labels; the prose is in the
// catalog under `logbook.globe.*`.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import * as THREE from 'three'
import Globe, { type GlobeMethods } from 'react-globe.gl'
import earthNightUrl from '../assets/earth-night.webp'
import { coarsestBasemap, loadBasemap } from '../basemap'
import {
  GLOBE_AMBIENT,
  GLOBE_SUN,
  disposeGlobeLines,
  globeLines,
  nightOnlyEmissive,
  loadRelief,
  paintGlobeTexture,
  readMapInks,
} from '../features/globeBasemap'
import { usePaletteKey } from '../usePaletteRoles'
import { gridCountsToPoints } from '../features/qsoPoints'
import { emptyAnswer } from '../features/logAnswers'
import { useLogAnswer } from '../features/logSource'
import { BAND_COLOR, bandColor } from '../bandColors'
import { subsolarPoint } from '../mapGeo'
import { surfaceGet, surfaceSet } from '../features/windowScope'
import { GlobePaused, useGlobeWebgl } from './globeWebgl'
import { MARKER_HALO } from './MapView'
import { t } from '../i18n'

/** Low→high band order = BAND_COLOR's key order (the app's canonical band list). */
const BAND_ORDER = Object.keys(BAND_COLOR)

/** Spin preference; '0' = off. Default ON — the slow rotation is the point of the band.
 *  PER-SURFACE: it is per-window animation (and per-window CPU) — stopping it on the board
 *  you are reading must not stop the showpiece globe on the other screen. */
const SPIN_KEY = 'nexus.logbook.globespin'

/** A dot as the 2-D map draws a live spot (MapView): a 2.8 px radius disc in the band's colour,
 *  edged by a 1 px MARKER_HALO stroke on the same circle, so the outside edge is 0.5 px beyond. */
const DOT_R = 2.8
const DOT_EDGE = 1
/** The dot's whole size on screen, its edge included. */
const DOT_PX = 2 * (DOT_R + DOT_EDGE / 2)

/** That dot as the GPU points' sprite (in place of PointsMaterial's default squares): a white disc,
 *  which the vertex colour paints in the band's colour, under the dark edge, which stays dark. Built
 *  once. Not mipmapped: at 6.6 px a mip level blurs the 1 px edge into the map under it. */
function dotSprite(): THREE.CanvasTexture {
  const c = document.createElement('canvas')
  c.width = 64
  c.height = 64
  const ctx = c.getContext('2d')
  if (ctx) {
    const k = 32 / (DOT_PX / 2)
    ctx.beginPath()
    ctx.arc(32, 32, DOT_R * k, 0, Math.PI * 2)
    ctx.fillStyle = '#ffffff'
    ctx.fill()
    ctx.lineWidth = DOT_EDGE * k
    ctx.strokeStyle = MARKER_HALO
    ctx.stroke()
  }
  const tex = new THREE.CanvasTexture(c)
  tex.colorSpace = THREE.SRGBColorSpace
  tex.generateMipmaps = false
  tex.minFilter = THREE.LinearFilter
  return tex
}

/** The Logbook's globe. It asks the log for what it draws — the bands in it, and the worked
 *  squares of the band on show — following the Logbook's `logTick`, and never holds the log.
 *  Reload, on a globe whose WebGL context was lost and never came back, mounts it afresh. */
export default function QsoGlobe({ logTick }: { logTick?: number }) {
  const [mount, setMount] = useState(0)
  return <QsoGlobeView key={mount} logTick={logTick} onReload={() => setMount((n) => n + 1)} />
}

function QsoGlobeView({ logTick, onReload }: { logTick?: number; onReload: () => void }) {
  const wrapRef = useRef<HTMLDivElement>(null)
  const globeRef = useRef<GlobeMethods | undefined>(undefined)
  const cloudRef = useRef<THREE.Points | null>(null)
  const [size, setSize] = useState({ w: 0, h: 0 })
  const [ready, setReady] = useState(false)
  const [spin, setSpin] = useState(() => surfaceGet(SPIN_KEY) !== '0')
  // Band filter — grids are a PER-BAND achievement (VUCC): a 2m square is its own
  // trophy and must never be pooled with the HF squares (operator, 2026-07-21).
  // 'all' = every band together (the overview); a specific band shows only ITS
  // squares and ITS count.
  const [band, setBand] = useState<string>('all')

  // Bands present in the log, in the app's canonical low→high order.
  const bandsQuestion = { kind: 'bandsInLog' } as const
  const bands = useLogAnswer(bandsQuestion, logTick) ?? emptyAnswer(bandsQuestion)
  const bandsInLog = useMemo(() => {
    return [...bands].sort((a, b) => {
      const ia = BAND_ORDER.indexOf(a)
      const ib = BAND_ORDER.indexOf(b)
      if (ia !== -1 && ib !== -1) return ia - ib
      if (ia !== -1) return -1
      if (ib !== -1) return 1
      return a.localeCompare(b)
    })
  }, [bands])

  // Measure the band BEFORE paint — react-globe.gl sizes to the whole window when
  // width/height are undefined (the same trap Globe3D guards against).
  useLayoutEffect(() => {
    const el = wrapRef.current
    if (!el) return
    const measure = () => setSize({ w: el.clientWidth, h: el.clientHeight })
    measure()
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  // QSOs → unique 4-char grid squares → dots (the shared reduction the 2-D map uses too, so the
  // two views plot identical points). The dedupe is what keeps a 50k-QSO FT8 log at ~a thousand
  // points instead of 50k — and the squares are counted where the log is; only the placing is here.
  const squaresQuestion = { kind: 'gridPoints', band } as const
  const squares = useLogAnswer(squaresQuestion, logTick) ?? emptyAnswer(squaresQuestion)
  const points = useMemo(() => gridCountsToPoints(squares), [squares])

  // Same base map as the Connect globe (features/globeBasemap.ts), so the two read as one app: the
  // 2-D map's own picture painted onto the sphere (again when the theme changes; the sea's colour
  // until then), city lights as a dim night-side glow, and the coast, borders and US state lines as
  // lines just above the surface — most logs are WAS-minded, so seeing which state a dot sits in is
  // the point. 1:50m: this band never zooms in close enough to want more.
  const globeMat = useMemo(() => {
    const loader = new THREE.TextureLoader()
    const night = loader.load(earthNightUrl)
    night.colorSpace = THREE.SRGBColorSpace
    const m = new THREE.MeshPhongMaterial({
      color: new THREE.Color(readMapInks().water),
      emissiveMap: night,
      emissive: new THREE.Color('#ffffff'),
      emissiveIntensity: 0.35,
      shininess: 4,
    })
    nightOnlyEmissive(m)
    return m
  }, [])
  const paletteKey = usePaletteKey()
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready) return
    let live = true
    let lines: THREE.Group | null = null
    void Promise.all([loadBasemap('50m'), loadRelief()]).then(([map, img]) => {
      if (!live) return
      const base = map ?? coarsestBasemap()
      const inks = readMapInks()
      const caps = g.renderer().capabilities
      const canvas = document.createElement('canvas')
      canvas.width = Math.min(4096, caps.maxTextureSize)
      canvas.height = canvas.width / 2
      paintGlobeTexture(canvas, base, inks, img)
      const tex = new THREE.CanvasTexture(canvas)
      tex.colorSpace = THREE.SRGBColorSpace
      tex.anisotropy = caps.getMaxAnisotropy()
      globeMat.map?.dispose()
      globeMat.map = tex
      globeMat.color.set('#ffffff')
      globeMat.needsUpdate = true
      lines = globeLines(base, inks)
      g.scene().add(lines)
    })
    return () => {
      live = false
      if (lines) {
        g.scene().remove(lines)
        disposeGlobeLines(lines)
      }
    }
  }, [ready, globeMat, paletteKey])
  useEffect(() => () => globeMat.map?.dispose(), [globeMat])

  // One-time light setup: warm sun at the subsolar point + low ambient (real
  // day/night terminator, night side never pure black). No bloom, no starfield —
  // this is a band above a data table, not a full-screen scene.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready) return
    const sun = new THREE.DirectionalLight(GLOBE_SUN.color, GLOBE_SUN.intensity)
    const ss = subsolarPoint(Date.now())
    const p = g.getCoords(ss.lat, ss.lon, 2)
    sun.position.set(p.x, p.y, p.z)
    const ambient = new THREE.AmbientLight(GLOBE_AMBIENT.color, GLOBE_AMBIENT.intensity)
    // globe.gl's own lights, set as Connect's globe sets them: they REPLACE its default camera-chasing
    // pair, so the terminator is real. Never by taking the scene's lights out here: globe.gl puts its
    // defaults in the scene from a timer of its own, which the browser may run after this effect, and
    // then they stayed on beside these (lit all round, washed out, no night side).
    g.lights([sun, ambient])
    return () => {
      sun.dispose()
      ambient.dispose()
    }
  }, [ready])

  // The worked-grid dot cloud — the 2-D map's spot language on the sphere: flat,
  // round, band-colored, hugging the surface. One GPU draw for the whole log.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready) return
    const pos = new Float32Array(points.length * 3)
    const col = new Float32Array(points.length * 3)
    const tmp = new THREE.Color()
    for (let i = 0; i < points.length; i++) {
      const pt = points[i]
      const c = g.getCoords(pt.lat, pt.lng, 0.004)
      pos[i * 3] = c.x
      pos[i * 3 + 1] = c.y
      pos[i * 3 + 2] = c.z
      // Band palette colour, brightened a touch for busier squares (log-scaled) —
      // the same "more activity reads brighter" cue as the map without size games.
      tmp.set(bandColor(pt.band))
      const boost = 0.72 + Math.min(0.28, Math.log10(pt.n + 1) * 0.2)
      col[i * 3] = tmp.r * boost
      col[i * 3 + 1] = tmp.g * boost
      col[i * 3 + 2] = tmp.b * boost
    }
    let cloud = cloudRef.current
    if (!cloud) {
      // Painted, never ADDED as light (THREE.AdditiveBlending): added light shows only on a dark
      // globe, and on the map-coloured one it washed every dot out to a white speck.
      const mat = new THREE.PointsMaterial({
        size: DOT_PX, // screen-space px
        map: dotSprite(),
        vertexColors: true,
        transparent: true,
        opacity: 0.95,
        sizeAttenuation: false,
        depthWrite: false,
      })
      cloud = new THREE.Points(new THREE.BufferGeometry(), mat)
      // Over the coast, border and state lines (transparent too), as on the 2-D map.
      cloud.renderOrder = 1
      cloudRef.current = cloud
      g.scene().add(cloud)
    }
    cloud.geometry.setAttribute('position', new THREE.BufferAttribute(pos, 3))
    cloud.geometry.setAttribute('color', new THREE.BufferAttribute(col, 3))
    return () => {
      // Full teardown on unmount/data change re-runs: the effect rebuilds attributes
      // in place, so only dispose when the component goes away.
    }
  }, [ready, points])

  // Dispose the cloud with the component (the WebGL context dies with the unmount,
  // but explicit disposal keeps three.js bookkeeping clean on remount cycles).
  useEffect(() => {
    return () => {
      const cloud = cloudRef.current
      if (cloud) {
        cloud.geometry.dispose()
        const m = cloud.material as THREE.PointsMaterial
        m.map?.dispose()
        m.dispose()
        cloudRef.current = null
      }
    }
  }, [])

  // The slow spin — the identical mechanism and speed as the Connect globe.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready) return
    const controls = g.controls() as { autoRotate: boolean; autoRotateSpeed: number }
    controls.autoRotateSpeed = 0.3
    controls.autoRotate = spin
    surfaceSet(SPIN_KEY, spin ? '1' : '0')
  }, [ready, spin])

  // Scrolled out of view → pause the ENTIRE render loop (not just the spin): globe.gl
  // keeps its rAF running even for an off-screen canvas, which is exactly the idle GPU
  // burn the operator asked to prevent. Resumes the moment the band scrolls back.
  useEffect(() => {
    const el = wrapRef.current
    const g = globeRef.current
    if (!el || !g || !ready) return
    const io = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) g.resumeAnimation()
        else g.pauseAnimation()
      },
      { threshold: 0.02 },
    )
    io.observe(el)
    return () => {
      io.disconnect()
      g.resumeAnimation() // never leave a live instance paused
    }
  }, [ready])

  // Last of the effects, so its cleanup (which hands the context back) runs after theirs: the one
  // above resumes the loop. The spin draws a restored context again on its own.
  const paused = useGlobeWebgl(globeRef, ready)

  return (
    <div className="qso-globe" ref={wrapRef}>
      <button
        type="button"
        className={`globe3d-spin${spin ? ' active' : ''}`}
        onClick={() => setSpin((s) => !s)}
        title={spin ? t('logbook.globe.spin.stop.title') : t('logbook.globe.spin.start.title')}
      >
        {spin ? t('logbook.globe.spin.pause') : t('logbook.globe.spin.play')}
      </button>
      <div className="qso-globe-hud">
        <select
          className="qso-globe-band-pick"
          value={band}
          onChange={(e) => setBand(e.target.value)}
          style={band === 'all' ? undefined : { color: bandColor(band), borderColor: bandColor(band) }}
          title={t('logbook.globe.band.title')}
        >
          {/* The <option> VALUES are band tokens; only the "all" label is prose. */}
          <option value="all">{t('logbook.globe.band.all')}</option>
          {bandsInLog.map((b) => (
            <option key={b} value={b}>
              {b}
            </option>
          ))}
        </select>
        <span className="qso-globe-count">
          {band === 'all'
            ? t('logbook.globe.count.all', { count: points.length })
            : t('logbook.globe.count.band', { count: points.length, band })}
        </span>
      </div>
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
        />
      )}
      {paused && <GlobePaused onReload={onReload} />}
    </div>
  )
}
