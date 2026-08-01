// Map label de-collision — the band-map's two-pass push-down/compress-up
// (BandMap.tsx) adapted to a 2-D canvas: labels live at arbitrary x, so only
// labels whose x-extents actually intersect contend for vertical space. Pure
// (no canvas, no DOM) so the layout is unit-testable; the caller measures text
// widths and draws at the returned centers.

/** One label's anchor: x = left edge, w = measured width, y = CENTER line
 * (canvas textBaseline "middle"). */
export interface LabelBox {
  x: number
  w: number
  y: number
}

/**
 * Resolve overlaps among label boxes; returns the adjusted center-y per input
 * index (input order preserved — anchors keep their identity). `rowH` is the
 * vertical footprint one label claims; `top`/`bottom` bound the allowed
 * centers (canvas edges inset by half a row).
 *
 * Pass 1 (top → bottom): a label landing on an x-overlapping earlier label
 * slides DOWN below it. Pass 2 (bottom → top): anything pushed past `bottom`
 * clamps there and compresses its column back up — the band-map's exact
 * shape, with `top` as the hard stop for the truly-overfull case (accepting
 * overlap then, like the band map, rather than losing a label off-canvas).
 */
export function decollideLabels(
  boxes: readonly LabelBox[],
  rowH: number,
  top: number,
  bottom: number,
): number[] {
  const y = boxes.map((b) => b.y)
  const order = boxes.map((_, i) => i).sort((a, b) => boxes[a].y - boxes[b].y)
  const xOverlap = (a: number, b: number) =>
    boxes[a].x < boxes[b].x + boxes[b].w && boxes[b].x < boxes[a].x + boxes[a].w
  for (let k = 1; k < order.length; k++) {
    const i = order[k]
    for (let j = 0; j < k; j++) {
      const p = order[j]
      if (xOverlap(i, p) && y[i] - y[p] < rowH) y[i] = y[p] + rowH
    }
  }
  for (let k = order.length - 1; k >= 0; k--) {
    const i = order[k]
    if (y[i] > bottom) y[i] = bottom
    for (let j = k - 1; j >= 0; j--) {
      const p = order[j]
      if (xOverlap(p, i) && y[i] - y[p] < rowH) y[p] = Math.max(top, Math.min(y[p], y[i] - rowH))
    }
  }
  return y
}
