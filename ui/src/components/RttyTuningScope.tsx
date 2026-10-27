// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): every word on screen comes
// from the catalog. Hertz figures are invariant tokens and go through `t()`'s number params.
//
// THE RTTY TUNING SCOPE — the crossed-ellipse XY display of a classic terminal unit, RIGHT of the
// waterfall in the waterfall's strip (operator's placement since 2026-10-26: the waterfall keeps the
// common height, the scope its size, and the waterfall gives up the width; ⊞ id `tuning`, shown by
// default). The nudges and the words stand beside the square, not under it: the strip is short and wide.
//
// WHAT IT DRAWS. The engine runs two resonators on the decoder's AFC-corrected mark and space
// (`tempo_core::rtty::scope`); the mark resonator is X and the space resonator Y. A tuned signal
// draws a square "+". A mistuned one turns the cross: clockwise when the signal sits HIGH,
// counter-clockwise when it sits LOW. Each poll brings the newest ~85 ms of points as i8 pairs
// against their own peak; this file fades the last picture (the phosphor), follows the level with
// a fast-attack, slow-decay scale so a fading signal keeps its size, and draws the dots, a
// crosshair and the M / S axis tags. The same reading goes out in words (`role="status"`, at most
// once a second) for anyone who cannot use the picture.
//
// ⚠️ THE NUDGES MOVE THE TRANSMITTER TOO. ◀ ▶ call `rtty_net(centre ∓ 5 Hz)`, the waterfall click's
// own command, and since 1.9.0 the AFSK transmitter follows the netted centre. So a nudge is a
// transmit-tone change, and both buttons are DISABLED WHILE KEYED (any over of ours, a latched
// stream, a tune): a held button cannot slide the tone in the middle of an over. A hold already
// repeating stops the instant `keyed` turns true, before its next step.
//
// UNDER THE STOP LINE (features/panelState.ts) it is a plain entry: it hosts no control that stops a
// transmission and none that starts one, and hiding it ends nothing in flight, so it carries no ⊞
// note. Its size lives in cockpit-panes.css (`.rtty-tuning`, `.rtty-tuning-box`).
//
// ITS POLL IS ITS COST: it asks only while it is mounted (shown), the cockpit is the visible view,
// the decoder is armed and the window is not hidden; under reduced motion it asks at 5 Hz, not 20.
import { useEffect, useRef, useState } from 'react'
import { getRttyScope } from '../api'
import { t } from '../i18n'
import { pollSingleFlight } from '../singleFlight'
import type { RttyScope } from '../types'
import { PaneCloseButton } from './panes/PaneCloseButton'

/** One nudge, Hz — the step an operator reaches for on a 170 Hz pair. */
export const NUDGE_HZ = 5
/** Within this many hertz the words say "tuned" (the engine's `scope::TUNED_HZ`). */
export const TUNED_HZ = 5
/** Hold-to-repeat: the first repeat after this long, then one per `HOLD_REPEAT_MS`. */
export const HOLD_DELAY_MS = 350
export const HOLD_REPEAT_MS = 80
/** Poll cadence: 20 Hz, 5 Hz under reduced motion. */
export const POLL_MS = 50
export const POLL_MS_REDUCED = 200
/** The words change at most this often, so a screen reader is not read a number per frame. */
export const STATUS_MS = 1000
/** How much of the last picture each frame leaves (the phosphor). */
const FADE = 0.3
/** The scale's slow decay per frame: a signal that fades keeps most of its size for a second. */
const SCALE_DECAY = 0.95
/** The axis tags: invariant tokens, the same in every language (Mark along X, Space along Y). */
const AXIS_MARK = 'M'
const AXIS_SPACE = 'S'

const reducedMotion = () =>
  (typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches) ||
  document.documentElement.getAttribute('data-motion') === 'reduce'

/** The tuning state in words, the text twin of the picture. `unavailable`: the read failed (a link
 *  that carries no scope); `scope` null otherwise means no frame has arrived yet. */
export function tuningWords(scope: RttyScope | null, armed: boolean, keyed: boolean, unavailable = false): string {
  if (!armed) return t('rtty.tuning.disarmed')
  if (keyed) return t('rtty.tuning.keyed')
  if (unavailable) return t('rtty.tuning.unavailable')
  if (!scope || !scope.live || scope.offsetHz == null) return t('rtty.tuning.noSignal')
  const hz = Math.round(Math.abs(scope.offsetHz))
  if (hz <= TUNED_HZ) return t('rtty.tuning.tuned')
  return scope.offsetHz > 0 ? t('rtty.tuning.high', { hz }) : t('rtty.tuning.low', { hz })
}

