// @vitest-environment jsdom
//
// Settings ▸ Appearance ▸ Workspace ▸ Look — the one-tap looks row (features/looks.ts). The table
// and its promises over every screen are looks.test.ts; asserted here is the row as the operator
// meets it, inside the real panel, with a host that owns the five settings the way App does:
//   · it is the first thing in Workspace;
//   · it names the look on screen, or Custom, by reading the settings back;
//   · a tap applies the look through the setters it owns and reaches nothing else (theme, scale,
//     colours stay put), and NOTHING applies on mount;
//   · Undo puts back what the tap replaced, and goes away once the look is no longer on screen;
//   · the hint says what a look never changes.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within, act } from '@testing-library/react'
import { useState } from 'react'
import { SettingsPanel } from './SettingsPanel'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FeaturesApi } from '../useFeatures'
import { LOOKS, lookAxesOf, type LookAxes } from '../features/looks'
import { DEFAULT_SELECTION } from '../features/paletteRoles'
import { EN } from '../i18n'

// THE BUDGET (2026-10-09). The slowest case here, "offers the six looks in order, and names the one on…", takes
// 0.55 s and 0.58 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
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

const DEFAULTS: LookAxes = { fieldMode: false, night: 'off', highContrast: false, textSize: 'normal', density: 'standard' }

/** Every setter the panel is handed. The five a look owns write the host's state as App's hooks
 *  would; the rest are the ones a look must never reach. */
function spies() {
  return {
    setFieldMode: vi.fn(),
    setNight: vi.fn(),
    setHighContrast: vi.fn(),
    setTextSize: vi.fn(),
    setDensity: vi.fn(),
    setTheme: vi.fn(),
    setScaleMode: vi.fn(),
    setScaleCap: vi.fn(),
    setPalette: vi.fn(),
    setLocalClock: vi.fn(),
  }
}
type Spies = ReturnType<typeof spies>

