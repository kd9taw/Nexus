// @vitest-environment jsdom
// STEADY CONTROLS (operator decision 2026-09-14, the remainder of the steady cached view).
// b23f0cf1 stopped a CONTROL lapse — a heartbeat reply landing after the 1.2 s freshness window —
// disabling the band control and its neighbours. It did not cover the controls whose enabled state
// hangs off the station's OWN readings instead: the amplifier buttons, the decode-depth chips, the
// RX offset field and the RX gain slider each required the station's PTT reading to be under a
// second old, and an observation poll landing late crosses that most seconds on a real WAN. Those
// greyed out under the operator exactly as the band control used to.
//
// One policy: `useStationHeld` is the single lapse-tolerant answer ("this browser holds station
// control"), and both `useStationCapability` and the observed-rig gate (`useRemoteStation().ready`)
// follow it. Both directions are tested here: a control keeps its enabled state through a lapse
// while control is held, AND still refuses when control is lost, when the station does not
// advertise the capability, when the rig is keyed, and when the reading is gone altogether.
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { AmpStrip } from '../components/AmpStrip'
import { RemoteOperationsContext, StationControlContext, StationDataContext, useStationCapability } from '../stationAccess'
import { RemoteObservationContext } from './amplifier-observation'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { useRadioLevels } from './useRadioLevels'
import { useReceiverSettings } from './useReceiverSettings'
import { useReceiverGain } from './useReceiverGain'
import { ageFrame, MEASUREMENT_STALE_MS } from '../remote-monitor/protocol'
import type { MonitorFrame } from '../remote-monitor/protocol'
import type { MonitorState } from '../remote-monitor/session'
import type { SettingsConfiguration } from './configuration'
import type { OperationState } from './operation-protocol'
import type { ControlCapability } from './station-operation'
import type { AppSnapshot } from '../types'

vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const CAPABILITIES: ControlCapability[] = ['radioLevels', 'receiverSettings', 'decoderSettings',
  'receiverGain', 'amplifier', 'ftSettings', 'ftRuntime', 'fmReceiver']

const clients: OperationClient[] = []
beforeAll(() => { globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver })
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks() })

async function step(ms = 10) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }
async function until(condition: () => boolean, limitMs = 5000) {
  for (let elapsed = 0; elapsed <= limitMs; elapsed += 10) { if (condition()) return; await step(10) }
  throw Error('condition not reached')
}

/** A station answering after a round trip, as in steady-view.test.tsx. `state` is live: a test may
 * move the lease to another device and the next heartbeat reply carries it. */
function station(capabilities: ControlCapability[] = CAPABILITIES) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: 3, ampReadSequence: 1 }, capabilities } }
  const link = { roundTripMs: 260, answering: true }
  const values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const sent: { type: string; requestId: string }[] = []
  const client: OperationClient = new OperationClient(raw => {
    const { request } = JSON.parse(raw)
    sent.push(request)
    if (!link.answering) return
    const value = request.type === 'state' || request.type === 'heartbeat' ? structuredClone(state) : null
    if (value) setTimeout(() => client.receive({ type: 'operationResponse', requestId: request.requestId, value }), link.roundTripMs)
  }, true, () => 1000 + performance.now(), undefined, 4, pendingControlStorage(() => storage, 'steady-controls', async (_key, run) => run()))
  clients.push(client)
  let lapses = 0, wasFresh = false
  client.subscribe(() => {
    const view = client.getSnapshot()
    if (wasFresh && !view.fresh && view.state?.phase === 'controlling') lapses++
    wasFresh = view.fresh
  })
  client.open()
  return { client, link, state, sent, lapses: () => lapses }
}

