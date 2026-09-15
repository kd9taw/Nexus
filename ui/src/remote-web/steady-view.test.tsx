// @vitest-environment jsdom
// STEADY CACHED VIEW (operator decision 2026-09-14). Over a real WAN a heartbeat reply lands after the
// 1.2 s control window, so station control is stale for a moment most seconds. That moment used to
// disable every station control (a native <select> closes when it is disabled, which is how an open band
// dropdown was cancelled) and drop the band dropdown's options. Now a station control stays usable
// through a brief lapse, its command waits for current control and is sent once, or is refused as not
// sent; nothing is sent while control is stale. Stale READINGS still refuse at once, and a transmit
// action is still refused at once rather than delayed.
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { BandPicker } from '../components/BandPicker'
import { RemoteOperationsContext, StationControlContext, StationDataContext, useStationCapability } from '../stationAccess'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { ApplicationClient } from './application-client'
import { STREAM_TOPICS } from './application-stream-protocol'
import type { OperationState } from './operation-protocol'
import type { ControlCapability } from './station-operation'
import type { AppSnapshot, BandChannel } from '../types'

const plan: BandChannel[] = [
  { band: '40m', dialMhz: 7.15, mode: 'LSB', label: '40m', group: 'HF', tx: true, note: '' },
  { band: '20m', dialMhz: 14.2, mode: 'USB', label: '20m', group: 'HF', tx: true, note: '' },
] as BandChannel[]
vi.mock('../api', async original => ({ ...await original<Record<string, unknown>>(),
  getLicensedBandPlan: vi.fn(async () => structuredClone(plan)), pickBand: vi.fn(async () => ({})) }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const clients: OperationClient[] = []
beforeAll(() => { globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver })
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks() })

async function step(ms = 10) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }
async function until(condition: () => boolean, limitMs = 5000) {
  for (let elapsed = 0; elapsed <= limitMs; elapsed += 10) { if (condition()) return; await step(10) }
  throw Error('condition not reached')
}
const record = () => ({ call: 'W1AW', grid: 'FN31', country: null, state: null, band: '20m', freqMhz: 14.25, mode: 'SSB', rstSent: '59',
  rstRcvd: '57', name: null, qth: null, comment: null, notes: 'Keep this contact', whenUnix: 1700000000, confirmed: false as const, awardConfirmed: false as const })

/** A station answering after a round trip (260 ms: 250 + the page taking the reply). It also applies a
 * station command and a manual log. The transport spy records every request that left the browser and
 * whether control was current then. */
function station(capabilities: ControlCapability[], extra: Partial<OperationState> = {}) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities }, ...extra }
  const link = { roundTripMs: 260, answering: true }
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const wire: { request: any; fresh: boolean }[] = []
  const values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client: OperationClient = new OperationClient(raw => {
    const { request } = JSON.parse(raw)
    wire.push({ request, fresh: client.getSnapshot().fresh })
    if (!link.answering) return
    const value = request.type === 'state' || request.type === 'heartbeat' ? state
      : request.type === 'stationControl' ? { operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'receiverState' }
      : request.type === 'logManual' ? { outcome: 'applied', evidence: 'fileSynced', uploads: 'stationPipeline', operationId: request.requestId } : null
    if (value) setTimeout(() => client.receive({ type: 'operationResponse', requestId: request.requestId, value }), link.roundTripMs)
  }, true, () => 1000 + performance.now(), undefined, 4, pendingControlStorage(() => storage, 'steady-view', async (_key, run) => run()))
  clients.push(client)
  let lapses = 0, wasFresh = false
  client.subscribe(() => {
    const view = client.getSnapshot()
    if (wasFresh && !view.fresh && view.state?.phase === 'controlling') lapses++
    wasFresh = view.fresh
  })
  client.open()
  const sent = (type: string) => wire.filter(w => w.request.type === type)
  return { client, link, wire, lapses: () => lapses, heartbeats: () => sent('heartbeat').length, writes: () => sent('stationControl'), logs: () => sent('logManual') }
}
/** Hold one heartbeat reply to 900 ms: control goes stale for about 650 ms with the lease still held. */
async function longLapse(h: ReturnType<typeof station>) {
  await until(() => h.client.getSnapshot().fresh)
  h.link.roundTripMs = 900
  await until(() => !h.client.getSnapshot().fresh)
  expect(h.client.getSnapshot().state?.phase).toBe('controlling')
}

it('a band dropdown stays enabled and keeps its options through every control lapse', async () => {
  const h = station(['bandSelection'])
  const snap = { activeRadioId: 1, radio: { source: 'native', operatingMode: 'phone', dialMhz: 7.15, band: '40m', sideband: 'LSB', catOk: true,
    txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, txAllowed: true } } as unknown as AppSnapshot
  const ui = render(<StationControlContext.Provider value={false}><StationDataContext.Provider value>
    <RemoteOperationsContext.Provider value={h.client}><BandPicker snap={snap} mode="phone"/></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  // The band dropdown is a Nexus menu (BandMenu.tsx). OPEN it first: the lapse this pins used to
  // disable the control, and a disabled control closes its open list under the operator.
  const trigger = () => ui.container.querySelector<HTMLButtonElement>('.band-menu-trigger')!
  const menu = () => document.querySelector<HTMLElement>('[role="menu"]')
  const items = () => menu()?.querySelectorAll('[role="menuitemradio"]').length ?? 0
  await until(() => h.client.getSnapshot().fresh && !trigger().disabled)
  act(() => { trigger().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })) })
  await until(() => items() === 2)
  let toggles = 0, removed = 0
  const observer = new MutationObserver(records => { for (const r of records) r.type === 'attributes' ? toggles++ : removed += r.removedNodes.length })
  observer.observe(trigger(), { attributes: true, attributeFilter: ['disabled'] })
  observer.observe(menu()!.querySelector('[role="group"]')!, { childList: true })
  const lapses = h.lapses()
  for (let elapsed = 0; elapsed < 2600; elapsed += 10) {
    await step(10)
    expect(trigger().disabled).toBe(false)
    expect(items()).toBe(2)
  }
  // Positive control: control really lapsed while the dropdown stayed put.
  expect(h.lapses()).toBeGreaterThan(lapses)
  await act(async () => { await Promise.resolve() })
  observer.disconnect()
  expect({ toggles, removed }).toEqual({ toggles: 0, removed: 0 })
})

