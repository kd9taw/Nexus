// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { RemoteOta } from './RemoteOta'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { RemoteOperationsContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import type { ApplicationClient } from './application-client'
import type { AppSnapshot } from '../types'
import type { QueryPage } from './application-query-protocol'
import { t } from '../i18n'
import fixture from './__fixtures__/ota.json'

const snap = { hunt: null } as AppSnapshot
type Activation = { program: string | null; reference: string | null; qsoCount: number }
const IDLE: Activation = { program: null, reference: null, qsoCount: 0 }
const page = (activation: Activation): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
  collection: 'ota', offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [],
  meta: { capturedAgeMs: 0, source: { ...structuredClone(fixture), activation } } })
afterEach(() => { cleanup(); vi.restoreAllMocks() })

function station(capabilities: string[], activation: Activation = fixture.activation) {
  const sent: Record<string, any>[] = []
  // A still clock: the first poll leaves and the state stays current with no heartbeat after it.
  const client = new OperationClient(s => sent.push(JSON.parse(s)), true, () => 1000, { read: () => null, write: () => {} }, 4)
  client.open()
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  reply({ stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: ['log.manual'], txArmed: false,
    transmitEpoch: null, controls: { context: { radioId: 1, radioConnection: null, ampConnection: null, ampReadSequence: null }, capabilities } })
  const invoke = vi.fn(async (): Promise<unknown> => page(activation))
  const source = new RemoteCollections({ invoke, supports: () => true, getPhase: () => 'ready' } as unknown as ApplicationClient)
  render(<StationDataContext.Provider value={true}><RemoteOperationsContext.Provider value={client}>
    <RemoteCollectionsContext.Provider value={source}><RemoteOta snap={snap} /></RemoteCollectionsContext.Provider>
  </RemoteOperationsContext.Provider></StationDataContext.Provider>)
  const changes = () => sent.filter(m => m.request.type === 'logChange').map(m => m.request)
  return { client, reply, invoke, changes }
}

it('offers no remote activation start or stop unless the station offers activation', async () => {
  const idle = station(['otaHunt'], IDLE)
  await screen.findByText('Logged park')
  expect(screen.queryByRole('button', { name: t('ota.activation.start') })).toBeNull()
  idle.client.disconnected()
  cleanup()
  const active = station(['otaHunt'])
  await screen.findByText('Logged park')
  expect(screen.queryByRole('button', { name: t('ota.activation.stop.label') })).toBeNull()
  active.client.disconnected()
})

it('starts your activation at the station from the board, and re-reads the board once it applies', async () => {
  const test = station(['otaActivation'], IDLE)
  await screen.findByText('Logged park')
  fireEvent.change(document.querySelector('.pota-act-ref')!, { target: { value: 'us-0005' } })
  fireEvent.click(screen.getByRole('button', { name: t('ota.activation.start') }))
  await waitFor(() => expect(test.changes()).toHaveLength(1))
  const [request] = test.changes()
  expect(request.change).toEqual({ kind: 'activation', program: 'POTA', reference: 'US-0005' })
  const reads = test.invoke.mock.calls.length
  await act(async () => test.reply({ operation: 'logChange', operationId: request.requestId, outcome: 'applied', evidence: 'stationState' }))
  await waitFor(() => expect(test.invoke.mock.calls.length).toBeGreaterThan(reads))
  test.client.disconnected()
})

it('ends your activation at the station from the board', async () => {
  const test = station(['otaActivation'])
  fireEvent.click(await screen.findByRole('button', { name: t('ota.activation.stop.label') }))
  await waitFor(() => expect(test.changes()).toHaveLength(1))
  expect(test.changes()[0].change).toEqual({ kind: 'clearActivation' })
  test.client.disconnected()
})
