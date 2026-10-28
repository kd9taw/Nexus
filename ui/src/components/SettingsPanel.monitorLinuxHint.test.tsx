// @vitest-environment jsdom
//
// "Play receive audio here": the hint to pick the output by its card name (hw: or plughw:) is about
// Linux's device names alone. On Windows and macOS there are no card names to pick by, so it shows
// only on Linux.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { EN } from '../i18n'
import defaultSettings from './__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 60_000 })

const host = vi.hoisted(() => ({ linux: false }))
vi.mock('../platform', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  get IS_LINUX() {
    return host.linux
  },
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
    getConnectionLog: vi.fn(() => Promise.resolve([])),
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

function open() {
  return render(
    <SettingsPanel
      target="headphone-monitor"
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
  host.linux = false
})

describe('Play receive audio here: the card-name hint', () => {
  it('is not shown on Windows or macOS', async () => {
    open()
    expect(await screen.findByText(EN['settings.headphoneMonitor.device.hint'])).not.toBeNull()
    expect(screen.queryByText(EN['settings.headphoneMonitor.device.linuxHint'])).toBeNull()
  })

  it('CONTROL: on Linux it is shown', async () => {
    host.linux = true
    open()
    expect(await screen.findByText(EN['settings.headphoneMonitor.device.hint'])).not.toBeNull()
    expect(screen.getByText(EN['settings.headphoneMonitor.device.linuxHint'])).not.toBeNull()
  })
})
