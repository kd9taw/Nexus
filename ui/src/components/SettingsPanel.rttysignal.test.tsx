// @vitest-environment jsdom
//
// Settings ▸ Digital ▸ RTTY ▸ Signal: the 50 baud choice and the saved tone pair (its low tone).
// The modem's half — 50 Bd keyed and copied, the pair the decoder, transmitter and privilege gate
// use — is pinned in Rust (`rttyrx.rs`, `engine/rtty_sideband_tests.rs`); this file pins the
// controls: what they offer, read and save.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { SettingsPanel, RTTY_LOW_TONE_HZ_MIN, RTTY_LOW_TONE_HZ_MAX } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

// The house budget for a SettingsPanel render under a loaded full suite.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module (the SettingsPanel.aprsplacement
// pattern) so a verb missing from a hand-kept list cannot throw on mount.
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
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.2'))
})
afterEach(cleanup)

async function openRtty(stored: Record<string, unknown>): Promise<HTMLElement> {
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', ...stored } as never),
  )
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Digital' }))
  await screen.findByText('Keying backend')
  return document.getElementById('settings-rtty') as HTMLElement
}

/** The `<select>` in the same field box as the `.settings-label` span `label`, or null. */
function selectFor(fs: HTMLElement, label: string): HTMLSelectElement | null {
  const span = within(fs)
    .queryAllByText(label)
    .find((n) => n.tagName === 'SPAN' && n.classList.contains('settings-label'))
  return (span?.closest('label')?.querySelector('select') as HTMLSelectElement | null) ?? null
}

async function clickSave() {
  const save = (await screen.findAllByRole('button', { name: 'Save' })).find(
    (b) => (b as HTMLButtonElement).type === 'submit',
  )!
  fireEvent.click(save)
}

/** The `<input>` in the same field box as the `.settings-label` span `label`, or null. */
function inputFor(fs: HTMLElement, label: string): HTMLInputElement | null {
  const span = within(fs)
    .queryAllByText(label)
    .find((n) => n.tagName === 'SPAN' && n.classList.contains('settings-label'))
  return (span?.closest('label')?.querySelector('input') as HTMLInputElement | null) ?? null
}

describe('Settings ▸ Digital ▸ RTTY ▸ baud rate', () => {
  it('offers 45.45, 50 and 75, and a 50 pick saves under the wire key', async () => {
    const fs = await openRtty({})
    const sel = selectFor(fs, 'Baud rate')!
    expect(sel.value).toBe('45.45')
    expect(Array.from(sel.options).map((o) => o.value)).toEqual(['45.45', '50', '75'])
    fireEvent.change(sel, { target: { value: '50' } })
    await clickSave()
    await waitFor(() =>
      expect(api.get('setSettings')).toHaveBeenCalledWith(expect.objectContaining({ rttyBaud: 50 })),
    )
  })

  it('reads a stored 50 back', async () => {
    const fs = await openRtty({ rttyBaud: 50 })
    expect(selectFor(fs, 'Baud rate')!.value).toBe('50')
  })
})

describe('Settings ▸ Digital ▸ RTTY ▸ the saved tone pair', () => {
  it('reads the 2125 standard when nothing is stored, and a 1275 entry saves under the wire key', async () => {
    // A settings file from before the key: absent, so the box shows the standard.
    const fs = await openRtty({ rttyLowToneHz: undefined })
    const box = inputFor(fs, 'Low tone (Hz)')!
    expect(box.value).toBe('2125')
    fireEvent.change(box, { target: { value: '1275' } })
    await clickSave()
    await waitFor(() =>
      expect(api.get('setSettings')).toHaveBeenCalledWith(expect.objectContaining({ rttyLowToneHz: 1275 })),
    )
  })

  it('reads a stored pair back, and is offered for true FSK too (the decoder still listens on it)', async () => {
    expect(inputFor(await openRtty({ rttyLowToneHz: 1275 }), 'Low tone (Hz)')!.value).toBe('1275')
    cleanup()
    const fs = await openRtty({ rttyBackend: 'fsk', rttyLowToneHz: 1275 })
    expect(inputFor(fs, 'Low tone (Hz)')!.value).toBe('1275')
  })
})

// The box cannot save junk: a cleared or out-of-range entry keeps the last valid tone, and a
// stored value outside the range loads (and saves) as the 2125 standard. 300–2700 keeps both
// tones of the widest shift (850) inside the passband the engine nets in.
/** The settings object the latest save sent. */
function lastSaved(): unknown {
  const calls = api.get('setSettings').mock.calls
  return calls[calls.length - 1][0]
}

describe('Settings ▸ Digital ▸ RTTY ▸ Low tone — never saves junk', () => {
  async function saveAfter(stored: Record<string, unknown>, ...typed: string[]) {
    const fs = await openRtty(stored)
    const box = inputFor(fs, 'Low tone (Hz)')!
    for (const v of typed) fireEvent.change(box, { target: { value: v } })
    // A real click on Save moves focus off the box first; jsdom's click does not.
    fireEvent.blur(box)
    await clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    return (lastSaved() as { rttyLowToneHz?: number }).rttyLowToneHz
  }

  it('offers exactly the valid range', async () => {
    const box = inputFor(await openRtty({}), 'Low tone (Hz)')!
    expect([box.min, box.max]).toEqual(['300', '2700'])
  })

  it('a cleared box saves the last valid tone, never 0', async () => {
    expect(await saveAfter({ rttyLowToneHz: 1275 }, '')).toBe(1275)
    cleanup()
    expect(await saveAfter({}, '1500', '')).toBe(1500)
  })

  it('an out-of-range or junk entry is not taken', async () => {
    expect(await saveAfter({ rttyLowToneHz: 1275 }, '5000')).toBe(1275)
    cleanup()
    expect(await saveAfter({ rttyLowToneHz: 1275 }, '100')).toBe(1275)
    cleanup()
    expect(await saveAfter({ rttyLowToneHz: 1275 }, '-5')).toBe(1275)
    cleanup()
    // CONTROL: the range edges are taken.
    expect(await saveAfter({}, '2700')).toBe(2700)
  })

  it('the box shows the last valid tone once you leave it', async () => {
    const box = inputFor(await openRtty({ rttyLowToneHz: 1275 }), 'Low tone (Hz)')!
    fireEvent.change(box, { target: { value: '' } })
    expect(box.value, 'clearing to type a new number is allowed').toBe('')
    fireEvent.blur(box)
    expect(box.value).toBe('1275')
  })

  it('a stored value outside the range loads and saves as 2125', async () => {
    for (const bad of [0, 9000, 299, 2701]) {
      const fs = await openRtty({ rttyLowToneHz: bad })
      expect(inputFor(fs, 'Low tone (Hz)')!.value, String(bad)).toBe('2125')
      await clickSave()
      await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
      expect((lastSaved() as { rttyLowToneHz?: number }).rttyLowToneHz, String(bad)).toBe(2125)
      cleanup()
      api.get('setSettings').mockClear()
    }
  })
})

it('the box offers the range the engine uses (settings.rs holds the other copy)', async () => {
  const rust = readFileSync(join(__dirname, '../../../crates/tempo-app/src/settings.rs'), 'utf8')
  expect(rust).toContain(`pub const RTTY_LOW_TONE_HZ_MIN: u32 = ${RTTY_LOW_TONE_HZ_MIN};`)
  expect(rust).toContain(`pub const RTTY_LOW_TONE_HZ_MAX: u32 = ${RTTY_LOW_TONE_HZ_MAX};`)
  expect(rust).toContain('fn default_rtty_low_tone_hz() -> u32 {\n    2125\n}')
})
