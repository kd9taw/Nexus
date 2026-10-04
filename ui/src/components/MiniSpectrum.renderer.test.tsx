// @vitest-environment jsdom
//
// WHAT THE MINI SPECTRUM HANDS THE SPECTRUM RENDERER (2026-10-04).
//
// The strip's trace is drawn by the renderer, as every scope's is, and keeps its own look: the
// accent on the well's face (DISPLAY WELLS). So the scene is pinned by value: a table from the face
// (entry 0, the floor the band is filled with) to the accent (entry 255, the line), the whole row
// across the strip in its own span, peak per pixel, and the range the strip has always drawn in
// (the shared visual AGC with a 10 dB minimum span). The renderer is a recording stand-in: jsdom has
// no canvas context, and the drawing itself is the harness's to check.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, render, waitFor } from '@testing-library/react'
import { MiniSpectrum } from './MiniSpectrum'
import { getSpectrumRow } from '../api'
import { agcRange, dbToSpan } from '../waterfall'
import type { SpectrumScene } from '../spectrum'

vi.mock('../api', () => ({ getSpectrumRow: vi.fn() }))
const scenes: SpectrumScene[] = []
const sizes: [number, number][] = []
vi.mock('../spectrum', async (importActual) => ({
  ...(await importActual<typeof import('../spectrum')>()),
  createSpectrumRenderer: () => ({
    backend: 'canvas2d',
    reason: '',
    canvas: null,
    rows: 0,
    resize: (w: number, h: number) => sizes.push([w, h]),
    commitRow() {},
    rowAt: () => null,
    clearHistory() {},
    draw: (s: SpectrumScene) => scenes.push(s),
    destroy() {},
  }),
}))

/** The two inks jsdom hands back (no stylesheet is loaded): the component's own defaults. */
const FACE = '#0b0f17'
const ACCENT = '#4ea1ff'
const RGB: Record<string, number[]> = { [FACE]: [11, 15, 23], [ACCENT]: [78, 161, 255] }
const ROW = Array.from({ length: 512 }, (_, i) => 0.25 + (i % 7) * 0.004 + (i === 300 ? 0.4 : 0))

beforeEach(() => {
  scenes.length = 0
  sizes.length = 0
  vi.mocked(getSpectrumRow).mockResolvedValue({ row: ROW, loHz: 0, hiHz: 4000, source: 'audio' } as never)
  // The 1x1 canvas the inks are measured on paints what it is handed.
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => {
    let colour = ''
    return {
      set fillStyle(v: string) {
        colour = v
      },
      fillRect() {},
      getImageData: () => ({ data: [...(RGB[colour] ?? [0, 0, 0]), 255] }),
    } as unknown as CanvasRenderingContext2D
  })
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(300)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(84)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

it('draws the row as an accent trace on the well, in the range the strip has always used', async () => {
  render(<MiniSpectrum pollMs={30} />)
  await waitFor(() => expect(scenes.length).toBeGreaterThan(0))
  const s = scenes[scenes.length - 1]
  const dpr = window.devicePixelRatio || 1
  expect([...s.lut.slice(0, 3)], 'entry 0: the well face').toEqual(RGB[FACE])
  expect([...s.lut.slice(255 * 4, 255 * 4 + 3)], 'entry 255: the accent').toEqual(RGB[ACCENT])
  expect(s.layout).toEqual({ traceH: Math.round(84 * dpr), stripH: 0, lineWidth: 1.25 * dpr })
  expect(sizes[sizes.length - 1]).toEqual([Math.round(300 * dpr), Math.round(84 * dpr)])
  expect([s.mode, s.detector]).toEqual(['2d', 'peak'])
  expect(s.view).toEqual({ loHz: 0, hiHz: 4000 })
  const { floor, ceil } = agcRange(ROW)
  expect(s.trace!.range).toEqual({ floor, ceil: Math.max(ceil, floor + dbToSpan(10)) })
  expect(s.trace!.frame.bins).toBe(ROW)
  expect([s.trace!.frame.loHz, s.trace!.frame.hiHz, s.trace!.frame.dbPerUnit]).toEqual([0, 4000, 120])
})
