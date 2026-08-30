// @vitest-environment jsdom
//
// The two Field Day header chips. Three things are worth pinning:
//
//   THE RATE IS THE TRAILING HOUR, and a contact with no timestamp is not counted. A rate is
//   read as a fact about the last sixty minutes; a guess dressed as one is worse than none.
//
//   WFD SHOWS RAW QSO POINTS AND NOTHING ELSE. The DTO carries the ARRL power-multiplier total
//   whatever the event is, so the honest number is a CHOICE the component makes — and a choice
//   is exactly what a later edit can quietly reverse. The fixture below deliberately gives WFD
//   a large `totalScore` so a chip that reached for it would be visibly wrong.
//
//   THE GOAL EDITOR NEVER SWALLOWS Esc. In this cockpit Esc stops the transmission. An input
//   that called `stopPropagation()` on its own Escape would make the stop key dead for as long
//   as the field was open, and nothing about the code would show it.
import { describe, it, expect, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { FdRateChip, FdScoreChip, fdTrailingRate } from './FdChips'
import type { FieldDayQso, FieldDayStatus } from '../types'

const NOW = 1_800_000_000

const at = (secsAgo: number): FieldDayQso => ({
  call: 'W1AW',
  class: '1A',
  section: 'IL',
  band: '20m',
  mode: 'CW',
  whenUnix: NOW - secsAgo,
})

const STATUS = (over: Partial<FieldDayStatus>): FieldDayStatus => ({
  myClass: '3A',
  mySection: 'WI',
  running: true,
  state: 'Idle',
  qsoCount: 213,
  sections: 47,
  points: 426,
  log: [],
  ...over,
})

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('the trailing-60-minute rate', () => {
  it('counts the last hour and nothing older', () => {
    expect(fdTrailingRate([at(60), at(3599), at(3601), at(7200)], NOW)).toBe(2)
  })

  it('ignores a contact with no timestamp rather than guessing one', () => {
    const untimed: FieldDayQso = { call: 'K1ABC', class: '1A', section: 'IL', band: '20m', mode: 'CW' }
    expect(fdTrailingRate([at(60), untimed], NOW)).toBe(1)
  })

  it('is zero on an empty log — not blank, not a dash', () => {
    expect(fdTrailingRate([], NOW)).toBe(0)
  })

  it('renders the count the helper computed', () => {
    render(<FdRateChip log={[at(60), at(120), at(7200)]} nowUnix={NOW} />)
    expect(document.querySelector('.fd-rate-chip')?.textContent).toContain('2')
  })
})

describe('the rate goal', () => {
  it('starts blank — an invented target on a first Field Day means nothing', () => {
    render(<FdRateChip log={[]} nowUnix={NOW} />)
    expect(screen.getByRole('button').textContent).toBe('set goal')
    expect(localStorage.getItem('nexus.fd.rateGoal')).toBeNull()
  })

  it('persists what the operator types, and shows it', () => {
    render(<FdRateChip log={[]} nowUnix={NOW} />)
    fireEvent.click(screen.getByRole('button'))
    const input = screen.getByLabelText('Contacts-per-hour goal')
    fireEvent.change(input, { target: { value: '60' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(localStorage.getItem('nexus.fd.rateGoal')).toBe('60')
    expect(screen.getByRole('button').textContent).toContain('60')
  })

  it('reads a stored goal back on mount', () => {
    localStorage.setItem('nexus.fd.rateGoal', '48')
    render(<FdRateChip log={[]} nowUnix={NOW} />)
    expect(screen.getByRole('button').textContent).toContain('48')
  })

  it('refuses a goal that is not a positive number, and clears rather than storing nonsense', () => {
    localStorage.setItem('nexus.fd.rateGoal', '60')
    render(<FdRateChip log={[]} nowUnix={NOW} />)
    fireEvent.click(screen.getByRole('button'))
    const input = screen.getByLabelText('Contacts-per-hour goal')
    fireEvent.change(input, { target: { value: 'lots' } })
    fireEvent.blur(input)
    expect(localStorage.getItem('nexus.fd.rateGoal')).toBeNull()
    expect(screen.getByRole('button').textContent).toBe('set goal')
  })

  it('lets Escape through to the cockpit’s TX stop, and closes the editor', () => {
    const seen: string[] = []
    const spy = (e: KeyboardEvent) => seen.push(e.key)
    window.addEventListener('keydown', spy)
    try {
      render(<FdRateChip log={[]} nowUnix={NOW} />)
      fireEvent.click(screen.getByRole('button'))
      const input = screen.getByLabelText('Contacts-per-hour goal')
      fireEvent.keyDown(input, { key: 'Escape' })
      // POSITIVE CONTROL for the listener itself: an ordinary key reaches the window too, so
      // an empty `seen` could never pass this by accident.
      expect(seen).toContain('Escape')
      expect(screen.getByRole('button'), 'the editor closed').toBeTruthy()
      fireEvent.keyDown(window, { key: 'A' })
      expect(seen).toContain('A')
    } finally {
      window.removeEventListener('keydown', spy)
    }
  })
})

describe('the score chip', () => {
  it('shows contacts, sections and the claimed total at ARRL Field Day', () => {
    render(<FdScoreChip fieldDay={STATUS({ event: 'arrlfd', totalScore: 1234, poweredPoints: 852 })} />)
    const text = document.querySelector('.fd-score-chip')?.textContent ?? ''
    expect(text).toContain('213')
    expect(text).toContain('47')
    expect(text).toContain('1234')
  })

  it('WFD shows RAW QSO POINTS ONLY — never the ARRL power-and-bonus total', () => {
    render(<FdScoreChip fieldDay={STATUS({ event: 'wfd', points: 426, totalScore: 9999, poweredPoints: 8888 })} />)
    const text = document.querySelector('.fd-score-chip')?.textContent ?? ''
    expect(text).toContain('426')
    expect(text, 'the ARRL total must not reach a WFD screen').not.toContain('9999')
    expect(text).not.toContain('8888')
    expect(document.querySelector('.fd-score-chip')?.getAttribute('title')).toContain(
      'multipliers apply at submission',
    )
  })

  it('falls back down the DTO — powered points, then raw QSO points — rather than blanking', () => {
    render(<FdScoreChip fieldDay={STATUS({ event: 'arrlfd', poweredPoints: 852 })} />)
    expect(document.querySelector('.fd-score-chip')?.textContent).toContain('852')
    cleanup()
    render(<FdScoreChip fieldDay={STATUS({ event: 'arrlfd' })} />)
    expect(document.querySelector('.fd-score-chip')?.textContent).toContain('426')
  })

  it('reads zeroes with no Field Day at all', () => {
    render(<FdScoreChip fieldDay={null} />)
    expect(document.querySelector('.fd-score-chip')?.textContent).toContain('0')
  })
})
