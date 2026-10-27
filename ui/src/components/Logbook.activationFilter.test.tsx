// @vitest-environment jsdom
//
// "This activation" in the Logbook: the same rows, scoped to the activation running now — its first
// park, whole, from 0000Z of the day it started, so a two-fer and a run across 0000Z are all in it.
// The engine answers the filter (its SQL is held to that in tempo-core's `filter_sql` tests); this
// holds the Logbook to asking exactly that question, and to offering it only while one runs.
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { t } from '../i18n'
import type { Activation } from '../types'
import * as api from '../api'
import type { LogQuestion } from '../features/logAnswers'

vi.setConfig({ testTimeout: 15_000 })

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, [])),
    exportGeneralLog: vi.fn(), saveTextToDownloads: vi.fn(),
    deleteQsoById: noop(), importAdif: noop(), editQsoById: vi.fn(async () => ({})),
    logOperators: vi.fn(async () => [] as string[]), exportLogForOperator: vi.fn(),
    logActivations: vi.fn(async () => []), exportLogForActivation: vi.fn(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: vi.fn(async () => ({})), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})

import { Logbook } from './Logbook'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const DAY = 86_400
const START = 1_793_057_400 // 23:30Z
const act: Activation = {
  program: 'POTA',
  reference: 'US-1234,US-5678',
  qsoCount: 3,
  parks: [
    { reference: 'US-1234', count: 3, p2p: 0 },
    { reference: 'US-5678', count: 3, p2p: 0 },
  ],
  startedAt: START,
  utcDay: Math.floor(START / DAY) + 1,
}

/** The filter of the latest page the Logbook asked for. */
const lastPageFilter = () => {
  const pages = vi.mocked(api.askLog).mock.calls.map(([q]) => q).filter((q) => q.kind === 'page')
  return (pages[pages.length - 1] as Extract<LogQuestion, { kind: 'page' }> | undefined)?.query.filter
}

it('filters to the activation’s own contacts, and back to every contact on a second click', async () => {
  render(<Logbook defaultBand="20m" defaultFreqMhz={14.285} defaultMode="SSB" activation={act} />)
  const chip = await screen.findByRole('button', { name: t('logbook.filters.activation.label') })
  expect(chip.getAttribute('aria-pressed')).toBe('false')
  fireEvent.click(chip)
  await waitFor(() => expect(lastPageFilter()).toEqual({ myRef: 'US-1234', fromUnix: Math.floor(START / DAY) * DAY }))
  expect(chip.getAttribute('aria-pressed')).toBe('true')
  fireEvent.click(chip)
  await waitFor(() => expect(lastPageFilter() ?? {}).toEqual({}))
})

it('is offered only while an activation runs', async () => {
  render(<Logbook defaultBand="20m" defaultFreqMhz={14.285} defaultMode="SSB" activation={null} />)
  await screen.findByRole('button', { name: t('logbook.filters.label') })
  expect(screen.queryByRole('button', { name: t('logbook.filters.activation.label') })).toBeNull()
})
