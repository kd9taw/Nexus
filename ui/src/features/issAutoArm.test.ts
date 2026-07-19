import { describe, it, expect, vi, beforeEach } from 'vitest'

// The module only touches '../toast' at runtime (SatPass is a type). Mock it so
// no window/timer shim is needed.
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
import { pushToast } from '../toast'
const toasts = vi.mocked(pushToast)

import { tickIssAutoArm, resetIssAutoArm, type IssRadio } from './issAutoArm'
import type { SatPass } from '../types'

function pass(aosSecs: number, minutes = 10): SatPass {
  return {
    name: 'ISS',
    aosUnix: aosSecs,
    losUnix: aosSecs + minutes * 60,
    maxElDeg: 45,
    aosAzDeg: 200,
    losAzDeg: 40,
  }
}

const OFF_FREQ: IssRadio = { dialMhz: 14.074, band: '20m', sideband: 'USB' }
const ON_CHANNEL: IssRadio = { dialMhz: 145.8, band: '2m', sideband: 'FM' }

function deps() {
  return { setFrequency: vi.fn(), sstvArm: vi.fn() }
}

beforeEach(() => {
  resetIssAutoArm()
  toasts.mockClear()
})

describe('tickIssAutoArm', () => {
  it('arms once at AOS: saves the dial, tunes 145.800 FM, arms SSTV', () => {
    const d = deps()
    const now = 1_900_000_000
    // Rose 1 min ago, still up — an in-progress pass.
    tickIssAutoArm(pass(now - 60), OFF_FREQ, true, d, now)
    expect(d.setFrequency).toHaveBeenCalledTimes(1)
    expect(d.setFrequency).toHaveBeenCalledWith(145.8, '2m', 'FM')
    expect(d.sstvArm).toHaveBeenCalledTimes(1)
    expect(d.sstvArm).toHaveBeenCalledWith(true)
    expect(String(toasts.mock.calls[0][0])).toContain('145.800 FM')
  })

  it('is idempotent within a pass — a second tick does nothing', () => {
    const d = deps()
    const now = 1_900_000_000
    const p = pass(now - 60)
    tickIssAutoArm(p, OFF_FREQ, true, d, now)
    d.setFrequency.mockClear()
    d.sstvArm.mockClear()
    // 30 s later, still in the same pass, rig now on-channel — must NOT re-arm.
    tickIssAutoArm(p, ON_CHANNEL, true, d, now + 30)
    expect(d.setFrequency).not.toHaveBeenCalled()
    expect(d.sstvArm).not.toHaveBeenCalled()
  })

  it('at LOS disarms SSTV and restores the saved dial', () => {
    const d = deps()
    const now = 1_900_000_000
    const p = pass(now - 60)
    tickIssAutoArm(p, OFF_FREQ, true, d, now) // arm; savedDial = OFF_FREQ
    d.setFrequency.mockClear()
    d.sstvArm.mockClear()
    // Pass ended; the rig is still parked on 145.800 FM → restore is safe.
    tickIssAutoArm(p, ON_CHANNEL, true, d, p.losUnix + 60)
    expect(d.sstvArm).toHaveBeenCalledWith(false)
    expect(d.setFrequency).toHaveBeenCalledTimes(1)
    expect(d.setFrequency).toHaveBeenCalledWith(14.074, '20m', 'USB')
  })

  it('does not restore against the operator — a mid-pass manual QSY is left alone', () => {
    const d = deps()
    const now = 1_900_000_000
    const p = pass(now - 60)
    tickIssAutoArm(p, OFF_FREQ, true, d, now) // arm
    d.setFrequency.mockClear()
    d.sstvArm.mockClear()
    // At LOS the operator has since tuned to 40 m — disarm, but DON'T yank them back.
    tickIssAutoArm(p, OFF_FREQ, true, d, p.losUnix + 60)
    expect(d.sstvArm).toHaveBeenCalledWith(false)
    expect(d.setFrequency).not.toHaveBeenCalled()
  })

  it('when disabled, unwinds an arm still in flight (restores the dial)', () => {
    const d = deps()
    const now = 1_900_000_000
    tickIssAutoArm(pass(now - 60), OFF_FREQ, true, d, now) // arm while enabled
    d.setFrequency.mockClear()
    d.sstvArm.mockClear()
    // Opt-in turned off mid-pass; the disabled tick ignores the pass and unwinds.
    tickIssAutoArm(null, ON_CHANNEL, false, d, now + 30)
    expect(d.sstvArm).toHaveBeenCalledWith(false)
    expect(d.setFrequency).toHaveBeenCalledWith(14.074, '20m', 'USB')
  })

  it('a disabled tick with nothing armed is a no-op', () => {
    const d = deps()
    tickIssAutoArm(pass(1_900_000_000), ON_CHANNEL, false, d, 1_900_000_000)
    expect(d.setFrequency).not.toHaveBeenCalled()
    expect(d.sstvArm).not.toHaveBeenCalled()
    expect(toasts).not.toHaveBeenCalled()
  })
})
