// @vitest-environment jsdom
//
// The Space Wx box's 30-day trends (NOAA's daily solar indices): SSN and the SFI/SSN lines.
//
// Every case is a way the block could mislead: a missing day drawn as a dive to zero, an old
// file presented as current, yesterday's sunspot count presented undated, a trend drawn from
// nothing. The file's rows carry their own dates, so the block can always say how old its
// newest day is — and it must.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import type { DailySolarIndex, DailySolarIndices } from '../../types'

const api = vi.hoisted(() => ({
  getSolarIndices: vi.fn((): Promise<DailySolarIndices> => Promise.resolve({ days: [] })),
}))
vi.mock('../../api', () => api)

import { SolarTrends, dayLabel } from './SolarTrends'
import { t } from '../../i18n'

afterEach(() => {
  cleanup()
  api.getSolarIndices.mockReset()
})

const DAY = 86_400
/** 00:00 UTC today, Unix seconds — built from the real clock, as the component reads it. */
const TODAY = Math.floor(Date.now() / 1000 / DAY) * DAY

/** `n` days ending `newestAgo` days before today, oldest first; `at(i)` gives day i's values. */
function file(
  n: number,
  newestAgo: number,
  at: (i: number) => Partial<DailySolarIndex> = () => ({}),
): DailySolarIndices {
  const days: DailySolarIndex[] = []
  for (let i = 0; i < n; i++) {
    const dayUnix = TODAY - (newestAgo + n - 1 - i) * DAY
    days.push({ dayUnix, sfi: 100 + i, ssn: 40 + i, ...at(i) })
  }
  return { days }
}

async function mount(f: DailySolarIndices | Error) {
  api.getSolarIndices.mockImplementation(() => (f instanceof Error ? Promise.reject(f) : Promise.resolve(f)))
  const r = render(<SolarTrends />)
  await waitFor(() => expect(api.getSolarIndices).toHaveBeenCalled())
  return r
}

const row = (c: HTMLElement, index: 'SFI' | 'SSN') => c.querySelector(`.swx-trend[data-index="${index}"]`)

describe('the 30-day trends', () => {
  it("shows the newest sunspot number with that day's date", async () => {
    const f = file(30, 1, (i) => (i === 29 ? { ssn: 46 } : {}))
    const { container } = await mount(f)
    await waitFor(() => expect(row(container, 'SSN')).not.toBeNull())
    const ssn = row(container, 'SSN')!
    expect(ssn.querySelector('.swx-trend-v')?.textContent).toBe('46')
    expect(ssn.querySelector('.swx-trend-d')?.textContent).toBe(dayLabel(f.days[29].dayUnix))
    // And the flux beside it, from the same row of the file.
    expect(row(container, 'SFI')!.querySelector('.swx-trend-v')?.textContent).toBe('129')
  })

  it('draws each index as a line, broken where NOAA has no value — never a dive to zero', async () => {
    const f = file(30, 1, (i) => (i === 10 ? { sfi: null } : {}))
    const { container } = await mount(f)
    await waitFor(() => expect(row(container, 'SFI')).not.toBeNull())
    const runs = (index: 'SFI' | 'SSN') =>
      [...row(container, index)!.querySelectorAll('polyline')].map(
        (p) => (p.getAttribute('points') ?? '').trim().split(/\s+/).length,
      )
    // Days 0–9 and 11–29: two runs, 29 points — the missing day is a gap, not a point at 0.
    expect(runs('SFI')).toEqual([10, 19])
    // Control: the sunspot series, complete, is one unbroken line of all thirty days.
    expect(runs('SSN')).toEqual([30])
  })

  it('a newest day without a value shows the last day that has one, dated as that day', async () => {
    const f = file(30, 1, (i) => (i === 29 ? { ssn: null } : i === 28 ? { ssn: 44 } : {}))
    const { container } = await mount(f)
    await waitFor(() => expect(row(container, 'SSN')).not.toBeNull())
    const ssn = row(container, 'SSN')!
    expect(ssn.querySelector('.swx-trend-v')?.textContent).toBe('44')
    expect(ssn.querySelector('.swx-trend-d')?.textContent).toBe(dayLabel(f.days[28].dayUnix))
  })

  it('says since when once the file has stopped arriving', async () => {
    const stale = file(30, 5)
    const { container } = await mount(stale)
    await waitFor(() => expect(container.querySelector('.swx-trend-head')).not.toBeNull())
    const head = container.querySelector('.swx-trend-head')!
    expect(head.classList.contains('stale')).toBe(true)
    expect(head.textContent).toBe(t('connect.solar.stale', { date: dayLabel(stale.days[29].dayUnix) }))
    cleanup()
    // Control: yesterday's file — the ordinary case — is not called stale.
    const { container: fresh } = await mount(file(30, 1))
    await waitFor(() => expect(fresh.querySelector('.swx-trend-head')).not.toBeNull())
    expect(fresh.querySelector('.swx-trend-head')!.classList.contains('stale')).toBe(false)
  })

  it('says there is no trend when the file never arrived, and draws nothing', async () => {
    const { container } = await mount({ days: [] })
    await waitFor(() => expect(container.querySelector('.swx-trend-none')).not.toBeNull())
    expect(container.querySelector('.swx-trend-none')!.textContent).toBe(t('connect.solar.unavailable'))
    expect(container.querySelector('svg')).toBeNull()
  })

  it('says the same when the fetch itself fails', async () => {
    const { container } = await mount(new Error('no station'))
    await waitFor(() => expect(container.querySelector('.swx-trend-none')).not.toBeNull())
    expect(container.querySelector('svg')).toBeNull()
  })
})
