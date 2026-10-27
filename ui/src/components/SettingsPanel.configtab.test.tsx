// @vitest-environment jsdom
//
// The per-radio Edit flow (0.17.0) decoupled "which radio the rig form edits" from "which radio
// is active", but only Save was taught to route accordingly. Test CAT and Auto-test still sent
// the whole flat form, and the backend folds a flat payload into the profile named by its
// `activeRadio` — so configuring radio 2 and pressing either button stamped radio 2's COM port,
// model and audio devices onto radio 1's profile, persisted. Operator report, 2026-07-25: with
// two radios configured, both ended up on one set of comm ports.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n/en'
import { setRemoteStation } from '../features/durableStore'

// THE BUDGET (2026-10-09). The slowest case here, "takes them OUT of Radio — a move that leaves a copy…", takes
// 0.33 s and 0.34 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  // SettingsPanel pulls ~50 verbs from ../api. They all resolve null, which is enough for a
  // mount — the assertions here are about WHICH save verb gets called, not what it returns.
  const VERBS = [
    'clearCloudlogKey', 'clearClublogPassword', 'clearEqslPassword', 'clearHamqthPassword',
    'clearHrdlogCode', 'clearLotwPassword', 'clearQrzLogbookKey', 'clearQrzPassword', 'detectRigs',
    'downloadEqslReport', 'downloadLotwReport', 'getAllRigModels', 'getAudioDevices', 'resetSettings', 'audioDevicesForPort', 'getBandPlan',
    'getRigModels', 'getSerialPortsDetailed', 'getSettings', 'setCloudlogKey', 'setClublogPassword',
    'setEqslPassword', 'setHamqthPassword', 'setHrdlogCode', 'setLotwPassword', 'setQrzLogbookKey',
    'setQrzPassword', 'setRepeaterbookToken', 'setRxGain', 'setSettings', 'setTxLevel', 'addRadio',
    'removeRadio', 'renameRadio', 'setActiveRadio', 'setRadioBands', 'updateRadioProfile', 'testCat',
    'probeCatPorts', 'qrzTestConnection', 'syncQrz', 'n3fjpTestConnection', 'getConnectionLog',
    'getCredentialsStatus', 'fetchLotwUsers', 'getLotwUsersStatus', 'fetchFccStates',
    'getFccStatesStatus', 'getTleStatus', 'fetchTlesNow', 'importTles', 'discoverFlex', 'civDiagnosticLog', 'civDiagnosticStatus',
    'allTxtLocation', 'revealAllTxt', 'recordingsLocation', 'revealRecordings', 'appVersion', 'getSpectrumRow', 'setFrequency',
    'getWatchlist', 'setWatchlist', 'openPanelWindow', 'getAssistanceJournal',
    'setUnassistedMode',
  ]
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get, VERBS }
})

// Derive the mock from the REAL module rather than a hand-kept verb list (upstream #79): a list
// goes stale the moment the panel calls a verb nobody added to it, and the failure is a crash
// inside an effect, not a missing-mock message. `getPortlessRigModels` was exactly that.
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

const FTDX10 = {
  id: 0,
  name: 'FTDX10',
  enabled: true,
  serialPort: 'COM3',
  baud: 38400,
  rigModel: 1042,
  rigModelName: 'Yaesu FTDX10',
  rigConn: 'serial',
  rigAddr: '',
  rigctldPort: 4532,
  rotctldPort: 4533,
  icomNativeCat: false,
  audioIn: 'in-0',
  audioOut: 'out-0',
  txLevel: 1,
  rxGain: 1,
  pttMethod: 'cat',
  rotatorModel: 0,
  rotatorPort: '',
  rotatorBaud: 9600,
  rotatorHost: '',
  nativeScope: 'auto',
  bands: [],
}
const IC9700 = {
  ...FTDX10,
  id: 1,
  name: 'IC-9700',
  serialPort: 'COM7',
  baud: 115200,
  rigModel: 3081,
  rigModelName: 'Icom IC-9700',
  rigctldPort: 4534,
  rotctldPort: 4535,
  icomNativeCat: true,
  audioIn: 'in-1',
  audioOut: 'out-1',
}

/** Settings with two radios, FTDX10 active, flat mirror = the FTDX10.
 * Built on the real `Settings::default()` (dumped from Rust) so the panel renders for real
 * rather than against a hand-guessed subset that drifts. */
