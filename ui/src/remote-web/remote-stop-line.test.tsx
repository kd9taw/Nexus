// @vitest-environment jsdom
//
// THE STOP LINE IN THE REMOTE BROWSER — the Remote variant of components/stop-line.test.tsx.
//
// Operator decision 2026-09-14: a remote operator who holds station control can press Stop TX in
// every cockpit, not only Operate. There is ONE remote stop path and every cockpit uses it: the
// header's Stop TX → api haltTx → the hosted control transport's `halt_tx` → the operation
// client's `stopTransmit` request (the same request Operate's FtStopControl sends). Nothing here
// adds a way to START a transmission.
//
// What this file computes, per cockpit that draws Stop TX in its CockpitHeader:
//   · with every panel shown AND with every panel hidden, Stop TX is on screen and ENABLED for a
//     browser that holds control (the local sweep's "no more disabled than it was" is not enough
//     here: remotely it used to be disabled in both states, which that sweep called consistent);
//   · a click sends EXACTLY ONE `stopTransmit` and no ordinary station command. The spy is the
//     operation client's own wire, so what it records really left the browser. Positive control:
//     the same spy has already recorded the client's opening state read before any click;
//   · with stale readings, stale control and a station command still pending (the busy banner),
//     Stop is enabled and its request leaves at once, with no wait for control to be current;
//   · a browser without station control (or with control but no transmit grant) gets the existing
//     refusal: Stop is disabled and a direct halt is refused `localPermissionRequired` with nothing
//     sent — the negative control for the click assertions above.
import { afterEach, beforeAll, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import type { ReactElement } from 'react'
import { PhoneCockpit } from '../components/PhoneCockpit'
import { CwCockpit } from '../components/CwCockpit'
import { RttyCockpit } from '../components/RttyCockpit'
import { PskCockpit } from '../components/PskCockpit'
import { Js8Cockpit } from '../components/Js8Cockpit'
import { SstvView } from '../components/SstvView'
import { ALL_PANEL_VOCABULARIES, PHONE_PANEL_IDS, CW_PANEL_IDS, RTTY_PANEL_IDS, PSK_PANEL_IDS, JS8_PANEL_IDS, SSTV_PANEL_IDS } from '../features/panelState'
import type { PanelLayoutApi } from '../features/panelState'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { installApplicationTransport } from '../applicationTransport'
import { haltTx } from '../api'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { controlTransport } from './control-transport'
import type { ApplicationClient } from './application-client'
import type { OperationState } from './operation-protocol'
import settings from '../components/__fixtures__/defaultSettings.json'
import type { AppSnapshot } from '../types'

vi.mock('../components/PhoneScope', () => ({ PhoneScope: () => <div/> }))
vi.mock('../components/BandStrip', () => ({ BandStrip: () => <div/> }))
vi.mock('../components/LogEntry', () => ({ LogEntry: () => <div/> }))
vi.mock('../components/SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('../components/Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap"/> }))
vi.mock('../components/VoiceKeyer', () => ({ VoiceKeyer: () => <div/> }))
vi.mock('./useJs8Context', () => ({ useJs8Context: () => ({ remote: true, value: null, loading: false, refresh: () => {} }) }))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (run: () => Promise<unknown>) => { try { return await run() } catch { return null } }),
}))

const rtty = { armed: true, afcHz: 0, afcLocked: false, text: '', charConf: [], baud: 45.45, shiftHz: 170, markHz: 2125, spaceHz: 2295,
  sending: false, latched: false, backend: 'afsk', keyerError: null, auto: false, seqState: 'idle', peer: null, peerExchange: [], heardCq: null }
const psk = { armed: true, afcHz: 0, signal: false, centerHz: 1000, text: '', charConf: [], sending: false, latched: false, keyerError: null }
const js8 = { speed: 'normal', rxSpeeds: 15, txEnabled: false, sending: false, hbOn: false, hbNextAtMs: null, hbIntervalMin: 0, cqOn: false,
  cqNextAtMs: null, cqIntervalMin: 0, autoreply: false, relay: false, hbAck: false,
  armed: { autoreply: false, relay: false, hbAck: false, hb: false, cq: false }, idleMinutes: 0, idleLimitMin: 60, idleTripped: false,
  activity: [], stations: [], inbox: [], queue: [], pendingReply: null, lastError: null }
const sstv = { armed: false, mode: null, linesDone: 0, linesTotal: 0, previewRgbBase64: null, previewWidth: 0, previewHeight: 0, hedrShiftHz: 0,
  gallery: [], health: { armed: false, audioPeak: 0, lastAudioUnix: null, drains: 0, visSeen: 0, lastVisUnix: null, unknownVis: 0,
    lastUnknownVisCode: null, lastUnknownVisUnix: null, images: 0, lastImageUnix: null },
  sending: false, txMode: null, txProgress: 0, txElapsedSecs: 0, txTotalSecs: 0 }
