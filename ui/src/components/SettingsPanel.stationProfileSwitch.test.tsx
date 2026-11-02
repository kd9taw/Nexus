// @vitest-environment jsdom
//
// "My station" profiles: Settings left open while the top bar switches the station to another
// profile. The Station tab's call (and grid, state, power) described the profile the station left;
// they are read again from the station, so the tab shows the call the station is on and a Save
// edits that profile. The rest of the form stays as the operator has it.
import { it, expect, vi, beforeEach, afterEach } from 'vitest'
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

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

const PROFILES = [
  { id: 'home', name: 'Home', call: 'KD9TAW', grid: 'EN52', state: 'WI' },
  { id: 'p1', name: 'Club', call: 'W9ABC', grid: 'EN43', state: 'WI' },
]

function station(active: 'home' | 'p1', opName = 'Op') {
  const p = PROFILES.find((x) => x.id === active)!
  return {
    ...defaultSettings,
    mycall: p.call,
    mygrid: p.grid,
    opState: p.state,
    opName,
    stationProfiles: PROFILES,
    activeStationProfile: active,
  } as never
}

function panel(activeStationProfile: string) {
  return (
    <SettingsPanel
      activeRadioId={0}
      activeStationProfile={activeStationProfile}
      scale={1 as never}
      scaleMode={'auto' as never}
      scaleCap={1 as never}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density={'comfortable' as never}
      onDensityChange={() => {}}
      onResetLayout={() => {}}
      features={features}
    />
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
  api.get('getSettings').mockImplementation(() => Promise.resolve(station('home')))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

it('a switch made while Settings is open shows the new profile\'s call, and keeps the rest of the form', async () => {
  const { rerender } = render(panel('home'))
  await screen.findByDisplayValue('KD9TAW')
  // The operator edits another field on the Station tab, then picks Club in the top bar.
  const name = screen.getByDisplayValue('Op') as HTMLInputElement
  fireEvent.change(name, { target: { value: 'Typed name' } })
  api.get('getSettings').mockImplementation(() => Promise.resolve(station('p1', 'Saved name')))
  rerender(panel('p1'))
  await waitFor(() => expect(screen.queryByDisplayValue('W9ABC')).not.toBeNull())
  expect(screen.queryByDisplayValue('KD9TAW'), 'the left profile\'s call is gone').toBeNull()
  expect(screen.queryByDisplayValue('EN43'), 'its grid too').not.toBeNull()
  expect(screen.queryByDisplayValue('Typed name'), 'the rest of the form is the operator\'s').not.toBeNull()
})

it('CONTROL: the same profile again reads nothing', async () => {
  const { rerender } = render(panel('home'))
  await screen.findByDisplayValue('KD9TAW')
  const reads = api.get('getSettings').mock.calls.length
  rerender(panel('home'))
  await new Promise((r) => setTimeout(r, 20))
  expect(api.get('getSettings').mock.calls.length).toBe(reads)
  expect(screen.queryByDisplayValue('KD9TAW')).not.toBeNull()
})
