import { expect, it, vi } from 'vitest'
import { advertisedOperationVersion, controlVersion } from './operation-version'
import { OperationRelay } from './operation-relay'
import { operationRequest, operationResponse, operationValue, type OperationState } from './operation-protocol'

const state = (): OperationState => ({
  stationBootId: crypto.randomUUID(), allowed: true, phase: 'available', leaseId: null,
  revision: 1, commandWindowId: null, nextSequence: null, leaseRemainingMs: null,
  actions: [], txArmed: false, controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities: ['decoder'] }
})
const peer = () => ({ frames: [] as any[], send(s: string) { this.frames.push(JSON.parse(s)) }, close: vi.fn() })

it('preserves the original advertisement while explicitly negotiating expanded operations', () => {
  expect(advertisedOperationVersion(1, 3)).toBe(1)
  expect(advertisedOperationVersion(2)).toBe(2)
  expect(advertisedOperationVersion(2, 3)).toBe(3)
  expect(advertisedOperationVersion(2, 999)).toBe(2)
  for (const base of [undefined, 0, 3, '2', null]) expect(advertisedOperationVersion(base, 3)).toBe(0)
  expect(controlVersion({ action: 'radio.filterWidth', mode: 'cw', expectedHz: 500, hz: 550 })).toBe(3)
  expect(controlVersion({ action: 'radio.function', mode: 'phone', func: 'manualNotch', expectedOn: false, on: true })).toBe(3)
  expect(controlVersion({ action: 'radio.agc', mode: 'cw', expectedSpeed: 'fast', speed: 'fast' })).toBe(3)
  expect(controlVersion({ action: 'radio.band', band: '40m', mode: 'phone' })).toBe(3)
  expect(controlVersion({ action: 'radio.tier', tier: 'FT4' })).toBe(3)
  expect(controlVersion({ action: 'radio.workspace', workspace: 'js8' })).toBe(3)
  expect(controlVersion({ action: 'decoder.clear', receiver: 'cw' })).toBe(2)
})

it('negotiates each legacy/new browser and station pair without upgrading the older peer', () => {
  for (const browserVersion of [1, 2, 3]) for (const stationVersion of [1, 2, 3]) {
    const relay = new OperationRelay(), station = peer(), browser = peer(), sessionId = crypto.randomUUID()
    relay.sync({ peer: station, supported: true, operationVersion: stationVersion }, [{ peer: browser, sessionId, deviceId: crypto.randomUUID() }], 1000)
    const request = { type: 'state', requestId: crypto.randomUUID() }
    relay.receiveBrowser(sessionId, { type: 'operationRequest', ...(browserVersion >= 2 ? { operationVersion: browserVersion } : {}), request }, 1000)
    const negotiated = Math.min(browserVersion, stationVersion)
    expect(station.frames).toHaveLength(1)
    expect(station.frames[0].operationVersion).toBe(negotiated >= 2 ? negotiated : undefined)
    const value = state()
    if (negotiated === 1) delete value.controls
    relay.receiveStation({ type: 'operationResponse', sessionId, requestId: request.requestId, value })
    expect(browser.frames[0].value.phase).toBe('available')
    expect(browser.close).not.toHaveBeenCalled()
    expect(station.close).not.toHaveBeenCalled()
  }
})

it('refuses expanded actions before a legacy station receives them, and allows receiver controls', () => {
  const relay = new OperationRelay(), station = peer(), browser = peer(), sessionId = crypto.randomUUID()
  relay.sync({ peer: station, supported: true, operationVersion: 2 }, [{ peer: browser, sessionId, deviceId: crypto.randomUUID() }], 1000)
  const request = { type: 'stationControl', requestId: crypto.randomUUID(), stationBootId: crypto.randomUUID(), leaseId: crypto.randomUUID(),
    expectedRevision: 1, commandWindowId: crypto.randomUUID(), clientSequence: 1, context: state().controls!.context, action: { action: 'radio.tier', tier: 'FT4' } }
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 3, request }, 1000)
  expect(station.frames).toHaveLength(0)
  expect(browser.frames[0].error).toBe('stationUnsupported')
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 3, request: { ...request, requestId: crypto.randomUUID(), action: { action: 'decoder.clear', receiver: 'cw' } } }, 1000)
  expect(station.frames).toHaveLength(1)
  expect(station.frames[0].operationVersion).toBe(2)
})

