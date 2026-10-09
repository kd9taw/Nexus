// @vitest-environment jsdom
//
// Settings ▸ Appearance ▸ Colours — the colour roles (features/paletteRoles.ts). Operator picks of
// 2026-09-26: "Presets first, hex later" (no hex field this round) and "Locked + solid ON AIR" (the
// transmit red, the critical orange and the need set have no control at all).
//
// The persistence and the attributes are usePaletteRoles's (usePaletteRoles.test.ts), the first
// paint is index-preseed.test.ts's and what each preset paints is styles-palette-roles.test.ts's.
// What is asserted here is the section itself: where it sits, one row per role with the role's
// name and its plain-language line, a chip per preset showing the operator's own pick, a Reset
// that is greyed at the default, and nothing for a locked colour.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FeaturesApi } from '../useFeatures'
import { EN } from '../i18n'
import { DEFAULT_SELECTION, PALETTE_ROLES, type PaletteRoleId, type PaletteSelection } from '../features/paletteRoles'
import { SKINS, type SkinId } from '../features/skins'

// THE BUDGET (2026-10-09). The slowest case here, "on a built-in theme, the Accent and Readout defaults…", takes
// 0.63 s and 0.68 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
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

const features = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

// The rows under test are plain strings; the catalog also holds plural records, hence `unknown`.
const E = EN as unknown as Record<string, string>

