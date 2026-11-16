// @vitest-environment jsdom
//
// Record CAT traffic: a diagnostics switch in Settings ▸ Services, beside the diagnostic log. It is
// off unless the backend says a trace is running (it is never a setting, so it is off at every
// launch), it turns on and off through the backend, and it shows the minutes the trace has left.
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
    mycall: 'W1AW', mygrid: 'FN31', activeRadio: 0, radios: [RADIO],
    band: '20m', dialMhz: 14.074, sideband: 'USB',
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

function mockSettings() {
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsFixture()))
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

const label = 'Record CAT traffic'
const switchOf = async () => {
  const name = await screen.findByText(label)
  const sw = name.closest('.settings-field')?.querySelector('[role="switch"]')
  expect(sw, 'the switch sits with its label').toBeTruthy()
  return sw as HTMLElement
}

/** The backend's trace: `secs` left, 0 = off; `cat_trace` starts it at 30 minutes or stops it. */
function backend(secs: number) {
  let left = secs
  api.get('catTraceStatus').mockImplementation(() => Promise.resolve(left))
  api.get('catTrace').mockImplementation((on: boolean) => {
    left = on ? 1800 : 0
    return Promise.resolve(left)
  })
}

const openServices = async () => {
  mockSettings()
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Services' }))
}

describe('Record CAT traffic', () => {
  it('is off when no trace is running, and says what it records', async () => {
    backend(0)
    await openServices()
    expect((await switchOf()).getAttribute('aria-checked')).toBe('false')
    expect(screen.getByText(/frequency, mode, split, VFO and PTT commands/)).toBeTruthy()
    expect(screen.getByText(/Turns itself off after 30 minutes or 2 MB/)).toBeTruthy()
    // It records what Nexus asks for, and says it cannot see Hamlib's own bytes to the radio.
    expect(screen.getByText(/does not see the bytes Hamlib then writes/)).toBeTruthy()
  })

  it('turns on through the backend and shows the minutes left', async () => {
    backend(0)
    await openServices()
    fireEvent.click(await switchOf())
    expect(api.get('catTrace')).toHaveBeenCalledWith(true)
    expect(await screen.findByText(/Recording CAT traffic: 30 minutes left/)).toBeTruthy()
    expect((await switchOf()).getAttribute('aria-checked')).toBe('true')
  })

  it('shows a trace that is already running, down to its last minute', async () => {
    backend(42)
    await openServices()
    expect(await screen.findByText(/Recording CAT traffic: 1 minute left/)).toBeTruthy()
    expect((await switchOf()).getAttribute('aria-checked')).toBe('true')
  })

  it('turns off through the backend', async () => {
    backend(600)
    await openServices()
    await screen.findByText(/10 minutes left/)
    fireEvent.click(await switchOf())
    expect(api.get('catTrace')).toHaveBeenCalledWith(false)
    await waitFor(async () => expect((await switchOf()).getAttribute('aria-checked')).toBe('false'))
    expect(screen.queryByText(/minutes left/)).toBeNull()
  })
})
