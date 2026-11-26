// @vitest-environment jsdom
//
// "CI-V address" (#528): a PER-RADIO field in Rig & CAT ▸ Advanced for a radio whose CI-V address
// was changed on its menu. Blank by default (the model's own address), offered only for a Hamlib
// Icom, saved with the radio, and an address that is not one stops the Save. The engine side (the
// native daemon and rigctld's `-c` talk on it; blank keeps the default) is pinned in service.rs.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET (2026-10-09). The slowest case here, "renders the operator three-radio table with every rule…", takes
// 0.32 s and 0.34 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module rather than a hand-kept list.
//
// The list was the problem: a verb missing from it made the panel THROW ON MOUNT ("No export is
// defined on the mock"), which presents as a behaviour regression in whichever test happened to
// run -- not as the out-of-date mock it actually is. Reading the real module's export names makes
// that failure impossible by construction.
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
  name: 'FTdx10',
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
  pttSerialPort: '',
  dataModesPlainSsb: false,
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
  rigModel: 3081,
  rigModelName: 'Icom IC-9700',
  rigctldPort: 4534,
  rotctldPort: 4535,
  audioIn: 'in-1',
  audioOut: 'out-1',
  bands: ['2m', '70cm'],
}
const FT991A = {
  ...FTDX10,
  id: 2,
  name: 'FT-991A',
  serialPort: 'COM9',
  rigModel: 1035,
  rigModelName: 'Yaesu FT-991A',
  rigctldPort: 4536,
  rotctldPort: 4537,
  audioIn: 'in-2',
  audioOut: 'out-2',
  bands: ['2m', '70cm'],
}

/** The operator's real shack: HF rig + a VHF weak-signal-digital rig + a VHF FM/APRS rig, with the
 * band+mode rules that split 2 m between the last two. FTdx10 active (where an HF day starts). */
function threeRadioSettings() {
  return {
    ...defaultSettings,
    ...FTDX10,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [FTDX10, IC9700, FT991A],
    routingRules: [
      { bands: ['2m', '70cm'], mode: 'fm', radio: 2 },
      { bands: ['2m', '70cm'], mode: 'digital', radio: 1 },
    ],
    defaultRadio: 0,
    band: '20m',
    dialMhz: 14.074,
    sideband: 'USB',
  }
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.3'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(threeRadioSettings() as never))
  api.get('routePreview').mockImplementation(() => Promise.resolve({ radio: 2, name: 'FT-991A' }))
})
afterEach(cleanup)

const openRadioTab = async () => {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
}

const openAdvanced = async () => {
  await openRadioTab()
  const group = document.getElementById('settings-rig-advanced')!
  fireEvent.click(group.querySelector('button.settings-group-toggle')!)
}

function activeIs(radio: typeof FT991A, civAddr = '') {
  const radios = [FTDX10, IC9700, FT991A].map((r) => (r.id === radio.id ? { ...r, civAddr } : r))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...threeRadioSettings(), ...radio, civAddr, activeRadio: radio.id, radios } as never),
  )
}

/** The footer Save (`type="submit"`) — other sections carry their own "Save …" buttons. */
function clickSave() {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
}

function lastCall(name: string): unknown[] | undefined {
  const calls = api.get(name).mock.calls
  return calls.length ? (calls[calls.length - 1] as unknown[]) : undefined
}

const field = () => screen.findByRole('textbox', { name: /^CI-V address/ }) as Promise<HTMLInputElement>

describe('CI-V address', () => {
  it('is offered for an Icom, blank by default, and a radio menu address is saved with the radio', async () => {
    activeIs(IC9700)
    renderPanel()
    await openAdvanced()
    const input = await field()
    expect(input.value).toBe('')
    expect(input.closest('.settings-field')!.textContent).toContain("model's default")
    fireEvent.change(input, { target: { value: '0x96' } })
    clickSave()
    // The radio's own fields go through the per-radio verb, which takes the radio's every field.
    await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
    const patch = lastCall('updateRadioProfile')![1] as Record<string, unknown>
    expect(patch.civAddr).toBe('0x96')
  })

  it('an address that is not one stops the Save and says why', async () => {
    activeIs(IC9700)
    renderPanel()
    await openAdvanced()
    fireEvent.change(await field(), { target: { value: 'E0' } })
    clickSave()
    expect(await screen.findAllByText(/CI-V address must be one or two hex digits/)).not.toHaveLength(0)
    expect(api.get('setSettings')).not.toHaveBeenCalled()
    expect(api.get('updateRadioProfile')).not.toHaveBeenCalled()
  })

  it('CONTROL: not offered on a radio with no CI-V', async () => {
    activeIs(FT991A)
    renderPanel()
    await openAdvanced()
    // The neighbouring switch is there, so the absence below is the gate and not a closed group.
    await screen.findByRole('switch', { name: /^Hold the data mode while SSTV is receiving/ })
    expect(screen.queryByRole('textbox', { name: /^CI-V address/ })).toBeNull()
  })
})
