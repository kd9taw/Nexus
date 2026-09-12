// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { AwardsJourney } from '../components/AwardsJourney'
import { StatsView } from '../components/StatsView'
import { getAwards, getConfirmationDiagnostics, getLog, getLogStats, uploadLotwReport } from '../api'
import type { DiagnosticsReport } from '../types'
import fixture from './__fixtures__/insights.json'

vi.mock('../api', () => ({
  getAwards: vi.fn(async () => fixture.awards),
  getConfirmationDiagnostics: vi.fn(async (): Promise<DiagnosticsReport | null> => null),
  getLog: vi.fn(async () => []), getLogStats: vi.fn(async () => fixture.geography),
  getJourney: vi.fn(async () => { throw new Error('unsupported') }),
  uploadLotwReport: vi.fn(), qrzPushQso: vi.fn(), clublogPushQso: vi.fn(), eqslPushQso: vi.fn(),
}))
afterEach(() => { cleanup(); vi.clearAllMocks(); localStorage.clear() })

it('renders the existing official award cards and chase filters from a remote summary without desktop side effects', async () => {
  render(<AwardsJourney showGamification observation={fixture.awards} />)
  await screen.findByText(/2301/)
  expect(getAwards).not.toHaveBeenCalled()
  expect(getConfirmationDiagnostics).not.toHaveBeenCalled()
  expect(getLog).not.toHaveBeenCalled()
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
  expect(getLogStats).not.toHaveBeenCalled()
  expect(container.querySelectorAll('.stats-bar-fill').length).toBeGreaterThan(5)
  expect(screen.getByText('2012')).toBeTruthy()
})

it('preserves the native award and statistics read paths', async () => {
  const view = render(<AwardsJourney showGamification={false} />)
  await waitFor(() => expect(getAwards).toHaveBeenCalledOnce())
  expect(getConfirmationDiagnostics).toHaveBeenCalledOnce()
  expect(getLog).toHaveBeenCalledOnce()
  view.unmount()
  render(<StatsView />)
  await waitFor(() => expect(getLogStats).toHaveBeenCalledOnce())
  expect(getLog).toHaveBeenCalledTimes(2)
})

it('retains the native confirmation upload action and never renders it for observation', async () => {
  const report = { diagnoses: [{ index: 0, award: 'DXCC', status: 'actionable', reasons: [{ code: 'R1', confidence: 'high', explanation: 'Upload needed', action: { kind: 'uploadToLotw' } }] }],
    buckets: [{ kind: 'uploadToLotw', count: 1, qsoIndices: [0] }], oneAway: [], waitingOnPartner: 0, pendingLag: 0 }
  vi.mocked(getConfirmationDiagnostics).mockResolvedValue(report)
  vi.mocked(uploadLotwReport).mockResolvedValue({ outcome: 'pending', dispatched: 1 } as Awaited<ReturnType<typeof uploadLotwReport>>)
  const view = render(<AwardsJourney showGamification={false} />)
  await waitFor(() => expect(view.container.querySelector('.conf-btn-bulk')).toBeTruthy())
  fireEvent.click(view.container.querySelector('.conf-btn-bulk')!)
  await waitFor(() => expect(uploadLotwReport).toHaveBeenCalledWith([0]))
  view.rerender(<AwardsJourney showGamification={false} observation={fixture.awards} />)
  expect(view.container.querySelector('.conf-btn')).toBeNull()
  expect(uploadLotwReport).toHaveBeenCalledOnce()
  vi.mocked(getConfirmationDiagnostics).mockResolvedValue(null as unknown as DiagnosticsReport)
})
