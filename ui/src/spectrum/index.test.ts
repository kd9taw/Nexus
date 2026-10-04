// @vitest-environment jsdom
//
// jsdom has neither a WebGL2 nor a 2D context, which is exactly the environment every component
// suite that will mount the renderer runs in. There the renderer must stay inert (draw nothing,
// throw nothing) while still keeping its history, so those suites can drive a host through it.
// What it draws is proved in a real browser (ui/spectrum-harness).
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createSpectrumRenderer } from './index'
import type { SpectrumRendererOptions, SpectrumScene } from './types'

const scene = (): SpectrumScene => ({
  view: { loHz: 0, hiHz: 4000 },
  lut: new Uint8ClampedArray(1024),
  layout: { traceH: 40, stripH: 10, lineWidth: 1 },
  detector: 'peak',
  mode: '2d',
  offsetRows: 0,
  newestAtTop: false,
  trace: null,
})

describe('createSpectrumRenderer without any canvas context', () => {
  it('is inert, says why, and still keeps the history', () => {
    const host = document.createElement('div')
    const r = createSpectrumRenderer(host)
    expect(r.backend).toBe('none')
    expect(r.canvas).toBeNull()
    expect(r.reason).toBe('no WebGL2 context')
    r.resize(300, 200)
    r.commitRow({ seq: 7, tMs: 70, loHz: 0, hiHz: 4000, bins: [0.1, 0.2], dbPerUnit: 120 }, { floor: 0, ceil: 1 })
    r.draw(scene())
    expect(r.rows).toBe(1)
    expect(r.rowAt(0)).toEqual({ loHz: 0, hiHz: 4000, tMs: 70, seq: 7 })
    r.clearHistory()
    expect(r.rows).toBe(0)
    r.destroy()
    expect(host.childElementCount).toBe(0)
  })

  it('names canvas-2D as the choice when it is asked for', () => {
    const r = createSpectrumRenderer(document.createElement('div'), { backend: 'canvas2d' })
    expect(r.reason).toBe('canvas-2D was asked for')
    r.destroy()
  })
})

// The choice, on a stubbed context. The stub's context is always lost, so a renderer that goes on to
// build WebGL2 stops at once and says 'the context is lost': that reason is how these tests see
// WebGL2 being tried. Each test imports the renderer afresh, since the page's verdict is cached.
describe('choosing a backend', () => {
  const SWIFTSHADER = 'ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero) (0x0000C0DE)), SwiftShader driver)'
  const GPU = 'ANGLE (NVIDIA, NVIDIA GeForce RTX 3060 (0x00002503) Direct3D11 vs_5_0 ps_5_0, D3D11)'
  const UNMASKED_RENDERER = 0x9246

  /** WebGL2 contexts whose renderer string is `name`; null = masked, as WebKitGTK masks it. */
  function contexts(name: string | null) {
    const gl = {
      getExtension: (ext: string) =>
        ext === 'WEBGL_debug_renderer_info'
          ? name === null
            ? null
            : { UNMASKED_RENDERER_WEBGL: UNMASKED_RENDERER }
          : ext === 'WEBGL_lose_context'
            ? { loseContext() {} }
            : null,
      getParameter: (p: number) => (p === UNMASKED_RENDERER ? name : null),
      isContextLost: () => true,
    }
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(((kind: string) =>
      kind === 'webgl2' ? gl : null) as unknown as HTMLCanvasElement['getContext'])
  }

  async function choose(opts?: SpectrumRendererOptions) {
    vi.resetModules()
    const { createSpectrumRenderer: create } = await import('./index')
    const r = create(document.createElement('div'), opts)
    const out = { backend: r.backend, reason: r.reason }
    r.destroy()
    return out
  }

  afterEach(() => {
    vi.restoreAllMocks()
    localStorage.clear()
  })

  it('turns down a software rasteriser by its renderer string, before building WebGL2', async () => {
    contexts(SWIFTSHADER)
    expect(await choose()).toEqual({
      backend: 'none',
      reason: `WebGL2 is software-rendered here (${SWIFTSHADER}); canvas-2D is faster`,
    })
  })

  it("tries WebGL2 on a GPU's renderer string, and on a masked one (the probe decides later)", async () => {
    contexts(GPU)
    expect((await choose()).reason).toBe('the context is lost')
    contexts(null)
    expect((await choose()).reason).toBe('the context is lost')
  })

  it('builds WebGL2 when it is asked for, whatever the renderer string says', async () => {
    contexts(SWIFTSHADER)
    expect((await choose({ backend: 'webgl2' })).reason).toBe('the context is lost')
  })

  it('takes the hidden setting, and an explicit backend over it', async () => {
    contexts(SWIFTSHADER)
    localStorage.setItem('nexus.spectrum.backend', 'webgl2')
    expect((await choose()).reason).toBe('the context is lost')
    localStorage.setItem('nexus.spectrum.backend', 'canvas2d')
    expect((await choose()).reason).toBe('canvas-2D was asked for')
    expect((await choose({ backend: 'webgl2' })).reason).toBe('the context is lost')
    // Anything else is ignored: the automatic choice stands.
    localStorage.setItem('nexus.spectrum.backend', 'gpu')
    expect((await choose()).reason).toMatch(/^WebGL2 is software-rendered here/)
  })
})
