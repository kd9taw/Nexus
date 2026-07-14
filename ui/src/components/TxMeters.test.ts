import { describe, it, expect } from 'vitest'
import { swrBar, alcBar, poBar, compBar } from './TxMeters'

describe('TX meter bar mapping + severity zones', () => {
  it('SWR: 1.0 is flat/ok, 1.5 warns, 2.0+ is hot', () => {
    expect(swrBar(1.0)).toMatchObject({ frac: 0, zone: 'ok', value: '1.0:1' })
    expect(swrBar(1.5).zone).toBe('warn')
    expect(swrBar(2.0).zone).toBe('hot')
    expect(swrBar(3.0).frac).toBe(1) // top of the bar
    expect(swrBar(6.0).frac).toBe(1) // clamped, not overflowing
  })

  it('ALC: normal action is ok, pegged is hot', () => {
    expect(alcBar(0.5).zone).toBe('ok')
    expect(alcBar(0.85).zone).toBe('warn')
    expect(alcBar(1.0)).toMatchObject({ frac: 1, zone: 'hot', value: '100%' })
  })

  it('Po: watts scaled to a 100 W reference, clamped', () => {
    expect(poBar(50)).toMatchObject({ frac: 0.5, value: '50 W' })
    expect(poBar(120).frac).toBe(1)
    expect(poBar(0).frac).toBe(0)
  })

  it('COMP: dB scaled to ~25 dB full scale; heavy comp warns', () => {
    expect(compBar(0).zone).toBe('ok')
    expect(compBar(20).zone).toBe('warn')
    expect(compBar(12)).toMatchObject({ value: '12 dB' })
  })
})
