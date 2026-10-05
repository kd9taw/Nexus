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
