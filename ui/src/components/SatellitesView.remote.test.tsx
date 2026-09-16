// @vitest-environment jsdom
//
// THE SATELLITE SECTION FROM A BROWSER. Until this batch every gesture here followed
// `useStationControl`, which is false in a browser, so the whole section rendered read-only: the
// readiness rail's Stop, the Doppler fix, the VFO map, the transponder cards, peg-lock and the
// element refresh were all drawn and all dead. Reading already worked — the passes, the schedule,
// the detail and the live track all arrive over Remote — so the section looked complete and did
// nothing.
//
// What is pinned here:
//  - the controls are live while the station advertises `satellite`, and each sends ITS OWN
//    gesture, named on the wire the station parses;
//  - without that hint they are drawn and disabled, and a click sends nothing (the defect above,
//    kept as its own case so a regression cannot pass as "the hint is always there");
//  - the rail's Stop is live even while this browser has a transmission armed — an FM bird is
//    worked by transmitting during the very pass this Stop ends.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import { RemoteCollectionsContext, type RemoteCollections } from '../remote-web/collections'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from '../remote-web/operation-client'
import { pendingControlStorage } from '../remote-web/control-storage'
import { pendingLogStorage, type ReceiptLock } from '../remote-web/operation-storage'
import type { OperationState } from '../remote-web/operation-protocol'
import type { ControlCapability, StationAction } from '../remote-web/station-operation'
import { navigationPages } from '../remote-web/__fixtures__/navigation-page'
import type { QueryPage } from '../remote-web/application-query-protocol'
import { t } from '../i18n'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn(() => Promise.resolve(null)),
  getSatSchedule: vi.fn(() => Promise.resolve([])),
  getSatPassNeeds: vi.fn(() => Promise.resolve([])),
  getSatDetail: vi.fn(() => Promise.resolve(null as never)),
  getSettings: vi.fn(() => Promise.resolve({ mygrid: GRID } as never)),
  setSettings: vi.fn(() => Promise.resolve({} as never)),
  setPegLock: vi.fn(() => Promise.resolve({} as never)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn(() => Promise.resolve(null)),
  startSatTrack: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn(() => Promise.resolve(null)),
  fetchTlesNow: vi.fn(() => Promise.resolve({} as never)),
  confirmSatUplink: vi.fn(() => Promise.resolve({} as never)),
}))
vi.mock('../api', () => api)
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

beforeEach(() => { localStorage.clear() })
afterEach(() => { cleanup(); vi.clearAllMocks() })

/** A live pass, as `get_remote_satellite_state` samples it: a track past AOS with a held
 * transponder, stale elements (so the rail offers its Elements fix) and Doppler switched off (so
 * it offers its Doppler fix). Exactly the state in which every rail control is on screen. */
const GRID = 'EN52', BIRD = 'SO-50'
const live = () => ({
  settings: { mygrid: GRID, rotatorConfigured: false, satDopplerOff: true, satVfoMap: 'off', radioPegged: false },
  track: {
    name: BIRD, state: 'tracking', mode: 'doppler-only', dopplerDownlink: false, dopplerUplink: false,
    uplinkOffer: 'none', uplinkOfferMap: null, uplinkRadio: 'IC-9700', uplinkRadioId: 1, txMode: null,
    rotorLost: false, azDeg: null, elDeg: null, aosAzDeg: 120, maxElDeg: 40, satAzDeg: null, satElDeg: null,
    rangeKm: null, rangeRateKmS: null, altKm: null, downlinkHz: null, uplinkHz: null, downlinkShiftHz: null,
    uplinkShiftHz: null, transponder: null, transponderIndex: null, inverting: false, offsetHz: null,
    halfWidthHz: null, elementAgeDays: 22, elementEpochUnix: 1785542400,
    aosUnix: Math.floor(Date.now() / 1000) - 60, losUnix: Math.floor(Date.now() / 1000) + 600,
  },
  held: { name: BIRD, index: 0, description: 'FM repeater', binding: { radioId: 1, radioName: 'IC-9700',
    band: '70cm', fm: true, simplex: false, downlinkMhz: 436.795, uplinkMhz: 145.85,
    pendingDownlinkMhz: null, pendingUplinkMhz: null, note: null } },
})

