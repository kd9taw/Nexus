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
const page = (): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
  collection: 'ota', offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [], meta: { capturedAgeMs: 0, source: structuredClone(fixture) } })
afterEach(() => { cleanup(); vi.restoreAllMocks() })

function station(capabilities: string[]) {
  const sent: Record<string, any>[] = []
  // A still clock: the first poll leaves and the state stays current with no heartbeat after it.
  const client = new OperationClient(s => sent.push(JSON.parse(s)), true, () => 1000, { read: () => null, write: () => {} }, 4)
  client.open()
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  reply({ stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: ['log.manual'], txArmed: false,
    transmitEpoch: null, controls: { context: { radioId: 1, radioConnection: null, ampConnection: null, ampReadSequence: null }, capabilities } })
  const invoke = vi.fn(async (): Promise<unknown> => page())
  const source = new RemoteCollections({ invoke, supports: () => true, getPhase: () => 'ready' } as unknown as ApplicationClient)
  const onHunt = vi.fn()
  render(<StationDataContext.Provider value={true}><RemoteOperationsContext.Provider value={client}>
    <RemoteCollectionsContext.Provider value={source}><RemoteOta snap={snap} onHunt={onHunt} /></RemoteCollectionsContext.Provider>
  </RemoteOperationsContext.Provider></StationDataContext.Provider>)
  const changes = () => sent.filter(m => m.request.type === 'logChange').map(m => m.request)
  return { client, reply, invoke, onHunt, changes }
}
const huntButton = (call: string) => screen.findByRole('button', { name: t('ota.hunt.button.aria', { call }) })

it('offers no remote hunt or hunt clear unless the station offers hunting', async () => {
  const test = station(['logEdit'])
  await screen.findByText('Logged park')
  expect(screen.queryByRole('button', { name: t('ota.hunt.button.aria', { call: 'W1AW' }) })).toBeNull()
  expect(screen.queryByRole('button', { name: t('ota.hunt.clear') })).toBeNull()
  test.client.disconnected()
})

it('tags the hunt at the station first, and only then hands the spot to the tune path', async () => {
  const test = station(['otaHunt'])
  fireEvent.click(await huntButton('W1AW'))
  await waitFor(() => expect(test.changes()).toHaveLength(1))
  const [request] = test.changes()
  expect(request.change).toEqual({ kind: 'hunt', call: 'W1AW', program: 'POTA', reference: 'US-0002' })
  expect(test.onHunt).not.toHaveBeenCalled()
  const reads = test.invoke.mock.calls.length
  await act(async () => test.reply({ operation: 'logChange', operationId: request.requestId, outcome: 'applied', evidence: 'stationState' }))
  await waitFor(() => expect(test.onHunt).toHaveBeenCalledTimes(1))
  expect(test.onHunt.mock.calls[0][0]).toMatchObject({ call: 'W1AW', program: 'POTA', reference: 'US-0002', freqMhz: 14.2 })
  await waitFor(() => expect(test.invoke.mock.calls.length).toBeGreaterThan(reads))
  test.client.disconnected()
})

it('never tunes for a hunt the station refused', async () => {
  const test = station(['otaHunt'])
  fireEvent.click(await huntButton('W1AW'))
  await waitFor(() => expect(test.changes()).toHaveLength(1))
  const [request] = test.changes()
  await act(async () => test.reply({ operation: 'logChange', operationId: request.requestId, outcome: 'rejected', reason: 'invalidChange' }))
  await new Promise(resolve => setTimeout(resolve, 50))
  expect(test.onHunt).not.toHaveBeenCalled()
  test.client.disconnected()
})

it('clears the station hunt from the banner when the station offers hunting', async () => {
  const test = station(['otaHunt'])
  fireEvent.click(await screen.findByRole('button', { name: t('ota.hunt.clear') }))
  await waitFor(() => expect(test.changes()).toHaveLength(1))
  expect(test.changes()[0].change).toEqual({ kind: 'clearHunt' })
  test.client.disconnected()
})
