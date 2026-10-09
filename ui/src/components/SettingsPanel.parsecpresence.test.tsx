// @vitest-environment jsdom
//
// Parsec presence mode's switch in Settings ▸ Radio ▸ Transmit limits & sharing — a SIGNED-OFF
// transmit-path feature (operator, 2026-09-27): stop-only, and OFF unless the operator turns it
// on. What is pinned here is what the operator sees and what Save writes: the switch ships off,
// it writes `parsecPresenceStop`, it says what the watcher found once it is on, and a station
// that cannot host Parsec is told so instead of being offered a switch that would do nothing.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { ParsecPresence } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n/en'

// THE BUDGET (2026-10-09). The slowest case here, "offers the switch, OFF, and says what it watches and…", takes
// 0.26 s and 0.29 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const platform = vi.hoisted(() => ({ windows: true }))
vi.mock('../platform', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  return {
    ...actual,
    get IS_WINDOWS() {
      return platform.windows
    },
  }
})

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

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function station(extra: Record<string, unknown> = {}) {
  return { ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', ...extra } as never
}

function renderPanel(parsecPresence?: ParsecPresence | null) {
  return render(
    <SettingsPanel
      activeRadioId={0}
      parsecPresence={parsecPresence}
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

async function openRadioTab() {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  // The fieldset the switch lives in, so an absent switch is not an absent tab.
  await waitFor(() => expect(document.getElementById('settings-transmit-limits')).toBeTruthy())
}

const theSwitch = () =>
  screen.queryByRole('switch', { name: EN['settings.transmit.parsecStop.label'] })

function clickSave() {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
}

const lastSaved = () => {
  const calls = api.get('setSettings').mock.calls
  return calls[calls.length - 1][0] as Record<string, unknown>
}

beforeEach(() => {
  platform.windows = true
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.15.0'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(station()))
})
afterEach(cleanup)

describe('on a Windows station — the Parsec host', () => {
  it('offers the switch, OFF, and says what it watches and what it stops', async () => {
    renderPanel(null)
    await openRadioTab()
    const sw = await waitFor(() => {
      const s = theSwitch()
      expect(s, 'the switch is not on the Radio tab').toBeTruthy()
      return s!
    })
    expect(sw.getAttribute('aria-checked')).toBe('false')
    expect(screen.getByText(EN['settings.transmit.parsecStop.hint'])).toBeTruthy()
  })

  it('writes parsecPresenceStop on Save, and only when it was turned on', async () => {
    renderPanel(null)
    await openRadioTab()
    const sw = await waitFor(() => theSwitch()!)
    fireEvent.click(sw)
    expect(sw.getAttribute('aria-checked')).toBe('true')
    clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(lastSaved().parsecPresenceStop).toBe(true)
  })

  it('says what the watcher found once the mode is on', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(station({ parsecPresenceStop: true })),
    )
    renderPanel({ status: 'connected', stoppedAt: null, stopped: [] })
    await openRadioTab()
    await waitFor(() =>
      expect(screen.getByText(EN['settings.transmit.parsecStop.status.connected'])).toBeTruthy(),
    )
  })

  it('POSITIVE CONTROL: with the switch off there is no readout, whatever the station says', async () => {
    renderPanel({ status: 'connected', stoppedAt: null, stopped: [] })
    await openRadioTab()
    await waitFor(() => expect(theSwitch()).toBeTruthy())
    expect(screen.queryByText(EN['settings.transmit.parsecStop.status.connected'])).toBeNull()
  })
})

describe('on a station that cannot host Parsec', () => {
  it('explains instead of offering a switch that would do nothing', async () => {
    platform.windows = false
    renderPanel(null)
    await openRadioTab()
    await waitFor(() =>
      expect(screen.getByText(EN['settings.transmit.parsecStop.unavailable'])).toBeTruthy(),
    )
    expect(theSwitch()).toBeNull()
  })
})
