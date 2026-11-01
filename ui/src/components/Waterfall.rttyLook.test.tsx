// @vitest-environment jsdom
//
// THE RTTY WATERFALL'S LOOK — the contrast curve, by value at known dB; the row cadence that makes
// the scroll smooth; the span buttons; and the callsign tags drawn at their frequency.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act, screen, fireEvent } from '@testing-library/react'
import { Waterfall } from './Waterfall'
import { getSpectrumRow } from '../api'
import { RTTY_WATERFALL_SPANS, bakeLut, curveForPalette, dbToSpan, paletteRamp, traceContrast } from '../waterfall'
import { lutIndex } from '../spectrum/aggregate'
import type { SpectrumRenderer, SpectrumScene } from '../spectrum'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({ getSpectrumRow: vi.fn() }))

const scenes: SpectrumScene[] = []
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
      commitRow() {},
      rowAt: () => null,
      clearHistory() {},
      draw: (scene) => scenes.push(scene),
      destroy() {},
    }),
  }
})

// A clock and an animation frame the test turns by hand: `frames(n, ms)` runs n frames ms apart.
let clock = 0
let queue: FrameRequestCallback[] = []
let realRaf: typeof requestAnimationFrame
let realCaf: typeof cancelAnimationFrame
async function frames(n: number, ms: number) {
  for (let i = 0; i < n; i++) {
    clock += ms
    const due = queue
    queue = []
    await act(async () => {
      for (const cb of due) cb(clock)
      // Let a fetch's settle and the latch release land before the next frame.
      for (let k = 0; k < 5; k++) await Promise.resolve()
    })
  }
}

beforeEach(() => {
  scenes.length = 0
  localStorage.clear()
  clock = 1000
  queue = []
  vi.spyOn(performance, 'now').mockImplementation(() => clock)
  vi.mocked(getSpectrumRow).mockReset().mockResolvedValue({ row: [], loHz: 0, hiHz: 4000, source: 'audio' })
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => null)
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  realRaf = globalThis.requestAnimationFrame
  realCaf = globalThis.cancelAnimationFrame
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => queue.push(cb)) as typeof requestAnimationFrame
  globalThis.cancelAnimationFrame = (() => {}) as typeof cancelAnimationFrame
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  globalThis.requestAnimationFrame = realRaf
  globalThis.cancelAnimationFrame = realCaf
  vi.restoreAllMocks()
})

const base = { transmitting: false, rxOffsetHz: 1500, txOffsetHz: 0, theme: 'dark' }

/** Relative luminance of LUT entry i. */
function lum(lut: Uint8ClampedArray, i: number): number {
  const lin = (c: number) => (c / 255 <= 0.04045 ? c / 255 / 12.92 : ((c / 255 + 0.055) / 1.055) ** 2.4)
  return 0.2126 * lin(lut[i * 4]) + 0.7152 * lin(lut[i * 4 + 1]) + 0.0722 * lin(lut[i * 4 + 2])
}
const rgb = (lut: Uint8ClampedArray, i: number) => [lut[i * 4], lut[i * 4 + 1], lut[i * 4 + 2]]

