// The retained history both renderers draw from: the DATA, never pixels. Ported concept from
// AetherSDR's WaterfallHistoryBuffer (GPLv3), as `../waterfallHistory.ts` is, reimplemented here
// for frames at their native resolution. See NOTICE.
//
// What differs from `waterfallHistory.ts`, and why:
//  * A row keeps the frame's own VALUES (float, its own bin count), not 8-bit intensities resampled
//    to a fixed column count. A 2048-bin Flex row is not max-pooled into 1024 columns on the way in,
//    and the average detector can still take the power mean of the bins a pixel covers.
//  * A row keeps the display range it was committed in, beside its span. The picture a row was
//    committed with is therefore the picture every later redraw gives it (today's behaviour: the
//    old ring baked the range in at push time), while the values stay exact.
//  * One ring serves both renderers. WebGL2 mirrors it into a texture one row per commit and
//    re-uploads it whole after a lost context comes back, which is why a restore keeps the history;
//    canvas-2D rebuilds its band from it on a cold change. Neither keeps history of its own.
//
// Per-row frequency frames are what make history reprojection free: every redraw maps each row
// from its OWN span onto the current view, so a retune or a zoom never needs history rewritten.

import { safeDbPerUnit } from './aggregate'
import type { DisplayRange, RowFrame, SpectrumFrame } from './types'

/** Default rows kept. 2048 rows of a 2048-bin source is the case the renderer is budgeted at:
 *  16 MB here, 8 MB as half floats on the GPU. A 512-bin audio ring is a quarter of that. */
export const DEFAULT_DEPTH = 2048
/** Widest row stored. A wider frame is max-pooled to this at push (a carrier never vanishes). */
export const MAX_BINS = 4096

/** Per-row metadata layout in `meta`. */
export const M_LO = 0
export const M_HI = 1
export const M_N = 2
export const M_DB_PER_UNIT = 3
export const M_FLOOR = 4
export const M_CEIL = 5
export const M_T = 6
export const M_SEQ = 7
export const META = 8

/**
 * Copy `src` into `n` values of `dst` from `at`: as-is when it has `n` bins, else max-pooled onto
 * `n` cells (each cell takes the peak of the bins that fall in it, so a one-bin carrier survives
 * the squeeze). Non-finite values are stored as 0 (the old ring's 8-bit store did the same), so no
 * NaN ever reaches a max or a shader.
 */
export function copyBins(src: ArrayLike<number>, dst: Float32Array, at: number, n: number): void {
  const len = src.length
  if (len === n) {
    for (let i = 0; i < n; i++) {
      const v = src[i]
      dst[at + i] = Number.isFinite(v) ? v : 0
    }
    return
  }
  for (let c = 0; c < n; c++) {
    const i0 = Math.floor((c * len) / n)
    const i1 = Math.max(i0 + 1, Math.floor(((c + 1) * len) / n))
    let m = -Infinity
    for (let i = i0; i < i1; i++) {
      const v = src[i]
      if (Number.isFinite(v) && v > m) m = v
    }
    dst[at + c] = m === -Infinity ? 0 : m
  }
}

export class SpectrumRing {
  readonly depth: number
  /** Column capacity: doubled until the widest row so far fits (at most `MAX_BINS`). */
  cols: number
  /** `depth` rows of `cols` values; row `r` starts at `r * cols` and holds `meta[r*META + M_N]`. */
  values: Float32Array
  readonly meta: Float64Array
  /** Ring index of the newest row (meaningless while `count` is 0). */
  head = 0
  count = 0
  /** Rows ever committed: the newest row is serial `serial - 1`. Lets a renderer tell "k new rows
   *  arrived" from "the view moved". */
  serial = 0
  /** Bumped when the storage is replaced (wider rows) or emptied: everything drawn is stale. */
  generation = 0

  constructor(depth = DEFAULT_DEPTH, cols = 512) {
    this.depth = Math.max(2, depth | 0)
    this.cols = Math.max(1, Math.min(MAX_BINS, cols | 0))
    this.values = new Float32Array(this.depth * this.cols)
    this.meta = new Float64Array(this.depth * META)
  }

  /** Append one row (see `copyBins` for what a non-finite or an over-wide frame becomes). Returns
   *  its ring index. */
  push(frame: SpectrumFrame, range: DisplayRange): number {
    let n = frame.bins.length
    if (n > MAX_BINS) n = MAX_BINS
    if (n > this.cols) this.widen(n)
    const r = this.count === 0 ? 0 : (this.head + 1) % this.depth
    copyBins(frame.bins, this.values, r * this.cols, n)
    const m = r * META
    this.meta[m + M_LO] = frame.loHz
    this.meta[m + M_HI] = frame.hiHz
    this.meta[m + M_N] = n
    this.meta[m + M_DB_PER_UNIT] = safeDbPerUnit(frame.dbPerUnit)
    this.meta[m + M_FLOOR] = range.floor
    this.meta[m + M_CEIL] = range.ceil
    this.meta[m + M_T] = frame.tMs
    this.meta[m + M_SEQ] = frame.seq
    this.head = r
    if (this.count < this.depth) this.count++
    this.serial++
    return r
  }

  /** Ring index of the row `age` back (0 = newest), or -1 out of range. */
  indexAt(age: number): number {
    if (!(age >= 0) || age >= this.count) return -1
    const r = this.head - Math.floor(age)
    return r < 0 ? r + this.depth : r
  }

  frameAt(age: number): RowFrame | null {
    const r = this.indexAt(age)
    if (r < 0) return null
    const m = r * META
    return { loHz: this.meta[m + M_LO], hiHz: this.meta[m + M_HI], tMs: this.meta[m + M_T], seq: this.meta[m + M_SEQ] }
  }

  clear(): void {
    this.count = 0
    this.head = 0
    this.generation++
  }

  /** Grow the column capacity, keeping every stored row. */
  private widen(n: number): void {
    let cols = this.cols
    while (cols < n) cols *= 2
    cols = Math.min(MAX_BINS, cols)
    const next = new Float32Array(this.depth * cols)
    for (let r = 0; r < this.depth; r++) {
      const keep = this.meta[r * META + M_N]
      next.set(this.values.subarray(r * this.cols, r * this.cols + keep), r * cols)
    }
    this.values = next
    this.cols = cols
    this.generation++
  }
}