/** A station that answers. It has to: the client admits one control receipt at a time and reads
 * the station's state back after each one, and a page whose receipt never resolves correctly
 * disables every control it holds — which is the state this file would otherwise be testing. */
function operations(capabilities: ControlCapability[], txArmed = false) {
  const values = new Map<string, string>(), wire: string[] = []
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const lock: ReceiptLock = async (_k, run) => run()
  const boot = crypto.randomUUID(), lease = crypto.randomUUID(), epoch = '00000000000000ff'
  let revision = 1, sequence = 1
  const outcomes = new Map<string, unknown>()
  const state = (): OperationState => ({ stationBootId: boot, allowed: true, phase: 'controlling', leaseId: lease,
    revision, commandWindowId: crypto.randomUUID(), nextSequence: sequence, leaseRemainingMs: 5000, actions: [],
    txArmed, ...(txArmed ? { transmitEpoch: epoch } : {}),
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities } })
  let client: OperationClient
  const answer = (sent: string) => {
    wire.push(sent)
    const request = JSON.parse(sent).request
    if (!request) return
    // `applied` is the honest answer for every one of these. What the RADIO then did is read back
    // from the live sample the rail polls, never from this receipt.
    const outcome = { operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'stationState' }
    if (request.type === 'stationControl') { sequence = request.clientSequence + 1; revision++; outcomes.set(request.requestId, outcome) }
    const value = request.type === 'stationControl' ? outcome
      : request.type === 'result' ? outcomes.get(request.operationId) ?? outcome : state()
    queueMicrotask(() => client.receive({ type: 'operationResponse', requestId: request.requestId, value: value as never }))
  }
  client = new OperationClient(answer, true, () => 1000 + performance.now(),
    pendingLogStorage(() => data, 'sat', lock), 3, pendingControlStorage(() => data, 'sat', lock))
  client.open()
  const actions = (): StationAction[] => wire.map(s => JSON.parse(s).request?.action).filter(Boolean)
  return { client, actions }
}

/** The two sealed documents the section reads: the catalog page and the open bird. Assembled by
 * the same chunker the station uses, so `loadNavigation`'s envelope checks are real here. */
const documents: Record<string, unknown> = {
  satellites: { mygrid: GRID, view: null },
  satellite: {
    mygrid: GRID, name: BIRD, logCount: 0, schedule: [],
    detail: { name: BIRD, norad: 27607, status: 'alive', dataFetchedAt: 1789012800, pass: null, passTrack: [],
      transmitters: [{ description: 'FM repeater', alive: true, mode: 'FM', kind: 'Transceiver',
        uplinkLowHz: 145_850_000, uplinkHighHz: null, downlinkLowHz: 436_795_000, downlinkHighHz: null,
        invert: false, uplinkMode: 'FM', downlinkMode: 'FM' }] },
  },
}

function browser(capabilities: ControlCapability[], txArmed = false) {
  const ops = operations(capabilities, txArmed)
  const page = async (args: { collection: string; search: string; cursor: string | null }): Promise<QueryPage> => {
    const raw = documents[args.collection]
    if (!raw) throw new Error('unexpectedCollection')
    const pages = navigationPages(args.collection, raw, args.search)
    return structuredClone(pages[args.cursor === null ? 0 : Number(args.cursor.split(':')[1])])
  }
  const source = { page, client: { supports: () => true, invoke: async () => structuredClone(live()) } } as unknown as RemoteCollections
  const view = render(
    <StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
      <RemoteOperationsContext.Provider value={ops.client}>
        <RemoteCollectionsContext.Provider value={source}>
          <SatellitesView focusSat={BIRD} />
        </RemoteCollectionsContext.Provider>
      </RemoteOperationsContext.Provider>
    </StationDataContext.Provider></StationControlContext.Provider>)
  return { ...ops, view }
}

const rail = () => screen.findByTestId('sat-rail')
const button = (label: string) => screen.getByRole('button', { name: label }) as HTMLButtonElement
/** The RAIL's Stop, not the tracking badge's ■ beside it: both end the pass, and the rail's is the
 * one the readiness chain owns. */
const railStop = async () => within(await rail()).getByRole('button', { name: t('sat.track.stop') }) as HTMLButtonElement

/** Each gesture gets its own mount. The client admits ONE control receipt at a time and settles it
 * against the station's next state, so driving five clicks through a single fake would be testing
 * that lifecycle rather than the gestures — and it is already tested where it lives. */
