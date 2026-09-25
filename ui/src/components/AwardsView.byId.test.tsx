// @vitest-environment jsdom
//
// AN AWARDS BUTTON ACTS ON THE CONTACT ITS DIAGNOSIS NAMES — BY ID (SPEC-2 v2 §3, C17a).
//
// The confirmation diagnosis is computed over the log as it stood when the view asked, and names
// each contact by its LOG POSITION and, on the desktop, by its id. A position is only right while
// nothing above it changes: another window deletes a contact above it (a Remote browser, the
// Logbook), and between the diagnosis and the press the same position names the NEIGHBOURING
// contact. The buttons used to act on positions, so a LoTW upload signed — under the operator's
// callsign certificate — a contact nobody diagnosed, and a QRZ push sent the wrong one. They act
// on ids now: the upload sends the report's ids, the push reads the contact by its id at the press.
//
// Here the diagnosis was made over W1AW, K1ABC, N2XYZ, JA1QRZ, VK2AAA; then K1ABC is deleted.
// Position 2 now names JA1QRZ (not N2XYZ), and position 3 names VK2AAA (not JA1QRZ).

import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { AwardsView } from './AwardsView'
import { t } from '../i18n'
import { getConfirmationDiagnostics, qrzPushQso, uploadLotwReport, uploadLotwReportByIds } from '../api'
import { createAskingLogSource } from '../features/askingLogSource'
import { answerFrom, type AnswerTo, type LogQuestion } from '../features/logAnswers'
import { setLogSource } from '../features/logSource'
import type { DiagnosticsReport, LoggedQso } from '../types'
import fixture from '../remote-web/__fixtures__/insights.json'

vi.mock('../api', () => ({
  getAwards: vi.fn(async () => fixture.awards),
  getConfirmationDiagnostics: vi.fn(),
  getJourney: vi.fn(async () => {
    throw new Error('unsupported')
  }),
  uploadLotwReport: vi.fn(async () => ({ dispatched: 1, outcome: 'pending' })),
  uploadLotwReportByIds: vi.fn(async () => ({ dispatched: 1, outcome: 'pending' })),
  qrzPushQso: vi.fn(async () => ({ result: 'ok' })),
  clublogPushQso: vi.fn(async () => ({ result: 'ok' })),
  eqslPushQso: vi.fn(async () => ({ outcome: 'accepted' })),
}))

const row = (id: string, call: string, whenUnix: number) =>
  ({ id, call, band: '20m', freqMhz: 14.074, mode: 'FT8', whenUnix, grid: 'FN31', confirmed: false, awardConfirmed: false }) as unknown as LoggedQso

/** The log the diagnosis was computed over. */
const DIAGNOSED = [row('id-W', 'W1AW', 100), row('id-K', 'K1ABC', 200), row('id-X', 'N2XYZ', 300), row('id-Q', 'JA1QRZ', 400), row('id-V', 'VK2AAA', 500)]
/** …and the log when the operator presses: K1ABC deleted in another window. */
const NOW = DIAGNOSED.filter((r) => r.id !== 'id-K')

const lotwReason = { code: 'R1', confidence: 'high', explanation: 'Never uploaded to LoTW', action: { kind: 'uploadToLotw' } }
const qrzReason = { code: 'R3', confidence: 'high', explanation: 'Not on QRZ', action: { kind: 'uploadToQrz' } }
const named = (index: number) => {
  const r = DIAGNOSED[index]
  return { index, id: r.id!, call: r.call, band: r.band, mode: r.mode, whenUnix: r.whenUnix }
}
/** The desktop's report: N2XYZ owed to LoTW (its bucket too), JA1QRZ owed to QRZ. */
const report = {
  diagnoses: [
    { ...named(2), award: 'DXCC', status: 'actionable', reasons: [lotwReason] },
    { ...named(3), award: 'DXCC', status: 'actionable', reasons: [qrzReason] },
  ],
  buckets: [{ kind: 'uploadToLotw', count: 1, qsoIndices: [2], qsoIds: ['id-X'] }],
  oneAway: [],
  waitingOnPartner: 0,
  pendingLag: 0,
} as unknown as DiagnosticsReport

