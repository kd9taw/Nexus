// The WebGL2 renderer: the history lives in a GPU texture ring, and every `draw` is a redraw from it.
//
// - The ring (spectrum/ring.ts) is mirrored into an R16F texture, one row per committed frame at
//   its native bin count, with each row's span, bin count, dB per unit and display range in a
//   second (RGBA32F) texture. A commit is one `texSubImage2D` of one row.
// - The waterfall is one fragment pass. Each pixel finds its row by age, maps its own frequency band
//   through THAT row's span (so a retune or a zoom reprojects history for free), reduces the bins it
//   covers with the detector (aggregate.ts, the same function canvas-2D runs), and looks the result up
//   in the palette texture. Pan, zoom, palette, scrollback, flow and resize are uniform changes.
// - The trace is two small draws whose vertices aggregate the trace texture per pixel column: the
//   gradient fill under the curve, then an anti-aliased ribbon along it.
// - The 3-D stack is dss.ts's two passes over the same ring textures.
//
// Frequencies reach the GPU as float32 offsets from an ORIGIN near the newest rows (`REBASE_HZ`), so
// an absolute 145 MHz span keeps sub-hertz resolution; the origin moves, and the row spans are
// re-uploaded, only when a row lands far from it.
//
// Context loss is survivable because no history lives only on the GPU: `restore()` rebuilds every
// program and texture and re-uploads the ring whole. The facade (index.ts) listens for the events
// and draws on canvas-2D in between.

import { GLSL_AGGREGATE, safeDbPerUnit } from './aggregate'
import { bandsOf, TRACE_STOPS } from './canvas2d'
import {
  DSS_MAX_GL_COLS,
  DSS_ROWS,
  GLSL_DSS_DERIVE_FS,
  GLSL_DSS_MESH_FS,
  GLSL_DSS_MESH_VS,
  GLSL_FULLSCREEN_VS,
  GLSL_HEAD,
} from './dss'
import { copyBins, M_CEIL, M_DB_PER_UNIT, M_FLOOR, M_HI, M_LO, M_N, MAX_BINS, META, type SpectrumRing } from './ring'
import type { Backend, SpectrumScene } from './types'

/** A row this far from the origin (Hz) moves it: float32 then still resolves 0.125 Hz. */
const REBASE_HZ = 2 ** 21

const WATERFALL_FS = `${GLSL_HEAD}
uniform sampler2D uRing;
uniform sampler2D uMeta;
uniform sampler2D uLut;
uniform float uH;
uniform float uTop;
uniform float uBandH;
uniform float uW;
uniform float uViewLo;
uniform float uViewHi;
uniform int uHead;
uniform int uCount;
uniform int uDepth;
uniform int uOffset;
uniform int uNewestAtTop;
uniform int uDetector;
out vec4 outColor;
${GLSL_AGGREGATE}
void main() {
  vec4 floorColor = texelFetch(uLut, ivec2(0, 0), 0);
  // gl_FragCoord.y counts up from the bottom; the band's rows count down from its top.
  int y = int(floor(uH - gl_FragCoord.y - uTop));
  int bandH = int(uBandH);
  int age = uOffset + (uNewestAtTop == 1 ? y : bandH - 1 - y);
  if (age >= uCount) {
    outColor = floorColor;
    return;
  }
  int r = uHead - age;
  if (r < 0) r += uDepth;
  vec4 m0 = texelFetch(uMeta, ivec2(0, r), 0);
  vec4 m1 = texelFetch(uMeta, ivec2(1, r), 0);
  float v;
  if (!aggregateRow(uRing, r, int(m0.z), m0.x, m0.y, uViewLo, (uViewHi - uViewLo) / uW,
                    floor(gl_FragCoord.x), uDetector, m0.w, v)) {
    outColor = floorColor;
    return;
  }
  outColor = texelFetch(uLut, ivec2(lutIndex(v, m1.x, m1.y), 0), 0);
}
`

