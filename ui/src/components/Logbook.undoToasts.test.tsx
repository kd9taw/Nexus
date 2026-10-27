// @vitest-environment jsdom
//
// THE UNDO ON THE IMPORT AND PURGE TOASTS UNDOES THAT IMPORT, THAT PURGE — the run the import or
// the purge handed back — and nothing else. Asking for "the latest import" or "the latest change
// of the operator's" at click time named whatever was logged in between: a contact a companion
// program logged a moment after the import (the same import path), or one the FT sequencer logged
// after the purge. That contact was taken out, the import or the purge stayed, and the toast said
// "undone".
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, waitFor, fireEvent, cleanup, screen } from '@testing-library/react'
import { Logbook } from './Logbook'
import { Toasts } from './Toasts'
import { t } from '../i18n'
import * as api from '../api'
import { dismissToast, subscribeToasts, type Toast } from '../toast'
import type { LogQuestion } from '../features/logAnswers'
import type { LoggedQso } from '../types'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  const undone = { changed: 3, run: 'undo-run', kept: [], held: [], gone: 0, alreadyThere: 0, changedSince: 0, duplicate: 0, unknownKind: 0, changedSinceCalls: [], duplicateCalls: [], owedCalls: [], owedAt: [] }
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    previewImport: vi.fn(async () => ({
      format: 'adif', template: null, records: 3, contacts: 3, firstUnix: null, lastUnix: null,
      new: 3, duplicates: 0, updates: 0, notes: [], coverage: [], unmapped: [],
    })),
    importLog: vi.fn(async () => ({
      stats: { added: 3, skipped: 0, updated: 0, total: 6, run: 'import-run' },
      report: {
        format: 'adif', template: null, records: 3, contacts: 3, added: 3, duplicates: 0, updated: 0,
        newOnly: false, fields: [], unmapped: [], notes: [], accounted: true,
      },
    })),
    purgeLog: vi.fn(async () => ({ removed: 3, run: 'purge-run' })),
    undoHistoryRun: vi.fn(async () => undone),
    exportGeneralLog: noop(), saveTextToDownloads: noop(), deleteQsoById: noop(),
    editQsoById: vi.fn(async () => ({})), logOperators: vi.fn(async () => [] as string[]),
    exportLogForOperator: noop(), logActivations: vi.fn(async () => []), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: vi.fn(async () => ({})), qrzLookup: noop(), markQslSentById: noop(), markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})

const contact = (call: string, minute: number): LoggedQso =>
  ({
    id: `id-${call}`, call, grid: null, band: '40m', freqMhz: 7.074, mode: 'FT8',
    rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
    country: null, whenUnix: Math.floor(Date.UTC(2026, 8, 14, 1, minute) / 1000),
    confirmed: false, awardConfirmed: false,
  }) as LoggedQso
const LOG = [contact('W1AAA', 1), contact('W2BBB', 2), contact('W3CCC', 3)]

let toasts: Toast[] = []
let unsubscribe = () => {}
beforeEach(() => {
  unsubscribe = subscribeToasts((all) => { toasts = all })
})
afterEach(() => {
  // The bus is app-wide: a toast one case raised must not be the one the next case presses.
  for (const x of toasts) dismissToast(x.id)
  unsubscribe()
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

async function openLogbook() {
  engineLog.mockResolvedValue(LOG)
  render(<Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />)
  await waitFor(() => expect(document.querySelectorAll('.logbook-row:not(.head)').length).toBe(3))
}

const toastSaying = (message: string) => toasts.find((x) => x.message.startsWith(message))

describe('the Undo on the import and purge toasts', () => {
  it("undoes the import's own run, whatever was logged since", async () => {
    await openLogbook()
    fireEvent.change(document.querySelector('.log-actions input[type="file"]') as HTMLInputElement, {
      target: { files: [new File(['<CALL:5>N4GHI<EOR>'], 'log.adi')] },
    })
    // The preview first; the import is the operator's pick.
    fireEvent.click(await screen.findByRole('button', { name: t('logbook.import.preview.import') }))
    await waitFor(() => expect(toastSaying(t('logbook.import.imported', { count: 3 }))).toBeTruthy())
    const toast = toastSaying(t('logbook.import.imported', { count: 3 }))!
    expect(toast.actionLabel).toBe(t('logbook.undo'))
    toast.action?.()
    await waitFor(() => expect(api.undoHistoryRun).toHaveBeenCalled())
    expect(vi.mocked(api.undoHistoryRun).mock.calls).toEqual([['import-run']])
    await waitFor(() => expect(toastSaying(t('logbook.import.undone'))).toBeTruthy())
  })

  it("undoes the purge's own run, whatever was logged since", async () => {
    await openLogbook()
    fireEvent.click(screen.getByRole('button', { name: t('logbook.purge.label') }))
    fireEvent.change(document.querySelector('.purge-field input') as HTMLInputElement, {
      target: { value: 'DELETE' },
    })
    fireEvent.click(screen.getByRole('button', { name: t('logbook.purge.confirm', { count: 3 }) }))
    await waitFor(() => expect(toastSaying(t('logbook.purge.done', { count: 3 }))).toBeTruthy())
    const toast = toastSaying(t('logbook.purge.done', { count: 3 }))!
    expect(toast.actionLabel).toBe(t('logbook.undo'))
    toast.action?.()
    await waitFor(() => expect(api.undoHistoryRun).toHaveBeenCalled())
    expect(vi.mocked(api.undoHistoryRun).mock.calls).toEqual([['purge-run']])
  })

  it('keeps the purge toast and its Undo when the undo is refused while transmitting', async () => {
    const refusal = 'Stop transmitting first: undoing this many contacts holds up the radio while it is made.'
    vi.mocked(api.undoHistoryRun).mockRejectedValueOnce(refusal)
    engineLog.mockResolvedValue(LOG)
    render(
      <>
        <Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />
        <Toasts />
      </>,
    )
    await waitFor(() => expect(document.querySelectorAll('.logbook-row:not(.head)').length).toBe(3))
    fireEvent.click(screen.getByRole('button', { name: t('logbook.purge.label') }))
    fireEvent.change(document.querySelector('.purge-field input') as HTMLInputElement, {
      target: { value: 'DELETE' },
    })
    fireEvent.click(screen.getByRole('button', { name: t('logbook.purge.confirm', { count: 3 }) }))
    const purged = t('logbook.purge.done', { count: 3 })
    await waitFor(() => expect(document.querySelector('.ui-toast-action')).not.toBeNull())
    const undo = () => document.querySelector('.ui-toast-action') as HTMLButtonElement

    fireEvent.click(undo())
    // Refused: the toast stays, saying why, with its Undo.
    const why = await screen.findByRole('alert')
    expect(why.textContent).toContain(refusal)
    expect(toastSaying(purged)?.note).toContain(refusal)
    expect(undo()).not.toBeNull()

    // Transmit off: the same Undo, pressed again, undoes that purge.
    fireEvent.click(undo())
    await waitFor(() => expect(toastSaying(t('logbook.purge.undone'))).toBeTruthy())
    expect(vi.mocked(api.undoHistoryRun).mock.calls).toEqual([['purge-run'], ['purge-run']])
    await waitFor(() => expect(toastSaying(purged)).toBeUndefined())
  })
})
