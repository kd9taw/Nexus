import { describe, expect, it } from 'vitest'
import { phoneOutsideBandPlan } from './bandPlanAdvice'

describe('phoneOutsideBandPlan (display only)', () => {
  it('flags phone on 30 m, which no IARU plan gives phone', () => {
    expect(phoneOutsideBandPlan(10.12, 'USB')).toBe('30m')
    expect(phoneOutsideBandPlan(10.14, 'LSB')).toBe('30m')
  })

  it('flags phone below the lowest phone line of every region', () => {
    expect(phoneOutsideBandPlan(7.02, 'LSB')).toBe('40m')
    expect(phoneOutsideBandPlan(14.07, 'USB')).toBe('20m')
    expect(phoneOutsideBandPlan(1.83, 'LSB')).toBe('160m')
    expect(phoneOutsideBandPlan(50.09, 'USB')).toBe('6m')
  })

  it('judges the transmitted passband, not the dial', () => {
    // An LSB carrier just above the 40 m line has its passband below it.
    expect(phoneOutsideBandPlan(7.031, 'LSB')).toBe('40m')
    expect(phoneOutsideBandPlan(7.0328, 'LSB')).toBeNull()
    // AM sends both sides of the carrier.
    expect(phoneOutsideBandPlan(14.101, 'AM')).toBe('20m')
    expect(phoneOutsideBandPlan(14.101, 'USB')).toBeNull()
  })

  it('says nothing where any region’s plan has phone, or off the covered bands', () => {
    // Region 1 phone on 40 m from 7.053, and the usual homes the band pickers park on.
    expect(phoneOutsideBandPlan(7.09, 'LSB')).toBeNull()
    for (const [dial, sb] of [
      [1.8458, 'LSB'], [3.6028, 'LSB'], [7.1278, 'LSB'], [14.15, 'USB'], [18.11, 'USB'],
      [21.2, 'USB'], [24.93, 'USB'], [28.3, 'USB'], [50.1, 'USB'], [144.1, 'USB'],
    ] as const) {
      expect(phoneOutsideBandPlan(dial, sb), `${dial} ${sb}`).toBeNull()
    }
    expect(phoneOutsideBandPlan(5.363, 'USB')).toBeNull() // 60 m: not judged
    expect(phoneOutsideBandPlan(432.1, 'USB')).toBeNull()
    expect(phoneOutsideBandPlan(Number.NaN, 'USB')).toBeNull()
  })
})
