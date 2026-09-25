// @vitest-environment jsdom
//
// A LoTW upload whose contacts changed while TQSL was signing them. TQSL runs for tens of
// seconds with the log unlocked; a contact edited or deleted in that window is not marked with
// the upload's result (LoTW holds the version that was signed), and the connection log says so
// in counts. Maintainer, 2026-09-23: "Show both on screen." So the upload's report carries the
// counts and the Logbook toasts them — otherwise an "Upload to LoTW" count that does not clear
// reads as an upload that failed.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { Logbook } from './Logbook'
import { Toasts } from './Toasts'
import { dismissToast, subscribeToasts, type Toast } from '../toast'
import { t } from '../i18n'
import type { LoggedQso, UploadReport } from '../types'
import * as api from '../api'
import type { LogQuestion } from '../features/logAnswers'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

/** The log the engine holds: `askLog` answers from it as the engine does (features/logAnswers.testkit). */
const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: vi.fn(() => Promise.resolve({})),
    markQslCardById: vi.fn(() => Promise.resolve({})),
    syncLotwReport: noop(), uploadLotwReport: vi.fn(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})

/** One contact LoTW has never had — so the Logbook offers "Upload to LoTW (1)". Written the way
 *  the station sends it (nulls where Rust has `None`), hence the cast. */
const unsent = [
  {
    call: 'K0ABC', grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
    rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
    country: 'United States', whenUnix: 1_700_000_000,
    confirmed: false, awardConfirmed: false,
    qslRcvd: null, qslSent: null, ota: null, upload: undefined,
  },
] as unknown as LoggedQso[]

let toasts: Toast[] = []
let unsubscribe: () => void = () => {}
beforeEach(() => {
  unsubscribe = subscribeToasts((now) => {
    toasts = now
  })
  engineLog.mockResolvedValue(unsent)
})
afterEach(() => {
  cleanup()
  for (const toast of toasts) dismissToast(toast.id)
  unsubscribe()
  vi.clearAllMocks()
})

/** Upload with TQSL answering `report`, and wait for the upload's own toast to land. */
async function upload(report: UploadReport) {
  vi.mocked(api.uploadLotwReport).mockResolvedValue(report)
  render(
    <>
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />
      <Toasts />
    </>,
  )
  fireEvent.click(await screen.findByRole('button', { name: /Upload to LoTW \(1\)/ }))
  // The outcome toast is the control: the upload ran and the toast column is live.
  await screen.findByText(t('logbook.lotw.upload.pending', { count: report.dispatched }))
}

/** Toasts about contacts that changed while TQSL signed, by the words only they say. */
const skipToasts = () => toasts.filter((x) => x.message.includes('TQSL was signing'))

describe('LoTW upload — contacts that changed while TQSL was signing', () => {
  it('shows the counts: how many changed, how many were edited and how many deleted', async () => {
    await upload({ dispatched: 5, outcome: 'pending', skippedEdited: 2, skippedDeleted: 1 })
    const said =
      t('logbook.lotw.skipped.changed', { count: 3 }) +
      t('logbook.lotw.skipped.edited', { count: 2 }) +
      t('logbook.lotw.skipped.deleted', { count: 1 })
    expect(await screen.findByText(said)).toBeTruthy()
    // The English, read as the operator reads it.
    expect(said).toMatch(/^3 QSOs changed while TQSL was signing this upload/)
    expect(said).toMatch(/2 were edited and are offered again with the next upload/)
    expect(said).toMatch(/1 was deleted from the log/)
    expect(skipToasts()).toHaveLength(1)
  })

  it('says nothing about deletes when only edits were skipped', async () => {
    await upload({ dispatched: 5, outcome: 'pending', skippedEdited: 1, skippedDeleted: 0 })
    await waitFor(() => expect(skipToasts()).toHaveLength(1))
    const said = skipToasts()[0].message
    expect(said).toBe(
      t('logbook.lotw.skipped.changed', { count: 1 }) + t('logbook.lotw.skipped.edited', { count: 1 }),
    )
    expect(said).not.toMatch(/deleted/)
  })

  it('adds nothing when every contact was recorded', async () => {
    await upload({ dispatched: 5, outcome: 'pending', skippedEdited: 0, skippedDeleted: 0 })
    expect(skipToasts()).toHaveLength(0)
  })

  it('adds nothing from a station older than the counts', async () => {
    // An older station's report has no counts at all.
    await upload({ dispatched: 5, outcome: 'pending' })
    expect(skipToasts()).toHaveLength(0)
  })
})
