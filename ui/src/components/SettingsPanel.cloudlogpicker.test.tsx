// @vitest-environment jsdom
//
// #226: "Find my station locations". Wavelog's station profile id is a NUMBER, and the operator
// has no way to know theirs without leaving the app — so Nexus can ask the instance and offer the
// list. The request carries the API KEY, so the operator's constraint is pinned here as a test:
// it goes out ONLY on an explicit press. Nothing on mount, nothing on a timer.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
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
    mycall: 'DG3ET', mygrid: 'JO31', activeRadio: 0, radios: [RADIO],
    band: '20m', dialMhz: 14.074, sideband: 'USB',
    cloudlogUrl: 'https://log.example.org', cloudlogStationId: '',
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

const STATIONS = [
  { stationId: '3', profileName: 'Home', callsign: 'DG3ET', gridsquare: 'JO31NF', active: true },
  { stationId: '7', profileName: 'Portable', callsign: 'DG3ET/P', gridsquare: 'JN48', active: false },
]

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
  api.get('getCloudlogStations').mockImplementation(() => Promise.resolve(STATIONS))
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

async function openLogging() {
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Logging & Connectors' }))
}

describe('Cloudlog/Wavelog station location picker (#226)', () => {
  it('asks the instance only on an explicit press — never on render', async () => {
    await openLogging()
    // The request spends the API key on the wire, so nothing may fire it but the operator.
    expect(api.get('getCloudlogStations')).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Find my station locations' }))
    await waitFor(() => expect(api.get('getCloudlogStations')).toHaveBeenCalledTimes(1))
  })

  it('offers what the instance returned and fills the field with the number', async () => {
    await openLogging()
    fireEvent.click(screen.getByRole('button', { name: 'Find my station locations' }))

    // Each choice names the number, the profile and the callsign — the operator recognises
    // their own station by the name, and the number is what the field wants.
    const home = await screen.findByRole('button', { name: /Home/ })
    expect(screen.getByRole('button', { name: /Portable/ })).toBeTruthy()
    fireEvent.click(home)

    // Scoped to the Cloudlog field the picker belongs to — the panel has other numeric inputs.
    await waitFor(() => {
      const group = screen
        .getByRole('button', { name: 'Find my station locations' })
        .closest('label.settings-field') as HTMLElement
      const field = group.querySelector('input[inputmode="numeric"]') as HTMLInputElement
      expect(field.value).toBe('3')
    })
  })

  it('says what to do when the instance is too old for the endpoint', async () => {
    api.get('getCloudlogStations').mockImplementation(() =>
      Promise.reject(
        new Error(
          'this Cloudlog/Wavelog has no station_info endpoint (an older version) — enter the station location number by hand',
        ),
      ),
    )
    await openLogging()
    fireEvent.click(screen.getByRole('button', { name: 'Find my station locations' }))
    expect(await screen.findByText(/enter the station location number by hand/i)).toBeTruthy()
  })

  it('says so when the instance has no station locations at all', async () => {
    api.get('getCloudlogStations').mockImplementation(() => Promise.resolve([]))
    await openLogging()
    fireEvent.click(screen.getByRole('button', { name: 'Find my station locations' }))
    expect(await screen.findByText(/no station locations/i)).toBeTruthy()
  })
})
