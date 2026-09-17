import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient, OperationFailure } from './operation-client'
import { OperationRelay } from './operation-relay'
import { pendingControlStorage } from './control-storage'
import { pendingLogStorage, type ReceiptLock } from './operation-storage'
import type { OperationState } from './operation-protocol'
import type { ControlCapability, ControlContext } from './station-operation'
import type { OperationVersion } from './operation-version'
import { controlTransport } from './control-transport'
import type { ApplicationClient } from './application-client'
import type { ApplicationTransport } from '../applicationTransport'
import { controlFailureMessage } from './control-failure'
import { t } from '../i18n'

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
/** A command made while the last one is still confirming — its window spent, the station's re-read
 * not yet answered — is a lapse: it waits for that re-read and, with none landing, is refused as not
 * sent. The state is held through the gap (operator ruling 2026-09-16), so the refusal is no longer
 * the instant `notController` of a dropped state. */
async function refusedWhileConfirming(h: ReturnType<typeof setup>, action: Parameters<OperationClient['control']>[0]) {
  const later = h.client.control(action).catch(e => e as Error)
  await h.advance(1600)
  expect(await later).toMatchObject({ message: 'notController', sent: false })
}
/** The epoch the station reports while these tests run, and one FT gesture that carries it. */
const FT_EPOCH = '000000000000002a'
const ftCall = { action: 'ft.call', expectedTier: 'FT8', transmitEpoch: FT_EPOCH,
  selection: { call: 'W1AW', grid: null, message: null, snr: null, freq: null } } as const
/** A browser one command later: the control stayed lit, its command window is spent, and the
 * station's re-read is in flight. This is the moment an FT gesture lands in — measured at 400-670 ms
 * in the compiled browser, because the re-read waits for a free slot in the request budget. */
async function confirming() {
  const h = setup(storage(), ['amplifier', 'ftOperate', 'ftCall', 'ftExchange'], 4)
  await h.advance(1000)
  const heartbeat = h.sent[h.sent.length - 1].request
  expect(heartbeat.type).toBe('heartbeat')
  h.client.receive({ type: 'operationResponse', requestId: heartbeat.requestId, value: { ...h.state, transmitEpoch: FT_EPOCH } })
  const first = h.client.control(action)
  await Promise.resolve(); await Promise.resolve()
  h.reply({ operation: 'stationControl', operationId: h.sent[h.sent.length - 1].request.requestId, outcome: 'applied', evidence: 'stationState' })
  expect(await first).toMatchObject({ outcome: 'applied' })
  const reread = await nextHeartbeat(h)
  const commands = () => h.sent.filter(w => w.request.type === 'stationControl')
  expect(commands()).toHaveLength(1)
  expect(h.client.getSnapshot()).toMatchObject({ fresh: false, state: { phase: 'controlling' } })
  const fresh = { ...h.state, transmitEpoch: FT_EPOCH, revision: 2, commandWindowId: crypto.randomUUID(), nextSequence: 2 }
  return { h, commands, fresh, land: (value: unknown = fresh) => h.client.receive({ type: 'operationResponse', requestId: reread.requestId, value }) }
}

// THE FT HOLD — operator approval 2026-09-17. An FT gesture made while the last command was still
// confirming used to be refused at once ("a transmit action carries its epoch and never waits out a
// lapse"), which an operator read as "Not sent" on a control that was still lit: every call, CQ,
// TX on/off, Resend or free text that followed another command inside the station's re-read. It now
// waits that re-read out, at most CONTROL_RESUME_MS, and is re-checked against the fresh state
// before anything leaves. The client-side wait only: no station sequencing, slot or parity change.
it('holds an FT gesture through the post-command lapse and sends it once, on the fresh window', async () => {
  const { h, commands, fresh, land } = await confirming()
  const call = h.client.control(ftCall)
  await h.advance(100)
  expect(commands(), 'nothing leaves on the spent window').toHaveLength(1)
  land(); await h.advance(10)
  expect(commands()).toHaveLength(2)
  expect(commands()[1].request).toMatchObject({ commandWindowId: fresh.commandWindowId, clientSequence: 2, expectedRevision: 2 })
  expect(commands()[1].request.action).toMatchObject({ action: 'ft.call', transmitEpoch: FT_EPOCH })
  h.reply({ operation: 'stationControl', operationId: commands()[1].request.requestId, outcome: 'applied', evidence: 'stationState' })
  expect(await call).toMatchObject({ outcome: 'applied' })
  h.client.disconnected()
})

// A RESEND (and free text, and Monitor) is a PREPARED gesture: it captures the command window it
// will go out on, so its wait has to happen before that capture — in the transport, where the FT
// value edits already waited. Same hold, same single command, same epoch.
it('holds a prepared FT gesture — Resend — the same way, and sends it on the fresh window', async () => {
  const { h, commands, fresh, land } = await confirming()
  const reads = { kind: 'remote', invoke: vi.fn(async () => ({ link: { tier: 'FT8' }, radio: { dialMhz: 7.074 } })) } as unknown as ApplicationTransport
  const transport = controlTransport(reads, { age: () => 0 } as unknown as ApplicationClient, h.client)
  const resend = transport.invoke('qso_resend', { expectedQso: { dxcall: 'W1AW', state: 'done', txNow: null, cqRunning: false } })
  await h.advance(100)
  expect(commands(), 'nothing leaves on the spent window').toHaveLength(1)
  land(); await h.advance(10)
  expect(commands()).toHaveLength(2)
  expect(commands()[1].request).toMatchObject({ commandWindowId: fresh.commandWindowId, expectedRevision: 2 })
  expect(commands()[1].request.action).toMatchObject({ action: 'ft.exchange', transmitEpoch: FT_EPOCH, change: { kind: 'resend' } })
  h.reply({ operation: 'stationControl', operationId: commands()[1].request.requestId, outcome: 'applied', evidence: 'stationState' })
  await h.advance(100)
  expect(await resend).toMatchObject({ link: { tier: 'FT8' } })
  h.client.disconnected()
})

