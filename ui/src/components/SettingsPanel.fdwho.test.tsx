// @vitest-environment jsdom
//
// THE THREE FIELD DAY IDENTITIES, SIDE BY SIDE.
//
// Club Field Day report (2026-08-30): "Confusing for what this is for, we have the primary
// operating callsign of field day, the operating station as in 'littlehouse' or 'commstrailer'
// which represents stations in a multi op situation, or operator name that might be operating
// under the fd call. hard to tell what value this is for."
//
// All three already existed and were modelled correctly — `mycall`, `fd_position_name`,
// `fd_operator`. What did not exist was any one screen where an operator could see that they are
// three DIFFERENT answers: the callsign lived on Station, the position on Contesting under a
// networking heading, the operator on Station and on the Field Day dashboard. Nothing said how
// they relate, so the position name read as a mystery box.
//
// These tests pin the two properties that make the fix a fix rather than more copy:
//   1. all three are on one screen, each with a sentence that distinguishes it from the others;
//   2. they are the SAME settings fields, not a second copy of the state — an edit made here is
//      the edit the Station tab shows and the edit that saves.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
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

// Mock EVERY export of `../api`, derived from the real module (the flexperradio pattern — a
// hand-kept list makes the panel throw on mount when a verb is added).
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

const RADIO = {
  id: 0,
  name: 'FTDX10',
  enabled: true,
  serialPort: '',
  baud: 38400,
  rigModel: 0,
  rigModelName: 'None',
  rigConn: 'serial',
  rigAddr: '',
  rigctldPort: 4532,
  rotctldPort: 4533,
  icomNativeCat: false,
  audioIn: '',
  audioOut: '',
  txLevel: 1,
  rxGain: 1,
  pttMethod: 'cat',
  rotatorModel: 0,
  rotatorPort: '',
  rotatorBaud: 9600,
  rotatorHost: '',
  nativeScope: 'auto',
  bands: [],
  flexRadioIp: '',
  flexNativePan: false,
  flexNativeAudio: false,
}

/** A club position mid-event: the club call on the air, a tent, and a person at the key. */
function settingsFixture(over: Record<string, unknown> = {}) {
  return {
    ...defaultSettings,
    ...RADIO,
    mycall: 'W9ABC',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [RADIO],
    band: '20m',
    dialMhz: 14.074,
    sideband: 'USB',
    fdPositionName: 'CW tent',
    fdOperator: 'KD9TAW',
    ...over,
  } as never
}

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
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsFixture()))
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

async function openTab(name: string) {
  fireEvent.click(await screen.findByRole('tab', { name }))
}

/** The footer Save (`type="submit"`) — other sections carry their own "Save …" buttons. */
async function clickSave() {
  const save = (await screen.findAllByRole('button', { name: 'Save' })).find(
    (b) => (b as HTMLButtonElement).type === 'submit',
  )!
  fireEvent.click(save)
}

/** The fieldset under test, so a query cannot accidentally match a control elsewhere. */
function whoBlock(): HTMLElement {
  const el = document.getElementById('settings-field-day-identity')
  expect(el, 'the Contesting tab must render the three-identity fieldset').toBeTruthy()
  return el as HTMLElement
}

/** The input a `settings-label` names, scoped to one fieldset. */
function fieldInput(scope: HTMLElement, label: string): HTMLInputElement {
  const field = [...scope.querySelectorAll('label.settings-field')].find(
    (l) => l.querySelector('.settings-label')?.textContent === label,
  )
  expect(field, `no field labelled "${label}" in this section`).toBeTruthy()
  return field!.querySelector('input') as HTMLInputElement
}

describe('the three Field Day identities are presented together', () => {
  it('shows the club call, the position and the operator on one screen', async () => {
    renderPanel()
    await openTab('Contesting')

    const who = whoBlock()
    expect(who.querySelector('legend')?.textContent).toBe("Who's who at this event")

    expect(fieldInput(who, 'Callsign on the air').value).toBe('W9ABC')
    expect(fieldInput(who, 'Position name').value).toBe('CW tent')
    expect(fieldInput(who, 'Operator at the key').value).toBe('KD9TAW')
  })

  it('says what each one is for, in terms that tell it apart from the other two', async () => {
    renderPanel()
    await openTab('Contesting')
    const text = whoBlock().textContent ?? ''

    // The callsign: what goes out, one for the whole site.
    expect(text).toMatch(/on the air.*every contact you log/i)
    // The position: which tent — and explicitly NOT something transmitted.
    expect(text).toMatch(/tent, trailer or table/i)
    expect(text).toMatch(/never goes on the air/i)
    // The operator: the one that changes when people swap seats.
    expect(text).toMatch(/takes the seat/i)
  })
})

describe('the three rows are the settings themselves, not a second copy', () => {
  it('saves an edit made here into mycall, fdPositionName and fdOperator', async () => {
    renderPanel()
    await openTab('Contesting')
    const who = whoBlock()

    fireEvent.change(fieldInput(who, 'Callsign on the air'), { target: { value: 'W9XYZ' } })
    fireEvent.change(fieldInput(who, 'Position name'), { target: { value: 'GOTA tent' } })
    fireEvent.change(fieldInput(who, 'Operator at the key'), { target: { value: 'N9DEF' } })
    await clickSave()

    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    const saved = api.get('setSettings').mock.calls[0][0] as Record<string, unknown>
    expect(saved.mycall).toBe('W9XYZ')
    expect(saved.fdPositionName).toBe('GOTA tent')
    expect(saved.fdOperator).toBe('N9DEF')
  })

  it('shows the edit on the Station tab too — one field, two places to reach it', async () => {
    renderPanel()
    await openTab('Contesting')

    fireEvent.change(fieldInput(whoBlock(), 'Callsign on the air'), {
      target: { value: 'W9XYZ' },
    })
    await openTab('Station')

    const station = document.getElementById('settings-operator-radio') as HTMLElement
    expect(fieldInput(station, 'Callsign').value).toBe('W9XYZ')
    expect(fieldInput(station, 'Operator at the key').value).toBe('KD9TAW')
  })

  it('leaves exactly ONE position-name box on the Contesting tab', async () => {
    // The old box lived under the club-sync networking heading with a hint that only said how
    // it looked on the board. Adding a second control for the same field would have made the
    // confusion worse, not better, so the field MOVED.
    renderPanel()
    await openTab('Contesting')
    const named = [...document.querySelectorAll('.settings-label')].filter(
      (l) => l.textContent === 'Position name',
    )
    expect(named).toHaveLength(1)
    expect(named[0].closest('fieldset')?.id).toBe('settings-field-day-identity')
  })
})
