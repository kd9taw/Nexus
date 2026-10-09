// @vitest-environment jsdom
//
// WHAT THE DIGITAL WATERFALL HANDS THE SPECTRUM RENDERER, BY VALUE (2026-10-04).
//
// The waterfall draws through the renderer (ui/src/spectrum): it fetches the AUDIO row, flattens it,
// sets the range it is drawn in, and commits it, and the renderer draws every picture from those
// commits. So the commits ARE the picture's contract, and they are pinned here number for number
// against the chain the waterfall has always run, written out independently below: the visible
// window's median floor, smoothed across rows, parked `WF_PARK_DB` over the noise, G and Z on top
// (`DIGITAL_WATERFALL_RANGE` 'parked'); our own FT transmission zeroed at the source with that
// smoothing frozen through it; and an RF scope row never drawn on the audio axis (the RF-row
// defence). The other rule the constant can name, the rig scope's auto range, is pinned the same
// way, so flipping the constant is a ruling and not an experiment.
//
// The renderer is a recording stand-in: jsdom has no canvas context, and what is under test is what
// the waterfall hands it, not how it rasterises (the harness's pixel fixtures do that, on both
// backends). The waterfall's own effect, row fetch and draw loop run as shipped.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { Waterfall } from './Waterfall'
import { getSpectrumRow } from '../api'
import { agcRange, applyGainZero, flattenRow, parkFloor, WF_DB_SPAN, WF_FLOOR_PCT } from '../waterfall'
import { autoRange } from '../spectrum/scaleRange'
import type { DisplayRange, SpectrumFrame, SpectrumRenderer, SpectrumScene } from '../spectrum'

// THE BUDGET (2026-10-09). The slowest case here, "one draw per committed row, and none while the source…", takes
// 0.57 s and 0.57 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({ getSpectrumRow: vi.fn() }))

/** Which rule the waterfall reads, switched per case (the shipped value is checked below). */
let rule: 'parked' | 'auto' = 'parked'
vi.mock('../waterfall', async (importActual) => {
  const actual = await importActual<typeof import('../waterfall')>()
  const mocked = { ...actual }
  Object.defineProperty(mocked, 'DIGITAL_WATERFALL_RANGE', { get: () => rule, enumerable: true })
  return mocked
})

const commits: { frame: SpectrumFrame & { bins: Float32Array }; range: DisplayRange }[] = []
const scenes: SpectrumScene[] = []
vi.mock('../spectrum', async (importActual) => {
  const actual = await importActual<typeof import('../spectrum')>()
  return {
    ...actual,
    createSpectrumRenderer: (): SpectrumRenderer => ({
      backend: 'canvas2d',
      reason: '',
      canvas: null,
      get rows() {
        return commits.length
      },
      resize() {},
      // The renderer copies the bins it is handed (the waterfall reuses its scratch row), and so
      // does this.
      commitRow: (frame, range) => commits.push({ frame: { ...frame, bins: Float32Array.from(frame.bins) }, range: { ...range } }),
      rowAt: () => null,
      clearHistory() {},
      draw: (scene) => scenes.push(scene),
      destroy() {},
    }),
  }
})

const BINS = 512
/** A seeded row on the audio axis: a noise floor near −90 dBFS with a little tilt, and one carrier. */
function audioRow(k: number, carrierBin = 200): number[] {
  let x = (k + 1) * 2654435761
  const rnd = () => ((x = (x * 1103515245 + 12345) >>> 0) / 2 ** 32)
  return Array.from({ length: BINS }, (_, i) => 0.25 + 0.02 * rnd() + (i / BINS) * 0.03 + (i === carrierBin ? 0.3 : 0))
}

let served: { row: number[]; loHz: number; hiHz: number; source?: string }[] = []
let realRaf: typeof requestAnimationFrame
let realCaf: typeof cancelAnimationFrame

beforeEach(() => {
  rule = 'parked'
  commits.length = 0
  scenes.length = 0
  served = []
  localStorage.clear()
  // Each ask takes the next queued row; with none queued the source answers the empty row a
  // quiet source does (the waterfall commits nothing for it).
  vi.mocked(getSpectrumRow).mockImplementation(() =>
    Promise.resolve(served.shift() ?? { row: [], loHz: 0, hiHz: 4000, source: 'audio' }),
  )
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => null)
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
  vi.restoreAllMocks()
})

