// @vitest-environment jsdom
//
// The "coming" chip: NOAA's watch is the headline and the countdown, the model a second line under
// a watch only, a measured jump at L1 the only "arrived" — and nothing at all on a quiet sky.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { act, cleanup, render, screen } from '@testing-library/react'

const api = vi.hoisted(() => ({ getArrivalOutlook: vi.fn() }))
vi.mock('../../api', () => api)

import { ArrivalChip, arrivalChipText, inLabel } from './ArrivalChip'
import { dayLabel } from './SolarTrends'
import { wxStampLabel } from '../../propViz'
import type { ArrivalOutlook, ArrivalModel } from '../../types'

const DAY = 86_400
const T0 = 1_793_404_800 // 2026-10-31 00:00 UTC
// The date words follow the computer's locale (the app's dayLabel/wxStampLabel); the tests read
// them through the same helpers and pin the rest of each sentence.
const OCT31 = dayLabel(T0)
const NOV1 = dayLabel(T0 + DAY)
const at = (secs: number) => wxStampLabel(secs)

const NONE: ArrivalOutlook = { watch: null, model: null, l1: null, rationale: null }
const WATCH = {
  serial: 1134,
  code: 'WATA20',
  issued: T0 - 5 * 3600 + 600, // 30 Oct 19:10Z
  days: [],
  comment: 'CH HSS effects.',
  window: [T0, T0 + 2 * DAY] as [number, number],
  maxG: 1,
}
const MODEL: ArrivalModel = {
  runAt: T0 - 3600,
  fetchedAt: T0 - 1800,
  peak: { at: T0 + 20 * 3600, speed: 517.4, density: 3, cloud: false },
  arrival: null,
}
const JUMP = { at: T0 + 2 * 3600 + 840, arrives: T0 + 3 * 3600 + 600, speedBefore: 400, speedAfter: 520, btBefore: 5, btAfter: 12 }

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('arrivalChipText', () => {
  it('draws nothing on a quiet sky or with no feed', () => {
    expect(arrivalChipText(null, T0)).toBeNull()
    expect(arrivalChipText(NONE, T0)).toBeNull()
    // A model run alone is louder than NOAA's own call, and NOAA has not made one.
    expect(arrivalChipText({ ...NONE, model: MODEL }, T0)).toBeNull()
    // A latest L1 minute with no jump is a reading, not an arrival.
    expect(arrivalChipText({ ...NONE, l1: { latest: { at: T0, arrives: T0 + 3600, speed: 400, bt: 5 }, jump: null } }, T0)).toBeNull()
  })

  it('counts down to the watch window, then says it is in force, then goes away', () => {
    const o = { ...NONE, watch: WATCH }
    const ahead = arrivalChipText(o, T0 - 21 * 3600)!
    expect(ahead.lines).toEqual([`🧲 G1 watch · NOAA expects ${OCT31}–${NOV1} (in 21 h)`])
    const inside = arrivalChipText(o, T0 + 3600)!
    expect(inside.lines[0]).toBe(`🧲 G1 watch in force · NOAA expects it through ${NOV1}`)
    expect(arrivalChipText(o, T0 + 2 * DAY)).toBeNull()
  })

  it('carries the issue time, the serial, the comment and the flare sentence on hover', () => {
    const { title } = arrivalChipText({ ...NONE, watch: WATCH }, T0 - 3600)!
    expect(title).toContain('WATA20')
    expect(title).toContain('1134')
    expect(title).toContain(at(T0 - 5 * 3600 + 600))
    expect(title).toContain('NOAA comment: CH HSS effects.')
    expect(title).toContain('A flare is never "coming"')
  })

  it('adds the model as its own labelled line under a watch, with its run time on hover', () => {
    const t = arrivalChipText({ ...NONE, watch: WATCH, model: MODEL }, T0 - 3600)!
    expect(t.lines[1]).toBe(`Model (WSA-Enlil): wind up to 517 km/s around ${at(T0 + 20 * 3600)}`)
    expect(t.title).toContain(`published ${at(T0 - 3600)}`)
    expect(t.title).toContain('a model, not a measurement')
    const withArrival = arrivalChipText(
      {
        ...NONE,
        watch: WATCH,
        model: { ...MODEL, runAt: null, arrival: { riseAt: T0, start: T0 - 6 * 3600, end: T0 + 8 * 3600, speedFrom: 380, speedTo: 520, cloud: false } },
      },
      T0 - 3600,
    )!
    expect(withArrival.lines[1]).toBe(`Model (WSA-Enlil): arrival between ${at(T0 - 6 * 3600)} and ${at(T0 + 8 * 3600)}, 520 km/s`)
    expect(withArrival.title).toContain('the latest WSA-Enlil run')
  })

  it('says "arrived" only from a measured jump at L1, with or without a watch', () => {
    const alone = arrivalChipText({ ...NONE, l1: { latest: null, jump: JUMP } }, T0 + 3 * 3600)!
    expect(alone.lines).toEqual([`Arrived at L1 ${at(JUMP.at)} · reaching Earth ~${at(JUMP.arrives)} (measured)`])
    const both = arrivalChipText({ ...NONE, watch: WATCH, l1: { latest: null, jump: JUMP } }, T0 + 3 * 3600)!
    expect(both.lines).toHaveLength(2)
    expect(both.lines.filter((l) => l.includes('Arrived'))).toHaveLength(1)
    expect(both.lines[0]).not.toContain('Arrived')
  })
})

describe('the stamps the chip uses', () => {
  it('are UTC, to the minute, with a Z', () => {
    expect(at(JUMP.at)).toMatch(/02:14Z$/)
    expect(at(JUMP.arrives)).toMatch(/03:10Z$/)
  })
})

describe('inLabel', () => {
  it('reads minutes, hours, then days and hours', () => {
    expect(inLabel(40 * 60)).toBe('in 40 min')
    expect(inLabel(21 * 3600)).toBe('in 21 h')
    expect(inLabel(51 * 3600)).toBe('in 2 d 3 h')
  })
})

describe('ArrivalChip', () => {
  it('renders the chip from the feed, as a status, with its sources on hover', async () => {
    vi.useFakeTimers({ toFake: ['Date'] })
    vi.setSystemTime((T0 - 21 * 3600) * 1000)
    api.getArrivalOutlook.mockResolvedValueOnce({ ...NONE, watch: WATCH })
    render(<ArrivalChip />)
    await act(async () => {})
    const chip = await screen.findByRole('status')
    expect(chip.textContent).toContain(`NOAA expects ${OCT31}–${NOV1} (in 21 h)`)
    expect(chip.getAttribute('title')).toContain('WATA20')
  })
})

describe('ArrivalChip on a narrow map', () => {
  it('draws the headline only and keeps the other lines on hover', async () => {
    vi.useFakeTimers({ toFake: ['Date'] })
    vi.setSystemTime((T0 - 21 * 3600) * 1000)
    api.getArrivalOutlook.mockResolvedValueOnce({ ...NONE, watch: WATCH, model: MODEL })
    render(<ArrivalChip compact />)
    await act(async () => {})
    const chip = await screen.findByRole('status')
    expect(chip.children).toHaveLength(1)
    expect(chip.classList.contains('compact'), 'the narrow-map rule keys on this class').toBe(true)
    expect(chip.textContent).toContain('G1 watch')
    expect(chip.getAttribute('title')).toContain('Model (WSA-Enlil): wind up to 517 km/s')
  })
})
