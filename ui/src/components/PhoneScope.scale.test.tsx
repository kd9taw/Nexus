// @vitest-environment jsdom
//
// THE FREQUENCY SCALE IS THE AXIS MODEL, BY VALUE. The numbers along the foot of the Phone and CW scopes
// are the RF frequency at each tick through `axisToRf`, the inverse of where the spot tags, the notch and
// the passband are placed, so the scale names what a tag, the notch and a click name. It used to read
// `dial + audio` on every audio axis: right on Phone's carrier-centred axis and on plain USB audio, but in
// true CW a tone at the pitch IS the dial, so the CW scope read the pitch high (600 Hz by default), and it
// ran backwards on the reverse sideband and for the soundcard keyer below 10 MHz, where the rig is on LSB
// (2026-10-04). Read off the real scope's marks canvas: each tick's x and the label drawn beside it, at a
// pitch that is not the default. A label is three decimals of MHz, and four (the 100 Hz digit) where the
// ticks stand under a kilohertz apart, so CW's narrow window no longer reads the same kHz under every tick
// (operator, 2026-10-04).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'
import type { SpotRow } from '../types'
import { axisToRf } from '../spectrum/overlays'
import { notchOnAxis, type AxisKind } from '../spectrum/markers'
import { axisTicks, cwScopeWindow, isSymmetricMode, scopeView, sidebandSign } from '../waterfall'

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
/** The width the stand-in canvas measures a string at: 6 px a character. */
const measured = (s: string) => s.length * 6
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
      save() {}, restore() {}, rect() {}, clip() {}, measureText: (s: string) => ({ width: measured(s) }),
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
    else if (x != null && /^text \d+\.\d{3,4} /.test(p)) {
      out.push(`${x} ${p.split(' ')[1]}`)
      x = null
    }
  }
  return out
}
/** The label for `hz` on a scale whose ticks stand `step` apart: the kHz an operator dials, three decimals
 *  of MHz, from a kilohertz up; below that the 100 Hz digit too, rounded in whole hertz. */
const mhz = (hz: number, step: number) => (step < 1000 ? (Math.round(hz / 100) / 1e4).toFixed(4) : (hz / 1e6).toFixed(3))
/** The label the scale draws at x, or undefined where it draws none. */
const labelAt = (x: number, scale: string[]) => scale.find((p) => p.startsWith(`${Math.round(x)} `))?.split(' ')[1]

/** The drawn view for this kind, as PhoneScope builds it from the row it is served. */
function viewOf(k: Kind, dial: number | null, over: { pitch?: number; viewLoHz?: number; viewHiHz?: number } = {}) {
  const row = k.rf ? RF_ROW : AUDIO_ROW
  return scopeView(row.loHz, row.hiHz, row.source, over.viewLoHz ?? k.viewLoHz, over.viewHiHz ?? k.viewHiHz,
    isCw(k) && !k.rf ? (over.pitch ?? PITCH) : null, sidebandSign(k.sideband), dial, isSymmetricMode(k.sideband), k.carrierCentered === true)
}
/** The step between the scale's ticks on that view. */
function stepOf(k: Kind, dial: number | null, over: { pitch?: number; viewLoHz?: number; viewHiHz?: number } = {}): number {
  const v = viewOf(k, dial, over)
  const ticks = axisTicks(v.loHz, v.hiHz, 6)
  return ticks[1] - ticks[0]
}

/** What the scale must read: at each round tick of the drawn view, the model's RF there (`axisToRf`), at
 *  the tick's pixel. Nothing where the model has no honest answer. */
