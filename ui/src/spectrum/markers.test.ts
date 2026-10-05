// The receiver marks' arithmetic, by value: where a passband and a notch sit on each kind of axis
// the scope draws, which edge a drag may move and what width it means, the legal range, and the
// tuning keys' step. Each answer is checked against an independent statement of the same fact
// (the drag box's `boxEdges`, the vendor-independent stepper range), never against itself.
import { describe, expect, it } from 'vitest'
import {
  PASSBAND_LIMITS,
  clampPassband,
  draggableEdges,
  drawPassband,
  edgeNear,
  edgesOnAxis,
  keyStepHz,
  notchOnAxis,
  passbandOnAxis,
  stepPassband,
  widthForEdge,
  type AxisKind,
} from './markers'
import { boxEdges } from '../tuneSnap'

const rf = (sideband: string, extra: Partial<AxisKind> = {}): AxisKind => ({
  rf: true,
  carrierCentered: false,
  sideband,
  dialHz: 14_200_000,
  pitchHz: 600,
  cwPitchRefDial: true,
  ...extra,
})
const audio = (sideband: string, carrierCentered: boolean): AxisKind => ({
  rf: false,
  carrierCentered,
  sideband,
  dialHz: 14_200_000,
  pitchHz: 600,
  cwPitchRefDial: true,
})

describe('the legal range is the steppers’ own', () => {
  it('rounds to the step and stays inside the range; a width that is not one is the floor', () => {
    const p = PASSBAND_LIMITS.phone
    expect([clampPassband(2449, p), clampPassband(2450, p), clampPassband(50, p), clampPassband(9000, p)]).toEqual([
      2400, 2500, 300, 4000,
    ])
    expect(clampPassband(Number.NaN, p)).toBe(300)
    expect(clampPassband(512, PASSBAND_LIMITS.cw)).toBe(500)
    expect(clampPassband(2100, PASSBAND_LIMITS.cw)).toBe(2000)
    // The same figures the ± steppers clamped to before they read this table.
    expect(PASSBAND_LIMITS).toEqual({
      phone: { minHz: 300, maxHz: 4000, stepHz: 100 },
      cw: { minHz: 50, maxHz: 2000, stepHz: 50 },
    })
  })

  it('a key steps one notch, stops at a rail, and never moves the wrong way', () => {
    const p = PASSBAND_LIMITS.phone
    expect(stepPassband(2400, 1, p)).toBe(2500)
    expect(stepPassband(2400, -1, p)).toBe(2300)
    expect(stepPassband(4000, 1, p)).toBe(4000)
    expect(stepPassband(300, -1, p)).toBe(300)
    // A width above the range (another mode's, before the next read) is not "widened" down to it.
    expect(stepPassband(6000, 1, p)).toBe(6000)
    expect(stepPassband(6000, -1, p)).toBe(4000)
  })
})

describe('where a passband sits, on every axis the scope draws', () => {
  it('a native RF row: USB above the dial, LSB below, CW centred — exactly the drag box', () => {
    expect(passbandOnAxis(rf('USB'), 2400)).toEqual({ lo: 14_200_000, hi: 14_202_400, anchor: 14_200_000 })
    expect(passbandOnAxis(rf('LSB'), 2400)).toEqual({ lo: 14_197_600, hi: 14_200_000, anchor: 14_200_000 })
    expect(passbandOnAxis(rf('CW'), 500)).toEqual({ lo: 14_199_750, hi: 14_200_250, anchor: 14_200_000 })
    expect(passbandOnAxis(rf('AM'), 6000)).toEqual({ lo: 14_197_000, hi: 14_203_000, anchor: 14_200_000 })
    for (const sb of ['USB', 'LSB', 'CW', 'CW-L', 'AM', 'FM']) {
      const p = passbandOnAxis(rf(sb), 1800)!
      const box = boxEdges(14_200_000, sb, 1800)
      expect([p.lo, p.hi], sb).toEqual([box.loHz, box.hiHz])
    }
  })

  it('the soundcard CW keyer rides SSB: its passband centres a pitch away from the dial', () => {
    const p = passbandOnAxis(rf('CW-L', { cwPitchRefDial: false }), 500)!
    expect(p.anchor).toBe(14_200_000 - 600)
    expect([p.lo, p.hi]).toEqual([14_199_150, 14_199_650])
  })

  it("Phone's carrier-centred audio axis: the dial is 0, USB runs up from it, LSB down", () => {
    expect(passbandOnAxis(audio('USB', true), 2400)).toEqual({ lo: 0, hi: 2400, anchor: 0 })
    expect(passbandOnAxis(audio('LSB', true), 2400)).toEqual({ lo: -2400, hi: 0, anchor: 0 })
  })

  it("CW's audio window centres on the pitch; SSB audio on a plain axis runs up from 0", () => {
    expect(passbandOnAxis(audio('CW', false), 500)).toEqual({ lo: 350, hi: 850, anchor: 600 })
    expect(passbandOnAxis(audio('CW-L', false), 500)).toEqual({ lo: 350, hi: 850, anchor: 600 })
    expect(passbandOnAxis(audio('LSB', false), 2400)).toEqual({ lo: 0, hi: 2400, anchor: 0 })
  })

  it('no honest place, no passband: a demodulated AM/FM baseband, an RF row with no dial, no width', () => {
    expect(passbandOnAxis(audio('AM', true), 6000)).toBeNull()
    expect(passbandOnAxis(audio('FM', false), 12000)).toBeNull()
    expect(passbandOnAxis(rf('USB', { dialHz: null }), 2400)).toBeNull()
    expect(passbandOnAxis(rf('USB'), 0)).toBeNull()
  })
})

