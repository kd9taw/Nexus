// @vitest-environment jsdom
//
// THE RF WINDOW A SCOPE DRAWS IS ASKED FOR, NOT ONLY CROPPED. On a native RF row the scope's poll
// carries the absolute window it wants (`getScopeFrame`'s `rfView`): an SDR's IQ panadapter computes
// its rows over it, finer than its whole span. The Band span's absolute window (`rfViewHz`) is drawn
// clamped to the row and reported through `onFeed`; the row's own span through `onSweep`. Runs the
// component's real loop and poll on fake timers against a scripted source (the cadence test's
// harness), with the reported 30 m case: an SDR sweeping 9.12-11.12 MHz.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'

const ROW = Array.from({ length: 2048 }, (_, i) => (i === 1034 ? 0.9 : 0.1))
const src = {
  source: 'sdrconnect',
  loHz: 9_120_000,
  hiHz: 11_120_000,
  seq: 0,
  asked: [] as ({ loHz: number; hiHz: number } | undefined)[],
}

vi.mock('../api', () => ({
  getScopeFrame: (_lo: number, _hi: number, _win: string | undefined, _seq: number, rfView?: { loHz: number; hiHz: number }) => {
    src.asked.push(rfView)
    src.seq++
    const rf = src.source !== 'audio'
    return Promise.resolve({
      seq: src.seq,
      tMs: Date.now(),
      source: src.source,
      loHz: rf ? src.loHz : 0,
      hiHz: rf ? src.hiHz : 4000,
      scale: rf ? { kind: 'relative' } : { kind: 'dbfs', loDb: -120, hiDb: 0 },
      bins: ROW,
    })
  },
}))

beforeEach(() => {
  Object.assign(src, { source: 'sdrconnect', loHz: 9_120_000, hiHz: 11_120_000, seq: 0, asked: [] })
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
  const paint = {
    fillRect() {}, clearRect() {}, putImageData() {}, beginPath() {}, closePath() {}, moveTo() {}, lineTo() {}, fillText() {},
    fill() {}, stroke() {}, setLineDash() {}, save() {}, restore() {}, rect() {}, clip() {},
    measureText: (t: string) => ({ width: t.length * 6 }), createLinearGradient: () => ({ addColorStop() {} }),
  }
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(((kind: string) =>
    kind === '2d' ? paint : null) as unknown as HTMLCanvasElement['getContext'])
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ x: 0, y: 0, left: 0, top: 0,
    width: 400, height: 200, right: 400, bottom: 200, toJSON() { return {} } })
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

const DIAL = 10_120_000
const BAND30 = { loHz: 10_095_000, hiHz: 10_155_000 }

describe('the RF window on a native panadapter row', () => {
  it('Band: draws the band clamped to the row, asks for it on every poll, and reports the sweep', async () => {
    const onFeed = vi.fn()
    const onSweep = vi.fn()
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={-1e9} viewHiHz={1e9} dialHz={DIAL}
      rfViewHz={BAND30} onFeed={onFeed} onSweep={onSweep} />)
    await run(600)
    expect(src.asked.length, 'control: the loop polled').toBeGreaterThan(5)
    // The first poll has no row yet, so asks nothing; every one after asks for the band.
    expect(src.asked[0]).toBeUndefined()
    expect(src.asked.slice(1).every((v) => v?.loHz === BAND30.loHz && v?.hiHz === BAND30.hiHz)).toBe(true)
    expect(onFeed).toHaveBeenLastCalledWith('sdrconnect', BAND30.loHz, BAND30.hiHz)
    expect(onSweep).toHaveBeenLastCalledWith('sdrconnect', 9_120_000, 11_120_000)
  })

  it('a band the row covers only part of is drawn as that part', async () => {
    Object.assign(src, { source: 'civ', loHz: 10_070_000, hiHz: 10_130_000 })
    const onFeed = vi.fn()
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={-1e9} viewHiHz={1e9} dialHz={DIAL}
      rfViewHz={BAND30} onFeed={onFeed} />)
    await run(400)
    expect(onFeed).toHaveBeenLastCalledWith('civ', BAND30.loHz, 10_130_000)
  })

  it('±25k: asks for the dial ± 25 kHz; Full asks for nothing', async () => {
    const { rerender } = render(<PhoneScope transmitting={false} theme="dark" viewLoHz={-25_000} viewHiHz={25_000} dialHz={DIAL} />)
    await run(400)
    expect(src.asked[src.asked.length - 1]).toEqual({ loHz: DIAL - 25_000, hiHz: DIAL + 25_000 })
    rerender(<PhoneScope transmitting={false} theme="dark" viewLoHz={-1e9} viewHiHz={1e9} dialHz={DIAL} />)
    src.asked.length = 0
    await run(400)
    expect(src.asked.length, 'control: still polling').toBeGreaterThan(2)
    expect(src.asked.every((v) => v === undefined), 'Full is the whole sweep: nothing asked').toBe(true)
  })

  it('an audio row asks for no RF window, whatever the props', async () => {
    src.source = 'audio'
    render(<PhoneScope transmitting={false} theme="dark" dialHz={DIAL} rfViewHz={BAND30} />)
    await run(400)
    expect(src.asked.length, 'control: the loop polled').toBeGreaterThan(2)
    expect(src.asked.every((v) => v === undefined)).toBe(true)
  })
})
