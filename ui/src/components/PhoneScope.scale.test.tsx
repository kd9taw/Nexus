// @vitest-environment jsdom
//
// THE FREQUENCY SCALE IS THE AXIS MODEL, BY VALUE. The numbers along the foot of the Phone and CW scopes
// are the RF frequency at each tick through `axisToRf`, the inverse of where the spot tags, the notch and
// the passband are placed, so the scale names what a tag, the notch and a click name. It used to read
// `dial + audio` on every audio axis: right on Phone's carrier-centred axis and on plain USB audio, but in
// true CW a tone at the pitch IS the dial, so the CW scope read the pitch high (600 Hz by default), and it
// ran backwards on the reverse sideband and for the soundcard keyer below 10 MHz, where the rig is on LSB
// (2026-10-04). Read off the real scope's marks canvas: each tick's x and the label drawn beside it, at a
// pitch that is not the default.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'
import type { SpotRow } from '../types'
import { axisToRf } from '../spectrum/overlays'
import { notchOnAxis, type AxisKind } from '../spectrum/markers'
import { axisTicks, isSymmetricMode, scopeView, sidebandSign } from '../waterfall'

/** The row served: demodulated audio, or a native RF sweep. Flat, so a click finds no signal to snap
 *  to and lands where it was aimed. */
const AUDIO_ROW = { source: 'rx', loHz: 0, hiHz: 4000 }
const RF_ROW = { source: 'civ', loHz: 14_150_000, hiHz: 14_250_000 }
let served = AUDIO_ROW
let seq = 0
vi.mock('../api', () => ({
  getScopeFrame: vi.fn(async () => ({
    seq: ++seq, tMs: Date.now(), ...served,
    scale: served.source === 'civ' ? { kind: 'relative' } : { kind: 'dbfs', loDb: -120, hiDb: 0 },
    bins: Array.from({ length: 512 }, () => 0.1),
  })),
  getPrivilegeSpans: vi.fn(async () => null),
}))

/** Each canvas's paint calls, by its class: the overlays' and the marks'. */
const painted = new Map<string, string[]>()
const of = (cls: string) => painted.get(cls) ?? []
beforeEach(() => {
  seq = 0
  served = AUDIO_ROW
  painted.clear()
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
    constructor(a: Uint8ClampedArray | number, b: number, c?: number) {
      if (typeof a === 'number') { this.width = a; this.height = b; this.data = new Uint8ClampedArray(a * b * 4) }
      else { this.data = a; this.width = b; this.height = c ?? a.length / 4 / b }
    }
  })
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  const r = (n: number) => Math.round(n * 100) / 100
  const recorderFor = (canvas: HTMLCanvasElement) => {
    const cls = canvas.className.includes('ph-scope-overlays') ? 'overlays' : canvas.className.includes('ph-scope-canvas') ? 'marks' : 'picture'
    let fill = ''
    let stroke = ''
    let alpha = 1
    const log = (s: string) => { const l = painted.get(cls) ?? []; l.push(s); painted.set(cls, l) }
    return {
      set fillStyle(v: string) { fill = v }, get fillStyle() { return fill },
      set strokeStyle(v: string) { stroke = v }, get strokeStyle() { return stroke },
      set globalAlpha(v: number) { alpha = v }, get globalAlpha() { return alpha },
      fillRect: (x: number, y: number, w: number, h: number) => log(`fill ${fill} ${r(alpha)} ${r(x)} ${r(y)} ${r(w)} ${r(h)}`),
      moveTo: (x: number, y: number) => log(`move ${stroke} ${r(x)} ${r(y)}`),
      fillText: (text: string, x: number) => log(`text ${text} ${r(x)}`),
      clearRect: () => { painted.set(cls, []) },
      putImageData() {}, beginPath() {}, closePath() {}, lineTo() {}, fill() {}, stroke() {}, setLineDash() {},
      save() {}, restore() {}, rect() {}, clip() {}, measureText: (s: string) => ({ width: s.length * 6 }),
      createLinearGradient: () => ({ addColorStop() {} }),
    }
  }
  const contexts = new WeakMap<HTMLCanvasElement, unknown>()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement, kind: string) {
    if (kind !== '2d') return null
    if (!contexts.has(this)) contexts.set(this, recorderFor(this))
    return contexts.get(this)
  } as unknown as HTMLCanvasElement['getContext'])
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ x: 0, y: 0, left: 0, top: 0,
    width: 800, height: 200, right: 800, bottom: 200, toJSON() { return {} } })
  HTMLCanvasElement.prototype.setPointerCapture = vi.fn()
  localStorage.clear()
})
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers() })
async function draw(ms = 150) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

