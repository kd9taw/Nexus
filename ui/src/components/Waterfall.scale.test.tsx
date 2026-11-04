// @vitest-environment jsdom
//
// THE WATERFALL'S FREQUENCY SCALE FOLLOWS ITS SPAN, AND NO MARK IS DRAWN THROUGH IT.
//
// Every mode on this component (FT, JS8, RTTY, PSK, SSTV, JTTY, the Tempo strip, the torn-off copy)
// reads audio hertz along its foot. Std and Full keep the 500 Hz the operator reads on FT8; a zoomed
// view steps finer (`freqScale.ts`), where it used to keep 500 Hz down to 1 kHz and leave three numbers
// across the whole window. Read off the overlay's own calls through the shade test's recording
// harness: a known 1000 × 200 CSS px box at text scale 1, an 18 px strip.
import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { Waterfall } from './Waterfall'
import { css, loadSheets } from '../cssCascade.testkit'
import { scaleStripHeight } from '../freqScale'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  getSpectrumRow: () => new Promise(() => {}),
}))

type Call = { op: string; args: unknown[]; fillStyle: string; alpha: number }

function recordingCtx() {
  const calls: Call[] = []
  const state: Record<string, unknown> = { fillStyle: '#000000', strokeStyle: '#000000', globalAlpha: 1, font: '', textAlign: 'left', textBaseline: 'alphabetic' }
  const ctx = new Proxy(state, {
    get(target, prop) {
      if (typeof prop !== 'string') return undefined
      if (prop in target) return target[prop]
      if (prop === 'measureText') return (s: string) => ({ width: s.length * 6 })
      if (prop === 'createLinearGradient') return () => ({ addColorStop() {} })
      return (...args: unknown[]) => {
        calls.push({ op: prop, args, fillStyle: String(target.fillStyle), alpha: Number(target.globalAlpha) })
      }
    },
    set(target, prop, value) {
      if (typeof prop === 'string') target[prop] = value
      return true
    },
  })
  return { ctx, calls }
}

const W = 1000
const H = 200
const STRIP = 18
const WF_H = H - STRIP
const TX = '#a1b2c3'
const RX = '#c3b2a1'
const INK = '#fefdfc'

let overlay: ReturnType<typeof recordingCtx>
let realRaf: typeof requestAnimationFrame
let realCaf: typeof cancelAnimationFrame

beforeEach(() => {
  localStorage.clear()
  overlay = recordingCtx()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    return (this.classList.contains('waterfall-overlay') ? overlay.ctx : null) as unknown as CanvasRenderingContext2D
  } as unknown as typeof HTMLCanvasElement.prototype.getContext)
  vi.spyOn(HTMLCanvasElement.prototype, 'getBoundingClientRect').mockImplementation(
    () => ({ left: 0, top: 0, right: W, bottom: H, width: W, height: H, x: 0, y: 0, toJSON() {} }) as DOMRect,
  )
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList) as typeof window.matchMedia
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
  for (const [k, v] of Object.entries({ '--tx': TX, '--rx': RX, '--well-bg': '#010203', '--well-ink': INK, '--mark': '#123456' }))
    document.documentElement.style.setProperty(k, v)
})

afterEach(() => {
  cleanup()
  globalThis.requestAnimationFrame = realRaf
  globalThis.cancelAnimationFrame = realCaf
  document.documentElement.removeAttribute('style')
  localStorage.clear()
  vi.restoreAllMocks()
})

const frames = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)))
function lastFrame(): Call[] {
  const at = overlay.calls.map((c) => c.op).lastIndexOf('clearRect')
  return overlay.calls.slice(at)
}
async function paint(props: Partial<React.ComponentProps<typeof Waterfall>> = {}, zoom?: string) {
  overlay.calls.length = 0
  if (zoom != null) localStorage.setItem('nexus.waterfall.ft.zoom', zoom)
  render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" {...props} />)
  await frames(120)
  return lastFrame()
}
/** The scale's labels: the digits drawn in the well's ink. */
const scale = (f: Call[]) => f.filter((c) => c.op === 'fillText' && c.fillStyle === INK && /^\d+$/.test(String(c.args[0]))).map((c) => Number(c.args[0]))

describe('the scale follows the span', () => {
  it('Std reads 500 Hz steps, as FT8 always has', async () => {
    expect(scale(await paint())).toEqual([500, 1000, 1500, 2000, 2500, 3000])
  })
  it('Full reads 500 Hz steps to 5000', async () => {
    expect(scale(await paint({}, '-1'))).toEqual([500, 1000, 1500, 2000, 2500, 3000, 3500, 4000, 4500, 5000])
  })
  it('2 kHz reads every 200 Hz', async () => {
    expect(scale(await paint({}, '2000'))).toEqual([600, 800, 1000, 1200, 1400, 1600, 1800, 2000, 2200, 2400])
  })
  it('1 kHz reads every 100 Hz, where it read three numbers', async () => {
    expect(scale(await paint({}, '1000'))).toEqual([1000, 1100, 1200, 1300, 1400, 1500, 1600, 1700, 1800, 1900, 2000])
  })
  it('600 Hz reads every 100 Hz', async () => {
    expect(scale(await paint({}, '600'))).toEqual([1200, 1300, 1400, 1500, 1600, 1700, 1800])
  })
  it('a mode’s own span key (RTTY’s 1k button) gets the same scale', async () => {
    localStorage.setItem('nexus.waterfall.rtty.zoom', '1000')
    overlay.calls.length = 0
    render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1500} theme="dark" zoomKey="nexus.waterfall.rtty.zoom" />)
    await frames(120)
    expect(scale(lastFrame())).toEqual([1000, 1100, 1200, 1300, 1400, 1500, 1600, 1700, 1800, 1900, 2000])
  })
})

