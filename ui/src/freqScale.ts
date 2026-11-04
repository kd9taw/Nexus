// THE FREQUENCY SCALE UNDER EVERY WATERFALL AND SCOPE — where its ticks fall, what they read, and
// the strip they are drawn in. One routine for all of them.
// 
// The FT waterfall has always drawn a strip of audio hertz along its foot (500, 1000 … 3000 on Std),
// and every mode on the `Waterfall` component (JS8, RTTY, PSK, SSTV, JTTY, the Tempo strip, the
// torn-off waterfall) inherited it. The Phone and CW scopes (`PhoneScope`, also the radio's own RF
// panadapter in the RF scope pane) drew only MHz labels — where a click lands, not how wide anything
// is — so on those screens there was no way to read a signal's width or the span shown. This module
// is the strip both components now draw.
// 
// WHAT A TICK READS follows the view's convention:
// - an AUDIO view reads audio hertz (`500`, `1000` …), what the FT waterfall reads. On Phone's
//   carrier-centred axis that is the pitch a point is heard at, so on LSB the numbers rise to the
//   left, and the guard band beyond the dial (no audio there) carries no tick. The CW window reads
//   audio hertz too, the unit its pitch is set in, so the dashed pitch line stands on its own number.
// - an RF panadapter reads the OFFSET FROM THE DIAL (`-5k` … `0` … `+5k`), as a radio's own scope
//   does; the absolute MHz stays on PhoneScope's in-picture scale above the strip. With the dial
//   unknown there is no honest offset, and the strip stays empty rather than inventing one.
// 
// WHERE THEY FALL: a round step (`SCALE_STEPS_HZ`) chosen from the span AND the room. No more than
// `SCALE_MAX_TICKS` across (so a 3–5 kHz view keeps 500 Hz, the FT waterfall's step), and never closer
// than one label's room (`SCALE_MIN_GAP_PX`, times the text scale), so a narrow pane or a 175 % UI
// steps out instead of crowding. A zoomed view gets finer steps: 200 Hz at 2 and 1.5 kHz, 100 Hz at 1 kHz,
// 600 Hz and the CW filter window, 50 Hz on the narrowest CW filter.
// 
// THE STRIP is the FT waterfall's, call for call (`Waterfall.shade.test.tsx` holds its frame byte for
// byte): the well's ground at 0.92, a 4 px tick at each step and the label just right of it in the
// well's ink at 0.7, the last label held inside the right edge. A label that would run into the one
// before it is left off and its tick kept — two labels run together read as one wrong number.
// Everything else on the picture (markers, passband edges, the filter-edge marks, the plates) stops
// at the strip's top, so no mark is drawn through a label.

import { isSymmetricMode, sidebandSign } from './waterfall'
import type { AxisKind } from './spectrum/markers'

/** The steps a scale may take, in Hz: numbers an operator recognises. */
export const SCALE_STEPS_HZ = [50, 100, 200, 500, 1_000, 2_000, 5_000, 10_000, 20_000, 50_000, 100_000, 200_000, 500_000, 1_000_000]
/** At most this many steps across a view. Every 3–5 kHz audio view keeps the FT waterfall's 500 Hz,
 *  Phone's Full (5 kHz and its eighth of guard band: 5.6 kHz, 11.25 steps) among them, while 600 Hz
 *  (12 steps of 50) takes 100 Hz, which reads cleaner on a narrow strip. */
export const SCALE_MAX_TICKS = 11.5
/** The least room between two ticks, CSS px at text scale 1: a four-digit label and its gap. */
export const SCALE_MIN_GAP_PX = 48

/** The strip's height in CSS px for a canvas `cssH` tall: thinner on a short strip so it does not
 *  eat the picture, and following the text size (the FT waterfall's `axisHFor`). */
export function scaleStripHeight(cssH: number, textScale: number): number {
  return Math.round((cssH < 160 ? 14 : 18) * textScale)
}

/** The step for a `spanHz` view `widthPx` wide, ticks no closer than `minGapPx` (a pane too narrow
 *  for two keeps one); null = no scale. */
export function scaleStepHz(spanHz: number, widthPx: number, minGapPx: number): number | null {
  if (!(spanHz > 0) || !Number.isFinite(spanHz) || !(widthPx > 0)) return null
  const step = SCALE_STEPS_HZ.find((s) => s <= spanHz && spanHz / s <= SCALE_MAX_TICKS && (widthPx * s) / spanHz >= minGapPx)
  if (step != null) return step
  // Too narrow for two labels: the widest step the span still holds, so one number stays to read.
  return [...SCALE_STEPS_HZ].reverse().find((s) => s <= spanHz) ?? null
}

