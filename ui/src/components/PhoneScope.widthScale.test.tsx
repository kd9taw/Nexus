// @vitest-environment jsdom
//
// THE PHONE AND CW SCOPES (AND THE RADIO'S PANADAPTER) READ HOW WIDE, ALONG THEIR FOOT, AS THE FT
// WATERFALL DOES.
//
// The operator: the FT waterfall reads 500, 1000 … 3000 under the picture, and the other modes should
// say as plainly how wide the view is. These scopes read only MHz (where a click lands), so a signal's
// width or the span shown could not be read off them. They now draw the FT waterfall's strip
// (`freqScale.ts`) at their foot: audio hertz on an audio feed, the offset from the dial on an RF
// panadapter, with the MHz kept just above it. Read off the real scope's marks canvas, 800 × 200 px
// at text scale 1 and one device px per px: an 18 px strip from y = 182.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'
import type { SpectrumScene } from '../spectrum'

/** Every scene the scope hands its renderer: the real renderer, its `draw` watched. */
const scenes = vi.hoisted(() => [] as SpectrumScene[])
vi.mock('../spectrum', async (importOriginal) => {
  const real = await importOriginal<typeof import('../spectrum')>()
  return {
    ...real,
    createSpectrumRenderer: (...a: Parameters<typeof real.createSpectrumRenderer>) => {
      const r = real.createSpectrumRenderer(...a)
      const draw = r.draw.bind(r)
      r.draw = (scene: SpectrumScene) => {
        scenes.push(scene)
        draw(scene)
      }
      return r
    },
  }
})

vi.setConfig({ testTimeout: 15_000 })

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

const W = 800
const H = 200
const TOP = 182
const GROUND = '#010203'
const INK = '#fefdfc'

type Op = { op: string; args: (number | string)[]; fill: string; stroke: string }
let marks: Op[] = []

beforeEach(() => {
  seq = 0
  served = AUDIO_ROW
  marks = []
  scenes.length = 0
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => setTimeout(() => cb(performance.now()), 16))
  vi.stubGlobal('cancelAnimationFrame', clearTimeout)
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  vi.stubGlobal('ImageData', class {
    data: Uint8ClampedArray; width: number; height: number
    constructor(a: Uint8ClampedArray | number, b: number, c?: number) {
      if (typeof a === 'number') { this.width = a; this.height = b; this.data = new Uint8ClampedArray(a * b * 4) }
      else { this.data = a; this.width = b; this.height = c ?? a.length / 4 / b }
    }
  })
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  const recorderFor = (canvas: HTMLCanvasElement) => {
    const isMarks = canvas.className.includes('ph-scope-canvas') && !canvas.className.includes('overlays')
    let fill = ''
    let stroke = ''
    const log = (op: string, args: (number | string)[]) => { if (isMarks) marks.push({ op, args, fill, stroke }) }
    return {
      set fillStyle(v: string) { fill = v }, get fillStyle() { return fill },
      set strokeStyle(v: string) { stroke = v }, get strokeStyle() { return stroke },
      globalAlpha: 1, lineWidth: 1, font: '', textAlign: 'left', textBaseline: 'alphabetic',
      fillRect: (...a: number[]) => log('fillRect', a),
      moveTo: (...a: number[]) => log('moveTo', a),
      lineTo: (...a: number[]) => log('lineTo', a),
      fillText: (t: string, x: number, y: number) => log('fillText', [t, x, y]),
      clearRect: () => { if (isMarks) marks = [] },
      measureText: (s: string) => ({ width: s.length * 6 }),
      putImageData() {}, beginPath() {}, closePath() {}, fill() {}, stroke() {}, setLineDash() {},
      save() {}, restore() {}, rect() {}, clip() {}, strokeRect() {}, arc() {},
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
    width: W, height: H, right: W, bottom: H, toJSON() { return {} } })
  for (const [k, v] of Object.entries({ '--well-bg': GROUND, '--well-ink': INK })) document.documentElement.style.setProperty(k, v)
  localStorage.clear()
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
  document.documentElement.removeAttribute('style')
})
async function draw(ms = 200) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

type Props = Parameters<typeof PhoneScope>[0]
async function scope(props: Partial<Props>, row = AUDIO_ROW) {
  served = row
  render(<PhoneScope transmitting={false} theme="dark" active interactive onTune={() => {}} {...props} />)
  await draw()
}
/** The strip's labels, in the order drawn: [label, x]. */
const strip = () => marks.filter((o) => o.op === 'fillText' && o.args[2] === TOP + 9).map((o) => [o.args[0], o.args[1]])
const at = (axisHz: number, lo: number, hi: number) => Math.round(((axisHz - lo) / (hi - lo)) * W) + 2

