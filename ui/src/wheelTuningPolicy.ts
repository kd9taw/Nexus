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
