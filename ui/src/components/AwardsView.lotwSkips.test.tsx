// @vitest-environment jsdom
//
// The Awards upload buttons sign a chosen set of contacts through the SAME LoTW batch as the
// Logbook's button, so a contact edited or deleted while TQSL signs is skipped here too — the
// case the batch's delete test models is exactly this one. Its counts reach the operator the
// same way: a toast beside the panel's own result line.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { AwardsJourney } from './AwardsJourney'
import { Toasts } from './Toasts'
import { dismissToast, subscribeToasts, type Toast } from '../toast'
import { t } from '../i18n'
import { getConfirmationDiagnostics, uploadLotwReport } from '../api'
import type { DiagnosticsReport } from '../types'
import fixture from '../remote-web/__fixtures__/insights.json'

vi.mock('../api', () => ({
  getAwards: vi.fn(async () => fixture.awards),
  getConfirmationDiagnostics: vi.fn(async (): Promise<DiagnosticsReport | null> => null),
  getLog: vi.fn(async () => []), getLogStats: vi.fn(async () => fixture.geography),
  getLogDelta: vi.fn(async () => ({ revision: 1, full: true, rows: [] })),
  getJourney: vi.fn(async () => { throw new Error('unsupported') }),
  uploadLotwReport: vi.fn(), qrzPushQso: vi.fn(), clublogPushQso: vi.fn(), eqslPushQso: vi.fn(),
}))

/** One contact the diagnostics say LoTW needs — so the panel offers its bulk upload. */
const report = {
  diagnoses: [{ index: 0, award: 'DXCC', status: 'actionable', reasons: [{ code: 'R1', confidence: 'high', explanation: 'Upload needed', action: { kind: 'uploadToLotw' } }] }],
  buckets: [{ kind: 'uploadToLotw', count: 1, qsoIndices: [0] }], oneAway: [], waitingOnPartner: 0, pendingLag: 0,
} as unknown as DiagnosticsReport

let toasts: Toast[] = []
let unsubscribe: () => void = () => {}
beforeEach(() => {
  unsubscribe = subscribeToasts((now) => {
    toasts = now
  })
  vi.mocked(getConfirmationDiagnostics).mockResolvedValue(report)
})
afterEach(() => {
  cleanup()
  for (const toast of toasts) dismissToast(toast.id)
  unsubscribe()
  vi.clearAllMocks()
  localStorage.clear()
})

it('an Awards upload that skipped contacts shows the same counts', async () => {
  vi.mocked(uploadLotwReport).mockResolvedValue({ dispatched: 1, outcome: 'pending', skippedEdited: 0, skippedDeleted: 1 })
  const view = render(
    <>
      <AwardsJourney showGamification={false} />
      <Toasts />
    </>,
  )
  await waitFor(() => expect(view.container.querySelector('.conf-btn-bulk')).toBeTruthy())
  fireEvent.click(view.container.querySelector('.conf-btn-bulk')!)
  // The panel's own result line is the control: the upload ran and was answered.
  await screen.findByText(t('awards.upload.pending', { count: 1 }))
  const said = t('logbook.lotw.skipped.changed', { count: 1 }) + t('logbook.lotw.skipped.deleted', { count: 1 })
  expect(await screen.findByText(said)).toBeTruthy()
  expect(said).toMatch(/^1 QSO changed while TQSL was signing this upload/)
})

it('an Awards upload that recorded every contact adds nothing', async () => {
  vi.mocked(uploadLotwReport).mockResolvedValue({ dispatched: 1, outcome: 'pending' })
  const view = render(
    <>
      <AwardsJourney showGamification={false} />
      <Toasts />
    </>,
  )
  await waitFor(() => expect(view.container.querySelector('.conf-btn-bulk')).toBeTruthy())
  fireEvent.click(view.container.querySelector('.conf-btn-bulk')!)
  await screen.findByText(t('awards.upload.pending', { count: 1 }))
  expect(toasts.filter((x) => x.message.includes('TQSL was signing'))).toHaveLength(0)
})
