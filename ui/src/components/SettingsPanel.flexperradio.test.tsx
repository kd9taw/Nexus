// @vitest-environment jsdom
//
// SILENT DATA LOSS ON A NORMAL OPERATOR ACTION (2026-08-17 Flex audit, wave-1 #30/#46).
//
// `flexRadioIp` / `flexNativePan` / `flexNativeAudio` were FLAT settings while every other rig
// field is per-radio. Two consequences, and the second is the expensive one:
//
//   1. Two Flexes (or a Flex plus anything) could not both be configured — one address for the
//      whole station, so a radio switch used the wrong radio's address.
//   2. The Settings per-radio Edit flow routes every save of the rig form through
//      `updateRadioProfile(radioPatch(form))`, and `radioPatch` enumerated 20 fields with none of
//      the three. So configuring radio 2 silently dropped radio 1's Flex config: Save reported
//      success, and the address was gone. Exactly the `pttSerialPort` class the code already
//      documents one screen up in settings.rs.
//
// The fields are now per-radio end to end. This pins the UI half — the patch the panel sends.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel, radioPatch } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module — a hand-kept list makes the panel
// throw on mount when a verb is added, which reads as a behaviour regression (see radioroster).
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
  flexRadioIp: '',
  flexNativePan: false,
  flexNativeAudio: false,
}
/** The NON-ACTIVE radio: a fully-configured Flex. This is the config that used to vanish. */
const FLEX = {
  ...FTDX10,
  id: 1,
  name: 'FLEX-6400',
  serialPort: '',
  rigModel: 2036,
  rigModelName: 'FlexRadio FLEX-6xxx (SmartSDR CAT)',
  rigConn: 'network',
  rigAddr: '127.0.0.1:5002',
  rigctldPort: 4534,
  rotctldPort: 4535,
  audioIn: 'in-1',
  audioOut: 'out-1',
  flexRadioIp: '192.0.2.77',
  flexNativePan: true,
  flexNativeAudio: true,
}

function settingsWith(radios: (typeof FTDX10)[]) {
  return {
    ...defaultSettings,
    ...FTDX10, // the flat mirror describes the ACTIVE radio (id 0)
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios,
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
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsWith([FTDX10, FLEX])))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('the Flex config travels with the radio it belongs to', () => {
  it('radioPatch carries the Flex three — the field set a per-radio Save is built from', () => {
    const patch = radioPatch({
      flexRadioIp: '192.0.2.77',
      flexNativePan: true,
      flexNativeAudio: true,
    })
    expect(patch.flexRadioIp).toBe('192.0.2.77')
    expect(patch.flexNativePan).toBe(true)
    expect(patch.flexNativeAudio).toBe(true)
    // …and an unset radio gets the honest empty/off, never `undefined` (the backend field is
    // non-optional, and `undefined` would serialize the key away).
    const empty = radioPatch({})
    expect(empty.flexRadioIp).toBe('')
    expect(empty.flexNativePan).toBe(false)
    expect(empty.flexNativeAudio).toBe(false)
  })

  it('saving a NON-ACTIVE radio keeps its Flex address instead of blanking it', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))

    // Edit the non-active Flex — the "Editing …" banner marks the retargeted form.
    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }))
    expect(await screen.findByText(/Editing FLEX-6400/)).toBeTruthy()

    // Save WITHOUT touching anything. This is the whole bug: the operator configures another
    // radio, saves, and the Flex fields they never went near are wiped.
    // The footer Save (`type="submit"`) — other sections carry their own "Save …" buttons.
    const save = screen
      .getAllByRole('button', { name: 'Save' })
      .find((b) => (b as HTMLButtonElement).type === 'submit')!
    fireEvent.click(save)

    await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
    const calls = api.get('updateRadioProfile').mock.calls
    const [id, patch] = calls[calls.length - 1] as [number, Record<string, unknown>]
    expect(id).toBe(1)
    expect(patch.flexRadioIp).toBe('192.0.2.77')
    expect(patch.flexNativePan).toBe(true)
    expect(patch.flexNativeAudio).toBe(true)
    // The station-wide write must NOT be the path taken — that is what stamped the edited
    // radio's config onto the active one (the 2026-07-25 report).
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })
})
