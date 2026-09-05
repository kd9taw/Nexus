// @vitest-environment jsdom
//
// The event banner reads the RUST-computed window (FieldDayStatus.eventStartUnix/
// eventEndUnix — fd_rules data interpreted by FdRuleset::next_or_running), not TS date
// math. The TS math this replaces hardcoded a 24-hour duration for both events, so during
// WFD's final six hours (it is a 30-hour event, 1600Z Sat → 21:59Z Sun) the banner claimed
// the event was over and counted down ~a year to the next one — the last surface still
// carrying the 24 h window bug class. Failing-first: this file was watched failing against
// the fdNextEvent() path before FieldDayView consumed the DTO window.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, act, cleanup } from '@testing-library/react'
import { FieldDayView } from './FieldDayView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: '' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
}))

// WFD 2026: Sat Jan 24 16:00Z → Sun Jan 25 22:00Z (30 hours).
const WFD_START = Date.UTC(2026, 0, 24, 16, 0, 0) / 1000
const WFD_END = WFD_START + 30 * 3600

const FD: FieldDayStatus = {
  myClass: '1O',
  mySection: 'IL',
  running: true,
  state: 'sp',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  event: 'wfd',
  eventStartUnix: WFD_START,
  eventEndUnix: WFD_END,
  rulesYear: 2026,
  rulesGenerated: '2026-08-29T00:00:00Z',
}

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('Field Day banner window comes from the DTO, not TS date math', () => {
  it('shows WFD as ACTIVE during its final six hours (hour 28 of 30)', async () => {
    // Sunday Jan 25, 20:00Z — inside the real 30 h window, but PAST the bogus
    // 24 h end (Sun 16:00Z) the deleted TS math produced.
    vi.useFakeTimers()
    vi.setSystemTime(new Date(Date.UTC(2026, 0, 25, 20, 0, 0)))

    render(<FieldDayView fieldDay={FD} onSetMode={() => {}} />)
    await settle()

    const subtitle = document.querySelector('.fd-event-subtitle')
    expect(subtitle).not.toBeNull()
    expect(subtitle!.textContent).toContain('active')
    expect(subtitle!.textContent).not.toContain('starts in')
    // And the range is the real Jan 24–25 weekend, not next year's.
    expect(subtitle!.textContent).toContain('Jan 24–25')
  })

  it('counts down to the DTO window before the event', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(Date.UTC(2026, 0, 10, 12, 0, 0)))

    render(<FieldDayView fieldDay={FD} onSetMode={() => {}} />)
    await settle()

    const subtitle = document.querySelector('.fd-event-subtitle')
    expect(subtitle!.textContent).toContain('starts in 14 days')
  })

  it('shows the rules-data identity line (rules year + generated date)', async () => {
    render(<FieldDayView fieldDay={FD} onSetMode={() => {}} />)
    await settle()

    const rules = document.querySelector('.fd-event-rules')
    expect(rules).not.toBeNull()
    expect(rules!.textContent).toContain('2026')
    expect(rules!.textContent).toContain('2026-08-29')
  })

  it('renders no countdown or rules line when the DTO carries no window (older backend)', async () => {
    const bare: FieldDayStatus = { ...FD }
    delete bare.eventStartUnix
    delete bare.eventEndUnix
    delete bare.rulesYear
    delete bare.rulesGenerated
    render(<FieldDayView fieldDay={bare} onSetMode={() => {}} />)
    await settle()

    // The event name still shows; the subtitle and rules line simply stay out.
    expect(screen.getAllByText('Winter Field Day').length).toBeGreaterThan(0)
    expect(document.querySelector('.fd-event-subtitle')?.textContent ?? '').toBe('')
    expect(document.querySelector('.fd-event-rules')).toBeNull()
  })
})