it('projects older station capability hints for each browser version after room hibernation', () => {
  for (const version of [1, 2, 3]) {
    const relay = new OperationRelay(), station = peer(), browser = peer(), sessionId = crypto.randomUUID()
    const peers = [{ peer: browser, sessionId, deviceId: crypto.randomUUID() }]
    relay.sync({ peer: station, supported: true, operationVersion: 3 }, peers, 1000)
    const requestId = crypto.randomUUID()
    relay.receiveBrowser(sessionId, { type: 'operationRequest', ...(version >= 2 ? { operationVersion: version } : {}), request: { type: 'state', requestId } }, 1000)
    const restored = new OperationRelay()
    restored.sync({ peer: station, supported: true, operationVersion: 3 }, peers, 1001)
    restored.restore(sessionId, relay.checkpoint(sessionId))
    // A pre-v3 native build may have advertised frequency/mode under v2.
    // Cloud upgrades must keep that hint from breaking a legacy parser.
    const wire = state()
    wire.controls!.capabilities = ['decoder', 'amplifier', 'frequency', 'mode', 'tier', 'ampFollowBand', 'workspace', 'decoderSettings','receiverSettings','receiverGain','bandSelection','receiverFilter','receiverDsp']
    restored.receiveStation({ type: 'operationResponse', sessionId, requestId, value: wire })
    expect(browser.frames).toHaveLength(1)
    if (version === 1) expect(browser.frames[0].value.controls).toBeUndefined()
    else expect(browser.frames[0].value.controls.capabilities).toEqual(version === 2 ? ['decoder', 'amplifier'] : wire.controls!.capabilities)
    expect(wire.controls!.capabilities).toHaveLength(13)
  }
})

it('restores pre-v3 checkpoints conservatively and rejects malformed stored versions', () => {
  const station = peer(), browser = peer(), sessionId = crypto.randomUUID(), requestId = crypto.randomUUID()
  const peers = [{ peer: browser, sessionId, deviceId: crypto.randomUUID() }]
  const restored = new OperationRelay()
  restored.sync({ peer: station, supported: true, operationVersion: 3 }, peers, 1001)
  restored.restore(sessionId, { version: 1, pending: [{ requestId, at: 1000, mutation: false }] })
  const wire = state()
  wire.controls!.capabilities = ['decoder', 'frequency', 'mode']
  restored.receiveStation({ type: 'operationResponse', sessionId, requestId, value: wire })
  expect(browser.frames[0].value.controls.capabilities).toEqual(['decoder'])
  for (const version of [0, 4, '3', null]) {
    expect(() => restored.restore(sessionId, { version: 1, pending: [{ requestId, at: 1000, mutation: false, operationVersion: version as never }] })).toThrow('invalidOperationCheckpoint')
  }
})

it('ignores bounded future capability hints without admitting an unknown action or mutating the wire value', () => {
  const wire = state()
  const capabilities = ['decoder', 'futureReceiver', 'tier']
  const input = { ...wire, controls: { ...wire.controls, capabilities } }
  const parsed = operationValue(input) as OperationState
  expect(parsed.controls!.capabilities).toEqual(['decoder', 'tier'])
  const response = operationResponse({ type: 'operationResponse', requestId: crypto.randomUUID(), value: input })
  expect('value' in response && (response.value as OperationState).controls!.capabilities).toEqual(['decoder', 'tier'])
  expect(input.controls.capabilities).toEqual(capabilities)
  for (const invalid of [['decoder', 'decoder'], ['a'.repeat(33)], ['send command'], [null], Array.from({ length: 65 }, (_, i) => `future${i}`)]) {
    expect(() => operationValue({ ...wire, controls: { ...wire.controls, capabilities: invalid } })).toThrow('invalidOperation')
  }
  expect(() => operationValue({ ...input, allowed: false })).toThrow('invalidOperation')
  expect(() => operationRequest({ type: 'stationControl', requestId: crypto.randomUUID(), stationBootId: wire.stationBootId,
    leaseId: crypto.randomUUID(), expectedRevision: 1, commandWindowId: crypto.randomUUID(), clientSequence: 1,
    context: wire.controls!.context, action: { action: 'futureReceiver', on: true }
  })).toThrow('invalidOperation')
})
