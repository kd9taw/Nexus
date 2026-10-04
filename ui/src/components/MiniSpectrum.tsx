// A compact, self-fetching live spectrum trace — the "is my audio alive / what's on
// the band" glance strip. Used where the full cockpit scopes don't fit: the Settings
// audio section (device confirmation while picking inputs) and the Connect pane grid.
// Polls the same engine spectrum row every scope shares (native RF preferred, audio
// FFT fallback) and draws a filled trace; honest idle state when the row is flat.
//
// The trace is drawn by the spectrum renderer (ui/src/spectrum), as every scope's is: one value per
// pixel from all the bins under it (their maximum), through WebGL2 or canvas-2D. It keeps this
// strip's own look, the accent on the well (DISPLAY WELLS), by handing the renderer a table that
// runs from the well's face to the accent instead of a waterfall palette.
import { useEffect, useRef, useState } from 'react'
import { getSpectrumRow } from '../api'
import { agcRange, dbToSpan, WF_DB_SPAN } from '../waterfall'
import { createSpectrumRenderer, type SpectrumRenderer, type SpectrumScene } from '../spectrum'
import type { Spectrum } from '../types'
import { remoteApplicationTransport, applicationSessionGeneration, onApplicationSessionChange } from '../applicationTransport'
import { t } from '../i18n'

interface Props {
  /** Poll cadence (ms). The row is a cheap cached clone backend-side. */
  pollMs?: number
  /** Canvas height (px). */
  height?: number
  /** Shown when the spectrum is flat (device silent / wrong input). */
  idleHint?: string
}

/** One small canvas to paint a colour on and read it back, made the first time it is needed. */
let swatch: CanvasRenderingContext2D | null | undefined
/** A CSS colour as the browser paints it, any syntax a canvas takes; null with no 2-D context. */
function rgbOf(colour: string): [number, number, number] | null {
  swatch ??= document.createElement('canvas').getContext('2d', { willReadFrequently: true })
  const ctx = swatch
  if (!ctx) return null
  ctx.fillStyle = colour
  ctx.fillRect(0, 0, 1, 1)
  const d = ctx.getImageData(0, 0, 1, 1).data
  return [d[0], d[1], d[2]]
}

/** The table the renderer draws the trace with: the well's face at 0, rising evenly to the accent at
 *  255. The renderer fills the band with entry 0, shades under the trace from 0.3, 0.7 and 1.0 of
 *  the way up, and strokes the line in entry 255: the accent trace on the well. */
function inkLut(face: [number, number, number], ink: [number, number, number]): Uint8ClampedArray {
  const lut = new Uint8ClampedArray(256 * 4)
  for (let i = 0; i < 256; i++) {
    const t = i / 255
    lut[i * 4] = Math.round(face[0] + (ink[0] - face[0]) * t)
    lut[i * 4 + 1] = Math.round(face[1] + (ink[1] - face[1]) * t)
    lut[i * 4 + 2] = Math.round(face[2] + (ink[2] - face[2]) * t)
    lut[i * 4 + 3] = 255
  }
  return lut
}

/** Format a Hz edge for the axis label (kHz below 1 MHz, MHz above). */
function fmtHz(hz: number): string {
  if (!Number.isFinite(hz)) return ''
  return hz >= 1_000_000 ? `${(hz / 1_000_000).toFixed(3)} MHz` : `${Math.round(hz / 1000)} kHz`
}

