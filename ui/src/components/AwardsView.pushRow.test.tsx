// @vitest-environment jsdom
//
// AN AWARDS PUSH SENDS THE CONTACT ITS DIAGNOSIS NAMES (SPEC-2 v3 C17b, V4; by id since C17a).
//
// The confirmation diagnosis names each contact by its id (`QsoDiagnosis.id`), and the per-row
// QRZ / ClubLog / eQSL buttons push the record with that id, read from `LogSource` when the button
// is pressed (a log POSITION can name another contact by then: AwardsView.byId.test.tsx). What must
// not move: the button pushes exactly the contact its diagnosis names, and a push refreshes the
// diagnosis — whose contacts are then the current ones.

import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { AwardsView } from './AwardsView'
import { t } from '../i18n'
import { clublogPushQso, getConfirmationDiagnostics, qrzPushQso } from '../api'
import type { DiagnosticsReport, LoggedQso } from '../types'
import type { LogQuestion } from '../features/logAnswers'
import fixture from '../remote-web/__fixtures__/insights.json'

const engine = vi.hoisted(() => ({ log: [] as unknown[] }))
vi.mock('../api', () => ({
  getAwards: vi.fn(async () => fixture.awards),
  getConfirmationDiagnostics: vi.fn(),
  // The engine answers each question over its log (features/logAnswers.testkit).
  askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, engine.log as LoggedQso[])),
  getJourney: vi.fn(async () => {
    throw new Error('unsupported')
  }),
  uploadLotwReport: vi.fn(),
  qrzPushQso: vi.fn(async () => ({ result: 'ok' })),
  clublogPushQso: vi.fn(async () => ({ result: 'ok' })),
  eqslPushQso: vi.fn(async () => ({ outcome: 'accepted' })),
}))

const row = (call: string, whenUnix: number) =>
  ({ id: `id-${call}`, call, band: '20m', freqMhz: 14.074, mode: 'FT8', whenUnix, grid: 'FN31', confirmed: false, awardConfirmed: false }) as unknown as LoggedQso

/** Two contacts the diagnosis wants pushed: position 2 to QRZ, position 0 to ClubLog. */
const report = {
  diagnoses: [
    { index: 2, id: 'id-N2XYZ', award: 'DXCC', status: 'actionable', reasons: [{ code: 'R3', confidence: 'high', explanation: 'Not on QRZ', action: { kind: 'uploadToQrz' } }] },
    { index: 0, id: 'id-W1AW', award: 'DXCC', status: 'actionable', reasons: [{ code: 'R4', confidence: 'high', explanation: 'Not on ClubLog', action: { kind: 'uploadToClublog' } }] },
  ],
  buckets: [],
  oneAway: [],
  waitingOnPartner: 0,
  pendingLag: 0,
} as unknown as DiagnosticsReport

beforeEach(() => {
  engine.log = [row('W1AW', 100), row('K1ABC', 200), row('N2XYZ', 300)]
  vi.mocked(getConfirmationDiagnostics).mockResolvedValue(report)
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

const pushButton = (service: string) => screen.findByRole('button', { name: t('awards.conf.push', { service }) })

it('each push button sends the contact its diagnosis names', async () => {
  render(<AwardsView showGamification={false} />)
  fireEvent.click(await pushButton('QRZ'))
  await waitFor(() => expect(qrzPushQso).toHaveBeenCalledTimes(1))
  expect(vi.mocked(qrzPushQso).mock.calls[0][0]).toEqual(engine.log[2])
  await screen.findByText(t('awards.push.qrz.ok', { call: 'N2XYZ' }))

  fireEvent.click(await pushButton('ClubLog'))
  await waitFor(() => expect(clublogPushQso).toHaveBeenCalledTimes(1))
  expect(vi.mocked(clublogPushQso).mock.calls[0][0]).toEqual(engine.log[0])
})

it('FIX: a diagnosis re-read after a push can push a contact logged since the view opened', async () => {
  // The view used to look positions up in the copy of the log it loaded when it OPENED, while the
  // diagnosis it re-reads after every push names contacts in the log as it is NOW. A contact
  // logged in between was listed, and its button answered "no QSO". A push now reads its contact
  // by id when it is pressed.
  render(<AwardsView showGamification={false} />)
  await pushButton('QRZ')
  engine.log = [...engine.log, row('JA1ABC', 400)]
  vi.mocked(getConfirmationDiagnostics).mockResolvedValue({
    ...report,
    diagnoses: [{ ...report.diagnoses[0], index: 3, id: 'id-JA1ABC' }],
  } as unknown as DiagnosticsReport)
  fireEvent.click(await pushButton('QRZ')) // pushes N2XYZ (position 2); the diagnosis is re-read
  await waitFor(() => expect(getConfirmationDiagnostics).toHaveBeenCalledTimes(2))
  // The re-read diagnosis has landed when the ClubLog row is GONE — a state change, not a phrase
  // both diagnoses contain.
  await waitFor(() => expect(screen.queryByText('Not on ClubLog')).toBeNull())
  fireEvent.click(await pushButton('QRZ')) // the contact logged since the view opened
  await waitFor(() => expect(qrzPushQso).toHaveBeenCalledTimes(2))
  expect(vi.mocked(qrzPushQso).mock.calls.map((c) => (c[0] as LoggedQso).call)).toEqual(['N2XYZ', 'JA1ABC'])
})
