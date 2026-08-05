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

describe('the port hint tells the truth about TCI', () => {
  // The hint shipped as "the TCI Server box … which is a WebSocket protocol Nexus can't
  // drive". Nexus does drive it: `rigmodels::extended_rig_models` ships (7, "TCI (SunSDR /
  // ExpertSDR)") and the bundled libhamlib-4.dll carries `tci1x.c` (default 127.0.0.1:50001).
  // The reporting operator's port IS 50001 — so he can tick "Show all models", find the TCI
  // entry, and have just been told by us that it cannot work. The hint's real job is
  // narrower and still true: 50001 is a TCI port, not a CAT port, so it is the wrong number
  // for THIS field.
  it('does not deny a model the build ships', async () => {
    await openRadioTab(networkRadio(2054, 'Thetis (Hermes Lite 2 / ANAN / HPSDR)'))
    const text = screen.getByText(/Running an SDR program/).textContent ?? ''
    expect(text).not.toMatch(/can'?t drive|cannot drive|unsupported/i)
    // Names the route that exists, so "not this box" doesn't read as "not possible".
    expect(text).toMatch(/TCI/)
    expect(text).toMatch(/model 7|\(7\)/)
    // …while still steering this field to the CAT port.
    expect(text).toMatch(/13013/)
    expect(text).toMatch(/50001/)
  })

  // ⭐ THE HINT IS RENDERED ON `rigConn === 'network'` ALONE — no model condition — so a
  // sentence about which port belongs in this field is a claim about EVERY model. "50001 is
  // not a CAT port" is true of the Thetis/PowerSDR CAT profiles and false of the one model
  // the same sentence tells the operator to go and find: take its advice, tick Show all
  // models, select "TCI (SunSDR / ExpertSDR)" (7), and the correct Network Address IS
  // 127.0.0.1:50001 — Hamlib tci1x's own default, and what `service.rs` spawns
  // `rigctld -m 7 -r 127.0.0.1:50001` against.
  it('does not call 50001 the wrong box while the TCI model is the one selected', async () => {
    await openRadioTab(networkRadio(7, 'TCI (SunSDR / ExpertSDR)'))
    const text = screen.getByText(/Running an SDR program/).textContent ?? ''
    // 50001 is this operator's address. Nothing may tell him it is not a CAT port, or that
    // some other box is the route "we recommend".
    expect(text).not.toMatch(/not a CAT port/i)
    expect(text).not.toMatch(/\bnot\b[^.]{0,40}TCI Server/i)
    // It must name his number as the one that belongs here.
    expect(text).toMatch(/50001/)
    expect(text).toMatch(/TCI Server/)
  })

  // The claim "beta in Hamlib" is not readable from the artifact we ship: `rig_caps.status`
  // in the bundled libhamlib-4.dll is an enum, not a string, so nothing in this build can
  // check it. It matched upstream source at the time of writing and nothing keeps it true.
  it('makes no claim about Hamlib maturity that this build cannot check', async () => {
    for (const model of [2054, 7] as const) {
      cleanup()
      await openRadioTab(networkRadio(model, 'x'))
      const text = screen.getByText(/Running an SDR program/).textContent ?? ''
      expect(text).not.toMatch(/beta|alpha|experimental/i)
    }
  })
})
