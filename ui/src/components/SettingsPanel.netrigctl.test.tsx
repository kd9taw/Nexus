// @vitest-environment jsdom
//
// PICKING NET RIGCTL WITH A COM PORT SAYS WHAT THAT MEANS, WHERE IT IS PICKED. NET rigctl reaches a
// radio through a rigctld on the network and opens no serial port, so a COM port beside it does
// nothing unless another program's rigctld already answers. The note sits under Connection, in the
// form, before any Save; the selects stay as the operator set them. A real radio on its port and a
// valid address have no note.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
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

async function renderRadio(radio: Record<string, unknown>) {
  const merged = {
    id: 0,
    name: 'Radio',
    enabled: true,
    baud: 9600,
    rigModelName: 'NET rigctl (remote rigctld)',
    rigctldPort: 4532,
    rotctldPort: 4533,
    pttMethod: 'cat',
    bands: [],
    ...radio,
  }
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({
      ...defaultSettings,
      ...merged,
      mycall: 'KD9TAW',
      mygrid: 'EN52',
      activeRadio: 0,
      radios: [merged],
    }),
  )
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
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  await screen.findByText('Connection')
}

const note = () =>
  [...document.querySelectorAll('.settings-hint')].filter((h) =>
    (h.textContent ?? '').startsWith('NET rigctl reaches a radio'),
  )

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
})
afterEach(cleanup)

describe('NET rigctl with a serial port, in the form', () => {
  it('says so under Connection, naming the port, and leaves the Connection as it was', async () => {
    await renderRadio({ rigModel: 2, rigConn: 'serial', serialPort: 'COM4', rigAddr: '' })
    expect(note()).toHaveLength(1)
    expect(note()[0].textContent).toContain('COM4')
    const conn = screen.getByText('Connection').closest('label')!.querySelector('select')!
    expect(conn.value).toBe('serial')
  })

  it('has no note on a valid address, or on a real radio on its port', async () => {
    await renderRadio({ rigModel: 2, rigConn: 'network', serialPort: 'COM4', rigAddr: 'shack-pi:4532' })
    expect(note()).toHaveLength(0)
    cleanup()
    await renderRadio({ rigModel: 1001, rigConn: 'serial', serialPort: 'COM4', rigAddr: '' })
    expect(note()).toHaveLength(0)
  })
})
