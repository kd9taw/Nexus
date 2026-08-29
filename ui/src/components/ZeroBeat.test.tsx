// @vitest-environment jsdom
// THE CW ZERO-BEAT INDICATOR — what the operator asked for, and the two ways it could lie.
//
// "a light that comes on when you are zero beat to the CW signal" (operator, 2026-08-28),
// approved as the light PLUS a direction cue: a dark light is the same picture at 80 Hz off
// and at 400 Hz off, and the aid has to guide you IN, not only confirm arrival.
//
// The two failures worth guarding are both honesty failures, not layout ones:
//   1. A confident reading with no signal. A dead band must read "nothing to tune to".
//   2. A light that comes on when it shouldn't, or refuses to when it should — so the
//      tolerance is exercised from both sides, at two filter widths.
//
// ⚠️ jsdom DOES NOT LAY OUT. Nothing here proves the widget fits the scope head; that is a
// browser measurement, reported separately.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, screen } from '@testing-library/react'

vi.mock('../api', () => ({ getMeters: vi.fn() }))
import { getMeters } from '../api'
import { ZeroBeat } from './ZeroBeat'
import { zeroBeatToleranceHz, ZERO_BEAT_RANGE_HZ } from '../waterfall'

const mocked = vi.mocked(getMeters)

async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms)
  })
}

/** Put a measured tone on the meter bus and let one poll land. */
async function reading(cwToneHz: number | null) {
  mocked.mockResolvedValue({ rxLevel: 0.1, smeterDb: null, cwToneHz })
  await advance(200)
}

const readout = () => document.querySelector('.zb-read')!.textContent ?? ''
const lit = () => document.querySelector('.zb')!.classList.contains('on')
const needleLeft = () =>
  parseFloat((document.querySelector('.zb-needle') as HTMLElement | null)?.style.left ?? 'NaN')

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'Date'] })
  mocked.mockReset()
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('the zero-beat light', () => {
  it('lights on the pitch and goes out either side of the tolerance', async () => {
    mocked.mockResolvedValue({ rxLevel: 0, smeterDb: null, cwToneHz: null })
    render(<ZeroBeat targetHz={600} filterHz={500} />)

    const tol = zeroBeatToleranceHz(500) // 25 Hz
    await reading(600)
    expect(lit(), 'exactly on the pitch').toBe(true)
    await reading(600 + tol - 1)
    expect(lit(), 'just inside tolerance, high side').toBe(true)
    await reading(600 - tol + 1)
    expect(lit(), 'just inside tolerance, low side').toBe(true)

    // THE CONTROL for those three: a hair outside must go dark, or "lights" proves nothing.
    await reading(600 + tol + 2)
    expect(lit(), 'just outside tolerance, high side').toBe(false)
    await reading(600 - tol - 2)
    expect(lit(), 'just outside tolerance, low side').toBe(false)
  })

  it('tightens with the rig filter — a 250 Hz filter demands a closer tune than a 2.4 kHz one', async () => {
    expect(zeroBeatToleranceHz(250)).toBeLessThan(zeroBeatToleranceHz(2400))
    mocked.mockResolvedValue({ rxLevel: 0, smeterDb: null, cwToneHz: null })
    render(<ZeroBeat targetHz={600} filterHz={250} />)
    await reading(600 + 30)
    expect(lit(), '30 Hz off is NOT zero beat behind a 250 Hz filter').toBe(false)
    cleanup()

    render(<ZeroBeat targetHz={600} filterHz={2400} />)
    await reading(600 + 30)
    expect(lit(), '…but it is behind a 2.4 kHz one').toBe(true)
  })
})

describe('the direction cue', () => {
  it('reads the signed offset so 80 Hz off does not look like 400 Hz off', async () => {
    mocked.mockResolvedValue({ rxLevel: 0, smeterDb: null, cwToneHz: null })
    render(<ZeroBeat targetHz={600} filterHz={500} />)

    await reading(680)
    const near = needleLeft()
    expect(readout()).toContain('+80')
    await reading(1000)
    const far = needleLeft()
    expect(readout()).toContain('+400')
    expect(far, 'further off must deflect further').toBeGreaterThan(near)
    expect(near, 'and both sit right of centre').toBeGreaterThan(50)

    // The other side of the marker deflects the other way — the needle follows the SCOPE's
    // axis (low audio left), so "centre the needle" always matches the picture beside it.
    await reading(520)
    expect(readout()).toContain('-80')
    expect(needleLeft()).toBeLessThan(50)
  })

  it('pins rather than running off the end when the signal is past the search range', async () => {
    mocked.mockResolvedValue({ rxLevel: 0, smeterDb: null, cwToneHz: null })
    render(<ZeroBeat targetHz={600} filterHz={500} />)
    await reading(600 + ZERO_BEAT_RANGE_HZ * 2)
    const left = needleLeft()
    expect(left).toBeGreaterThan(90)
    expect(left).toBeLessThanOrEqual(100)
  })
})

describe('honesty', () => {
  it('reads "nothing to tune to" on a dead band rather than a confident zero', async () => {
    mocked.mockResolvedValue({ rxLevel: 0, smeterDb: null, cwToneHz: null })
    render(<ZeroBeat targetHz={600} filterHz={500} />)
    await advance(200)
    expect(lit(), 'no signal is NOT zero beat').toBe(false)
    expect(readout()).not.toMatch(/[0-9]/)
    expect(document.querySelector('.zb-needle'), 'and no needle to read').toBeNull()

    // POSITIVE CONTROL: the same widget DOES show a reading when one arrives, so the blank
    // above is a verdict about the band and not a widget that never renders anything.
    await reading(640)
    expect(readout()).toContain('+40')

    // …and back to blank when the signal goes away. A held reading points the operator at a
    // station that is no longer there.
    await reading(null)
    expect(readout()).not.toMatch(/[0-9]/)
    expect(lit()).toBe(false)
  })

  it('costs nothing in a hidden host', async () => {
    mocked.mockResolvedValue({ rxLevel: 0, smeterDb: null, cwToneHz: 640 })
    render(<ZeroBeat targetHz={600} filterHz={500} active={false} />)
    await advance(1000)
    expect(mocked.mock.calls.length).toBe(0)
  })

  it('names itself for a screen reader without narrating at 10 Hz', async () => {
    mocked.mockResolvedValue({ rxLevel: 0, smeterDb: null, cwToneHz: 640 })
    render(<ZeroBeat targetHz={600} filterHz={500} />)
    await advance(200)
    const group = screen.getByRole('group', { name: /zero/i })
    expect(group).toBeTruthy()
    // A live region here would read a new number ten times a second. Deliberately absent.
    expect(group.querySelector('[aria-live]')).toBeNull()
  })
})
