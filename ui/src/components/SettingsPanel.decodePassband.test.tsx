// @vitest-environment jsdom
//
// THE DECODER PASSBAND BOXES CLAMPED ON EVERY KEYSTROKE. Settings ▸ Digital's F low / F high
// (200–2900 / 200–4000 Hz) forced each keystroke into range, so the "1" of "1500" became 200 at
// once and the box could not be typed to any value that does not start with an in-range prefix.
// The boxes now keep the typed text and clamp when the operator leaves them. Typed ONE KEY AT A
// TIME onto what the box holds after React has answered the last key.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { Settings } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module (the SettingsPanel.js8 pattern), so a verb
// missing from a hand-kept list cannot throw on mount.
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

/** The fixture with a callsign (a Save refuses without one). */
const SETTINGS = { ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' }

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
  api.get('getSettings').mockImplementation(() => Promise.resolve(SETTINGS as never))
})
afterEach(cleanup)

async function openDigital() {
  render(
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
  fireEvent.click(await screen.findByRole('tab', { name: 'Digital' }))
  await screen.findByText('Transmit speed') // the tab has rendered once a JS8 control is up
}

function typeOver(input: HTMLInputElement, text: string) {
  fireEvent.focus(input)
  fireEvent.change(input, { target: { value: '' } })
  for (const ch of text) fireEvent.change(input, { target: { value: input.value + ch } })
}

/** The footer Save (`type="submit"`) — other sections carry their own "Save …" buttons. */
async function saved(): Promise<Settings> {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
  await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
  const calls = api.get('setSettings').mock.calls
  return calls[calls.length - 1][0] as Settings
}

const low = () => screen.getByRole('spinbutton', { name: 'Decoder F low (Hz)' }) as HTMLInputElement
const high = () => screen.getByRole('spinbutton', { name: 'Decoder F high (Hz)' }) as HTMLInputElement

describe('Settings ▸ Digital decoder passband', () => {
  it('types 1500 into F low and 3500 into F high, and saves both', async () => {
    await openDigital()
    typeOver(low(), '1500')
    expect(low().value).toBe('1500')
    fireEvent.blur(low())
    typeOver(high(), '3500')
    expect(high().value).toBe('3500')
    fireEvent.blur(high())
    const s = await saved()
    expect(s.decodeFLowHz).toBe(1500)
    expect(s.decodeFHighHz).toBe(3500)
  })

  it('clamps an out-of-range entry on leaving the box, and still keeps F low under F high', async () => {
    await openDigital()
    typeOver(low(), '50')
    expect(low().value).toBe('50')
    fireEvent.blur(low())
    expect(low().value).toBe('200')
    typeOver(high(), '9000')
    fireEvent.blur(high())
    expect(high().value).toBe('4000')
    // The ordering rule that was already there still runs after the clamp.
    typeOver(low(), '2900')
    fireEvent.blur(low())
    typeOver(high(), '250')
    fireEvent.blur(high())
    expect(high().value).toBe('2901')
  })
})
