import { useEffect, useRef } from 'react'
import { getFastPower } from '../api'
import type { DecodeRow } from '../types'

/**
 * The Fast Graph — MSK144's display, replacing the frequency waterfall while that tier is
 * active.
 *
 * WHY A TIME AXIS. On a meteor-scatter mode every signal sits at 1500 Hz and lives for
 * milliseconds, so a frequency waterfall shows one unmoving stripe and the actual event — the
 * ping — is invisible (operator report, martho24: "you don't need a 3 kHz display, you need a
 * 15/30/60 second display"). WSJT-X hides its waterfall in MSK144 and shows exactly this graph
 * (`widgets/fastplot.cpp`): X = seconds across ONE T/R period, a green power-vs-time trace,
 * current period on top and the previous period below it, so a ping that just ended stays
 * on screen a full period for reading and pouncing.
 *
 * DATA: raw 20 ms RMS samples from the rx-dsp thread (`get_fast_power`, meter bus, no engine
 * mutex — pings must keep rendering while the engine is busy with a boundary decode). The raw
 * series, not the ballistics-shaped level meter: a ping IS the fast attack the meter smooths
 * away. Decode markers ride `DecodeRow.dtSec`, which on MSK144 is the ping time within the
 * period — the T column, not a clock skew.
 */
interface Props {
  /** T/R period in seconds (the msk144PeriodS setting: 5/10/15/30). */
  periodS: number
  /** Current-period decodes; each paints a tick + callsign at its T. */
  decodes?: DecodeRow[]
  theme: string
}

/** Floor of the dB scale — a tick RMS below this draws at the baseline. ~S0 audio. */
const DB_FLOOR = -60
const POLL_MS = 100

export function FastGraph({ periodS, decodes, theme }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const periodRef = useRef(periodS)
  periodRef.current = periodS
  const decodesRef = useRef(decodes)
  decodesRef.current = decodes
  const themeRef = useRef(theme)
  themeRef.current = theme

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    let running = true
    /** Samples of the CURRENT period and the PREVIOUS one, as (secIntoPeriod, dbNorm 0..1). */
    let cur: Array<[number, number]> = []
    let prev: Array<[number, number]> = []
    let lastSeq = 0
    let lastPeriodIndex = -1

    // Device-pixel sizing, the Waterfall.tsx pattern (reduced: this strip re-renders whole
    // frames each poll, so a retained bitmap buys nothing).
    let devW = 0
    let devH = 0
    const ro = new ResizeObserver((entries) => {
      const dpcb = entries[0]?.devicePixelContentBoxSize?.[0]
      const dpr = window.devicePixelRatio || 1
      const rect = canvas.getBoundingClientRect()
      devW = Math.max(1, dpcb ? dpcb.inlineSize : Math.round(rect.width * dpr))
      devH = Math.max(1, dpcb ? dpcb.blockSize : Math.round(rect.height * dpr))
      canvas.width = devW
      canvas.height = devH
      draw()
    })
    ro.observe(canvas)

    const norm = (rms: number) => {
      // 0..1 linear RMS → 0..1 over a fixed dB window above the floor. Fixed, not auto-fit:
      // the whole point (as with the rig scope's SCOPE_WINDOW_DB) is that a strong ping draws
      // TALL and a weak one draws short.
      const db = 20 * Math.log10(Math.max(rms, 1e-6))
      return Math.max(0, Math.min(1, (db - DB_FLOOR) / -DB_FLOOR))
    }

    const drawPanel = (y0: number, h: number, pts: Array<[number, number]>, dim: boolean) => {
      const period = periodRef.current
      ctx.strokeStyle = dim ? 'rgba(80,200,120,0.55)' : 'rgb(80,220,120)'
      ctx.lineWidth = Math.max(1, devH / 300)
      ctx.beginPath()
      let started = false
      for (const [t, v] of pts) {
        const x = (t / period) * devW
        const y = y0 + h - 1 - v * (h - 2)
        if (started) ctx.lineTo(x, y)
        else {
          ctx.moveTo(x, y)
          started = true
        }
      }
      if (started) ctx.stroke()
      // 1 s ticks along the panel base, the fastplot scale.
      ctx.fillStyle = 'rgba(255,255,255,0.25)'
      for (let s = 1; s < period; s++) {
        const x = Math.round((s / period) * devW)
        ctx.fillRect(x, y0 + h - Math.max(3, h * 0.06), 1, Math.max(3, h * 0.06))
      }
    }

    const draw = () => {
      if (devW <= 0 || devH <= 0) return
      const dark = themeRef.current !== 'light'
      ctx.fillStyle = dark ? '#0b0f14' : '#f2f5f8'
      ctx.fillRect(0, 0, devW, devH)
      const half = Math.floor(devH / 2)
      drawPanel(0, half - 1, cur, false)
      drawPanel(half + 1, devH - half - 1, prev, true)
      // divider
      ctx.fillStyle = 'rgba(128,128,128,0.4)'
      ctx.fillRect(0, half - 1, devW, 1)
      // Decode markers on the CURRENT panel: a tick + callsign at each ping's T.
      const period = periodRef.current
      ctx.font = `${Math.max(9, Math.round(devH / 16))}px system-ui, sans-serif`
      for (const d of decodesRef.current ?? []) {
        const t = d.dtSec
        if (t == null || t < 0 || t > period) continue
        const x = (t / period) * devW
        ctx.fillStyle = 'rgba(255,200,80,0.9)'
        ctx.fillRect(Math.round(x), 0, 1, half - 1)
        const call = d.from ?? ''
        if (call) ctx.fillText(call, Math.min(x + 3, devW - 60), Math.max(12, devH / 16))
      }
    }

    const tick = async () => {
      if (!running) return
      try {
        const batch = await getFastPower(lastSeq)
        if (batch.length > 0) {
          lastSeq = batch[batch.length - 1].seq + 1
          const period = periodRef.current
          for (const s of batch) {
            // Period phase from wall clock, WSJT-X's own convention (periods align to UTC).
            const secIntoPeriod = (s.unixMs / 1000) % period
            const periodIndex = Math.floor(s.unixMs / 1000 / period)
            if (periodIndex !== lastPeriodIndex) {
              if (lastPeriodIndex !== -1) prev = cur
              cur = []
              lastPeriodIndex = periodIndex
            }
            cur.push([secIntoPeriod, norm(s.rms)])
          }
          draw()
        }
      } catch {
        /* transient IPC failure — next poll retries */
      }
      if (running) setTimeout(tick, POLL_MS)
    }
    void tick()

    return () => {
      running = false
      ro.disconnect()
    }
    // run once; live props via refs
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return (
    <div className="fastgraph-wrap" style={{ position: 'relative', width: '100%', height: '100%' }}>
      <canvas
        ref={canvasRef}
        aria-label="Fast Graph — signal power across the T/R period; pings draw as spikes"
        style={{ position: 'absolute', inset: 0, width: '100%', height: '100%' }}
      />
    </div>
  )
}
