// The frequency scale every waterfall and scope draws (`freqScale.ts`): where its ticks fall at each
// span the pickers offer, what they read in each view's convention, and how the strip lays its labels.
//
// The operator's request: the FT waterfall reads 500, 1000 … 3000 along its foot, and the other modes'
// waterfalls and scopes should say as plainly how wide the view is. The spans are the pickers' own
// (`WATERFALL_ZOOMS`, `RTTY_WATERFALL_SPANS`, the CW scope's Filter/Std/Full), read through the same
// functions the views use (`zoomRange`, `cwAudioView`), so a picker change moves these with it.
import { describe, expect, it } from 'vitest'
import {
  drawScaleStrip,
  rfOffsetLabel,
  scaleStepHz,
  scaleStripHeight,
  scaleTicks,
  scopeScaleTicks,
  SCALE_MIN_GAP_PX,
} from './freqScale'
import { cwAudioView, RTTY_WATERFALL_SPANS, WATERFALL_ZOOMS, zoomRange } from './waterfall'
import type { AxisKind } from './spectrum/markers'

/** A 1000 CSS px waterfall at text scale 1: the FT waterfall's width in the recorded fixtures. */
const W = 1000
const gap = SCALE_MIN_GAP_PX

describe('the step follows the span the picker shows', () => {
  const stepAt = (value: number) => {
    const { lo, hi } = zoomRange(1500, value)
    return scaleStepHz(hi - lo, W, gap)
  }
  it('Std and Full keep the FT waterfall’s 500 Hz', () => {
    expect(stepAt(0)).toBe(500)
    expect(stepAt(-1)).toBe(500)
    const { lo, hi } = zoomRange(1500, 0)
    expect(scaleTicks(lo, hi, W, gap)).toEqual([500, 1000, 1500, 2000, 2500, 3000])
  })
  it('a zoomed view gets finer steps: 200 Hz at 2 and 1.5 kHz, 100 Hz at 1 kHz and 600 Hz', () => {
    expect(WATERFALL_ZOOMS.map((z) => [z.label, stepAt(z.value)])).toEqual([
      ['Std', 500],
      ['Full', 500],
      ['2 kHz', 200],
      ['1.5 kHz', 200],
      ['1 kHz', 100],
      ['600 Hz', 100],
    ])
    expect(scaleTicks(1000, 2000, W, gap)).toEqual([1000, 1100, 1200, 1300, 1400, 1500, 1600, 1700, 1800, 1900, 2000])
  })
  it('RTTY’s span buttons follow the same rule', () => {
    expect(RTTY_WATERFALL_SPANS.map((s) => [s.label, stepAt(s.value)])).toEqual(
      RTTY_WATERFALL_SPANS.map((s) => [s.label, s.widthHz <= 1000 ? 100 : s.widthHz <= 2000 ? 200 : 500]),
    )
  })
  it('the CW scope: 100 Hz on Filter (centred on the pitch), 500 Hz on Std and Full', () => {
    const f = cwAudioView('filter', 700, 500) // 500 Hz filter + skirt = 625 Hz round 700
    expect(scaleStepHz(f.hiHz - f.loHz, W, gap)).toBe(100)
    expect(scaleTicks(f.loHz, f.hiHz, W, gap)).toEqual([400, 500, 600, 700, 800, 900, 1000])
    for (const span of ['std', 'full'] as const) {
      const v = cwAudioView(span, 700, 500)
      expect(scaleStepHz(v.hiHz - v.loHz, W, gap), span).toBe(500)
    }
  })
})

describe('the step yields to the room, never crowding', () => {
  it('the narrowest CW filter window (300 Hz) reads every 50 Hz', () => {
    const f = cwAudioView('filter', 600, 200)
    expect(f.hiHz - f.loHz).toBe(300)
    expect(scaleStepHz(300, W, gap)).toBe(50)
  })
  it('a narrow pane steps out: never two ticks closer than a label', () => {
    for (const w of [120, 200, 280, 400, 640]) {
      for (const span of [600, 1000, 2000, 2800, 4800]) {
        const t = scaleTicks(200, 200 + span, w, gap)
        for (let i = 1; i < t.length; i++) expect(((t[i] - t[i - 1]) / span) * w, `${w} px, ${span} Hz`).toBeGreaterThanOrEqual(gap)
      }
    }
    expect(scaleStepHz(2800, 200, gap)).toBe(1000) // the old narrow rule's step, now from the room
  })
  it('a pane too narrow for two labels keeps one number to read', () => {
    expect(scaleTicks(200, 3000, 60, gap)).toEqual([2000])
    expect(scaleTicks(200, 3000, 1, gap)).toEqual([2000])
  })
  it('a 175 % text scale steps out the same way, with the labels it enlarges', () => {
    expect(scaleStepHz(1000, 600, gap)).toBe(100)
    expect(scaleStepHz(1000, 600, gap * 1.75)).toBe(200)
  })
  it('never more than twelve ticks (both ends), and nothing for a degenerate view', () => {
    for (const span of [50, 600, 1000, 2000, 2800, 4800, 5625, 200_000]) expect(scaleTicks(0, span, 5000, gap).length).toBeLessThanOrEqual(12)
    expect(scaleTicks(1000, 1000, W, gap)).toEqual([])
    expect(scaleTicks(2000, 1000, W, gap)).toEqual([])
    expect(scaleTicks(0, Number.NaN, W, gap)).toEqual([])
    expect(scaleTicks(0, 3000, 0, gap)).toEqual([])
  })
  it('the strip is the FT waterfall’s: 18 px, 14 px on a short strip, following the text size', () => {
    expect(scaleStripHeight(200, 1)).toBe(18)
    expect(scaleStripHeight(150, 1)).toBe(14)
    expect(scaleStripHeight(200, 1.75)).toBe(32)
  })
})

