import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { OperationRelay } from './operation-relay'
import { pendingControlStorage } from './control-storage'
import { pendingLogStorage, type ReceiptLock } from './operation-storage'
import type { OperationState } from './operation-protocol'
import type { ControlCapability, ControlContext } from './station-operation'
import type { OperationVersion } from './operation-version'
import { controlTransport } from './control-transport'
import type { ApplicationClient } from './application-client'
import type { ApplicationTransport } from '../applicationTransport'

afterEach(() => vi.useRealTimers())
const action = { action: 'amplifier.operate', expectedOperate: false, operate: true } as const
function storage() {
  const values = new Map<string, string>(), locks = new Set<string>()
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k,v) }, removeItem: (k: string) => { values.delete(k) } }
  const lock: ReceiptLock = async (k, run) => {
    if (locks.has(k)) throw Error('remoteBusy')
    locks.add(k)
    try { return await run() } finally { locks.delete(k) }
  }
  return { data, lock }
}
function setup(store = storage(), capabilities: ControlCapability[] = ['decoder', 'amplifier'], version: OperationVersion = capabilities.some(c => ['frequency', 'mode', 'tier', 'ampFollowBand'].includes(c)) ? 3 : 2) {
  vi.useFakeTimers()
  let now = 1000
  const sent: any[] = []
  const controls = pendingControlStorage(() => store.data, 'station', store.lock)
  const logs = pendingLogStorage(() => store.data, 'station', store.lock)
  const client = new OperationClient(s => sent.push(JSON.parse(s)), true, () => now, logs, version, controls)
  const s: OperationState = {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000,
    actions: [], txArmed: false, controls: { context: { radioId: 1, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 }, capabilities }
  }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  client.open(); reply(s)
  return { client, controls, logs, sent, state: s, reply, store, advance: async (ms: number) => { now += ms; await vi.advanceTimersByTimeAsync(ms) } }
}

it('refuses to borrow a newer radio or amplifier connection for the displayed gesture', async () => {
  for (const changed of ['radioId', 'radioConnection', 'ampConnection'] as const) {
    const h = setup()
    const displayed: ControlContext = { ...h.state.controls!.context, [changed]: 77 }
    const attempted = h.client.control(action, displayed).catch(e => e as Error)
    try {
      await Promise.resolve(); await Promise.resolve()
      expect(h.sent.filter(w => w.request.type === 'stationControl'), changed).toHaveLength(0)
      expect(await attempted).toMatchObject({ message: 'staleContext' })
      expect(h.client.getSnapshot().controlPending).toBeNull()
    } finally { h.client.disconnected() }
  }
})

it('keeps the displayed read sequence and requires the dedicated v3 follow capability', async () => {
  const intent = { action: 'amplifier.followBand', radioId: 1, expectedSettingsRevision: 'a'.repeat(64), expectedFollow: false, follow: true } as const
  for (const [capabilities, version] of [[['amplifier'], 3], [['ampFollowBand'], 2]] as [ControlCapability[], OperationVersion][]) {
    const h = setup(storage(), capabilities, version)
    await expect(h.client.control(intent)).rejects.toThrow(version === 2 ? 'stationUnsupported' : 'notController')
    expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
    h.client.disconnected()
  }
  const h = setup(storage(), ['ampFollowBand'])
  const displayed = { ...h.state.controls!.context, ampReadSequence: 4 }
  const pending = h.client.control(intent, displayed)
  await Promise.resolve(); await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(request.context).toEqual(displayed)
  expect(request.action).toEqual(intent)
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'settingsSaved' })
  expect(await pending).toMatchObject({ evidence: 'settingsSaved' })
  expect(h.client.getSnapshot().controlPending).toBeNull()
  h.client.disconnected()
})

