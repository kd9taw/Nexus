// @vitest-environment jsdom
// The month grid + chase digest: the parts where getting it wrong is silent.
// A calendar that puts an operation on the wrong day, or a digest that ranks a
// finished expedition above a live one, looks perfectly fine and is useless.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { DxpedMonth } from './DxpedMonth'
import { DxpedDigest } from './DxpedDigest'
import type { CalendarEntry, DxpedWindow } from '../../types'

/** 2026-07-29 00:00 UTC — a Wednesday, so weekday placement is checkable. */
const NOW = Date.UTC(2026, 6, 29, 12, 0, 0) / 1000
const DAY = 86_400

// Without this each render stacks on the last one and the queries below start
// matching the PREVIOUS test's DOM.
afterEach(cleanup)

function entry(over: Partial<CalendarEntry> & { call: string }): CalendarEntry {
  return {
    entity: 'Test Land',
    region: 'OC',
    startUnix: NOW,
    endUnix: NOW + DAY,
    bands: ['20m'],
    modes: ['FT8'],
    octant: 'SW',
    bearingDeg: 240,
    distanceKm: 12000,
    outlook: [],
    best: '',
    ...over,
  }
}

describe('DxpedMonth', () => {
  it('marks exactly one day as today', () => {
    render(<DxpedMonth entries={[entry({ call: 'VK9CM' })]} nowUnix={NOW} />)
    expect(screen.getAllByText('TODAY')).toHaveLength(1)
  })

  it('labels a multi-day operation ONCE, on the day it starts', () => {
    // A 4-day expedition should read as one bar with one callsign, not as the
    // same call shouted on four consecutive days.
    render(
      <DxpedMonth
        entries={[entry({ call: 'VK9CM', startUnix: NOW, endUnix: NOW + 4 * DAY })]}
        nowUnix={NOW}
      />,
    )
    expect(screen.getAllByText('VK9CM')).toHaveLength(1)
  })

  it('places an operation on its own days and nowhere else', () => {
    // Dates are UTC-MIDNIGHT aligned on purpose. An operation running noon-to-noon
    // genuinely touches three calendar days, which is correct but makes a poor
    // assertion — this pins the boundary case instead: a run ending exactly at a
    // day boundary must NOT paint that day.
    const midnight = Date.UTC(2026, 6, 29) / 1000
    const start = midnight + 3 * DAY
    render(
      <DxpedMonth
        entries={[entry({ call: 'T33T', startUnix: start, endUnix: start + 2 * DAY })]}
        nowUnix={NOW}
      />,
    )
    const bars = screen.getAllByTitle(/^T33T/)
    expect(bars).toHaveLength(2)
  })

  it('renders nothing at all when there are no announcements', () => {
    const { container } = render(<DxpedMonth entries={[]} nowUnix={NOW} />)
    expect(container.firstChild).toBeNull()
  })
})

describe('DxpedDigest', () => {
  it('puts an on-the-air operation above one that has not started', () => {
    render(
      <DxpedDigest
        entries={[
          entry({ call: 'LATER', startUnix: NOW + 5 * DAY, endUnix: NOW + 6 * DAY }),
          entry({ call: 'LIVE', startUnix: NOW - DAY, endUnix: NOW + DAY }),
        ]}
        nowUnix={NOW}
      />,
    )
    const calls = screen.getAllByRole('button').map((b) => b.textContent)
    expect(calls[0]).toContain('LIVE')
    expect(screen.getByText('ON THE AIR')).toBeTruthy()
  })

  it('drops operations that have already finished', () => {
    render(
      <DxpedDigest
        entries={[entry({ call: 'GONE', startUnix: NOW - 9 * DAY, endUnix: NOW - 2 * DAY })]}
        nowUnix={NOW}
      />,
    )
    expect(screen.queryByText('GONE')).toBeNull()
  })

  it('names the best day only when the model clears the Fair boundary', () => {
    // 0.3 is the Fair boundary the rest of the prop UI uses. Below it, claiming a
    // "best day" would dress up a bad path as a plan.
    const poor: DxpedWindow = {
      call: 'DUD',
      engine: 'modelled',
      best: '20m Poor',
      outlook: [],
      days: [{ dayUnix: NOW + DAY, best: '20m', score: 0.1 }],
    }
    const good: DxpedWindow = {
      call: 'GEM',
      engine: 'p533',
      best: '17m Good 0230-0430Z',
      outlook: [],
      days: [{ dayUnix: NOW + DAY, best: '17m', score: 0.8 }],
    }
    const windows = new Map([
      ['DUD', poor],
      ['GEM', good],
    ])
    render(
      <DxpedDigest
        entries={[
          entry({ call: 'DUD', startUnix: NOW, endUnix: NOW + 3 * DAY }),
          entry({ call: 'GEM', startUnix: NOW, endUnix: NOW + 3 * DAY }),
        ]}
        windows={windows}
        nowUnix={NOW}
      />,
    )
    expect(screen.getByText(/^best /)).toBeTruthy()
    expect(screen.getAllByText(/^best /)).toHaveLength(1)
  })
})
