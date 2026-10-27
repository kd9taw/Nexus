// @vitest-environment jsdom
//
// The Activation pane: each park's count is the station's own, shown in the strip's words, so the
// pane and the strip can never say different numbers; its contacts are the Logbook's answer to the
// activation's own filter; it sends nothing. The log is a stub source and the api a mock.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import type { Activation, AppSnapshot, LoggedQso } from '../types'
import { t } from '../i18n'
import { createAskingLogSource } from '../features/askingLogSource'
import { setLogSource } from '../features/logSource'
import type { LogPage, LogQuestion } from '../features/logAnswers'

const api = vi.hoisted(() => ({
  getOtaSpots: vi.fn(async () => []),
  getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
  selfSpot: vi.fn(),
  askLog: vi.fn(),
}))
vi.mock('../api', () => ({
  ...api,
  clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  setActivationState: vi.fn(), setActivationGrid: vi.fn(), clearActivation: vi.fn(), resumeActivation: vi.fn(),
  downloadParks: vi.fn(), importParksCsv: vi.fn(), importHuntedParksCsv: vi.fn(), logActivations: vi.fn(async () => []),
}))

import { ActivationPane, ACTIVATION_ROWS } from './ActivationPane'
import { PotaSotaView } from './PotaSotaView'
import { PaneBody } from './connect/PaneFrame'
import type { PaneContext } from './connect/paneContext'

vi.setConfig({ testTimeout: 15_000 })

const DAY = 86_400
// 2026-10-26 23:30Z, and an activation that started there and now runs at 00:20Z the next day.
const START = 1_793_057_400
const NOW = START + 50 * 60

const twoFer = (over: Partial<Activation> = {}): Activation => ({
  program: 'POTA',
  reference: 'US-1234,US-5678',
  qsoCount: 7,
  states: [],
  myState: null,
  parks: [
    { reference: 'US-1234', count: 7, p2p: 2 },
    { reference: 'US-5678', count: 10, p2p: 0 },
  ],
  grid: 'EN52',
  startedAt: START,
  utcDay: Math.floor(NOW / DAY),
  ...over,
})

const qso = (call: string, whenUnix: number, theirRef?: string): LoggedQso =>
  ({
    id: `id-${call}`,
    call,
    band: '20m',
    mode: 'SSB',
    freqMhz: 14.285,
    whenUnix,
    ota: { myProgram: 'POTA', myRef: 'US-1234,US-5678', theirProgram: theirRef ? 'POTA' : null, theirRef: theirRef ?? null },
  }) as LoggedQso

/** The questions the pane asked, and the page the stub answers every page question with. */
const asked: LogQuestion[] = []
let rows: LoggedQso[] = []
beforeEach(() => {
  asked.length = 0
  rows = []
  setLogSource(
    createAskingLogSource(async <Q extends LogQuestion>(q: Q) => {
      asked.push(q)
      const page: Partial<LogPage> = { query: (q as { query: LogPage['query'] }).query, rows, total: rows.length, keys: rows.map((r) => r.id!) }
      return page as never
    }),
  )
})
afterEach(() => {
  cleanup()
  localStorage.clear()
})

/** The parks' lines the pane shows, as text. */
const paneParks = () =>
  [...document.querySelectorAll('.act-pane-park .act-pane-count')].map((el) => el.textContent)

it('shows each park’s count exactly as the strip does — the station’s count, never its own', async () => {
  const act = twoFer()
  // Twelve contacts listed (dupes among them): the pane still says the station's 7 and 10.
  rows = Array.from({ length: 12 }, (_, i) => qso(`W${i}ABC`, NOW - i * 60))
  const snap = { hunt: null, radio: { dialMhz: 14.285 }, activation: act } as unknown as AppSnapshot
  render(
    <>
      <PotaSotaView snap={snap} />
      <ActivationPane act={act} nowUnix={NOW} />
    </>,
  )
  const strip = [...document.querySelectorAll('.pota-act-progress')]
    .map((el) => el.textContent ?? '')
    .filter((s) => s.includes(' of 10'))
  expect(paneParks()).toEqual([
    t('ota.activation.progress', { reference: 'US-1234', count: 7 }),
    t('ota.activation.progress', { reference: 'US-5678', count: 10 }),
  ])
  // The strip's chips start with the same words, park for park.
  expect(strip.map((s) => s.split(' · ')[0])).toEqual(paneParks())
  const togo = [...document.querySelectorAll('.act-pane-togo')].map((el) => el.textContent)
  expect(togo).toEqual([t('ota.activation.toGo', { count: 3 }), t('ota.activation.valid')])
  expect([...document.querySelectorAll('.act-pane-p2p')].map((el) => el.textContent)).toEqual([
    t('ota.activation.p2p', { count: 2 }),
    t('ota.activation.p2p', { count: 0 }),
  ])
  await waitFor(() => expect(document.querySelectorAll('.act-pane-row:not(.empty)')).toHaveLength(12))
})