// THE LOAD-BEARING HALF: a held gesture must never reach the wire after the operator has stood the
// transmitter down, after the station's epoch moved under it, or after the wait ran out. Each case
// asserts the refusal AND that nothing was sent — the hold is the only thing that changed.
it('control: a held FT gesture is refused with nothing sent — on a Stop, a TX-off, a moved epoch, an over-long lapse, and never twice', async () => {
  const txOff = { action: 'ft.txEnabled', expectedTier: 'FT8', transmitEpoch: FT_EPOCH, on: false } as const
  {
    // Stop, pressed while the gesture waits. Nothing of that gesture may follow it onto the wire.
    const { h, commands, land } = await confirming()
    const call = h.client.control(ftCall).catch(e => e)
    await h.advance(10)
    void h.client.stopTransmit().catch(() => {})
    land(); await h.advance(10)
    expect(await call).toMatchObject({ message: 'staleContext', sent: false })
    expect(commands()).toHaveLength(1)
    h.client.disconnected()
  }
  {
    // TX off, pressed while the gesture waits: the stand-down goes out, the held call does not.
    const { h, commands, land } = await confirming()
    const call = h.client.control(ftCall).catch(e => e)
    await h.advance(10)
    const off = h.client.control(txOff)
    land(); await h.advance(10)
    expect(await call).toMatchObject({ message: 'staleContext', sent: false })
    expect(commands()).toHaveLength(2)
    expect(commands()[1].request.action).toMatchObject({ action: 'ft.txEnabled', on: false })
    h.reply({ operation: 'stationControl', operationId: commands()[1].request.requestId, outcome: 'applied', evidence: 'stationState' })
    expect(await off).toMatchObject({ outcome: 'applied' })
    h.client.disconnected()
  }
  {
    // The station's transmit epoch moved while the gesture waited: the checks at the send are
    // untouched, and they are what refuses it.
    const { h, commands, fresh, land } = await confirming()
    const call = h.client.control(ftCall).catch(e => e)
    await h.advance(10)
    land({ ...fresh, transmitEpoch: '000000000000002b' }); await h.advance(10)
    expect(await call).toMatchObject({ message: 'staleContext', sent: false })
    expect(commands()).toHaveLength(1)
    h.client.disconnected()
  }
  {
    // The re-read never lands inside CONTROL_RESUME_MS: refused, and never sent late when it does.
    const { h, commands, land } = await confirming()
    const call = h.client.control(ftCall).catch(e => e)
    await h.advance(1600)
    expect(await call).toMatchObject({ message: 'notController', sent: false, busy: false })
    expect(commands()).toHaveLength(1)
    land(); await h.advance(300)
    expect(commands(), 'a refused gesture is never replayed').toHaveLength(1)
    h.client.disconnected()
  }
  {
    // Two clicks inside one hold are one transmission: the second is refused, not queued.
    const { h, commands, land } = await confirming()
    const first = h.client.control(ftCall)
    const second = h.client.control(ftCall).catch(e => e)
    await h.advance(10)
    land(); await h.advance(10)
    expect(commands(), 'one gesture, one command').toHaveLength(2)
    expect(await second).toMatchObject({ message: 'operationUnknown', sent: false })
    h.reply({ operation: 'stationControl', operationId: commands()[1].request.requestId, outcome: 'applied', evidence: 'stationState' })
    expect(await first).toMatchObject({ outcome: 'applied' })
    h.client.disconnected()
  }
})

