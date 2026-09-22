// @vitest-environment jsdom
//
// #215 — Settings ▸ Appearance ▸ Workspace carries High contrast, on its own row.
//
// The reporter asked for larger type AND a higher-contrast theme. Field mode answered both at
// once and was the ONLY route to the contrast tokens, so "more contrast at the size I have"
// was not a thing the app could be told: in auto scale mode, turning field mode on moves the
// zoom as well (`fieldFitScale`). The size half needed nothing — UI scale, two rows down, is
// an eleven-step ladder plus a cap, and a boolean beside it would be a worse second control
// for an axis that already had a better one. So this is one row, contrast only.
//
// The derivation and the persistence are `useContrastPrefs`'s and are tested in
// useContrastPrefs.test.ts; the first-paint copy is in index-preseed.test.ts. What is
// asserted here is that the control exists where the registry says it does, shows the
// operator's own choice, reports a change, and discloses the one case where something else
// is also asking for the tokens.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FeaturesApi } from '../useFeatures'
import { EN } from '../i18n'

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

// ⚠️ Every prop under test is passed THROUGH, never defaulted here — a fixture helper that
// supplies `highContrast` renders the "on" case for the absent test, because a JS default
// parameter fires on an explicit `undefined`. `fieldMode` is in the same position: the
// disclosure test below turns on the state this helper must not invent.
function renderPanel(props: {
  highContrast?: boolean
  onHighContrastChange?: (on: boolean) => void
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

const LABEL = EN['settings.workspace.contrast.label']
const contrastGroup = () => screen.getByRole('group', { name: LABEL })

describe('#215 Settings ▸ Workspace ▸ High contrast', () => {
  it('shows OFF as the current choice and reports turning it on', async () => {
    const changed = vi.fn()
    renderPanel({ highContrast: false, onHighContrastChange: changed })
    const g = await screen.findByRole('group', { name: LABEL })
    const off = within(g).getByRole('button', { name: EN['settings.workspace.contrast.off'] })
    const on = within(g).getByRole('button', { name: EN['settings.workspace.contrast.on'] })
    expect(off.getAttribute('aria-pressed')).toBe('true')
    expect(on.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(on)
    expect(changed).toHaveBeenCalledWith(true)
  })

  it('shows ON when it is on, and reports turning it off', async () => {
    // The mirror, and not decoration: a row rendering `aria-pressed` from a constant rather
    // than from the prop passes the case above and fails this one.
    const changed = vi.fn()
    renderPanel({ highContrast: true, onHighContrastChange: changed })
    const g = await screen.findByRole('group', { name: LABEL })
    const off = within(g).getByRole('button', { name: EN['settings.workspace.contrast.off'] })
    const on = within(g).getByRole('button', { name: EN['settings.workspace.contrast.on'] })
    expect(on.getAttribute('aria-pressed')).toBe('true')
    expect(off.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(off)
    expect(changed).toHaveBeenCalledWith(false)
  })

  it('sits in Workspace, under Theme and above Field mode and UI scale', async () => {
    // Under Theme because it modifies the palette that row picks; above Field mode because
    // field mode is the bundle of this plus the size change, and above UI scale because that
    // is the size axis this row deliberately does not touch.
    renderPanel({
      highContrast: false,
      onHighContrastChange: () => {},
      fieldMode: false,
      onFieldModeChange: () => {},
      theme: 'dark',
      onThemeChange: () => {},
    })
    const g = await screen.findByRole('group', { name: LABEL })
    const section = g.closest('fieldset')
    expect(section, 'High contrast is not inside a Settings section at all').not.toBeNull()
    expect(
      section?.querySelector('legend')?.textContent,
      'High contrast is not in the Workspace section',
    ).toBe(EN['settings.workspace.legend'])
    const labels = Array.from(section!.querySelectorAll('.settings-label')).map((n) => n.textContent)
    const at = (label: string) => {
      const i = labels.indexOf(label)
      expect(i, `"${label}" is not a row of the Workspace section`).toBeGreaterThanOrEqual(0)
      return i
    }
    expect(at(LABEL), 'High contrast is above Theme').toBeGreaterThan(
      at(EN['settings.workspace.theme.label']),
    )
    expect(at(LABEL), 'High contrast is below Field mode').toBeLessThan(
      at(EN['settings.workspace.field.label']),
    )
    expect(at(LABEL), 'High contrast is below UI scale').toBeLessThan(
      at(EN['settings.workspace.scale.label']),
    )
  })

  it('is not offered by a host that does not wire it', async () => {
    renderPanel({})
    // The Workspace section itself rendered (positive control), just without this field.
    await screen.findByRole('group', { name: EN['settings.workspace.density.aria'] })
    expect(() => contrastGroup()).toThrow()
  })
})

describe('#215 the row does not lie while field mode is also asking for the tokens', () => {
  const hintOf = (g: HTMLElement) =>
    g.closest('.settings-field')?.querySelector('.settings-hint')?.textContent

  it('says nothing about field mode when field mode is off', async () => {
    // The control for the test below. Without it, a hint hard-coded to the field wording
    // would pass that one and this row would be permanently wrong.
    renderPanel({ highContrast: false, onHighContrastChange: () => {}, fieldMode: false })
    const g = await screen.findByRole('group', { name: LABEL })
    expect(hintOf(g)).toBe(EN['settings.workspace.contrast.hint'])
  })

  it('discloses that field mode is applying high contrast anyway', async () => {
    // The brief's requirement, and the one state where the row's own value and what the
    // operator sees on screen legitimately disagree: field mode is independently lighting
    // the same tokens. The stored preference is untouched and still shown as Off — so the
    // hint has to say why the screen looks the way it does, or the row reads as broken.
    renderPanel({ highContrast: false, onHighContrastChange: () => {}, fieldMode: true })
    const g = await screen.findByRole('group', { name: LABEL })
    expect(hintOf(g)).toBe(EN['settings.workspace.contrast.hint.field'])
    // And the switch stays live: the standing preference is still the operator's to set,
    // and it is what the screen falls back to when field mode goes off.
    const off = within(g).getByRole('button', {
      name: EN['settings.workspace.contrast.off'],
    }) as HTMLButtonElement
    const on = within(g).getByRole('button', {
      name: EN['settings.workspace.contrast.on'],
    }) as HTMLButtonElement
    expect(off.disabled, 'the Off chip went dead while field mode was on').toBe(false)
    expect(on.disabled, 'the On chip went dead while field mode was on').toBe(false)
    expect(off.getAttribute('aria-pressed'), 'field mode overwrote the stored choice').toBe('true')
  })
})
