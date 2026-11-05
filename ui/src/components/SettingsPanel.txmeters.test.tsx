// @vitest-environment jsdom
//
// #343 — Settings ▸ Workspace carries the TX meters' look: Bars or Needle. Clamping and the default
// are the hook's job (txMeterStyle.test.ts); this pins that the control exists where the registry
// says it does, shows the current choice, and that a pick reaches the meters' store.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FeaturesApi } from '../useFeatures'
import { EN } from '../i18n'
import { TX_METER_STYLE_KEY } from '../txMeterStyle'

// THE BUDGET (2026-10-09). The slowest case here, "is not offered by a host that does not wire it", takes 0.35 s and
// 0.42 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one core,
// past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
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

const features = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function renderPanel() {
  return render(
    <SettingsPanel
      target="workspace"
      activeRadioId={0}
      scale={1 as never}
      scaleMode={'auto' as never}
      scaleCap={1 as never}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density={'standard' as never}
      onDensityChange={() => {}}
      onResetLayout={() => {}}
      features={features}
    />,
  )
}

beforeEach(() => {
  // The panel maps over these on first render and waits for real settings before drawing a tab
  // (the SettingsPanel.deeplink.test.tsx setup): a bare `null` leaves it on its loading state.
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.2.6'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

const label = EN['settings.workspace.txMeters.label']

describe('#343 Settings ▸ Workspace ▸ TX meters', () => {
  it('shows Bars by default, and a pick of Needle is stored and shown', async () => {
    renderPanel()
    const g = await screen.findByRole('group', { name: label })
    const bars = within(g).getByRole('button', { name: EN['settings.workspace.txMeters.bars'] })
    const needle = within(g).getByRole('button', { name: EN['settings.workspace.txMeters.needle'] })
    expect(bars.getAttribute('aria-pressed')).toBe('true')
    expect(needle.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(needle)
    expect(localStorage.getItem(TX_METER_STYLE_KEY)).toBe('needle')
    expect(needle.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(bars)
    expect(localStorage.getItem(TX_METER_STYLE_KEY)).toBe('bars')
    expect(bars.getAttribute('aria-pressed')).toBe('true')
  })

  it('shows a stored Needle as the current choice', async () => {
    localStorage.setItem(TX_METER_STYLE_KEY, 'needle')
    renderPanel()
    const g = await screen.findByRole('group', { name: label })
    expect(within(g).getByRole('button', { name: EN['settings.workspace.txMeters.needle'] }).getAttribute('aria-pressed')).toBe('true')
  })
})
