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
import type { FeaturesApi } from '../useFeatures'
import { t } from '../i18n'

vi.mock('../api', async (original) => {
  const actual = await original<Record<string, unknown>>()
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [name, typeof value === 'function' ? vi.fn(async () => null) : value]))
})
import { setSettings, updateRadioProfile } from '../api'

// THE BUDGET (2026-10-09). The slowest case here, "enables only the preferences the station lets this…", takes 0.85 s
// and 0.82 s on one core (two runs), nearly all of it CPU work (2.83 s at a third of a CPU); a loaded full suite on
// this box has run cases up to 20 times slower than one core, 16.4 s for this one. 20 s holds that; a test that hangs
// still fails, after 20 s.
vi.setConfig({ testTimeout: 20_000 })

const clients: OperationClient[] = []
const features = { enabled: () => true, all: () => [], profile: 'full', setEnabled: () => {}, setProfile: () => {} } as unknown as FeaturesApi
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks() })
async function tick(ms = 0) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

function fixture(capabilities: string[], settings: Record<string, unknown> = {}) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  let doc = structuredClone(configuration) as SettingsConfiguration
  doc.revision = 'a'.repeat(64)
  Object.assign(doc.settings, { autoLog: false, preferRrr: false, contestCheck: '' }, settings)
  const frame = structuredClone(frames.spe) as MonitorFrame
  const sent: any[] = [], values = new Map<string, string>()
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000, undefined, 4,
    pendingControlStorage(() => data, 'settings-test', async (_key, run) => run()))
  clients.push(client)
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: ['log.manual'], txArmed: false,
    transmitEpoch: null, controls: { context: { radioId: 0, radioConnection: 4, ampConnection: null, ampReadSequence: null },
      capabilities: capabilities as NonNullable<OperationState['controls']>['capabilities'] } }
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value: state })
  const page = vi.fn(async () => {
    const pages = navigationPages('settings', doc)
    return { ...pages[0], rows: pages.flatMap(p => p.rows), nextCursor: null }
  })
  render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={client}><RemoteObservationContext.Provider value={{ status: 'current', frame }}>
      <RemoteCollectionsContext.Provider value={{ page } as unknown as RemoteCollections}><SettingsPanel activeRadioId={0}
        scale={1 as never} scaleMode={'auto' as never} scaleCap={1 as never} onScaleModeChange={() => {}} onScaleCapChange={() => {}}
        density={'comfortable' as never} onDensityChange={() => {}} onResetLayout={() => {}} features={features} />
      </RemoteCollectionsContext.Provider>
    </RemoteObservationContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  return { client, sent, get doc() { return doc }, setDoc(next: SettingsConfiguration) { doc = next },
    changes: () => sent.filter(w => w.request.type === 'logChange').map(w => w.request),
    reply: (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value }) }
}
const tab = (name: string) => fireEvent.click(screen.getByRole('tab', { name }))
const autoLog = () => screen.getByRole('switch', { name: t('settings.digital.autoLog.label') }) as HTMLButtonElement
const preferRrr = () => screen.getByRole('switch', { name: t('settings.digital.preferRrr.label') }) as HTMLButtonElement
// The field's label also wraps its hint, so the accessible name starts with the label text.
const contestCheck = () => screen.getByRole('textbox', { name: name => name.startsWith(t('settings.contestStation.check.label')) }) as HTMLInputElement
const saveButton = () => screen.getAllByRole<HTMLButtonElement>('button', { name: /^Save$/ }).find(b => b.type === 'submit')!

it('enables only the preferences the station lets this browser change, and saves just the one that changed', async () => {
  const h = fixture(['settingsLogging', 'settingsControl'])
  await tick()
  expect(screen.getByText(t('remote.settingsEditable')).textContent).toMatch(/stay read-only here\. Change them in Nexus at the station, in person or streamed to this browser/)
  tab('Digital')
  expect(autoLog().disabled).toBe(false)
  // Positive control on the lock: a sequencing preference beside it stays read-only.
  expect(preferRrr().disabled).toBe(true)
  expect(saveButton().disabled).toBe(true)
  fireEvent.click(autoLog())
  expect(autoLog().getAttribute('aria-checked')).toBe('true')
  // An ordinary refresh of the Settings document keeps the unsaved edit.
  await tick(25_000)
  expect(autoLog().getAttribute('aria-checked')).toBe('true')
  expect(saveButton().disabled).toBe(false)
  fireEvent.click(saveButton()); await tick()
  expect(h.changes()).toHaveLength(1)
  const [request] = h.changes()
  expect(request.change).toEqual({ kind: 'settings', revision: 'a'.repeat(64), values: { autoLog: true } })
  const fresh = structuredClone(h.doc); fresh.revision = 'b'.repeat(64); fresh.settings.autoLog = true
  h.setDoc(fresh)
  await act(async () => h.reply({ operation: 'logChange', operationId: request.requestId, outcome: 'applied', evidence: 'settingsSaved' }))
  await tick()
  expect(screen.getByText(t('settings.panel.saved'))).toBeTruthy()
  expect(autoLog().getAttribute('aria-checked')).toBe('true')
  expect(setSettings).not.toHaveBeenCalled()
  expect(updateRadioProfile).not.toHaveBeenCalled()
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
})

