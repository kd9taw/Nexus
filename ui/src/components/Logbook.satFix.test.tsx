// @vitest-environment jsdom
//
// #163: the Logbook offers the past satellite QSOs that went to LoTW with the wrong band as ONE
// batch to re-upload corrected. The offer shows only when there are some, says how many, and does
// nothing until the operator confirms; cancelling sends nothing.
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { t } from '../i18n'
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

const fixes = vi.hoisted(() => ({ ids: [] as string[] }))
const confirm = vi.hoisted(() => ({ yes: true }))
vi.mock('../confirm', () => ({ confirmDialog: vi.fn(async () => confirm.yes) }))
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
    lotwSatBandFixes: vi.fn(async () => fixes.ids),
    lotwFixSatBands: vi.fn(async (ids: string[]) => ({ dispatched: ids.length, outcome: 'pending', detail: null })),
  }
})

import { Logbook } from './Logbook'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  fixes.ids = []
  confirm.yes = true
})

const label = (n: number) => t('logbook.lotw.satFix.label', { count: n })

it('offers nothing when no satellite QSO went to LoTW with the wrong band', async () => {
  render(<Logbook defaultBand="20m" defaultFreqMhz={14.285} defaultMode="SSB" />)
  await waitFor(() => expect(api.lotwSatBandFixes).toHaveBeenCalled())
  expect(screen.queryByRole('button', { name: /^Fix satellite uploads/ })).toBeNull()
})

it('offers them as one batch and re-uploads exactly those, only once confirmed', async () => {
  fixes.ids = ['a1', 'b2', 'c3']
  render(<Logbook defaultBand="20m" defaultFreqMhz={14.285} defaultMode="SSB" />)
  const button = await screen.findByRole('button', { name: label(3) })
  fireEvent.click(button)
  await waitFor(() => expect(api.lotwFixSatBands).toHaveBeenCalledWith(['a1', 'b2', 'c3']))
  expect(api.lotwFixSatBands).toHaveBeenCalledTimes(1)
})

it('sends nothing when the operator cancels', async () => {
  fixes.ids = ['a1']
  confirm.yes = false
  render(<Logbook defaultBand="20m" defaultFreqMhz={14.285} defaultMode="SSB" />)
  fireEvent.click(await screen.findByRole('button', { name: label(1) }))
  await new Promise((r) => setTimeout(r, 20))
  expect(api.lotwFixSatBands).not.toHaveBeenCalled()
})