const baseFrame = (over: Partial<MonitorFrame['station']['radio']> = {}): MonitorFrame => ({
  version: 2, source: 'fixture', epoch: 'e1', sequence: 1, generatedAtMs: 0,
  station: { call: 'KD9TAW', grid: 'EN52', radio: { id: 1, name: 'rig', dialMhz: 14.074, band: '20m', mode: 'digital',
    rigMode: 'USB', catConnected: true, rigKeyed: false, nexusBusy: false, rigDialMhz: 14.074,
    readings: { cat: { connectionGeneration: 7, readSequence: 1, ageMs: 0 }, dial: { connectionGeneration: 7, readSequence: 1, ageMs: 0 },
      mode: { connectionGeneration: 7, readSequence: 1, ageMs: 0 }, ptt: { connectionGeneration: 7, readSequence: 1, ageMs: 0 } }, ...over },
  amplifier: { reading: { connectionGeneration: 3, readSequence: 1, ageMs: 0 }, family: 'spe', model: '15K', followBand: false,
    linked: true, reason: '', operate: false, transmitting: false, outputWatts: 0, bandLabel: '20m', swr: null, swrAtu: null,
    volts: null, amps: null, temp: null, tempCelsius: true, alarm: '', alarmRaised: false, warning: '', warningRaised: false, kpaFault: null } },
})
const observed = (ageMs = 0, over: Partial<MonitorFrame['station']['radio']> = {}): MonitorState =>
  ({ status: 'current', frame: ageFrame(baseFrame(over), ageMs) })

const snapshot = (): AppSnapshot => ({ activeRadioId: 1, link: { tier: 'FT8' },
  radio: { source: 'native', operatingMode: 'digital', dialMhz: 14.074, band: '20m', sideband: 'USB', rigMode: 'USB', catOk: true,
    txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, txAllowed: true, txBusyReason: null,
    rfPower: 0.4, decodeDepth: 2, rxOffsetHz: 1500, txOffsetHz: 1500, filterWidthHz: 2700, agc: 'fast', sidebandOverride: null } } as unknown as AppSnapshot)

const configuration = (): SettingsConfiguration =>
  ({ revision: 'r1', settings: { activeRadio: 1, rxGain: 4, ampModel: 'spe', ampPort: '/dev/ttyUSB0', ampFollowBand: false } } as unknown as SettingsConfiguration)

/** Every remaining station control's enabled state, as one JSON line per render. The amplifier
 * buttons are asserted on the real DOM below; these are the gates behind the other controls. */
function Probe({ snap }: { snap: AppSnapshot }) {
  const levels = useRadioLevels(snap)
  const receiver = useReceiverSettings(snap, 'FT8')
  const gain = useReceiverGain(configuration(), snap.activeRadioId, snap.radio, () => {})
  const flags = {
    rfPowerSlider: levels.can('power'),
    decodeDepthChips: receiver.depthAllowed,
    rxOffsetField: receiver.rxAllowed,
    txOffsetField: useStationCapability('ftSettings'),
    rxGainSlider: gain.canEdit,
  }
  return <span data-flags={JSON.stringify(flags)}/>
}

function page(h: ReturnType<typeof station>, o: MonitorState, snap: AppSnapshot, children: React.ReactNode) {
  return <StationControlContext.Provider value={false}><StationDataContext.Provider value>
    <RemoteOperationsContext.Provider value={h.client}><RemoteObservationContext.Provider value={o}>
      <Probe snap={snap}/>{children}
    </RemoteObservationContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
}

const ampStrip = (snap: AppSnapshot) =>
  <AmpStrip amp={{ family: 'spe', model: '15K', linked: true, reason: '', operate: false, transmitting: false,
    outputWatts: 0, bandLabel: '20m', alarm: '', alarmRaised: false, warning: '', warningRaised: false } as never}
  radioId={snap.activeRadioId} radioTransmitting={false}/>

