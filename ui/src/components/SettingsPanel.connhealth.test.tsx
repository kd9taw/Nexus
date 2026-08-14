// @vitest-environment jsdom
//
// Settings ▸ Connections — the dot means "your QSOs are getting out", not "a secret exists".
//
// THE DEFECT. Every row's dot came from a keychain read: `stored ? 'on' : 'off'`. So a
// revoked ClubLog app-password, a rotated QRZ Logbook API key and a mistyped HRDLog upload
// code all stayed GREEN forever — the secret was still sitting in the keychain, which is
// the one thing that had not stopped being true. The single panel built to answer "are my
// contacts reaching the services?" answered a different question, and answered it
// reassuringly.
//
// What is pinned here is the render, not the derivation (ui/src/settings/connHealth.test.ts
// owns that): a failing connector must LOOK failed, a never-exercised one must not look
// healthy, and the service's own reason must be on screen.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
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
const dot = (r: HTMLElement) => r.querySelector('.conn-dot')?.className ?? ''

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

describe('the connector dot reports the last round trip, not the keychain', () => {
  it('shows a REVOKED credential as failing, with the service reason — it used to stay green', async () => {
    // The exact reported defect: the password was revoked at ClubLog, the push 403s, and
    // the secret is still in the keychain. `stored` is true and always will be.
    api.get('getCredentialsStatus').mockImplementation(() =>
      Promise.resolve([
        cred({
          lastSuccessUnix: 1_754_000_000,
          lastFailureUnix: 1_754_600_000,
          lastFailureDetail: 'ClubLog rejected the app password',
        }),
      ]),
    )
    renderPanel()
    const r = await row('ClubLog')
    expect(dot(r)).toMatch(/\bbad\b/)
    expect(dot(r)).not.toMatch(/\bon\b/)
    expect(r.textContent).toMatch(/ClubLog rejected the app password/)
  })

  it('does NOT call a stored-but-never-exercised connector healthy', async () => {
    // The honest-degradation case, and the whole point: never verified is amber, not green.
    // Without this the panel just moves the lie from "a secret exists" to "no news is good".
    api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([cred()]))
    renderPanel()
    const r = await row('ClubLog')
    expect(dot(r)).toMatch(/\bwarn\b/)
    expect(r.textContent).toMatch(/not verified yet/i)
  })

  it('shows a working connector green, with when it last got through', async () => {
    // The positive control. Without it, a dot hard-coded to "bad" would satisfy the two
    // assertions above and prove nothing.
    api.get('getCredentialsStatus').mockImplementation(() =>
      Promise.resolve([cred({ lastSuccessUnix: Math.floor(Date.now() / 1000) - 600 })]),
    )
    renderPanel()
    const r = await row('ClubLog')
    expect(dot(r)).toMatch(/\bon\b/)
    expect(r.textContent).toMatch(/last upload/i)
  })

  it('surfaces the ClubLog 403 kill-switch, which was only ever one line in the log', async () => {
    api.get('getCredentialsStatus').mockImplementation(() =>
      Promise.resolve([cred({ paused: true, lastSuccessUnix: 1_754_000_000 })]),
    )
    renderPanel()
    const r = await row('ClubLog')
    expect(dot(r)).toMatch(/\bbad\b/)
    expect(r.textContent).toMatch(/paused/i)
  })

  it('does not flag a lookup-only connector for never having uploaded', async () => {
    api.get('getCredentialsStatus').mockImplementation(() =>
      Promise.resolve([
        cred({ id: 'repeaterbook', connector: 'RepeaterBook', uploads: false, identity: '' }),
      ]),
    )
    renderPanel()
    const r = await row('RepeaterBook')
    expect(r.textContent).toMatch(/lookup only/i)
    expect(dot(r)).not.toMatch(/\bbad\b|\bwarn\b/)
  })

  it('keys the Test button on the stable id, not the prose label', async () => {
    // `connector === 'QRZ Logbook'` was a sentinel the consumer had to guess; rewording the
    // label silently removed the only credential test in the grid.
    api.get('getCredentialsStatus').mockImplementation(() =>
      Promise.resolve([
        cred({ id: 'qrz-logbook', connector: 'QRZ logbook (renamed)', identity: 'kd9taw' }),
      ]),
    )
    renderPanel()
    const r = await row('QRZ logbook (renamed)')
    expect(r.querySelector('.settings-test-btn')).toBeTruthy()
  })
})