/** The station's re-read after a command goes out on the next tick the request budget admits. */
async function nextHeartbeat(h: ReturnType<typeof setup>) {
  for (let i = 0; i < 8 && h.sent[h.sent.length - 1].request.type !== 'heartbeat'; i++) await h.advance(250)
  const request = h.sent[h.sent.length - 1].request
  expect(request.type).toBe('heartbeat')
  return request
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
  await refusedWhileConfirming(h, { action: 'radio.mode', mode: 'cw', followFrequency: true })
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
  await refusedWhileConfirming(h, { action: 'radio.frequency', dialMhz: 7.074, band: '40m', sideband: 'USB' })
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
  await refusedWhileConfirming(h, { action: 'radio.mode', mode: 'cw', followFrequency: true })
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
  relay.sync({ peer: station, supported: true }, [{ peer: browser, sessionId: 'browser', deviceId: crypto.randomUUID(), commandUntil: Number.MAX_SAFE_INTEGER }], 1000)
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

// A confirmed command CONSUMES its window: the station clears every command window when it answers
// a control. The client used to enforce that by dropping its state, which greyed every held control
// out for a second after every click (operator ruling 2026-09-16: a control stays lit while it
// confirms). The state is now kept, so the refusal of a second command from the spent window has to
// hold on its own — "the button was disabled" is no longer evidence. Both directions are asserted:
// nothing leaves on the spent window, and the same command goes out once on the re-read's window.
it('keeps the state through a confirmed command, refuses a second command from its spent window, and sends it on the next', async () => {
  const transmitEpoch = '000000000000002a'
  const confirmed = async () => {
    const h = setup(storage(), ['amplifier', 'ftOperate'], 4)
    await h.advance(1000)
    const heartbeat = h.sent[h.sent.length - 1].request
    expect(heartbeat.type).toBe('heartbeat')
    h.client.receive({ type: 'operationResponse', requestId: heartbeat.requestId, value: { ...h.state, transmitEpoch } })
    // A gesture prepared on this window, before the command spends it.
    const prepared = h.client.prepareControl()
    const first = h.client.control(action)
    await Promise.resolve(); await Promise.resolve()
    const request = h.sent[h.sent.length - 1].request
    expect(request).toMatchObject({ type: 'stationControl', commandWindowId: h.state.commandWindowId, clientSequence: 1, expectedRevision: 1 })
    h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'stationState' })
    expect(await first).toMatchObject({ outcome: 'applied' })
    // The state is held — this browser still controls — but its window is spent.
    expect(h.client.getSnapshot()).toMatchObject({ fresh: false, controlRefreshing: true, controlPending: null,
      state: { phase: 'controlling', commandWindowId: h.state.commandWindowId } })
    // The 250 ms tick must not revive the spent window; it does ask the station for the re-read.
    const reread = await nextHeartbeat(h)
    expect(h.client.getSnapshot()).toMatchObject({ fresh: false, state: { phase: 'controlling' } })
    const commands = () => h.sent.filter(w => w.request.type === 'stationControl')
    expect(commands()).toHaveLength(1)
    return { h, prepared, reread, commands }
  }
  {
    // A transmit action carries its epoch and, since the operator's approval of 2026-09-17, waits
    // this lapse out exactly as the ordinary command below does. The rule it replaces was "a
    // transmit action never waits out a lapse: refused at once, not sent", and what an operator saw
    // for it was "Not sent" on a control that was still lit. Nothing leaves on the spent window
    // either way; the hold's own cancels — a Stop, TX off, a moved epoch, a lapse that outlasts the
    // wait, and never twice — are the control beside "holds an FT gesture through the post-command
    // lapse", which is where that half is asserted.
    const { h, commands, reread } = await confirmed()
    const transmit = h.client.control({ action: 'ft.txEnabled', expectedTier: 'FT8', transmitEpoch, on: true })
    await h.advance(10)
    expect(commands(), 'nothing on the spent window').toHaveLength(1)
    const next = { ...h.state, transmitEpoch, revision: 2, commandWindowId: crypto.randomUUID(), nextSequence: 2 }
    h.client.receive({ type: 'operationResponse', requestId: reread.requestId, value: next })
    await h.advance(10)
    expect(commands()).toHaveLength(2)
    expect(commands()[1].request).toMatchObject({ commandWindowId: next.commandWindowId, clientSequence: 2, expectedRevision: 2 })
    h.reply({ operation: 'stationControl', operationId: commands()[1].request.requestId, outcome: 'applied', evidence: 'stationState' })
    expect(await transmit).toMatchObject({ outcome: 'applied' })
    h.client.disconnected()
  }
  {
    // An ordinary command inside the confirming window is a lapse: it waits for the re-read and
    // leaves once on the NEW window, never the spent one. The gesture prepared on the spent window
    // is refused when it fires — the revision it captured is gone.
    const { h, prepared, reread, commands } = await confirmed()
    const second = h.client.control(action)
    await h.advance(100)
    expect(commands()).toHaveLength(1)
    const next = { ...h.state, transmitEpoch, revision: 2, commandWindowId: crypto.randomUUID(), nextSequence: 2 }
    h.client.receive({ type: 'operationResponse', requestId: reread.requestId, value: next })
    await h.advance(10)
    expect(commands()).toHaveLength(2)
    expect(commands()[1].request).toMatchObject({ commandWindowId: next.commandWindowId, clientSequence: 2, expectedRevision: 2 })
    h.reply({ operation: 'stationControl', operationId: commands()[1].request.requestId, outcome: 'applied', evidence: 'stationState' })
    expect(await second).toMatchObject({ outcome: 'applied' })
    const again = await nextHeartbeat(h)
    h.client.receive({ type: 'operationResponse', requestId: again.requestId, value: { ...next, revision: 3, nextSequence: 3 } })
    expect(h.client.getSnapshot()).toMatchObject({ fresh: true, state: { revision: 3 } })
    await expect(prepared(action)).rejects.toMatchObject({ message: 'staleContext', sent: false })
    expect(commands()).toHaveLength(2)
    h.client.disconnected()
  }
  {
    // The re-read never lands: the command made in the confirming window is refused as not sent.
    const { h, commands } = await confirmed()
    const second = h.client.control(action).catch(e => e)
    await h.advance(1600)
    expect(await second).toMatchObject({ message: 'notController', sent: false, busy: false })
    expect(commands()).toHaveLength(1)
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

it('records whether a failed control left the browser, and sends nothing for one refused before sending', async () => {
  // Positive control for the transport spy: this request IS sent, and the station refuses it.
  const answered = setup()
  const refused = answered.client.control(action).catch(e => e)
  await Promise.resolve(); await Promise.resolve()
  const request = answered.sent.filter(w => w.request.type === 'stationControl')
  expect(request).toHaveLength(1)
  answered.client.receive({ type: 'operationResponse', requestId: request[0].request.requestId, error: 'staleContext' })
  expect(await refused).toMatchObject({ message: 'staleContext', sent: true })
  expect(answered.client.getSnapshot().controlError).toEqual({ code: 'staleContext', sent: true, busy: false })
  answered.client.disconnected()

  // A heartbeat in flight outlives the click's freshness window: refused before anything is sent.
  const h = setup()
  await h.advance(1000)
  expect(h.sent[h.sent.length - 1].request.type).toBe('heartbeat')
  const dropped = h.client.control(action).catch(e => e)
  await h.advance(250)
  expect(await dropped).toMatchObject({ message: 'windowExpired', sent: false })
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
  expect(h.client.getSnapshot().controlError).toEqual({ code: 'windowExpired', sent: false, busy: false })
  h.client.disconnected()
})

it('marks a Log QSO gesture refused before sending as not sent', async () => {
  const h = setup(storage(), ['qsoLogging'], 4)
  await h.advance(1000)
  expect(h.sent[h.sent.length - 1].request.type).toBe('heartbeat')
  const reads = { kind: 'remote', invoke: vi.fn() } as unknown as ApplicationTransport
  const transport = controlTransport(reads, { age: () => Infinity } as unknown as ApplicationClient, h.client)
  const dropped = transport.invoke('log_current_qso', { expectedKey: '0'.repeat(31) + '1', expectedTier: 'FT8',
    expectedQso: { dxcall: 'W1AW', state: 'done', txNow: null, cqRunning: false } }).catch(e => e)
  await h.advance(250)
  expect(await dropped).toMatchObject({ message: 'windowExpired', sent: false })
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
  h.client.disconnected()
})

// THE REFUSAL BEHIND THE BUTTON. The Log button now waits for the sample that carries this QSO's
// key (QsoLoggingControls.test.tsx), and that is where the race is fixed. The refusal under it has
// to stay exactly as it was: a gesture that reaches the transport without a key is refused before
// anything is sent, and it is refused OUT LOUD — an OperationFailure the caller says, never a
// silent no-op.
it('control: a Log QSO gesture with no key is still refused, unsent, and says so', async () => {
  const h = setup(storage(), ['qsoLogging'], 4)
  await h.advance(1000)
  const reads = { kind: 'remote', invoke: vi.fn() } as unknown as ApplicationTransport
  const transport = controlTransport(reads, { age: () => Infinity } as unknown as ApplicationClient, h.client)
  const refused = await transport.invoke('log_current_qso', { expectedKey: null, expectedTier: 'FT8',
    expectedQso: { dxcall: 'W1AW', state: 'done', txNow: null, cqRunning: false } }).catch(e => e)
  expect(refused).toBeInstanceOf(OperationFailure)
  expect(refused).toMatchObject({ message: 'invalidOperation', sent: false })
  expect(controlFailureMessage(refused), 'the operator is told, not left guessing').toBe(t('remote.controlNotSent'))
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
  h.client.disconnected()
})

it('carries a stationBusy refusal as busy data, and keeps a sent command with no answer unconfirmed', async () => {
  // The station refused before anything changed: its own failure kind, not "not confirmed".
  const h = setup()
  const busy = h.client.control(action).catch(e => e)
  await Promise.resolve(); await Promise.resolve()
  const request = h.sent.filter(w => w.request.type === 'stationControl')
  expect(request).toHaveLength(1)
  h.client.receive({ type: 'operationResponse', requestId: request[0].request.requestId, error: 'stationBusy' })
  expect(await busy).toMatchObject({ message: 'stationBusy', sent: true, busy: true })
  expect(h.client.getSnapshot().controlError).toEqual({ code: 'stationBusy', sent: true, busy: true })
  h.client.disconnected()

  // Sent and never answered: still "not confirmed", never busy.
  const lost = setup()
  const unanswered = lost.client.control(action).catch(e => e)
  await Promise.resolve(); await Promise.resolve()
  expect(lost.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  await lost.advance(7500)
  expect(await unanswered).toMatchObject({ message: 'operationUnknown', sent: true, busy: false })
  expect(lost.client.getSnapshot().controlError).toEqual({ code: 'operationUnknown', sent: true, busy: false })
  lost.client.disconnected()
})

it('passes a rejected stationBusy outcome on as busy data', async () => {
  const h = setup(storage(), ['frequency'])
  const reads = { kind: 'remote', invoke: vi.fn() } as unknown as ApplicationTransport
  const transport = controlTransport(reads, { age: () => Infinity } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('set_frequency', { dialMhz: 7.074, band: '40m', mode: 'USB' }).catch(e => e)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(request.type).toBe('stationControl')
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'rejected', reason: 'stationBusy' })
  expect(await result).toMatchObject({ message: 'stationBusy', sent: true, busy: true })
  h.client.disconnected()
})

it('sends a typed FT offset only once control is current again, and refuses it as not sent if control stays stale', async () => {
  const transmitEpoch = '000000000000002a'
  const expected = { key: '1'.padStart(32, '0'), txOffsetHz: 1500, rxOffsetHz: 1500, holdTxFreq: false, txEven: true, txCycleAuto: true }
  const lapsed = async () => {
    const h = setup(storage(), ['ftSettings'], 4)
    await h.advance(1000)
    const first = h.sent[h.sent.length - 1].request
    expect(first.type).toBe('heartbeat')
    h.client.receive({ type: 'operationResponse', requestId: first.requestId, value: { ...h.state, transmitEpoch } })
    // The next heartbeat goes unanswered: past its 1200 ms window control is stale, the lease still held.
    await h.advance(1000)
    expect(h.sent[h.sent.length - 1].request.type).toBe('heartbeat')
    await h.advance(250)
    expect(h.client.getSnapshot()).toMatchObject({ fresh: false, state: { phase: 'controlling' } })
    const reads = { kind: 'remote', invoke: vi.fn(async () => ({})) } as unknown as ApplicationTransport
    return { h, transport: controlTransport(reads, { age: () => 0 } as unknown as ApplicationClient, h.client) }
  }
  const heartbeats = (h: ReturnType<typeof setup>) => h.sent.filter(w => w.request.type === 'heartbeat')
  const commands = (h: ReturnType<typeof setup>) => h.sent.filter(w => w.request.type === 'stationControl')
  {
    const { h, transport } = await lapsed()
    void transport.invoke('set_tx_offset', { hz: 1800, expectedTier: 'FT8', expected }).catch(() => {})
    await h.advance(100)
    // Positive control for the spy: it recorded both real heartbeats. No station command while stale.
    expect(heartbeats(h)).toHaveLength(2)
    expect(commands(h)).toHaveLength(0)
    const pending = heartbeats(h)[1].request
    h.client.receive({ type: 'operationResponse', requestId: pending.requestId, value: { ...h.state, transmitEpoch } })
    await h.advance(10)
    expect(commands(h)).toHaveLength(1)
    expect(commands(h)[0].request.action).toMatchObject({ action: 'ft.setting', change: { kind: 'txOffset', hz: 1800 } })
    h.client.disconnected()
  }
  {
    const { h, transport } = await lapsed()
    const typed = transport.invoke('set_tx_offset', { hz: 1800, expectedTier: 'FT8', expected }).catch(e => e)
    await h.advance(1600)
    expect(await typed).toMatchObject({ message: 'notController', sent: false, busy: false })
    expect(heartbeats(h)).toHaveLength(2)
    expect(commands(h)).toHaveLength(0)
    h.client.disconnected()
  }
})

// Remote parity batch 1: the AI-CW switch and the FT Decode button.
function decoderGesture(capabilities: ControlCapability[], version: OperationVersion, snapshot: any) {
  const h = setup(storage(), capabilities, version)
  const sample = { age: Infinity, value: snapshot }
  const invoke = vi.fn(async () => sample.value)
  const transport = controlTransport({ kind: 'remote', invoke } as unknown as ApplicationTransport,
    { age: () => sample.age } as unknown as ApplicationClient, h.client)
  const commands = () => h.sent.filter(w => w.request.type === 'stationControl')
  return { h, sample, invoke, transport, commands }
}

it('sends one AI-CW choice bound to the displayed state and returns only a later sample showing it', async () => {
  const g = decoderGesture(['aiCw'], 3, { aiCw: { enabled: false, status: '', text: '' }, link: { tier: 'FT8' } })
  const result = g.transport.invoke('set_ai_cw', { on: true })
  await g.h.advance(0)
  expect(g.commands()).toHaveLength(1)
  const request = g.commands()[0].request
  expect(request.action).toEqual({ action: 'decoder.aiCw', expectedOn: false, on: true })
  g.h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'settingsSaved' })
  await g.h.advance(100)
  expect(g.invoke).toHaveBeenCalledTimes(1)
  g.sample.value = { aiCw: { enabled: true, status: '', text: '' }, link: { tier: 'FT8' } }
  g.sample.age = 0
  await g.h.advance(50)
  expect(await result).toEqual(g.sample.value)
  expect(g.commands()).toHaveLength(1)
  g.h.client.disconnected()
})

