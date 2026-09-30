// @vitest-environment jsdom
//
// #391: THE POP-UP SWITCH IN SETTINGS. InsaneSplash, 2026-09-29: "how do I turn off the popup
// notifications in the bottom right of the application?" The switch is Spots & Alerts ▸ Alerts ▸
// Pop-up notifications. What it does to the corner is App.popups.test.tsx and toast.test.ts; this
// file pins the operator's half. First: the fixture here is the shipped `defaultSettings.json`,
// which carries NO `popupNotifications`, the settings file of every existing install, and the switch
// must come up ON from it, or an upgrade would silence the corner where an error is said. Then the
// save round trip, both ways, because a switch the panel shows but never writes looks identical to
// one that works until the next launch.
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

const radio = {
  id: 0,
  name: 'Radio 1',
  enabled: true,
  serialPort: 'COM3',
  baud: 38400,
  rigModel: 3081,
  rigModelName: 'Icom IC-9700',
  rigConn: 'serial',
  rigAddr: '',
  rigctldPort: 4532,
  rotctldPort: 4533,
  icomNativeCat: true,
  audioIn: 'in-0',
  audioOut: 'out-0',
  txLevel: 0.25,
  rxGain: 1,
  pttMethod: 'cat',
  rotatorModel: 0,
  rotatorPort: '',
  rotatorBaud: 9600,
  rotatorHost: '',
  nativeScope: 'auto',
  bands: [],
}

/** ⚠️ Deliberately the shipped fixture, which carries no `popupNotifications` — the settings
 *  file of every install that predates the switch. */
function settingsDoc() {
  return {
    ...defaultSettings,
    ...radio,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [radio],
  } as never
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
      radio={undefined}
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
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsDoc()))
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.0.1'))
  api.get('getDxccEntityNames').mockImplementation(() =>
    Promise.resolve(['France', 'Fed. Rep. of Germany', 'Japan', 'United States']),
  )
})
afterEach(cleanup)

async function openAlerts() {
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Spots & Alerts' }))
  return await screen.findByText('Pop-up notifications')
}

const popupSwitch = () =>
  screen.getByText('Pop-up notifications').closest('label')?.querySelector('button[role="switch"]') as HTMLButtonElement

function clickSave() {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
}

const lastSaved = () => {
  const calls = api.get('setSettings').mock.calls
  return calls[calls.length - 1][0] as Record<string, unknown>
}

describe('the pop-up switch in Settings', () => {
  it('sits in the Alerts section, ON from a settings file that has never heard of it', async () => {
    const label = await openAlerts()
    expect(label.closest('fieldset')?.querySelector('legend')?.textContent).toBe('Alerts')
    expect(popupSwitch().getAttribute('aria-checked'), 'an old settings file turned the pop-ups off').toBe('true')
  })

  it('writes false when switched off and saved, and true when switched back on', async () => {
    await openAlerts()
    fireEvent.click(popupSwitch())
    expect(popupSwitch().getAttribute('aria-checked')).toBe('false')
    clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(lastSaved().popupNotifications, 'off never reached the saved document').toBe(false)
    fireEvent.click(popupSwitch())
    clickSave()
    await waitFor(() => expect(api.get('setSettings').mock.calls.length).toBeGreaterThan(1))
    expect(lastSaved().popupNotifications).toBe(true)
  })
})