/** Not the 600 Hz default, so nothing can pass by assuming it. */
const PITCH = 500
/** The CW cockpit's audio window at that pitch behind a 400 Hz filter (`cwScopeWindow`): a tick every
 *  100 Hz, at x = 80, 240, 400 (the pitch), 560 and 720 of 800 px. */
const CW_VIEW = { viewLoHz: 250, viewHiHz: 750 }
const xAudio = (hz: number) => ((hz - CW_VIEW.viewLoHz) / (CW_VIEW.viewHiHz - CW_VIEW.viewLoHz)) * 800

interface Kind {
  name: string
  sideband: string
  /** False = the soundcard keyer: CW carried as a tone on SSB. */
  cwPitchRefDial?: boolean
  carrierCentered?: boolean
  rf?: boolean
  viewLoHz: number
  viewHiHz: number
  dial: number
}
const isCw = (k: Kind) => k.sideband.startsWith('CW')

/** Every axis kind the scope draws, with the props its host passes. The CW cockpit hands the scope `CW`
 *  or `CW-L` (`cwScopeSideSign`: the rig's reverse sideband, or the soundcard keyer's LSB below 10 MHz).
 *  Each dial ends in 30 Hz, so across the sweep below no label sits on a kilohertz rounding edge. */
const KINDS: Kind[] = [
  { name: 'true CW', sideband: 'CW', ...CW_VIEW, dial: 7_030_030 },
  { name: 'true CW on the reverse sideband', sideband: 'CW-L', ...CW_VIEW, dial: 7_030_030 },
  { name: 'the soundcard keyer above 10 MHz (USB)', sideband: 'CW', cwPitchRefDial: false, ...CW_VIEW, dial: 14_030_030 },
  { name: 'the soundcard keyer below 10 MHz (LSB)', sideband: 'CW-L', cwPitchRefDial: false, ...CW_VIEW, dial: 7_030_030 },
  { name: 'plain USB audio', sideband: 'USB', ...CW_VIEW, dial: 14_200_030 },
  { name: 'plain LSB audio', sideband: 'LSB', ...CW_VIEW, dial: 7_150_030 },
  { name: "Phone's carrier-centred axis, USB", sideband: 'USB', carrierCentered: true, viewLoHz: 0, viewHiHz: 2400, dial: 14_200_030 },
  { name: "Phone's carrier-centred axis, LSB", sideband: 'LSB', carrierCentered: true, viewLoHz: 0, viewHiHz: 2400, dial: 7_150_030 },
  { name: 'a native RF row', sideband: 'USB', rf: true, viewLoHz: -50_000, viewHiHz: 50_000, dial: 14_200_030 },
]
const kind = (name: string) => KINDS.find((k) => k.name === name)!

/** The axis the scope draws for this kind, as PhoneScope builds it for its marks. */
const axisOf = (k: Kind, dial: number | null, pitch = PITCH): AxisKind => ({
  rf: k.rf === true,
  carrierCentered: k.carrierCentered === true && k.rf !== true,
  sideband: k.sideband,
  dialHz: dial,
  pitchHz: pitch,
  cwPitchRefDial: k.cwPitchRefDial !== false,
})

function scene(k: Kind, dial: number | null, over: Partial<Parameters<typeof PhoneScope>[0]> = {}) {
  served = k.rf ? RF_ROW : AUDIO_ROW
  const onTune = vi.fn()
  const props = {
    transmitting: false, theme: 'dark', active: true, interactive: true, onTune,
    sideband: k.sideband, dialHz: dial, viewLoHz: k.viewLoHz, viewHiHz: k.viewHiHz,
    carrierCentered: k.carrierCentered === true, pitchHz: PITCH, cwPitchRefDial: k.cwPitchRefDial !== false,
    // The CW cockpit's pitch line on an audio row.
    markerHz: isCw(k) && !k.rf ? PITCH : null,
    ...over,
  }
  const ui = render(<PhoneScope {...props} />)
  const canvas = ui.container.querySelector<HTMLCanvasElement>('canvas.ph-scope-canvas')!
  const rerender = (more: Partial<Parameters<typeof PhoneScope>[0]>) => ui.rerender(<PhoneScope {...props} {...more} />)
  return { canvas, onTune, rerender }
}

