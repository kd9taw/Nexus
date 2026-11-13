// @vitest-environment jsdom
//
// #487 — the Tune timeout (the auto-release of a key-down tune carrier) applies to Tune on every
// screen, not only FT8, but it sat in Settings ▸ Digital ▸ Digital (FT8/FT4) ▸ Transmit &
// Sequencing, where an operator looking for it on the Radio tab did not find it. It now sits in
// Settings ▸ Radio ▸ Transmit limits & sharing, right after the high-SWR cutoff, under the same
// storage key (`tuneTimeoutSecs`) and with the same save rule (cleared = the 12 s default).
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n/en'

// THE BUDGET (2026-10-09). The slowest case here, "offers the switch, OFF, and says what it watches and…", takes
// 0.26 s and 0.29 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
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

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function station(extra: Record<string, unknown> = {}) {
  return { ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', ...extra } as never
}

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

async function openRadioTab() {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  // The fieldset the switch lives in, so an absent switch is not an absent tab.
  await waitFor(() => expect(document.getElementById('settings-transmit-limits')).toBeTruthy())
}

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
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.15.0'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(station()))
})

describe('#487 the Tune timeout lives with the other transmit limits', () => {
  it('is in Radio ▸ Transmit limits, right after the high-SWR cutoff, and saves under tuneTimeoutSecs', async () => {
    api.get('getSettings').mockImplementation(() => Promise.resolve(station({ tuneTimeoutSecs: 20 })))
    renderPanel()
    await openRadioTab()
    const limits = document.getElementById('settings-transmit-limits')!
    const input = limits.querySelector<HTMLInputElement>('[data-setting="tuneTimeoutSecs"]')
    expect(input, 'the Tune timeout is not in Transmit limits').not.toBeNull()
    expect(input!.value).toBe('20')
    expect(screen.getByLabelText(EN['settings.transmit.tuneTimeout.label'])).toBe(input)
    // Beside the SWR cutoff: the field right before it is the cutoff's.
    const field = input!.closest('.settings-field')!
    expect(field.previousElementSibling?.querySelector('[data-setting="swrStopEnabled"]'), 'not next to the SWR cutoff').not.toBeNull()
    fireEvent.change(input!, { target: { value: '30' } })
    clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(lastSaved().tuneTimeoutSecs).toBe(30)
  })

  it('a cleared box saves the 12 s default, never 0', async () => {
    renderPanel()
    await openRadioTab()
    const input = document.querySelector<HTMLInputElement>('#settings-transmit-limits [data-setting="tuneTimeoutSecs"]')!
    fireEvent.change(input, { target: { value: '' } })
    clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(lastSaved().tuneTimeoutSecs).toBe(12)
  })

  it('is no longer on the Digital tab, so there is one Tune timeout', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Digital' }))
    await waitFor(() => expect(document.getElementById('settings-digital-ft8-ft4')).toBeTruthy())
    expect(document.querySelectorAll('[data-setting="tuneTimeoutSecs"]')).toHaveLength(0)
  })
})
