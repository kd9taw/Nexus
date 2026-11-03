// @vitest-environment jsdom
//
// Settings ▸ Appearance ▸ Workspace ▸ UI scale carries the answer to the one-time prompt a
// scaled Windows display gets (useScaleFitNotice.ts): Auto (the smaller fit, 80% at 125%) or Off
// (the size Nexus used before). The prompt asks once; this is where the answer is seen and
// changed afterwards. The hook side (stored, applied live) is useScale.scaled.test.ts.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FeaturesApi } from '../useFeatures'
import type { FitRule, ScaleMode } from '../useScale'
import { EN } from '../i18n'

// THE BUDGET: the house 15 s, as in SettingsPanel.textsize.test.tsx (a loaded full suite on
// this box has run SettingsPanel cases up to 20 times slower than one core, past vitest's 5 s).
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
// `scaleFitRule` would render a chosen state for the not-wired case below.
function renderPanel(props: {
  scaleMode?: ScaleMode
  scaleFitRule?: FitRule
  onScaleFitRuleChange?: (r: FitRule) => void
}) {
  const { scaleMode = 'auto', ...rest } = props
  return render(
    <SettingsPanel
      target="workspace"
      activeRadioId={0}
      scale={80}
      scaleMode={scaleMode}
      scaleCap={100}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density="standard"
      onDensityChange={() => {}}
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

const GROUP = EN['settings.workspace.scale.fit.aria']
const CHIPS: [FitRule, string][] = [
  ['scaled', EN['settings.workspace.scale.fit.auto']],
  ['classic', EN['settings.workspace.scale.fit.off']],
]
const pressed = (g: HTMLElement) =>
  within(g)
    .getAllByRole('button')
    .filter((b) => b.getAttribute('aria-pressed') === 'true')
    .map((b) => b.textContent)

describe('Settings ▸ Workspace ▸ UI scale: the scaled-display answer', () => {
  it('shows the current answer, Auto or Off, and presses only that one', async () => {
    for (const [rule, label] of CHIPS) {
      const { unmount } = renderPanel({ scaleFitRule: rule, onScaleFitRuleChange: () => {} })
      const g = await screen.findByRole('group', { name: GROUP })
      expect(within(g).getAllByRole('button').map((b) => b.textContent)).toEqual(CHIPS.map(([, l]) => l))
      expect(pressed(g), `answer '${rule}'`).toEqual([label])
      unmount()
    }
  })

  it('reports the answer the operator taps', async () => {
    const changed = vi.fn()
    renderPanel({ scaleFitRule: 'scaled', onScaleFitRuleChange: changed })
    const g = await screen.findByRole('group', { name: GROUP })
    fireEvent.click(within(g).getByRole('button', { name: EN['settings.workspace.scale.fit.off'] }))
    expect(changed).toHaveBeenLastCalledWith('classic')
    fireEvent.click(within(g).getByRole('button', { name: EN['settings.workspace.scale.fit.auto'] }))
    expect(changed).toHaveBeenLastCalledWith('scaled')
  })

  it('sits inside the UI scale row, which Text size still follows', async () => {
    renderPanel({ scaleFitRule: 'scaled', onScaleFitRuleChange: () => {} })
    const g = await screen.findByRole('group', { name: GROUP })
    const field = g.closest('.settings-field')!
    expect(field.querySelector('.settings-label')?.textContent).toBe(EN['settings.workspace.scale.label'])
  })

  it('is not offered with a pinned scale, which the rule never touches', async () => {
    renderPanel({ scaleMode: 100, scaleFitRule: 'scaled', onScaleFitRuleChange: () => {} })
    await screen.findByRole('group', { name: EN['settings.workspace.scale.aria'] }) // positive control
    expect(screen.queryByRole('group', { name: GROUP })).toBeNull()
  })

  it('is not offered where the rule cannot apply (no answer to show) or by a host that does not wire it', async () => {
    const { unmount } = renderPanel({ onScaleFitRuleChange: () => {} })
    await screen.findByRole('group', { name: EN['settings.workspace.scale.cap.aria'] }) // positive control
    expect(screen.queryByRole('group', { name: GROUP })).toBeNull()
    unmount()
    renderPanel({ scaleFitRule: 'scaled' })
    await screen.findByRole('group', { name: EN['settings.workspace.scale.cap.aria'] })
    expect(screen.queryByRole('group', { name: GROUP })).toBeNull()
  })
})
