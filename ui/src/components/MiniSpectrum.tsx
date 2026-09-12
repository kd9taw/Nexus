// A compact, self-fetching live spectrum trace — the "is my audio alive / what's on
// the band" glance strip. Used where the full cockpit scopes don't fit: the Settings
// audio section (device confirmation while picking inputs) and the Connect pane grid.
// Polls the same engine spectrum row every scope shares (native RF preferred, audio
// FFT fallback) and draws a filled trace; honest idle state when the row is flat.
import { useEffect, useRef, useState } from 'react'
import { getSpectrumRow } from '../api'
import { agcRange, dbToSpan, normalize } from '../waterfall'
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

/** Format a Hz edge for the axis label (kHz below 1 MHz, MHz above). */
function fmtHz(hz: number): string {
  if (!Number.isFinite(hz)) return ''
  return hz >= 1_000_000 ? `${(hz / 1_000_000).toFixed(3)} MHz` : `${Math.round(hz / 1000)} kHz`
}

export function MiniSpectrum({ pollMs = 120, height = 96, idleHint }: Props) {
  const remote=!!remoteApplicationTransport()
  const canvasRef = useRef<HTMLCanvasElement>(null)
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

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    if(!spec?.row?.length){if(remote)canvas.getContext('2d')?.clearRect(0,0,canvas.width,canvas.height);return}
    const dpr = window.devicePixelRatio || 1
    const w = canvas.clientWidth
    const h = canvas.clientHeight
    if (w <= 0 || h <= 0) return
    // Resize only on a real size change (Waterfall.tsx:335 is the reference): this
    // effect re-runs on every 120 ms spectrum poll, and an unguarded assignment
    // discarded the backing store ~8x a second for nothing.
    const devW = Math.round(w * dpr)
    const devH = Math.round(h * dpr)
    if (canvas.width !== devW || canvas.height !== devH) {
      canvas.width = devW
      canvas.height = devH
    }
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    // setTransform, NOT scale(): scale() is CUMULATIVE, and the resize above used to
    // reset the matrix on every draw. With the resize guarded, a cumulative scale
    // would compound dpr every frame and the trace would march off-canvas.
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    const styles = getComputedStyle(canvas)
    const accent = styles.getPropertyValue('--accent').trim() || '#4ea1ff'
    const dim = styles.getPropertyValue('--text-dim').trim() || '#888'
    ctx.clearRect(0, 0, w, h)
    // Faint mid gridline for scale reference.
    ctx.strokeStyle = dim
    ctx.globalAlpha = 0.2
    ctx.beginPath()
    ctx.moveTo(0, h / 2)
    ctx.lineTo(w, h / 2)
    ctx.stroke()
    ctx.globalAlpha = 1
    // The trace: filled area under a polyline.
    //
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
    const yFor = (v: number) => h - normalize(v, floor, top) * (h - 4)
    ctx.beginPath()
    ctx.moveTo(0, h)
    for (let i = 0; i < row.length; i++) {
      const x = (i / (row.length - 1)) * w
      ctx.lineTo(x, yFor(row[i]))
    }
    ctx.lineTo(w, h)
    ctx.closePath()
    ctx.globalAlpha = 0.25
    ctx.fillStyle = accent
    ctx.fill()
    ctx.globalAlpha = 1
    ctx.strokeStyle = accent
    ctx.lineWidth = 1.25
    ctx.beginPath()
    for (let i = 0; i < row.length; i++) {
      const x = (i / (row.length - 1)) * w
      const y = yFor(row[i])
      if (i === 0) ctx.moveTo(x, y)
      else ctx.lineTo(x, y)
    }
    ctx.stroke()
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
      <canvas ref={canvasRef} className="mini-spectrum-canvas" style={{ height }} />
      {!alive && idleHint && <div className="mini-spectrum-idle">{remote&&!spec?t('remote.collectionUnavailable'):idleHint}</div>}
    </div>
  )
}
