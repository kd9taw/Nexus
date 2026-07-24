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

/** One stored row's metadata. */
export interface RowFrame {
  /** Frequency window this row's columns span (Hz — audio-passband or absolute RF). */
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

  constructor(cols: number, depth = DEFAULT_DEPTH) {
    this.cols = Math.max(1, cols | 0)
    this.depth = Math.max(2, depth | 0)
    this.data = new Uint8Array(this.depth * this.cols)
    this.frames = new Float64Array(this.depth * 3)
  }

  get columns(): number {
    return this.cols
  }

  /** Rows currently stored (≤ depth). */
  get length(): number {
    return this.count
  }

  /** Append one row of NORMALIZED intensities (0..1 → stored as 0..255). `row` may be any
   * length — it is resampled to the history's column count with peak-preserving max-pooling
   * (a decimated carrier must not vanish; AetherSDR's downsample rule). */
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
      // Resample to cols: for each destination cell take the MAX over its source span.
      for (let i = 0; i < this.cols; i++) {
        const s0 = Math.floor((i * n) / this.cols)
        const s1 = Math.max(s0 + 1, Math.floor(((i + 1) * n) / this.cols))
        let m = 0
        for (let s = s0; s < s1 && s < n; s++) {
          const v = row[s]
          if (v > m) m = v
        }
        this.data[base + i] = m <= 0 ? 0 : m >= 1 ? 255 : (m * 255) | 0
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
   * Render a viewport FROM DATA into an RGBA buffer (width `outW` × height `outH`,
   * newest row at the BOTTOM), mapping each output column through the requested view
   * window [viewLoHz, viewHiHz] and each ROW's OWN stored frame — so history stays
   * frequency-honest across retunes/zoom. `offsetRows` scrolls back in time (0 = live
   * tail). Columns outside a row's stored span render the palette floor (lut[0..2]).
   *
   * Cold-path only (palette/zoom/resize/scrollback): O(outW × outH). The hot path
   * appends via `push` + the caller's retained-buffer copyWithin scroll.
   */
  renderInto(
    out: Uint8ClampedArray,
    outW: number,
    outH: number,
    viewLoHz: number,
    viewHiHz: number,
    lut: Uint8ClampedArray,
    offsetRows = 0,
  ): void {
    const span = viewHiHz - viewLoHz
    const floorR = lut[0]
    const floorG = lut[1]
    const floorB = lut[2]
    for (let y = 0; y < outH; y++) {
      // Bottom row = newest (age offsetRows), top row = oldest visible.
      const age = offsetRows + (outH - 1 - y)
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
      const rowSpan = fr.hiHz - fr.loHz
      for (let x = 0; x < outW; x++) {
        const hz = viewLoHz + (span * (x + 0.5)) / outW
        const p = o + x * 4
        if (hz < fr.loHz || hz > fr.hiHz) {
          out[p] = floorR
          out[p + 1] = floorG
          out[p + 2] = floorB
          out[p + 3] = 255
          continue
        }
        const col = Math.min(this.cols - 1, Math.floor(((hz - fr.loHz) / rowSpan) * this.cols))
        const li = this.at(age, col) * 4
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
