// RECEIVER MARKERS, FILTER EDGES AND THE NOTCH — the pure half of what the Phone/CW scope draws
// over its picture and lets the operator grab. No React and no canvas state: PhoneScope asks these
// where a mark sits on the axis it is drawing and what a gesture or a key means, and the overlay
// draws through `drawPassband` / `drawNotch`.
//
// ⚠️ A WIDTH IS ALL THE RADIO TAKES. Hamlib's `M <mode> <passband>` and an Icom's `1A 03` set the
// filter's WIDTH, never where it sits, so an SSB passband keeps its near edge on the carrier and
// only the FAR edge moves (`draggableEdges`); CW, AM and FM sit centred, and both edges move
// together, mirrored. The passband's shape is `tuneSnap.boxEdges`' — the drag box's — so the box
// a drag shows and the marker it leaves agree to the hertz. Neither knows the rig's low cut or IF
// shift, so on SSB both sit up to a few hundred hertz below the skirt the scope shows; that is a
// fact of the protocols above, not a rounding here.
//
// ⚠️ THE LIMITS ARE THE COCKPIT'S ± STEPPER'S. The edge, the keys and the stepper read ONE table
// (`PASSBAND_LIMITS`), so no gesture can command a width the stepper could not; the radio then
// takes the nearest filter it has (the native Icom path clamps to its own table in the backend).
import { boxEdges } from '../tuneSnap'
import { isSymmetricMode, sidebandSign } from '../waterfall'

/** The widths a cockpit commands and the step a key moves by. */
export interface PassbandLimits {
  minHz: number
  maxHz: number
  stepHz: number
}

/** Phone 300–4000 Hz in 100s, CW 50–2000 Hz in 50s: the ± steppers' own range and step. */
export const PASSBAND_LIMITS: Record<'phone' | 'cw', PassbandLimits> = {
  phone: { minHz: 300, maxHz: 4000, stepHz: 100 },
  cw: { minHz: 50, maxHz: 2000, stepHz: 50 },
}

/** A width made legal: rounded to the step, inside the range. A width that is not a number is the
 *  range's floor rather than NaN on the wire. */
export function clampPassband(hz: number, l: PassbandLimits): number {
  if (!Number.isFinite(hz)) return l.minHz
  const stepped = Math.round(hz / l.stepHz) * l.stepHz
  return Math.min(l.maxHz, Math.max(l.minHz, stepped))
}

/** One step narrower (`dir` −1) or wider (+1) from `widthHz`, legal. Never the wrong way: at a rail
 *  "wider" cannot narrow (a width above the range, say from another mode, stays where it is). */
export function stepPassband(widthHz: number, dir: 1 | -1, l: PassbandLimits): number {
  const next = clampPassband(widthHz + dir * l.stepHz, l)
  return (dir > 0 && next <= widthHz) || (dir < 0 && next >= widthHz) ? widthHz : next
}

export type Edge = 'lo' | 'hi'

const symmetric = (sideband: string) => {
  const m = sideband.trim().toUpperCase()
  return m.startsWith('CW') || isSymmetricMode(m)
}

/** The edges a drag may move. SSB: only the far one (USB's top, LSB's bottom) — the near edge is
 *  the carrier. CW, AM, FM: both. */
export function draggableEdges(sideband: string): Edge[] {
  if (symmetric(sideband)) return ['lo', 'hi']
  return sidebandSign(sideband) > 0 ? ['hi'] : ['lo']
}

/** What the scope is drawing, as far as a mark's place on it goes. */
export interface AxisKind {
  /** A native RF row (Flex, CI-V, FT-710): the axis is absolute RF Hz. */
  rf: boolean
  /** Phone's audio axis: RF offset from the dial, the dial at 0. Otherwise an audio row is in
   *  plain audio Hz (CW's window around the pitch). */
  carrierCentered: boolean
  sideband: string
  /** The live dial (absolute Hz); needed on an RF row. */
  dialHz: number | null
  /** CW sidetone pitch (Hz). */
  pitchHz: number
  /** CW: the dial reads the signal (true CW). False = the soundcard keyer's SSB carrier. */
  cwPitchRefDial: boolean
}

/** A passband on the drawn axis: its edges and its anchor, the point a width is measured from
 *  (the carrier on SSB, the centre otherwise). */
export interface AxisPassband {
  lo: number
  hi: number
  anchor: number
}

/** Where a passband of `widthHz` sits on the drawn axis — or null where the axis has no honest
 *  place for one: a demodulated AM/FM baseband (no RF mapping, the click's own rule), an RF row
 *  with the dial unknown, or no width. */
export function passbandOnAxis(a: AxisKind, widthHz: number): AxisPassband | null {
  if (!(widthHz > 0)) return null
  const m = a.sideband.trim().toUpperCase()
  const cw = m.startsWith('CW')
  let anchor: number
  if (a.rf) {
    if (a.dialHz == null || !(a.dialHz > 0)) return null
    // The soundcard keyer rides SSB: its tone is heard at the pitch from a dial sign×pitch away.
    anchor = a.dialHz + (cw && !a.cwPitchRefDial ? sidebandSign(a.sideband) * a.pitchHz : 0)
  } else if (isSymmetricMode(m)) {
    return null
  } else if (!a.carrierCentered && !cw) {
    // A plain audio axis: SSB audio runs up from 0 on either sideband.
    return { lo: 0, hi: widthHz, anchor: 0 }
  } else {
    // The carrier-centred axis puts the dial at 0; CW's audio window centres on the pitch.
    anchor = a.carrierCentered ? 0 : a.pitchHz
  }
  const e = boxEdges(anchor, a.sideband, widthHz)
  return { lo: e.loHz, hi: e.hiHz, anchor }
}

