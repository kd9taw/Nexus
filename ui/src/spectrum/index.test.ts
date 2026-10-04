// @vitest-environment jsdom
//
// jsdom has neither a WebGL2 nor a 2D context, which is exactly the environment every component
// suite that will mount the renderer runs in. There the renderer must stay inert (draw nothing,
// throw nothing) while still keeping its history, so those suites can drive a host through it.
// What it draws is proved in a real browser (ui/spectrum-harness).
import { describe, expect, it } from 'vitest'
import { createSpectrumRenderer } from './index'
import type { SpectrumScene } from './types'

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
