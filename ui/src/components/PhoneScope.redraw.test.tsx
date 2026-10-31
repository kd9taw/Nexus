// @vitest-environment jsdom
//
// WHAT THE RIG SCOPE DRAWS WHEN NOTHING HAS CHANGED: nothing.
//
// The scope polls about 30 times a second, and it used to draw the whole picture and redraw every mark
// on every poll, a new sweep or not. On a radio sweeping a few times a second in "a row per sweep",
// most polls bring nothing new, so most of those draws put the same pixels back. These pin the waste
// shut, each with its control:
//
//   the picture  drawn when a row is committed, the trace moves, or the view or the look changes;
//                not on a poll that changes none of them;
//   the marks    (the dial, the passband, the frequency scale on `.ph-scope-canvas`) drawn again when
//                anything they are drawn from changes; not on a poll that changes none of it.
//
// These run the component's real loop, poll and renderer on fake timers, against a scripted source
// that sweeps once and then stands still. The paint targets are recording fakes, one per canvas, so
// the marks canvas's clears are counted apart from the renderer's.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'
import { Canvas2dBackend } from '../spectrum/canvas2d'

vi.setConfig({ testTimeout: 15_000 })

/** One carrier on a flat floor. */
const ROW = Array.from({ length: 512 }, (_, i) => (i === 200 ? 0.9 : 0.1))

/** The scripted source: a new sweep every `sweepEvery` polls. */
const src = { sweepEvery: 1_000_000, polls: 0 }

vi.mock('../api', () => ({
  getScopeFrame: (_lo: number, _hi: number, _w: string | undefined, lastSeq: number) => {
    src.polls++
    const published = Math.floor((src.polls - 1) / src.sweepEvery) + 1
    if (published <= lastSeq) return Promise.resolve(null)
    return Promise.resolve({
      seq: published,
      tMs: 1_700_000_000_000 + published,
      source: 'civ',
      loHz: 14_175_000,
      hiHz: 14_225_000,
      scale: { kind: 'relative' },
      bins: ROW,
    })
  },
}))

/** Renderer draws, and clears of the marks canvas (one per redraw of the marks). */
let draws = 0
let marks = 0

