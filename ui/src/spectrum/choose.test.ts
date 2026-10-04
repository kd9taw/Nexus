// @vitest-environment jsdom
//
// The choice's pure half: what a renderer string and the probe's times decide, and the hidden
// setting. The probe itself draws, so it is proved in a real browser (ui/spectrum-harness,
// `capability`); how the renderer acts on all of this is in index.test.ts.
import { afterEach, describe, expect, it } from 'vitest'
import { askedFor, byName, byTimes, rendererName } from './choose'

describe('byName', () => {
  it.each([
    'ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero) (0x0000C0DE)), SwiftShader driver)',
    'llvmpipe (LLVM 15.0.7, 256 bits)',
    'softpipe',
    'ANGLE (Microsoft, Microsoft Basic Render Driver (0x0000008C) Direct3D11 vs_5_0 ps_5_0, D3D11)',
    'Apple Software Renderer',
  ])('sends a software rasteriser to canvas-2D: %s', (name) => {
    expect(byName(name)).toEqual({ pick: 'canvas2d', why: `WebGL2 is software-rendered here (${name}); canvas-2D is faster` })
  })

  it.each([
    'ANGLE (NVIDIA, NVIDIA GeForce RTX 3060 (0x00002503) Direct3D11 vs_5_0 ps_5_0, D3D11)',
    'ANGLE (Intel, Intel(R) UHD Graphics 620 (0x00005917) Direct3D11 vs_5_0 ps_5_0, D3D11)',
    'Mesa Intel(R) UHD Graphics 620 (KBL GT2)',
    // A GPU's string can name LLVM (Mesa's shader compiler) without being llvmpipe.
    'AMD Radeon RX 6600 (radeonsi, navi23, LLVM 15.0.7, DRM 3.49, 6.1.0)',
    'Apple M1',
    'V3D 4.2',
  ])('keeps WebGL2 for a GPU: %s', (name) => {
    expect(byName(name)).toEqual({ pick: 'webgl2', why: '' })
  })

  it('leaves a masked string to the probe', () => {
    expect(byName(null)).toBeNull()
  })
})

describe('rendererName', () => {
  const UNMASKED_RENDERER = 0x9246
  const context = (ext: object | null, value: unknown) =>
    ({
      getExtension: (name: string) => (name === 'WEBGL_debug_renderer_info' ? ext : null),
      getParameter: (p: number) => (p === UNMASKED_RENDERER ? value : null),
    }) as unknown as WebGL2RenderingContext
  const ext = { UNMASKED_RENDERER_WEBGL: UNMASKED_RENDERER }

  it('reads the unmasked renderer string', () => {
    expect(rendererName(context(ext, 'llvmpipe (LLVM 15.0.7, 256 bits)'))).toBe('llvmpipe (LLVM 15.0.7, 256 bits)')
  })

  it('is null where the extension is masked, or the string is empty', () => {
    expect(rendererName(context(null, 'llvmpipe'))).toBeNull()
    expect(rendererName(context(ext, ''))).toBeNull()
    expect(rendererName(context(ext, null))).toBeNull()
  })
})

describe('byTimes', () => {
  it('hands over to canvas-2D only when WebGL2 is more than twice as slow', () => {
    // SwiftShader in headless Chrome, as the probe timed it (2026-10-04).
    expect(byTimes(2.06, 0.03).pick).toBe('canvas2d')
    expect(byTimes(2.01, 1).pick).toBe('canvas2d')
    expect(byTimes(2, 1).pick).toBe('webgl2')
    expect(byTimes(0.05, 0.3).pick).toBe('webgl2')
  })

  it('keeps WebGL2 under 1 ns a pixel, however cheap canvas-2D is', () => {
    // The probe's band is 1024×256 px: the floor is 0.262 ms a frame. A GPU's time there is mostly
    // the read-back's round trip, and canvas-2D can be cheaper still on a fast CPU.
    expect(byTimes(0.26, 0.03).pick).toBe('webgl2')
    expect(byTimes(0.27, 0.03).pick).toBe('canvas2d')
    // Both too quick for the clock: nothing to hand over for.
    expect(byTimes(0, 0).pick).toBe('webgl2')
  })

  it('says what it timed, and gives WebGL2 no reason', () => {
    expect(byTimes(2.06, 0.03)).toEqual({ pick: 'canvas2d', why: 'the probe timed WebGL2 at 2.06 ms a frame and canvas-2D at 0.03 ms' })
    expect(byTimes(0.05, 0.3)).toEqual({ pick: 'webgl2', why: '' })
  })
})

describe('askedFor (the hidden setting)', () => {
  afterEach(() => localStorage.clear())

  it('takes webgl2 or canvas2d, and nothing else', () => {
    expect(askedFor()).toBeNull()
    localStorage.setItem('nexus.spectrum.backend', 'webgl2')
    expect(askedFor()).toBe('webgl2')
    localStorage.setItem('nexus.spectrum.backend', 'canvas2d')
    expect(askedFor()).toBe('canvas2d')
    localStorage.setItem('nexus.spectrum.backend', 'WebGL2')
    expect(askedFor()).toBeNull()
  })
})
