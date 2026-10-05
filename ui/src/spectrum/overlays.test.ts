// THE SCOPE'S OVERLAYS, BY VALUE: where an RF frequency lands on each axis the scope draws, BandMap's
// fade and collision rules laid along a horizontal axis, the licence-class tint from the gate's spans,
// and the FT decodes and offsets at dial ± offset. The drawing is read back from a recording context.
import { describe, expect, it } from 'vitest'
import type { DecodeRow, NeedTag, SpotRow } from '../types'
import type { AxisKind } from './markers'
import {
  axisToRf,
  dataSide,
  drawOverlays,
  ftOverlay,
  layoutTags,
  outsideSpans,
  rfOfOffset,
  rfOnAxis,
  spotAlpha,
  type OverlayInks,
  type OverlayScene,
} from './overlays'
import { scopeSpots, spotInk } from './scopeSpots'

const DIAL = 14_200_000
const axis = (over: Partial<AxisKind> = {}): AxisKind => ({
  rf: false, carrierCentered: false, sideband: 'USB', dialHz: DIAL, pitchHz: 600, cwPitchRefDial: true, ...over,
})

describe("rfOnAxis: an RF frequency on the axis the scope draws (the receiver markers' axis model)", () => {
  it('a native RF row is absolute RF, dial or not', () => {
    expect(rfOnAxis(axis({ rf: true }), 14_201_234)).toBe(14_201_234)
    expect(rfOnAxis(axis({ rf: true, dialHz: null }), 14_201_234)).toBe(14_201_234)
  })
  it("Phone's carrier-centred axis is the offset from the dial on either sideband", () => {
    expect(rfOnAxis(axis({ carrierCentered: true }), DIAL + 1500)).toBe(1500)
    expect(rfOnAxis(axis({ carrierCentered: true, sideband: 'LSB', dialHz: 7_150_000 }), 7_148_500)).toBe(-1500)
  })
  it("CW's audio window: true CW hears the dial at the pitch; the soundcard keyer rides SSB", () => {
    const cw = axis({ sideband: 'CW', dialHz: 14_030_000 })
    expect(rfOnAxis(cw, 14_030_000)).toBe(600)
    expect(rfOnAxis(cw, 14_030_200)).toBe(800)
    expect(rfOnAxis({ ...cw, sideband: 'CW-R' }, 14_030_200)).toBe(400)
    expect(rfOnAxis({ ...cw, cwPitchRefDial: false }, 14_030_600)).toBe(600)
    expect(rfOnAxis({ ...cw, cwPitchRefDial: false, sideband: 'CW-L' }, 14_029_400)).toBe(600)
  })
  it('no honest place: an AM/FM baseband, or an audio axis with the dial unknown', () => {
    expect(rfOnAxis(axis({ carrierCentered: true, sideband: 'FM' }), DIAL)).toBeNull()
    expect(rfOnAxis(axis({ sideband: 'AM' }), DIAL)).toBeNull()
    expect(rfOnAxis(axis({ carrierCentered: true, dialHz: null }), DIAL)).toBeNull()
    expect(rfOnAxis(axis({ carrierCentered: true, dialHz: 0 }), DIAL)).toBeNull()
    expect(rfOnAxis(axis({ rf: true }), Number.NaN)).toBeNull()
  })
  it('axisToRf is its inverse on every axis kind', () => {
    const kinds = [
      axis({ rf: true }),
      axis({ carrierCentered: true }),
      axis({ carrierCentered: true, sideband: 'LSB' }),
      axis({ sideband: 'CW' }),
      axis({ sideband: 'CW-L' }),
      axis({ sideband: 'CW', cwPitchRefDial: false }),
      axis({ sideband: 'USB' }),
    ]
    for (const k of kinds) {
      for (const rf of [DIAL - 2500, DIAL, DIAL + 700]) {
        const ax = rfOnAxis(k, rf)
        expect(ax, JSON.stringify(k)).not.toBeNull()
        expect(axisToRf(k, ax!)).toBe(rf)
      }
    }
    expect(axisToRf(axis({ sideband: 'FM', carrierCentered: true }), 0)).toBeNull()
  })
})

