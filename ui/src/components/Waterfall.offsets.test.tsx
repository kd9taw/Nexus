// @vitest-environment jsdom
//
// THE RX AND TX OFFSETS A WATERFALL GESTURE SETS, BY VALUE (2026-10-04).
//
// The digital waterfall's gestures set the operator's RX and TX audio offsets, and the TX offset
// is where the next FT over is keyed. Nothing about that may move when the waterfall's picture
// changes how it is drawn, so this file pins the numbers themselves, not a formula for them:
// `__fixtures__/waterfallOffsets.json` holds every value the waterfall handed `onTune` in this
// matrix, RECORDED from the waterfall as it was before it drew through the spectrum renderer
// (`b8dcda7dd`). A difference here is a change to where the station transmits. Never re-record the
// file to make a change pass; find out why the number moved.
//
// The matrix: every view the picker offers plus WSPR's fixed sub-band, the RX marker walked across
// the passband in an order that makes a zoomed window hold still and then page (the "pan" a zoom
// does, issue #164), two canvas boxes (one offset from the page's left edge, at an odd width),
// twelve points across each box, and every gesture `tuneTarget` answers plus the two it refuses.
// Each click must call `onTune` at once, during the mousedown, with exactly the recorded hertz and
// target, cancel the browser's default exactly where it did, and the right button must never open
// a menu. The same matrix runs under each renderer backend the component can find itself on, and
// through the torn-off waterfall window, where the click reaches the engine as `setRxOffset` /
// `setTxOffset`.
//
// jsdom lays nothing out, so the one box the click handler measures is stubbed, as
// Waterfall.zoom.test.tsx and Waterfall.widthClicks.test.tsx do; the real-browser half of this
// proof (real input events, real hit-testing, both real backends) is the spectrum harness's
// `offsets` probe.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, fireEvent, cleanup, act } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { Waterfall } from './Waterfall'
import { DetachedPanel } from '../DetachedPanel'
import { setRxOffset, setTxOffset } from '../api'
import { WSPR_WATERFALL_WINDOW } from '../waterfall'
import type { SpectrumRenderer } from '../spectrum'

// THE BUDGET (2026-10-09). The slowest case here, "Std view, the RX marker walked across the passband", takes 0.35 s
// and 0.23 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one
// core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  getSpectrumRow: vi.fn(() => Promise.resolve({ row: [], loHz: 200, hiHz: 4000 })),
  setRxOffset: vi.fn(() => Promise.resolve(null)),
  setTxOffset: vi.fn(() => Promise.resolve(null)),
  // The torn-off window's other mount-time readers, answered harmlessly (DetachedPanel.test.tsx).
  subscribeSnapshot: vi.fn((cb: (s: unknown) => void) => {
    cb(popOutSnapshot)
    return () => {}
  }),
  getBandPlan: vi.fn(() => Promise.resolve([])),
  getPropagation: vi.fn(() => Promise.resolve(null)),
  getNeedAlerts: vi.fn(() => Promise.resolve([])),
  getAllSpots: vi.fn(() => Promise.resolve([])),
  getSettings: vi.fn(() => Promise.resolve(null)),
  getWindowBehind: vi.fn(() => Promise.resolve({ supported: false, on: false })),
  setWindowBehind: vi.fn(() => Promise.resolve({ supported: false, on: false })),
  getSolarIndices: vi.fn(() => Promise.resolve({ days: [] })),
}))

// The backend the component's renderer reports. `real` is the renderer itself, which jsdom (no
// canvas context of either kind) leaves inert; the other two are what a host sees in a browser.
type Backend = 'real' | 'webgl2' | 'canvas2d'
let backend: Backend = 'real'
vi.mock('../spectrum', async (importActual) => {
  const actual = await importActual<typeof import('../spectrum')>()
  return {
    ...actual,
    createSpectrumRenderer: (host: HTMLElement, opts?: Parameters<typeof actual.createSpectrumRenderer>[1]) => {
      if (backend === 'real') return actual.createSpectrumRenderer(host, opts)
      const kind = backend
      const fake: SpectrumRenderer = {
        backend: kind,
        reason: '',
        canvas: null,
        rows: 0,
        resize() {},
        commitRow() {},
        rowAt: () => null,
        clearHistory() {},
        draw() {},
        destroy() {},
      }
      return fake
    },
  }
})