it('no station control toggles disabled while control is held and the observation poll runs late', async () => {
  const h = station()
  const snap = snapshot()
  const ui = render(page(h, observed(0), snap, ampStrip(snap)))
  const flags = () => JSON.parse(ui.container.querySelector('span')!.dataset.flags!) as Record<string, boolean>
  const buttons = () => [...ui.container.querySelectorAll('.amp-strip button')] as HTMLButtonElement[]
  await until(() => h.client.getSnapshot().fresh && flags().rfPowerSlider && !buttons()[0]?.disabled)
  expect(Object.values(flags()).every(Boolean)).toBe(true)
  expect(buttons()).toHaveLength(3)
  expect(buttons().map(b => b.disabled)).toEqual([false, false, false])

  // A real WAN: the heartbeat reply lands after the 1.2 s window (control lapses), and the
  // observation poll lands after the 1 s reading window. Neither may move a control.
  const toggles: Record<string, number> = {}
  let previous = flags(), ampBefore = buttons().map(b => b.disabled)
  let ampToggles = 0
  h.link.roundTripMs = 900
  const started = performance.now()
  for (let elapsed = 0; elapsed < 6000; elapsed += 50) {
    ui.rerender(page(h, observed((performance.now() - started) % 1600), snap, ampStrip(snap)))
    await step(50)
    const now = flags()
    for (const key of Object.keys(now)) if (now[key] !== previous[key]) toggles[key] = (toggles[key] ?? 0) + 1
    previous = now
    const amp = buttons().map(b => b.disabled)
    if (amp.join() !== ampBefore.join()) ampToggles++
    ampBefore = amp
  }
  // Positive controls: control really lapsed, and the reading really aged past its window.
  expect(h.lapses()).toBeGreaterThan(0)
  expect(observed(1400).frame!.station.radio.readings.ptt!.ageMs).toBeGreaterThan(1000)
  expect({ toggles, ampToggles }).toEqual({ toggles: {}, ampToggles: 0 })
  expect(Object.values(flags()).every(Boolean)).toBe(true)
})

// The other way a held control went grey, in TWO halves and this test now covers both: while the
// command is in flight the client holds a pending receipt, and after it lands the client used to
// drop its state until the station's re-read answered — so all of these went dead from the click
// until the re-read, about a second per click (operator ruling 2026-09-16: a control stays lit
// while it confirms). The state is kept through the second half, and the receipt no longer darkens
// anything in the first. control.test.ts proves that neither window can pass a command: the spent
// one refuses and waits for the next, and one made while the receipt is out is refused unsent.
it('no station control toggles disabled across a confirmed command and the station re-read', async () => {
  const h = station()
  const snap = snapshot()
  const ui = render(page(h, observed(0), snap, ampStrip(snap)))
  const flags = () => JSON.parse(ui.container.querySelector('span')!.dataset.flags!) as Record<string, boolean>
  const buttons = () => [...ui.container.querySelectorAll('.amp-strip button')] as HTMLButtonElement[]
  await until(() => h.client.getSnapshot().fresh && flags().rfPowerSlider && !buttons()[0]?.disabled)
  expect(Object.values(flags()).every(Boolean)).toBe(true)
  expect(buttons().map(b => b.disabled)).toEqual([false, false, false])

  const toggles: Record<string, number> = {}
  let previous = flags(), ampBefore = buttons().map(b => b.disabled), ampToggles = 0, refreshing = 0
  const sample = () => {
    const now = flags()
    for (const key of Object.keys(now)) if (now[key] !== previous[key]) toggles[key] = (toggles[key] ?? 0) + 1
    previous = now
    const amp = buttons().map(b => b.disabled)
    if (amp.join() !== ampBefore.join()) ampToggles++
    ampBefore = amp
    if (h.client.getSnapshot().controlRefreshing) refreshing++
  }
  let command!: Promise<unknown>
  await act(async () => { command = h.client.control({ action: 'amplifier.operate', expectedOperate: false, operate: true }); await Promise.resolve() })
  const request = h.sent[h.sent.length - 1]
  expect(request.type).toBe('stationControl')
  // THE WINDOW THIS TEST USED TO SKIP. Sampling began after the outcome, so it only ever covered
  // the second half — state kept, receipt cleared. The half the operator actually sees is this
  // one: the receipt is written the moment the command leaves and holds until the station answers,
  // a whole round trip, and it is the term the Responsiveness twin attributes 100% of the
  // remaining controls-off events to. Measured on the twin before this was fixed: 8 events of
  // 550 ms on a 100 ms polled link, 12 of 250-300 ms pushed, 8 of 550 ms on a 400 ms link.
  let pending = 0
  for (let elapsed = 0; elapsed < 260; elapsed += 20) { sample(); if (h.client.getSnapshot().controlPending) pending++; await step(20) }
  // The station applied it and spent the window; its re-read answers after the link's round trip.
  Object.assign(h.state, { revision: 2, commandWindowId: crypto.randomUUID(), nextSequence: 2 })
  await act(async () => {
    h.client.receive({ type: 'operationResponse', requestId: request.requestId, value: { operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'stationState' } })
    await command
  })
  for (let elapsed = 0; elapsed < 2000; elapsed += 50) { sample(); await step(50) }
  // Positive controls: the command really was in flight while it was sampled, the confirming gap
  // really happened, and the re-read really landed.
  expect(pending).toBeGreaterThan(0)
  expect(refreshing).toBeGreaterThan(0)
  expect(h.client.getSnapshot()).toMatchObject({ fresh: true, controlRefreshing: false, state: { revision: 2 } })
  expect({ toggles, ampToggles }).toEqual({ toggles: {}, ampToggles: 0 })
  expect(Object.values(flags()).every(Boolean)).toBe(true)
})