beforeEach(() => {
  Object.assign(src, { sweepEvery: 1_000_000, polls: 0 })
  draws = 0
  marks = 0
  localStorage.clear()
  localStorage.setItem('nexus.phonescope.rows', 'sweep')
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
  // One recording 2D fake per canvas; `webgl2` answers null, so the renderer draws on canvas-2D.
  const fakes = new WeakMap<HTMLCanvasElement, unknown>()
  const paintFor = (cv: HTMLCanvasElement) => ({
    canvas: cv,
    fillRect() {}, putImageData() {}, beginPath() {}, closePath() {}, moveTo() {}, lineTo() {}, fillText() {},
    fill() {}, stroke() {}, setLineDash() {}, save() {}, restore() {}, rect() {}, clip() {}, drawImage() {},
    clearRect() {
      if (cv.classList.contains('ph-scope-canvas')) marks++
    },
    measureText: (t: string) => ({ width: t.length * 6 }), createLinearGradient: () => ({ addColorStop() {} }),
  })
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement, kind: string) {
    if (kind !== '2d') return null
    if (!fakes.has(this)) fakes.set(this, paintFor(this))
    return fakes.get(this)
  } as unknown as HTMLCanvasElement['getContext'])
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ x: 0, y: 0, left: 0, top: 0,
    width: 400, height: 200, right: 400, bottom: 200, toJSON() { return {} } })
  const draw = Canvas2dBackend.prototype.draw
  vi.spyOn(Canvas2dBackend.prototype, 'draw').mockImplementation(function (this: Canvas2dBackend, scene) {
    draws++
    return draw.call(this, scene)
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

/** The Phone scope on an RF row: dial line, passband and scale all drawn. */
const scope = (dialHz = 14_200_000) => (
  <PhoneScope transmitting={false} theme="dark" viewLoHz={-1e9} viewHiHz={1e9} dialHz={dialHz} passbandHz={2400} />
)

describe('the picture: drawn only when something in it changed', () => {
  it('does not redraw on polls that bring no new sweep once the trace has settled', async () => {
    render(scope())
    // One sweep, then the trace's peak hold falls back to it (150 ms) and nothing moves.
    await run(2000)
    const polls0 = src.polls
    const draws0 = draws
    await run(1000)
    expect(src.polls - polls0, 'control: the loop kept polling').toBeGreaterThanOrEqual(20)
    expect(draws0, 'control: the sweep was drawn').toBeGreaterThan(0)
    expect(draws - draws0).toBe(0)
  })

  it('CONTROL: draws every new sweep', async () => {
    src.sweepEvery = 4
    render(scope())
    await run(2000)
    const polls0 = src.polls
    const draws0 = draws
    await run(1000)
    const sweeps = Math.floor((src.polls - 1) / 4) - Math.floor((polls0 - 1) / 4)
    expect(sweeps, 'control: the source swept').toBeGreaterThan(3)
    expect(draws - draws0).toBeGreaterThanOrEqual(sweeps)
  })

  it('CONTROL: smooth scroll draws every poll, each one a row', async () => {
    localStorage.setItem('nexus.phonescope.rows', 'smooth')
    render(scope())
    await run(2000)
    const polls0 = src.polls
    const draws0 = draws
    await run(1000)
    expect(src.polls - polls0).toBeGreaterThanOrEqual(20)
    expect(draws - draws0).toBe(src.polls - polls0)
  })

  it('CONTROL: a look changed with nothing new from the source is drawn at once', async () => {
    render(scope())
    await run(2000)
    const draws0 = draws
    fireEvent.click(screen.getByRole('button', { name: '◭' }))
    expect(draws - draws0).toBeGreaterThan(0)
  })

  it('CONTROL: a dial moved between two sweeps redraws the picture on the next poll', async () => {
    // A 4 kHz window on the dial (not the whole sweep), so the window moves with it.
    const narrow = (dialHz: number) => (
      <PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={4000} dialHz={dialHz} passbandHz={2400} />
    )
    const { rerender } = render(narrow(14_200_000))
    await run(2000)
    const draws0 = draws
    await run(500)
    expect(draws - draws0, 'control: a still window drew nothing').toBe(0)
    rerender(narrow(14_190_000))
    await run(100)
    expect(draws - draws0).toBeGreaterThan(0)
  })
})

describe('the marks: drawn again only when what they show changed', () => {
  it('does not redraw the marks on polls that change nothing they are drawn from', async () => {
    render(scope())
    await run(2000)
    const polls0 = src.polls
    const marks0 = marks
    await run(1000)
    expect(src.polls - polls0, 'control: the loop kept polling').toBeGreaterThanOrEqual(20)
    expect(marks0, 'control: the marks were drawn').toBeGreaterThan(0)
    expect(marks - marks0).toBe(0)
  })

  it('does not redraw the marks on smooth scroll repeats either', async () => {
    localStorage.setItem('nexus.phonescope.rows', 'smooth')
    render(scope())
    await run(2000)
    const marks0 = marks
    await run(1000)
    expect(marks - marks0).toBe(0)
  })

  it('CONTROL: a dial moved between two sweeps redraws the marks on the next poll', async () => {
    const { rerender } = render(scope())
    await run(2000)
    const marks0 = marks
    rerender(scope(14_201_000))
    await run(100)
    expect(marks - marks0).toBe(1)
  })

  it('CONTROL: a reported width that changes redraws the marks', async () => {
    const { rerender } = render(scope())
    await run(2000)
    const marks0 = marks
    rerender(<PhoneScope transmitting={false} theme="dark" viewLoHz={-1e9} viewHiHz={1e9} dialHz={14_200_000} passbandHz={1800} />)
    await run(100)
    expect(marks - marks0).toBeGreaterThanOrEqual(1)
  })
})
