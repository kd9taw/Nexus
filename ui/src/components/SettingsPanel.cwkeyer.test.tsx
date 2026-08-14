// @vitest-environment jsdom
//
// Field report 2026-08 (N0UMF, IC-7410 behind a Timewave Navigator). Two things in this
// panel sent that operator down a road with no keying at the end of it:
//
//  1. The WinKeyer port box rendered unconditionally while the serial-keyline port and line
//     rendered only for their backend. So the ONLY visible port box under "Keyer" belonged
//     to a backend that was not selected — fill it in, Save, and nothing keys, because
//     `cwKeyer` is still the default `cat`. It is now gated like its siblings.
//  2. The keyline hint offered "US Navigator" as an example of a DTR keying interface. The
//     Navigator keys through a K1EL WinKey micro, which ignores DTR entirely — the hint
//     pointed a Navigator owner at the one backend that cannot drive his hardware.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.0.1'))
})
afterEach(cleanup)

/** Open Modes ▸ CW with the given keyer backend selected. */
async function openCw(cwKeyer: string) {
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', cwKeyer } as never),
  )
  renderPanel()
  // CW is its own tab since the eleven per-mode fieldsets were split into Phone · CW · Digital;
  // a CW operator no longer scrolls past a ~660-line FT8 fieldset and six weak-signal tiers to
  // reach their keyer.
  fireEvent.click(await screen.findByRole('tab', { name: 'CW' }))
  // Unconditional on this tab — wait for it before asserting anything is absent.
  await screen.findByText('Keyer backend')
}

describe('the port boxes under Keyer belong to the selected backend', () => {
  it('CAT keying offers no port box at all — the rig is the keyer', async () => {
    await openCw('cat')
    expect(screen.queryByText('WinKeyer port')).toBeNull()
    expect(screen.queryByText('Keyline serial port')).toBeNull()
  })

  it('the soundcard keyer offers none either', async () => {
    await openCw('soundcard')
    expect(screen.queryByText('WinKeyer port')).toBeNull()
    expect(screen.queryByText('Keyline serial port')).toBeNull()
  })

  it('WinKeyer shows its port and only its port', async () => {
    await openCw('winkeyer')
    expect(screen.queryByText('WinKeyer port')).not.toBeNull()
    expect(screen.queryByText('Keyline serial port')).toBeNull()
  })

  it('the serial keyline shows its port and line, and not the WinKeyer box', async () => {
    await openCw('serial')
    expect(screen.queryByText('Keyline serial port')).not.toBeNull()
    expect(screen.queryByText('Keying line')).not.toBeNull()
    expect(screen.queryByText('WinKeyer port')).toBeNull()
  })
})

describe('the keyline hint does not send WinKey owners to the DTR backend', () => {
  const keylineHint = () =>
    screen.getByText(/USB-to-serial into your keying interface/).textContent ?? ''

  it('stops offering the Navigator as an example of a DTR keying interface', async () => {
    await openCw('serial')
    const examples = keylineHint().match(/keying interface \(([^)]*)\)/)?.[1] ?? ''
    expect(examples.length).toBeGreaterThan(0) // the list still exists…
    expect(examples).not.toMatch(/Navigator/i) // …and no longer claims this one
  })

  it('names where a WinKey-based interface actually goes', async () => {
    // Deleting the word would leave a Navigator owner exactly as stuck. The hint has to
    // route him, by name, to the backend that can drive the thing.
    await openCw('serial')
    const hint = keylineHint()
    expect(hint).toMatch(/Navigator/i)
    expect(hint).toMatch(/does\s+not\s+key on DTR/i)
    expect(hint).toMatch(/WinKeyer backend/i)
  })
})
