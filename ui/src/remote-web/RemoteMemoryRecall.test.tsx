// @vitest-environment jsdom
// Remote parity batch 1: recall a station memory from the hosted Memories view. The bank stays
// read-only; each row's Tune (and the row itself) recalls it while the station advertises
// memoryRecall, and nothing is written to a browser bank.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { RemoteMemories } from './RemoteMemories'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { pendingLogStorage, type ReceiptLock } from './operation-storage'
import type { OperationState } from './operation-protocol'
import type { ControlCapability } from './station-operation'
import type { ApplicationClient } from './application-client'
import type { QueryPage } from './application-query-protocol'
import { memoriesStore, type Memory } from '../features/memories'
import fixture from './__fixtures__/memories.json'

afterEach(() => { cleanup(); vi.restoreAllMocks(); localStorage.clear() })

const page = (): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
  collection: 'memories', offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [],
  meta: { capturedAgeMs: 0, source: { bank: structuredClone(fixture), sourceAgeMs: 250 } } as unknown as QueryPage['meta'] })

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

function memories(capabilities: ControlCapability[]) {
  const client = { invoke: vi.fn(async () => page()), supports: () => true, getPhase: () => 'ready' } as unknown as ApplicationClient
  const onRecall = vi.fn((_m: Memory) => {})
  const view = render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={operations(capabilities)}>
      <RemoteCollectionsContext.Provider value={new RemoteCollections(client)}><RemoteMemories myGrid="FN31" onRecall={onRecall} /></RemoteCollectionsContext.Provider>
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  return { ...view, onRecall }
}

const row = async (name: string) => {
  const main = (await screen.findByText(name)).closest<HTMLButtonElement>('.mv-row-main')!
  return { main, tune: main.parentElement!.querySelector<HTMLButtonElement>('.mv-row-tune') }
}

it('recalls a station memory from its row while the station advertises it, writing no browser bank', async () => {
  const update = vi.spyOn(memoriesStore, 'update'), stored = vi.spyOn(Storage.prototype, 'setItem')
  const { onRecall, container } = memories(['memoryRecall'])
  const { main, tune } = await row('QRP calling')
  expect(tune).toBeTruthy()
  expect(tune!.disabled).toBe(false)
  fireEvent.click(tune!)
  expect(onRecall).toHaveBeenCalledTimes(1)
  expect(onRecall.mock.calls[0][0].id).toBe('m-cw')
  expect(main.disabled).toBe(false)
  fireEvent.click(main)
  expect(onRecall).toHaveBeenCalledTimes(2)
  // The bank itself stays read-only: no edit, delete or move controls, and no browser bank write.
  expect(container.querySelector('.mv-row-edit, .mv-row-del, .mv-side-add, input[type=file]')).toBeNull()
  expect(update).not.toHaveBeenCalled()
  expect(stored).not.toHaveBeenCalled()
})

it('offers no recall without the station hint', async () => {
  const { onRecall } = memories(['repeaterTuning', 'workSpot'])
  const { main, tune } = await row('QRP calling')
  expect(tune).toBeNull()
  expect(main.disabled).toBe(true)
  fireEvent.click(main)
  expect(onRecall).not.toHaveBeenCalled()
})
