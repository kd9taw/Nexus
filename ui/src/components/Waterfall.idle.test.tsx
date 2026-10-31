// @vitest-environment jsdom
//
// THE WATERFALL'S DRAW LOOP SLEEPS WHEN THERE IS NOTHING TO DRAW (2026-10-30).
//
// Two kinds of wasted work, both found measuring an idle Operate screen in real Chrome:
//   1. A waterfall kept mounted but hidden (`active={false}`: the cockpit is navigated away, or the
//      waterfall is popped out) re-armed `requestAnimationFrame` every frame to do nothing. Three are
//      mounted, so 120–180 empty callbacks a second ran forever, and a pending rAF keeps the browser's
//      frame pipeline awake at the display rate. Hidden, the loop now stops; shown again, it restarts.
//   2. PAUSED, the overlay (pause chip and time tape) was cleared and redrawn every frame because the
//      ages it prints move with the clock. It is now drawn when a word it prints changes (an age
//      ticks over, a scrollback step), or when anything else it shows moves — and never otherwise.
//
// The rAF here is a hand-cranked queue, so a test counts exactly the callbacks the waterfall asks
// for. The renderer is a recording stand-in and the overlay context a recording proxy (jsdom has
// neither); the component's own effect, loop and drawOverlay run as shipped.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { Waterfall } from './Waterfall'
import { ageLabel } from '../waterfallHistory'
import { getSpectrumRow } from '../api'
import type { SpectrumRenderer } from '../spectrum'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({ getSpectrumRow: vi.fn() }))

let draws = 0
/** The stand-in rows' timestamps: row `seq` was committed at `rowTms + seq·rowGapMs`. */
let rowTms = 0
let rowGapMs = 1000
/** Every string the overlay printed since the test last emptied it. */
const texts: string[] = []
/** Rows the stand-in renderer holds (scrollback reaches back this far). */
let held = 0
vi.mock('../spectrum', async (importActual) => {
  const actual = await importActual<typeof import('../spectrum')>()
  return {
    ...actual,
    createSpectrumRenderer: (): SpectrumRenderer => ({
      backend: 'canvas2d',
      reason: '',
      canvas: null,
      get rows() {
        return held
      },
      resize() {},
      commitRow() {
        held++
      },
      // Row `age` back from the newest, numbered as the waterfall numbers them (newest = `held`).
      rowAt: (age: number) => (age < held ? { seq: held - age, tMs: rowTms + (held - age) * rowGapMs, loHz: 0, hiHz: 4000 } : null),
      clearHistory() {},
      draw: () => {
        draws++
      },
      destroy() {},
    }),
  }
})

/** The rAF queue: what the waterfall has asked to run on the next frame. */
let queue = new Map<number, FrameRequestCallback>()
let nextId = 1
let clock = 0
/** Run one frame `ms` after the last; returns how many callbacks it ran. */
function frame(ms = 16): number {
  clock += ms
  const due = [...queue.values()]
  queue = new Map()
  for (const cb of due) cb(clock)
  return due.length
}
async function frames(n: number, ms = 16): Promise<number> {
  let ran = 0
  for (let i = 0; i < n; i++) {
    await act(async () => {
      ran += frame(ms)
    })
  }
  return ran
}

/** Overlay draws, counted by the transform every drawOverlay sets first. */
let overlayDraws = 0
function recordingCtx() {
  const state: Record<string, unknown> = { fillStyle: '#000000', globalAlpha: 1, font: '' }
  return new Proxy(state, {
    get(target, prop) {
      if (typeof prop !== 'string') return undefined
      if (prop in target) return target[prop]
      if (prop === 'measureText') return () => ({ width: 10 })
      if (prop === 'createLinearGradient') return () => ({ addColorStop() {} })
      return (...a: unknown[]) => {
        if (prop === 'fillText') texts.push(String(a[0]))
        if (prop === 'setTransform') overlayDraws++
      }
    },
    set(target, prop, value) {
      if (typeof prop === 'string') target[prop] = value
      return true
    },
  })
}

let realRaf: typeof requestAnimationFrame
let realCaf: typeof cancelAnimationFrame
let now = 1_000_000

