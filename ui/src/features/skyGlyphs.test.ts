// THE MOON GLYPH — its phase, read off the drawing calls.
//
// jsdom has no canvas pixels (the real-browser check reads those), so the context here records
// every call. What each assertion pins is something an operator reads off the marker: how much of
// the disc is lit, which side, and that a new moon can still be found.
import { describe, it, expect } from 'vitest'
import { drawMoon } from './skyGlyphs'

type Op = { k: string; a: unknown[]; fill: unknown; stroke: unknown; width: unknown }
function recorder() {
  const ops: Op[] = []
  const store: Record<string, unknown> = {}
  const ctx = new Proxy(store, {
    get: (t, k) =>
      k in t
        ? t[k as string]
        : (...a: unknown[]) => {
            ops.push({ k: String(k), a, fill: t.fillStyle, stroke: t.strokeStyle, width: t.lineWidth })
            return { addColorStop() {} }
          },
  })
  return { ctx: ctx as unknown as CanvasRenderingContext2D, ops }
}

const INKS = { lit: '#e9e6da', dark: '#39404d' }
const HALO = 'rgba(2, 7, 12, 0.9)'
const X = 100
const Y = 60
const R = 10

function draw(illuminated: number, litRight: boolean) {
  const { ctx, ops } = recorder()
  drawMoon(ctx, X, Y, R, illuminated, litRight, INKS, HALO)
  return ops
}

/** The lit part: the terminator it is bounded by and the transform it was drawn under. */
function litPart(ops: Op[]) {
  const kinds = ops.map((o) => o.k)
  const fill = ops.findIndex((o) => o.k === 'fill' && o.fill === INKS.lit)
  if (fill < 0) return null
  const begin = kinds.lastIndexOf('beginPath', fill)
  const save = kinds.lastIndexOf('save', begin)
  const setup = ops.slice(save, begin)
  const path = ops.slice(begin, fill)
  const ellipse = path.find((o) => o.k === 'ellipse')!
  const limb = path.find((o) => o.k === 'arc')!
  return {
    at: setup.find((o) => o.k === 'translate')?.a,
    mirrored: setup.some((o) => o.k === 'scale' && (o.a[0] as number) < 0),
    limb: limb.a,
    halfWidth: ellipse.a[2] as number,
    height: ellipse.a[3] as number,
    // Back up from the bottom to the top anticlockwise = through the lit side: a crescent's bow.
    towardLitLimb: ellipse.a[7] === true,
  }
}

describe('the moon glyph', () => {
  it('fills the whole disc dark first, at the moon, then lights part of it', () => {
    const ops = draw(0.3, true)
    const first = ops.findIndex((o) => o.k === 'fill')
    expect(ops[first].fill, 'the first fill is the dark disc').toBe(INKS.dark)
    const arc = ops.slice(0, first).filter((o) => o.k === 'arc').pop()!
    expect(arc.a.slice(0, 3)).toEqual([X, Y, R])
    const lit = litPart(ops)!
    expect(lit, 'no lit part').not.toBeNull()
    expect(lit.at, 'the lit part is drawn at the moon').toEqual([X, Y])
    expect(lit.limb.slice(0, 5), 'bounded by the limb, top to bottom through the right').toEqual([0, 0, R, -Math.PI / 2, Math.PI / 2])
  })

  it('a crescent: the terminator is r·|2k − 1| wide and bows toward the lit limb', () => {
    const lit = litPart(draw(0.2, true))!
    expect(lit.halfWidth).toBeCloseTo(R * 0.6, 9)
    expect(lit.height).toBe(R)
    expect(lit.towardLitLimb).toBe(true)
  })

  it('a gibbous moon: the same width, bowing away, so more than half is lit', () => {
    const lit = litPart(draw(0.8, true))!
    expect(lit.halfWidth).toBeCloseTo(R * 0.6, 9)
    expect(lit.towardLitLimb).toBe(false)
  })

  it('a quarter moon is lit to a straight line', () => {
    expect(litPart(draw(0.5, true))!.halfWidth).toBeCloseTo(0, 9)
  })

  it('a full moon is lit to the far rim', () => {
    const lit = litPart(draw(1, true))!
    expect(lit.halfWidth).toBeCloseTo(R, 9)
    expect(lit.towardLitLimb).toBe(false)
  })

  it('lit on the right as asked, and mirrored to the left', () => {
    expect(litPart(draw(0.3, true))!.mirrored).toBe(false)
    expect(litPart(draw(0.3, false))!.mirrored).toBe(true)
  })

  it('a new moon has no lit part, and its rim still shows where it is', () => {
    const ops = draw(0, true)
    expect(litPart(ops)).toBeNull()
    expect(ops.filter((o) => o.k === 'stroke').length, 'no rim: a new moon would vanish on a dark sea').toBeGreaterThan(0)
  })

  it('the rim’s dark halo sits wholly OUTSIDE the disc, so it never covers a thin crescent', () => {
    const ops = draw(0.05, true)
    const halo = ops.findIndex((o) => o.k === 'stroke' && o.stroke === HALO)
    expect(halo, 'no halo round the moon').toBeGreaterThan(-1)
    const begin = ops.map((o) => o.k).lastIndexOf('beginPath', halo)
    const ring = ops.slice(begin, halo).find((o) => o.k === 'arc')!
    const inner = (ring.a[2] as number) - (ops[halo].width as number) / 2
    expect(inner, 'the halo’s inner edge reaches into the disc').toBeGreaterThanOrEqual(R)
  })
})
