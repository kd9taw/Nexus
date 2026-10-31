// @vitest-environment jsdom
//
// "Use this computer's microphone" (Settings ▸ Radio ▸ Audio): a PER-RADIO switch, off by default so
// a station on the radio's own microphone sees no change. On, it offers the microphone picker (no
// system-default entry: the engine never captures the default) and the gain. The save carries the
// three fields on the radio's patch. The safety rules themselves are pinned in the engine
// (crates/tempo-app/src/engine/pc_mic_tests.rs) and the radio loop (service/tests/pc_mic.rs).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel, radioPatch } from './SettingsPanel'
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

describe("Use this computer's microphone", () => {
  it('is off by default, and on it offers the microphone and the gain', async () => {
    api.get('getAudioDevices').mockImplementation(() =>
      Promise.resolve({ input: ['in-0', 'Desk Mic'], output: ['out-0'] }),
    )
    renderPanel()
    await openRadioTab()
    const sw = await screen.findByRole('switch', { name: "Use this computer's microphone" })
    expect(sw.getAttribute('aria-checked')).toBe('false')
    expect(screen.queryByRole('slider', { name: 'Microphone gain' })).toBeNull()
    fireEvent.click(sw)
    expect(sw.getAttribute('aria-checked')).toBe('true')
    const gain = await screen.findByRole('slider', { name: 'Microphone gain' })
    expect(gain.getAttribute('min')).toBe('-20')
    expect(gain.getAttribute('max')).toBe('20')
    const picker = (await screen.findByText('Choose a microphone…')).closest('select')!
    const options = Array.from(picker.querySelectorAll('option')).map((o) => o.value)
    // The first entry chooses nothing; there is no "system default" entry to fall back to.
    expect(options[0]).toBe('')
    expect(picker.textContent).not.toContain('System default')
  })

  it('rides the radio patch', () => {
    const patch = radioPatch({ pcMic: true, pcMicDevice: 'Desk Mic', pcMicGainDb: 6 })
    expect(patch.pcMic).toBe(true)
    expect(patch.pcMicDevice).toBe('Desk Mic')
    expect(patch.pcMicGainDb).toBe(6)
    const off = radioPatch({})
    expect(off.pcMic).toBe(false)
    expect(off.pcMicDevice).toBe('')
    expect(off.pcMicGainDb).toBe(0)
  })
})
