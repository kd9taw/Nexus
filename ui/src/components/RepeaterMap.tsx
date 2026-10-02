// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): every word comes from the catalog. What
// the canvas writes is data, not words: hearham's callsigns, the place names Program was given and
// a distance `units.ts` formats.
//
// PROGRAM'S MAP — the machines a search found, on a map (the operator's pick, 2026-09-30:
// "hearham-only map now"). EVERY DOT AND EVERY WORD ON IT IS HEARHAM'S: a machine stands where its
// hearham row places it and carries that row's callsign, output and town (`RepeaterMapPoint`, cut
// from the hearham row by the Rust merge, never from the merged record). hearham invites map use;
// RepeaterBook's terms forbid putting its rows on one, so a machine only RepeaterBook lists is not
// drawn, and the RSGB list is not mapped for now. For the same reason a marker shows no mode and no
// on-air state (those can be another directory's): the one thing it adds is the operator's own —
// whether the machine is in the channel list. Program says what is left off beside the map.
//
// It reuses the 2-D map's rendering (mapGeo: the azimuthal-equidistant projection, the bundled
// basemap and state lines, range rings) and its rules (the MAP BASEMAP tokens, `markerScaleFor`,
// `placeHoverCard`, the dark marker halo), not MapView itself: that component is Connect's, with
// its own layers, picker and stored setup, none of which a channel list needs.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { geoDistance, geoInterpolate, geoPath, type GeoProjection } from 'd3-geo'
import { basemap, destinationPoint, greatCircle, makeProjection, project, rangeRing, usStateBorders } from '../mapGeo'
import type { LatLon } from '../grid'
import { markerScaleFor, placeHoverCard } from './MapView'
import { useSkinActive } from '../useSkin'
import { fmtDistanceKm, useUnits } from '../units'
import { mhzLabel } from '../features/radioprog'
import { t } from '../i18n'

/** One machine on the map, from its hearham row. */
export interface MapMarker {
  /** The row's channel id: what a click hands back. */
  id: string
  lat: number
  lon: number
  call: string
  mhz: number
  city: string
  /** In the channel list: the one state the map shows that is not hearham's. */
  added: boolean
  /** A click adds it or takes it off (the row's ＋ rule). */
  pickable: boolean
}

interface Props {
  markers: readonly MapMarker[]
  /** The search's centre, or a route's start. */
  from: LatLon
  fromLabel: string
  /** A route's other end; absent on a search around one place. */
  to?: LatLon | null
  toLabel?: string
  /** The radius, or a route's corridor, km. */
  reachKm: number
  onPick: (id: string) => void
}

const R_EARTH_KM = 6371
/** How much of the half-box the searched area reaches, leaving the edge for a label. */
const FIT = 0.84
/** makeProjection's AEQD disc: radius = half the short side × this × zoom. */
const AEQD_DISC = 0.94

/** The machines: amber reads on the basemap's dark land and sea, which are dark in both themes
 *  (styles.css "MAP BASEMAP"), and on every built-in theme's own basemap. */
const MACHINE = '#ffcc44'
/** The radius ring and the route's corridor: the 2-D map's quiet path blue, so the area recedes
 *  under the machines in it. */
const AREA = '#8fb8d8'
const AREA_FILL = 'rgba(143, 184, 216, 0.14)'
const INK = '#eef3f6'
/** MapView's MARKER_HALO: a dark edge that lifts a marker or a label off whatever it lands on. */
const HALO = 'rgba(2, 7, 12, 0.9)'

/** Where the map is centred and how far it must reach: the place, or the middle of the route. */
function frame(from: LatLon, to: LatLon | null | undefined, reachKm: number): { center: LatLon; fitKm: number } {
  if (!to) return { center: from, fitKm: reachKm }
  const a: [number, number] = [from.lon, from.lat]
  const b: [number, number] = [to.lon, to.lat]
  const [lon, lat] = geoInterpolate(a, b)(0.5)
  return { center: { lat, lon }, fitKm: (geoDistance(a, b) * R_EARTH_KM) / 2 + reachKm }
}

/** The projection that fits the searched area to a `w`×`h` box: the 2-D map's AEQD, zoomed so
 *  the area's reach lands at FIT of the half short side. Distances from the centre are true. */
