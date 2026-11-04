// @vitest-environment jsdom
// #438: the rag-chew Start control on the CW and Phone log line. TIME_ON is stamped when the call
// enters the line and TIME_OFF at Log; Start restarts TIME_ON at now, for a long contact whose call
// went in early. Nothing else about the stamping changes.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { logQso } from '../api'
import type { AppSnapshot } from '../types'
import { css, loadSheets } from '../cssCascade.testkit'
import { beforeAll } from 'vitest'

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
/** What the button SHOWS: its visible label, not the hidden sizer that holds its width. */
const shownLabel = () => startBtn()!.querySelector('.le-start-label > :not([data-off])')!.textContent
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
      expect(shownLabel()).toContain('14:33')
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
      expect(shownLabel()).toBe('Start')
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
      expect(shownLabel()).toBe('Start')
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

describe('the Start label keeps its width (#438)', () => {
  beforeAll(() => loadSheets())

  it('the button is sized by its widest label from the start, so a press moves nothing after it', () => {
    renderStrip(true)
    fireEvent.change(callBox(), { target: { value: 'w1aw' } })
    const labels = [...startBtn()!.querySelectorAll('.le-start-label > *')]
    // Both labels are laid in the same grid cell: the wider one (with the time) sizes the button.
    expect(labels.map((l) => l.textContent)).toEqual(['Start', 'Start 00:00'])
    for (const l of labels) expect(css(l, 'grid-area') ?? `${css(l, 'grid-row-start')} / ${css(l, 'grid-column-start')}`).toMatch(/^1 \/ 1/)
    expect(css(startBtn()!.querySelector('.le-start-label')!, 'display')).toBe('inline-grid')
    // The one not shown takes its room but is not seen, and is not read out.
    const off = startBtn()!.querySelector('.le-start-label > [data-off]')!
    expect(off.textContent).toBe('Start 00:00')
    expect(css(off, 'visibility')).toBe('hidden')
    expect(off.getAttribute('aria-hidden')).toBe('true')
    expect(css(startBtn()!, 'font-variant-numeric')).toBe('tabular-nums')
    fireEvent.click(startBtn()!)
    expect(startBtn()!.querySelector('.le-start-label > [data-off]')!.textContent, 'after a press, the plain label is the sizer').toBe('Start')
  })
})

describe('a restored line does not wear another contact\'s Start time (#438)', () => {
  const at = (tick: number, band = '20m') =>
    ({ radio: { band, dialMhz: band === '20m' ? 14.2 : 7.2 }, hunt: null, bandChangeTick: tick }) as unknown as AppSnapshot
  const strip = (tick: number, band?: string) => (
    <LogEntry snap={at(tick, band)} mode="SSB" defaultRst="59" exchange="terrestrial" fieldDay={null} fdMode={undefined} startButton clearCallOnBandChange />
  )

  it('Restore puts the set-aside call back with a plain Start, and its own start time', async () => {
    const now = vi.spyOn(Date, 'now')
    try {
      const t0 = 1_790_000_000
      now.mockReturnValue(t0 * 1000)
      const view = render(strip(1))
      fireEvent.change(callBox(), { target: { value: 'ea8xx' } })
      // A band change sets EA8XX aside.
      view.rerender(strip(2, '40m'))
      await act(async () => {})
      // Another contact, restarted with Start at t0 + 600.
      now.mockReturnValue((t0 + 300) * 1000)
      fireEvent.change(callBox(), { target: { value: 'k9xyz' } })
      now.mockReturnValue((t0 + 600) * 1000)
      fireEvent.click(startBtn()!)
      expect(shownLabel()).toContain(':')
      fireEvent.click(screen.getByRole('button', { name: 'Restore' }))
      await act(async () => {})
      expect(callBox()).toHaveProperty('value', 'EA8XX')
      expect(shownLabel(), "K9XYZ's Start time stayed on EA8XX").toBe('Start')
      now.mockReturnValue((t0 + 900) * 1000)
      fireEvent.click(logBtn())
      await act(async () => {})
      expect(mockedLogQso.mock.calls[mockedLogQso.mock.calls.length - 1][0].whenUnix, 'EA8XX keeps its own start').toBe(t0)
    } finally {
      now.mockRestore()
    }
  })
})
