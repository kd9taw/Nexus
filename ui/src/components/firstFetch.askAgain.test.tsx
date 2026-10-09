// @vitest-environment jsdom
//
// AWARDS, JOURNEY AND STATISTICS ASK AGAIN WHILE A CONTACT IS STILL BEING SAVED.
//
// On a slow disk the engine refuses a read of the log until the contact logged before it is saved
// (features/notAnswered), rather than answer without it. These views read the log once, when they
// open, so a refusal used to show their failed state until the view was opened again. Each now asks
// again a second later, three times at most. Held by value against a fake engine that refuses and
// then answers with the contact: the view shows the contact once the engine answers, and nothing
// failed in between; after the last refusal it shows its failed state and asks no more. Any other
// failure is shown at once and not asked again, as before.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render, screen } from '@testing-library/react'
import type { AwardSummary, GeoLogStats, JourneySummary, LoggedQso } from '../types'
import type { LogQuestion } from '../features/logAnswers'
import { answerAs } from '../features/logAnswers.testkit'
import { ASK_AGAIN_AFTER_MS, ASK_AGAIN_TIMES, NOT_ANSWERED } from '../features/notAnswered'
import { t } from '../i18n'

vi.mock('../api', () => ({
  getAwards: vi.fn(),
  getConfirmationDiagnostics: vi.fn(),
  uploadLotwReportByIds: vi.fn(),
  qrzPushQso: vi.fn(),
  clublogPushQso: vi.fn(),
  eqslPushQso: vi.fn(),
  getJourney: vi.fn(),
  getSettings: vi.fn(),
  askLog: vi.fn(),
  getLogStats: vi.fn(),
}))
vi.mock('../features/shareCard', () => ({ shareCard: vi.fn() }))

import { AwardsView } from './AwardsView'
import { JourneyView } from './JourneyView'
import { StatsView } from './StatsView'
import { askLog, getAwards, getConfirmationDiagnostics, getJourney, getLogStats, getSettings } from '../api'

/** The engine's refusal, as the desktop's IPC rejects with it: a bare string. */
const REFUSED = `${NOT_ANSWERED}: a logbook change is still on its way (0 of 1 saved)`
/** Any other failure. */
const UNREADABLE = 'the logbook could not be read: disk I/O error'

/** The first ask settles; then a second goes by. */
const settle = () => act(async () => {})
const aSecond = () => act(() => vi.advanceTimersByTimeAsync(ASK_AGAIN_AFTER_MS))
/** Long after the last ask could have been made. */
const aMinute = () => act(() => vi.advanceTimersByTimeAsync(60_000))
/** Every second the asking again takes. */
async function everyAskAgain() {
  for (let i = 0; i < ASK_AGAIN_TIMES; i++) await aSecond()
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  vi.mocked(getConfirmationDiagnostics).mockResolvedValue(null as never)
  vi.mocked(getSettings).mockResolvedValue({ mycall: 'KD9TAW' } as never)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.resetAllMocks()
})

// ── Awards ──────────────────────────────────────────────────────────────────────────────────────

/** The summary of a 201-contact log, ZD7AA just logged among them. */
const AWARDS = {
  qsos: 201,
  confirmedQsos: 120,
  dxccWorked: 101,
  dxccConfirmed: 90,
  dxccCredited: 80,
  readyToSubmit: 10,
  slotsWorked: 300,
  slotsConfirmed: 250,
  bands: [],
  modes: [],
  needed: [],
  slotNeeded: [],
  achievements: [],
  fiveBandWorked: 40,
  fiveBandConfirmed: 30,
  satDxccWorked: 0,
  satDxccConfirmed: 0,
  wazWorked: 30,
  wazConfirmed: 25,
  honorRoll: { currentTotal: 340, confirmed: 90, threshold: 331, achieved: false, needed: 241, numberOne: false, numberOneNeeded: 250 },
  was: { worked: 40, confirmed: 30, needed: [], fiveBandWorked: 10, fiveBandConfirmed: 5 },
  vucc: { worked: 10, confirmed: 5, bands: [], satWorked: 0, satConfirmed: 0, awards: [] },
  iota: { worked: 1, confirmed: 0, cardConfirmed: 0 },
  bandTargets: [],
} as unknown as AwardSummary
const withTheContact = t('awards.confirmed.note', { confirmed: 120, total: 201 })

