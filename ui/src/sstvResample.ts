// THE DOWNSCALE PLAN: a progressive 2:1 halving chain, then one final ≤2× step.
//
// ⚠️ WHY NOT ONE-SHOT `drawImage(src, 0,0, 320,256)`. A phone photo is 4032 px wide and
// Scottie 1 is 320 — a 12.6× reduction. A single step with any fixed small kernel reads
// two to four source pixels per destination pixel and throws away the other ~155. That is
// textbook undersampling, and it does not soften fine detail, it ALIASES it: foliage,
// brickwork, fabric weave and text turn into moiré and crawling speckle. On SSTV that is
// uniquely expensive — the analogue line scan has no error correction, so the aliasing is
// transmitted faithfully, eats the mode's real resolution, and a Scottie DX over spends
// 270 s sending it.
//
// ⚠️ WHY NOT `imageSmoothingQuality = 'high'`. It is advisory. Chromium/Skia honours it
// with a mipmap chain; WebKitGTK maps it onto Cairo's filter enum, whose downscale
// behaviour has varied across versions. Nexus ships against both engines and can pin
// neither, and betting the picture on an unspecified hint is exactly the shape of
// "worked in testing, broken for a real operator".
//
// ⭐ WHY HALVING IS RIGHT *AND PROVABLE*. At exactly 2:1, bilinear sampling is
// arithmetically identical to a 2×2 box average: the destination centre `i+0.5` maps to
// source `2i+1`, precisely the boundary between source pixels `2i` and `2i+1`, so both
// axes get weights 0.5/0.5 and the result is the exact mean of the four covered pixels.
// `n` halvings therefore give a true 2ⁿ×2ⁿ box pyramid INDEPENDENT of whose bilinear it
// is, because every engine agrees on that degenerate case. That converts a quality
// *hint* into an *identity*.
//
// Three requirements or the identity breaks, and all three are the caller's to keep:
//   1. `ctx.imageSmoothingEnabled = true` set EXPLICITLY on every intermediate context.
//      With smoothing off, "bilinear" is nearest-neighbour and the chain becomes 4:1
//      decimation — strictly worse than the naive one-shot.
//   2. CROP BEFORE SCALING, and scale only the cropped region, or the halvings average
//      across pixels that were going to be cropped away. Fold the crop into the first
//      step's source rect.
//   3. The crop origin is rounded to even source pixels (`sstvCrop.cropWindow`), so the
//      2:1 phase stays exact.

/** One step of the plan: draw the previous stage into a canvas of this size. */
export interface ResampleStep {
  w: number
  h: number
}

/**
 * The intermediate sizes to draw through, ending at exactly `targetW`×`targetH`.
 *
 * Halve while both axes still have a factor of two or more left, then finish with one
 * step for the residual ≤2× factor, where bilinear reads at least a quarter of the
 * contributing pixels and is adequate.
 *
 * Worked example — a 4032×3024 phone photo to Scottie 1 (320×256). The cover crop is
 * 3779×3024, and the plan is `3779×3024 → 1889×1512 → 944×756 → 472×378 → 320×256`:
 * three halvings and one final step, four `drawImage` calls, largest intermediate
 * 1889×1512. Sub-frame work, run once on load and once per mode change.
 *
 * An exact fit (a pre-sized 320×256 drop) returns just the target: the chain is empty and
 * the single draw is 1:1, a pixel-identical passthrough. An UPSCALE returns just the
 * target too — one bilinear enlargement, because blocky is worse than soft on an
 * analogue line scan.
 */
export function halvingChain(
  srcW: number,
  srcH: number,
  targetW: number,
  targetH: number,
): ResampleStep[] {
  const steps: ResampleStep[] = []
  if (
    !Number.isFinite(srcW) ||
    !Number.isFinite(srcH) ||
    srcW <= 0 ||
    srcH <= 0 ||
    targetW <= 0 ||
    targetH <= 0
  ) {
    return [{ w: Math.max(1, targetW), h: Math.max(1, targetH) }]
  }
  let w = Math.round(srcW)
  let h = Math.round(srcH)
  // ⚠️ HALVE PER AXIS, not in lockstep. In normal use the input IS the crop window, whose
  // aspect equals the target's, so both axes halve together and the distinction never
  // shows. It shows the moment they do not: a 4000×300 panorama into 320×256 has 12.5×
  // to give on the width and nothing on the height, and a both-axes guard would refuse to
  // halve at all and hand the whole 12.5× reduction to one bilinear step — the exact
  // aliasing this module exists to prevent. An axis that is already close enough is left
  // at 1:1 for that step, which is a plain row copy and filters nothing.
  for (;;) {
    const nw = Math.floor(w / 2) >= targetW ? Math.floor(w / 2) : w
    const nh = Math.floor(h / 2) >= targetH ? Math.floor(h / 2) : h
    if (nw === w && nh === h) break
    w = nw
    h = nh
    steps.push({ w, h })
  }
  const last = steps[steps.length - 1]
  if (!last || last.w !== targetW || last.h !== targetH) {
    steps.push({ w: targetW, h: targetH })
  }
  return steps
}

/** How many pixels the largest intermediate stage holds — what the chain actually costs
 *  in memory. Used by the composer to keep the drag re-render honest about its budget. */
export function peakStagePixels(steps: ResampleStep[]): number {
  return steps.reduce((m, s) => Math.max(m, s.w * s.h), 0)
}
