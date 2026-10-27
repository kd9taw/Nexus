// @vitest-environment jsdom
//
// The one Connect to my station… entry in Settings ▸ Remote access, on the PC an operator works FROM:
// it opens the window that finds the station on this network first and otherwise signs in to reach it
// over the internet. It replaced the two entries that were here (Remote stations… and Stations on
// this network…). Pinned here: it is offered on Windows only, at the END of the card (the beta line
// and the streaming switch stay first, as ordered on 2026-10-02), one click asks Rust to open the
// window once, in the app's language, and a failure says so in words.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n/en'

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

function renderPanel() {
  const panel = (
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
    />
  )
  return render(panel)
}

async function openRemoteTab() {
  fireEvent.click(await screen.findByRole('tab', { name: 'Remote' }))
  await waitFor(() => expect(document.getElementById('settings-remote-access')).toBeTruthy())
}

const card = () => document.getElementById('settings-remote-access')!
const theButton = () => screen.queryByRole('button', { name: EN['settings.connectStation.open'] })
const opens = () => api.get('openLanStationsWindow')

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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.16.0'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' }),
  )
})
afterEach(cleanup)

describe('on Windows', () => {
  it('is offered at the end of Remote access, after the streaming switch, and says what F11 and Esc do', async () => {
    renderPanel()
    await openRemoteTab()
    const button = await waitFor(() => theButton()!)
    expect(card().contains(button)).toBe(true)
    const streaming = screen.getByRole('switch', { name: EN['settings.remoteStream.label'] })
    expect(streaming.compareDocumentPosition(button) & Node.DOCUMENT_POSITION_FOLLOWING, 'after the switch').toBeTruthy()
    const hint = screen.getByText(EN['settings.connectStation.hint'])
    expect(hint.closest('.settings-field')?.contains(button)).toBe(true)
    expect(hint.textContent).toMatch(/F11 switches the window to full screen and back, and Esc over the picture still stops transmitting/)
  })

  it('asks Rust to open the window once per click, and holds the button while it does', async () => {
    let finish: () => void = () => {}
    opens().mockImplementation(() => new Promise<void>((resolve) => { finish = resolve }))
    renderPanel()
    await openRemoteTab()
    const button = await waitFor(() => theButton()!)
    fireEvent.click(button)
    expect(opens()).toHaveBeenCalledTimes(1)
    expect(opens().mock.calls[0]).toEqual(['en'])
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(true))
    finish()
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false))
    expect(screen.queryByText(EN['settings.connectStation.failed'])).toBeNull()
  })

  it('is the only way in: the two entries it replaced are gone', async () => {
    renderPanel()
    await openRemoteTab()
    await waitFor(() => theButton()!)
    // Their old labels, written out: the strings themselves left the catalogs with the entries.
    expect(screen.queryByRole('button', { name: 'Remote stations…' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Stations on this network…' })).toBeNull()
  })

  it('says so in words when the window could not be opened, and lets the operator try again', async () => {
    opens().mockImplementation(() => Promise.reject(new Error('remoteUnreachable')))
    renderPanel()
    await openRemoteTab()
    const button = await waitFor(() => theButton()!)
    fireEvent.click(button)
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe(EN['settings.connectStation.failed'])
    expect((button as HTMLButtonElement).disabled).toBe(false)
    opens().mockImplementation(() => Promise.resolve(null))
    fireEvent.click(button)
    await waitFor(() => expect(screen.queryByText(EN['settings.connectStation.failed'])).toBeNull())
    expect(opens()).toHaveBeenCalledTimes(2)
  })
})

describe('where the window is not offered', () => {
  it('says to use a browser on Linux and macOS, with no button', async () => {
    platform.windows = false
    renderPanel()
    await openRemoteTab()
    await waitFor(() => expect(screen.getByText(EN['settings.remoteStations.unavailable'])).toBeTruthy())
    expect(card().contains(screen.getByText(EN['settings.remoteStations.unavailable']))).toBe(true)
    expect(theButton()).toBeNull()
  })

})
