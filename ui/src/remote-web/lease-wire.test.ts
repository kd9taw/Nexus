// THE LEASE AND THE STOP, BYTE FOR BYTE. Acquire, Heartbeat and Release hold a stream's lease, and
// Stop on the operation lane is the second road a Stop takes beside the stream's own channel. The
// station parses all four with `deny_unknown_fields` (`remote_service/operations.rs`), so a key
// added, renamed or reordered here is a station in the field that refuses the lease or the Stop.
// Each case is the exact text `OperationClient` hands its socket, at every lane version it speaks.
import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { OPERATION_VERSIONS } from './operation-version'
import type { OperationState } from './operation-protocol'

const BOOT = '10000000-0000-4000-8000-000000000001'
const LEASE = '10000000-0000-4000-8000-000000000002'
const WINDOW = '10000000-0000-4000-8000-000000000003'
const EPOCH = '000000000000002a'
const state = (phase: OperationState['phase'], leaseId: string | null): OperationState => ({ stationBootId: BOOT, allowed: true, phase,
  leaseId, revision: 1, commandWindowId: leaseId ? WINDOW : null, nextSequence: leaseId ? 1 : null, leaseRemainingMs: leaseId ? 5000 : null,
  actions: [], txArmed: false, transmitEpoch: EPOCH })

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks() })

it.each(OPERATION_VERSIONS)('the lease and the operation lane\'s Stop leave the page exactly as stations parse them (v%s)', async version => {
  vi.useFakeTimers()
  let n = 0
  vi.spyOn(crypto, 'randomUUID').mockImplementation(() => `20000000-0000-4000-8000-${String(++n).padStart(12, '0')}` as `${string}-${string}-${string}-${string}-${string}`)
  const wire: string[] = []
  const c = new OperationClient(text => wire.push(text), true, () => Date.now(), version)
  const last = () => JSON.parse(wire[wire.length - 1]!) as { request: { requestId: string } }
  const answer = (value: unknown) => c.receive({ type: 'operationResponse', requestId: last().request.requestId, value })
  const lane = version >= 2 ? `"operationVersion":${version},` : ''

  c.open()
  answer(state('available', null))
  const acquired = c.acquire()
  expect(wire[wire.length - 1]).toBe(`{"type":"operationRequest",${lane}"request":{"type":"acquire","requestId":"20000000-0000-4000-8000-000000000002","stationBootId":"${BOOT}"}}`)
  answer(state('controlling', LEASE))
  await acquired

  const sent = wire.length
  await vi.advanceTimersByTimeAsync(1000)
  expect(wire.length, 'the next automatic read went out').toBe(sent + 1)
  expect(wire[wire.length - 1]).toBe(`{"type":"operationRequest",${lane}"request":{"type":"heartbeat","requestId":"20000000-0000-4000-8000-000000000003","leaseId":"${LEASE}"}}`)
  answer(state('controlling', LEASE))

  if (version >= 4) {
    const stopped = c.stopTransmit()
    expect(wire[wire.length - 1]).toBe(`{"type":"operationRequest","operationVersion":4,"request":{"type":"stopTransmit","requestId":"20000000-0000-4000-8000-000000000004","stationBootId":"${BOOT}","leaseId":"${LEASE}","transmitEpoch":"${EPOCH}"}}`)
    answer({ stop: 'accepted' })
    await stopped
  }

  const released = c.release()
  const id = version >= 4 ? '000000000005' : '000000000004'
  expect(wire[wire.length - 1]).toBe(`{"type":"operationRequest",${lane}"request":{"type":"release","requestId":"20000000-0000-4000-8000-${id}","leaseId":"${LEASE}"}}`)
  answer(state('available', null))
  await released
  c.disconnected()
})
