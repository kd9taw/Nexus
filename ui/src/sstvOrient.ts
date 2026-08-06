// EXIF orientation → canvas transform. The eight-row table and nothing else.
//
// Apply the matrix, then `drawImage(src, 0, 0)`. `w`/`h` in the result are the size the
// DESTINATION canvas must be — swapped for orientations 5–8, which is the half that gets
// forgotten and is exactly why a portrait phone photo comes out sideways.
//
// Cross-checked entry for entry against an independent implementation: the `image` crate
// 0.25.5, `src/metadata.rs::Orientation::from_exif` — 1 NoTransforms, 2 FlipHorizontal,
// 3 Rotate180, 4 FlipVertical, 5 Rotate90FlipH, 6 Rotate90, 7 Rotate270FlipH,
// 8 Rotate270. 5 and 7 come essentially only from mirrored front-camera pipelines and
// some scanners, but they cost one table row each, so there is no reason to ship 4 of 8.

import type { ExifOrientation } from './sstvExif'

/** A 2D affine for `ctx.setTransform(a, b, c, d, e, f)`, with the destination canvas
 *  size that goes with it. */
export interface OrientTransform {
  /** Destination canvas width (source height for orientations 5–8). */
  w: number
  /** Destination canvas height (source width for orientations 5–8). */
  h: number
  /** `setTransform` arguments, in order: a, b, c, d, e, f. */
  m: [number, number, number, number, number, number]
}

/**
 * The transform that puts a `srcW`×`srcH` image upright, for the given EXIF orientation.
 *
 * `orientation` outside 1–8 is treated as 1 — the same thing `Orientation::from_exif`
 * does with a value it cannot map, and the right answer for the scanners that write a
 * garbage tag.
 */
export function orientTransform(
  orientation: number,
  srcW: number,
  srcH: number,
): OrientTransform {
  const W = srcW
  const H = srcH
  switch (orientation as ExifOrientation) {
    case 2: // mirror horizontal
      return { w: W, h: H, m: [-1, 0, 0, 1, W, 0] }
    case 3: // rotate 180
      return { w: W, h: H, m: [-1, 0, 0, -1, W, H] }
    case 4: // mirror vertical
      return { w: W, h: H, m: [1, 0, 0, -1, 0, H] }
    case 5: // mirror horizontal + rotate 270 CW (transpose)
      return { w: H, h: W, m: [0, 1, 1, 0, 0, 0] }
    case 6: // rotate 90 CW — THE iPhone PORTRAIT CASE
      return { w: H, h: W, m: [0, 1, -1, 0, H, 0] }
    case 7: // mirror horizontal + rotate 90 CW (anti-transpose)
      return { w: H, h: W, m: [0, -1, -1, 0, H, W] }
    case 8: // rotate 270 CW (= 90 CCW)
      return { w: H, h: W, m: [0, -1, 1, 0, 0, W] }
    default: // 1, and anything unrecognised
      return { w: W, h: H, m: [1, 0, 0, 1, 0, 0] }
  }
}

/** True when the orientation swaps the axes (5–8) — the case that changes the picture's
 *  ASPECT RATIO, and therefore the cover-crop window, the drag bounds and the
 *  "is this narrower than the target" branch. This is why orientation is applied first,
 *  before anything measures the image. */
export function orientationSwapsAxes(orientation: number): boolean {
  return orientation >= 5 && orientation <= 8
}

/**
 * ⭐ THE DOUBLE-ROTATION GUARD, and it is a measurement rather than a browser-version bet.
 *
 * For orientations 5–8 the file's own header carries the UNROTATED dimensions
 * (`readIntrinsicSize`). If the decoder handed back dimensions that are swapped relative
 * to those, the engine already applied the orientation itself and applying our matrix
 * too would rotate the picture twice. Returns the orientation to actually apply: the tag,
 * or 1 when the decode has already done the work.
 *
 * `intrinsic` null (a format whose header we do not parse, or a truncated one) → trust
 * the tag, which is the behaviour that is correct on the engines that do nothing.
 */
export function effectiveOrientation(
  tag: number,
  intrinsic: { w: number; h: number } | null,
  decoded: { w: number; h: number },
): number {
  if (!orientationSwapsAxes(tag)) return tag
  if (!intrinsic || intrinsic.w <= 0 || intrinsic.h <= 0) return tag
  const sameWayRound = decoded.w === intrinsic.w && decoded.h === intrinsic.h
  const swapped = decoded.w === intrinsic.h && decoded.h === intrinsic.w
  // Only a clean swap is evidence; anything else (a decoder that scaled, a header we
  // misread) falls back to trusting the tag.
  if (swapped && !sameWayRound) return 1
  return tag
}
