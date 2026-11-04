// @vitest-environment jsdom
//
// THE RADIO'S FILTER IS DRAWN ON EVERY AUDIO WATERFALL AS EDGE MARKERS (2026-11-03), so the dark
// stopband above a narrow filter reads as the radio's filter and not as a waterfall cut short.
//
// The radio reports only a WIDTH. On USB/LSB data modes the demodulated audio runs up from 0 Hz,
// so the passband is 0..width (`audioRowPassband`, the Phone scope's own `boxEdges` placement): the
// 0 Hz edge is always below the view, and the upper edge is drawn at the width. The marker may not
// hide decode content: one pixel, half strength, never a fill, and drawn before every other mark
// so the RX/TX markers, tags and bands land over it.
//
// The recording-canvas harness is Waterfall.shade.test.tsx's (1000 x 200 CSS px at text scale 1, an
// 18 px axis strip, the default view 200–3000 Hz). The "draws exactly as before" control for a
// waterfall given no width is that file's fixture test, which runs every host's props unchanged.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { Waterfall } from './Waterfall'

// The house budget for a canvas test under a loaded full suite (Waterfall.width.test.tsx).
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  getSpectrumRow: () => new Promise(() => {}),
}))

type Call = { op: string; args: unknown[]; fillStyle: string; alpha: number; font: string }

function recordingCtx() {
  const calls: Call[] = []
  const state: Record<string, unknown> = {
    fillStyle: '#000000',
    strokeStyle: '#000000',
    globalAlpha: 1,
    font: '',
    lineWidth: 1,
    textAlign: 'left',
    textBaseline: 'alphabetic',
  }
  const ctx = new Proxy(state, {
    get(target, prop) {
      if (typeof prop !== 'string') return undefined
      if (prop in target) return target[prop]
      if (prop === 'measureText') return () => ({ width: 10 })
      if (prop === 'createLinearGradient') return () => ({ addColorStop() {} })
      return (...args: unknown[]) => {
        calls.push({ op: prop, args, fillStyle: String(target.fillStyle), alpha: Number(target.globalAlpha), font: String(target.font) })
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
  for (const [k, v] of Object.entries({ '--tx': '#a1b2c3', '--rx': '#c3b2a1', '--well-bg': '#010203', '--well-ink': '#fefdfc', '--mark': '#123456' }))
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

async function paint(props: Partial<React.ComponentProps<typeof Waterfall>> = {}) {
  overlay.calls.length = 0
  const r = render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" {...props} />)
  await frames(120)
  return r
}

/** Where a view `lo..hi` across W puts a frequency. */
const x = (hz: number, lo = 200, hi = 3000) => Math.round(((hz - lo) / (hi - lo)) * W)
const WF_H = H - 18
/** The filter-edge lines: 1 px wide, the picture's full height, at half strength in the scale ink. */
const edges = (f: Call[]) =>
  f
    .filter((c) => c.op === 'fillRect' && c.alpha === 0.5 && (c.args as number[])[2] === 1 && (c.args as number[])[3] === WF_H)
    .map((c) => ({ x: (c.args as number[])[0], ink: c.fillStyle }))

describe('the radio’s reported filter on an audio waterfall', () => {
  it('USB data, a 2400 Hz filter: one edge at 2400 Hz in the Std view (the 0 Hz edge is below it)', async () => {
    await paint({ passbandHz: 2400, passbandMode: 'PKTUSB' })
    expect(edges(lastFrame())).toEqual([{ x: x(2400), ink: '#fefdfc' }])
  })

  it('in Full (200–5000 Hz) a 3000 Hz filter shows its edge, which sits on Std’s right border', async () => {
    await paint({ passbandHz: 3000, passbandMode: 'USB' })
    expect(edges(lastFrame()), 'on the Std view’s own edge: nothing to mark').toEqual([])
    cleanup()
    localStorage.setItem('nexus.waterfall.ft.zoom', '-1')
    await paint({ passbandHz: 3000, passbandMode: 'USB' })
    expect(edges(lastFrame())).toEqual([{ x: x(3000, 200, 5000), ink: '#fefdfc' }])
  })

  it('it never hides a decode or a marker: drawn first, under the RX/TX markers, and never filled', async () => {
    await paint({ passbandHz: 2400, passbandMode: 'USB', markerWidthHz: 43.75 })
    const f = lastFrame()
    const edgeAt = f.findIndex((c) => c.op === 'fillRect' && c.alpha === 0.5 && (c.args as number[])[3] === WF_H)
    const firstMarker = f.findIndex((c) => c.op === 'fillRect' && (c.fillStyle === '#a1b2c3' || c.fillStyle === '#c3b2a1'))
    expect(edgeAt, 'the edge is drawn').toBeGreaterThan(0)
    expect(firstMarker, 'the markers come after it').toBeGreaterThan(edgeAt)
    // Nothing else at the edge's strength: no fill across the stopband.
    expect(f.filter((c) => c.op === 'fillRect' && c.alpha === 0.5).map((c) => (c.args as number[])[2])).toEqual([1])
  })

  it('no edge without a reported width, or where a width cannot place one (FM, CW, FSK RTTY)', async () => {
    for (const props of [
      { passbandHz: null, passbandMode: 'USB' },
      { passbandHz: undefined, passbandMode: 'USB' },
      { passbandHz: 2400, passbandMode: 'FM' },
      { passbandHz: 500, passbandMode: 'CW' },
      { passbandHz: 2400, passbandMode: 'RTTY' },
    ]) {
      await paint(props)
      expect(edges(lastFrame()), JSON.stringify(props)).toEqual([])
      cleanup()
    }
    // POSITIVE CONTROL, same harness.
    await paint({ passbandHz: 2400, passbandMode: 'USB' })
    expect(edges(lastFrame())).toHaveLength(1)
  })

  it('a new width redraws the edge where the radio now says', async () => {
    const r = await paint({ passbandHz: 2400, passbandMode: 'USB' })
    r.rerender(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" passbandHz={1800} passbandMode="USB" />)
    await frames(120)
    expect(edges(lastFrame())).toEqual([{ x: x(1800), ink: '#fefdfc' }])
  })
})
