// @vitest-environment jsdom
//
// WHICH ROWS THE RIG SCOPE COMMITS, and what each one says it is.
//
// The scope polls 20 times a second; an IC-9700's CI-V scope sweeps a few times a second. It used
// to commit a waterfall row on every poll, so a slow source scrolled copies of its last sweep that
// looked exactly like new data (ui/spectrum-harness, the cadence probe: 82% of rows at 3 sweeps a
// second). The backend now numbers every frame, the scope asks for one newer than the last it drew,
// and the operator picks what happens when there is none (2026-10-04, "Build both, I'll compare"):
//
//   smooth scroll (the default)  a row every poll; between sweeps the newest one again, MARKED as a
//                                repeat — the sweep's own number, and its own time;
//   a row per sweep              a row when the source sweeps, and nothing else.
//
// These run the component's real loop, its real poll and the renderer's real ring, on fake timers so
// every poll is counted, against a scripted source. The ring is read where rows enter it
// (`SpectrumRing.push`), which is the history the scope draws, scrolls back and stacks in 3D.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'
import { SpectrumRing } from '../spectrum/ring'
import { Canvas2dBackend } from '../spectrum/canvas2d'
import type { SpectrumScene } from '../spectrum'

/** One carrier on a flat floor: bin 200 at 0.9, the rest 0.1 (the median, the noise). */
const ROW = Array.from({ length: 512 }, (_, i) => (i === 200 ? 0.9 : 0.1))

/** The scripted source. `sweepEvery` polls make one new sweep; `numbered` false = a source nobody
 *  numbers (the Companion path and the Remote's rows), which answers every poll. */
const src = { sweepEvery: 3, numbered: true, source: 'audio', polls: 0, asked: [] as { lastSeq: number; window: string | undefined }[] }
/** The producer's own time of sweep `n`. */
const sweepTime = (n: number) => 1_700_000_000_000 + n * 1000

vi.mock('../api', () => ({
  getScopeFrame: (loHz: number, hiHz: number, window: string | undefined, lastSeq: number) => {
    void loHz
    void hiHz
    src.polls++
    src.asked.push({ lastSeq, window })
    const published = Math.floor((src.polls - 1) / src.sweepEvery) + 1
    if (src.numbered && published <= lastSeq) return Promise.resolve(null)
    const rf = src.source === 'civ'
    return Promise.resolve({
      seq: src.numbered ? published : 0,
      tMs: sweepTime(published),
      source: src.source,
      loHz: rf ? 144_975_000 : 0,
      hiHz: rf ? 145_025_000 : 4000,
      scale: rf ? { kind: 'relative' } : { kind: 'dbfs', loDb: -120, hiDb: 0 },
      bins: ROW,
    })
  },
}))

/** Every row the scope committed, as the ring received it. */
let rows: { seq: number; tMs: number; n: number }[]
/** Every scene the renderer was asked to draw. */
let scenes: SpectrumScene[]

beforeEach(() => {
  Object.assign(src, { sweepEvery: 3, numbered: true, source: 'audio', polls: 0, asked: [] })
  rows = []
  scenes = []
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
    rows.push({ seq: frame.seq, tMs: frame.tMs, n: frame.bins.length })
    return push.call(this, frame, range)
  })
  const draw = Canvas2dBackend.prototype.draw
  vi.spyOn(Canvas2dBackend.prototype, 'draw').mockImplementation(function (this: Canvas2dBackend, scene) {
    scenes.push(scene)
    return draw.call(this, scene)
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

/** Run the scope's own loop for `ms` of fake time (a poll every 50 ms). */
async function run(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms)
  })
}

describe('smooth scroll (the default): a row every poll, each repeat marked as one', () => {
  it('commits a row on every poll, new sweeps under their own number and time', async () => {
    render(<PhoneScope transmitting={false} theme="dark" />)
    await run(1500)
    expect(src.polls, 'control: the loop never polled').toBeGreaterThanOrEqual(20)
    // One row for every poll the source answered, new or not.
    expect(rows.length).toBe(src.polls)
    // Every sweep reached the history, in order, none skipped.
    const seqs = [...new Set(rows.map((r) => r.seq))]
    expect(seqs).toEqual(Array.from({ length: seqs.length }, (_, i) => i + 1))
    expect(seqs.length).toBe(Math.ceil(src.polls / src.sweepEvery))
  })

  it('marks every repeat: the number of the row before it, and a later time of its own', async () => {
    render(<PhoneScope transmitting={false} theme="dark" />)
    await run(1500)
    let repeats = 0
    for (let i = 0; i < rows.length; i++) {
      const r = rows[i]
      const first = i === 0 || rows[i - 1].seq !== r.seq
      if (first) {
        // A new sweep keeps the producer's time.
        expect(r.tMs, `row ${i}: a new sweep carries its own time`).toBe(sweepTime(r.seq))
      } else {
        repeats++
        expect(r.tMs, `row ${i}: a repeat is stamped when it was committed`).toBeGreaterThan(rows[i - 1].tMs - 1)
        expect(r.tMs).not.toBe(sweepTime(r.seq))
      }
    }
    // Two of every three polls found nothing new: they are the repeats, every one marked.
    expect(repeats, 'control: the source never stood still').toBeGreaterThan(0)
    expect(repeats).toBe(rows.length - new Set(rows.map((r) => r.seq)).size)
  })
})

