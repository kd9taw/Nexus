// @vitest-environment jsdom
//
// #248 (remarkable_mango): the Units preference — km or miles, °C or °F, for the whole app —
// lived under Settings ▸ Digital ▸ Station Housekeeping, beside FT8 sequencing options, and no
// search word reached it. An operator asking for miles in the receive window was told where it
// was; nobody would have found it. It is a station-wide setting, so it belongs on the Station
// tab, and the registry's search and deep link must land on it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { searchSettings, resolveTarget } from '../settings/registry'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module (the fdwho pattern).
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

const RADIO = {
  id: 0, name: 'FTDX10', enabled: true, serialPort: '', baud: 38400, rigModel: 0,
  rigModelName: 'None', rigConn: 'serial', rigAddr: '', rigctldPort: 4532, rotctldPort: 4533,
  icomNativeCat: false, audioIn: '', audioOut: '', txLevel: 1, rxGain: 1, pttMethod: 'cat',
  rotatorModel: 0, rotatorPort: '', rotatorBaud: 9600, rotatorHost: '', nativeScope: 'auto',
  bands: [], flexRadioIp: '', flexNativePan: false, flexNativeAudio: false,
}

function settingsFixture() {
  return {
    ...defaultSettings,
    ...RADIO,
    mycall: 'W9ABC', mygrid: 'EN52', activeRadio: 0, radios: [RADIO],
    band: '20m', dialMhz: 14.074, sideband: 'USB', units: 'imperial',
  } as never
}

const features: FeaturesApi = {
  enabled: () => true, setEnabled: vi.fn(), all: () => [], profile: 'full', setProfile: vi.fn(),
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
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsFixture()))
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

async function openTab(name: string) {
  fireEvent.click(await screen.findByRole('tab', { name }))
}

describe('Units is a station-wide setting (#248)', () => {
  it('renders on the Station tab, inside Operator & Radio, with the saved value', async () => {
    renderPanel()
    await openTab('Station')
    // Positive control: the section really rendered — the callsign field is in it.
    const station = await screen.findByDisplayValue('W9ABC')
    const section = document.getElementById('settings-operator-radio')
    expect(section, 'the Station tab must render Operator & Radio').toBeTruthy()
    expect(section!.contains(station)).toBe(true)

    const units = screen.getByRole('combobox', { name: 'Units' }) as HTMLSelectElement
    expect(section!.contains(units), 'Units must sit in the Station section').toBe(true)
    expect(units.value).toBe('imperial')
  })

  it('is no longer filed under Digital ▸ Station Housekeeping', async () => {
    renderPanel()
    await openTab('Digital')
    // Positive control: the Digital tab rendered its FT8/FT4 section.
    expect(await screen.findByText('Station Housekeeping')).toBeTruthy()
    expect(screen.queryByRole('combobox', { name: 'Units' })).toBeNull()
  })

  it('is found by the words an operator types, and the hit lands on the Station section', () => {
    for (const q of ['units', 'miles', 'imperial', 'metric', 'km', 'fahrenheit']) {
      const hits = searchSettings(q)
      expect(hits[0]?.section.id, `search "${q}"`).toBe('operator-radio')
    }
    expect(resolveTarget('operator-radio')).toMatchObject({ tab: 'station' })
  })
})