/** The snapshot the torn-off window is handed, set per case before it mounts. */
let popOutSnapshot: unknown = null

interface Golden {
  recordedFrom: string
  views: { name: string; zoom?: string; fixed?: { lo: number; hi: number } }[]
  rxPath: number[]
  boxes: { left: number; width: number }[]
  fractions: number[]
  gestures: { name: string; init: Record<string, number | boolean>; target: 'rx' | 'tx' | 'both' | null; prevented: boolean }[]
  menuPrevented: boolean
  /** hz[view][box][step][fraction]: what every tuning gesture set at that point. */
  hz: number[][][][]
}
const golden = JSON.parse(
  readFileSync(resolve(process.cwd(), 'src/components/__fixtures__/waterfallOffsets.json'), 'utf8'),
) as Golden

let realRaf: typeof requestAnimationFrame
let realCaf: typeof cancelAnimationFrame
let realMatchMedia: typeof window.matchMedia
beforeEach(() => {
  localStorage.clear()
  vi.mocked(setRxOffset).mockClear()
  vi.mocked(setTxOffset).mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  // With a renderer that draws, the waterfall's draw loop runs (it stands idle where nothing can
  // draw): give it the browser's frame clock and media query, as Waterfall.markers.test.tsx does.
  realRaf = globalThis.requestAnimationFrame
  realCaf = globalThis.cancelAnimationFrame
  realMatchMedia = window.matchMedia
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) =>
    setTimeout(() => cb(performance.now()), 16) as unknown as number) as typeof requestAnimationFrame
  globalThis.cancelAnimationFrame = ((id: number) => clearTimeout(id as unknown as NodeJS.Timeout)) as typeof cancelAnimationFrame
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  localStorage.clear()
  backend = 'real'
  globalThis.requestAnimationFrame = realRaf
  globalThis.cancelAnimationFrame = realCaf
  window.matchMedia = realMatchMedia
})

/** The canvas the click handler measures, with a known box (jsdom lays nothing out). */
function canvasOf(root: ParentNode, box: { left: number; width: number }): HTMLCanvasElement {
  const canvas = root.querySelector('canvas.waterfall-canvas') as HTMLCanvasElement
  expect(canvas, 'the waterfall canvas the gestures land on').not.toBeNull()
  canvas.getBoundingClientRect = () =>
    ({ left: box.left, top: 0, right: box.left + box.width, bottom: 200, width: box.width, height: 200, x: box.left, y: 0 }) as DOMRect
  return canvas
}

/** Every gesture at every point of one box at one RX position: what reached `onTune`. */
function clickAcross(canvas: HTMLCanvasElement, box: { left: number; width: number }, onTune: ReturnType<typeof vi.fn>) {
  const out: { gesture: string; fraction: number; tuned: unknown[] | null; prevented: boolean }[] = []
  for (const g of golden.gestures) {
    for (const fraction of golden.fractions) {
      onTune.mockClear()
      const notCancelled = fireEvent.mouseDown(canvas, { ...g.init, clientX: box.left + fraction * box.width })
      // Synchronously, during the mousedown: the offset is set when the operator clicks, not later.
      expect(onTune.mock.calls.length, `${g.name} at ${fraction}: at most one tune per click`).toBeLessThanOrEqual(1)
      out.push({ gesture: g.name, fraction, tuned: onTune.mock.calls[0] ?? null, prevented: !notCancelled })
    }
  }
  return out
}

describe('the recording itself', () => {
  it('is the matrix this file says it is, and reads as the waterfall’s own axis', () => {
    expect(golden.recordedFrom).toBe('b8dcda7dd')
    expect(golden.views.map((v) => v.name)).toEqual(['Std', 'Full', '2 kHz', '1.5 kHz', '1 kHz', '600 Hz', 'WSPR'])
    expect(golden.gestures.filter((g) => g.target === null).map((g) => g.name)).toEqual(['middle', 'back'])
    expect(golden.hz).toHaveLength(golden.views.length)
    // CONTROL on the recording: two views whose window never moves, against the arithmetic a reader
    // can check by hand (Std is 200-3000 Hz, WSPR's sub-band 1400-1600 Hz), so the file cannot be a
    // table of anything else.
    const std = golden.views.findIndex((v) => v.name === 'Std')
    const wspr = golden.views.findIndex((v) => v.name === 'WSPR')
    expect(golden.views[wspr].fixed, 'the recording used WSPR’s own sub-band').toEqual(WSPR_WATERFALL_WINDOW)
    golden.boxes.forEach((_, b) =>
      golden.rxPath.forEach((_, s) =>
        golden.fractions.forEach((f, i) => {
          expect(golden.hz[std][b][s][i]).toBe(Math.round(200 + f * 2800))
          expect(golden.hz[wspr][b][s][i]).toBe(Math.round(1400 + f * 200))
        }),
      ),
    )
  })
})