/** The trace's per-column value, shared by its two draws. Spans are relative to the view's low edge. */
const GLSL_TRACE = `
uniform sampler2D uTrace;
uniform int uN;
uniform float uRowLo;
uniform float uRowHi;
uniform float uViewSpan;
uniform float uW;
uniform float uH;
uniform float uTraceH;
uniform vec2 uRange;
uniform int uDetector;
uniform float uDbPerUnit;
${GLSL_AGGREGATE}
float traceY(float x) {
  float v;
  float t = 0.0;
  if (x < uW && aggregateRow(uTrace, 0, uN, uRowLo, uRowHi, 0.0, uViewSpan / uW, x, uDetector, uDbPerUnit, v)) {
    t = strengthIn(v, uRange.x, uRange.y);
  }
  return uTraceH - t * (uTraceH - 1.0);
}
vec4 clip(vec2 p) {
  return vec4(p.x / uW * 2.0 - 1.0, 1.0 - p.y / uH * 2.0, 0.0, 1.0);
}
`

/** `drawArrays(TRIANGLE_STRIP, 0, 2 × (W + 1))`: a curve point and the band floor per column. */
const TRACE_FILL_VS = `${GLSL_HEAD}${GLSL_TRACE}
void main() {
  int c = gl_VertexID / 2;
  bool top = gl_VertexID - c * 2 == 0;
  float x = float(c);
  gl_Position = clip(vec2(x, top ? traceY(x) : uTraceH));
}
`

const TRACE_FILL_FS = `${GLSL_HEAD}
uniform sampler2D uLut;
uniform float uH;
uniform float uTraceH;
out vec4 outColor;
void main() {
  // The canvas path's linear gradient: offset 0 at the band's bottom, 1 at its top.
  float g = clamp(1.0 - (uH - gl_FragCoord.y) / uTraceH, 0.0, 1.0);
  vec4 c0 = vec4(texelFetch(uLut, ivec2(${TRACE_STOPS[0]}, 0), 0).rgb, 0.45);
  vec4 c1 = vec4(texelFetch(uLut, ivec2(${TRACE_STOPS[1]}, 0), 0).rgb, 0.8);
  vec4 c2 = vec4(texelFetch(uLut, ivec2(${TRACE_STOPS[2]}, 0), 0).rgb, 0.95);
  outColor = g < 0.6 ? mix(c0, c1, g / 0.6) : mix(c1, c2, (g - 0.6) / 0.4);
}
`

/** `drawArrays(TRIANGLES, 0, 6 × (W − 1))`: one ribbon quad per segment between pixel columns. */
const TRACE_STROKE_VS = `${GLSL_HEAD}${GLSL_TRACE}
uniform float uLineWidth;
out float vAcross;
out float vHalf;
void main() {
  int s = gl_VertexID / 6;
  int corner = gl_VertexID - s * 6;
  vec2 p0 = vec2(float(s), traceY(float(s)));
  vec2 p1 = vec2(float(s + 1), traceY(float(s + 1)));
  vec2 d = p1 - p0;
  float len = length(d);
  d = len > 0.0 ? d / len : vec2(1.0, 0.0);
  vec2 nrm = vec2(-d.y, d.x);
  float hw = uLineWidth * 0.5 + 0.5;
  float side = (corner == 0 || corner == 1 || corner == 3) ? 1.0 : -1.0;
  bool atEnd = corner == 1 || corner == 2 || corner == 4;
  vec2 p = (atEnd ? p1 : p0) + nrm * hw * side;
  vAcross = side * hw;
  vHalf = uLineWidth * 0.5;
  gl_Position = clip(p);
}
`

const TRACE_STROKE_FS = `${GLSL_HEAD}
uniform sampler2D uLut;
in float vAcross;
in float vHalf;
out vec4 outColor;
void main() {
  float a = clamp(vHalf + 0.5 - abs(vAcross), 0.0, 1.0);
  outColor = vec4(texelFetch(uLut, ivec2(${TRACE_STOPS[2]}, 0), 0).rgb, a);
}
`

interface Program {
  program: WebGLProgram
  u: Record<string, WebGLUniformLocation | null>
}

/** Everything that dies with the context. */
interface Resources {
  vao: WebGLVertexArrayObject
  waterfall: Program
  traceFill: Program
  traceStroke: Program
  dssDerive: Program
  dssMesh: Program
  ring: WebGLTexture
  ringCols: number
  meta: WebGLTexture
  lut: WebGLTexture
  trace: WebGLTexture
  traceCols: number
  dss: { tex: WebGLTexture; fbo: WebGLFramebuffer; cols: number } | null
}

// Texture units, fixed for the life of the context.
const U_RING = 0
const U_META = 1
const U_LUT = 2
const U_TRACE = 3
const U_DSS = 4