it('stale station readings still refuse a control at once, even while control is held', async () => {
  const Probe = () => <span>{String(useStationCapability('bandSelection'))}</span>
  const h = station(['bandSelection'])
  const page = (readings: boolean) => <StationControlContext.Provider value={false}><StationDataContext.Provider value={readings}>
    <RemoteOperationsContext.Provider value={h.client}><Probe/></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const ui = render(page(true))
  await until(() => ui.container.textContent === 'true')
  ui.rerender(page(false))
  expect(ui.container.textContent).toBe('false')
  // Held control is not current control, and it never outranks stale readings either.
  await longLapse(h)
  expect(ui.container.textContent).toBe('false')
})

it('a station command made during a control lapse sends nothing while stale, then is sent once on current control', async () => {
  const h = station(['decoder'])
  await longLapse(h)
  let settled = false
  const result = h.client.control({ action: 'decoder.clear', receiver: 'cw' }).finally(() => { settled = true })
  for (let elapsed = 0; elapsed < 300; elapsed += 10) { await step(10); expect(h.writes()).toHaveLength(0) }
  // Positive control for the spy: it records the heartbeat whose reply is still on its way.
  expect(h.heartbeats()).toBeGreaterThan(0)
  expect(settled).toBe(false)
  await until(() => h.writes().length === 1)
  expect(h.writes()[0].fresh).toBe(true)
  await until(() => settled)
  expect(await result).toMatchObject({ outcome: 'applied' })
  await step(2000)
  expect(h.writes()).toHaveLength(1)
})

it('a station command whose wait expires is refused as not sent, and nothing is sent', async () => {
  const h = station(['decoder'])
  await until(() => h.client.getSnapshot().fresh)
  h.link.answering = false
  await until(() => !h.client.getSnapshot().fresh)
  let settled = false
  const refused = h.client.control({ action: 'decoder.clear', receiver: 'cw' }).catch(error => error).finally(() => { settled = true })
  await step(1000)
  // It waited: a lapse is not a refusal until the resume window has passed.
  expect(settled).toBe(false)
  await step(700)
  expect(await refused).toMatchObject({ message: 'notController', sent: false, busy: false })
  expect(h.writes()).toHaveLength(0)
  expect(h.heartbeats()).toBeGreaterThan(0)
})

it('a transmit action is never delayed by a lapse: it is still refused at once, unsent', async () => {
  const transmitEpoch = '000000000000002a'
  const h = station(['ftOperate'], { transmitEpoch })
  await longLapse(h)
  let settled = false
  const refused = h.client.control({ action: 'ft.txEnabled', expectedTier: 'FT8', transmitEpoch, on: false } as never)
    .catch(error => error).finally(() => { settled = true })
  await step(0)
  expect(settled).toBe(true)
  expect(await refused).toMatchObject({ message: 'notController', sent: false })
  await step(2000)
  expect(h.writes()).toHaveLength(0)
})

it('a manual log made during a control lapse is sent once on current control, or refused as not sent', async () => {
  {
    const h = station([], { actions: ['log.manual'] })
    await longLapse(h)
    let settled = false
    const logged = h.client.log(record()).finally(() => { settled = true })
    for (let elapsed = 0; elapsed < 300; elapsed += 10) { await step(10); expect(h.logs()).toHaveLength(0) }
    expect(settled).toBe(false)
    await until(() => h.logs().length === 1)
    expect(h.logs()[0].fresh).toBe(true)
    await until(() => settled)
    expect((await logged).outcome).toBe('applied')
    h.client.disconnected()
  }
  {
    const h = station([], { actions: ['log.manual'] })
    await until(() => h.client.getSnapshot().fresh)
    h.link.answering = false
    await until(() => !h.client.getSnapshot().fresh)
    let settled = false
    const refused = h.client.log(record()).catch(error => error).finally(() => { settled = true })
    await step(1000)
    expect(settled).toBe(false)
    await step(700)
    expect(await refused).toMatchObject({ message: 'notController', sent: false })
    expect(h.logs()).toHaveLength(0)
  }
})

it('repeated stream reads keep the identity of every unchanged part of a station sample', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  const sent: Record<string, unknown>[] = [], client = new ApplicationClient(m => sent.push(JSON.parse(m)), vi.fn(), 2)
  client.open(); client.receive({ type: 'applicationCapabilities', version: 2, commands: STREAM_TOPICS })
  const data = { mycall: 'TEST', radio: { dialMhz: 7.074, band: '40m' }, stations: [{ call: 'W1AW', snr: -10 }] }
  const first = client.invoke<typeof data>('get_snapshot')
  await vi.advanceTimersByTimeAsync(1)
  const requestId = sent[1].requestId as string
  client.receive({ type: 'applicationFrame', requestId, updates: [{ type: 'applicationResult', requestId, command: 'get_snapshot', revision: 1, baseRevision: null, ageMs: 0, data, removed: [] }] })
  const a = await first, b = await client.invoke<typeof data>('get_snapshot')
  expect(b).toEqual(data)
  // An unchanged sample is the same object: React bails out instead of re-rendering the workspace.
  expect(b).toBe(a)
  client.disconnected()
})