const spot = (over: Partial<SpotRow> = {}): SpotRow => ({
  call: 'K1ABC', entity: '', zone: 0, band: '20m', freqMhz: 14.2015, mode: 'Phone', spotter: 'W1AW',
  corroborators: [], ageSecs: 60, comment: '', licensed: true, ...over,
})

describe("spot tags: BandMap's fade, its mark, and its set", () => {
  it("fades as BandMap does: unknown age fresh, then down to 40 % over 30 minutes", () => {
    expect(spotAlpha(-1)).toBe(0.95)
    expect(spotAlpha(0)).toBe(1)
    expect(spotAlpha(900)).toBe(0.5)
    expect(spotAlpha(1800)).toBe(0.4)
    expect(spotAlpha(7200)).toBe(0.4)
  })
  it('takes the need colour, never on a beacon, and the dim POTA colour where no need colours it', () => {
    const needs = new Map<string, NeedTag>([['K1ABC', 'NewEntity'], ['4U1UN', 'NewEntity'], ['N0POT', 'NewBand']])
    const types = new Map<string, 'Pota' | 'Sota' | 'Dxped'>([['K2POT', 'Pota'], ['N0POT', 'Pota']])
    expect(spotInk(spot(), needs, types)).toBe('--need-entity')
    expect(spotInk(spot({ call: '4U1UN', beacon: 'ncdxf' }), needs, types)).toBeNull()
    expect(spotInk(spot({ call: 'K2POT' }), needs, types)).toBe('--pota-dim')
    expect(spotInk(spot({ call: 'N0POT' }), needs, types)).toBe('--need-band')
    expect(spotInk(spot({ call: 'W9XYZ' }), needs, types)).toBeNull()
  })
  it("tags BandMap's set: this scope's mode on this band", () => {
    const rows = [spot(), spot({ call: 'CW1', mode: 'CW' }), spot({ call: 'B40', band: '40m' })]
    expect(scopeSpots(rows, 'Phone', '20m').map((s) => s.spot.call)).toEqual(['K1ABC'])
    expect(scopeSpots(rows, 'CW', '20m').map((s) => s.spot.call)).toEqual(['CW1'])
  })
})

describe("layoutTags: BandMap's collision rules along the axis", () => {
  it('pushes an overlapping label forward; the first stays centred on its tick', () => {
    const { placed, hidden } = layoutTags([{ x: 100, w: 40, rank: 0 }, { x: 110, w: 40, rank: 1 }], 800, 4)
    expect(hidden).toBe(0)
    expect(placed).toEqual([{ i: 0, left: 80 }, { i: 1, left: 124 }])
  })
  it('keeps a label inside the axis, and compresses back from the far end', () => {
    expect(layoutTags([{ x: 790, w: 40, rank: 0 }], 800, 4).placed).toEqual([{ i: 0, left: 760 }])
    expect(layoutTags([{ x: 5, w: 40, rank: 0 }], 800, 4).placed).toEqual([{ i: 0, left: 0 }])
    const three = layoutTags([{ x: 780, w: 40, rank: 0 }, { x: 785, w: 40, rank: 1 }, { x: 790, w: 40, rank: 2 }], 800, 4)
    expect(three.placed).toEqual([{ i: 0, left: 672 }, { i: 1, left: 716 }, { i: 2, left: 760 }])
  })
  it('caps the set to the freshest that fit and counts the rest', () => {
    const r = layoutTags([{ x: 100, w: 300, rank: 3 }, { x: 400, w: 300, rank: 1 }, { x: 700, w: 300, rank: 2 }], 800, 4)
    expect(r.hidden).toBe(1)
    expect(r.placed.map((p) => p.i)).toEqual([1, 2])
  })
  it('stops at the first fresh tag that does not fit: never a staler one in its place', () => {
    const r = layoutTags([{ x: 100, w: 500, rank: 0 }, { x: 300, w: 400, rank: 1 }, { x: 600, w: 100, rank: 2 }], 800, 4)
    expect(r.placed.map((p) => p.i)).toEqual([0])
    expect(r.hidden).toBe(2)
  })
  it('lays out none on an axis too narrow for any', () => {
    expect(layoutTags([{ x: 10, w: 50, rank: 0 }], 40, 4)).toEqual({ placed: [], hidden: 1 })
  })
})