beforeEach(() => {
  draws = 0
  rowTms = 0
  rowGapMs = 1000
  texts.length = 0
  held = 0
  overlayDraws = 0
  queue = new Map()
  clock = performance.now()
  now = 1_000_000
  localStorage.clear()
  // A quiet source: every ask answers the empty row, which commits nothing.
  vi.mocked(getSpectrumRow).mockImplementation(() => Promise.resolve({ row: [], loHz: 0, hiHz: 4000, source: 'audio' }))
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(
    () => recordingCtx() as unknown as CanvasRenderingContext2D,
  )
  vi.spyOn(Date, 'now').mockImplementation(() => now)
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  realRaf = globalThis.requestAnimationFrame
  realCaf = globalThis.cancelAnimationFrame
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => {
    const id = nextId++
    queue.set(id, cb)
    return id
  }) as typeof requestAnimationFrame
  globalThis.cancelAnimationFrame = ((id: number) => {
    queue.delete(id)
  }) as typeof cancelAnimationFrame
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  globalThis.requestAnimationFrame = realRaf
  globalThis.cancelAnimationFrame = realCaf
  vi.restoreAllMocks()
})

const wf = (active: boolean) => (
  <Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1500} theme="dark" active={active} rowMs={50} />
)

describe('the waterfall loop while hidden', () => {
  it('asks for no frame while inactive, and none at all after the first', async () => {
    render(wf(false))
    // The mount's first frame finds it hidden and stops: nothing is asked for after it.
    const ran = await frames(30)
    expect(ran).toBeLessThanOrEqual(1)
    expect(queue.size).toBe(0)
    expect(vi.mocked(getSpectrumRow)).not.toHaveBeenCalled()
  })

  it('restarts when shown again, fetching rows on its cadence, with exactly one loop', async () => {
    const { rerender } = render(wf(false))
    await frames(5)
    expect(queue.size).toBe(0)
    rerender(wf(true))
    expect(queue.size).toBe(1)
    // 30 frames of 16 ms at 50 ms rows: the loop runs every frame and fetches.
    const ran = await frames(30)
    expect(ran).toBe(30)
    expect(vi.mocked(getSpectrumRow).mock.calls.length).toBeGreaterThanOrEqual(5)
    // One loop, not two: a show that arrives while the loop still runs adds none.
    rerender(wf(false))
    rerender(wf(true))
    expect(queue.size).toBe(1)
    await frames(3)
    expect(queue.size).toBe(1)
  })

  it('draws the picture once on the way back, whatever was skipped while away', async () => {
    const { rerender } = render(wf(true))
    await frames(3)
    rerender(wf(false))
    await frames(5)
    const before = draws
    rerender(wf(true))
    await frames(3)
    expect(draws).toBe(before + 1)
  })

  it('keeps running while shown (the control: a live loop asks every frame)', async () => {
    render(wf(true))
    expect(await frames(20)).toBe(20)
    expect(queue.size).toBe(1)
  })
})

