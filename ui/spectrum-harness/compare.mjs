// The pixel comparator: how far a rendered fixture may drift from its stored picture.

/**
 * A pixel DIFFERS when any channel is more than `channel` apart, and two pictures MATCH when no
 * more than `maxFraction` of their pixels differ. The slack is for the rasteriser, not the
 * renderer: anti-aliased trace edges move a little between Chrome builds, while a palette, a
 * level, a span or a row slip moves most of the picture.
 */
export const TOLERANCE = { channel: 16, maxFraction: 0.005 }

/** Compare two `{ width, height, rgba }` pictures. Also returns a diff image: differing pixels
 *  red, the rest the expected picture dimmed, so a failure can be read at a glance. */
export function comparePixels(actual, expected, tol = TOLERANCE) {
  if (actual.width !== expected.width || actual.height !== expected.height) {
    return {
      match: false,
      reason: `size ${actual.width}x${actual.height}, stored ${expected.width}x${expected.height}`,
      differing: actual.width * actual.height,
      fraction: 1,
      maxDelta: 255,
      diff: null,
    }
  }
  const n = actual.width * actual.height
  const diff = new Uint8Array(n * 4)
  let differing = 0
  let maxDelta = 0
  for (let i = 0; i < n; i++) {
    const o = i * 4
    let d = 0
    for (let c = 0; c < 4; c++) d = Math.max(d, Math.abs(actual.rgba[o + c] - expected.rgba[o + c]))
    if (d > maxDelta) maxDelta = d
    if (d > tol.channel) {
      differing++
      diff.set([255, 0, 0, 255], o)
    } else {
      for (let c = 0; c < 3; c++) diff[o + c] = expected.rgba[o + c] >> 2
      diff[o + 3] = 255
    }
  }
  const fraction = differing / n
  const match = fraction <= tol.maxFraction
  return {
    match,
    reason: match ? '' : `${differing} px (${(fraction * 100).toFixed(2)}%) differ by more than ${tol.channel}`,
    differing,
    fraction,
    maxDelta,
    diff,
  }
}