export function RttyTuningScope({
  active,
  armed,
  keyed,
  markHz,
  spaceHz,
  onNet,
  closeProps,
}: {
  /** False while the cockpit is kept alive behind another screen: no poll. */
  active: boolean
  /** The decoder is armed (nothing to draw otherwise). */
  armed: boolean
  /** Our own over, a latched stream or a tune — the cockpit's `keyed`. Nudges are off. */
  keyed: boolean
  markHz: number
  spaceHz: number
  /** Net the decoder (and the AFSK transmitter) onto a new centre — `rtty_net`. */
  onNet: (hz: number) => void
  /** The column's own ✕ when it carries its own head; omitted inside a frame that has one. */
  closeProps?: { onRemove?: () => void; hideNote?: string }
}) {
  const [scope, setScope] = useState<RttyScope | null>(null)
  const [unavailable, setUnavailable] = useState(false)
  const [words, setWords] = useState('')
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const frameRef = useRef<RttyScope | null>(null)
  const drawRef = useRef<(s: RttyScope | null) => void>(() => {})

  // The poll. A failed read (a link that carries no scope) shows as "not on this link".
  useEffect(() => {
    if (!active || !armed) {
      setScope(null)
      frameRef.current = null
      drawRef.current(null)
      return
    }
    let lastSeq = -1
    return pollSingleFlight('rtty scope', reducedMotion() ? POLL_MS_REDUCED : POLL_MS, (owns) => {
      if (document.hidden) return Promise.resolve()
      return getRttyScope()
        .then((s) => {
          if (!owns()) return
          if (s.seq === lastSeq && s.live === frameRef.current?.live) return
          lastSeq = s.seq
          frameRef.current = s
          setScope(s)
          setUnavailable(false)
          drawRef.current(s)
        })
        .catch(() => {
          if (!owns()) return
          frameRef.current = null
          setScope(null)
          setUnavailable(true)
          drawRef.current(null)
        })
    })
  }, [active, armed])

  // The words: at once when the state's KIND changes (no signal → a reading, tuned → high), but a
  // reading whose only change is its number at most once a second. Kind = the words with the
  // number taken out, which holds in every language.
  const nextWords = tuningWords(scope, armed, keyed, unavailable)
  const lastWordsAt = useRef(-Infinity)
  useEffect(() => {
    const now = performance.now()
    if (nextWords === words) return
    const kind = (w: string) => w.replace(/\d+/g, '#')
    const wait = STATUS_MS - (now - lastWordsAt.current)
    if (wait <= 0 || kind(nextWords) !== kind(words)) {
      lastWordsAt.current = now
      setWords(nextWords)
      return
    }
    const id = window.setTimeout(() => {
      lastWordsAt.current = performance.now()
      setWords(nextWords)
    }, wait)
    return () => window.clearTimeout(id)
  }, [nextWords, words])

  // The canvas: device-pixel sizing (the Waterfall.tsx pattern), phosphor fade, auto-scale.
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx || typeof ResizeObserver === 'undefined') return
    let devW = 1
    let devH = 1
    let scale = 0
    const colours = () => {
      const cs = getComputedStyle(canvas)
      return {
        bg: cs.getPropertyValue('--bg').trim() || '#000',
        trace: cs.getPropertyValue('--rx').trim() || '#69d98d',
        grid: cs.getPropertyValue('--text-faint').trim() || '#555',
        tag: cs.getPropertyValue('--text-dim').trim() || '#888',
      }
    }
    const draw = (s: RttyScope | null) => {
      const c = colours()
      const cx = devW / 2
      const cy = devH / 2
      // The largest square the box holds, centred: the box yields height on a short pane.
      const side = Math.min(devW, devH)
      const r = side * 0.45
      // The phosphor: the last picture dims instead of vanishing. Without a live frame, clear.
      ctx.globalAlpha = s?.live && s.xy.length ? FADE : 1
      ctx.fillStyle = c.bg
      ctx.fillRect(0, 0, devW, devH)
      ctx.globalAlpha = 1
      // Crosshair and axis tags (M along X, S along Y).
      ctx.strokeStyle = c.grid
      ctx.lineWidth = Math.max(1, side / 200)
      ctx.beginPath()
      ctx.moveTo(cx - r, cy)
      ctx.lineTo(cx + r, cy)
      ctx.moveTo(cx, cy - r)
      ctx.lineTo(cx, cy + r)
      ctx.stroke()
      ctx.fillStyle = c.tag
      ctx.font = `${Math.max(7, Math.round(side / 14))}px sans-serif`
      ctx.textBaseline = 'middle'
      ctx.fillText(AXIS_MARK, cx + r - side / 14, cy - side / 20)
      ctx.fillText(AXIS_SPACE, cx + side / 40, cy - r + side / 28)
      if (!s?.live || !s.xy.length || s.peak <= 0) {
        scale = 0
        return
      }
      // Fast attack, slow decay: the frame's own peak against a level that remembers.
      scale = s.peak > scale ? s.peak : scale * SCALE_DECAY + s.peak * (1 - SCALE_DECAY)
      const k = (r * (s.peak / scale)) / 127
      const dot = Math.max(1, Math.round(side / 130))
      ctx.fillStyle = c.trace
      for (let i = 0; i + 1 < s.xy.length; i += 2) {
        ctx.fillRect(cx + s.xy[i] * k - dot / 2, cy - s.xy[i + 1] * k - dot / 2, dot, dot)
      }
    }
    drawRef.current = draw
    const ro = new ResizeObserver((entries) => {
      const dpcb = entries[0]?.devicePixelContentBoxSize?.[0]
      const dpr = window.devicePixelRatio || 1
      const rect = canvas.getBoundingClientRect()
      // A keep-alive host's 0×0 first fire must not size the bitmap to nothing.
      if (!dpcb && (rect.width === 0 || rect.height === 0)) return
      devW = Math.max(1, dpcb ? dpcb.inlineSize : Math.round(rect.width * dpr))
      devH = Math.max(1, dpcb ? dpcb.blockSize : Math.round(rect.height * dpr))
      canvas.width = devW
      canvas.height = devH
      draw(frameRef.current)
    })
    ro.observe(canvas)
    return () => {
      ro.disconnect()
      drawRef.current = () => {}
    }
  }, [])

  // ◀ ▶ with hold-to-repeat. The target accumulates across a hold (each step is 5 Hz from the
  // last one asked for, not from a centre the poll has yet to bring back).
  const centre = (markHz + spaceHz) / 2
  const nudgeOff = keyed || !armed
  const target = useRef<number | null>(null)
  const timer = useRef<number | null>(null)
  const keyedRef = useRef(keyed)
  keyedRef.current = keyed
  const stopHold = () => {
    if (timer.current != null) window.clearTimeout(timer.current)
    timer.current = null
    target.current = null
  }
  const step = (dir: 1 | -1) => {
    // Re-checked on every repeat: an over that starts mid-hold ends the hold before it moves TX.
    if (keyedRef.current) {
      stopHold()
      return
    }
    const next = (target.current ?? centre) + dir * NUDGE_HZ
    target.current = next
    onNet(next)
  }
  const startHold = (dir: 1 | -1) => {
    stopHold()
    step(dir)
    const repeat = (wait: number) => {
      timer.current = window.setTimeout(() => {
        if (timer.current == null) return
        step(dir)
        if (timer.current != null) repeat(HOLD_REPEAT_MS)
      }, wait)
    }
    if (target.current != null) repeat(HOLD_DELAY_MS)
  }
  useEffect(() => {
    if (nudgeOff) stopHold()
  }, [nudgeOff])
  useEffect(() => stopHold, [])

  const nudge = (dir: 1 | -1, label: string, glyph: string) => (
    <button
      type="button"
      className="rtty-tuning-btn"
      disabled={nudgeOff}
      aria-label={label}
      title={keyed ? t('rtty.tuning.nudge.keyed') : t('rtty.tuning.nudge.title')}
      onPointerDown={(e) => {
        if (e.button !== 0) return
        startHold(dir)
      }}
      onPointerUp={stopHold}
      onPointerLeave={stopHold}
      onPointerCancel={stopHold}
      // A keyboard press (Enter / Space) arrives as a click with no pointer: one step.
      onClick={(e) => {
        if (e.detail === 0) step(dir)
        if (timer.current == null) target.current = null
      }}
    >
      {glyph}
    </button>
  )

  const plate = !armed ? t('rtty.tuning.disarmed') : keyed ? t('rtty.tuning.keyed') : null
  const title = t('rtty.tuning.title')
  return (
    <div className="rtty-tuning" role="group" aria-label={title}>
      {/* The square (cockpit-panes.css): the window's share, capped by the strip's height where the
          operator drags the strip shorter, and the picture is drawn as the largest square that fits. Its
          ✕ sits in its corner rather than on a head row of its own, which would cost the picture. */}
      <div className="rtty-tuning-box">
        <div className="rtty-tuning-picture" role="img" aria-label={t('rtty.tuning.scope.aria', { state: nextWords })}>
          <canvas ref={canvasRef} className="rtty-tuning-canvas" aria-hidden="true" />
          {/* The words below already say this to a screen reader. */}
          {plate && <span className="rtty-tuning-plate" aria-hidden="true">{plate}</span>}
        </div>
        {closeProps && (
          <span className="rtty-tuning-close">
            <PaneCloseButton title={title} onRemove={closeProps.onRemove} hideNote={closeProps.hideNote} />
          </span>
        )}
      </div>
      <div className="rtty-tuning-nudge">
        {nudge(-1, t('rtty.tuning.down.aria', { hz: NUDGE_HZ }), '◀')}
        <span className="rtty-tuning-mark" title={t('rtty.tuning.mark.title')}>
          {t('rtty.tuning.mark', { hz: Math.round(markHz) })}
        </span>
        {nudge(1, t('rtty.tuning.up.aria', { hz: NUDGE_HZ }), '▶')}
      </div>
      {/* A polite live region (what role="status" is), without the role: the cockpit's own status
          line keeps that name to itself. */}
      <p className="rtty-tuning-status" aria-live="polite" aria-atomic="true">
        {words || nextWords}
      </p>
    </div>
  )
}
