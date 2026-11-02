// @vitest-environment jsdom
//
// The band-plan chip: an amber note when phone is outside the usual band plan (30 m phone, say).
// Display only — the picker renders it beside the band, and it is shown only while transmit is
// allowed (a locked station already has the 🔒 chip, which says more). CW is never judged here.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { BandPicker } from './BandPicker'
import type { AppSnapshot } from '../types'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getLicensedBandPlan: vi.fn(async () => []),
}))

afterEach(cleanup)

const chip = (radio: Record<string, unknown>, mode = 'phone') => {
  const snap = {
    radio: { dialMhz: 10.12, band: '30m', sideband: 'USB', txAllowed: true, ...radio },
  } as unknown as AppSnapshot
  render(<BandPicker snap={snap} mode={mode} />)
  return document.querySelector('.band-plan-advice')
}

describe('the band picker band-plan chip', () => {
  it('shows on 30 m phone, naming the band', () => {
    const el = chip({})
    expect(el?.textContent).toBe('Outside band plan')
    expect(el?.getAttribute('title')).toContain('30m')
  })

  it('judges the split transmit frequency when there is one', () => {
    expect(chip({ dialMhz: 14.2, band: '20m', txEmissionMhz: 14.07 })).not.toBeNull()
  })

  it('is absent inside the plan, in CW, and while transmit is locked', () => {
    expect(chip({ dialMhz: 14.2, band: '20m' })).toBeNull()
    cleanup()
    expect(chip({}, 'cw')).toBeNull()
    cleanup()
    expect(chip({ txAllowed: false })).toBeNull()
  })
})
