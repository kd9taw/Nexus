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
//   · a browser without station control (none at all, or a logging-only lease) gets the existing
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
import type { AppSnapshot, Settings } from '../types'
import App, { type BrowserWorkspace } from '../App'
import { allFeatureIds, featureById, type View } from '../features/registry'
import { dismissToast, subscribeToasts, withErrorToast } from '../toast'
import { EN } from '../i18n'

// THE BUDGET (2026-10-04). The App the hosted-page tests here mount is real work, and it scales with the CPU a test
// gets. Apart from the Esc sweep, which has its own budget below, the slowest test takes 0.41 s on a quiet box,
// 2.4–2.9 s with a fifth of a CPU and 6.0 s with a tenth, against vitest's 5 s default. 15 s is over twice the
// tenth; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../components/PhoneScope', () => ({ PhoneScope: () => <div/> }))
vi.mock('../components/BandStrip', () => ({ BandStrip: () => <div/> }))
vi.mock('../components/LogEntry', () => ({ LogEntry: () => <div/> }))
vi.mock('../components/SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('../components/Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap"/> }))
vi.mock('../components/VoiceKeyer', () => ({ VoiceKeyer: () => <div/> }))
vi.mock('./useJs8Context', () => ({ useJs8Context: () => ({ remote: true, value: null, loading: false, refresh: () => {} }) }))
// The rest of the module is the real one: the hosted App below subscribes to its popups.
vi.mock('../toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
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

// 'loggingOnly': a lease held for logging alone; the station publishes no stop token for it. A browser
// with station control but no transmit permission holds the token (stop anything, 2026-09-14), so the
// station, not this file, decides that case; it is 'control' here.
type Authority = 'control' | 'noControl' | 'loggingOnly'
/** A browser session wired exactly as BrowserApplication wires it: a real operation client behind the
 * real hosted control transport, installed under the application API the cockpits call. With
 * `attempted`, every command the page hands the transport is recorded there first, refused or not. */
function session(authority: Authority, capabilities: string[] = [], answer?: (command: string) => unknown, attempted?: string[]) {
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
    const value = answer ? answer(command) : command === 'get_snapshot' ? snap : command.includes('settings') ? settings
      : /licensed_band_plan|band_plan|get_log|unproven|voice_messages|memories/.test(command) ? []
      : command.includes('rtty') ? rtty : command.includes('psk') ? psk : command.includes('js8') ? js8
      : command.includes('sstv') ? sstv : command.startsWith('cw') || command.includes('_cw') ? cw : {}
    return structuredClone(value) as T
  } }
  const transport = controlTransport(reads, { age: () => 0 } as unknown as ApplicationClient, client)
  uninstall = installApplicationTransport(!attempted ? transport : { kind: 'remote', invoke: <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    attempted.push(command)
    return transport.invoke<T>(command, args)
  } })
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