describe('the edges a drag moves, and the width it means', () => {
  it('SSB moves only the far edge — the near one is the carrier; CW, AM and FM move both', () => {
    expect(draggableEdges('USB')).toEqual(['hi'])
    expect(draggableEdges('LSB')).toEqual(['lo'])
    for (const sb of ['CW', 'CW-L', 'CW-R', 'AM', 'FM']) expect(draggableEdges(sb), sb).toEqual(['lo', 'hi'])
    // A plain audio axis draws every SSB passband growing upward.
    expect(edgesOnAxis(audio('LSB', false))).toEqual(['hi'])
    expect(edgesOnAxis(audio('LSB', true))).toEqual(['lo'])
  })

  it('an edge put back where it is reads back the width it came from, on every axis', () => {
    const axes = [rf('USB'), rf('LSB'), rf('CW'), rf('AM'), audio('USB', true), audio('LSB', true), audio('CW', false), audio('USB', false)]
    for (const a of axes) {
      const p = passbandOnAxis(a, 1800)!
      for (const e of edgesOnAxis(a)) {
        expect(widthForEdge(a, p, e, e === 'lo' ? p.lo : p.hi), `${a.sideband} rf=${a.rf} ${e}`).toBeCloseTo(1800, 6)
      }
    }
    // …and moving the far USB edge 300 Hz out is 300 Hz wider; a CW edge 100 Hz out is 200 wider.
    const usb = passbandOnAxis(rf('USB'), 1800)!
    expect(widthForEdge(rf('USB'), usb, 'hi', usb.hi + 300)).toBe(2100)
    const cw = passbandOnAxis(rf('CW'), 500)!
    expect(widthForEdge(rf('CW'), cw, 'lo', cw.lo - 100)).toBe(700)
  })

  it('a press grabs the nearest grabbable edge inside the tolerance, and nothing else', () => {
    expect(edgeNear(103, 100, 300, ['lo', 'hi'], 5)).toBe('lo')
    expect(edgeNear(298, 100, 300, ['lo', 'hi'], 5)).toBe('hi')
    expect(edgeNear(106, 100, 300, ['lo', 'hi'], 5)).toBeNull()
    expect(edgeNear(101, 100, 300, ['hi'], 5), 'the carrier edge is not grabbable on SSB').toBeNull()
    expect(edgeNear(104, 100, 106, ['lo', 'hi'], 5), 'two edges in reach: the nearer').toBe('hi')
  })
})

describe('the manual notch on each axis', () => {
  it('sits at the audio frequency it is set to, carried onto the axis the scope draws', () => {
    expect(notchOnAxis(rf('USB'), 1000)).toBe(14_201_000)
    expect(notchOnAxis(rf('LSB'), 1000)).toBe(14_199_000)
    // True CW: a tone AT the pitch is a signal ON the dial.
    expect(notchOnAxis(rf('CW'), 700)).toBe(14_200_100)
    expect(notchOnAxis(rf('CW-L'), 700)).toBe(14_199_900)
    // The soundcard keyer's rig is in SSB: audio maps as it does on SSB.
    expect(notchOnAxis(rf('CW', { cwPitchRefDial: false }), 700)).toBe(14_200_700)
    expect(notchOnAxis(audio('USB', true), 1000)).toBe(1000)
    expect(notchOnAxis(audio('LSB', true), 1000)).toBe(-1000)
    expect(notchOnAxis(audio('CW', false), 700)).toBe(700)
  })

  it('has no place in AM or FM, with no dial on an RF row, or at no frequency', () => {
    expect(notchOnAxis(rf('AM'), 1000)).toBeNull()
    expect(notchOnAxis(audio('FM', true), 1000)).toBeNull()
    expect(notchOnAxis(rf('USB', { dialHz: null }), 1000)).toBeNull()
    expect(notchOnAxis(rf('USB'), 0)).toBeNull()
  })
})

describe('the tuning keys’ step', () => {
  it('is about a hundredth of the view, on a 1-2-5 ladder, from a CW window to a wide panadapter', () => {
    expect(keyStepHz(800)).toBe(10)
    expect(keyStepHz(625)).toBe(5)
    expect(keyStepHz(2900)).toBe(20)
    expect(keyStepHz(4000)).toBe(50)
    expect(keyStepHz(50_000)).toBe(500)
    expect(keyStepHz(100_000)).toBe(1000)
    expect(keyStepHz(500_000)).toBe(5000)
    expect(keyStepHz(50)).toBe(1)
  })
})

describe('drawing a passband', () => {
  function recorder() {
    const calls: string[] = []
    let stroke = ''
    let fill = ''
    let width = 1
    const ctx = {
      set strokeStyle(v: string) {
        stroke = v
      },
      set fillStyle(v: string) {
        fill = v
      },
      set lineWidth(v: number) {
        width = v
      },
      fillRect: (x: number, y: number, w: number, h: number) => calls.push(`fill ${fill} ${x},${y},${w},${h}`),
      beginPath() {},
      moveTo: (x: number) => calls.push(`line ${stroke} w${width} x${x}`),
      lineTo() {},
      stroke() {},
    }
    return { calls, ctx: ctx as unknown as CanvasRenderingContext2D }
  }

  it('fills between the edges, and marks only the grabbable edge heavier with a tab', () => {
    const { calls, ctx } = recorder()
    drawPassband(ctx, (hz) => hz / 10, 200, { lo: 1000, hi: 3400, anchor: 1000 }, '255, 255, 255', ['hi'], 1)
    expect(calls).toEqual([
      'fill rgba(255, 255, 255, 0.09) 100,0,240,200',
      'line rgba(255, 255, 255, 0.4) w1 x100',
      'line rgba(255, 255, 255, 0.85) w2 x340',
      'fill rgba(255, 255, 255, 0.85) 337,0,6,6',
    ])
  })
})
