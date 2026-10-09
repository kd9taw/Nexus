// @vitest-environment jsdom
//
// Settings ▸ Appearance ▸ Theme ▸ Night: Off / On / Auto (operator, 2026-09-26: "Settings +
// Auto by sun"). It is a Settings row and NOT a top-bar chip — Field stays the only quick chip —
// and it sits directly under High contrast, the other row that changes how the theme above it
// paints. Auto is civil dusk to civil dawn at the station's grid square, so with no grid square it
// cannot work, and the row says so instead of looking broken.
//
// The hook, the sun and the storage are useNight.test.ts; the colours are
// styles-night-contrast.test.ts; the first paint is index-preseed.test.ts. Asserted here: the row
// exists where it belongs, shows the operator's own choice, reports a tap, and explains Auto.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FeaturesApi } from '../useFeatures'
import type { NightChoice } from '../useNight'
import { EN } from '../i18n'

// THE BUDGET (2026-10-09). The slowest case here, "offers Off / On / Auto and presses only the current…", takes
// 0.42 s and 1.05 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
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

// Every prop under test is passed THROUGH, never defaulted here: a helper that supplied `night`
// would render a chosen state for the not-wired case below.
function renderPanel(props: {
  night?: NightChoice
  onNightChange?: (c: NightChoice) => void
  nightGridKnown?: boolean
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
      density="standard"
      onDensityChange={() => {}}
      onResetLayout={() => {}}
      features={features}
      highContrast={false}
      onHighContrastChange={() => {}}
      fieldMode={false}
      onFieldModeChange={() => {}}
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

const NIGHT = EN['settings.workspace.night.label']
const CHIPS: [NightChoice, string][] = [
  ['off', EN['settings.workspace.night.off']],
  ['on', EN['settings.workspace.night.on']],
  ['auto', EN['settings.workspace.night.auto']],
]
const pressed = (g: HTMLElement) =>
  within(g)
    .getAllByRole('button')
    .filter((b) => b.getAttribute('aria-pressed') === 'true')
    .map((b) => b.textContent)
const hintOf = (g: HTMLElement) => g.closest('.settings-field')!.querySelector('.settings-hint')!.textContent

describe('Settings ▸ Workspace ▸ Night', () => {
  it('offers Off / On / Auto and presses only the current one, for each of them', async () => {
    for (const [id, label] of CHIPS) {
      const { unmount } = renderPanel({ night: id, onNightChange: () => {}, nightGridKnown: true })
      const g = await screen.findByRole('group', { name: NIGHT })
      expect(within(g).getAllByRole('button').map((b) => b.textContent)).toEqual(CHIPS.map(([, l]) => l))
      expect(pressed(g), `current '${id}'`).toEqual([label])
      unmount()
    }
  })

  it('reports the chip the operator taps', async () => {
    const changed = vi.fn()
    renderPanel({ night: 'off', onNightChange: changed, nightGridKnown: true })
    const g = await screen.findByRole('group', { name: NIGHT })
    for (const [id, label] of CHIPS) {
      fireEvent.click(within(g).getByRole('button', { name: label }))
      expect(changed).toHaveBeenLastCalledWith(id)
    }
  })

  it('sits directly under High contrast, above Field mode', async () => {
    renderPanel({ night: 'off', onNightChange: () => {}, nightGridKnown: true })
    const g = await screen.findByRole('group', { name: NIGHT })
    const section = g.closest('fieldset')!
    // Theme, not Workspace, since the Display sections (2026-09-26): the row moved with the cards.
    expect(section.querySelector('legend')?.textContent).toBe(EN['settings.theme.legend'])
    const labels = Array.from(section.querySelectorAll('.settings-label')).map((n) => n.textContent)
    const at = labels.indexOf(NIGHT)
    expect(at, 'Night is not a row of the Theme section').toBeGreaterThanOrEqual(0)
    expect(labels[at - 1], 'Night does not follow High contrast').toBe(EN['settings.workspace.contrast.label'])
    expect(labels[at + 1], 'Field mode does not follow Night').toBe(EN['settings.workspace.field.label'])
  })

  it('explains Auto: at dusk at the grid square, and says when there is no grid square', async () => {
    const { unmount } = renderPanel({ night: 'auto', onNightChange: () => {}, nightGridKnown: true })
    let g = await screen.findByRole('group', { name: NIGHT })
    expect(hintOf(g)).toBe(EN['settings.workspace.night.hint.auto'])
    unmount()
    renderPanel({ night: 'auto', onNightChange: () => {}, nightGridKnown: false })
    g = await screen.findByRole('group', { name: NIGHT })
    expect(hintOf(g)).toBe(EN['settings.workspace.night.hint.noGrid'])
    expect(hintOf(g)).toMatch(/^Auto needs your grid square/)
  })

  it('Off and On explain what Night does, and never mention the grid', async () => {
    for (const id of ['off', 'on'] as const) {
      // No grid square: only Auto needs one, so only Auto may complain about it.
      const { unmount } = renderPanel({ night: id, onNightChange: () => {}, nightGridKnown: false })
      const g = await screen.findByRole('group', { name: NIGHT })
      expect(hintOf(g), id).toBe(EN['settings.workspace.night.hint'])
      unmount()
    }
  })

  it('is not offered by a host that does not wire it', async () => {
    renderPanel({})
    // The tab rendered (positive control), just without this row.
    await screen.findByRole('group', { name: EN['settings.workspace.contrast.label'] })
    expect(screen.queryByRole('group', { name: NIGHT })).toBeNull()
  })
})