it('a park’s day under two calls: the strip and the pane both say POTA counts each call separately', () => {
  const act = twoFer({
    parks: [
      {
        reference: 'US-1234',
        count: 10,
        p2p: 0,
        calls: [
          { callsign: 'KD9TAW', count: 6 },
          { callsign: 'KD9TAW/M', count: 4 },
        ],
      },
      { reference: 'US-5678', count: 10, p2p: 0, calls: [{ callsign: 'KD9TAW', count: 10 }] },
    ],
  })
  const snap = { hunt: null, radio: { dialMhz: 14.285 }, activation: act } as unknown as AppSnapshot
  render(
    <>
      <PotaSotaView snap={snap} />
      <ActivationPane act={act} nowUnix={NOW} />
    </>,
  )
  const note = 'US-1234: contacts under 2 calls (KD9TAW 6, KD9TAW/M 4) — POTA counts each call separately'
  expect([...document.querySelectorAll('.pota-act-calls')].map((el) => el.textContent)).toEqual([note])
  expect([...document.querySelectorAll('.act-pane-calls')].map((el) => el.textContent)).toEqual([note])
  // One call at every park: no note anywhere.
  cleanup()
  const one = twoFer()
  render(
    <>
      <PotaSotaView snap={{ ...snap, activation: one } as unknown as AppSnapshot} />
      <ActivationPane act={one} nowUnix={NOW} />
    </>,
  )
  expect(document.querySelectorAll('.pota-act-calls, .act-pane-calls')).toHaveLength(0)
})

it('lists the activation’s own contacts: the Logbook asked for its first park from 0000Z of the start day', async () => {
  rows = [qso('K2NEW', NOW - 60, 'US-0001,US-0002'), qso('K1OLD', START + 60)]
  render(<ActivationPane act={twoFer()} nowUnix={NOW} />)
  await waitFor(() => expect(document.querySelectorAll('.act-pane-row:not(.empty)')).toHaveLength(2))
  const page = asked.find((q) => q.kind === 'page') as Extract<LogQuestion, { kind: 'page' }>
  // Across 0000Z: from the start day's 0000Z, so both days' contacts are in it.
  expect(page.query.filter).toEqual({ myRef: 'US-1234', fromUnix: Math.floor(START / DAY) * DAY })
  expect(page.query.sort).toBe('time')
  expect(page.query.asc).toBe(false)
  expect(page.limit).toBe(ACTIVATION_ROWS)
  const listed = [...document.querySelectorAll('.act-pane-row')].map((r) =>
    [...r.querySelectorAll('span')].map((s) => s.textContent).join('|'),
  )
  // Newest first as the answer gives it; a hunter's two-fer names both their parks.
  expect(listed).toEqual(['00:19Z|K2NEW|20m SSB|US-0001 US-0002', '23:31Z|K1OLD|20m SSB|'])
  // On the park 50 minutes; 0000Z of the next day is 23 h 40 m away.
  expect(screen.getByText(t('activation.pane.running', { span: '50 m' }))).toBeTruthy()
  expect(screen.getByText(t('activation.pane.midnight', { span: '23 h 40 m' }))).toBeTruthy()
  // The rate over those two: one more contact in the 49 minutes since the older one.
  expect(document.querySelector('.act-pane-rate')?.textContent).toContain(
    t('activation.pane.rate', { rate: Math.round(3600 / (49 * 60)), count: 2 }),
  )
})

it('with no activation running it says so and asks the log nothing', async () => {
  render(<ActivationPane act={null} />)
  expect(screen.getByText(t('activation.pane.none'))).toBeTruthy()
  await new Promise((r) => setTimeout(r, 20))
  expect(asked).toHaveLength(0)
})

it('as a box: the window lends it the activation; without one, its one line', () => {
  const ctx = (activation?: PaneContext['activation']) => ({ activation }) as unknown as PaneContext
  const { unmount } = render(<PaneBody pane="activation" ctx={ctx()} />)
  expect(screen.getByText(t('connect.pane.activation.basic'))).toBeTruthy()
  unmount()
  render(<PaneBody pane="activation" ctx={ctx({ act: twoFer() })} />)
  expect(paneParks()).toHaveLength(2)
  // It holds no button of any kind: nothing in it sends, starts or stops anything.
  expect(document.querySelectorAll('.act-pane button')).toHaveLength(0)
})
