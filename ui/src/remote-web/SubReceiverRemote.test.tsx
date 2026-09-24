// @vitest-environment jsdom
//
// THE SUB ROW ON THE REMOTE PAGE (dual-receiver programme, the Remote stage).
//
// The hosted page reuses the Phone and CW cockpits, so it draws the same SUB row the desktop does
// — for a station whose snapshot carries a Sub Nexus can command — and its sliders command the Sub
// through the station's own operation contract: the desktop call `set_sub_level` becomes one typed
// `radio.subLevel` intent (control-transport), and the station answers it with the desktop's own
// engine verb and every refusal that verb makes.
//
// ⛔ AND IT DEGRADES ON AN OLD STATION. A relay deploy puts this page ahead of every station in the
// field, and a station older than the receivers field sends no `receivers` at all: the page then
// draws no SUB row and no MAIN plate — the same document a single-receiver station draws. A station
// that sends receivers but does not advertise `subReceiverLevels` gets the row with its sliders dead.
import { afterEach, beforeAll, expect, it, describe, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { CwCockpit } from '../components/CwCockpit'
import { PhoneCockpit } from '../components/PhoneCockpit'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { controlTransport } from './control-transport'
import { installApplicationTransport, type ApplicationTransport } from '../applicationTransport'
import type { ApplicationClient } from './application-client'
import type { AppSnapshot, ReceiversStatus } from '../types'
import type { OperationState } from './operation-protocol'
import type { ControlCapability } from './station-operation'
import settings from '../components/__fixtures__/defaultSettings.json'
import { RemoteObservationContext } from './amplifier-observation'
import frames from '../remote-monitor/fixtures.v2.json'
import type { MonitorState } from '../remote-monitor/session'
import type { MonitorFrame } from '../remote-monitor/protocol'

vi.mock('../api', async original => {
  // Every read auto-stubbed — EXCEPT `setSubLevel`, which stays REAL: it is the one call under
  // test, and it must reach the installed Remote transport exactly as it does in the page.
  const actual = await original<Record<string, unknown>>()
  const reads: Record<string, unknown> = { getLicensedBandPlan: [], getBandPlan: [], getCatCwUnprovenRigModels: [],
    getMeters: { rxLevel: 0, smeterDb: null, cwToneHz: null } }
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [name,
    name === 'setSubLevel' || typeof value !== 'function' ? value : vi.fn(async () => structuredClone(reads[name] ?? {}))]))
})
vi.mock('../components/PhoneScope', () => ({ PhoneScope: () => <div/> }))
vi.mock('../components/BandStrip', () => ({ BandStrip: () => <div/> }))
vi.mock('../components/VoiceKeyer', () => ({ VoiceKeyer: () => <div/> }))
vi.mock('../components/LogEntry', () => ({ LogEntry: () => <div/> }))
vi.mock('../components/SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (run: () => Promise<unknown>) => run()) }))
import { getSettings, cwDecode } from '../api'

const clients: OperationClient[] = []
const uninstall: (() => void)[] = []
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  uninstall.splice(0).forEach(u => u())
  clients.splice(0).forEach(c => c.disconnected())
  vi.useRealTimers()
  vi.clearAllMocks()
})

/** An IC-7610 on Nexus's own CI-V control: a commandable Sub with its own AF. */
const DUAL: ReceiversStatus = {
  main: { id: 'main', stages: { frontEnd: 'own', dsp: 'own', audio: 'own' } },
  sub: { id: 'sub', stages: { frontEnd: 'own', dsp: 'unknown', audio: 'own' }, afGain: 0.25 },
  subCapability: 'present',
  subCommandable: true,
}

/**
 * The page's cockpit against a station advertising `capabilities` at operation `version`, whose
 * snapshot carries `receivers` exactly as given — `undefined` leaves the key off altogether, the
 * shape a station older than the field sends. NO defaults on the two under test.
 */
