// Lean 3-D "world of contacts" globe for the top of the Logbook: every logged QSO's
// grid square as a glowing dot on the same textured day/night earth as the Connect
// globe (identical material recipe), with the same gentle 0.3°/frame auto-spin and a
// ▶/⏸ toggle. DELIBERATELY not Globe3D: no propagation spots, no aurora/MUF/PCA
// layers, no insight rail, no background pollers — the logbook band shows *your*
// contacts and nothing else, and can never destabilize the Connect globe.
//
// Resource story (the operator's hard requirement): the Logbook view is rendered
// inside App's view switch, so this component UNMOUNTS when you leave the Logbook —
// WebGL context destroyed, zero GPU. While mounted, an IntersectionObserver pauses
// the globe's whole render loop (`pauseAnimation`) once the band scrolls out of view
// inside the log's scroll container, so reading old QSOs at the bottom of a long log
// costs nothing either.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import * as THREE from 'three'
import Globe, { type GlobeMethods } from 'react-globe.gl'
import earthUrl from '../assets/earth-relief.webp'
import earthNightUrl from '../assets/earth-night.webp'
import { gridToLatLon } from '../grid'
import { subsolarPoint } from '../mapGeo'
import type { LoggedQso } from '../types'

/** Spin preference; '0' = off. Default ON — the slow rotation is the point of the band. */
const SPIN_KEY = 'nexus.logbook.globespin'

interface GridPoint {
  lat: number
  lng: number
  /** QSOs logged in this 4-char square (drives the dot size + tooltip). */
  n: number
}

export default function QsoGlobe({ qsos }: { qsos: LoggedQso[] }) {
  const wrapRef = useRef<HTMLDivElement>(null)
  const globeRef = useRef<GlobeMethods | undefined>(undefined)
  const [size, setSize] = useState({ w: 0, h: 0 })
  const [ready, setReady] = useState(false)
  const [spin, setSpin] = useState(() => {
    try {
      return localStorage.getItem(SPIN_KEY) !== '0'
    } catch {
      return true
    }
  })

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

  // QSOs → unique 4-char grid squares → dots. The dedupe is what keeps a 50k-QSO FT8
  // log at a few hundred points instead of 50k (the proven Globe3D coverage pipeline);
  // the per-square count survives as dot weight. QSOs with no grid are skipped — the
  // DXCC-centroid fallback is a planned fast-follow (needs a backend resolve).
  const points = useMemo<GridPoint[]>(() => {
    const counts = new Map<string, number>()
    for (const q of qsos) {
      const gr = (q.grid ?? '').trim().toUpperCase()
      if (gr.length >= 4) {
        const key = gr.slice(0, 4)
        counts.set(key, (counts.get(key) ?? 0) + 1)
      }
    }
    const pts: GridPoint[] = []
    counts.forEach((n, gr) => {
      const ll = gridToLatLon(gr)
      if (ll) pts.push({ lat: ll.lat, lng: ll.lon, n })
    })
    return pts
  }, [qsos])

  // Same material recipe as the Connect globe (Globe3D) so the two read as one app:
  // day relief darkened to the cool blue-grey, city lights as a dim night-side glow.
  const globeMat = useMemo(() => {
    const loader = new THREE.TextureLoader()
    const day = loader.load(earthUrl)
    day.colorSpace = THREE.SRGBColorSpace
    const night = loader.load(earthNightUrl)
    night.colorSpace = THREE.SRGBColorSpace
    return new THREE.MeshPhongMaterial({
      map: day,
      color: new THREE.Color('#28323d'),
      emissiveMap: night,
      emissive: new THREE.Color('#ffffff'),
      emissiveIntensity: 0.35,
      shininess: 4,
    })
  }, [])

  // One-time light setup: warm sun at the subsolar point + low ambient (real
  // day/night terminator, night side never pure black). No bloom, no starfield —
  // this is a band above a data table, not a full-screen scene.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready) return
    const sun = new THREE.DirectionalLight('#fff2dc', 1.7)
    const ss = subsolarPoint(Date.now())
    const p = g.getCoords(ss.lat, ss.lon, 2)
    sun.position.set(p.x, p.y, p.z)
    const ambient = new THREE.AmbientLight('#8899bb', 0.35)
    const scene = g.scene()
    // Replace globe.gl's default camera-chasing lights so the terminator is real.
    const defaults = scene.children.filter((c) => c.type.endsWith('Light'))
    defaults.forEach((l) => scene.remove(l))
    scene.add(sun)
    scene.add(ambient)
    return () => {
      scene.remove(sun)
      scene.remove(ambient)
      sun.dispose()
      ambient.dispose()
    }
  }, [ready])

  // The slow spin — the identical mechanism and speed as the Connect globe.
  useEffect(() => {
    const g = globeRef.current
    if (!g || !ready) return
    const controls = g.controls() as { autoRotate: boolean; autoRotateSpeed: number }
    controls.autoRotateSpeed = 0.3
    controls.autoRotate = spin
    try {
      localStorage.setItem(SPIN_KEY, spin ? '1' : '0')
    } catch {
      /* ignore */
    }
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

  return (
    <div className="qso-globe" ref={wrapRef}>
      <button
        type="button"
        className={`globe3d-spin${spin ? ' active' : ''}`}
        onClick={() => setSpin((s) => !s)}
        title={spin ? 'Stop the slow rotation' : 'Start the slow rotation'}
      >
        {spin ? '⏸ Spin' : '▶ Spin'}
      </button>
      <span className="qso-globe-count">
        {points.length} grid square{points.length === 1 ? '' : 's'} worked
      </span>
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
          pointsData={points}
          pointLat="lat"
          pointLng="lng"
          // Coverage blue (matches Globe3D's "My coverage" cloud) — reads as "mine".
          pointColor={() => '#4da3ff'}
          pointAltitude={0.012}
          // Busier squares get slightly larger dots (log-scaled, capped).
          pointRadius={(d: object) => 0.26 + Math.min(0.3, Math.log10((d as GridPoint).n + 1) * 0.18)}
          pointLabel={(d: object) => {
            const p = d as GridPoint
            return `${p.n} QSO${p.n === 1 ? '' : 's'}`
          }}
        />
      )}
    </div>
  )
}