it('adapts a frequency gesture through its own capability and returns only a later station sample', async () => {
  const h = setup(storage(), ['frequency'])
  let sampleAge = Infinity
  const getSnapshot = vi.fn(async () => ({ radio: { dialMhz: 7.074 } }))
  const reads = { kind: 'remote', invoke: getSnapshot } as ApplicationTransport
  const client = { age: () => sampleAge } as unknown as ApplicationClient
  const transport = controlTransport(reads, client, h.client)
  const result = transport.invoke('set_frequency', { dialMhz: 7.074, band: '40m', mode: 'USB' })
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(request.action).toEqual({ action: 'radio.frequency', dialMhz: 7.074, band: '40m', sideband: 'USB' })
  expect(getSnapshot).not.toHaveBeenCalled()
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' })
  await h.advance(100)
  expect(getSnapshot).not.toHaveBeenCalled()
  sampleAge = 0
  await h.advance(50)
  expect(await result).toEqual({ radio: { dialMhz: 7.074 } })
  expect(getSnapshot).toHaveBeenCalledExactlyOnceWith('get_snapshot')
  await expect(h.client.control({ action: 'radio.mode', mode: 'cw', followFrequency: true })).rejects.toThrow('notController')
  await expect(transport.invoke('set_frequency', { dialMhz: 7.074, band: '40m', mode: 'USB', command: 'key' })).rejects.toThrow('invalidOperation')
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it('adapts explicit mode entry separately from frequency and waits for a later station sample', async () => {
  const h = setup(storage(), ['mode'])
  let sampleAge = Infinity
  const getSnapshot = vi.fn(async () => ({ radio: { operatingMode: 'cw', txEnabled: false } }))
  const reads = { kind: 'remote', invoke: getSnapshot } as ApplicationTransport
  const transport = controlTransport(reads, { age: () => sampleAge } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('set_operating_mode', { mode: 'cw', followFreq: true })
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(request.action).toEqual({ action: 'radio.mode', mode: 'cw', followFrequency: true })
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' })
  await h.advance(100)
  expect(getSnapshot).not.toHaveBeenCalled()
  sampleAge = 0
  await h.advance(50)
  expect(await result).toEqual({ radio: { operatingMode: 'cw', txEnabled: false } })
  for (const args of [{ mode: 'cw', followFreq: true, arm: true }, { mode: 'cw' }, { mode: 'future', followFreq: true }]) {
    await expect(transport.invoke('set_operating_mode', args)).rejects.toThrow('invalidOperation')
  }
  await expect(h.client.control({ action: 'radio.frequency', dialMhz: 7.074, band: '40m', sideband: 'USB' })).rejects.toThrow('notController')
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it.each(['cw', 'phone'] as const)('sends one %s Work intent and waits for matching later station data before handoff', async mode => {
  const h = setup(storage(), ['workSpot'], 3)
  let sampleAge = Infinity
  const snapshot = { radio: { operatingMode: mode, dialMhz: 7.19876, txEnabled: false } }
  const getSnapshot = vi.fn(async () => snapshot)
  const transport = controlTransport({ kind: 'remote', invoke: getSnapshot } as ApplicationTransport,
    { age: () => sampleAge } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('work_spot', { mode, freqMhz: 7.19876, band: '40m', call: 'N2SPOT/P', tier: null })
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(request.action).toEqual({ action: 'radio.workSpot', mode, dialMhz: 7.19876, band: '40m', call: 'N2SPOT/P' })
  expect(getSnapshot).not.toHaveBeenCalled()
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' })
  await h.advance(100)
  expect(getSnapshot).not.toHaveBeenCalled()
  sampleAge = 0
  await h.advance(50)
  expect(await result).toEqual(snapshot)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it.each(['frequency', 'mode', 'evidence'])('refuses a Work handoff after a mismatched %s without replaying the intent', async changed => {
  const h = setup(storage(), ['workSpot'], 3)
  const snapshot = { radio: { operatingMode: changed === 'mode' ? 'phone' : 'cw', dialMhz: changed === 'frequency' ? 14.074 : 7.023 } }
  const transport = controlTransport({ kind: 'remote', invoke: vi.fn(async () => snapshot) } as ApplicationTransport,
    { age: () => 0 } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('work_spot', { mode: 'cw', freqMhz: 7.023, band: '40m', call: 'N2SPOT', tier: null }).catch(e => e)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: changed === 'evidence' ? 'receiverState' : 'radioReadback' })
  expect(await result).toMatchObject({ message: changed === 'evidence' ? 'operationUnknown' : 'readingUnavailable' })
  await h.advance(1500)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it('refuses Work on older stations, without its capability, or with unsupported native arguments', async () => {
  for (const [capabilities, version] of [[['workSpot'], 2], [['frequency', 'mode'], 3]] as [ControlCapability[], OperationVersion][]) {
    const h = setup(storage(), capabilities, version)
    await expect(h.client.control({ action: 'radio.workSpot', mode: 'cw', dialMhz: 7.023, band: '40m', call: 'N2SPOT' })).rejects.toThrow(version === 2 ? 'stationUnsupported' : 'notController')
    expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
    h.client.disconnected()
  }
  const h = setup(storage(), ['workSpot'], 3)
  const invoke = vi.fn(), transport = controlTransport({ kind: 'remote', invoke } as ApplicationTransport, {} as ApplicationClient, h.client)
  const args = { mode: 'cw', freqMhz: 7.023, band: '40m', call: 'N2SPOT', tier: null }
  for (const bad of [{ ...args, tier: 'FT8' }, { ...args, splitUpKhz: 2 }, { ...args, txEnabled: true }, { ...args, mode: 'digital' }, { ...args, call: null }, { ...args, call: 'N2SPOT\nT 1' }]) {
    await expect(transport.invoke('work_spot', bad)).rejects.toThrow()
  }
  expect(invoke).not.toHaveBeenCalled()
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
  h.client.disconnected()
})

it('a receiver grant cannot tune and an uncertain frequency is never sent again', async () => {
  const a = { action: 'radio.frequency', dialMhz: 7.074, band: '40m', sideband: 'USB' } as const
  const receiver = setup(storage(), ['decoder', 'amplifier'], 3)
  await expect(receiver.client.control(a)).rejects.toThrow('notController')
  receiver.client.disconnected()
  const h = setup(storage(), ['frequency']), result = h.client.control(a)
  await Promise.resolve()
  const id = h.sent[h.sent.length - 1].request.requestId
  h.reply({ operation: 'stationControl', operationId: id, outcome: 'unknown', reason: 'hardwareUnconfirmed' })
  expect(await result).toMatchObject({ outcome: 'unknown' })
  await expect(h.client.control(a)).rejects.toThrow('operationUnknown')
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it('adapts the existing tier selector through its own v3 capability and later snapshot', async () => {
  const h = setup(storage(), ['tier'])
  let sampleAge = Infinity
  const read = vi.fn(async () => ({ link: { tier: 'FT4' } }))
  const transport = controlTransport({ kind: 'remote', invoke: read } as ApplicationTransport, { age: () => sampleAge } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('set_tier', { tier: 'FT4' })
  await Promise.resolve()
  const frame = h.sent[h.sent.length - 1], request = frame.request
  expect(frame.operationVersion).toBe(3)
  expect(request.action).toEqual({ action: 'radio.tier', tier: 'FT4' })
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' })
  await h.advance(100)
  expect(read).not.toHaveBeenCalled()
  sampleAge = 0
  await h.advance(50)
  expect(await result).toEqual({ link: { tier: 'FT4' } })
  expect(read).toHaveBeenCalledExactlyOnceWith('get_snapshot')
  for (const args of [{ tier: 'FT4', arm: true }, { tier: 'future' }, {}]) await expect(transport.invoke('set_tier', args)).rejects.toThrow('invalidOperation')
  await expect(h.client.control({ action: 'radio.mode', mode: 'cw', followFrequency: true })).rejects.toThrow('notController')
  h.client.disconnected()
  const legacy = setup(storage(), ['tier'], 2)
  await expect(legacy.client.control({ action: 'radio.tier', tier: 'FT4' })).rejects.toThrow('stationUnsupported')
  expect(legacy.sent.filter(f => f.request.type === 'stationControl')).toHaveLength(0)
  legacy.client.disconnected()
})

it('sends one explicit intent and retains it until a later native readback', async () => {
  const h = setup(), completed = h.client.control(action)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(h.sent[h.sent.length - 1].operationVersion).toBe(2)
  expect(request).toMatchObject({ type: 'stationControl', action, context: h.state.controls!.context })
  const receipt = { operation: 'stationControl', operationId: request.requestId }
  h.reply({ ...receipt, outcome: 'pending' })
  await Promise.resolve(); await Promise.resolve()
  expect(h.controls.read()?.operationId).toBe(request.requestId)
  await h.advance(250)
  expect(h.sent[h.sent.length - 1].request).toMatchObject({ type: 'heartbeat', leaseId: h.state.leaseId })
  h.reply({ ...h.state, revision: 2, nextSequence: 2 })
  await h.advance(250)
  expect(h.sent[h.sent.length - 1].request).toMatchObject({ type: 'result', operationId: request.requestId })
  h.reply({ ...receipt, outcome: 'applied', evidence: 'amplifierReadback' })
  expect(await completed).toMatchObject({ outcome: 'applied', evidence: 'amplifierReadback' })
  expect(h.controls.read()).toBeNull()
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it('does not replay an uncertain toggle after disconnect or reload', async () => {
  const h = setup(), result = h.client.control(action).catch(e => e.message)
  await Promise.resolve()
  const operationId = h.sent[h.sent.length - 1].request.requestId
  h.client.disconnected()
  await result
  const next = setup(h.store)
  await expect(next.client.control(action)).rejects.toThrow('operationUnknown')
  expect(next.sent.map(w => w.request.type)).toEqual(['state'])
  const checked = next.client.refreshControl()
  await Promise.resolve()
  next.reply({ operation: 'stationControl', operationId, outcome: 'unknown', reason: 'hardwareUnconfirmed' })
  await checked
  expect(next.controls.read()?.operationId).toBe(operationId)
  await next.client.acknowledgeControl()
  expect(next.controls.read()).toBeNull()
  next.client.disconnected()
})

it('recovers a saved follow setting after reload without sending another save', async () => {
  const intent = { action: 'amplifier.followBand', radioId: 1, expectedSettingsRevision: 'a'.repeat(64), expectedFollow: false, follow: true } as const
  const h = setup(storage(), ['ampFollowBand'])
  const pending = h.client.control(intent).catch(e => e.message)
  await Promise.resolve()
  const operationId = h.sent[h.sent.length - 1].request.requestId
  expect(h.controls.read()?.action).toEqual(intent)
  h.client.disconnected(); await pending
  const next = setup(h.store, ['ampFollowBand'])
  await expect(next.client.control(intent)).rejects.toThrow('operationUnknown')
  const checked = next.client.refreshControl()
  await Promise.resolve()
  next.reply({ operation: 'stationControl', operationId, outcome: 'applied', evidence: 'settingsSaved' })
  expect(await checked).toMatchObject({ evidence: 'settingsSaved' })
  expect(next.controls.read()).toBeNull()
  expect(next.sent.map(w => w.request.type)).toEqual(['state', 'result'])
  next.client.disconnected()
})

it('binds result kinds and never clears a control intent using a log receipt', async () => {
  const h = setup(), result = h.client.control(action).catch(() => {})
  await Promise.resolve()
  const id = h.sent[h.sent.length - 1].request.requestId
  expect(() => h.reply({ operationId: id, outcome: 'applied', evidence: 'fileSynced', uploads: 'stationPipeline' })).toThrow('invalidOperation')
  expect(h.controls.read()?.operationId).toBe(id)
  h.reply({ operation: 'stationControl', operationId: id, outcome: 'rejected', reason: 'stationBusy' })
  await result
  expect(h.controls.read()).toBeNull()
  h.client.disconnected()
})

it('keeps logging and operating receipts exclusive across tabs', async () => {
  const h = setup(), entry = { operationId: crypto.randomUUID(), action }
  await h.controls.exclusive(() => h.controls.write(entry))
  const another = pendingLogStorage(() => h.store.data, 'station', h.store.lock)
  await expect(another.exclusive!(() => another.write(crypto.randomUUID()))).rejects.toThrow('operationUnknown')
  const oldTab = pendingControlStorage(() => h.store.data, 'station', h.store.lock)
  oldTab.read()
  await h.controls.exclusive(() => h.controls.write(null))
  const newer = { operationId: crypto.randomUUID(), action }
  await h.controls.exclusive(() => h.controls.write(newer))
  await oldTab.exclusive(() => oldTab.write(null))
  expect(h.controls.read()).toEqual(newer)
  h.client.disconnected()
})

it('refuses station control on an old station while retaining legacy read negotiation', () => {
  const frames: any[] = [], replies: any[] = [], relay = new OperationRelay()
  const station = { send: (v: unknown) => { frames.push(v) }, close: () => {} }, browser = { send: (v: unknown) => { replies.push(v) }, close: () => {} }
  // The legacy peer shape is accepted by the relay. Control is versioned separately.
  relay.sync({ peer: station, supported: true }, [{ peer: browser, sessionId: 'browser', deviceId: crypto.randomUUID() }], 1000)
  const requestId = crypto.randomUUID()
  const request = { type: 'stationControl', requestId, stationBootId: crypto.randomUUID(), leaseId: crypto.randomUUID(), commandWindowId: crypto.randomUUID(), expectedRevision: 1, clientSequence: 1, context: { radioId: 1, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 }, action }
  relay.receiveBrowser('browser', { type: 'operationRequest', operationVersion: 2, request }, 1000)
  expect(frames).toHaveLength(0)
  expect(JSON.stringify(replies)).toContain('stationUnsupported')
})


it('a prepared tuning gesture cannot borrow a newer lease, revision or command deadline', async () => {
  for (const change of ['revision', 'lease', 'deadline'] as const) {
    const h = setup(storage(), ['frequency'])
    const prepared = h.client.prepareControl()
    await h.advance(change === 'deadline' ? 1250 : 1000)
    h.reply({ ...h.state, revision: h.state.revision + (change === 'revision' ? 1 : 0),
      leaseId: change === 'lease' ? crypto.randomUUID() : h.state.leaseId, commandWindowId: crypto.randomUUID() })
    const frequency = { action: 'radio.frequency', dialMhz: 7.075, band: '40m', sideband: 'USB' } as const
    await expect(prepared(frequency)).rejects.toThrow(change === 'deadline' ? 'windowExpired' : 'staleContext')
    expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
    // A later explicit gesture captures the now-current station authority.
    const fresh = h.client.control(frequency)
    await Promise.resolve(); await Promise.resolve()
    const request = h.sent[h.sent.length - 1].request
    expect(request.action).toEqual(frequency)
    h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' })
    expect(await fresh).toMatchObject({ outcome: 'applied' })
    h.client.disconnected()
  }
})

it('selects a configured radio with displayed context and waits for both new snapshot and Settings', async () => {
  const h = setup(storage(), ['radioSelection'], 3)
  let snapshotAge = Infinity, settingsAge = Infinity, active = 1
  const read = vi.fn(async (command: string) => command === 'get_settings' ? { activeRadio: active } : { activeRadioId: active, radios: [{ id: 1 }, { id: 2 }] })
  const transport = controlTransport({ kind: 'remote', invoke: read } as ApplicationTransport,
    { age: (command: string) => command === 'get_settings' ? settingsAge : snapshotAge } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('set_active_radio', { id: 2 })
  await h.advance(0)
  const request = h.sent[h.sent.length - 1].request
  expect(request.action).toEqual({ action: 'radio.select', radioId: 2 })
  expect(request.context.radioId).toBe(1)
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' })
  await h.advance(50)
  active = 2; snapshotAge = 0
  await h.advance(50)
  expect(read).toHaveBeenNthCalledWith(2, 'get_settings')
  expect(read).toHaveBeenCalledTimes(2)
  settingsAge = 0
  await h.advance(50)
  expect(await result).toMatchObject({ activeRadioId: 2 })
  expect(read).toHaveBeenLastCalledWith('get_settings')
  expect(h.sent.filter(w=>w.request.type==='stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it('refuses stale radio displays, unconfigured targets, extra arguments and pre-v3 selection', async () => {
  for (const [active, target] of [[9, 2], [1, 99]]) {
    const h = setup(storage(), ['radioSelection'], 3)
    const transport = controlTransport({kind:'remote',invoke:vi.fn(async()=>({activeRadioId:active,radios:[{id:1},{id:2}]}))} as ApplicationTransport, {} as ApplicationClient, h.client)
    await expect(transport.invoke('set_active_radio',{id:target})).rejects.toThrow('readingUnavailable')
    for (const args of [{id:2,command:'key'},{id:-1},{}]) await expect(transport.invoke('set_active_radio',args)).rejects.toThrow('invalidOperation')
    expect(h.sent.filter(w=>w.request.type==='stationControl')).toHaveLength(0)
    h.client.disconnected()
  }
  const legacy = setup(storage(), ['radioSelection'], 2)
  await expect(legacy.client.control({action:'radio.select',radioId:2})).rejects.toThrow('stationUnsupported')
  legacy.client.disconnected()
})