describe('the Awards view', () => {
  it('asks again after a refusal, and shows the contact once the engine answers', async () => {
    vi.mocked(getAwards).mockRejectedValueOnce(REFUSED).mockResolvedValue(AWARDS)
    render(<AwardsView showGamification={false} />)
    await settle()
    expect(getAwards).toHaveBeenCalledTimes(1)
    expect(screen.getByText(t('awards.loading.title')), 'still loading, not failed').toBeTruthy()
    await aSecond()
    expect(getAwards).toHaveBeenCalledTimes(2)
    expect(screen.getByText(withTheContact)).toBeTruthy()
    expect(screen.queryByText(t('awards.load.failed.title'))).toBeNull()
  })

  it(`refused ${1 + ASK_AGAIN_TIMES} times, it shows its failed state and asks no more`, async () => {
    vi.mocked(getAwards).mockRejectedValue(REFUSED)
    render(<AwardsView showGamification={false} />)
    await settle()
    await everyAskAgain()
    expect(getAwards).toHaveBeenCalledTimes(1 + ASK_AGAIN_TIMES)
    expect(screen.getByText(t('awards.load.failed.title'))).toBeTruthy()
    await aMinute()
    expect(getAwards, 'never past its limit').toHaveBeenCalledTimes(1 + ASK_AGAIN_TIMES)
  })

  it('any other failure shows its failed state at once, and is not asked again', async () => {
    vi.mocked(getAwards).mockRejectedValue(UNREADABLE)
    render(<AwardsView showGamification={false} />)
    await settle()
    expect(screen.getByText(t('awards.load.failed.title'))).toBeTruthy()
    await aMinute()
    expect(getAwards).toHaveBeenCalledTimes(1)
  })

  it('its confirmation diagnostics ask again too', async () => {
    vi.mocked(getAwards).mockResolvedValue(AWARDS)
    vi.mocked(getConfirmationDiagnostics).mockReset().mockRejectedValueOnce(REFUSED).mockResolvedValue(null as never)
    render(<AwardsView showGamification={false} />)
    await settle()
    await aSecond()
    expect(getConfirmationDiagnostics).toHaveBeenCalledTimes(2)
  })
})

// ── Journey ─────────────────────────────────────────────────────────────────────────────────────

/** A Journey whose first DX contact is ZD7AA, just logged. */
const JOURNEY = {
  level: 1,
  xp: 10,
  xpIntoLevel: 10,
  xpForLevel: 100,
  totalQsos: 1,
  nextMilestone: null,
  firsts: [
    {
      id: 'first-dx',
      title: 'First DX Contact',
      meaning: 'You worked a station in another country.',
      heritage: 'The first one is the one you remember.',
      unlocked: true,
      whenUnix: 1,
      detail: 'ZD7AA · St Helena',
    },
  ],
  ladders: [],
  collections: [],
  feats: [],
  bests: [],
  streak: { enabled: false, weeks: 0, bestWeeks: 0, activeThisWeek: false },
} as unknown as JourneySummary

describe('the Journey view', () => {
  it('asks again after a refusal, and shows the contact once the engine answers', async () => {
    vi.mocked(getJourney).mockRejectedValueOnce(REFUSED).mockResolvedValue(JOURNEY)
    render(<JourneyView />)
    await settle()
    expect(screen.getByText(t('journey.loading.title')), 'still loading, not failed').toBeTruthy()
    await aSecond()
    expect(getJourney).toHaveBeenCalledTimes(2)
    expect(screen.getByText(/ZD7AA · St Helena/)).toBeTruthy()
    expect(screen.queryByText(t('journey.load.failed.title'))).toBeNull()
  })

  it(`refused ${1 + ASK_AGAIN_TIMES} times, it shows its failed state with the engine's words and asks no more`, async () => {
    vi.mocked(getJourney).mockRejectedValue(REFUSED)
    render(<JourneyView />)
    await settle()
    await everyAskAgain()
    expect(getJourney).toHaveBeenCalledTimes(1 + ASK_AGAIN_TIMES)
    expect(screen.getByText(t('journey.load.failed.title'))).toBeTruthy()
    expect(screen.getByText(REFUSED)).toBeTruthy()
    await aMinute()
    expect(getJourney, 'never past its limit').toHaveBeenCalledTimes(1 + ASK_AGAIN_TIMES)
  })

  it('any other failure shows its failed state at once, and is not asked again', async () => {
    vi.mocked(getJourney).mockRejectedValue(UNREADABLE)
    render(<JourneyView />)
    await settle()
    expect(screen.getByText(UNREADABLE)).toBeTruthy()
    await aMinute()
    expect(getJourney).toHaveBeenCalledTimes(1)
  })
})

