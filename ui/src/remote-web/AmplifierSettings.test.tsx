// @vitest-environment jsdom
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { SettingsPanel } from '../components/SettingsPanel'
import { AmpStrip } from '../components/AmpStrip'
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
import type { AmpStatus } from '../types'
import type { FeaturesApi } from '../useFeatures'

vi.mock('../api', async (original) => {
  const actual = await original<Record<string, unknown>>()
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [name, typeof value === 'function' ? vi.fn(async () => null) : value]))
})
import { setSettings, ampCommand, updateRadioProfile } from '../api'

const clients: OperationClient[] = []
const features = { enabled: () => true, all: () => [], profile: 'full', setEnabled: () => {}, setProfile: () => {} } as unknown as FeaturesApi
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks() })
async function tick(ms = 0) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

function fixture(follow = false, capability = true, panel = true) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  let doc = structuredClone(configuration) as SettingsConfiguration
  doc.revision = 'a'.repeat(64)
  Object.assign(doc.settings, { ampModel: 'spe', ampPort: 'test-amp', ampFollowBand: follow })
  const frame = structuredClone(frames.spe) as MonitorFrame
  const radio = frame.station.radio, amp = frame.station.amplifier!
  radio.id = 0; radio.rigKeyed = false; radio.nexusBusy = false; radio.catConnected = true
  radio.readings.cat = { connectionGeneration: 4, readSequence: 10, ageMs: 0 }
  radio.readings.ptt = { connectionGeneration: 4, readSequence: 10, ageMs: 0 }
  Object.assign(amp, { followBand: follow, linked: true, operate: false, outputWatts: 0, transmitting: false, bandLabel: '20m', reading: { connectionGeneration: 5, readSequence: 12, ageMs: 0 } })
  const sent: any[] = [], values = new Map<string, string>()
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000, undefined, 3,
    pendingControlStorage(() => data, 'amp-test', async (_key, run) => run()))
  clients.push(client)
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 0, radioConnection: 4, ampConnection: 5, ampReadSequence: 11 }, capabilities: capability ? ['amplifier', 'ampFollowBand'] : ['amplifier'] } }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  client.open(); reply(state)
  const page = vi.fn(async () => {
    const pages = navigationPages('settings', doc)
    return { ...pages[0], rows: pages.flatMap(p => p.rows), nextCursor: null }
  })
  const source = { page } as unknown as RemoteCollections
  const view = (current = frame, live = true, displayedRadio = 0) =>
    <StationControlContext.Provider value={false}><StationDataContext.Provider value={live}>
      <RemoteOperationsContext.Provider value={client}><RemoteObservationContext.Provider value={{ status: 'current', frame: current }}>
        <RemoteCollectionsContext.Provider value={source}>{panel ? <SettingsPanel activeRadioId={displayedRadio} target="amplifier"
          scale={1 as never} scaleMode={'auto' as never} scaleCap={1 as never} onScaleModeChange={() => {}} onScaleCapChange={() => {}}
          density={'comfortable' as never} onDensityChange={() => {}} onResetLayout={() => {}} features={features} />
          : <AmpStrip amp={amp as AmpStatus} radioId={displayedRadio} />}</RemoteCollectionsContext.Provider>
      </RemoteObservationContext.Provider></RemoteOperationsContext.Provider>
    </StationDataContext.Provider></StationControlContext.Provider>
  const rendered = render(view())
  return { ...rendered, view, frame, sent, client, state, reply, page,
    get doc() { return doc }, setDoc(next: SettingsConfiguration) { doc = next },
    writes: () => sent.filter(w => w.request.type === 'stationControl') }
}
function checkbox() { return screen.getByRole('checkbox', { name: /Follow the radio/ }) as HTMLInputElement }
function saveButton() { return screen.getAllByRole<HTMLButtonElement>('button', { name: /^Save$/ }).find(b => b.type === 'submit')! }

