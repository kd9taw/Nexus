// @vitest-environment jsdom
//
// THE BETA-CHANNEL SWITCH HAS ONE WRITER, AND IT IS NOT THE FORM.
//
// `betaUpdates` used to ride along in the whole-struct `setSettings` payload like any other
// checkbox. That made every surface that posts a settings payload a writer of it — and a
// payload is a snapshot from whenever its sender last read the settings. Some senders hold
// theirs for the life of the window: the APRS cockpit is mounted permanently (`.aprs-host`,
// hidden rather than unmounted) and refreshes its copy only after its own writes, so a control
// on it posts a `betaUpdates` that can be hours old.
//
// For most fields that is an ordinary stale-form revert. This one is different in the only way
// that matters: **its loss is silent in both directions.** A beta tester dropped back to the
// stable channel gets no error, no toast and no log line — the betas simply stop arriving, and
// the maintainer finds out when the feedback dries up. There is nothing to notice and nothing
// to grep for, which is why it gets a dedicated verb instead of one more careful caller.
//
// The backend half (a settings save cannot move the field) is pinned in
// `crates/tempo-app/src/engine.rs`. This is the frontend half: the switch must call the verb,
// and Save must not carry the field.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
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

// Mock EVERY export of `../api`, derived from the real module (the flexperradio pattern — a
// hand-kept list makes the panel throw on mount when a verb is added).
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

function lastCall(name: string): unknown[] | undefined {
  const calls = api.get(name).mock.calls
  return calls.length ? (calls[calls.length - 1] as unknown[]) : undefined
}

/** An operator already opted in — the state the loss is expensive from. */
function settingsFixture(betaUpdates: boolean) {
  return { ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', betaUpdates } as never
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

function setUp(betaUpdates: boolean) {
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.11.1'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsFixture(betaUpdates)))
}

beforeEach(() => setUp(false))
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

/** The footer Save (`type="submit"`) — other sections carry their own "Save …" buttons. */
function clickSave() {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
}

describe('the beta-channel switch writes through its own verb, not the settings form', () => {
  it('turning it on calls setBetaUpdates(true) and never setSettings', async () => {
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Appearance' }))
    fireEvent.click(await screen.findByRole('switch', { name: 'Turn on beta updates' }))

    await waitFor(() => expect(api.get('setBetaUpdates')).toHaveBeenCalled())
    expect(lastCall('setBetaUpdates')![0]).toBe(true)
    // Not the heavyweight path: a whole-struct save is exactly what a stale sender can undo.
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })

  it('turning it off calls setBetaUpdates(false) — the switch stays two-way', async () => {
    // The positive control. Without it the same code would pass by never calling the verb
    // with `false`, i.e. by making the opt-in impossible to leave.
    setUp(true)
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Appearance' }))
    fireEvent.click(await screen.findByRole('switch', { name: 'Turn off beta updates' }))

    await waitFor(() => expect(api.get('setBetaUpdates')).toHaveBeenCalled())
    expect(lastCall('setBetaUpdates')![0]).toBe(false)
  })

  it('an ordinary Save does not carry the operator OUT of the beta channel', async () => {
    // The regression itself, at the seam that produced it: the panel holds a whole-Settings
    // form, and every Save posts it. Whatever it says about `betaUpdates` must not be what
    // decides the channel — the backend keeps the live value, and this pins that the frontend
    // is not quietly relying on the payload to carry the operator's choice either.
    setUp(true)
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Station' }))
    clickSave()

    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    const payload = lastCall('setSettings')![0] as Record<string, unknown>
    expect(payload.betaUpdates).toBe(true) // never flipped to false by the form round trip
    expect(api.get('setBetaUpdates')).not.toHaveBeenCalled() // Save is not a channel change
  })
})
