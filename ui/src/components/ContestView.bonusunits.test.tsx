// @vitest-environment jsdom
//
// FIELD DAY BONUSES WORTH SO MUCH PER UNIT — ARRL Field Day 2026 rules, 7.3.
//
// Four bonuses are counted, not ticked once: 100 per transmitter on emergency power up to 20
// (7.3.1), 20 per youth participant up to 100 and 40 at Class B (7.3.15), 10 per formal message
// up to 100 (7.3.6), and five per GOTA contact with no limit (7.3.13.1), which the club log counts.
// The checklist mirrors `fd_rules::FdRuleset::bonus_value`, and every value below is the rule's.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup, within } from '@testing-library/react'
import { ContestView, FD_BONUSES, buildSummaryText, fdBonusLimit, fdBonusValue } from './ContestView'
import { exportLog, setSettings, saveTextToDownloads } from '../api'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus, Settings } from '../types'

let settingsNow: Record<string, unknown> = {}

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => settingsNow),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => 'NATURAL POWER QSOs (ARRL Field Day rule 7.3.8)\n'),
  fdClubExport: vi.fn(async () => ''),
  saveTextToDownloads: vi.fn(async () => '/home/op/Downloads/fd.txt'),
  openPanelWindow: vi.fn(async () => {}),
}))

const bonus = (id: string) => FD_BONUSES.find((b) => b.id === id)!

/** One bonus's value, claimed (or not) with `count` typed, at entry class `cls`. */
const value = (id: string, count: number | undefined, cls = '3A', claimed = true) =>
  fdBonusValue(bonus(id), claimed ? [id] : [], count === undefined ? {} : { [id]: count }, cls)

describe('a per-unit bonus scores per unit, up to its limit', () => {
  it('emergency power: 100 per transmitter, up to 20 (7.3.1)', () => {
    expect([0, 1, 3, 20, 21].map((n) => value('emergency-power', n))).toEqual([0, 100, 300, 2000, 2000])
    expect(value('emergency-power', undefined)).toBe(100)
    expect(value('emergency-power', 3, '3A', false)).toBe(0)
  })

  it('youth: 20 each up to 100, and 40 at Class B (7.3.15)', () => {
    expect([0, 1, 5, 6].map((n) => value('youth', n))).toEqual([0, 20, 100, 100])
    expect([0, 1, 2, 3].map((n) => value('youth', n, '2B'))).toEqual([0, 20, 40, 40])
    expect(fdBonusLimit(bonus('youth'), ' 1b ')).toBe(2)
    expect(fdBonusLimit(bonus('youth'), '')).toBe(5)
  })

  it('message handling: 10 per message, up to 100 (7.3.6)', () => {
    expect([0, 1, 10, 11].map((n) => value('message-handling', n))).toEqual([0, 10, 100, 100])
  })

  it('GOTA is the club log’s to count: nothing a laptop ticks or types moves it', () => {
    expect(value('gota', 50)).toBe(0)
    expect(fdBonusLimit(bonus('gota'), '3A')).toBeNull()
  })

  it('the GOTA coach and site responsibilities are one box each (7.3.13.2, 7.3.18)', () => {
    expect([value('gota-coach', undefined), value('gota-coach', 4)]).toEqual([100, 100])
    expect([value('site-responsibilities', undefined), value('site-responsibilities', 4)]).toEqual([50, 50])
    expect(value('gota-coach', undefined, '3A', false)).toBe(0)
  })
})

describe('the summary names each per-unit claim with its count', () => {
  it('writes count × per unit = points', () => {
    const text = buildSummaryText({
      eventName: 'ARRL Field Day',
      isWfd: false,
      rulesYear: 2026,
      rulesGenerated: '2026-10-14T12:00:00Z',
      myClass: '3A',
      mySection: 'WI',
      log: [],
      modes: { dig: 0, cw: 0, ph: 0 },
      workedSet: new Set<string>(),
      powerMult: 2,
      qsoPts: 0,
      poweredPoints: 0,
      bonusPoints: 400,
      totalScore: 400,
      claimedBonuses: [
        { ...bonus('emergency-power'), points: 300, units: 3 },
        bonus('w1aw-bulletin'),
      ],
    })
    expect(text).toContain('Bonuses claimed (2, 400 pts):')
    expect(text).toContain('  Emergency Power — 3 × 100 = 300 pts')
    expect(text).toContain('  W1AW Bulletin — 100 pts')
  })
})

