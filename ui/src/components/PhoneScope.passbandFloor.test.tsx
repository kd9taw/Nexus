// @vitest-environment jsdom
//
// THE AUDIO SCOPE'S NOISE FLOOR IS MEASURED INSIDE THE RADIO'S PASSBAND (2026-11-03).
//
// The CW scope now opens on Std (200–3000 Hz) and Phone on Full (0–5000 Hz), wider than the radio's
// filter, so most of the window is the filter's stopband, ~40 dB down. The floor is the window's
// median, and over that window the median IS the stopband: the floor sat 40 dB under the receiver's
// noise and the passband drew as one bright slab (seen in Chrome on both cockpits). Inside the
// reported passband the median is the receiver's noise again.
//
// The observable is the ▲dB readout, which is the strongest signal's height above that floor: the
// same row reads 24 dB over the receiver's noise, not 64 dB over the stopband. The harness is
// PhoneScope.readout.test.tsx's (the component's own loop under fake timers).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render, screen } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'

vi.setConfig({ testTimeout: 15_000 })

/** A post-filter audio row: receiver noise at 0.5 inside `passHz` (from `passLo`), the stopband
 *  40 dB under it, one carrier at `carrierHz` 24 dB over the noise (1 unit = 120 dB). */
const src = { passLo: 100, passHi: 2500, carrierHz: 1000, polls: 0, source: 'audio' }

vi.mock('../api', () => ({
  getScopeFrame: (lo: number, hi: number) => {
    src.polls++
    const bins = Array.from({ length: 512 }, (_, i) => {
      const hz = lo + ((i + 0.5) * (hi - lo)) / 512
      if (Math.abs(hz - src.carrierHz) < (hi - lo) / 512) return 0.7
      return hz > src.passLo && hz < src.passHi ? 0.5 : 0.5 - 40 / 120
    })
    return Promise.resolve({ seq: src.polls, tMs: 1_700_000_000_000 + src.polls * 50, source: src.source, loHz: lo, hiHz: hi,
      scale: { kind: 'dbfs', loDb: -120, hiDb: 0 }, bins })
  },
}))

beforeEach(() => {
  Object.assign(src, { passLo: 100, passHi: 2500, carrierHz: 1000, polls: 0, source: 'audio' })
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
    fill() {}, stroke() {}, setLineDash() {}, save() {}, restore() {}, rect() {}, clip() {}, arc() {}, strokeRect() {},
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
/** The ▲dB readout, as a number. */
const readout = () => Number(/▲\s*(-?\d+)/.exec(screen.getByTitle(/strongest signal in this view stands above the noise floor/).textContent ?? '')?.[1])

describe('the audio scope’s noise floor is the receiver’s, not its stopband’s', () => {
  it('Phone, Full (0–5000 Hz) over a 2.4 kHz filter: the carrier reads 24 dB over the noise', async () => {
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={5000} carrierCentered sideband="USB" passbandHz={2400} />)
    await run(600)
    expect(src.polls, 'control: the loop polled').toBeGreaterThan(3)
    expect(readout()).toBe(24)
  })

  it('Phone on LSB, Full over a 2.4 kHz filter: the mirrored axis measures the same passband, 24 dB', async () => {
    // LSB runs the carrier-centred axis backwards (the passband is −2400..0 on it), so the floor's
    // window is the passband's edges negated and swapped back into row Hz: 0..2400 of the row.
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={5000} carrierCentered sideband="LSB" passbandHz={2400} />)
    await run(600)
    expect(src.polls, 'control: the loop polled').toBeGreaterThan(3)
    expect(readout()).toBe(24)
  })

  it('CONTROL on LSB: with no reported filter, the stopband’s median', async () => {
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={5000} carrierCentered sideband="LSB" passbandHz={null} />)
    await run(600)
    expect(readout()).toBe(64)
  })

  it('CW, Std (200–3000 Hz) over a 500 Hz filter round a 600 Hz pitch: 24 dB over the noise', async () => {
    Object.assign(src, { passLo: 350, passHi: 850, carrierHz: 600 })
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={200} viewHiHz={3000} sideband="CW" pitchHz={600} markerHz={600} passbandHz={500} />)
    await run(600)
    expect(readout()).toBe(24)
  })

  it('CONTROL: with no reported filter the window’s median is all there is, and it is the stopband', async () => {
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={5000} carrierCentered sideband="USB" passbandHz={null} />)
    await run(600)
    expect(readout()).toBe(64)
  })

  it('a filter that fills the window measures what it always did', async () => {
    // Auto: the window IS the filter, so the passband and the window agree.
    Object.assign(src, { passLo: 0, passHi: 6000 })
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={2400} carrierCentered sideband="USB" passbandHz={2400} />)
    await run(600)
    expect(readout()).toBe(24)
  })
})
