// @vitest-environment jsdom
//
// SETTINGS ▸ CONTESTING — the contest picker and the role/category block (spec §9).
//
// Three things this owes an operator, and each has been wrong in a shipped build:
//
//  1. ONE contest picker, FIRST on the tab. The contest decides the exchange, the dupe
//     rule, the multiplier universe and the Cabrillo headers, so Field Day Setup is the
//     setup for whichever contest is picked — not the place the contest is chosen.
//  2. The ROLE is READ-ONLY. A role is derived from where the operator is; one that
//     could be typed independently of the location would let the two disagree.
//  3. `CATEGORY-OPERATOR` is a DECLARATION the operator makes. It shipped as the string
//     literal `MULTI-OP`, so every solo Field Day entry Nexus exported claimed more
//     than one operator was at the station.
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

// Mock EVERY export of `../api`, derived from the real module rather than a hand-kept list.
//
// The list was the problem: a verb missing from it made the panel THROW ON MOUNT ("No export is
// defined on the mock"), which presents as a behaviour regression in whichever test happened to
// run -- not as the out-of-date mock it actually is. Reading the real module's export names makes
// that failure impossible by construction.
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

function renderPanel(fieldDay: unknown = null) {
  return render(
    <SettingsPanel
      fieldDay={fieldDay as never}
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
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
})
afterEach(cleanup)

const openContesting = async () =>
  fireEvent.click(await screen.findByRole('tab', { name: 'Contesting' }))

/** The fieldset a control sits in, by its <legend>. */
const sectionOf = (el: HTMLElement) =>
  el.closest('fieldset')?.querySelector('legend')?.textContent ?? null

/** The chip group a labelled control owns. The section's <legend> can carry the same
 *  word as the control's own label, so match on the LABEL element specifically. */
const groupFor = async (label: string) =>
  (await screen.findByText(label, { selector: '.settings-label' })).closest('.settings-field')!

const chip = async (label: string, text: string) =>
  [...(await groupFor(label)).querySelectorAll('button')].find(
    (b) => b.textContent === text,
  ) as HTMLButtonElement


describe('the contest picker', () => {
  it('is the first section on the tab, and Field Day Setup comes after it', async () => {
    renderPanel()
    await openContesting()
    const legends = [...document.querySelectorAll('fieldset.settings-section legend')].map(
      (n) => n.textContent,
    )
    expect(legends[0]).toBe('Contest')
    expect(legends.indexOf('Field Day Setup')).toBeGreaterThan(0)
  })

  it('offers the shipped contests and picks the one the settings name', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({ ...defaultSettings, fdEvent: 'wfd' } as never),
    )
    renderPanel()
    await openContesting()
    const group = await groupFor('Contest')
    // ⭐ TWELVE contests: the two Field Day events, the four state QSO parties batch 8
    // shipped as rulesets and nobody could select, Sweepstakes' TWO weekends, and CQ WW's
    // and CQ WPX's two each — ARRL and CQ both run the CW and Phone runnings as separate
    // contests on separate weekends, with separate scores and separate Cabrillo tokens.
    // The names are the sponsors' own.
    expect([...group.querySelectorAll('button')].map((b) => b.textContent)).toEqual([
      'ARRL Field Day',
      'Winter Field Day',
      'ARRL November Sweepstakes (CW)',
      'ARRL November Sweepstakes (Phone)',
      'CQ World-Wide DX Contest (CW)',
      'CQ World-Wide DX Contest (SSB)',
      'CQ World-Wide WPX Contest (CW)',
      'CQ World-Wide WPX Contest (SSB)',
      'California QSO Party',
      'Ohio QSO Party',
      'Tennessee QSO Party',
      'Texas QSO Party',
    ])
    expect((await chip('Contest', 'Winter Field Day')).getAttribute('aria-pressed')).toBe('true')
  })

  it('picks a QSO party, and Field Day Setup drops the class and section it does not send', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({ ...defaultSettings, fdEvent: 'tnqp' } as never),
    )
    renderPanel()
    await openContesting()
    expect((await chip('Contest', 'Tennessee QSO Party')).getAttribute('aria-pressed')).toBe(
      'true',
    )
    // CLASS and SECTION are Field Day's own exchange. A Tennessee QSO Party operator
    // sends a county, from Station data — prompting for a Field Day class here would be
    // asking for a value nothing transmits.
    expect(screen.queryByPlaceholderText('1D')).toBeNull()
    // The ARRL/RAC section input, found by ITS OWN datalist rather than by the `WI`
    // placeholder — Station data's state box carries the same placeholder and is
    // exactly the control a QSO party operator does fill in.
    expect(document.querySelector('#fd-section-list')).toBeNull()
    // POSITIVE CONTROL: the same panel on a Field Day event still shows both.
    cleanup()
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({ ...defaultSettings, fdEvent: 'arrlfd' } as never),
    )
    renderPanel()
    await openContesting()
    expect(await screen.findByPlaceholderText('1D')).not.toBeNull()
    expect(document.querySelector('#fd-section-list')).not.toBeNull()
  })

  it('lives in ONE place — the picker is not also inside Field Day Setup', async () => {
    renderPanel()
    await openContesting()
    // Two controls writing one setting is how they come to disagree. The contest chips
    // exist exactly once on the tab.
    const chips = [...document.querySelectorAll('button.theme-chip')].filter(
      (b) => b.textContent === 'ARRL Field Day',
    )
    expect(chips).toHaveLength(1)
    expect(sectionOf(chips[0] as HTMLElement)).toBe('Contest')
  })
})

describe('the role / category block', () => {
  it('says there is no role to choose when the contest is symmetric', async () => {
    renderPanel({ role: '' })
    await openContesting()
    expect(
      await screen.findByText(/This contest works the same way for everyone/),
    ).not.toBeNull()
  })

  it('names the live role, read-only, when the session has one', async () => {
    renderPanel({ role: 'in_state' })
    await openContesting()
    expect(await screen.findByText(/Running as in_state/)).not.toBeNull()
    // READ-ONLY: nothing in the role field is a control.
    expect((await groupFor('Your role')).querySelector('button, input, select')).toBeNull()
  })

  it('declares SINGLE-OP by default and writes the token the operator picks', async () => {
    renderPanel()
    await openContesting()
    expect((await chip('Entry category', 'SINGLE-OP')).getAttribute('aria-pressed')).toBe('true')
    expect((await chip('Entry category', 'MULTI-OP')).getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(await chip('Entry category', 'MULTI-OP'))
    expect((await chip('Entry category', 'MULTI-OP')).getAttribute('aria-pressed')).toBe('true')
    // CHECKLOG is the third — a log sent to help the sponsor check others.
    expect(await chip('Entry category', 'CHECKLOG')).not.toBeUndefined()
  })
})
