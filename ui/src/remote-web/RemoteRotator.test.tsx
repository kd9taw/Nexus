// @vitest-environment jsdom
// Remote parity batch 1: the rotator from a browser. Pointing and stopping are operator gestures
// only, live while the station advertises `rotator` AND has a rotator configured. Nothing points or
// polls the rotator on mount, and with no collections source (an older station, or one that does
// not offer the v17 heading read) the readout stays "—". The heading itself is pinned separately,
// in RemoteRotatorHeading.test.tsx.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import * as api from '../api'
import { RotorStrip } from '../components/RotorStrip'
import { NeededPanel } from '../components/NeededPanel'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { pendingLogStorage, type ReceiptLock } from './operation-storage'
import type { OperationState } from './operation-protocol'
import type { ControlCapability } from './station-operation'
import type { NeedAlert } from '../types'
import { t } from '../i18n'

let rotatorModel = 1
vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ rotatorModel, rotatorHost: '' })),
  readRotator: vi.fn(async () => 123),
  pointRotator: vi.fn(async () => {}),
  stopRotator: vi.fn(async () => {}),
  stopSatTrack: vi.fn(async () => {}),
  getSatTrackStatus: vi.fn(async () => null),
  getSatTransponder: vi.fn(async () => null),
  setSatTransponder: vi.fn(async () => {}),
  getDeclination: vi.fn(async () => null),
  openQrzPage: vi.fn(async () => {}),
}))

beforeEach(() => { rotatorModel = 1; vi.clearAllMocks() })
afterEach(() => cleanup())

function operations(capabilities: ControlCapability[]) {
  const values = new Map<string, string>(), sent: string[] = []
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const lock: ReceiptLock = async (_k, run) => run()
  const client = new OperationClient(s => sent.push(s), true, () => 1000, pendingLogStorage(() => data, 'station', lock), 3, pendingControlStorage(() => data, 'station', lock))
  client.open()
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities } }
  client.receive({ type: 'operationResponse', requestId: JSON.parse(sent[sent.length - 1]).request.requestId, value: state })
  return client
}

const remote = (capabilities: ControlCapability[], children: ReactNode) => render(
  <StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={operations(capabilities)}>{children}</RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)

const settle = () => act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })
const stopButton = () => screen.queryByTitle(t('rotor.strip.stop.title')) as HTMLButtonElement | null

it('shows a live remote Stop and the point-at slew with the hint and a configured rotator, and never polls or points on mount', async () => {
  const pointAt = vi.fn()
  remote(['rotator'], <RotorStrip targetCall="JA1ABC" onPointAt={pointAt} />)
  await waitFor(() => expect(stopButton()).not.toBeNull())
  await settle()
  expect(stopButton()!.disabled).toBe(false)
  // No heading from this station: the strip says so instead of inventing one.
  expect(screen.getByTitle(t('remote.b1.rotatorNoHeading'))).toBeTruthy()
  expect(api.readRotator).not.toHaveBeenCalled()
  expect(api.getSatTrackStatus).not.toHaveBeenCalled()
  expect(api.stopRotator).not.toHaveBeenCalled()
  expect(pointAt).not.toHaveBeenCalled()
  // Positive control: the explicit gestures send exactly one command each.
  fireEvent.click(stopButton()!)
  await settle()
  expect(api.stopRotator).toHaveBeenCalledTimes(1)
  // A satellite track is a desktop loop the browser cannot stop; the remote Stop never asks.
  expect(api.stopSatTrack).not.toHaveBeenCalled()
  fireEvent.click(screen.getByTitle(t('rotor.strip.pointAt.title', { call: 'JA1ABC' })))
  expect(pointAt).toHaveBeenCalledTimes(1)
  expect(pointAt).toHaveBeenCalledWith('JA1ABC')
})

it('shows no live remote strip without the hint, and nothing at all with no rotator configured', async () => {
  remote(['frequency', 'aprsTuning'], <RotorStrip targetCall="JA1ABC" onPointAt={vi.fn()} />)
  await settle()
  expect(stopButton()).toBeNull()
  expect(screen.getByLabelText(t('remote.rotatorUnavailable'))).toBeTruthy()
  cleanup()
  rotatorModel = 0
  remote(['rotator'], <RotorStrip targetCall="JA1ABC" onPointAt={vi.fn()} />)
  await settle()
  expect(stopButton()).toBeNull()
  expect(screen.queryByLabelText(t('remote.rotatorUnavailable'))).toBeNull()
  expect(api.stopRotator).not.toHaveBeenCalled()
  expect(api.readRotator).not.toHaveBeenCalled()
})

const need = { call: 'JA1ABC', entity: 'Japan', band: '20m', zone: 25, tags: [], priority: 1, headline: 'New one', mode: 'CW', freqMhz: 14.02 } as unknown as NeedAlert
const needed = (onPoint?: (call: string) => void) => (
  <NeededPanel alerts={[need]} bandPlan={[]} selectedCall={null} onQsy={() => {}} onSelect={() => {}} onWork={() => {}} onPoint={onPoint} />
)

it('points from the Needed board with the hint: one azimuth per Go and one call per row arrow, never on mount', async () => {
  const onPoint = vi.fn()
  remote(['rotator'], needed(onPoint))
  await settle()
  const input = screen.getByLabelText(t('needed.rotator.azimuth.label')) as HTMLInputElement
  const go = screen.getByTitle(t('needed.rotator.go.title')) as HTMLButtonElement
  expect(api.readRotator).not.toHaveBeenCalled()
  expect(api.pointRotator).not.toHaveBeenCalled()
  expect(onPoint).not.toHaveBeenCalled()
  fireEvent.change(input, { target: { value: '123.4' } })
  fireEvent.click(go)
  await settle()
  expect(api.pointRotator).toHaveBeenCalledTimes(1)
  // The widget's own normalisation; the transport sends it to the tenth of a degree.
  expect(vi.mocked(api.pointRotator).mock.calls[0][0]).toBeCloseTo(123.4, 6)
  fireEvent.click(screen.getByTitle(t('needed.row.point.title', { call: 'JA1ABC' })))
  expect(onPoint).toHaveBeenCalledTimes(1)
  expect(onPoint).toHaveBeenCalledWith('JA1ABC')
})

it('keeps the Needed board rotator controls away without the hint', async () => {
  const onPoint = vi.fn()
  remote(['workSpot', 'frequency'], needed(onPoint))
  await settle()
  expect(screen.queryByLabelText(t('needed.rotator.azimuth.label'))).toBeNull()
  expect(screen.queryByTitle(t('needed.row.point.title', { call: 'JA1ABC' }))).toBeNull()
  expect(api.readRotator).not.toHaveBeenCalled()
  expect(api.pointRotator).not.toHaveBeenCalled()
})