// ── Statistics ──────────────────────────────────────────────────────────────────────────────────

const qso = (call: string, country: string) =>
  ({ call, band: '20m', mode: 'FT8', freqMhz: 14.074, whenUnix: 1_700_000_000, confirmed: false, awardConfirmed: false,
    country, grid: 'IH74' }) as unknown as LoggedQso
/** The log with ZD7AA, just logged, as the engine answers once it is saved. */
const LOG = [qso('W1AW', 'United States'), qso('K1ABC', 'United States'), qso('N2XYZ', 'United States'), qso('ZD7AA', 'St Helena')]
const GEO = {
  total: 4,
  resolved: 4,
  dx: 1,
  domestic: 3,
  byContinent: [
    { continent: 'NA', qsos: 3, entities: 1 },
    { continent: 'AF', qsos: 1, entities: 1 },
  ],
  byZone: [
    { zone: 5, qsos: 3 },
    { zone: 36, qsos: 1 },
  ],
} as unknown as GeoLogStats

/** The engine's log questions over `LOG`, refused the first `refusals` times. */
function engineRefusing(refusals: number) {
  let left = refusals
  vi.mocked(askLog).mockImplementation(async (q: LogQuestion) => {
    if (left-- > 0) throw REFUSED
    return answerAs(q, LOG)
  })
}
const total = () => document.querySelector('.stats-summary .stats-num')?.textContent

describe('the Statistics view', () => {
  it('asks again after a refusal, and counts the contact once the engine answers', async () => {
    engineRefusing(1)
    vi.mocked(getLogStats).mockResolvedValue(GEO)
    render(<StatsView />)
    await settle()
    expect(screen.getByText(t('stats.loading')), 'still loading, not failed').toBeTruthy()
    await aSecond()
    expect(askLog).toHaveBeenCalledTimes(2)
    expect(total()).toBe('4')
    expect(screen.queryByText(t('stats.failed'))).toBeNull()
  })

  it(`refused ${1 + ASK_AGAIN_TIMES} times, it shows its failed state and asks no more`, async () => {
    engineRefusing(Infinity)
    vi.mocked(getLogStats).mockResolvedValue(GEO)
    render(<StatsView />)
    await settle()
    await everyAskAgain()
    expect(askLog).toHaveBeenCalledTimes(1 + ASK_AGAIN_TIMES)
    expect(screen.getByText(t('stats.failed'))).toBeTruthy()
    await aMinute()
    expect(askLog, 'never past its limit').toHaveBeenCalledTimes(1 + ASK_AGAIN_TIMES)
  })

  it('its geographic cards ask again too, and show the contact once the engine answers', async () => {
    engineRefusing(0)
    vi.mocked(getLogStats).mockRejectedValueOnce(REFUSED).mockResolvedValue(GEO)
    render(<StatsView />)
    await settle()
    expect(total()).toBe('4')
    expect(screen.queryByText('AF'), 'not before the engine answers').toBeNull()
    await aSecond()
    expect(getLogStats).toHaveBeenCalledTimes(2)
    expect(screen.getByText('AF')).toBeTruthy()
  })

  it('any other failure shows its failed state at once, and is not asked again', async () => {
    vi.mocked(askLog).mockRejectedValue(UNREADABLE)
    vi.mocked(getLogStats).mockRejectedValue(UNREADABLE)
    render(<StatsView />)
    await settle()
    expect(screen.getByText(t('stats.failed'))).toBeTruthy()
    await aMinute()
    expect(askLog).toHaveBeenCalledTimes(1)
    expect(getLogStats).toHaveBeenCalledTimes(1)
  })
})
