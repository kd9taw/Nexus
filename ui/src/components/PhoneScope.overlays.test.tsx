// @vitest-environment jsdom
//
// THE OVERLAYS THROUGH THE REAL SCOPE: spot tags, the licence-class tint and the FT offsets on their
// own canvas. The draw loop, the pointer handlers and the overlay key all run; each canvas's paint
// target records separately. Asserted BY VALUE (where a tag or an edge sits, which spot a click
// works), with the gestures beside them unchanged: a click away from a tag still snaps, a grabbable
// filter edge keeps the press, and a moved press is still the box. And a spot change redraws the
// overlays alone: the renderer's draws are counted, with a control that a redraw of the picture is
// seen by the same count.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'
import { t } from '../i18n'
import type { SpectrumScene } from '../spectrum'
import type { SpotRow } from '../types'
import type { ScopeSpot } from '../spectrum/overlays'

// A native RF panadapter, 14.150–14.250 MHz in 512 bins.
let seq = 0
const spans = { value: { class: 'general', mode: 'phone', unrestricted: false, spans: [[14.225, 14.35]] as [number, number][] } }
vi.mock('../api', () => ({
  getScopeFrame: vi.fn(async () => ({
    seq: ++seq, tMs: Date.now(), source: 'civ', loHz: 14_150_000, hiHz: 14_250_000,
    scale: { kind: 'dbfs', loDb: -120, hiDb: 0 },
    bins: Array.from({ length: 512 }, (_, i) => (Math.abs(i - 266) <= 1 ? 0.9 : 0.1)),
  })),
  getPrivilegeSpans: vi.fn(async (mode: string) => ({ ...spans.value, mode })),
}))
import { getPrivilegeSpans } from '../api'
// The renderer runs for real; each draw it is asked for is counted.
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

