// Retained waterfall history — the data model behind the scroll (ported concept from
// AetherSDR's WaterfallHistoryBuffer, GPLv3; reimplemented for Nexus's normalized rows).
//
// The old scroll was pixel-based: getImageData/putImageData shifting the canvas each row,
// which (a) forces the canvas CPU-backed (willReadFrequently) because a GPU readback per
// row stalls the main thread, (b) loses everything above the viewport (no scrollback),
// (c) bakes the palette into history (a palette switch only affects NEW rows), and
// (d) smears on resize/zoom because old pixels can only be stretched, not re-derived.
//
// This model retains the DATA instead: a ring of 8-bit normalized-intensity rows, each
// stamped with its frequency window {loHz, hiHz} and a timestamp. The visible viewport is
// (re)rendered FROM DATA into a retained RGBA buffer — scrolled with copyWithin on the hot
// path (no canvas readback; the canvas becomes write-only), and fully rebuilt only on the
// cold paths (palette change, zoom, resize, scrollback), which is exactly when a rebuild
// is wanted: instant palette recolor of accumulated history, smear-free zoom/resize, and
// pause + scrollback with honest per-row frequency mapping across retunes.
//
// Pure TS + typed arrays, no DOM: unit-tested independently of the canvas.
//
// Both the store (`push`) and the re-render (`renderInto`) go through `resampleRow`, the
// SAME bin→pixel mapping the live bottom-row path uses. They used to disagree: the live
// path interpolated, these two point-sampled, so a palette switch / zoom / resize / pause
// repainted the whole accumulated waterfall as hard-edged rectangles — the operator's
// "looks so 8 bit" (2026-08-03).

import { resampleRow } from './waterfall'

/** One stored row's metadata. */
export interface RowFrame {
  /** Frequency window this row's columns span (Hz — audio-passband, absolute RF, or the
   * dial-relative OFFSET the rig scope's carrier-centered axis stores, which is the one
   * frame that can be negative: an LSB row is stored mirrored, so it spans −hi..−lo). */
  loHz: number
  hiHz: number
  /** Wall-clock stamp (ms) — drives the scrollback time tape. */
  tsMs: number
}

/** Default history depth (rows). ~2048 rows × 1024 cols = 2 MB of Uint8 — at the FT8
 * cadence (~8 rows/s) that is ~4 minutes; at PhoneScope's 20 Hz, ~100 seconds. */
export const DEFAULT_DEPTH = 2048

export class WaterfallHistory {
  private readonly depth: number
  private readonly cols: number
  private data: Uint8Array
  private frames: Float64Array // [loHz, hiHz, tsMs] × depth
  private head = 0 // next write index (ring)
  private count = 0 // rows stored (≤ depth)
  /** Reused resample scratch (cols wide) so `push` allocates nothing per row. */
  private scratch: Float32Array

  constructor(cols: number, depth = DEFAULT_DEPTH) {
    this.cols = Math.max(1, cols | 0)
    this.depth = Math.max(2, depth | 0)
    this.data = new Uint8Array(this.depth * this.cols)
    this.frames = new Float64Array(this.depth * 3)
    this.scratch = new Float32Array(this.cols)
  }

  get columns(): number {
    return this.cols
  }

  /** Rows currently stored (≤ depth). */
  get length(): number {
    return this.count
  }

  /** Append one row of NORMALIZED intensities (0..1 → stored as 0..255). `row` may be any
   * length — it is resampled to the history's column count by `resampleRow`: max-pooled
   * when the row is WIDER than cols (a decimated carrier must not vanish; AetherSDR's
   * downsample rule) and interpolated when it is NARROWER. The narrow case is the FT
   * waterfall's (512 bins into 1024 columns): the old max-pool degenerated there to plain
   * duplication — each bin written to ⌈cols/n⌉ identical columns — baking a staircase into
   * history that no amount of render-side smoothing could undo. */
  push(row: ArrayLike<number>, loHz: number, hiHz: number, tsMs: number): void {
    const base = this.head * this.cols
    const n = row.length
    if (n === 0) return
    if (n === this.cols) {
      for (let i = 0; i < n; i++) {
        const v = row[i]
        this.data[base + i] = v <= 0 ? 0 : v >= 1 ? 255 : (v * 255) | 0
      }
    } else {
      // Resample onto the column grid over the row's own span (view === row, so the only
      // regime that applies is the n↔cols ratio).
      const s = this.scratch
      resampleRow(row, loHz, hiHz, loHz, hiHz, s)
      for (let i = 0; i < this.cols; i++) {
        const v = s[i]
        this.data[base + i] = !(v > 0) ? 0 : v >= 1 ? 255 : (v * 255) | 0
      }
    }
    const f = this.head * 3
    this.frames[f] = loHz
    this.frames[f + 1] = hiHz
    this.frames[f + 2] = tsMs
    this.head = (this.head + 1) % this.depth
    if (this.count < this.depth) this.count++
  }

