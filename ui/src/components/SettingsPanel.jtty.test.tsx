// @vitest-environment jsdom
//
// Settings ▸ Digital ▸ PSK ▸ JTTY — the JTTY receiver's opt-out, PSK's twin. JTTY is the Keyboard
// section's other modem, so its one form setting sits in the PSK section (the Digital tab is at
// its section ceiling), findable by searching for JTTY. On by default, so an absent key reads as
// on; a click turns it off and the save writes `jttyRxAutoArm: false` (the Rust side round-trips
// the key: `jtty_settings_default_round_trip_and_wire_keys`). F Tol is NOT a form control: the
// JTTY cockpit is its one writer.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { searchSettings } from '../settings/registry'

// The house budget: a loaded full suite has run cases up to 20 times slower than one core.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module, so no verb can throw on mount.
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

let stored: Record<string, unknown> = {}

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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.2'))
  stored = { ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' }
  api.get('getSettings').mockImplementation(() => Promise.resolve(stored as never))
})
afterEach(cleanup)

async function openPsk(): Promise<HTMLFieldSetElement> {
  render(
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
  fireEvent.click(await screen.findByRole('tab', { name: 'Digital' }))
  await screen.findByText('Start receiving when JTTY opens')
  const fs = document.getElementById('settings-psk')
  expect(fs, 'no <fieldset id="settings-psk">').not.toBeNull()
  return fs as HTMLFieldSetElement
}

/** The switch in the same `<label>` as the label span. */
function toggle(fs: HTMLElement, label: string): HTMLElement {
  const span = within(fs)
    .getAllByText(label)
    .find((n) => n.tagName === 'SPAN' && n.classList.contains('settings-label'))
  expect(span, `no settings-label span "${label}"`).toBeDefined()
  const el = span!.closest('label')!.querySelector('button[role="switch"]')
  expect(el, `no switch for "${label}"`).not.toBeNull()
  return el as HTMLElement
}

async function clickSave() {
  const save = (await screen.findAllByRole('button', { name: 'Save' })).find(
    (b) => (b as HTMLButtonElement).type === 'submit',
  )!
  fireEvent.click(save)
}

describe('Settings ▸ Digital ▸ PSK ▸ JTTY', () => {
  it('is a JTTY group inside the PSK section, on by default, and a click saves it off', async () => {
    const fs = await openPsk()
    expect(within(fs).getByText('JTTY', { selector: '.settings-featgroup-title' })).toBeTruthy()
    const sw = toggle(fs, 'Start receiving when JTTY opens')
    expect(sw.getAttribute('aria-checked')).toBe('true')
    fireEvent.click(sw)
    expect(sw.getAttribute('aria-checked')).toBe('false')
    await clickSave()
    await waitFor(() =>
      expect(api.get('setSettings')).toHaveBeenCalledWith(expect.objectContaining({ jttyRxAutoArm: false })),
    )
    // Control: PSK's own switch in the same section is untouched by the JTTY one.
    expect(api.get('setSettings')).toHaveBeenCalledWith(expect.objectContaining({ pskRxAutoArm: true }))
  })

  it('reads a saved opt-out back as off, and an absent key as on', async () => {
    stored = { ...defaultSettings, jttyRxAutoArm: false }
    let fs = await openPsk()
    expect(toggle(fs, 'Start receiving when JTTY opens').getAttribute('aria-checked')).toBe('false')
    cleanup()
    const { jttyRxAutoArm: _gone, ...older } = defaultSettings as Record<string, unknown>
    stored = older
    fs = await openPsk()
    expect(toggle(fs, 'Start receiving when JTTY opens').getAttribute('aria-checked')).toBe('true')
  })

  it('is found by searching for JTTY', () => {
    expect(searchSettings('jtty').map((h) => h.section.id)).toContain('psk')
  })
})