describe('what the strip reads', () => {
  it('Phone (USB, Full): audio hertz from the dial out, 500 Hz apart, nothing in the guard band', async () => {
    await scope({ sideband: 'USB', carrierCentered: true, viewLoHz: 0, viewHiHz: 5000, dialHz: 14_200_000 })
    // The carrier-centred axis: [−5000/8, 5000] (`scopeView`).
    const labels = strip()
    expect(labels.map((l) => l[0])).toEqual(['0', '500', '1000', '1500', '2000', '2500', '3000', '3500', '4000', '4500', '5000'])
    expect(labels[0][1], 'the 0 stands on the dial').toBe(at(0, -625, 5000))
    expect(labels[4][1]).toBe(at(2000, -625, 5000))
  })
  it('Phone on LSB: the same numbers, rising to the left', async () => {
    await scope({ sideband: 'LSB', carrierCentered: true, viewLoHz: 0, viewHiHz: 2700, dialHz: 7_150_000 })
    const labels = strip()
    expect(labels.map((l) => l[0])).toEqual(['2500', '2000', '1500', '1000', '500', '0'])
    expect(labels[1][1]).toBe(at(-2000, -2700, 2700 / 8))
  })
  it('Phone with the dial unknown still reads its width: the audio does not need the dial', async () => {
    await scope({ sideband: 'USB', carrierCentered: true, viewLoHz: 0, viewHiHz: 2700, dialHz: null })
    expect(strip().map((l) => l[0])).toEqual(['0', '500', '1000', '1500', '2000', '2500'])
  })
  it('CW on the Filter span: audio hertz every 100, the pitch on its own number', async () => {
    // `cwScopeWindow(700, 500)`: 388–1013.
    await scope({ sideband: 'CW', viewLoHz: 388, viewHiHz: 1013, markerHz: 700, pitchHz: 700, dialHz: 7_030_000 })
    const labels = strip()
    expect(labels.map((l) => l[0])).toEqual(['400', '500', '600', '700', '800', '900', '1000'])
    expect(labels[3][1]).toBe(at(700, 388, 1013))
  })
  it('CW on Std: 500 Hz, as the FT waterfall’s Std', async () => {
    await scope({ sideband: 'CW', viewLoHz: 200, viewHiHz: 3000, markerHz: 600, pitchHz: 600, dialHz: 7_030_000 })
    expect(strip().map((l) => l[0])).toEqual(['500', '1000', '1500', '2000', '2500', '3000'])
  })
  it('a radio’s RF panadapter: the offset from the dial, as the radio’s own scope prints it', async () => {
    await scope({ sideband: 'USB', viewLoHz: -1e9, viewHiHz: 1e9, dialHz: 14_200_000 }, RF_ROW)
    const labels = strip()
    expect(labels.map((l) => l[0])).toEqual(['-50k', '-40k', '-30k', '-20k', '-10k', '0', '+10k', '+20k', '+30k', '+40k', '+50k'])
    expect(labels[5][1], 'the 0 stands on the dial').toBe(at(14_200_000, 14_150_000, 14_250_000))
  })
  it('an RF panadapter with the dial unknown keeps its strip empty rather than invent an offset', async () => {
    await scope({ sideband: 'USB', viewLoHz: -1e9, viewHiHz: 1e9, dialHz: null }, RF_ROW)
    expect(marks.some((o) => o.op === 'fillRect' && o.args.join() === `0,${TOP},${W},18`), 'the strip').toBe(true)
    expect(strip()).toEqual([])
  })
})

describe('the strip sits under everything, and the MHz above it', () => {
  it('drawn last, in the well’s ground and ink, and every line above stops at it', async () => {
    await scope({ sideband: 'USB', carrierCentered: true, viewLoHz: 0, viewHiHz: 2700, dialHz: 14_200_000, filterWidthHz: 2400, passbandHz: 2400 })
    const ground = marks.findIndex((o) => o.op === 'fillRect' && o.args.join() === `0,${TOP},${W},18`)
    expect(ground, 'the strip is drawn').toBeGreaterThanOrEqual(0)
    expect(marks[ground].fill).toBe(GROUND)
    expect(marks.slice(ground).every((o) => o.op === 'fillRect' || o.op === 'fillText'), 'nothing drawn over the strip').toBe(true)
    for (const o of marks.filter((o) => o.op === 'fillText' && o.args[2] === TOP + 9)) expect(o.fill).toBe(INK)
  })
  it('the MHz scale stands above the strip, every label and tick', async () => {
    await scope({ sideband: 'USB', carrierCentered: true, viewLoHz: 0, viewHiHz: 2700, dialHz: 14_200_000 })
    const mhz = marks.filter((o) => o.op === 'fillText' && /^\d+\.\d{3,4}$/.test(String(o.args[0])))
    expect(mhz.length, 'the MHz scale is still drawn').toBeGreaterThan(0)
    for (const o of mhz) expect(o.args[2] as number, `${o.args[0]}`).toBeLessThanOrEqual(TOP - 2)
    const rules = marks.filter((o) => o.op === 'lineTo' && o.stroke === 'rgba(255, 255, 255, 0.20)')
    expect(rules.length).toBeGreaterThan(0)
    for (const o of rules) expect(o.args[1]).toBe(TOP)
  })
  it('no two strip labels overlap, and the last stays inside the edge', async () => {
    for (const [lo, hi] of [[0, 5000], [0, 2700], [0, 1500], [0, 800]]) {
      cleanup()
      marks = []
      await scope({ sideband: 'USB', carrierCentered: true, viewLoHz: lo, viewHiHz: hi, dialHz: 14_200_000 })
      const labels = strip() as [string, number][]
      expect(labels.length, `${hi}`).toBeGreaterThan(1)
      for (let i = 1; i < labels.length; i++) expect(labels[i][1], `${hi}: ${labels[i][0]}`).toBeGreaterThan(labels[i - 1][1] + labels[i - 1][0].length * 6)
      const last = labels[labels.length - 1]
      expect(last[1] + last[0].length * 6).toBeLessThanOrEqual(W)
    }
  })
})

