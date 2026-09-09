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
  decoderApplies?: boolean | null,
): number {
  if (tag === 1) return 1
  if (orientationSwapsAxes(tag)) {
    // 5–8 swap the axes, so the decode's own dimensions are direct evidence for THIS
    // image — the strongest signal, kept as primary.
    if (intrinsic && intrinsic.w > 0 && intrinsic.h > 0) {
      const sameWayRound = decoded.w === intrinsic.w && decoded.h === intrinsic.h
      const swapped = decoded.w === intrinsic.h && decoded.h === intrinsic.w
      if (swapped && !sameWayRound) return 1 // decoder already rotated
      if (sameWayRound && !swapped) return tag // decoder left it as stored
    }
    // Dimensions ambiguous (square, scaled, or a header we could not read): fall through
    // to the engine probe, same as the non-swap cases below.
  }
  // ⭐ 2, 3, 4 (mirror-H, ROTATE-180/UPSIDE-DOWN, mirror-V) DO NOT change the dimensions,
  // so the measurement above cannot see them — which is exactly the gap that shipped a
  // Mac-only upside-down image (orientation 3): WebView2 honours `imageOrientation:'none'`
  // and the app's own matrix is correct, WebKitGTK ignores it and applies the rotation
  // itself, so the app's matrix rotated a second time. `decoderApplies` is the one-time
  // engine probe (`probeDecoderAppliesOrientation`): if the decoder applies EXIF itself,
  // the picture is already upright and our matrix must be skipped. Unknown → trust the tag,
  // which is the pre-fix behaviour and correct on the engines that do nothing.
  if (decoderApplies === true) return 1
  return tag
}

/** The 2×1 orientation-6 (rotate-90°) JPEG the probe decodes: it comes back 1×2 iff the
 *  engine applied the orientation. Kept tiny (704 B) and inline so the probe needs no
 *  network and no asset. */
const PROBE_JPEG_B64 =
  '/9j/4AAQSkZJRgABAQAAAQABAAD/4QAiRXhpZgAATU0AKgAAAAgAAQESAAMAAAABAAYAAAAAAAD/2wBDAAMCAgMCAgMDAwMEAwMEBQgFBQQEBQoHBwYIDAoMDAsKCwsNDhIQDQ4RDgsLEBYQERMUFRUVDA8XGBYUGBIUFRT/2wBDAQMEBAUEBQkFBQkUDQsNFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBT/wAARCAABAAIDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD4H8Q/8h/Uv+vmX/0M0UUV/ptkP/Ipwn/XuH/pKPAzr/kZ4r/r5P8A9KZ//9k='

let probeCache: Promise<boolean | null> | null = null

/**
 * Does this engine's decode pipeline apply EXIF orientation even when asked not to?
 *
 * A property of the ENGINE, not the image: WebView2 (Windows) honours
 * `createImageBitmap(blob, { imageOrientation: 'none' })` and returns the stored pixels;
 * WebKitGTK / WKWebView (Linux, macOS) ignores it (or the `<img>` fallback applies
 * orientation by default). Decoding the 2×1 probe and reading whether it came back 1×2
 * answers it once. Cached for the session. `null` only when it genuinely cannot decide.
 *
 * The dimension-based guard in `effectiveOrientation` already learns this for a 5–8 image,
 * but a 1–4 image gives it nothing to measure; this probe is that same knowledge, obtained
 * without waiting for the operator to happen to load a rotated photo.
 */
export function probeDecoderAppliesOrientation(): Promise<boolean | null> {
  if (probeCache) return probeCache
  probeCache = (async () => {
    const cib = (
      globalThis as { createImageBitmap?: (b: Blob, o?: unknown) => Promise<ImageBitmap> }
    ).createImageBitmap
    // No createImageBitmap → the decode falls back to an <img>, which applies orientation.
    if (!cib) return true
    try {
      const bytes = Uint8Array.from(atob(PROBE_JPEG_B64), (c) => c.charCodeAt(0))
      const blob = new Blob([bytes], { type: 'image/jpeg' })
      let bmp: ImageBitmap
      try {
        bmp = await cib(blob, { imageOrientation: 'none' })
      } catch {
        bmp = await cib(blob) // option unsupported → the default path, which applies EXIF
      }
      if (!bmp || bmp.width === 0) return null
      if (bmp.width === 1 && bmp.height === 2) return true // rotated → engine applied it
      if (bmp.width === 2 && bmp.height === 1) return false // as stored → honoured 'none'
      return null
    } catch {
      return null
    }
  })()
  return probeCache
}

/** Test seam: forget the cached probe so a case can set its own engine behaviour. */
export function __resetOrientationProbeForTest(): void {
  probeCache = null
}
