// @vitest-environment jsdom
//
// "STOP SENT", NEVER "STOPPED" — what the browser may claim after an accepted Stop.
//
// `stop_transmit` answers on ACCEPTANCE, not on RF: it says so itself, and when the station's
// Engine is held (a radio-loop tick, another command) the halt runs afterwards on its own thread
// with no deadline. So between the reply and the transmitter actually going free the rig is still
// on the air, and a browser that said "stopped" there would be telling the operator the opposite
// of the truth about a keyed radio.
//
// What this file computes:
//   · the two transitions, on the real remote path — click → SENDING, the station's acceptance →
//     SENT while its own reading still shows the transmitter keyed, and a later reading showing it
//     free → STOPPED. The middle step is the one the old wording got wrong;
//   · the busy station, which is the ordinary case rather than the exception: the confirmation
//     arrives a beat after the acceptance, and until it does the browser says SENT;
//   · POSITIVE CONTROL, twice over, because a component that never says "stopped" would pass the
//     assertions above by accident: the same click against a station already reporting the
//     transmitter free says STOPPED at once, and the catalog is checked for the two words
//     themselves (a state machine that is right under strings that both read "stopped" is not);
//   · a refused Stop claims nothing at all.
import { afterEach, beforeAll, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { PhoneCockpit } from '../components/PhoneCockpit'
import { FtStopControl } from '../components/FtStopControl'
import type { PanelLayoutApi } from '../features/panelState'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { installApplicationTransport } from '../applicationTransport'
import { EN, t } from '../i18n'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { controlTransport } from './control-transport'
import type { ApplicationClient } from './application-client'
import type { OperationState } from './operation-protocol'
import settings from '../components/__fixtures__/defaultSettings.json'
import type { AppSnapshot, RadioStatus } from '../types'

vi.mock('../components/PhoneScope', () => ({ PhoneScope: () => <div/> }))
vi.mock('../components/BandStrip', () => ({ BandStrip: () => <div/> }))
vi.mock('../components/LogEntry', () => ({ LogEntry: () => <div/> }))
vi.mock('../components/SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('../components/Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap"/> }))
vi.mock('../components/VoiceKeyer', () => ({ VoiceKeyer: () => <div/> }))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (run: () => Promise<unknown>) => { try { return await run() } catch { return null } }),
}))

const RADIO = { source: 'native', dialMhz: 14.2, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null, rigMode: 'USB',
  operatingMode: 'phone', transmitting: false, tuning: false, rigKeyed: false, txEnabled: false, txAllowed: true, qsoRecording: false,
  rfPower: null, micGain: null, nrLevel: 0.3, agc: 'fast', nb: false, nr: false, notch: null, comp: null, vox: null, filterWidthHz: 500,
  splitTxMhz: null, smeterDb: null, cwWpm: 22, cwKeyer: 'cat', phoneSegLo: null, phoneSegHi: null } as unknown as RadioStatus
/** The station's own reading of its transmitter — keyed, or free. */
const radio = (keyed: boolean): RadioStatus => ({ ...RADIO, transmitting: keyed, rigKeyed: keyed, txEnabled: keyed })
const snapshot = (keyed: boolean) => ({ activeRadioId: 1, mycall: 'N0CALL', mygrid: 'AA00', mode: 'qso', stations: [], recentDecodes: [],
  conversations: [], highlights: [], link: { tier: 'FT8', dtSec: 0 }, radio: radio(keyed) } as unknown as AppSnapshot)

const clients: OperationClient[] = []
let uninstall: (() => void) | undefined
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
beforeEach(() => { localStorage.clear(); vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null) })
afterEach(() => { cleanup(); uninstall?.(); uninstall = undefined; clients.splice(0).forEach(c => c.disconnected()); vi.restoreAllMocks() })

/** A controlling browser on the real hosted transport, exactly as `remote-stop-line.test.tsx` wires it. */
function session() {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const wire: any[] = []
  const values = new Map<string, string>()
  const store = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(raw => wire.push(JSON.parse(raw).request), true, () => 1000 + performance.now(), undefined, 4,
    pendingControlStorage(() => store, 'stop-progress', async (_key, run) => run()))
  clients.push(client)
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling',
    leaseId: crypto.randomUUID(), revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000,
    actions: [], txArmed: false, transmitEpoch: '000000000000002a',
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities: [] as never } }
  client.open()
  client.receive({ type: 'operationResponse', requestId: wire[wire.length - 1].requestId, value: state })
  const reads = { kind: 'remote' as const, invoke: async <T,>(command: string): Promise<T> => {
    const value = command === 'get_snapshot' ? snapshot(true) : command.includes('settings') ? settings
      : /licensed_band_plan|band_plan|get_log|unproven|voice_messages|memories/.test(command) ? [] : {}
    return structuredClone(value) as T
  } }
  uninstall = installApplicationTransport(controlTransport(reads, { age: () => 0 } as unknown as ApplicationClient, client))
  return { client, wire, stops: () => wire.filter(r => r.type === 'stopTransmit') }
}

