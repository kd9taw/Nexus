// The Map surface — an offline azimuthal-equidistant "Beam Map" centered on the
// operator's grid, drawn on Canvas2D with d3-geo (no tiles, no WebGL). Beam
// headings are true radials; range rings are real great-circle distance. Colors
// route through the shared tokens (status/need) and the colormap LUT, so color
// means one thing app-wide. See tasks/specs/UI-map.md.
import { useEffect, useMemo, useRef, useState } from 'react'
import { geoPath } from 'd3-geo'
import { RotateCcw } from 'lucide-react'
import type { PropagationSnapshot, Station, WorkableCard } from '../types'
import type { Theme } from '../useTheme'
import { gridToLatLon, haversineKm, bearingDeg, type LatLon } from '../grid'
import {
  basemap,
  graticule,
  makeProjection,
  project,
  rangeRing,
  destinationPoint,
  greatCircle,
  type Projection,
} from '../mapGeo'
import { sampleLut } from '../colormaps'
import { needMeta } from '../propViz'
import { StateBlock } from './StateBlock'

interface Props {
  myGrid: string
  theme: Theme
  stations: Station[]
  prop: PropagationSnapshot | null
  selectedCall: string | null
  onSelectCall: (call: string | null) => void
}

type LayerKey = 'coast' | 'grid' | 'rings' | 'stations' | 'paths' | 'openings' | 'dxped'
interface Layer {
  label: string
  visible: boolean
  opacity: number
}
const DEFAULT_LAYERS: Record<LayerKey, Layer> = {
  coast: { label: 'Coastlines', visible: true, opacity: 0.55 },
  grid: { label: 'Grid (20°×10°)', visible: true, opacity: 0.3 },
  rings: { label: 'Range rings', visible: true, opacity: 0.5 },
  stations: { label: 'Spots', visible: true, opacity: 1 },
  paths: { label: 'Selected path', visible: true, opacity: 1 },
  openings: { label: 'Openings', visible: true, opacity: 0.7 },
  dxped: { label: 'DXpeditions', visible: true, opacity: 1 },
}
const RINGS_KM = [1000, 3000, 5000, 10000]

function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || '#888'
}
function snrToken(snr: number): { v: string; r: number } {
  if (snr >= -12) return { v: '--snr-strong', r: 5 }
  if (snr >= -22) return { v: '--snr-marginal', r: 4 }
  return { v: '--snr-weak', r: 3 }
}