describe('what a tick reads follows the view', () => {
  const axis = (over: Partial<AxisKind>): AxisKind => ({
    rf: false, carrierCentered: false, sideband: 'USB', dialHz: 14_200_000, pitchHz: 600, cwPitchRefDial: true, ...over,
  })
  it('Phone on USB: audio hertz from the dial up, nothing in the guard band below it', () => {
    // Full on Phone: the 5 kHz audio width carrier-centred, [−W/8, +W] (`scopeView`).
    const t = scopeScaleTicks(axis({ carrierCentered: true }), -625, 5000, W, gap)
    expect(t.map((x) => x.label)).toEqual(['0', '500', '1000', '1500', '2000', '2500', '3000', '3500', '4000', '4500', '5000'])
    expect(t[0].at).toBe(0) // the dial line
  })
  it('Phone on LSB: the same audio hertz, rising to the left', () => {
    const t = scopeScaleTicks(axis({ carrierCentered: true, sideband: 'LSB' }), -5000, 625, W, gap)
    expect(t.map((x) => [x.at, x.label])).toEqual([
      [-5000, '5000'], [-4500, '4500'], [-4000, '4000'], [-3500, '3500'], [-3000, '3000'], [-2500, '2500'],
      [-2000, '2000'], [-1500, '1500'], [-1000, '1000'], [-500, '500'], [0, '0'],
    ])
  })
  it('the CW window: audio hertz, the unit its pitch is set in', () => {
    const t = scopeScaleTicks(axis({ sideband: 'CW' }), 388, 1013, W, gap)
    expect(t.map((x) => x.label)).toEqual(['400', '500', '600', '700', '800', '900', '1000'])
  })
  it('an RF panadapter: the offset from the dial, as a radio’s scope reads', () => {
    const t = scopeScaleTicks(axis({ rf: true, dialHz: 14_074_000 }), 14_064_000, 14_084_000, W, gap)
    expect(t.map((x) => x.label)).toEqual(['-10k', '-8k', '-6k', '-4k', '-2k', '0', '+2k', '+4k', '+6k', '+8k', '+10k'])
    expect(t[5].at).toBe(14_074_000)
    // A dial off the round kilohertz keeps the ticks on round OFFSETS: 0 stands on the dial itself.
    const odd = scopeScaleTicks(axis({ rf: true, dialHz: 14_074_330 }), 14_069_330, 14_079_330, W, gap)
    expect(odd.find((x) => x.label === '0')?.at).toBe(14_074_330)
  })
  it('an RF panadapter with the dial unknown reads nothing rather than inventing an offset', () => {
    expect(scopeScaleTicks(axis({ rf: true, dialHz: null }), 14_064_000, 14_084_000, W, gap)).toEqual([])
    expect(scopeScaleTicks(axis({ rf: true, dialHz: 0 }), 14_064_000, 14_084_000, W, gap)).toEqual([])
  })
  it('offset labels', () => {
    expect([0, 500, -500, 1000, -2500, 12_500, 100_000, -0.2].map(rfOffsetLabel)).toEqual(['0', '+500', '-500', '+1k', '-2.5k', '+12.5k', '+100k', '0'])
  })
})

describe('the strip lays its labels', () => {
  /** A recording stand-in: each label drawn and where, a 6 px-a-character measure. */
  function rec() {
    const texts: [string, number, number][] = []
    const rects: number[][] = []
    const ctx = {
      globalAlpha: 1, fillStyle: '', font: '', textAlign: '', textBaseline: '',
      fillRect: (...a: number[]) => rects.push(a),
      fillText: (s: string, x: number, y: number) => texts.push([s, x, y]),
      measureText: (s: string) => ({ width: s.length * 6 }),
    }
    return { ctx: ctx as unknown as CanvasRenderingContext2D, texts, rects }
  }
  const base = { w: 400, top: 182, h: 18, ground: '#000', ink: '#fff', fontPx: 10, gfx: 1, text: 1 }
  it('a label right of its tick, the last held inside the edge, every tick drawn', () => {
    const r = rec()
    drawScaleStrip(r.ctx, { ...base, marks: [{ x: 100, label: '500' }, { x: 400, label: '3000' }] })
    expect(r.texts).toEqual([['500', 102, 191], ['3000', 374, 191]])
    expect(r.rects).toEqual([[0, 182, 400, 18], [100, 182, 1, 4], [400, 182, 1, 4]])
  })
  it('a label that would run into the one before it is left off; its tick stays', () => {
    const r = rec()
    drawScaleStrip(r.ctx, { ...base, marks: [{ x: 360, label: '+12.5k' }, { x: 380, label: '+15k' }] })
    expect(r.texts.map((t) => t[0])).toEqual(['+12.5k'])
    expect(r.rects.length).toBe(3)
  })
  it('a label wider than the edge room is held inside by its own width', () => {
    const r = rec()
    drawScaleStrip(r.ctx, { ...base, marks: [{ x: 400, label: '+100.25k' }] })
    expect(r.texts[0][1]).toBe(400 - (8 * 6 + 2))
  })
})