/** The round ticks across `[loHz, hiHz]` at that step, both ends included. */
export function scaleTicks(loHz: number, hiHz: number, widthPx: number, minGapPx: number): number[] {
  const step = scaleStepHz(hiHz - loHz, widthPx, minGapPx)
  if (step == null) return []
  const out: number[] = []
  // Counted, not accumulated, so a long run never drifts off its round numbers; `+ 0` drops a -0.
  for (let k = Math.ceil(loHz / step); k * step <= hiHz; k++) out.push(k * step + 0)
  return out
}

/** An RF offset from the dial as a radio's scope prints it: `0`, `+500`, `-2.5k`, `+10k`. */
export function rfOffsetLabel(offHz: number): string {
  const off = Math.round(offHz)
  if (off === 0) return '0'
  const sign = off > 0 ? '+' : '-'
  const a = Math.abs(off)
  return a < 1000 ? `${sign}${a}` : `${sign}${Number((a / 1000).toFixed(2))}k`
}

/** A tick: where it stands on the drawn axis, and what it reads. */
export interface ScaleTick {
  at: number
  label: string
}

/**
 * The ticks for one of PhoneScope's views, `[lo, hi]` on its axis (`AxisKind`), `widthPx` wide.
 * Audio hertz on an audio axis, the offset from the dial on an RF one (see the header).
 */
export function scopeScaleTicks(axis: AxisKind, lo: number, hi: number, widthPx: number, minGapPx: number): ScaleTick[] {
  if (axis.rf) {
    const dial = axis.dialHz
    if (dial == null || !(dial > 0)) return []
    return scaleTicks(lo - dial, hi - dial, widthPx, minGapPx).map((off) => ({ at: dial + off, label: rfOffsetLabel(off) }))
  }
  // Phone's carrier-centred axis is the RF offset from the dial: the audio heard there is that offset
  // on USB (and on AM/FM, whose axis is laid out the same way), its negative on LSB.
  const sign = axis.carrierCentered && !isSymmetricMode(axis.sideband.trim().toUpperCase()) ? sidebandSign(axis.sideband) : 1
  const out: ScaleTick[] = []
  for (const t of scaleTicks(lo, hi, widthPx, minGapPx)) {
    const audio = sign * t + 0
    // Below 0 Hz there is no audio: the carrier-centred guard band beyond the dial.
    if (audio >= 0) out.push({ at: t, label: `${audio}` })
  }
  return out
}

/** One strip, as the FT waterfall draws it. Units are whatever the context draws in: CSS px under
 *  the Waterfall's transform, device px on PhoneScope's marks. */
export interface ScaleStrip {
  /** Width of the drawing, and the strip's top and height. */
  w: number
  top: number
  h: number
  /** Each tick's x and its label. */
  marks: { x: number; label: string }[]
  /** The well's ground and ink (`--well-bg`, `--well-ink`). */
  ground: string
  ink: string
  /** The label font's size. */
  fontPx: number
  /** Drawing units per CSS px for the tick and its gap (1 under a CSS-px transform, `scaleY` on
   *  device px), and per CSS px of TEXT (the text scale, times `scaleY` on device px). */
  gfx: number
  text: number
}

/** Draw the strip: its ground, a tick at each mark and the label beside it (see the header). */
export function drawScaleStrip(ctx: CanvasRenderingContext2D, s: ScaleStrip): void {
  ctx.globalAlpha = 0.92
  ctx.fillStyle = s.ground
  ctx.fillRect(0, s.top, s.w, s.h)
  ctx.globalAlpha = 0.7
  ctx.fillStyle = s.ink
  ctx.font = `${s.fontPx}px system-ui, sans-serif`
  ctx.textAlign = 'left'
  ctx.textBaseline = 'middle'
  // The room the last label keeps from the right edge: the FT waterfall's 26 px, or the label's own
  // width where that is wider (an RF offset such as `+12.5k`).
  let clearFrom = -Infinity
  for (const m of s.marks) {
    ctx.fillRect(m.x, s.top, s.gfx, 4 * s.gfx)
    const w = ctx.measureText(m.label).width
    const lx = Math.min(s.w - Math.max(26 * s.text, w + 2 * s.text), m.x + 2 * s.gfx)
    if (lx < clearFrom) continue
    ctx.fillText(m.label, lx, s.top + s.h / 2)
    clearFrom = lx + w + 4 * s.text
  }
  ctx.globalAlpha = 1
}