const cw = { text: '', wpm: 22, sent: [], keyerError: null, candidates: [], state: 'listening', headline: '', prompt: '', recommended: null,
  workedCall: null, rst: null, name: null }

const radio = { source: 'native', dialMhz: 14.2, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null, rigMode: 'USB',
  operatingMode: 'phone', transmitting: false, tuning: false, rigKeyed: false, txEnabled: false, txAllowed: true, qsoRecording: false,
  rfPower: null, micGain: null, nrLevel: 0.3, agc: 'fast', nb: false, nr: false, notch: null, comp: null, vox: null, filterWidthHz: 500,
  splitTxMhz: null, smeterDb: null, cwWpm: 22, cwKeyer: 'cat', phoneSegLo: null, phoneSegHi: null }
const snap = { activeRadioId: 1, mycall: 'N0CALL', mygrid: 'AA00', mode: 'qso', stations: [], recentDecodes: [], conversations: [],
  highlights: [], link: { tier: 'FT8', dtSec: 0 }, radio } as unknown as AppSnapshot

const clients: OperationClient[] = []
let uninstall: (() => void) | undefined
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
beforeEach(() => { localStorage.clear(); vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null) })
afterEach(() => { cleanup(); uninstall?.(); uninstall = undefined; clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.restoreAllMocks() })

type Authority = 'control' | 'noControl' | 'noTransmitGrant'
/** A browser session wired exactly as BrowserApplication wires it: a real operation client behind the
 * real hosted control transport, installed under the application API the cockpits call. */
function session(authority: Authority, capabilities: string[] = []) {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const wire: any[] = []
  const values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(raw => wire.push(JSON.parse(raw).request), true, () => 1000 + performance.now(), undefined, 4,
    pendingControlStorage(() => storage, 'remote-stop-line', async (_key, run) => run()))
  clients.push(client)
  const controlling = authority !== 'noControl'
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: controlling ? 'controlling' : 'available',
    leaseId: controlling ? crypto.randomUUID() : null, revision: 1, commandWindowId: controlling ? crypto.randomUUID() : null,
    nextSequence: controlling ? 1 : null, leaseRemainingMs: controlling ? 5000 : null, actions: [], txArmed: false,
    ...(authority === 'control' ? { transmitEpoch: '000000000000002a' } : {}),
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities: capabilities as never } }
  client.open()
  client.receive({ type: 'operationResponse', requestId: wire[wire.length - 1].requestId, value: state })
  const reads = { kind: 'remote' as const, invoke: async <T,>(command: string): Promise<T> => {
    const value = command === 'get_snapshot' ? snap : command.includes('settings') ? settings
      : /licensed_band_plan|band_plan|get_log|unproven|voice_messages|memories/.test(command) ? []
      : command.includes('rtty') ? rtty : command.includes('psk') ? psk : command.includes('js8') ? js8
      : command.includes('sstv') ? sstv : command.startsWith('cw') || command.includes('_cw') ? cw : {}
    return structuredClone(value) as T
  } }
  uninstall = installApplicationTransport(controlTransport(reads, { age: () => 0 } as unknown as ApplicationClient, client))
  return { client, state, wire, stops: () => wire.filter(r => r.type === 'stopTransmit'), commands: () => wire.filter(r => r.type === 'stationControl') }
}

function panelsWith(removed: readonly string[]): PanelLayoutApi<string> {
  const noop = () => {}
  return { layout: { v: 1, state: {}, share: {} }, stateOf: id => removed.includes(id) ? 'removed' : 'docked', setPanelState: noop,
    shareOf: () => 1, setShare: noop, setShares: noop, undo: noop, canUndo: false, undoRemoves: [], reset: noop }
}

type Case = { cockpit: string; view: string; ids: readonly string[]; element: (panels: PanelLayoutApi<string>) => ReactElement }
// Each cockpit with the props App gives it (the stop-line sweep's rule: onSetTxEnabled draws the latch).
const CASES: Case[] = [
  { cockpit: 'Phone', view: 'phone', ids: PHONE_PANEL_IDS, element: p => <PhoneCockpit snap={snap} theme="dark" spots={[]} onWorkSpot={() => {}} onSnap={() => {}} panels={p as never}/> },
  { cockpit: 'CW', view: 'cw', ids: CW_PANEL_IDS, element: p => <CwCockpit snap={snap} theme="dark" spots={[]} onWorkSpot={() => {}} onSnap={() => {}} panels={p as never}/> },
  { cockpit: 'RTTY', view: 'rtty', ids: RTTY_PANEL_IDS, element: p => <RttyCockpit snap={snap} panels={p as never} onSetTxEnabled={() => {}}/> },
  { cockpit: 'PSK', view: 'psk', ids: PSK_PANEL_IDS, element: p => <PskCockpit snap={snap} panels={p as never} onSetTxEnabled={() => {}}/> },
  { cockpit: 'JS8', view: 'js8', ids: JS8_PANEL_IDS, element: p => <Js8Cockpit snap={snap} panels={p as never} onSetTxEnabled={() => {}}/> },
  { cockpit: 'SSTV', view: 'sstv', ids: SSTV_PANEL_IDS, element: p => <SstvView snap={snap} panels={p as never} onSetTxEnabled={() => {}}/> },
]

