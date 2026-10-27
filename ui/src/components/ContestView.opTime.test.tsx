// @vitest-environment jsdom
//
// THE CONTEST SUMMARY SAYS HOW LONG THE STATION WAS ON.
//
// Operating time follows the sponsors' rule: a break of 30 minutes or more between two contacts is
// off time, the rest from the first contact to the last is on time.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, act, cleanup, within } from '@testing-library/react'
import { ContestView, buildSummaryText } from './ContestView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayQso, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...defaultSettings })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  fdSetUpload: vi.fn(async () => ({})),
  saveTextToDownloads: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
}))

const T0 = Date.UTC(2026, 9, 18, 15, 0) / 1000
const row = (call: string, mins: number, nr?: number): FieldDayQso =>
  ({
    call,
    class: '',
    section: '',
    band: '20m',
    mode: 'CW',
    submode: '',
    whenUnix: T0 + mins * 60,
    mex: '599 KANE',
    rcvd: ['599', 'COOK'],
    ...(nr === undefined ? {} : { nr }),
  }) as FieldDayQso

// 15:00, 15:10, then a 45-minute break, 15:55, 16:05: on 0:20, off 0:45.
const LOG = [row('K9AAA', 0, 1), row('W9BBB', 10, 2), row('N9DDD', 55, 4), row('K9EEE', 65, 5)]

const ILQP = (log: FieldDayQso[]): FieldDayStatus =>
  ({
    running: true,
    state: 'sp',
    qsoCount: log.length,
    sections: 0,
    points: 4,
    multCount: 2,
    poweredPoints: 8,
    totalScore: 8,
    bonusPoints: 0,
    event: 'ilqp',
    role: 'in_state',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
    sentExchange: '599 KANE',
    log,
    boards: [],
  }) as unknown as FieldDayStatus

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

afterEach(() => cleanup())

describe('operating time', () => {
  it('the scoreboard shows the on time and the off time', async () => {
    render(<ContestView fieldDay={ILQP(LOG)} onSetMode={() => {}} />)
    await settle()
    const on = document.querySelector('[data-optime="on"]') as HTMLElement
    const off = document.querySelector('[data-optime="off"]') as HTMLElement
    expect(within(on).getByText('0:20')).toBeTruthy()
    expect(within(off).getByText('0:45')).toBeTruthy()
    expect(on.textContent).toContain('On time')
    expect(off.textContent).toContain('Off time')
  })

  it('no tiles before the first contact', async () => {
    render(<ContestView fieldDay={ILQP([])} onSetMode={() => {}} />)
    await settle()
    expect(document.querySelector('[data-optime]')).toBeNull()
  })

  it('the Score Summary states both, and the rule', () => {
    const text = buildSummaryText({
      eventName: 'Illinois QSO Party',
      isWfd: false,
      rulesYear: 2026,
      rulesGenerated: '2026-09-17T00:00:00Z',
      myClass: '',
      mySection: '',
      log: LOG,
      modes: { dig: 0, cw: 4, ph: 0 },
      workedSet: new Set<string>(),
      powerMult: 1,
      qsoPts: 8,
      poweredPoints: 8,
      bonusPoints: 0,
      totalScore: 16,
      claimedBonuses: [],
      contest: { sending: '599 KANE', multCount: 2 },
    })
    expect(text).toContain('Operating time:  on 0:20   off 0:45 (breaks of 30 min or more)')
  })

  it('the Score Summary takes the contest’s own minimum: the same log under CQ WW’s 60 minutes has no break', () => {
    const text = buildSummaryText({
      eventName: 'CQ WW DX CW',
      isWfd: false,
      rulesYear: 2026,
      rulesGenerated: '2026-09-17T00:00:00Z',
      myClass: '',
      mySection: '',
      log: LOG,
      modes: { dig: 0, cw: 4, ph: 0 },
      workedSet: new Set<string>(),
      powerMult: 1,
      qsoPts: 8,
      poweredPoints: 8,
      bonusPoints: 0,
      totalScore: 16,
      claimedBonuses: [],
      contest: { sending: '599 4', multCount: 2 },
      minOffSec: 60 * 60,
    })
    expect(text).toContain('Operating time:  on 1:05   off 0:00 (breaks of 60 min or more)')
  })
})
