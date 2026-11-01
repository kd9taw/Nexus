// @vitest-environment jsdom
//
// ⭐ A SAVE THAT CHANGES THE STATION'S CALL WHILE A CONTEST ENTRY RUNS asks once before it saves: the
// entry is operated under one call, and its log and Cabrillo stay under it. "Change the call" saves;
// Cancel saves nothing. Outside an entry it saves at once, and a change of the operator at the key
// (`fdOperator`), which a club rotates, never asks.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import { ConfirmHost } from '../confirm'
import type { FeaturesApi } from '../useFeatures'
import type { FieldDayStatus, Settings } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'

// The house budget for a Settings render under a loaded full suite (SettingsPanel.stalecopy.test.tsx).
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

let station: Record<string, unknown> = {}

/** The snapshot's block for an ARRL Field Day entry running under `entryCall`. */
const entry = (entryCall: string) =>
  ({ running: false, state: 'Idle', qsoCount: 3, sections: 3, points: 6, event: 'arrlfd', entryCall, log: [] }) as unknown as FieldDayStatus

/** Settings open on a station with the call K9ABC, its contest mode as `held` says, and the live
 *  contest block `live` (none outside an entry). */
function openOn(held: Partial<Settings>, live: FieldDayStatus | null) {
  station = structuredClone({
    ...defaultSettings,
    mycall: 'K9ABC',
    mygrid: 'EN52',
    fdClass: '2A',
    fdSection: 'WI',
    fdOperator: 'K9ABC',
    ...held,
  })
  const features = {
    enabled: () => true,
    setEnabled: vi.fn(),
    all: () => [],
    profile: 'full',
    setProfile: vi.fn(),
  } as unknown as FeaturesApi
  render(
    <>
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
        fieldDay={live}
      />
      <ConfirmHost />
    </>,
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
  api.get('getAssistanceJournal').mockImplementation(() => Promise.resolve([]))
  api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(station)))
  api.get('setSettings').mockImplementation((s: Record<string, unknown>) => {
    station = structuredClone(s)
    return Promise.resolve(null)
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const writes = () => api.get('setSettings').mock.calls.map((c) => c[0] as Record<string, unknown>)

/** On the Contesting tab, type `value` into the field labelled `label`. */
async function edit(label: string, value: string) {
  await screen.findAllByDisplayValue('K9ABC')
  fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
  const field = screen
    .getAllByRole('textbox')
    .find((el) => el.closest('label')?.querySelector('.settings-label')?.textContent === label)
  if (!field) throw new Error(`no field labelled ${label}`)
  fireEvent.change(field, { target: { value } })
}

/** The footer Save (`type="submit"`). */
const pressSave = () =>
  fireEvent.click(
    screen.getAllByRole('button', { name: 'Save' }).find((b) => (b as HTMLButtonElement).type === 'submit')!,
  )

const BODY =
  'This entry is operated under K9ABC. Field Day rules allow one call per entry, and a radio used under one call may not then be used under another. Your log and Cabrillo stay under K9ABC.'

describe('a station call changed while a contest entry runs', () => {
  it('asks once, naming the entry call, and saves nothing until it is answered; Cancel saves nothing', async () => {
    openOn({ fdActive: true }, entry('K9ABC'))
    await edit('Callsign on the air', 'K9ABC/P')
    pressSave()
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByText('Change the call anyway?')).toBeTruthy()
    expect(within(dialog).getByText(BODY)).toBeTruthy()
    expect(writes()).toEqual([])
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(writes()).toEqual([])
  })

  it('"Change the call" saves the new call', async () => {
    openOn({ fdActive: true }, entry('K9ABC'))
    await edit('Callsign on the air', 'K9ABC/P')
    pressSave()
    fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Change the call' }))
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(writes()[0]).toMatchObject({ mycall: 'K9ABC/P' })
  })

  it('also for a contest started elsewhere after Settings opened (the form still holds the mode off)', async () => {
    openOn({ fdActive: false }, entry('K9ABC'))
    await edit('Callsign on the air', 'W9XYZ')
    pressSave()
    expect(await screen.findByRole('dialog')).toBeTruthy()
    expect(writes()).toEqual([])
  })
})

describe('CONTROLS: no question', () => {
  it('outside a contest entry a changed call saves at once', async () => {
    openOn({ fdActive: false }, null)
    await edit('Callsign on the air', 'K9ABC/P')
    pressSave()
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(writes()[0]).toMatchObject({ mycall: 'K9ABC/P' })
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('changing the operator at the key during an entry saves at once', async () => {
    openOn({ fdActive: true }, entry('K9ABC'))
    await edit('Operator at the key', 'W9OP')
    pressSave()
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(writes()[0]).toMatchObject({ mycall: 'K9ABC', fdOperator: 'W9OP' })
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('a Save during an entry that leaves the call as it is saves at once', async () => {
    openOn({ fdActive: true }, entry('K9ABC'))
    await edit('Callsign on the air', ' k9abc ')
    pressSave()
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(screen.queryByRole('dialog')).toBeNull()
  })
})
