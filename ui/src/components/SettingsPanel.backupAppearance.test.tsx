// @vitest-environment jsdom
//
// Backup & reset carries the APPEARANCE (operator pick of 2026-09-26, "Per computer + Backup";
// features/appearanceBackup.ts is the file format). Asserted here, through the real panel:
//   · Back up saves the station's bundle with the look on screen added beside it;
//   · Restore hands the station the file exactly as picked, and only once the station has
//     accepted it puts the look back, through the same setters the rows use;
//   · a backup from before this has nothing to put back, and restores exactly as it always did;
//   · a restore the station refuses puts nothing back.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import { ConfirmHost } from '../confirm'
import type { FeaturesApi } from '../useFeatures'
import { DEFAULT_SELECTION } from '../features/paletteRoles'
import type { SkinId } from '../features/skins'
import { LOGBOOK_GLOBE_KEY } from '../features/logbookGlobe'
import { FT_PALETTE_SCOPE, WF_PALETTE_KEY } from '../waterfallPalette'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n'

// THE BUDGET (2026-10-09). The slowest case here, "a built-in theme travels with its base: backed up as it…", takes
// 0.34 s and 0.25 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
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
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => {
    try {
      return await fn()
    } catch {
      return undefined
    }
  }),
}))

const features = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

/** The station's own bundle, as serde writes it. */
const STATION = `{
  "app": "1.16.0",
  "kind": "nexus-settings-backup",
  "schema": 1,
  "settings": {
    "mycall": "KD9TAW"
  },
  "uiState": {}
}`

function setters() {
  return {
    setTheme: vi.fn(),
    setSkin: vi.fn(),
    setHighContrast: vi.fn(),
    setNight: vi.fn(),
    setTextSize: vi.fn(),
    setDensity: vi.fn(),
    setMotion: vi.fn(),
    setPalette: vi.fn(),
    setFieldMode: vi.fn(),
    setScaleMode: vi.fn(),
    setScaleCap: vi.fn(),
  }
}
type Setters = ReturnType<typeof setters>

function renderPanel(s: Setters, look: { theme?: 'system' | 'dark' | 'light'; skin?: SkinId | null } = {}) {
  return render(
    <>
      <SettingsPanel
        target="configurations"
        activeRadioId={0}
        scale={100 as never}
        scaleMode={'auto' as never}
        scaleCap={100 as never}
        onScaleModeChange={s.setScaleMode}
        onScaleCapChange={s.setScaleCap}
        density="dense"
        onDensityChange={s.setDensity}
        textSize="large"
        onTextSizeChange={s.setTextSize}
        onResetLayout={() => {}}
        features={features}
        theme={look.theme ?? 'system'}
        onThemeChange={s.setTheme}
        skin={look.skin ?? null}
        onSkinChange={s.setSkin}
        fieldMode
        onFieldModeChange={s.setFieldMode}
        highContrast
        onHighContrastChange={s.setHighContrast}
        night="auto"
        onNightChange={s.setNight}
        nightGridKnown
        palette={{ ...DEFAULT_SELECTION, accent: 'violet' }}
        onPaletteChange={s.setPalette}
        motion="reduce"
        onMotionChange={s.setMotion}
      />
      <ConfirmHost />
    </>,
  )
}

beforeEach(() => {
  localStorage.clear()
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.16.0'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
  api.get('exportSettingsBundle').mockImplementation(() => Promise.resolve(STATION))
  api.get('saveTextToDownloads').mockImplementation(() => Promise.resolve('/home/op/Downloads/x.json'))
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  localStorage.clear()
})

async function restore(text: string, container: HTMLElement) {
  await screen.findByText(EN['settings.transmit.backup.label'])
  const input = container.querySelector('input[type="file"][accept*="json"]') as HTMLInputElement
  fireEvent.change(input, { target: { files: [new File([text], 'nexus-settings.json', { type: 'application/json' })] } })
  await waitFor(() => expect(screen.getByRole('button', { name: EN['settings.backup.restore.confirm.action'] })).toBeTruthy())
  fireEvent.click(screen.getByRole('button', { name: EN['settings.backup.restore.confirm.action'] }))
  await waitFor(() => expect(api.get('importSettingsBundle')).toHaveBeenCalled())
}

