// @vitest-environment jsdom
//
// THE CONTEST SETUP FROM THE CONTEST SCREEN: "Set up a contest…" with none running and "Change setup…"
// on one that runs, each opening the setup (at the exchange for a change); the quiet "this weekend"
// line only when a contest on the picker starts within seven days and none runs; the Running button
// saying plainly what it does on an FT mode; and the end checklist carrying the running log's missing
// Cabrillo lines and the Summary.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { ContestView } from './ContestView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus } from '../types'
import type { ContestWindow, FdRulesetDto } from '../api'

let station: Record<string, unknown> = {}
let windows: ContestWindow[] = []
let snapshotFieldDay: FieldDayStatus | null = null

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...station })),
  setSettings: vi.fn(async (s: Record<string, unknown>) => {
    station = { ...s }
    return {}
  }),
  getFdRuleset: vi.fn(async () => null),
  previewFdRuleset: vi.fn(async () => null),
  contestWindows: vi.fn(async () => windows),
  getSnapshot: vi.fn(async () => ({ fieldDay: snapshotFieldDay, mode: 'chat', radio: {}, link: {} })),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  saveTextToDownloads: vi.fn(async () => ''),
  fdMergeToGeneral: vi.fn(async () => ({ added: 2, already: 0, refused: 0, queued: false })),
  setUnassistedMode: vi.fn(async () => ({})),
  fdClubLogNewest: vi.fn(async () => null),
}))

const rules: FdRulesetDto = {
  event: 'ilqp',
  rulesYear: 2026,
  bannedModes: ['FT8', 'FT4'],
  spottingAllowed: true,
  clusterAllowed: true,
  enforcement: 'warn',
  entryClasses: ['ILLINOIS FIXED HIGH POWER', 'UNLIMITED'],
}

const session = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    running: false,
    state: 'Idle',
    qsoCount: 1,
    sections: 0,
    points: 1,
    log: [{ call: 'W9XYZ', band: '20m', mode: 'CW', whenUnix: 1_760_000_000 }],
    workedSections: [],
    event: 'ilqp',
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'COOK' },
    ],
    sentExchange: '599 COOK',
    receives: [],
    rulesYear: 2026,
    cabrilloMissing: ['ENTRY-CLASS'],
    ...over,
  }) as unknown as FieldDayStatus

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 12; i++) await Promise.resolve()
  })
}

async function openOn(fieldDay: FieldDayStatus | null, fdActive = fieldDay !== null) {
  render(
    <ContestView
      fieldDay={fieldDay}
      onSetMode={vi.fn()}
      fdActive={fdActive}
      fdRuleset={rules}
      onOpenSettings={vi.fn()}
      onSettingsSaved={vi.fn()}
    />,
  )
  await settle()
}

beforeEach(() => {
  station = { ...defaultSettings, fdEvent: 'ilqp' }
  windows = []
  snapshotFieldDay = null
})
afterEach(cleanup)

describe('the setup, from the Contest screen', () => {
  it('with none running: "Set up a contest…" opens it at the pick', async () => {
    await openOn(null)
    fireEvent.click(screen.getByRole('button', { name: 'Set up a contest…' }))
    await settle()
    expect(screen.getByRole('dialog', { name: 'Set up a contest' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Which contest?' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Change setup…' })).toBeNull()
  })

  it('on one that runs: "Change setup…" opens it at the exchange', async () => {
    await openOn(session())
    fireEvent.click(screen.getByRole('button', { name: 'Change setup…' }))
    await settle()
    expect(screen.getByRole('dialog', { name: 'Change contest setup' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'What Illinois QSO Party sends' })).toBeTruthy()
  })
})

describe('the quiet "this weekend" line', () => {
  const inDays = (d: number): ContestWindow => {
    const start = Math.floor(Date.now() / 1000) + d * 86_400
    return { id: 'nyqp', startUnix: start, endUnix: start + 86_400 }
  }

  it('names a contest starting within seven days, with Set up…', async () => {
    windows = [inDays(3)]
    await openOn(null)
    const line = document.querySelector('.contest-soon')!
    expect(line.textContent).toMatch(/^New York QSO Party this weekend: it starts [A-Z][a-z]{2} \d{1,2} \d{4}Z\. Set up…$/)
    fireEvent.click(screen.getByRole('button', { name: 'Set up…' }))
    await settle()
    // The setup opens on that contest.
    expect(screen.getByRole('radio', { name: /New York QSO Party/ }).getAttribute('aria-checked')).toBe('true')
  })

  it('CONTROL: nothing for a contest eight days off', async () => {
    windows = [inDays(8)]
    await openOn(null)
    expect(document.querySelector('.contest-soon')).toBeNull()
  })

  it('CONTROL: nothing while a contest runs', async () => {
    windows = [inDays(3)]
    await openOn(session())
    expect(document.querySelector('.contest-soon')).toBeNull()
  })
})

describe('the Running button says what it does on FT', () => {
  it('names TX on and CQ for an FT mode, and the screen no longer says it never transmits', async () => {
    await openOn(session())
    expect(screen.getByRole('button', { name: 'Running' }).getAttribute('title')).toBe(
      'Running: you call CQ. On an FT mode (FT8, FT4) it turns TX on and starts calling CQ at once.',
    )
  })
})

describe('the end checklist from the Contest screen', () => {
  it('carries the running log\'s missing lines with the contest\'s own choices, and the Summary', async () => {
    await openOn(session())
    fireEvent.click(screen.getByRole('switch'))
    await settle()
    expect(screen.getByRole('dialog', { name: 'Contest over?' })).toBeTruthy()
    const cls = screen.getByRole('combobox', { name: 'Entry class (ENTRY-CLASS)' }) as HTMLSelectElement
    expect([...cls.options].map((o) => o.value)).toEqual(['', 'ILLINOIS FIXED HIGH POWER', 'UNLIMITED'])
    expect((screen.getByRole('checkbox', { name: /Export the Summary/ }) as HTMLInputElement).checked).toBe(false)
  })
})
