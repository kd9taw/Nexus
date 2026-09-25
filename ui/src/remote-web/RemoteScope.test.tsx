// @vitest-environment jsdom
// Remote parity batch 1: the rig-scope controls from a browser. They are live while the station
// advertises `rigScope` and the cockpit shows that scope family; they send only on an operator
// gesture, never on mount or when the feed arrives, and a refused change says why.
import { afterEach, beforeAll, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { PhoneCockpit } from '../components/PhoneCockpit'
import { CwCockpit } from '../components/CwCockpit'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { RemoteObservationContext } from './amplifier-observation'
import { OperationClient, OperationFailure } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { t } from '../i18n'
import type { AppSnapshot } from '../types'
import type { ControlCapability } from './station-operation'
import type { MonitorFrame } from '../remote-monitor/protocol'
import type { MonitorState } from '../remote-monitor/session'
import settings from '../components/__fixtures__/defaultSettings.json'
import frames from '../remote-monitor/fixtures.v2.json'

// The scope stub reports the feed source the test names, once, as the real scope does on its first row.
const scope = vi.hoisted(() => ({ feed: null as string | null }))
vi.mock('../api', async original => {
  const actual = await original<Record<string, unknown>>()
  const reads: Record<string, unknown> = { getLicensedBandPlan: [], getBandPlan: [], getCatCwUnprovenRigModels: [],
    getSpectrumRow: { row: [], loHz: 200, hiHz: 4000 },
    // The CW cockpit reads its decode on mount; an empty decode, not an empty object.
    cwDecode: { text: '', wpm: 0, sent: [], keyerError: null, candidates: [], rst: null, name: null, state: 'listening', headline: '', prompt: '', recommended: null, workedCall: null } }
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [name,
    typeof value === 'function' ? vi.fn(async () => structuredClone(reads[name] ?? {})) : value]))
})
vi.mock('../components/PhoneScope', async () => {
  const { useEffect } = await import('react')
  return {
    PhoneScope: ({ onFeed }: { onFeed?: (source: string, loHz: number, hiHz: number) => void }) => {
      useEffect(() => { if (scope.feed) onFeed?.(scope.feed, 14_150_000, 14_250_000) }, [])
      return <div/>
    }
  }
})
vi.mock('../components/BandStrip', () => ({ BandStrip: () => <div/> }))
vi.mock('../components/VoiceKeyer', () => ({ VoiceKeyer: () => <div/> }))
vi.mock('../components/LogEntry', () => ({ LogEntry: () => <div/> }))
vi.mock('../components/SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./useJs8Context', () => ({ useJs8Context: () => ({ remote: true, value: null, loading: false, refresh: () => {} }) }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (run: () => Promise<unknown>) => run()) }))
import { getSettings, setFlexPanSpan, setScopeRef, setScopeSpan, setYaesuScopeMode } from '../api'
import { pushToast } from '../toast'

const clients: OperationClient[] = []
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
beforeEach(() => {
  scope.feed = null
  localStorage.clear()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  vi.mocked(getSettings).mockResolvedValue(structuredClone(settings) as never)
})
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.clearAllMocks(); vi.restoreAllMocks() })
const flush = () => act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })

function station(capabilities: ControlCapability[]) {
  const sent: string[] = [], values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(wire), true, () => 1000 + performance.now(), undefined, 3,
    pendingControlStorage(() => storage, 'scope-gestures', async (_key, run) => run()))
  clients.push(client)
  client.open()
  client.receive({ type: 'operationResponse', requestId: JSON.parse(sent[sent.length - 1]).request.requestId, value: {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities } } })
  return client
}

function snap(mode: 'phone' | 'cw', scopeModeCode: number | null = null) {
  return { activeRadioId: 1, mycall: 'N0CALL', mygrid: 'AA00', mode: 'qso', stations: [], recentDecodes: [],
    conversations: [], highlights: [], link: { tier: 'FT8', dtSec: 0 }, radio: { source: 'native', operatingMode: mode,
      dialMhz: 14.2, band: '20m', sideband: 'USB', rigMode: mode === 'cw' ? 'CW' : 'USB', catOk: true, txEnabled: false, transmitting: false,
      rigKeyed: false, tuning: false, txAllowed: true, filterWidthHz: 2400, rfPower: 0.5, micGain: 0.5, compLevel: 0.5, notchFreqHz: 600,
      cwWpm: 22, cwKeyer: 'cat', nrLevel: 0.3, agc: 'fast', refusedAgc: null, nb: false, nr: false, notch: false, manualNotch: false,
      comp: false, vox: false, splitTxMhz: null, smeterDb: null, scopeModeCode } } as unknown as AppSnapshot
}