describe('Backup & reset carries the appearance', () => {
  it('says so in the row, and says what stays with each computer', async () => {
    renderPanel(setters())
    const label = await screen.findByText(EN['settings.transmit.backup.label'])
    const hints = Array.from(label.closest('.settings-field')!.querySelectorAll('.settings-hint')).map((h) => h.textContent)
    expect(hints).toContain(EN['settings.transmit.backup.appearance'])
    expect(EN['settings.transmit.backup.appearance']).toMatch(/Field mode and UI scale stay with each computer/)
  })

  it('Back up saves the look on screen beside the station’s bundle', async () => {
    localStorage.setItem(WF_PALETTE_KEY, 'cividis')
    localStorage.setItem(LOGBOOK_GLOBE_KEY, 'off')
    renderPanel(setters())
    fireEvent.click(await screen.findByRole('button', { name: EN['settings.transmit.backup.action'] }))
    await waitFor(() => expect(api.get('saveTextToDownloads')).toHaveBeenCalled())
    const saved = JSON.parse(api.get('saveTextToDownloads').mock.calls[0][1] as string)
    expect(saved.settings).toEqual({ mycall: 'KD9TAW' })
    expect(saved.appearance).toEqual({
      theme: 'system',
      skin: null,
      highContrast: true,
      night: 'auto',
      textSize: 'large',
      density: 'dense',
      motion: 'reduce',
      colours: { ...DEFAULT_SELECTION, accent: 'violet' },
      waterfallPalette: 'cividis',
      ftWaterfallPalette: 'turbo',
      logbookGlobe: false,
    })
  })

  it('Restore gives the station the file as picked, then puts the look back', async () => {
    const s = setters()
    api.get('importSettingsBundle').mockResolvedValueOnce({ settings: defaultSettings })
    const file = JSON.stringify({
      ...JSON.parse(STATION),
      appearance: {
        theme: 'light',
        skin: 'paper',
        highContrast: false,
        night: 'on',
        textSize: 'larger',
        density: 'touch',
        motion: 'system',
        colours: { accent: 'blue', readout: 'green' },
        waterfallPalette: 'viridis',
        ftWaterfallPalette: 'cividis',
        logbookGlobe: false,
      },
    })
    const { container } = renderPanel(s)
    await restore(file, container)
    expect(api.get('importSettingsBundle')).toHaveBeenCalledWith(file)
    await waitFor(() => expect(s.setTheme).toHaveBeenCalledWith('light'))
    expect(s.setSkin).toHaveBeenCalledWith('paper')
    expect(s.setHighContrast).toHaveBeenCalledWith(false)
    expect(s.setNight).toHaveBeenCalledWith('on')
    expect(s.setTextSize).toHaveBeenCalledWith('larger')
    expect(s.setDensity).toHaveBeenCalledWith('touch')
    expect(s.setMotion).toHaveBeenCalledWith('system')
    expect(s.setPalette).toHaveBeenCalledWith('accent', 'blue')
    expect(s.setPalette).toHaveBeenCalledWith('readout', 'green')
    expect(s.setPalette).toHaveBeenCalledTimes(2) // the roles the file names, and only those
    expect(localStorage.getItem(WF_PALETTE_KEY)).toBe('viridis')
    expect(localStorage.getItem(`${WF_PALETTE_KEY}.${FT_PALETTE_SCOPE}`)).toBe('cividis')
    expect(localStorage.getItem(LOGBOOK_GLOBE_KEY)).toBe('off')
    // Field mode and UI scale are never restored.
    expect(s.setFieldMode).not.toHaveBeenCalled()
    expect(s.setScaleMode).not.toHaveBeenCalled()
    expect(s.setScaleCap).not.toHaveBeenCalled()
  })

  it('a built-in theme travels with its base: backed up as it paints, restored on it, and null brings the standard theme back', async () => {
    renderPanel(setters(), { theme: 'dark', skin: 'amber-lcd' })
    fireEvent.click(await screen.findByRole('button', { name: EN['settings.transmit.backup.action'] }))
    await waitFor(() => expect(api.get('saveTextToDownloads')).toHaveBeenCalled())
    const saved = JSON.parse(api.get('saveTextToDownloads').mock.calls[0][1] as string)
    expect({ theme: saved.appearance.theme, skin: saved.appearance.skin }).toEqual({ theme: 'dark', skin: 'amber-lcd' })
    cleanup()

    // A file whose theme disagrees with its built-in theme's base (hand-edited, say) still paints the
    // built-in theme: the base comes with it, applied after the file's own theme.
    let s = setters()
    api.get('importSettingsBundle').mockResolvedValueOnce({ settings: defaultSettings })
    const odd = JSON.stringify({ ...JSON.parse(STATION), appearance: { theme: 'light', skin: 'nebula' } })
    let { container } = renderPanel(s)
    await restore(odd, container)
    await waitFor(() => expect(s.setSkin).toHaveBeenCalledWith('nebula'))
    expect(s.setTheme.mock.calls).toEqual([['light'], ['dark']])
    cleanup()

    s = setters()
    api.get('importSettingsBundle').mockResolvedValueOnce({ settings: defaultSettings })
    const standard = JSON.stringify({ ...JSON.parse(STATION), appearance: { theme: 'dark', skin: null } })
    ;({ container } = renderPanel(s, { theme: 'dark', skin: 'slate' }))
    await restore(standard, container)
    await waitFor(() => expect(s.setSkin).toHaveBeenCalledWith(null))
    expect(s.setTheme.mock.calls).toEqual([['dark']])
  })

  it('a backup from before this restores the station and leaves the look alone', async () => {
    const s = setters()
    api.get('importSettingsBundle').mockResolvedValueOnce({ settings: defaultSettings })
    const { container } = renderPanel(s)
    await restore(STATION, container)
    expect(api.get('importSettingsBundle')).toHaveBeenCalledWith(STATION)
    await waitFor(() => expect(api.get('getSettings').mock.calls.length).toBeGreaterThan(1))
    for (const [name, fn] of Object.entries(s)) expect(fn, name).not.toHaveBeenCalled()
    expect(localStorage.getItem(WF_PALETTE_KEY)).toBeNull()
  })

  it('a restore the station refuses puts nothing back', async () => {
    const s = setters()
    api.get('importSettingsBundle').mockRejectedValueOnce(new Error('That backup is missing its format version.'))
    const file = JSON.stringify({ ...JSON.parse(STATION), appearance: { theme: 'light', waterfallPalette: 'viridis' } })
    const { container } = renderPanel(s)
    await restore(file, container)
    await new Promise((r) => setTimeout(r, 0))
    expect(s.setTheme).not.toHaveBeenCalled()
    expect(localStorage.getItem(WF_PALETTE_KEY)).toBeNull()
  })
})
