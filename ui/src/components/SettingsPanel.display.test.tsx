// @vitest-environment jsdom
//
// Settings ▸ Appearance, regrouped into DISPLAY SECTIONS (the look-and-feel redesign, piece 7):
// Workspace (the one-tap looks and the sizes) · Theme · Colours · Waterfall & scopes · Map & globe
// · Performance, then the tab's other sections as before. The registry holds the order
// (registry.test.ts compares it with the source); asserted here is what renders, where:
//   · Theme holds the theme cards, then High contrast, Night and Field mode, in that order;
//   · Waterfall & scopes holds the two palette pickers the cockpit headers also carry, writing the
//     same two keys, with Cividis offered as colour-blind safe and Turbo still the default;
//   · Map & globe holds the Logbook globe switch;
//   · Performance holds Motion (Follow the computer / Reduce).
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FeaturesApi } from '../useFeatures'
import type { Motion } from '../useMotion'
import { DEFAULT_SELECTION } from '../features/paletteRoles'
import { LOGBOOK_GLOBE_KEY } from '../features/logbookGlobe'
import { FT_PALETTE_SCOPE, WF_PALETTE_KEY } from '../waterfallPalette'
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

function renderPanel(props: { motion?: Motion; onMotionChange?: (m: Motion) => void } = {}) {
  return render(
    <SettingsPanel
      target="appearance"
      activeRadioId={0}
      scale={100 as never}
      scaleMode={'auto' as never}
      scaleCap={100 as never}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density="standard"
      onDensityChange={() => {}}
      textSize="normal"
      onTextSizeChange={() => {}}
      onResetLayout={() => {}}
      features={features}
      theme="dark"
      onThemeChange={() => {}}
      fieldMode={false}
      onFieldModeChange={() => {}}
      highContrast={false}
      onHighContrastChange={() => {}}
      night="off"
      onNightChange={() => {}}
      nightGridKnown
      palette={DEFAULT_SELECTION}
      onPaletteChange={() => {}}
      motion="system"
      onMotionChange={() => {}}
      {...props}
    />,
  )
}

beforeEach(() => {
  localStorage.clear()
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
  localStorage.clear()
})

const sectionNamed = async (legendKey: keyof typeof EN) => {
  const legend = await screen.findByText(EN[legendKey] as string, { selector: 'legend' })
  return legend.closest('fieldset') as HTMLElement
}
const rowLabels = (s: HTMLElement) =>
  Array.from(s.querySelectorAll('.settings-label')).map((n) => n.textContent)