function scaleWanted(k: Kind, dial: number | null, over: { pitch?: number; viewLoHz?: number; viewHiHz?: number } = {}): string[] {
  const pitch = over.pitch ?? PITCH
  const v = viewOf(k, dial, over)
  const a = axisOf(k, dial, pitch)
  const ticks = axisTicks(v.loHz, v.hiHz, 6)
  const out: string[] = []
  for (const t of ticks) {
    const hz = axisToRf(a, t)
    if (hz == null) break
    out.push(`${Math.round(((t - v.loHz) / (v.hiHz - v.loHz)) * 800)} ${mhz(hz, ticks[1] - ticks[0])}`)
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
    // 7.030130 MHz reads "7.0301"; a pitch-high scale read 7.030630 or 7.030830, "7.0306" or "7.0308".
    for (const name of ['true CW', 'true CW on the reverse sideband']) {
      for (const pitch of [500, 700]) {
        // `cwScopeWindow(pitch, 400)`: the pitch lands at x = 400 either way.
        const view = { viewLoHz: pitch - 250, viewHiHz: pitch + 250 }
        scene(kind(name), 7_030_130, { pitchHz: pitch, markerHz: pitch, ...view })
        await draw()
        expect(axisToRf(axisOf(kind(name), 7_030_130, pitch), pitch), 'the model: a tone at the pitch IS the dial').toBe(7_030_130)
        expect(scaleRead(), `${name} at ${pitch} Hz`).toEqual(scaleWanted(kind(name), 7_030_130, { pitch, ...view }))
        expect(labelAt(400, scaleRead()), `${name} at ${pitch} Hz`).toBe('7.0301')
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

describe('the scale reads to 100 Hz where its ticks stand under a kilohertz apart', () => {
  // Operator, 2026-10-04: on CW's narrow window, 300 to 800 Hz wide, three decimals of MHz put the same kHz
  // under every tick (`7.030` five times by default). Under a 1 kHz step each label carries the 100 Hz digit;
  // from 1 kHz up it is the label it always was. AM and FM stay blank (above).
  const CW_KINDS = KINDS.filter(isCw)
  /** Every width the CW cockpit's window takes (`cwScopeWindow`: the filter plus a quarter, from 300 to
   *  800 Hz), and the 300-800 Hz window itself. */
  const CW_WINDOWS = [...[240, 400, 500, 560, null].map((f) => cwScopeWindow(PITCH, f)), { loHz: 300, hiHz: 800 }]

  it('pins a 300-800 Hz window, with its ticks off a 50 Hz rounding edge and on one', async () => {
    const view = { viewLoHz: 300, viewHiHz: 800 }
    scene(kind('true CW'), 7_030_000, view)
    await draw()
    expect(scaleRead()).toEqual(['0 7.0298', '160 7.0299', '320 7.0300', '480 7.0301', '640 7.0302', '800 7.0303'])
    cleanup()
    // Every tick ends in 50 Hz: rounded half up in whole hertz, never toward its neighbour.
    scene(kind('true CW'), 7_030_050, view)
    await draw()
    expect(scaleRead()).toEqual(['0 7.0299', '160 7.0300', '320 7.0301', '480 7.0302', '640 7.0303', '800 7.0304'])
    cleanup()
    // A window too narrow for two ticks (no cockpit makes one: CW's floors at 300 Hz) reads like the finest step.
    scene(kind('true CW'), 7_030_000, { viewLoHz: 450, viewHiHz: 520 })
    await draw()
    expect(scaleRead()).toEqual(['571 7.0300'])
  })

  it.each(CW_KINDS)('$name: adjacent labels are one step apart, never alike, on every CW window and dial', async (k) => {
    for (const w of CW_WINDOWS) {
      const view = { viewLoHz: w.loHz, viewHiHz: w.hiHz }
      const s = scene(k, k.dial, view)
      // Through a whole 100 Hz at the CW tuning step, 10 Hz, so the ticks cross the 50 Hz rounding edge.
      for (let d = 0; d < 100; d += 10) {
        const dial = k.dial - 30 + d
        s.rerender({ dialHz: dial })
        await draw()
        const labels = scaleRead().map((p) => p.split(' ')[1])
        const at = `${k.name}, window ${w.loHz}-${w.hiHz} Hz, dial ${dial}`
        expect(labels.length, `control: ${at} has a scale`).toBeGreaterThanOrEqual(3)
        // Not merely different: a step's own distance apart, so two ticks 100 Hz apart never read 200.
        const units = stepOf(k, dial, view) / 100
        for (let i = 1; i < labels.length; i++) {
          const apart = Math.abs(Math.round((Number(labels[i]) - Number(labels[i - 1])) * 1e4))
          expect(apart, `${at}: ${labels[i - 1]} then ${labels[i]}`).toBe(units)
        }
      }
      cleanup()
    }
  })

  it("does the same on Phone's scope at a 2.4 kHz width, where the ticks stand 500 Hz apart", async () => {
    // It read `14.200 14.200 14.201 14.201 14.202` here.
    scene(kind("Phone's carrier-centred axis, USB"), 14_200_000)
    await draw()
    expect(scaleRead()).toEqual(['89 14.2000', '237 14.2005', '385 14.2010', '533 14.2015', '681 14.2020'])
  })

  it('keeps its three decimals, byte for byte, where the ticks stand a kilohertz or more apart', async () => {
    // What the scale drew before the 100 Hz digit, `(hz / 1e6).toFixed(3)` at the model's RF: Phone at a 4 kHz
    // width and plain audio over the whole row (a tick every 1 kHz), and a native RF row 6, 12, 30 and 100 kHz
    // wide (1, 2, 5 and 20 kHz).
    const phone = kind("Phone's carrier-centred axis, USB")
    scene(phone, 14_200_030, { viewLoHz: 0, viewHiHz: 4000 })
    await draw()
    expect(scaleRead()).toEqual(['89 14.200', '267 14.201', '444 14.202', '622 14.203', '800 14.204'])
    cleanup()
    const wide: [Kind, number, number][] = [
      [phone, 0, 4000],
      [kind("Phone's carrier-centred axis, LSB"), 0, 4000],
      [kind('plain USB audio'), 0, 4000],
      [kind('plain LSB audio'), 0, 4000],
      ...[3_000, 6_000, 15_000, 50_000].map((h): [Kind, number, number] => [kind('a native RF row'), -h, h]),
    ]
    for (const [k, lo, hi] of wide) {
      const view = { viewLoHz: lo, viewHiHz: hi }
      const s = scene(k, k.dial, view)
      for (const d of [0, 20, 470, 950]) {
        const dial = k.dial + d
        s.rerender({ dialHz: dial })
        await draw()
        const at = `${k.name}, ${lo} to ${hi} Hz, dial ${dial}`
        expect(stepOf(k, dial, view), `control: ${at} steps a kilohertz or more`).toBeGreaterThanOrEqual(1000)
        const v = viewOf(k, dial, view)
        const before = axisTicks(v.loHz, v.hiHz, 6).map((t) =>
          `${Math.round(((t - v.loHz) / (v.hiHz - v.loHz)) * 800)} ${(axisToRf(axisOf(k, dial), t)! / 1e6).toFixed(3)}`)
        expect(before.length, `control: ${at} has a scale`).toBeGreaterThan(2)
        expect(scaleRead(), at).toEqual(before)
      }
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
    expect(labelAt(xAudio(600), before)).toBe(mhz(at600, stepOf(k, dial)))

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

describe('no label prints over another, at any width', () => {
  // On a narrow scope the label at the right edge, nudged in so it is not clipped, stood over the one before it:
  // the Remote page's CW scope on a phone, at 20 m and at 2 m, where the 100 Hz digit lengthens every label
  // (2026-10-05). A label with no room beside the one before it is left off, and its tick stays. Every other
  // label stands where it always has, 3 px right of its tick or nudged in from an edge, so a scale with room for
  // all of them is drawn as it was.

  /** The scope laid out `width` px wide, at no zoom: the scale's text unit is 1 px. */
  const sized = (width: number) => vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ x: 0, y: 0,
    left: 0, top: 0, width, height: 200, right: width, bottom: 200, toJSON() { return {} } })
  /** The x of every tick drawn, left to right. */
  const ticksRead = () => of('marks').filter((p) => p.startsWith(TICK_RULE)).map((p) => Number(p.slice(TICK_RULE.length).split(' ')[0]))
  /** The labels drawn, left to right: each one's tick, its text, and where it starts and ends. */
  function labelsRead() {
    const out: { tick: number; text: string; x: number; end: number }[] = []
    let tick: number | null = null
    for (const p of of('marks')) {
      if (p.startsWith(TICK_RULE)) tick = Number(p.slice(TICK_RULE.length).split(' ')[0])
      else if (tick != null && /^text \d+\.\d{3,4} /.test(p)) {
        const [, text, x] = p.split(' ')
        out.push({ tick, text, x: Number(x), end: Number(x) + measured(text) })
        tick = null
      }
    }
    return out
  }
  /** Where the scale has always put a label `w` wide: 3 px right of its tick, nudged in to 2 px from an edge. */
  const placed = (tick: number, w: number, width: number) => Math.min(width - w - 2, Math.max(2, tick + 3))
  /** The room a label keeps from the one before it: the 3 px it keeps from its own tick. */
  const ROOM = 3

  /** The CW cockpit's 300-800 Hz window (a 550 Hz pitch behind a 400 Hz filter): six ticks 100 Hz apart, the
   *  last on the right edge. */
  const NARROW = { pitch: 550, viewLoHz: 300, viewHiHz: 800 }
  const cwView = (v: typeof NARROW) => ({ pitchHz: v.pitch, markerHz: v.pitch, viewLoHz: v.viewLoHz, viewHiHz: v.viewHiHz })

  it.each([{ band: '20 m', dial: 14_030_050 }, { band: '2 m', dial: 144_050_050 }])(
    'the CW scope on a phone at $band: each label stands clear of the one before it', async ({ band, dial }) => {
      // A phone held upright.
      for (const width of [360, 390, 412]) {
        sized(width)
        scene(kind('true CW'), dial, cwView(NARROW))
        await draw()
        const labels = labelsRead()
        expect(labels.length, `control: ${band} at ${width} px has a scale`).toBeGreaterThanOrEqual(3)
        for (let i = 1; i < labels.length; i++) {
          expect(labels[i].x - labels[i - 1].end, `${band} at ${width} px: ${labels[i - 1].text} then ${labels[i].text}`)
            .toBeGreaterThanOrEqual(ROOM)
        }
        cleanup()
      }
    })

  it('at every width, a label is left off only where it has no room, and the rest stand where they always have', async () => {
    // Every scale the scopes draw: CW's 300-800 Hz window from 40 m to 23 cm (the longest labels), its widest
    // window (200-1000 Hz, the last tick on the edge), Phone at a 2.4 kHz width, and a native RF row.
    const SCALES: { name: string; k: Kind; dial: number; view?: typeof NARROW }[] = [
      { name: 'CW at 40 m', k: kind('true CW'), dial: 7_030_050, view: NARROW },
      { name: 'CW at 20 m', k: kind('true CW'), dial: 14_030_050, view: NARROW },
      { name: 'CW at 2 m', k: kind('true CW'), dial: 144_050_050, view: NARROW },
      { name: 'CW at 23 cm', k: kind('true CW'), dial: 1_296_050_050, view: NARROW },
      { name: 'CW, 200-1000 Hz', k: kind('true CW'), dial: 14_030_000, view: { pitch: 600, viewLoHz: 200, viewHiHz: 1000 } },
      { name: 'Phone at 2.4 kHz', k: kind("Phone's carrier-centred axis, USB"), dial: 14_200_000 },
      { name: 'a native RF row', k: kind('a native RF row'), dial: 14_200_030 },
    ]
    // Phone widths closely, then the widths a desktop cockpit gives it.
    const WIDTHS = [...Array.from({ length: 34 }, (_, i) => 200 + 16 * i), 800, 1024, 1280]
    let leftOff = 0
    for (const { name, k, dial, view } of SCALES) {
      // What each tick reads (the model, as above), left to right.
      const texts = scaleWanted(k, dial, view).map((p) => p.split(' ')[1])
      for (const width of WIDTHS) {
        sized(width)
        scene(k, dial, view ? cwView(view) : {})
        await draw()
        const at = `${name} at ${width} px`
        const ticks = ticksRead()
        const labels = labelsRead()
        // Every tick stands, labelled or not.
        expect(ticks.length, at).toBe(texts.length)
        let end = -Infinity
        let j = 0
        for (let i = 0; i < ticks.length; i++) {
          const x = placed(ticks[i], measured(texts[i]), width)
          if (labels[j]?.tick === ticks[i]) {
            // Drawn: the tick's own reading, where the scale always put it, and clear of the label before it.
            expect(labels[j].text, at).toBe(texts[i])
            expect(labels[j].x, `${at}: ${texts[i]}`).toBe(x)
            expect(x - end, `${at}: ${texts[i]} stands clear of the label before it`).toBeGreaterThanOrEqual(ROOM)
            end = labels[j].end
            j++
          } else {
            // Left off: only where it would have stood within that room.
            expect(x - end, `${at}: ${texts[i]} was left off with room for it`).toBeLessThan(ROOM)
            leftOff++
          }
        }
        expect(j, `${at}: every label drawn is its tick's`).toBe(labels.length)
        // At a desktop cockpit's widths every tick keeps its label: those scales are drawn as they always were.
        if (width >= 800) expect(labels.length, at).toBe(ticks.length)
        cleanup()
      }
    }
    expect(leftOff, 'control: some width here is too narrow for every label').toBeGreaterThan(0)
  }, 60_000)
})
