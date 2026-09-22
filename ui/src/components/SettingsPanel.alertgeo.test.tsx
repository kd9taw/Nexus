// @vitest-environment jsdom
//
// THE GEOGRAPHIC ALERT SCOPE'S SETTINGS CONTROLS (#174, barnburner6503: "filter the spots and
// alerts ... by country or continent ... instead of having all world spots available").
//
// The predicate itself is pinned in `features/dxccGeo.test.ts` and its effect on real alerts in
// `alerts.geo.test.ts`. What this file checks is the OPERATOR'S HALF, and the first case is the
// one that matters most: the fixture here is the shipped `defaultSettings.json`, which carries
// NEITHER new key — exactly the settings file every existing install already has on disk. If the
// panel came up with anything ticked, every one of those operators would silently lose alerts
// on upgrade.
//
// The second thing pinned here is the SAVE ROUND TRIP. A scope the panel shows but never writes
// is the failure this feature is most exposed to, because both fields are absent-by-default on
// the Rust side — the loud failure and the silent reset look identical from in here.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
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

const radio = {
  id: 0,
  name: 'Radio 1',
  enabled: true,
  serialPort: 'COM3',
  baud: 38400,
  rigModel: 3081,
  rigModelName: 'Icom IC-9700',
  rigConn: 'serial',
  rigAddr: '',
  rigctldPort: 4532,
  rotctldPort: 4533,
  icomNativeCat: true,
  audioIn: 'in-0',
  audioOut: 'out-0',
  txLevel: 0.25,
  rxGain: 1,
  pttMethod: 'cat',
  rotatorModel: 0,
  rotatorPort: '',
  rotatorBaud: 9600,
  rotatorHost: '',
  nativeScope: 'auto',
  bands: [],
}

/** ⚠️ Deliberately the shipped fixture, which carries NEITHER `alertContinents` nor
 *  `alertEntities` — the settings file of every install that predates this feature. */
function settingsDoc() {
  return {
    ...defaultSettings,
    ...radio,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [radio],
  } as never
}

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
      radio={undefined}
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

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsDoc()))
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.0.1'))
  api.get('getDxccEntityNames').mockImplementation(() =>
    Promise.resolve(['France', 'Fed. Rep. of Germany', 'Japan', 'United States']),
  )
})
afterEach(cleanup)

const CONTINENTS = [
  'Europe (EU)',
  'North America (NA)',
  'South America (SA)',
  'Africa (AF)',
  'Asia (AS)',
  'Oceania (OC)',
]

async function openAlerts() {
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Spots & Alerts' }))
  return await screen.findByRole('checkbox', { name: 'Europe (EU)' })
}

const box = (name: string) => screen.getByRole('checkbox', { name }) as HTMLInputElement

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

/**
 * "No scope" as it is actually written. A half the operator never touched stays ABSENT from the
 * saved document rather than becoming `[]`, and that is deliberate on both sides: the panel adds
 * no key to a settings.json that never had one, and the Rust field is `#[serde(default)]`, so an
 * absent list deserializes to an empty Vec. Absent and empty are the same statement — alert on
 * everywhere — and a test that demanded one spelling would be pinning an accident.
 */
const noScope = (v: unknown) => expect(v ?? []).toEqual([])

describe('the default is everywhere', () => {
  it('offers all six continents and ticks NONE of them for a settings file without the key', async () => {
    await openAlerts()
    // Positive control first: if the checkboxes were absent the "none ticked" claim below would
    // pass vacuously, which is the whole failure mode this line exists to rule out.
    for (const name of CONTINENTS) expect(box(name), name).toBeTruthy()
    for (const name of CONTINENTS) expect(box(name).checked, name).toBe(false)
  })

  it('saves an empty scope — nothing ticked must not become a scope of nothing', async () => {
    await openAlerts()
    // Touch an unrelated field so the form is dirty and the save actually runs.
    fireEvent.click(box('Europe (EU)'))
    fireEvent.click(box('Europe (EU)'))
    clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    // Ticked and unticked, so this half WAS written — and it came back to empty, not to a scope
    // admitting nothing.
    expect(lastSaved().alertContinents).toEqual([])
    noScope(lastSaved().alertEntities)
  })
})

describe('ticking continents', () => {
  it('ticks Europe alone and saves just EU', async () => {
    await openAlerts()
    fireEvent.click(box('Europe (EU)'))

    expect(box('Europe (EU)').checked).toBe(true)
    // The discriminator: a filter test where every box moves together cannot tell "ticked
    // Europe" from "ticked everything".
    expect(box('Asia (AS)').checked, 'Asia is untouched').toBe(false)

    clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(lastSaved().alertContinents).toEqual(['EU'])
  })

  it('stores in cty.dat order however they were ticked, and untick removes', async () => {
    await openAlerts()
    fireEvent.click(box('Asia (AS)'))
    fireEvent.click(box('Europe (EU)'))
    clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    // NA, SA, EU, AF, AS, OC — EU precedes AS, though AS was ticked first.
    expect(lastSaved().alertContinents).toEqual(['EU', 'AS'])

    fireEvent.click(box('Asia (AS)'))
    clickSave()
    await waitFor(() => expect(api.get('setSettings').mock.calls.length).toBeGreaterThan(1))
    expect(lastSaved().alertContinents).toEqual(['EU'])
  })
})

describe('adding a country by name', () => {
  it('searches the DXCC table and adds the chosen entity by its cty.dat name', async () => {
    await openAlerts()
    // Nothing is offered until the operator types — the table is not even fetched.
    expect(api.get('getDxccEntityNames')).not.toHaveBeenCalled()

    fireEvent.change(screen.getByPlaceholderText('type to search, e.g. France'), {
      target: { value: 'german' },
    })
    // Matched case-insensitively on the cty.dat spelling, which is the trap this control exists
    // to defuse: the operator says Germany, the table says "Fed. Rep. of Germany".
    const option = await screen.findByRole('option', { name: 'Fed. Rep. of Germany' })
    expect(option).toBeTruthy()

    fireEvent.change(screen.getByRole('listbox', { name: 'Matching DXCC entities' }), {
      target: { value: 'Fed. Rep. of Germany' },
    })

    expect(box('Fed. Rep. of Germany').checked, 'it appears in the chosen list').toBe(true)
    clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(lastSaved().alertEntities).toEqual(['Fed. Rep. of Germany'])
    // The entity half is independent of the continent half — they union, they do not replace,
    // and adding a country leaves the continent half untouched.
    noScope(lastSaved().alertContinents)
  })

  it('unticking a chosen country drops it', async () => {
    await openAlerts()
    fireEvent.change(screen.getByPlaceholderText('type to search, e.g. France'), {
      target: { value: 'japan' },
    })
    await screen.findByRole('option', { name: 'Japan' })
    fireEvent.change(screen.getByRole('listbox', { name: 'Matching DXCC entities' }), {
      target: { value: 'Japan' },
    })
    expect(box('Japan').checked).toBe(true)

    fireEvent.click(box('Japan'))
    clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(lastSaved().alertEntities).toEqual([])
  })
})
