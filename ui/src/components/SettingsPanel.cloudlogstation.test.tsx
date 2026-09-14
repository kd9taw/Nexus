// @vitest-environment jsdom
//
// #226 (DG3ET): Wavelog's station profile id is the NUMBER of a station location owned by the
// API key's user. The reporter typed his callsign, Wavelog answered 401 "station id does not
// belong to the API key owner", and nothing in Settings had ever said the field wants a number.
// A value that is not a number now gets an inline note right under the field.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
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

function settingsFixture(stationId: string) {
  return {
    ...defaultSettings,
    ...RADIO,
    mycall: 'DG3ET', mygrid: 'JO31', activeRadio: 0, radios: [RADIO],
    band: '20m', dialMhz: 14.074, sideband: 'USB',
    cloudlogUrl: 'https://log.example.org', cloudlogStationId: stationId,
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

function mockSettings(stationId: string) {
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsFixture(stationId)))
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
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const NOTE = /looks like a callsign/i

describe('Cloudlog/Wavelog station profile id (#226)', () => {
  it('notes, under the field, that a callsign is not the location number', async () => {
    mockSettings('DG3ET')
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Logging & Connectors' }))
    // Positive control: the field is on screen with the value the reporter typed.
    const field = await screen.findByDisplayValue('DG3ET', { selector: 'input[inputmode="numeric"]' })
    expect(field).toBeTruthy()
    const note = screen.getByText(NOTE)
    expect(field.closest('.settings-field')?.contains(note), 'the note sits with the field').toBe(true)
  })

  it('says nothing for a location number', async () => {
    mockSettings('3')
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Logging & Connectors' }))
    expect(await screen.findByDisplayValue('3', { selector: 'input[inputmode="numeric"]' })).toBeTruthy()
    expect(screen.queryByText(NOTE)).toBeNull()
  })

  it('says what the number is in the hint itself', async () => {
    mockSettings('')
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Logging & Connectors' }))
    expect(await screen.findByText(/station location number/i)).toBeTruthy()
  })
})
