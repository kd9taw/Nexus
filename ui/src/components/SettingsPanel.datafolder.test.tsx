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
  // Stated rather than left off. These are the values under test below, and a fixture that simply
  // omitted them would make `undefined` the "no warning" case — the warning tests would then pass
  // against a panel that never reads the fields at all.
  network: false,
  syncSuspected: false,
}

beforeEach(() => {
  for (const name of ['getRigModels', 'getAllRigModels', 'getSerialPortsDetailed', 'getBandPlan',
    'getCredentialsStatus', 'getConnectionLog', 'detectRigs']) api.get(name).mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.13.0'))
  api.get('getSettings').mockImplementation(() => Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW' } as never))
  api.get('getDataFolder').mockImplementation(() => Promise.resolve(FOLDER))
  api.get('setDataFolder').mockImplementation(() => Promise.resolve({ files: 3, bytes: 42 }))
  // The picker is desktop-only, so the panel asks. Mocked EXPLICITLY: the blanket mock above
  // hands every export a `() => Promise.resolve(null)`, and a Promise is truthy — Browse would
  // render for the wrong reason and the browser case could never be expressed.
  api.get('isTauri').mockImplementation(() => true)
  api.get('pickDataFolder').mockImplementation(() => Promise.resolve(null))
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
const browseBtn = () => screen.queryByRole('button', { name: EN['settings.dataFolder.browse'] })
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

  // #289 follow-up — the operator's report was "there is no windows explorer popup to select a
  // folder, I don't want users having to type out a full filesystem path". What can be pinned
  // here is the wiring: which command the button reaches, what it does with the answer, and
  // that it never became the ONLY way in. Whether a native dialog actually appears is not
  // observable in jsdom and is the operator's machine to confirm.
  describe('Browse', () => {
    it('fills the field from the folder the operator picked, and adopts nothing by itself', async () => {
      api.get('pickDataFolder').mockImplementation(() => Promise.resolve('D:\\Ham\\Nexus'))
      renderPanel()
      await waitFor(() => expect(api.get('getDataFolder')).toHaveBeenCalled())

      fireEvent.click(browseBtn()!)
      await waitFor(() => expect(api.get('pickDataFolder')).toHaveBeenCalled())
      await waitFor(() => expect(path().value).toBe('D:\\Ham\\Nexus'))
      // Picking is not choosing: the folder is only adopted by Use / Copy, which is what keeps
      // the copy-or-not decision in the operator's hands.
      expect(api.get('setDataFolder')).not.toHaveBeenCalled()

      fireEvent.click(useBtn())
      await waitFor(() => expect(api.get('setDataFolder')).toHaveBeenCalledWith('D:\\Ham\\Nexus', false))
    })

    it('leaves a typed path alone when the dialog is cancelled', async () => {
      api.get('pickDataFolder').mockImplementation(() => Promise.resolve(null))
      renderPanel()
      await waitFor(() => expect(api.get('getDataFolder')).toHaveBeenCalled())
      fireEvent.change(path(), { target: { value: '\\\\nas\\ham\\nexus' } })

      fireEvent.click(browseBtn()!)
      await waitFor(() => expect(api.get('pickDataFolder')).toHaveBeenCalled())
      expect(path().value).toBe('\\\\nas\\ham\\nexus')
      expect(useBtn().hasAttribute('disabled')).toBe(false)
    })

    it('a typed path still works with no picker in reach, and the dead button is not shown', async () => {
      // A browser context: api.ts's bridge() throws, isTauri() is false. The UNC path is the
      // reason the field must survive this — no OS folder dialog will reach one.
      api.get('isTauri').mockImplementation(() => false)
      renderPanel()
      await waitFor(() => expect(api.get('getDataFolder')).toHaveBeenCalled())

      expect(browseBtn()).toBeNull()
      fireEvent.change(path(), { target: { value: '\\\\nas\\ham\\nexus' } })
      fireEvent.click(useBtn())
      await waitFor(() => expect(api.get('setDataFolder')).toHaveBeenCalledWith('\\\\nas\\ham\\nexus', false))
      expect(api.get('pickDataFolder')).not.toHaveBeenCalled()
    })

    it('is shown at all when the desktop shell is there — the control for the check above', async () => {
      renderPanel()
      await waitFor(() => expect(api.get('getDataFolder')).toHaveBeenCalled())
      expect(browseBtn()).not.toBeNull()
    })
  })

  // C8 — NEXUS_DATA_DIR and a hand-edited data-dir.json both reach the folder in use without
  // passing the Rust refusal, and neither can be undone by relocating the operator's log. This
  // readout is what covers them, so it has to be shown to appear AND to stay away.
  describe('the folder in use is reported when it is somewhere a database is at risk', () => {
    const withFolder = async (extra: Partial<typeof FOLDER>) => {
      api.get('getDataFolder').mockImplementation(() => Promise.resolve({ ...FOLDER, ...extra }))
      renderPanel()
      await waitFor(() => expect(api.get('getDataFolder')).toHaveBeenCalled())
    }

    it('warns when the folder in use is on network storage', async () => {
      await withFolder({ network: true, current: '/mnt/nas/nexus' })
      expect(await screen.findByText(EN['settings.dataFolder.onNetwork'])).toBeTruthy()
      expect(screen.queryByText(EN['settings.dataFolder.onSync'])).toBeNull()
    })

    it('warns separately when the folder in use only LOOKS synced', async () => {
      await withFolder({ syncSuspected: true, current: '/home/op/Dropbox/nexus' })
      expect(await screen.findByText(EN['settings.dataFolder.onSync'])).toBeTruthy()
      // The heuristic must not borrow the certain warning's words — one refuses, one does not.
      expect(screen.queryByText(EN['settings.dataFolder.onNetwork'])).toBeNull()
    })

    it('says neither about an ordinary local folder', async () => {
      // POSITIVE CONTROL for both assertions above: the same panel, the same render path, with
      // the two flags false. Without it a panel that showed both warnings unconditionally would
      // pass every check in this block.
      await withFolder({})
      expect(screen.getByText(FOLDER.current)).toBeTruthy()
      expect(screen.queryByText(EN['settings.dataFolder.onNetwork'])).toBeNull()
      expect(screen.queryByText(EN['settings.dataFolder.onSync'])).toBeNull()
    })
  })

  it('can go back to the default folder', async () => {
    renderPanel()
    await waitFor(() => expect(api.get('getDataFolder')).toHaveBeenCalled())
    fireEvent.click(screen.getByRole('button', { name: EN['settings.dataFolder.reset'] }))
    await waitFor(() => expect(api.get('clearDataFolder')).toHaveBeenCalled())
  })
})
