// COVER-CROP GEOMETRY for the SSTV composer — pure, so the whole of it is testable in
// jsdom, which has no canvas at all (this project's jsdom returns null from
// `getContext('2d')` and has no `createImageBitmap`, `OffscreenCanvas` or `ImageData`).
//
// THE STATE IS A CENTRE, NOT A RECTANGLE. `{ cx, cy }` is where in the source picture the
// crop is centred, normalised 0–1. The window's SIZE is always derived from the source
// and the mode's raster, never stored. That one choice is what makes the mode-change
// behaviour fall out for free: switching Scottie 1 (320×256, 1.250) to Robot 36
// (320×240, 1.333) keeps the operator's framing intent and just re-derives the window at
// the new aspect, instead of snapping back to centre or carrying a stale rectangle that
// no longer matches the raster.
//
// ⚠️ ASPECT IS NOT ADJUSTABLE AND CANNOT BE. The raster is fixed by the mode and
// `sstv_send` refuses any mismatch (`src-tauri/src/lib.rs`, "Image is W×H but MODE needs
// …"). So there are no resize handles and no aspect control; the only freedom is WHICH
// PART. That deletes the entire "I cropped it wrong and it got refused" class of failure.

/** Where the crop window is centred in the source picture, normalised 0–1. */
export interface CropCentre {
  cx: number
  cy: number
}

/** Centre crop — what a freshly dropped picture gets, and byte-identical to what the
 *  composer did before it had a drag box at all. */
export const CENTRE: CropCentre = { cx: 0.5, cy: 0.5 }

/** The source rectangle that fills a `targetW`×`targetH` raster, in source pixels. */
export interface CropWindow {
  sx: number
  sy: number
  sw: number
  sh: number
}

/** Which axis the operator can actually drag along. Cover-fit means exactly one axis has
 *  travel — the over-long one — unless the aspects happen to match, when there is none.
 *  Drives the cursor (`ew-resize` / `ns-resize` / `move`) and the "no crop needed" note. */
export type FreeAxis = 'x' | 'y' | 'none'

/** The uniform scale that makes the source COVER the target (the larger of the two
 *  ratios — the smaller would letterbox). */
export function coverScale(srcW: number, srcH: number, targetW: number, targetH: number): number {
  if (srcW <= 0 || srcH <= 0 || targetW <= 0 || targetH <= 0) return 1
  return Math.max(targetW / srcW, targetH / srcH)
}

/** The crop window's size in SOURCE pixels: the target raster divided by the cover scale.
 *  Exactly one of these equals the source dimension; the other is smaller. */
export function windowSize(
  srcW: number,
  srcH: number,
  targetW: number,
  targetH: number,
): { cw: number; ch: number } {
  const s = coverScale(srcW, srcH, targetW, targetH)
  return {
    cw: Math.min(srcW, Math.round(targetW / s)),
    ch: Math.min(srcH, Math.round(targetH / s)),
  }
}

/** Which axis has travel for this source/target pair. */
export function freeAxis(
  srcW: number,
  srcH: number,
  targetW: number,
  targetH: number,
): FreeAxis {
  const { cw, ch } = windowSize(srcW, srcH, targetW, targetH)
  const xFree = cw < srcW
  const yFree = ch < srcH
  if (xFree && !yFree) return 'x'
  if (yFree && !xFree) return 'y'
  return 'none'
}

/** Hold the centre inside the range that keeps the window wholly within the source, so
 *  there is never a black edge or an edge-replicated pixel. On an axis that is already
 *  full the interval collapses to the single point 0.5 and the drag is inert THERE with
 *  no special case — the clamp is the special case. */
export function clampCentre(
  srcW: number,
  srcH: number,
  targetW: number,
  targetH: number,
  c: CropCentre,
): CropCentre {
  const { cw, ch } = windowSize(srcW, srcH, targetW, targetH)
  const hx = srcW > 0 ? cw / (2 * srcW) : 0.5
  const hy = srcH > 0 ? ch / (2 * srcH) : 0.5
  const cx = Number.isFinite(c.cx) ? c.cx : 0.5
  const cy = Number.isFinite(c.cy) ? c.cy : 0.5
  return {
    cx: Math.min(Math.max(cx, hx), 1 - hx),
    cy: Math.min(Math.max(cy, hy), 1 - hy),
  }
}