describe('no mark is drawn through the scale', () => {
  /** Every fillRect in a marker's or the filter edge's colour, as [x, y, w, h]. */
  const rects = (f: Call[], fill: string) => f.filter((c) => c.op === 'fillRect' && c.fillStyle === fill).map((c) => c.args as number[])
  it('the RX/TX markers, their labels and the filter-edge marks all stop above the strip, at every span', async () => {
    for (const zoom of ['0', '-1', '2000', '1000', '600']) {
      cleanup()
      const f = await paint({ markerWidthHz: 50, txOffsetHz: 1400, passbandHz: 2400, passbandMode: 'USB' }, zoom)
      for (const fill of [TX, RX]) {
        const rs = rects(f, fill)
        expect(rs.length, `${zoom}: ${fill} drawn`).toBeGreaterThan(0)
        for (const [, y, , h] of rs) expect(y + h, `${zoom}: a ${fill} mark runs into the strip`).toBeLessThanOrEqual(WF_H)
      }
      for (const c of f.filter((c) => c.op === 'fillText' && ['TX', 'RX'].includes(String(c.args[0]))))
        expect(Number(c.args[2]), `${zoom}: the ${c.args[0]} label sits in the strip`).toBeLessThan(WF_H)
      // The filter edge: the scale ink at half strength, a 1 px line over the picture only.
      const edges = f.filter((c) => c.op === 'fillRect' && c.fillStyle === INK && (c.args[2] as number) === 1 && (c.args[1] as number) === 0)
      for (const e of edges) expect((e.args[3] as number), `${zoom}: the filter edge runs into the strip`).toBeLessThanOrEqual(WF_H)
      // Every scale label is drawn inside the strip, after its ground.
      const ground = f.findIndex((c) => c.op === 'fillRect' && (c.args as number[]).join() === `0,${WF_H},${W},${STRIP}`)
      expect(ground, `${zoom}: the strip's ground`).toBeGreaterThanOrEqual(0)
      for (const c of f.filter((c) => c.op === 'fillText' && /^\d+$/.test(String(c.args[0])))) {
        expect(Number(c.args[2])).toBe(WF_H + STRIP / 2)
        expect(f.indexOf(c)).toBeGreaterThan(ground)
      }
    }
  })
  it('no two labels overlap, and the last stays inside the edge', async () => {
    for (const zoom of ['0', '-1', '2000', '1000', '600']) {
      cleanup()
      const labels = (await paint({}, zoom)).filter((c) => c.op === 'fillText' && /^\d+$/.test(String(c.args[0])))
      for (let i = 1; i < labels.length; i++) {
        const prevEnd = (labels[i - 1].args[1] as number) + String(labels[i - 1].args[0]).length * 6
        expect(labels[i].args[1] as number, `${zoom}: ${labels[i].args[0]} overlaps ${labels[i - 1].args[0]}`).toBeGreaterThan(prevEnd)
      }
      const last = labels[labels.length - 1]
      expect((last.args[1] as number) + String(last.args[0]).length * 6, zoom).toBeLessThanOrEqual(W)
    }
  })
})

describe('the colour legend stays off the scale', () => {
  // The legend is a fixed-height overlay at the top right. On a short waterfall (175 % UI zoom, a
  // divider dragged down) it ran down over the strip and covered the last number. Through the real
  // cascade: the legend is held inside its stage, above the strip's tallest height and its own gap.
  beforeAll(() => loadSheets())
  it('its winning max-height ends a gap above the 18 px strip, and what cannot fit is clipped', () => {
    const { container } = render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" />)
    const legend = container.querySelector('.wf-legend')!
    const stage = legend.parentElement!
    expect(stage.classList.contains('wf-stage')).toBe(true)
    expect(css(stage, 'position')).toBe('relative')
    expect(css(legend, 'position')).toBe('absolute')
    const top = parseFloat(css(legend, 'top') ?? '')
    const max = /^calc\(100% - (\d+)px\)$/.exec(css(legend, 'max-height') ?? '')
    expect(max, `max-height is ${css(legend, 'max-height')}`).not.toBeNull()
    // Its top inset, the tallest strip (18 px at text scale 1: the zoom scales both alike), and the same gap again.
    expect(Number(max![1])).toBeGreaterThanOrEqual(top + scaleStripHeight(200, 1) + top)
    expect(css(legend, 'overflow')).toBe('hidden')
  })
})

describe('a legend squeezed to no bar is hidden, not left as a lone "0"', () => {
  // At 175 % on a short waterfall the legend, held off the scale, kept only its top tick: a "0" in the
  // corner that reads like a frequency. With its colour bar under 12 px it is hidden whole.
  beforeAll(() => loadSheets())
  const withBar = async (h: number) => {
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (this: HTMLElement) {
      return this.classList.contains('wf-legend-bar') ? h : 0
    })
    const { container } = render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" />)
    await frames(60)
    return container.querySelector('.wf-legend')!
  }
  it('a 4 px bar: the legend is hidden through the cascade', async () => {
    const legend = await withBar(4)
    expect(legend.hasAttribute('data-squeezed')).toBe(true)
    expect(css(legend, 'visibility')).toBe('hidden')
  })
  it('control: a full 56 px bar shows the legend', async () => {
    const legend = await withBar(56)
    expect(legend.hasAttribute('data-squeezed')).toBe(false)
    expect(css(legend, 'visibility')).not.toBe('hidden')
  })
})
