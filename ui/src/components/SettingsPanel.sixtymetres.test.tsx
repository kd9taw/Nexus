// @vitest-environment jsdom
//
// #175 — Settings ▸ Digital ▸ Working frequencies shows the stock FT8 table read-only. Since the
// 60 m band button follows the licence country (US 5.3715, everyone else 5.357 in the WRC-15
// segment), a single 5.3715 in that table would contradict the button for most of the world.
// The 60 m row states both dials and which one applies to whom — without copying the callsign
// rule into the UI, where it could drift from the engine's.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FeaturesApi } from '../useFeatures'

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

const features = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

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
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', workingFrequencies: [] } as never),
  )
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('#175 the stock 60 m FT8 row names both dials', () => {
  it('shows the US channel and the worldwide dial, not a lone 5.3715', async () => {
    render(
      <SettingsPanel
        target="working-frequencies"
        activeRadioId={0}
        scale={1 as never}
        scaleMode={'auto' as never}
        scaleCap={1 as never}
        onScaleModeChange={() => {}}
        onScaleCapChange={() => {}}
        density={'standard' as never}
        onDensityChange={() => {}}
        onResetLayout={() => {}}
        features={features}
      />,
    )
    const rows = await screen.findAllByText('60m')
    const stockRow = rows.map((c) => c.closest('.freq-row')).find((r) => r?.textContent?.includes('FT8'))
    expect(stockRow, 'the stock 60 m FT8 row').toBeTruthy()
    expect(stockRow!.textContent).toContain('5.371500')
    expect(stockRow!.textContent).toContain('5.357000')
    // Positive control: another row still prints its single stock dial.
    const twenty = (await screen.findAllByText('20m'))
      .map((c) => c.closest('.freq-row'))
      .find((r) => r?.textContent?.includes('FT8'))
    expect(twenty!.textContent).toContain('14.074000')
    expect(twenty!.textContent).not.toContain('5.357000')
  })
})
