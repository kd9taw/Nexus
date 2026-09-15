// @vitest-environment jsdom
//
// #289 — Settings ▸ Config ▸ Data & log folder.
//
// The safety half is Rust (src-tauri: the pointer is written LAST, the copy is verified byte for
// byte, nothing is ever moved or deleted, a folder that would open an empty logbook is refused).
// This pins the operator-facing half: the control exists where the registry says it does, it says
// which folder is in use and that a change waits for a restart, and the two buttons are the two
// different acts — adopt a folder, or copy the log there first.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
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

const FOLDER = {
  current: '/home/op/.config/tempo',
  default: '/home/op/.config/tempo',
  source: 'default',
  chosen: null,
  logPresent: true,
}

beforeEach(() => {
  for (const name of ['getRigModels', 'getAllRigModels', 'getSerialPortsDetailed', 'getBandPlan',
    'getCredentialsStatus', 'getConnectionLog', 'detectRigs']) api.get(name).mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.13.0'))
  api.get('getSettings').mockImplementation(() => Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW' } as never))
  api.get('getDataFolder').mockImplementation(() => Promise.resolve(FOLDER))
  api.get('setDataFolder').mockImplementation(() => Promise.resolve({ files: 3, bytes: 42 }))
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

function renderPanel() {
  return render(
    <SettingsPanel
      target="data-folder"
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
    />,
  )
}

const useBtn = () => screen.getByRole('button', { name: EN['settings.dataFolder.use'] })
const copyBtn = () => screen.getByRole('button', { name: EN['settings.dataFolder.copy'] })
const path = () => screen.getByLabelText(EN['settings.dataFolder.path.label']) as HTMLInputElement

describe('#289 the data & log folder control', () => {
  it('says which folder is in use, and that a change waits for a restart', async () => {
    renderPanel()
    await waitFor(() => expect(api.get('getDataFolder')).toHaveBeenCalled())
    expect(await screen.findByText(FOLDER.current)).toBeTruthy()
    expect(screen.getByText(EN['settings.dataFolder.restart'])).toBeTruthy()
    // The warning an operator needs BEFORE putting a log on a synced drive.
    expect(screen.getByText(EN['settings.dataFolder.note'])).toBeTruthy()
  })

  it('offers nothing to press until a folder is typed', async () => {
    renderPanel()
    await waitFor(() => expect(api.get('getDataFolder')).toHaveBeenCalled())
    expect(useBtn().hasAttribute('disabled')).toBe(true)
    expect(copyBtn().hasAttribute('disabled')).toBe(true)
    fireEvent.change(path(), { target: { value: '  ' } })
    expect(useBtn().hasAttribute('disabled')).toBe(true) // whitespace is not a folder
    fireEvent.change(path(), { target: { value: '/mnt/nas/nexus' } })
    expect(useBtn().hasAttribute('disabled')).toBe(false)
  })

  it('adopting a folder and copying into it are two different acts', async () => {
    renderPanel()
    await waitFor(() => expect(api.get('getDataFolder')).toHaveBeenCalled())
    fireEvent.change(path(), { target: { value: '/mnt/nas/nexus' } })

    fireEvent.click(useBtn())
    await waitFor(() => expect(api.get('setDataFolder')).toHaveBeenCalledWith('/mnt/nas/nexus', false))

    fireEvent.click(copyBtn())
    await waitFor(() => expect(api.get('setDataFolder')).toHaveBeenCalledWith('/mnt/nas/nexus', true))
    // The copy reports what it carried and verified, not a bare "done".
    expect(await screen.findByText(EN['settings.dataFolder.copied']
      .replace('{{files}}', '3').replace('{{bytes}}', '42'))).toBeTruthy()
  })

  it('can go back to the default folder', async () => {
    renderPanel()
    await waitFor(() => expect(api.get('getDataFolder')).toHaveBeenCalled())
    fireEvent.click(screen.getByRole('button', { name: EN['settings.dataFolder.reset'] }))
    await waitFor(() => expect(api.get('clearDataFolder')).toHaveBeenCalled())
  })
})
