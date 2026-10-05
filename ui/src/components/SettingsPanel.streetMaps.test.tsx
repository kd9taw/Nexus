// @vitest-environment jsdom
//
// THE STREET MAPS BLOCK IS WHERE THE RULING PUTS IT (2026-10-04, D5): Settings ▸ Appearance, in the
// section that holds the Logbook-globe switch (Map & globe), and only while the street map is offered.
// Renders the real panel and walks to the tab the way an operator does. The setup is the Logbook-globe
// test's, with the street-map commands answering.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { __resetStreetMapsForTests } from '../features/streetMaps'
import { t } from '../i18n'

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

const offer = (bench: boolean) => {
  __resetStreetMapsForTests()
  api.get('isTauri').mockImplementation(() => true as never)
  api.get('streetMapInfo').mockImplementation(() => Promise.resolve({ folder: '/maps', bench }))
  api.get('streetMapPacks').mockImplementation(() => Promise.resolve([]))
  api.get('streetMapUnfinished').mockImplementation(() => Promise.resolve([]))
}

describe('Settings ▸ Appearance ▸ Map & globe ▸ Street maps', () => {
  it('sits in Map & globe beside the Logbook-globe switch while the street map is offered', async () => {
    offer(true)
    const section = await openMapGlobe()
    const label = await screen.findByText(t('settings.streetMaps.label'))
    expect(section?.contains(label), 'the block is not inside the Map & globe section').toBe(true)
  })

  it('is not there while it is not offered', async () => {
    offer(false)
    const section = await openMapGlobe()
    expect(section, 'CONTROL: the section rendered').not.toBeNull()
    await act(async () => {})
    expect(screen.queryByText(t('settings.streetMaps.label'))).toBeNull()
  })
})
