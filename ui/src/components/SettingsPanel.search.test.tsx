// @vitest-environment jsdom
//
// Find-a-setting search — 0.17's approved item B6, three batches late.
//
// The audit behind that plan called 230 controls without search "disqualifying"; it is 280-odd
// now, and what shipped in its place was two reorganisations, each of which decayed. Rearranging
// asks the operator to guess the same shape the author did; search lets them use their own words.
//
// What is pinned here is the part that makes it worth having: it searches the registry's
// KEYWORDS, so the words that only ever appear in hint text still find their section. Nothing in
// "Rig & CAT" or "Audio" contains "COM port" or "sound card", and issue #62 was an operator who
// could not find a toggle that had already shipped.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, waitFor, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  const VERBS = [
    'clearCloudlogKey', 'clearClublogPassword', 'clearEqslPassword', 'clearHamqthPassword',
    'clearHrdlogCode', 'clearLotwPassword', 'clearQrzLogbookKey', 'clearQrzPassword', 'detectRigs',
    'downloadEqslReport', 'downloadLotwReport', 'getAllRigModels', 'getAudioDevices', 'getBandPlan',
    'getRigModels', 'getSerialPortsDetailed', 'getSettings', 'setCloudlogKey', 'setClublogPassword',
    'setEqslPassword', 'setHamqthPassword', 'setHrdlogCode', 'setLotwPassword', 'setQrzLogbookKey',
    'setQrzPassword', 'setRepeaterbookToken', 'setRxGain', 'setSettings', 'setTxLevel', 'addRadio',
    'removeRadio', 'renameRadio', 'setActiveRadio', 'setRadioBands', 'updateRadioProfile', 'testCat',
    'probeCatPorts', 'qrzTestConnection', 'syncQrz', 'n3fjpTestConnection', 'getConnectionLog',
    'getCredentialsStatus', 'fetchLotwUsers', 'getLotwUsersStatus', 'fetchFccStates',
    'getFccStatesStatus', 'getTleStatus', 'fetchTlesNow', 'importTles', 'discoverFlex',
    'civDiagnosticLog', 'civDiagnosticStatus', 'allTxtLocation', 'revealAllTxt',
    'recordingsLocation', 'revealRecordings', 'appVersion', 'getSpectrumRow', 'setFrequency',
    'getWatchlist', 'setWatchlist', 'openPanelWindow', 'getAssistanceJournal', 'setUnassistedMode',
  ]
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get, VERBS }
})

vi.mock('../api', () => {
  const mod: Record<string, unknown> = {}
  for (const v of api.VERBS) mod[v] = api.get(v)
  return mod
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function renderPanel(target?: string, onSaved?: () => void) {
  return render(
    <SettingsPanel
      target={target}
      onSaved={onSaved}
      activeRadioId={0}
      scale={1 as never}
      scaleMode={'auto' as never}
      scaleCap={1 as never}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density={'comfortable' as never}
      onDensityChange={() => {}}
      onResetLayout={() => {}}
      features={features}
    />,
  )
}

/** Which tab is selected, by the rail's own aria state. */
const selectedTab = () =>
  screen.getAllByRole('tab').find((t) => t.getAttribute('aria-selected') === 'true')?.textContent

beforeEach(() => {
  // The list-returning verbs must hand back real shapes: the panel maps over them on first
  // render, so a bare `null` from the default spy crashes FrequencyControl before a single tab
  // exists to assert on.
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.2.6'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
  // jsdom has no layout engine, so scrollIntoView is not implemented. Spy on the prototype so
  // the assertion is "the panel ASKED to scroll the right element", which is the behaviour that
  // matters — the browser owns the rest.
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})


describe('the operator can find a setting by the words they would use', () => {
  it('finds Rig & CAT by "com port" — a phrase absent from every heading', async () => {
    renderPanel()
    const box = await screen.findByRole('combobox', { name: /find a setting/i })
    fireEvent.change(box, { target: { value: 'com port' } })
    const hit = await screen.findByRole('option', { name: /Rig & CAT/ })
    expect(hit.textContent).toMatch(/matched/i) // it explains WHY it is a result
  })

  it('takes you there when you pick a result, opening the right tab', async () => {
    renderPanel()
    const box = await screen.findByRole('combobox', { name: /find a setting/i })
    fireEvent.change(box, { target: { value: 'sound card' } })
    const hit = await screen.findByRole('button', { name: /Audio/ })
    fireEvent.pointerDown(hit)
    await waitFor(() => expect(selectedTab()).toBe('Radio'))
  })

  it('EXPANDS a collapsed disclosure it lands in — the issue #62 case, end to end', async () => {
    // "plain ssb" is only reachable through the collapsed Advanced group. An operator searched
    // for exactly this, could not find it, and filed a feature request for a shipped feature.
    renderPanel()
    const box = await screen.findByRole('combobox', { name: /find a setting/i })
    fireEvent.change(box, { target: { value: 'plain ssb' } })
    const hit = await screen.findByRole('button', { name: /Advanced/ })
    fireEvent.pointerDown(hit)
    await waitFor(() => expect(selectedTab()).toBe('Radio'))
    await waitFor(() => {
      const toggle = screen.getAllByRole('button').find((b) => b.textContent?.trim() === 'Advanced')
      expect(toggle?.getAttribute('aria-expanded')).toBe('true')
    })
  })

  it('says so when nothing matches, instead of showing an empty box', async () => {
    renderPanel()
    const box = await screen.findByRole('combobox', { name: /find a setting/i })
    fireEvent.change(box, { target: { value: 'zzzznotasetting' } })
    expect(await screen.findByText(/nothing matches/i)).toBeTruthy()
  })

  it('shows nothing at all until something is typed', async () => {
    renderPanel()
    await screen.findByRole('combobox', { name: /find a setting/i })
    // The control for the test above: an always-open list would satisfy it just as well.
    expect(screen.queryByRole('listbox')).toBeNull()
  })

  it('Enter picks the first result and never submits the settings form', async () => {
    const onSaved = vi.fn()
    renderPanel(undefined, onSaved)
    const box = await screen.findByRole('combobox', { name: /find a setting/i })
    fireEvent.change(box, { target: { value: 'wpm' } })
    await screen.findByRole('option', { name: /CW/ })
    fireEvent.keyDown(box, { key: 'Enter' })
    await waitFor(() => expect(selectedTab()).toBe('CW'))
    expect(api.spies.setSettings).not.toHaveBeenCalled()
  })
})