/** The scale as drawn: `x label` for each tick, its rule (the one stroke at this alpha) then its label. */
const TICK_RULE = 'move rgba(255, 255, 255, 0.20) '
function scaleRead(): string[] {
  const out: string[] = []
  let x: string | null = null
  for (const p of of('marks')) {
    if (p.startsWith(TICK_RULE)) x = p.slice(TICK_RULE.length).split(' ')[0]
    else if (x != null && /^text \d+\.\d{3} /.test(p)) {
      out.push(`${x} ${p.split(' ')[1]}`)
      x = null
    }
  }
  return out
}
const mhz = (hz: number) => (hz / 1e6).toFixed(3)
/** The label the scale draws at x, or undefined where it draws none. */
const labelAt = (x: number, scale: string[]) => scale.find((p) => p.startsWith(`${Math.round(x)} `))?.split(' ')[1]

/** What the scale must read: at each round tick of the drawn view, the model's RF there (`axisToRf`), at
 *  the tick's pixel. Nothing where the model has no honest answer. */
function scaleWanted(k: Kind, dial: number | null, over: { pitch?: number; viewLoHz?: number; viewHiHz?: number } = {}): string[] {
  const pitch = over.pitch ?? PITCH
  const row = k.rf ? RF_ROW : AUDIO_ROW
  const v = scopeView(row.loHz, row.hiHz, row.source, over.viewLoHz ?? k.viewLoHz, over.viewHiHz ?? k.viewHiHz,
    isCw(k) && !k.rf ? pitch : null, sidebandSign(k.sideband), dial, isSymmetricMode(k.sideband), k.carrierCentered === true)
  const a = axisOf(k, dial, pitch)
  const out: string[] = []
  for (const t of axisTicks(v.loHz, v.hiHz, 6)) {
    const hz = axisToRf(a, t)
    if (hz == null) break
    out.push(`${Math.round(((t - v.loHz) / (v.hiHz - v.loHz)) * 800)} ${mhz(hz)}`)
  }
  return out
}

describe('the frequency scale is the axis model, by value', () => {
  it.each(KINDS)('$name: each label is axisToRf at its tick, across a kilohertz of dials', async (k) => {
    const s = scene(k, k.dial)
    for (let step = 0; step <= 10; step++) {
      const dial = k.dial + 100 * step
      s.rerender({ dialHz: dial })
      await draw()
      const want = scaleWanted(k, dial)
      expect(want.length, 'control: this axis has a scale to compare').toBeGreaterThan(2)
      expect(scaleRead(), `${k.name}, dial ${dial}`).toEqual(want)
    }
  })

  it('labels a native RF row at its own frequencies, the dial known or not', async () => {
    // It is absolute already: adding the dial here would label this 14 MHz sweep at 28 MHz.
    for (const dial of [null, 14_200_030]) {
      scene(kind('a native RF row'), dial)
      await draw()
      expect(scaleRead(), `dial ${dial}`).toEqual(['80 14.160', '240 14.180', '400 14.200', '560 14.220', '720 14.240'])
      cleanup()
    }
  })

  it('reads the dial under the pitch line in true CW, on either sideband and at any pitch', async () => {
    // 7.030130 MHz reads "7.030"; a pitch-high scale read 7.030630 or 7.030830, "7.031".
    for (const name of ['true CW', 'true CW on the reverse sideband']) {
      for (const pitch of [500, 700]) {
        // `cwScopeWindow(pitch, 400)`: the pitch lands at x = 400 either way.
        const view = { viewLoHz: pitch - 250, viewHiHz: pitch + 250 }
        scene(kind(name), 7_030_130, { pitchHz: pitch, markerHz: pitch, ...view })
        await draw()
        expect(axisToRf(axisOf(kind(name), 7_030_130, pitch), pitch), 'the model: a tone at the pitch IS the dial').toBe(7_030_130)
        expect(scaleRead(), `${name} at ${pitch} Hz`).toEqual(scaleWanted(kind(name), 7_030_130, { pitch, ...view }))
        expect(labelAt(400, scaleRead()), `${name} at ${pitch} Hz`).toBe('7.030')
        cleanup()
      }
    }
  })

  it('draws no number where none is honest: the dial unknown, or an AM or FM baseband', async () => {
    const phone = kind("Phone's carrier-centred axis, USB")
    scene(phone, 14_200_030)
    await draw()
    expect(scaleRead().length, 'control: the same axis with a sideband and a dial is labelled').toBeGreaterThan(2)
    cleanup()
    const blank: [Kind, number | null][] = [
      [kind('true CW'), null],
      [{ ...phone, name: 'FM', sideband: 'FM' }, 146_520_030],
      [{ ...phone, name: 'AM', sideband: 'AM' }, 3_885_030],
      [{ ...kind('plain USB audio'), name: 'AM, plain audio', sideband: 'AM' }, 3_885_030],
    ]
    for (const [k, dial] of blank) {
      scene(k, dial)
      await draw()
      expect(scaleRead(), k.name).toEqual([])
      cleanup()
    }
  })
})