it.each([false, true])('the actual Nexus checkbox waits for Save and confirms the saved choice (initial %s)', async prior => {
  const h = fixture(prior)
  await tick()
  expect(checkbox().checked).toBe(prior)
  expect(checkbox().disabled).toBe(false)
  fireEvent.click(checkbox())
  expect(checkbox().checked).toBe(!prior)
  expect(h.writes()).toHaveLength(0)
  await tick(25_000) // Normal Settings refresh must preserve the unsaved draft.
  expect(h.page.mock.calls.length).toBeGreaterThan(1)
  expect(checkbox().checked).toBe(!prior)
  expect(saveButton().disabled).toBe(false)
  fireEvent.click(saveButton()); await tick()
  expect(h.writes()).toHaveLength(1)
  const request = h.writes()[0].request
  expect(request.action).toEqual({ action: 'amplifier.followBand', radioId: 0, expectedSettingsRevision: 'a'.repeat(64), expectedFollow: prior, follow: !prior })
  const fresh = structuredClone(h.doc); fresh.revision = 'b'.repeat(64); fresh.settings.ampFollowBand = !prior
  h.setDoc(fresh)
  act(() => h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'settingsSaved' }))
  await tick()
  expect(checkbox().checked).toBe(!prior)
  expect(screen.getByText('Saved')).toBeTruthy()
  for (const nativeWrite of [setSettings, updateRadioProfile, ampCommand]) expect(nativeWrite).not.toHaveBeenCalled()
})

it('refuses an old draft after a station edit and preserves the actual native Save affordance', async () => {
  const h = fixture(false); await tick()
  fireEvent.click(checkbox())
  const fresh = structuredClone(h.doc); fresh.revision = 'c'.repeat(64)
  h.setDoc(fresh); await tick(25_000)
  expect(checkbox().checked).toBe(false)
  expect(saveButton().disabled).toBe(true)
  expect(screen.getByRole('alert').textContent).toContain('station settings changed')
  expect(h.writes()).toHaveLength(0)
  fireEvent.click(checkbox())
  expect(saveButton().disabled).toBe(false)
})

it('keeps the old choice after a disk refusal and blocks Save for missing capability or mismatched radio', async () => {
  const h = fixture(false); await tick()
  fireEvent.click(checkbox()); fireEvent.click(saveButton()); await tick()
  const request = h.writes()[0].request
  act(() => h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'rejected', reason: 'persistenceFailed' }))
  await tick()
  expect(h.doc.settings.ampFollowBand).toBe(false)
  expect(screen.getByRole('alert').textContent).toContain('saved choice is unchanged')
  expect(screen.queryByText('Saved')).toBeNull()
  h.unmount()
  const readonly = fixture(false, false); await tick()
  expect(checkbox().disabled).toBe(true); expect(saveButton().disabled).toBe(true)
  readonly.rerender(readonly.view(readonly.frame, true, 8)); await tick()
  expect(checkbox().disabled).toBe(true)
  expect(readonly.writes()).toHaveLength(0)
})

it('enables saving Off with missing readings while On waits for an idle disarmed exciter', async () => {
  const h = fixture(true); await tick()
  const lost = structuredClone(h.frame); lost.station.radio.rigKeyed = null; lost.station.radio.readings.ptt = null
  h.rerender(h.view(lost)); fireEvent.click(checkbox())
  expect(saveButton().disabled).toBe(false)
  h.unmount()
  const on = fixture(false); await tick()
  on.rerender(on.view(lost)); fireEvent.click(checkbox())
  expect(saveButton().disabled).toBe(true)
  expect(screen.getByText(/disarm transmit and wait/)).toBeTruthy()
  on.rerender(on.view()); expect(saveButton().disabled).toBe(false)
})

it('the actual amplifier strip binds its displayed measurement and disables manual band steps during follow', async () => {
  const h = fixture(true, true, false)
  expect((screen.getByRole('button', { name: /band up/i }) as HTMLButtonElement).disabled).toBe(true)
  const button = screen.getByRole('button', { name: /^Standby$/ }) as HTMLButtonElement
  expect(button.disabled).toBe(false)
  fireEvent.click(button); await tick()
  const request = h.writes()[0].request
  expect(request.context).toEqual({ radioId: 0, radioConnection: 4, ampConnection: 5, ampReadSequence: 12 })
  expect(request.action).toEqual({ action: 'amplifier.operate', expectedOperate: false, operate: true })
  act(() => h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'amplifierReadback' }))
  h.rerender(h.view(h.frame, true, 8))
  expect(screen.getByRole('button', { name: 'Amplifier status unavailable' })).toBeTruthy()
  expect(screen.queryByRole('button', { name: /^Standby$/ })).toBeNull()
  for (const control of screen.getAllByRole<HTMLButtonElement>('button')) expect(control.disabled).toBe(true)
  expect(ampCommand).not.toHaveBeenCalled()
})
