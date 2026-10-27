// @vitest-environment jsdom
//
// Settings and the first-run experience answer: Settings ▸ Station ▸ Your experience (its one
// writer, the confirm before a hand-tuned feature set is replaced) and the folding the two "new"
// answers bring (a section no first-hour operator needs shows its heading only, until Show all or a
// deep link). No answer, the panel as it always was: an updating station sees no change.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FeaturesApi } from '../useFeatures'
import { ConfirmHost } from '../confirm'
import { EN } from '../i18n'
import { SETTINGS_SECTIONS } from '../settings/registry'
import { resolveEnabled } from '../features/profiles'

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


function makeFeatures(profile: string, enabled: Record<string, boolean> = {}) {
  return {
    enabled,
    isOn: () => true,
    toggle: vi.fn(),
    applyProfile: vi.fn(),
    applyProfiles: vi.fn(),
    profile,
    landing: 'operate',
    firstRun: false,
    dismissedReveals: [],
    dismissReveal: vi.fn(),
  } as unknown as FeaturesApi
}

function renderPanel(target: string, level: string | undefined, features = makeFeatures('everything')) {
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({
      ...defaultSettings,
      mycall: 'KD9TAW',
      mygrid: 'EN52',
      ...(level ? { experienceLevel: level } : {}),
    } as never),
  )
  render(
    <>
      <SettingsPanel
        target={target}
        activeRadioId={0}
        scale={1 as never}
        scaleMode={'auto' as never}
        scaleCap={1 as never}
        onScaleModeChange={() => {}}
        onScaleCapChange={() => {}}
        density="standard"
        onDensityChange={() => {}}
        onResetLayout={() => {}}
        features={features}
        highContrast={false}
        onHighContrastChange={() => {}}
        fieldMode={false}
        onFieldModeChange={() => {}}
      />
      <ConfirmHost />
    </>,
  )
  return features
}

