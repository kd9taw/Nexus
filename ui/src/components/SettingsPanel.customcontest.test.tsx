// @vitest-environment jsdom
//
// SETTINGS ▸ CONTESTING and the operator's own contests: they are on the contest picker by the names
// the operator gave them, the picked one offers Edit…, Export… and Delete, and what its exchange has
// the operator TYPE (an age) is a box in Your station data, saved per contest in `contestEntered` —
// on this computer only, never in the contest's shared definition.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { CustomContest } from '../api'
import { setCustomContests } from '../features/customContests'
import defaultSettings from './__fixtures__/defaultSettings.json'

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

const SPRINT: CustomContest = {
  def: {
    format: 'nexus-custom-contest',
    version: 1,
    id: 'custom_club_sprint',
    name: 'Club Sprint',
    cabrillo_contest: 'CLUB-SPRINT',
    modes: ['CW'],
    window: { kind: 'weekend', month: 3, weekend: 'nth_full', n: 1, start_hour_utc: 14, duration_hours: 4 },
    exchange: [{ field: 'rst' }, { field: 'age' }],
    dupe: 'band',
  },
  enteredSlots: ['AGE'],
  sentSettings: [],
  exchangeLocked: false,
}

function renderPanel() {
  return render(
    <SettingsPanel
      fieldDay={null as never}
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

let stored: Record<string, unknown> = {}

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  setCustomContests([])
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.3'))
  api.get('listCustomContests').mockImplementation(() => Promise.resolve([structuredClone(SPRINT)]))
  stored = { ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', fdEvent: 'custom_club_sprint', contestEntered: { custom_other: { NAME: 'ALEX' } } }
  api.get('getSettings').mockImplementation(() => Promise.resolve({ ...stored } as never))
  api.get('setSettings').mockImplementation((s: unknown) => {
    stored = { ...(s as Record<string, unknown>) }
    return Promise.resolve({} as never)
  })
})
afterEach(cleanup)

const openContesting = async () => fireEvent.click(await screen.findByRole('tab', { name: 'Contesting' }))
const inputFor = async (label: string) =>
  (await screen.findByText(label, { selector: '.settings-label' })).closest('.settings-field')!.querySelector('input') as HTMLInputElement
const lastSaved = () => {
  const calls = api.get('setSettings').mock.calls
  return calls.length ? (calls[calls.length - 1][0] as Record<string, unknown>) : null
}

describe("the operator's own contests in Settings", () => {
  it('are on the contest picker by their own names, the picked one with Edit…, Export… and Delete', async () => {
    renderPanel()
    await openContesting()
    const chip = await screen.findByRole('button', { name: 'Club Sprint' })
    expect(chip.getAttribute('aria-pressed')).toBe('true')
    const field = chip.closest('.settings-field')!
    for (const name of ['Edit…', 'Export…', 'Delete']) {
      expect([...field.querySelectorAll('button')].some((b) => b.textContent === name), name).toBe(true)
    }
  })

  it('⭐ a typed age is saved per contest in contestEntered, beside the other contests\' values, and read back', async () => {
    const view = renderPanel()
    await openContesting()
    fireEvent.change(await inputFor('Your age'), { target: { value: '42' } })
    fireEvent.click(await screen.findByRole('button', { name: /^Save$/i }))
    await waitFor(() => expect(lastSaved()).not.toBeNull())
    expect(lastSaved()!.contestEntered).toEqual({ custom_other: { NAME: 'ALEX' }, custom_club_sprint: { AGE: '42' } })
    view.unmount()
    renderPanel()
    await openContesting()
    expect((await inputFor('Your age')).value).toBe('42')
  })

  it('a shipped contest asks no typed value', async () => {
    stored = { ...stored, fdEvent: 'ilqp' }
    renderPanel()
    await openContesting()
    await screen.findByRole('button', { name: 'Club Sprint' })
    expect(screen.queryByText('Your age', { selector: '.settings-label' })).toBeNull()
  })
})