it.each(['readback', 'evidence'])('refuses an AI-CW choice after a mismatched %s without sending it again', async changed => {
  const g = decoderGesture(['aiCw'], 3, { aiCw: { enabled: false, status: '', text: '' } })
  g.sample.age = 0
  const result = g.transport.invoke('set_ai_cw', { on: true }).catch(e => e)
  await g.h.advance(0)
  const request = g.commands()[0].request
  g.h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: changed === 'evidence' ? 'receiverState' : 'settingsSaved' })
  expect(await result).toMatchObject({ message: changed === 'evidence' ? 'operationUnknown' : 'readingUnavailable' })
  await g.h.advance(1500)
  expect(g.commands()).toHaveLength(1)
  g.h.client.disconnected()
})

it('refuses malformed, unchanged, unhinted and older-station AI-CW choices before anything is sent', async () => {
  const shown = { aiCw: { enabled: false, status: '', text: '' } }
  const g = decoderGesture(['aiCw'], 3, shown)
  for (const bad of [{}, { on: 'yes' }, { on: true, extra: 1 }, { on: false }, undefined])
    await expect(g.transport.invoke('set_ai_cw', bad as Record<string, unknown>)).rejects.toMatchObject({ sent: false })
  expect(g.commands()).toHaveLength(0)
  g.h.client.disconnected()
  for (const [capabilities, version, message] of [[['decoder'], 3, 'notController'], [['aiCw'], 2, 'stationUnsupported']] as [ControlCapability[], OperationVersion, string][]) {
    const old = decoderGesture(capabilities, version, shown)
    await expect(old.transport.invoke('set_ai_cw', { on: true })).rejects.toMatchObject({ message })
    expect(old.commands()).toHaveLength(0)
    old.h.client.disconnected()
  }
})

it('sends one FT redecode for the displayed tier and returns a later station sample', async () => {
  const g = decoderGesture(['redecode'], 3, { link: { tier: 'FT4' } })
  const result = g.transport.invoke('redecode', {})
  await g.h.advance(0)
  const request = g.commands()[0].request
  expect(request.action).toEqual({ action: 'decoder.redecode', expectedTier: 'FT4' })
  g.h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'receiverState' })
  await g.h.advance(100)
  expect(g.invoke).toHaveBeenCalledTimes(1)
  g.sample.age = 0
  await g.h.advance(50)
  expect(await result).toEqual(g.sample.value)
  g.h.client.disconnected()
  const wrong = decoderGesture(['redecode'], 3, { link: { tier: 'FT8' } })
  wrong.sample.age = 0
  const refused = wrong.transport.invoke('redecode', {}).catch(e => e)
  await wrong.h.advance(0)
  const sent = wrong.commands()[0].request
  wrong.h.reply({ operation: 'stationControl', operationId: sent.requestId, outcome: 'applied', evidence: 'settingsSaved' })
  expect(await refused).toMatchObject({ message: 'operationUnknown' })
  wrong.h.client.disconnected()
})

it('refuses a redecode with arguments, off FT8/FT4, without its hint or on an older station', async () => {
  const g = decoderGesture(['redecode'], 3, { link: { tier: 'FT8' } })
  await expect(g.transport.invoke('redecode', { depth: 3 })).rejects.toMatchObject({ sent: false })
  g.h.client.disconnected()
  const msk = decoderGesture(['redecode'], 3, { link: { tier: 'MSK144' } })
  await expect(msk.transport.invoke('redecode', {})).rejects.toMatchObject({ message: 'invalidOperation', sent: false })
  expect(msk.commands()).toHaveLength(0)
  msk.h.client.disconnected()
  for (const [capabilities, version, message] of [[['decoder', 'receiverSettings'], 3, 'notController'], [['redecode'], 2, 'stationUnsupported']] as [ControlCapability[], OperationVersion, string][]) {
    const old = decoderGesture(capabilities, version, { link: { tier: 'FT8' } })
    await expect(old.transport.invoke('redecode', {})).rejects.toMatchObject({ message })
    expect(old.commands()).toHaveLength(0)
    old.h.client.disconnected()
  }
})

