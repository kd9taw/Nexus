// @vitest-environment jsdom
//
// #378 (IZ5FSA): a Wavelog/Cloudlog on the LAN with no certificate. Plain http:// is accepted for
// an address on the operator's own network only, and the API key then travels unencrypted, so the
// Base URL field says so as soon as an http:// address is in it, and says nothing for https://.
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

function settingsFixture(url: string) {
  return {
    ...defaultSettings,
    ...RADIO,
    mycall: 'IZ5FSA', mygrid: 'JN53', activeRadio: 0, radios: [RADIO],
    band: '20m', dialMhz: 14.074, sideband: 'USB',
    cloudlogUrl: url, cloudlogStationId: '1',
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

function mockSettings(url: string) {
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsFixture(url)))
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

const NOTE = /travels unencrypted to this address/i

describe('Cloudlog/Wavelog over plain http on the LAN (#378)', () => {
  it('says, under the Base URL, that plain http sends the API key unencrypted', async () => {
    mockSettings('http://192.168.1.20/wavelog')
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Logging & Connectors' }))
    // Positive control: the Base URL field is on screen with the reporter's kind of address.
    const field = await screen.findByDisplayValue('http://192.168.1.20/wavelog')
    const note = screen.getByText(NOTE)
    expect(field.closest('.settings-field')?.contains(note), 'the note sits with the field').toBe(
      true,
    )
    expect(note.textContent).toMatch(/192\.168\.x\.x/)
  })

  it('says nothing for an https address', async () => {
    mockSettings('https://log.example.org')
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Logging & Connectors' }))
    expect(await screen.findByDisplayValue('https://log.example.org'), 'control').toBeTruthy()
    expect(screen.queryByText(NOTE)).toBeNull()
  })

  it('says it as soon as an http address is typed', async () => {
    mockSettings('https://log.example.org')
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Logging & Connectors' }))
    const field = await screen.findByDisplayValue('https://log.example.org')
    expect(screen.queryByText(NOTE), 'control: nothing before the edit').toBeNull()
    fireEvent.change(field, { target: { value: 'HTTP://wavelog.lan' } })
    expect(await screen.findByText(NOTE)).toBeTruthy()
  })
})