function twoRadioSettings() {
  return {
    ...defaultSettings,
    ...FTDX10,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [FTDX10, IC9700],
    band: '20m',
    dialMhz: 14.074,
    sideband: 'USB',
  } as never
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function renderPanel() {
  return render(
    <SettingsPanel
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

beforeEach(() => {
  // Reset in place — the mocked module captured these exact spy objects at import time, so
  // replacing them here would leave the component calling the old ones.
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.17.12'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(twoRadioSettings()))
  api.get('probeCatPorts').mockImplementation(() =>
    Promise.resolve({
      found: true,
      detail: 'Icom IC-9700 on COM7 @ 115200 baud',
      portName: 'COM7',
      baud: 115200,
      model: 3081,
      modelName: 'Icom IC-9700',
      freqMhz: 144.174,
      modelSeeded: false,
    }),
  )
  api.get('testCat').mockImplementation(() => Promise.resolve({ ok: true, detail: 'ok' }))
})
afterEach(cleanup)


describe('Config tab: a findable home for backup and restore', () => {
  const openTab = async () => fireEvent.click(await screen.findByRole('tab', { name: 'Config' }))

  // The RESET cases that used to live here are now SettingsPanel.reset.test.tsx, which drives
  // the real confirm dialog. They were written against `window.confirm` — inert in this webview
  // — so they passed while the button they described would have done nothing on macOS.
  it('exists as its own tab — Backup and Restore were unfindable under Radio > Transmit limits', async () => {
    renderPanel()
    await openTab()
    expect(await screen.findByRole('button', { name: 'Back up' })).toBeTruthy()
    expect(await screen.findByRole('button', { name: 'Restore…' })).toBeTruthy()
    expect(await screen.findByRole('button', { name: /reset all settings/i })).toBeTruthy()
  })

  it('takes them OUT of Radio — a move that leaves a copy behind is not a move', async () => {
    // The duplicate is the failure mode a merge produces on its own: both parents render the
    // block, both look right in isolation, and the operator gets two of everything.
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    await screen.findByText('Transmit limits & sharing')
    expect(screen.queryByRole('button', { name: 'Back up' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Restore…' })).toBeNull()
    expect(screen.queryByRole('button', { name: /reset all settings/i })).toBeNull()
  })

  it('says where they moved from, once — an operator who knew the old place is not left hunting', async () => {
    renderPanel()
    await openTab()
    const note = document.querySelector('#settings-configurations .settings-note')?.textContent ?? ''
    expect(note).toMatch(/Transmit limits & sharing/)
    // Rendered through <T>, so an entity in the catalog would surface as literal characters.
    expect(note).not.toContain('&amp;')
  })
})

// From afar, Reveal would open the station's file manager on the station's screen and is refused,
// so ALL.TXT and the recordings are saved to this computer instead. At the station, Reveal stays.
describe('the station’s files: Reveal at the station, Save to this computer from afar', () => {
  const openTab = async () =>
    fireEvent.click(await screen.findByRole('tab', { name: EN['settings.tabs.logging'] }))
  // The Logging tab lists the connectors' credentials and their log, each an array.
  beforeEach(() => {
    api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
    api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  })

  it('from afar ALL.TXT saves here and the recordings list the station’s files; nothing reveals', async () => {
    setRemoteStation(true)
    try {
      api.get('shackFileList').mockImplementation(() => Promise.resolve([]))
      renderPanel()
      await openTab()
      // The one left is the diagnostic log's, which is this computer's own file.
      expect(screen.getAllByRole('button', { name: EN['settings.integrations.allTxt.reveal'] })).toHaveLength(1)
      expect(screen.queryByRole('button', { name: EN['settings.integrations.qsoWav.reveal'] })).toBeNull()
      api.get('shackFileRead').mockImplementation(() => Promise.resolve({ data: 'QUJD', offset: 0, size: 3 }))
      api.get('shackFileSave').mockImplementation(() => Promise.resolve({ name: 'ALL.TXT', path: '/home/op/Downloads/ALL.TXT' }))
      fireEvent.click(await screen.findByRole('button', { name: EN['remote.stationFiles.save.label'] }))
      await waitFor(() => expect(api.get('shackFileSave')).toHaveBeenCalledWith('ALL.TXT', 'QUJD', 0))
      expect(api.get('shackFileRead')).toHaveBeenCalledWith('allTxt', 'ALL.TXT', 0)
      fireEvent.click(screen.getByRole('button', { name: EN['remote.stationFiles.list.label'] }))
      await waitFor(() => expect(api.get('shackFileList')).toHaveBeenCalledWith('recordings'))
      expect(api.get('revealAllTxt')).not.toHaveBeenCalled()
      expect(api.get('revealRecordings')).not.toHaveBeenCalled()
    } finally {
      setRemoteStation(false)
    }
  })

  it('at the station both still reveal, and nothing is fetched (CONTROL)', async () => {
    renderPanel()
    await openTab()
    // ALL.TXT's and the diagnostic log's share a label.
    const reveals = await screen.findAllByRole('button', { name: EN['settings.integrations.allTxt.reveal'] })
    expect(reveals).toHaveLength(2)
    for (const reveal of reveals) fireEvent.click(reveal)
    fireEvent.click(screen.getByRole('button', { name: EN['settings.integrations.qsoWav.reveal'] }))
    await waitFor(() => expect(api.get('revealRecordings')).toHaveBeenCalledTimes(1))
    expect(api.get('revealAllTxt')).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('button', { name: EN['remote.stationFiles.save.label'] })).toBeNull()
    expect(api.get('shackFileList')).not.toHaveBeenCalled()
  })
})