it.each(['noControl', 'loggingOnly'] as const)('a browser %s gets the existing refusal: Stop is disabled and a halt sends nothing', async authority => {
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

// The relay holds a session to two Stops a second, and Stop TX stays ready after an accepted Stop, so a
// fast operator meets that limit: a third press in a second is refused `remoteBusy` with the first two
// accepted. The station is already stopping, so the page must not say "Could not stop transmit"
// (2026-10-03). Phone's TX strip, with the page's real toast.
it('a Stop the relay refuses within a second of an accepted one raises no failure, and the stop line keeps it', async () => {
  // The client's clock (`1000 + performance.now()`, in `session`) is the test's: it moves only when the test moves it,
  // so the third press is inside the relay's second however long the box takes between the presses. On the real clock
  // a box that stalled for over a second there put the third press outside it, and the page then rightly said the Stop
  // failed. Timers stay real: `settle` waits on them.
  vi.useFakeTimers({ toFake: ['performance'] })
  const real = await vi.importActual<typeof import('../toast')>('../toast')
  const toast = vi.mocked(withErrorToast), stubbed = toast.getMockImplementation()!
  toast.mockImplementation(real.withErrorToast)
  const raised = new Map<number, string>()
  const off = subscribeToasts(all => { for (const t of all) raised.set(t.id, t.message) })
  const failures = () => [...raised.values()].filter(m => m.startsWith(EN['shell.halt.failed']))
  try {
    const pressed = async (h: ReturnType<typeof session>, reply: object) => {
      fireEvent.click(stopTx()[0])
      await settle()
      const stop = h.stops()[h.stops().length - 1]
      act(() => h.client.receive({ type: 'operationResponse', requestId: stop.requestId, ...reply }))
      await settle()
    }
    // POSITIVE CONTROL: refused `remoteBusy` with nothing accepted before it, the press says so.
    const alone = session('control')
    remote(alone.client, CASES[0].element(panelsWith([])))
    await settle()
    await pressed(alone, { error: 'remoteBusy' })
    expect(failures(), 'control: a refused Stop toasts').toEqual([`${EN['shell.halt.failed']}: remoteBusy`])
    cleanup(); uninstall?.(); uninstall = undefined
    act(() => { for (const id of raised.keys()) dismissToast(id) })
    raised.clear()

    const h = session('control')
    remote(h.client, CASES[0].element(panelsWith([])))
    await settle()
    await pressed(h, { value: { stop: 'accepted' } })
    await pressed(h, { value: { stop: 'accepted' } })
    await pressed(h, { error: 'remoteBusy' })
    expect(h.stops(), 'premise: three presses, three stops sent').toHaveLength(3)
    expect(failures(), 'a third press inside the relay’s second said the Stop failed').toEqual([])
    expect([EN['remote.stop.sent'], EN['remote.stop.stopped']], 'the stop line lost the acceptance')
      .toContain(document.querySelector('.cockpit-stopstate')?.textContent)
    expect(stopTx()[0].disabled).toBe(false)
  } finally {
    off()
    act(() => { for (const id of raised.keys()) dismissToast(id) })
    toast.mockImplementation(stubbed)
  }
})

// ── Esc on the hosted page ──────────────────────────────────────────────────────────────────────
// The hosted page is App. While Tempo, Phone, SSTV, APRS or Satellites is on show, App binds Esc to
// the shell's halt: the api haltTx that the cockpits' Stop TX uses above. So a browser holding
// control sends exactly one stopTransmit for an Esc on each of those screens, and a browser without
// control sends none. Operate's own Esc is the reference; it reaches the same halt through App.
const ESC_RAIL: Array<[view: string, rail: string]> = [['chat', 'Tempo'], ['phone', 'Phone'], ['sstv', 'SSTV'], ['aprs', 'APRS'], ['sats', 'Satellites']]
// CW, RTTY, PSK and JS8 bind their own Esc only with local control, so on the hosted page Esc did
// nothing there while their Stop TX (the TX strip's) sent the station's stop. There App binds their
// Esc to that same halt (operator, 2026-10-02).
const COCKPIT_RAIL: Array<[view: string, rail: string]> = [['cw', 'CW'], ['rtty', 'RTTY'], ['psk', 'PSK'], ['js8', 'JS8']]
/** What the hosted App reads, served as stale-actionability.test.tsx serves it: the snapshot and the
 * settings, and every other read failing as an unsupported one does, which the screens catch. */
const digital = { ...snap, radio: { ...snap.radio, operatingMode: 'digital' } } as AppSnapshot
function hostedAnswer(command: string): unknown {
  if (command === 'get_snapshot') return digital
  if (command.includes('settings')) return settings
  if (command.includes('band_plan')) return []
  if (command.startsWith('get_')) throw new Error('applicationUnsupported')
  return undefined
}
/** The hosted page as BrowserApplication mounts it, with every screen these tests visit offered. `over`
 * withdraws a screen's station API, or makes the readings stale (BrowserApplication's `!stale`). */
function hosted(client: OperationClient, over: Partial<BrowserWorkspace> = {}) {
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: Object.fromEntries(allFeatureIds().map(id => [id, true])) }))
  return render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={!over.stale}>
    <RemoteOperationsContext.Provider value={client}>
      <App remote={{ snapshot: digital, settings: settings as unknown as Settings, bandPlan: [], stale: false, status: <div/>, cwPhone: true, keyboard: true, js8: true, stationModes: true, navigation: true, ...over }}/>
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
}
/** Go to `view` by its rail button, as the operator does, and check it is the screen on show. */
async function railTo(view: string, rail: string) {
  const button = [...document.querySelectorAll<HTMLButtonElement>('.mode-nav button')].find(b => b.querySelector('.mode-label')?.textContent === rail)
  expect(button, `no rail button for ${rail}`).toBeDefined()
  fireEvent.click(button!)
  await settle()
  expect(document.title, `control: ${view} is the screen on show`).toBe(`${featureById(view as View)!.label} — Nexus`)
}
function escOn() {
  const on = document.activeElement ?? document.body
  fireEvent.keyDown(on, { key: 'Escape', code: 'Escape' })
  fireEvent.keyUp(on, { key: 'Escape', code: 'Escape' })
}

