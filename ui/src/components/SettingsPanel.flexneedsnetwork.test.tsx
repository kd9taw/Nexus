// @vitest-environment jsdom
//
// THE FLEX SWITCHES SAY WHY THEY CANNOT BE USED, INSTEAD OF VANISHING. The Flex native panadapter,
// DAX audio and client work only over the network connection. On a Flex model (2036 / 23005) with
// Connection = Serial they used to be hidden without a word, so an operator told to "turn on Flex
// native panadapter" could not find it. Pinned here: on a Flex off the network the three show,
// disabled, each with "Needs Connection: Network."; a click changes nothing. On the network they
// are live with no note; a radio that is not a Flex shows none of them.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

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

// Mock EVERY export of `../api`, derived from the real module, as the panel's other suites do.
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

const LABELS = ['Flex native panadapter (Beta)', 'Flex native DAX audio (Beta)', 'Flex native client (Beta)']
const NOTE = 'Needs Connection: Network.'

/** A FLEX-6400 on a serial (COM) connection: the reporter's setup. */
const FLEX_SERIAL = {
  id: 0,
  name: 'FLEX-6400',
  enabled: true,
  serialPort: 'COM5',
  baud: 38400,
  rigModel: 2036,
  rigModelName: 'FlexRadio FLEX-6xxx (SmartSDR CAT)',
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
  flexRadioIp: '',
  flexNativePan: false,
  flexNativeAudio: false,
  flexNativeCat: false,
}

function settingsWith(radio: typeof FLEX_SERIAL) {
  return {
    ...defaultSettings,
    ...radio,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [radio],
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

/** The panel on Radio ▸ Rig & CAT with its Advanced group open, where the Flex switches live. */
async function renderAdvanced(radio: typeof FLEX_SERIAL) {
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsWith(radio)))
  render(
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
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  await screen.findByText('Advanced')
  const group = document.getElementById('settings-rig-advanced')
  expect(group, 'the Rig & CAT Advanced group renders').not.toBeNull()
  fireEvent.click(group!.querySelector('button.settings-group-toggle')!)
  // The control every case below is measured against: the group really is open.
  await screen.findByText('rigctld TCP Port')
}

/** The switch under `label`, or null when the field is not rendered. */
const switchFor = (label: string) =>
  (screen.queryByText(label)?.closest('label')?.querySelector('[role="switch"]') ?? null) as HTMLButtonElement | null
const noteFor = (label: string) =>
  [...(screen.queryByText(label)?.closest('label')?.querySelectorAll('.settings-hint') ?? [])].some(
    (h) => h.textContent === NOTE,
  )

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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('the Flex native switches on a Flex that is not on the network', () => {
  it.each([
    ['FLEX-6xxx (2036) on Serial', FLEX_SERIAL],
    ['the other Flex model (23005) on Serial', { ...FLEX_SERIAL, rigModel: 23005 }],
  ])('%s: all three show, disabled, saying they need the network connection', async (_name, radio) => {
    await renderAdvanced(radio)
    for (const label of LABELS) {
      const sw = switchFor(label)
      expect(sw, `${label} is shown`).not.toBeNull()
      expect(sw!.disabled, `${label} is disabled`).toBe(true)
      expect(noteFor(label), `${label} says why`).toBe(true)
      // A click on a disabled switch changes nothing.
      fireEvent.click(sw!)
      expect(sw!.getAttribute('aria-checked')).toBe('false')
    }
  })

  it('on the network the three are live, with no note', async () => {
    await renderAdvanced({ ...FLEX_SERIAL, rigConn: 'network', rigAddr: '127.0.0.1:5002', serialPort: '' })
    for (const label of LABELS) {
      const sw = switchFor(label)
      expect(sw, `${label} is shown`).not.toBeNull()
      expect(sw!.disabled, `${label} is live`).toBe(false)
      expect(noteFor(label), `${label} carries no note`).toBe(false)
    }
    expect(screen.queryByText(NOTE)).toBeNull()
  })

  it('a radio that is not a Flex shows none of them', async () => {
    await renderAdvanced({ ...FLEX_SERIAL, name: 'FTDX10', rigModel: 1042, rigModelName: 'Yaesu FTDX10' })
    for (const label of LABELS) expect(switchFor(label), label).toBeNull()
    expect(screen.queryByText(NOTE)).toBeNull()
  })
})
