// @vitest-environment jsdom
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { CwCockpit } from '../components/CwCockpit'
import { OperateCockpit } from '../components/OperateCockpit'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { RemoteObservationContext } from './amplifier-observation'
import { OperationClient, OperationFailure } from './operation-client'
import { pendingControlStorage } from './control-storage'
import type { AppSnapshot } from '../types'
import type { MonitorFrame } from '../remote-monitor/protocol'
import type { MonitorState } from '../remote-monitor/session'
import type { ControlCapability } from './station-operation'
import settings from '../components/__fixtures__/defaultSettings.json'
import frames from '../remote-monitor/fixtures.v2.json'
import { t } from '../i18n'

// Remote parity batch 1: the CW cockpit's AI-CW switch and Operate's Decode button (and F6)
// are live for a browser holding their own hint, and stay off without it.

vi.mock('../api', async original => {
  const actual = await original<Record<string, unknown>>()
  const reads: Record<string, unknown> = { getLicensedBandPlan: [], getBandPlan: [], getLog: [] }
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [name,
    typeof value === 'function' ? vi.fn(async () => structuredClone(reads[name] ?? {})) : value]))
})
vi.mock('../components/PhoneScope', () => ({ PhoneScope: () => <div/> }))
vi.mock('../components/BandStrip', () => ({ BandStrip: () => <div/> }))
vi.mock('../components/LogEntry', () => ({ LogEntry: () => <div/> }))
vi.mock('../components/SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('../components/Waterfall', () => ({ Waterfall: () => <div/> }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (run: () => Promise<unknown>) => run()) }))
import { cwDecode, getCatCwUnprovenRigModels, getSettings, redecode, setAiCw } from '../api'
import { pushToast } from '../toast'

const clients: OperationClient[] = []
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks() })
async function tick(ms = 0) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }
const noop = () => {}

function station(capabilities: ControlCapability[]) {
  const sent: any[] = [], values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000 + performance.now(), undefined, 3,
    pendingControlStorage(() => storage, 'decoder-gestures', async (_key, run) => run()))
  clients.push(client)
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value: {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities } } })
  const frame = structuredClone(frames.spe) as MonitorFrame
  frame.station.radio.id = 1; frame.station.radio.readings.cat!.connectionGeneration = 7; frame.station.amplifier = null
  return { client, observation: { status: 'current', frame } as MonitorState }
}

function remote(capabilities: ControlCapability[], local: boolean, child: JSX.Element) {
  const { client, observation } = station(capabilities)
  return <StationControlContext.Provider value={local}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={local ? null : client}>
      <RemoteObservationContext.Provider value={observation}>{child}</RemoteObservationContext.Provider>
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
}

function cw(capabilities: ControlCapability[], local = false) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  vi.mocked(getSettings).mockResolvedValue(structuredClone(settings) as never)
  vi.mocked(getCatCwUnprovenRigModels).mockResolvedValue([])
  vi.mocked(cwDecode).mockResolvedValue({ text: '', wpm: 0, sent: [], candidates: [], keyerError: null,
    state: 'listening', headline: '', prompt: '', recommended: null, workedCall: null, rst: null, name: null } as never)
  const snap = { activeRadioId: 1, mycall: 'N0CALL', mygrid: 'AA00', mode: 'qso', stations: [], recentDecodes: [], conversations: [], highlights: [],
    link: { tier: 'FT8', dtSec: 0 }, aiCw: { enabled: false, status: '', text: '' },
    radio: { source: 'native', operatingMode: 'cw', dialMhz: 14.035, band: '20m', sideband: 'USB', rigMode: 'CW', catOk: true,
      txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, txAllowed: true, filterWidthHz: 500, cwWpm: 22, cwKeyer: 'cat',
      nrLevel: 0.3, agc: 'fast', refusedAgc: null, nb: false, nr: false, notch: null, splitTxMhz: null, smeterDb: null } } as unknown as AppSnapshot
  const onSnap = vi.fn()
  const ui = render(remote(capabilities, local, <CwCockpit snap={snap} theme="dark" spots={[]} onWorkSpot={noop} onSnap={onSnap}/>))
  const aiSwitch = () => [...ui.container.querySelectorAll<HTMLButtonElement>('button[role="switch"]')]
    .find(b => b.title === t('cw.decode.ai.off.title'))!
  return { ui, onSnap, aiSwitch }
}