function Host({ start, s }: { start: LookAxes; s: Spies }) {
  const [a, setA] = useState(start)
  const own =
    <K extends keyof LookAxes>(k: K, spy: (v: LookAxes[K]) => void) =>
    (v: LookAxes[K]) => {
      spy(v)
      setA((p) => ({ ...p, [k]: v }))
    }
  return (
    <SettingsPanel
      target="workspace"
      activeRadioId={0}
      scale={100 as never}
      scaleMode={'auto' as never}
      scaleCap={100 as never}
      onScaleModeChange={s.setScaleMode}
      onScaleCapChange={s.setScaleCap}
      density={a.density}
      onDensityChange={own('density', s.setDensity)}
      textSize={a.textSize}
      onTextSizeChange={own('textSize', s.setTextSize)}
      localClock={false}
      onLocalClockChange={s.setLocalClock}
      onResetLayout={() => {}}
      features={features}
      theme="dark"
      onThemeChange={s.setTheme}
      fieldMode={a.fieldMode}
      onFieldModeChange={own('fieldMode', s.setFieldMode)}
      highContrast={a.highContrast}
      onHighContrastChange={own('highContrast', s.setHighContrast)}
      night={a.night}
      onNightChange={own('night', s.setNight)}
      nightGridKnown
      palette={DEFAULT_SELECTION}
      onPaletteChange={s.setPalette}
    />
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

const LOOK = EN['settings.workspace.looks.label']
const row = () => screen.findByRole('group', { name: LOOK })
const pressed = (g: HTMLElement) =>
  within(g)
    .queryAllByRole('button')
    .filter((b) => b.getAttribute('aria-pressed') === 'true')
    .map((b) => b.textContent)
const chip = (g: HTMLElement, label: string) => within(g).getByRole('button', { name: label })
const custom = () => screen.queryByText(EN['settings.workspace.looks.custom'])
const undo = () => screen.queryByRole('button', { name: EN['settings.workspace.looks.undo'] })
const noWrites = (s: Spies) => {
  for (const [name, fn] of Object.entries(s)) expect(fn, `${name} was called`).not.toHaveBeenCalled()
}

describe('Settings ▸ Workspace ▸ Look', () => {
  it('is the first thing in Workspace, above UI scale', async () => {
    render(<Host start={DEFAULTS} s={spies()} />)
    const g = await row()
    const section = g.closest('fieldset')!
    expect(section.querySelector('legend')?.textContent).toBe(EN['settings.workspace.legend'])
    const labels = Array.from(section.querySelectorAll('.settings-label')).map((n) => n.textContent)
    expect(labels[0], 'the Look row is not the first row of Workspace').toBe(LOOK)
    // Language only renders once a second catalog is installed; UI scale is always there.
    expect(labels.indexOf(EN['settings.workspace.scale.label'])).toBeGreaterThan(0)
  })

  it('offers the six looks in order, and names the one on screen — for each of them', async () => {
    for (const look of LOOKS) {
      const { unmount } = render(<Host start={lookAxesOf(look, DEFAULTS)} s={spies()} />)
      const g = await row()
      expect(within(g).getAllByRole('button').map((b) => b.textContent)).toEqual(LOOKS.map((l) => EN[l.labelKey]))
      expect(pressed(g), look.id).toEqual([EN[look.labelKey]])
      expect(custom(), `${look.id} shows Custom`).toBeNull()
      unmount()
    }
  })

  it('says Custom, and presses nothing, when the settings match no look', async () => {
    // Large text at Standard density is no look's: the size looks set Normal or Larger.
    render(<Host start={{ ...DEFAULTS, textSize: 'large' }} s={spies()} />)
    const g = await row()
    expect(pressed(g)).toEqual([])
    expect(custom()).not.toBeNull()
    expect(custom()!.getAttribute('title')).toBe(EN['settings.workspace.looks.custom.title'])
  })

  it('applies nothing on mount, whatever the screen shows', async () => {
    for (const start of [DEFAULTS, { ...DEFAULTS, textSize: 'large' as const }, lookAxesOf(LOOKS[4], DEFAULTS)]) {
      const s = spies()
      const { unmount } = render(<Host start={start} s={s} />)
      await row()
      await act(async () => {})
      noWrites(s)
      unmount()
    }
  })

  it('a tap applies the look through its own setters, and never reaches the theme, scale or colours', async () => {
    const s = spies()
    render(<Host start={DEFAULTS} s={s} />)
    const g = await row()
    fireEvent.click(chip(g, EN['settings.workspace.looks.bigClear']))
    expect(s.setHighContrast).toHaveBeenCalledWith(true)
    expect(s.setTextSize).toHaveBeenCalledWith('larger')
    expect(s.setDensity).toHaveBeenCalledWith('guided')
    // Big & clear keeps Field mode and Night; the defaults already had them off.
    expect(s.setFieldMode).not.toHaveBeenCalled()
    expect(s.setNight).not.toHaveBeenCalled()
    for (const k of ['setTheme', 'setScaleMode', 'setScaleCap', 'setPalette', 'setLocalClock'] as const) {
      expect(s[k], k).not.toHaveBeenCalled()
    }
    expect(pressed(await row())).toEqual([EN['settings.workspace.looks.bigClear']])
  })

  it('a look that keeps Field mode and Night does keep them', async () => {
    const s = spies()
    render(<Host start={{ ...DEFAULTS, fieldMode: true, night: 'on' }} s={s} />)
    fireEvent.click(chip(await row(), EN['settings.workspace.looks.contest']))
    expect(s.setDensity).toHaveBeenCalledWith('dense')
    expect(s.setFieldMode).not.toHaveBeenCalled()
    expect(s.setNight).not.toHaveBeenCalled()
    expect(pressed(await row())).toEqual([EN['settings.workspace.looks.contest']])
  })

  it('a manual change to a setting the look owns reads Custom', async () => {
    render(<Host start={DEFAULTS} s={spies()} />)
    fireEvent.click(chip(await row(), EN['settings.workspace.looks.contest']))
    expect(pressed(await row())).toEqual([EN['settings.workspace.looks.contest']])
    const text = await screen.findByRole('group', { name: EN['settings.workspace.textSize.label'] })
    fireEvent.click(within(text).getByRole('button', { name: EN['settings.workspace.textSize.large'] }))
    expect(pressed(await row())).toEqual([])
    expect(custom()).not.toBeNull()
  })

  it('Undo puts back what the tap replaced, and nothing it did not', async () => {
    const s = spies()
    const mine: LookAxes = { fieldMode: false, night: 'auto', highContrast: true, textSize: 'large', density: 'dense' }
    render(<Host start={mine} s={s} />)
    expect(undo(), 'Undo before any tap').toBeNull()
    fireEvent.click(chip(await row(), EN['settings.workspace.looks.shack']))
    expect(pressed(await row())).toEqual([EN['settings.workspace.looks.shack']])
    fireEvent.click(undo()!)
    expect(s.setHighContrast).toHaveBeenLastCalledWith(true)
    expect(s.setTextSize).toHaveBeenLastCalledWith('large')
    expect(s.setDensity).toHaveBeenLastCalledWith('dense')
    // Night was on Auto: Shack leaves Auto alone, so there was nothing to put back.
    expect(s.setNight).not.toHaveBeenCalled()
    expect(pressed(await row())).toEqual([])
    expect(undo(), 'Undo after it was used').toBeNull()
  })

  it('Undo never reverts a setting the look does not own, changed after the tap', async () => {
    // Contest owns text size, density and contrast — not Field mode. Turn Field mode on after the
    // tap (a Field Day contest): Contest is still on screen, so Undo is still offered, and it must
    // put back Contest's own settings while Field mode stays exactly where the operator put it.
    const s = spies()
    render(<Host start={{ ...DEFAULTS, density: 'guided' }} s={s} />)
    fireEvent.click(chip(await row(), EN['settings.workspace.looks.contest']))
    const field = await screen.findByRole('group', { name: EN['settings.workspace.field.label'] })
    fireEvent.click(within(field).getByRole('button', { name: EN['settings.workspace.field.on'] }))
    expect(pressed(await row())).toEqual([EN['settings.workspace.looks.contest']])
    expect(undo(), 'Undo gone while the look is still on screen').not.toBeNull()
    s.setFieldMode.mockClear()
    fireEvent.click(undo()!)
    expect(s.setDensity).toHaveBeenLastCalledWith('guided')
    expect(s.setFieldMode, 'Undo reverted Field mode, which Contest never set').not.toHaveBeenCalled()
  })

  it('Undo goes away once the look is no longer on screen', async () => {
    render(<Host start={{ ...DEFAULTS, textSize: 'large' }} s={spies()} />)
    fireEvent.click(chip(await row(), EN['settings.workspace.looks.touch']))
    expect(undo()).not.toBeNull()
    const density = await screen.findByRole('group', { name: EN['settings.workspace.density.aria'] })
    fireEvent.click(within(density).getByRole('button', { name: EN['settings.workspace.density.dense'] }))
    expect(undo(), 'an Undo that would revert a change made after the look').toBeNull()
  })

  it('the hint says what a look never changes: the theme, UI scale and colours', async () => {
    render(<Host start={DEFAULTS} s={spies()} />)
    const g = await row()
    const hint = g.closest('.settings-field')!.querySelector('.settings-hint')!.textContent
    expect(hint).toBe(EN['settings.workspace.looks.hint'])
    for (const w of ['theme', 'UI scale', 'colours']) expect(hint, w).toContain(w)
  })

  it('each chip’s tooltip lists exactly the settings it sets', async () => {
    render(<Host start={DEFAULTS} s={spies()} />)
    const g = await row()
    const title = (id: string) =>
      chip(g, EN[LOOKS.find((l) => l.id === id)!.labelKey] as string).getAttribute('title') ?? ''
    for (const label of ['field', 'night', 'contrast', 'textSize', 'density'] as const) {
      expect(title('shack'), `Shack's tooltip lacks ${label}`).toContain(EN[`settings.workspace.${label}.label`])
    }
    expect(title('contest')).not.toContain(EN['settings.workspace.field.label'])
    expect(title('contest')).not.toContain(EN['settings.workspace.night.label'])
    expect(title('field')).not.toContain(EN['settings.workspace.contrast.label'])
  })

  it('is not offered by a host that does not wire all five settings', async () => {
    render(
      <SettingsPanel
        target="workspace"
        activeRadioId={0}
        scale={100 as never}
        scaleMode={'auto' as never}
        scaleCap={100 as never}
        onScaleModeChange={() => {}}
        onScaleCapChange={() => {}}
        density="standard"
        onDensityChange={() => {}}
        onResetLayout={() => {}}
        features={features}
      />,
    )
    // The Workspace section rendered (positive control), just without this row.
    await screen.findByRole('group', { name: EN['settings.workspace.density.aria'] })
    expect(screen.queryByRole('group', { name: LOOK })).toBeNull()
  })
})
