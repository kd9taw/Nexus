// @vitest-environment jsdom
//
// #253 — Settings ▸ Workspace carries the switch for the optional local clock beside UTC.
// Off by default is the hook's job (useLocalClock.test.ts); this pins that the control exists
// where the registry says it does, reflects the current choice, and reports a change.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FeaturesApi } from '../useFeatures'
import { EN } from '../i18n'

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

function renderPanel(localClock?: boolean, onLocalClockChange?: (on: boolean) => void) {
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
      localClock={localClock}
      onLocalClockChange={onLocalClockChange}
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
})

const group = () => screen.getByRole('group', { name: EN['settings.workspace.localClock.label'] })

describe('#253 Settings ▸ Workspace ▸ local time beside UTC', () => {
  it('shows the current choice and reports a change', async () => {
    const changed = vi.fn()
    renderPanel(false, changed)
    const g = await screen.findByRole('group', { name: EN['settings.workspace.localClock.label'] })
    const off = within(g).getByRole('button', { name: EN['settings.workspace.localClock.off'] })
    const on = within(g).getByRole('button', { name: EN['settings.workspace.localClock.on'] })
    expect(off.getAttribute('aria-pressed')).toBe('true')
    expect(on.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(on)
    expect(changed).toHaveBeenCalledWith(true)
  })

  it('is not offered by a host that does not wire it', async () => {
    renderPanel()
    // The Workspace section itself rendered (positive control), just without this field.
    await screen.findByRole('group', { name: EN['settings.workspace.density.aria'] })
    expect(() => group()).toThrow()
  })
})
