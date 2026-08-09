// @vitest-environment jsdom
// Sharing the rig with another program (#48, rogerloxton): "I can't get VarAC or FreeDV to use
// RigCTL when Nexus is running."
//
// A serial port is exclusive-open, so while Nexus holds it nothing else can reach the radio.
// But Nexus does not own the rig directly — it drives it through Hamlib's rigctld, which is a
// SERVER, and VarAC/FreeDV/WSJT-X/JS8Call/fldigi all speak that protocol. The endpoint existed
// all along (`rigctld_port`, per-radio and validated unique); it was simply never shown to
// anyone, so the apparent answer was "quit Nexus".
//
// What is worth pinning is that the address shown is the REAL one for the radio being edited.
// A hardcoded 4532 would look right on a single-radio station and silently send a second-radio
// operator to the wrong rig — the failure would be another program controlling the wrong radio,
// which is worse than no answer at all.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  // SettingsPanel pulls ~50 verbs from ../api. They all resolve null, which is enough for a
  // mount — the assertions here are about WHICH save verb gets called, not what it returns.
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

vi.mock('../api', () => {
  const mod: Record<string, unknown> = {}
  for (const v of api.VERBS) mod[v] = api.get(v)
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

/** Click "Edit" on the non-active radio (the IC-9700) so the flat rig form describes it. */
describe('sharing the rig over Hamlib NET rigctl', () => {
  /** The address and its hint, found by structure — the rendered text is split across
   *  elements (the port is interpolated, and the hint nests <strong>/<em>/<code>), so a
   *  whole-string text query cannot see either. */
  async function shareRow(container: HTMLElement) {
    // The panel is tabbed; the rig fieldset renders only under Radio.
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    await waitFor(() => {
      expect(container.querySelector('.rig-share-addr')).toBeTruthy()
    })
    const addr = container.querySelector('.rig-share-addr')!
    const hint = addr.closest('.settings-field')!.querySelector('.settings-hint')!
    return { addr, hint }
  }

  it("shows the active radio's own rigctld address, not a hardcoded default", async () => {
    const { container } = renderPanel()
    const { addr } = await shareRow(container)
    // The FTDX10 is active and its port is 4532.
    expect(addr.textContent).toBe('127.0.0.1:4532')
  })

  it('follows the radio being edited, so a second rig is not sent the first one\'s address', async () => {
    const { container } = renderPanel()
    await shareRow(container)
    // Only the NON-active radio offers Edit; the IC-9700 is on 4534 in this fixture. This is
    // the case a hardcoded 4532 would pass a single-radio check and still get wrong — and the
    // failure would be another program controlling the WRONG radio.
    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }))
    await waitFor(() => {
      expect(container.querySelector('.rig-share-addr')!.textContent).toBe('127.0.0.1:4534')
    })
  })

  it('names the protocol the other programs actually ask for', async () => {
    const { container } = renderPanel()
    const { hint } = await shareRow(container)
    // "Hamlib NET rigctl" is the string in VarAC/WSJT-X/fldigi's own rig lists. Describing it
    // any other way sends the operator hunting a dropdown for words that are not there.
    expect(hint.textContent).toMatch(/Hamlib NET rigctl/i)
    expect(hint.textContent).toMatch(/VarAC/)
    expect(hint.textContent).toMatch(/FreeDV/)
    // The instruction that is easy to miss and fails silently.
    expect(hint.textContent, 'a serial port left set in the other program defeats this')
      .toMatch(/serial port blank/i)
  })

  it('warns that two programs can both command the rig', async () => {
    const { container } = renderPanel()
    const { hint } = await shareRow(container)
    expect(hint.textContent).toMatch(/argue|conflict/i)
  })
})