describe('outsideSpans: where the class may not transmit, inside the view', () => {
  it('is the view minus the spans, half-open as the gate is', () => {
    expect(outsideSpans([[14.15, 14.35]], 14_100_000, 14_400_000)).toEqual([[14_100_000, 14_150_000], [14_350_000, 14_400_000]])
    expect(outsideSpans([[14.0, 14.35]], 14_100_000, 14_200_000)).toEqual([])
    expect(outsideSpans([[7.0, 7.3]], 14_100_000, 14_200_000)).toEqual([[14_100_000, 14_200_000]])
    expect(outsideSpans([[14.0, 14.1], [14.15, 14.35]], 14_100_000, 14_200_000)).toEqual([[14_100_000, 14_150_000]])
  })
  it("keeps 60 m's channel gaps", () => {
    const sixty: [number, number][] = [[5.3306, 5.3334], [5.3466, 5.3494], [5.3515, 5.3665]]
    const out = outsideSpans(sixty, 5_330_000, 5_352_000)
    expect(out.map(([a, b]) => [Math.round(a), Math.round(b)])).toEqual([
      [5_330_000, 5_330_600], [5_333_400, 5_346_600], [5_349_400, 5_351_500],
    ])
  })
})

describe('the FT cockpit on the RF scope: decodes and offsets at dial ± offset', () => {
  it("puts a data signal on the gate's side: below the dial only for LSB", () => {
    expect(dataSide('USB')).toBe(1)
    expect(dataSide('pktusb')).toBe(1)
    expect(dataSide('LSB')).toBe(-1)
    expect(dataSide(' PKTLSB ')).toBe(-1)
    expect(dataSide('FM')).toBe(1)
    expect(rfOfOffset(14_074_000, 1500, 1)).toBe(14_075_500)
    expect(rfOfOffset(3_573_000, 1500, -1)).toBe(3_571_500)
  })
  it('takes the FT8 and FT4 decodes with a sender, never our own transmission', () => {
    const row = (over: Partial<DecodeRow>): DecodeRow => ({
      from: 'K1ABC', snr: -10, dtSec: 0.1, freqHz: 1200, message: 'CQ K1ABC FN42', isCq: true,
      directedToMe: false, worked: false, tier: 'FT8', rv: -1, ...over,
    })
    const ft = ftOverlay([
      row({}), row({ from: 'W2DEF', tier: 'FT4', freqHz: 900, snr: 3, directedToMe: true }),
      row({ from: null }), row({ mine: true }), row({ from: 'Q65X', tier: 'Q65' }),
    ], 1500, 1700, 'USB')
    expect(ft).toEqual({
      side: 1, rxHz: 1500, txHz: 1700,
      decodes: [{ call: 'K1ABC', hz: 1200, snr: -10, me: false }, { call: 'W2DEF', hz: 900, snr: 3, me: true }],
    })
  })
})