function page(mode: 'phone' | 'cw', receivers: ReceiversStatus | undefined, capabilities: ControlCapability[], version: 2 | 3 = 3) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  vi.mocked(getSettings).mockResolvedValue(structuredClone(settings) as never)
  vi.mocked(cwDecode).mockResolvedValue({ text: '', wpm: 22, sent: [], candidates: [], keyerError: null,
    state: 'listening', headline: '', prompt: '', recommended: null, workedCall: null, rst: null, name: null } as never)
  const radio: Record<string, unknown> = { source: 'native', operatingMode: mode,
    dialMhz: 14.275, band: '20m', sideband: 'USB', rigMode: mode === 'cw' ? 'CW' : 'USB', catOk: true,
    txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, txAllowed: true, filterWidthHz: mode === 'cw' ? 500 : 2400,
    rfPower: 0.5, micGain: 0.5, cwWpm: 22, cwKeyer: 'cat', nrLevel: 0.3, agc: 'fast', refusedAgc: null,
    nb: false, nr: false, notch: false, manualNotch: false, splitTxMhz: null, smeterDb: null }
  if (receivers !== undefined) radio.receivers = receivers
  const snap = { activeRadioId: 1, mycall: 'N0CALL', mygrid: 'AA00', mode: 'qso', stations: [], recentDecodes: [],
    conversations: [], highlights: [], link: { tier: 'FT8', dtSec: 0 }, radio } as unknown as AppSnapshot
  const sent: any[] = [], values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000 + performance.now(), undefined, version,
    pendingControlStorage(() => storage, 'sub-receiver', async (_key, run) => run()))
  clients.push(client)
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities } }
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value: state })
  // The page's own transport: the desktop call becomes a typed station intent here.
  const reads = { kind: 'remote', invoke: vi.fn(async () => snap) } as unknown as ApplicationTransport
  uninstall.push(installApplicationTransport(controlTransport(reads, { age: () => 0 } as unknown as ApplicationClient, client)))
  const frame = structuredClone(frames.spe) as MonitorFrame
  frame.station.radio.id = 1; frame.station.radio.readings.cat!.connectionGeneration = 7; frame.station.amplifier = null
  const observation = { status: 'current', frame } as MonitorState
  const Component = mode === 'cw' ? CwCockpit : PhoneCockpit
  const ui = render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={client}>
      <RemoteObservationContext.Provider value={observation}>
        <Component snap={snap} theme="dark" spots={[]} onWorkSpot={() => {}} />
      </RemoteObservationContext.Provider>
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  const writes = () => sent.filter(w => w.request.type === 'stationControl')
  return { ...ui, writes }
}

async function settle() {
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
}

const subRow = () => document.querySelector('[data-receiver="sub"]')
const mainPlate = () => document.querySelector('[data-receiver-plate="main"]')

describe.each(['phone', 'cw'] as const)('the Remote page, %s cockpit', mode => {
  it('⭐ a commandable Sub draws the SUB row, and its slider sends ONE radio.subLevel intent — never a Main level', async () => {
    const p = page(mode, DUAL, ['subReceiverLevels', 'radioLevels'])
    await settle()
    expect(subRow(), 'no SUB row on the page').not.toBeNull()
    expect(mainPlate(), 'no MAIN plate beside it').not.toBeNull()
    const af = screen.getByLabelText('Sub receiver AF gain') as HTMLInputElement
    expect(af.disabled, 'the Sub slider is dead with the station advertising its capability').toBe(false)
    fireEvent.change(af, { target: { value: '40' } })
    await settle()
    const ws = p.writes()
    expect(ws, 'exactly one station intent').toHaveLength(1)
    expect(ws[0].request.action).toEqual({ action: 'radio.subLevel', level: 'afGain', value: 0.4 })
    expect(ws.some(w => w.request.action.action === 'radio.level'), 'a Main level was sent').toBe(false)
  })

  it('a station that does not advertise subReceiverLevels: the row is drawn, its sliders dead, nothing sent', async () => {
    const p = page(mode, DUAL, ['radioLevels', 'receiverDsp'])
    await settle()
    expect(subRow()).not.toBeNull()
    const af = screen.getByLabelText('Sub receiver AF gain') as HTMLInputElement
    expect(af.disabled).toBe(true)
    fireEvent.change(af, { target: { value: '40' } })
    await settle()
    expect(p.writes()).toHaveLength(0)
  })

  it('an operation-v2 station (older lane): the sliders are dead and nothing is sent', async () => {
    const p = page(mode, DUAL, ['subReceiverLevels'], 2)
    await settle()
    const af = screen.getByLabelText('Sub receiver AF gain') as HTMLInputElement
    expect(af.disabled).toBe(true)
    fireEvent.change(af, { target: { value: '40' } })
    await settle()
    expect(p.writes()).toHaveLength(0)
  })

  it('⛔ an OLD station (no receivers at all) draws no SUB row and no MAIN plate — the single-receiver document', async () => {
    const old = page(mode, undefined, ['subReceiverLevels', 'radioLevels'])
    await settle()
    expect(subRow()).toBeNull()
    expect(mainPlate()).toBeNull()
    const oldHtml = old.container.innerHTML.replace(/:r[0-9a-z]+:/g, ':rID:')
    cleanup()
    uninstall.splice(0).forEach(u => u())
    clients.splice(0).forEach(c => c.disconnected())
    vi.useRealTimers()
    // …the same document a new station with ONE receiver draws.
    const single = page(mode, { main: DUAL.main, sub: null, subCapability: 'unknown', subCommandable: null }, ['subReceiverLevels', 'radioLevels'])
    await settle()
    expect(single.container.innerHTML.replace(/:r[0-9a-z]+:/g, ':rID:')).toBe(oldHtml)
  })

  it('a Sub the station cannot command (Hamlib) draws nothing on the page either', async () => {
    page(mode, { ...DUAL, subCommandable: false }, ['subReceiverLevels'])
    await settle()
    expect(subRow()).toBeNull()
    expect(mainPlate()).toBeNull()
  })
})
