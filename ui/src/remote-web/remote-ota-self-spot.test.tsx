// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { RemoteOta } from './RemoteOta'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { RemoteOperationsContext, StationDataContext } from '../stationAccess'
import { ConfirmHost } from '../confirm'
import { OperationClient } from './operation-client'
import type { ApplicationClient } from './application-client'
import type { AppSnapshot } from '../types'
import type { QueryPage } from './application-query-protocol'
import { t } from '../i18n'
import fixture from './__fixtures__/ota.json'

const toast = vi.hoisted(() => ({ pushToast: vi.fn() }))
vi.mock('../toast', async (original) => ({ ...(await original<typeof import('../toast')>()), pushToast: toast.pushToast }))

// The fixture station is activating POTA US-0001.
const snap = { hunt: null, mycall: 'W9XYZ', radio: { dialMhz: 14.285 } } as unknown as AppSnapshot
const page = (): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
  collection: 'ota', offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [], meta: { capturedAgeMs: 0, source: structuredClone(fixture) } })
beforeEach(() => toast.pushToast.mockReset())
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
  render(<StationDataContext.Provider value={true}><RemoteOperationsContext.Provider value={client}>
    <RemoteCollectionsContext.Provider value={source}><RemoteOta snap={snap} /></RemoteCollectionsContext.Provider>
    <ConfirmHost />
  </RemoteOperationsContext.Provider></StationDataContext.Provider>)
  const changes = () => sent.filter(m => m.request.type === 'logChange').map(m => m.request)
  return { client, reply, changes }
}

async function confirmSpot(test: ReturnType<typeof station>) {
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.button') }))
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(test.changes()).toHaveLength(1))
  return test.changes()[0]
}

it('offers no self-spot unless the station advertises it', async () => {
  const test = station(['otaActivation'])
  await screen.findByText('Logged park')
  expect(screen.queryByRole('button', { name: t('ota.selfSpot.button') })).toBeNull()
  test.client.disconnected()
})

it('asks on every click, posts nothing on cancel, and sends exactly what the confirm showed', async () => {
  const test = station(['otaActivation', 'selfSpot'])
  const button = await screen.findByRole('button', { name: t('ota.selfSpot.button') })
  fireEvent.click(button)
  expect(await screen.findByText(t('ota.selfSpot.confirm.body', { reference: 'US-0001', freq: '14.2850' }))).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
  await waitFor(() => expect(screen.queryByRole('button', { name: t('ota.selfSpot.confirm.post') })).toBeNull())
  expect(test.changes()).toHaveLength(0)
  // A second click asks again: the earlier answer is never remembered.
  const request = await confirmSpot(test)
  expect(request.change).toEqual({ kind: 'selfSpot', reference: 'US-0001', dialHz: 14_285_000 })
  await act(async () => test.reply({ operation: 'logChange', operationId: request.requestId, outcome: 'applied', evidence: 'spotPosted',
    spot: { pota: 'posted', cluster: 'queued' } }))
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.both'), 'success'))
  test.client.disconnected()
})

it('reports each target from the receipt, and a refusal of both is told per target, not as a generic failure', async () => {
  const test = station(['otaActivation', 'selfSpot'])
  const first = await confirmSpot(test)
  await act(async () => test.reply({ operation: 'logChange', operationId: first.requestId, outcome: 'applied', evidence: 'spotPosted',
    spot: { pota: 'loginRequired', cluster: 'queued' } }))
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.pota.loginRequired'), 'error', 8000))
  expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.cluster.queued'), 'success')
  expect(toast.pushToast).toHaveBeenCalledTimes(2)
  test.client.disconnected()
})

it('a receipt where neither target took the spot names both reasons', async () => {
  const test = station(['otaActivation', 'selfSpot'])
  const request = await confirmSpot(test)
  await act(async () => test.reply({ operation: 'logChange', operationId: request.requestId, outcome: 'rejected', reason: 'spotNotPosted',
    spot: { pota: 'failed', cluster: 'unavailable' } }))
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledTimes(2))
  expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.pota.failed'), 'error', 8000)
  expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.cluster.unavailable'), 'error', 8000)
  expect(toast.pushToast).not.toHaveBeenCalledWith(t('remote.logChangeFailed'), 'error', 6000)
  test.client.disconnected()
})
