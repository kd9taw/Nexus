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

async function openWorkspace() {
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
  return document.getElementById('settings-workspace')
}

describe('Logbook globe switch (D#278)', () => {
  it('sits in Appearance ▸ Workspace, on by default, and turning it off writes the setting', async () => {
    const workspace = await openWorkspace()
    const box = screen.getByRole('checkbox', { name: 'Show the 3-D globe above the Logbook' }) as HTMLInputElement
    expect(workspace?.contains(box), 'the switch is not inside the Workspace section').toBe(true)
    expect(box.checked).toBe(true)
    fireEvent.click(box)
    expect(box.checked).toBe(false)
    expect(localStorage.getItem(LOGBOOK_GLOBE_KEY)).toBe('off')
    fireEvent.click(box)
    expect(localStorage.getItem(LOGBOOK_GLOBE_KEY)).toBe('on')
  })
})
