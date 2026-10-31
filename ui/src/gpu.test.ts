// @vitest-environment jsdom
//
// The GPU probes create a WebGL context to ask their question, and a context nobody releases stays
// live until the page lets go of the canvas. Chromium keeps at most 16 live WebGL contexts per
// renderer process and evicts the OLDEST when a 17th is made, so a leaked probe context is one
// fewer for the maps and the spectrum, and enough of them cost a live view its context. Every
// probe here must hand its context back, on every path, without changing what it decides.
//
// jsdom has no WebGL, so `getContext` is stubbed with just what the probes read: a renderer
// string, a texture size, and the WEBGL_lose_context handle whose `loseContext` releases it.
import { afterEach, describe, expect, it, vi } from 'vitest'
// globe.gl is imported here, at collection, and never inside a test. Loading it pulls in all of
// three.js: about 220 ms on an idle machine, which is 90% of the one case that builds a globe. With
// the box loaded, that import alone took more than the 5 s test budget, so the case failed although
// nothing it checks had changed. Module collection has no timeout, so the budget below covers only
// what the case checks.
import Globe from 'globe.gl'
import { gpuCapableForGlobe, webgl2Available } from './gpu'

const UNMASKED_RENDERER_WEBGL = 0x9246
const MAX_TEXTURE_SIZE = 0x0d33

interface FakeGl {
  renderer?: string
  maxTex?: number
  /** WebKitGTK's default: WEBGL_debug_renderer_info is not exposed at all. */
  masked?: boolean
  /** A driver that throws mid-probe. */
  throws?: boolean
}

function fakeGl({ renderer = 'ANGLE (NVIDIA GeForce RTX 3060)', maxTex = 16384, masked, throws }: FakeGl) {
  const loseContext = vi.fn()
  const gl = {
    MAX_TEXTURE_SIZE,
    getExtension: (name: string) => {
      if (name === 'WEBGL_lose_context') return { loseContext }
      if (name === 'WEBGL_debug_renderer_info') return masked ? null : { UNMASKED_RENDERER_WEBGL }
      return null
    },
    getParameter: (p: number) => {
      if (throws) throw new Error('driver went away')
      if (p === UNMASKED_RENDERER_WEBGL) return renderer
      if (p === MAX_TEXTURE_SIZE) return maxTex
      return null
    },
  }
  return { gl, loseContext }
}

/** Serve `gl` for the context kinds listed, null for the rest — which is what a browser does. */
function serve(gl: unknown, kinds: string[]) {
  return vi
    .spyOn(HTMLCanvasElement.prototype, 'getContext')
    .mockImplementation(((kind: string) => (kinds.includes(kind) ? gl : null)) as never)
}

afterEach(() => vi.restoreAllMocks())

describe('gpuCapableForGlobe', () => {
  // Each case: what the machine looks like, and what the probe has always answered for it.
  const cases: Array<[string, FakeGl, boolean]> = [
    ['a hardware GPU with big textures', {}, true],
    ['llvmpipe (software GL)', { renderer: 'llvmpipe (LLVM 17.0.6, 256 bits)' }, false],
    ['SwiftShader', { renderer: 'ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device))' }, false],
    ['a masked renderer string (WebKitGTK)', { masked: true }, false],
    ['a GPU with small textures', { maxTex: 2048 }, false],
    ['a driver that throws mid-probe', { throws: true }, false],
  ]

  for (const [name, machine, answer] of cases) {
    it(`answers ${answer} for ${name} and releases its probe context`, () => {
      const { gl, loseContext } = fakeGl(machine)
      serve(gl, ['webgl2'])
      expect(gpuCapableForGlobe()).toBe(answer)
      expect(loseContext).toHaveBeenCalledTimes(1)
    })
  }

  it('falls back to WebGL 1 as before, and releases that context too', () => {
    const { gl, loseContext } = fakeGl({})
    serve(gl, ['webgl'])
    expect(gpuCapableForGlobe()).toBe(true)
    expect(loseContext).toHaveBeenCalledTimes(1)
  })

  it('answers false with no WebGL at all (nothing to release)', () => {
    serve(null, [])
    expect(gpuCapableForGlobe()).toBe(false)
  })
})

