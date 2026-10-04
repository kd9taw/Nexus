// The spectrum renderer's contract: what a host (the rig scope, the digital waterfall) hands the
// renderer, and what both implementations behind it promise.
//
// Two first-class implementations sit behind it. WebGL2 keeps the history in a GPU texture ring
// and redraws every frame from it, so pan, zoom, palette, scrollback and resize are uniform
// changes. Canvas-2D is today's paint code: a retained RGBA band scrolled on the CPU and rebuilt
// from the same ring on a cold change. Canvas-2D is NOT a fallback afterthought. On Linux and the
// Pi the WebKitGTK webview masks the GPU, so it is the common case there, and both paths draw the
// same pixel fixtures in the real-browser harness (ui/spectrum-harness). `createSpectrumRenderer`
// (index.ts) picks the faster of the two here (choose.ts: canvas-2D on a software rasteriser), and
// WebGL2 only once it has created a context and read a test picture back from it, never by asking
// `gpu.ts`, whose probe fails closed on WebKitGTK.
//
// The renderer draws pixels and nothing else: no fetching, no AGC, no text. The host decides when
// a row is committed (when its source's sweep counter advances), which display range it is drawn
// in, and paints its axis and markers on its own layer above.

/** One spectrum frame, as far as drawing it goes. */
export interface SpectrumFrame {
  /** The producer's sweep counter. Stored with each committed row; the renderer never decides
   *  cadence, the host commits a row when this advances. */
  seq: number
  /** Producer timestamp, ms. Stored with each committed row for the scrollback time tape. */
  tMs: number
  /** The span the bins cover, Hz (absolute RF, or the audio passband), `loHz < hiHz`. Bin `i`
   *  covers `[lo + i·w, lo + (i+1)·w)`, the same cell rule as `resampleRow`. */
  loHz: number
  hiHz: number
  /** Values on the source's own axis, LINEAR IN dB (0..1 on the wire), at the source's native
   *  resolution: 512 audio, 475 CI-V, up to 2048 Flex, 850 FT-710. */
  bins: ArrayLike<number>
  /** dB that one unit of value spans (120 on the audio axis, `WF_DB_SPAN`). Only the average
   *  detector reads it: it averages POWER, and a mean of dB values is the geometric-mean trap
   *  `resampleRow`'s header warns about. */
  dbPerUnit: number
}

/** The display window on the value axis: `floor` draws as the palette's first entry and `ceil`
 *  as its last, the same `normalize` the components use. A committed row keeps the range it was
 *  committed in, so history does not re-colour when the range moves (today's behaviour). */
export interface DisplayRange {
  floor: number
  ceil: number
}

/** How a pixel that covers several bins reduces them: their maximum, or the mean of their power.
 *  A pixel narrower than a bin interpolates between the two nearest bin centres either way. */
export type Detector = 'peak' | 'average'

/** Bands of the canvas, in device pixels. The trace sits at the top, the waterfall fills the rest,
 *  and a strip at the bottom is left for the host's axis. `dss` draws the 3-D stack over the trace
 *  and waterfall area together. */
export interface SpectrumLayout {
  /** Height of the trace band at the top; 0 = no trace. */
  traceH: number
  /** Height of the strip at the bottom, painted the palette floor (the host's axis goes on top). */
  stripH: number
  /** Width of the trace line, device px (the components use `max(1, scaleY)`). */
  lineWidth: number
}

export type SpectrumMode = '2d' | 'dss'

/** Everything one `draw` paints from, besides the committed rows. */
export interface SpectrumScene {
  /** The frequency window across the canvas width, in the frames' Hz. */
  view: { loHz: number; hiHz: number }
  /** 256 × RGBA palette (`bakeLut`). Compared by reference: pass the same array until the
   *  palette changes. */
  lut: Uint8ClampedArray
  layout: SpectrumLayout
  detector: Detector
  mode: SpectrumMode
  /** Scrollback: the age of the newest visible row, 0 = live. A paused host advances it by one
   *  per committed row to hold the picture still. */
  offsetRows: number
  /** The operator's flow toggle: newest row at the top instead of the bottom. */
  newestAtTop: boolean
  /** The trace and the range it is drawn in; null = an empty trace band. */
  trace: { frame: SpectrumFrame; range: DisplayRange } | null
}

/** What a committed row remembers besides its values. */
export interface RowFrame {
  loHz: number
  hiHz: number
  tMs: number
  seq: number
}

export type BackendKind = 'webgl2' | 'canvas2d' | 'none'

export interface SpectrumRenderer {
  /** Which implementation is drawing now. It changes while a lost WebGL2 context is away, and
   *  is `none` only where no canvas context exists at all (jsdom). */
  readonly backend: BackendKind
  /** Why WebGL2 is not drawing, when it is not ('' while it is). */
  readonly reason: string
  /** The canvas drawing now. It is replaced while a lost context is away, so a host listens for
   *  pointer events on its own element, never on this. */
  readonly canvas: HTMLCanvasElement | null
  /** Rows committed and still held (at most the ring's depth). */
  readonly rows: number
  /** Set the drawing size in device pixels (the host measures its box). */
  resize(width: number, height: number): void
  /** Append one row to the history, drawn in `range` for as long as it is kept. */
  commitRow(frame: SpectrumFrame, range: DisplayRange): void
  /** The stored frame of the row `age` back (0 = newest), or null. */
  rowAt(age: number): RowFrame | null
  /** Drop the history (a band change: old rows describe another frequency world). */
  clearHistory(): void
  draw(scene: SpectrumScene): void
  /** Release the context(s) and take the canvas out of the host. */
  destroy(): void
}

export interface SpectrumRendererOptions {
  /** `auto` (default) picks the faster backend here (choose.ts). `webgl2` uses WebGL2 whenever its
   *  self-test passes, even where `auto` would not (the harness tests it on a software rasteriser);
   *  `canvas2d` skips WebGL2 altogether. Either one also beats the hidden setting. */
  backend?: 'auto' | 'webgl2' | 'canvas2d'
  /** Rows of history kept. */
  depth?: number
}

/** What the facade (index.ts) needs of each implementation. Both draw from the facade's ring. */
export interface Backend {
  readonly kind: 'webgl2' | 'canvas2d'
  readonly canvas: HTMLCanvasElement
  resize(width: number, height: number): void
  /** The ring gained a row (WebGL2 uploads it; canvas-2D draws it at the next `draw`). */
  rowCommitted(): void
  draw(scene: SpectrumScene): void
  destroy(): void
}
