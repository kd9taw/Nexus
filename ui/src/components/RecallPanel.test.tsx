// @vitest-environment jsdom
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { RecallPanel } from './RecallPanel'
import type { CallHistory } from '../features/callHistory'
import type { LoggedQso } from '../types'

afterEach(cleanup)

function qso(over: Partial<LoggedQso> = {}): LoggedQso {
  return {
    call: 'W1ABC',
    grid: 'FN31',
    band: '20m',
    freqMhz: 14.25,
    mode: 'SSB',
    rstSent: '59',
    rstRcvd: '57',
    whenUnix: Date.UTC(2026, 2, 14) / 1000, // 14 Mar 26
    ...(over as object),
  } as LoggedQso
}

function hist(over: Partial<CallHistory> = {}): CallHistory {
  const qsos = over.qsos ?? [qso()]
  return {
    qsos,
    count: qsos.length,
    workedBefore: qsos.length > 0,
    dupeThisBand: false,
    lastUnix: qsos.length ? qsos[0].whenUnix : null,
    confirmedCount: 0,
    bands: ['20m'],
    modes: ['SSB'],
    ...over,
    // keep the derived fields consistent with whatever qsos we were handed
    ...(over.qsos ? { count: over.qsos.length } : {}),
  }
}

describe('RecallPanel — compact (Phone / CW cockpits)', () => {
  // REGRESSION (operator, 2026-07-26): "the log this qso on Voice is no longer showing previous
  // contacts for a CS entered and resolved."
  //
  // Compact was introduced to stop the full card shoving the cockpit around, and it collapsed the
  // whole history down to a "worked 3×" count. That answers "have I worked them?" but NOT the
  // question actually being asked mid-contact: when, on what band, and what did we exchange.
  // The list is bounded (a fixed-height scroller) rather than dropped.
  it('lists previous contacts, not just a count', () => {
    render(
      <RecallPanel
        call="W1ABC"
        band="40m"
        compact
        hist={hist({
          qsos: [
            qso({ band: '20m', mode: 'SSB', whenUnix: Date.UTC(2026, 2, 14) / 1000 }),
            qso({ band: '15m', mode: 'CW', rstSent: '599', rstRcvd: '579', whenUnix: Date.UTC(2025, 10, 2) / 1000 }),
          ],
        })}
      />,
    )
    // The date / band+mode / report of each prior contact must be readable.
    expect(screen.getByText('14 Mar 26')).toBeTruthy()
    expect(screen.getByText('20m SSB')).toBeTruthy()
    expect(screen.getByText('59/57')).toBeTruthy()
    expect(screen.getByText('02 Nov 25')).toBeTruthy()
    expect(screen.getByText('15m CW')).toBeTruthy()
    expect(screen.getByText('599/579')).toBeTruthy()
  })

  it('still leads with the call and the decide-now flags', () => {
    render(<RecallPanel call="w1abc" band="20m" compact hist={hist({ dupeThisBand: true })} />)
    expect(screen.getByText('W1ABC')).toBeTruthy() // upper-cased for readback
    expect(screen.getByText(/DUPE 20m/)).toBeTruthy()
  })

  it('says so plainly on a never-worked call, with no empty history block', () => {
    const { container } = render(<RecallPanel call="W9XYZ" band="20m" compact hist={hist({ qsos: [] })} />)
    expect(screen.getByText('first contact')).toBeTruthy()
    expect(container.querySelector('.recall-line-log')).toBeNull()
  })

  // The reason compact exists at all: it must not be able to grow without bound and push the
  // operating panes off screen. The history scrolls inside a ceiling instead.
  it('keeps a long history bounded rather than growing the pane', () => {
    const many = Array.from({ length: 40 }, (_, i) =>
      qso({ whenUnix: Date.UTC(2026, 0, 1) / 1000 - i * 86_400 }),
    )
    const { container } = render(<RecallPanel call="W1ABC" band="20m" compact hist={hist({ qsos: many })} />)
    const log = container.querySelector('.recall-line-log')
    expect(log).toBeTruthy()
    expect(log?.querySelectorAll('.recall-line-row').length).toBe(40) // all present…
    expect(log?.getAttribute('role')).toBe('list') // …and reachable as a list, scrolled by CSS
  })

  it('renders nothing until enough of a call is typed', () => {
    const { container } = render(<RecallPanel call="W1" band="20m" compact hist={hist()} />)
    expect(container.firstChild).toBeNull()
  })
})