describe('webgl2Available — the street map gate', () => {
  it('is true whenever a WebGL2 context can be created, and releases it', () => {
    const { gl, loseContext } = fakeGl({})
    serve(gl, ['webgl2'])
    expect(webgl2Available()).toBe(true)
    expect(loseContext).toHaveBeenCalledTimes(1)
  })

  // The two machines the globe probe turns away: software GL and a masked renderer string. Street
  // runs on both — every Linux and Pi install masks the string, and software GL is allowed.
  it('accepts software GL and a masked renderer, which the globe probe refuses', () => {
    for (const machine of [{ renderer: 'llvmpipe (LLVM 17.0.6, 256 bits)' }, { masked: true }]) {
      const { gl, loseContext } = fakeGl(machine)
      serve(gl, ['webgl2'])
      expect(gpuCapableForGlobe()).toBe(false)
      expect(webgl2Available()).toBe(true)
      expect(loseContext).toHaveBeenCalledTimes(2)
      vi.restoreAllMocks()
    }
  })

  it('does not count WebGL 1 — MapLibre v6 needs WebGL2', () => {
    const { gl, loseContext } = fakeGl({})
    serve(gl, ['webgl'])
    expect(webgl2Available()).toBe(false)
    expect(loseContext).not.toHaveBeenCalled()
  })

  it('is false when creating the context throws', () => {
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => {
      throw new Error('blocklisted')
    })
    expect(webgl2Available()).toBe(false)
  })
})

// The 3-D globes are drawn by three.js, which makes a WebGL 2 context or throws ("Error creating WebGL
// context"). A computer that makes only WebGL 1 passes gpuCapableForGlobe, so it showed a globe that threw:
// the whole Logbook became its error page (a report from Linux Mint on 1.18.0). This probe asks three.js's
// own question once a page, and GlobeGuard puts a note where the answer is no.
describe('webgl2ForGlobe: the context three.js asks for, once a page', () => {
  /** gpu.ts afresh: the answer is kept for the page, so each case starts without one. */
  async function fresh() {
    vi.resetModules()
    return (await import('./gpu')).webgl2ForGlobe
  }

  it('asks for WebGL 2 with the attributes three.js asks for when globe.gl builds a globe', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    const asked: unknown[][] = []
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(((...args: unknown[]) => {
      asked.push(args)
      return null
    }) as never)
    // Both globes build globe.gl with no rendererConfig of their own (react-globe.gl passes none either).
    expect(() => new Globe(document.createElement('div'))).toThrow('Error creating WebGL context')
    const three = asked[0]
    expect(three[0]).toBe('webgl2')
    asked.length = 0
    expect((await fresh())()).toBe(false)
    expect(asked[0]).toEqual(three)
  })

  it('is true where that context can be made, and hands it straight back', async () => {
    const { gl, loseContext } = fakeGl({})
    serve(gl, ['webgl2'])
    expect((await fresh())()).toBe(true)
    expect(loseContext).toHaveBeenCalledTimes(1)
  })

  it('does not count WebGL 1: three.js draws with WebGL 2 only', async () => {
    const { gl, loseContext } = fakeGl({})
    serve(gl, ['webgl'])
    expect((await fresh())()).toBe(false)
    expect(loseContext).not.toHaveBeenCalled()
  })

  it('asks once and keeps the answer: no second context, the same answer', async () => {
    const { gl, loseContext } = fakeGl({})
    const getContext = serve(gl, ['webgl2'])
    const webgl2ForGlobe = await fresh()
    expect([webgl2ForGlobe(), webgl2ForGlobe(), webgl2ForGlobe()]).toEqual([true, true, true])
    expect(getContext).toHaveBeenCalledTimes(1)
    expect(loseContext).toHaveBeenCalledTimes(1)
  })

  it('keeps a no the same way', async () => {
    const getContext = serve(null, [])
    const webgl2ForGlobe = await fresh()
    expect([webgl2ForGlobe(), webgl2ForGlobe()]).toEqual([false, false])
    expect(getContext).toHaveBeenCalledTimes(1)
  })

  it('is false when creating the context throws', async () => {
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => {
      throw new Error('blocklisted')
    })
    expect((await fresh())()).toBe(false)
  })
})