const panels: PanelLayoutApi<string> = { layout: { v: 1, state: {}, share: {} }, stateOf: () => 'docked', setPanelState: () => {},
  shareOf: () => 1, setShare: () => {}, setShares: () => {}, undo: () => {}, canUndo: false, undoRemoves: [], reset: () => {} }
const phone = (keyed: boolean) => <PhoneCockpit snap={snapshot(keyed)} theme="dark" spots={[]} onWorkSpot={() => {}} onSnap={() => {}} panels={panels as never}/>
const remote = (client: OperationClient, element: React.ReactElement) =>
  render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={client}>{element}</RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
async function settle() { await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)) }) }
const shown = (key: 'remote.stop.sending' | 'remote.stop.sent' | 'remote.stop.stopped') => screen.queryByText(t(key))

it('the acceptance says stop sent, and only the station reporting the transmitter free says stopped', async () => {
  const h = session()
  const view = remote(h.client, phone(true))
  await settle()
  expect(shown('remote.stop.sent'), 'nothing is claimed before a Stop').toBeNull()
  expect(shown('remote.stop.stopped')).toBeNull()

  fireEvent.click(screen.getByRole('button', { name: /^stop tx$/i }))
  await settle()
  expect(h.stops(), 'one click, one stopTransmit').toHaveLength(1)
  expect(shown('remote.stop.sending'), 'in flight').not.toBeNull()
  expect(shown('remote.stop.stopped')).toBeNull()

  // The station is BUSY: it accepts at once and the halt runs on its own thread afterwards, so its
  // own reading still shows the transmitter keyed. This is the step the old wording got wrong.
  await act(async () => {
    h.client.receive({ type: 'operationResponse', requestId: h.stops()[0].requestId, value: { stop: 'accepted' } })
  })
  await settle()
  expect(shown('remote.stop.sent'), 'accepted, not confirmed off the air').not.toBeNull()
  expect(shown('remote.stop.stopped'), 'the rig is still keyed — nothing may say stopped').toBeNull()

  // A beat later the station's reading shows the transmitter free.
  view.rerender(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={h.client}>{phone(false)}</RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  await settle()
  expect(shown('remote.stop.stopped'), 'the station reports the transmitter free').not.toBeNull()
  expect(shown('remote.stop.sent')).toBeNull()
})

it('POSITIVE CONTROL: the same click against an already free transmitter says stopped at once', async () => {
  const h = session()
  remote(h.client, phone(false))
  await settle()
  fireEvent.click(screen.getByRole('button', { name: /^stop tx$/i }))
  await settle()
  await act(async () => {
    h.client.receive({ type: 'operationResponse', requestId: h.stops()[0].requestId, value: { stop: 'accepted' } })
  })
  await settle()
  expect(shown('remote.stop.stopped'), 'the stopped wording is reachable — so its absence above is a real result').not.toBeNull()
  expect(shown('remote.stop.sent')).toBeNull()
})

it('a refused Stop claims nothing, and a control with no station reading never claims stopped', async () => {
  const h = session()
  remote(h.client, <FtStopControl onHaltTx={() => { void h.client.stopTransmit().catch(() => {}) }}/>)
  await settle()
  fireEvent.click(screen.getByRole('button', { name: /^stop tx$/i }))
  await settle()
  await act(async () => {
    h.client.receive({ type: 'operationResponse', requestId: h.stops()[0].requestId, error: 'staleContext' })
  })
  await settle()
  expect(shown('remote.stop.sent'), 'a refusal is not an acceptance').toBeNull()
  expect(shown('remote.stop.stopped')).toBeNull()
})

it('POSITIVE CONTROL: the two states are actually different words', () => {
  expect(EN['remote.stop.sent']).toBe('Stop sent')
  expect(EN['remote.stop.stopped']).toBe('Stopped')
  expect(EN['remote.stop.sent']).not.toBe(EN['remote.stop.stopped'])
  // The acceptance wording must not assert RF has stopped.
  expect(EN['remote.stop.sent'].toLowerCase()).not.toContain('stopped')
})