export function programProjection(
  from: LatLon,
  to: LatLon | null | undefined,
  reachKm: number,
  w: number,
  h: number,
): GeoProjection {
  const { center, fitKm } = frame(from, to, reachKm)
  const zoom = (FIT * Math.PI * R_EARTH_KM) / (AEQD_DISC * Math.max(1, fitKm))
  return makeProjection('aeqd', center, w, h, { zoom, rotate: null, panX: 0, panY: 0 })
}

/** How far a site's machines stand from it on the screen, px at marker scale 1. */
export const SITE_SPREAD_PX = 6

/** Machines on one site (a 2 m and a 70 cm machine on one tower, the commonest case) project to
 *  one point, where they would stack into one dot that only one of them could answer. So each
 *  site's machines are spread on a ring of `spread` px around it, in list order from the top,
 *  each still a dot, a hover and a click of its own. */
export function spreadSites<P extends { x: number; y: number }>(placed: P[], spread: number): P[] {
  const out = placed.slice()
  const done = new Set<number>()
  for (let i = 0; i < out.length; i++) {
    if (done.has(i)) continue
    const site = [i]
    for (let j = i + 1; j < out.length; j++) {
      if (!done.has(j) && Math.hypot(out[j].x - out[i].x, out[j].y - out[i].y) < 1) site.push(j)
    }
    if (site.length < 2) continue
    const { x, y } = out[i]
    site.forEach((k, n) => {
      done.add(k)
      const a = (2 * Math.PI * n) / site.length - Math.PI / 2
      out[k] = { ...out[k], x: x + spread * Math.cos(a), y: y + spread * Math.sin(a) }
    })
  }
  return out
}

const cssVar = (name: string, fallback: string): string =>
  getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback

export function RepeaterMap({ markers, from, fromLabel, to, toLabel, reachKm, onPick }: Props) {
  const units = useUnits()
  const skin = useSkinActive()
  const wrapRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [size, setSize] = useState({ w: 0, h: 0 })
  useEffect(() => {
    const el = wrapRef.current
    if (!el) return
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }))
    ro.observe(el)
    setSize({ w: el.clientWidth, h: el.clientHeight })
    return () => ro.disconnect()
  }, [])
  // Device px per layout px, from `devicePixelContentBoxSize`: under `.app`'s CSS zoom it is not
  // `devicePixelRatio`, and sizing the bitmap by that left it soft (MapView's devScale, which
  // explains the measurement).
  const [devScale, setDevScale] = useState(() => window.devicePixelRatio || 1)
  useEffect(() => {
    const el = canvasRef.current
    if (!el) return
    const apply = (entry?: ResizeObserverEntry) => {
      const dev = entry?.devicePixelContentBoxSize?.[0]
      const css = entry?.contentBoxSize?.[0]
      let s = window.devicePixelRatio || 1
      if (dev && css && css.inlineSize > 0) s = dev.inlineSize / css.inlineSize
      if (!Number.isFinite(s) || s <= 0) s = 1
      setDevScale((prev) => (Math.abs(prev - s) < 1e-3 ? prev : s))
    }
    const ro = new ResizeObserver((entries) => apply(entries[0]))
    try {
      ro.observe(el, { box: 'device-pixel-content-box' })
    } catch {
      ro.observe(el)
    }
    apply()
    return () => ro.disconnect()
  }, [])

  const ms = markerScaleFor(size.w, size.h)
  const proj = useMemo(
    () => (size.w > 0 && size.h > 0 ? programProjection(from, to, reachKm, size.w, size.h) : null),
    [from, to, reachKm, size.w, size.h],
  )
  /** Every marker's screen position: what is drawn and what a pointer hits are the same numbers. */
  const placed = useMemo(() => {
    if (!proj) return []
    const out: { m: MapMarker; x: number; y: number }[] = []
    for (const m of markers) {
      const p = project(proj, { lat: m.lat, lon: m.lon })
      if (p) out.push({ m, x: p[0], y: p[1] })
    }
    return spreadSites(out, SITE_SPREAD_PX * markerScaleFor(size.w, size.h))
  }, [proj, markers, size.w, size.h])

  useEffect(() => {
    const canvas = canvasRef.current
    const { w, h } = size
    if (!canvas || !proj || w === 0 || h === 0) return
    const devW = Math.round(w * devScale)
    const devH = Math.round(h * devScale)
    if (canvas.width !== devW || canvas.height !== devH) {
      canvas.width = devW
      canvas.height = devH
    }
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    ctx.setTransform(devScale, 0, 0, devScale, 0, 0)
    ctx.clearRect(0, 0, w, h)
    const path = geoPath(proj, ctx)
    const mono = cssVar('--font-mono', 'monospace')

    ctx.fillStyle = cssVar('--map-ocean', '#0f2334')
    ctx.fillRect(0, 0, w, h)
    ctx.beginPath()
    path(basemap())
    ctx.fillStyle = cssVar('--map-land', '#364a3c')
    ctx.fill()
    ctx.lineWidth = 0.8
    ctx.strokeStyle = cssVar('--map-coast', '#6f8a98')
    ctx.stroke()
    ctx.beginPath()
    path(usStateBorders())
    ctx.lineWidth = 0.7
    ctx.strokeStyle = cssVar('--map-state', '#4d6675')
    ctx.stroke()

    const haloText = (s: string, x: number, y: number) => {
      ctx.lineJoin = 'round'
      ctx.lineWidth = Math.max(2, 2.6 * ms)
      ctx.strokeStyle = HALO
      ctx.strokeText(s, x, y)
      ctx.fillText(s, x, y)
    }
    ctx.font = `600 ${Math.round(11 * ms)}px ${mono}`
    ctx.textBaseline = 'middle'

    // The searched area: the radius ring, or the corridor either side of the route's straight line
    // (a stroke as wide as the corridor, its round caps the reach past either end).
    const pxPerKm = proj.scale() / R_EARTH_KM
    if (to) {
      ctx.beginPath()
      path(greatCircle(from, to))
      ctx.lineCap = 'round'
      ctx.lineJoin = 'round'
      ctx.lineWidth = Math.max(2, 2 * reachKm * pxPerKm)
      ctx.strokeStyle = AREA_FILL
      ctx.stroke()
      ctx.lineWidth = 1.2
      ctx.setLineDash([5, 4])
      ctx.strokeStyle = AREA
      ctx.stroke()
      ctx.setLineDash([])
      ctx.lineCap = 'butt'
    } else {
      ctx.beginPath()
      path(rangeRing(from, reachKm))
      ctx.fillStyle = AREA_FILL
      ctx.fill()
      ctx.lineWidth = 1.2
      ctx.setLineDash([5, 4])
      ctx.strokeStyle = AREA
      ctx.stroke()
      ctx.setLineDash([])
      const top = project(proj, destinationPoint(from, 0, reachKm))
      if (top) {
        ctx.fillStyle = AREA
        ctx.textAlign = 'center'
        haloText(fmtDistanceKm(reachKm, units), top[0], top[1] - 9 * ms)
      }
    }

    // Machines, the farthest first, so the nearest (the list's first) sit on top.
    const r = 4.5 * ms
    for (let i = placed.length - 1; i >= 0; i--) {
      const { m, x, y } = placed[i]
      ctx.beginPath()
      ctx.arc(x, y, r, 0, Math.PI * 2)
      ctx.fillStyle = m.added ? MACHINE : HALO
      ctx.fill()
      ctx.lineWidth = Math.max(1.5, 1.8 * ms)
      ctx.strokeStyle = m.added ? HALO : MACHINE
      ctx.stroke()
      if (!m.added) continue
      ctx.beginPath()
      ctx.arc(x, y, r + 1.6 * ms, 0, Math.PI * 2)
      ctx.lineWidth = 1.2
      ctx.strokeStyle = MACHINE
      ctx.stroke()
    }
    // Callsigns, nearest first, each only where it does not cover one already written.
    ctx.textAlign = 'left'
    ctx.fillStyle = INK
    const taken: [number, number, number, number][] = []
    const lh = 13 * ms
    for (const { m, x, y } of placed) {
      if (!m.call) continue
      const lx = x + r + 4 * ms
      const lw = ctx.measureText(m.call).width
      const box: [number, number, number, number] = [lx, y - lh / 2, lx + lw, y + lh / 2]
      if (box[2] > w || taken.some((b) => box[0] < b[2] && box[2] > b[0] && box[1] < b[3] && box[3] > b[1]))
        continue
      taken.push(box)
      haloText(m.call, lx, y)
    }
    // The place (or the route's two ends) last, over everything.
    const ends: [LatLon, string][] = to ? [[from, fromLabel], [to, toLabel ?? '']] : [[from, fromLabel]]
    for (const [ll, label] of ends) {
      const p = project(proj, ll)
      if (!p) continue
      const arm = 6 * ms
      ctx.lineCap = 'round'
      for (const [lw, colour] of [
        [Math.max(3, 4 * ms), HALO],
        [Math.max(1.5, 2 * ms), INK],
      ] as const) {
        ctx.beginPath()
        ctx.moveTo(p[0] - arm, p[1])
        ctx.lineTo(p[0] + arm, p[1])
        ctx.moveTo(p[0], p[1] - arm)
        ctx.lineTo(p[0], p[1] + arm)
        ctx.lineWidth = lw
        ctx.strokeStyle = colour
        ctx.stroke()
      }
      ctx.lineCap = 'butt'
      if (label) {
        ctx.fillStyle = INK
        ctx.textAlign = 'center'
        haloText(label, p[0], p[1] + arm + 8 * ms)
      }
    }
  }, [size, devScale, proj, placed, ms, from, to, fromLabel, toLabel, reachKm, units, skin])

  // ── pointer: hover names a machine, a click adds it or takes it off ──
  const [hover, setHover] = useState<{ m: MapMarker; x: number; y: number } | null>(null)
  const hit = (e: { clientX: number; clientY: number }) => {
    const rect = canvasRef.current?.getBoundingClientRect()
    if (!rect) return null
    const px = e.clientX - rect.left
    const py = e.clientY - rect.top
    const reach = Math.max(8, 7 * ms)
    let best: { m: MapMarker; x: number; y: number; d: number } | null = null
    for (const p of placed) {
      const d = Math.hypot(p.x - px, p.y - py)
      if (d <= reach && (!best || d < best.d)) best = { ...p, d }
    }
    return best
  }
  const hoverRef = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    const el = hoverRef.current
    if (!el || !hover) return
    const { left, top } = placeHoverCard({
      ax: hover.x,
      ay: hover.y,
      cw: el.offsetWidth,
      ch: el.offsetHeight,
      vw: size.w,
      vh: size.h,
      clear: 10 * ms,
    })
    el.style.left = `${left}px`
    el.style.top = `${top}px`
  }, [hover, size.w, size.h, ms])

  return (
    <div ref={wrapRef} className="rp-map-stage">
      <canvas
        ref={canvasRef}
        role="img"
        aria-label={t('program.map.aria', { count: markers.length })}
        style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', cursor: hover?.m.pickable ? 'pointer' : undefined }}
        onPointerMove={(e) => {
          const b = hit(e)
          setHover((cur) => (b ? (cur?.m.id === b.m.id ? cur : { m: b.m, x: b.x, y: b.y }) : null))
        }}
        onPointerLeave={() => setHover(null)}
        onClick={(e) => {
          const b = hit(e)
          if (b?.m.pickable) onPick(b.m.id)
        }}
      />
      {hover && (
        <div
          ref={hoverRef}
          className="map-hover"
          style={placeHoverCard({ ax: hover.x, ay: hover.y, cw: 0, ch: 0, vw: size.w, vh: size.h, clear: 10 * ms })}
        >
          {`${hover.m.call || '—'} ${mhzLabel(hover.m.mhz)}${hover.m.city ? ` · ${hover.m.city}` : ''}`}
          {hover.m.pickable && (
            <>
              <br />
              {hover.m.added ? t('program.row.remove.title') : t('program.row.add.title')}
            </>
          )}
        </div>
      )}
      {/* The plotted machines as words, for a screen reader: the canvas draws them, this names
          them (hearham's callsign, output and town), in list order. */}
      <ul className="sr-only">
        {markers.map((m) => (
          <li key={m.id}>{`${m.call || '—'} ${mhzLabel(m.mhz)}${m.city ? ` · ${m.city}` : ''}`}</li>
        ))}
      </ul>
    </div>
  )
}