beforeEach(() => {
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.2.6'))
  api.get('setExperienceLevel').mockImplementation(() => Promise.resolve({} as never))
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const select = () => screen.queryByLabelText(EN['settings.station.experience.label']) as HTMLSelectElement
const section = (id: string) => document.getElementById(`settings-${id}`) as HTMLElement
const SHOW_ALL = EN['settings.fold.showAll']

describe('Settings ▸ Station ▸ Your experience', () => {
  it('says Not set for a station that was never asked, and folds nothing', async () => {
    renderPanel('operator-radio', undefined)
    await waitFor(() => expect(select()).toBeTruthy())
    expect(select().value).toBe('')
    expect(select().selectedOptions[0].textContent).toBe(EN['settings.station.experience.notSet'])
    expect(document.querySelectorAll('[data-folded]').length).toBe(0)
    expect(screen.queryByText(SHOW_ALL)).toBeNull()
  })

  it('shows the stored answer, with no Not set left to pick', async () => {
    renderPanel('operator-radio', 'newToDigital')
    await waitFor(() => expect(select()?.value).toBe('newToDigital'))
    expect([...select().options].map((o) => o.value)).toEqual(['newToHobby', 'newToDigital', 'experienced'])
  })

  it('saves a pick through its writer and re-picks the starting features', async () => {
    const features = renderPanel('operator-radio', undefined)
    await waitFor(() => expect(select()).toBeTruthy())
    fireEvent.change(select(), { target: { value: 'newToDigital' } })
    await waitFor(() => expect(api.get('setExperienceLevel')).toHaveBeenCalledWith('newToDigital'))
    await waitFor(() => expect(features.applyProfiles).toHaveBeenCalledWith(['starter'], ['phone', 'cw']))
    // Its own writer only: a pick is not a whole-settings save.
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })

  it('asks before a named profile loses screens, names them, and changes nothing when declined', async () => {
    // An updating station on the Everything profile (not hand-tuned) picks New to digital.
    const features = renderPanel('operator-radio', undefined, makeFeatures('everything', resolveEnabled('everything')))
    await waitFor(() => expect(select()).toBeTruthy())
    fireEvent.change(select(), { target: { value: 'newToDigital' } })
    const cancel = await screen.findByRole('button', { name: /cancel/i })
    const body = await screen.findByText((_, el) => !!el?.textContent?.startsWith('These screens turn off:') && el.children.length === 0)
    expect(body.textContent).toContain('RTTY')
    // Phone and CW stay on for this answer, so they are not named.
    expect(body.textContent).not.toMatch(/\bPhone\b/)
    fireEvent.click(cancel)
    await new Promise((r) => setTimeout(r, 20))
    expect(api.get('setExperienceLevel')).not.toHaveBeenCalled()
    expect(features.applyProfiles).not.toHaveBeenCalled()
    expect(select().value).toBe('')
  })

  it('does not ask when the answer turns no screen off', async () => {
    // Already the starter set: New to the hobby turns nothing off.
    const starter = resolveEnabled('starter')
    const features = renderPanel('operator-radio', undefined, makeFeatures('starter', starter))
    await waitFor(() => expect(select()).toBeTruthy())
    fireEvent.change(select(), { target: { value: 'newToHobby' } })
    await waitFor(() => expect(api.get('setExperienceLevel')).toHaveBeenCalledWith('newToHobby'))
    expect(screen.queryByRole('button', { name: /cancel/i })).toBeNull()
    expect(features.applyProfiles).toHaveBeenCalledWith(['starter'], [])
  })

  it('asks before replacing a hand-tuned feature set, and changes nothing when declined', async () => {
    const features = renderPanel('operator-radio', 'experienced', makeFeatures('custom', resolveEnabled('everything')))
    await waitFor(() => expect(select()?.value).toBe('experienced'))
    fireEvent.change(select(), { target: { value: 'newToHobby' } })
    const cancel = await screen.findByRole('button', { name: /cancel/i })
    fireEvent.click(cancel)
    await new Promise((r) => setTimeout(r, 20))
    expect(api.get('setExperienceLevel')).not.toHaveBeenCalled()
    expect(features.applyProfiles).not.toHaveBeenCalled()
  })
})

describe('Settings folding for a new operator', () => {
  it('folds the Appearance sections nobody needs in hour one, and Show all opens them', async () => {
    renderPanel('station', 'newToHobby')
    await waitFor(() => expect(select()).toBeTruthy())
    // (Station's one section is needed in hour one.)
    // Station's one section is needed in hour one: nothing folds on this tab.
    expect(section('operator-radio').hasAttribute('data-folded')).toBe(false)
    expect(screen.queryByText(SHOW_ALL)).toBeNull()
    fireEvent.click(screen.getByRole('tab', { name: EN['settings.tabs.appearance'] }))
    await waitFor(() => expect(section('theme')).toBeTruthy())
    expect(section('theme').getAttribute('data-folded')).toBe('true')
    // Features and Accessibility never fold, for any answer.
    expect(section('features').hasAttribute('data-folded')).toBe(false)
    expect(section('accessibility').hasAttribute('data-folded')).toBe(false)
    // The heading stays as the label.
    expect(section('theme').querySelector('legend')).toBeTruthy()
    fireEvent.click(screen.getByText(SHOW_ALL))
    expect(section('theme').hasAttribute('data-folded')).toBe(false)
    expect(screen.queryByText(SHOW_ALL)).toBeNull()
  })

  it('keeps a hour-one section open on a tab that folds others (Radio)', async () => {
    renderPanel('radio', 'newToDigital')
    await waitFor(() => expect(section('rig-control')).toBeTruthy())
    expect(section('rig-control').hasAttribute('data-folded')).toBe(false)
    expect(section('audio').hasAttribute('data-folded')).toBe(false)
    expect(section('rotator').getAttribute('data-folded')).toBe('true')
  })

  it('opens the section a deep link names, though the rest of its tab stays folded', async () => {
    renderPanel('rotator', 'newToHobby')
    await waitFor(() => expect(section('rotator')).toBeTruthy())
    expect(section('rotator').hasAttribute('data-folded')).toBe(false)
    expect(section('orbital-elements').getAttribute('data-folded')).toBe('true')
  })

  it('a deep link to a section inside a folded section opens the section around it', async () => {
    renderPanel('antenna-gain', 'newToHobby')
    await waitFor(() => expect(section('antenna-gain')).toBeTruthy())
    expect(section('antenna-gain').closest('[data-folded]')).toBeNull()
    expect(section('integrations-feeds').hasAttribute('data-folded')).toBe(false)
  })

  it('a search pick of a section inside a folded section opens the section around it', async () => {
    renderPanel('station', 'newToHobby')
    await waitFor(() => expect(select()).toBeTruthy())
    fireEvent.change(document.querySelector('.settings-search input') as HTMLElement, { target: { value: 'antenna gain' } })
    const hit = await screen.findByText('Antenna gain (advanced)', { selector: '.settings-search-hit-label' })
    fireEvent.pointerDown(hit.closest('button') as HTMLElement)
    await waitFor(() => expect(section('antenna-gain')).toBeTruthy())
    expect(section('antenna-gain').closest('[data-folded]')).toBeNull()
    // Control: a folded section beside it on that tab stays folded.
    const folded = document.querySelectorAll('[data-folded]')
    expect(folded.length).toBeGreaterThan(0)
  })

  it('every nested section, as a target, ends up outside every fold', async () => {
    const nested = SETTINGS_SECTIONS.filter((x) => x.parent)
    expect(nested.length).toBeGreaterThan(0)
    for (const n of nested) {
      for (const level of ['newToHobby', 'newToDigital']) {
        renderPanel(n.id, level)
        await waitFor(() => expect(section(n.id), n.id).toBeTruthy())
        expect(section(n.id).closest('[data-folded]'), `${n.id} for ${level}`).toBeNull()
        cleanup()
      }
    }
  })

  it('folds nothing for an experienced operator', async () => {
    renderPanel('theme', 'experienced')
    await waitFor(() => expect(section('theme')).toBeTruthy())
    expect(document.querySelectorAll('[data-folded]').length).toBe(0)
    expect(screen.queryByText(SHOW_ALL)).toBeNull()
  })
})
