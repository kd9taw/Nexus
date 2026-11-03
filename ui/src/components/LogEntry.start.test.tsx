// @vitest-environment jsdom
// #438: the rag-chew Start control on the CW and Phone log line. TIME_ON is stamped when the call
// enters the line and TIME_OFF at Log; Start restarts TIME_ON at now, for a long contact whose call
// went in early. Nothing else about the stamping changes.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { logQso } from '../api'
import type { AppSnapshot } from '../types'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
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
  setLogFormGrid: vi.fn(() => Promise.resolve()),
}))

const mockedLogQso = vi.mocked(logQso)
const snap = { radio: { band: '20m', dialMhz: 14.2 }, hunt: null } as unknown as AppSnapshot

function renderStrip(startButton: boolean) {
  render(
    <LogEntry
      snap={snap}
      mode="SSB"
      defaultRst="59"
      exchange="terrestrial"
      fieldDay={null}
      fdMode={undefined}
      startButton={startButton}
    />,
  )
}
const callBox = () => screen.getByPlaceholderText('Call')
const startBtn = () => screen.queryByRole('button', { name: /^Start/ })
const logBtn = () => screen.getByRole('button', { name: 'Log' })

beforeEach(() => mockedLogQso.mockClear())
afterEach(() => cleanup())

describe('LogEntry — the rag-chew Start control (#438)', () => {
  it('Start restarts TIME_ON at now; Log still stamps TIME_OFF', () => {
    const now = vi.spyOn(Date, 'now')
    try {
      const t0 = 1_790_000_000 // 2026-09-21 14:13:20 UTC
      renderStrip(true)
      now.mockReturnValue(t0 * 1000)
      fireEvent.change(callBox(), { target: { value: 'w1aw' } })
      // Twenty minutes of calling before the contact really starts.
      now.mockReturnValue((t0 + 1200) * 1000)
      fireEvent.click(startBtn()!)
      // The time it set is shown on the control.
      expect(startBtn()!.textContent).toContain('14:33')
      expect(startBtn()!.getAttribute('title')).toContain('14:33')
      // Correcting the call a key at a time keeps the restarted start.
      now.mockReturnValue((t0 + 1300) * 1000)
      fireEvent.change(callBox(), { target: { value: 'w1awx' } })
      now.mockReturnValue((t0 + 3000) * 1000)
      fireEvent.click(logBtn())
      const rec = mockedLogQso.mock.calls[0][0]
      expect(rec.whenUnix).toBe(t0 + 1200)
      expect(rec.timeOffUnix).toBe(t0 + 3000)
    } finally {
      now.mockRestore()
    }
  })

  it('without a press, the start is still when the call was entered', () => {
    const now = vi.spyOn(Date, 'now')
    try {
      const t0 = 1_790_000_000
      renderStrip(true)
      now.mockReturnValue(t0 * 1000)
      fireEvent.change(callBox(), { target: { value: 'w1aw' } })
      expect(startBtn()!.textContent).toBe('Start')
      now.mockReturnValue((t0 + 600) * 1000)
      fireEvent.click(logBtn())
      const rec = mockedLogQso.mock.calls[0][0]
      expect(rec.whenUnix).toBe(t0)
      expect(rec.timeOffUnix).toBe(t0 + 600)
    } finally {
      now.mockRestore()
    }
  })

  it('a new contact forgets the restart: the next call starts when it is entered', () => {
    const now = vi.spyOn(Date, 'now')
    try {
      const t0 = 1_790_000_000
      renderStrip(true)
      now.mockReturnValue(t0 * 1000)
      fireEvent.change(callBox(), { target: { value: 'w1aw' } })
      now.mockReturnValue((t0 + 100) * 1000)
      fireEvent.click(startBtn()!)
      fireEvent.change(callBox(), { target: { value: 'k9xyz' } })
      expect(startBtn()!.textContent).toBe('Start')
      now.mockReturnValue((t0 + 400) * 1000)
      fireEvent.click(logBtn())
      expect(mockedLogQso.mock.calls[0][0].whenUnix).toBe(t0 + 100)
    } finally {
      now.mockRestore()
    }
  })

  it('works only while a call is entered, and is drawn only where the cockpit asks for it', () => {
    renderStrip(true)
    // Drawn from the start, as Lookup is, so the first letter typed moves nothing in the row.
    expect((startBtn() as HTMLButtonElement).disabled, 'an empty line has no contact to start').toBe(true)
    fireEvent.change(callBox(), { target: { value: 'w1aw' } })
    expect((startBtn() as HTMLButtonElement).disabled).toBe(false)
    cleanup()
    renderStrip(false)
    fireEvent.change(callBox(), { target: { value: 'w1aw' } })
    expect(startBtn(), 'a strip that does not ask for it has none').toBeNull()
  })

  it('is not offered while the other-radio override sets the time by hand', () => {
    renderStrip(true)
    fireEvent.change(callBox(), { target: { value: 'w1aw' } })
    fireEvent.click(screen.getByRole('button', { name: /another radio/i }))
    expect(startBtn()).toBeNull()
  })
})