function compile(gl: WebGL2RenderingContext, vs: string, fs: string, names: string[]): Program | string {
  const shader = (type: number, src: string): WebGLShader | string => {
    const s = gl.createShader(type)
    if (!s) return 'createShader returned null'
    gl.shaderSource(s, src)
    gl.compileShader(s)
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) return `shader: ${gl.getShaderInfoLog(s) ?? 'no log'}`
    return s
  }
  const v = shader(gl.VERTEX_SHADER, vs)
  if (typeof v === 'string') return v
  const f = shader(gl.FRAGMENT_SHADER, fs)
  if (typeof f === 'string') return f
  const program = gl.createProgram()
  if (!program) return 'createProgram returned null'
  gl.attachShader(program, v)
  gl.attachShader(program, f)
  gl.linkProgram(program)
  gl.deleteShader(v)
  gl.deleteShader(f)
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return `link: ${gl.getProgramInfoLog(program) ?? 'no log'}`
  const u: Record<string, WebGLUniformLocation | null> = {}
  for (const n of names) u[n] = gl.getUniformLocation(program, n)
  return { program, u }
}

/** A texture with no filtering (every read is a texelFetch) and storage of one level. */
function texture(gl: WebGL2RenderingContext, unit: number, format: number, w: number, h: number): WebGLTexture {
  const t = gl.createTexture()!
  gl.activeTexture(gl.TEXTURE0 + unit)
  gl.bindTexture(gl.TEXTURE_2D, t)
  // NEAREST is not decoration: a float texture is incomplete under the default mipmap filter, and
  // an incomplete texture reads as zero everywhere.
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
  gl.texStorage2D(gl.TEXTURE_2D, 1, format, w, h)
  return t
}

const WATERFALL_UNIFORMS = ['uRing', 'uMeta', 'uLut', 'uH', 'uTop', 'uBandH', 'uW', 'uViewLo', 'uViewHi', 'uHead', 'uCount', 'uDepth', 'uOffset', 'uNewestAtTop', 'uDetector']
const TRACE_UNIFORMS = ['uTrace', 'uLut', 'uN', 'uRowLo', 'uRowHi', 'uViewSpan', 'uW', 'uH', 'uTraceH', 'uRange', 'uDetector', 'uDbPerUnit', 'uLineWidth']
const DERIVE_UNIFORMS = ['uRing', 'uMeta', 'uHead', 'uCount', 'uDepth', 'uOffset', 'uCols', 'uDetector', 'uViewLo', 'uViewHi']
const MESH_UNIFORMS = ['uField', 'uLut', 'uCols', 'uRows', 'uW', 'uH', 'uPlotH']

export class WebGl2Backend implements Backend {
  readonly kind = 'webgl2'
  readonly canvas: HTMLCanvasElement
  private readonly gl: WebGL2RenderingContext
  private readonly ring: SpectrumRing
  private res: Resources | null = null
  private w = 0
  private h = 0
  /** The ring state the textures hold. */
  private syncedGeneration = -1
  private syncedSerial = 0
  private origin = 0
  private lutUploaded: Uint8ClampedArray | null = null
  private traceBuf = new Float32Array(0)
  private readonly rowMeta = new Float32Array(8)

  /** A context and a passing self-test, or the reason there is neither. */
  static create(canvas: HTMLCanvasElement, ring: SpectrumRing): WebGl2Backend | string {
    let gl: WebGL2RenderingContext | null = null
    try {
      gl = canvas.getContext('webgl2', {
        alpha: false,
        antialias: false,
        depth: false,
        stencil: false,
        premultipliedAlpha: true,
        preserveDrawingBuffer: false,
      })
    } catch {
      gl = null
    }
    if (!gl) return 'no WebGL2 context'
    const b = new WebGl2Backend(canvas, gl, ring)
    const why = b.restore()
    if (why) {
      b.destroy()
      return why
    }
    return b
  }

  private constructor(canvas: HTMLCanvasElement, gl: WebGL2RenderingContext, ring: SpectrumRing) {
    this.canvas = canvas
    this.gl = gl
    this.ring = ring
  }

