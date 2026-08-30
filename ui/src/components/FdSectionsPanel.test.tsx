// @vitest-environment jsdom
//
// The cockpit's framing of the shipped sections board. Two properties, and one mirror.
//
//   THE BOARD IS THIS POSITION'S. `workedSections` is the own log's sections and nothing more
//   (the club DTO carries only a COUNT), so the label has to say so — a board read as the
//   club's progress would send an operator past a section the club still needs.
//
//   A VERDICT ONLY FOR A REAL SECTION. "NEW MULT" for a half-typed or bogus code promises a
//   multiplier the commit is about to refuse: `logIt`'s exchange gate validates against the
//   same universe, and the two must agree.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { FdSectionsPanel, fdWorkedSectionSet } from './FdSectionsPanel'
import type { FieldDayStatus } from '../types'

// FieldDayView (which owns SectionsBoard) reaches for the Tauri bridge at import time.
vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({})),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
  saveTextToDownloads: vi.fn(async () => '/tmp/x'),
}))

// ⚠️ The path goes through a VARIABLE. Vite rewrites a literal `new URL('./x.tsx',
// import.meta.url)` into a served asset URL — the same read then fails with "The URL must be
// of scheme file", which reads as a Node problem and is not one.
const read = (rel: string) => readFileSync(fileURLToPath(new URL(rel, import.meta.url)), 'utf8')
const FIELDDAY_VIEW_SRC = read('./FieldDayView.tsx')

const STATUS = (over: Partial<FieldDayStatus>): FieldDayStatus => ({
  myClass: '3A',
  mySection: 'WI',
  running: true,
  state: 'Idle',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  ...over,
})

const verdict = () => document.querySelector('[role="status"]')?.textContent ?? ''

afterEach(cleanup)

describe('the worked-section set mirrors FieldDayView’s', () => {
  it('prefers the DTO field and uppercases it', () => {
    expect([...fdWorkedSectionSet(STATUS({ workedSections: ['wi', ' il ', ''] }))].sort()).toEqual([
      'IL',
      'WI',
    ])
  })

  it('falls back to the log’s own sections when the field is absent', () => {
    const log = [{ call: 'W1AW', class: '1A', section: 'ct', band: '20m', mode: 'CW' }]
    expect([...fdWorkedSectionSet(STATUS({ log }))]).toEqual(['CT'])
  })

  it('is empty with no Field Day at all', () => {
    expect(fdWorkedSectionSet(null).size).toBe(0)
  })

  it('applies the SAME rule FieldDayView’s private workedSectionSet does', () => {
    // Neither side is wrong on its own — only the pair is, and this pane exists precisely
    // because the dashboard's copy could not be reused whole.
    const m = FIELDDAY_VIEW_SRC.match(/function workedSectionSet\([\s\S]*?\n\}/)
    expect(m, 'workedSectionSet not found in FieldDayView.tsx').not.toBeNull()
    const body = m![0].replace(/\s+/g, ' ')
    expect(body).toContain('fieldDay?.workedSections ?? (fieldDay?.log ?? []).map((q) => q.section)')
    expect(body).toContain('s.trim().toUpperCase()')
  })
})

describe('the board says whose sections it shows', () => {
  it('labels itself "this position" — never read as the club’s progress', () => {
    render(<FdSectionsPanel fieldDay={STATUS({ workedSections: ['WI'] })} />)
    expect(document.body.textContent).toContain('this position')
  })

  it('still renders the shipped 83-section board underneath', () => {
    render(<FdSectionsPanel fieldDay={STATUS({ workedSections: ['WI'] })} />)
    expect(document.querySelector('[aria-label="Worked sections board"]')).not.toBeNull()
    expect(document.querySelector('[aria-label="Wisconsin, worked"]')).not.toBeNull()
  })
})

describe('the verdict on the section being typed', () => {
  const FD = STATUS({ workedSections: ['WI', 'IL'] })

  it('says NEW MULT for a real section that is not in the log yet', () => {
    render(<FdSectionsPanel fieldDay={FD} draftSection="ct" />)
    expect(verdict()).toContain('NEW MULT')
    expect(verdict()).toContain('CT')
  })

  it('says worked for one already in the log — the same board it is drawn from', () => {
    render(<FdSectionsPanel fieldDay={FD} draftSection="WI" />)
    expect(verdict()).toContain('WI')
    expect(verdict()).not.toContain('NEW MULT')
  })

  it('says NOTHING for a blank, a half-typed code or a bogus one', () => {
    for (const draft of ['', ' ', 'W', 'ZZ', 'XX9']) {
      render(<FdSectionsPanel fieldDay={FD} draftSection={draft} />)
      expect(verdict(), `draft "${draft}" must claim no multiplier`).toBe('')
      cleanup()
    }
    // POSITIVE CONTROL: the same render WITH a real code does produce a verdict, so the
    // silence above is a judgement rather than a component that never speaks.
    render(<FdSectionsPanel fieldDay={FD} draftSection="ME" />)
    expect(verdict()).toContain('NEW MULT')
  })
})