describe('the overlay while paused', () => {
  it('is drawn when the ages tick over, not every frame', async () => {
    held = 100
    const { container } = render(wf(true))
    await frames(3)
    fireEvent.click(container.querySelector('.wf-pause')!)
    await frames(2)
    const start = overlayDraws
    // 50 frames with the clock held still: at most the one draw the pause itself causes.
    await frames(50)
    expect(overlayDraws - start).toBeLessThanOrEqual(1)
    // A second on, every age prints one more: one draw, then quiet again.
    now += 1000
    const turned = overlayDraws
    await frames(10)
    expect(overlayDraws - turned).toBe(1)
  })

  it('is drawn at once when the flow flips (the tape is read in it)', async () => {
    held = 100
    // A real height, so the tape's four ages sit on four different rows (jsdom measures 0).
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, width: 800, height: 200, right: 800, bottom: 200, toJSON: () => ({}),
    } as DOMRect)
    const { container } = render(wf(true))
    await frames(3)
    fireEvent.click(container.querySelector('.wf-pause')!)
    await frames(5)
    const start = overlayDraws
    // The tape reads the direction, so flipping it while paused turns its ages end for end: redrawn now.
    fireEvent.click(container.querySelector('.wf-flow')!)
    await frames(2)
    expect(overlayDraws - start).toBe(1)
  })

  it('is drawn at once for a scrollback step (the wheel moves the tape)', async () => {
    held = 100
    const { container } = render(wf(true))
    await frames(3)
    fireEvent.click(container.querySelector('.wf-pause')!)
    await frames(5)
    const start = overlayDraws
    // Newest at the top (the default), so wheel-down goes back in time. Each step, the first and
    // a later one, redraws at once.
    const canvas = container.querySelector('.waterfall-canvas')!
    fireEvent.wheel(canvas, { deltaY: 100 })
    await frames(2)
    expect(overlayDraws - start).toBe(1)
    fireEvent.wheel(canvas, { deltaY: 100 })
    await frames(2)
    expect(overlayDraws - start).toBe(2)
  })

  it('is not drawn for a row committed while paused (the picture holds still)', async () => {
    held = 100
    let serve = false
    vi.mocked(getSpectrumRow).mockImplementation(() =>
      Promise.resolve(
        serve
          ? { row: Array.from({ length: 512 }, () => 0.3), loHz: 0, hiHz: 4000, source: 'audio' }
          : { row: [], loHz: 0, hiHz: 4000, source: 'audio' },
      ),
    )
    const { container } = render(wf(true))
    await frames(3)
    fireEvent.click(container.querySelector('.wf-pause')!)
    // Scroll back first, so the chip already reads "back" and only the commits move the offset.
    fireEvent.wheel(container.querySelector('.waterfall-canvas')!, { deltaY: 100 })
    await frames(3)
    const start = overlayDraws
    const heldBefore = held
    serve = true
    // 20 frames, 320 ms, inside one wall-clock second: rows commit, the offset follows each one,
    // and the newest row on screen is still the same row.
    await frames(20)
    expect(held).toBeGreaterThan(heldBefore)
    expect(overlayDraws - start).toBe(0)
  })

  it('is drawn once when the first row commits at the live tail (the chip turns to "back")', async () => {
    held = 100
    let serve = false
    vi.mocked(getSpectrumRow).mockImplementation(() =>
      Promise.resolve(
        serve
          ? { row: Array.from({ length: 512 }, () => 0.3), loHz: 0, hiHz: 4000, source: 'audio' }
          : { row: [], loHz: 0, hiHz: 4000, source: 'audio' },
      ),
    )
    const { container } = render(wf(true))
    await frames(3)
    fireEvent.click(container.querySelector('.wf-pause')!)
    await frames(3)
    const start = overlayDraws
    serve = true
    // The first commit takes the picture off the live tail (the chip now reads how far back);
    // the ones after it hold still.
    await frames(20)
    expect(held).toBeGreaterThan(101)
    expect(overlayDraws - start).toBe(1)
  })

  it('is not drawn at all while live and still (the control)', async () => {
    render(wf(true))
    await frames(3)
    const start = overlayDraws
    await frames(50)
    expect(overlayDraws - start).toBe(0)
  })

  // An age prints rounded to its OWN row's whole second (`ageLabel`), so it ticks over at
  // `tMs + k·1000 + 500`, not when the wall clock's second turns. Keyed on the wall-clock second,
  // a row 1.2 s old at pause showed "−now" for 1.4 s of every 2 while the true label was "−2s".
  for (const phase of [1500, 1200, 1900]) {
    it(`prints the true age at every moment (a row ${phase} ms old at pause)`, async () => {
      held = 1
      rowGapMs = 0
      rowTms = now - phase
      const { container } = render(wf(true))
      await frames(3)
      texts.length = 0
      fireEvent.click(container.querySelector('.wf-pause')!)
      await frames(2)
      let onScreen = texts.filter((s) => s.startsWith('−')).pop()
      const stale: string[] = []
      for (let step = 0; step < 40; step++) {
        now += 50
        texts.length = 0
        await frames(1)
        const drawn = texts.filter((s) => s.startsWith('−'))
        if (drawn.length) onScreen = drawn[drawn.length - 1]
        const truth = `−${ageLabel(now - rowTms)}`
        if (onScreen !== truth) stale.push(`+${now - 1_000_000} ms: on screen ${onScreen}, true ${truth}`)
      }
      expect(stale).toEqual([])
    })
  }
})