// ---- The drawing, read back ----
type Call = string
function recorder(): { ctx: CanvasRenderingContext2D; calls: Call[] } {
  const calls: Call[] = []
  let fill = ''
  let stroke = ''
  let alpha = 1
  const r = (n: number) => Math.round(n * 100) / 100
  const ctx = {
    set fillStyle(v: string) { fill = v }, get fillStyle() { return fill },
    set strokeStyle(v: string) { stroke = v }, get strokeStyle() { return stroke },
    set globalAlpha(v: number) { alpha = v }, get globalAlpha() { return alpha },
    font: '', textAlign: 'left', textBaseline: 'top', lineWidth: 1,
    fillRect: (x: number, y: number, w: number, h: number) => calls.push(`fill ${fill} ${r(alpha)} ${r(x)} ${r(y)} ${r(w)} ${r(h)}`),
    fillText: (s: string, x: number, y: number) => calls.push(`text ${fill} ${r(alpha)} ${s} ${r(x)} ${r(y)}`),
    moveTo: (x: number, y: number) => calls.push(`move ${stroke} ${r(alpha)} ${r(x)} ${r(y)}`),
    lineTo: (x: number, y: number) => calls.push(`to ${r(x)} ${r(y)}`),
    measureText: (s: string) => ({ width: s.length * 6 }),
    beginPath() {}, stroke() {}, save() {}, restore() {}, setLineDash() {},
  }
  return { ctx: ctx as unknown as CanvasRenderingContext2D, calls }
}
const INKS: OverlayInks = { ink: 'INK', ground: 'GROUND', blocked: 'BLOCKED', rx: 'RX', tx: 'TX', color: (t) => `C(${t})` }
/** An 800×200 RF scope over 14.100–14.400 MHz. */
const scene = (over: Partial<OverlayScene> = {}): OverlayScene => ({
  w: 800, h: 200, textPx: 1, lineW: 1, lo: 14_100_000, hi: 14_400_000, axis: axis({ rf: true }),
  spans: null, spots: [], ft: null, transmitting: false, ...over,
})
const xRf = (hz: number) => ((hz - 14_100_000) / 300_000) * 800

