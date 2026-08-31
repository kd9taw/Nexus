// @vitest-environment jsdom
//
// FIELD DAY BONUS PLANNING — three states, and only one of them scores.
//
// A club knows on Friday which bonuses it expects (media publicity, a public location, a
// safety officer, a youth op) and wants to see what it is still chasing as the weekend
// runs. What it must NEVER see is an intention counted as points: `settings.fdBonuses` is
// the EARNED list the ARRL entry is made of, `settings.fdBonusesPlanned` is the plan, and
// nothing that scores, exports or reports reads the second one. A scoring surface that
// quietly counted a plan would be a submitted-score error.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup, within } from '@testing-library/react'
import { FieldDayView, FD_BONUSES, fdBonusState, fdBonusTally } from './FieldDayView'
import { setSettings, saveTextToDownloads } from '../api'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus, Settings } from '../types'

let settingsNow: Record<string, unknown> = {}

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => settingsNow),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  saveTextToDownloads: vi.fn(async () => '/home/op/Downloads/fd-summary.txt'),
  openPanelWindow: vi.fn(async () => {}),
}))

/** No score fields on the snapshot, so the view falls back to computing the bonus points
 *  from settings — the exact path a plan could leak into. */
const FD: FieldDayStatus = {
  myClass: '2A',
  mySection: 'WI',
  running: false,
  state: 'Idle',
  qsoCount: 4,
  sections: 2,
  points: 6,
  log: [],
}

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

const ALL_IDS = FD_BONUSES.map((b) => b.id)
const ALL_POINTS = FD_BONUSES.reduce((n, b) => n + b.points, 0)

async function mount(patch: Partial<Settings>, fd: FieldDayStatus = FD) {
  settingsNow = { ...defaultSettings, fdPowerMult: 2, fdBonuses: [], fdBonusesPlanned: [], ...patch }
  const r = render(<FieldDayView fieldDay={fd} onSetMode={() => {}} />)
  await settle()
  return r
}

/** Open the Scoring disclosure (it only opens itself while the event is running). */
const openScoring = () => fireEvent.click(screen.getByRole('button', { name: /Bonuses/ }))

/** The settings object the last setSettings() call was given. */
const lastSaved = () => {
  const calls = vi.mocked(setSettings).mock.calls
  return calls[calls.length - 1][0] as unknown as Record<string, unknown>
}

beforeEach(() => {
  vi.mocked(setSettings).mockClear()
  vi.mocked(saveTextToDownloads).mockClear()
})
afterEach(() => cleanup())

describe('the three-state model', () => {
  it('reads earned, planned and untouched out of the two id lists', () => {
    expect(fdBonusState('youth', ['youth'], [])).toBe('earned')
    expect(fdBonusState('youth', [], ['youth'])).toBe('planned')
    expect(fdBonusState('youth', [], [])).toBe('none')
    // An id on BOTH lists is earned: confirming a planned bonus must not have to edit two
    // lists, and un-confirming a mis-click puts the plan back rather than losing it.
    expect(fdBonusState('youth', ['youth'], ['youth'])).toBe('earned')
  })

  it('never counts one bonus in both columns', () => {
    const t = fdBonusTally(['youth'], ['youth', 'safety-officer'])
    expect(t.earnedPoints).toBe(100)
    expect(t.plannedPoints).toBe(100) // safety-officer only
    expect(t.potentialPoints).toBe(200)
    expect(t.earnedCount).toBe(1)
    expect(t.plannedCount).toBe(1)
  })
})

