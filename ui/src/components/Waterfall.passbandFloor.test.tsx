// @vitest-environment jsdom
//
// THE DIGITAL WATERFALL'S BLACK POINT IS SET FROM THE RECEIVER'S NOISE, INSIDE THE RADIO'S PASSBAND
// (2026-11-03). Full now reaches 5000 Hz; over a 2.4 kHz filter half of that view is the filter's
// stopband, 40 dB down, and the AGC's median would land there and light the whole passband. With the
// radio's width reported, the median is taken inside the passband (`noiseFloorWindowHz`), as the
// Phone/CW scope does. The observable is the range each row is committed in.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { Waterfall } from './Waterfall'
import { dbToSpan } from '../waterfall'
import type { SpectrumRenderer } from '../spectrum'

vi.setConfig({ testTimeout: 15_000 })

/** A post-filter audio row, 0–5000 Hz in 640 bins: noise at 0.5 below 2500 Hz, the stopband 40 dB
 *  under it, a little grain so a percentile is a noise statistic. */
vi.mock('../api', () => ({
  getSpectrumRow: vi.fn(() =>
    Promise.resolve({
      row: Array.from({ length: 640 }, (_, i) => ((i + 0.5) * 7.8125 < 2500 ? 0.5 : 0.5 - 40 / 120) + 0.004 * ((i * 7919) % 11) / 11),
      loHz: 0,
      hiHz: 5000,
      source: 'audio',
    }),
  ),
}))

const committed: { floor: number; ceil: number }[] = []
vi.mock('../spectrum', async (importActual) => {
  const actual = await importActual<typeof import('../spectrum')>()
  return {
    ...actual,
    createSpectrumRenderer: (): SpectrumRenderer => ({
      backend: 'canvas2d',
      reason: '',
      canvas: null,
      rows: 0,
      resize() {},
      commitRow(_f, range) {
        committed.push(range)
      },
      rowAt: () => null,
      clearHistory() {},
      draw() {},
      destroy() {},
    }),
  }
})

let realRaf: typeof requestAnimationFrame
let realCaf: typeof cancelAnimationFrame
beforeEach(() => {
  committed.length = 0
  localStorage.clear()
  // Full (200–5000 Hz), the view this is about.
  localStorage.setItem('nexus.waterfall.ft.zoom', '-1')
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  realRaf = globalThis.requestAnimationFrame
  realCaf = globalThis.cancelAnimationFrame
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) =>
    setTimeout(() => cb(performance.now()), 16) as unknown as number) as typeof requestAnimationFrame
  globalThis.cancelAnimationFrame = ((id: number) => clearTimeout(id as unknown as NodeJS.Timeout)) as typeof cancelAnimationFrame
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  globalThis.requestAnimationFrame = realRaf
  globalThis.cancelAnimationFrame = realCaf
  localStorage.clear()
})

async function floorWith(props: Partial<React.ComponentProps<typeof Waterfall>>): Promise<number> {
  render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1500} theme="dark" {...props} />)
  await act(() => new Promise((r) => setTimeout(r, 700)))
  expect(committed.length, 'control: rows were committed').toBeGreaterThan(2)
  return committed[committed.length - 1].floor
}

describe('the black point is the receiver’s noise, inside the passband', () => {
  it('Full over a 2.4 kHz filter: the black point sits at the passband noise, not 40 dB under it', async () => {
    const inside = await floorWith({ passbandHz: 2400, passbandMode: 'PKTUSB' })
    cleanup()
    committed.length = 0
    const blind = await floorWith({ passbandHz: null })
    // CONTROL: without a reported filter the median is the stopband's — the slab this fixes.
    expect(blind).toBeLessThan(0.5 - dbToSpan(30))
    expect(inside).toBeGreaterThan(0.5 - dbToSpan(3))
  })
})
