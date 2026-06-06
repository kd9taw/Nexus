import { useEffect, useRef } from 'react'
import { getSpectrumRow } from '../api'
import type { DecodeRow } from '../types'

interface Props {
  transmitting: boolean
  /** Receive audio offset (Hz) — the green marker (where we listen). */
  rxOffsetHz: number
  /** Transmit audio offset (Hz) — the red marker (where we transmit). */
  txOffsetHz: number
  /** Recently decoded signals, marked at their real audio offsets. */
  decodes: DecodeRow[]
  theme: string
  /** Click to tune: `shift` = set TX offset, otherwise set RX offset. */
  onTune?: (freqHz: number, shift: boolean) => void
}

// Audio passband shown on the waterfall (matches the engine's 200–2900 Hz band).
const F_MIN = 200
const F_MAX = 2900
const BINS = 120

// Map a 0..1 magnitude to an RGB color for the given theme.
function palette(theme: string, v: number): [number, number, number] {
  const t = Math.max(0, Math.min(1, v))
  if (theme === 'amber') {
    // amber-on-black: black -> deep amber -> bright amber -> white-hot
    const r = Math.min(255, Math.round(40 + t * 215))
    const g = Math.min(255, Math.round(t * t * 170))
    const b = Math.round(t * t * t * 60)
    return [r, g, b]
  }
  if (theme === 'light') {
    // light: white -> teal -> deep blue (dark = strong) for sunlight contrast
    const r = Math.round(255 - t * 215)
    const g = Math.round(255 - t * 150)
    const b = Math.round(255 - t * 90)
    return [r, g, b]
  }
  // dark (default): black -> teal -> green -> yellow -> white (modern "inferno-ish" cool)
  const stops: [number, [number, number, number]][] = [
    [0.0, [8, 12, 24]],
    [0.25, [16, 70, 96]],
    [0.5, [20, 150, 130]],
    [0.7, [80, 200, 120]],
    [0.85, [220, 220, 90]],
    [1.0, [255, 255, 235]],
  ]
  for (let i = 0; i < stops.length - 1; i++) {
    const [p0, c0] = stops[i]
    const [p1, c1] = stops[i + 1]
    if (t >= p0 && t <= p1) {
      const f = (t - p0) / (p1 - p0)
      return [
        Math.round(c0[0] + (c1[0] - c0[0]) * f),
        Math.round(c0[1] + (c1[1] - c0[1]) * f),
        Math.round(c0[2] + (c1[2] - c0[2]) * f),
      ]
    }
  }
  return stops[stops.length - 1][1]
}

function freqToX(hz: number, width: number): number {
  const f = Math.max(F_MIN, Math.min(F_MAX, hz))
  return ((f - F_MIN) / (F_MAX - F_MIN)) * width
}

function binToFreq(bin: number): number {
  return F_MIN + (bin / (BINS - 1)) * (F_MAX - F_MIN)
}

function xToFreq(x: number, width: number): number {
  return F_MIN + (x / width) * (F_MAX - F_MIN)
}

