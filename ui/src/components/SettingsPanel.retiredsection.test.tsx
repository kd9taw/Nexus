// @vitest-environment jsdom
//
// SETTINGS ▸ CONTESTING ▸ ARRL SECTION — what an operator whose saved section ARRL has
// RETIRED reads, at the field where they have to fix it.
//
// The section list moved from a pre-2017 83 to the sponsor's current 85. Two of the three
// codes that went are renames ARRL publishes as such (`GTA`→`GH`, `NT`→`TER`) and
// `Settings::load` applies them before this panel ever sees them. The third is `MAR`: the
// Maritime section SPLIT into `NB`/`NS`/`PE`, nothing can know which of three provinces
// its operator is in, so the stored value is left alone and the operator is ASKED.
//
// ⚠️ **Being asked is only acceptable if the question explains itself.** To that operator
// `MAR` was a valid section the last time they ran Field Day, so the generic
// "isn't a known ARRL/RAC section" reads as the app being broken rather than as something
// they can fix. This file pins the sentence that replaces it — and pins that it is a
// SEPARATE branch, by holding the generic message in place for a code that never existed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
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

/** Mount the panel with `fdSection` already on disk, as an upgrade would find it. */
const withStoredSection = async (fdSection: string) => {
  api
    .get('getSettings')
    .mockImplementation(() =>
      Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', fdSection } as never),
    )
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Contesting' }))
  return (await screen.findByText('ARRL Section', { selector: '.settings-label' })).closest(
    '.settings-field',
  ) as HTMLElement
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.3'))
})
afterEach(cleanup)

describe('a saved section ARRL has retired', () => {
  it('⭐ says what the section WAS and which codes replaced it', async () => {
    const field = await withStoredSection('MAR')
    const warn = field.querySelector('.fd-section-warn') as HTMLElement
    expect(warn, 'a retired section must be flagged').toBeTruthy()
    // The stored code, so they recognise which value is the problem…
    expect(warn.textContent).toContain('MAR')
    // …the section's NAME, which is what they actually know it by…
    expect(warn.textContent).toContain('Maritime')
    // …and every code that replaced it, because choosing is now their job.
    for (const successor of ['NB', 'NS', 'PE']) {
      expect(warn.textContent, `names ${successor}`).toContain(successor)
    }
    // The field is still editable — it explains, it does not lock them out.
    expect((field.querySelector('input') as HTMLInputElement).disabled).toBe(false)
  })

  it('keeps the stored value in the box rather than blanking it', async () => {
    const field = await withStoredSection('MAR')
    // Blanking would destroy the only evidence of where they were, and would leave them
    // the generic "set your section" message with nothing to say what changed.
    expect((field.querySelector('input') as HTMLInputElement).value).toBe('MAR')
  })

  it('⭐ CONTROL: a code that never existed still gets the generic message', async () => {
    const field = await withStoredSection('ZZ')
    const warn = field.querySelector('.fd-section-warn') as HTMLElement
    expect(warn, 'an unknown code is still flagged').toBeTruthy()
    expect(warn.textContent).toContain("isn't a known ARRL/RAC section")
    // The retired wording must NOT appear — otherwise the first test would pass on a
    // message that says the same thing to everybody.
    expect(warn.textContent).not.toContain('replaced by')
  })

  it('CONTROL: a current section is not flagged at all', async () => {
    const field = await withStoredSection('WI')
    expect(field.querySelector('.fd-section-warn')).toBeNull()
  })

  it('CONTROL: a section from the Maritime split is current and not flagged', async () => {
    const field = await withStoredSection('NS')
    expect(field.querySelector('.fd-section-warn')).toBeNull()
  })
})
