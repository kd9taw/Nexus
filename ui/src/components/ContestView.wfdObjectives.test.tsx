// @vitest-environment jsdom
//
// WINTER FIELD DAY'S OBJECTIVES on the contest screen. The sponsor's 2027 rules score a log
// as QSO points × (OM + 1), OM being the multipliers of the objectives completed (p.7), and
// ask the entrant to select those objectives when submitting. So Winter Field Day's screen
// shows the sponsor's thirteen objectives where ARRL Field Day shows its bonuses, ticked into
// a setting of their own, with the claimed total as the score line, hints from the log for
// the three objectives a log can show, and a note when the QRP objective and the declared
// Power category disagree.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { ContestView } from './ContestView'
import { setSettings, saveTextToDownloads } from '../api'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayQso, FieldDayStatus, Settings } from '../types'

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

const q = (call: string, band: string, mode: string): FieldDayQso => ({
  call,
  class: '1H',
  section: 'MN',
  band,
  mode,
  submode: mode === 'DIG' ? 'RTTY' : '',
})

/** The shared fixture's numbers, as the engine sends them: 5 QSO points, OM 7, total 40. */
const WFD: FieldDayStatus = {
  composing: [
    { key: 'CLASS', raw: '3O' },
    { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
  ],
  running: false,
  state: 'Idle',
  event: 'wfd',
  qsoCount: 3,
  sections: 3,
  points: 5,
  poweredPoints: 5,
  bonusPoints: 0,
  totalScore: 40,
  objectiveMultiplier: 7,
  log: [q('K1ABC', '20m', 'CW'), q('N0XYZ', '40m', 'PH'), q('N7OUT', '80m', 'DIG')],
}

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

async function mount(patch: Partial<Settings>, fd: FieldDayStatus = WFD) {
  settingsNow = {
    ...defaultSettings,
    fdEvent: 'wfd',
    contestCategoryPower: 'QRP',
    fdBonuses: [],
    fdBonusesPlanned: [],
    fdObjectives: ['wfd-alt-power-100', 'wfd-qrp'],
    fdObjectivesPlanned: [],
    ...patch,
  }
  const r = render(<ContestView fieldDay={fd} onSetMode={() => {}} />)
  await settle()
  return r
}

const openObjectives = () => fireEvent.click(screen.getByRole('button', { name: /^Objectives/ }))

/** One objective's checkbox. */
const box = (c: HTMLElement, id: string) => c.querySelector(`#fd-objective-${id}`) as HTMLInputElement

const lastSaved = () => {
  const calls = vi.mocked(setSettings).mock.calls
  return calls[calls.length - 1][0] as unknown as Record<string, unknown>
}

beforeEach(() => {
  vi.mocked(setSettings).mockClear()
  vi.mocked(saveTextToDownloads).mockClear()
})
afterEach(() => cleanup())

describe('the Winter Field Day objectives checklist', () => {
  it('lists the sponsor\'s thirteen objectives with their multipliers, and no ARRL bonuses or power chips', async () => {
    const { container } = await mount({})
    openObjectives()
    const rows = [...container.querySelectorAll('[data-objective-state]')]
    expect(rows.length).toBe(13)
    expect(rows.map((r) => r.querySelector('.fd-bonus-pts')!.textContent)).toEqual(
      ['×1', '×2', '×3', '×1', '×2', '×3', '×1', '×1', '×6', '×6', '×2', '×4', '×2'],
    )
    expect(container.querySelector('#fd-bonus-emergency-power')).toBeNull()
    expect(container.querySelector('.fd-power-chip')).toBeNull()
    // The header counts what is done, with the ENGINE's multiplier.
    expect(screen.getByRole('button', { name: /^Objectives/ }).textContent).toContain('3/13 completed · OM 7')
  })

  it('shows an objective a ticked one brings with it as done, and not as a box of its own to untick', async () => {
    const { container } = await mount({})
    openObjectives()
    const implied = box(container, 'wfd-alt-power-equipment')
    expect([implied.checked, implied.disabled]).toEqual([true, true])
    const ticked = box(container, 'wfd-alt-power-100')
    expect([ticked.checked, ticked.disabled]).toEqual([true, false])
  })

  it('a tick writes the objectives setting and never the bonus list', async () => {
    const { container } = await mount({})
    openObjectives()
    fireEvent.click(box(container, 'wfd-six-hours'))
    await settle()
    const saved = lastSaved()
    expect(saved.fdObjectives).toEqual(['wfd-alt-power-100', 'wfd-qrp', 'wfd-six-hours'])
    expect(saved.fdBonuses).toEqual([])
  })

  it('planning an objective writes the plan list and leaves the ticked list alone', async () => {
    await mount({})
    openObjectives()
    fireEvent.click(screen.getByRole('button', { name: /^Plan Operate away from home/ }))
    await settle()
    const saved = lastSaved()
    expect([saved.fdObjectivesPlanned, saved.fdObjectives]).toEqual([
      ['wfd-away-from-home'],
      ['wfd-alt-power-100', 'wfd-qrp'],
    ])
  })

  it('the score line is the claimed total, QSO points × (OM + 1)', async () => {
    const { container } = await mount({})
    expect(container.querySelector('.fd-score-math')!.textContent).toBe('QSO pts 5 × (OM 7 + 1) = 40')
  })

  it('hints from the log: the bands holding three contacts and the modes worked', async () => {
    const six = ['160m', '80m', '40m', '20m', '15m', '10m'].flatMap((b) => [
      q('K1ABC', b, 'CW'),
      q('N0XYZ', b, 'PH'),
      q('N7OUT', b, 'DIG'),
    ])
    const { container } = await mount({}, { ...WFD, log: six, qsoCount: six.length })
    openObjectives()
    const hint = (id: string) =>
      container.querySelector(`[data-objective-state] #fd-objective-${id}`)!.parentElement!.textContent
    expect(hint('wfd-six-bands')).toContain('log: 6 of 6 bands with 3+ contacts')
    expect(hint('wfd-twelve-bands')).toContain('log: 6 of 12 bands with 3+ contacts')
    expect(hint('wfd-multiple-modes')).toContain('log: 3 modes')
    // Hints only: nothing is ticked for them.
    expect(box(container, 'wfd-six-bands').checked).toBe(false)
  })

  it('says so when the QRP objective and the Power category disagree, and is quiet when they agree', async () => {
    const note = (c: HTMLElement) => c.querySelector('.fd-bonuses-body [role="note"]')?.textContent ?? null
    let r = await mount({ contestCategoryPower: 'LOW' })
    openObjectives()
    expect(note(r.container)).toContain('“Operate the event QRP” is ticked, but your Power category')
    cleanup()
    r = await mount({ contestCategoryPower: 'QRP', fdObjectives: ['wfd-alt-power-100'] })
    openObjectives()
    expect(note(r.container)).toBe('Your Power category is QRP, but “Operate the event QRP” (×4) is not ticked.')
    cleanup()
    r = await mount({ contestCategoryPower: 'QRP' })
    openObjectives()
    expect(note(r.container)).toBeNull()
  })

  it('the downloaded summary lists the objectives with their multipliers, the OM and the total', async () => {
    await mount({})
    fireEvent.click(screen.getByRole('button', { name: 'Summary' }))
    await settle()
    const calls = vi.mocked(saveTextToDownloads).mock.calls
    const text = calls[calls.length - 1][1] as string
    expect(text).toContain('Objectives completed (3, OM 7):')
    expect(text).toContain('  Operate station equipment on alternative power — ×1')
    expect(text).toContain('  Operate the event QRP — ×4')
    expect(text).toContain('  TOTAL                      40')
    expect(text).not.toContain('Power multiplier')
  })

  it('a station that does not score the objectives (older than them) keeps the bonus checklist it scores', async () => {
    const older: FieldDayStatus = { ...WFD, objectiveMultiplier: undefined, totalScore: 5 }
    const { container } = await mount({}, older)
    expect(screen.queryByRole('button', { name: /^Objectives/ })).toBeNull()
    expect(screen.getByRole('button', { name: /^Bonuses/ })).toBeTruthy()
    expect(container.querySelector('.fd-score-math')!.textContent).toContain('apply at submission')
  })

  it('a Remote observation shows the ticks read-only, with no note it cannot check', async () => {
    const { container } = render(
      <ContestView
        fieldDay={WFD}
        observation={{
          fdOperator: '',
          fdPowerMult: 1,
          fdBonuses: [],
          fdBonusesPlanned: [],
          fdObjectives: ['wfd-qrp'],
          fdObjectivesPlanned: [],
        }}
      />,
    )
    await settle()
    openObjectives()
    const qrp = box(container, 'wfd-qrp')
    expect([qrp.checked, qrp.disabled]).toEqual([true, true])
    expect(container.querySelector('.fd-bonuses-body [role="note"]')).toBeNull()
  })
})

describe('the Winter Field Day log table', () => {
  it('puts the H/I/M/O category letters on the class column, where they travel with the class number', async () => {
    const { container } = await mount({})
    const head = (col: string) => container.querySelector(`.fd-log-head .fd-col.${col}`)!.textContent
    // The 2027 rules: class is the number of transmitters, category is H, I, O or M (2O).
    expect([head('cls'), head('sec')]).toEqual(['Category (H/I/M/O)', 'Section'])
  })
})