export function Waterfall({ transmitting, rxOffsetHz, txOffsetHz, decodes, theme, onTune }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const rafRef = useRef<number | null>(null)
  // refs so the animation loop always reads current props without re-subscribing
  const txRef = useRef(transmitting)
  const themeRef = useRef(theme)
  const rxOffRef = useRef(rxOffsetHz)
  const txOffRef = useRef(txOffsetHz)
  const decodesRef = useRef(decodes)

  txRef.current = transmitting
  themeRef.current = theme
  rxOffRef.current = rxOffsetHz
  txOffRef.current = txOffsetHz
  decodesRef.current = decodes

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    let running = true
    let acc = 0
    let last = performance.now()
    const ROW_MS = 120 // new waterfall row cadence

    // device-pixel sizing
    const resize = () => {
      const rect = canvas.getBoundingClientRect()
      const dpr = window.devicePixelRatio || 1
      canvas.width = Math.max(1, Math.round(rect.width * dpr))
      canvas.height = Math.max(1, Math.round(rect.height * dpr))
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    }
    resize()
    const ro = new ResizeObserver(resize)
    ro.observe(canvas)

    // Bottom freq-axis strip (CSS px) — thinner when the waterfall is a short
    // horizontal strip (top layout) so it doesn't eat the limited height.
    const axisHFor = (h: number) => (h < 160 ? 14 : 18)

    const drawRow = async () => {
      const rect = canvas.getBoundingClientRect()
      const W = rect.width
      const H = rect.height
      const AXIS_H = axisHFor(H)
      const wfH = H - AXIS_H
      if (W <= 0 || wfH <= 0) return

      // scroll existing image up by 1px
      try {
        const img = ctx.getImageData(0, 1, W * (window.devicePixelRatio || 1), (wfH - 1) * (window.devicePixelRatio || 1))
        ctx.putImageData(img, 0, 0)
      } catch {
        // ignore (e.g. zero-size during layout)
      }

      const spec = await getSpectrumRow(txRef.current)
      const row = spec.row
      const th = themeRef.current
      // draw newest row at the bottom of the waterfall area
      const y = wfH - 1
      for (let x = 0; x < W; x++) {
        const f = xToFreq(x, W)
        const bin = ((f - F_MIN) / (F_MAX - F_MIN)) * (row.length - 1)
        const b0 = Math.floor(bin)
        const b1 = Math.min(row.length - 1, b0 + 1)
        const frac = bin - b0
        const v = row[b0] * (1 - frac) + row[b1] * frac
        const [r, g, b] = palette(th, v)
        ctx.fillStyle = `rgb(${r},${g},${b})`
        ctx.fillRect(x, y, 1, 1)
      }
    }

    const drawOverlay = () => {
      const rect = canvas.getBoundingClientRect()
      const W = rect.width
      const H = rect.height
      const AXIS_H = axisHFor(H)
      const wfH = H - AXIS_H
      const th = themeRef.current
      const axisColor =
        th === 'light' ? 'rgba(40,50,70,0.7)' : th === 'amber' ? 'rgba(255,176,0,0.7)' : 'rgba(190,205,230,0.7)'
      const axisBg =
        th === 'light' ? 'rgba(245,247,250,0.95)' : th === 'amber' ? 'rgba(10,7,2,0.95)' : 'rgba(10,14,22,0.92)'

      // --- bottom frequency axis ---
      ctx.fillStyle = axisBg
      ctx.fillRect(0, wfH, W, AXIS_H)
      ctx.fillStyle = axisColor
      ctx.font = '10px system-ui, sans-serif'
      ctx.textBaseline = 'middle'
      const labelStep = W < 280 ? 1000 : 500 // sparser labels when narrow
      for (let f = labelStep; f <= F_MAX; f += labelStep) {
        const x = freqToX(f, W)
        ctx.fillRect(x, wfH, 1, 4)
        ctx.fillText(`${f}`, Math.min(W - 26, x + 2), wfH + AXIS_H / 2)
      }

      // --- decoded-signal markers at their REAL audio offsets ---
      ctx.font = '10px system-ui, sans-serif'
      const seen = new Set<string>()
      decodesRef.current
        .filter((d) => d.freqHz > 0)
        .slice(0, 8)
        .forEach((d) => {
          const call = d.from ?? d.message.split(' ')[0]
          if (seen.has(call)) return
          seen.add(call)
          const x = freqToX(d.freqHz, W)
          // thin tick at the signal's offset
          ctx.fillStyle =
            th === 'light' ? 'rgba(40,50,70,0.5)' : 'rgba(190,205,230,0.45)'
          ctx.fillRect(x, 0, 1, wfH)
          // call label, colored by SNR
          const rows = wfH < 90 ? 2 : 4 // fewer label rows in a short strip
          const y = 16 + (seen.size % rows) * 13
          const w = ctx.measureText(call).width + 8
          ctx.fillStyle =
            th === 'amber' ? 'rgba(40,26,0,0.8)' : th === 'light' ? 'rgba(255,255,255,0.82)' : 'rgba(10,16,28,0.78)'
          ctx.fillRect(Math.min(W - w - 2, x + 1), y - 7, w, 14)
          ctx.fillStyle = d.snr >= -12 ? '#3ddc8c' : d.snr >= -18 ? '#e0b020' : '#9aa6b8'
          ctx.fillText(call, Math.min(W - w + 2, x + 5), y)
        })

      // --- TX marker (red) then RX marker (green), drawn last so they're on top ---
      const txx = freqToX(txOffRef.current, W)
      ctx.fillStyle = txRef.current ? 'rgba(255,70,70,0.95)' : 'rgba(255,90,90,0.7)'
      ctx.fillRect(txx - 1, 0, 2, wfH)
      ctx.fillStyle = '#ff5a5a'
      ctx.font = '600 10px system-ui, sans-serif'
      ctx.fillText('TX', Math.min(W - 18, txx + 3), 9)

      const rxx = freqToX(rxOffRef.current, W)
      ctx.fillStyle = 'rgba(60,220,140,0.9)'
      ctx.fillRect(rxx - 1, 0, 2, wfH)
      ctx.fillStyle = '#3ddc8c'
      ctx.fillText('RX', Math.min(W - 18, rxx + 3), wfH - 6)
    }

    const loop = (now: number) => {
      if (!running) return
      acc += now - last
      last = now
      let pending = Promise.resolve()
      if (acc >= ROW_MS) {
        acc = 0
        pending = drawRow()
      }
      pending.then(() => {
        if (running) drawOverlay()
      })
      rafRef.current = requestAnimationFrame(loop)
    }
    rafRef.current = requestAnimationFrame(loop)

    return () => {
      running = false
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current)
      ro.disconnect()
    }
    // intentionally run once; live props read via refs
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const handleClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!onTune) return
    const rect = canvasRef.current!.getBoundingClientRect()
    const x = e.clientX - rect.left
    onTune(Math.round(xToFreq(x, rect.width)), e.shiftKey)
  }

  return (
    <div className="waterfall-wrap">
      <div className="panel-header">
        <h2>Waterfall</h2>
        <span className="wf-hint">click = RX · shift-click = TX</span>
      </div>
      <canvas
        ref={canvasRef}
        className="waterfall-canvas"
        onClick={handleClick}
        title="Click to set RX offset; Shift-click to set TX offset"
      />
    </div>
  )
}

export { binToFreq }
