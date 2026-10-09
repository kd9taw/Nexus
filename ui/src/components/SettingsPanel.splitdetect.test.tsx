// @vitest-environment jsdom
//
// "Follow the radio's split" (`splitDetectEnabled`): the switch Settings never had.
//
// The setting shipped in 1.9.1 as "new in Settings, off by default", and the radio loop and the
// licence gate have honoured it since, but no control ever reached the panel: the only way to turn
// it on was to edit settings.json. These hold the switch to what the release said it was: on Radio
// ▸ Rig & CAT, off for a file that predates it, showing a stored value, and saved when changed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET (2026-10-09). The slowest case here, "turning it on is saved as splitDetectEnabled: true, and…", takes
// 0.27 s and 0.31 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
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

// Every export of `../api`, derived from the real module (a hand-kept list throws on mount the
// day a verb is added).
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

/** The backend's settings with `splitDetectEnabled` set to `stored`, or absent altogether, as
 *  in a settings file written before the field existed. */
function settingsFixture(stored: boolean | undefined) {
  const s: Record<string, unknown> = {
    ...defaultSettings,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
  }
  delete s.splitDetectEnabled
  if (stored !== undefined) s.splitDetectEnabled = stored
  return s as never
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function renderPanel(settings: never) {
  api.get('getSettings').mockImplementation(() => Promise.resolve(settings))
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
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const NAME = "Follow the radio's split"

async function theSwitch() {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  return (await screen.findByRole('switch', { name: NAME })) as HTMLButtonElement
}

function clickSave() {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
}

async function savedPayload(): Promise<Record<string, unknown>> {
  await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
  const calls = api.get('setSettings').mock.calls
  return calls[calls.length - 1][0] as Record<string, unknown>
}

describe("Follow the radio's split — the Settings switch", () => {
  it('is on Radio ▸ Rig & CAT, and off for a settings file written before the setting', async () => {
    renderPanel(settingsFixture(undefined))
    const sw = await theSwitch()
    expect(sw.getAttribute('aria-checked'), 'an old file loads as today: off').toBe('false')
    expect(sw.closest('#settings-rig-control'), 'it lives in the Rig & CAT section').not.toBeNull()
    // The help line says what it does and what it costs, beside the switch.
    const help = sw.closest('.settings-field')?.querySelector('.settings-hint')?.textContent ?? ''
    expect(help).toMatch(/split you set at the radio/)
    expect(help).toMatch(/never asked/)
  })

  it('shows a stored value', async () => {
    renderPanel(settingsFixture(true))
    expect((await theSwitch()).getAttribute('aria-checked')).toBe('true')
  })

  it('turning it on is saved as splitDetectEnabled: true, and off again as false', async () => {
    renderPanel(settingsFixture(false))
    const sw = await theSwitch()
    fireEvent.click(sw)
    expect(sw.getAttribute('aria-checked')).toBe('true')
    clickSave()
    expect((await savedPayload()).splitDetectEnabled).toBe(true)

    api.get('setSettings').mockClear()
    fireEvent.click(sw)
    expect(sw.getAttribute('aria-checked')).toBe('false')
    clickSave()
    expect((await savedPayload()).splitDetectEnabled).toBe(false)
  })
})