describe('a tag, the notch and a click land where the scale says', () => {
  const spot = (hz: number): SpotRow => ({
    call: 'K1ABC', entity: '', zone: 0, band: '40m', freqMhz: hz / 1e6, mode: 'CW', spotter: 'W1AW',
    corroborators: [], ageSecs: 60, comment: '', licensed: true,
  })
  // The CW dials sit on the click's 10 Hz grid and the SSB dials on its 100 Hz grid, so the click's
  // rounding moves nothing and the comparison below is exact.
  const AGREE: { k: Kind; dial: number }[] = [
    { k: kind('true CW'), dial: 7_030_130 },
    { k: kind('true CW on the reverse sideband'), dial: 7_030_130 },
    { k: kind('the soundcard keyer above 10 MHz (USB)'), dial: 14_030_130 },
    { k: kind('the soundcard keyer below 10 MHz (LSB)'), dial: 7_030_130 },
    { k: kind('plain USB audio'), dial: 14_200_200 },
    { k: kind('plain LSB audio'), dial: 7_150_200 },
  ]
  it.each(AGREE)('$k.name', async ({ k, dial }) => {
    const a = axisOf(k, dial)
    // A spot at the frequency the model puts at the 600 Hz tick, and the rig's notch at 700 Hz of audio.
    const at600 = axisToRf(a, 600)!
    const s = scene(k, dial, { spots: [{ spot: spot(at600), ink: null }], notchHz: 700 })
    await draw()
    const before = scaleRead()

    // The tag's tick stands on the 600 Hz tick of the scale, and the scale there reads the spot.
    expect(of('overlays')).toContain(`move  ${xAudio(600)} 27`)
    expect(labelAt(xAudio(600), before)).toBe(mhz(at600))

    // The notch band is centred on the 700 Hz tick (3 px wide from x = 719), and the RF it removes, where
    // the RF scope draws it, is what the scale reads there.
    expect(of('marks')).toContain(`fill rgba(255, 96, 96, 0.45) 1 ${Math.round(xAudio(700) - 1.5)} 0 3 200`)
    expect(notchOnAxis({ ...a, rf: true }, 700)).toBe(axisToRf(a, 700))

    // A click on the 700 Hz tick brings that signal to the pitch line (CW) or its voice to 300 Hz (SSB), and
    // the scale then reads there what it read under the click: true CW tunes the dial to it exactly.
    fireEvent.pointerDown(s.canvas, { button: 0, clientX: xAudio(700), clientY: 150 })
    fireEvent.pointerUp(s.canvas, { button: 0, clientX: xAudio(700), clientY: 150 })
    expect(s.onTune).toHaveBeenCalledTimes(1)
    const tuned: number = s.onTune.mock.calls[0][0].dialHz
    const landing = isCw(k) ? PITCH : 300
    expect(axisToRf({ ...a, dialHz: tuned }, landing), 'the model after the click').toBe(axisToRf(a, 700))
    if (isCw(k) && k.cwPitchRefDial !== false) expect(tuned).toBe(axisToRf(a, 700))
    s.rerender({ dialHz: tuned })
    await draw()
    expect(labelAt(xAudio(landing), scaleRead()), 'the scale after the click').toBe(labelAt(xAudio(700), before))
  })
})
