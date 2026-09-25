// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { AwardsJourney } from '../components/AwardsJourney'
import { StatsView } from '../components/StatsView'
import { askLog, getAwards, getConfirmationDiagnostics, getLog, getLogDelta, getLogStats, uploadLotwReport, uploadLotwReportByIds } from '../api'
import type { LogQuestion } from '../features/logAnswers'
import type { DiagnosticsReport } from '../types'
import fixture from './__fixtures__/insights.json'

vi.mock('../api', () => ({
  getAwards: vi.fn(async () => fixture.awards),
  getConfirmationDiagnostics: vi.fn(async (): Promise<DiagnosticsReport | null> => null),
  getLog: vi.fn(async () => []), getLogStats: vi.fn(async () => fixture.geography),
  getLogDelta: vi.fn(async () => ({ revision: 1, full: true, rows: [] })),
  // The engine, over an empty log.
  askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, [])),
  getJourney: vi.fn(async () => { throw new Error('unsupported') }),
  uploadLotwReport: vi.fn(), uploadLotwReportByIds: vi.fn(), qrzPushQso: vi.fn(), clublogPushQso: vi.fn(), eqslPushQso: vi.fn(),
}))
afterEach(() => { cleanup(); vi.clearAllMocks(); localStorage.clear() })

it('renders the existing official award cards and chase filters from a remote summary without desktop side effects', async () => {
  render(<AwardsJourney showGamification observation={fixture.awards} />)
  await screen.findByText(/2301/)
  expect(getAwards).not.toHaveBeenCalled()
  expect(getConfirmationDiagnostics).not.toHaveBeenCalled()
  expect(getLog).not.toHaveBeenCalled()
  expect(getLogDelta).not.toHaveBeenCalled()
  expect(askLog).not.toHaveBeenCalled()
  const journey = screen.getByRole('tab', { name: 'Journey' }) as HTMLButtonElement
  expect(journey.disabled).toBe(true)
  expect(screen.getByRole('tab', { name: 'Official Awards' }).getAttribute('aria-selected')).toBe('true')
  expect(screen.getByText('Japan')).toBeTruthy()
  fireEvent.change(screen.getByRole('textbox', { name: /filter/i }), { target: { value: 'Germany' } })
  expect(screen.queryByText('Japan')).toBeNull()
})

it('renders full-log Statistics using the supplied totals and the existing charts', async () => {
  const { container } = render(<StatsView observation={{ statistics: fixture.statistics, geography: fixture.geography }} />)
  expect((await screen.findAllByText('2301')).length).toBeGreaterThan(0)
  expect(getLog).not.toHaveBeenCalled()
  expect(getLogDelta).not.toHaveBeenCalled()
  expect(askLog).not.toHaveBeenCalled()
  expect(getLogStats).not.toHaveBeenCalled()
  expect(container.querySelectorAll('.stats-bar-fill').length).toBeGreaterThan(5)
  expect(screen.getByText('2012')).toBeTruthy()
})

it('preserves the native award and statistics read paths', async () => {
  const view = render(<AwardsJourney showGamification={false} />)
  await waitFor(() => expect(getAwards).toHaveBeenCalledOnce())
  expect(getConfirmationDiagnostics).toHaveBeenCalledOnce()
  // Neither view holds the log (SPEC-2 v3 C17b): Awards asks the engine about a contact only when
  // a button that names one is pressed — nothing here — and Statistics asks for its roll-up.
  await new Promise((r) => setTimeout(r, 30))
  expect(askLog).not.toHaveBeenCalled()
  view.unmount()
  render(<StatsView />)
  await waitFor(() => expect(getLogStats).toHaveBeenCalledOnce())
  await waitFor(() => expect(askLog).toHaveBeenCalledWith({ kind: 'statistics' }))
  expect(getLogDelta).not.toHaveBeenCalled()
  expect(getLog).not.toHaveBeenCalled()
})