describe.each(['real', 'webgl2', 'canvas2d'] as const)('every gesture sets the recorded offset, by value (renderer: %s)', (which) => {
  it.each(golden.views.map((v, i) => [v.name, i] as const))('%s view, the RX marker walked across the passband', (_name, index) => {
    backend = which
    const view = golden.views[index]
    golden.boxes.forEach((box, b) => {
      if (view.zoom != null) localStorage.setItem('nexus.waterfall.zoom', view.zoom)
      const onTune = vi.fn()
      const at = (rx: number) => (
        <Waterfall
          transmitting={false}
          rxOffsetHz={rx}
          txOffsetHz={1000}
          theme="dark"
          onTune={onTune}
          fixedWindow={view.fixed}
        />
      )
      const { container, rerender, unmount } = render(at(golden.rxPath[0]))
      const canvas = canvasOf(container, box)
      golden.rxPath.forEach((rx, s) => {
        rerender(at(rx))
        const seen = clickAcross(canvas, box, onTune)
        for (const c of seen) {
          const g = golden.gestures.find((x) => x.name === c.gesture)!
          const i = golden.fractions.indexOf(c.fraction)
          const where = `${view.name}, box ${b}, RX ${rx}, ${c.gesture} at ${c.fraction}`
          expect(c.tuned, where).toEqual(g.target ? [golden.hz[index][b][s][i], g.target] : null)
          expect(c.prevented, `${where}: default cancelled`).toBe(g.prevented)
        }
      })
      expect(!fireEvent.contextMenu(canvas), 'the right-click gesture must never open a menu').toBe(golden.menuPrevented)
      unmount()
      localStorage.clear()
    })
  })
})

describe.each(['real', 'webgl2', 'canvas2d'] as const)('the torn-off waterfall sets the same offsets on the engine (renderer: %s)', (which) => {
  // The pop-out window wires the same gestures to the engine itself: RX to setRxOffset, TX to
  // setTxOffset, both to both, each called during the click with the hertz the docked strip sets.
  const cases = [
    { view: 'Std', rx: 1500, step: 0 },
    { view: '600 Hz', rx: 2650, step: 2 },
  ]
  it.each(cases)('$view view, RX at $rx', async ({ view: name, rx, step }) => {
    backend = which
    const v = golden.views.findIndex((x) => x.name === name)
    const view = golden.views[v]
    // The 600 Hz case reaches RX 2650 the way the recording did: from 1500, held across 1520,
    // paged when the marker left. The torn-off window mounts on the default 1500 and then hears
    // the station's 2650, which pages the same window.
    expect(golden.rxPath[step]).toBe(rx)
    if (view.zoom != null) localStorage.setItem('nexus.waterfall.zoom', view.zoom)
    popOutSnapshot = {
      radio: { transmitting: false, rxOffsetHz: rx, txOffsetHz: 1000 },
      link: { tier: 'FT8', periodSecs: 15 },
    }
    let root!: ReturnType<typeof render>
    await act(async () => {
      root = render(<DetachedPanel panel="waterfall" />)
    })
    const box = golden.boxes[0]
    const canvas = canvasOf(root.container, box)
    for (const g of golden.gestures) {
      for (const [i, fraction] of golden.fractions.entries()) {
        vi.mocked(setRxOffset).mockClear()
        vi.mocked(setTxOffset).mockClear()
        fireEvent.mouseDown(canvas, { ...g.init, clientX: box.left + fraction * box.width })
        const hz = golden.hz[v][0][step][i]
        const where = `${name}, ${g.name} at ${fraction}`
        expect(vi.mocked(setRxOffset).mock.calls, `${where}: RX`).toEqual(g.target === 'rx' || g.target === 'both' ? [[hz]] : [])
        expect(vi.mocked(setTxOffset).mock.calls, `${where}: TX`).toEqual(g.target === 'tx' || g.target === 'both' ? [[hz]] : [])
      }
    }
  })
})