/**
 * The source rectangle to draw from, in source pixels.
 *
 * ⚠️ **The origin is rounded to EVEN source pixels.** The resampler is a chain of exact
 * 2:1 halvings, where bilinear sampling is arithmetically identical to a 2×2 box average
 * — but only because the destination centre `i+0.5` maps to the source pixel boundary
 * `2i+1`, giving both neighbours a weight of exactly 0.5. An odd origin shifts that phase
 * to 0.25/0.75: still a fine low-pass, but the exactness is free to keep, and a one-pixel
 * shift at 12× downscale is invisible.
 */
export function cropWindow(
  srcW: number,
  srcH: number,
  targetW: number,
  targetH: number,
  c: CropCentre,
): CropWindow {
  const { cw, ch } = windowSize(srcW, srcH, targetW, targetH)
  const cl = clampCentre(srcW, srcH, targetW, targetH, c)
  const even = (v: number, max: number) =>
    Math.max(0, Math.min(max, Math.round(v / 2) * 2))
  return {
    sx: even(cl.cx * srcW - cw / 2, Math.max(0, srcW - cw)),
    sy: even(cl.cy * srcH - ch / 2, Math.max(0, srcH - ch)),
    sw: cw,
    sh: ch,
  }
}

/**
 * Move the crop by a pointer drag, with the delta scaled through the WHOLE chain:
 * preview CSS pixels → target-raster pixels → source pixels → normalised centre.
 *
 * Getting this wrong is the classic "the drag feels three times too fast": the preview is
 * shown at an integer upscale of the raster, so a 100 px drag on a 3× preview must move
 * the same picture content as a 100 px drag on a 1× one. The image moves UNDER a fixed
 * frame, so dragging right reveals more of the picture's left — hence the sign.
 */
export function dragCentre(
  srcW: number,
  srcH: number,
  targetW: number,
  targetH: number,
  c: CropCentre,
  dxCss: number,
  dyCss: number,
  previewScale: number,
): CropCentre {
  const s = coverScale(srcW, srcH, targetW, targetH)
  const k = previewScale > 0 ? previewScale : 1
  // CSS px → raster px (÷ previewScale) → source px (÷ coverScale) → fraction (÷ src).
  const dxSrc = dxCss / k / s
  const dySrc = dyCss / k / s
  return clampCentre(srcW, srcH, targetW, targetH, {
    cx: c.cx - (srcW > 0 ? dxSrc / srcW : 0),
    cy: c.cy - (srcH > 0 ? dySrc / srcH : 0),
  })
}

/** Nudge by whole TARGET-raster pixels — the keyboard path (arrows, shift for ten).
 *  Always available: the preview is focusable, and a11y here is not optional. */
export function nudgeCentre(
  srcW: number,
  srcH: number,
  targetW: number,
  targetH: number,
  c: CropCentre,
  dxRaster: number,
  dyRaster: number,
): CropCentre {
  return dragCentre(srcW, srcH, targetW, targetH, c, dxRaster, dyRaster, 1)
}

/** True when the source already IS the raster: both clamp intervals collapse, the drag is
 *  inert, the halving chain is empty and `drawImage` is 1:1 — a pre-sized 320×256 drop is
 *  a pixel-identical passthrough. Worth saying in the UI rather than showing a dead
 *  affordance. */
export function isExactFit(
  srcW: number,
  srcH: number,
  targetW: number,
  targetH: number,
): boolean {
  return srcW === targetW && srcH === targetH
}

/** True when the picture is smaller than the raster on either axis, so it will be
 *  ENLARGED and look soft. Not a refusal: a 160×120 webcam grab is a legitimate thing to
 *  send and every other SSTV program enlarges it. It earns a persistent badge on the
 *  preview — visible at Send time, unlike a toast. */
export function isUpscale(
  srcW: number,
  srcH: number,
  targetW: number,
  targetH: number,
): boolean {
  return srcW > 0 && srcH > 0 && (srcW < targetW || srcH < targetH)
}
