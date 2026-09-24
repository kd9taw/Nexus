// @vitest-environment jsdom
//
// Settings ▸ Confirmations ▸ World Radio League — "Export ADIF for WRL" writes the whole log for
// an import at worldradioleague.com. It is an export like the Logbook's, so it follows the same
// ruling (SPEC-2 C15): the file is written whatever the logbook database holds, and the screen
// says how many recent changes it lacks — nothing more when it lacks nothing.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { pushToast } from '../toast'
import { t } from '../i18n'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module (see SettingsPanel.lotwupload.test.tsx
// for why a hand-kept list is the trap).
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

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

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
  api.get('saveTextToDownloads').mockImplementation(() => Promise.resolve('/tmp/nexus-log-for-wrl.adi'))
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

/** Export for WRL with the station answering `saving`/`held`, and wait for the export's own
 *  toast — the control: the file was written. */
async function exportForWrl(saving: number, held: number) {
  api.get('exportGeneralLog').mockImplementation(() => Promise.resolve({ text: '<EOR>\n', saving, held }))
  render(
    <SettingsPanel
      target="confirmations"
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
  fireEvent.click(await screen.findByRole('button', { name: t('settings.confirmations.wrl.export.action') }))
  const done = t('settings.confirmations.wrl.export.done', { path: '/tmp/nexus-log-for-wrl.adi' })
  await waitFor(() => expect(vi.mocked(pushToast)).toHaveBeenCalledWith(done, 'success', 8000))
  expect(api.get('saveTextToDownloads')).toHaveBeenCalledWith('nexus-log-for-wrl.adi', '<EOR>\n')
}

const said = () => vi.mocked(pushToast).mock.calls.map((c) => c[0])

describe('the WRL export says what it lacks', () => {
  it('says how many recent changes the file lacks while they are still being saved', async () => {
    await exportForWrl(1, 0)
    expect(said()).toContain(t('logbook.export.lacks.saving', { count: 1 }))
  })

  it('says nothing more when the file lacks nothing', async () => {
    await exportForWrl(0, 0)
    expect(said().filter((m) => m.includes('not in the export'))).toEqual([])
  })
})
