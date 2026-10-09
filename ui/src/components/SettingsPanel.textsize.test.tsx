// @vitest-environment jsdom
//
// #215 — Settings ▸ Appearance ▸ Workspace carries Text size (Normal / Large / Larger) and a
// four-way Density (Comfortable / Standard / Compact / Touch).
//
// Text size is the half of #215 that UI scale only approximated: scale magnifies EVERYTHING
// (chrome, meters, the waterfall's strip) and so shrinks how much fits, while the reporter asked
// for bigger words. The hook and its persistence are useTextSize.test.ts; the sheet's
// sheet-wide scaling is styles-text-size.test.ts; the first paint is index-preseed.test.ts.
// What is asserted here is that the rows exist where the registry says, show the operator's
// own choice, and report a change.
//
// Density grew from two chips to four. The two it had were labelled Comfortable (id
// `standard`) and Compact (id `dense`); `guided` — the roomier value — had no writer at all.
// Now Comfortable names `guided`, Standard names `standard`, and each chip is pressed for its
// OWN id only: the two-chip row pressed "Comfortable" for anything that was not dense, which
// with four choices would press it for three of them.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FeaturesApi } from '../useFeatures'
import type { Density } from '../useDensity'
import type { TextSize } from '../useTextSize'
import { EN } from '../i18n'

// THE BUDGET (2026-10-09). The slowest case here, "offers Normal / Large / Larger and presses only the…", takes
// 0.45 s and 0.37 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
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

// Every prop under test is passed THROUGH, never defaulted here: a helper that supplied
// `textSize` would render a chosen state for the not-wired case below.
function renderPanel(props: {
  textSize?: TextSize
  onTextSizeChange?: (s: TextSize) => void
  density?: Density
  onDensityChange?: (d: Density) => void
}) {
  const { density = 'standard', onDensityChange = () => {}, ...rest } = props
  return render(
    <SettingsPanel
      target="workspace"
      activeRadioId={0}
      scale={1 as never}
      scaleMode={'auto' as never}
      scaleCap={1 as never}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density={density}
      onDensityChange={onDensityChange}
      onResetLayout={() => {}}
      features={features}
      {...rest}
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

const TEXT = EN['settings.workspace.textSize.label']
const SIZE_CHIPS: [TextSize, string][] = [
  ['normal', EN['settings.workspace.textSize.normal']],
  ['large', EN['settings.workspace.textSize.large']],
  ['larger', EN['settings.workspace.textSize.larger']],
]
const DENSITY_CHIPS: [Density, string][] = [
  ['guided', EN['settings.workspace.density.guided']],
  ['standard', EN['settings.workspace.density.standard']],
  ['dense', EN['settings.workspace.density.dense']],
  ['touch', EN['settings.workspace.density.touch']],
]
const pressed = (g: HTMLElement) =>
  within(g)
    .getAllByRole('button')
    .filter((b) => b.getAttribute('aria-pressed') === 'true')
    .map((b) => b.textContent)

describe('#215 Settings ▸ Workspace ▸ Text size', () => {
  it('offers Normal / Large / Larger and presses only the current one, for each of them', async () => {
    for (const [id, label] of SIZE_CHIPS) {
      const { unmount } = renderPanel({ textSize: id, onTextSizeChange: () => {} })
      const g = await screen.findByRole('group', { name: TEXT })
      expect(within(g).getAllByRole('button').map((b) => b.textContent)).toEqual(SIZE_CHIPS.map(([, l]) => l))
      expect(pressed(g), `current '${id}'`).toEqual([label])
      unmount()
    }
  })

  it('reports the chip the operator taps', async () => {
    const changed = vi.fn()
    renderPanel({ textSize: 'normal', onTextSizeChange: changed })
    const g = await screen.findByRole('group', { name: TEXT })
    fireEvent.click(within(g).getByRole('button', { name: EN['settings.workspace.textSize.larger'] }))
    expect(changed).toHaveBeenLastCalledWith('larger')
    fireEvent.click(within(g).getByRole('button', { name: EN['settings.workspace.textSize.large'] }))
    expect(changed).toHaveBeenLastCalledWith('large')
  })

  it('sits in Workspace directly under UI scale, above Density', async () => {
    // Beside the other size control on purpose: UI scale makes EVERYTHING bigger, this makes
    // the words bigger — an operator who came for one should find the other.
    renderPanel({ textSize: 'normal', onTextSizeChange: () => {} })
    const g = await screen.findByRole('group', { name: TEXT })
    const section = g.closest('fieldset')!
    expect(section.querySelector('legend')?.textContent).toBe(EN['settings.workspace.legend'])
    const labels = Array.from(section.querySelectorAll('.settings-label')).map((n) => n.textContent)
    const at = labels.indexOf(TEXT)
    expect(at, 'Text size is not a row of the Workspace section').toBeGreaterThanOrEqual(0)
    expect(labels[at - 1], 'Text size does not follow UI scale').toBe(EN['settings.workspace.scale.label'])
    expect(labels[at + 1], 'Density does not follow Text size').toBe(EN['settings.workspace.density.label'])
  })

  it('is not offered by a host that does not wire it', async () => {
    renderPanel({})
    // The Workspace section rendered (positive control), just without this row.
    await screen.findByRole('group', { name: EN['settings.workspace.density.aria'] })
    expect(screen.queryByRole('group', { name: TEXT })).toBeNull()
  })
})

describe('Settings ▸ Workspace ▸ Density: Comfortable / Standard / Compact / Touch', () => {
  const group = () => screen.findByRole('group', { name: EN['settings.workspace.density.aria'] })

  it('offers the four in order and presses ONLY the current one, for each of them', async () => {
    for (const [id, label] of DENSITY_CHIPS) {
      const { unmount } = renderPanel({ density: id })
      const g = await group()
      expect(within(g).getAllByRole('button').map((b) => b.textContent)).toEqual(DENSITY_CHIPS.map(([, l]) => l))
      expect(pressed(g), `current '${id}'`).toEqual([label])
      unmount()
    }
  })

  it('each chip reports its own id', async () => {
    const changed = vi.fn()
    renderPanel({ density: 'standard', onDensityChange: changed })
    const g = await group()
    for (const [id, label] of DENSITY_CHIPS) {
      fireEvent.click(within(g).getByRole('button', { name: label }))
      expect(changed).toHaveBeenLastCalledWith(id)
    }
  })
})