describe('the RTTY contrast curve, at known dB', () => {
  // The waterfall's usual window: 24 dB over the parked floor. Noise grain 3 dB up, a keying
  // mark/space pair 15 dB up.
  const floor = 0.2
  const ceil = floor + dbToSpan(24)
  const noise = lutIndex(floor + dbToSpan(3), floor, ceil)
  const trace = lutIndex(floor + dbToSpan(15), floor, ceil)

  it('inferno: the grain darker, the trace brighter, the contrast between them doubled', () => {
    expect([noise, trace]).toEqual([32, 159])
    const lin = bakeLut('inferno')
    const rtty = bakeLut('inferno', 256, traceContrast)
    expect(rgb(lin, noise)).toEqual([27, 12, 65])
    expect(rgb(rtty, noise)).toEqual([9, 3, 31])
    expect(rgb(lin, trace)).toEqual([207, 68, 70])
    expect(rgb(rtty, trace)).toEqual([244, 131, 26])
    const ratio = (l: Uint8ClampedArray) => (lum(l, trace) + 0.05) / (lum(l, noise) + 0.05)
    expect(ratio(lin)).toBeCloseTo(3.88, 1)
    expect(ratio(rtty)).toBeCloseTo(7.77, 1)
  })

  it('viridis too (the contrast 3.9 → 7.5)', () => {
    const ratio = (l: Uint8ClampedArray) => (lum(l, trace) + 0.05) / (lum(l, noise) + 0.05)
    expect(ratio(bakeLut('viridis'))).toBeCloseTo(3.86, 1)
    expect(ratio(bakeLut('viridis', 256, traceContrast))).toBeCloseTo(7.54, 1)
  })

  it('the curve keeps the ends and is monotonic', () => {
    expect(traceContrast(0)).toBe(0)
    expect(traceContrast(1)).toBe(1)
    for (let i = 1; i <= 100; i++) expect(traceContrast(i / 100)).toBeGreaterThanOrEqual(traceContrast((i - 1) / 100))
  })

  it('turbo (the default palette): the curve ends at its yellow-green, and the contrast still rises', () => {
    expect(paletteRamp('turbo')).toEqual({ top: 0.6, rises: true })
    const lin = bakeLut('turbo')
    const rtty = bakeLut('turbo', 256, curveForPalette('turbo', traceContrast))
    expect(rgb(rtty, noise)).toEqual([3, 3, 14])
    expect(rgb(rtty, trace)).toEqual([61, 241, 152])
    const ratio = (l: Uint8ClampedArray) => (lum(l, trace) + 0.05) / (lum(l, noise) + 0.05)
    expect(ratio(lin)).toBeCloseTo(11.64, 1)
    expect(ratio(rtty)).toBeCloseTo(13.91, 1)
  })

  it('a palette that darkens on the way to its brightest takes no curve', () => {
    for (const n of ['digipan', 'linrad', 'negative'] as const) expect(curveForPalette(n, traceContrast), n).toBeUndefined()
    expect(curveForPalette('inferno', traceContrast)).toBe(traceContrast)
  })

  it('the waterfall draws through the curve it is handed, and the plain map without one', async () => {
    localStorage.setItem('nexus.waterfall.palette', 'inferno')
    render(<Waterfall {...base} contrastCurve={traceContrast} />)
    await frames(2, 16.6)
    expect(Array.from(scenes[scenes.length - 1].lut)).toEqual(Array.from(bakeLut('inferno', 256, traceContrast)))
    cleanup()
    scenes.length = 0
    localStorage.setItem('nexus.waterfall.palette', 'inferno')
    render(<Waterfall {...base} />)
    await frames(2, 16.6)
    expect(Array.from(scenes[scenes.length - 1].lut)).toEqual(Array.from(bakeLut('inferno')))
  })

  it('turbo, the default palette, draws through the fitted curve', async () => {
    localStorage.setItem('nexus.waterfall.palette', 'turbo')
    render(<Waterfall {...base} contrastCurve={traceContrast} />)
    await frames(2, 16.6)
    expect(Array.from(scenes[scenes.length - 1].lut)).toEqual(Array.from(bakeLut('turbo', 256, curveForPalette('turbo', traceContrast))))
  })
})

describe('the row cadence', () => {
  // Only the RTTY cockpit keeps the remainder. Every other caller keeps the cadence it had: a row on
  // the first frame at or past `rowMs`, the overshoot dropped. These counts are that behaviour,
  // measured on the component before the remainder was kept.
  it('without keepRowRemainder (FT): 120 ms rows land every 8 frames, 15 rows in 2 s, not 16-17', async () => {
    render(<Waterfall {...base} />)
    await frames(1, 16.6)
    vi.mocked(getSpectrumRow).mockClear()
    await frames(120, 16.6) // 1992 ms
    expect(vi.mocked(getSpectrumRow).mock.calls.length).toBe(15)
  })

  it('without keepRowRemainder a 50 ms row still takes 4 frames: 30 rows in 2 s, not 40', async () => {
    render(<Waterfall {...base} rowMs={50} />)
    await frames(1, 16.6)
    vi.mocked(getSpectrumRow).mockClear()
    await frames(120, 16.6)
    expect(vi.mocked(getSpectrumRow).mock.calls.length).toBe(30)
  })

  it('a 50 ms row on a display a little faster than 60 Hz: 40 rows in 2 s, not one every 4 frames', async () => {
    render(<Waterfall {...base} rowMs={50} keepRowRemainder />)
    await frames(1, 16.6)
    vi.mocked(getSpectrumRow).mockClear()
    await frames(120, 16.6) // 1992 ms
    const n = vi.mocked(getSpectrumRow).mock.calls.length
    expect(n, `${n} rows in 1992 ms`).toBeGreaterThanOrEqual(39)
    expect(n).toBeLessThanOrEqual(40)
  })

  it('the speed is the row time: 200 ms rows are a quarter of 50 ms rows', async () => {
    render(<Waterfall {...base} rowMs={200} keepRowRemainder />)
    await frames(1, 16.6)
    vi.mocked(getSpectrumRow).mockClear()
    await frames(120, 16.6)
    expect(vi.mocked(getSpectrumRow).mock.calls.length).toBe(10)
  })

  it('a slow fetch does not come back as a burst of rows', async () => {
    let release: (v: unknown) => void = () => {}
    vi.mocked(getSpectrumRow).mockImplementationOnce(() => new Promise((r) => (release = r)) as never)
    render(<Waterfall {...base} rowMs={50} keepRowRemainder />)
    await frames(4, 16.6) // the first fetch starts, and hangs
    expect(vi.mocked(getSpectrumRow).mock.calls.length).toBe(1)
    await frames(20, 16.6) // 332 ms held
    expect(vi.mocked(getSpectrumRow).mock.calls.length).toBe(1)
    await act(async () => release({ row: [], loHz: 0, hiHz: 4000, source: 'audio' }))
    vi.mocked(getSpectrumRow).mockClear()
    await frames(3, 16.6)
    expect(vi.mocked(getSpectrumRow).mock.calls.length, 'rows in the 3 frames after the stall').toBeLessThanOrEqual(2)
  })
})