describe('the picture leaves the strip to the scale', () => {
  // The renderer is told to stop its rows above the strip (`layout.stripH`), so no waterfall row — the
  // newest, when the flow puts it at the bottom — is ever drawn under the numbers.
  it('the scene’s strip is the strip’s own height whenever one is drawn', async () => {
    await scope({ sideband: 'USB', carrierCentered: true, viewLoHz: 0, viewHiHz: 2700, dialHz: 14_200_000 })
    const ground = marks.find((o) => o.op === 'fillRect' && o.args[1] === TOP && o.args[0] === 0)
    expect(ground, 'the strip is drawn').toBeTruthy()
    expect(scenes.length, 'control: the picture was drawn').toBeGreaterThan(0)
    const last = scenes[scenes.length - 1]
    expect(last.mode).toBe('2d')
    expect(last.layout.stripH).toBe(ground!.args[3])
    expect(last.layout.stripH).toBe(H - TOP)
  })
})

describe('the scale’s spacing follows the text size', () => {
  // At a 175–200 % UI zoom the labels are drawn twice the size, so the ticks must stand further apart:
  // the scope hands the text scale to the scale's room rule, not a fixed 48 px.
  it('at text scale 2 Phone’s Full steps out from 500 to 1000 Hz, with the strip and labels twice the size', async () => {
    vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (this: HTMLElement) {
      return this.classList.contains('ph-scope-canvas') ? W / 2 : 0
    })
    await scope({ sideband: 'USB', carrierCentered: true, viewLoHz: 0, viewHiHz: 5000, dialHz: 14_200_000 })
    const top = H - 36
    const labels = marks.filter((o) => o.op === 'fillText' && o.args[2] === top + 18).map((o) => o.args[0])
    expect(labels).toEqual(['0', '1000', '2000', '3000', '4000', '5000'])
  })
  it('control: at text scale 1 the same view keeps 500 Hz', async () => {
    await scope({ sideband: 'USB', carrierCentered: true, viewLoHz: 0, viewHiHz: 5000, dialHz: 14_200_000 })
    expect(strip().length).toBe(11)
  })
})

describe('the strip follows the theme', () => {
  // The app sets `data-theme` on <html> in an effect that runs AFTER the scope has redrawn for the new
  // theme prop, so a strip painted then read the OLD theme's well inks and kept them: the marks are
  // drawn again only when what they are drawn from moves, and nothing moved after the attribute did.
  // Night and a built-in skin retune the same tokens through attributes of their own; in the app their
  // hooks also re-render the scope after the attribute, but the scope answers the attribute itself, so
  // it does not depend on that order.
  for (const [attr, value] of [['data-theme', 'light'], ['data-night', '1'], ['data-skin', 'amber']] as const) {
    it(`repaints in the new well inks once ${attr} changes, even with nothing else moving`, async () => {
      const props = { sideband: 'USB', carrierCentered: true, viewLoHz: 0, viewHiHz: 2700, dialHz: 14_200_000 }
      served = AUDIO_ROW
      const ui = render(<PhoneScope transmitting={false} theme="dark" active interactive onTune={() => {}} {...props} />)
      await draw()
      const inkOf = () => marks.filter((o) => o.op === 'fillText' && o.args[2] === TOP + 9).map((o) => o.fill)
      expect(inkOf()[0]).toBe(INK)
      // The host's prop moves first (the scope redraws, the page still in the old theme's tokens)…
      if (attr === 'data-theme') ui.rerender(<PhoneScope transmitting={false} theme="light" active interactive onTune={() => {}} {...props} />)
      await draw()
      // …then the page's attribute, and with it the token.
      document.documentElement.style.setProperty('--well-ink', '#a0a0a0')
      document.documentElement.setAttribute(attr, value)
      await draw()
      expect(new Set(inkOf())).toEqual(new Set(['#a0a0a0']))
      document.documentElement.removeAttribute(attr)
    })
  }
})
