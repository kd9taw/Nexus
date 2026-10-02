// @vitest-environment jsdom
//
// GETTING OUT LISTS EVERY RECEIVER (plan piece H8): the box's full list of who is hearing you.
//
// The backend (`propagation::getting_out`) keeps the latest report per receiver over its window
// and sorts them most-distant first; the box used to print the first six and drop the rest. The
// full list is every receiver it returns, in that order, each row the call, where (octant and
// distance), the band, the SNR they decoded you at and how long ago — scrolling inside the box.
// Rendered through the real registry entry; jsdom does not lay out, so the row's wrapping is
// measured in a real browser.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { cleanup, fireEvent, render } from '@testing-library/react'
import { paneById } from './panes'
import type { PaneContext } from './paneContext'
import type { GettingOut, HeardMe } from '../../types'

const heard = (i: number): HeardMe => ({
  call: `DL${i}AA`,
  grid: 'JN58',
  band: ['20m', '17m', '15m'][i % 3],
  snr: -20 + i,
  bearingDeg: 45,
  km: 16000 - i * 1000,
  octant: 'NE',
  ageSecs: 30 + i * 90,
})

function mount(getout: GettingOut, onSelectCall = vi.fn()) {
  const ctx = { getout, onSelectCall } as unknown as PaneContext
  const view = render(<>{paneById('getout')!.expert(ctx)}</>)
  return { ...view, onSelectCall }
}

afterEach(cleanup)

describe('the Getting Out box lists everyone who hears you', () => {
  it('lists every receiver the station reports, most distant first, not only the first six', () => {
    const reports = Array.from({ length: 14 }, (_, i) => heard(i))
    const { container } = mount({ count: 14, maxKm: 16000, reports })
    const calls = [...container.querySelectorAll('.getout-list .go-call')].map((c) => c.textContent)
    expect(calls).toEqual(reports.map((r) => r.call))
  })

  it("says how long ago each one heard you, beside the band and the SNR", () => {
    const { container } = mount({ count: 2, maxKm: 16000, reports: [heard(0), heard(3)] })
    const rows = [...container.querySelectorAll('.getout-list li')]
    expect(rows.map((r) => r.querySelector('.go-age')?.textContent)).toEqual(['30s', '5m'])
    expect(rows.map((r) => r.querySelector('.go-snr')?.textContent)).toEqual(['-20 dB', '-17 dB'])
    expect(rows.map((r) => r.querySelector('.go-band')?.textContent)).toEqual(['20m', '20m'])
  })

  it('a row still selects its station on the map', () => {
    const { container, onSelectCall } = mount({ count: 1, maxKm: 16000, reports: [heard(2)] })
    fireEvent.click(container.querySelector('.getout-list li')!)
    expect(onSelectCall).toHaveBeenCalledWith('DL2AA')
  })
})
