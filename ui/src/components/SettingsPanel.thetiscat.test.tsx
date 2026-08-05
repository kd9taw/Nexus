// @vitest-environment jsdom
//
// Field report 2026-08: a Hermes Lite 2 owner running Thetis could not make CAT work in
// Nexus (it worked in WSJT-X), and got it going by picking a FlexRadio profile. Two things
// in this panel were part of that: "Network (FlexRadio / remote)" told an SDR operator the
// row was not for them, and the two Flex native-stream toggles were gated on the rig model's
// NAME containing "flex" — which the PowerSDR entry's label matched, offering a SmartSDR
// VITA-49 stream to an ANAN/HL2 that cannot serve one.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
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
    'civDiagnosticLog', 'civDiagnosticStatus', 'allTxtLocation', 'revealAllTxt', 'appVersion',
    'getSpectrumRow', 'setFrequency', 'getWatchlist', 'setWatchlist', 'openPanelWindow',
    'getAssistanceJournal', 'setUnassistedMode',
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

/** One radio, on a network CAT address, with the given model. */
function networkRadio(rigModel: number, rigModelName: string) {
  const radio = {
    id: 0,
    name: 'Radio 1',
    enabled: true,
    serialPort: '',
    baud: 38400,
    rigModel,
    rigModelName,
    rigConn: 'network',
    rigAddr: '127.0.0.1:13013',
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
  return {
    ...defaultSettings,
    ...radio,
    // `flexRadioIp` is the OTHER half of the Flex gate — empty here, so the model number is
    // the only thing that can open those toggles.
    flexRadioIp: '',
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [radio],
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.0.1'))
})
afterEach(cleanup)

async function openRadioTab(settings: unknown) {
  api.get('getSettings').mockImplementation(() => Promise.resolve(settings))
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  // The Connection row is unconditional on this tab — wait for it before asserting absence.
  await screen.findByText('Connection')
}

/** The Flex native-stream toggles live in the collapsed "Advanced" disclosure. Open it —
 *  an absence assertion against a collapsed group would pass for every model. */
async function openAdvanced() {
  fireEvent.click(await screen.findByRole('button', { name: /Advanced/ }))
  // Proof the group really opened, so the toggle assertions below mean something.
  await screen.findByText('rigctld TCP Port')
}

describe('the SmartSDR-only toggles are gated on the radio, not on the word "flex"', () => {
  it('an SDR program that merely mentions FLEX is not offered a SmartSDR stream', async () => {
    // The shipped 2048 label carries "legacy FLEX" — and the old label was literally
    // "FlexRadio PowerSDR". Neither radio can serve a SmartSDR VITA-49 panadapter or DAX.
    await openRadioTab(networkRadio(2048, 'PowerSDR / mRX PS (Apache ANAN / legacy FLEX)'))
    await openAdvanced()
    expect(screen.queryByText(/Flex native panadapter/i)).toBeNull()
    expect(screen.queryByText(/Flex native DAX audio/i)).toBeNull()
  })

  it('Thetis is not offered one either', async () => {
    await openRadioTab(networkRadio(2054, 'Thetis (Hermes Lite 2 / ANAN / HPSDR)'))
    await openAdvanced()
    expect(screen.queryByText(/Flex native panadapter/i)).toBeNull()
    expect(screen.queryByText(/Flex native DAX audio/i)).toBeNull()
  })

  it('a real FLEX-6xxx on SmartSDR CAT still gets both — the verified-good configuration', async () => {
    await openRadioTab(networkRadio(2036, 'FlexRadio FLEX-6xxx (SmartSDR CAT)'))
    await openAdvanced()
    expect(screen.queryByText(/Flex native panadapter/i)).not.toBeNull()
    expect(screen.queryByText(/Flex native DAX audio/i)).not.toBeNull()
  })
})

describe('the Connection row tells an SDR operator it is for them', () => {
  it('the network option is not labelled as a FlexRadio row', async () => {
    await openRadioTab(networkRadio(2054, 'Thetis (Hermes Lite 2 / ANAN / HPSDR)'))
    const network = screen.getByRole('option', { name: /^Network/ }) as HTMLOptionElement
    expect(network.value).toBe('network') // the persisted value must NOT move
    expect(network.textContent).toMatch(/SDR/i)
    expect(network.textContent).not.toMatch(/FlexRadio/i)
  })
})