// A fresh session per screen: the operation client refuses a second stop while the first is unanswered
// ('remoteBusy'), and nothing answers here.
it.each([['operate (the reference)', 'operate', 'FT'], ...[...ESC_RAIL, ...COCKPIT_RAIL].map(([view, rail]) => [view, view, rail])] as Array<[string, string, string]>)(
  'the hosted page, %s: Esc sends exactly one stopTransmit and no station command', async (_name, view, rail) => {
    // Recorded at the transport too: a second halt on the same press is refused before the wire.
    const attempted: string[] = []
    const h = session('control', [], hostedAnswer, attempted)
    hosted(h.client)
    await settle()
    expect(document.title, 'control: the hosted page opens on Operate').toBe(`${featureById('operate')!.label} — Nexus`)
    if (view !== 'operate') await railTo(view, rail)
    ;(document.activeElement as HTMLElement | null)?.blur()
    expect(h.stops()).toHaveLength(0)
    escOn()
    await settle()
    expect(h.stops(), `${view}: one Esc, one stopTransmit`).toHaveLength(1)
    expect(attempted.filter(c => c === 'halt_tx'), `${view}: one Esc, one halt`).toEqual(['halt_tx'])
    expect(h.stops()[0]).toMatchObject({ stationBootId: h.state.stationBootId, leaseId: h.state.leaseId, transmitEpoch: '000000000000002a' })
    expect(h.commands(), 'an Esc sent an ordinary station command').toHaveLength(0)
  })

// THIS TEST'S BUDGET (2026-10-04). It walks every screen and presses Esc on each: 0.52 s on a quiet box, 3.8–4.6 s
// with a fifth of a CPU and 9.9–13.3 s with a tenth. 30 s is over twice the tenth; a hang still fails, after 30 s.
it('the hosted page: a browser without control sends nothing for Esc on any of those screens', async () => {
  const h = session('noControl', [], hostedAnswer)
  hosted(h.client)
  await settle()
  for (const [view, rail] of [['operate', 'FT'], ...ESC_RAIL, ...COCKPIT_RAIL] as Array<[string, string]>) {
    if (view !== 'operate') await railTo(view, rail)
    escOn()
    await settle()
  }
  expect(h.stops()).toHaveLength(0)
  expect(h.commands()).toHaveLength(0)
}, 30_000)

// Stop authority is what every Stop TX button follows (useStationStopControl), and App's Esc follows
// it too: an observer's Esc is not a refused halt, it is no halt at all, so it never reaches the
// transport and never toasts a refusal. Recorded at the transport, as BrowserApplication.test.tsx's
// observer cockpits are, so a refused attempt would show here even though nothing left the browser.
it('the hosted page: an observer’s Esc on those screens does not even attempt a halt', async () => {
  const attempted: string[] = []
  uninstall = installApplicationTransport({ kind: 'remote', invoke: async <T,>(command: string): Promise<T> => {
    attempted.push(command)
    return hostedAnswer(command) as T
  } })
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: Object.fromEntries(allFeatureIds().map(id => [id, true])) }))
  render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <App remote={{ snapshot: digital, settings: settings as unknown as Settings, bandPlan: [], stale: false, status: <div/>, cwPhone: true, stationModes: true, navigation: true }}/>
  </StationDataContext.Provider></StationControlContext.Provider>)
  await settle()
  for (const [view, rail] of ESC_RAIL) {
    await railTo(view, rail)
    escOn()
    await settle()
  }
  expect(attempted.length, 'control: the transport recorded the page’s reads').toBeGreaterThan(0)
  expect(attempted.filter(c => c === 'halt_tx')).toEqual([])
})