export function MiniSpectrum({ pollMs = 120, height = 96, idleHint }: Props) {
  const remote=!!remoteApplicationTransport()
  // The renderer's box: it puts its own canvas in here and fills it.
  const hostRef = useRef<HTMLDivElement>(null)
  const rendererRef = useRef<SpectrumRenderer | null>(null)
  /** The ink table, and the two colours it was built from (rebuilt when either moves). */
  const inksRef = useRef<{ key: string; lut: Uint8ClampedArray | null } | null>(null)
  const [spec, setSpec] = useState<Spectrum | null>(null)
  const [alive, setAlive] = useState(false)

  useEffect(() => {
    let mounted = true
    const clear=()=>{if(mounted){setSpec(null);setAlive(false)}}
    const unsubscribe=remote?onApplicationSessionChange(clear):()=>{}
    const tick = () => {
      const generation=applicationSessionGeneration()
      getSpectrumRow(false)
        .then((s) => {
          if (!mounted || generation!==applicationSessionGeneration()) return
          setSpec(s)
          // "Alive" = visible dynamic range in the row (a silent/wrong device is flat).
          // On the dB axis the 0.05 threshold reads as ~6 dB of spread across the band,
          // which still separates a dead input (a digitally silent capture floors the whole
          // row at 0, so the spread is exactly 0) from any real one.
          const row = s.row ?? []
          let min = 1
          let max = 0
          for (const v of row) {
            if (v < min) min = v
            if (v > max) max = v
          }
          setAlive(row.length > 0 && max - min > 0.05)
        })
        .catch(() => {if(remote && generation===applicationSessionGeneration())clear()})
    }
    tick()
    const iv = setInterval(tick, pollMs)
    return () => {
      mounted = false
      unsubscribe()
      clearInterval(iv)
    }
  }, [pollMs,remote])

  // The renderer lives as long as the strip: a trace only, so it keeps two rows, not a history.
  useEffect(() => {
    const host = hostRef.current
    if (!host) return
    const renderer = createSpectrumRenderer(host, { depth: 2 })
    rendererRef.current = renderer
    return () => {
      renderer.destroy()
      rendererRef.current = null
    }
  }, [])

  useEffect(() => {
    const host = hostRef.current
    const renderer = rendererRef.current
    // No canvas context at all (jsdom): nothing to draw on.
    if (!host || !renderer || renderer.backend === 'none') return
    // The inks, read off the well itself (the DISPLAY WELLS pattern): its face and its accent.
    const styles = getComputedStyle(host)
    const face = styles.getPropertyValue('--well-bg').trim() || '#0b0f17'
    const accent = styles.getPropertyValue('--accent').trim() || '#4ea1ff'
    const key = `${face} ${accent}`
    if (inksRef.current?.key !== key) {
      const f = rgbOf(face)
      const a = rgbOf(accent)
      inksRef.current = { key, lut: f && a ? inkLut(f, a) : null }
    }
    const lut = inksRef.current.lut
    if (!lut) return
    const dpr = window.devicePixelRatio || 1
    const base: Omit<SpectrumScene, 'view' | 'trace'> = {
      lut,
      layout: { traceH: Math.round(host.clientHeight * dpr), stripH: 0, lineWidth: 1.25 * dpr },
      detector: 'peak',
      mode: '2d',
      offsetRows: 0,
      newestAtTop: true,
    }
    if (!spec?.row?.length) {
      // A remote reading that failed clears the trace (the band is left the bare face); a local
      // one keeps the last picture until the next row.
      if (remote) renderer.draw({ ...base, view: { loHz: 0, hiHz: 1 }, trace: null })
      return
    }
    const w = host.clientWidth
    const h = host.clientHeight
    if (w <= 0 || h <= 0) return
    // Resize only on a real size change (Waterfall.tsx is the reference): this effect re-runs on
    // every spectrum poll, and the renderer clears its canvas when it is sized.
    renderer.resize(Math.round(w * dpr), Math.round(h * dpr))
    // Row values are the UI's 0..1 contract, but that axis is LINEAR IN dB against an
    // ABSOLUTE full-scale reference (2026-08-04) — it is no longer self-scaling. The old
    // draw plotted them raw, which only ever filled the box because the producer divided
    // every row by its own loudest bin, pinning the peak to the top for free. With an
    // absolute reference a real capture sits in a narrow band partway up (a -90 dBFS floor
    // is 0.25, a -50 dBFS signal 0.58) and the trace would have flattened into an
    // uninformative smear near the bottom — a silent regression in the one strip whose
    // whole job is "is my audio alive". So scale it the same way every other scope does:
    // the shared visual-AGC window, with the same 10 dB minimum span the Phone/CW scope
    // uses so a noise-only band stays a low flat line instead of being stretched to
    // full height.
    const row = spec.row
    const { floor, ceil } = agcRange(row)
    const top = Math.max(ceil, floor + dbToSpan(10))
    // The whole row across the strip, in the row's own span.
    const loHz = spec.loHz ?? 0
    const hiHz = spec.hiHz != null && spec.hiHz > loHz ? spec.hiHz : loHz + 1
    renderer.draw({
      ...base,
      view: { loHz, hiHz },
      trace: { frame: { seq: 0, tMs: 0, loHz, hiHz, bins: row, dbPerUnit: WF_DB_SPAN }, range: { floor, ceil: top } },
    })
  }, [spec,remote])

  const srcBadge =
    remote&&!spec?'—':spec?.source === 'flex' ? 'FLEX RF' : spec?.source === 'civ' ? 'CI-V RF' : 'AUDIO'

  return (
    <div className="mini-spectrum">
      <div className="mini-spectrum-head">
        <span className="mini-spectrum-src">{srcBadge}</span>
        <span className="mini-spectrum-span">
          {spec?.loHz != null && spec?.hiHz != null
            ? `${fmtHz(spec.loHz)} – ${fmtHz(spec.hiHz)}`
            : ''}
        </span>
      </div>
      {/* A display well (styles.css DISPLAY WELLS): the `--well-bg`/`--accent` read above come off
          this box, so the trace is drawn in the dark theme's inks on a face dark in both. The
          renderer's canvas fills it. */}
      <div ref={hostRef} className="mini-spectrum-canvas well" style={{ height }} />
      {!alive && idleHint && <div className="mini-spectrum-idle">{remote&&!spec?t('remote.collectionUnavailable'):idleHint}</div>}
    </div>
  )
}
