// @vitest-environment jsdom
//
// START AT SIGN-IN HAS ONE WRITER, AND IT IS NOT THE FORM — and the switch moves only once the
// computer has agreed.
//
// `launchAtLogin` mirrors an operating-system login entry (the Windows Run key, a macOS
// LaunchAgent, an XDG autostart file). The backend changes that entry first and records the
// choice only if it worked, so the switch must wait for the answer: an optimistic flip would show
// "on" on a computer that refused. The backend half (no settings payload can move the field) is
// pinned in `crates/tempo-app/src/engine.rs`.
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

// Mock EVERY export of `../api`, derived from the real module (a hand-kept list makes the panel
// throw on mount when a verb is added).
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

function lastCall(name: string): unknown[] | undefined {
  const calls = api.get(name).mock.calls
  return calls.length ? (calls[calls.length - 1] as unknown[]) : undefined
}

function settingsFixture(launchAtLogin: boolean) {
  return { ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', launchAtLogin } as never
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

function setUp(launchAtLogin: boolean) {
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.11.1'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsFixture(launchAtLogin)))
}

beforeEach(() => setUp(false))
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('the start-at-sign-in switch', () => {
  it('is off by default, and turning it on calls setLaunchAtLogin(true), never setSettings', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Appearance' }))
    const off = await screen.findByRole('switch', { name: 'Turn on start at sign-in' })
    expect(off.getAttribute('aria-checked')).toBe('false')
    fireEvent.click(off)

    await waitFor(() => expect(api.get('setLaunchAtLogin')).toHaveBeenCalled())
    expect(lastCall('setLaunchAtLogin')![0]).toBe(true)
    const on = await screen.findByRole('switch', { name: 'Turn off start at sign-in' })
    expect(on.getAttribute('aria-checked')).toBe('true')
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })

  it('turning it off calls setLaunchAtLogin(false) — the switch stays two-way', async () => {
    setUp(true)
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Appearance' }))
    fireEvent.click(await screen.findByRole('switch', { name: 'Turn off start at sign-in' }))

    await waitFor(() => expect(api.get('setLaunchAtLogin')).toHaveBeenCalled())
    expect(lastCall('setLaunchAtLogin')![0]).toBe(false)
  })

  it('says so, and leaves the switch off, when the computer refuses', async () => {
    api.get('setLaunchAtLogin').mockImplementation(() => Promise.reject('launchAtLoginUnsupported'))
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Appearance' }))
    fireEvent.click(await screen.findByRole('switch', { name: 'Turn on start at sign-in' }))

    expect(await screen.findByText(/did not let Nexus change whether it starts at sign-in/)).toBeTruthy()
    expect(
      screen.getByRole('switch', { name: 'Turn on start at sign-in' }).getAttribute('aria-checked'),
    ).toBe('false')
  })
})
