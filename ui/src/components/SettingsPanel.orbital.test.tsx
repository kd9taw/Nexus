// @vitest-environment jsdom
//
// Settings ▸ Radio ▸ Orbital elements — the manual refresh speaks OPERATOR.
//
// What is pinned here (the honest-manual-refresh rework):
//
//  - The "Last refresh" line never speaks HTTP. While the mirror is
//    pre-launch-dead (404) and the elements are current, the line explains
//    calmly instead of alarming; the raw fetch error survives in the line's
//    title attribute for debugging — tooltip material, never the headline.
//  - The Update-now toast rides the same composer: a manual click whose
//    mirror leg died but whose Celestrak escalation landed announces
//    "fetched from Celestrak"; a genuinely failed attempt RESOLVES (the
//    command no longer rejects for a failed attempt) and toasts what failed
//    and what to do — again without HTTP in the headline.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import { pushToast } from '../toast'
import type { FeaturesApi } from '../useFeatures'
import type { TleStatus } from '../api'
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

const NOW = Math.floor(Date.now() / 1000)

function tleStatus(over: Partial<TleStatus> = {}): TleStatus {
  return {
    count: 97,
    usableCount: 97,
    agingCount: 0,
    heldBackCount: 0,
    fetchedAt: NOW - 3600,
    source: 'mirror',
    importedCount: 0,
    elementAgeDays: 0.3,
    blockedUntil: 0,
    ...over,
  }
}

/** The pre-launch state: mirror 404s, elements current. */
const mirror404 = () =>
  tleStatus({
    lastError: 'TLE mirror fetch failed: HTTP 404',
    lastErrorKind: 'mirrorUnreachable',
  })

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.24.5'))
  api.get('getTleStatus').mockImplementation(() => Promise.resolve(tleStatus()))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
  vi.mocked(pushToast).mockClear()
})
afterEach(cleanup)

const openRadio = async () => fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))

describe('the Last refresh line', () => {
  it('speaks operator for the pre-launch 404 mirror, raw error demoted to the tooltip', async () => {
    api.get('getTleStatus').mockImplementation(() => Promise.resolve(mirror404()))
    renderPanel()
    await openRadio()
    const line = await screen.findByText(/Last refresh:/)
    expect(line.textContent).toContain(
      "The element mirror isn't reachable (it goes live with the next release); your elements are 0.3 d old — current.",
    )
    // The headline never speaks HTTP…
    expect(line.textContent).not.toMatch(/HTTP|404/i)
    // …but the raw error survives for debugging, as the line's tooltip.
    expect(line.getAttribute('title')).toBe('TLE mirror fetch failed: HTTP 404')
  })
})

// The CHANGELOG claims the held-back birds are "counted and reported in their
// own right … the Settings line names the number". The number rode only the
// two mirrorUnreachable branches of the composer, and that line renders at all
// only while `lastError` is set — so on a landed refresh (the state after the
// mirror goes live) nothing named it. The count belongs to the fieldset's own
// status line, which is always there.
describe('the element counts in the fieldset status line', () => {
  it('names the held-back birds with no error in sight', async () => {
    api.get('getTleStatus').mockImplementation(() =>
      Promise.resolve(tleStatus({ count: 367, usableCount: 337, heldBackCount: 30 })),
    )
    renderPanel()
    await openRadio()
    expect(await screen.findByText(/367 birds · 30 sit out past 30 d/)).toBeTruthy()
  })

  it('a drifting set reads differently from a few slow-cadence birds', async () => {
    api.get('getTleStatus').mockImplementation(() =>
      Promise.resolve(
        tleStatus({ count: 140, usableCount: 100, agingCount: 49, heldBackCount: 40 }),
      ),
    )
    renderPanel()
    await openRadio()
    expect(await screen.findByText(/140 birds · 49 past 14 d · 40 sit out past 30 d/)).toBeTruthy()
  })

  it('a clean catalog states the count and nothing more', async () => {
    renderPanel()
    await openRadio()
    const line = await screen.findByText(/97 birds/)
    expect(line.textContent).not.toMatch(/sit out|past 14 d/)
  })
})

describe('the Update-now result', () => {
  it('announces a landed Celestrak escalation as such', async () => {
    api.get('fetchTlesNow').mockImplementation(() =>
      Promise.resolve(tleStatus({ source: 'celestrak', fetchedAt: NOW })),
    )
    renderPanel()
    await openRadio()
    fireEvent.click(await screen.findByRole('button', { name: 'Update now' }))
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(
        'Mirror unreachable — fetched from Celestrak: 97 birds',
        'success',
        expect.anything(),
      ),
    )
  })

  it('a failed attempt RESOLVES and toasts operator words, not HTTP', async () => {
    api.get('fetchTlesNow').mockImplementation(() =>
      Promise.resolve(
        tleStatus({
          lastError: 'TLE mirror fetch failed: HTTP 404; Celestrak TLE fetch failed: network: timeout',
          lastErrorKind: 'failed',
        }),
      ),
    )
    renderPanel()
    await openRadio()
    fireEvent.click(await screen.findByRole('button', { name: 'Update now' }))
    await waitFor(() => expect(pushToast).toHaveBeenCalled())
    const calls = vi.mocked(pushToast).mock.calls
    const [text, kind] = calls[calls.length - 1]
    expect(kind).toBe('error')
    expect(text).toBe(
      'Element update failed — no source delivered a usable set; retry shortly or import an element file.',
    )
    expect(text).not.toMatch(/HTTP/i)
  })

  it('the pre-launch 404 with current elements toasts calm info, not an error', async () => {
    api.get('fetchTlesNow').mockImplementation(() => Promise.resolve(mirror404()))
    renderPanel()
    await openRadio()
    fireEvent.click(await screen.findByRole('button', { name: 'Update now' }))
    await waitFor(() => expect(pushToast).toHaveBeenCalled())
    const calls = vi.mocked(pushToast).mock.calls
    const [text, kind] = calls[calls.length - 1]
    expect(kind).toBe('info')
    expect(text).toContain("The element mirror isn't reachable")
  })
})