// Remote parity batch 1: split, XIT, VFO and RIT. Each is bound to the value the page displayed,
// needs the station's stationState answer, and returns only a later sample showing the change.
const splitSnapshot = (radio: Record<string, unknown> = {}) => ({ radio: { splitTxMhz: null, ritHz: 0, xitHz: 0, activeVfo: 'A', ...radio } })
it.each([
  ['set_split', { txMhz: 14.032 }, { action: 'radio.split', expectedTxMhz: null, txMhz: 14.032 }, 'splitTuning', { splitTxMhz: 14.032 }],
  ['set_split', { txMhz: null }, { action: 'radio.split', expectedTxMhz: 14.032, txMhz: null }, 'splitTuning', { splitTxMhz: null }],
  ['set_xit', { hz: -4000 }, { action: 'radio.xit', expectedHz: 0, hz: -4000 }, 'splitTuning', { xitHz: -4000 }],
  ['set_vfo', { vfo: 'B' }, { action: 'radio.vfo', expectedVfo: 'A', vfo: 'B' }, 'splitTuning', { activeVfo: 'B' }],
  ['set_rit', { hz: 10 }, { action: 'radio.rit', expectedHz: 0, hz: 10 }, 'ritTuning', { ritHz: 10 }]
] as const)('%s %j sends one bound intent and returns a later sample showing it', async (command, args, action, capability, after) => {
  const before = command === 'set_split' && args.txMhz === null ? splitSnapshot({ splitTxMhz: 14.032 }) : splitSnapshot()
  const g = decoderGesture([capability], 3, before)
  const result = g.transport.invoke(command, { ...args })
  await g.h.advance(0)
  expect(g.commands()).toHaveLength(1)
  const request = g.commands()[0].request
  expect(request.action).toEqual(action)
  g.h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'stationState' })
  await g.h.advance(100)
  expect(g.invoke).toHaveBeenCalledTimes(1)
  g.sample.value = splitSnapshot(after)
  g.sample.age = 0
  await g.h.advance(50)
  expect(await result).toEqual(g.sample.value)
  expect(g.commands()).toHaveLength(1)
  g.h.client.disconnected()
})

it.each(['readback', 'evidence', 'privileges'])('refuses a split after a mismatched %s without sending it again', async changed => {
  const g = decoderGesture(['splitTuning'], 3, splitSnapshot())
  g.sample.age = 0
  const result = g.transport.invoke('set_split', { txMhz: 14.020 }).catch(e => e)
  await g.h.advance(0)
  const request = g.commands()[0].request
  g.h.reply(changed === 'privileges'
    ? { operation: 'stationControl', operationId: request.requestId, outcome: 'rejected', reason: 'outsidePrivileges' }
    : { operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: changed === 'evidence' ? 'radioReadback' : 'stationState' })
  const error = await result
  expect(error).toMatchObject({ message: changed === 'evidence' ? 'operationUnknown' : changed === 'privileges' ? 'outsidePrivileges' : 'readingUnavailable' })
  if (changed === 'privileges') expect(controlFailureMessage(error)).toBe(t('remote.b1.outsidePrivileges'))
  await g.h.advance(1500)
  expect(g.commands()).toHaveLength(1)
  g.h.client.disconnected()
})

it('refuses malformed, unchanged, unhinted and older-station split and clarifier requests before sending', async () => {
  const g = decoderGesture(['splitTuning', 'ritTuning'], 3, splitSnapshot())
  for (const [command, bad] of [['set_split', {}], ['set_split', { txMhz: '14.032' }], ['set_split', { txMhz: 14.032, extra: 1 }], ['set_split', { txMhz: null }],
    ['set_rit', { hz: 0 }], ['set_rit', { hz: 10.5 }], ['set_xit', { hz: 10000 }], ['set_vfo', { vfo: 'A' }], ['set_vfo', { vfo: 'C' }], ['swap_vfo', undefined]] as const)
    await expect(g.transport.invoke(command, bad as Record<string, unknown> | undefined)).rejects.toMatchObject({ sent: false })
  expect(g.commands()).toHaveLength(0)
  g.h.client.disconnected()
  for (const [capabilities, version, message] of [[['ritTuning'], 3, 'notController'], [['splitTuning'], 2, 'stationUnsupported']] as [ControlCapability[], OperationVersion, string][]) {
    const old = decoderGesture(capabilities, version, splitSnapshot())
    await expect(old.transport.invoke('set_split', { txMhz: 14.032 })).rejects.toMatchObject({ message })
    expect(old.commands()).toHaveLength(0)
    old.h.client.disconnected()
  }
})