function mount(cockpit: 'phone' | 'cw', capabilities: ControlCapability[], scopeModeCode: number | null = null) {
  const frame = structuredClone(frames.spe) as MonitorFrame
  frame.station.radio.id = 1; frame.station.radio.readings.cat!.connectionGeneration = 7; frame.station.amplifier = null
  const observation = { status: 'current', frame } as MonitorState
  const Cockpit = cockpit === 'phone' ? PhoneCockpit : CwCockpit
  return render(<StationControlContext.Provider value={false}><StationDataContext.Provider value>
    <RemoteOperationsContext.Provider value={station(capabilities)}><RemoteObservationContext.Provider value={observation}>
      <Cockpit snap={snap(cockpit, scopeModeCode)} theme="dark" spots={[]} onWorkSpot={() => {}} onSnap={() => {}}/>
    </RemoteObservationContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
}

it('the Icom span chips and reference are live with rigScope on a CI-V feed, and nothing is sent on mount', async () => {
  scope.feed = 'civ'
  mount('phone', ['rigScope'])
  await flush()
  const chip = screen.getByTitle(t('phone.rigScope.span.title', { span: '±25k' })) as HTMLButtonElement
  const reference = screen.getByLabelText(t('phone.rigScope.ref.aria')) as HTMLInputElement
  expect(chip.disabled).toBe(false)
  expect(reference.disabled).toBe(false)
  expect(reference.style.visibility).not.toBe('hidden')
  // Nothing moves the radio because a page opened or a feed arrived.
  expect(setScopeSpan).not.toHaveBeenCalled()
  expect(setScopeRef).not.toHaveBeenCalled()
  // Positive control: one click sends exactly one change.
  fireEvent.click(chip)
  await flush()
  expect(setScopeSpan).toHaveBeenCalledTimes(1)
  expect(setScopeSpan).toHaveBeenCalledWith(25_000)
  fireEvent.change(reference, { target: { value: '-35' } })
  await flush()
  expect(setScopeRef).toHaveBeenCalledTimes(1)
  expect(setScopeRef).toHaveBeenCalledWith(-35)
})

it('a remote reference drag keeps one change in flight and then sends only the newest value', async () => {
  scope.feed = 'civ'
  let release: () => void = () => {}
  vi.mocked(setScopeRef).mockImplementationOnce(() => new Promise(resolve => { release = () => resolve({} as never) }))
  mount('phone', ['rigScope'])
  await flush()
  const reference = screen.getByLabelText(t('phone.rigScope.ref.aria')) as HTMLInputElement
  for (const value of ['-35', '-40', '-45']) fireEvent.change(reference, { target: { value } })
  await flush()
  expect(setScopeRef).toHaveBeenCalledTimes(1)
  expect(setScopeRef).toHaveBeenLastCalledWith(-35)
  await act(async () => { release() })
  await flush()
  expect(setScopeRef).toHaveBeenCalledTimes(2)
  expect(setScopeRef).toHaveBeenLastCalledWith(-45)
})

it('without the hint the Icom controls are off and send nothing', async () => {
  scope.feed = 'civ'
  mount('phone', ['frequency'])
  await flush()
  const chip = screen.getByTitle(t('phone.rigScope.span.title', { span: '±25k' })) as HTMLButtonElement
  expect(chip.disabled).toBe(true)
  fireEvent.click(chip)
  expect((screen.getByLabelText(t('phone.rigScope.ref.aria')) as HTMLInputElement).disabled).toBe(true)
  await flush()
  expect(setScopeSpan).not.toHaveBeenCalled()
})

it('the Flex pan span in CW is live with rigScope on a Flex feed and off without it', async () => {
  scope.feed = 'flex'
  mount('cw', ['rigScope'])
  await flush()
  const chip = screen.getByTitle(t('cw.flexPan.span.title', { span: '200k' })) as HTMLButtonElement
  expect(chip.disabled).toBe(false)
  expect(setFlexPanSpan).not.toHaveBeenCalled()
  fireEvent.click(chip)
  await flush()
  expect(setFlexPanSpan).toHaveBeenCalledTimes(1)
  expect(setFlexPanSpan).toHaveBeenCalledWith(200_000)
  cleanup()
  vi.mocked(setFlexPanSpan).mockClear()
  mount('cw', ['frequency'])
  await flush()
  const off = screen.getByTitle(t('cw.flexPan.span.title', { span: '200k' })) as HTMLButtonElement
  expect(off.disabled).toBe(true)
  fireEvent.click(off)
  await flush()
  expect(setFlexPanSpan).not.toHaveBeenCalled()
})

it('the FT-710 position is live with rigScope and a mode code, and a refused change says why', async () => {
  mount('phone', ['rigScope'], 0x34)
  await flush()
  const position = screen.getByLabelText(t('phone.scope.yaesu.pos.aria')) as HTMLSelectElement
  expect(position.disabled).toBe(false)
  expect(setYaesuScopeMode).not.toHaveBeenCalled()
  vi.mocked(setYaesuScopeMode).mockRejectedValueOnce(new OperationFailure('stationBusy', true, true))
  fireEvent.change(position, { target: { value: 'fix' } })
  await flush()
  expect(setYaesuScopeMode).toHaveBeenCalledTimes(1)
  expect(setYaesuScopeMode).toHaveBeenCalledWith('fix')
  expect(pushToast).toHaveBeenCalledWith(t('remote.controlBusy'), 'error')
  cleanup()
  vi.mocked(setYaesuScopeMode).mockClear()
  mount('phone', ['frequency'], 0x34)
  await flush()
  const off = screen.getByLabelText(t('phone.scope.yaesu.pos.aria')) as HTMLSelectElement
  expect(off.disabled).toBe(true)
  expect((screen.getByLabelText(t('phone.scope.yaesu.span.aria')) as HTMLSelectElement).disabled).toBe(true)
})