it('renders station diagnostics beside an observed summary with every action as guidance, not a button', async () => {
  const report: DiagnosticsReport = {
    diagnoses: [
      { index: 0, award: 'DXCC/WAS', status: 'needsAction', reasons: [{ code: 'r3', confidence: 'confident', explanation: 'K1ABC is confirmed on a non-award source only.', action: { kind: 'uploadToLotw' } }] },
      { index: 1, award: 'DXCC/WAS', status: 'needsAction', reasons: [{ code: 'r1', confidence: 'confident', explanation: 'Never pushed to QRZ.', action: { kind: 'uploadToQrz' } }] },
      { index: 2, award: 'DXCC/WAS', status: 'needsAction', reasons: [{ code: 'r9', confidence: 'likely', explanation: 'ClubLog sign-in expired.', action: { kind: 'reauthenticate', source: 'ClubLog' } }] },
    ],
    buckets: [{ kind: 'Upload to LoTW', count: 12, qsoIndices: [] }], oneAway: [{ entity: 'Japan', bands: ['20m'], newEntity: true }], waitingOnPartner: 0, pendingLag: 3 }
  const onOpenSettings = vi.fn()
  const view = render(<AwardsJourney showGamification={false} observation={fixture.awards} diagnostics={report} onOpenSettings={onOpenSettings} />)
  await screen.findByText('K1ABC is confirmed on a non-award source only.')
  expect(view.container.querySelectorAll('.conf-row')).toHaveLength(3)
  expect(view.container.querySelector('.conf-panel button')).toBeNull()
  expect([...view.container.querySelectorAll('.conf-act')].map(e => e.textContent)).toHaveLength(3)
  expect(screen.getByText('12')).toBeTruthy()
  expect(getConfirmationDiagnostics).not.toHaveBeenCalled()
  expect(getLog).not.toHaveBeenCalled()
  expect(getLogDelta).not.toHaveBeenCalled()
  expect(uploadLotwReport).not.toHaveBeenCalled()
  expect(uploadLotwReportByIds).not.toHaveBeenCalled()
  // Control: the same report on the desktop path — where the engine names each contact by id —
  // renders the live LoTW and sign-in buttons.
  view.unmount()
  vi.mocked(getConfirmationDiagnostics).mockResolvedValueOnce({ ...report, diagnoses: report.diagnoses.map((d, i) => ({ ...d, id: `id-${i}` })) })
  const desktop = render(<AwardsJourney showGamification={false} onOpenSettings={onOpenSettings} />)
  await waitFor(() => expect(desktop.container.querySelectorAll('.conf-panel button').length).toBeGreaterThanOrEqual(2))
})

it('retains the native confirmation upload action and never renders it for observation', async () => {
  const report = { diagnoses: [{ index: 0, id: 'id-0', award: 'DXCC', status: 'actionable', reasons: [{ code: 'R1', confidence: 'high', explanation: 'Upload needed', action: { kind: 'uploadToLotw' } }] }],
    buckets: [{ kind: 'uploadToLotw', count: 1, qsoIndices: [0], qsoIds: ['id-0'] }], oneAway: [], waitingOnPartner: 0, pendingLag: 0 }
  vi.mocked(getConfirmationDiagnostics).mockResolvedValue(report)
  vi.mocked(uploadLotwReportByIds).mockResolvedValue({ outcome: 'pending', dispatched: 1 } as Awaited<ReturnType<typeof uploadLotwReportByIds>>)
  const view = render(<AwardsJourney showGamification={false} />)
  await waitFor(() => expect(view.container.querySelector('.conf-btn-bulk')).toBeTruthy())
  fireEvent.click(view.container.querySelector('.conf-btn-bulk')!)
  await waitFor(() => expect(uploadLotwReportByIds).toHaveBeenCalledWith(['id-0']))
  view.rerender(<AwardsJourney showGamification={false} observation={fixture.awards} />)
  expect(view.container.querySelector('.conf-btn')).toBeNull()
  expect(uploadLotwReportByIds).toHaveBeenCalledOnce()
  expect(uploadLotwReport).not.toHaveBeenCalled()
  vi.mocked(getConfirmationDiagnostics).mockResolvedValue(null as unknown as DiagnosticsReport)
})
