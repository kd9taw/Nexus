// INTEGER-STEP UPSCALE for the SSTV live decode (census growers.md #9, 2026-07-30).
//
// The in-flight decode is a low-resolution bitmap drawn at its native pixel size and
// upscaled by CSS. The old rule was a fixed `min(100%, 480px)` — a deliberate 3× crisp
// step for the ≤160px preview — which on an ultrawide left ~2900px of dead width around
// a postage stamp, in the mode whose entire point is the picture. The design ruling from
// the census: widen in WHOLE multiples only. `image-rendering: pixelated` is crisp at an
// integer scale (every source pixel becomes an exact k×k block) and smeary at a
// fractional one (rows of pixels land on half-device-pixel boundaries), so blur is worse
// than margin.
//
// This is the pure half; SstvView measures the stage (ResizeObserver, 0×0-guarded like
// useRegionCols) and stamps `--sstv-img-w: k·nativeW px` on the canvas, whose CSS keeps
// a `min(100%, …)` yield so the one unavoidable fractional case — a stage smaller than
// the native decode — shrinks-to-fit instead of clipping.

/**
 * Largest whole multiple `k` (1 ≤ k ≤ cap) of a `nativeW`×`nativeH` decode that fits an
 * `availW`×`availH` stage on BOTH axes. Never 0 and never fractional: a stage too small
 * for even 1× still returns 1 — the CSS `min(100%)` guard owns that degenerate case.
 * The 6× cap is taste, not geometry: past it the picture reads as a wall of blocks and
 * the surplus is better left as margin.
 */
export function integerScaleStep(
  nativeW: number,
  nativeH: number,
  availW: number,
  availH: number,
  cap = 6,
): number {
  if (!Number.isFinite(nativeW) || nativeW <= 0) return 1
  if (!Number.isFinite(nativeH) || nativeH <= 0) return 1
  if (!Number.isFinite(availW) || !Number.isFinite(availH)) return 1
  const k = Math.min(cap, Math.floor(availW / nativeW), Math.floor(availH / nativeH))
  return Math.max(1, k)
}

/**
 * The CSS pixel width SstvView stamps as `--sstv-img-w`: `integerScaleStep`'s whole
 * multiple whenever at least 1× fits, with ONE deliberate exception — a stage SHORTER
 * than the native decode. The CSS `min(100%, …)` yield covers only the width axis;
 * `height: auto` then follows the canvas ratio into the stage's `overflow: hidden`, so
 * a too-short stage clipped the picture top and bottom (review 2026-07-31). Since the
 * measured height is known here, that case returns the fractional width whose
 * ratio-derived height exactly fits — the same shrink-to-fit-beats-clipping ruling the
 * width axis already has. `availH <= 0` (no measurable room at all) keeps 1× native and
 * leaves the width yield to do what it can.
 */
export function sstvImageWidth(
  nativeW: number,
  nativeH: number,
  availW: number,
  availH: number,
  cap = 6,
): number {
  const k = integerScaleStep(nativeW, nativeH, availW, availH, cap)
  if (k === 1 && nativeH > availH && availH > 0) {
    return Math.max(1, Math.floor((availH * nativeW) / nativeH))
  }
  return k * nativeW
}
