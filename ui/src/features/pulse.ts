/**
 * The live "this band is open RIGHT NOW" breath both map surfaces share.
 *
 * ONE module on purpose (2D↔3D parity is a stated contract): the 2-D map fixed
 * the frozen-sine bug — feeding the pulse from the 60 s greyline tick lands the
 * sine on an arbitrary value once a minute, so an OPEN band could render dimmer
 * than a closed one — and left a warning comment; the 3-D globe then shipped
 * the exact value the comment warns against. Callers pass LIVE wall time
 * (`Date.now()`), never a slow-tick snapshot, and re-draw on a gated 1 s tick
 * (only while something is actually open, skipped when `document.hidden`).
 */

/** Heat-blob breath for an open band: 0.4–1.0, ~2.8 s period. */
export function heatPulse(nowMs: number): number {
  return 0.7 + 0.3 * Math.sin(nowMs / 450)
}

/** Brightness for a spot: breathing when its band is open, steady when not. */
export function heatBoost(open: boolean, nowMs: number): number {
  return open ? heatPulse(nowMs) : 0.55
}

/** Opening-sector breath: 0.5–1.0, slower (~4.4 s) than the heat blobs. */
export function sectorPulse(nowMs: number): number {
  return 0.75 + 0.25 * Math.sin(nowMs / 700)
}