  /** The frame of the row `age` rows back (0 = newest). `null` when out of range. */
  frameAt(age: number): RowFrame | null {
    if (age < 0 || age >= this.count) return null
    const idx = (this.head - 1 - age + this.depth * 2) % this.depth
    const f = idx * 3
    return { loHz: this.frames[f], hiHz: this.frames[f + 1], tsMs: this.frames[f + 2] }
  }

  /** Raw stored intensity (0..255) at (`age` rows back, column). 0 when out of range. */
  private at(age: number, col: number): number {
    const idx = (this.head - 1 - age + this.depth * 2) % this.depth
    return this.data[idx * this.cols + col]
  }

  /** Public read of the stored intensity (0..255) at (`age` rows back, column) — for the 3D
   * (3DSS) renderer, which samples rows directly. Bounds-checked → 0 out of range. */
  intensityAt(age: number, col: number): number {
    if (age < 0 || age >= this.count || col < 0 || col >= this.cols) return 0
    return this.at(age, col)
  }

  /**
   * Render a viewport FROM DATA into an RGBA buffer (width `outW` × height `outH`),
   * mapping each output column through the requested view window [viewLoHz, viewHiHz]
   * and each ROW's OWN stored frame — so history stays frequency-honest across
   * retunes/zoom. `offsetRows` scrolls back in time (0 = live tail) and is an AGE, so it
   * counts from the newest visible row whichever end of the viewport that is. Columns
   * outside a row's stored span render the palette floor (lut[0..2]).
   *
   * `newestAtTop` is the operator's scroll-direction toggle (`nexus.waterfall.flow`):
   * DEFAULT FALSE = newest row at the BOTTOM, history travelling up the screen, which is
   * what every build before it did and what an upgrade must keep. True mirrors it — newest
   * at the top, history travelling down.
   * ⚠️ The caller's HOT path (retained-buffer copyWithin + where the new row is written)
   * must flip with it. This cold path re-runs on palette/theme/zoom/resize/pause/2D↔3D, so
   * a half-flip tears or inverts the picture the moment the operator touches any of those.
   *
   * Cold-path only (palette/zoom/resize/scrollback): O(outW × outH). The hot path
   * appends via `push` + the caller's retained-buffer copyWithin scroll — and goes
   * through the same `resampleRow`, so a rebuild reproduces the live picture instead
   * of replacing it with a blockier one.
   */
  renderInto(
    out: Uint8ClampedArray,
    outW: number,
    outH: number,
    viewLoHz: number,
    viewHiHz: number,
    lut: Uint8ClampedArray,
    offsetRows = 0,
    newestAtTop = false,
  ): void {
    const span = viewHiHz - viewLoHz
    const floorR = lut[0]
    const floorG = lut[1]
    const floorB = lut[2]
    const px = new Float32Array(outW)
    for (let y = 0; y < outH; y++) {
      // Newest row (age offsetRows) at the bottom by default, at the top when flipped;
      // the opposite end is the oldest visible either way.
      const age = offsetRows + (newestAtTop ? y : outH - 1 - y)
      const o = y * outW * 4
      const fr = this.frameAt(age)
      if (!fr || !(span > 0) || !(fr.hiHz > fr.loHz)) {
        for (let x = 0; x < outW; x++) {
          const p = o + x * 4
          out[p] = floorR
          out[p + 1] = floorG
          out[p + 2] = floorB
          out[p + 3] = 255
        }
        continue
      }
      // Stored columns for this row, resampled onto the viewport. NaN = this pixel's
      // frequency is outside the row's own span (a retune) → palette floor.
      const idx = (this.head - 1 - age + this.depth * 2) % this.depth
      const stored = this.data.subarray(idx * this.cols, (idx + 1) * this.cols)
      resampleRow(stored, fr.loHz, fr.hiHz, viewLoHz, viewHiHz, px)
      for (let x = 0; x < outW; x++) {
        const v = px[x]
        const p = o + x * 4
        if (Number.isNaN(v)) {
          out[p] = floorR
          out[p + 1] = floorG
          out[p + 2] = floorB
          out[p + 3] = 255
          continue
        }
        const li = (v <= 0 ? 0 : v >= 255 ? 255 : Math.round(v)) * 4
        out[p] = lut[li]
        out[p + 1] = lut[li + 1]
        out[p + 2] = lut[li + 2]
        out[p + 3] = 255
      }
    }
  }

  /** Max scrollback offset that still shows a full viewport of `outH` rows. */
  maxOffset(outH: number): number {
    return Math.max(0, this.count - outH)
  }

  /** Drop all history (band change — old rows describe another frequency world). */
  clear(): void {
    this.count = 0
    this.head = 0
  }
}

/** Format a scrollback age for the time tape: "now", "12s", "1m05". */
export function ageLabel(ms: number): string {
  if (ms < 1500) return 'now'
  const s = Math.round(ms / 1000)
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  const rem = s % 60
  return `${m}m${rem.toString().padStart(2, '0')}`
}