export function MapView({ myGrid, theme, stations, prop, selectedCall, onSelectCall }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const wrapRef = useRef<HTMLDivElement>(null)
  const [kind, setKind] = useState<Projection>('aeqd')
  const [layers, setLayers] = useState(DEFAULT_LAYERS)
  const [size, setSize] = useState({ w: 0, h: 0 })
  const [hover, setHover] = useState<{ x: number; y: number; text: string } | null>(null)

  const me = useMemo(() => gridToLatLon(myGrid), [myGrid])
  const dxCards: WorkableCard[] = useMemo(() => {
    const seen = new Set<string>()
    return (prop?.dxpeditions.workableNow ?? []).filter((c) => {
      if (seen.has(c.call)) return false
      seen.add(c.call)
      return true
    })
  }, [prop])
  const selStation = useMemo(
    () => stations.find((s) => s.call === selectedCall) ?? null,
    [stations, selectedCall],
  )

  // Track container size.
  useEffect(() => {
    const el = wrapRef.current
    if (!el) return
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }))
    ro.observe(el)
    setSize({ w: el.clientWidth, h: el.clientHeight })
    return () => ro.disconnect()
  }, [])

  // Project all stations once per draw input (also used for hit-testing).
  const placed = useMemo(() => {
    if (!me || size.w === 0) return [] as Array<{ s: Station; ll: LatLon; xy: [number, number] }>
    const proj = makeProjection(kind, me, size.w, size.h)
    const out: Array<{ s: Station; ll: LatLon; xy: [number, number] }> = []
    for (const s of stations) {
      if (!s.grid) continue
      const ll = gridToLatLon(s.grid)
      if (!ll) continue
      const xy = project(proj, ll)
      if (xy) out.push({ s, ll, xy })
    }
    return out
  }, [me, kind, size, stations])

  // Draw.
  useEffect(() => {
    const canvas = canvasRef.current
    const { w, h } = size
    if (!canvas || w === 0 || h === 0 || !me) return
    const dpr = window.devicePixelRatio || 1
    canvas.width = Math.round(w * dpr)
    canvas.height = Math.round(h * dpr)
    const ctx = canvas.getContext('2d')!
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, w, h)

    const proj = makeProjection(kind, me, w, h)
    const path = geoPath(proj, ctx)
    const c = project(proj, me)

    if (layers.coast.visible) {
      ctx.globalAlpha = layers.coast.opacity
      ctx.beginPath()
      path(basemap())
      ctx.strokeStyle = cssVar('--border')
      ctx.lineWidth = 1
      ctx.stroke()
    }
    if (layers.grid.visible) {
      ctx.globalAlpha = layers.grid.opacity
      ctx.beginPath()
      path(graticule())
      ctx.strokeStyle = cssVar('--border-soft')
      ctx.lineWidth = 0.5
      ctx.stroke()
    }
    if (layers.rings.visible && kind === 'aeqd') {
      ctx.globalAlpha = layers.rings.opacity
      ctx.strokeStyle = cssVar('--border')
      ctx.setLineDash([3, 3])
      ctx.lineWidth = 0.75
      for (const km of RINGS_KM) {
        ctx.beginPath()
        path(rangeRing(me, km))
        ctx.stroke()
      }
      ctx.setLineDash([])
    }
    ctx.globalAlpha = 1

    // Openings — bearing wedge out to maxKm, colored by probability (LUT).
    if (layers.openings.visible && prop) {
      ctx.globalAlpha = layers.openings.opacity
      for (const o of prop.openings) {
        const [r, g, b] = sampleLut('inferno', Math.max(0.2, o.probability))
        ctx.fillStyle = `rgb(${r}, ${g}, ${b})`
        ctx.beginPath()
        if (c) ctx.moveTo(c[0], c[1])
        for (let a = -16; a <= 16; a += 4) {
          const p = project(proj, destinationPoint(me, o.bearingDeg + a, o.maxKm))
          if (p) ctx.lineTo(p[0], p[1])
        }
        ctx.closePath()
        ctx.globalAlpha = layers.openings.opacity * 0.35
        ctx.fill()
        ctx.globalAlpha = 1
      }
    }

    // Selected great-circle path.
    if (layers.paths.visible && selStation?.grid) {
      const sll = gridToLatLon(selStation.grid)
      if (sll) {
        ctx.beginPath()
        path(greatCircle(me, sll))
        ctx.strokeStyle = cssVar('--accent')
        ctx.lineWidth = 1.5
        ctx.stroke()
      }
    }

    // Station dots (color = SNR token, size = SNR — CVD-safe redundant channel).
    if (layers.stations.visible) {
      ctx.globalAlpha = layers.stations.opacity
      for (const { s, xy } of placed) {
        const { v, r } = snrToken(s.snr)
        ctx.beginPath()
        ctx.arc(xy[0], xy[1], r, 0, Math.PI * 2)
        ctx.fillStyle = cssVar(v)
        ctx.fill()
        if (s.call === selectedCall) {
          ctx.beginPath()
          ctx.arc(xy[0], xy[1], r + 3, 0, Math.PI * 2)
          ctx.strokeStyle = cssVar('--accent')
          ctx.lineWidth = 2
          ctx.stroke()
        }
      }
      ctx.globalAlpha = 1
    }

    // DXpedition markers — placed by bearing+distance, glyph+color by need.
    if (layers.dxped.visible) {
      ctx.font = '13px system-ui'
      ctx.textAlign = 'center'
      ctx.textBaseline = 'middle'
      for (const card of dxCards) {
        const pos = destinationPoint(me, card.bearingDeg, card.distanceKm)
        const p = project(proj, pos)
        if (!p) continue
        const nm = needMeta(card.need)
        ctx.fillStyle = cssVar(nm.cssVar)
        ctx.fillText(nm.glyph, p[0], p[1])
      }
    }

    // Own station marker (on top).
    if (c) {
      ctx.beginPath()
      ctx.arc(c[0], c[1], 4, 0, Math.PI * 2)
      ctx.fillStyle = cssVar('--accent')
      ctx.fill()
      ctx.strokeStyle = cssVar('--bg')
      ctx.lineWidth = 1.5
      ctx.stroke()
    }
    // theme is a draw dependency so colors refresh on theme switch.
    void theme
  }, [me, kind, size, layers, placed, prop, dxCards, selStation, selectedCall, theme])

  if (!me) {
    return (
      <div className="map-view">
        <StateBlock
          kind="empty"
          title="Set your grid to see the map"
          detail="The Beam Map centers on your Maidenhead grid — set it in Settings, then every heading and range ring is measured from your QTH."
        />
      </div>
    )
  }

  const onMove = (e: React.MouseEvent) => {
    const rect = canvasRef.current!.getBoundingClientRect()
    const mx = e.clientX - rect.left
    const my = e.clientY - rect.top
    let best: { d: number; text: string } | null = null
    for (const { s, ll, xy } of placed) {
      const d = Math.hypot(xy[0] - mx, xy[1] - my)
      if (d < 9 && (!best || d < best.d)) {
        const km = Math.round(haversineKm(me, ll))
        best = { d, text: `${s.call} · ${s.grid} · ${s.snr} dB · ${bearingDeg(me, ll)}° ${km.toLocaleString()} km` }
      }
    }
    setHover(best ? { x: mx, y: my, text: best.text } : null)
  }
  const onClick = (e: React.MouseEvent) => {
    const rect = canvasRef.current!.getBoundingClientRect()
    const mx = e.clientX - rect.left
    const my = e.clientY - rect.top
    let best: { d: number; call: string } | null = null
    for (const { s, xy } of placed) {
      const d = Math.hypot(xy[0] - mx, xy[1] - my)
      if (d < 9 && (!best || d < best.d)) best = { d, call: s.call }
    }
    onSelectCall(best ? (best.call === selectedCall ? null : best.call) : null)
  }

  const prov = prop?.source ?? 'demo'

  return (
    <div className="map-view">
      <div className="map-toolbar">
        <div className="map-proj" role="group" aria-label="Projection">
          <button className={kind === 'aeqd' ? 'active' : ''} onClick={() => setKind('aeqd')}>
            Beam (AEQD)
          </button>
          <button className={kind === 'world' ? 'active' : ''} onClick={() => setKind('world')}>
            World
          </button>
        </div>
        <span className="map-center">◎ {myGrid}</span>
        <span className={`map-prov prov-${prov}`}>{prov === 'live' ? 'LIVE' : prov === 'cached' ? 'CACHED' : 'DEMO'}</span>
        <button className="map-reset" onClick={() => setLayers(DEFAULT_LAYERS)} title="Reset layers">
          <RotateCcw size={13} /> Reset
        </button>
      </div>

      <div className="map-body">
        <div className="map-canvas-wrap" ref={wrapRef}>
          <canvas
            ref={canvasRef}
            style={{ width: '100%', height: '100%' }}
            onMouseMove={onMove}
            onMouseLeave={() => setHover(null)}
            onClick={onClick}
          />
          {hover && (
            <div className="map-hover" style={{ left: hover.x + 12, top: hover.y + 12 }}>
              {hover.text}
            </div>
          )}
          <MapLegend />
        </div>

        <aside className="map-layers">
          <h3>Layers</h3>
          {(Object.keys(layers) as LayerKey[]).map((k) => (
            <div className="map-layer" key={k}>
              <label>
                <input
                  type="checkbox"
                  checked={layers[k].visible}
                  onChange={(e) => setLayers((L) => ({ ...L, [k]: { ...L[k], visible: e.target.checked } }))}
                />
                {layers[k].label}
              </label>
              <input
                type="range"
                min={0}
                max={1}
                step={0.05}
                value={layers[k].opacity}
                onChange={(e) => setLayers((L) => ({ ...L, [k]: { ...L[k], opacity: Number(e.target.value) } }))}
                aria-label={`${layers[k].label} opacity`}
              />
            </div>
          ))}
        </aside>
      </div>
    </div>
  )
}

function MapLegend() {
  const stops = useMemo(() => {
    return Array.from({ length: 6 }, (_, i) => {
      const [r, g, b] = sampleLut('inferno', i / 5)
      return `rgb(${r}, ${g}, ${b}) ${(i / 5) * 100}%`
    }).join(', ')
  }, [])
  return (
    <div className="map-legend" aria-hidden="true">
      <span>SNR ▂</span>
      <span className="snr-key weak" />
      <span className="snr-key marginal" />
      <span className="snr-key strong" />
      <span>▇</span>
      <span className="map-legend-sep" />
      <span>opening</span>
      <span className="map-legend-bar" style={{ background: `linear-gradient(90deg, ${stops})` }} />
    </div>
  )
}