/** The engine, answering from the log as it is now. */
const asked: LogQuestion[] = []
beforeEach(() => {
  asked.length = 0
  setLogSource(
    createAskingLogSource(async <Q extends LogQuestion>(q: Q) => {
      asked.push(q)
      return answerFrom(NOW, q, 2) as AnswerTo<Q>
    }),
  )
  vi.mocked(getConfirmationDiagnostics).mockResolvedValue(report)
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

const lotwButton = () => screen.findByRole('button', { name: t('awards.conf.uploadToLotw') })
const qrzButton = () => screen.findByRole('button', { name: t('awards.conf.push', { service: 'QRZ' }) })
const bulkButton = () => screen.findByRole('button', { name: t('awards.conf.bucket.upload', { count: 1 }) })

it('FIX: a row\'s LoTW upload signs the contact its diagnosis names, not the one now at its position', async () => {
  render(<AwardsView showGamification={false} />)
  fireEvent.click(await lotwButton())
  await screen.findByText(t('awards.upload.pending', { count: 1 }))
  expect(uploadLotwReport, 'uploaded by position — after the delete, position 2 is JA1QRZ').not.toHaveBeenCalled()
  expect(uploadLotwReportByIds).toHaveBeenCalledTimes(1)
  expect(vi.mocked(uploadLotwReportByIds).mock.calls[0][0]).toEqual(['id-X'])
})

it('FIX: a bucket\'s upload signs the contacts the bucket names, not the ones now at their positions', async () => {
  render(<AwardsView showGamification={false} />)
  fireEvent.click(await bulkButton())
  await screen.findByText(t('awards.upload.pending', { count: 1 }))
  expect(uploadLotwReport, 'uploaded by position').not.toHaveBeenCalled()
  expect(vi.mocked(uploadLotwReportByIds).mock.calls).toEqual([[['id-X']]])
})

it('FIX: a push sends the contact its diagnosis names, read by its id when it is pressed', async () => {
  render(<AwardsView showGamification={false} />)
  fireEvent.click(await qrzButton())
  await waitFor(() => expect(qrzPushQso).toHaveBeenCalledTimes(1))
  const pushed = vi.mocked(qrzPushQso).mock.calls[0][0]
  expect(pushed.call, 'pushed by position — after the delete, position 3 is VK2AAA').toBe('JA1QRZ')
  expect(pushed).toEqual(NOW.find((r) => r.id === 'id-Q'))
  await screen.findByText(t('awards.push.qrz.ok', { call: 'JA1QRZ' }))
  expect(asked.filter((q) => q.kind === 'rowsAt'), 'a position asked for').toEqual([])
})

it('FIX: a push whose contact was deleted meanwhile pushes nothing', async () => {
  vi.mocked(getConfirmationDiagnostics).mockResolvedValue({
    ...report,
    diagnoses: [{ ...named(1), award: 'DXCC', status: 'actionable', reasons: [qrzReason] }],
    buckets: [],
  } as unknown as DiagnosticsReport)
  render(<AwardsView showGamification={false} />)
  fireEvent.click(await qrzButton())
  await screen.findByText(t('awards.push.noQso'))
  expect(qrzPushQso, 'K1ABC is gone; position 1 is N2XYZ now').not.toHaveBeenCalled()
})

it('FIX: a contact the report does not name by id offers no button — never an action by position', async () => {
  // Every contact the desktop's report names carries its id; a report without them (an older
  // engine's) must degrade to the guidance chips rather than act on positions.
  const { qsoIds: _ids, ...bucket } = report.buckets[0]
  vi.mocked(getConfirmationDiagnostics).mockResolvedValue({
    ...report,
    diagnoses: report.diagnoses.map(({ id: _id, ...d }) => d),
    buckets: [bucket],
  } as unknown as DiagnosticsReport)
  const view = render(<AwardsView showGamification={false} />)
  await screen.findByText('Not on QRZ')
  // The chips are the control: the rows are drawn, as guidance.
  expect(screen.getByText(t('awards.conf.uploadToLotw')).tagName).toBe('SPAN')
  expect(screen.getByText(t('awards.conf.push', { service: 'QRZ' })).tagName).toBe('SPAN')
  expect(view.container.querySelector('.conf-btn'), 'a button acting on a position').toBeNull()
})
