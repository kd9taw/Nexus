// @vitest-environment jsdom
//
// THE RF SCOPE PANE — the radio's own panadapter, and nothing else, as an opt-in pane in the digital
// cockpits (operator, 2026-10-03: "Yes, opt-in pane").
//
// What it must do, each run through PhoneScope's real loop and real poll on fake timers, against a
// scripted source:
//   · it asks the RF read (`getRfFrame`), never the rig scope's (`getScopeFrame`), whose fallback is
//     the audio FFT — a second copy of the waterfall beside it;
//   · with no panadapter it says so, and draws nothing;
//   · a sweep is drawn, and the chip goes;
//   · behind another screen it asks for nothing — its poll IS the station's reason to stream the Icom
//     scope in a data mode, so a pane nobody sees must not poll;
//   · it is display only: no gesture on it can move the dial;
//   · its ✕ is the panel record's own hide.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { RfScopePane } from './RfScopePane'
import { SpectrumRing } from '../spectrum/ring'

/** One carrier on a flat floor over a CI-V sweep's 475 points. */
const SWEEP = Array.from({ length: 475 }, (_, i) => (i === 200 ? 0.9 : 0.1))

/** The scripted source: `rf` false answers the empty frame (no panadapter); true sweeps once every
 *  `sweepEvery` polls. */
const src = { rf: false, sweepEvery: 3, polls: 0, asked: [] as number[], scopeAsked: 0 }

vi.mock('../api', () => ({
  getRfFrame: (lastSeq: number) => {
    src.polls++
    src.asked.push(lastSeq)
    if (!src.rf) {
      return Promise.resolve({ seq: 0, tMs: Date.now(), source: '', loHz: 0, hiHz: 0, scale: { kind: 'relative' }, bins: [] })
    }
    const published = Math.floor((src.polls - 1) / src.sweepEvery) + 1
    if (published <= lastSeq) return Promise.resolve(null)
    return Promise.resolve({
      seq: published,
      tMs: Date.now(),
      source: 'civ',
      loHz: 144_149_000,
      hiHz: 144_199_000,
      scale: { kind: 'relative' },
      slice: 0,
      bins: SWEEP,
    })
  },
  getScopeFrame: () => {
    src.scopeAsked++
    return Promise.resolve(null)
  },
}))

/** Every row the scope committed to its history. */
let rows: number[]

beforeEach(() => {
  Object.assign(src, { rf: false, sweepEvery: 3, polls: 0, asked: [], scopeAsked: 0 })
  rows = []
  localStorage.clear()
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance', 'Date'] })
  vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => setTimeout(() => cb(performance.now()), 16))
  vi.stubGlobal('cancelAnimationFrame', clearTimeout)
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.stubGlobal('ImageData', class {
    data: Uint8ClampedArray; width: number; height: number
    constructor(a: Uint8ClampedArray | number, b: number, c?: number) {
      if (typeof a === 'number') { this.width = a; this.height = b; this.data = new Uint8ClampedArray(a * b * 4) }
      else { this.data = a; this.width = b; this.height = c ?? a.length / 4 / b }
    }
  })
  // A 2D paint target only: `webgl2` answers null, so the renderer draws on its canvas-2D path.
  const paint = {
    fillRect() {}, clearRect() {}, putImageData() {}, beginPath() {}, closePath() {}, moveTo() {}, lineTo() {}, fillText() {},
    fill() {}, stroke() {}, setLineDash() {}, save() {}, restore() {}, rect() {}, clip() {},
    measureText: (t: string) => ({ width: t.length * 6 }), createLinearGradient: () => ({ addColorStop() {} }),
  }
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(((kind: string) =>
    kind === '2d' ? paint : null) as unknown as HTMLCanvasElement['getContext'])
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ x: 0, y: 0, left: 0, top: 0,
    width: 400, height: 200, right: 400, bottom: 200, toJSON() { return {} } })
  const push = SpectrumRing.prototype.push
  vi.spyOn(SpectrumRing.prototype, 'push').mockImplementation(function (this: SpectrumRing, frame, range) {
    rows.push(frame.seq)
    return push.call(this, frame, range)
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

async function run(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms)
  })
}

const pane = (over: { active?: boolean; onRemove?: () => void } = {}) => (
  <RfScopePane
    closeProps={{ onRemove: over.onRemove ?? (() => {}) }}
    dialMhz={144.174}
    keyed={false}
    theme="dark"
    active={over.active ?? true}
  />
)

describe('the RF scope pane draws the radio’s panadapter and nothing else', () => {
  it('asks the RF read, never the rig scope’s (whose fallback is the audio FFT)', async () => {
    render(pane())
    await run(1000)
    expect(src.polls, 'control: the loop never polled').toBeGreaterThanOrEqual(10)
    expect(src.scopeAsked, 'the pane asked the rig scope’s read').toBe(0)
  })

  it('with no panadapter it says so, and draws nothing', async () => {
    render(pane())
    await run(600)
    expect(screen.getByRole('status').textContent).toBe('No scope data from the radio')
    expect(rows, 'an empty frame reached the history').toEqual([])
  })

  it('a sweep lands: the chip goes and the sweep is drawn, each one asked for past the last', async () => {
    src.rf = true
    render(pane())
    await run(1000)
    expect(screen.queryByRole('status'), 'the chip outlived the first sweep').toBeNull()
    expect(rows.length, 'no sweep was drawn').toBeGreaterThan(0)
    expect(Math.max(...src.asked), 'the poll never named the sweep it drew').toBeGreaterThan(0)
    // …and when the radio stops, it says so again.
    src.rf = false
    await run(300)
    expect(screen.getByRole('status').textContent).toBe('No scope data from the radio')
  })

  it('behind another screen it asks for nothing — its poll is the station’s reason to stream', async () => {
    const view = render(pane({ active: false }))
    await run(1500)
    expect(src.polls, 'a pane nobody sees kept asking for the stream').toBe(0)
    view.rerender(pane({ active: true }))
    await run(500)
    expect(src.polls, 'control: shown again, it asks').toBeGreaterThan(0)
  })

  it('is display only: no gesture can move the dial from it', async () => {
    src.rf = true
    const { container } = render(pane())
    await run(500)
    const canvas = container.querySelector('canvas.ph-scope-canvas')!
    expect(canvas.classList.contains('tunable')).toBe(false)
    expect(canvas.getAttribute('title')).toBeNull()
  })

  it('its ✕ is the panel record’s own hide, named for the pane', async () => {
    const onRemove = vi.fn()
    render(pane({ onRemove }))
    const frame = screen.getByRole('region', { name: 'RF scope' })
    expect(frame.getAttribute('data-pane')).toBe('rfScope')
    fireEvent.click(screen.getByRole('button', { name: /RF scope/ }))
    expect(onRemove).toHaveBeenCalledTimes(1)
  })
})
