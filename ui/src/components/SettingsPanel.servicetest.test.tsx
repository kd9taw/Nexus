// @vitest-environment jsdom
//
// Settings ▸ Connections — the LoTW, eQSL and ClubLog Test buttons. Each calls its own read-only
// Test command and shows the answer; none may reach an upload or a download that merges into the
// log. The backend's half (which request each Test sends) is pinned in src-tauri's
// `connector_checks` and in each tempo-core connector's `login_test_tests`.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { CredStatus } from '../types'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
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

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

/** A row with everything healthy; each case overrides just what it is about. */
function cred(over: Partial<CredStatus> = {}): CredStatus {
  return {
    id: 'clublog',
    connector: 'ClubLog',
    stored: true,
    identity: 'kd9taw@example.invalid',
    uploads: true,
    enabled: true,
    lastSuccessUnix: null,
    lastFailureUnix: null,
    lastFailureDetail: null,
    paused: false,
    ...over,
  }
}

function renderPanel() {
  return render(
    <SettingsPanel
      target="connections"
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

/** The row's own element, by the connector name in its `.conn-name` cell. Scoped to the
 *  grid on purpose — "ClubLog" is also a featgroup title further down the same tab. */
async function row(name: string) {
  return await waitFor(() => {
    const found = [...document.querySelectorAll('.conn-status-row')].find(
      (r) => r.querySelector('.conn-name')?.textContent === name,
    )
    if (!found) throw new Error(`no connections row named ${name}`)
    return found as HTMLElement
  })
}

beforeEach(() => {
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
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


/** Every api verb that uploads to a service or merges a download into the log. */
const SENDS_OR_MERGES = [
  'uploadLotwReport',
  'uploadLotwReportByIds',
  'markLotwUploaded',
  'downloadLotwReport',
  'downloadEqslReport',
  'eqslPushQso',
  'clublogPushQso',
  'qrzPushQso',
  'syncQrz',
]

const TESTS = [
  { id: 'lotw', connector: 'LoTW', verb: 'lotwTestConnection' },
  { id: 'eqsl', connector: 'eQSL', verb: 'eqslTestConnection' },
  { id: 'clublog', connector: 'ClubLog', verb: 'clublogTestConnection' },
]

describe('the LoTW, eQSL and ClubLog Test buttons', () => {
  for (const { connector, verb } of TESTS) {
    it(`${connector}: Test calls only its read-only check and shows the answer`, async () => {
      api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve(TESTS.map((t) => cred({ id: t.id, connector: t.connector }))))
      api.get(verb).mockImplementation(() => Promise.resolve(`${connector} accepted the login.`))
      renderPanel()
      const r = await row(connector)
      const btn = r.querySelector('.settings-test-btn') as HTMLButtonElement
      expect(btn).toBeTruthy()
      fireEvent.click(btn)
      await waitFor(() => expect(document.querySelector('.conn-test-result.ok')?.textContent).toBe(`✓ ${connector}: ${connector} accepted the login.`))
      expect(api.get(verb)).toHaveBeenCalledTimes(1)
      // Negative control: the other Tests and every upload / merging download stay untouched.
      for (const other of TESTS.filter((t) => t.verb !== verb)) expect(api.get(other.verb)).not.toHaveBeenCalled()
      for (const sends of SENDS_OR_MERGES) expect(api.get(sends)).not.toHaveBeenCalled()
    })
  }

  it("shows a refusal in the service's own words", async () => {
    api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([cred({ id: 'eqsl', connector: 'eQSL' })]))
    api.get('eqslTestConnection').mockImplementation(() => Promise.reject('eQSL said: Error: No such Callsign found'))
    renderPanel()
    fireEvent.click((await row('eQSL')).querySelector('.settings-test-btn') as HTMLButtonElement)
    await waitFor(() =>
      expect(document.querySelector('.conn-test-result.fail')?.textContent).toBe(
        '✗ eQSL test failed: eQSL said: Error: No such Callsign found',
      ),
    )
  })

  it('gives no Test to a row without a read-only check', async () => {
    // Control: the rows the Tests are keyed on do get one, so the absence below means something.
    api.get('getCredentialsStatus').mockImplementation(() =>
      Promise.resolve([
        cred({ id: 'hrdlog', connector: 'HRDLog.net' }),
        cred({ id: 'qrz-xml', connector: 'QRZ callbook (name/QTH)', uploads: false }),
        cred({ id: 'clublog', connector: 'ClubLog' }),
      ]),
    )
    renderPanel()
    expect((await row('ClubLog')).querySelector('.settings-test-btn')).toBeTruthy()
    expect((await row('HRDLog.net')).querySelector('.settings-test-btn')).toBeNull()
    expect((await row('QRZ callbook (name/QTH)')).querySelector('.settings-test-btn')).toBeNull()
  })
})
