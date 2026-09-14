// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
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

// The fixture station is activating POTA US-0001.
const snap = { hunt: null, mycall: 'W9XYZ', radio: { dialMhz: 14.285 } } as unknown as AppSnapshot
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
  render(<StationDataContext.Provider value={true}><RemoteOperationsContext.Provider value={client}>
    <RemoteCollectionsContext.Provider value={source}><RemoteOta snap={snap} /></RemoteCollectionsContext.Provider>
    <ConfirmHost />
  </RemoteOperationsContext.Provider></StationDataContext.Provider>)
  const changes = () => sent.filter(m => m.request.type === 'logChange').map(m => m.request)
  return { client, reply, changes }
}

it('offers no self-spot unless the station advertises it', async () => {
  const test = station(['otaActivation'])
  await screen.findByText('Logged park')
  expect(screen.queryByRole('button', { name: t('remote.selfSpot') })).toBeNull()
  test.client.disconnected()
})

it('asks on every click, posts nothing on cancel, and sends exactly what the confirm showed', async () => {
  const test = station(['otaActivation', 'selfSpot'])
  const button = await screen.findByRole('button', { name: t('remote.selfSpot') })
  fireEvent.click(button)
  expect(await screen.findByText(t('remote.selfSpotConfirm.body', { reference: 'US-0001', freq: '14.2850' }))).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
  await waitFor(() => expect(screen.queryByRole('button', { name: t('remote.selfSpotConfirm.post') })).toBeNull())
  expect(test.changes()).toHaveLength(0)
  // A second click asks again: the earlier answer is never remembered.
  fireEvent.click(button)
  fireEvent.click(await screen.findByRole('button', { name: t('remote.selfSpotConfirm.post') }))
  await waitFor(() => expect(test.changes()).toHaveLength(1))
  const [request] = test.changes()
  expect(request.change).toEqual({ kind: 'selfSpot', reference: 'US-0001', dialHz: 14_285_000 })
  await act(async () => test.reply({ operation: 'logChange', operationId: request.requestId, outcome: 'applied', evidence: 'spotQueued' }))
  test.client.disconnected()
})
