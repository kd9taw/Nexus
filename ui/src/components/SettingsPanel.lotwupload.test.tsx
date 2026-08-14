// @vitest-environment jsdom
//
// Settings ▸ Confirmations ▸ LoTW — the button says which DIRECTION it goes, and the
// automatic batch upload refuses the one configuration that would corrupt data.
//
// Two separate defects are pinned here.
//
// (A) THE RELABEL. The field read "LoTW sync" / "Sync LoTW now". "Sync" reads
//     bidirectional, and operators took it for the upload. It is a pure DOWNLOAD
//     (`downloadLotwReport`); the upload lives on the Logbook's "Upload to LoTW (N)"
//     button. A control that names the wrong direction is worse than an unnamed one:
//     the operator believes their contacts went to ARRL and they did not.
//
// (B) THE TRAVELER GATE. "Sign from ADIF location" makes TQSL sign the whole batch from
//     the operator's CURRENT call+grid. That is only ever right when a human picks the
//     moment (upload BEFORE you move). An unattended timer eventually fires after the
//     move and signs older contacts with the new grid — permanently wrong data at ARRL.
//     So the automatic upload is DISABLED in that mode, with the reason on screen rather
//     than hidden, and turning traveler mode on turns the automatic upload off.
//     The backend gate (`Engine::lotw_auto_upload_due`) is the real one; this is the
//     courtesy half, and it is the half the operator actually sees.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, waitFor, fireEvent, act } from '@testing-library/react'
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
}

/** Seed the settings fetch; every case here differs only in the LoTW keys. */
function settings(over: Record<string, unknown> = {}) {
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', ...over } as never),
  )
}

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
  settings()
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('the LoTW confirmations button names its own direction', () => {
  it('offers "Download confirmations", not the bidirectional-sounding "Sync LoTW now"', async () => {
    renderPanel()
    expect(await screen.findByRole('button', { name: /download confirmations/i })).toBeTruthy()
    expect(screen.queryByRole('button', { name: /sync lotw now/i })).toBeNull()
  })

  it('points at where the upload actually lives, in its OWN hint', async () => {
    renderPanel()
    const btn = await screen.findByRole('button', { name: /download confirmations/i })
    // Scoped to this field: the Station Location hint below already names the upload button,
    // so an unscoped query would pass without the download field saying anything at all.
    const field = btn.closest('.settings-field')
    expect(field?.textContent).toMatch(/Upload to LoTW/i)
    expect(field?.textContent).toMatch(/only pulls confirmations/i)
  })
})

describe('the automatic LoTW upload refuses traveler mode', () => {
  it('is a live switch when a named Station Location is in use', async () => {
    settings({ lotwStationLocation: 'HOME', lotwUseAdifLocation: false })
    renderPanel()
    const sw = await screen.findByRole('switch', { name: /upload to lotw automatically/i })
    expect(sw.hasAttribute('disabled')).toBe(false)
  })

  it('sends lotwAutoUpload: true when switched on', async () => {
    settings({ lotwStationLocation: 'HOME', lotwUseAdifLocation: false })
    renderPanel()
    const sw = await screen.findByRole('switch', { name: /upload to lotw automatically/i })
    await act(async () => { fireEvent.click(sw) })
    await waitFor(() => expect(sw.getAttribute('aria-checked')).toBe('true'))
    const save = screen.getByRole('button', { name: /^save$/i })
    await act(async () => { fireEvent.click(save) })
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    const calls = api.get('setSettings').mock.calls
    const sent = calls[calls.length - 1]?.[0] as Record<string, unknown>
    expect(sent.lotwAutoUpload).toBe(true)
  })

  // THE GATE, in the direction that matters. Without this the traveler refusal is untested
  // exactly where it protects the operator's ARRL record.
  it('is DISABLED while "sign from ADIF location" is on, with the reason on screen', async () => {
    settings({ lotwUseAdifLocation: true })
    renderPanel()
    const sw = await screen.findByRole('switch', { name: /upload to lotw automatically/i })
    expect(sw.hasAttribute('disabled')).toBe(true)
    // Visible, not hidden, and in THIS field's own hint — the reason has to be where the
    // operator is looking when they find the switch dead, not somewhere else on the page.
    const field = sw.closest('.settings-field')
    expect(field?.textContent).toMatch(/Unavailable while .Sign from ADIF location/i)
    expect(field?.textContent).toMatch(/wherever you are NOW/i)
  })

  it('turns the automatic upload OFF when traveler mode is turned ON', async () => {
    settings({ lotwStationLocation: 'HOME', lotwAutoUpload: true, lotwUseAdifLocation: false })
    renderPanel()
    const sw = await screen.findByRole('switch', { name: /upload to lotw automatically/i })
    expect(sw.getAttribute('aria-checked')).toBe('true')
    const traveler = screen.getByRole('checkbox', {
      name: /sign lotw uploads from the adif location/i,
    })
    await act(async () => { fireEvent.click(traveler) })
    // The two settings can never be true at once — the operator does not have to notice.
    await waitFor(() => expect(sw.getAttribute('aria-checked')).toBe('false'))
  })
})
