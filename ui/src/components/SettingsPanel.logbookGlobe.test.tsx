// @vitest-environment jsdom
//
// D#278 — the Logbook globe switch is ON SCREEN where the registry says it lives
// (Settings ▸ Appearance ▸ Workspace), and flipping it writes the setting the Logbook reads.
// Renders the real panel and walks to the tab the way an operator does (the language picker's
// test explains why a control on an unrendered tab must not count as "found").
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { LOGBOOK_GLOBE_KEY } from '../features/logbookGlobe'

// THE BUDGET (2026-10-09). The slowest case here, "sits in Appearance ▸ Map & globe, on by default, and…", takes
// 0.23 s and 0.27 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
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

const RADIO = {
  id: 0, name: 'FTDX10', enabled: true, serialPort: 'COM3', baud: 38400, rigModel: 1042,
  rigModelName: 'Yaesu FTDX10', rigConn: 'serial', rigAddr: '', rigctldPort: 4532,
  rotctldPort: 4533, icomNativeCat: false, audioIn: 'in-0', audioOut: 'out-0', txLevel: 1,
  rxGain: 1, pttMethod: 'cat', rotatorModel: 0, rotatorPort: '', rotatorBaud: 9600,
  rotatorHost: '', nativeScope: 'auto', bands: [],
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

beforeEach(() => {
  localStorage.clear()
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.13.0'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, ...RADIO, mycall: 'KD9TAW', mygrid: 'EN52', activeRadio: 0, radios: [RADIO] }),
  )
})
afterEach(cleanup)

async function openMapGlobe() {
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
  await screen.findByRole('tab', { name: /appearance/i })
  fireEvent.click(screen.getByRole('tab', { name: /appearance/i }))
  return document.getElementById('settings-map-globe')
}

describe('Logbook globe switch (D#278)', () => {
  it('sits in Appearance ▸ Map & globe, on by default, and turning it off writes the setting', async () => {
    // Map & globe, not Workspace, since the Display sections (2026-09-26).
    const section = await openMapGlobe()
    const box = screen.getByRole('checkbox', { name: 'Show the 3-D globe above the Logbook' }) as HTMLInputElement
    expect(section?.contains(box), 'the switch is not inside the Map & globe section').toBe(true)
    expect(box.checked).toBe(true)
    fireEvent.click(box)
    expect(box.checked).toBe(false)
    expect(localStorage.getItem(LOGBOOK_GLOBE_KEY)).toBe('off')
    fireEvent.click(box)
    expect(localStorage.getItem(LOGBOOK_GLOBE_KEY)).toBe('on')
  })
})