describe('the span buttons', () => {
  const tag = { key: 'W1AW', hz: 1600, label: 'W1AW', title: 'W1AW', opacity: 1, dim: false }
  const leftOf = () => parseFloat((screen.getByRole('button', { name: 'W1AW' }) as HTMLElement).style.left)

  it('replace the select, set the span, persist it, and move what is drawn', async () => {
    render(<Waterfall {...base} spanChoices={RTTY_WATERFALL_SPANS} zoomKey="nexus.waterfall.rtty.zoom" tags={[tag]} />)
    expect(screen.queryByRole('combobox', { name: 'Waterfall zoom span' })).toBeNull()
    const group = screen.getByRole('group', { name: 'Waterfall span' })
    const btn = (label: string) => Array.from(group.querySelectorAll('button')).find((b) => b.textContent === label)!
    // Std (200–3000 Hz) by default: 1600 Hz is half way.
    expect(btn('3k').getAttribute('aria-pressed')).toBe('true')
    expect(leftOf()).toBeCloseTo(50, 6)
    fireEvent.click(btn('1k'))
    expect(localStorage.getItem('nexus.waterfall.rtty.zoom')).toBe('1000')
    // 1 kHz round the decoder at 1500: 1000–2000 Hz, so 1600 Hz is 60 % across.
    expect(leftOf()).toBeCloseTo(60, 6)
    fireEvent.click(btn('4k'))
    expect(localStorage.getItem('nexus.waterfall.rtty.zoom')).toBe('-1')
    expect(leftOf()).toBeCloseTo((1400 / 3800) * 100, 6)
    cleanup()
    render(<Waterfall {...base} spanChoices={RTTY_WATERFALL_SPANS} zoomKey="nexus.waterfall.rtty.zoom" tags={[tag]} />)
    expect(screen.getByRole('button', { name: '4k' }).getAttribute('aria-pressed')).toBe('true')
    expect(leftOf()).toBeCloseTo((1400 / 3800) * 100, 6)
  })

  it('every button value is one the zoom select also offers (one persisted vocabulary)', async () => {
    const { WATERFALL_ZOOMS } = await import('../waterfall')
    for (const s of RTTY_WATERFALL_SPANS) expect(WATERFALL_ZOOMS.some((z) => z.value === s.value), s.label).toBe(true)
  })
})

describe('the tags', () => {
  const tags = [
    { key: 'W1AW', hz: 1600, label: 'W1AW', title: 'Net onto W1AW', opacity: 1, dim: false },
    { key: 'K5LLL', hz: 3500, label: 'K5LLL', title: 'Net onto K5LLL', opacity: 0.5, dim: true },
  ]
  it('drawn at their frequency, only inside the view, and a click hands back the frequency', () => {
    const onTag = vi.fn()
    render(<Waterfall {...base} tags={tags} onTag={onTag} />)
    const w1aw = screen.getByRole('button', { name: 'W1AW' })
    expect(w1aw.style.left).toBe('50%')
    expect(screen.queryByRole('button', { name: 'K5LLL' }), '3500 Hz is outside 200–3000').toBeNull()
    fireEvent.click(w1aw)
    expect(onTag).toHaveBeenCalledWith(1600)
  })

  it('opacity and the dim class are the host’s', () => {
    render(<Waterfall {...base} tags={tags} onTag={() => {}} fixedWindow={{ lo: 200, hi: 4000 }} />)
    const k = screen.getByRole('button', { name: 'K5LLL' })
    expect(k.style.opacity).toBe('0.5')
    expect(k.className).toBe('wf-tag dim')
    expect(screen.getByRole('button', { name: 'W1AW' }).className).toBe('wf-tag')
  })

  it('while keyed a tag is disabled and a click does nothing; with no handler it is only a label', () => {
    const onTag = vi.fn()
    render(<Waterfall {...base} tags={tags} onTag={onTag} keyed />)
    const w1aw = screen.getByRole('button', { name: 'W1AW' }) as HTMLButtonElement
    expect(w1aw.disabled).toBe(true)
    fireEvent.click(w1aw)
    expect(onTag).not.toHaveBeenCalled()
    cleanup()
    render(<Waterfall {...base} tags={tags} />)
    expect((screen.getByRole('button', { name: 'W1AW' }) as HTMLButtonElement).disabled).toBe(true)
  })
})