it.each(['FT8', 'FT4'] as const)('sends one %s digital Work intent and waits for the spot frequency, digital section and tier', async tier => {
  const h = setup(storage(), ['workDigitalSpot'], 3)
  let sampleAge = Infinity
  const snapshot = { radio: { operatingMode: 'digital', dialMhz: 14.0765, txEnabled: false }, link: { tier } }
  const getSnapshot = vi.fn(async () => snapshot)
  const transport = controlTransport({ kind: 'remote', invoke: getSnapshot } as ApplicationTransport,
    { age: () => sampleAge } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('work_spot', { mode: 'digital', freqMhz: 14.0765, band: '20m', call: 'JA2DEF/P', tier })
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(request.action).toEqual({ action: 'radio.workDigitalSpot', tier, dialMhz: 14.0765, band: '20m', call: 'JA2DEF/P' })
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' })
  await h.advance(100)
  expect(getSnapshot).not.toHaveBeenCalled()
  sampleAge = 0
  await h.advance(50)
  expect(await result).toEqual(snapshot)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it.each(['tier', 'section', 'frequency', 'evidence'])('refuses a digital Work handoff after a mismatched %s without replaying it', async changed => {
  const h = setup(storage(), ['workDigitalSpot'], 3)
  const snapshot = { radio: { operatingMode: changed === 'section' ? 'cw' : 'digital', dialMhz: changed === 'frequency' ? 14.074 : 14.080 }, link: { tier: changed === 'tier' ? 'FT8' : 'FT4' } }
  const transport = controlTransport({ kind: 'remote', invoke: vi.fn(async () => snapshot) } as ApplicationTransport,
    { age: () => 0 } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('work_spot', { mode: 'digital', freqMhz: 14.080, band: '20m', call: 'JA2DEF', tier: 'FT4' }).catch(e => e)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: changed === 'evidence' ? 'stationState' : 'radioReadback' })
  expect(await result).toMatchObject({ message: changed === 'evidence' ? 'operationUnknown' : 'readingUnavailable' })
  await h.advance(1500)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it('sends one RTTY Work intent and waits for the spot frequency in the RTTY section', async () => {
  const h = setup(storage(), ['workRttySpot'], 3)
  let sampleAge = Infinity
  const snapshot = { radio: { operatingMode: 'rtty', dialMhz: 14.0865, txEnabled: false } }
  const getSnapshot = vi.fn(async () => snapshot)
  const transport = controlTransport({ kind: 'remote', invoke: getSnapshot } as ApplicationTransport,
    { age: () => sampleAge } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('work_spot', { mode: 'rtty', freqMhz: 14.0865, band: '20m', call: 'JA2DEF/P', tier: null })
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(request.action).toEqual({ action: 'radio.workRttySpot', dialMhz: 14.0865, band: '20m', call: 'JA2DEF/P' })
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' })
  await h.advance(100)
  expect(getSnapshot).not.toHaveBeenCalled()
  sampleAge = 0
  await h.advance(50)
  expect(await result).toEqual(snapshot)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it.each(['section', 'frequency', 'evidence'])('refuses an RTTY Work handoff after a mismatched %s without replaying it', async changed => {
  const h = setup(storage(), ['workRttySpot'], 3)
  const snapshot = { radio: { operatingMode: changed === 'section' ? 'digital' : 'rtty', dialMhz: changed === 'frequency' ? 14.074 : 14.0865 } }
  const transport = controlTransport({ kind: 'remote', invoke: vi.fn(async () => snapshot) } as ApplicationTransport,
    { age: () => 0 } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('work_spot', { mode: 'rtty', freqMhz: 14.0865, band: '20m', call: 'JA2DEF', tier: null }).catch(e => e)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: changed === 'evidence' ? 'stationState' : 'radioReadback' })
  expect(await result).toMatchObject({ message: changed === 'evidence' ? 'operationUnknown' : 'readingUnavailable' })
  await h.advance(1500)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it('refuses RTTY Work on older stations, without its own hint, or carrying a tier', async () => {
  // An older desktop offers no workRttySpot, and workSpot alone must not stand in for it: a
  // station that never learned RTTY Work is simply not offered the control.
  for (const [capabilities, version, message] of [[['workRttySpot'], 2, 'stationUnsupported'], [['workSpot', 'workDigitalSpot'], 3, 'notController']] as [ControlCapability[], OperationVersion, string][]) {
    const h = setup(storage(), capabilities, version)
    const transport = controlTransport({ kind: 'remote', invoke: vi.fn() } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
    await expect(transport.invoke('work_spot', { mode: 'rtty', freqMhz: 14.0865, band: '20m', call: 'JA2DEF', tier: null })).rejects.toThrow(message)
    expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
    h.client.disconnected()
  }
  const h = setup(storage(), ['workRttySpot'], 3)
  const invoke = vi.fn(), transport = controlTransport({ kind: 'remote', invoke } as ApplicationTransport, {} as ApplicationClient, h.client)
  const args = { mode: 'rtty', freqMhz: 14.0865, band: '20m', call: 'JA2DEF', tier: null }
  for (const bad of [{ ...args, tier: 'FT8' }, { ...args, call: null }, { ...args, band: '21m' }, { ...args, freqMhz: 0 },
    { ...args, splitUpKhz: 2 }, { ...args, txEnabled: true }]) {
    await expect(transport.invoke('work_spot', bad)).rejects.toThrow()
  }
  expect(invoke).not.toHaveBeenCalled()
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
  h.client.disconnected()
})

const RECALLS = [
  ['CW', { section: 'cw', dialMhz: 14.06, band: '20m', sideband: null, fm: null },
    { action: 'radio.memoryRecall', section: 'cw', dialMhz: 14.06, band: '20m', sideband: null }, 'cw'],
  ['FM repeater', { section: 'phone', dialMhz: 146.94, band: '2m', sideband: null, fm: { shift: 'minus', offsetHz: 600000, toneHz: 103.5 } },
    { action: 'radio.memoryRecall', section: 'phone', dialMhz: 146.94, band: '2m', sideband: null, fm: { shift: 'minus', offsetHz: 600000, toneHz: 103.5 } }, 'phone'],
] as [string, Record<string, unknown>, Record<string, unknown>, string][]

it.each(RECALLS)('sends one %s memory recall and waits for a later sample on its dial and section', async (_name, args, expected, section) => {
  const h = setup(storage(), ['memoryRecall'], 3)
  let sampleAge = Infinity
  const snapshot = { radio: { operatingMode: section, dialMhz: args.dialMhz, txEnabled: false } }
  const getSnapshot = vi.fn(async () => snapshot)
  const transport = controlTransport({ kind: 'remote', invoke: getSnapshot } as ApplicationTransport,
    { age: () => sampleAge } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('remote_recall_memory', args)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(request.action).toEqual(expected)
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' })
  await h.advance(100)
  expect(getSnapshot).not.toHaveBeenCalled()
  sampleAge = 0
  await h.advance(50)
  expect(await result).toEqual(snapshot)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it.each(['section', 'frequency', 'evidence'])('refuses a memory recall handoff after a mismatched %s without replaying it', async changed => {
  const h = setup(storage(), ['memoryRecall'], 3)
  const snapshot = { radio: { operatingMode: changed === 'section' ? 'digital' : 'cw', dialMhz: changed === 'frequency' ? 14.074 : 14.06 } }
  const transport = controlTransport({ kind: 'remote', invoke: vi.fn(async () => snapshot) } as ApplicationTransport,
    { age: () => 0 } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('remote_recall_memory', RECALLS[0][1]).catch(e => e)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: changed === 'evidence' ? 'stationState' : 'radioReadback' })
  expect(await result).toMatchObject({ message: changed === 'evidence' ? 'operationUnknown' : 'readingUnavailable' })
  await h.advance(1500)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it('refuses a memory recall on older stations, without its hint, or with unreviewed arguments', async () => {
  const args = RECALLS[1][1]
  for (const [capabilities, version, message] of [[['memoryRecall'], 2, 'stationUnsupported'], [['repeaterTuning', 'workSpot'], 3, 'notController']] as [ControlCapability[], OperationVersion, string][]) {
    const h = setup(storage(), capabilities, version)
    const transport = controlTransport({ kind: 'remote', invoke: vi.fn() } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
    await expect(transport.invoke('remote_recall_memory', args)).rejects.toThrow(message)
    expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
    h.client.disconnected()
  }
  const h = setup(storage(), ['memoryRecall'], 3)
  const invoke = vi.fn(), transport = controlTransport({ kind: 'remote', invoke } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
  for (const bad of [{ ...args, section: 'rtty' }, { ...args, sideband: 'USB' }, { ...args, fm: { shift: 'up', offsetHz: 0, toneHz: 0 } },
    { ...args, txEnabled: true }, { section: 'cw', dialMhz: 14.06, band: '20m', sideband: null }]) {
    await expect(transport.invoke('remote_recall_memory', bad)).rejects.toThrow()
  }
  expect(invoke).not.toHaveBeenCalled()
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
  h.client.disconnected()
})

it('sends one repeater tune naming the machine and waits for a later sample on its output', async () => {
  const h = setup(storage(), ['repeaterTuning'], 3)
  let sampleAge = Infinity
  const snapshot = { radio: { operatingMode: 'phone', dialMhz: 146.94, txEnabled: false } }
  const getSnapshot = vi.fn(async () => snapshot)
  const transport = controlTransport({ kind: 'remote', invoke: getSnapshot } as ApplicationTransport,
    { age: () => sampleAge } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('repeater_tune', { outputMhz: 146.94, shift: 'minus', offsetHz: 600000, toneHz: 100 })
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(request.action).toEqual({ action: 'radio.repeater', outputMhz: 146.94, shift: 'minus', offsetHz: 600000, toneHz: 100 })
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' })
  await h.advance(100)
  expect(getSnapshot).not.toHaveBeenCalled()
  sampleAge = 0
  await h.advance(50)
  expect(await result).toEqual(snapshot)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it.each(['frequency', 'evidence'])('refuses a repeater tune handoff after a mismatched %s without replaying it', async changed => {
  const h = setup(storage(), ['repeaterTuning'], 3)
  const snapshot = { radio: { operatingMode: 'phone', dialMhz: changed === 'frequency' ? 146.52 : 146.94 } }
  const transport = controlTransport({ kind: 'remote', invoke: vi.fn(async () => snapshot) } as ApplicationTransport,
    { age: () => 0 } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('repeater_tune', { outputMhz: 146.94, shift: 'minus', offsetHz: 0, toneHz: 0 }).catch(e => e)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: changed === 'evidence' ? 'stationState' : 'radioReadback' })
  expect(await result).toMatchObject({ message: changed === 'evidence' ? 'operationUnknown' : 'readingUnavailable' })
  await h.advance(1500)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it('refuses a repeater tune on older stations, without its hint, with unreviewed arguments, and names a licence refusal', async () => {
  const args = { outputMhz: 146.94, shift: 'minus', offsetHz: 600000, toneHz: 100 }
  for (const [capabilities, version, message] of [[['repeaterTuning'], 2, 'stationUnsupported'], [['frequency', 'workSpot'], 3, 'notController']] as [ControlCapability[], OperationVersion, string][]) {
    const h = setup(storage(), capabilities, version)
    const transport = controlTransport({ kind: 'remote', invoke: vi.fn() } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
    await expect(transport.invoke('repeater_tune', args)).rejects.toThrow(message)
    expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
    h.client.disconnected()
  }
  const h = setup(storage(), ['repeaterTuning'], 3)
  const invoke = vi.fn(), transport = controlTransport({ kind: 'remote', invoke } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
  for (const bad of [{ ...args, shift: 'up' }, { ...args, offsetHz: -1 }, { ...args, toneHz: 88.55 }, { ...args, outputMhz: 28.5 },
    { ...args, txEnabled: true }, { outputMhz: 146.94, shift: 'minus', offsetHz: 600000 }]) {
    await expect(transport.invoke('repeater_tune', bad)).rejects.toThrow()
  }
  expect(invoke).not.toHaveBeenCalled()
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
  const refused = transport.invoke('repeater_tune', args).catch(e => e)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'rejected', reason: 'outsidePrivileges' })
  expect(await refused).toMatchObject({ message: 'outsidePrivileges' })
  expect(invoke).not.toHaveBeenCalled()
  h.client.disconnected()
})

it('refuses digital Work on older stations, without its own hint, or with any other native argument', async () => {
  for (const [capabilities, version, message] of [[['workDigitalSpot'], 2, 'stationUnsupported'], [['workSpot'], 3, 'notController']] as [ControlCapability[], OperationVersion, string][]) {
    const h = setup(storage(), capabilities, version)
    const transport = controlTransport({ kind: 'remote', invoke: vi.fn() } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
    await expect(transport.invoke('work_spot', { mode: 'digital', freqMhz: 14.074, band: '20m', call: 'JA2DEF', tier: 'FT8' })).rejects.toThrow(message)
    expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
    h.client.disconnected()
  }
  const h = setup(storage(), ['workDigitalSpot', 'workSpot'], 3)
  const invoke = vi.fn(), transport = controlTransport({ kind: 'remote', invoke } as ApplicationTransport, {} as ApplicationClient, h.client)
  const args = { mode: 'digital', freqMhz: 14.074, band: '20m', call: 'JA2DEF', tier: 'FT8' }
  for (const bad of [{ ...args, mode: 'cw' }, { ...args, mode: 'phone' }, { ...args, mode: 'rtty' }, { ...args, tier: 'FT2' }, { ...args, tier: 'ft8' },
    { ...args, tier: null }, { ...args, call: null }, { ...args, band: '21m' }, { ...args, splitUpKhz: 2 }, { ...args, txEnabled: true }]) {
    await expect(transport.invoke('work_spot', bad)).rejects.toThrow()
  }
  expect(invoke).not.toHaveBeenCalled()
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
  h.client.disconnected()
})

it('sends one APRS tune to the regional channel and waits for a later sample on that dial', async () => {
  const h = setup(storage(), ['aprsTuning'], 3)
  let sampleAge = Infinity
  const snapshot = { radio: { operatingMode: 'digital', dialMhz: 144.8, txEnabled: false } }
  const getSnapshot = vi.fn(async () => snapshot)
  const transport = controlTransport({ kind: 'remote', invoke: getSnapshot } as ApplicationTransport,
    { age: () => sampleAge } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('aprs_tune', { dialMhz: 144.8 })
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(request.action).toEqual({ action: 'radio.aprsTune', dialMhz: 144.8 })
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' })
  await h.advance(100)
  expect(getSnapshot).not.toHaveBeenCalled()
  sampleAge = 0
  await h.advance(50)
  expect(await result).toEqual(snapshot)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it.each(['frequency', 'evidence'])('refuses an APRS tune handoff after a mismatched %s without replaying it', async changed => {
  const h = setup(storage(), ['aprsTuning'], 3)
  const snapshot = { radio: { operatingMode: 'digital', dialMhz: changed === 'frequency' ? 146.52 : 144.39 } }
  const transport = controlTransport({ kind: 'remote', invoke: vi.fn(async () => snapshot) } as ApplicationTransport,
    { age: () => 0 } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('aprs_tune', { dialMhz: 144.39 }).catch(e => e)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: changed === 'evidence' ? 'stationState' : 'radioReadback' })
  expect(await result).toMatchObject({ message: changed === 'evidence' ? 'operationUnknown' : 'readingUnavailable' })
  await h.advance(1500)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it('refuses an APRS tune on older stations, without its hint, or away from an APRS channel', async () => {
  for (const [capabilities, version, message] of [[['aprsTuning'], 2, 'stationUnsupported'], [['frequency', 'repeaterTuning'], 3, 'notController']] as [ControlCapability[], OperationVersion, string][]) {
    const h = setup(storage(), capabilities, version)
    const transport = controlTransport({ kind: 'remote', invoke: vi.fn() } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
    await expect(transport.invoke('aprs_tune', { dialMhz: 144.39 })).rejects.toThrow(message)
    expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
    h.client.disconnected()
  }
  const h = setup(storage(), ['aprsTuning'], 3)
  const invoke = vi.fn(), transport = controlTransport({ kind: 'remote', invoke } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
  for (const bad of [{ dialMhz: 146.52 }, { dialMhz: '144.39' }, { dialMhz: 144.39, band: '2m' }, {}, undefined]) {
    await expect(transport.invoke('aprs_tune', bad)).rejects.toThrow()
  }
  expect(invoke).not.toHaveBeenCalled()
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
  h.client.disconnected()
})

it('points the rotator by azimuth through a pending receipt and resolves on the station outcome, with no radio sample to wait for', async () => {
  const h = setup(storage(), ['rotator'], 3)
  const reads = vi.fn()
  const transport = controlTransport({ kind: 'remote', invoke: reads } as ApplicationTransport,
    { age: () => 0 } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('point_rotator', { azDeg: 123.44 })
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  // One decimal is what the station accepts; the page never sends more.
  expect(request.action).toEqual({ action: 'rotator.point', azimuthDeg: 123.4 })
  const receipt = { operation: 'stationControl', operationId: request.requestId }
  h.reply({ ...receipt, outcome: 'pending' })
  await Promise.resolve(); await Promise.resolve()
  await h.advance(250)
  h.reply({ ...h.state, revision: 2, nextSequence: 2 })
  await h.advance(250)
  expect(h.sent[h.sent.length - 1].request).toMatchObject({ type: 'result', operationId: request.requestId })
  h.reply({ ...receipt, outcome: 'applied', evidence: 'stationState' })
  expect(await result).toBeUndefined()
  expect(reads).not.toHaveBeenCalled()
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it.each([
  ['point_rotator_at_call', { call: 'JA1ABC' }, { action: 'rotator.pointAtCall', call: 'JA1ABC' }],
  ['stop_rotator', undefined, { action: 'rotator.stop' }],
  ['stop_rotator', {}, { action: 'rotator.stop' }],
  ['point_rotator', { azDeg: 359.97 }, { action: 'rotator.point', azimuthDeg: 0 }],
] as const)('maps %s to one rotator action and resolves with no bearing', async (command, args, action) => {
  const h = setup(storage(), ['rotator'], 3)
  const transport = controlTransport({ kind: 'remote', invoke: vi.fn() } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
  const result = transport.invoke(command, args as Record<string, unknown> | undefined)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(request.action).toEqual(action)
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'stationState' })
  expect(await result).toBeUndefined()
  h.client.disconnected()
})

it.each([
  [{ outcome: 'unknown', reason: 'hardwareUnconfirmed' }, 'operationUnknown'],
  [{ outcome: 'rejected', reason: 'hardwareUnavailable' }, 'hardwareUnavailable'],
  [{ outcome: 'applied', evidence: 'radioReadback' }, 'operationUnknown'],
] as const)('refuses a rotator outcome %o as %s without replaying it', async (outcome, message) => {
  const h = setup(storage(), ['rotator'], 3)
  const transport = controlTransport({ kind: 'remote', invoke: vi.fn() } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('stop_rotator', undefined).catch(e => e)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  h.reply({ operation: 'stationControl', operationId: request.requestId, ...outcome })
  expect(await result).toMatchObject({ message })
  await h.advance(1500)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it('sends no rotator command to an older station, without the hint, or with anything beside its fields, and leaves the heading read alone', async () => {
  for (const [capabilities, version, message] of [[['rotator'], 2, 'stationUnsupported'], [['frequency', 'aprsTuning'], 3, 'notController']] as [ControlCapability[], OperationVersion, string][]) {
    const h = setup(storage(), capabilities, version)
    const transport = controlTransport({ kind: 'remote', invoke: vi.fn() } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
    await expect(transport.invoke('stop_rotator', undefined)).rejects.toThrow(message)
    await expect(transport.invoke('point_rotator', { azDeg: 90 })).rejects.toThrow(message)
    expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
    h.client.disconnected()
  }
  const h = setup(storage(), ['rotator'], 3)
  const invoke = vi.fn(async () => null), transport = controlTransport({ kind: 'remote', invoke } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
  for (const [command, bad] of [['point_rotator', { azDeg: 90, elDeg: 10 }], ['point_rotator', { azDeg: '90' }], ['point_rotator', {}], ['point_rotator', undefined],
    ['point_rotator_at_call', { call: 'ja1abc' }], ['point_rotator_at_call', { call: 'JA1ABC', azDeg: 1 }], ['stop_rotator', { now: true }]] as [string, Record<string, unknown> | undefined][]) {
    await expect(transport.invoke(command, bad)).rejects.toThrow()
  }
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
  // The heading has no remote path: the read passes through untouched (and the reads refuse it).
  expect(invoke).not.toHaveBeenCalled()
  await transport.invoke('read_rotator')
  expect(invoke).toHaveBeenCalledWith('read_rotator', undefined)
  h.client.disconnected()
})

const SCOPE_SETTINGS = [
  ['set_scope_span', { hz: 25_000 }, { setting: 'span', hz: 25_000 }],
  ['set_scope_ref', { tenthsDb: -35 }, { setting: 'ref', tenthsDb: -35 }],
  ['set_flex_pan_span', { hz: 200_000 }, { setting: 'panSpan', hz: 200_000 }],
  ['set_flex_pan_ref', { refDbm: -80 }, { setting: 'panRef', refDbm: -80 }],
  ['set_flex_pan_ref', { refDbm: null }, { setting: 'panRef', refDbm: null }],
  // 0x41 'A' = W/F FIX (NORMAL): the later sample must show a FIX position.
  ['set_yaesu_scope_mode', { position: 'fix' }, { setting: 'position', position: 'fix' }],
] as const

it.each(SCOPE_SETTINGS)('maps %s to one rig scope setting and returns a later station sample', async (command, args, setting) => {
  const h = setup(storage(), ['rigScope'], 3)
  let sampleAge = Infinity
  const snapshot = { radio: { dialMhz: 14.2, scopeModeCode: 0x41 } }
  const getSnapshot = vi.fn(async () => snapshot)
  const transport = controlTransport({ kind: 'remote', invoke: getSnapshot } as ApplicationTransport,
    { age: () => sampleAge } as unknown as ApplicationClient, h.client)
  const result = transport.invoke(command, args)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(request.action).toEqual({ action: 'radio.scope', ...setting })
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'stationState' })
  await h.advance(100)
  expect(getSnapshot).not.toHaveBeenCalled()
  sampleAge = 0
  await h.advance(50)
  expect(await result).toEqual(snapshot)
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it.each(['position', 'evidence'])('refuses a rig scope change after a mismatched %s without sending it again', async changed => {
  const h = setup(storage(), ['rigScope'], 3)
  // 0x34 '4' = W/F CENTER (NORMAL): not the FIX the operator asked for.
  const snapshot = { radio: { dialMhz: 14.2, scopeModeCode: changed === 'position' ? 0x34 : 0x41 } }
  const transport = controlTransport({ kind: 'remote', invoke: vi.fn(async () => snapshot) } as ApplicationTransport,
    { age: () => 0 } as unknown as ApplicationClient, h.client)
  const result = transport.invoke('set_yaesu_scope_mode', { position: 'fix' })
  const settled = result.catch(error => error)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: changed === 'evidence' ? 'radioReadback' : 'stationState' })
  await h.advance(100)
  expect(((await settled) as Error).message).toBe(changed === 'position' ? 'readingUnavailable' : 'operationUnknown')
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it('refuses a rig scope change on older stations, without its hint, or with unreviewed arguments, and leaves Icom center/fixed alone', async () => {
  for (const [capabilities, version, message] of [[['rigScope'], 2, 'stationUnsupported'], [['frequency', 'receiverDsp'], 3, 'notController']] as [ControlCapability[], OperationVersion, string][]) {
    const h = setup(storage(), capabilities, version)
    const transport = controlTransport({ kind: 'remote', invoke: vi.fn(async () => ({ radio: {} })) } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
    for (const [command, args] of SCOPE_SETTINGS) await expect(transport.invoke(command, args)).rejects.toThrow(message)
    expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
    h.client.disconnected()
  }
  const h = setup(storage(), ['rigScope'], 3)
  const invoke = vi.fn(async () => null), transport = controlTransport({ kind: 'remote', invoke } as ApplicationTransport, { age: () => 0 } as unknown as ApplicationClient, h.client)
  for (const [command, bad] of [['set_scope_span', { hz: 2_400 }], ['set_scope_span', { hz: 25_000, band: '20m' }], ['set_scope_span', {}], ['set_scope_span', undefined],
    ['set_scope_ref', { tenthsDb: '0' }], ['set_yaesu_scope_mode', { position: 'middle' }], ['set_flex_pan_span', { hz: 1 }],
    ['set_flex_pan_ref', {}], ['set_flex_pan_ref', { refDbm: -80, auto: true }]] as [string, Record<string, unknown> | undefined][]) {
    await expect(transport.invoke(command, bad)).rejects.toThrow()
  }
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
  // Icom center/fixed has no desktop control and no remote action: it passes to the read allowlist.
  expect(invoke).not.toHaveBeenCalled()
  await transport.invoke('set_scope_fixed', { fixed: true })
  expect(invoke).toHaveBeenCalledWith('set_scope_fixed', { fixed: true })
  h.client.disconnected()
})