// FT's Esc rides the same listener, with the same authority (operator, 2026-10-01). It used to call
// the halt whoever pressed it, so on the hosted Operate page an observer's Esc reached the transport,
// which refused it before the wire, and the page toasted "Could not stop transmit:
// localPermissionRequired". Recorded at the transport, refused or not, with the page's real toast.
it('the hosted page: an observer’s Esc on FT sends no halt and shows no refusal toast', async () => {
  const real = await vi.importActual<typeof import('../toast')>('../toast')
  const toast = vi.mocked(withErrorToast), stubbed = toast.getMockImplementation()!
  toast.mockImplementation(real.withErrorToast)
  const raised: number[] = []
  const off = subscribeToasts(all => { for (const t of all) if (!raised.includes(t.id)) raised.push(t.id) })
  const before = [...raised]
  try {
    const attempted: string[] = []
    hosted(session('noControl', [], hostedAnswer, attempted).client)
    await settle()
    expect(document.title, 'control: the hosted page opens on Operate').toBe(`${featureById('operate')!.label} — Nexus`)
    ;(document.activeElement as HTMLElement | null)?.blur()
    escOn()
    await settle()
    expect(attempted.length, 'control: the transport recorded the page’s reads').toBeGreaterThan(0)
    expect(attempted.filter(c => c === 'halt_tx'), 'an observer’s Esc attempted a halt').toEqual([])
    expect(document.body.textContent, 'an observer’s Esc toasted a refusal').not.toContain(EN['shell.halt.failed'])
    // POSITIVE CONTROL: the halt Esc used to make, made the same way, is recorded and toasts its refusal.
    await act(async () => { await withErrorToast(() => haltTx(), EN['shell.halt.failed']) })
    expect(attempted.filter(c => c === 'halt_tx'), 'control: a halt attempt is recorded').toEqual(['halt_tx'])
    expect(document.body.textContent, 'control: a refusal toasts on this page').toContain(`${EN['shell.halt.failed']}: localPermissionRequired`)
  } finally {
    off()
    act(() => { for (const id of raised) if (!before.includes(id)) dismissToast(id) })
    toast.mockImplementation(stubbed)
  }
})

// ── Esc on the hosted CW, RTTY, PSK and JS8 (operator, 2026-10-02) ──────────────────────────────
/** The one remote Stop TX on show: the TX strip's, which a controlling browser sends with. */
const shownRemoteStops = () => [...document.querySelectorAll<HTMLButtonElement>('button[data-remote-stop]')].filter(b => b.closest('[hidden]') == null)

// Compared on the wire, in two sessions: the client refuses a second stop while the first is
// unanswered, and nothing answers here.
it.each(COCKPIT_RAIL)('the hosted page, %s: Esc sends exactly what its Stop TX sends', async (view, rail) => {
  const sentBy = async (gesture: () => void) => {
    const attempted: string[] = []
    const h = session('control', [], hostedAnswer, attempted)
    hosted(h.client)
    await settle()
    await railTo(view, rail)
    ;(document.activeElement as HTMLElement | null)?.blur()
    gesture()
    await settle()
    const sent = { halts: attempted.filter(c => c === 'halt_tx').length, commands: h.commands().length,
      stops: h.stops().map(({ requestId: _id, stationBootId, leaseId, ...rest }) => ({ ...rest, ownBoot: stationBootId === h.state.stationBootId, ownLease: leaseId === h.state.leaseId })) }
    cleanup(); uninstall?.(); uninstall = undefined
    return sent
  }
  const byStopTx = await sentBy(() => {
    const stops = shownRemoteStops()
    expect(stops, `control: ${view} shows one remote Stop TX`).toHaveLength(1)
    expect(stops[0].disabled, `control: ${view}'s Stop TX is enabled`).toBe(false)
    fireEvent.click(stops[0])
  })
  expect(byStopTx, `control: ${view}'s Stop TX sends the station's stop`).toEqual({ halts: 1, commands: 0,
    stops: [{ type: 'stopTransmit', transmitEpoch: '000000000000002a', ownBoot: true, ownLease: true }] })
  expect(await sentBy(escOn), `${view}: Esc sent something other than its Stop TX`).toEqual(byStopTx)
})

// Stale readings refuse ordinary controls at once, never Stop (above), so they never take Esc's away.
it.each(COCKPIT_RAIL)('the hosted page, %s: with stale readings Esc still sends the stop', async (view, rail) => {
  const attempted: string[] = []
  const h = session('control', [], hostedAnswer, attempted)
  hosted(h.client, { stale: true })
  await settle()
  await railTo(view, rail)
  expect(shownRemoteStops().map(b => b.disabled), `control: ${view}'s Stop TX is enabled with stale readings`).toEqual([false])
  ;(document.activeElement as HTMLElement | null)?.blur()
  escOn()
  await settle()
  expect(attempted.filter(c => c === 'halt_tx'), `${view}: one Esc, one halt`).toEqual(['halt_tx'])
  expect(h.stops()).toHaveLength(1)
  expect(h.commands()).toHaveLength(0)
})

