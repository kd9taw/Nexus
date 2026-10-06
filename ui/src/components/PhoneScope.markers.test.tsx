// @vitest-environment jsdom
//
// THE RECEIVER MARKER, ITS FILTER EDGES AND THE KEYS, through the real scope: the draw loop, the
// pointer handlers and the key handler all run; the paint target only records. Every gesture is
// asserted BY VALUE — the width or the dial it hands the host — and the existing gestures are
// asserted unchanged beside it: a press that does not move is still the click, a press away from
// an edge is still the box, and in the edge scan's band a press away from an edge is still the
// scan's. The Sub's marker is display only: a click on it tunes nothing.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'
import { t } from '../i18n'
import type { SpectrumScene } from '../spectrum'
import { useScopePassband } from '../useScopeTune'
import { PASSBAND_LIMITS } from '../spectrum/markers'

// One row the test can swap: a native RF panadapter (14.150–14.250 MHz, 512 bins, a carrier just
// above 14.2020 MHz) or a soundcard audio row (0–4000 Hz).
const ROW = { rf: true }
let seq = 0
vi.mock('../api', () => ({
  getScopeFrame: vi.fn(async () =>
    ROW.rf
      ? {
          seq: ++seq, tMs: Date.now(), source: 'civ', loHz: 14_150_000, hiHz: 14_250_000,
          scale: { kind: 'dbfs', loDb: -120, hiDb: 0 },
          bins: Array.from({ length: 512 }, (_, i) => (Math.abs(i - 266) <= 1 ? 0.9 : 0.1)),
        }
      : {
          seq: ++seq, tMs: Date.now(), source: 'rx', loHz: 0, hiHz: 4000,
          scale: { kind: 'dbfs', loDb: -120, hiDb: 0 },
          bins: Array.from({ length: 512 }, (_, i) => (Math.abs(i - 200) <= 2 ? 0.9 : 0.1)),
        },
  ),
}))
// The renderer runs for real; its scenes are kept so a scrollback can be read by its offset.
const scenes: SpectrumScene[] = []
vi.mock('../spectrum', async (orig) => {
  const m = await orig<typeof import('../spectrum')>()
  return {
    ...m,
    createSpectrumRenderer: (...args: Parameters<typeof m.createSpectrumRenderer>) => {
      const r = m.createSpectrumRenderer(...args)
      const draw = r.draw.bind(r)
      r.draw = (scene: SpectrumScene) => {
        scenes.push(scene)
        draw(scene)
      }
      return r
    },
  }
})