it('every one of them still refuses when this browser no longer holds station control', async () => {
  const h = station()
  const snap = snapshot()
  const ui = render(page(h, observed(0), snap, ampStrip(snap)))
  const flags = () => JSON.parse(ui.container.querySelector('span')!.dataset.flags!) as Record<string, boolean>
  const buttons = () => [...ui.container.querySelectorAll('.amp-strip button')] as HTMLButtonElement[]
  await until(() => flags().rfPowerSlider && !buttons()[0].disabled)
  // Another device took the lease: held control is gone, and no lapse tolerance survives it.
  Object.assign(h.state, { phase: 'occupied', leaseId: null, commandWindowId: null, nextSequence: null, leaseRemainingMs: null })
  await until(() => !flags().rfPowerSlider)
  expect(Object.values(flags()).some(Boolean)).toBe(false)
  expect(buttons().map(b => b.disabled)).toEqual([true, true, true])
})

it('a capability the station does not advertise stays refused, lapse or no lapse', async () => {
  const h = station(CAPABILITIES.filter(c => c !== 'amplifier' && c !== 'receiverSettings'))
  const snap = snapshot()
  const ui = render(page(h, observed(0), snap, ampStrip(snap)))
  const flags = () => JSON.parse(ui.container.querySelector('span')!.dataset.flags!) as Record<string, boolean>
  const buttons = () => [...ui.container.querySelectorAll('.amp-strip button')] as HTMLButtonElement[]
  await until(() => flags().rfPowerSlider)
  // The rig's own levels are advertised and stay usable; the amplifier and the receive settings
  // are not advertised and stay refused — through the lapse as well.
  h.link.roundTripMs = 900
  await until(() => !h.client.getSnapshot().fresh)
  expect(flags().rfPowerSlider).toBe(true)
  expect(flags().decodeDepthChips).toBe(false)
  expect(flags().rxOffsetField).toBe(false)
  expect(buttons().map(b => b.disabled)).toEqual([true, true, true])
})

it('the rig keyed, or its reading gone altogether, still refuses while control is held', async () => {
  const h = station()
  const snap = snapshot()
  const ui = render(page(h, observed(0), snap, ampStrip(snap)))
  const flags = () => JSON.parse(ui.container.querySelector('span')!.dataset.flags!) as Record<string, boolean>
  const buttons = () => [...ui.container.querySelectorAll('.amp-strip button')] as HTMLButtonElement[]
  await until(() => !buttons()[0].disabled && flags().decodeDepthChips)
  // The station says the rig is keyed. Held control never overrides that.
  ui.rerender(page(h, observed(0, { rigKeyed: true }), snap, ampStrip(snap)))
  expect(flags().decodeDepthChips).toBe(false)
  expect(buttons().map(b => b.disabled)).toEqual([true, true, true])
  // Recovered, so the refusal above is about the keying and not about a wedged probe.
  ui.rerender(page(h, observed(0), snap, ampStrip(snap)))
  expect(buttons().map(b => b.disabled)).toEqual([false, false, false])
  // The 5 s measurement bound is unchanged: past it the frame drops the reading, and `rigKeyed`
  // with it, so there is no observation left to act on however firmly control is held.
  const gone = observed(MEASUREMENT_STALE_MS)
  expect(gone.frame!.station.radio.readings.ptt).toBe(null)
  ui.rerender(page(h, gone, snap, ampStrip(snap)))
  expect(flags().decodeDepthChips).toBe(false)
  expect(flags().rxGainSlider).toBe(false)
  expect(buttons().map(b => b.disabled)).toEqual([true, true, true])
})