function remote(client: OperationClient, element: ReactElement, current = true) {
  return render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={current}>
    <RemoteOperationsContext.Provider value={client}>{element}</RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
}
async function settle(ms = 0) { await act(async () => { await new Promise(resolve => setTimeout(resolve, ms)) }) }
const stopTx = () => screen.getAllByRole('button', { name: /^stop tx$/i }) as HTMLButtonElement[]

it.each(CASES.map(c => [c.cockpit, c] as const))('%s: a controlling browser has an enabled Stop TX with every panel shown or hidden, and a click sends exactly one stopTransmit', async (_name, c) => {
  for (const removed of [[], [...c.ids]]) {
    const h = session('control')
    // Positive control for the spy: the client's opening state read is already on the wire.
    expect(h.wire.map(r => r.type)).toEqual(['state'])
    remote(h.client, c.element(panelsWith(removed)))
    await settle()
    const buttons = stopTx()
    expect(buttons, `${c.cockpit} {${removed.join(', ')}}: Stop TX is on screen`).toHaveLength(1)
    expect(buttons[0].disabled, `${c.cockpit} {${removed.join(', ')}}: Stop TX is enabled remotely`).toBe(false)
    expect(buttons[0].getAttribute('data-remote-stop')).toBe('true')
    expect(h.stops()).toHaveLength(0)
    fireEvent.click(buttons[0])
    await settle()
    expect(h.stops(), `${c.cockpit}: one click, one stopTransmit`).toHaveLength(1)
    expect(h.stops()[0]).toMatchObject({ stationBootId: h.state.stationBootId, leaseId: h.state.leaseId, transmitEpoch: '000000000000002a' })
    expect(h.commands()).toHaveLength(0)
    cleanup(); uninstall?.(); uninstall = undefined
  }
})

it.each(CASES.map(c => [c.cockpit, c] as const))('%s: with stale readings, stale control and a command pending, Stop stays enabled and is sent at once', async (_name, c) => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  const h = session('control', ['decoder'])
  // An ordinary station command left unanswered: the busy banner's state, and it holds the request slot.
  void h.client.control({ action: 'decoder.clear', receiver: 'cw' }).catch(() => {})
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(h.commands()).toHaveLength(1)
  await act(async () => { await vi.advanceTimersByTimeAsync(1500) })
  expect(h.client.getSnapshot()).toMatchObject({ fresh: false, busy: true })
  expect(h.client.getSnapshot().controlPending).not.toBeNull()
  remote(h.client, c.element(panelsWith([])), false)
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  const [button] = stopTx()
  expect(button.disabled).toBe(false)
  fireEvent.click(button)
  // No timer advances: the request leaves on the click's own microtasks, waiting for nothing.
  await act(async () => { await Promise.resolve(); await Promise.resolve() })
  expect(h.stops()).toHaveLength(1)
  expect(h.commands()).toHaveLength(1)
})

it.each(['noControl', 'noTransmitGrant'] as const)('a browser %s gets the existing refusal: Stop is disabled and a halt sends nothing', async authority => {
  for (const c of CASES) {
    const h = session(authority)
    remote(h.client, c.element(panelsWith([])))
    await settle()
    const [button] = stopTx()
    expect(button.disabled, `${c.cockpit}: Stop TX disabled without stop authority`).toBe(true)
    fireEvent.click(button)
    await expect(haltTx()).rejects.toThrow('localPermissionRequired')
    await settle()
    expect(h.stops()).toHaveLength(0)
    cleanup(); uninstall?.(); uninstall = undefined
  }
})

it('every cockpit vocabulary has a Remote stop case, or is declared elsewhere', () => {
  const ELSEWHERE: Record<string, string> = {
    // Operate's Stop TX is FtStopControl (useStationStopControl) in the QSO strip and the log dialog;
    // FtStopControl, QsoLoggingControls and the hosted browser suite already send it remotely.
    operate: 'FtStopControl',
    // Connect draws no transmit control.
    connect: 'no transmit control',
  }
  for (const vocab of ALL_PANEL_VOCABULARIES)
    expect(CASES.some(c => c.view === vocab.view) || vocab.view in ELSEWHERE, `${vocab.view} has no Remote stop case`).toBe(true)
  for (const c of CASES) expect([...c.ids]).toEqual([...ALL_PANEL_VOCABULARIES.find(v => v.view === c.view)!.panelIds])
})