/** Every overlay paint call, as text: the fills, the line positions and the plates. */
let painted: string[] = []
beforeEach(() => {
  ROW.rf = true
  scenes.length = 0
  painted = []
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
  let fill = ''
  let stroke = ''
  const paint = {
    set fillStyle(v: string) { fill = v },
    get fillStyle() { return fill },
    set strokeStyle(v: string) { stroke = v },
    get strokeStyle() { return stroke },
    fillRect: (x: number, _y: number, w: number) => painted.push(`fill ${fill} ${Math.round(x)} ${Math.round(w)}`),
    moveTo: (x: number) => painted.push(`line ${stroke} ${Math.round(x)}`),
    fillText: (text: string, x: number) => painted.push(`text ${fill} ${text} ${Math.round(x)}`),
    clearRect: () => { painted = [] },
    putImageData() {}, beginPath() {}, closePath() {}, lineTo() {}, fill() {}, stroke() {}, setLineDash() {},
    save() {}, restore() {}, rect() {}, clip() {}, measureText: (s: string) => ({ width: s.length * 6 }),
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

const MAIN_FILL = 'rgba(255, 255, 255, 0.09)'
const GRAB = 'rgba(255, 255, 255, 0.85)'
/** x (px of 800) of an RF frequency on the 14.150–14.250 MHz view. */
const xRf = (hz: number) => ((hz - 14_150_000) / 100_000) * 800
const DIAL = 14_200_000

function scope(over: Partial<Parameters<typeof PhoneScope>[0]> = {}) {
  const onTune = vi.fn()
  const onPassband = vi.fn()
  const props = {
    transmitting: false, theme: 'dark', active: true, interactive: true, sideband: 'USB', dialHz: DIAL,
    viewLoHz: -50_000, viewHiHz: 50_000, filterWidthHz: 2400, passbandHz: 2400 as number | null,
    onTune, onPassband, ...over,
  }
  const ui = render(<PhoneScope {...props} />)
  const canvas = ui.container.querySelector<HTMLCanvasElement>('canvas.ph-scope-canvas')!
  const rerender = (more: Partial<Parameters<typeof PhoneScope>[0]>) => ui.rerender(<PhoneScope {...props} {...more} />)
  return { ui, canvas, onTune, onPassband, rerender }
}
const press = (c: HTMLElement, x: number) => fireEvent.pointerDown(c, { button: 0, clientX: x, clientY: 100 })
const move = (c: HTMLElement, x: number) => fireEvent.pointerMove(c, { button: 0, clientX: x, clientY: 100 })
const release = (c: HTMLElement, x: number) => fireEvent.pointerUp(c, { button: 0, clientX: x, clientY: 100 })

describe('the receiver marker', () => {
  it('draws the REPORTED passband where the drag box sits — and nothing from the fallback width', async () => {
    const { rerender } = scope()
    await draw()
    expect(painted).toContain(`fill ${MAIN_FILL} ${Math.round(xRf(DIAL))} ${Math.round(xRf(DIAL + 2400) - xRf(DIAL))}`)
    expect(painted, 'USB: only the far edge is a handle').toContain(`line ${GRAB} ${Math.round(xRf(DIAL + 2400))}`)
    expect(painted).not.toContain(`line ${GRAB} ${Math.round(xRf(DIAL))}`)
    // No reported width: the drag box still has its 2.4 kHz fallback, the marker draws nothing.
    rerender({ passbandHz: null })
    await draw()
    expect(painted.some((p) => p.startsWith(`fill ${MAIN_FILL}`))).toBe(false)
  })

  it('draws no handle where the host commands no width, or the scope is not interactive', async () => {
    const { rerender } = scope({ onPassband: undefined })
    await draw()
    expect(painted.some((p) => p.startsWith(`fill ${MAIN_FILL}`)), 'the passband still shows').toBe(true)
    expect(painted.some((p) => p.startsWith(`line ${GRAB}`))).toBe(false)
    rerender({ interactive: false })
    await draw()
    expect(painted.some((p) => p.startsWith(`line ${GRAB}`))).toBe(false)
  })

  it('draws the manual notch where the rig puts it, on the sideband it is heard on', async () => {
    const { rerender } = scope({ notchHz: 1000 })
    await draw()
    const notch = (x: number) => painted.some((p) => p.startsWith('fill rgba(255, 96, 96, 0.45)') && Math.abs(Number(p.split(' ')[5]) - x) <= 2)
    expect(notch(xRf(DIAL + 1000))).toBe(true)
    rerender({ notchHz: 1000, sideband: 'LSB' })
    await draw()
    expect(notch(xRf(DIAL - 1000))).toBe(true)
    rerender({ notchHz: 1000, sideband: 'AM' })
    await draw()
    expect(painted.some((p) => p.startsWith('fill rgba(255, 96, 96'))).toBe(false)
  })

  it('draws a second receiver only where a host passes one, in its own colour, on an RF row', async () => {
    const { rerender } = scope()
    await draw()
    expect(painted.some((p) => p.includes(' SUB '))).toBe(false)
    const sub = { dialHz: 14_230_000, sideband: 'USB', widthHz: 2400 }
    rerender({ subReceiver: sub })
    await draw()
    expect(painted).toContain(`text rgba(64, 200, 255, 0.8) SUB ${Math.round(xRf(14_230_000) + 3)}`)
    expect(painted).toContain(`line rgba(64, 200, 255, 0.8) ${Math.round(xRf(14_230_000))}`)
    expect(painted).toContain(`fill rgba(64, 200, 255, 0.09) ${Math.round(xRf(14_230_000))} ${Math.round(xRf(14_232_400) - xRf(14_230_000))}`)
    expect(painted.some((p) => p.startsWith('line rgba(64, 200, 255, 0.85)')), 'never a handle').toBe(false)
  })

  it('hides the Sub outside the span, and on an audio row', async () => {
    const SUB_INK = 'rgba(64, 200, 255'
    // 14.260 MHz is past the 14.150–14.250 view: no plate, and nothing of it on the 800 px canvas.
    const { rerender } = scope({ subReceiver: { dialHz: 14_260_000, sideband: 'USB', widthHz: 2400 } })
    await draw()
    expect(painted.some((p) => p.includes(' SUB '))).toBe(false)
    for (const p of painted.filter((p) => p.includes(SUB_INK))) expect(Number(p.split(' ')[5]), p).toBeGreaterThanOrEqual(800)
    // CONTROL: the same Sub inside the span is drawn.
    rerender({ subReceiver: { dialHz: 14_240_000, sideband: 'USB', widthHz: null } })
    await draw()
    expect(painted).toContain(`line ${SUB_INK}, 0.8) ${Math.round(xRf(14_240_000))}`)
    // An audio row is Main's own receiver audio: no Sub on it, whatever its dial — and no passband of
    // the Sub's laid on Main's audio as if it were Main's.
    cleanup()
    ROW.rf = false
    scope({ subReceiver: { dialHz: DIAL + 1000, sideband: 'USB', widthHz: 2400 }, carrierCentered: true, viewLoHz: 0, viewHiHz: 2400 })
    await draw()
    expect(painted.some((p) => p.includes(SUB_INK))).toBe(false)
  })
})

describe("the Sub's marker is display only", () => {
  const SUB = { dialHz: 14_230_000, sideband: 'USB', widthHz: null }
  const at = (c: HTMLElement, x: number, y: number) => {
    fireEvent.pointerDown(c, { button: 0, clientX: x, clientY: y })
    fireEvent.pointerUp(c, { button: 0, clientX: x, clientY: y })
  }

  it('⛔ a click on its line or its SUB plate tunes nothing; beside it the click snaps, and a drag from it still drags', async () => {
    const { canvas, onTune, onPassband } = scope({ subReceiver: SUB })
    await draw()
    const x = xRf(14_230_000)
    at(canvas, x, 100) // on the line
    at(canvas, x + 4, 150) // inside its grab tolerance, low in the waterfall
    at(canvas, x - 4, 20)
    at(canvas, x + 15, 8) // on the plate ("SUB" from x + 3)
    expect(onTune).not.toHaveBeenCalled()
    expect(onPassband).not.toHaveBeenCalled()
    // CONTROL: 8 px beside the line, under the plate, the click snaps as it does anywhere else.
    at(canvas, x + 8, 100)
    expect(onTune).toHaveBeenCalledOnce()
    expect(onTune.mock.calls[0][0].kind).toBe('click')
    // A press there that MOVES is the box drag, which tunes by the hand's travel, as everywhere.
    onTune.mockClear()
    press(canvas, x)
    move(canvas, x + 40)
    release(canvas, x + 40)
    expect(onTune.mock.calls.map((c) => c[0].kind)).toContain('drag')
  })

  it('⛔ the Remote page: a click on the marker hands the station nothing; beside it, the click', async () => {
    const click = vi.fn()
    const { canvas, onTune } = scope({ subReceiver: SUB, onBeginClick: () => click })
    await draw()
    const x = xRf(14_230_000)
    at(canvas, x, 100)
    at(canvas, x + 15, 8)
    expect(click).not.toHaveBeenCalled()
    at(canvas, x + 8, 100)
    expect(click).toHaveBeenCalledOnce()
    expect(onTune).not.toHaveBeenCalled()
  })

  it('says so under the pointer: no tuning cursor over it', async () => {
    const { canvas } = scope({ subReceiver: SUB })
    await draw()
    move(canvas, xRf(14_230_000))
    expect(canvas.style.cursor).toBe('default')
    move(canvas, xRf(14_230_000) + 8)
    expect(canvas.style.cursor).toBe('')
  })
})

describe('the filter edge, by value', () => {
  it('a dragged far edge commands each new width, clamped, and the release sends the last', async () => {
    const { canvas, onTune, onPassband } = scope()
    await draw()
    press(canvas, xRf(DIAL + 2400) - 2) // inside the 5 px grab, and > 6 px from the first move
    move(canvas, xRf(DIAL + 3400)) // 3.4 kHz
    move(canvas, xRf(DIAL + 2949)) // 2949 → the step's 2900
    move(canvas, 780) // past 4 kHz → the range's top
    release(canvas, 780)
    expect(onPassband.mock.calls).toEqual([[3400], [2900], [4000], [4000]])
    expect(onTune, 'no box drag, no click').not.toHaveBeenCalled()
  })

  it('an edge pressed and released WITHOUT moving is still the click that snaps', async () => {
    const { canvas, onTune, onPassband } = scope()
    await draw()
    const x = xRf(DIAL + 2400)
    press(canvas, x)
    release(canvas, x)
    expect(onPassband).not.toHaveBeenCalled()
    expect(onTune).toHaveBeenCalledOnce()
    expect(onTune.mock.calls[0][0].kind).toBe('click')
  })

  it('CW moves both edges, mirrored: the low edge 100 Hz out is 200 Hz wider', async () => {
    // A ±5 kHz panadapter, so 100 Hz is 8 px — past the click's 6 px wobble.
    const { canvas, onPassband } = scope({ sideband: 'CW', passbandHz: 500, filterWidthHz: 500, cockpit: 'cw', viewLoHz: -5000, viewHiHz: 5000 })
    await draw()
    const x = (hz: number) => ((hz - (DIAL - 5000)) / 10_000) * 800
    press(canvas, x(DIAL - 250))
    move(canvas, x(DIAL - 350))
    expect(onPassband.mock.calls).toEqual([[700]])
  })

  it("LSB's carrier edge is the box's: a press there slides the passband as it always did", async () => {
    const { canvas, onTune, onPassband } = scope({ sideband: 'LSB' })
    await draw()
    press(canvas, xRf(DIAL))
    move(canvas, xRf(DIAL) + 40)
    release(canvas, xRf(DIAL) + 40)
    expect(onPassband).not.toHaveBeenCalled()
    expect(onTune.mock.calls.map((c) => c[0].kind)).toContain('drag')
  })

  it("an edge inside the edge scan's band is grabbed and dragged; a press in the band beside it still scans", async () => {
    // A 6.1 kHz view with a 4 kHz USB passband puts the far edge 13 px from the right border,
    // inside the 36 px band where a held drag scrolls the band.
    const { canvas, onPassband, onTune } = scope({ passbandHz: 4000, viewLoHz: -2000, viewHiHz: 4100 })
    await draw()
    const x = (hz: number) => ((hz - (DIAL - 2000)) / 6100) * 800
    const edgeX = x(DIAL + 4000) // 786.9
    // CONTROL: 8 px outside the edge, still in the band, a held drag scans the band as it always did.
    press(canvas, edgeX + 8)
    move(canvas, edgeX + 16)
    await draw(40) // the scan's own frames
    release(canvas, edgeX + 16)
    expect(onPassband).not.toHaveBeenCalled()
    expect(onTune.mock.calls.map((c) => c[0].kind)).toContain('drag')
    onTune.mockClear()
    // On the edge: the width follows the hand, and nothing tunes.
    press(canvas, edgeX)
    move(canvas, x(DIAL + 3000))
    move(canvas, x(DIAL + 2500))
    release(canvas, x(DIAL + 2500))
    expect(onPassband.mock.calls).toEqual([[3000], [2500], [2500]])
    expect(onTune, 'no scan, no box, no click').not.toHaveBeenCalled()
  })

  it.each([
    // Auto span is the reported width: the axis is [−300, 2400] on USB and the far edge IS the right
    // border; on LSB the axis is mirrored, [−2400, 300], and the far edge is the left border.
    ['USB', 798, (hz: number) => ((hz + 300) / 2700) * 800, 840],
    ['LSB', 2, (hz: number) => ((2400 - hz) / 2700) * 800, -40],
  ] as const)(
    "Phone's Auto span, %s: the far edge on the border is grabbed, dragged in to narrow and out past the border to widen",
    async (sideband, grab, xOfWidth, past) => {
      ROW.rf = false
      const { canvas, onPassband, onTune } = scope({ sideband, carrierCentered: true, viewLoHz: 0, viewHiHz: 2400, passbandHz: 2400 })
      await draw()
      press(canvas, grab) // 2 px in from the border, on the edge
      move(canvas, xOfWidth(2000)) // 2.0 kHz
      // 40 px past the border at 2700 Hz per 800 px: 2400 + 135 → 2535 → the step's 2500. The axis holds
      // under the hand while the edge is dragged, so past the border it runs on along the same scale.
      move(canvas, past)
      release(canvas, past)
      expect(onPassband.mock.calls).toEqual([[2000], [2500], [2500]])
      expect(onTune).not.toHaveBeenCalled()
    },
  )

  it("a grab in the scan band writes through the cockpit's coalescer: one width a flush, the last", async () => {
    ROW.rf = false
    const send = vi.fn(async () => undefined)
    function Host() {
      const onPassband = useScopePassband({ enabled: true, limits: PASSBAND_LIMITS.phone, send })
      return (
        <PhoneScope transmitting={false} theme="dark" active interactive sideband="USB" dialHz={DIAL} carrierCentered
          viewLoHz={0} viewHiHz={2400} filterWidthHz={2400} passbandHz={2400} onTune={vi.fn()} onPassband={onPassband} />
      )
    }
    const ui = render(<Host />)
    const canvas = ui.container.querySelector<HTMLCanvasElement>('canvas.ph-scope-canvas')!
    await draw()
    const x = (hz: number) => ((hz + 300) / 2700) * 800
    press(canvas, 798)
    move(canvas, x(2000))
    move(canvas, x(2200))
    move(canvas, x(1800))
    expect(send, 'nothing before the flush').not.toHaveBeenCalled()
    await draw(120)
    expect(send.mock.calls).toEqual([[1800]])
    move(canvas, 840) // 2500, past the border
    release(canvas, 840)
    await draw(120)
    expect(send.mock.calls).toEqual([[1800], [2500]])
  })

  it('no edge moves for a host that commands no width, a scope that is not interactive, or a click-only one', async () => {
    for (const over of [{ onPassband: undefined }, { interactive: false }, { onBeginClick: () => () => {} }]) {
      cleanup()
      const { canvas, onPassband } = scope(over)
      await draw()
      press(canvas, xRf(DIAL + 2400))
      move(canvas, xRf(DIAL + 3400))
      release(canvas, xRf(DIAL + 3400))
      expect(onPassband, JSON.stringify(Object.keys(over))).not.toHaveBeenCalled()
    }
  })

  it('a transmitter keying mid-drag ends the drag: nothing more is commanded', async () => {
    const { canvas, onPassband, rerender } = scope()
    await draw()
    press(canvas, xRf(DIAL + 2400) - 2)
    move(canvas, xRf(DIAL + 3000))
    rerender({ interactive: false })
    move(canvas, xRf(DIAL + 3500))
    release(canvas, xRf(DIAL + 3500))
    expect(onPassband.mock.calls).toEqual([[3000]])
  })

  it("holds the axis still under the hand while the width it commands resizes the host's view", async () => {
    // CW's audio window is sized from the filter (cwScopeWindow): 625 Hz around the pitch for 500.
    ROW.rf = false
    const view = (w: number) => {
      const span = Math.min(800, Math.max(300, Math.round(w * 1.25)))
      const lo = Math.max(0, Math.round(600 - span / 2))
      return { viewLoHz: lo, viewHiHz: lo + span }
    }
    const onFeed = vi.fn()
    const { canvas, onPassband, rerender } = scope({ sideband: 'CW', cockpit: 'cw', passbandHz: 500, filterWidthHz: 500, onFeed, ...view(500) })
    await draw()
    const xAudio = (hz: number) => ((hz - 288) / 625) * 800
    press(canvas, xAudio(850))
    move(canvas, 760) // 881.75 Hz → 2·281.75 = 563.5 → 550
    // The host takes the width and resizes its window — mid-drag.
    rerender({ passbandHz: 550, filterWidthHz: 550, ...view(550) })
    await draw()
    move(canvas, 761)
    move(canvas, 760)
    expect(onPassband.mock.calls.map((c) => c[0])).toEqual([550])
    release(canvas, 760)
    await draw()
    // CONTROL: released, the axis follows the host's new window.
    expect(onFeed).toHaveBeenLastCalledWith('rx', view(550).viewLoHz, view(550).viewHiHz)
  })
})

describe('a key for every gesture, on the focused scope', () => {
  it('is a focusable instrument the arrows reach, named by its keys', async () => {
    const { canvas } = scope()
    expect(canvas.tabIndex).toBe(0)
    expect(canvas.getAttribute('role')).toBe('application')
    expect(canvas.getAttribute('aria-label')).toBe(t('scope.canvas.keys.aria'))
    expect(screen.getByRole('application', { name: t('scope.canvas.keys.aria') })).toBe(canvas)
  })

  it('← and → tune by a hundredth of the view on its grid, Shift by ten, and a held key walks on', async () => {
    const { canvas, onTune } = scope()
    await draw()
    fireEvent.keyDown(canvas, { key: 'ArrowRight' })
    fireEvent.keyDown(canvas, { key: 'ArrowRight' }) // the dial prop has not moved yet
    fireEvent.keyDown(canvas, { key: 'ArrowLeft' })
    fireEvent.keyDown(canvas, { key: 'ArrowRight', shiftKey: true })
    expect(onTune.mock.calls.map((c) => c[0])).toEqual([
      { dialHz: 14_201_000, kind: 'drag' },
      { dialHz: 14_202_000, kind: 'drag' },
      { dialHz: 14_201_000, kind: 'drag' },
      { dialHz: 14_210_000, kind: 'drag' },
    ])
  })

  it('Enter is the click at the passband centre: the same signal, the same dial', async () => {
    const { canvas, onTune } = scope()
    await draw()
    const centre = xRf(DIAL + 1200)
    press(canvas, centre)
    release(canvas, centre)
    fireEvent.keyDown(canvas, { key: 'Enter' })
    expect(onTune).toHaveBeenCalledTimes(2)
    expect(onTune.mock.calls[1][0]).toEqual(onTune.mock.calls[0][0])
    expect(onTune.mock.calls[1][0].kind).toBe('click')
  })

  it('[ and ] narrow and widen the filter one step of the cockpit, and stop at its rails', async () => {
    const { canvas, onPassband, rerender } = scope()
    await draw()
    fireEvent.keyDown(canvas, { key: ']' })
    fireEvent.keyDown(canvas, { key: ']' }) // walks on from the width just commanded
    fireEvent.keyDown(canvas, { key: '[' })
    expect(onPassband.mock.calls).toEqual([[2500], [2600], [2500]])
    onPassband.mockClear()
    rerender({ passbandHz: 4000 })
    await draw(2500) // the commanded width has expired; the rig's 4 kHz stands
    fireEvent.keyDown(canvas, { key: ']' })
    expect(onPassband).not.toHaveBeenCalled()
  })

  it('↑ and ↓ scroll back while paused — the wheel’s direction — and do nothing live', async () => {
    const { canvas } = scope()
    await draw(1500) // a history to scroll back through
    fireEvent.keyDown(canvas, { key: 'ArrowDown' })
    const live = scenes.length
    fireEvent.click(screen.getByTitle(t('scope.pause.title')))
    await draw(100) // rows keep arriving while paused
    fireEvent.keyDown(canvas, { key: 'ArrowDown' })
    fireEvent.keyDown(canvas, { key: 'ArrowDown' })
    fireEvent.keyDown(canvas, { key: 'ArrowUp' })
    // While paused the offset also advances one per arriving row (the picture is held still), so
    // the keys are read by the step they add: ↓ is three rows back (newest at the top), ↑ three on.
    const [a, b, c] = scenes.slice(live).map((s) => s.offsetRows).slice(-3)
    expect(a).toBeGreaterThan(0)
    expect([b - a, c - b]).toEqual([3, -3])
  })

  it('⛔ never touches Esc or Space: they reach the window unprevented, and nothing is commanded', async () => {
    const { canvas, onTune, onPassband } = scope()
    await draw()
    const seen: string[] = []
    const onWindow = (e: KeyboardEvent) => seen.push(`${e.key}:${e.defaultPrevented}`)
    window.addEventListener('keydown', onWindow)
    fireEvent.keyDown(canvas, { key: 'Escape' })
    fireEvent.keyDown(canvas, { key: ' ', code: 'Space' })
    window.removeEventListener('keydown', onWindow)
    expect(seen).toEqual(['Escape:false', ' :false'])
    expect(onTune).not.toHaveBeenCalled()
    expect(onPassband).not.toHaveBeenCalled()
  })

  it('no key tunes or resizes while the scope is not interactive (TX), and the Remote gets the click only', async () => {
    const { canvas, onTune, onPassband } = scope({ interactive: false })
    await draw()
    for (const key of ['ArrowRight', 'Enter', ']']) fireEvent.keyDown(canvas, { key })
    expect(onTune).not.toHaveBeenCalled()
    expect(onPassband).not.toHaveBeenCalled()
    cleanup()
    const click = vi.fn()
    const remote = scope({ onBeginClick: () => click })
    await draw()
    for (const key of ['ArrowRight', ']', 'Enter']) fireEvent.keyDown(remote.canvas, { key })
    expect(remote.onTune).not.toHaveBeenCalled()
    expect(remote.onPassband).not.toHaveBeenCalled()
    expect(click).toHaveBeenCalledOnce()
  })
})