const gestures: [string, () => HTMLElement, StationAction][] = [
  ['the rail Doppler fix', () => button(t('sat.rail.doppler.turnOn')), { action: 'satellite.doppler', on: true }],
  ['the rail Elements fix', () => button(t('sat.rail.elements.refresh')), { action: 'satellite.elements' }],
  // The map select carries the rig the rail NAMED, never whichever radio is active at the station
  // when the click lands.
  ['the VFO map select', () => screen.getByLabelText(t('sat.rail.vfoMap.aria')), { action: 'satellite.uplinkMap', map: 'a-up-b-down', radioId: 1 }],
  ['the radio binding peg', () => button(t('sat.binding.unpegged')), { action: 'satellite.peg', on: true }],
]

it.each(gestures)('%s is live with the satellite hint and sends its own gesture', async (_name, find, expected) => {
  const { actions } = browser(['satellite'])
  await rail()
  const control = find() as HTMLButtonElement | HTMLSelectElement
  expect(control.disabled).toBe(false)
  // ⛔ Nothing is sent because the section opened or the sample arrived — only because of a click.
  expect(actions()).toEqual([])
  if (control instanceof HTMLSelectElement) fireEvent.change(control, { target: { value: 'a-up-b-down' } })
  else fireEvent.click(control)
  await waitFor(() => expect(actions()).toEqual([expected]))
})

it('the transponder chooser hands the dial back through the station, indexing the list it was shown', async () => {
  const { actions } = browser(['satellite'])
  await rail()
  const none = screen.getByLabelText(t('sat.transponder.none.aria')) as HTMLInputElement
  expect(none.disabled).toBe(false)
  fireEvent.click(none)
  // `index: null` IS the handback, and it is a consent statement the station honours per bird.
  await waitFor(() => expect(actions()).toEqual([{ action: 'satellite.transponder', name: BIRD, index: null, auto: false }]))
  expect(api.setSatTransponder).not.toHaveBeenCalled()
})

it('the rail Stop ends the pass through the station, not the desktop command', async () => {
  const { actions } = browser(['satellite'])
  const stop = await railStop()
  expect(stop.disabled).toBe(false)
  fireEvent.click(stop)
  await waitFor(() => expect(actions()).toEqual([{ action: 'satellite.stopTrack' }]))
  // The desktop's own commands are not what a browser reaches.
  expect(api.stopSatTrack).not.toHaveBeenCalled()
  expect(api.setPegLock).not.toHaveBeenCalled()
  expect(api.setSettings).not.toHaveBeenCalled()
  expect(api.fetchTlesNow).not.toHaveBeenCalled()
  expect(api.confirmSatUplink).not.toHaveBeenCalled()
})

it('without the hint the whole section is drawn and dead, and a click sends nothing', async () => {
  // The parity defect itself. `frequency` is a station control this browser really holds, so this
  // is a page WITH control that the station has not offered the satellite section to — not a page
  // that is simply disconnected.
  const { actions } = browser(['frequency'])
  await rail()
  const stop = await railStop()
  expect(stop.disabled).toBe(true)
  expect((screen.getByLabelText(t('sat.rail.vfoMap.aria')) as HTMLSelectElement).disabled).toBe(true)
  expect(button(t('sat.rail.doppler.turnOn')).disabled).toBe(true)
  expect(button(t('sat.binding.unpegged')).disabled).toBe(true)
  fireEvent.click(stop)
  fireEvent.click(button(t('sat.rail.doppler.turnOn')))
  await new Promise(resolve => setTimeout(resolve, 10))
  expect(actions()).toEqual([])
  expect(api.stopSatTrack).not.toHaveBeenCalled()
})

it('the rail Stop stays live while this browser has a transmission armed', async () => {
  // ⛔ SAFETY. An FM bird is worked by transmitting DURING the pass these controls armed, so the
  // one control that ends that pass must not be the one that goes dead when a transmission is up.
  const { actions } = browser(['satellite'], true)
  await rail()
  const stop = await railStop()
  expect(stop.disabled).toBe(false)
  fireEvent.click(stop)
  await waitFor(() => expect(actions()).toEqual([{ action: 'satellite.stopTrack' }]))
})
