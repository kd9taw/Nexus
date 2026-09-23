// @vitest-environment jsdom
//
// ⭐ #165 — WHEN THE SHARING PORT IS TAKEN, THE SHARE BLOCK MUST STOP CLAIMING IT WORKS.
//
// "Rigctld was already running. I start it myself before any other hamradio software. If it
// runs on its default port 4532, Nexus is not working properly." The workaround has always
// existed — the "Share this radio with other programs" switch — and the operator has no way to
// know that is the switch that saves them, because the one place in the app that makes a claim
// about port 4532 prints `127.0.0.1:4532` whether or not anything is listening there.
//
// The refusal already had a good sentence written for it. It went to the connection log, which,
// in the reporter's own words about this bug, "is not where anyone looks". These cases pin it
// where they WERE looking: the block that shows the address.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { RadioStatus } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n/en'

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Derived from the real module rather than a hand-kept list — a verb missing from a hand-kept
// mock makes the panel throw on mount, which presents as a behaviour regression.
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

const FTDX10 = {
  id: 0,
  name: 'FTDX10',
  enabled: true,
  serialPort: 'COM3',
  baud: 38400,
  rigModel: 1042,
  rigModelName: 'Yaesu FTDX10',
  rigConn: 'serial',
  rigAddr: '',
  rigctldPort: 4534,
  rotctldPort: 4533,
  icomNativeCat: false,
  audioIn: 'in-0',
  audioOut: 'out-0',
  txLevel: 1,
  rxGain: 1,
  pttMethod: 'cat',
  rotatorModel: 0,
  rotatorPort: '',
  rotatorBaud: 9600,
  rotatorHost: '',
  nativeScope: 'auto',
  bands: [],
}

/** The reporter's station: one radio, sharing ON (the shipped default) on 4532. */
function sharingSettings() {
  return {
    ...defaultSettings,
    ...FTDX10,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [FTDX10],
    catBroker: true,
    catBrokerPort: 4532,
    band: '20m',
    dialMhz: 14.074,
    sideband: 'USB',
  } as never
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

/** The sentence the Rust side builds on an `AddrInUse`, as it reaches the snapshot. Pasted
 *  rather than imported — it crosses a language boundary, and the Rust test
 *  `a_taken_sharing_port_names_the_switch_that_resolves_it` is what holds its wording. */
const REFUSED =
  'Not sharing this radio: another program is already listening on 127.0.0.1:4532 — usually ' +
  "your own rigctld, a second copy of Nexus, or another radio's daemon. Nexus does not need " +
  'this port to drive the radio, so if that other program is the one you want, switch off ' +
  '"Share this radio with other programs" in Settings ▸ Radio. To go on sharing through Nexus ' +
  'instead, set "Sharing port" to a number nothing else uses. Retrying quietly.'

function renderPanel(radio?: Partial<RadioStatus>) {
  return render(
    <SettingsPanel
      activeRadioId={0}
      radio={radio as RadioStatus | undefined}
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

/** Open the Radio tab and wait for the share block to exist. */
async function openShareBlock(container: HTMLElement) {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  await waitFor(() => {
    expect(container.querySelector('.rig-share-row')).toBeTruthy()
  })
  return container.querySelector('.rig-share-row') as HTMLElement
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.14.0'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(sharingSettings()))
  api.get('testCat').mockImplementation(() => Promise.resolve({ ok: true, detail: 'ok' }))
})
afterEach(cleanup)

describe('the share block when the sharing port is already taken', () => {
  it('says what is wrong instead of printing an address nothing answers on', async () => {
    const { container } = renderPanel({ catShareError: REFUSED } as Partial<RadioStatus>)
    const row = await openShareBlock(container)

    const fault = row.querySelector('.rig-share-fault')
    expect(fault, 'the refusal is not on screen at all').toBeTruthy()
    expect(fault?.textContent).toContain('already listening on 127.0.0.1:4532')
    expect(fault?.getAttribute('role')).toBe('alert')

    // THE HALF THAT WAS A LIE. With the bind refused there is nothing to paste into WSJT-X,
    // so the address — and the Copy button that exists to hand it over — must be gone.
    expect(
      row.querySelector('.rig-share-addr'),
      'still offering an address nothing is listening on',
    ).toBeNull()
  })

  it('names the switch the operator has to find, in the words printed beside it', async () => {
    const { container } = renderPanel({ catShareError: REFUSED } as Partial<RadioStatus>)
    const row = await openShareBlock(container)
    const fault = row.querySelector('.rig-share-fault')?.textContent ?? ''
    // The whole point of the ticket: the toggle that saves this operator exists, and nothing
    // told them it was the one. It is named here by exactly the words on its own label.
    expect(fault).toContain(EN['settings.transmit.share.label'])
    expect(fault).toContain(EN['settings.rigControl.sharingPort.label'])
  })

  it('POSITIVE CONTROL: a station whose bind succeeded still gets its address', async () => {
    // Without this the test above passes on a block that shows the fault unconditionally, or
    // on one that has simply lost the address for everyone.
    const { container } = renderPanel({ catShareError: null } as Partial<RadioStatus>)
    const row = await openShareBlock(container)
    expect(row.querySelector('.rig-share-fault')).toBeNull()
    expect(row.querySelector('.rig-share-addr')?.textContent).toBe('127.0.0.1:4532')
  })

  it('POSITIVE CONTROL: a panel given no radio status at all is unchanged', async () => {
    // Remote sessions and the first paint before the first snapshot land here.
    const { container } = renderPanel(undefined)
    const row = await openShareBlock(container)
    expect(row.querySelector('.rig-share-fault')).toBeNull()
    expect(row.querySelector('.rig-share-addr')?.textContent).toBe('127.0.0.1:4532')
  })
})
