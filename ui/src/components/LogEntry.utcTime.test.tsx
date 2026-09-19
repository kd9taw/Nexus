// @vitest-environment jsdom
//
// #280, the log strip's half: the "Log a contact from another radio" override takes the contact's
// UTC time, and that box was a native `type="time"` control — which WebView2 draws in the OS
// locale, so a 12-hour Windows PC showed 00:58 UTC as "12:58 AM". It is now a plain 24-hour UTC
// box (HH:MM or HH:MM:SS), and a time that is not one holds the Log button instead of being
// quietly replaced by "now".
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { logQso } from '../api'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => {
  const getLog = vi.fn(() => Promise.resolve([]))
  return {
    fdLogManual: vi.fn(() => Promise.resolve({})),
    contestLogManual: vi.fn(() => Promise.resolve({})),
    logQso: vi.fn(() => Promise.resolve({})),
    getLog,
    // The shared log store (big-log fix) reads through get_log_delta.
    getLogDelta: vi.fn(async () => ({ revision: 1, full: true, rows: await getLog() })),
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
  const time = screen.getByLabelText('Time (UTC)') as HTMLInputElement
  const date = screen.getByLabelText('Date (UTC)') as HTMLInputElement
  return { time, date }
}
const logButton = () => screen.getByRole('button', { name: 'Log' }) as HTMLButtonElement

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('the override takes the contact time as 24-hour UTC (#280)', () => {
  it('is a plain text box holding a 24-hour time, not a native control the OS locale draws', () => {
    const { time } = openOverride()
    expect(time.type, 'the time is a native control the OS locale renders').toBe('text')
    // Seeded with now, in 24-hour UTC.
    expect(time.value).toMatch(/^([01]\d|2[0-3]):[0-5]\d$/)
  })

  it('logs the UTC instant typed, to the second when seconds are given', async () => {
    const { time, date } = openOverride()
    fireEvent.change(date, { target: { value: '2026-09-14' } })
    fireEvent.change(time, { target: { value: '00:58:37' } })
    fireEvent.click(logButton())
    await waitFor(() => expect(logQso).toHaveBeenCalled())
    const rec = vi.mocked(logQso).mock.calls[0][0]
    expect(rec.whenUnix).toBe(Math.floor(Date.UTC(2026, 8, 14, 0, 58, 37) / 1000))
  })

  it('refuses 25:00: Log is held, says why, and nothing is logged', () => {
    const { time, date } = openOverride()
    fireEvent.change(date, { target: { value: '2026-09-14' } })
    fireEvent.change(time, { target: { value: '25:00' } })
    expect(logButton().disabled, 'Log stayed armed on an impossible time').toBe(true)
    expect(time.getAttribute('aria-invalid')).toBe('true')
    expect(screen.getByText(/HH:MM/)).toBeTruthy()
    // Enter in a report field is the other way to log; it must refuse too.
    fireEvent.keyDown(screen.getAllByPlaceholderText('RST')[0], { key: 'Enter' })
    expect(logQso).not.toHaveBeenCalled()
  })
})
