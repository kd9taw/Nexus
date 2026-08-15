// @vitest-environment jsdom
// Back up / restore the whole station (#28 item 4). There was no way to keep a copy of any of
// this: settings.json sits in a config folder most operators never open.
//
// The claim worth pinning is the SAFETY one. The file goes to Downloads and operators mail these
// to each other for help, so the hint promises it holds no passwords or keys — and that promise
// is only true because the export redacts the ClubLog key, which IS in settings.json and which
// ClubLog auto-revokes once it becomes public. If the wording and the redaction ever drift
// apart, the wording is the one that gets believed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  // SettingsPanel pulls ~50 verbs from ../api. They all resolve null, which is enough for a
  // mount — the assertions here are about WHICH save verb gets called, not what it returns.
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module rather than a hand-kept list.
//
// The list was the problem: a verb missing from it made the panel THROW ON MOUNT ("No export is
// defined on the mock"), which presents as a behaviour regression in whichever test happened to
// run -- not as the out-of-date mock it actually is. Reading the real module's export names makes
// that failure impossible by construction.
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

const FTDX10 = {
  id: 0,
  name: 'FTDX10',
  enabled: true,
  serialPort: 'COM3',
  baud: 38400,
  rigModel: 1042,
  rigModelName: 'Yaesu FTDX10',
  rigConn: 'serial',
  rigAddr: '',
  rigctldPort: 4532,
  rotctldPort: 4533,
  icomNativeCat: false,
  audioIn: 'in-0',
  audioOut: 'out-0',
  txLevel: 1,
  rxGain: 1,
  pttMethod: 'cat',
  rotatorModel: 0,
  rotatorPort: '',
  rotatorBaud: 9600,
  rotatorHost: '',
  nativeScope: 'auto',
  bands: [],
}
const IC9700 = {
  ...FTDX10,
  id: 1,
  name: 'IC-9700',
  serialPort: 'COM7',
  baud: 115200,
  rigModel: 3081,
  rigModelName: 'Icom IC-9700',
  rigctldPort: 4534,
  rotctldPort: 4535,
  icomNativeCat: true,
  audioIn: 'in-1',
  audioOut: 'out-1',
}

/** Settings with two radios, FTDX10 active, flat mirror = the FTDX10.
 * Built on the real `Settings::default()` (dumped from Rust) so the panel renders for real
 * rather than against a hand-guessed subset that drifts. */
function twoRadioSettings() {
  return {
    ...defaultSettings,
    ...FTDX10,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [FTDX10, IC9700],
    band: '20m',
    dialMhz: 14.074,
    sideband: 'USB',
  } as never
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function renderPanel() {
  return render(
    <SettingsPanel
      activeRadioId={0}
      scale={1 as never}
      scaleMode={'auto' as never}
      scaleCap={1 as never}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density={'comfortable' as never}
      onDensityChange={() => {}}
      onResetLayout={() => {}}
      features={features}
    />,
  )
}

beforeEach(() => {
  // Reset in place — the mocked module captured these exact spy objects at import time, so
  // replacing them here would leave the component calling the old ones.
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.17.12'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(twoRadioSettings()))
  api.get('probeCatPorts').mockImplementation(() =>
    Promise.resolve({
      found: true,
      detail: 'Icom IC-9700 on COM7 @ 115200 baud',
      portName: 'COM7',
      baud: 115200,
      model: 3081,
      modelName: 'Icom IC-9700',
      freqMhz: 144.174,
      modelSeeded: false,
    }),
  )
  api.get('testCat').mockImplementation(() => Promise.resolve({ ok: true, detail: 'ok' }))
})
afterEach(cleanup)

/** Click "Edit" on the non-active radio (the IC-9700) so the flat rig form describes it. */
describe('backing up the station', () => {
  async function backupRow() {
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    const label = await screen.findByText('Back up your setup')
    return label.closest('.settings-field') as HTMLElement
  }

  // RESTORE MUST REFRESH WHAT IS ON SCREEN. `import_settings_bundle` used to hand-roll
  // `eng.apply_settings()` and return nothing, instead of routing through `set_settings` the way
  // `reset_settings` does. So it applied the bundle to the running engine, persisted nothing, and
  // handed the panel no snapshot -- which went on rendering the PRE-restore form. To the operator
  // (2026-08-14) Restore appeared to do nothing at all.
  //
  // And the stale form stayed live, so the next Save wrote the OLD values back over the restored
  // ones: a silent revert of an explicit, confirmed, "this cannot be undone" action. Nothing
  // tested this path at all -- there was not one reference to importSettingsBundle in the suite.
  it('re-reads the settings after a restore, so the panel cannot show stale values', async () => {
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true)
    const { container } = renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))

    // NOT the first file input: the TLE importer is also one and comes first in the DOM.
    // Pick the restore input by what it accepts, or this drives the wrong feature entirely.
    const input = container.querySelector(
      'input[type="file"][accept*="json"]',
    ) as HTMLInputElement
    expect(input).toBeTruthy()

    // The real command returns the refreshed AppSnapshot; the blanket mock returns null for
    // every verb, and a null snapshot is exactly the "something went wrong" case the handler
    // must NOT treat as success.
    api.get('importSettingsBundle').mockResolvedValueOnce({ settings: defaultSettings })
    const bundle = JSON.stringify({ kind: 'nexus-settings-backup', schema: 1, settings: {} })
    const file = new File([bundle], 'nexus-backup.json', { type: 'application/json' })
    const before = api.get('getSettings').mock.calls.length
    fireEvent.change(input, { target: { files: [file] } })

    await waitFor(() => expect(api.get('importSettingsBundle')).toHaveBeenCalled())
    // The point: settings are re-read AFTER the import, not left to the stale form.
    await waitFor(() =>
      expect(api.get('getSettings').mock.calls.length).toBeGreaterThan(before),
    )
    confirmSpy.mockRestore()
  })

  it('offers both halves — a backup is no use without a restore', async () => {
    renderPanel()
    const row = await backupRow()
    const labels = [...row.querySelectorAll('button')].map((b) => b.textContent?.trim())
    expect(labels).toContain('Back up')
    expect(labels).toContain('Restore…')
  })

  it('says plainly that it carries no passwords or keys', async () => {
    renderPanel()
    const row = await backupRow()
    const hint = row.querySelector('.settings-hint')!.textContent ?? ''
    expect(hint).toMatch(/no\s+passwords or API keys/i)
    // And that the log is NOT in it — otherwise operators hand around every contact they have
    // made without realising.
    expect(hint).toMatch(/log is separate|contact log/i)
    // Restore is destructive; the hint has to say so before the click, not after.
    expect(hint).toMatch(/replaces your current setup/i)
  })
})
