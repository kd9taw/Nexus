// @vitest-environment jsdom
//
// The FreeDV block in Settings ▸ Radio ▸ Transmit limits & sharing: the address it shows is where
// FreeDV's contacts are taken, and port 0 is the listener switched off, so the block says it is not
// listening rather than naming an address nothing serves.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { t } from '../i18n'
import defaultSettings from './__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 60_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module.
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})

const features: FeaturesApi = {
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
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

/** The FreeDV block as Settings draws it for these stored settings. */
async function freedvBlock(read: Record<string, unknown>): Promise<HTMLElement> {
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'K2DEF', ...read } as never),
  )
  const { findByTestId } = render(
    <SettingsPanel
      target="transmit-limits"
      activeRadioId={0}
      radio={{ swrScaleVerified: true } as never}
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
  return findByTestId('freedv-settings')
}

describe('the FreeDV contacts address in Settings', () => {
  it('names the address on the port it listens on', async () => {
    const block = await freedvBlock({ freedvLog: true, freedvLogPort: 2240 })
    expect(block.textContent).toContain('127.0.0.1:2240')
    expect(block.textContent).not.toContain(t('settings.transmit.freedv.logPort.off'))
  })

  it('on port 0 says it is not listening, and names no address', async () => {
    const block = await freedvBlock({ freedvLog: true, freedvLogPort: 0 })
    expect(block.textContent).toContain(t('settings.transmit.freedv.logPort.off'))
    expect(block.textContent).not.toContain('127.0.0.1:')
  })
})
