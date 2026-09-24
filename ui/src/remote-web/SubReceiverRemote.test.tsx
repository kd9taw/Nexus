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
import { pushToast } from '../toast'

const clients: OperationClient[] = []
const uninstall: (() => void)[] = []
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(async () => {
  cleanup()
  uninstall.splice(0).forEach(u => u())
  clients.splice(0).forEach(c => c.disconnected())
  vi.useRealTimers()
  // A command still out when the station goes away fails HERE, and its toast must be counted in
  // this test — never leak into the next test's mocks.
  await new Promise(resolve => setTimeout(resolve, 0))
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
  const tree = (s: AppSnapshot = snap, available = true) => <StationControlContext.Provider value={false}><StationDataContext.Provider value={available}>
    <RemoteOperationsContext.Provider value={client}>
      <RemoteObservationContext.Provider value={observation}>
        <Component snap={s} theme="dark" spots={[]} onWorkSpot={() => {}} />
      </RemoteObservationContext.Provider>
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const ui = render(tree())
  const writes = () => sent.filter(w => w.request.type === 'stationControl')
  // The same page again, with another snapshot (`s`) or with the station's readings gone stale.
  const again = (s: AppSnapshot = snap, available = true) => ui.rerender(tree(s, available))
  return { ...ui, writes, snap, again }
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

  // ⭐ A DRAG IS ONE INTENT. The station confirms one command at a time; a command per movement
  // would be refused while the first confirms — an error per movement, and the released value lost.
  it('⭐ a drag sends ONE radio.subLevel intent — the released value, on release, none while it moves', async () => {
    const p = page(mode, DUAL, ['subReceiverLevels'])
    await settle()
    const af = () => screen.getByLabelText('Sub receiver AF gain') as HTMLInputElement
    fireEvent.pointerDown(af(), { pointerId: 1 })
    fireEvent.change(af(), { target: { value: '30' } })
    await settle()
    fireEvent.change(af(), { target: { value: '45' } })
    await settle()
    expect(p.writes(), 'a movement was sent before the release').toHaveLength(0)
    expect(Number(af().value), 'the thumb follows the hand').toBe(45)
    fireEvent.pointerUp(af(), { pointerId: 1 })
    await settle()
    expect(p.writes()).toHaveLength(1)
    expect(p.writes()[0].request.action).toEqual({ action: 'radio.subLevel', level: 'afGain', value: 0.45 })
    expect(pushToast, 'a drag raised an error').not.toHaveBeenCalled()
  })

  it('a held adjustment key sends once, on release — and key repeat cannot restart a canceled edit', async () => {
    const p = page(mode, DUAL, ['subReceiverLevels'])
    await settle()
    const af = () => screen.getByLabelText('Sub receiver AF gain') as HTMLInputElement
    fireEvent.keyDown(af(), { key: 'ArrowRight' })
    fireEvent.change(af(), { target: { value: '26' } })
    await settle()
    // Permission lapses and returns while the key is held: the repeat is the SAME gesture.
    p.again(p.snap, false)
    await settle()
    p.again()
    await settle()
    fireEvent.keyDown(af(), { key: 'ArrowRight', repeat: true })
    fireEvent.change(af(), { target: { value: '27' } })
    fireEvent.keyUp(af(), { key: 'ArrowRight' })
    await settle()
    expect(p.writes(), 'a canceled key edit revived through key repeat').toHaveLength(0)
    fireEvent.keyDown(af(), { key: 'ArrowRight' })
    fireEvent.change(af(), { target: { value: '26' } })
    fireEvent.keyDown(af(), { key: 'ArrowRight', repeat: true })
    fireEvent.change(af(), { target: { value: '27' } })
    await settle()
    expect(p.writes()).toHaveLength(0)
    fireEvent.keyUp(af(), { key: 'ArrowRight' })
    await settle()
    expect(p.writes()).toHaveLength(1)
    expect(p.writes()[0].request.action.value).toBe(0.27)
  })

  it.each(['pointer cancel', 'blur', 'permission loss', 'radio change'] as const)('a drag canceled by %s cannot revive when the context returns', async reason => {
    const p = page(mode, DUAL, ['subReceiverLevels'])
    await settle()
    const af = () => screen.getByLabelText('Sub receiver AF gain') as HTMLInputElement
    fireEvent.pointerDown(af(), { pointerId: 1 })
    fireEvent.change(af(), { target: { value: '35' } })
    await settle()
    if (reason === 'pointer cancel') fireEvent.pointerCancel(af(), { pointerId: 1 })
    else if (reason === 'blur') fireEvent.blur(af())
    else {
      p.again(reason === 'radio change' ? { ...p.snap, activeRadioId: 2 } : p.snap, reason !== 'permission loss')
      await settle()
      p.again()
      await settle()
    }
    fireEvent.change(af(), { target: { value: '42' } })
    fireEvent.pointerUp(af(), { pointerId: 1 })
    await settle()
    expect(p.writes(), 'a canceled drag was sent').toHaveLength(0)
    expect(Number(af().value), 'the thumb stays on a value nobody sent').toBe(25)
    // A fresh drag is not blocked by the canceled one.
    fireEvent.pointerDown(af(), { pointerId: 2 })
    fireEvent.change(af(), { target: { value: '42' } })
    fireEvent.pointerUp(af(), { pointerId: 2 })
    await settle()
    expect(p.writes()).toHaveLength(1)
    expect(p.writes()[0].request.action.value).toBe(0.42)
  })

  it('a pointer cancel ends the gesture with no release, and the next drag still sends', async () => {
    const p = page(mode, DUAL, ['subReceiverLevels'])
    await settle()
    const af = () => screen.getByLabelText('Sub receiver AF gain') as HTMLInputElement
    fireEvent.pointerDown(af(), { pointerId: 1 })
    fireEvent.change(af(), { target: { value: '35' } })
    fireEvent.pointerCancel(af(), { pointerId: 1 })
    await settle()
    expect(Number(af().value), 'a canceled drag leaves the thumb where it began').toBe(25)
    fireEvent.pointerDown(af(), { pointerId: 2 })
    fireEvent.change(af(), { target: { value: '40' } })
    fireEvent.pointerUp(af(), { pointerId: 2 })
    await settle()
    expect(p.writes()).toHaveLength(1)
    expect(p.writes()[0].request.action.value).toBe(0.4)
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
