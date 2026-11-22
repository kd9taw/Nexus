// @vitest-environment jsdom
//
// The club event's own fields in Settings ▸ Contesting (Event name, Host port, Join event at) are named
// for a screen reader: each is drawn in a real <label> with its words, as every other Settings field is.
// In a <div> the words named nothing, and a screen reader read "edit text" or "spin button" alone.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n/en'

// The house budget for a Settings render under a loaded full suite.
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
// day a verb is missing from it).
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

async function openContesting() {
  fireEvent.click(await screen.findByRole('tab', { name: /contesting/i }))
  await waitFor(() => expect(document.getElementById('settings-field-day-club')).toBeTruthy())
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.17.0'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
})
afterEach(cleanup)

/** The field's name starts with its label's words (a wrapping label names it with its hint after them). */
const named = (key: keyof typeof EN) => new RegExp('^' + EN[key].replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))

describe('the club event fields have accessible names', () => {
  it('Event name, Host port and Join event at are each named by their label', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: /contesting/i }))
    await waitFor(() => expect(document.getElementById('settings-field-day-club')).toBeTruthy())
    const club = within(document.getElementById('settings-field-day-club')!)
    expect(club.getByRole('textbox', { name: named('settings.fdClub.eventName.label') })).toBeTruthy()
    expect(club.getByRole('spinbutton', { name: named('settings.fdClub.hostPort.label') })).toBeTruthy()
    expect(club.getByRole('textbox', { name: named('settings.fdClub.join.label') }).getAttribute('data-setting')).toBe('fdJoinAddr')
  })
})
