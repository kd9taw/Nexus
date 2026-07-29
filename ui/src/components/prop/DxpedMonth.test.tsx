// @vitest-environment jsdom
// The month grid + chase digest: the parts where getting it wrong is silent.
// A calendar that puts an operation on the wrong day, or a digest that ranks a
// finished expedition above a live one, looks perfectly fine and is useless.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { DxpedMonth } from './DxpedMonth'
import { DxpedDigest } from './DxpedDigest'
import { DxpedCalendar } from './DxpedCalendar'
import { dxpedColorIndex } from './dxpedLanes'
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

  it('draws ONE bar spanning the run, not a chip per day', () => {
    // Dates are UTC-MIDNIGHT aligned on purpose. This pins the boundary case: a
    // run ending exactly at a day boundary must NOT paint that day, and the whole
    // run is a single grid item spanning its columns.
    const midnight = Date.UTC(2026, 6, 29) / 1000
    const start = midnight + 3 * DAY
    render(
      <DxpedMonth
        entries={[entry({ call: 'T33T', startUnix: start, endUnix: start + 2 * DAY })]}
        nowUnix={NOW}
      />,
    )
    const bars = screen.getAllByTitle(/^T33T/)
    expect(bars).toHaveLength(1)
    // NOW is a Wednesday, so the row starts Mon 27 Jul and start+3d is Sat 1 Aug
    // — the weekend, column 6, running into Sunday.
    expect(bars[0].style.gridColumn).toBe('6 / span 2')
  })

  it('splits a run at the week boundary and labels BOTH rows', () => {
    // Two anonymous continuation rows was the bug. Each week row carries the
    // callsign; the edge it crosses is what goes square and flush.
    const midnight = Date.UTC(2026, 6, 29) / 1000
    render(
      <DxpedMonth
        entries={[entry({ call: 'VP8PJ', startUnix: midnight, endUnix: midnight + 9 * DAY })]}
        nowUnix={NOW}
      />,
    )
    const bars = screen.getAllByTitle(/^VP8PJ/)
    expect(bars).toHaveLength(2)
    expect(screen.getAllByText('VP8PJ')).toHaveLength(2)
    expect(bars[0].className).toContain('cont-next')
    expect(bars[0].className).not.toContain('cont-prev')
    expect(bars[1].className).toContain('cont-prev')
    expect(bars[1].className).not.toContain('cont-next')
  })

  it('gives an operation the same colour slot everywhere it appears', () => {
    const midnight = Date.UTC(2026, 6, 29) / 1000
    render(
      <DxpedMonth
        entries={[entry({ call: 'VK9CM', startUnix: midnight, endUnix: midnight + 9 * DAY })]}
        nowUnix={NOW}
      />,
    )
    const slots = screen.getAllByTitle(/^VK9CM/).map((b) => b.getAttribute('data-dxc'))
    expect(new Set(slots).size).toBe(1)
    expect(slots[0]).toBe(String(dxpedColorIndex('VK9CM')))
  })

  it('shows bands on a bar wide enough to hold them, and never on a narrow one', () => {
    const midnight = Date.UTC(2026, 6, 29) / 1000
    const bands = ['160m', '80m', '40m', '20m']
    render(
      <DxpedMonth
        entries={[
          entry({ call: 'WIDE', bands, startUnix: midnight, endUnix: midnight + 5 * DAY }),
          entry({ call: 'NARROW', bands, startUnix: midnight, endUnix: midnight + DAY }),
        ]}
        nowUnix={NOW}
      />,
    )
    expect(screen.getByText('160m·80m·40m+1')).toBeTruthy()
    expect(screen.getAllByText('160m·80m·40m+1')).toHaveLength(1)
    // Both bars still name their operation — bands are what gets dropped.
    expect(screen.getByText('NARROW')).toBeTruthy()
  })

  it('offers "+N" rather than silently hiding a crowded day', () => {
    const midnight = Date.UTC(2026, 6, 29) / 1000
    const crowd = Array.from({ length: 6 }, (_, i) =>
      entry({ call: `OP${i}`, startUnix: midnight, endUnix: midnight + 2 * DAY }),
    )
    render(<DxpedMonth entries={crowd} nowUnix={NOW} maxLanes={4} />)
    const more = screen.getAllByText('+3')
    expect(more.length).toBeGreaterThan(0)
    // Only three of the six got a bar; the rest are behind the chip, not lost.
    expect(screen.queryByText('OP5')).toBeNull()
    fireEvent.click(more[0])
    expect(screen.getByText('OP5')).toBeTruthy()
  })

  it('renders nothing at all when there are no announcements', () => {
    const { container } = render(<DxpedMonth entries={[]} nowUnix={NOW} />)
    expect(container.firstChild).toBeNull()
  })

  it('a bar click opens the page AND still syncs the details rail', () => {
    // Both, deliberately. The operator asked a click to open the webpage; the
    // rail sync is what the click already did, and losing it would make the
    // calendar unable to reach the detail at all.
    const onSelect = vi.fn()
    const onOpenPage = vi.fn()
    const e = entry({ call: '3Y0L', website: 'https://3y0l.com/' })
    render(
      <DxpedMonth entries={[e]} nowUnix={NOW} onSelect={onSelect} onOpenPage={onOpenPage} />,
    )
    fireEvent.click(screen.getByTitle(/^3Y0L/))
    expect(onSelect).toHaveBeenCalledWith('3Y0L')
    expect(onOpenPage).toHaveBeenCalledWith(e)
  })

  it('tells the operator where the click goes before they click it', () => {
    render(
      <DxpedMonth
        entries={[
          entry({ call: 'SITE', website: 'https://3y0l.com/' }),
          entry({ call: 'NOSITE', startUnix: NOW + 2 * DAY, endUnix: NOW + 3 * DAY }),
        ]}
        nowUnix={NOW}
      />,
    )
    expect(screen.getByTitle(/^SITE/).title).toContain('https://3y0l.com/')
    // The fallback says so rather than pretending it is the expedition's site.
    const fallback = screen.getByTitle(/^NOSITE/).title
    expect(fallback).toContain('No website announced')
    expect(fallback).toContain('https://www.qrz.com/db/NOSITE')
  })
})

describe('DxpedCalendar — the details rail', () => {
  it('reaches the same page from the detail entry, labelled', () => {
    // The bar and the rail must both get there; the rail is where the operator
    // can SEE which of the two destinations it is before committing.
    const openPage = vi.fn()
    const site = entry({ call: '3Y0L', website: 'https://3y0l.com/' })
    const none = entry({ call: 'TY5FR', startUnix: NOW + 2 * DAY, endUnix: NOW + 3 * DAY })
    render(<DxpedCalendar entries={[site, none]} openPage={openPage} />)
    // The rail is `hidden` behind the Details tab, so it is correctly absent from
    // the a11y tree until the operator switches to it.
    expect(screen.queryAllByRole('button', { name: /↗/ })).toHaveLength(0)
    fireEvent.click(screen.getByRole('tab', { name: 'Details' }))
    const buttons = screen.getAllByRole('button', { name: /↗/ })
    expect(buttons).toHaveLength(2)
    expect(buttons[0].textContent).toContain('Website')
    expect(buttons[0].title).toContain('https://3y0l.com/')
    expect(buttons[1].textContent).toContain('QRZ')
    expect(buttons[1].title).toContain('No website announced')
    fireEvent.click(buttons[0])
    expect(openPage).toHaveBeenCalledWith(site)
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