describe('Settings ▸ Appearance: the Display sections', () => {
  it('lists Workspace, Theme, Colours, Waterfall & scopes, Map & globe and Performance first, in that order', async () => {
    renderPanel()
    await sectionNamed('settings.performance.legend')
    const legends = Array.from(document.querySelectorAll('fieldset.settings-section > legend')).map((l) => l.textContent)
    expect(legends.slice(0, 6)).toEqual([
      EN['settings.workspace.legend'],
      EN['settings.theme.legend'],
      EN['settings.colours.legend'],
      EN['settings.waterfallScopes.legend'],
      EN['settings.mapGlobe.legend'],
      EN['settings.performance.legend'],
    ])
  })

  it('Theme holds the theme cards, then High contrast, Night and Field mode', async () => {
    renderPanel()
    const s = await sectionNamed('settings.theme.legend')
    expect(s.id).toBe('settings-theme')
    expect(within(s).getByRole('group', { name: EN['theme.aria'] })).toBeTruthy()
    expect(rowLabels(s)).toEqual([
      EN['settings.workspace.theme.label'],
      EN['settings.workspace.contrast.label'],
      EN['settings.workspace.night.label'],
      EN['settings.workspace.field.label'],
    ])
    // …and none of them is left behind in Workspace.
    const w = await sectionNamed('settings.workspace.legend')
    for (const k of ['theme', 'contrast', 'night', 'field'] as const) {
      expect(rowLabels(w), `${k} is still in Workspace`).not.toContain(EN[`settings.workspace.${k}.label`])
    }
  })

  it('Waterfall & scopes holds both palette pickers, on the same keys the cockpit pickers write', async () => {
    renderPanel()
    const s = await sectionNamed('settings.waterfallScopes.legend')
    expect(s.id).toBe('settings-waterfall-scopes')
    const shared = within(s).getByRole('combobox', { name: EN['settings.waterfallScopes.shared.label'] }) as HTMLSelectElement
    const ft = within(s).getByRole('combobox', { name: EN['settings.waterfallScopes.ft.label'] }) as HTMLSelectElement
    // Turbo is still the default of both, and nothing is written by opening Settings.
    expect(shared.value).toBe('turbo')
    expect(ft.value).toBe('turbo')
    expect(localStorage.getItem(WF_PALETTE_KEY)).toBeNull()
    expect(localStorage.getItem(`${WF_PALETTE_KEY}.${FT_PALETTE_SCOPE}`)).toBeNull()
    // Cividis is offered, named colour-blind safe, in both.
    for (const sel of [shared, ft]) {
      const cividis = Array.from(sel.options).find((o) => o.value === 'cividis')
      expect(cividis?.textContent).toBe(EN['waterfall.palette.cividis'])
    }
    fireEvent.change(ft, { target: { value: 'cividis' } })
    expect(localStorage.getItem(`${WF_PALETTE_KEY}.${FT_PALETTE_SCOPE}`)).toBe('cividis')
    expect(localStorage.getItem(WF_PALETTE_KEY), 'the FT pick reached the shared palette').toBeNull()
    fireEvent.change(shared, { target: { value: 'viridis' } })
    expect(localStorage.getItem(WF_PALETTE_KEY)).toBe('viridis')
    expect(localStorage.getItem(`${WF_PALETTE_KEY}.${FT_PALETTE_SCOPE}`), 'the shared pick reached FT').toBe('cividis')
    // A picker SHOWS its pick through its palette event, whose listener is a passive effect
    // (waterfallPalette.ts); the panel mounts these after its settings load, outside act(), so on a
    // loaded run a pick can land before that effect has run. The effect re-reads storage when it
    // does, so the selects converge on what was stored: wait for that, never assume it.
    await waitFor(() => {
      expect(ft.value).toBe('cividis')
      expect(shared.value).toBe('viridis')
    })
  })

  it('Map & globe holds the Logbook globe switch', async () => {
    renderPanel()
    const s = await sectionNamed('settings.mapGlobe.legend')
    expect(s.id).toBe('settings-map-globe')
    const box = within(s).getByRole('checkbox', { name: EN['settings.workspace.logbookGlobe.aria'] }) as HTMLInputElement
    expect(box.checked).toBe(true)
    fireEvent.click(box)
    expect(localStorage.getItem(LOGBOOK_GLOBE_KEY)).toBe('off')
    const w = await sectionNamed('settings.workspace.legend')
    expect(rowLabels(w)).not.toContain(EN['settings.workspace.logbookGlobe.label'])
  })

  it('Performance holds Motion: Follow the computer or Reduce, the current one pressed, a tap reported', async () => {
    const changed = vi.fn()
    for (const m of ['system', 'reduce'] as const) {
      const { unmount } = renderPanel({ motion: m, onMotionChange: changed })
      const s = await sectionNamed('settings.performance.legend')
      expect(s.id).toBe('settings-performance')
      const g = within(s).getByRole('group', { name: EN['settings.performance.motion.label'] })
      const pressed = within(g)
        .getAllByRole('button')
        .filter((b) => b.getAttribute('aria-pressed') === 'true')
        .map((b) => b.textContent)
      expect(pressed).toEqual([EN[`settings.performance.motion.${m}`]])
      unmount()
    }
    renderPanel({ motion: 'system', onMotionChange: changed })
    const s = await sectionNamed('settings.performance.legend')
    fireEvent.click(within(s).getByRole('button', { name: EN['settings.performance.motion.reduce'] }))
    expect(changed).toHaveBeenLastCalledWith('reduce')
  })

  it('offers no Performance section to a host that does not wire Motion', async () => {
    renderPanel({ onMotionChange: undefined })
    // The tab rendered (positive control), just without this section.
    await sectionNamed('settings.mapGlobe.legend')
    expect(screen.queryByText(EN['settings.performance.legend'], { selector: 'legend' })).toBeNull()
  })
})