  /** Build every program and texture, prove the context draws, and upload the ring. Also the
   *  answer to `webglcontextrestored`. Returns why it cannot draw, or null. */
  restore(): string | null {
    const gl = this.gl
    this.res = null
    this.syncedGeneration = -1
    this.lutUploaded = null
    if (gl.isContextLost()) return 'the context is lost'
    const maxTex = gl.getParameter(gl.MAX_TEXTURE_SIZE) as number
    if (!(maxTex >= Math.max(MAX_BINS, this.ring.depth))) return `MAX_TEXTURE_SIZE ${maxTex} is too small`
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1)
    const programs = {
      waterfall: compile(gl, GLSL_FULLSCREEN_VS, WATERFALL_FS, WATERFALL_UNIFORMS),
      traceFill: compile(gl, TRACE_FILL_VS, TRACE_FILL_FS, TRACE_UNIFORMS),
      traceStroke: compile(gl, TRACE_STROKE_VS, TRACE_STROKE_FS, TRACE_UNIFORMS),
      dssDerive: compile(gl, GLSL_FULLSCREEN_VS, GLSL_DSS_DERIVE_FS, DERIVE_UNIFORMS),
      dssMesh: compile(gl, GLSL_DSS_MESH_VS, GLSL_DSS_MESH_FS, MESH_UNIFORMS),
    }
    for (const [name, p] of Object.entries(programs)) if (typeof p === 'string') return `${name} ${p}`
    const vao = gl.createVertexArray()
    if (!vao) return 'createVertexArray returned null'
    const res: Resources = {
      vao,
      waterfall: programs.waterfall as Program,
      traceFill: programs.traceFill as Program,
      traceStroke: programs.traceStroke as Program,
      dssDerive: programs.dssDerive as Program,
      dssMesh: programs.dssMesh as Program,
      ring: texture(gl, U_RING, gl.R16F, this.ring.cols, this.ring.depth),
      ringCols: this.ring.cols,
      meta: texture(gl, U_META, gl.RGBA32F, 2, this.ring.depth),
      lut: texture(gl, U_LUT, gl.RGBA8, 256, 1),
      trace: texture(gl, U_TRACE, gl.R32F, 512, 1),
      traceCols: 512,
      dss: null,
    }
    // Samplers sit on fixed units, set once per program.
    const units: [Program, string, number][] = [
      [res.waterfall, 'uRing', U_RING],
      [res.waterfall, 'uMeta', U_META],
      [res.waterfall, 'uLut', U_LUT],
      [res.traceFill, 'uTrace', U_TRACE],
      [res.traceFill, 'uLut', U_LUT],
      [res.traceStroke, 'uTrace', U_TRACE],
      [res.traceStroke, 'uLut', U_LUT],
      [res.dssDerive, 'uRing', U_RING],
      [res.dssDerive, 'uMeta', U_META],
      [res.dssMesh, 'uField', U_DSS],
      [res.dssMesh, 'uLut', U_LUT],
    ]
    for (const [p, name, unit] of units) {
      gl.useProgram(p.program)
      gl.uniform1i(p.u[name], unit)
    }
    const err = gl.getError()
    if (err !== gl.NO_ERROR) return `GL error 0x${err.toString(16)} while building`
    this.res = res
    const failed = this.selfTest()
    if (failed) {
      this.res = null
      return `self-test: ${failed}`
    }
    this.sync()
    return null
  }

  /** The context is gone: every handle is dead. Called by the facade on `webglcontextlost`. */
  lost(): void {
    this.res = null
    this.syncedGeneration = -1
    this.lutUploaded = null
  }

  resize(width: number, height: number): void {
    if (this.canvas.width !== width) this.canvas.width = width
    if (this.canvas.height !== height) this.canvas.height = height
    this.w = width
    this.h = height
  }

  rowCommitted(): void {
    if (this.res && !this.gl.isContextLost()) this.sync()
  }

  draw(scene: SpectrumScene): void {
    const gl = this.gl
    const res = this.res
    const { w, h } = this
    if (!res || w <= 0 || h <= 0 || gl.isContextLost()) return
    this.sync()
    const lut = scene.lut
    if (lut !== this.lutUploaded) {
      gl.activeTexture(gl.TEXTURE0 + U_LUT)
      gl.bindTexture(gl.TEXTURE_2D, res.lut)
      gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, 256, 1, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array(lut.buffer, lut.byteOffset, 1024))
      this.lutUploaded = lut
    }
    const { traceH, plotH, stripH } = bandsOf(scene.layout, h)
    gl.bindFramebuffer(gl.FRAMEBUFFER, null)
    gl.viewport(0, 0, w, h)
    gl.bindVertexArray(res.vao)
    gl.disable(gl.BLEND)
    gl.disable(gl.SCISSOR_TEST)
    // The palette floor everywhere first: the strip stays it, and so does any band with no rows.
    gl.clearColor(lut[0] / 255, lut[1] / 255, lut[2] / 255, 1)
    gl.clear(gl.COLOR_BUFFER_BIT)
    gl.enable(gl.SCISSOR_TEST)
    if (scene.mode === 'dss') {
      this.drawDss(res, scene, plotH)
    } else {
      const bandH = plotH - traceH
      if (bandH > 0) this.drawWaterfall(res, scene, traceH, bandH, stripH)
      if (traceH > 0 && scene.trace) this.drawTrace(res, scene, traceH)
    }
    gl.disable(gl.SCISSOR_TEST)
    gl.disable(gl.BLEND)
    gl.bindVertexArray(null)
  }

  destroy(): void {
    const gl = this.gl
    const res = this.res
    this.res = null
    if (res && !gl.isContextLost()) {
      for (const p of [res.waterfall, res.traceFill, res.traceStroke, res.dssDerive, res.dssMesh]) gl.deleteProgram(p.program)
      for (const t of [res.ring, res.meta, res.lut, res.trace]) gl.deleteTexture(t)
      if (res.dss) {
        gl.deleteFramebuffer(res.dss.fbo)
        gl.deleteTexture(res.dss.tex)
      }
      gl.deleteVertexArray(res.vao)
    }
    // Hand the context back now rather than when the page lets go of the canvas: browsers cap live
    // WebGL contexts and evict the oldest. The facade has already stopped listening, so this loss
    // is not mistaken for a real one.
    gl.getExtension('WEBGL_lose_context')?.loseContext()
  }

  private drawWaterfall(res: Resources, scene: SpectrumScene, top: number, bandH: number, stripH: number): void {
    const gl = this.gl
    const ring = this.ring
    const p = res.waterfall
    gl.scissor(0, stripH, this.w, bandH)
    gl.useProgram(p.program)
    gl.uniform1f(p.u.uH, this.h)
    gl.uniform1f(p.u.uTop, top)
    gl.uniform1f(p.u.uBandH, bandH)
    gl.uniform1f(p.u.uW, this.w)
    gl.uniform1f(p.u.uViewLo, scene.view.loHz - this.origin)
    gl.uniform1f(p.u.uViewHi, scene.view.hiHz - this.origin)
    gl.uniform1i(p.u.uHead, ring.head)
    gl.uniform1i(p.u.uCount, ring.count)
    gl.uniform1i(p.u.uDepth, ring.depth)
    gl.uniform1i(p.u.uOffset, Math.max(0, Math.floor(scene.offsetRows)))
    gl.uniform1i(p.u.uNewestAtTop, scene.newestAtTop ? 1 : 0)
    gl.uniform1i(p.u.uDetector, scene.detector === 'average' ? 1 : 0)
    gl.drawArrays(gl.TRIANGLES, 0, 3)
  }

  private drawTrace(res: Resources, scene: SpectrumScene, traceH: number): void {
    const gl = this.gl
    const { frame, range } = scene.trace!
    const n = Math.min(frame.bins.length, MAX_BINS)
    if (n === 0) return
    if (n > res.traceCols) {
      gl.deleteTexture(res.trace)
      let cols = res.traceCols
      while (cols < n) cols *= 2
      res.trace = texture(gl, U_TRACE, gl.R32F, cols, 1)
      res.traceCols = cols
    }
    if (this.traceBuf.length < n) this.traceBuf = new Float32Array(res.traceCols)
    const buf = this.traceBuf
    copyBins(frame.bins, buf, 0, n)
    gl.activeTexture(gl.TEXTURE0 + U_TRACE)
    gl.bindTexture(gl.TEXTURE_2D, res.trace)
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, n, 1, gl.RED, gl.FLOAT, buf, 0)
    gl.scissor(0, this.h - traceH, this.w, traceH)
    gl.enable(gl.BLEND)
    // Colour blends over the floor; alpha accumulates toward opaque, so the canvas never shows
    // the page through a half-covered pixel.
    gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA)
    for (const p of [res.traceFill, res.traceStroke]) {
      gl.useProgram(p.program)
      gl.uniform1i(p.u.uN, n)
      gl.uniform1f(p.u.uRowLo, frame.loHz - scene.view.loHz)
      gl.uniform1f(p.u.uRowHi, frame.hiHz - scene.view.loHz)
      gl.uniform1f(p.u.uViewSpan, scene.view.hiHz - scene.view.loHz)
      gl.uniform1f(p.u.uW, this.w)
      gl.uniform1f(p.u.uH, this.h)
      gl.uniform1f(p.u.uTraceH, traceH)
      gl.uniform2f(p.u.uRange, range.floor, range.ceil)
      gl.uniform1i(p.u.uDetector, scene.detector === 'average' ? 1 : 0)
      gl.uniform1f(p.u.uDbPerUnit, safeDbPerUnit(frame.dbPerUnit))
      gl.uniform1f(p.u.uLineWidth, Math.max(1, scene.layout.lineWidth))
    }
    gl.useProgram(res.traceFill.program)
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 2 * (this.w + 1))
    gl.useProgram(res.traceStroke.program)
    gl.drawArrays(gl.TRIANGLES, 0, 6 * (this.w - 1))
  }

  private drawDss(res: Resources, scene: SpectrumScene, plotH: number): void {
    const gl = this.gl
    const ring = this.ring
    const offset = Math.max(0, Math.floor(scene.offsetRows))
    const rows = Math.max(0, Math.min(DSS_ROWS, ring.count - offset))
    const cols = Math.max(2, Math.min(this.w, DSS_MAX_GL_COLS))
    if (rows === 0 || plotH <= 0) return
    if (!res.dss || res.dss.cols !== cols) {
      if (res.dss) {
        gl.deleteFramebuffer(res.dss.fbo)
        gl.deleteTexture(res.dss.tex)
      }
      const tex = texture(gl, U_DSS, gl.RGBA8, cols, DSS_ROWS)
      const fbo = gl.createFramebuffer()!
      gl.bindFramebuffer(gl.FRAMEBUFFER, fbo)
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0)
      res.dss = { tex, fbo, cols }
    }
    // Pass 1: the strength field, one fragment per (column, depth row).
    gl.disable(gl.SCISSOR_TEST)
    gl.bindFramebuffer(gl.FRAMEBUFFER, res.dss.fbo)
    gl.viewport(0, 0, cols, DSS_ROWS)
    const d = res.dssDerive
    gl.useProgram(d.program)
    gl.uniform1i(d.u.uHead, ring.head)
    gl.uniform1i(d.u.uCount, ring.count)
    gl.uniform1i(d.u.uDepth, ring.depth)
    gl.uniform1i(d.u.uOffset, offset)
    gl.uniform1i(d.u.uCols, cols)
    gl.uniform1i(d.u.uDetector, scene.detector === 'average' ? 1 : 0)
    gl.uniform1f(d.u.uViewLo, scene.view.loHz - this.origin)
    gl.uniform1f(d.u.uViewHi, scene.view.hiHz - this.origin)
    gl.drawArrays(gl.TRIANGLES, 0, 3)
    // Pass 2: the mesh, back row first.
    gl.bindFramebuffer(gl.FRAMEBUFFER, null)
    gl.viewport(0, 0, this.w, this.h)
    gl.enable(gl.SCISSOR_TEST)
    gl.scissor(0, this.h - plotH, this.w, plotH)
    gl.enable(gl.BLEND)
    gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA)
    gl.activeTexture(gl.TEXTURE0 + U_DSS)
    gl.bindTexture(gl.TEXTURE_2D, res.dss.tex)
    const m = res.dssMesh
    gl.useProgram(m.program)
    gl.uniform1i(m.u.uCols, cols)
    gl.uniform1i(m.u.uRows, rows)
    gl.uniform1f(m.u.uW, this.w)
    gl.uniform1f(m.u.uH, this.h)
    gl.uniform1f(m.u.uPlotH, plotH)
    gl.drawArraysInstanced(gl.TRIANGLES, 0, (cols - 1) * 12, rows)
  }

  /** Bring the ring textures up to the ring: the rows committed since the last sync, or all of it
   *  after a widened or cleared ring and on (re)creation. */
  private sync(): void {
    const res = this.res
    if (!res) return
    const gl = this.gl
    const ring = this.ring
    if (ring.generation !== this.syncedGeneration || ring.cols !== res.ringCols) {
      this.uploadAll(res)
      return
    }
    const fresh = ring.serial - this.syncedSerial
    if (fresh <= 0) return
    if (fresh >= ring.count) {
      this.uploadAll(res)
      return
    }
    gl.activeTexture(gl.TEXTURE0 + U_RING)
    gl.bindTexture(gl.TEXTURE_2D, res.ring)
    for (let age = fresh - 1; age >= 0; age--) {
      const r = ring.indexAt(age)
      const n = ring.meta[r * META + M_N]
      if (n > 0) gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, r, n, 1, gl.RED, gl.FLOAT, ring.values, r * ring.cols)
      if (!this.nearOrigin(r)) {
        this.origin = this.centreOf(r)
        this.uploadMeta(res)
      } else {
        this.uploadRowMeta(res, r)
      }
    }
    this.syncedSerial = ring.serial
  }

  private uploadAll(res: Resources): void {
    const gl = this.gl
    const ring = this.ring
    if (res.ringCols !== ring.cols) {
      gl.deleteTexture(res.ring)
      res.ring = texture(gl, U_RING, gl.R16F, ring.cols, ring.depth)
      res.ringCols = ring.cols
    }
    gl.activeTexture(gl.TEXTURE0 + U_RING)
    gl.bindTexture(gl.TEXTURE_2D, res.ring)
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, ring.cols, ring.depth, gl.RED, gl.FLOAT, ring.values)
    this.origin = ring.count > 0 ? this.centreOf(ring.head) : 0
    this.uploadMeta(res)
    this.syncedGeneration = ring.generation
    this.syncedSerial = ring.serial
  }

  private uploadMeta(res: Resources): void {
    const ring = this.ring
    const all = new Float32Array(ring.depth * 8)
    for (let r = 0; r < ring.depth; r++) this.packMeta(r, all, r * 8)
    const gl = this.gl
    gl.activeTexture(gl.TEXTURE0 + U_META)
    gl.bindTexture(gl.TEXTURE_2D, res.meta)
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, 2, ring.depth, gl.RGBA, gl.FLOAT, all)
  }

  private uploadRowMeta(res: Resources, r: number): void {
    this.packMeta(r, this.rowMeta, 0)
    const gl = this.gl
    gl.activeTexture(gl.TEXTURE0 + U_META)
    gl.bindTexture(gl.TEXTURE_2D, res.meta)
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, r, 2, 1, gl.RGBA, gl.FLOAT, this.rowMeta)
  }

  /** A row's two meta texels: (lo, hi relative to the origin, bins, dB per unit), (floor, ceil). */
  private packMeta(r: number, out: Float32Array, o: number): void {
    const m = this.ring.meta
    const b = r * META
    out[o] = m[b + M_LO] - this.origin
    out[o + 1] = m[b + M_HI] - this.origin
    out[o + 2] = m[b + M_N]
    out[o + 3] = m[b + M_DB_PER_UNIT]
    out[o + 4] = m[b + M_FLOOR]
    out[o + 5] = m[b + M_CEIL]
    out[o + 6] = 0
    out[o + 7] = 1
  }

  private centreOf(r: number): number {
    const m = this.ring.meta
    return (m[r * META + M_LO] + m[r * META + M_HI]) / 2
  }

  private nearOrigin(r: number): boolean {
    const m = this.ring.meta
    return Math.abs(m[r * META + M_LO] - this.origin) <= REBASE_HZ && Math.abs(m[r * META + M_HI] - this.origin) <= REBASE_HZ
  }

  /**
   * Draw a known picture through the real waterfall program and read it back: a 4-bin, 2-row ring
   * in R16F, its spans in RGBA32F, a palette in RGBA8, into a 4×2 RGBA8 framebuffer. A context that
   * creates but draws garbage (a broken driver, a masked or partial WebGL2) fails here instead of
   * on the operator's screen. Returns what went wrong, or null.
   */
  private selfTest(): string | null {
    const gl = this.gl
    const res = this.res!
    const rows = [
      [0, 0.25, 0.5, 1],
      [1, 0.75, 0.125, 0.0625],
    ]
    const lut = new Uint8Array(1024)
    for (let i = 0; i < 256; i++) lut.set([i, 255 - i, (i * 37) & 255, 255], i * 4)
    const ringTex = texture(gl, U_RING, gl.R16F, 4, 2)
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, 4, 2, gl.RED, gl.FLOAT, new Float32Array(rows.flat()))
    const metaTex = texture(gl, U_META, gl.RGBA32F, 2, 2)
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, 2, 2, gl.RGBA, gl.FLOAT, new Float32Array([0, 4, 4, 120, 0, 1, 0, 1, 0, 4, 4, 120, 0, 1, 0, 1]))
    const lutTex = texture(gl, U_LUT, gl.RGBA8, 256, 1)
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, 256, 1, gl.RGBA, gl.UNSIGNED_BYTE, lut)
    const target = texture(gl, 7, gl.RGBA8, 4, 2)
    const fbo = gl.createFramebuffer()
    const out = new Uint8Array(4 * 2 * 4)
    let why: string | null = null
    try {
      gl.bindFramebuffer(gl.FRAMEBUFFER, fbo)
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, target, 0)
      if (gl.checkFramebufferStatus(gl.FRAMEBUFFER) !== gl.FRAMEBUFFER_COMPLETE) return 'an RGBA8 framebuffer is incomplete'
      gl.viewport(0, 0, 4, 2)
      gl.disable(gl.SCISSOR_TEST)
      gl.disable(gl.BLEND)
      gl.bindVertexArray(res.vao)
      const p = res.waterfall
      gl.useProgram(p.program)
      gl.uniform1f(p.u.uH, 2)
      gl.uniform1f(p.u.uTop, 0)
      gl.uniform1f(p.u.uBandH, 2)
      gl.uniform1f(p.u.uW, 4)
      gl.uniform1f(p.u.uViewLo, 0)
      gl.uniform1f(p.u.uViewHi, 4)
      gl.uniform1i(p.u.uHead, 1)
      gl.uniform1i(p.u.uCount, 2)
      gl.uniform1i(p.u.uDepth, 2)
      gl.uniform1i(p.u.uOffset, 0)
      gl.uniform1i(p.u.uNewestAtTop, 0)
      gl.uniform1i(p.u.uDetector, 0)
      gl.drawArrays(gl.TRIANGLES, 0, 3)
      gl.readPixels(0, 0, 4, 2, gl.RGBA, gl.UNSIGNED_BYTE, out)
      // GL row 0 is the bottom: the newest row (ring row 1); row 1 the one before it.
      const index = (v: number) => (v >= 1 ? 255 : Math.round(v * 255))
      for (let y = 0; y < 2; y++) {
        const src = rows[1 - y]
        for (let x = 0; x < 4; x++) {
          const want = lut.subarray(index(src[x]) * 4, index(src[x]) * 4 + 4)
          const got = out.subarray((y * 4 + x) * 4, (y * 4 + x) * 4 + 4)
          for (let c = 0; c < 4; c++) {
            if (Math.abs(got[c] - want[c]) > 1) {
              why ??= `pixel (${x},${y}) read ${Array.from(got).join(',')}, expected ${Array.from(want).join(',')}`
            }
          }
        }
      }
      const err = gl.getError()
      if (!why && err !== gl.NO_ERROR) why = `GL error 0x${err.toString(16)} while drawing`
    } finally {
      gl.bindFramebuffer(gl.FRAMEBUFFER, null)
      gl.bindVertexArray(null)
      gl.deleteFramebuffer(fbo)
      for (const t of [ringTex, metaTex, lutTex, target]) gl.deleteTexture(t)
      // The scratch textures were bound in the real ones' units; put the real ones back.
      gl.activeTexture(gl.TEXTURE0 + U_RING)
      gl.bindTexture(gl.TEXTURE_2D, res.ring)
      gl.activeTexture(gl.TEXTURE0 + U_META)
      gl.bindTexture(gl.TEXTURE_2D, res.meta)
      gl.activeTexture(gl.TEXTURE0 + U_LUT)
      gl.bindTexture(gl.TEXTURE_2D, res.lut)
      gl.activeTexture(gl.TEXTURE0 + U_TRACE)
      gl.bindTexture(gl.TEXTURE_2D, res.trace)
    }
    return why
  }
}