const settle = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)))
async function until(test: () => boolean, what: string) {
  for (let i = 0; i < 200 && !test(); i++) await settle(20)
  expect(test(), what).toBe(true)
}

/** The waterfall's chain, written out: what each served audio row must be committed as. */
function expected(rows: { row: number[]; blank: boolean }[], gain = 0, zero = 0) {
  const view = { lo: 200, hi: 3000 } // the default Std view
  let agc = { floor: 0, ceil: 1, init: false }
  let auto = { floor: 0, ceil: 1 }
  return rows.map(({ row, blank }) => {
    const bins = new Float32Array(row.length)
    flattenRow(row, bins)
    if (blank) bins.fill(0)
    const binHz = 4000 / row.length
    const vLo = Math.max(0, Math.floor((view.lo - 0) / binHz))
    const vHi = Math.min(row.length, Math.ceil((view.hi - 0) / binHz))
    const { floor, ceil } = agcRange(bins.slice(vLo, vHi), WF_FLOOR_PCT)
    if (!blank) {
      agc = agc.init ? { floor: agc.floor + (floor - agc.floor) * 0.1, ceil: agc.ceil + (ceil - agc.ceil) * 0.1, init: true } : { floor, ceil, init: true }
      auto = autoRange(bins, WF_DB_SPAN, vLo, vHi)
    }
    const base = rule === 'auto' ? auto : parkFloor(agc.floor, agc.ceil)
    return { bins, range: applyGainZero(base.floor, base.ceil, gain, zero) }
  })
}