/** The width that puts `edge` at `axisHz` — the inverse of `passbandOnAxis` for one edge. On a
 *  plain audio axis an SSB passband grows upward whatever the sideband. Unclamped. */
export function widthForEdge(a: AxisKind, p: AxisPassband, edge: Edge, axisHz: number): number {
  if (symmetric(a.sideband)) return 2 * (edge === 'hi' ? axisHz - p.anchor : p.anchor - axisHz)
  if (!a.rf && !a.carrierCentered) return axisHz - p.anchor
  return edge === 'hi' ? axisHz - p.anchor : p.anchor - axisHz
}

/** The edges a drag may move on this axis: `draggableEdges`, except that a plain audio axis
 *  draws every SSB passband growing upward. */
export function edgesOnAxis(a: AxisKind): Edge[] {
  if (!a.rf && !a.carrierCentered && !symmetric(a.sideband)) return ['hi']
  return draggableEdges(a.sideband)
}

/** The grabbable edge within `tolPx` of `x` (all CSS px), nearest first; null when none is. */
export function edgeNear(x: number, loPx: number, hiPx: number, edges: Edge[], tolPx: number): Edge | null {
  let best: Edge | null = null
  let bestD = Infinity
  for (const e of edges) {
    const d = Math.abs(x - (e === 'lo' ? loPx : hiPx))
    if (d <= tolPx && d < bestD) {
      best = e
      bestD = d
    }
  }
  return best
}

/** Where a MANUAL notch at `audioHz` (the rig's audio frequency) sits on the drawn axis — null for
 *  AM/FM, whose audio maps to both sides of the carrier, and for an RF row with no dial. */
export function notchOnAxis(a: AxisKind, audioHz: number): number | null {
  if (!Number.isFinite(audioHz) || audioHz <= 0) return null
  const m = a.sideband.trim().toUpperCase()
  if (isSymmetricMode(m)) return null
  const s = sidebandSign(a.sideband)
  if (a.rf) {
    if (a.dialHz == null || !(a.dialHz > 0)) return null
    // True CW: a tone at the pitch is a signal ON the dial.
    const off = m.startsWith('CW') && a.cwPitchRefDial ? audioHz - a.pitchHz : audioHz
    return a.dialHz + s * off
  }
  return a.carrierCentered ? s * audioHz : audioHz
}

/** A key's tuning step for a view `spanHz` wide: about a hundredth of it, on a 1-2-5 ladder, so a
 *  press moves the passband a visible but small distance on every scope from CW's 800 Hz window to
 *  a ±250 kHz panadapter. */
export function keyStepHz(spanHz: number): number {
  const raw = Math.abs(spanHz) / 100
  if (!(raw >= 1)) return 1
  const p = Math.pow(10, Math.floor(Math.log10(raw)))
  const n = raw / p
  return (n < 1.5 ? 1 : n < 3.5 ? 2 : n < 7.5 ? 5 : 10) * p
}

/** Colours, by receiver. Main is the dial line's own white; the Sub is cyan, so two passbands on
 *  one panadapter can never be read as each other's. */
export const MARK_RGB = { main: '255, 255, 255', sub: '64, 200, 255' } as const

/** Draw a passband: a light fill between its edges, and the edges themselves — heavier, with a
 *  grab tab at the top, where they can be dragged. `x` maps axis Hz to device px. */
export function drawPassband(
  ctx: CanvasRenderingContext2D,
  x: (axisHz: number) => number,
  heightPx: number,
  p: AxisPassband,
  rgb: string,
  grabbable: Edge[],
  scale: number,
): void {
  const l = x(p.lo)
  const r = x(p.hi)
  ctx.fillStyle = `rgba(${rgb}, 0.09)`
  ctx.fillRect(Math.min(l, r), 0, Math.max(1, Math.abs(r - l)), heightPx)
  for (const e of ['lo', 'hi'] as const) {
    const ex = Math.round(e === 'lo' ? l : r)
    const grab = grabbable.includes(e)
    ctx.strokeStyle = `rgba(${rgb}, ${grab ? 0.85 : 0.4})`
    ctx.lineWidth = Math.max(1, (grab ? 2 : 1) * scale)
    ctx.beginPath()
    ctx.moveTo(ex, 0)
    ctx.lineTo(ex, heightPx)
    ctx.stroke()
    if (grab) {
      ctx.fillStyle = `rgba(${rgb}, 0.85)`
      const w = 3 * scale
      ctx.fillRect(ex - w, 0, 2 * w, 6 * scale)
    }
  }
}

/** Draw the manual notch as a narrow band through the whole picture, trace and waterfall. */
export function drawNotch(ctx: CanvasRenderingContext2D, xPx: number, heightPx: number, scale: number): void {
  const w = Math.max(2, 3 * scale)
  ctx.fillStyle = 'rgba(255, 96, 96, 0.45)'
  ctx.fillRect(Math.round(xPx - w / 2), 0, w, heightPx)
}
