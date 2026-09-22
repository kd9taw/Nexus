// @vitest-environment jsdom
//
// #215 — Settings ▸ Appearance ▸ Workspace carries Field mode, beside Theme and UI scale.
//
// The reporter asked for larger type and stronger contrast and could not find either. Half the
// answer was under a label that does not say "size" until you click Manual (UI scale), and the
// other half — the contrast — was not in Settings at all: it is the chip marked "Field" in the
// top bar, named for operating outdoors and therefore nowhere near the question being asked.
// This pins the entry in the place the operator was told it would be.
//
// The boolean is `useFieldMode`'s and its own hook tests own the default and the
// `data-contrast` attribute; what is asserted here is that the control exists where the
// registry says it does, shows the current choice, and reports a change.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FeaturesApi } from '../useFeatures'
import { EN } from '../i18n'
import { searchSettings } from '../settings/registry'

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

const features = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

// ⚠️ `fieldMode` is passed THROUGH, never defaulted here. A fixture helper that supplies the
// value under test renders the "on" case for an "absent" test, because a JS default parameter
// fires on an explicit `undefined` — and the absent case is exactly what the third test below
// is for.
function renderPanel(props: {
  fieldMode?: boolean
  onFieldModeChange?: (on: boolean) => void
  theme?: 'light' | 'dark'
  onThemeChange?: (t: 'light' | 'dark') => void
}) {
  return render(
    <SettingsPanel
      target="workspace"
      activeRadioId={0}
      scale={1 as never}
      scaleMode={'auto' as never}
      scaleCap={1 as never}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density={'standard' as never}
      onDensityChange={() => {}}
      onResetLayout={() => {}}
      features={features}
      {...props}
    />,
  )
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
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const LABEL = EN['settings.workspace.field.label']
const fieldGroup = () => screen.getByRole('group', { name: LABEL })

describe('#215 Settings ▸ Workspace ▸ Field mode', () => {
  it('shows OFF as the current choice and reports turning it on', async () => {
    const changed = vi.fn()
    renderPanel({ fieldMode: false, onFieldModeChange: changed })
    const g = await screen.findByRole('group', { name: LABEL })
    const off = within(g).getByRole('button', { name: EN['settings.workspace.field.off'] })
    const on = within(g).getByRole('button', { name: EN['settings.workspace.field.on'] })
    expect(off.getAttribute('aria-pressed')).toBe('true')
    expect(on.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(on)
    expect(changed).toHaveBeenCalledWith(true)
  })

  it('shows ON when it is on, and reports turning it off', async () => {
    // The mirror of the case above, and not decoration: a row that rendered `aria-pressed`
    // from a constant rather than from the prop passes the first test and fails this one.
    const changed = vi.fn()
    renderPanel({ fieldMode: true, onFieldModeChange: changed })
    const g = await screen.findByRole('group', { name: LABEL })
    const off = within(g).getByRole('button', { name: EN['settings.workspace.field.off'] })
    const on = within(g).getByRole('button', { name: EN['settings.workspace.field.on'] })
    expect(on.getAttribute('aria-pressed')).toBe('true')
    expect(off.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(off)
    expect(changed).toHaveBeenCalledWith(false)
  })

  it('sits in the Workspace section, between Theme and UI scale', async () => {
    // The ask was not "a switch exists somewhere" — it was that this belongs beside the two
    // settings the operator was already sent to and found only half an answer in.
    renderPanel({
      fieldMode: false,
      onFieldModeChange: () => {},
      theme: 'dark',
      onThemeChange: () => {},
    })
    const g = await screen.findByRole('group', { name: LABEL })
    const section = g.closest('fieldset')
    expect(section, 'Field mode is not inside a Settings section at all').not.toBeNull()
    expect(
      section?.querySelector('legend')?.textContent,
      'Field mode is not in the Workspace section',
    ).toBe(EN['settings.workspace.legend'])
    const labels = Array.from(section!.querySelectorAll('.settings-label')).map((n) => n.textContent)
    const at = (label: string) => {
      const i = labels.indexOf(label)
      expect(i, `"${label}" is not a row of the Workspace section`).toBeGreaterThanOrEqual(0)
      return i
    }
    expect(at(LABEL), 'Field mode is above Theme').toBeGreaterThan(
      at(EN['settings.workspace.theme.label']),
    )
    expect(at(LABEL), 'Field mode is below UI scale').toBeLessThan(
      at(EN['settings.workspace.scale.label']),
    )
  })

  it('is not offered by a host that does not wire it', async () => {
    renderPanel({})
    // The Workspace section itself rendered (positive control), just without this field.
    await screen.findByRole('group', { name: EN['settings.workspace.density.aria'] })
    expect(() => fieldGroup()).toThrow()
  })

  it('the words the reporter searched with reach it', () => {
    // The other half of the hunt, and the reason a row alone is not the fix: the size half
    // was already findable ('text size', 'zoom') and the contrast half matched nothing, so
    // Settings search answered "no results" to the question actually being asked.
    const ids = (q: string) => searchSettings(q).map((h) => h.section.id)
    for (const q of ['contrast', 'high contrast', 'field mode', 'accessibility']) {
      expect(ids(q), `Settings search cannot find Workspace from "${q}"`).toContain('workspace')
    }
    // CONTROL, both directions. A search that returned everything would satisfy the loop
    // above, and 'field' on its own must still belong to the station-preset section — an
    // exact keyword outranks all but a label, so claiming it here would have moved a hit.
    expect(ids('rotator'), 'the search matches everything').not.toContain('workspace')
    expect(ids('field')[0], '"field" no longer lands on the home/field station presets')
      .not.toBe('workspace')
  })
})
