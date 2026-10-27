// @vitest-environment jsdom
//
// SETTINGS ON A COMPUTER DRIVING A STATION, where two things are the station's alone. The Remote
// card's station half (this station's Remote service, its browsers, its own network listener) asked
// the station and was refused: it said "Check the connection and your service access", and the
// network switch sat off and disabled with no reason. The Connections log is never read from afar
// (its lines carry the services' own words), so it sat empty, saying no events had happened yet.
// Each now says it is at the station. At the station itself nothing changes.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { EN } from '../i18n'
import defaultSettings from './__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 60_000 })

const driving = vi.hoisted(() => ({ on: false }))
vi.mock('../features/durableStore', async (actual) => ({
  ...(await actual<typeof import('../features/durableStore')>()),
  drivesAnotherStation: () => driving.on,
}))
vi.mock('../platform', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  IS_WINDOWS: true,
}))
vi.mock('../remote-native/RemoteStation', () => ({ RemoteStation: () => <div data-testid="remote-station" /> }))
vi.mock('../remote-native/LanStation', () => ({ LanStation: () => <div data-testid="lan-station" /> }))
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? vi.fn(() => Promise.resolve(null)) : actual[name]
  }
  return {
    ...mod,
    getSettings: vi.fn(() => Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW' })),
    getRigModels: vi.fn(() => Promise.resolve([])),
    getAllRigModels: vi.fn(() => Promise.resolve([])),
    getSerialPortsDetailed: vi.fn(() => Promise.resolve([])),
    getBandPlan: vi.fn(() => Promise.resolve([])),
    getAudioDevices: vi.fn(() => Promise.resolve({ input: [], output: [] })),
    getCredentialsStatus: vi.fn(() => Promise.resolve([])),
    getConnectionLog: vi.fn(() => Promise.reject('That can only be done at the station, on its own screen.')),
    detectRigs: vi.fn(() => Promise.resolve([])),
    appVersion: vi.fn(() => Promise.resolve('1.2.6')),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

const features = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function open(target: string) {
  return render(
    <SettingsPanel
      target={target}
      activeRadioId={0}
      radio={{ swrScaleVerified: true } as never}
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
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  driving.on = false
})

describe('Settings on a computer driving a station', () => {
  it("the Remote card's station half says it is set up at the station", async () => {
    driving.on = true
    open('remote-access')
    expect(await screen.findByText(EN['settings.remote.atStation'])).not.toBeNull()
    expect(screen.queryByTestId('remote-station')).toBeNull()
    expect(screen.queryByTestId('lan-station')).toBeNull()
  })

  it('CONTROL: at the station the card is the station’s own', async () => {
    open('remote-access')
    expect(await screen.findByTestId('remote-station')).not.toBeNull()
    expect(screen.getByTestId('lan-station')).not.toBeNull()
    expect(screen.queryByText(EN['settings.remote.atStation'])).toBeNull()
  })

  it('the Connections log says it is kept at the station', async () => {
    driving.on = true
    open('connections')
    expect(await screen.findByText(EN['settings.connections.log.atStation'])).not.toBeNull()
    expect(screen.queryByText(EN['settings.connections.log.empty'])).toBeNull()
  })

  it('CONTROL: at the station an empty log says no events yet', async () => {
    open('connections')
    expect(await screen.findByText(EN['settings.connections.log.empty'])).not.toBeNull()
    expect(screen.queryByText(EN['settings.connections.log.atStation'])).toBeNull()
  })
})
