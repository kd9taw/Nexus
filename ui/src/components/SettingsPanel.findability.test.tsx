// @vitest-environment jsdom
//
// TWO SETTINGS THAT WORK BUT ARE NOT FOUND, each now pointed at from where an operator looks.
//
// #522: with Nexus's own Flex client on, the radio's native DAX audio is a separate switch, off by
// default, so the waterfall heard the laptop's microphone and the reporter read the audio devices
// as reversed. Settings ▸ Radio ▸ Audio now says so under the input device, with a link that opens
// the switch, while the client runs, native audio is off and the input is not a DAX device.
//
// #498: the WSJT-X UDP address is part of each saved profile, so loading a profile replaced the
// address the operator had typed and the screen never said so. The address now says it, with a
// link to Settings ▸ Radio ▸ Profiles.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n/en'

// The house budget for a panel render on a loaded box (see SettingsPanel.flexperradio.test.tsx).
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module, as the panel's other suites do.
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

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

/** The reporter's radio: a FLEX-8600 on Nexus's own Flex client, native DAX audio off (as it
 *  ships), the laptop's microphone picked as the input. */
const FLEX = {
  id: 0,
  name: 'FLEX-8600',
  enabled: true,
  serialPort: '',
  baud: 38400,
  rigModel: 2036,
  rigModelName: 'FlexRadio FLEX-6xxx (SmartSDR CAT)',
  rigConn: 'network',
  rigAddr: '127.0.0.1:5002',
  rigctldPort: 4532,
  rotctldPort: 4533,
  icomNativeCat: false,
  audioIn: 'Microphone Array (Realtek)',
  audioOut: 'Speakers (Realtek)',
  txLevel: 1,
  rxGain: 1,
  pttMethod: 'cat',
  rotatorModel: 0,
  rotatorPort: '',
  rotatorBaud: 9600,
  rotatorHost: '',
  nativeScope: 'auto',
  bands: [],
  flexRadioIp: '192.0.2.10',
  flexNativePan: false,
  flexNativeAudio: false,
  flexNativeCat: true,
}

let stored: Record<string, unknown> = {}

beforeEach(() => {
  stored = {}
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() =>
    Promise.resolve({
      input: ['Microphone Array (Realtek)', 'DAX Audio RX 1 (FlexRadio DAX)'].map((name) => ({ name, label: name })),
      output: ['Speakers (Realtek)', 'DAX TX (FlexRadio DAX)'].map((name) => ({ name, label: name })),
    }),
  )
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.25.0'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'W1AW', mygrid: 'FN31', ...stored } as never),
  )
  // The panel scrolls to the section it opens at; jsdom has no layout to scroll.
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(cleanup)

function renderAt(target: string) {
  render(
    <SettingsPanel
      target={target}
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

/** The Flex radio, flat and in the roster, with `over` on top. */
function flexStation(over: Partial<typeof FLEX> = {}) {
  const radio = { ...FLEX, ...over }
  stored = { ...radio, activeRadio: 0, radios: [radio], band: '20m', dialMhz: 14.074, sideband: 'USB' }
}

/** The note's link, found by the words it carries, inside the Audio section. */
const daxLink = () =>
  [...(document.querySelectorAll('#settings-audio button') ?? [])].find((b) =>
    b.textContent?.includes('Flex native DAX audio (Beta) in Settings ▸ Radio ▸ Rig & CAT ▸ Advanced'),
  ) as HTMLButtonElement | undefined

describe('#522: the Audio section points a Flex client operator at native DAX audio', () => {
  it("shows for the reporter's setup, and its link opens the switch it names", async () => {
    flexStation()
    renderAt('audio')
    await screen.findByText(EN['settings.audio.input.label'])
    await waitFor(() => expect(daxLink(), 'the note and its link show').toBeTruthy())
    // The note sits with the input device it is about.
    const field = daxLink()!.closest('.settings-field')!
    expect(field.textContent).toContain(EN['settings.audio.input.label'])
    expect(document.getElementById('settings-rig-advanced')?.querySelector('[role="switch"]') ?? null).toBeNull()
    fireEvent.click(daxLink()!)
    // The Rig & CAT Advanced group opens on the switch.
    const label = await screen.findByText(EN['settings.rigControl.flexAudio.label'])
    const sw = label.closest('label')!.querySelector('[role="switch"]')!
    expect(document.getElementById('settings-rig-advanced')?.contains(sw)).toBe(true)
    expect(sw.getAttribute('aria-checked')).toBe('false')
  })

  it('goes once native DAX audio is turned on', async () => {
    flexStation({ flexNativeAudio: true })
    renderAt('audio')
    await screen.findByText(EN['settings.audio.input.label'])
    await screen.findByDisplayValue('Microphone Array (Realtek)')
    expect(daxLink()).toBeUndefined()
  })

  it('does not show with a DAX device picked as the input', async () => {
    flexStation({ audioIn: 'DAX Audio RX 1 (FlexRadio DAX)', audioOut: 'DAX TX (FlexRadio DAX)' })
    renderAt('audio')
    await screen.findByDisplayValue('DAX Audio RX 1 (FlexRadio DAX)')
    expect(daxLink()).toBeUndefined()
  })

  it('does not show with the Flex client off (SmartSDR CAT drives the radio)', async () => {
    flexStation({ flexNativeCat: false })
    renderAt('audio')
    await screen.findByDisplayValue('Microphone Array (Realtek)')
    expect(daxLink()).toBeUndefined()
  })
})

describe('#498: the WSJT-X UDP address says each saved profile carries its own', () => {
  it('says so under the address, and its link opens Settings ▸ Radio ▸ Profiles', async () => {
    stored = { wsjtxUdp: true }
    renderAt('integrations-feeds')
    await screen.findByText(EN['settings.integrations.udpAddr.label'])
    const field = document.querySelector('[data-setting="wsjtxUdpAddr"]')!.closest('.settings-field')!
    const link = [...field.querySelectorAll('button')].find((b) =>
      b.textContent?.includes('Settings ▸ Radio ▸ Profiles'),
    )
    expect(link, 'the signpost and its link show beside the address').toBeTruthy()
    expect(document.getElementById('settings-profiles')).toBeNull()
    fireEvent.click(link!)
    await waitFor(() => expect(document.getElementById('settings-profiles')).not.toBeNull())
    expect(screen.getByRole('tab', { name: 'Radio' }).getAttribute('aria-selected')).toBe('true')
  })
})