it('a station preference needs station control, and an edit the station overtook is dropped and said', async () => {
  const logging = fixture(['settingsLogging'])
  await tick()
  tab('Digital')
  expect(autoLog().disabled).toBe(false)
  tab('Contesting')
  expect(contestCheck().disabled).toBe(true)
  cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers()
  const h = fixture(['settingsControl'])
  await tick()
  tab('Contesting')
  expect(contestCheck().disabled).toBe(false)
  fireEvent.change(contestCheck(), { target: { value: '73' } })
  const moved = structuredClone(h.doc); moved.revision = 'c'.repeat(64); moved.settings.contestCheck = '99'
  h.setDoc(moved)
  await tick(25_000)
  expect(contestCheck().value).toBe('99')
  expect(screen.getByRole('alert').textContent).toBe(t('remote.settingsChanged'))
  fireEvent.change(contestCheck(), { target: { value: '74' } })
  fireEvent.click(saveButton()); await tick()
  expect(h.changes().map(r => r.change)).toEqual([{ kind: 'settings', revision: 'c'.repeat(64), values: { contestCheck: '74' } }])
  expect(logging.changes()).toHaveLength(0)
})

it('against a desktop that offers no preference writes, every field stays read-only and nothing is sent', async () => {
  const h = fixture(['amplifier', 'qsoLogging', 'logEdit'])
  await tick()
  expect(screen.queryByText(t('remote.settingsEditable'))).toBeNull()
  tab('Digital')
  expect(autoLog().disabled).toBe(true)
  tab('Contesting')
  expect(contestCheck().disabled).toBe(true)
  expect(saveButton().disabled).toBe(true)
  expect(h.changes()).toHaveLength(0)
})

// JS8 groups change only at the station: it denies `js8Groups` to every Remote write (its
// WRITE_DENIED_KEYS, since writes began in 1.13.0) and this page cannot build one. So a group
// JS8Call will not let be joined (@APRSIS, @JS8NET) cannot reach a station from here. The field
// is read-only while it holds one, and a preference saved beside it goes out alone and is not
// refused: the desktop panel's refusal asks for an edit the Remote cannot make.
it('never writes JS8 groups: the field stays read-only holding @APRSIS, and a save beside it sends only its own key', async () => {
  const h = fixture(['settingsLogging', 'settingsControl'], { js8Groups: ['@APRSIS'] })
  await tick()
  tab('Digital')
  expect((screen.getByDisplayValue('@APRSIS') as HTMLInputElement).disabled, 'the Groups field is read-only here').toBe(true)
  fireEvent.click(autoLog())
  fireEvent.click(saveButton()); await tick()
  expect(h.changes().map(r => r.change.values), 'the save beside it').toEqual([{ autoLog: true }])
  expect(screen.queryByText(/cannot be joined/), "the desktop panel's refusal is not the Remote's").toBeNull()
})

// Following the radio's split is a transmit-gate input the station denies to every Remote write
// (`splitDetectEnabled` is in its WRITE_DENIED_KEYS), so the switch stays read-only here even with
// station control, while a preference the station does offer is editable in the same page.
it("keeps Follow the radio's split read-only from a browser, even with station control", async () => {
  fixture(['settingsLogging', 'settingsControl'])
  await tick()
  tab('Digital')
  expect(autoLog().disabled, 'control: this page may change an offered preference').toBe(false)
  tab('Radio')
  const follow = screen.getByRole('switch', { name: t('settings.rigControl.splitDetect.label') }) as HTMLButtonElement
  expect(follow.disabled).toBe(true)
})
