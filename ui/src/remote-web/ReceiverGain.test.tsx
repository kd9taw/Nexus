// @vitest-environment jsdom
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { SettingsPanel } from '../components/SettingsPanel'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { RemoteObservationContext } from './amplifier-observation'
import { RemoteCollectionsContext, type RemoteCollections } from './collections'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { navigationPages } from './__fixtures__/navigation-page'
import configuration from './__fixtures__/configuration-settings.json'
import frames from '../remote-monitor/fixtures.v2.json'
import type { SettingsConfiguration } from './configuration'
import type { MonitorFrame } from '../remote-monitor/protocol'
import type { OperationState } from './operation-protocol'
import type { RadioStatus } from '../types'
import type { FeaturesApi } from '../useFeatures'
import type { ControlCapability } from './station-operation'

vi.mock('../api', async original => {
  const actual = await original<Record<string, unknown>>()
  const reads: Record<string, unknown> = { getRigModels: [], getAllRigModels: [], getSerialPortsDetailed: [],
    getBandPlan: [], getAudioDevices: { input: [], output: [] }, getCredentialsStatus: {}, detectRigs: [], appVersion: 'test' }
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [name,
    typeof value === 'function' ? vi.fn(async () => structuredClone(reads[name] ?? null)) : value]))
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (run: () => Promise<unknown>) => run()) }))
import { setSettings, setRxGain, setTxLevel, updateRadioProfile, getSettings } from '../api'
import { pushToast } from '../toast'

const clients: OperationClient[] = []
const features = { enabled: () => true, all: () => [], profile: 'full', setEnabled: () => {}, setProfile: () => {} } as unknown as FeaturesApi
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks() })
async function tick(ms = 0) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

function fixture(capabilities: ControlCapability[] = ['receiverGain'], version: 2 | 3 = 3, remote = true) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  let doc = structuredClone(configuration) as SettingsConfiguration
  doc.revision = 'a'.repeat(64)
  Object.assign(doc.settings, { activeRadio: 0, rxGain: 1, txLevel: 0.5 })
  vi.mocked(getSettings).mockResolvedValue(doc.settings as never)
  const frame = structuredClone(frames.spe) as MonitorFrame
  Object.assign(frame.station.radio, { id: 0, catConnected: true, rigKeyed: false, nexusBusy: false })
  frame.station.radio.readings.cat = { connectionGeneration: 7, readSequence: 10, ageMs: 0 }
  frame.station.radio.readings.ptt = { connectionGeneration: 7, readSequence: 10, ageMs: 0 }
  frame.station.amplifier = null
  const radio = { source: 'native', catOk: true, txEnabled: false, transmitting: false, rigKeyed: false, tuning: false,
    operatingMode: 'digital', dialMhz: 14.074, band: '20m', sideband: 'USB', txLevel: 0.5 } as RadioStatus
  const sent: any[] = [], values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000, undefined, version,
    pendingControlStorage(() => storage, 'receiver-gain-test', async (_key, run) => run()))
  clients.push(client)
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 0, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities } }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  client.open(); reply(state)
  const page = vi.fn(async () => {
    const pages = navigationPages('settings', doc)
    return { ...pages[0], rows: pages.flatMap(p => p.rows), nextCursor: null }
  })
  const source = { page } as unknown as RemoteCollections
  const view = (current = radio, observation = frame, available = true, displayedRadio = 0) =>
    <StationControlContext.Provider value={!remote}><StationDataContext.Provider value={available}>
      <RemoteOperationsContext.Provider value={remote ? client : null}><RemoteObservationContext.Provider value={{ status: 'current', frame: observation }}>
        <RemoteCollectionsContext.Provider value={remote ? source : null}>
          <SettingsPanel activeRadioId={displayedRadio} radio={current} target="audio"
            scale={1 as never} scaleMode={'auto' as never} scaleCap={1 as never} onScaleModeChange={() => {}} onScaleCapChange={() => {}}
            density={'comfortable' as never} onDensityChange={() => {}} onResetLayout={() => {}} features={features} />
        </RemoteCollectionsContext.Provider>
      </RemoteObservationContext.Provider></RemoteOperationsContext.Provider>
    </StationDataContext.Provider></StationControlContext.Provider>
  return { ...render(view()), view, radio, frame, sent, client, state, reply, page,
    get doc() { return doc }, setDoc(next: SettingsConfiguration) { doc = next },
    writes: () => sent.filter(w => w.request.type === 'stationControl') }
}
const slider = () => screen.getByRole<HTMLInputElement>('slider', { name: /RX capture gain/i })
function drag(value: string) { fireEvent.pointerDown(slider()); fireEvent.change(slider(), { target: { value } }) }

