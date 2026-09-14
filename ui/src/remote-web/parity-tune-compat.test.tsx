// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, renderHook } from '@testing-library/react'
import type { ReactNode } from 'react'
import { OperationClient } from './operation-client'
import { OperationRelay } from './operation-relay'
import { pendingControlStorage } from './control-storage'
import { pendingLogStorage, type ReceiptLock } from './operation-storage'
import { operationValue, type OperationState } from './operation-protocol'
import { CONTROL_CAPABILITIES, TUNE_CAPABILITIES, TX_IDLE_CAPABILITIES, type ControlCapability } from './station-operation'
import type { OperationVersion } from './operation-version'
import { RemoteOperationsContext, StationControlContext, StationDataContext, useStationCapability } from '../stationAccess'

// Remote parity batch 1 ships with no operation version bump, so every mixed pair of hosted site
// and desktop must keep working on capability hints alone. These tests pin the four pairs.

afterEach(() => { cleanup(); vi.useRealTimers() })

/** The vocabulary every page shipped before this batch knew. An older page validates with it. */
const PRE_BATCH: readonly string[] = CONTROL_CAPABILITIES.filter(c => !(TUNE_CAPABILITIES as readonly string[]).includes(c))

function storage() {
  const values = new Map<string, string>(), locks = new Set<string>()
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const lock: ReceiptLock = async (k, run) => {
    if (locks.has(k)) throw Error('remoteBusy')
    locks.add(k)
    try { return await run() } finally { locks.delete(k) }
  }
  return { data, lock }
}

function state(capabilities: string[], txArmed = false): OperationState {
  return {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed,
    ...(txArmed ? { transmitEpoch: '0123456789abcdef' } : {}),
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities: capabilities as ControlCapability[] }
  }
}

function station(capabilities: string[], version: OperationVersion, txArmed = false) {
  const store = storage(), sent: any[] = []
  const client = new OperationClient(s => sent.push(JSON.parse(s)), true, () => 1000,
    pendingLogStorage(() => store.data, 'station', store.lock), version, pendingControlStorage(() => store.data, 'station', store.lock))
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value: state(capabilities, txArmed) })
  const wrapper = ({ children }: { children: ReactNode }) => (
    <StationControlContext.Provider value={false}>
      <StationDataContext.Provider value={true}>
        <RemoteOperationsContext.Provider value={client}>{children}</RemoteOperationsContext.Provider>
      </StationDataContext.Provider>
    </StationControlContext.Provider>
  )
  const allowed = (capability: ControlCapability) => renderHook(() => useStationCapability(capability), { wrapper }).result.current
  return { client, sent, allowed }
}

it('old site with a new desktop: an older page drops the new hints and grants nothing new', () => {
  // The new names really are new: an older page's vocabulary did not have them.
  for (const name of TUNE_CAPABILITIES) expect(PRE_BATCH).not.toContain(name)
  // v3-era hints: the relay scopes the v4-only FT hints away at operation version 3.
  const existing = ['decoder', 'amplifier', 'frequency']
  const fromNewStation = state([...existing, ...TUNE_CAPABILITIES])
  // What an older page keeps is its own validator's filter over the same bounded list.
  const older = (operationValue(fromNewStation) as OperationState).controls!.capabilities.filter(c => PRE_BATCH.includes(c))
  expect(older).toEqual(existing)
  // Positive control: the same state is still a valid, controlling state (it did not throw).
  expect((operationValue(fromNewStation) as OperationState).phase).toBe('controlling')
  // A relay built from this source forwards the state intact to a page, never closing the station.
  const relay = new OperationRelay(), stationPeer = { frames: [] as any[], send(s: string) { this.frames.push(JSON.parse(s)) }, close: vi.fn() }
  const browser = { frames: [] as any[], send(s: string) { this.frames.push(JSON.parse(s)) }, close: vi.fn() }
  const sessionId = crypto.randomUUID(), requestId = crypto.randomUUID()
  relay.sync({ peer: stationPeer, supported: true, operationVersion: 3 }, [{ peer: browser, sessionId, deviceId: crypto.randomUUID() }], 1000)
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 3, request: { type: 'state', requestId } }, 1000)
  relay.receiveStation({ type: 'operationResponse', sessionId, requestId, value: fromNewStation })
  expect(stationPeer.close).not.toHaveBeenCalled()
  expect(browser.frames[browser.frames.length - 1].value.controls.capabilities).toEqual([...existing, ...TUNE_CAPABILITIES])
})

it('an unknown future hint is still dropped, so a later batch cannot break this page either', () => {
  const future = operationValue(state(['decoder', 'rotatorElevation', 'aiCw'])) as OperationState
  expect(future.controls!.capabilities).toEqual(['decoder', 'aiCw'])
})

it('new site with an old desktop: no hint, no control', () => {
  const old = station([...PRE_BATCH], 3)
  for (const name of TUNE_CAPABILITIES) expect(old.allowed(name), name).toBe(false)
  // Positive control: an existing v3 hint from the same station is still usable.
  expect(old.allowed('frequency')).toBe(true)
})

it('new site with a new desktop: each hint enables its control at v3 and never at v2', () => {
  const current = station([...TUNE_CAPABILITIES], 3)
  for (const name of TUNE_CAPABILITIES) expect(current.allowed(name), name).toBe(true)
  const v2 = station([...TUNE_CAPABILITIES], 2)
  for (const name of TUNE_CAPABILITIES) expect(v2.allowed(name), name).toBe(false)
})

it('controls that move the transmit frequency or feed the FT sequencer stay off while TX is armed', () => {
  const armed = station([...TUNE_CAPABILITIES], 4, true)
  for (const name of TUNE_CAPABILITIES)
    expect(armed.allowed(name), name).toBe(!(TX_IDLE_CAPABILITIES as readonly string[]).includes(name))
  // Positive control: the same controls are live with TX idle at the same version.
  const idle = station([...TUNE_CAPABILITIES], 4)
  for (const name of TX_IDLE_CAPABILITIES) expect(idle.allowed(name), name).toBe(true)
})