function operate(capabilities: ControlCapability[], tier: 'FT8' | 'MSK144' = 'FT8') {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  const snap = { activeRadioId: 1, mode: 'qso', mycall: 'N0CALL', mygrid: 'AA00', stations: [], recentDecodes: [], conversations: [], highlights: [],
    harqRescues: 0, clearTick: 0, qso: null, link: { tier, periodSecs: 15 }, radio: { source: 'native', sourceLabel: 'Native',
      operatingMode: 'digital', dialMhz: 14.074, band: '20m', sideband: 'USB', slot: 0, catOk: true, txEnabled: false,
      transmitting: false, rigKeyed: false, tuning: false, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, txEven: true, txCycleAuto: true } } as unknown as AppSnapshot
  const panels = { layout: { v: 1 as const, state: {}, share: {} }, stateOf: () => 'removed' as const, setPanelState: noop,
    shareOf: () => 1, setShare: noop, setShares: noop, undo: noop, canUndo: false, undoRemoves: [], reset: noop }
  const onSnap = vi.fn()
  const ui = render(remote(capabilities, false,
    <OperateCockpit snap={snap} tier={tier} onSnap={onSnap} theme="dark" bandPlan={[]} onTierChange={noop} onSetFrequency={noop}
      onSourceChange={noop} onTune={noop} onCall={noop} onSetTxLevel={noop} onSetMode={noop} onSetTxEven={noop} onSetTxCycleAuto={noop}
      onResend={noop} onFreetext={noop} onLog={noop} onOverrideTx={noop} onHaltTx={noop} roster={<div/>} needByCall={new Map()}
      selectedCall={null} onSelect={noop} layoutMode="classic" onLayoutMode={noop} panels={panels} active={true}/>))
  const decode = () => ui.container.querySelector<HTMLButtonElement>('.cockpit-decode-btn')!
  return { ui, onSnap, decode }
}

it('the AI-CW switch is live for a browser with its hint and sends the flipped choice', async () => {
  const h = cw(['aiCw', 'decoder']); await tick()
  expect(h.aiSwitch().disabled).toBe(false)
  const later = { aiCw: { enabled: true } } as unknown as AppSnapshot
  vi.mocked(setAiCw).mockResolvedValueOnce(later)
  fireEvent.click(h.aiSwitch()); await tick()
  expect(setAiCw).toHaveBeenCalledWith(true)
  expect(h.onSnap).toHaveBeenCalledWith(later)
})

it('the AI-CW switch says why a refused remote choice did not land', async () => {
  const h = cw(['aiCw']); await tick()
  vi.mocked(setAiCw).mockRejectedValueOnce(new OperationFailure('stationBusy', true, true))
  fireEvent.click(h.aiSwitch()); await tick()
  expect(pushToast).toHaveBeenCalledWith(t('remote.controlBusy'), 'error')
})

it('the AI-CW switch stays off without its hint; the local desktop keeps it', async () => {
  const h = cw(['decoder', 'receiverFilter']); await tick()
  expect(h.aiSwitch().disabled).toBe(true)
  fireEvent.click(h.aiSwitch()); await tick()
  expect(setAiCw).not.toHaveBeenCalled()
  cleanup()
  const local = cw([], true); await tick()
  expect(local.aiSwitch().disabled).toBe(false)
})

it('Decode and F6 redecode FT8 for a browser with the hint', async () => {
  const h = operate(['redecode']); await tick()
  expect(h.decode().disabled).toBe(false)
  fireEvent.click(h.decode()); await tick()
  expect(redecode).toHaveBeenCalledTimes(1)
  fireEvent.keyDown(window, { key: 'F6' }); await tick()
  expect(redecode).toHaveBeenCalledTimes(2)
  vi.mocked(redecode).mockRejectedValueOnce(new OperationFailure('notController', false))
  fireEvent.click(h.decode()); await tick()
  expect(pushToast).toHaveBeenCalledWith(t('remote.controlNotSent'), 'error')
})

it.each([[[] as ControlCapability[], 'FT8'], [['redecode'] as ControlCapability[], 'MSK144']] as const)('Decode and F6 stay off without the hint or off FT8/FT4 (%j, %s)', async (capabilities, tier) => {
  const h = operate([...capabilities], tier); await tick()
  expect(h.decode().disabled).toBe(true)
  fireEvent.click(h.decode())
  fireEvent.keyDown(window, { key: 'F6' }); await tick()
  expect(redecode).not.toHaveBeenCalled()
})