/** Each canvas's paint calls, by its class: the overlays', the marks', and the picture's. */
const painted = new Map<string, string[]>()
const clears = new Map<string, number>()
const of = (cls: string) => painted.get(cls) ?? []
beforeEach(() => {
  seq = 0
  scenes.length = 0
  painted.clear()
  clears.clear()
  spans.value = { class: 'general', mode: 'phone', unrestricted: false, spans: [[14.225, 14.35]] }
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
      clearRect: () => { painted.set(cls, []); clears.set(cls, (clears.get(cls) ?? 0) + 1) },
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

const DIAL = 14_200_000
/** x (px of 800) of an RF frequency on the 14.150–14.250 MHz view. */
const xRf = (hz: number) => ((hz - 14_150_000) / 100_000) * 800
/** The tag lane's middle (px): under the plates' row, 14 px down, a 13 px tag. */
const LANE_Y = 20

const row = (over: Partial<SpotRow> = {}): SpotRow => ({
  call: 'K1ABC', entity: '', zone: 0, band: '20m', freqMhz: 14.2015, mode: 'Phone', spotter: 'W1AW',
  corroborators: [], ageSecs: 60, comment: '', licensed: true, ...over,
})
const tagged = (...rows: SpotRow[]): ScopeSpot[] => rows.map((spot) => ({ spot, ink: null }))

function scope(over: Partial<Parameters<typeof PhoneScope>[0]> = {}) {
  const onTune = vi.fn()
  const onPassband = vi.fn()
  const onSpot = vi.fn()
  const props = {
    transmitting: false, theme: 'dark', active: true, interactive: true, sideband: 'USB', dialHz: DIAL,
    viewLoHz: -50_000, viewHiHz: 50_000, filterWidthHz: 2400, passbandHz: 2400 as number | null,
    onTune, onPassband, onSpot, spots: tagged(row()), ...over,
  }
  const ui = render(<PhoneScope {...props} />)
  const canvas = ui.container.querySelector<HTMLCanvasElement>('canvas.ph-scope-canvas')!
  const rerender = (more: Partial<Parameters<typeof PhoneScope>[0]>) => ui.rerender(<PhoneScope {...props} {...more} />)
  return { ui, canvas, onTune, onPassband, onSpot, rerender }
}
const press = (c: HTMLElement, x: number, y = LANE_Y) => fireEvent.pointerDown(c, { button: 0, clientX: x, clientY: y })
const move = (c: HTMLElement, x: number, y = LANE_Y) => fireEvent.pointerMove(c, { button: 0, clientX: x, clientY: y })
const release = (c: HTMLElement, x: number, y = LANE_Y) => fireEvent.pointerUp(c, { button: 0, clientX: x, clientY: y })

describe('spot tags on the Phone and CW scope', () => {
  it('tags a spot at its frequency, on the overlays canvas and not the marks', async () => {
    scope()
    await draw()
    const x = Math.round(xRf(14_201_500))
    expect(of('overlays')).toContain(`move  ${x} 27`)
    expect(of('overlays').some((p) => p.startsWith('text K1ABC'))).toBe(true)
    expect(of('marks').some((p) => p.includes('K1ABC'))).toBe(false)
  })

  it("a click on a tag works that spot — BandMap's action — and tunes nothing", async () => {
    const s = scope()
    await draw()
    const x = xRf(14_201_500)
    press(s.canvas, x)
    release(s.canvas, x)
    expect(s.onSpot).toHaveBeenCalledTimes(1)
    expect(s.onSpot.mock.calls[0][0]).toMatchObject({ call: 'K1ABC', freqMhz: 14.2015 })
    expect(s.onTune).not.toHaveBeenCalled()
  })

  it('a click away from every tag still snaps, as it always did', async () => {
    const s = scope()
    await draw()
    const x = xRf(14_201_500)
    press(s.canvas, x, 100)
    release(s.canvas, x, 100)
    expect(s.onSpot).not.toHaveBeenCalled()
    expect(s.onTune).toHaveBeenCalledWith(expect.objectContaining({ kind: 'click' }))
  })

  it('a grabbable filter edge keeps the press, over a tag as anywhere', async () => {
    // A spot on the far edge of the 2.4 kHz USB passband.
    const s = scope({ spots: tagged(row({ freqMhz: 14.2024 })) })
    await draw()
    const edgeX = xRf(DIAL + 2400)
    // Pressed and released unmoved on the edge: the scope's own click, never the spot.
    press(s.canvas, edgeX)
    release(s.canvas, edgeX)
    expect(s.onSpot).not.toHaveBeenCalled()
    expect(s.onTune).toHaveBeenCalledWith(expect.objectContaining({ kind: 'click' }))
    // Dragged: the filter width, never the spot.
    press(s.canvas, edgeX)
    move(s.canvas, edgeX + 40)
    release(s.canvas, edgeX + 40)
    expect(s.onPassband).toHaveBeenCalled()
    expect(s.onSpot).not.toHaveBeenCalled()
  })

  it('a press on a tag that moves is the box drag, not a work', async () => {
    const s = scope()
    await draw()
    const x = xRf(14_201_500)
    press(s.canvas, x)
    move(s.canvas, x + 60)
    release(s.canvas, x + 60)
    expect(s.onSpot).not.toHaveBeenCalled()
    expect(s.onTune).toHaveBeenCalledWith(expect.objectContaining({ kind: 'drag' }))
  })

  it('tags are display only where the scope does not tune, on the click-only scope, or with no host action', async () => {
    const x = xRf(14_201_500)
    const off = scope({ interactive: false })
    await draw()
    press(off.canvas, x)
    release(off.canvas, x)
    expect(off.onSpot).not.toHaveBeenCalled()
    cleanup()
    const clickOnly = vi.fn()
    const remote = scope({ onBeginClick: () => clickOnly })
    await draw()
    press(remote.canvas, x)
    release(remote.canvas, x)
    expect(remote.onSpot).not.toHaveBeenCalled()
    cleanup()
    // No host action: the press is the scope's own click, as before the tags.
    const none = scope({ onSpot: undefined })
    await draw()
    press(none.canvas, x)
    release(none.canvas, x)
    expect(none.onTune).toHaveBeenCalledWith(expect.objectContaining({ kind: 'click' }))
  })

  it('says where a tag can be clicked, and only there', async () => {
    const s = scope()
    await draw()
    move(s.canvas, xRf(14_201_500))
    expect(s.canvas.style.cursor).toBe('pointer')
    move(s.canvas, xRf(14_201_500), 100)
    expect(s.canvas.style.cursor).toBe('')
  })

  it('draws nothing in 3D', async () => {
    scope()
    await draw()
    expect(of('overlays').some((p) => p.startsWith('text K1ABC'))).toBe(true)
    fireEvent.click(screen.getByTitle(t('scope.dss.off.title')))
    await draw()
    expect(of('overlays')).toEqual([])
  })
})

describe('the licence-class band edges', () => {
  it("tints, by value, where the station's class may not emit the live section's signal", async () => {
    scope({ privilegeMode: 'phone' })
    await draw()
    expect(getPrivilegeSpans).toHaveBeenCalledWith('phone')
    const edge = xRf(14_225_000)
    expect(of('overlays')).toContain(`fill  0.16 0 0 ${Math.round(edge * 100) / 100} 200`)
    expect(of('overlays')).toContain(`move  ${Math.round(edge)} 0`)
  })

  it('tints nothing for an unrestricted class, and asks nothing with no section', async () => {
    spans.value = { class: 'open', mode: 'phone', unrestricted: true, spans: [] }
    scope({ privilegeMode: 'phone' })
    await draw()
    expect(of('overlays').some((p) => p.includes(' 0.16 '))).toBe(false)
    cleanup()
    vi.mocked(getPrivilegeSpans).mockClear()
    scope()
    await draw()
    expect(getPrivilegeSpans).not.toHaveBeenCalled()
  })
})

describe('a spot change redraws no picture', () => {
  it('draws the overlays again and asks the renderer for nothing; a palette change is seen by the same count', async () => {
    const s = scope()
    await draw()
    const pictureDraws = scenes.length
    const overlayDraws = clears.get('overlays') ?? 0
    expect(pictureDraws).toBeGreaterThan(0)
    // No poll between these: only the spots move.
    for (let i = 0; i < 5; i++) {
      await act(async () => { s.rerender({ spots: tagged(row({ call: `K${i}XYZ`, freqMhz: 14.2 + i / 1000 })) }) })
    }
    expect(scenes.length, 'a spot change asked the renderer to draw').toBe(pictureDraws)
    expect(clears.get('overlays'), 'each spot change drew the overlays again').toBe(overlayDraws + 5)
    expect(of('overlays').some((p) => p.startsWith('text K4XYZ'))).toBe(true)
    // Control: the same count sees a picture redraw.
    await act(async () => { s.rerender({ theme: 'light' }) })
    expect(scenes.length).toBeGreaterThan(pictureDraws)
  })

  it('draws them again while paused, with the picture held', async () => {
    const s = scope()
    await draw()
    fireEvent.click(screen.getByTitle(t('scope.pause.title')))
    await draw()
    const pictureDraws = scenes.length
    await act(async () => { s.rerender({ spots: tagged(row({ call: 'W9NEW' })) }) })
    await draw(300)
    expect(scenes.length).toBe(pictureDraws)
    expect(of('overlays').some((p) => p.startsWith('text W9NEW'))).toBe(true)
  })
})

describe('the FT cockpit on the RF scope', () => {
  it('draws the decodes and the RX/TX offsets at dial ± offset, display only', async () => {
    const ft = { side: 1 as const, rxHz: 1500, txHz: 1700, decodes: [{ call: 'K1ABC', hz: 1200, snr: -10, me: false }] }
    scope({ spots: null, ft, onTune: undefined, onSpot: undefined, onPassband: undefined, interactive: false })
    await draw()
    const o = of('overlays')
    expect(o).toContain(`fill  0.9 ${Math.round((xRf(DIAL + 1500) - 1) * 100) / 100} 0 2 200`)
    expect(o).toContain(`fill  0.7 ${Math.round((xRf(DIAL + 1700) - 1) * 100) / 100} 0 2 200`)
    expect(o).toContain(`move  ${Math.round(xRf(DIAL + 1200))} 27`)
    expect(o.some((p) => p.startsWith('text K1ABC'))).toBe(true)
  })
})
