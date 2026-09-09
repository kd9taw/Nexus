// @vitest-environment jsdom
//
// SETTINGS ▸ CONTESTING ▸ YOUR STATION DATA — the block a SENT exchange comes from (§3.4).
//
// Before it, `mycall`/`mygrid` plus `fdClass`/`fdSection` were the whole of "where I am",
// so a QSO party's in-state role — which sends the operator's COUNTY — had nothing to
// send, and Sweepstakes' check had no source anywhere in the build. The rules loader now
// refuses a ruleset naming a sent slot whose source is not one of these fields, which
// makes the gap a red gate rather than an operator who cannot fill their own exchange.
//
// ⚠️ **A settings key lives in more than one place** and `tsc` covers exactly one of
// them: the Rust struct, the TS interface, the panel and the default fixture are four
// separate sites. What this file proves is the half tsc cannot — that a value typed into
// the panel reaches the `set_settings` payload, and comes back when the form reloads.
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

// Mock every export of `../api`, derived from the real module — a hand-kept list made the
// panel throw on mount when it fell behind, which reads as a behaviour regression.
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

/** The settings the panel is told are on disk. */
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.3'))
  stored = { ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' }
  api.get('getSettings').mockImplementation(() => Promise.resolve({ ...stored } as never))
  // A save persists, exactly as the backend does — which is what lets the SECOND mount
  // below be a real round trip rather than a re-read of the same fixture.
  api.get('setSettings').mockImplementation((s: unknown) => {
    stored = { ...(s as Record<string, unknown>) }
    return Promise.resolve({} as never)
  })
})
afterEach(cleanup)

const openContesting = async () =>
  fireEvent.click(await screen.findByRole('tab', { name: 'Contesting' }))

/** The input a labelled field owns. */
const inputFor = async (label: string) =>
  (await screen.findByText(label, { selector: '.settings-label' }))
    .closest('.settings-field')!
    .querySelector('input') as HTMLInputElement

const save = async () => fireEvent.click(await screen.findByRole('button', { name: /^Save$/i }))

const lastSaved = () => {
  const calls = api.get('setSettings').mock.calls
  return calls.length ? (calls[calls.length - 1][0] as Record<string, unknown>) : null
}

/** Every field of §3.4's block, with a value and the label it is entered under. */
const BLOCK = [
  { key: 'contestQthCounty', label: 'County', typed: 'FRAN', want: 'FRAN' },
  { key: 'contestQthState', label: 'State or province', typed: 'WI', want: 'WI' },
  { key: 'contestCheck', label: 'Check', typed: '74', want: '74' },
  { key: 'contestCqZone', label: 'CQ zone', typed: '4', want: 4 },
  { key: 'contestItuZone', label: 'ITU zone', typed: '8', want: 8 },
  { key: 'contestPower', label: 'Power sent', typed: 'KW', want: 'KW' },
] as const

describe('the station-data block', () => {
  it('renders under the contest picker and above Field Day Setup', async () => {
    renderPanel()
    await openContesting()
    const legends = [...document.querySelectorAll('fieldset.settings-section legend')].map(
      (n) => n.textContent,
    )
    expect(legends).toContain('Your station data')
    expect(legends.indexOf('Your station data')).toBeGreaterThan(legends.indexOf('Contest'))
    expect(legends.indexOf('Your station data')).toBeLessThan(
      legends.indexOf('Field Day Setup'),
    )
  })

  it('offers every field §3.4 names, and every one of them is empty by default', async () => {
    renderPanel()
    await openContesting()
    for (const f of BLOCK) {
      const el = await inputFor(f.label)
      expect(el, `${f.label} must be on the tab`).toBeTruthy()
      // Nothing here can be guessed, and a guessed exchange goes on the air.
      expect(el.value, `${f.label} starts empty`).toBe('')
    }
  })

  it('⭐ carries every value into the save payload and back out of a reload', async () => {
    const view = renderPanel()
    await openContesting()
    for (const f of BLOCK) {
      fireEvent.change(await inputFor(f.label), { target: { value: f.typed } })
    }
    await save()
    await waitFor(() => expect(lastSaved()).not.toBeNull())

    const sent = lastSaved()!
    for (const f of BLOCK) {
      expect(sent[f.key], `${f.key} in the save payload`).toBe(f.want)
    }
    // ⚠️ The frozen names ride along untouched — §8(c). The `contest_*` block was added
    // BESIDE them; a save that dropped one would silently reset a shipped Field Day
    // setup, which is the exact hazard renaming a persisted serde field carries.
    expect(sent.fdClass).toBe(defaultSettings.fdClass)
    expect(sent.fdSection).toBe(defaultSettings.fdSection)
    expect(sent.fdEvent).toBe(defaultSettings.fdEvent)

    // THE ROUND TRIP. Mount again against what the save persisted: a key that reached
    // the payload but not the form on reload is still a key that does not work.
    view.unmount()
    renderPanel()
    await openContesting()
    for (const f of BLOCK) {
      expect((await inputFor(f.label)).value, `${f.key} after a reload`).toBe(f.typed)
    }
  })

  it('clamps a zone instead of sending a number that is not a zone', async () => {
    renderPanel()
    await openContesting()
    // `<input type="number" max>` is advisory in every browser — typing past it is
    // allowed, and the value would go on the air inside an exchange.
    fireEvent.change(await inputFor('CQ zone'), { target: { value: '99' } })
    fireEvent.change(await inputFor('ITU zone'), { target: { value: '400' } })
    await save()
    await waitFor(() => expect(lastSaved()).not.toBeNull())
    expect(lastSaved()!.contestCqZone).toBe(40)
    expect(lastSaved()!.contestItuZone).toBe(90)
    // POSITIVE CONTROL: a legal zone is not clamped, or the rule is just "always 40".
    fireEvent.change(await inputFor('CQ zone'), { target: { value: '5' } })
    await save()
    await waitFor(() => expect(lastSaved()!.contestCqZone).toBe(5))
  })
})
