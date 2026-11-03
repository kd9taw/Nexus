// @vitest-environment jsdom
//
// THE FLEX SWITCHES NO LONGER VANISH ON A SERIAL CONNECTION. On a Flex model (2036 / 23005) with
// Connection = Serial, Settings hid the Flex native panadapter, DAX audio and client without a
// word, so an operator told to "turn on Flex native panadapter" could not find it.
//
// Only the PANADAPTER needs Connection: Network (`native_spectrum_kind` serves the Flex stream on
// the network alone). The native client and its DAX audio talk to the radio at its own address on
// Serial as well as Network (`flex_client_reachable`; OmniRig, the Icom network connection and TCI
// take the radio first), so they work on Serial. Pinned here: on
// a Flex off the network the panadapter shows disabled with "Needs Connection: Network." and a
// click changes nothing, while the client and DAX audio show live, with no note, and turn on and
// off. On the network all three are live with no note; a radio that is not a Flex shows none.
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

const PAN = 'Flex native panadapter (Beta)'
const CLIENT_AND_AUDIO = ['Flex native DAX audio (Beta)', 'Flex native client (Beta)']
const LABELS = [PAN, ...CLIENT_AND_AUDIO]
const NOTE = 'Needs Connection: Network.'
const UNBENCHED = 'Let Nexus send CW, Tune and ATU through the native Flex client (not yet bench-tested)'
const NOT_HERE = "The Flex native client doesn't run on OmniRig or TCI."

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
  ])('%s: the panadapter shows disabled, saying it needs the network connection', async (_name, radio) => {
    await renderAdvanced(radio)
    const sw = switchFor(PAN)
    expect(sw, 'the panadapter is shown').not.toBeNull()
    expect(sw!.disabled, 'the panadapter is disabled').toBe(true)
    expect(noteFor(PAN), 'the panadapter says why').toBe(true)
    // A click on a disabled switch changes nothing.
    fireEvent.click(sw!)
    expect(sw!.getAttribute('aria-checked')).toBe('false')
    // The note is the panadapter's alone.
    expect(screen.getAllByText(NOTE)).toHaveLength(1)
  })

  it.each([
    ['FLEX-6xxx (2036) on Serial', FLEX_SERIAL],
    ['the other Flex model (23005) on Serial', { ...FLEX_SERIAL, rigModel: 23005 }],
  ])('%s: the client and DAX audio are live, with no note, and turn on', async (_name, radio) => {
    await renderAdvanced(radio)
    for (const label of CLIENT_AND_AUDIO) {
      const sw = switchFor(label)
      expect(sw, `${label} is shown`).not.toBeNull()
      expect(sw!.disabled, `${label} is live`).toBe(false)
      expect(noteFor(label), `${label} carries no note`).toBe(false)
      fireEvent.click(sw!)
      expect(sw!.getAttribute('aria-checked'), `${label} turns on`).toBe('true')
    }
  })

  it('a client switched on earlier can be turned off on Serial', async () => {
    await renderAdvanced({ ...FLEX_SERIAL, flexNativeCat: true, flexNativeAudio: true, flexRadioIp: '192.0.2.77' })
    for (const label of CLIENT_AND_AUDIO) {
      const sw = switchFor(label)!
      expect(sw.getAttribute('aria-checked')).toBe('true')
      fireEvent.click(sw)
      expect(sw.getAttribute('aria-checked'), `${label} turns off`).toBe('false')
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

// OmniRig and TCI hand CAT to another program: the CAT daemon starts that connection's own driver
// before it ever asks for the Flex client (`spawn_cat_daemon`), so the client, its DAX audio and
// its CW/Tune/ATU setting do nothing there. They show with a note saying so, refuse to turn ON, and
// still turn OFF — a setting left on from another connection must never be stuck on.
describe('the Flex client switches on OmniRig and TCI', () => {
  const noteOn = (label: string) =>
    [...(screen.queryByText(label)?.closest('label')?.querySelectorAll('.settings-hint') ?? [])].some(
      (h) => h.textContent === NOT_HERE,
    )

  it.each(['omnirig', 'tci'])('on %s, an off client and DAX audio say why and do not turn on', async (conn) => {
    await renderAdvanced({ ...FLEX_SERIAL, rigConn: conn, flexRadioIp: '192.0.2.77' })
    for (const label of CLIENT_AND_AUDIO) {
      const sw = switchFor(label)
      expect(sw, `${label} is shown`).not.toBeNull()
      expect(noteOn(label), `${label} says why`).toBe(true)
      expect(sw!.disabled, `${label} refuses to turn on`).toBe(true)
      fireEvent.click(sw!)
      expect(sw!.getAttribute('aria-checked')).toBe('false')
    }
  })

  it.each(['omnirig', 'tci'])('on %s, switches left on say why and turn off', async (conn) => {
    await renderAdvanced({
      ...FLEX_SERIAL,
      rigConn: conn,
      flexRadioIp: '192.0.2.77',
      flexNativeCat: true,
      flexNativeAudio: true,
      flexNativeUnbenched: true,
    } as typeof FLEX_SERIAL)
    // The CW/Tune/ATU setting first: it shows only while the client is on.
    for (const label of [UNBENCHED, ...CLIENT_AND_AUDIO]) {
      const sw = switchFor(label)
      expect(sw, `${label} is shown`).not.toBeNull()
      expect(noteOn(label), `${label} says why`).toBe(true)
      expect(sw!.disabled, `${label} can still be turned off`).toBe(false)
      fireEvent.click(sw!)
      expect(switchFor(label)?.getAttribute('aria-checked') ?? 'false', `${label} turns off`).toBe('false')
    }
  })

  it('on Serial and on the network the client switches carry no such note', async () => {
    for (const radio of [FLEX_SERIAL, { ...FLEX_SERIAL, rigConn: 'network', rigAddr: '127.0.0.1:5002' }]) {
      await renderAdvanced({ ...radio, flexRadioIp: '192.0.2.77' })
      expect(screen.queryByText(NOT_HERE)).toBeNull()
      cleanup()
    }
  })
})