describe('ONLY EARNED BONUSES SCORE', () => {
  it('the score line ignores a fully planned menu', async () => {
    const { container } = await mount({ fdBonuses: [], fdBonusesPlanned: ALL_IDS })
    // 6 QSO pts × 2 = 12, and 1450 points of plan add exactly nothing.
    expect(ALL_POINTS).toBe(1450)
    const math = container.querySelector('.fd-score-math')!.textContent!
    expect(math).toContain('bonuses 0')
    expect(math).toContain('= 12')
    expect(math).not.toContain('1450')
  })

  it('the downloaded score summary claims nothing that was only planned', async () => {
    await mount({ fdBonuses: ['w1aw-bulletin'], fdBonusesPlanned: ALL_IDS })
    fireEvent.click(screen.getByRole('button', { name: 'Summary' }))
    await settle()
    const calls = vi.mocked(saveTextToDownloads).mock.calls
    const text = calls[calls.length - 1][1] as string
    expect(text).toContain('Bonuses claimed (1, 100 pts):')
    expect(text).toContain('W1AW Bulletin — 100 pts')
    expect(text).toContain('+ bonuses                  100')
    // Every other bonus is on the plan and must not appear as claimed.
    expect(text).not.toContain('Youth Participation')
    expect(text).not.toContain('Safety Officer')
  })

  it('planning a bonus writes the plan list and leaves the earned list alone', async () => {
    await mount({ fdBonuses: ['youth'], fdBonusesPlanned: [] })
    openScoring()
    fireEvent.click(screen.getByRole('button', { name: /^Plan Safety Officer/ }))
    await settle()
    expect(lastSaved().fdBonusesPlanned).toEqual(['safety-officer'])
    expect(lastSaved().fdBonuses).toEqual(['youth'])
  })

  it('confirming a planned bonus moves it into the earned list, plan untouched', async () => {
    await mount({ fdBonuses: [], fdBonusesPlanned: ['youth'] })
    openScoring()
    const row = document.querySelector('[data-bonus-state="planned"]')!
    expect(within(row as HTMLElement).getByText('Youth Participation')).toBeTruthy()
    fireEvent.click(within(row as HTMLElement).getByRole('checkbox'))
    await settle()
    expect(lastSaved().fdBonuses).toEqual(['youth'])
    expect(lastSaved().fdBonusesPlanned).toEqual(['youth'])
  })
})

describe('the chase is visible and says which number is real', () => {
  it('shows earned, planned and the ceiling', async () => {
    const { container } = await mount({
      fdBonuses: ['w1aw-bulletin'],
      fdBonusesPlanned: ['youth', 'safety-officer'],
    })
    openScoring()
    const chase = container.querySelector('.fd-chase')!.textContent!
    expect(chase).toContain('Earned 100 pts')
    expect(chase).toContain('counted in your score')
    expect(chase).toContain('Planned +200 pts')
    expect(chase).toContain('not scored until you tick it')
    expect(chase).toContain('If all land 300 pts')
  })

  it('names the chase on the collapsed header too', async () => {
    await mount({ fdBonuses: [], fdBonusesPlanned: ['youth', 'safety-officer'] })
    expect(screen.getByRole('button', { name: /2 planned · \+200 pts/ })).toBeTruthy()
  })
})

describe('the two halves of the score are on one surface', () => {
  it('the power chips write the same fdPowerMult Settings writes', async () => {
    await mount({ fdPowerMult: 2 })
    openScoring()
    fireEvent.click(screen.getByRole('button', { name: '×5 QRP / battery' }))
    await settle()
    expect(lastSaved().fdPowerMult).toBe(5)
    // and nothing else on the scoring panel moved
    expect(lastSaved().fdBonuses).toEqual([])
    expect(lastSaved().fdBonusesPlanned).toEqual([])
  })

  it('echoes the multiplier on the collapsed header, so it is findable', async () => {
    await mount({ fdPowerMult: 5 })
    expect(screen.getByRole('button', { name: /×5 power/ })).toBeTruthy()
  })
})

describe('the 15 bonuses are impossible to miss during a running event', () => {
  it('opens the checklist itself once the event is running', async () => {
    await mount({}, { ...FD, running: true })
    expect(screen.getByRole('group', { name: 'Claimed FD bonuses' })).toBeTruthy()
  })

  it('stays closed when no event is running', async () => {
    await mount({}, { ...FD, running: false })
    expect(screen.queryByRole('group', { name: 'Claimed FD bonuses' })).toBeNull()
  })
})
