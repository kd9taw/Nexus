// @vitest-environment jsdom
//
// The date half of #280 in the log strip's "Log a contact from another radio" override
// (logging-lens review, 2026-09-19). Its TIME is a plain 24-hour UTC box; its DATE was still a
// native `type="date"` control, which WebView2 draws in the OS locale and fills from a "Today"
// button that means the LOCAL day. Hand-logging after 0000Z west of Greenwich, that Today is
// yesterday in UTC — the contact is filed a whole UTC day early.
//
// The date is now a plain UTC text box (YYYY-MM-DD), seeded with the UTC day, refusing what is
// not a date instead of blanking it.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { logQso } from '../api'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => {
  return {
    fdLogManual: vi.fn(() => Promise.resolve({})),
    contestLogManual: vi.fn(() => Promise.resolve({})),
    contestWorking: vi.fn(() => Promise.resolve({})),
    contestEntryReset: vi.fn(() => Promise.resolve({})),
    logQso: vi.fn(() => Promise.resolve({})),
    lookupPark: vi.fn(() => Promise.resolve(null)),
    lookupParkLive: vi.fn(() => Promise.resolve(null)),
    qrzLookup: vi.fn(() => Promise.resolve(null)),
    resolveEntity: vi.fn(() => Promise.resolve(null)),
    searchParks: vi.fn(() => Promise.resolve([])),
    setCwPeerInfo: vi.fn(() => Promise.resolve()),
  }
})

const snap = { radio: { band: '2m', dialMhz: 146.52 }, hunt: null, stations: [] } as unknown as AppSnapshot

function openOverride() {
  render(<LogEntry snap={snap} mode="SSB" defaultRst="59" exchange="terrestrial" />)
  fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 've3abc' } })
  fireEvent.click(screen.getByRole('button', { name: /Log a contact from another radio/ }))
  return {
    time: screen.getByLabelText('Time (UTC)') as HTMLInputElement,
    date: screen.getByLabelText('Date (UTC)') as HTMLInputElement,
  }
}
const logButton = () => screen.getByRole('button', { name: 'Log' }) as HTMLButtonElement

beforeEach(() => {
  // 00:30 UTC on the 20th — 19:30 the previous evening for a US operator, which is exactly when
  // a "Today" drawn from the local clock files the contact on the wrong UTC day.
  //
  // ONLY the clock is faked: `waitFor` polls on a real timer, and faking those too made it spin
  // until its own 5 s deadline instead of seeing the log land.
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date(Date.UTC(2026, 8, 20, 0, 30)))
})

afterEach(() => {
  vi.useRealTimers()
  cleanup()
  vi.clearAllMocks()
})

describe('the override takes the contact date as UTC text', () => {
  it('is a plain text box seeded with the UTC day, not a control the OS locale draws', () => {
    const { date } = openOverride()
    expect(date.type, 'the date is a native control the OS locale draws and dates itself').toBe('text')
    expect(date.value).toBe('2026-09-20')
  })

  it('logs the UTC day that was typed', async () => {
    const { time, date } = openOverride()
    fireEvent.change(date, { target: { value: '2026-09-19' } })
    fireEvent.change(time, { target: { value: '00:58:37' } })
    fireEvent.click(logButton())
    await waitFor(() => expect(logQso).toHaveBeenCalled())
    expect(vi.mocked(logQso).mock.calls[0][0].whenUnix).toBe(
      Math.floor(Date.UTC(2026, 8, 19, 0, 58, 37) / 1000),
    )
  })

  it('keeps an impossible date on screen, marks it, holds Log, and logs nothing', () => {
    const { date } = openOverride()
    fireEvent.change(date, { target: { value: '2026-02-30' } })
    // A native date control throws the typing away and shows a blank box, which reads as
    // "I cleared it" rather than "that is not a date".
    expect(date.value, 'the typed date was discarded instead of refused').toBe('2026-02-30')
    expect(date.getAttribute('aria-invalid')).toBe('true')
    expect(logButton().disabled, 'Log stayed armed on an impossible date').toBe(true)
    expect(screen.getByText(/YYYY-MM-DD/)).toBeTruthy()
    // Enter in a report field is the other way to log; it must refuse too.
    fireEvent.keyDown(screen.getAllByPlaceholderText('RST')[0], { key: 'Enter' })
    expect(logQso).not.toHaveBeenCalled()
  })
})
