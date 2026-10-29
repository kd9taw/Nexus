// @vitest-environment jsdom
//
// CW click-to-tune is exact at every span of the CW audio scope. A click on a received tone tunes
// the dial that puts it on the pitch, and that dial must not depend on how wide the scope is
// drawn: the Filter window round the pitch and the waterfalls' Std view (200–3000 Hz) map the
// same tone to the same dial. The real draw loop, pointer handling, signal detector and sideband
// math run; only the audio row is sampled.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'
import { cwAudioView } from '../waterfall'

/** The received tones (audio Hz). The row honours the window it is asked for, as the engine does. */
let tones = [750]
let seq = 0
vi.mock('../api', () => ({
  getScopeFrame: vi.fn(async (lo: number, hi: number) => ({
    seq: ++seq, tMs: Date.now(), source: 'rx', loHz: lo, hiHz: hi,
    scale: { kind: 'dbfs', loDb: -120, hiDb: 0 },
    bins: Array.from({ length: 512 }, (_, i) => {
      const hz = lo + ((i + 0.5) / 512) * (hi - lo)
      return tones.some((t) => Math.abs(hz - t) <= Math.max(4, (hi - lo) / 512)) ? 0.9 : 0.1
    }),
  })),
}))

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => setTimeout(() => cb(performance.now()), 16))
  vi.stubGlobal('cancelAnimationFrame', clearTimeout)
  vi.stubGlobal('PointerEvent', class extends MouseEvent {
    pointerId: number
    constructor(type: string, init: PointerEventInit = {}) { super(type, init); this.pointerId = init.pointerId ?? 1 }
  })
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  vi.stubGlobal('ImageData', class {
    data: Uint8ClampedArray; width: number; height: number
    // Both forms: (width, height), which the renderer's band takes, and (data, width, height).
    constructor(a: Uint8ClampedArray | number, b: number, c?: number) {
      if (typeof a === 'number') { this.width = a; this.height = b; this.data = new Uint8ClampedArray(a * b * 4) }
      else { this.data = a; this.width = b; this.height = c ?? a.length / 4 / b }
    }
  })
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  // A 2D paint target only: `webgl2` answers null, so the renderer draws on its canvas-2D path.
  const paint = {
    fillRect() {}, clearRect() {}, putImageData() {}, beginPath() {}, closePath() {}, moveTo() {}, lineTo() {}, fillText() {}, fill() {}, stroke() {}, setLineDash() {},
    save() {}, restore() {}, rect() {}, clip() {}, measureText: (t: string) => ({ width: t.length * 6 }),
    createLinearGradient: () => ({ addColorStop() {} }),
  }
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(((kind: string) =>
    kind === '2d' ? paint : null) as unknown as HTMLCanvasElement['getContext'])
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ x: 0, y: 0, left: 0, top: 0,
    width: 800, height: 200, right: 800, bottom: 200, toJSON() { return {} } })
  HTMLCanvasElement.prototype.setPointerCapture = vi.fn()
  localStorage.clear()
})
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers() })
async function draw(ms = 150) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

const DIAL = 14_050_000
const PITCH = 600
const W = 800

/** Click the scope at audio frequency `tone` (Hz), at a span; the dial the click tunes. */
async function clickTone(span: 'filter' | 'std', tone: number) {
  const { loHz, hiHz } = cwAudioView(span, PITCH, 500)
  const onTune = vi.fn()
  const ui = render(
    <PhoneScope cockpit="cw" sideband="CW" dialHz={DIAL} pitchHz={PITCH} cwPitchRefDial
      viewLoHz={loHz} viewHiHz={hiHz} markerHz={PITCH} transmitting={false} active interactive
      theme="dark" filterWidthHz={500} onTune={onTune} />,
  )
  await draw(300)
  const canvas = ui.container.querySelector<HTMLCanvasElement>('canvas.ph-scope-canvas')!
  const x = ((tone - loHz) / (hiHz - loHz)) * W
  fireEvent.pointerDown(canvas, { button: 0, clientX: x, clientY: 100 })
  fireEvent.pointerUp(canvas, { button: 0, clientX: x, clientY: 100 })
  cleanup()
  expect(onTune, `${span}: the click tuned nothing`).toHaveBeenCalledOnce()
  return onTune.mock.calls[0][0].dialHz as number
}

describe('CW click-to-tune at every span', () => {
  it.each([750, 480, 820])('a tone at %i Hz tunes the same dial at Filter and at Std', async (tone) => {
    tones = [tone]
    const atFilter = await clickTone('filter', tone)
    const atStd = await clickTone('std', tone)
    expect(atStd).toBe(atFilter)
    // The dial that puts the tone on the pitch (CW, upper side): up by the tone's distance above it.
    expect(atFilter).toBe(DIAL + (tone - PITCH))
  })

  it.each([700, 520, 880])('with no tone to snap to, a click at %i Hz tunes that frequency at Filter and at Std', async (hz) => {
    tones = []
    const atFilter = await clickTone('filter', hz)
    const atStd = await clickTone('std', hz)
    expect(atFilter).toBe(DIAL + (hz - PITCH))
    expect(atStd).toBe(DIAL + (hz - PITCH))
  })
})