describe('a row per sweep', () => {
  beforeEach(() => localStorage.setItem('nexus.phonescope.rows', 'sweep'))

  it('commits exactly one row per sweep and never a copy', async () => {
    render(<PhoneScope transmitting={false} theme="dark" />)
    await run(1500)
    expect(src.polls, 'control: the loop never polled').toBeGreaterThanOrEqual(20)
    expect(rows.map((r) => r.seq)).toEqual(Array.from({ length: rows.length }, (_, i) => i + 1))
    expect(rows.length).toBe(Math.ceil(src.polls / src.sweepEvery))
    expect(rows.every((r) => r.tMs === sweepTime(r.seq))).toBe(true)
  })

  it('CONTROL: switched to smooth scroll mid-stream, the same scope commits on every poll again', async () => {
    render(<PhoneScope transmitting={false} theme="dark" />)
    await run(600)
    const before = rows.length
    fireEvent.click(screen.getByRole('button', { name: 'Display settings' }))
    fireEvent.click(screen.getByRole('button', { name: 'Row per sweep' }))
    const polls0 = src.polls
    await run(600)
    expect(before, 'control: rows were committed before the switch').toBeGreaterThan(0)
    expect(rows.length - before).toBe(src.polls - polls0)
  })
})

describe('the frame number round trip', () => {
  it('each poll names the newest sweep the scope has drawn', async () => {
    render(<PhoneScope transmitting={false} theme="dark" />)
    await run(1000)
    expect(src.asked[0].lastSeq, 'nothing drawn yet').toBe(0)
    for (let k = 1; k < src.asked.length; k++) {
      expect(src.asked[k].lastSeq, `poll ${k + 1}`).toBe(Math.floor((k - 1) / src.sweepEvery) + 1)
    }
  })

  it('a source nobody numbers answers every poll, and every answer is a new row — in both looks', async () => {
    src.numbered = false
    render(<PhoneScope transmitting={false} theme="dark" />)
    await run(600)
    cleanup()
    const smooth = rows.length
    const smoothPolls = src.polls
    rows = []
    src.polls = 0
    localStorage.setItem('nexus.phonescope.rows', 'sweep')
    render(<PhoneScope transmitting={false} theme="dark" />)
    await run(600)
    expect(smoothPolls, 'control: the loop never polled').toBeGreaterThan(5)
    expect(smooth).toBe(smoothPolls)
    expect(rows.length).toBe(src.polls)
    expect(src.asked.every((a) => a.lastSeq === 0)).toBe(true)
  })

  it('asks with the analysis window the cockpit’s record holds', async () => {
    localStorage.setItem('nexus.scope.cw', JSON.stringify({ window: 'fast' }))
    render(<PhoneScope transmitting={false} theme="dark" cockpit="cw" />)
    await run(300)
    expect(src.asked.length, 'control: nothing was polled').toBeGreaterThan(0)
    expect(src.asked.every((a) => a.window === 'fast')).toBe(true)
  })
})

describe('pause', () => {
  it('keeps filling the history while the picture holds still', async () => {
    render(<PhoneScope transmitting={false} theme="dark" />)
    await run(500)
    fireEvent.click(screen.getByRole('button', { name: '⏸' }))
    const committed = rows.length
    const drawn = scenes.length
    await run(500)
    // History kept filling, and nothing was drawn over the held picture.
    expect(rows.length - committed, 'control: no row arrived while paused').toBeGreaterThan(3)
    expect(scenes.length).toBe(drawn)
    // A wheel notch back draws the held picture 3 rows further back: the offset had moved with every
    // row that arrived, so the picture stood still until the operator moved it.
    fireEvent.wheel(document.querySelector('canvas.ph-scope-canvas')!, { deltaY: 100 })
    expect(scenes[scenes.length - 1].offsetRows).toBe(rows.length - committed + 3)
  })
})

describe('the ▲dB readout', () => {
  const readout = () => document.querySelector('.ph-scope-dyn')?.textContent

  // The whole row in view (the Phone cockpit's Full span), so the carrier is in the readout's window.
  it('reads the audio feed on its 120 dB axis', async () => {
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={-1e9} viewHiHz={1e9} />)
    await run(300)
    // 0.9 over a 0.1 median.
    expect(readout()).toBe('▲96 dB')
  })

  it('reads a CI-V scope in the 80 dB Icom gives its display, not 120', async () => {
    src.source = 'civ'
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={-1e9} viewHiHz={1e9} />)
    await run(300)
    expect(readout()).toBe('▲64 dB')
  })
})