describe('the rows the waterfall commits', () => {
  it('ships the parked floor until the operator rules (2026-10-04)', async () => {
    const actual = await vi.importActual<typeof import('../waterfall')>('../waterfall')
    expect(actual.DIGITAL_WATERFALL_RANGE).toBe('parked')
  })

  it.each(['parked', 'auto'] as const)('%s: each audio row, flattened, in the range the chain gives it, numbered once', async (which) => {
    rule = which
    const rows = Array.from({ length: 6 }, (_, k) => audioRow(k))
    served = rows.map((row) => ({ row, loHz: 0, hiHz: 4000, source: 'audio' }))
    const t0 = Date.now()
    render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" rowMs={20} />)
    await until(() => commits.length === rows.length, 'every row committed')
    const want = expected(rows.map((row) => ({ row, blank: false })))
    commits.forEach((c, i) => {
      expect([...c.frame.bins], `row ${i}: the flattened values`).toEqual([...want[i].bins])
      expect(c.range, `row ${i}: its range`).toEqual(want[i].range)
      expect([c.frame.seq, c.frame.loHz, c.frame.hiHz, c.frame.dbPerUnit], `row ${i}`).toEqual([i + 1, 0, 4000, WF_DB_SPAN])
      expect(c.frame.tMs).toBeGreaterThanOrEqual(t0)
      expect(c.frame.tMs).toBeLessThanOrEqual(Date.now())
    })
    // CONTROL: the two rules really differ on these rows, so each case can only pass on its own.
    rule = which === 'parked' ? 'auto' : 'parked'
    const other = expected(rows.map((row) => ({ row, blank: false })))
    rule = which
    expect(want[5].range).not.toEqual(other[5].range)
  })

  it.each(['parked', 'auto'] as const)('%s: our own FT over is a dark band, and the range holds still through it', async (which) => {
    rule = which
    const rows = Array.from({ length: 6 }, (_, k) => audioRow(k))
    const { rerender } = render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" rowMs={20} txBlanks />)
    const keyed = [false, false, true, true, false, false]
    for (let i = 0; i < rows.length; i++) {
      rerender(<Waterfall transmitting={keyed[i]} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" rowMs={20} txBlanks />)
      served.push({ row: rows[i], loHz: 0, hiHz: 4000, source: 'audio' })
      await until(() => commits.length === i + 1, `row ${i} committed`)
    }
    const want = expected(rows.map((row, i) => ({ row, blank: keyed[i] })))
    commits.forEach((c, i) => {
      expect([...c.frame.bins], `row ${i}`).toEqual([...want[i].bins])
      expect(c.range, `row ${i}`).toEqual(want[i].range)
    })
    expect([...commits[2].frame.bins].every((v) => v === 0), 'a keyed row is zeroed').toBe(true)
    expect(commits[3].range, 'the range is frozen through the over').toEqual(commits[1].range)
  })

  it('a surface whose overs are long (no txBlanks) draws what it is served while keyed', async () => {
    const rows = [audioRow(1), audioRow(2)]
    served = rows.map((row) => ({ row, loHz: 0, hiHz: 4000, source: 'audio' }))
    render(<Waterfall transmitting rxOffsetHz={1500} txOffsetHz={1000} theme="dark" rowMs={20} />)
    await until(() => commits.length === 2, 'both rows committed')
    const want = expected(rows.map((row) => ({ row, blank: false })))
    expect([...commits[1].frame.bins]).toEqual([...want[1].bins])
  })

  it('an RF scope row is never drawn on the audio axis: it is skipped, and the numbering does not see it', async () => {
    served = [
      { row: audioRow(1), loHz: 0, hiHz: 4000, source: 'audio' },
      { row: audioRow(2), loHz: 144_975_000, hiHz: 145_025_000, source: 'civ' },
      { row: audioRow(3), loHz: 14_000_000, hiHz: 14_200_000, source: 'flex' },
      { row: audioRow(4), loHz: 7_000_000, hiHz: 7_100_000, source: 'yaesu' },
      { row: audioRow(5), loHz: 0, hiHz: 4000, source: 'audio' },
    ]
    render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" rowMs={20} />)
    await until(() => served.length === 0 && commits.length === 2, 'the audio rows committed')
    await settle(100)
    expect(commits.map((c) => [c.frame.seq, c.frame.loHz, c.frame.hiHz])).toEqual([
      [1, 0, 4000],
      [2, 0, 4000],
    ])
    // CONTROL: the same rows labelled audio are committed, so the skip is the label's doing.
    cleanup()
    commits.length = 0
    served = [2, 3, 4].map((k) => ({ row: audioRow(k), loHz: 0, hiHz: 4000, source: 'audio' }))
    render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" rowMs={20} />)
    await until(() => commits.length === 3, 'the relabelled rows committed')
    // It asks for the AUDIO row and nothing else (the mocked api has no other spectrum command).
    expect(vi.mocked(getSpectrumRow)).toHaveBeenCalled()
  })
})

describe('the picture is drawn only when something moved', () => {
  it('one draw per committed row, and none while the source is quiet', async () => {
    const rows = Array.from({ length: 4 }, (_, k) => audioRow(k))
    served = rows.map((row) => ({ row, loHz: 0, hiHz: 4000, source: 'audio' }))
    render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" rowMs={20} />)
    await until(() => commits.length === rows.length, 'every row committed')
    await settle(120)
    const after = scenes.length
    // Mount draws once (the empty band); each row at most once more.
    expect(after).toBeGreaterThanOrEqual(1)
    expect(after).toBeLessThanOrEqual(1 + rows.length)
    await settle(300)
    expect(scenes.length, 'frames with no new row drew the picture again').toBe(after)
  })

  it('a paused waterfall keeps committing, draws nothing, and its picture holds still', async () => {
    const { container } = render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" rowMs={20} />)
    served = [1, 2, 3, 4, 5, 6].map((k) => ({ row: audioRow(k), loHz: 0, hiHz: 4000, source: 'audio' }))
    await until(() => commits.length === 6, 'live rows committed')
    await settle(60)
    const last = () => scenes[scenes.length - 1]
    const pause = container.querySelector('button.wf-pause') as HTMLButtonElement
    await act(async () => pause.click())
    const drawn = scenes.length
    expect(last().offsetRows).toBe(0)
    served = [7, 8, 9].map((k) => ({ row: audioRow(k), loHz: 0, hiHz: 4000, source: 'audio' }))
    await until(() => commits.length === 9, 'paused rows committed')
    await settle(100)
    expect(scenes.length, 'rows arriving while paused redrew the picture').toBe(drawn)
    // One wheel step back from the PAUSED picture: the three rows that arrived since are not
    // scrolled through first (the newest row on screen had already aged by three).
    const canvas = container.querySelector('canvas.waterfall-canvas') as HTMLCanvasElement
    await act(async () => {
      canvas.dispatchEvent(new WheelEvent('wheel', { deltaY: 100, bubbles: true }))
    })
    expect(last().offsetRows).toBe(3 + 3)
    // Resume: back to the live tail, drawn at once.
    await act(async () => pause.click())
    expect(last().offsetRows).toBe(0)
    expect(scenes.length).toBe(drawn + 2)
  })
})
