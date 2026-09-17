// @vitest-environment jsdom
//
// THE CONTEST VIEW IN A CONTEST THAT IS NOT FIELD DAY, rendered with the props App gives it
// and a `fieldDay` of the shape the engine sends for CQ WW CW.
//
// Every one of these used to be Field Day's whatever the picker said: the banner read "ARRL
// Field Day", the header showed a class and section (as dashes — the contest has neither),
// the log table's columns were Class and Section (empty on every row), the Score Summary
// printed a station class, a power multiplier and a bonus list, and the Field Day bonus
// checklist sat under the score of a contest that has no bonuses.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, act, cleanup } from '@testing-library/react'
import { ContestView, buildSummaryText } from './ContestView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayQso, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ ...defaultSettings })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  fdSetUpload: vi.fn(async () => ({})),
  saveTextToDownloads: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
}))

const row = (call: string, rcvd: string[]): FieldDayQso =>
  ({
    call,
    class: '',
    section: '',
    band: '20m',
    mode: 'CW',
    submode: '',
    whenUnix: Date.UTC(2026, 10, 28, 0, 5) / 1000,
    mex: '599 4',
    rcvd,
  }) as FieldDayQso

const CQWW: FieldDayStatus = {
  running: true,
  state: 'sp',
  qsoCount: 2,
  sections: 0,
  points: 6,
  multCount: 4,
  poweredPoints: 24,
  totalScore: 24,
  bonusPoints: 0,
  event: 'cqww_cw',
  role: '',
  receives: [
    { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
    { key: 'ZN', kind: 'number', required: true, min: 1, max: 40, adif: 'CQZ' },
  ],
  composing: [
    { key: 'RST', raw: '599' },
    { key: 'ZN', raw: '4' },
  ],
  sentExchange: '4',
  log: [row('DL1ABC', ['599', '14']), row('JA1ABC', ['599', '25'])],
  boards: [],
} as unknown as FieldDayStatus

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

afterEach(() => cleanup())

describe('the view names the contest that is running', () => {
  it('banner and header say CQ WW, and the header shows what is being sent', async () => {
    render(<ContestView fieldDay={CQWW} onSetMode={() => {}} />)
    await settle()
    expect(document.querySelector('.fd-event-name')!.textContent).toBe(
      'CQ World-Wide DX Contest (CW)',
    )
    expect(document.querySelector('.conv-peer')!.textContent).toBe('CQ WW CW')
    expect(document.body.textContent).not.toMatch(/ARRL Field Day/)
    // The sent exchange, not a class/section pair shown as dashes.
    expect(document.querySelector('.fd-class')!.textContent).toBe('4')
  })

  it('the log table’s columns are the contest’s received slots, filled from each row', async () => {
    render(<ContestView fieldDay={CQWW} onSetMode={() => {}} />)
    await settle()
    const head = [...document.querySelectorAll('.fd-log-head .fd-col')].map((n) => n.textContent)
    expect(head).toEqual(['Time', 'Call', 'RST', 'Zone', 'Band', 'Mode'])
    const first = document.querySelector('.fd-log-row')!
    const cells = [...first.querySelectorAll('.fd-col')].map((n) => n.textContent)
    expect(cells.slice(1, 4)).toEqual(['DL1ABC', '599', '14'])
    // No first-row "MULT" tag: that marker is Field Day's first-section-worked, and a CQ WW
    // row has no section.
    expect(first.classList.contains('mult')).toBe(false)
  })

  it('does not offer Field Day’s bonus checklist', async () => {
    render(<ContestView fieldDay={CQWW} onSetMode={() => {}} />)
    await settle()
    expect(document.querySelector('.fd-bonuses-section')).toBeNull()
    // POSITIVE CONTROL: Field Day still has it.
    cleanup()
    render(
      <ContestView
        fieldDay={{
          ...CQWW,
          event: 'arrlfd',
          receives: [
            { key: 'CLASS', kind: 'pattern', required: true },
            { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections' },
          ],
          composing: [
            { key: 'CLASS', raw: '1A' },
            { key: 'SECTION', raw: 'IL', domain: 'fd_sections' },
          ],
          multCount: undefined,
          log: [],
        }}
        onSetMode={() => {}}
      />,
    )
    await settle()
    expect(document.querySelector('.fd-bonuses-section')).not.toBeNull()
    expect(screen.getAllByText('ARRL Field Day').length).toBeGreaterThan(0)
    expect(document.querySelector('.conv-peer')!.textContent).toBe('Field Day')
  })
})

describe('the Score Summary of a contest that is not Field Day', () => {
  it('names the contest, what was sent and the multipliers — no class, power or bonuses', () => {
    const text = buildSummaryText({
      eventName: 'CQ World-Wide DX Contest (CW)',
      isWfd: false,
      rulesYear: 2026,
      rulesGenerated: '2026-09-17T00:00:00Z',
      myClass: '',
      mySection: '',
      log: CQWW.log,
      modes: { dig: 0, cw: 2, ph: 0 },
      workedSet: new Set<string>(),
      powerMult: 1,
      qsoPts: 6,
      poweredPoints: 6,
      bonusPoints: 0,
      totalScore: 24,
      claimedBonuses: [],
      contest: { sending: '599 4', multCount: 4 },
    })
    expect(text).toContain('CQ WORLD-WIDE DX CONTEST (CW) — SCORE SUMMARY')
    expect(text).toContain('Sending 599 4')
    expect(text).toContain('Multipliers: 4')
    expect(text).toMatch(/TOTAL\s+24/)
    for (const fdOnly of ['Station class', 'Sections worked', 'Power multiplier', 'Bonuses claimed']) {
      expect(text, fdOnly).not.toContain(fdOnly)
    }
  })
})