// ---- the checklist itself -------------------------------------------------------------------

const FD: FieldDayStatus = {
  composing: [
    { key: 'CLASS', raw: '3A' },
    { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
  ],
  running: true,
  state: 'Idle',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
}

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

async function mount(patch: Partial<Settings>) {
  settingsNow = { ...defaultSettings, fdEvent: 'arrlfd', fdClass: '3A', fdBonuses: [], fdBonusesPlanned: [], ...patch }
  const r = render(<ContestView fieldDay={FD} onSetMode={() => {}} />)
  await settle()
  return r
}

const lastSaved = () => {
  const calls = vi.mocked(setSettings).mock.calls
  return calls[calls.length - 1][0] as Settings
}

const row = (id: string) => document.querySelector(`[data-bonus-state] input[id="fd-bonus-${id}"]`)?.closest('.fd-bonus-row') as HTMLElement

afterEach(() => {
  cleanup()
  vi.mocked(setSettings).mockClear()
})

describe('the checklist takes the count and scores it', () => {
  it('a claimed per-unit bonus shows its value and saves the count typed', async () => {
    await mount({ fdBonuses: ['emergency-power'], fdBonusCounts: { 'emergency-power': 3 } })
    const r = row('emergency-power')
    expect(within(r).getByText('300 pts')).toBeTruthy()
    const count = within(r).getByRole('spinbutton', { name: 'How many for Emergency Power' })
    expect((count as HTMLInputElement).value).toBe('3')
    fireEvent.change(count, { target: { value: '4' } })
    await settle()
    expect(lastSaved().fdBonusCounts).toEqual({ 'emergency-power': 4 })
    expect(lastSaved().fdBonuses).toEqual(['emergency-power'])
  })

  it('the earned chase counts the claim at its count', async () => {
    const { container } = await mount({
      fdBonuses: ['youth', 'w1aw-bulletin'],
      fdBonusCounts: { youth: 3 },
    })
    expect(container.querySelector('.fd-chase')!.textContent).toContain('Earned 160 pts')
  })

  it('an unclaimed per-unit bonus says what each unit is worth, and its count waits for the tick', async () => {
    await mount({})
    const r = row('message-handling')
    expect(within(r).getByText('10 pts each')).toBeTruthy()
    expect((within(r).getByRole('spinbutton') as HTMLInputElement).disabled).toBe(true)
  })

  it('the GOTA row has nothing to tick: the club log counts it', async () => {
    await mount({})
    const gota = screen.getByText('GOTA Station').closest('.fd-bonus-row') as HTMLElement
    expect(within(gota).queryByRole('checkbox')).toBeNull()
    expect(within(gota).getByText('5 pts per GOTA contact, counted from the club log')).toBeTruthy()
  })
})

describe('the natural power list (7.3.8)', () => {
  it('the switch saves fdNaturalPower and nothing else on the scoring panel', async () => {
    await mount({ fdNaturalPower: false })
    fireEvent.click(screen.getByRole('checkbox', { name: 'On natural power now' }))
    await settle()
    expect(lastSaved().fdNaturalPower).toBe(true)
    expect(lastSaved().fdBonuses).toEqual([])
  })

  it('the export asks the station for its list and saves it as a text file', async () => {
    await mount({})
    fireEvent.click(screen.getByRole('button', { name: 'Natural power' }))
    await settle()
    expect(vi.mocked(exportLog)).toHaveBeenCalledWith('natural-power')
    const calls = vi.mocked(saveTextToDownloads).mock.calls
    expect(calls[calls.length - 1][0]).toMatch(/natural-power-\d{4}-\d{2}-\d{2}\.txt$/)
  })
})
