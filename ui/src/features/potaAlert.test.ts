import { describe, it, expect, vi, beforeEach } from 'vitest'
import { newlySpottedRefs, processPotaAlert, resetPotaAlertsForTest } from './potaAlert'
import { doubleBeep } from '../alerts'
import type { OtaMapSpot } from '../types'

vi.mock('../alerts', () => ({ doubleBeep: vi.fn() }))

const beeps = vi.mocked(doubleBeep)

function spot(reference: string): OtaMapSpot {
  return {
    program: 'POTA',
    reference,
    name: 'Test Park',
    activator: 'W1AW',
    freqMhz: 14.074,
    mode: 'FT8',
    lat: 40,
    lon: -75,
    approx: false,
    ageSecs: 30,
    newRef: false,
  }
}

beforeEach(() => {
  resetPotaAlertsForTest()
  beeps.mockClear()
})

describe('newlySpottedRefs (pure set-diff on reference)', () => {
  it('detects newly-spotted references and does not re-fire for known ones', () => {
    const r1 = newlySpottedRefs(new Set(), [spot('US-1'), spot('US-2')])
    expect(r1.fresh.sort()).toEqual(['US-1', 'US-2'])
    const r2 = newlySpottedRefs(r1.next, [spot('US-1'), spot('US-2'), spot('US-3')])
    expect(r2.fresh).toEqual(['US-3']) // US-1/US-2 already known → silent
  })

  it('a reference that drops off the feed is not carried forward', () => {
    const r1 = newlySpottedRefs(new Set(['US-1']), [spot('US-2')])
    expect(r1.fresh).toEqual(['US-2'])
    expect(r1.next.has('US-1')).toBe(false)
  })
})

describe('processPotaAlert (cold-start-silent, rate-limited beep)', () => {
  it('primes silently on the first tick — 200 already-running parks beep zero times', () => {
    // The cold-start case this alert exists to get right: on the first poll `seen` is empty,
    // so every park already on the air would read as "fresh" without priming — a beep-burst
    // for activations that were running long before the operator turned this on.
    const spots = Array.from({ length: 200 }, (_, i) => spot(`US-${i}`))
    processPotaAlert(spots)
    expect(beeps).not.toHaveBeenCalled()
  })

  it('beeps once a genuinely new reference appears on a later tick', () => {
    processPotaAlert([spot('US-1'), spot('US-2')]) // primes
    processPotaAlert([spot('US-1'), spot('US-2'), spot('US-3')])
    expect(beeps).toHaveBeenCalledTimes(1)
    expect(beeps).toHaveBeenCalledWith(880)
  })

  it('does not re-fire once a reference is known', () => {
    processPotaAlert([spot('US-1')]) // primes
    processPotaAlert([spot('US-1')])
    expect(beeps).not.toHaveBeenCalled()
  })

  it('rate-limits to one beep per tick regardless of how many references are new', () => {
    processPotaAlert([]) // primes
    processPotaAlert([spot('US-1'), spot('US-2'), spot('US-3')])
    expect(beeps).toHaveBeenCalledTimes(1)
  })
})
