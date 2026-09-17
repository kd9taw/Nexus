// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { RemoteResponsiveness } from './RemoteResponsiveness'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { RemoteWheelTuningContext } from './wheel-tuning-context'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { scriptedStation } from './__fixtures__/scripted-station'
import { t } from '../i18n'

type Station = ReturnType<typeof scriptedStation>
const open: Station[] = []
afterEach(() => { cleanup(); open.splice(0).forEach(s => s.close()); vi.restoreAllMocks() })
async function tick(ms: number) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

function mount(station: Station) {
  const collections = new RemoteCollections(station.application)
  return render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={station.operations}><RemoteCollectionsContext.Provider value={collections}>
      <RemoteWheelTuningContext.Provider value={station.tuning}><RemoteResponsiveness /></RemoteWheelTuningContext.Provider>
    </RemoteCollectionsContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
}

it('attaches the probe only while mounted, runs the check on the button, and copies the report as text', async () => {
  const station = scriptedStation(100)
  open.push(station)
  const writeText = vi.fn(async (_text: string) => {})
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
  expect(station.operations.probe).toBeNull()
  const view = mount(station)
  expect(station.operations.probe, 'mounting the panel is what attaches the probe').not.toBeNull()
  await tick(1600)
  const run = screen.getByRole('button', { name: t('remote.responsiveness.run') })
  expect(run).not.toHaveProperty('disabled', true)
  expect(screen.getByRole('status').textContent).toBe(t('remote.responsiveness.hint'))
  fireEvent.click(run)
  await tick(10)
  expect(run).toHaveProperty('disabled', true)
  expect(screen.getByRole('status').textContent).toBe(t('remote.responsiveness.phase.tuning'))
  await tick(45_000)
  expect(run).toHaveProperty('disabled', false)
  const report = view.container.querySelector<HTMLTextAreaElement>('textarea.remote-responsiveness-report')!
  expect(report.value).toContain('Link round trip: 100 ms typical, 100 ms worst')
  expect(report.value).toContain('Within budget:')
  expect(report.value, 'operator prose, not internals').not.toMatch(/p95|\.ts|operationId/)
  fireEvent.click(screen.getByRole('button', { name: t('remote.responsiveness.copy') }))
  await tick(10)
  expect(writeText).toHaveBeenCalledWith(report.value)
  expect(screen.getByRole('button', { name: t('remote.responsiveness.copied') })).toBeTruthy()
  view.unmount()
  expect(station.operations.probe, 'unmounting detaches it').toBeNull()
  expect(station.tuning.probe).toBeNull()
})

it('offers nothing to a browser without station control', async () => {
  const station = scriptedStation(100)
  open.push(station)
  render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={null}><RemoteCollectionsContext.Provider value={new RemoteCollections(station.application)}>
      <RemoteWheelTuningContext.Provider value={null}><RemoteResponsiveness /></RemoteWheelTuningContext.Provider>
    </RemoteCollectionsContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  expect(station.operations.probe).toBeNull()
  expect(screen.getByRole('button', { name: t('remote.responsiveness.run') })).toHaveProperty('disabled', true)
  expect(screen.getByRole('status').textContent).toBe(t('remote.responsiveness.needControl'))
})