// Where the station does not offer the screen, the page shows a notice and no Stop TX, and Esc
// there still sends nothing.
it.each([['cw', 'CW', { cwPhone: false }], ['rtty', 'RTTY', { keyboard: false }], ['psk', 'PSK', { keyboard: false }], ['js8', 'JS8', { js8: false }]] as Array<[string, string, Partial<BrowserWorkspace>]>)(
  'the hosted page, %s, when the station does not offer it: Esc sends nothing', async (view, rail, over) => {
    const attempted: string[] = []
    const h = session('control', [], hostedAnswer, attempted)
    hosted(h.client, over)
    await settle()
    await railTo(view, rail)
    expect(document.querySelector('.remote-view-unavailable'), `control: ${view} shows the notice`).not.toBeNull()
    expect(screen.queryAllByRole('button', { name: /^stop tx$/i }), `control: ${view} shows no Stop TX`).toHaveLength(0)
    ;(document.activeElement as HTMLElement | null)?.blur()
    escOn()
    await settle()
    expect(attempted.filter(c => c === 'halt_tx'), `${view}: Esc on the notice attempted a halt`).toEqual([])
    expect(h.stops()).toHaveLength(0)
  })

// An observer's Esc there stays nothing: no halt attempted (recorded at the transport, refused or
// not), so no refusal toasts, with the page's real toast as on FT above.
it.each(COCKPIT_RAIL.flatMap(([view, rail]) => (['noControl', 'loggingOnly'] as const).map(a => [view, a, rail] as const)))(
  'the hosted page, %s, a browser %s: Esc attempts no halt and shows nothing', async (view, authority, rail) => {
    const real = await vi.importActual<typeof import('../toast')>('../toast')
    const toast = vi.mocked(withErrorToast), stubbed = toast.getMockImplementation()!
    toast.mockImplementation(real.withErrorToast)
    const raised: number[] = []
    const off = subscribeToasts(all => { for (const t of all) if (!raised.includes(t.id)) raised.push(t.id) })
    const before = [...raised]
    try {
      const attempted: string[] = []
      const h = session(authority, [], hostedAnswer, attempted)
      hosted(h.client)
      await settle()
      await railTo(view, rail)
      ;(document.activeElement as HTMLElement | null)?.blur()
      escOn()
      await settle()
      expect(attempted.length, 'control: the transport recorded the page’s reads').toBeGreaterThan(0)
      expect(attempted.filter(c => c === 'halt_tx'), `${view}: an observer’s Esc attempted a halt`).toEqual([])
      expect(h.stops()).toHaveLength(0)
      expect(document.body.textContent, `${view}: an observer’s Esc toasted a refusal`).not.toContain(EN['shell.halt.failed'])
      // POSITIVE CONTROL: the halt made directly is recorded and toasts its refusal on this page.
      await act(async () => { await withErrorToast(() => haltTx(), EN['shell.halt.failed']) })
      expect(attempted.filter(c => c === 'halt_tx'), 'control: a halt attempt is recorded').toEqual(['halt_tx'])
      expect(document.body.textContent, 'control: a refusal toasts on this page').toContain(`${EN['shell.halt.failed']}: localPermissionRequired`)
    } finally {
      off()
      act(() => { for (const id of raised) if (!before.includes(id)) dismissToast(id) })
      toast.mockImplementation(stubbed)
    }
  })

it('every cockpit vocabulary has a Remote stop case, or is declared elsewhere', () => {
  const ELSEWHERE: Record<string, string> = {
    // Operate's Stop TX is FtStopControl (useStationStopControl) in the QSO strip and the log dialog;
    // FtStopControl, QsoLoggingControls and the hosted browser suite already send it remotely.
    operate: 'FtStopControl',
    // Connect draws no transmit control, and App draws no top bar on Connect: the stop line's one
    // ruled exception (the operator, 2026-10-01: "remove all radio control from connect, reclaim
    // that space"). Transmit there is stopped by Esc — App's halt, through the same api haltTx the
    // header's Stop TX calls — or by leaving the screen.
    connect: 'no transmit control; the ruled exception: "remove all radio control from connect, reclaim that space" (Esc stops)',
    // The dashboard rail's slots: the rail draws no transmit control, and it is not on the hosted page.
    dashrail: 'no transmit control; not on the hosted page',
  }
  for (const vocab of ALL_PANEL_VOCABULARIES)
    expect(CASES.some(c => c.view === vocab.view) || vocab.view in ELSEWHERE, `${vocab.view} has no Remote stop case`).toBe(true)
  for (const c of CASES) expect([...c.ids]).toEqual([...ALL_PANEL_VOCABULARIES.find(v => v.view === c.view)!.panelIds])
})
