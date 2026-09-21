// @vitest-environment jsdom
//
// THE S-METER. The operator could not find the meters at all: Phone's S reading was a 10-px bar
// buried in the scope header and the TX meters are four rows that stay blank until the first
// over. An arc-and-needle face was tried first and rejected on sight — centred in a wide region
// with nothing beside it, it read as an ornament. This is the replacement, and its claims are
// about READINGS, not geometry: jsdom never lays out, so the arithmetic is asserted on exported
// pure functions and only structure and text on the render.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { SMeter, sFrac, sLabel, S9_FRAC } from './SMeter'
import type { RadioStatus } from '../types'

afterEach(cleanup)

const radio = (over: Record<string, unknown> = {}) => ({ ...over }) as unknown as RadioStatus
const segs = () => [...document.querySelectorAll('.ph-smeter-seg')]
const lit = () => segs().filter((s) => s.classList.contains('lit'))
const value = () => screen.getByTestId('smeter-value').textContent

describe('the reading', () => {
  it('maps dB relative to S9 onto S-units, 6 dB per unit', () => {
    expect(sLabel(-48)).toBe('S1')
    expect(sLabel(-12)).toBe('S7')
    expect(sLabel(0)).toBe('S9')
  })

  it('never rounds an over-S9 reading UP — that would overstate a signal', () => {
    expect(sLabel(0)).toBe('S9')
    expect(sLabel(20)).toBe('S9+20')
    // 19.4 is not S9+20. Rounding to nearest is fine; rounding a signal UP is not.
    expect(sLabel(19.4)).toBe('S9+19')
  })

  it('puts S9 just under half scale, leaving the whole top half to the +dB band', () => {
    expect(S9_FRAC).toBeCloseTo(0.474, 3)
    expect(sFrac(-54)).toBe(0)
    expect(sFrac(60)).toBe(1)
    // Clamped, so a rig reporting nonsense cannot light past the end of the bar.
    expect(sFrac(9999)).toBe(1)
    expect(sFrac(-9999)).toBe(0)
  })
})

describe('the bar', () => {
  it('lights more segments for a stronger signal, and none for the floor', () => {
    const { rerender } = render(<SMeter radio={radio({ smeterDb: -54 })} />)
    const atFloor = lit().length
    rerender(<SMeter radio={radio({ smeterDb: -12 })} />)
    const atS7 = lit().length
    rerender(<SMeter radio={radio({ smeterDb: 40 })} />)
    const atS9p40 = lit().length
    expect(atFloor).toBe(0)
    expect(atS7).toBeGreaterThan(atFloor)
    expect(atS9p40).toBeGreaterThan(atS7)
    // The scale is printed whatever the reading — a rig prints its whole face.
    expect(document.querySelectorAll('.ph-smeter-tick')).toHaveLength(8)
  })

  it('colours the over-S9 segments differently, and only those', () => {
    render(<SMeter radio={radio({ smeterDb: 60 })} />)
    const hot = segs().filter((s) => s.classList.contains('hot'))
    expect(hot.length).toBeGreaterThan(0)
    expect(hot.length, 'the top band is the minority of the scale').toBeLessThan(segs().length)
    // Every hot segment is above S9 and every one below it is not — the split is the reading,
    // not a guess at where to put the red.
    segs().forEach((s, i) => {
      const at = (i + 0.5) / segs().length
      expect(s.classList.contains('hot')).toBe(at > S9_FRAC)
    })
  })

  it('shows a dash and lights nothing when the rig does not report an S-meter', () => {
    // Absent is not S0. "This radio has no CAT S-meter" and "there is no signal" are different
    // facts and only one of them is about the band.
    render(<SMeter radio={radio({})} />)
    expect(value()).toBe('—')
    expect(lit()).toHaveLength(0)
  })

  it('pauses rather than zeroing while keyed', () => {
    // A rig stops reporting receive meters on transmit. A bar collapsing to the floor on every
    // key-down would read as the signal going away, which is a claim about the band.
    const { rerender } = render(<SMeter radio={radio({ smeterDb: -12 })} />)
    expect(value()).toBe('S7')
    for (const keyedBy of [{ transmitting: true }, { rigKeyed: true }, { txBusyReason: 'tune' }]) {
      rerender(<SMeter radio={radio({ smeterDb: -12, ...keyedBy })} />)
      expect(value(), `keyed via ${Object.keys(keyedBy)[0]}`).toBe('—')
      expect(lit(), 'nothing lit while we are not listening').toHaveLength(0)
    }
  })
})
