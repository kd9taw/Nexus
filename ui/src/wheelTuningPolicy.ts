import { bandLabelForMhz, bandRangeForLabel } from './band'

/** Shared native wheel policy: a burst parks at a band edge. Leaving that edge
 * requires a new step from a dial already acknowledged by the station. */
export function clampWheelTarget(hz: number, fromHz: number, committedHz: number | null): { hz: number; hitEdge: boolean } {
  const range = bandRangeForLabel(bandLabelForMhz(fromHz / 1e6))
  if (!range) return { hz, hitEdge: false }
  const lo = Math.round(range.lo * 1e6), hi = Math.round(range.hi * 1e6)
  const edge = hz < lo ? lo : hz > hi ? hi : null
  if (edge == null || (fromHz === edge && fromHz === committedHz)) return { hz, hitEdge: false }
  return { hz: edge, hitEdge: true }
}

/** #273 — move `steps` whole steps of `stepHz` from `fromHz`, the FIRST one landing on the step
 * grid, the way a rig's VFO rounds on its first click. From 14.110.250 at 1 kHz: +1 → 14.111.000,
 * −1 → 14.110.000, +10 → 14.120.000 (rounded, then nine more). A dial already on the grid moves
 * exactly `steps × stepHz`. Integer Hz in, integer Hz out. */
export function stepFrom(fromHz: number, steps: number, stepHz: number): number {
  if (steps === 0 || !(stepHz > 0)) return fromHz
  const off = ((fromHz % stepHz) + stepHz) % stepHz
  if (off === 0) return fromHz + steps * stepHz
  const dir = Math.sign(steps)
  const first = dir > 0 ? fromHz - off + stepHz : fromHz - off
  return first + (steps - dir) * stepHz
}