it.each(['pointer', 'keyboard'])('the native gain slider commits once on %s release and waits for the saved station value', async input => {
  const h = fixture(); await tick()
  expect(slider().disabled).toBe(false)
  if (input === 'pointer') fireEvent.pointerDown(slider())
  else fireEvent.keyDown(slider(), { key: 'ArrowRight' })
  for (const value of ['1.5', '2', '2.5']) fireEvent.change(slider(), { target: { value } })
  expect(slider().value).toBe('2.5'); expect(h.writes()).toHaveLength(0)
  if (input === 'pointer') fireEvent.pointerUp(slider())
  else fireEvent.keyUp(slider(), { key: 'ArrowRight' })
  fireEvent.blur(slider()); fireEvent.pointerUp(slider()); await tick()
  expect(h.writes()).toHaveLength(1)
  const command = h.writes()[0].request
  expect(command.action).toEqual({ action: 'receiver.rxGain', radioId: 0, expectedSettingsRevision: 'a'.repeat(64), expectedGain: 1, gain: 2.5 })
  expect(command.context).toEqual({ radioId: 0, radioConnection: 7, ampConnection: null, ampReadSequence: null })
  expect(slider().value).toBe('1'); expect(slider().disabled).toBe(true)
  act(() => h.reply({ operation: 'stationControl', operationId: command.requestId, outcome: 'applied', evidence: 'settingsSaved' }))
  await tick()
  expect(slider().value).toBe('1')
  const fresh = structuredClone(h.doc); fresh.revision = 'b'.repeat(64); fresh.settings.rxGain = 2.5
  h.setDoc(fresh); await tick(25_000)
  expect(slider().value).toBe('2.5')
  for (const native of [setRxGain, setTxLevel, setSettings, updateRadioProfile]) expect(native).not.toHaveBeenCalled()
})

it('cancels a whole drag after a local settings edit instead of rebasing later moves', async () => {
  const h = fixture(); await tick(); drag('2')
  const fresh = structuredClone(h.doc); fresh.revision = 'b'.repeat(64); fresh.settings.rxGain = 1.5
  h.setDoc(fresh); await tick(25_000)
  expect(slider().value).toBe('1.5')
  fireEvent.change(slider(), { target: { value: '3' } }); fireEvent.pointerUp(slider()); await tick()
  expect(h.writes()).toHaveLength(0)
  drag('2.5'); fireEvent.pointerUp(slider()); await tick()
  expect(h.writes()[0].request.action).toMatchObject({ expectedSettingsRevision: 'b'.repeat(64), expectedGain: 1.5, gain: 2.5 })
})

it('losing and regaining observation during a drag requires a new gesture', async () => {
  const h = fixture(); await tick(); drag('2')
  h.rerender(h.view(h.radio, h.frame, false)); await tick()
  expect(screen.queryByRole('slider', { name: /RX capture gain/i })).toBeNull()
  h.rerender(h.view()); await tick()
  fireEvent.change(slider(), { target: { value: '3' } }); fireEvent.pointerUp(slider()); await tick()
  expect(h.writes()).toHaveLength(0)
  drag('2.5'); fireEvent.pointerUp(slider()); await tick()
  expect(h.writes()).toHaveLength(1)
})

it('cancel, blur and unchanged releases never submit a gain request', async () => {
  const h = fixture(); await tick()
  for (const cancel of [() => fireEvent.pointerCancel(slider()), () => fireEvent.blur(slider())]) {
    drag('2'); cancel(); fireEvent.pointerUp(slider()); await tick()
  }
  drag('1'); fireEvent.pointerUp(slider()); await tick()
  expect(h.writes()).toHaveLength(0)
  expect(slider().value).toBe('1')
})

it('a refused save restores the station value and never falls through to a local write', async () => {
  const h = fixture(); await tick(); drag('2.5'); fireEvent.pointerUp(slider()); await tick()
  const request = h.writes()[0].request
  act(() => h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'rejected', reason: 'persistenceFailed' }))
  await tick()
  expect(slider().value).toBe('1')
  expect(h.doc.settings.rxGain).toBe(1)
  expect(pushToast).toHaveBeenCalled()
  expect(setRxGain).not.toHaveBeenCalled()
})

it('requires its own capability and version plus current native idle radio evidence', async () => {
  for (const [capabilities, version] of [[['receiverSettings'], 3], [['receiverGain'], 2]] as [ControlCapability[], 2 | 3][]) {
    const h = fixture(capabilities, version); await tick(); expect(slider().disabled).toBe(true)
    drag('2'); fireEvent.pointerUp(slider()); await tick(); expect(h.writes()).toHaveLength(0); h.unmount()
  }
  const h = fixture(); await tick()
  for (const patch of [{ source: 'companion' }, { txEnabled: true }, { transmitting: true }, { rigKeyed: true }, { tuning: true }, { catOk: false }]) {
    h.rerender(h.view({ ...h.radio, ...patch } as RadioStatus)); await tick(); expect(slider().disabled).toBe(true)
  }
  const old = structuredClone(h.frame); old.station.radio.readings.ptt!.ageMs = 2000
  h.rerender(h.view(h.radio, old)); await tick(); expect(slider().disabled).toBe(true)
  h.rerender(h.view(h.radio, h.frame, true, 99)); await tick(); expect(slider().disabled).toBe(true)
  h.rerender(h.view()); await tick(); expect(slider().disabled).toBe(false)
  drag('2'); fireEvent.pointerUp(slider()); await tick(); expect(h.writes()).toHaveLength(1)
})

it('the native slider retains its existing release-only local API path', async () => {
  const h = fixture([], 3, false); await tick()
  expect(slider().disabled).toBe(false)
  drag('2.5'); expect(setRxGain).not.toHaveBeenCalled()
  fireEvent.pointerUp(slider()); await tick()
  expect(setRxGain).toHaveBeenCalledExactlyOnceWith(2.5)
  expect(h.writes()).toHaveLength(0)
})