describe('drawOverlays', () => {
  it("tints where the class may not transmit, edge to edge, and marks each privilege edge", () => {
    const { ctx, calls } = recorder()
    drawOverlays(ctx, scene({ spans: { unrestricted: false, spans: [[14.225, 14.35]] } }), INKS)
    const tint = calls.filter((c) => c.startsWith('fill BLOCKED'))
    expect(tint).toEqual([
      `fill BLOCKED 0.16 0 0 ${Math.round(xRf(14_225_000) * 100) / 100} 200`,
      `fill BLOCKED 0.16 ${Math.round(xRf(14_350_000) * 100) / 100} 0 ${Math.round((800 - xRf(14_350_000)) * 100) / 100} 200`,
    ])
    // The privilege edges, never the view's own edges.
    const edges = calls.filter((c) => c.startsWith('move BLOCKED')).map((c) => Number(c.split(' ')[3]))
    expect(edges).toEqual([Math.round(xRf(14_225_000)), Math.round(xRf(14_350_000))])
  })
  it('tints nothing for an unrestricted class, or with no spans', () => {
    for (const spans of [{ unrestricted: true, spans: [] as [number, number][] }, null]) {
      const { ctx, calls } = recorder()
      drawOverlays(ctx, scene({ spans }), INKS)
      expect(calls.filter((c) => c.includes('BLOCKED'))).toEqual([])
    }
  })
  it('maps the tint through the axis: on the carrier-centred axis, relative to the dial', () => {
    const { ctx, calls } = recorder()
    // General at 14.2252 MHz USB on a −400…+3200 Hz axis: the phone floor is 200 Hz below the dial.
    drawOverlays(ctx, scene({ lo: -400, hi: 3200, axis: axis({ carrierCentered: true, dialHz: 14_225_200 }),
      spans: { unrestricted: false, spans: [[14.225, 14.35]] } }), INKS)
    expect(calls.filter((c) => c.startsWith('fill BLOCKED'))).toEqual([`fill BLOCKED 0.16 0 0 ${Math.round((200 / 3600) * 800 * 100) / 100} 200`])
  })
  it('tags each spot at its true frequency, faded by age, and returns the label as its target', () => {
    const { ctx, calls } = recorder()
    const s = spot({ freqMhz: 14.2015, ageSecs: 900 })
    const out = drawOverlays(ctx, scene({ spots: [{ spot: s, ink: '--need-entity' }] }), INKS)
    const x = xRf(14_201_500)
    expect(out.tags).toBe(1)
    expect(out.hidden).toBe(0)
    // The tick stands at the true frequency, at the spot's fade.
    expect(calls).toContain(`move INK 0.5 ${Math.round(x)} 27`)
    expect(calls.some((c) => c.startsWith('text INK 0.5 K1ABC'))).toBe(true)
    // The need colour on the label's leading edge, as BandMap's border.
    expect(calls.some((c) => c.startsWith('fill C(--need-entity) 0.5'))).toBe(true)
    expect(out.hits).toHaveLength(1)
    expect(out.hits[0].spot).toBe(s)
    expect(out.hits[0].l).toBeLessThanOrEqual(x)
    expect(out.hits[0].r).toBeGreaterThanOrEqual(x)
  })
  it('tags nothing off the view, and says how many it could not fit', () => {
    const { ctx, calls } = recorder()
    const many = Array.from({ length: 30 }, (_, i) => ({ spot: spot({ call: `K${i}ABCDEFGH`, freqMhz: 14.2 + i * 0.0001, ageSecs: i }), ink: null }))
    const off = { spot: spot({ call: 'OFF', freqMhz: 14.5 }), ink: null }
    const out = drawOverlays(ctx, scene({ spots: [...many, off] }), INKS)
    expect(out.tags + out.hidden).toBe(30)
    expect(out.hidden).toBeGreaterThan(0)
    expect(calls.some((c) => c.includes(` +${out.hidden} `))).toBe(true)
    expect(calls.some((c) => c.includes(' OFF '))).toBe(false)
  })
  it('draws no tags where the scope is too short to hold them; the tint still shows', () => {
    const { ctx, calls } = recorder()
    const out = drawOverlays(ctx, scene({ h: 30, spots: [{ spot: spot(), ink: null }], spans: { unrestricted: false, spans: [[14.225, 14.35]] } }), INKS)
    expect(out.tags).toBe(0)
    expect(out.hits).toEqual([])
    expect(calls.some((c) => c.startsWith('fill BLOCKED'))).toBe(true)
  })
  it('draws the RX and TX offsets and the decodes at dial ± offset, on the side the sideband says', () => {
    const ft = { side: 1 as const, rxHz: 1500, txHz: 1700, decodes: [{ call: 'K1ABC', hz: 1200, snr: -10, me: false }] }
    const { ctx, calls } = recorder()
    drawOverlays(ctx, scene({ axis: axis({ rf: true, dialHz: 14_200_000 }), ft }), INKS)
    expect(calls).toContain(`fill RX 0.9 ${Math.round((xRf(14_201_500) - 1) * 100) / 100} 0 2 200`)
    expect(calls).toContain(`fill TX 0.7 ${Math.round((xRf(14_201_700) - 1) * 100) / 100} 0 2 200`)
    expect(calls).toContain(`move INK 1 ${Math.round(xRf(14_201_200))} 27`)
    // LSB: below the dial.
    const lsb = recorder()
    drawOverlays(lsb.ctx, scene({ axis: axis({ rf: true, dialHz: 14_300_000 }), ft: { ...ft, side: -1 } }), INKS)
    expect(lsb.calls).toContain(`fill RX 0.9 ${Math.round((xRf(14_298_500) - 1) * 100) / 100} 0 2 200`)
    // The TX line brightens while transmitting.
    const keyed = recorder()
    drawOverlays(keyed.ctx, scene({ axis: axis({ rf: true, dialHz: 14_200_000 }), ft, transmitting: true }), INKS)
    expect(keyed.calls).toContain(`fill TX 0.95 ${Math.round((xRf(14_201_700) - 1) * 100) / 100} 0 2 200`)
  })
})