// Every prop under test is passed THROUGH, never defaulted here (SettingsPanel.highcontrast's rule).
function renderPanel(props: {
  palette?: PaletteSelection
  onPaletteChange?: (role: PaletteRoleId, presetId: string) => void
  skin?: SkinId | null
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
      theme="dark"
      onThemeChange={() => {}}
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

const section = async () => {
  const legend = await screen.findByText(E['settings.colours.legend'], { selector: 'legend' })
  return legend.closest('fieldset') as HTMLElement
}
const group = (s: HTMLElement, id: PaletteRoleId) =>
  within(s).getByRole('group', { name: E[PALETTE_ROLES.find((r) => r.id === id)!.labelKey] })
const resetOf = (s: HTMLElement, id: PaletteRoleId) =>
  within(s).getByRole('button', {
    name: E['settings.colours.reset.aria'].replace('{{role}}', E[PALETTE_ROLES.find((r) => r.id === id)!.labelKey]),
  })

describe('Settings ▸ Appearance ▸ Colours', () => {
  it('sits on the Appearance tab, right after Theme', async () => {
    // Right after Theme, whose cards pick the palette these presets retune (it followed Workspace
    // while the theme row lived there, before the Display sections of 2026-09-26).
    renderPanel({ palette: DEFAULT_SELECTION, onPaletteChange: () => {} })
    const s = await section()
    expect(s.id).toBe('settings-colours')
    const legends = Array.from(document.querySelectorAll('fieldset.settings-section > legend')).map((l) => l.textContent)
    expect(legends.indexOf(E['settings.colours.legend'])).toBe(legends.indexOf(E['settings.theme.legend']) + 1)
  })

  it('has one row per role, in the table’s order, each with its name and what it paints', async () => {
    renderPanel({ palette: DEFAULT_SELECTION, onPaletteChange: () => {} })
    const s = await section()
    const rows = Array.from(s.querySelectorAll('.settings-field'))
    expect(rows.map((r) => r.querySelector('.settings-label')?.textContent)).toEqual(PALETTE_ROLES.map((r) => E[r.labelKey]))
    expect(rows.map((r) => r.querySelector('.settings-hint')?.textContent)).toEqual(PALETTE_ROLES.map((r) => E[r.hintKey]))
    expect(E['palette.accent.hint']).toBe('Selected chips, focus rings, your own chat bubbles')
    expect(E['palette.readout.hint']).toBe('The frequency digits and other readouts')
  })

  it('shows a chip per preset with the operator’s own pick pressed, and each chip shows its colour in both themes', async () => {
    const picked = { ...DEFAULT_SELECTION, readout: 'amber', ok: 'teal' }
    renderPanel({ palette: picked, onPaletteChange: () => {} })
    const s = await section()
    for (const r of PALETTE_ROLES) {
      const chips = within(group(s, r.id)).getAllByRole('button')
      expect(chips.map((c) => c.textContent), r.id).toEqual(r.presets.map((p) => E[p.labelKey]))
      expect(chips.map((c) => c.getAttribute('aria-pressed')), r.id).toEqual(r.presets.map((p) => String(p.id === picked[r.id])))
      chips.forEach((c, i) => {
        const sw = c.querySelector('.palette-swatch') as HTMLElement
        expect(sw.style.getPropertyValue('--swatch-dark'), `${r.id}=${r.presets[i].id}`).toBe(r.presets[i].dark[r.swatch])
        expect(sw.style.getPropertyValue('--swatch-light'), `${r.id}=${r.presets[i].id}`).toBe(r.presets[i].light[r.swatch])
      })
    }
  })

  it('on a built-in theme, the Accent and Readout defaults are the theme’s own colour, and the signal rows are as they are', async () => {
    for (const x of SKINS) {
      renderPanel({ palette: { ...DEFAULT_SELECTION, readout: 'amber' }, onPaletteChange: () => {}, skin: x.id })
      const s = await section()
      for (const r of PALETTE_ROLES) {
        const first = within(group(s, r.id)).getAllByRole('button')[0]
        const sw = first.querySelector('.palette-swatch') as HTMLElement
        const retuned = r.id === 'accent' || r.id === 'readout'
        expect(first.textContent, `${x.id} ${r.id}`).toBe(retuned ? E['palette.preset.themeOwn'] : E[r.presets[0].labelKey])
        expect(sw.style.getPropertyValue('--swatch-dark'), `${x.id} ${r.id}`).toBe(retuned ? x.day[r.swatch] : r.presets[0].dark[r.swatch])
        expect(sw.style.getPropertyValue('--swatch-light'), `${x.id} ${r.id}`).toBe(retuned ? x.day[r.swatch] : r.presets[0].light[r.swatch])
      }
      // The operator's own pick still shows as picked, and the other presets keep their names.
      expect(within(group(s, 'readout')).getByRole('button', { name: E['palette.preset.amber'] }).getAttribute('aria-pressed')).toBe('true')
      cleanup()
    }
  })

  it('reports a pick', async () => {
    const changed = vi.fn()
    renderPanel({ palette: DEFAULT_SELECTION, onPaletteChange: changed })
    const s = await section()
    fireEvent.click(within(group(s, 'accent')).getByRole('button', { name: E['palette.preset.violet'] }))
    expect(changed).toHaveBeenCalledWith('accent', 'violet')
  })

  it('greys Reset while a role is on its default, and Reset goes back to it', async () => {
    const changed = vi.fn()
    renderPanel({ palette: { ...DEFAULT_SELECTION, amber: 'gold' }, onPaletteChange: changed })
    const s = await section()
    for (const r of PALETTE_ROLES) {
      expect((resetOf(s, r.id) as HTMLButtonElement).disabled, r.id).toBe(r.id !== 'amber')
    }
    fireEvent.click(resetOf(s, 'amber'))
    expect(changed).toHaveBeenCalledWith('amber', 'amber')
  })

  it('offers no control for a locked colour, and no hex field', async () => {
    renderPanel({ palette: DEFAULT_SELECTION, onPaletteChange: () => {} })
    const s = await section()
    // Every group is a role from the table — so none is TX, critical or a need colour.
    const groups = within(s).getAllByRole('group').map((g) => g.getAttribute('aria-label'))
    expect(groups).toEqual(PALETTE_ROLES.map((r) => E[r.labelKey]))
    expect(s.querySelectorAll('input').length, 'a free-entry colour field').toBe(0)
    // The words a locked control would carry are absent from the section's controls.
    const words = within(s).getAllByRole('button').map((b) => (b.textContent ?? '') + (b.getAttribute('aria-label') ?? ''))
    for (const locked of [/\bTX\b/, /transmit/i, /critical/i, /orange/i, /need/i]) {
      expect(words.filter((w) => locked.test(w)), String(locked)).toEqual([])
    }
  })

  it('is not offered by a host that does not wire it', async () => {
    renderPanel({})
    // The Appearance tab itself rendered (positive control), just without this section.
    await screen.findByText(E['settings.workspace.legend'], { selector: 'legend' })
    expect(screen.queryByText(E['settings.colours.legend'], { selector: 'legend' })).toBeNull()
  })
})
